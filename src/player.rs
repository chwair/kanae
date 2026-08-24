use cxx_qt_lib::{QStringList, QString};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::thread;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use cd_da_reader::{CdReader, Toc};

use crate::cd_reader::{self, DriveInfo, TrackInfo, PendingDiscResult};
use crate::audio_player::{self, AudioController};
use crate::musicbrainz::AlbumMetadata;

#[cxx_qt::bridge]
mod player_bridge {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QStringList, drive_list)]
        #[qproperty(i32, selected_drive_index)]
        #[qproperty(i32, current_track)]
        #[qproperty(i32, total_tracks)]
        #[qproperty(f64, current_time)]
        #[qproperty(f64, total_time)]
        #[qproperty(bool, is_playing)]
        #[qproperty(QStringList, track_names)]
        #[qproperty(QStringList, track_titles)]
        #[qproperty(QStringList, track_artists)]
        #[qproperty(QStringList, track_paths)]
        #[qproperty(QString, drive_status)]
        #[qproperty(QString, album_title)]
        #[qproperty(QString, album_artist)]
        #[qproperty(QString, album_year)]
        #[qproperty(QString, cover_art_path)]
        #[qproperty(i32, cd_disc_number)]
        #[qproperty(i32, cd_disc_count)]
        #[qproperty(bool, is_loading)]
        #[qproperty(QStringList, lyric_lines)]
        #[qproperty(QStringList, lyric_times)]
        #[qproperty(bool, lyrics_loading)]
        #[qproperty(bool, is_file_mode)]
        #[qproperty(bool, is_single_file)]
        #[qproperty(QStringList, vis_bands)]
        #[qproperty(QString, queue_json)]
        #[qproperty(i32, vis_band_count)]
        #[qproperty(bool, shuffle)]
        /// 0 = off, 1 = repeat all, 2 = repeat one.
        #[qproperty(i32, repeat_mode)]
        #[qproperty(i32, queue_len)]
        type PlayerController = super::PlayerControllerRust;

        #[qinvokable]
        #[cxx_name = "scanDrives"]
        fn scan_drives(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "selectDrive"]
        fn select_drive(self: Pin<&mut Self>, index: i32);

        #[qinvokable]
        #[cxx_name = "refreshDisc"]
        fn refresh_disc(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "playPause"]
        fn play_pause(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "nextTrack"]
        fn next_track(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "previousTrack"]
        fn previous_track(self: Pin<&mut Self>);

        #[qinvokable]
        fn seek(self: Pin<&mut Self>, seconds: f64);

        #[qinvokable]
        #[cxx_name = "loadTrack"]
        fn load_track(self: Pin<&mut Self>, index: i32);

        #[qinvokable]
        #[cxx_name = "updatePosition"]
        fn update_position(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "updateVisualizer"]
        fn update_visualizer(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "checkDrive"]
        fn check_drive(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "setVolumeLevel"]
        fn set_volume_level(self: Pin<&mut Self>, v: f64);

        #[qinvokable]
        #[cxx_name = "pollLoad"]
        fn poll_load(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "fetchLyrics"]
        fn fetch_lyrics(self: Pin<&mut Self>, track_name: QString, artist_name: QString, album_name: QString, duration_secs: f64);

        #[qinvokable]
        #[cxx_name = "pollLyrics"]
        fn poll_lyrics(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "reapplyLyrics"]
        fn reapply_lyrics(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "setDiscordEnabled"]
        fn set_discord_enabled(self: Pin<&mut Self>, value: bool);

        #[qinvokable]
        #[cxx_name = "initSmtc"]
        fn init_smtc(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "openFilesDialog"]
        fn open_files_dialog(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "openFolderDialog"]
        fn open_folder_dialog(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "ejectOrClose"]
        fn eject_or_close(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "ejectDisc"]
        fn eject_disc(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "loadDisc"]
        fn load_disc(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "openDroppedPaths"]
        fn open_dropped_paths(self: Pin<&mut Self>, urls: QStringList);

        #[qinvokable]
        #[cxx_name = "enqueuePaths"]
        fn enqueue_paths(self: Pin<&mut Self>, paths: QStringList);

        #[qinvokable]
        #[cxx_name = "queueRemove"]
        fn queue_remove(self: Pin<&mut Self>, index: i32);

        #[qinvokable]
        #[cxx_name = "queueMove"]
        fn queue_move(self: Pin<&mut Self>, from: i32, to: i32);

        #[qinvokable]
        #[cxx_name = "queueClear"]
        fn queue_clear(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "queuePlayAt"]
        fn queue_play_at(self: Pin<&mut Self>, index: i32);

        #[qinvokable]
        #[cxx_name = "toggleShuffle"]
        fn toggle_shuffle(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "cycleRepeat"]
        fn cycle_repeat(self: Pin<&mut Self>);

        #[qinvokable]
        #[cxx_name = "setCrossfadeConfig"]
        fn set_crossfade_config(self: Pin<&mut Self>, enabled: bool, secs: f64);
    }
}

/// Album-level fields shown above a local track list.
#[derive(Clone, Default)]
struct AlbumMeta {
    title: String,
    artist: String,
    year: String,
    cover: Option<String>,
}

/// For a single file, show the track's own title/artist. For multiple files,
/// use a common album when they share one and fall back to the folder name.
fn derive_album_meta(
    tracks: &[crate::file_player::LocalTrack],
    input_paths: &[String],
    is_single: bool,
) -> AlbumMeta {
    if is_single {
        let t = &tracks[0];
        return AlbumMeta {
            title: t.title.clone(),
            artist: t.artist.clone(),
            year: t.year.clone(),
            cover: t.cover_art_path.clone(),
        };
    }
    let first_album = tracks[0].album.clone();
    let all_same = tracks.iter().all(|t| t.album == first_album);
    let title = if all_same && !first_album.is_empty() {
        first_album
    } else {
        input_paths.first()
            .and_then(|p| std::path::Path::new(p).file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string()
    };
    let first_aa = tracks[0].album_artist.clone();
    let same_aa  = tracks.iter().all(|t| t.album_artist == first_aa);
    let first_year = tracks[0].year.clone();
    let same_year  = tracks.iter().all(|t| t.year == first_year);
    AlbumMeta {
        title,
        artist: if same_aa && !first_aa.is_empty() { first_aa } else { String::new() },
        year:   if same_year { first_year } else { String::new() },
        cover:  tracks[0].cover_art_path.clone(),
    }
}

/// Index in `0..len` from the system clock. Shuffle order does not need to be
/// cryptographically anything, and this keeps the dependency list unchanged.
fn pseudo_random(len: usize) -> usize {
    if len == 0 { return 0; }
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos() as u64 ^ d.as_secs())
        .unwrap_or(0);
    // xorshift so successive calls a few ms apart don't correlate.
    let mut x = nanos | 1;
    x ^= x << 13; x ^= x >> 7; x ^= x << 17;
    (x % len as u64) as usize
}

/// The album a file belongs to, as the sibling audio files in its directory
/// that share its album tag, plus the file's index within them.
///
/// Falls back to the file on its own when it has no album siblings, so an
/// untagged loose file still plays.
pub fn album_context_for(path: &std::path::Path) -> (Vec<crate::file_player::LocalTrack>, i32) {
    let this = crate::file_player::read_file_metadata(path);
    let Some(dir) = path.parent() else { return (vec![this], 0) };

    let siblings = crate::file_player::collect_files_from_paths(
        &[dir.to_string_lossy().into_owned()],
    );
    // Directories can hold several albums; keep only the matching one. An empty
    // album tag means the track is a standalone single.
    let album: Vec<_> = if this.album.is_empty() {
        Vec::new()
    } else {
        siblings.into_iter().filter(|t| t.album == this.album).collect()
    };
    match album.iter().position(|t| t.path == this.path) {
        Some(i) if album.len() > 1 => (album, i as i32),
        _ => (vec![this], 0),
    }
}

/// Where playback goes back to once the queue drains.
struct QueueResume {
    tracks: Vec<crate::file_player::LocalTrack>,
    is_single: bool,
    meta: AlbumMeta,
    next_index: i32,
}

pub struct PlayerState {
    drives: Vec<DriveInfo>,
    current_drive_path: Option<String>,
    cd_reader: Option<CdReader>,
    toc: Option<Toc>,
    tracks: Vec<TrackInfo>,
    playback_thread: Option<thread::JoinHandle<()>>,
    stop_playback: Arc<AtomicBool>,
    current_position: Arc<AtomicU64>,
    playback_start_offset: f64,
    playback_ended: Arc<AtomicBool>,
    playback_disc_error: Arc<AtomicBool>, // set on errors, distinguishes ejection from track end
    volume: Arc<AtomicU64>,
    heard_position: Arc<AtomicU64>, // oldest chunk in rodio's queue — what the listener hears
    disc_load_result: Arc<Mutex<Option<PendingDiscResult>>>,
    disc_load_thread: Option<thread::JoinHandle<()>>,
    disc_check_active: Arc<AtomicBool>,
    metadata_loaded: bool,
    current_disc_id: String,
    lyric_result: Arc<Mutex<Option<Option<Vec<crate::lrclib::LyricLine>>>>>,
    lyric_fetch_thread: Option<thread::JoinHandle<()>>,
    lyric_fetch_generation: Arc<AtomicU64>,
    /// Original (non-romanized) lyric lines for the current track, kept so the
    /// romanize toggle can be re-applied live without re-fetching.
    lyrics_src: Vec<crate::lrclib::LyricLine>,
    smtc_handle: Option<crate::smtc::SmtcHandle>,
    track_titles_plain:  Vec<String>,
    track_artists_plain: Vec<String>,
    smtc_album:        String,
    smtc_album_artist: String,
    smtc_cover_url:    String,
    is_file_mode: bool,
    file_tracks: Vec<crate::file_player::LocalTrack>,
    discord: Option<crate::discord::DiscordPresence>,
    discord_enabled: bool,
    /// Whole second of the last SMTC/Discord progress push; used to throttle
    /// those updates to 1 Hz (the position timer itself ticks at 10 Hz).
    last_progress_push_sec: i64,
    /// Shared spectrum tap handed to each playback thread's AudioController;
    /// the GUI drains it once per frame via `update_visualizer`.
    vis_tap: Arc<Mutex<audio_player::VisTap>>,
    /// Current displayed bar heights (0..1). Bars snap up to new energy and
    /// fall under gravity, the audioviz/crav-style motion.
    vis_smooth: Vec<f32>,
    /// Per-bar downward fall velocity for the gravity effect.
    vis_vel: Vec<f32>,
    /// Timestamp of the previous visualizer update, so gravity is applied with
    /// real delta-time and stays smooth regardless of frame rate.
    vis_last_update: Option<std::time::Instant>,
    /// Auto-gain reference level: tracks the current loudness so the bars use
    /// the full height for both quiet and loud material.
    vis_ref: f32,
    /// Songs to play ahead of the rest of the album.
    queue: crate::queue::Queue,
    /// Captured the first time the queue interrupts an album, so playback can
    /// return to it once the queue drains.
    queue_resume: Option<QueueResume>,

