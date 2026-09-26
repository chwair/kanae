use cd_da_reader::{CdReader, Toc, CdReaderError};
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::musicbrainz::AlbumMetadata;

/// sectors between the last audio track and the data session on a CD-Extra
/// disc; matches the gap cd-da-reader leaves out of its own track reads.
const CD_EXTRA_GAP_SECTORS: u32 = 11_400;

#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub path: String,
    pub display_name: String,
}

#[derive(Debug, Clone)]
pub struct TrackInfo {
    pub track_number: u8,
    pub duration_seconds: f64,
}

/// Result produced by the background disc-load thread and consumed by poll_load.
pub enum PendingDiscResult {
    /// TOC read successfully; tracks, durations and optional metadata are ready.
    /// `looked_up` tells whether MusicBrainz was queried for this read.
    Loaded { tracks: Vec<TrackInfo>, durations: Vec<String>, metadata: Option<AlbumMetadata>, disc_id: String, looked_up: bool },
    /// a disc is in the drive but it has no audio tracks (data CD, DVD...).
    NotAudio { status: String },
    /// Drive opened but disc absent or unreadable.
    Empty { status: String },
    /// Could not open the drive at all.
    Unavailable { status: String },
    /// another thread of ours had the drive open; nothing was learned.
    Busy,
}

// cd-da-reader keeps a single process-wide drive handle: a second open fails
// and dropping any reader closes it for everyone. every open goes through this
// lease so only one thread touches the drive at a time.
static DRIVE_LEASED: AtomicBool = AtomicBool::new(false);

struct DriveLease;

