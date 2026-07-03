use cd_da_reader::{CdReader, Toc, CdReaderError};
use std::io;

use crate::musicbrainz::AlbumMetadata;

#[derive(Debug, Clone)]
pub struct DriveInfo {
    pub path: String,
    pub has_audio_cd: bool,
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
    Loaded { tracks: Vec<TrackInfo>, durations: Vec<String>, metadata: Option<AlbumMetadata>, disc_id: String },
    /// Drive opened but disc absent or unreadable.
    Empty { status: String },
    /// Could not open the drive at all.
    Unavailable { status: String },
}

pub fn scan_drives() -> Vec<DriveInfo> {
    match CdReader::list_drives() {
        Ok(drives) => drives.iter().map(|drive| {

            let display_name = drive_letter(&drive.path);

            let status = if drive.has_audio_cd {
                format!("{} (Audio CD)", display_name)
            } else {
                format!("{} (Empty)", display_name)
            };
            
            DriveInfo {
                path: drive.path.clone(),
                has_audio_cd: drive.has_audio_cd,
                display_name: status,
            }
        }).collect(),
        Err(e) => {
            eprintln!("Failed to scan drives: {}", e);
            Vec::new()
        }
    }
}

pub fn open_drive(path: &str) -> io::Result<CdReader> {
    CdReader::open(path)
}

pub fn read_toc(reader: &CdReader) -> Result<Toc, CdReaderError> {
    reader.read_toc()
}

pub fn get_track_info(toc: &Toc) -> Vec<TrackInfo> {
    let mut tracks = Vec::new();
    
    for track_num in toc.first_track..=toc.last_track {
        if let Some(track) = toc.tracks.iter().find(|t| t.number == track_num) {
            let start_lba = track.start_lba;
            let end_lba = if track_num == toc.last_track {
                toc.leadout_lba
            } else {
                toc.tracks.iter()
                    .find(|t| t.number == track_num + 1)
                    .map(|t| t.start_lba)
                    .unwrap_or(toc.leadout_lba)
            };
            
            let sector_count = end_lba.saturating_sub(start_lba);
            let duration_seconds = sector_count as f64 / 75.0; // 75 sectors/sec
            
            tracks.push(TrackInfo {
                track_number: track_num,
                duration_seconds,
            });
        }
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