    /// Crossfade config, mirrored from settings.json.
    crossfade: bool,
    crossfade_secs: f64,
    /// Handed to the running track so the player can tell it to ramp out.
    fade_out_secs: Arc<AtomicU64>,
    /// The outgoing track during a crossfade. It owns orphaned copies of the
    /// progress atomics, so it can finish without disturbing the new track.
    fading_thread: Option<thread::JoinHandle<()>>,
    fading_stop: Option<Arc<AtomicBool>>,
    /// Set once the handover for the current track has been started, so the
    /// position tick doesn't fire it again every 100 ms.
    crossfade_armed: bool,
    /// Indices already played in this shuffle pass, so it doesn't repeat until
    /// the album is exhausted.
    shuffle_history: Vec<i32>,
    /// Set by the handover so the track started next ramps in.
    pending_fade_in: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        let settings = crate::library_cache::load_settings();
        Self {
            drives: Vec::new(),
            current_drive_path: None,
            cd_reader: None,
            toc: None,
            tracks: Vec::new(),
            playback_thread: None,
            stop_playback: Arc::new(AtomicBool::new(false)),
            current_position: Arc::new(AtomicU64::new(0)),
            playback_start_offset: 0.0,
            playback_ended: Arc::new(AtomicBool::new(false)),
            playback_disc_error: Arc::new(AtomicBool::new(false)),
            volume: Arc::new(AtomicU64::new((1.0_f64).to_bits())),
            heard_position: Arc::new(AtomicU64::new(0)),
            disc_load_result: Arc::new(Mutex::new(None)),
            disc_load_thread: None,
            disc_check_active: Arc::new(AtomicBool::new(false)),
            metadata_loaded: false,
            current_disc_id: String::new(),
            lyric_result: Arc::new(Mutex::new(None)),
            lyric_fetch_thread: None,
            lyric_fetch_generation: Arc::new(AtomicU64::new(0)),
            lyrics_src: Vec::new(),
            smtc_handle: None,
            track_titles_plain:  Vec::new(),
            track_artists_plain: Vec::new(),
            smtc_album:        String::new(),
            smtc_album_artist: String::new(),
            smtc_cover_url:    String::new(),
            is_file_mode: false,
            file_tracks: Vec::new(),
            discord: crate::discord::DiscordPresence::new(),
            discord_enabled: settings.discord_rpc,
            last_progress_push_sec: -1,
            vis_tap: Arc::new(Mutex::new(audio_player::VisTap::new())),
            vis_smooth: vec![0.0; audio_player::VIS_BANDS],
            vis_vel: vec![0.0; audio_player::VIS_BANDS],
            vis_last_update: None,
            vis_ref: 1.0e-3,
            queue: crate::queue::Queue::new(),
            queue_resume: None,
            crossfade: settings.crossfade,
            crossfade_secs: settings.crossfade_secs,
            fade_out_secs: Arc::new(AtomicU64::new(0)),
            fading_thread: None,
            fading_stop: None,
            crossfade_armed: false,
            shuffle_history: Vec::new(),
            pending_fade_in: false,
        }
    }
}

impl PlayerState {
    pub fn sync_discord(&mut self, current_track: i32, is_playing: bool) {
        // When RPC is disabled, clear any existing presence and push nothing.
        if !self.discord_enabled {
            if let Some(ref mut d) = self.discord { d.update(None); }
            return;
        }
        let info = if current_track < 0 {
            None
        } else {
            let idx      = current_track as usize;
            let title    = self.track_titles_plain.get(idx).cloned().unwrap_or_default();
            let raw_ar   = self.track_artists_plain.get(idx).cloned().unwrap_or_default();
            let artist   = if raw_ar.is_empty() { self.smtc_album_artist.clone() } else { raw_ar };
            let album    = self.smtc_album.clone();
            let cover    = self.smtc_cover_url.clone();
            let pos      = f64::from_bits(self.heard_position.load(Ordering::Relaxed));
            let duration = if self.is_file_mode {
                self.file_tracks.get(idx).map(|t| t.duration_secs).unwrap_or(0.0)
            } else {
                self.tracks.get(idx).map(|t| t.duration_seconds).unwrap_or(0.0)
            };
            Some(crate::discord::TrackInfo {
                title, artist, album, cover_url: cover,
                position_secs: pos, duration_secs: duration, is_playing,
            })
        };
        if let Some(ref mut d) = self.discord {
            d.update(info);
        }
    }
}

pub struct PlayerControllerRust {
    drive_list: QStringList,
    selected_drive_index: i32,
    current_track: i32,
    total_tracks: i32,
    current_time: f64,
    total_time: f64,
    is_playing: bool,
    track_names: QStringList,
    track_titles: QStringList,
    track_artists: QStringList,
    track_paths: QStringList,
    drive_status: QString,
    album_title: QString,
    album_artist: QString,
    album_year: QString,
    cover_art_path: QString,
    cd_disc_number: i32,
    cd_disc_count: i32,
    is_loading: bool,
    lyric_lines: QStringList,
    lyric_times: QStringList,
    lyrics_loading: bool,
    is_file_mode: bool,
    is_single_file: bool,
    vis_bands: QStringList,
    queue_json: QString,
    vis_band_count: i32,
    shuffle: bool,
    repeat_mode: i32,
    queue_len: i32,

    state: Arc<Mutex<PlayerState>>,
}

impl Default for PlayerControllerRust {
    fn default() -> Self {
        Self {
            drive_list: QStringList::default(),
            selected_drive_index: -1,
            current_track: -1,
            total_tracks: 0,
            current_time: 0.0,
            total_time: 0.0,
            is_playing: false,
            track_names: QStringList::default(),
            track_titles: QStringList::default(),
            track_artists: QStringList::default(),
            track_paths: QStringList::default(),
            drive_status: QString::from("No disc inserted"),
            album_title: QString::from("Unknown Album"),
            album_artist: QString::from("Unknown Artist"),
            album_year: QString::from(""),
            cover_art_path: QString::from(""),
            cd_disc_number: 0,
            cd_disc_count: 0,
            is_loading: false,
            lyric_lines: QStringList::default(),
            lyric_times: QStringList::default(),
            lyrics_loading: false,
            is_file_mode: false,
            is_single_file: false,
            vis_bands: QStringList::default(),
            queue_json: QString::from("[]"),
            vis_band_count: audio_player::VIS_BANDS as i32,
            shuffle: false,
            repeat_mode: 0,
            queue_len: 0,
            state: Arc::new(Mutex::new(PlayerState::default())),
        }
    }
}

impl player_bridge::PlayerController {
    pub fn scan_drives(mut self: Pin<&mut Self>) {
        let drives = cd_reader::scan_drives();
        
        if drives.is_empty() {
            self.as_mut().set_drive_list(QStringList::default());
            self.as_mut().set_selected_drive_index(-1);
            // Don't wipe file-mode track state — the timer calls scan_drives
            // periodically and would otherwise clear the loaded track list.
            if !self.state.lock().unwrap().is_file_mode {
                self.as_mut().set_track_names(QStringList::default());
                self.as_mut().set_total_tracks(0);
                self.as_mut().set_drive_status(QString::from("No optical drive detected"));
            }
            return;
        }
        
        let mut list = QStringList::default();
        let mut auto_select_index = -1;
        
        for (i, drive) in drives.iter().enumerate() {
            list.append(QString::from(&drive.display_name));
            if drive.has_audio_cd && auto_select_index == -1 {
                auto_select_index = i as i32;
            }
        }
        
        if let Ok(mut state) = self.state.lock() {
            state.drives = drives;
        }
        
        self.as_mut().set_drive_list(list);

        // Don't auto-select or interact with drives while local files are playing.
        if self.state.lock().unwrap().is_file_mode {
            return;
        }

        if auto_select_index >= 0 {
            self.as_mut().set_selected_drive_index(auto_select_index);
            self.select_drive(auto_select_index);
        } else if !self.state.lock().unwrap().drives.is_empty() {
            self.as_mut().set_selected_drive_index(0);
            self.select_drive(0);
        }
    }

    pub fn select_drive(mut self: Pin<&mut Self>, index: i32) {
        // Don't let drive selection interrupt local file playback.
        if self.state.lock().unwrap().is_file_mode {
            return;
        }
        let drive_path = {
            let state = self.state.lock().unwrap();
            if index < 0 || index as usize >= state.drives.len() {
                return;
            }
            state.drives[index as usize].path.clone()
        };

        self.as_mut().stop_playback_internal();
        {
            let mut state = self.state.lock().unwrap();
            state.cd_reader = None;
            state.toc = None;
            state.tracks.clear();
            state.current_drive_path = Some(drive_path);
        }
        self.as_mut().set_selected_drive_index(index);
        self.refresh_disc();
    }

    pub fn refresh_disc(mut self: Pin<&mut Self>) {
        if *self.as_ref().is_loading() {
            return;
        }
        let (drive_path, result_slot) = {
            let state = self.state.lock().unwrap();
            let path = match state.current_drive_path.clone() {
                Some(p) => p,
                None => {
                    drop(state);
                    self.as_mut().set_total_tracks(0);
                    self.as_mut().set_drive_status(QString::from("No drive selected"));
                    return;
                }
            };
            let slot = state.disc_load_result.clone();
            *slot.lock().unwrap() = None;
            drop(state);
            // Drop existing reader so the thread can open an exclusive handle.
            self.state.lock().unwrap().cd_reader = None;
            (path, slot)
        };
        // Join any existing background thread BEFORE spawning a new one to
        // guarantee only one thread holds the drive open at a time.
        {
            let mut state = self.state.lock().unwrap();
            state.disc_check_active.store(false, Ordering::Relaxed);
            if let Some(old) = state.disc_load_thread.take() {
                drop(state);
                let _ = old.join();
            }
        }
        let handle = thread::spawn(move || {
            let result = match cd_reader::open_drive(&drive_path) {
                Err(_) => PendingDiscResult::Unavailable {
                    status: "Drive unavailable".to_string(),
                },
                Ok(reader) => match cd_reader::read_toc(&reader) {
                    Ok(toc) => {
                        let tracks = cd_reader::get_track_info(&toc);
                        let durations = tracks
                            .iter()
                            .map(|t| cd_reader::format_duration(t.duration_seconds))
                            .collect();
                        let metadata = crate::musicbrainz::lookup_metadata(&toc);
                        let disc_id = crate::musicbrainz::calculate_disc_id(&toc);
                        PendingDiscResult::Loaded { tracks, durations, metadata, disc_id }
                    }
                    Err(_) => PendingDiscResult::Empty {
                        status: "No disc inserted".to_string(),
                    },
                },
            };
            *result_slot.lock().unwrap() = Some(result);
        });
        self.state.lock().unwrap().disc_load_thread = Some(handle);
        // Tell SMTC we're loading while the spinner shows.
        {
            let mut state = self.state.lock().unwrap();
            if let Some(ref mut h) = state.smtc_handle {
                h.update(crate::smtc::SmtcUpdate::Metadata {
                    title:     "Loading disc...".to_string(),
                    artist:    "Kanae".to_string(),
                    album:     String::new(),
                    cover_url: None,
                    duration:  None,
                });
                h.update(crate::smtc::SmtcUpdate::Stopped);
            }
        }
        self.as_mut().set_is_loading(true);
    }