impl DriveLease {
    fn acquire(wait: Duration) -> Option<Self> {
        let deadline = Instant::now() + wait;
        loop {
            if DRIVE_LEASED
                .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
                .is_ok()
            {
                return Some(DriveLease);
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

impl Drop for DriveLease {
    fn drop(&mut self) {
        DRIVE_LEASED.store(false, Ordering::Release);
    }
}

/// an open drive. derefs to the crate reader and frees the drive on drop.
pub struct Drive {
    reader: CdReader,
    // declared after `reader` so the handle closes before the lease is released
    _lease: DriveLease,
}

impl std::ops::Deref for Drive {
    type Target = CdReader;
    fn deref(&self) -> &CdReader {
        &self.reader
    }
}

/// list optical drives without touching the discs in them, so it is cheap
/// enough to call from a UI thread.
pub fn scan_drives() -> Vec<DriveInfo> {
    list_drive_paths()
        .into_iter()
        .map(|path| DriveInfo { display_name: drive_letter(&path), path })
        .collect()
}

#[cfg(target_os = "windows")]
fn list_drive_paths() -> Vec<String> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    (b'A'..=b'Z')
        .map(|l| l as char)
        .filter(|letter| {
            let root: Vec<u16> = OsStr::new(&format!("{}:\\", letter))
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            unsafe { winapi::um::fileapi::GetDriveTypeW(root.as_ptr()) == winapi::um::winbase::DRIVE_CDROM }
        })
        .map(|letter| format!("\\\\.\\{}:", letter))
        .collect()
}

#[cfg(target_os = "linux")]
fn list_drive_paths() -> Vec<String> {
    let mut paths: Vec<String> = std::fs::read_dir("/sys/class/block")
        .map(|dir| {
            dir.flatten()
                .filter_map(|e| e.file_name().into_string().ok())
                .filter(|name| name.starts_with("sr"))
                .map(|name| format!("/dev/{}", name))
                .collect()
        })
        .unwrap_or_default();
    paths.sort();
    paths
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn list_drive_paths() -> Vec<String> {
    // macOS discovery reads the I/O Registry passively and never opens a device
    match CdReader::list_drives() {
        Ok(drives) => drives.into_iter().map(|d| d.path).collect(),
        Err(e) => {
            eprintln!("Failed to scan drives: {}", e);
            Vec::new()
        }
    }
}

/// open a drive, waiting briefly if another of our threads is using it.
/// a drive still held after that fails with `ErrorKind::WouldBlock`.
pub fn open_drive(path: &str) -> io::Result<Drive> {
    let lease = DriveLease::acquire(Duration::from_secs(3))
        .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "drive is busy"))?;
    let reader = CdReader::open(path)?;
    Ok(Drive { reader, _lease: lease })
}

pub fn read_toc(reader: &CdReader) -> Result<Toc, CdReaderError> {
    reader.read_toc()
}

/// read what is in the drive. blocking (spin-up, SCSI timeouts, network), so
/// only call it from a background thread. `should_lookup` gets the disc id and
/// decides whether MusicBrainz is queried; the drive is released before that.
pub fn probe_disc(path: &str, should_lookup: impl FnOnce(&str) -> bool) -> PendingDiscResult {
    let toc = {
        let reader = match open_drive(path) {
            Ok(r) => r,
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => return PendingDiscResult::Busy,
            Err(e) => {
                eprintln!("[cd] open {} failed: {}", path, e);
                return PendingDiscResult::Unavailable { status: "Drive unavailable".to_string() };
            }
        };
        let toc = reader.read_toc();
        drop(reader);
        match toc {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[cd] read_toc {} failed: {}", path, e);
                return PendingDiscResult::Empty { status: "No disc inserted".to_string() };
            }
        }
    };

    let tracks = get_track_info(&toc);
    if tracks.is_empty() {
        return PendingDiscResult::NotAudio { status: "Not an audio CD".to_string() };
    }
    let durations = tracks.iter().map(|t| format_duration(t.duration_seconds)).collect();
    let disc_id = crate::musicbrainz::calculate_disc_id(&toc);
    let looked_up = should_lookup(&disc_id);
    let metadata = if looked_up { crate::musicbrainz::lookup_metadata(&toc) } else { None };
    PendingDiscResult::Loaded { tracks, durations, metadata, disc_id, looked_up }
}

/// playable (audio) tracks of a disc; data tracks are left out.
pub fn get_track_info(toc: &Toc) -> Vec<TrackInfo> {
    let mut tracks = Vec::new();

    for (idx, track) in toc.tracks.iter().enumerate() {
        if !track.is_audio || track.number < toc.first_track || track.number > toc.last_track {
            continue;
        }
        let rest = &toc.tracks[idx + 1..];
        let end_lba = match rest.first() {
            // cd-extra: the audio session ends a fixed gap before the data track
            Some(_) if rest.iter().all(|t| !t.is_audio) => {
                rest[0].start_lba.saturating_sub(CD_EXTRA_GAP_SECTORS)
            }
            Some(next) => next.start_lba,
            None => toc.leadout_lba,
        };

        let sector_count = end_lba.saturating_sub(track.start_lba);
        if sector_count == 0 {
            continue;
        }
        tracks.push(TrackInfo {
            track_number: track.number,
            duration_seconds: sector_count as f64 / 75.0, // 75 sectors/sec
        });
    }

    tracks
}

pub fn format_duration(seconds: f64) -> String {
    let mins = (seconds / 60.0).floor() as u32;
    let secs = (seconds % 60.0).floor() as u32;
    format!("{:02}:{:02}", mins, secs)
}

/// Extract a short display label (e.g. "D:") from a drive path such as
/// "\\.\D:" (Windows), "/dev/sr0" (Linux) or "disk6" (macOS).
pub fn drive_letter(drive_path: &str) -> String {
    if drive_path.contains('\\') {
        drive_path.split('\\').last().unwrap_or(drive_path).to_string()
    } else if let Some(dev) = drive_path.strip_prefix("/dev/") {
        dev.to_string()
    } else {
        drive_path.to_string()
    }
}

/// eject on a worker thread; the tray motor can take a couple of seconds.
pub fn eject_drive_async(drive_path: &str) {
    let path = drive_path.to_string();
    std::thread::spawn(move || eject_drive(&path));
}

pub fn eject_drive(drive_path: &str) {
    #[cfg(target_os = "windows")]
    eject_drive_windows(drive_path);

    #[cfg(target_os = "linux")]
    match std::process::Command::new("eject").arg(drive_path).status() {
        Ok(s) if s.success() => eprintln!("[eject] ejected {}", drive_path),
        Ok(s) => eprintln!("[eject] eject exited with {} for {}", s, drive_path),
        Err(e) => eprintln!("[eject] failed to run eject: {}", e),
    }

    #[cfg(target_os = "macos")]
    match std::process::Command::new("diskutil").args(["eject", drive_path]).status() {
        Ok(s) if s.success() => eprintln!("[eject] ejected {}", drive_path),
        Ok(s) => eprintln!("[eject] diskutil eject exited with {} for {}", s, drive_path),
        Err(e) => eprintln!("[eject] failed to run diskutil: {}", e),
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    eprintln!("[eject] eject not implemented on this platform ({})", drive_path);
}

#[cfg(target_os = "windows")]
fn eject_drive_windows(drive_path: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;

    // Build a wide "\\.\D:" style device path from e.g. "D:\" or "\\.\D:".
    // The path may start with '\' so find the first alphabetic character.
    let letter = drive_path.chars()
        .find(|c| c.is_ascii_alphabetic())
        .unwrap_or('D')
        .to_ascii_uppercase();
    let device = format!("\\\\.\\{}:", letter);
    let wide: Vec<u16> = OsStr::new(&device)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let handle = winapi::um::fileapi::CreateFileW(
            wide.as_ptr(),
            winapi::um::winnt::GENERIC_READ | winapi::um::winnt::GENERIC_WRITE,
            winapi::um::winnt::FILE_SHARE_READ | winapi::um::winnt::FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            winapi::um::fileapi::OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if handle == winapi::um::handleapi::INVALID_HANDLE_VALUE {
            eprintln!("[eject] CreateFileW failed for {}", device);
            return;
        }
        let mut bytes_returned: u32 = 0;
        let ok = winapi::um::ioapiset::DeviceIoControl(
            handle,
            winapi::um::winioctl::IOCTL_STORAGE_EJECT_MEDIA,
            std::ptr::null_mut(), 0,
            std::ptr::null_mut(), 0,
            &mut bytes_returned,
            std::ptr::null_mut(),
        );
        if ok == 0 {
            eprintln!("[eject] DeviceIoControl EJECT_MEDIA failed for {}", device);
        } else {
            eprintln!("[eject] ejected {}", device);
        }
        winapi::um::handleapi::CloseHandle(handle);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cd_da_reader::Track;

    fn track(number: u8, start_lba: u32, is_audio: bool) -> Track {
        Track { number, start_lba, start_msf: (0, 0, 0), is_audio }
    }

    #[test]
    fn data_disc_has_no_playable_tracks() {
        let toc = Toc { first_track: 1, last_track: 1, tracks: vec![track(1, 0, false)], leadout_lba: 300_000 };
        assert!(get_track_info(&toc).is_empty());
    }

    #[test]
    fn audio_disc_keeps_every_track() {
        let toc = Toc {
            first_track: 1,
            last_track: 2,
            tracks: vec![track(1, 0, true), track(2, 7_500, true)],
            leadout_lba: 22_500,
        };
        let tracks = get_track_info(&toc);
        assert_eq!(tracks.iter().map(|t| t.track_number).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(tracks[0].duration_seconds, 100.0);
        assert_eq!(tracks[1].duration_seconds, 200.0);
    }

    #[test]
    fn cd_extra_drops_data_track_and_session_gap() {
        let toc = Toc {
            first_track: 1,
            last_track: 3,
            tracks: vec![track(1, 0, true), track(2, 7_500, true), track(3, 30_000 + CD_EXTRA_GAP_SECTORS, false)],
            leadout_lba: 60_000,
        };
        let tracks = get_track_info(&toc);
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[1].duration_seconds, 300.0);
    }
}