    pub fn play_pause(mut self: Pin<&mut Self>) {
        let is_playing = *self.as_ref().is_playing();

        if is_playing {
            let heard_pos = f64::from_bits(
                self.state.lock().unwrap().heard_position.load(Ordering::Relaxed)
            );
            self.state.lock().unwrap().playback_start_offset = heard_pos;
            self.as_mut().set_current_time(heard_pos);
            self.as_mut().stop_playback_internal();
            {
                let mut state = self.state.lock().unwrap();
                if let Some(ref mut h) = state.smtc_handle {
                    h.update(crate::smtc::SmtcUpdate::Paused {
                        progress: std::time::Duration::from_secs_f64(heard_pos.max(0.0)),
                    });
                }
            }
            {
                let current_track = *self.as_ref().current_track();
                self.state.lock().unwrap().sync_discord(current_track, false);
            }
        } else {
            self.as_mut().start_playback();
            {
                let mut state = self.state.lock().unwrap();
                // Force an SMTC "Playing" push on the next position tick even if
                // the whole second hasn't changed since the pause.
                state.last_progress_push_sec = -1;
            }
            {
                let current_track = *self.as_ref().current_track();
                self.state.lock().unwrap().sync_discord(current_track, true);
            }
        }
    }

    // ── Queue ───────────────────────────────────────────────────────────────

    /// Republish the queue to the frontends after any mutation.
    fn refresh_queue(mut self: Pin<&mut Self>) {
        let (json, len) = {
            let state = self.state.lock().unwrap();
            (state.queue.to_json(), state.queue.len() as i32)
        };
        self.as_mut().set_queue_json(QString::from(json.as_str()));
        self.as_mut().set_queue_len(len);
    }

    /// Append every audio file under `paths` — a song, an album folder, or a
    /// mix of both.
    pub fn enqueue_paths(self: Pin<&mut Self>, paths: QStringList) {
        let list: Vec<String> = (0..paths.len()).filter_map(|i| paths.get(i).map(|s| s.to_string())).collect();
        if list.is_empty() { return; }
        self.state.lock().unwrap().queue.extend_from_paths(&list);
        self.refresh_queue();
    }

    pub fn queue_remove(self: Pin<&mut Self>, index: i32) {
        if index < 0 { return; }
        self.state.lock().unwrap().queue.remove(index as usize);
        self.refresh_queue();
    }

    pub fn queue_move(self: Pin<&mut Self>, from: i32, to: i32) {
        if from < 0 || to < 0 { return; }
        self.state.lock().unwrap().queue.move_entry(from as usize, to as usize);
        self.refresh_queue();
    }

    pub fn queue_clear(self: Pin<&mut Self>) {
        self.state.lock().unwrap().queue.clear();
        self.refresh_queue();
    }

    /// Skip straight to a queue entry, dropping the ones before it.
    pub fn queue_play_at(mut self: Pin<&mut Self>, index: i32) {
        if index < 0 { return; }
        let entry = {
            let mut state = self.state.lock().unwrap();
            for _ in 0..index { state.queue.pop_front(); }
            state.queue.pop_front()
        };
        let Some(entry) = entry else { return };
        self.as_mut().capture_queue_resume();
        self.as_mut().play_queue_entry(entry);
        self.as_mut().refresh_queue();
        // Clicking an entry means play it, unlike the load-only advance path.
        self.start_playback();
    }

    /// Remember the album to come back to, unless the queue already took over
    /// (in which case the original resume point is the one we want).
    fn capture_queue_resume(self: Pin<&mut Self>) {
        let current = *self.as_ref().current_track();
        let mut state = self.state.lock().unwrap();
        if state.queue_resume.is_some() || !state.is_file_mode || state.file_tracks.is_empty() {
            return;
        }
        let tracks = state.file_tracks.clone();
        let is_single = tracks.len() == 1;
        state.queue_resume = Some(QueueResume {
            meta: AlbumMeta {
                title:  state.smtc_album.clone(),
                artist: state.smtc_album_artist.clone(),
                year:   String::new(),
                cover:  if state.smtc_cover_url.is_empty() { None } else { Some(state.smtc_cover_url.clone()) },
            },
            tracks,
            is_single,
            next_index: current + 1,
        });
    }

    /// Swap the session over to a queued song, loaded but not started — callers
    /// decide whether playback continues, matching `load_track`.
    ///
    /// The song is loaded *in its album*, not on its own: a one-track session
    /// would show up everywhere as a pseudo-album, so clicking the now-playing
    /// title would open a fake album containing only that song instead of
    /// landing on the track within its real one.
    fn play_queue_entry(mut self: Pin<&mut Self>, entry: crate::queue::QueueEntry) {
        let (tracks, index) = album_context_for(&entry.path);
        let is_single = tracks.len() == 1;
        let dir = entry.path.parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let meta = derive_album_meta(&tracks, &[dir], is_single);
        self.as_mut().apply_local_tracks(tracks, is_single, meta);
        self.as_mut().load_track_at(index);
    }

    /// Load whatever should play next: queued songs first, then the rest of the
    /// album, then the album the queue interrupted. Like `load_track`, this
    /// leaves playback stopped; the caller starts it. Returns false when there
    /// is nothing left to play.
    fn advance(mut self: Pin<&mut Self>) -> bool {
        let next_queued = self.state.lock().unwrap().queue.pop_front();
        if let Some(entry) = next_queued {
            self.as_mut().capture_queue_resume();
            self.as_mut().play_queue_entry(entry);
            self.refresh_queue();
            return true;
        }

        // Queue just drained — go back to where it interrupted.
        let resume = self.state.lock().unwrap().queue_resume.take();
        if let Some(r) = resume {
            let count = r.tracks.len() as i32;
            let next  = r.next_index;
            self.as_mut().apply_local_tracks(r.tracks, r.is_single, r.meta);
            if next < count {
                self.as_mut().load_track_at(next);
                return true;
            }
            return false;
        }

        match self.as_mut().pick_next_index() {
            Some(i) => { self.as_mut().load_track_at(i); true }
            None => false,
        }
    }

    /// The next index within the current track list, honouring repeat and
    /// shuffle. `None` means playback has run out.
    fn pick_next_index(self: Pin<&mut Self>) -> Option<i32> {
        let current = *self.as_ref().current_track();
        let total   = *self.as_ref().total_tracks();
        let repeat  = *self.as_ref().repeat_mode();
        let shuffle = *self.as_ref().shuffle();
        if total <= 0 { return None; }

        // Repeat-one wins over everything: the same track again.
        if repeat == 2 { return Some(current.max(0)); }

        if shuffle && total > 1 {
            let mut st = self.state.lock().unwrap();
            st.shuffle_history.retain(|i| *i >= 0 && *i < total);
            if !st.shuffle_history.contains(&current) && current >= 0 {
                st.shuffle_history.push(current);
            }
            // A full pass is done; either stop or start a fresh one.
            if st.shuffle_history.len() as i32 >= total {
                st.shuffle_history.clear();
                if repeat != 1 { return None; }
            }
            let pool: Vec<i32> = (0..total).filter(|i| !st.shuffle_history.contains(i)).collect();
            let pick = pool.get(pseudo_random(pool.len())).copied();
            return pick.or(Some(current.max(0)));
        }

        if current + 1 < total { return Some(current + 1); }
        if repeat == 1 { return Some(0); }
        None
    }

    pub fn next_track(mut self: Pin<&mut Self>) {
        // An explicit skip should move on even in repeat-one.
        let repeat = *self.as_ref().repeat_mode();
        if repeat == 2 {
            self.as_mut().set_repeat_mode(1);
            self.as_mut().advance();
            self.as_mut().set_repeat_mode(2);
            return;
        }
        self.advance();
    }

    /// Hand the current track over to the next one with an overlap.
    ///
    /// The outgoing thread keeps playing, so it must stop sharing state with the
    /// player: it is moved aside with its own stop flag and given orphaned copies
    /// of the progress atomics, otherwise it would keep publishing its position
    /// and would raise `playback_ended` for a track that is no longer current.
    fn begin_crossfade(mut self: Pin<&mut Self>) {
        // Check there is something to fade into before telling the current track
        // to ramp out — the ramp ends its stream early, so asking for one with
        // nothing to follow would just truncate the last track.
        let has_next = {
            let st = self.state.lock().unwrap();
            let queued = !st.queue.is_empty() || st.queue_resume.is_some();
            drop(st);
            queued
                || *self.as_ref().shuffle()
                || *self.as_ref().repeat_mode() == 1
                || *self.as_ref().current_track() + 1 < *self.as_ref().total_tracks()
        };
        if !has_next {
            self.state.lock().unwrap().crossfade_armed = true;
            return;
        }

        // Reap a previous crossfade before starting another.
        let prev = {
            let mut state = self.state.lock().unwrap();
            if let Some(flag) = state.fading_stop.take() { flag.store(true, Ordering::Relaxed); }
            state.fading_thread.take()
        };
        if let Some(h) = prev { let _ = h.join(); }

        // Hold the outgoing thread in a local across `advance`, which calls
        // stop_playback_internal and would otherwise reap it right back.
        let (outgoing, outgoing_stop) = {
            let mut state = self.state.lock().unwrap();
            let secs = state.crossfade_secs;
            state.fade_out_secs.store(secs.to_bits(), Ordering::Relaxed);

            let t = state.playback_thread.take();
            let flag = state.stop_playback.clone();

            // Fresh atomics for the incoming track; the outgoing thread keeps
            // writing to the old ones, which nothing reads any more.
            state.stop_playback    = Arc::new(AtomicBool::new(false));
            state.playback_ended   = Arc::new(AtomicBool::new(false));
            state.heard_position   = Arc::new(AtomicU64::new(0));
            state.current_position = Arc::new(AtomicU64::new(0));
            state.vis_tap = Arc::new(Mutex::new(audio_player::VisTap::new()));
            state.pending_fade_in = true;
            state.crossfade_armed = true;
            (t, flag)
        };

        if self.as_mut().advance() {
            self.as_mut().start_playback();
            let mut state = self.state.lock().unwrap();
            state.pending_fade_in = false;
            state.fading_thread = outgoing;
            state.fading_stop = Some(outgoing_stop);
        } else {
            // Nothing follows, so let the outgoing track play out in full.
            let mut state = self.state.lock().unwrap();
            state.fade_out_secs.store(0f64.to_bits(), Ordering::Relaxed);
            state.pending_fade_in = false;
            state.playback_thread = outgoing;
            state.stop_playback = outgoing_stop;
        }
    }

    pub fn toggle_shuffle(mut self: Pin<&mut Self>) {
        let on = !*self.as_ref().shuffle();
        self.as_mut().set_shuffle(on);
        self.state.lock().unwrap().shuffle_history.clear();
    }

    pub fn cycle_repeat(mut self: Pin<&mut Self>) {
        let next = (*self.as_ref().repeat_mode() + 1) % 3;
        self.as_mut().set_repeat_mode(next);
    }

    pub fn set_crossfade_config(self: Pin<&mut Self>, enabled: bool, secs: f64) {
        let mut st = self.state.lock().unwrap();
        st.crossfade = enabled;
        st.crossfade_secs = secs.clamp(
            crate::library::CROSSFADE_MIN_SECS, crate::library::CROSSFADE_MAX_SECS);
    }

    pub fn previous_track(self: Pin<&mut Self>) {
        let current = *self.as_ref().current_track();
        
        if current > 0 {
            self.load_track(current - 1);
        }
    }

    pub fn seek(mut self: Pin<&mut Self>, seconds: f64) {
        let was_playing = *self.as_ref().is_playing();
        {
            let mut state = self.state.lock().unwrap();
            state.playback_start_offset = seconds;
            state.current_position.store(seconds.to_bits(), Ordering::Relaxed);
            state.heard_position.store(seconds.to_bits(), Ordering::Relaxed);
        }
        self.as_mut().set_current_time(seconds);
        if was_playing {
            // start_playback stops the current thread then restarts at the new offset.
            self.start_playback();
        }
    }

    /// Track selection from the UI. Choosing a track by hand supersedes the
    /// album the queue was going to return to.
    pub fn load_track(self: Pin<&mut Self>, index: i32) {
        self.state.lock().unwrap().queue_resume = None;
        self.load_track_at(index);
    }

    fn load_track_at(mut self: Pin<&mut Self>, index: i32) {
        let (duration, _is_file) = {
            let state = self.state.lock().unwrap();
            if index < 0 { return; }
            if state.is_file_mode {
                if index as usize >= state.file_tracks.len() { return; }
                (state.file_tracks[index as usize].duration_secs, true)
            } else {
                if index as usize >= state.tracks.len() { return; }
                (state.tracks[index as usize].duration_seconds, false)
            }
        };

        self.as_mut().stop_playback_internal();
        {
            let mut state = self.state.lock().unwrap();
            state.playback_start_offset = 0.0;
            state.current_position.store(0u64, Ordering::Relaxed);
        }
        self.as_mut().set_current_track(index);
        self.as_mut().set_total_time(duration);
        self.as_mut().set_current_time(0.0);
        {
            let mut state = self.state.lock().unwrap();
            let idx     = index as usize;
            let title   = state.track_titles_plain.get(idx).cloned().unwrap_or_default();
            let raw_art = state.track_artists_plain.get(idx).cloned().unwrap_or_default();
            let artist  = if raw_art.is_empty() { state.smtc_album_artist.clone() } else { raw_art };
            let album   = state.smtc_album.clone();
            let cover   = state.smtc_cover_url.clone();
            eprintln!("[smtc] load_track cover raw={:?}", cover);
            let dur     = std::time::Duration::from_secs_f64(duration.max(0.0));
            if let Some(ref mut h) = state.smtc_handle {
                h.update(crate::smtc::SmtcUpdate::Metadata {
                    title,
                    artist,
                    album,
                    cover_url: if cover.is_empty() { None } else { Some(cover) },
                    duration:  Some(dur),
                });
                h.update(crate::smtc::SmtcUpdate::Stopped);
            }
        }
        self.state.lock().unwrap().sync_discord(index, false);
    }

    fn start_playback(mut self: Pin<&mut Self>) {
        self.as_mut().stop_playback_internal();

        // Also join any running background disc-check thread so it releases
        // its exclusive drive handle before the playback thread opens its own.
        {
            let mut state = self.state.lock().unwrap();
            state.disc_check_active.store(false, Ordering::Relaxed);
            if let Some(old) = state.disc_load_thread.take() {
                drop(state);
                let _ = old.join();
            }
        }

        let current_track = *self.as_ref().current_track();

        // File mode: decode local audio file instead of reading CD.
        if self.state.lock().unwrap().is_file_mode {
            let (file_path, stop_flag, start_offset, current_position, volume_arc,
                 playback_ended_arc, heard_position_arc, vis_tap) = {
                let state = self.state.lock().unwrap();
                if current_track < 0 || (current_track as usize) >= state.file_tracks.len() {
                    eprintln!("[file] invalid track index {}", current_track); return;
                }
                let file_path = state.file_tracks[current_track as usize].path.clone();
                state.stop_playback.store(false, Ordering::Relaxed);
                state.playback_ended.store(false, Ordering::Relaxed);
                state.playback_disc_error.store(false, Ordering::Relaxed);
                let offset = state.playback_start_offset;
                state.heard_position.store(offset.to_bits(), Ordering::Relaxed);
                (file_path, state.stop_playback.clone(), offset,
                 state.current_position.clone(), state.volume.clone(),
                 state.playback_ended.clone(), state.heard_position.clone(), state.vis_tap.clone())
            };
            // A crossfaded start ramps in over the same span the outgoing track
            // ramps out; `fade_out_secs` is how the player later asks *this*
            // track to hand over in turn.
            let (fade_in, fade_out_arc) = {
                let mut state = self.state.lock().unwrap();
                state.crossfade_armed = false;
                state.fade_out_secs = Arc::new(AtomicU64::new(0));
                let fi = if state.crossfade && state.pending_fade_in { state.crossfade_secs } else { 0.0 };
                state.pending_fade_in = false;
                (fi, state.fade_out_secs.clone())
            };
            let handle = thread::spawn(move || {
                crate::file_player::play_local_file(file_path, start_offset, stop_flag, volume_arc,
                                heard_position_arc, current_position, playback_ended_arc, vis_tap,
                                fade_in, fade_out_arc);
            });
            self.state.lock().unwrap().playback_thread = Some(handle);
            self.as_mut().set_is_playing(true);
            {
                let mut state = self.state.lock().unwrap();
                let pos = std::time::Duration::from_secs_f64(
                    f64::from_bits(state.heard_position.load(Ordering::Relaxed)).max(0.0)
                );
                if let Some(ref mut h) = state.smtc_handle {
                    h.update(crate::smtc::SmtcUpdate::Playing { progress: pos });
                }
            }
            return;
        }

        // Extract needed data and release the CD reader handle so the thread can open its own
        let (drive_path, track_number, stop_flag, start_offset, current_position, volume_arc, playback_ended_arc, playback_error_arc, heard_position_arc, vis_tap) = {
            let mut state = self.state.lock().unwrap();

            let drive_path = match state.current_drive_path.clone() {
                Some(p) => p,
                None => { eprintln!("No drive selected"); return; }
            };
            // Guard: tracks must be loaded (toc is no longer kept after load)
            if state.tracks.is_empty() {
                eprintln!("No tracks loaded"); return;
            }
            let track_number = if current_track >= 0 && (current_track as usize) < state.tracks.len() {
                state.tracks[current_track as usize].track_number
            } else {
                eprintln!("Invalid track index"); return;
            };
            state.stop_playback.store(false, Ordering::Relaxed);
            state.playback_ended.store(false, Ordering::Relaxed);
            state.playback_disc_error.store(false, Ordering::Relaxed);
            let stop_flag = state.stop_playback.clone();
            let start_offset = state.playback_start_offset;
            let current_position = state.current_position.clone();
            let volume_arc = state.volume.clone();
            let playback_ended_arc = state.playback_ended.clone();
            let playback_error_arc  = state.playback_disc_error.clone();
            let heard_position_arc = state.heard_position.clone();
            // Seed heard_position to the start offset so the seek bar is correct immediately.
            heard_position_arc.store(start_offset.to_bits(), Ordering::Relaxed);
            // Release the shared CdReader; the playback thread will open its own.
            state.cd_reader = None;
            (drive_path, track_number, stop_flag, start_offset, current_position, volume_arc, playback_ended_arc, playback_error_arc, heard_position_arc, state.vis_tap.clone())
        };

        let handle = thread::spawn(move || {
            // Create audio controller owned by this thread.
            let audio_controller = match AudioController::new() {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("Failed to init audio: {}", e);
                    playback_ended_arc.store(true, Ordering::Relaxed);
                    return;
                }
            };

            // Open an exclusive CD reader handle for this thread.
            // Retry up to 10 times with increasing delays to let the disc
            // spin up — a freshly-inserted or just-stopped disc may not be
            // ready to open for a second or two.
            let reader = {
                let mut last_err = String::new();
                let mut result = None;
                for attempt in 0..10u32 {
                    if stop_flag.load(Ordering::Relaxed) { return; }
                    match cd_reader::open_drive(&drive_path) {
                        Ok(r) => { result = Some(r); break; }
                        Err(e) => {
                            last_err = e.to_string();
                            let delay_ms = 200 * (1 << attempt.min(4)); // 200..3200 ms
                            eprintln!("[play] open_drive attempt {} failed: {} — retrying in {}ms", attempt+1, e, delay_ms);
                            thread::sleep(std::time::Duration::from_millis(delay_ms));
                        }
                    }
                }
                match result {
                    Some(r) => r,
                    None => {
                        eprintln!("[play] open_drive gave up: {}", last_err);
                        playback_error_arc.store(true, Ordering::Relaxed);
                        playback_ended_arc.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            };

            // Read TOC — also retry; the drive may be readable but the TOC
            // not yet available immediately after spin-up.
            let toc = {
                let mut last_err = String::new();
                let mut result = None;
                for attempt in 0..5u32 {
                    if stop_flag.load(Ordering::Relaxed) { return; }
                    match cd_reader::read_toc(&reader) {
                        Ok(t) => { result = Some(t); break; }
                        Err(e) => {
                            last_err = e.to_string();
                            let delay_ms = 300 * (1 << attempt.min(3));
                            eprintln!("[play] read_toc attempt {} failed: {} — retrying in {}ms", attempt+1, e, delay_ms);
                            thread::sleep(std::time::Duration::from_millis(delay_ms));
                        }
                    }
                }
                match result {
                    Some(t) => t,
                    None => {
                        eprintln!("[play] read_toc gave up: {}", last_err);
                        playback_error_arc.store(true, Ordering::Relaxed);
                        playback_ended_arc.store(true, Ordering::Relaxed);
                        return;
                    }
                }
            };

            use cd_da_reader::{TrackStreamConfig, RetryConfig};
            let stream_cfg = TrackStreamConfig {
                sectors_per_chunk: 6,
                retry: RetryConfig {
                    max_attempts: 5,
                    initial_backoff_ms: 30,
                    max_backoff_ms: 500,
                    reduce_chunk_on_retry: true,
                    min_sectors_per_read: 1,
                },
            };

            let mut stream = match reader.open_track_stream(&toc, track_number, stream_cfg) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Failed to open track stream: {}", e);
                    playback_error_arc.store(true, Ordering::Relaxed);
                    playback_ended_arc.store(true, Ordering::Relaxed);
                    return;
                }
            };

            if start_offset > 0.0 {
                if let Err(e) = stream.seek_to_seconds(start_offset as f32) {
                    eprintln!("Seek failed: {}", e);
                }
            }

            // Software volume: apply as a per-chunk linear ramp to avoid audible
            // clicks when the user moves the volume slider. Never touch rodio's
            // own set_volume (which would apply gain mid-stream and pop).
            let mut current_vol = f64::from_bits(volume_arc.load(Ordering::Relaxed)) as f32;

            // heard_position tracking: we maintain a small ring of chunk-start
            // positions parallel to what's queued in rodio. The front of the ring
            // is the start of the chunk currently being played — this is what we
            // show on the seek bar and use as the pause resume point.
            // Each entry is (chunk start seconds, chunk length in frames); the
            // frame counts let the visualizer subtract what is still queued so
            // it shows the audio being heard rather than the one just read.
            let mut pending: std::collections::VecDeque<(f64, u64)> = std::collections::VecDeque::new();

            // Frames the visualizer has been fed, and how many of those have
            // actually reached the device. The tap is attached with a channel
            // count of 1 because the counter is already in mono frames.
            let mut vis_written: u64 = 0;
            let vis_played = Arc::new(AtomicU64::new(0));
            if let Ok(mut tap) = vis_tap.lock() { tap.attach(vis_played.clone(), 1); }
            let publish_vis = |pending: &std::collections::VecDeque<(f64, u64)>, written: u64| {
                let queued: u64 = pending.iter().map(|&(_, n)| n).sum();
                vis_played.store(written.saturating_sub(queued), Ordering::Relaxed);
            };

            loop {
                if stop_flag.load(Ordering::Relaxed) {
                    audio_controller.stop();
                    break;
                }

                match stream.next_chunk() {
                    Ok(Some(chunk)) => {
                        // Compute the track-relative seconds span of this chunk.
                        // chunk is raw CD audio: 16-bit LE stereo at 44100 Hz.
                        let chunk_secs = chunk.len() as f64 / (4.0 * 44100.0); // 4 = 2ch * 2 bytes
                        let chunk_end   = stream.current_seconds() as f64;
                        let chunk_start = chunk_end - chunk_secs;

                        // Apply a smooth volume ramp over all samples in this chunk.
                        // Ramping from current_vol → target_vol avoids discontinuities.
                        let target_vol = f64::from_bits(volume_arc.load(Ordering::Relaxed)) as f32;
                        let raw = audio_player::bytes_to_f32_samples(&chunk);
                        // Feed the visualizer with pre-volume samples so bar
                        // heights are independent of the playback volume.
                        if let Ok(mut tap) = vis_tap.lock() { tap.push(&raw, 2); }
                        let chunk_frames = (raw.len() / 2) as u64;
                        vis_written += chunk_frames;
                        let n = raw.len() as f32;
                        let samples: Vec<f32> = raw.iter().enumerate().map(|(i, &s)| {
                            let t = i as f32 / n;
                            (s * (current_vol + (target_vol - current_vol) * t)).clamp(-1.0, 1.0)
                        }).collect();
                        current_vol = target_vol;

                        audio_controller.append_samples(samples, 44100, 2);

                        // Register this chunk in the heard-position ring.
                        pending.push_back((chunk_start, chunk_frames));
                        publish_vis(&pending, vis_written);

                        // Throttle: keep at most 1 chunk queued ahead so volume
                        // changes take effect quickly (within one chunk, ~80 ms).
                        while audio_controller.queue_len() > 1
                            && !stop_flag.load(Ordering::Relaxed)
                        {
                            thread::sleep(std::time::Duration::from_millis(50));
                            // During the wait, rodio may finish chunks. Prune the ring
                            // so its length matches the actual queue depth and the
                            // heard position stays fresh.
                            let q = audio_controller.queue_len();
                            while pending.len() > q.max(1) { pending.pop_front(); }
                            if let Some(&(hp, _)) = pending.front() {
                                heard_position_arc.store(hp.to_bits(), Ordering::Relaxed);
                            }
                            publish_vis(&pending, vis_written);
                            thread::sleep(std::time::Duration::from_millis(20));
                        }

                        // Prune ring to match current queue depth.
                        let q = audio_controller.queue_len();
                        while pending.len() > q.max(1) { pending.pop_front(); }

                        // Publish heard position (oldest queued chunk's start).
                        let heard = pending.front().map(|&(hp, _)| hp).unwrap_or(chunk_start);
                        heard_position_arc.store(heard.to_bits(), Ordering::Relaxed);
                        publish_vis(&pending, vis_written);

                        // Also publish read-ahead for any code that still uses it.
                        current_position.store(chunk_end.to_bits(), Ordering::Relaxed);
                    }
                    Ok(None) => {
                        // All CD data read — drain the audio device before dropping.
                        while !audio_controller.is_empty() {
                            if stop_flag.load(Ordering::Relaxed) {
                                audio_controller.stop();
                                break;
                            }
                            // Keep heard position advancing toward the track end.
                            let q = audio_controller.queue_len();
                            while pending.len() > q.max(1) { pending.pop_front(); }
                            if let Some(&(hp, _)) = pending.front() {
                                heard_position_arc.store(hp.to_bits(), Ordering::Relaxed);
                            }
                            publish_vis(&pending, vis_written);
                            thread::sleep(std::time::Duration::from_millis(50));
                        }
                        break;
                    }
                    Err(e) => { eprintln!("Read error: {}", e); playback_error_arc.store(true, Ordering::Relaxed); break; }
                }
            }
            if !stop_flag.load(Ordering::Relaxed) {
                playback_ended_arc.store(true, Ordering::Relaxed);
            }
        });
        
        {
            let mut state = self.state.lock().unwrap();
            state.playback_thread = Some(handle);
        }
        
        self.as_mut().set_is_playing(true);
        {
            let mut state = self.state.lock().unwrap();
            let pos = std::time::Duration::from_secs_f64(
                f64::from_bits(state.heard_position.load(Ordering::Relaxed)).max(0.0)
            );
            if let Some(ref mut h) = state.smtc_handle {
                h.update(crate::smtc::SmtcUpdate::Playing { progress: pos });
            }
        }
    }

    fn stop_playback_internal(mut self: Pin<&mut Self>) {
        {
            let state = self.state.lock().unwrap();
            state.stop_playback.store(true, Ordering::Relaxed);
        }
        // An outgoing crossfade is still audible on its own thread and its own
        // stop flag; it has to be cut too or it would outlive the stop.
        let fading = {
            let mut state = self.state.lock().unwrap();
            if let Some(flag) = state.fading_stop.take() { flag.store(true, Ordering::Relaxed); }
            state.fading_thread.take()
        };
        if let Some(h) = fading {
            let _ = h.join();
        }
        let handle = self.state.lock().unwrap().playback_thread.take();
        if let Some(h) = handle {
            let _ = h.join();
        }
        // Re-open the CdReader now that the thread has released its exclusive handle.
        {
            let mut state = self.state.lock().unwrap();
            if !state.is_file_mode && state.cd_reader.is_none() {
                if let Some(path) = state.current_drive_path.clone() {
                    if let Ok(reader) = cd_reader::open_drive(&path) {
                        state.cd_reader = Some(reader);
                    }
                }
            }
        }
        self.as_mut().set_is_playing(false);
    }

    /// Drain the spectrum tap and publish the bar heights as a `QStringList` of
    /// `0..1` values for the GUI. Called on a ~60 fps timer while playing.
    ///
    /// Processing (audioviz / crav-inspired), on the volume-independent samples
    /// captured pre-gain in the decode threads:
    /// 1. **Auto-gain** — a reference level tracks current loudness so quiet and
    ///    loud tracks both fill the display; it rises fast, relaxes slowly, and
    ///    has a floor so silence isn't amplified into noise.
    /// 2. **Logarithmic (dB) scaling** — the loudest band maps to full height and
    ///    anything below `FLOOR_DB` maps to 0, diminishing quiet noise.
    /// 3. **Gravity** — bars snap up to new energy, then fall under acceleration
    ///    (delta-time driven) so they glide instead of shaking.
    pub fn update_visualizer(mut self: Pin<&mut Self>) {
        // The frontend picks the bar count from the width it has to draw in.
        let want_bands = (*self.as_ref().vis_band_count()).max(1) as usize;
        let smoothed = {
            let mut guard = self.state.lock().unwrap();
            let st = &mut *guard;
            let raw = {
                let tap = st.vis_tap.lock().unwrap();
                audio_player::compute_bands(&tap, want_bands)
            };
            let n = raw.len();
            if st.vis_smooth.len() != n { st.vis_smooth = vec![0.0; n]; }
            if st.vis_vel.len()    != n { st.vis_vel    = vec![0.0; n]; }

            // Real delta-time keeps motion smooth regardless of timer jitter;
            // clamp guards against a big gap after a pause (the timer is idle
            // while stopped, so `now - last` can be large on resume).
            let now = std::time::Instant::now();
            let dt = match st.vis_last_update {
                Some(t) => (now - t).as_secs_f32().clamp(0.0, 0.1),
                None    => 1.0 / 60.0,
            };
            st.vis_last_update = Some(now);

            // 1. Auto-gain reference: rise fast (~50 ms) to catch a louder
            //    passage, relax slowly (~1.2 s) when it quiets, floored so dead
            //    silence can't crank the gain up into background noise.
            const REF_FLOOR: f32 = 1.0e-3;
            let frame_peak = raw.iter().copied().fold(0.0f32, f32::max);
            let k = if frame_peak > st.vis_ref {
                1.0 - (-dt / 0.05).exp()
            } else {
                1.0 - (-dt / 1.2).exp()
            };
            st.vis_ref += (frame_peak - st.vis_ref) * k;
            if st.vis_ref < REF_FLOOR { st.vis_ref = REF_FLOOR; }

            // 2. + 3. dB scaling then gravity. FLOOR_DB sets the visible dynamic
            //    range (more negative = show quieter detail); GRAVITY sets the
            //    falloff speed (higher = shorter tails).
            const FLOOR_DB: f32 = -42.0;
            const GRAVITY:  f32 = 16.0;
            for i in 0..n {
                let norm = (raw[i] / st.vis_ref).min(1.0);
                let target = if norm <= 1.0e-4 {
                    0.0
                } else {
                    (1.0 - 20.0 * norm.log10() / FLOOR_DB).clamp(0.0, 1.0)
                };
                if target >= st.vis_smooth[i] {
                    st.vis_smooth[i] = target;      // snap up to new energy
                    st.vis_vel[i] = 0.0;
                } else {
                    st.vis_vel[i] += GRAVITY * dt;  // fall under gravity
                    st.vis_smooth[i] -= st.vis_vel[i] * dt;
                    if st.vis_smooth[i] < 0.0 { st.vis_smooth[i] = 0.0; st.vis_vel[i] = 0.0; }
                }
            }
            st.vis_smooth.clone()
        };
        let mut list = QStringList::default();
        for v in smoothed {
            list.append(QString::from(&format!("{:.3}", v)));
        }
        self.as_mut().set_vis_bands(list);
    }

    pub fn update_position(mut self: Pin<&mut Self>) {
        // Reap a finished crossfade tail. Without this `fading_thread` stays
        // occupied and the guard in the tick below blocks every later overlap.
        {
            let mut state = self.state.lock().unwrap();
            let done = state.fading_thread.as_ref().map(|h| h.is_finished()).unwrap_or(false);
            if done {
                let h = state.fading_thread.take();
                state.fading_stop = None;
                drop(state);
                if let Some(h) = h { let _ = h.join(); }
            }
        }

        // Drain SMTC commands regardless of play state so buttons work when paused.
        let smtc_cmds: Vec<crate::smtc::SmtcCommand> = {
            let state = self.state.lock().unwrap();
            state.smtc_handle.as_ref()
                .map(|h| h.drain_commands())
                .unwrap_or_default()
        };
        for cmd in smtc_cmds {
            use crate::smtc::SmtcCommand;
            match cmd {
                SmtcCommand::Toggle => { self.as_mut().play_pause(); }
                SmtcCommand::Next => {
                    let was_playing = *self.as_ref().is_playing();
                    self.as_mut().next_track();
                    if was_playing { self.as_mut().play_pause(); }
                }
                SmtcCommand::Previous => {
                    let was_playing = *self.as_ref().is_playing();
                    self.as_mut().previous_track();
                    if was_playing { self.as_mut().play_pause(); }
                }
                SmtcCommand::Seek(s) => { self.as_mut().seek(s); }
            }
        }

        if !*self.as_ref().is_playing() {
            // Discord's cover art finishes uploading on a background thread, and
            // only sync_discord collects it. Keep ticking it while paused or the
            // art never lands until playback resumes. update() is a cheap no-op
            // once the presence it pushed is up to date.
            let current_track = *self.as_ref().current_track();
            self.state.lock().unwrap().sync_discord(current_track, false);
            return;
        }
        let ended = {
            let state = self.state.lock().unwrap();
            state.playback_ended.swap(false, Ordering::Relaxed)
        };
        if ended {
            let handle = self.state.lock().unwrap().playback_thread.take();
            if let Some(h) = handle { let _ = h.join(); }
            // Check if the thread exited due to a disc error (ejection) rather than
            // a natural track end.  If so, clear tracks and let check_drive recover.
            let disc_error = self.state.lock().unwrap()
                .playback_disc_error.swap(false, Ordering::Relaxed);
            {
                let mut state = self.state.lock().unwrap();
                state.playback_start_offset = 0.0;
                state.current_position.store(0u64, Ordering::Relaxed);
                state.heard_position.store(0u64, Ordering::Relaxed);
            }
            self.as_mut().set_is_playing(false);
            self.as_mut().set_current_time(0.0);

            if disc_error {
                {
                    let mut state = self.state.lock().unwrap();
                    state.tracks.clear();
                    state.metadata_loaded = false;
                    state.current_disc_id.clear();
                    if let Some(ref mut h) = state.smtc_handle {
                        h.update(crate::smtc::SmtcUpdate::Stopped);
                    }
                }
                self.as_mut().set_track_names(QStringList::default());
                self.as_mut().set_track_titles(QStringList::default());
                self.as_mut().set_track_artists(QStringList::default());
                self.as_mut().set_track_paths(QStringList::default());
                self.as_mut().set_total_tracks(0);
                self.as_mut().set_current_track(-1);
                self.as_mut().set_total_time(0.0);
                self.as_mut().set_album_title(QString::from("Unknown Album"));
                self.as_mut().set_album_artist(QString::from("Unknown Artist"));
                self.as_mut().set_album_year(QString::from(""));
                self.as_mut().set_cd_disc_number(0);
                self.as_mut().set_cd_disc_count(0);
                self.as_mut().set_cover_art_path(QString::from(""));
                self.as_mut().set_drive_status(QString::from("No disc inserted"));
                self.as_mut().set_lyric_lines(QStringList::default());
                self.as_mut().set_lyric_times(QStringList::default());
                return;
            }

            // Queue entries come before the rest of the album; `advance` also
            // handles returning to the album once the queue drains.
            if self.as_mut().advance() {
                self.start_playback();
            } else {
                let mut state = self.state.lock().unwrap();
                if let Some(ref mut h) = state.smtc_handle {
                    h.update(crate::smtc::SmtcUpdate::Stopped);
                }
            }
            return;
        }
        let pos = {
            let state = self.state.lock().unwrap();
            f64::from_bits(state.heard_position.load(Ordering::Relaxed))
        };
        self.as_mut().set_current_time(pos);

        // Start the overlap once the track is within a crossfade of its end.
        // File mode only: CD playback streams from the drive and can't have two
        // reads in flight, and repeat-one has nothing to fade into.
        {
            let total = *self.as_ref().total_time();
            let should = {
                let st = self.state.lock().unwrap();
                st.crossfade
                    && !st.crossfade_armed
                    && st.is_file_mode
                    && st.fading_thread.is_none()
                    && total > st.crossfade_secs * 2.0
                    && pos > 0.0
                    && total - pos <= st.crossfade_secs
            };
            if should && *self.as_ref().repeat_mode() != 2 {
                self.as_mut().begin_crossfade();
                return;
            }
        }

        // SMTC progress and Discord presence only need second-level granularity;
        // pushing them on every 100 ms tick costs a WinRT/IPC call each time.
        let sec = pos.max(0.0) as i64;
        let should_push = {
            let mut state = self.state.lock().unwrap();
            let changed = state.last_progress_push_sec != sec;
            if changed { state.last_progress_push_sec = sec; }
            changed
        };
        if should_push {
            {
                let mut state = self.state.lock().unwrap();
                if let Some(ref mut h) = state.smtc_handle {
                    h.update(crate::smtc::SmtcUpdate::Playing {
                        progress: std::time::Duration::from_secs_f64(pos.max(0.0)),
                    });
                }
            }
            let current_track = *self.as_ref().current_track();
            let is_playing    = *self.as_ref().is_playing();
            self.state.lock().unwrap().sync_discord(current_track, is_playing);
        }
    }

    pub fn check_drive(mut self: Pin<&mut Self>) {
        if *self.as_ref().is_playing() || *self.as_ref().is_loading() {
            return;
        }
        if self.state.lock().unwrap().is_file_mode {
            return;
        }
        if self.state.lock().unwrap().disc_check_active.load(Ordering::Relaxed) {
            return;
        }
        let maybe_path = self.state.lock().unwrap().current_drive_path.clone();
        let current_path = match maybe_path {
            Some(p) => p,
            None => return,
        };
        let result_slot = {
            let state = self.state.lock().unwrap();
            let slot = state.disc_load_result.clone();
            *slot.lock().unwrap() = None;
            slot
        };
        // Drop the reader so the background thread can open an exclusive handle.
        self.state.lock().unwrap().cd_reader = None;
        let meta_already_loaded = self.state.lock().unwrap().metadata_loaded;
        {
            let mut state = self.state.lock().unwrap();
            if let Some(old) = state.disc_load_thread.take() {
                drop(state);
                let _ = old.join();
            }
        }
        let handle = thread::spawn(move || {
            let result = match cd_reader::open_drive(&current_path) {
                Err(_) => PendingDiscResult::Unavailable {
                    status: "Drive unavailable".to_string(),
                },
                Ok(reader) => match cd_reader::read_toc(&reader) {
                    Ok(toc) => {
                        let tracks = cd_reader::get_track_info(&toc);
                        let durations = tracks
                            .iter()
                            .map(|t| cd_reader::format_duration(t.duration_seconds))
                            .collect();
                        let metadata = if meta_already_loaded {
                            None
                        } else {
                            crate::musicbrainz::lookup_metadata(&toc)
                        };
                        let disc_id = crate::musicbrainz::calculate_disc_id(&toc);
                        PendingDiscResult::Loaded { tracks, durations, metadata, disc_id }
                    }
                    Err(_) => PendingDiscResult::Empty {
                        status: "No disc inserted".to_string(),
                    },
                },
            };
            *result_slot.lock().unwrap() = Some(result);
        });
        {
            let mut state = self.state.lock().unwrap();
            state.disc_load_thread = Some(handle);
            state.disc_check_active.store(true, Ordering::Relaxed);
        }
        let no_disc = *self.as_ref().total_tracks() == 0;
        if no_disc {
            self.as_mut().set_is_loading(true);
        }
    }

    pub fn poll_load(mut self: Pin<&mut Self>) {
        let result = {
            let result_slot = self.state.lock().unwrap().disc_load_result.clone();
            let x = result_slot.lock().unwrap().take();
            x
        };
        let Some(result) = result else { return };
        let t = self.state.lock().unwrap().disc_load_thread.take();
        if let Some(t) = t { let _ = t.join(); }
        self.state.lock().unwrap().disc_check_active.store(false, Ordering::Relaxed);
        let was_user_load = *self.as_ref().is_loading();
        if was_user_load {
            self.as_mut().set_is_loading(false);
        }
        let had_track_count = *self.as_ref().total_tracks();
        let had_tracks = had_track_count > 0;
        let drive_path = self.state.lock().unwrap().current_drive_path.clone();
        match result {
            PendingDiscResult::Loaded { tracks, durations, metadata, disc_id } => {
                let is_new_disc = !had_tracks;
                let track_count = tracks.len() as i32;

                self.state.lock().unwrap().current_disc_id = disc_id.clone();

                if !was_user_load && had_tracks && track_count == had_track_count {
                    let meta_ok = self.state.lock().unwrap().metadata_loaded;
                    if meta_ok || metadata.is_none() {
                        let mut state = self.state.lock().unwrap();
                        state.tracks = tracks;
                        if let Some(ref path) = drive_path {
                            state.cd_reader = cd_reader::open_drive(path).ok();
                        }
                        return;
                    }
                }

                let mut dur_list = QStringList::default();
                for d in &durations {
                    dur_list.append(QString::from(d.as_str()));
                }

                let mut title_list   = QStringList::default();
                let mut artist_list  = QStringList::default();
                let mut title_plain  = Vec::new();
                let mut artist_plain = Vec::new();
                for i in 0..tracks.len() {
                    let title = metadata
                        .as_ref()
                        .and_then(|m| m.track_titles.get(i))
                        .map(String::as_str)
                        .unwrap_or("");
                    let display = if title.is_empty() {
                        format!("Track {}", i + 1)
                    } else {
                        title.to_string()
                    };
                    title_list.append(QString::from(display.as_str()));
                    title_plain.push(display);

                    let ta = metadata
                        .as_ref()
                        .and_then(|m| m.track_artists.get(i))
                        .map(String::as_str)
                        .unwrap_or("");
                    artist_list.append(QString::from(ta));
                    artist_plain.push(ta.to_string());
                }

                if let Some(ref meta) = metadata {
                    self.as_mut().set_album_title(QString::from(meta.title.as_str()));
                    self.as_mut().set_album_artist(QString::from(meta.artist.as_str()));
                    self.as_mut().set_album_year(QString::from(meta.year.as_str()));
                    self.as_mut().set_cd_disc_number(meta.disc_number as i32);
                    self.as_mut().set_cd_disc_count(meta.disc_count as i32);
                    let art = meta.cover_art_url.as_deref().unwrap_or("");
                    self.as_mut().set_cover_art_path(QString::from(art));
                    {
                        let mut state = self.state.lock().unwrap();
                        state.metadata_loaded    = true;
                        state.smtc_album         = meta.title.clone();
                        state.smtc_album_artist  = meta.artist.clone();
                        state.smtc_cover_url     = if art.is_empty() { String::new() } else { art.to_string() };
                        state.track_titles_plain  = title_plain;
                        state.track_artists_plain = artist_plain;
                    }
                } else {
                    self.as_mut().set_album_title(QString::from("Unknown Album"));
                    self.as_mut().set_album_artist(QString::from("Unknown Artist"));
                    self.as_mut().set_album_year(QString::from(""));
                    self.as_mut().set_cd_disc_number(0);
                    self.as_mut().set_cd_disc_count(0);
                    self.as_mut().set_cover_art_path(QString::from(""));
                    {
                        let mut state = self.state.lock().unwrap();
                        state.metadata_loaded    = false;
                        state.smtc_album.clear();
                        state.smtc_album_artist.clear();
                        state.smtc_cover_url.clear();
                        state.track_titles_plain  = title_plain;
                        state.track_artists_plain = artist_plain;
                    }
                }

                {
                    let mut state = self.state.lock().unwrap();
                    state.tracks = tracks;
                    state.toc = None;
                    if let Some(ref path) = drive_path {
                        state.cd_reader = cd_reader::open_drive(path).ok();
                    }
                }
                self.as_mut().set_track_names(dur_list);
                self.as_mut().set_track_titles(title_list);
                self.as_mut().set_track_artists(artist_list);
                // CD tracks have no queueable path.
                self.as_mut().set_track_paths(QStringList::default());
                self.as_mut().set_total_tracks(track_count);
                self.as_mut().set_drive_status(QString::from(""));
                if is_new_disc && track_count > 0 {
                    self.as_mut().set_current_track(-1);
                    self.as_mut().set_current_time(0.0);
                    self.as_mut().set_total_time(0.0);
                }
            }
            PendingDiscResult::Empty { status } | PendingDiscResult::Unavailable { status } => {
                // Try to restore the reader handle for the next check cycle.
                if let Some(ref path) = drive_path {
                    if let Ok(r) = cd_reader::open_drive(path) {
                        self.state.lock().unwrap().cd_reader = Some(r);
                    }
                }
                if had_tracks {
                    {
                        let mut state = self.state.lock().unwrap();
                        state.toc = None;
                        state.tracks.clear();
                        state.playback_start_offset = 0.0;
                        state.current_position.store(0u64, Ordering::Relaxed);
                    }
                    self.as_mut().set_track_names(QStringList::default());
                    self.as_mut().set_track_titles(QStringList::default());
                    self.as_mut().set_track_artists(QStringList::default());
                self.as_mut().set_track_paths(QStringList::default());
                    self.as_mut().set_total_tracks(0);
                    self.as_mut().set_current_track(-1);
                    self.as_mut().set_current_time(0.0);
                    self.as_mut().set_total_time(0.0);
                    self.as_mut().set_album_title(QString::from("Unknown Album"));
                    self.as_mut().set_album_artist(QString::from("Unknown Artist"));
                    self.as_mut().set_album_year(QString::from(""));
                    self.as_mut().set_cd_disc_number(0);
                    self.as_mut().set_cd_disc_count(0);
                    self.as_mut().set_cover_art_path(QString::from(""));
                    self.as_mut().set_lyric_lines(QStringList::default());
                    self.as_mut().set_lyric_times(QStringList::default());
                    {
                        let mut state = self.state.lock().unwrap();
                        state.metadata_loaded = false;
                        state.track_titles_plain.clear();
                        state.track_artists_plain.clear();
                        state.smtc_album.clear();
                        state.smtc_album_artist.clear();
                        state.smtc_cover_url.clear();
                    }
                    {
                        let mut state = self.state.lock().unwrap();
                        if let Some(ref mut h) = state.smtc_handle {
                            h.update(crate::smtc::SmtcUpdate::Metadata {
                                title:     status.clone(),
                                artist:    "Kanae".to_string(),
                                album:     String::new(),
                                cover_url: None,
                                duration:  None,
                            });
                            h.update(crate::smtc::SmtcUpdate::Stopped);
                        }
                    }
                }
                self.as_mut().set_drive_status(QString::from(status.as_str()));
            }
        }
    }

    pub fn set_volume_level(self: Pin<&mut Self>, v: f64) {
        let state = self.state.lock().unwrap();
        state.volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    pub fn init_smtc(self: Pin<&mut Self>) {
        println!("[smtc] init_smtc() called from QML");
        eprintln!("[smtc] initialising...");
        match crate::smtc::init_for_gui() {
            Some(handle) => {
                let mut state = self.state.lock().unwrap();
                state.smtc_handle = Some(handle);
                if let Some(ref mut h) = state.smtc_handle {
                    h.update(crate::smtc::SmtcUpdate::Metadata {
                        title:     "No disc".to_string(),
                        artist:    "Kanae".to_string(),
                        album:     String::new(),
                        cover_url: None,
                        duration:  None,
                    });
                    h.update(crate::smtc::SmtcUpdate::Stopped);
                }
                eprintln!("[smtc] handle stored in player");
            }
            None => eprintln!("[smtc] init returned None; media session unavailable"),
        }
    }

    pub fn fetch_lyrics(
        mut self: Pin<&mut Self>,
        track_name: QString,
        artist_name: QString,
        album_name: QString,
        duration_secs: f64,
    ) {
        self.as_mut().set_lyric_lines(QStringList::default());
        self.as_mut().set_lyric_times(QStringList::default());
        self.as_mut().set_lyrics_loading(true);

        let track_name = track_name.to_string();
        let artist_name = artist_name.to_string();
        let album_name = album_name.to_string();

        if track_name.is_empty() {
            self.as_mut().set_lyrics_loading(false);
            return;
        }

        let disc_id   = self.state.lock().unwrap().current_disc_id.clone();
        let track_idx = *self.as_ref().current_track();
        // For file tracks, grab the path for a better cache key.
        let file_path: String = {
            let st = self.state.lock().unwrap();
            if st.is_file_mode && track_idx >= 0 {
                st.file_tracks.get(track_idx as usize)
                    .map(|t| t.path.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else { String::new() }
        };
        let lrc_limit_disabled = crate::library_cache::load_settings().lrc_limit_disabled;
        let generation = {
            let state = self.state.lock().unwrap();
            state.lyric_fetch_generation.fetch_add(1, Ordering::SeqCst) + 1
        };
        let gen_arc = self.state.lock().unwrap().lyric_fetch_generation.clone();
        let result_slot = {
            let state = self.state.lock().unwrap();
            let slot = state.lyric_result.clone();
            *slot.lock().unwrap() = None;
            slot
        };
        if let Some(old) = self.state.lock().unwrap().lyric_fetch_thread.take() {
            drop(old);
        }

        let handle = thread::spawn(move || {
            use crate::lyric_cache::{LyricContentCache, cd_key, file_key};

            // A sidecar .lrc next to the audio file always wins over web/cached lyrics.
            if let Some(lines) = crate::lrclib::load_local_lrc(&file_path) {
                if gen_arc.load(Ordering::SeqCst) == generation {
                    *result_slot.lock().unwrap() = Some(Some(lines));
                }
                return;
            }

            let cache_key = if !disc_id.is_empty() && track_idx >= 0 {
                cd_key(&disc_id, track_idx)
            } else {
                file_key(&file_path, &track_name, &artist_name)
            };

            let mut content_cache = LyricContentCache::load();

            // Fast-reject: track previously had no lyrics.
            if content_cache.has_no_lyrics(&cache_key) {
                eprintln!("[lrclib] no-lyrics cache hit for key {}", cache_key);
                if gen_arc.load(Ordering::SeqCst) == generation {
                    *result_slot.lock().unwrap() = Some(None);
                }
                return;
            }

            // Content cache hit: re-parse the cached raw LRC.
            if let Some(raw_lrc) = content_cache.get_lrc(&cache_key) {
                eprintln!("[lrclib] lrc content cache hit for key {}", cache_key);
                content_cache.save();
                let lines = crate::lrclib::parse_lrc(&raw_lrc);
                if gen_arc.load(Ordering::SeqCst) == generation {
                    *result_slot.lock().unwrap() = Some(if lines.is_empty() { None } else { Some(lines) });
                }
                return;
            }

            // Cache miss → call the API.
            let result: Option<Vec<crate::lrclib::LyricLine>> =
                match crate::lrclib::fetch_synced_lyrics(&track_name, &artist_name, &album_name, duration_secs) {
                    Some((_id, raw_lrc, lines)) => {
                        content_cache.insert_lrc(&cache_key, &raw_lrc, lrc_limit_disabled);
                        content_cache.save();
                        Some(lines)
                    }
                    None => {
                        content_cache.insert_no_lyrics(&cache_key, lrc_limit_disabled);
                        content_cache.save();
                        None
                    }
                };

            // Only publish the result if this fetch is still the current one.
            if gen_arc.load(Ordering::SeqCst) == generation {
                *result_slot.lock().unwrap() = Some(result);
            } else {
                eprintln!("[lrclib] discarding stale result for generation {}", generation);
            }
        });
        self.state.lock().unwrap().lyric_fetch_thread = Some(handle);
    }

    pub fn poll_lyrics(mut self: Pin<&mut Self>) {
        let result = {
            let state = self.state.lock().unwrap();
            let x = state.lyric_result.lock().unwrap().take();
            x
        };
        let Some(maybe_lines) = result else { return };
        self.as_mut().set_lyrics_loading(false);
        if let Some(t) = self.state.lock().unwrap().lyric_fetch_thread.take() {
            drop(t);
        }
        match maybe_lines {
            Some(lines) => {
                self.state.lock().unwrap().lyrics_src = lines;
                self.reapply_lyrics();
            }
            None => {
                self.state.lock().unwrap().lyrics_src.clear();
            }
        }
    }

    /// Rebuild the displayed lyric lists from the stored originals, applying the
    /// current romanize-lyrics setting. Invoked from QML when the user toggles
    /// the setting, and internally after each fetch — no re-fetch required.
    pub fn reapply_lyrics(mut self: Pin<&mut Self>) {
        let romanize = crate::library_cache::load_settings().romanize_lyrics;
        let mut lines = self.state.lock().unwrap().lyrics_src.clone();
        crate::romaji::romanize_lines(&mut lines, romanize);
        let mut texts = QStringList::default();
        let mut times = QStringList::default();
        for line in &lines {
            texts.append(QString::from(line.text.as_str()));
            times.append(QString::from(line.time_secs.to_string().as_str()));
        }
        self.as_mut().set_lyric_lines(texts);
        self.as_mut().set_lyric_times(times);
    }

    /// Toggle Discord Rich Presence at runtime, applying immediately (clears the
    /// presence when disabled, re-publishes the current track when enabled).
    pub fn set_discord_enabled(self: Pin<&mut Self>, value: bool) {
        let current_track = *self.as_ref().current_track();
        let is_playing    = *self.as_ref().is_playing();
        let mut state = self.state.lock().unwrap();
        state.discord_enabled = value;
        state.sync_discord(current_track, is_playing);
    }

    pub fn open_files_dialog(self: Pin<&mut Self>) {
        let paths = rfd::FileDialog::new()
            .add_filter("Audio", &["mp3", "mp2", "mp1", "flac", "ogg", "opus", "m4a", "mp4", "aac", "alac", "wav", "aiff", "aif", "caf", "mka", "wma", "ape"])
            .set_title("Open Audio Files")
            .pick_files();
        if let Some(paths) = paths {
            let strs: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
            let is_single = strs.len() == 1;
            self.load_local_tracks(strs, is_single);
        }
    }

    pub fn open_folder_dialog(self: Pin<&mut Self>) {
        let path = rfd::FileDialog::new()
            .set_title("Open Folder")
            .pick_folder();
        if let Some(path) = path {
            self.load_local_tracks(vec![path.to_string_lossy().into_owned()], false);
        }
    }

    pub fn open_dropped_paths(mut self: Pin<&mut Self>, urls: QStringList) {
        eprintln!("[dbg] open_dropped_paths: {} url(s)", urls.len());
        for i in 0..urls.len() {
            if let Some(s) = urls.get(i) { eprintln!("[dbg]   raw[{}]: {}", i, s); }
        }
        let paths: Vec<String> = (0..urls.len())
            .filter_map(|i| {
                let s = urls.get(i).map(|qs| qs.to_string())?;
                let path = if let Some(p) = s.strip_prefix("file:///") {
                    // Windows:  file:///C:/...  → C:/...
                    // Unix:     file:///home/... → /home/...  (re-add the leading /)
                    #[cfg(target_os = "windows")]
                    { p.to_string() }
                    #[cfg(not(target_os = "windows"))]
                    { format!("/{}" , p) }
                } else if let Some(p) = s.strip_prefix("file://") {
                    p.to_string()
                } else {
                    s.clone()
                };
                if path.is_empty() { None } else { Some(path) }
            })
            .collect();

        eprintln!("[dbg] open_dropped_paths: {} resolved path(s)", paths.len());
        for (i, p) in paths.iter().enumerate() {
            eprintln!("[dbg]   path[{}]: {} (exists={})", i, p, std::path::Path::new(p).exists());
        }

        if paths.is_empty() { eprintln!("[dbg] open_dropped_paths: no paths after resolve, returning"); return; }

        {
            let mut state = self.state.lock().unwrap();
            state.is_file_mode = false;
            state.file_tracks.clear();
            state.tracks.clear();
            state.metadata_loaded = false;
            state.smtc_album.clear();
            state.smtc_album_artist.clear();
            state.smtc_cover_url.clear();
            state.track_titles_plain.clear();
            state.track_artists_plain.clear();
            if let Some(ref mut h) = state.smtc_handle {
                h.update(crate::smtc::SmtcUpdate::Stopped);
            }
        }
        self.as_mut().stop_playback_internal();
        self.as_mut().set_is_file_mode(false);
        self.as_mut().set_is_single_file(false);
        self.as_mut().set_total_tracks(0);
        self.as_mut().set_current_track(-1);
        self.as_mut().set_current_time(0.0);
        self.as_mut().set_total_time(0.0);
        self.as_mut().set_track_names(QStringList::default());
        self.as_mut().set_track_titles(QStringList::default());
        self.as_mut().set_track_artists(QStringList::default());
                self.as_mut().set_track_paths(QStringList::default());
        self.as_mut().set_album_title(QString::from("Unknown Album"));
        self.as_mut().set_album_artist(QString::from("Unknown Artist"));
        self.as_mut().set_album_year(QString::from(""));
        self.as_mut().set_cover_art_path(QString::from(""));
        self.as_mut().set_lyric_lines(QStringList::default());
        self.as_mut().set_lyric_times(QStringList::default());

        let is_single = paths.len() == 1 && !std::path::Path::new(&paths[0]).is_dir();
        self.load_local_tracks(paths, is_single);
    }

    /// Switch back to CD mode from file mode.
    /// Clears file-mode state, restores CD mode, and triggers a disc load
    /// so the track list is repopulated from the previously selected drive.
    pub fn load_disc(mut self: Pin<&mut Self>) {
        self.as_mut().stop_playback_internal();
        {
            let mut state = self.state.lock().unwrap();
            state.is_file_mode = false;
            state.file_tracks.clear();
            state.tracks.clear();
            state.metadata_loaded = false;
            state.smtc_album.clear();
            state.smtc_album_artist.clear();
            state.smtc_cover_url.clear();
            state.track_titles_plain.clear();
            state.track_artists_plain.clear();
            if let Some(ref mut h) = state.smtc_handle {
                h.update(crate::smtc::SmtcUpdate::Stopped);
            }
        }
        self.as_mut().set_is_file_mode(false);
        self.as_mut().set_is_single_file(false);
        self.as_mut().set_total_tracks(0);
        self.as_mut().set_current_track(-1);
        self.as_mut().set_current_time(0.0);
        self.as_mut().set_total_time(0.0);
        self.as_mut().set_track_names(QStringList::default());
        self.as_mut().set_track_titles(QStringList::default());
        self.as_mut().set_track_artists(QStringList::default());
                self.as_mut().set_track_paths(QStringList::default());
        self.as_mut().refresh_disc();
    }

    pub fn eject_or_close(mut self: Pin<&mut Self>) {
        if *self.as_ref().is_file_mode() {
            {
                let mut state = self.state.lock().unwrap();
                state.is_file_mode = false;
                state.file_tracks.clear();
                state.tracks.clear();
                state.metadata_loaded    = false;
                state.smtc_album.clear();
                state.smtc_album_artist.clear();
                state.smtc_cover_url.clear();
                state.track_titles_plain.clear();
                state.track_artists_plain.clear();
                if let Some(ref mut h) = state.smtc_handle {
                    h.update(crate::smtc::SmtcUpdate::Stopped);
                }
            }
            self.as_mut().stop_playback_internal();
            self.as_mut().set_is_file_mode(false);
            self.as_mut().set_is_single_file(false);
            self.as_mut().set_total_tracks(0);
            self.as_mut().set_current_track(-1);
            self.as_mut().set_current_time(0.0);
            self.as_mut().set_total_time(0.0);
            self.as_mut().set_track_names(QStringList::default());
            self.as_mut().set_track_titles(QStringList::default());
            self.as_mut().set_track_artists(QStringList::default());
                self.as_mut().set_track_paths(QStringList::default());
            self.as_mut().set_album_title(QString::from("Unknown Album"));
            self.as_mut().set_album_artist(QString::from("Unknown Artist"));
            self.as_mut().set_album_year(QString::from(""));
            self.as_mut().set_cover_art_path(QString::from(""));
            self.as_mut().set_lyric_lines(QStringList::default());
            self.as_mut().set_lyric_times(QStringList::default());
            self.as_mut().set_drive_status(QString::from("No disc inserted"));
            // Trigger a fresh drive scan so the CD path resumes normally.
            self.as_mut().scan_drives();
        } else {
            self.as_mut().stop_playback_internal();
            let drive_path = self.state.lock().unwrap().current_drive_path.clone();
            if let Some(path) = drive_path {
                cd_reader::eject_drive(&path);
            }
        }
    }

    /// Physically eject the selected drive's tray. Unlike eject_or_close this
    /// never touches file-mode state: local playback keeps running while the
    /// tray opens.
    pub fn eject_disc(mut self: Pin<&mut Self>) {
        let (path, is_file) = {
            let state = self.state.lock().unwrap();
            let path = state.current_drive_path.clone()
                .or_else(|| state.drives.first().map(|d| d.path.clone()));
            (path, state.is_file_mode)
        };
        let Some(path) = path else { return };
        if !is_file {
            self.as_mut().stop_playback_internal();
        }
        cd_reader::eject_drive(&path);
    }

    fn load_local_tracks(self: Pin<&mut Self>, input_paths: Vec<String>, is_single: bool) {
        eprintln!("[dbg] load_local_tracks: {} input path(s), is_single={}", input_paths.len(), is_single);
        for (i, p) in input_paths.iter().enumerate() {
            eprintln!("[dbg]   input[{}]: {}", i, p);
        }
        // Local files have no disc ID — clear it so the lyric cache is never
        // read from or written to during file-mode playback.
        self.state.lock().unwrap().current_disc_id.clear();

        let tracks = crate::file_player::collect_files_from_paths(&input_paths);
        eprintln!("[dbg] load_local_tracks: collect_files_from_paths found {} track(s)", tracks.len());
        if tracks.is_empty() {
            eprintln!("[file] no audio files found in provided paths");
            return;
        }

        // Picking an album by hand supersedes anything the queue was going to
        // come back to.
        self.state.lock().unwrap().queue_resume = None;

        let meta = derive_album_meta(&tracks, &input_paths, is_single);
        self.apply_local_tracks(tracks, is_single, meta);
    }

    /// Publish a already-collected set of local tracks as the active session.
    /// Split out of `load_local_tracks` so the queue can swap sessions in and
    /// out without re-reading metadata from disk.
    fn apply_local_tracks(mut self: Pin<&mut Self>, tracks: Vec<crate::file_player::LocalTrack>, is_single: bool, meta: AlbumMeta) {
        if tracks.is_empty() { return; }
        let AlbumMeta { title: album_title, artist: album_artist, year: album_year, cover: cover_art } = meta;

        let track_count = tracks.len() as i32;
        let mut dur_list    = QStringList::default();
        let mut title_list  = QStringList::default();
        let mut artist_list = QStringList::default();
        let mut path_list   = QStringList::default();
        let mut title_plain  = Vec::new();
        let mut artist_plain = Vec::new();

        for track in &tracks {
            dur_list.append(QString::from(track.display_duration().as_str()));
            title_list.append(QString::from(track.title.as_str()));
            artist_list.append(QString::from(track.artist.as_str()));
            path_list.append(QString::from(track.path.to_string_lossy().as_ref()));
            title_plain.push(track.title.clone());
            artist_plain.push(track.artist.clone());
        }

        // Set file mode flag before stopping playback so stop_playback_internal
        // does not attempt to reopen a CD drive (it checks is_file_mode).
        // Intentionally keep current_drive_path so the user can switch back to
        // the disc without losing the selected drive.
        {
            let mut state = self.state.lock().unwrap();
            state.is_file_mode = true;
        }
        self.as_mut().stop_playback_internal();

        {
            let mut state = self.state.lock().unwrap();
            state.file_tracks        = tracks;
            state.tracks.clear();
            state.metadata_loaded    = true;
            state.smtc_album         = album_title.clone();
            state.smtc_album_artist  = album_artist.clone();
            state.smtc_cover_url     = cover_art.clone().unwrap_or_default();
            state.track_titles_plain = title_plain;
            state.track_artists_plain = artist_plain;
        }

        self.as_mut().set_is_file_mode(true);
        self.as_mut().set_is_single_file(is_single);
        self.as_mut().set_track_names(dur_list);
        self.as_mut().set_track_titles(title_list);
        self.as_mut().set_track_artists(artist_list);
        self.as_mut().set_track_paths(path_list);
        self.as_mut().set_total_tracks(track_count);
        self.as_mut().set_current_track(-1);
        self.as_mut().set_current_time(0.0);
        self.as_mut().set_total_time(0.0);
        self.as_mut().set_album_title(QString::from(album_title.as_str()));
        self.as_mut().set_album_artist(QString::from(album_artist.as_str()));
        self.as_mut().set_album_year(QString::from(album_year.as_str()));
        self.as_mut().set_cover_art_path(QString::from(cover_art.as_deref().unwrap_or("")));
        self.as_mut().set_drive_status(QString::from(""));
        self.as_mut().set_lyric_lines(QStringList::default());
        self.as_mut().set_lyric_times(QStringList::default());

        {
            let mut state = self.state.lock().unwrap();
            if let Some(ref mut h) = state.smtc_handle {
                h.update(crate::smtc::SmtcUpdate::Stopped);
            }
        }
    }
}
