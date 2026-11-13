use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::{
    event::{self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen, SetTitle},
};
use ratatui::{backend::CrosstermBackend, layout::Rect, Terminal};

use ratatui_image::{picker::{Picker, ProtocolType}, protocol::StatefulProtocol};
use crate::library_cache::{TuiImageMethod, TuiSettings};
use crate::cd_reader::{DriveInfo, TrackInfo, PendingDiscResult};
use crate::file_player::LocalTrack;
use crate::library::{LibraryAlbum, LibraryScanResult, LibrarySettings};

mod input;
mod render;
mod search;
mod settings;
mod theme;
mod thumbs;

use theme::Theme;

// library node for tui

/// display state of the audio-cd node shown at the library root.
#[derive(Clone, PartialEq)]
struct CdNodeInfo {
    /// album title when a disc is loaded, otherwise "Audio CD".
    title:   String,
    /// secondary line: artist / drive letter / "No disc".
    sub:     String,
    /// true while the disc toc/metadata is being read.
    loading: bool,
}

#[derive(Clone)]
enum TuiLibraryNode {
    Folder { path: PathBuf, name: String },
    Album  { album: LibraryAlbum },
    Cd     { info: CdNodeInfo },
}

impl TuiLibraryNode {
    fn name(&self) -> &str {
        match self {
            TuiLibraryNode::Folder { name, .. } => name,
            TuiLibraryNode::Album  { album }     => &album.album,
            TuiLibraryNode::Cd     { info }      => &info.title,
        }
    }
    fn sub(&self) -> &str {
        match self {
            TuiLibraryNode::Folder { .. }    => "",
            TuiLibraryNode::Album  { album } => &album.album_artist,
            TuiLibraryNode::Cd     { info }  => &info.sub,
        }
    }
    fn year(&self) -> &str {
        match self {
            TuiLibraryNode::Album { album } => &album.year,
            _                               => "",
        }
    }
    fn icon(&self, ic: &Icons) -> &'static str {
        match self {
            TuiLibraryNode::Folder { .. } => ic.folder,
            TuiLibraryNode::Album  { .. } => ic.album,
            TuiLibraryNode::Cd     { .. } => ic.cd,
        }
    }
}

// icon sets

#[derive(Copy, Clone)]
struct Icons {
    spinner:      &'static [&'static str],
    folder:       &'static str,
    album:        &'static str,
    cd:           &'static str,
    album_ph:     &'static str,
    nav_back:     &'static str,
    nav_fwd:      &'static str,
    play:         &'static str,
    pause:        &'static str,
    skip_prev:    &'static str,
    skip_next:    &'static str,
    checked:      &'static str,
    unchecked:    &'static str,
    sel_marker:   &'static str,
    trash:        &'static str,
    rescan:       &'static str,
    add:          &'static str,
    cycle_left:   &'static str,
    cycle_right:  &'static str,
    now_playing:  &'static str,
    search:       &'static str,
    queue:        &'static str,
    close:        &'static str,
    shuffle:      &'static str,
    repeat:       &'static str,
    repeat_one:   &'static str,
    lyric_gap:    &'static str,
    // one cell of the instrumental break progress bar
    lyric_bar:    &'static str,
    crumb:        &'static str,
    grid:         &'static str,
    list:         &'static str,
    volume:       &'static str,
}

impl Icons {
    fn from_set(set: crate::library_cache::TuiIconSet) -> Self {
        use crate::library_cache::TuiIconSet;
        match set {
            TuiIconSet::NerdFonts => Icons {
                // ee06-ee0b: nf-cod progress spinner frames
                spinner:     &["\u{EE06}", "\u{EE07}", "\u{EE08}", "\u{EE09}", "\u{EE0A}", "\u{EE0B}"],
                folder:      "\u{E5FF}",    // nf-cod-folder
                album:       "\u{F0025}",   // nf-md-album
                cd:          "\u{F0025}",
                album_ph:    "\u{F0025}",
                nav_back:    "\u{EA9B}",    // nf-cod-arrow_left
                nav_fwd:     "\u{EA9C}",    // nf-cod-arrow_right
                play:        "\u{F040A}",    // nf-cod-play
                pause:       "\u{F03E4}",    // nf-cod-debug_pause
                skip_prev:   "\u{F04AE}",   // nf-md-skip_previous
                skip_next:   "\u{F04AD}",   // nf-md-skip_next
                checked:     "\u{F0856}",   // nf-md-checkbox_marked
                unchecked:   "\u{E640}",    // nf-pl-toggle_off
                sel_marker:  "\u{F44A}",    // nf-cod-triangle_right
                trash:       "\u{F0A79}",   // nf-md-trash_can
                rescan:      "\u{F099B}",   // nf-md-reload
                add:         "\u{F0415}",   // nf-md-plus
                cycle_left:  "\u{F035E}",   // nf-md-chevron_left
                cycle_right: "\u{F035F}",   // nf-md-chevron_right
                now_playing: "\u{F040A}",
                search:      "\u{EA6D}",   // nf-cod-search
                queue:       "\u{F0CB8}",  // nf-md-playlist_music
                close:       "\u{F0156}",  // nf-md-close
                shuffle:     "\u{F049D}",  // nf-md-shuffle_variant
                repeat:      "\u{F0456}",  // nf-md-repeat
                repeat_one:  "\u{F0458}",  // nf-md-repeat_once
                lyric_gap:   "\u{F075A}",  // nf-md-music_note
                lyric_bar:   "\u{2501}",
                crumb:       "\u{203A}",
                grid:        "\u{F0570}",  // nf-md-view_grid
                list:        "\u{F0572}",  // nf-md-view_list
                volume:      "\u{F057E}",  // nf-md-volume_high
            },
            TuiIconSet::Unicode => Icons {
                spinner:     &["\u{25D0}", "\u{25D3}", "\u{25D1}", "\u{25D2}"],
                folder:      "\u{25B6}",
                album:       "\u{266A}",
                cd:          "\u{2299}",
                album_ph:    "\u{2299}",
                nav_back:    "\u{2190}",
                nav_fwd:     "\u{2192}",
                play:        "\u{25B6}",
                pause:       "\u{23F8}",
                skip_prev:   "|\u{25C0}",
                skip_next:   "\u{25B6}|",
                checked:     "[\u{25A0}]",
                unchecked:   "[\u{25A1}]",
                sel_marker:  "\u{25B8}",
                trash:       "\u{1F5D1}",
                rescan:      "\u{21BA}",
                add:         "+",
                cycle_left:  "\u{25C0}",
                cycle_right: "\u{25B6}",
                now_playing: "\u{25B6}",
                search:      "/",
                queue:       "\u{2630}",
                close:       "\u{00D7}",
                shuffle:     "\u{21C4}",
                repeat:      "\u{21BB}",
                repeat_one:  "\u{21BB}1",
                lyric_gap:   "\u{266A}",
                lyric_bar:   "\u{2501}",
                crumb:       "\u{203A}",
                grid:        "\u{25A6}",
                list:        "\u{2261}",
                volume:      "\u{266A}",
            },
            TuiIconSet::PlainText => Icons {
                spinner:     &["|", "/", "-", "\\\\"],
                folder:      ">",
                album:       "*",
                cd:          "~",
                album_ph:    "( )",
                nav_back:    "<",
                nav_fwd:     ">",
                play:        ">",
                pause:       "||",
                skip_prev:   "|<",
                skip_next:   ">|",
                checked:     "[x]",
                unchecked:   "[ ]",
                sel_marker:  ">",
                trash:       "[X]",
                rescan:      "[R]",
                add:         "[+]",
                cycle_left:  "<",
                cycle_right: ">",
                now_playing: ">",
                search:      "/",
                queue:       "Q",
                close:       "x",
                shuffle:     "Sh",
                repeat:      "Rp",
                repeat_one:  "R1",
                lyric_gap:   "~",
                lyric_bar:   "-",
                crumb:       ">",
                grid:        "#",
                list:        "=",
                volume:      "vol",
            },
        }
    }
}

/// album-level fields for a local track list, so a session can be rebuilt
/// without re-reading metadata.
#[derive(Clone, Default)]
struct FileAlbumMeta {
    title:  String,
    artist: String,
    year:   String,
    cover:  String,
}

/// where playback goes back to once the queue drains.
struct TuiQueueResume {
    tracks: Vec<LocalTrack>,
    meta: FileAlbumMeta,
    next_index: i32,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum RepeatMode {
    Off,
    All,
    One,
}

impl RepeatMode {
    fn next(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::One,
            Self::One => Self::Off,
        }
    }
}

/// index in `0..len` from the system clock. shuffle order does not need to be
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

struct TuiPlayerState {
    // drive / cd
    drives:             Vec<DriveInfo>,
    current_drive_idx:  Option<usize>,
    cd_tracks:          Vec<TrackInfo>,
    disc_status:        String,
    is_loading:         bool,
    /// when the current disc read started; used to only surface the
    /// "Reading CD" state after 1s so the empty-drive poll doesn't flash it.
    loading_since:      Option<Instant>,
    disc_load_result:   Arc<Mutex<Option<PendingDiscResult>>>,
    disc_load_thread:   Option<thread::JoinHandle<()>>,
    current_disc_id:    String,
    metadata_loaded:    bool,
    /// disc id we last attempted a musicbrainz lookup for, so a disc that is
    /// not in the database is not re-queried every poll cycle.
    meta_attempt_disc_id: String,
    /// disc position / media count within the release (multi-cd sets). 0 = unknown.
    disc_number:        u32,
    disc_count:         u32,

    // file mode
    is_file_mode:       bool,
    file_tracks:        Vec<LocalTrack>,

    // play order, session-only like the gui
    shuffle:            bool,
    repeat:             RepeatMode,
    /// indices already played in this shuffle pass, so it doesn't repeat until
    /// every track had its turn.
    shuffle_history:    Vec<i32>,

    // queue: songs that play ahead of the rest of the album
    queue:              crate::queue::Queue,
    /// album to return to once the queue drains.
    queue_resume:       Option<TuiQueueResume>,

    // playback
    current_track:      i32,
    total_tracks:       i32,
    is_playing:         bool,
    total_time:         f64,
    playback_start_offset: f64,

    // shared atomics (with playback thread)
    stop_flag:          Arc<AtomicBool>,
    current_position:   Arc<AtomicU64>,
    heard_position:     Arc<AtomicU64>,
    playback_ended:     Arc<AtomicBool>,
    playback_error:     Arc<AtomicBool>,
    volume:             Arc<AtomicU64>,
    seek_signal:        Arc<crate::audio_player::SeekSignal>,
    playback_thread:    Option<thread::JoinHandle<()>>,

    // metadata
    album_title:        String,
    album_artist:       String,
    album_year:         String,
    current_cover_url:  String,
    /// album-level cover for the file session; current_cover_url follows the
    /// playing track, which can carry its own art.
    album_cover_url:    String,

    // track lists (parallel to cd_tracks / file_tracks)
    track_titles:       Vec<String>,
    track_artists:      Vec<String>,
    track_durations:    Vec<String>,

    // lyrics
    lyric_lines:        Vec<String>,
    // original (non-romanized) lyric text, kept so the romanize toggle applies live.
    lyric_lines_src:    crate::lrclib::Lyrics,
    // per displayed line word timings (start secs, end secs, chars); empty when line-synced.
    lyric_words:        Vec<Vec<(f64, f64, usize)>>,
    // false for plain lyrics, which have no current line
    lyric_synced:       bool,
    lyric_instrumental: bool,
    lyric_times:        Vec<f64>,
    lyrics_fetch_done:  bool,
    lyric_result:       Arc<Mutex<Option<Option<crate::lrclib::Lyrics>>>>,
    lyric_fetch_thread: Option<thread::JoinHandle<()>>,
    lyric_fetch_gen:    Arc<AtomicU64>,
}

impl Default for TuiPlayerState {
    fn default() -> Self {
        Self {
            drives:              vec![],
            current_drive_idx:   None,
            cd_tracks:           vec![],
            disc_status:         "No disc inserted".into(),
            is_loading:          false,
            loading_since:       None,
            disc_load_result:    Arc::new(Mutex::new(None)),
            disc_load_thread:    None,
            current_disc_id:     String::new(),
            metadata_loaded:     false,
            meta_attempt_disc_id: String::new(),
            disc_number:         0,
            disc_count:          0,
            is_file_mode:        false,
            file_tracks:         vec![],
            shuffle:             false,
            repeat:              RepeatMode::Off,
            shuffle_history:     vec![],
            queue:               crate::queue::Queue::new(),
            queue_resume:        None,
            current_track:       -1,
            total_tracks:        0,
            is_playing:          false,
            total_time:          0.0,
            playback_start_offset: 0.0,
            stop_flag:           Arc::new(AtomicBool::new(false)),
            current_position:    Arc::new(AtomicU64::new(0)),
            heard_position:      Arc::new(AtomicU64::new(0)),
            playback_ended:      Arc::new(AtomicBool::new(false)),
            playback_error:      Arc::new(AtomicBool::new(false)),
            volume:              Arc::new(AtomicU64::new((1.0_f64).to_bits())),
            seek_signal:         Arc::new(crate::audio_player::SeekSignal::default()),
            playback_thread:     None,
            album_title:         "Unknown Album".into(),
            album_artist:        "Unknown Artist".into(),
            album_year:          String::new(),
            current_cover_url:   String::new(),
            album_cover_url:     String::new(),
            track_titles:        vec![],
            track_artists:       vec![],
            track_durations:     vec![],
            lyric_lines:         vec![],
            lyric_lines_src:     Default::default(),
            lyric_words:         vec![],
            lyric_synced:        false,
            lyric_instrumental:  false,
            lyric_times:         vec![],
            lyrics_fetch_done:   false,
            lyric_result:        Arc::new(Mutex::new(None)),
            lyric_fetch_thread:  None,
            lyric_fetch_gen:     Arc::new(AtomicU64::new(0)),
        }
    }
}

impl TuiPlayerState {
    fn current_time(&self) -> f64 {
        f64::from_bits(self.heard_position.load(Ordering::Relaxed))
    }

    fn volume_f64(&self) -> f64 {
        f64::from_bits(self.volume.load(Ordering::Relaxed))
    }

    fn stop_playback(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Some(h) = self.playback_thread.take() {
            let _ = h.join();
        }
        self.is_playing = false;
    }

    fn scan_drives(&mut self) {
        let drives = crate::cd_reader::scan_drives();
        self.drives = drives;
    }

    /// "CD" tag for the library node - includes the disc position within the
    /// release for multi-cd sets, e.g. "CD 2/3" (from musicbrainz).
    fn cd_tag(&self) -> String {
        if self.disc_count > 1 && self.disc_number > 0 {
            format!("CD {}/{}", self.disc_number, self.disc_count)
        } else {
            "CD".to_string()
        }
    }

    /// library-root cd node state, or None when no optical drive is present.
    fn cd_node_info(&self) -> Option<CdNodeInfo> {
        if self.drives.is_empty() { return None; }
        if self.show_loading() && !self.is_file_mode {
            Some(CdNodeInfo {
                title:   "Reading CD...".into(),
                sub:     "CD".into(),
                loading: true,
            })
        } else if self.total_tracks > 0 && !self.is_file_mode {
            let tag = self.cd_tag();
            let sub = if self.album_artist.is_empty() || self.album_artist == "Unknown Artist" {
                tag
            } else {
                format!("{} · {}", self.album_artist, tag)
            };
            Some(CdNodeInfo { title: self.album_title.clone(), sub, loading: false })
        } else {
            Some(CdNodeInfo {
                title:   "Audio CD".into(),
                sub:     "No disc".into(),
                loading: false,
            })
        }
    }

    /// physically eject the selected drive's tray. never touches file-mode
    /// playback: local files keep playing while the tray opens.
    fn eject(&mut self) {
        let path = self.current_drive_idx
            .and_then(|i| self.drives.get(i))
            .or_else(|| self.drives.first())
            .map(|d| d.path.clone());
        let Some(path) = path else { return };
        if !self.is_file_mode {
            self.stop_playback();
        }
        crate::cd_reader::eject_drive_async(&path);
    }

    fn load_disc(&mut self) {
        if self.is_loading { return; }
        let drive_path = match self.current_drive_idx {
            Some(i) if i < self.drives.len() => self.drives[i].path.clone(),
            _ => return,
        };
        if let Some(old) = self.disc_load_thread.take() {
            let _ = old.join();
        }
        *self.disc_load_result.lock().unwrap() = None;
        let result_slot = self.disc_load_result.clone();
        let meta_loaded = self.metadata_loaded;
        let attempted_id = self.meta_attempt_disc_id.clone();
        let current_id = self.current_disc_id.clone();
        let handle = thread::spawn(move || {
            // look up once per disc: a newly inserted disc is always queried;
            // the same disc is re-queried only if neither loaded nor
            // previously attempted.
            let result = crate::cd_reader::probe_disc(&drive_path, |disc_id| {
                disc_id != current_id || (!meta_loaded && attempted_id != disc_id)
            });
            *result_slot.lock().unwrap() = Some(result);
        });
        self.disc_load_thread = Some(handle);
        self.is_loading = true;
        if self.loading_since.is_none() {
            self.loading_since = Some(Instant::now());
        }
    }

    /// whether the ui should show the "Reading CD" state: only after the disc
    /// read has been running for a second straight, so the quick periodic
    /// empty-drive checks don't flash it.
    fn show_loading(&self) -> bool {
        self.is_loading
            && self.loading_since.is_some_and(|t| t.elapsed() >= Duration::from_secs(1))
    }

    fn poll_load(&mut self) {
        let result = self.disc_load_result.lock().unwrap().take();
        let Some(result) = result else { return };
        if let Some(t) = self.disc_load_thread.take() { let _ = t.join(); }
        self.is_loading = false;
        self.loading_since = None;

        match result {
            PendingDiscResult::Busy => {}
            PendingDiscResult::Loaded { tracks, durations, metadata, disc_id, .. } => {
                // periodic same-disc re-poll with no fresh metadata: keep the
                // existing titles/artists instead of resetting to "Track NN".
                let same_disc = metadata.is_none()
                    && self.current_disc_id == disc_id
                    && self.total_tracks == tracks.len() as i32
                    && !self.track_titles.is_empty();
                if same_disc {
                    self.cd_tracks       = tracks;
                    self.track_durations = durations;
                    self.disc_status     = String::new();
                    return;
                }

                self.current_disc_id = disc_id.clone();
                self.meta_attempt_disc_id = disc_id;
                self.total_tracks    = tracks.len() as i32;
                self.track_durations = durations;
                self.track_titles    = (0..tracks.len()).map(|i| {
                    metadata.as_ref()
                        .and_then(|m| m.track_titles.get(i))
                        .filter(|s| !s.is_empty())
                        .map(|s| s.clone())
                        .unwrap_or_else(|| format!("Track {}", i + 1))
                }).collect();
                self.track_artists   = (0..tracks.len()).map(|i| {
                    metadata.as_ref()
                        .and_then(|m| m.track_artists.get(i))
                        .cloned()
                        .unwrap_or_default()
                }).collect();
                if let Some(ref meta) = metadata {
                    self.album_title   = meta.title.clone();
                    self.album_artist  = meta.artist.clone();
                    self.album_year    = meta.year.clone();
                    self.current_cover_url = meta.cover_art_url.clone().unwrap_or_default();
                    self.metadata_loaded = true;
                    self.disc_number = meta.disc_number;
                    self.disc_count  = meta.disc_count;
                } else {
                    self.album_title  = "Unknown Album".into();
                    self.album_artist = "Unknown Artist".into();
                    self.album_year   = String::new();
                    self.metadata_loaded = false;
                    self.disc_number = 0;
                    self.disc_count  = 0;
                }
                self.cd_tracks  = tracks;
                self.disc_status = String::new();
                if self.current_track < 0 { self.current_track = -1; }
            }
            PendingDiscResult::NotAudio  { status } |
            PendingDiscResult::Empty     { status } |
            PendingDiscResult::Unavailable { status } => {
                self.disc_status  = status;
                self.total_tracks = 0;
                self.cd_tracks.clear();
                self.track_titles.clear();
                self.track_artists.clear();
                self.track_durations.clear();
                self.current_track = -1;
                self.total_time    = 0.0;
                self.album_title   = "Unknown Album".into();
                self.album_artist  = "Unknown Artist".into();
                self.album_year    = String::new();
                self.metadata_loaded = false;
                self.meta_attempt_disc_id.clear();
                self.current_disc_id.clear();
                self.disc_number = 0;
                self.disc_count  = 0;
                self.current_cover_url.clear();
                self.lyric_lines.clear();
                self.lyric_lines_src = Default::default();
                self.lyric_words.clear();
                self.lyric_instrumental = false;
                self.lyric_times.clear();
                self.lyrics_fetch_done = false;
            }
        }
    }

    fn load_file_tracks(&mut self, paths: Vec<String>) {
        let mut tracks = crate::file_player::collect_files_from_paths(&paths);
        if tracks.is_empty() { return; }
        let cover = crate::file_player::album_cover(&mut tracks).unwrap_or_default();

        let first_album = tracks[0].album.clone();
        let all_same = tracks.iter().all(|t| t.album == first_album);
        let title = if all_same && !first_album.is_empty() {
            first_album
        } else {
            paths.first()
                .and_then(|p| std::path::Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("Files")
                .to_string()
        };
        let first_aa = tracks[0].album_artist.clone();
        let first_year = tracks[0].year.clone();
        let meta = FileAlbumMeta {
            title,
            artist: if tracks.iter().all(|t| t.album_artist == first_aa) && !first_aa.is_empty() {
                first_aa
            } else { String::new() },
            year: if tracks.iter().all(|t| t.year == first_year) { first_year } else { String::new() },
            cover,
        };

        // picking an album by hand supersedes the queue's return point.
        self.queue_resume = None;
        self.apply_file_tracks(tracks, meta);
    }

    /// publish an already-collected set of local tracks as the active session.
    /// split out of `load_file_tracks` so the queue can swap sessions without
    /// re-reading metadata from disk.
    fn apply_file_tracks(&mut self, tracks: Vec<LocalTrack>, meta: FileAlbumMeta) {
        if tracks.is_empty() { return; }
        self.stop_playback();
        self.current_disc_id.clear();

        let n = tracks.len();
        self.track_durations = tracks.iter().map(|t| t.display_duration()).collect();
        self.track_titles    = tracks.iter().map(|t| t.title.clone()).collect();
        self.track_artists   = tracks.iter().map(|t| t.artist.clone()).collect();

        self.album_title  = meta.title;
        self.album_artist = meta.artist;
        self.album_year   = meta.year;
        self.album_cover_url   = meta.cover.clone();
        self.current_cover_url = meta.cover;
        self.file_tracks   = tracks;
        self.is_file_mode  = true;
        self.total_tracks  = n as i32;
        self.current_track = -1;
        self.total_time    = 0.0;
        self.is_playing    = false;
        self.metadata_loaded = true;
        self.lyric_lines.clear();
        self.lyric_lines_src = Default::default();
        self.lyric_words.clear();
        self.lyric_instrumental = false;
        self.lyric_times.clear();
        self.lyrics_fetch_done = false;
        self.heard_position.store(0u64, Ordering::Relaxed);
        self.current_position.store(0u64, Ordering::Relaxed);
    }

    /// track selection from the ui. choosing a track by hand supersedes the
    /// album the queue was going to return to.
    fn load_track(&mut self, idx: i32) {
        self.queue_resume = None;
        self.load_track_at(idx);
    }

    fn load_track_at(&mut self, idx: i32) {
        if idx < 0 { return; }
        let duration = if self.is_file_mode {
            if (idx as usize) >= self.file_tracks.len() { return; }
            self.file_tracks[idx as usize].duration_secs
        } else {
            if (idx as usize) >= self.cd_tracks.len() { return; }
            self.cd_tracks[idx as usize].duration_seconds
        };
        self.stop_playback();
        self.playback_start_offset = 0.0;
        self.current_position.store(0u64, Ordering::Relaxed);
        self.heard_position.store(0u64, Ordering::Relaxed);
        self.current_track = idx;
        self.total_time    = duration;
        // tracks with their own art show it while playing; the rest fall back
        // to the album cover.
        if self.is_file_mode {
            self.current_cover_url = crate::file_player::track_cover(&mut self.file_tracks[idx as usize])
                .unwrap_or_else(|| self.album_cover_url.clone());
        }
    }

    fn play_pause(&mut self) {
        if self.is_playing {
            let pos = self.current_time();
            self.playback_start_offset = pos;
            self.stop_playback();
        } else {
            self.start_playback();
        }
    }

    fn start_playback(&mut self) {
        self.stop_playback();
        let idx = self.current_track;
        if idx < 0 { return; }

        self.stop_flag.store(false, Ordering::Relaxed);
        self.playback_ended.store(false, Ordering::Relaxed);
        self.playback_error.store(false, Ordering::Relaxed);

        if self.is_file_mode {
            let file_path  = match self.file_tracks.get(idx as usize) {
                Some(t) => t.path.clone(),
                None    => return,
            };
            let offset      = self.playback_start_offset;
            self.heard_position.store(offset.to_bits(), Ordering::Relaxed);
            let stop_flag   = self.stop_flag.clone();
            let vol         = self.volume.clone();
            let heard       = self.heard_position.clone();
            let pos         = self.current_position.clone();
            let ended       = self.playback_ended.clone();
            let seek        = self.seek_signal.clone();
            let seek_gen    = seek.generation();
            // the tui has no visualizer; hand the player a throwaway tap.
            let vis_tap = std::sync::Arc::new(std::sync::Mutex::new(crate::audio_player::VisTap::new()));
            let handle = thread::spawn(move || {
                // the tui has no crossfade: no fade-in, and a handover flag that is never set.
                crate::file_player::play_local_file(file_path, offset, stop_flag, vol, heard, pos, ended,
                                                    vis_tap, 0.0, std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0)),
                                                    seek, seek_gen);
            });
            self.playback_thread = Some(handle);
            self.is_playing = true;
            return;
        }

        let drive_path = match self.current_drive_idx {
            Some(i) if i < self.drives.len() => self.drives[i].path.clone(),
            _ => return,
        };
        if self.cd_tracks.is_empty() { return; }
        let track_number = match self.cd_tracks.get(idx as usize) {
            Some(t) => t.track_number,
            None    => return,
        };
        let offset   = self.playback_start_offset;
        self.heard_position.store(offset.to_bits(), Ordering::Relaxed);
        let stop_flag   = self.stop_flag.clone();
        let vol         = self.volume.clone();
        let heard       = self.heard_position.clone();
        let pos         = self.current_position.clone();
        let ended       = self.playback_ended.clone();
        let error       = self.playback_error.clone();

        let handle = thread::spawn(move || {
            use crate::audio_player::AudioController;
            let audio = match AudioController::new() {
                Ok(c)  => c,
                Err(e) => { eprintln!("[tui-play] audio init failed: {}", e); ended.store(true, Ordering::Relaxed); return; }
            };
            // open drive with retries
            let reader = {
                let mut r = None;
                for attempt in 0..10u32 {
                    if stop_flag.load(Ordering::Relaxed) { return; }
                    match crate::cd_reader::open_drive(&drive_path) {
                        Ok(rd) => { r = Some(rd); break; }
                        Err(_) => { thread::sleep(Duration::from_millis(200 * (1 << attempt.min(4)))); }
                    }
                }
                match r { Some(rd) => rd, None => { error.store(true, Ordering::Relaxed); ended.store(true, Ordering::Relaxed); return; } }
            };
            let toc = match crate::cd_reader::read_toc(&reader) {
                Ok(t)  => t,
                Err(_) => { error.store(true, Ordering::Relaxed); ended.store(true, Ordering::Relaxed); return; }
            };
            use cd_da_reader::{RetryConfig, TrackStreamConfig};
            let cfg = TrackStreamConfig {
                sectors_per_chunk: 6,
                retry: RetryConfig { max_attempts: 5, initial_backoff_ms: 30, max_backoff_ms: 500, reduce_chunk_on_retry: true, min_sectors_per_read: 1 },
            };
            let mut stream = match reader.open_track_stream(&toc, track_number, cfg) {
                Ok(s)  => s,
                Err(e) => { eprintln!("[tui-play] open_track_stream: {}", e); error.store(true, Ordering::Relaxed); ended.store(true, Ordering::Relaxed); return; }
            };
            if offset > 0.0 { let _ = stream.seek_to_seconds(offset as f32); }

            let mut cur_vol = f64::from_bits(vol.load(Ordering::Relaxed)) as f32;
            let mut pending: std::collections::VecDeque<f64> = Default::default();

            loop {
                if stop_flag.load(Ordering::Relaxed) { audio.stop(); break; }
                match stream.next_chunk() {
                    Ok(Some(chunk)) => {
                        let chunk_secs  = chunk.len() as f64 / (4.0 * 44100.0);
                        let chunk_end   = stream.current_seconds() as f64;
                        let chunk_start = chunk_end - chunk_secs;
                        let tgt_vol = f64::from_bits(vol.load(Ordering::Relaxed)) as f32;
                        let raw = crate::audio_player::bytes_to_f32_samples(&chunk);
                        let n = raw.len() as f32;
                        let samples: Vec<f32> = raw.iter().enumerate().map(|(i, &s)| {
                            let t = i as f32 / n;
                            (s * (cur_vol + (tgt_vol - cur_vol) * t)).clamp(-1.0, 1.0)
                        }).collect();
                        cur_vol = tgt_vol;
                        audio.append_samples(samples, 44100, 2);
                        pending.push_back(chunk_start);
                        while audio.queue_len() > 1 && !stop_flag.load(Ordering::Relaxed) {
                            thread::sleep(Duration::from_millis(50));
                            let q = audio.queue_len();
                            while pending.len() > q.max(1) { pending.pop_front(); }
                            if let Some(&hp) = pending.front() { heard.store(hp.to_bits(), Ordering::Relaxed); }
                            thread::sleep(Duration::from_millis(20));
                        }
                        let q = audio.queue_len();
                        while pending.len() > q.max(1) { pending.pop_front(); }
                        let hp = pending.front().copied().unwrap_or(chunk_start);
                        heard.store(hp.to_bits(), Ordering::Relaxed);
                        pos.store(chunk_end.to_bits(), Ordering::Relaxed);
                    }
                    Ok(None) => {
                        while !audio.is_empty() {
                            if stop_flag.load(Ordering::Relaxed) { audio.stop(); break; }
                            let q = audio.queue_len();
                            while pending.len() > q.max(1) { pending.pop_front(); }
                            if let Some(&hp) = pending.front() { heard.store(hp.to_bits(), Ordering::Relaxed); }
                            thread::sleep(Duration::from_millis(50));
                        }
                        break;
                    }
                    Err(e) => { eprintln!("[tui-play] read error: {}", e); error.store(true, Ordering::Relaxed); break; }
                }
            }
            if !stop_flag.load(Ordering::Relaxed) { ended.store(true, Ordering::Relaxed); }
        });
        self.playback_thread = Some(handle);
        self.is_playing = true;
    }

    fn seek(&mut self, secs: f64) {
        let was_playing = self.is_playing;
        self.playback_start_offset = secs;
        self.heard_position.store(secs.to_bits(), Ordering::Relaxed);
        self.current_position.store(secs.to_bits(), Ordering::Relaxed);
        // a running file stream seeks in place instead of reopening the device
        let alive = self.playback_thread.as_ref().map_or(false, |h| !h.is_finished())
            && !self.playback_ended.load(Ordering::Relaxed);
        if was_playing && self.is_file_mode && alive {
            self.seek_signal.request(secs);
        } else if was_playing {
            self.start_playback();
        }
    }

    // queue

    fn enqueue_paths(&mut self, paths: &[String]) -> usize {
        self.queue.extend_from_paths(paths)
    }

    /// remember the album to come back to, unless the queue already took over.
    fn capture_queue_resume(&mut self) {
        if self.queue_resume.is_some() || !self.is_file_mode || self.file_tracks.is_empty() {
            return;
        }
        self.queue_resume = Some(TuiQueueResume {
            tracks: self.file_tracks.clone(),
            meta: FileAlbumMeta {
                title:  self.album_title.clone(),
                artist: self.album_artist.clone(),
                year:   self.album_year.clone(),
                cover:  self.album_cover_url.clone(),
            },
            next_index: self.current_track + 1,
        });
    }

    /// swap the session over to a queued song, loaded in its album (not started)
    /// so the track list shows the real album with the song selected.
    fn play_queue_entry(&mut self, entry: crate::queue::QueueEntry) {
        let (mut tracks, index) = crate::file_player::album_context_for(&entry.path);
        let cover = crate::file_player::album_cover(&mut tracks).unwrap_or_default();
        let first = &tracks[0];
        let meta = if tracks.len() == 1 {
            FileAlbumMeta {
                title:  first.title.clone(),
                artist: first.artist.clone(),
                year:   first.year.clone(),
                cover:  cover.clone(),
            }
        } else {
            FileAlbumMeta {
                title:  first.album.clone(),
                artist: first.album_artist.clone(),
                year:   first.year.clone(),
                cover:  cover.clone(),
            }
        };
        self.apply_file_tracks(tracks, meta);
        self.load_track_at(index);
    }

    /// load whatever should play next: queued songs first, then the rest of the
    /// album, then the album the queue interrupted. leaves playback stopped.
    fn advance(&mut self) -> bool {
        if let Some(entry) = self.queue.pop_front() {
            self.capture_queue_resume();
            self.play_queue_entry(entry);
            return true;
        }
        if let Some(r) = self.queue_resume.take() {
            let count = r.tracks.len() as i32;
            let next  = r.next_index;
            self.apply_file_tracks(r.tracks, r.meta);
            if next < count {
                self.load_track_at(next);
                return true;
            }
            return false;
        }
        match self.pick_next_index() {
            Some(i) => {
                self.load_track_at(i);
                true
            }
            None => false,
        }
    }

    /// the next index within the current track list, honouring repeat and
    /// shuffle (same rules as the gui). `None` means playback has run out.
    fn pick_next_index(&mut self) -> Option<i32> {
        let current = self.current_track;
        let total = self.total_tracks;
        if total <= 0 { return None; }

        // repeat-one wins over everything: the same track again.
        if self.repeat == RepeatMode::One { return Some(current.max(0)); }

        if self.shuffle && total > 1 {
            self.shuffle_history.retain(|i| *i >= 0 && *i < total);
            if current >= 0 && !self.shuffle_history.contains(&current) {
                self.shuffle_history.push(current);
            }
            // a full pass is done; either stop or start a fresh one.
            if self.shuffle_history.len() as i32 >= total {
                self.shuffle_history.clear();
                if self.repeat != RepeatMode::All { return None; }
                self.shuffle_history.push(current);
            }
            let pool: Vec<i32> = (0..total).filter(|i| !self.shuffle_history.contains(i)).collect();
            return pool.get(pseudo_random(pool.len())).copied().or(Some(current.max(0)));
        }

        if current + 1 < total { return Some(current + 1); }
        if self.repeat == RepeatMode::All { return Some(0); }
        None
    }

    fn toggle_shuffle(&mut self) {
        self.shuffle = !self.shuffle;
        self.shuffle_history.clear();
    }

    fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.next();
    }

    /// jump straight to a queue entry, dropping the ones before it.
    fn queue_play_at(&mut self, index: usize) {
        for _ in 0..index { self.queue.pop_front(); }
        let Some(entry) = self.queue.pop_front() else { return };
        self.capture_queue_resume();
        self.play_queue_entry(entry);
        self.start_playback();
    }

    fn next_track(&mut self) {
        let was_playing = self.is_playing;
        // an explicit skip moves on even in repeat-one.
        let repeat = self.repeat;
        if repeat == RepeatMode::One { self.repeat = RepeatMode::All; }
        let moved = self.advance();
        self.repeat = repeat;
        if moved && was_playing {
            self.start_playback();
        }
    }

    fn prev_track(&mut self) {
        let pos = self.current_time();
        if pos > 4.0 {
            self.seek(0.0);
        } else if self.current_track > 0 {
            let was_playing = self.is_playing;
            self.load_track(self.current_track - 1);
            if was_playing { self.start_playback(); }
        }
    }

    fn set_volume(&self, v: f64) {
        self.volume.store(v.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
    }

    fn update_position(&mut self) {
        if !self.is_playing { return; }
        if self.playback_ended.swap(false, Ordering::Relaxed) {
            if let Some(h) = self.playback_thread.take() { let _ = h.join(); }
            let disc_err = self.playback_error.swap(false, Ordering::Relaxed);
            self.playback_start_offset = 0.0;
            self.heard_position.store(0u64, Ordering::Relaxed);
            self.current_position.store(0u64, Ordering::Relaxed);
            self.is_playing = false;

            if disc_err {
                self.cd_tracks.clear();
                self.total_tracks = 0;
                self.current_track = -1;
                self.total_time = 0.0;
                self.disc_status = "No disc inserted".into();
                self.track_titles.clear();
                self.track_artists.clear();
                self.track_durations.clear();
                self.metadata_loaded = false;
                self.lyric_lines.clear();
                self.lyric_lines_src = Default::default();
                self.lyric_words.clear();
                self.lyric_instrumental = false;
                self.lyric_times.clear();
                self.lyrics_fetch_done = false;
                return;
            }

            // queue entries come first; `advance` also handles returning to the
            // album once the queue drains.
            if self.advance() {
                self.start_playback();
                self.fetch_lyrics_for_current();
            }
        }
    }

    fn fetch_lyrics_for_current(&mut self) {
        let idx    = self.current_track as usize;
        let title  = self.track_titles.get(idx).cloned().unwrap_or_default();
        let raw_ar = self.track_artists.get(idx).cloned().unwrap_or_default();
        let artist = if raw_ar.is_empty() { self.album_artist.clone() } else { raw_ar };
        let album  = self.album_title.clone();
        let dur    = self.total_time;
        let disc_id   = self.current_disc_id.clone();
        let track_idx = self.current_track;
        let file_path: String = if self.is_file_mode && track_idx >= 0 {
            self.file_tracks.get(track_idx as usize)
                .map(|t| t.path.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else { String::new() };
        let lrc_limit_disabled = crate::library_cache::load_settings().lrc_limit_disabled;

        if title.is_empty() { return; }

        self.lyric_lines.clear();
        self.lyric_lines_src = Default::default();
        self.lyric_words.clear();
        self.lyric_instrumental = false;
        self.lyric_times.clear();
        self.lyrics_fetch_done = false;

        let gen = self.lyric_fetch_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let gen_arc     = self.lyric_fetch_gen.clone();
        let result_slot = { *self.lyric_result.lock().unwrap() = None; self.lyric_result.clone() };
        if let Some(old) = self.lyric_fetch_thread.take() { drop(old); }

        let handle = thread::spawn(move || {
            use crate::lyric_cache::{LyricContentCache, cd_key, file_key};

            // a sidecar .lrc next to the audio file always wins over web/cached lyrics.
            if let Some(lyrics) = crate::lrclib::load_local_lrc(&file_path) {
                if gen_arc.load(Ordering::SeqCst) == gen {
                    *result_slot.lock().unwrap() = Some(Some(lyrics));
                }
                return;
            }

            let cache_key = if !disc_id.is_empty() && track_idx >= 0 {
                cd_key(&disc_id, track_idx)
            } else {
                file_key(&file_path, &title, &artist)
            };

            let mut content_cache = LyricContentCache::load();

            if content_cache.has_no_lyrics(&cache_key) {
                eprintln!("[lrclib] no-lyrics cache hit for key {}", cache_key);
                if gen_arc.load(Ordering::SeqCst) == gen {
                    *result_slot.lock().unwrap() = Some(None);
                }
                return;
            }

            if let Some(raw_lrc) = content_cache.get_lrc(&cache_key) {
                eprintln!("[lrclib] lrc content cache hit for key {}", cache_key);
                content_cache.save();
                let lyrics = crate::lrclib::parse_lyrics(&raw_lrc);
                if gen_arc.load(Ordering::SeqCst) == gen {
                    *result_slot.lock().unwrap() = Some(if lyrics.is_empty() { None } else { Some(lyrics) });
                }
                return;
            }

            let result = match crate::lrclib::fetch_lyrics(&title, &artist, &album, dur) {
                Some((_id, raw_lrc, lyrics)) => {
                    content_cache.insert_lrc(&cache_key, &raw_lrc, lrc_limit_disabled);
                    content_cache.save();
                    Some(lyrics)
                }
                None => {
                    content_cache.insert_no_lyrics(&cache_key, lrc_limit_disabled);
                    content_cache.save();
                    None
                }
            };

            if gen_arc.load(Ordering::SeqCst) == gen {
                *result_slot.lock().unwrap() = Some(result);
            }
        });
        self.lyric_fetch_thread = Some(handle);
    }

    fn poll_lyrics(&mut self) {
        let result = self.lyric_result.lock().unwrap().take();
        if let Some(maybe_lines) = result {
            if let Some(t) = self.lyric_fetch_thread.take() { drop(t); }
            self.lyrics_fetch_done = true;
            if let Some(lyrics) = maybe_lines {
                self.lyric_times = lyrics.lines.iter().map(|l| l.time_secs).collect();
                self.lyric_synced = lyrics.synced;
                self.lyric_instrumental = lyrics.instrumental;
                self.lyric_lines_src = lyrics;
                let romanize = crate::library_cache::load_settings().romanize_lyrics;
                self.apply_romanization(romanize);
            }
        }
    }

    /// re-derive the displayed lyric lines from the stored originals, romanizing
    /// japanese lines when `romanize` is set. cheap enough to call on toggle.
    fn apply_romanization(&mut self, romanize: bool) {
        let mut lyrics = self.lyric_lines_src.clone();
        crate::romaji::romanize_lyrics(&mut lyrics, romanize);
        self.lyric_words = lyrics.lines.iter()
            .map(|l| l.words.iter().map(|w| (w.time_secs, w.end_secs, w.text.chars().count())).collect())
            .collect();
        self.lyric_lines = lyrics.lines.into_iter().map(|l| l.text).collect();
    }

    /// how many characters of lyric line `li` are sung, fractional inside the
    /// word being sung, or None when the line has no word timings.
    fn sung_chars(&self, li: usize) -> Option<f64> {
        let words = self.lyric_words.get(li).filter(|w| !w.is_empty())?;
        let t = self.current_time();
        let mut sung = 0.0;
        for &(start, end, chars) in words {
            if t >= end {
                sung += chars as f64;
            } else {
                if t > start { sung += chars as f64 * (t - start) / (end - start); }
                break;
            }
        }
        Some(sung)
    }

    /// how far into the instrumental break at line `li` playback is (0..1), or
    /// None when that line is not a break.
    fn break_progress(&self, li: usize) -> Option<f64> {
        if !self.lyric_synced { return None; }
        let len = crate::lrclib::break_len(&self.lyric_times, &self.lyric_lines, li)?;
        Some(((self.current_time() - self.lyric_times[li]) / len).clamp(0.0, 1.0))
    }

    fn active_lyric_idx(&self) -> i32 {
        if !self.lyric_synced { return -1; }
        let t = self.current_time();
        let mut best = -1i32;
        for (i, &ts) in self.lyric_times.iter().enumerate() {
            if ts <= t + 0.05 { best = i as i32; } else { break; }
        }
        best
    }
}

// library state

struct TuiLibraryState {
    settings:      LibrarySettings,
    scan_result:   Option<LibraryScanResult>,
    stop_scan:     Arc<AtomicBool>,
    scan_thread:   Option<thread::JoinHandle<()>>,
    progress_rx:   Option<std::sync::mpsc::Receiver<crate::library::ScanProgress>>,
    done_result:   Arc<Mutex<Option<LibraryScanResult>>>,
    is_scanning:   bool,
    scan_message:  String,
    // navigation
    nav_stack:     Vec<PathBuf>,
    nav_idx:       usize,
    nodes:         Vec<TuiLibraryNode>,
    // node list rebuilds are expensive (clones every album), so they only run
    // when something changed: this flag, or the cd node differing from last tick.
    nodes_dirty:   bool,
    last_cd_info:  Option<CdNodeInfo>,
    // album browse (previewing an album before loading it). identified by the
    // album's stable id, so multiple albums/singles in one directory differ.
    browse_album_path: Option<String>,
    browse_tracks:     Vec<BrowseTrack>,
    /// bumped whenever `scan_result` changes, so derived data (the search
    /// index) knows to rebuild.
    scan_gen:      u64,
    search_index:  search::SearchIndex,
}

#[derive(Clone)]
pub struct BrowseTrack {
    pub title:    String,
    pub artist:   String,
    pub duration: String,
    pub path:     PathBuf,
}

impl Default for TuiLibraryState {
    fn default() -> Self {
        let settings = crate::library_cache::load_settings();
        Self {
            settings,
            scan_result:  None,
            stop_scan:    Arc::new(AtomicBool::new(false)),
            scan_thread:  None,
            progress_rx:  None,
            done_result:  Arc::new(Mutex::new(None)),
            is_scanning:  false,
            scan_message: String::new(),
            nav_stack:    vec![],
            nav_idx:      0,
            nodes:        vec![],
            nodes_dirty:  true,
            last_cd_info: None,
            browse_album_path: None,
            browse_tracks:     vec![],
            scan_gen:      0,
            search_index:  search::SearchIndex::default(),
        }
    }
}

impl TuiLibraryState {
    fn start_scan(&mut self) {
        if self.settings.search_paths.is_empty() { return; }
        self.stop_scan.store(true, Ordering::Relaxed);
        if let Some(t) = self.scan_thread.take() { let _ = t.join(); }
        let (tx, rx) = std::sync::mpsc::sync_channel(64);
        let done     = Arc::new(Mutex::new(None));
        let stop     = Arc::new(AtomicBool::new(false));
        let settings = self.settings.clone();
        let prev     = self.scan_result.clone();
        let stop2 = stop.clone(); let done2 = done.clone();
        let handle = thread::spawn(move || {
            // reuse the previous result so unchanged directories skip re-reading.
            let r = crate::library::scan(&settings, stop2, tx, prev.as_ref());
            *done2.lock().unwrap() = Some(r);
        });
        self.stop_scan = stop; self.scan_thread = Some(handle);
        self.progress_rx = Some(rx); self.done_result = done;
        self.is_scanning = true; self.scan_message = "Scanning…".into();
    }

    fn poll_scan(&mut self) {
        let mut has_new = false;
        if let Some(ref rx) = self.progress_rx {
            while let Ok(p) = rx.try_recv() {
                if p.files_found > 0 || p.dirs_visited > 0 {
                    self.scan_message = format!("Scanning\u{2026} {} files, {} folders", p.files_found, p.dirs_visited);
                }
                if !p.new_albums.is_empty() {
                    let result = self.scan_result.get_or_insert_with(|| crate::library::LibraryScanResult { albums: vec![], dirs: vec![], dir_mtimes: Default::default() });
                    // upsert by album id so re-streamed unchanged dirs don't duplicate.
                    let incoming: std::collections::HashSet<String> =
                        p.new_albums.iter().map(|a| a.id.clone()).collect();
                    result.albums.retain(|a| !incoming.contains(&a.id));
                    result.albums.extend(p.new_albums);
                    has_new = true;
                }
            }
        }
        if has_new {
            self.nodes_dirty = true;
            self.scan_gen += 1;
        }
        let done = { let mut g = self.done_result.lock().unwrap(); g.take() };
        if let Some(result) = done {
            crate::library_cache::save_cache(&result);
            self.scan_result  = Some(result);
            self.progress_rx  = None;
            self.is_scanning  = false;
            self.scan_message = String::new();
            self.nodes_dirty = true;
            self.scan_gen += 1;
        }
    }

    fn init(&mut self) {
        if let Some(result) = crate::library_cache::load_cache() {
            if crate::library_cache::is_legacy_cache(&result) {
                // old cache (pre-id): discard and full-rescan (prev = None) so
                // albums get ids, singles split, and covers re-extract.
                self.start_scan();
            } else {
                let rescan = crate::library_cache::needs_rescan(&self.settings, &result);
                self.scan_result = Some(result);
                self.nodes_dirty = true;
                self.scan_gen += 1;
                if rescan { self.start_scan(); }
            }
        } else if !self.settings.search_paths.is_empty() {
            self.start_scan();
        }
    }

    fn current_dir(&self) -> Option<&PathBuf> {
        if self.nav_stack.is_empty() { return None; }
        self.nav_stack.get(self.nav_idx)
    }

    fn refresh_nodes(&mut self, cd_info: Option<CdNodeInfo>) {
        let dir = self.current_dir().cloned();

        let mut nodes: Vec<TuiLibraryNode> = Vec::new();

        if let Some(info) = cd_info {
            if dir.is_none() { nodes.push(TuiLibraryNode::Cd { info }); }
        }

        // even with no scanned library, the cd node (if any) should show.
        let scan = match &self.scan_result { Some(s) => s, None => { self.nodes = nodes; return; } };

        match dir {
            None => {
                if self.settings.merge_all_folders {
                    for album in &scan.albums {
                        nodes.push(TuiLibraryNode::Album { album: album.clone() });
                    }
                } else {
                    let mut seen = std::collections::HashSet::new();
                    for root in &self.settings.search_paths {
                        for dir in &scan.dirs {
                            if let Some(parent) = dir.parent() {
                                if parent == root.as_path() && seen.insert(dir.clone()) {
                                    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                                    nodes.push(TuiLibraryNode::Folder { path: dir.clone(), name });
                                }
                            }
                        }
                        // albums in immediate sub-folders of root, plus loose
                        // tracks/singles sitting directly in the root folder.
                        for album in scan.albums.iter().filter(|a| a.dir.parent() == Some(root.as_path()) || a.dir == *root) {
                            if !nodes.iter().any(|n| matches!(n, TuiLibraryNode::Album { album: a } if a.id == album.id)) {
                                nodes.push(TuiLibraryNode::Album { album: album.clone() });
                            }
                        }
                    }
                    if nodes.is_empty() {
                        for album in &scan.albums {
                            nodes.push(TuiLibraryNode::Album { album: album.clone() });
                        }
                    }
                }
            }
            Some(ref current) => {
                for child_dir in scan.dirs.iter().filter(|d| d.parent() == Some(current.as_path())) {
                    let name = child_dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
                    if !self.settings.ignored_folders.contains(child_dir) {
                        nodes.push(TuiLibraryNode::Folder { path: child_dir.clone(), name });
                    }
                }
                for album in scan.albums.iter().filter(|a| a.dir.parent() == Some(current.as_path()) || a.dir == *current) {
                    nodes.push(TuiLibraryNode::Album { album: album.clone() });
                }
                if nodes.is_empty() {
                    for album in scan.albums.iter().filter(|a| a.dir == *current) {
                        nodes.push(TuiLibraryNode::Album { album: album.clone() });
                    }
                }
            }
        }

        // display order: cd tile first, then folders (a->z), then albums in the
        // configured sort mode. sort_by is stable, so ties keep discovery order.
        let mode = self.settings.album_sort.clone();
        let rank = |n: &TuiLibraryNode| match n {
            TuiLibraryNode::Cd { .. }     => 0u8,
            TuiLibraryNode::Folder { .. } => 1,
            TuiLibraryNode::Album { .. }  => 2,
        };
        nodes.sort_by(|a, b| rank(a).cmp(&rank(b)).then_with(|| match (a, b) {
            (TuiLibraryNode::Folder { name: na, .. }, TuiLibraryNode::Folder { name: nb, .. }) =>
                na.to_lowercase().cmp(&nb.to_lowercase()),
            (TuiLibraryNode::Album { album: x }, TuiLibraryNode::Album { album: y }) =>
                crate::library::compare_albums(
                    &mode,
                    (&x.album_artist, &x.album, &x.year),
                    (&y.album_artist, &y.album, &y.year),
                ).then_with(|| x.id.cmp(&y.id)),
            _ => std::cmp::Ordering::Equal,
        }));
        self.nodes = nodes;
    }

    fn navigate_to(&mut self, path: PathBuf) {
        let idx = self.nav_idx;
        let len = self.nav_stack.len();
        if idx + 1 < len { self.nav_stack.truncate(idx + 1); }
        self.nav_stack.push(path);
        self.nav_idx = self.nav_stack.len() - 1;
    }

    fn navigate_back(&mut self) {
        if self.nav_idx > 0 {
            self.nav_idx -= 1;
        } else {
            self.nav_stack.clear();
            self.nav_idx = 0;
        }
    }

    fn navigate_forward(&mut self) {
        let l = self.nav_stack.len();
        if self.nav_idx + 1 < l { self.nav_idx += 1; }
    }

    fn can_go_back(&self)    -> bool { self.nav_idx > 0 || !self.nav_stack.is_empty() }
    fn can_go_forward(&self) -> bool { self.nav_idx + 1 < self.nav_stack.len() }

    fn navigate_to_root(&mut self) {
        self.nav_stack.clear();
        self.nav_idx = 0;
    }

    fn browse_album(&mut self, id: &str) {
        self.browse_album_path = Some(id.to_string());
        let tracks = self.scan_result.as_ref()
            .and_then(|r| r.albums.iter().find(|a| a.id == id))
            .map(|album| crate::file_player::read_album_metadata(&album.track_paths)
                .into_iter()
                .map(|meta| {
                    let secs = meta.duration_secs as u64;
                    BrowseTrack {
                        title:    meta.title,
                        artist:   meta.artist,
                        duration: format!("{:02}:{:02}", secs / 60, secs % 60),
                        path:     meta.path,
                    }
                }).collect())
            .unwrap_or_default();
        self.browse_tracks = tracks;
    }
}

// view state

#[derive(Clone, Copy, PartialEq, Debug)]
enum View {
    Library,
    Album,
    Settings,
}

// main app struct

struct TuiApp {
    player:  TuiPlayerState,
    library: TuiLibraryState,

    view:    View,

    browse_dir:        Option<String>,   // album id currently being previewed
    browse_album_name: String,
    /// the previewed album itself, for the album header (artist, year).
    browse_album:      Option<LibraryAlbum>,
    file_mode_active:  bool,
    cd_view_active:    bool,

    content_selected: usize,   // cursor / selected item index
    content_scroll:   usize,   // first visible row (auto-adjusted by render)
    /// item index of the first entry in `content_item_rects`.
    content_item_base: usize,
    /// grid columns and items per page from the last frame, for 2d / page keys.
    grid_cols:        usize,
    page_items:       usize,
    sidebar_scroll:   usize,
    seek_dragging:    bool,
    seek_bar_rect:    Rect,
    vol_dragging:     bool,
    vol_bar_rect:     Rect,
    btn_prev:   Rect,
    btn_play:   Rect,
    btn_next:   Rect,
    btn_back:   Rect,
    btn_fwd:    Rect,
    btn_search: Rect,
    btn_view:   Rect,
    btn_shuffle: Rect,
    btn_repeat:  Rect,
    btn_queue_clear: Rect,
    btn_queue_close: Rect,
    btn_queue_toggle: Rect,
    lyrics_rect: Rect,
    /// set when the lyrics were scrolled by hand; auto-follow pauses until the
    /// resync button is clicked, the track changes, or a while passes.
    lyrics_user_scroll: Option<Instant>,
    btn_lyrics_resync: Rect,
    lyrics_track: i32,
    content_item_rects: Vec<Rect>,
    cd_eject_rect: Option<Rect>,
    lyric_item_rects:   Vec<Rect>,
    lyric_row_lyric_idx: Vec<usize>,
    last_position_poll: Instant,
    last_drive_scan:    Instant,
    last_disc_check:    Instant,
    last_lyrics_poll:   Instant,
    last_lib_change_check: Instant,
    /// transient messages, oldest first, shown stacked above the scan toast.
    toasts: Vec<(String, Instant)>,
    /// toasts on screen last frame, so graphics get repainted when one goes.
    toast_signature: u64,
    quitting: bool,
    sidebar_w: u16,
    divider_dragging: bool,
    divider_rect: Rect,
    // marquee animation phase (incremented ~every 350 ms)
    marquee_phase: usize,
    last_marquee: Instant,
    // lyrics loading shimmer phase (incremented ~every 80 ms)
    lyric_shimmer_phase: usize,
    last_lyric_shimmer: Instant,
    // album art via ratatui-image
    picker:            Option<Picker>,
    cover_protocol:    Option<StatefulProtocol>,
    cover_loaded_url:  String,
    cover_aspect:      f32,   // width/height ratio; 1.0 = square (default)
    thumbs:            thumbs::Thumbs,
    // os media transport controls (smtc / mpris / now playing)
    smtc:             Option<crate::smtc::SmtcHandle>,
    smtc_last_track:  i32,
    smtc_last_cover:  String,
    smtc_was_playing: bool,
    last_smtc_update: Instant,
    // clickable breadcrumb segments in the path bar
    breadcrumb_rects:       Vec<Rect>,
    breadcrumb_nav_targets: Vec<Option<usize>>,
    // true only when tracks were loaded externally (drag-drop), not from library
    is_external_file: bool,
    // dir of the album currently loaded in the player (from library browse)
    playing_album_dir: Option<String>,   // album id loaded in the player
    settings:            settings::SettingsState,
    tui_settings:        TuiSettings,
    theme:               Theme,
    /// the terminal's (foreground, background) from the startup query.
    term_colors:         Option<((u8, u8, u8), (u8, u8, u8))>,
    /// library search; Some while the search bar is open.
    search:              Option<search::Search>,
    help_open:           bool,
    // auto-detected image protocol (saved at startup so "Auto" can be restored)
    auto_detected_proto: Option<ProtocolType>,
    // protocols the terminal actually answered the startup query with.
    supported_protos: Vec<ProtocolType>,
    // the startup picker, kept so "None" can be undone without re-querying.
    base_picker: Option<Picker>,
    // queue pane
    queue_open:     bool,
    /// when true the queue pane takes the arrow keys instead of the track list.
    queue_focus:    bool,
    queue_selected: usize,
    queue_item_rects: Vec<Rect>,
    queue_item_base:  usize,
    queue_rect:       Rect,
    /// true when the terminal was too narrow to dock the queue beside the list.
    queue_fullscreen: bool,
    /// library cursor (selected, scroll) to restore when leaving an album.
    album_return:     (usize, usize),
    // set when the terminal contents may no longer match our buffer, so the
    // next frame is emitted in full instead of as a diff.
    needs_full_redraw: bool,
    // view the last frame was drawn for, to notice a switch away from the cover.
    last_drawn_view: View,
    /// what the image grid showed last frame; graphics protocols need a full
    /// redraw when tiles move, or stale pictures linger.
    grid_signature: u64,
    icons: Icons,
    discord: Option<crate::discord::DiscordPresence>,
    // last string written via SetTitle, so we only touch the terminal when it changes.
    last_terminal_title: String,
}

impl TuiApp {
    fn new() -> Self {
        let mut library = TuiLibraryState::default();
        library.init();
        let mut player  = TuiPlayerState::default();
        player.scan_drives();
        if !player.drives.is_empty() {
            player.current_drive_idx = Some(0);
            player.load_disc();
        }
        let mut app = Self::with_state(player, library, crate::library_cache::load_tui_settings());
        app.smtc    = crate::smtc::init_for_tui();
        app.discord = crate::discord::DiscordPresence::new();
        app
    }

    /// an app around already-built state, with no os integrations attached.
    fn with_state(player: TuiPlayerState, library: TuiLibraryState, tui_settings: TuiSettings) -> Self {
        let icons = Icons::from_set(tui_settings.icon_set);
        let theme = Theme::new(tui_settings.theme, tui_settings.accent, None);
        let now = Instant::now();
        Self {
            player,
            library,
            view:   View::Library,
            browse_dir:        None,
            browse_album_name: String::new(),
            browse_album:      None,
            file_mode_active:  false,
            cd_view_active:    false,
            content_selected:  0,
            content_scroll:    0,
            content_item_base: 0,
            grid_cols:         1,
            page_items:        10,
            sidebar_scroll:    0,
            seek_dragging:     false,
            seek_bar_rect:     Rect::default(),
            vol_dragging:      false,
            vol_bar_rect:      Rect::default(),
            btn_prev:   Rect::default(),
            btn_play:   Rect::default(),
            btn_next:   Rect::default(),
            btn_back:   Rect::default(),
            btn_fwd:    Rect::default(),
            btn_search: Rect::default(),
            btn_view:   Rect::default(),
            btn_shuffle: Rect::default(),
            btn_repeat:  Rect::default(),
            btn_queue_clear: Rect::default(),
            btn_queue_close: Rect::default(),
            btn_queue_toggle: Rect::default(),
            lyrics_rect: Rect::default(),
            lyrics_user_scroll: None,
            btn_lyrics_resync: Rect::default(),
            lyrics_track: -1,
            content_item_rects:   vec![],
            cd_eject_rect:        None,
            lyric_item_rects:     vec![],
            lyric_row_lyric_idx:  vec![],
            last_position_poll: now,
            last_drive_scan:    now,
            last_disc_check:    now,
            last_lyrics_poll:   now,
            last_lib_change_check: now,
            toasts: Vec::new(),
            toast_signature: 0,
            quitting:  false,
            sidebar_w: 30,
            divider_dragging: false,
            divider_rect: Rect::default(),
            marquee_phase: 0,
            last_marquee: now,
            lyric_shimmer_phase: 0,
            last_lyric_shimmer: now,
            picker:           None,
            cover_protocol:   None,
            cover_loaded_url: String::new(),
            cover_aspect:     1.0,
            thumbs:           thumbs::Thumbs::new(),
            smtc:             None,
            smtc_last_track:  -2,
            smtc_last_cover:  String::new(),
            smtc_was_playing: false,
            last_smtc_update: now,
            breadcrumb_rects:       vec![],
            breadcrumb_nav_targets: vec![],
            is_external_file: false,
            playing_album_dir: None,
            settings:          settings::SettingsState::default(),
            tui_settings,
            theme,
            term_colors:       None,
            search:            None,
            help_open:         false,
            auto_detected_proto: None,
            supported_protos: Vec::new(),
            base_picker: None,
            queue_open: false,
            queue_focus: false,
            queue_selected: 0,
            queue_item_rects: vec![],
            queue_item_base: 0,
            queue_rect: Rect::default(),
            queue_fullscreen: false,
            album_return: (0, 0),
            needs_full_redraw: true,
            last_drawn_view: View::Library,
            grid_signature: 0,
            icons,
            discord: None,
            last_terminal_title: "Kanae".to_string(),
        }
    }

    /// bring the node list and search results up to date. runs every tick and
    /// before each input event, so keys handled in one batch never act on a
    /// list that a previous key in the batch already changed.
    fn sync_nodes(&mut self) {
        // rebuild the node list only when the library changed or the cd node
        // state differs - not on every 50 ms tick (it clones every album).
        let cd_info = self.player.cd_node_info();
        if self.library.nodes_dirty || cd_info != self.library.last_cd_info {
            self.library.last_cd_info = cd_info.clone();
            self.library.refresh_nodes(cd_info);
            self.library.nodes_dirty = false;
        }

        if let Some(ref mut search) = self.search {
            let lib = &mut self.library;
            search.refresh(&mut lib.search_index, lib.scan_result.as_ref(), &lib.settings, lib.scan_gen);
        }
    }

    fn toast(&mut self, msg: impl Into<String>) {
        let msg = msg.into();
        // the same message again just stays up longer.
        self.toasts.retain(|(m, _)| *m != msg);
        self.toasts.push((msg, Instant::now()));
        if self.toasts.len() > 3 { self.toasts.remove(0); }
    }

    /// true when covers are drawn with a real graphics protocol (sixel, kitty,
    /// iterm2) rather than text cells, which changes what a redraw must do.
    fn uses_graphics(&self) -> bool {
        self.picker.as_ref().is_some_and(|p| p.protocol_type() != ProtocolType::Halfblocks)
    }

    /// the library items currently on screen: search results while searching,
    /// otherwise the current folder.
    fn visible_nodes(&self) -> &[TuiLibraryNode] {
        match &self.search {
            Some(s) if !s.query.trim().is_empty() => &s.results,
            _ => &self.library.nodes,
        }
    }

    fn save_tui_settings(&mut self) {
        crate::library_cache::save_tui_settings(&self.tui_settings);
    }

    fn apply_theme(&mut self) {
        self.theme = Theme::new(self.tui_settings.theme, self.tui_settings.accent, self.term_colors);
        self.needs_full_redraw = true;
    }

    fn reset_content_cursor(&mut self) {
        self.content_selected = 0;
        self.content_scroll = 0;
    }

    /// leave an album view back to the library listing it came from.
    fn back_to_library(&mut self) {
        self.browse_dir = None;
        self.browse_album_name.clear();
        self.browse_album = None;
        let came_from_album = self.view == View::Album;
        self.view = View::Library;
        if came_from_album {
            (self.content_selected, self.content_scroll) = self.album_return;
        } else {
            self.reset_content_cursor();
        }
    }

    /// window/terminal-tab title: "Kanae" when idle, otherwise "Kanae - Artist - Title",
    /// with a "(paused)" suffix when loaded but not currently playing.
    fn terminal_title(&self) -> String {
        if self.player.current_track < 0 || self.player.total_tracks <= 0 {
            return "Kanae".to_string();
        }
        let i = self.player.current_track as usize;
        let title = self.player.track_titles.get(i).map(|s| s.as_str()).unwrap_or("");
        if title.is_empty() {
            return "Kanae".to_string();
        }
        let artist = {
            let a = self.player.track_artists.get(i).map(|s| s.as_str()).unwrap_or("");
            if a.is_empty() { self.player.album_artist.as_str() } else { a }
        };
        let song = if artist.is_empty() { title.to_string() } else { format!("{} - {}", artist, title) };
        if self.player.is_playing {
            format!("Kanae -- {}", song)
        } else {
            format!("Kanae -- {} (paused)", song)
        }
    }

    /// load (or clear) the cover image whenever `current_cover_url` changes.
    fn load_cover(&mut self) {
        let url = self.player.current_cover_url.clone();
        self.cover_loaded_url = url.clone();

        if url.is_empty() {
            self.cover_protocol = None;
            self.cover_aspect   = 1.0;
            return;
        }

        let path = thumbs::cover_url_to_path(&url);

        let picker = match self.picker.as_ref() {
            Some(p) => p,
            None => { self.cover_protocol = None; return; }
        };

        match image::open(&path) {
            Ok(img) => {
                let (w, h) = (img.width(), img.height());
                self.cover_aspect   = if h > 0 { w as f32 / h as f32 } else { 1.0 };
                self.cover_protocol = Some(picker.new_resize_protocol(img));
            }
            Err(_) => {
                self.cover_aspect   = 1.0;
                self.cover_protocol = None;
            }
        }
    }

    /// apply a (potentially changed) image method setting to the picker.
    fn apply_image_method(&mut self) {
        let proto = match self.tui_settings.image_method {
            TuiImageMethod::None => {
                self.picker = None;
                self.cover_protocol = None;
                self.cover_loaded_url.clear();
                self.thumbs.clear();
                self.needs_full_redraw = true;
                return;
            }
            TuiImageMethod::Auto => {
                self.auto_detected_proto.unwrap_or(ProtocolType::Halfblocks)
            }
            method => resolve_protocol(method),
        };
        match self.picker.as_mut() {
            Some(p) => p.set_protocol_type(proto),
            None    => {
                // re-enabling after "None": restore the startup picker so the
                // queried cell size and capabilities come back with it.
                let mut p = self.base_picker.clone().unwrap_or_else(Picker::halfblocks);
                p.set_protocol_type(proto);
                self.picker = Some(p);
            }
        }
        // force cover reload with the new protocol.
        self.cover_protocol = None;
        self.cover_loaded_url.clear();
        self.thumbs.clear();
        self.needs_full_redraw = true;
    }

    fn tick(&mut self) {
        let now = Instant::now();

        if now.duration_since(self.last_position_poll) > Duration::from_millis(100) {
            self.player.update_position();
            self.last_position_poll = now;
        }

        self.player.poll_load();

        if now.duration_since(self.last_lyrics_poll) > Duration::from_millis(300) {
            self.player.poll_lyrics();
            self.last_lyrics_poll = now;
        }

        self.library.poll_scan();

        // live library scan: every few seconds, cheaply check whether the
        // filesystem changed and kick off an incremental rescan if so.
        if !self.library.is_scanning
            && now.duration_since(self.last_lib_change_check) > Duration::from_secs(3)
        {
            self.last_lib_change_check = now;
            if let Some(ref result) = self.library.scan_result {
                if crate::library_cache::dirs_changed(
                    &self.library.settings.search_paths,
                    &result.dirs,
                    &result.dir_mtimes,
                ) {
                    self.library.start_scan();
                }
            }
        }

        self.sync_nodes();
        self.toasts.retain(|(_, t)| t.elapsed() < Duration::from_secs(3));
        self.thumbs.poll(self.picker.as_ref());

        let drive_interval = if self.player.total_tracks > 0 { Duration::from_secs(1) } else { Duration::from_secs(3) };
        if !self.player.is_file_mode && now.duration_since(self.last_drive_scan) > drive_interval {
            self.player.scan_drives();
            if !self.player.drives.is_empty() && self.player.current_drive_idx.is_none() {
                self.player.current_drive_idx = Some(0);
            }
            self.last_drive_scan = now;
        }

        if !self.player.is_playing && !self.player.is_loading && !self.player.is_file_mode {
            if now.duration_since(self.last_disc_check) > Duration::from_secs(2) {
                if self.player.current_drive_idx.is_some() {
                    self.player.load_disc();
                }
                self.last_disc_check = now;
            }
        }

        if now.duration_since(self.last_marquee) > Duration::from_millis(350) {
            self.marquee_phase = self.marquee_phase.wrapping_add(1);
            self.last_marquee = now;
        }

        if now.duration_since(self.last_lyric_shimmer) > Duration::from_millis(80) {
            self.lyric_shimmer_phase = self.lyric_shimmer_phase.wrapping_add(1);
            self.last_lyric_shimmer = now;
        }

        if self.player.is_playing && self.player.lyric_lines.is_empty()
            && !self.player.lyrics_fetch_done
            && self.player.lyric_result.lock().unwrap().is_none()
            && self.player.lyric_fetch_thread.is_none()
        {
            self.player.fetch_lyrics_for_current();
        }

        // smtc: drain os media-key commands
        {
            use crate::smtc::SmtcCommand;
            let commands: Vec<SmtcCommand> = self.smtc
                .as_ref()
                .map(|h| h.drain_commands())
                .unwrap_or_default();
            for cmd in commands {
                match cmd {
                    SmtcCommand::Toggle => {
                        if self.player.current_track >= 0 { self.player.play_pause(); }
                    }
                    SmtcCommand::Next => {
                        self.player.next_track();
                        if self.player.is_playing { self.player.fetch_lyrics_for_current(); }
                    }
                    SmtcCommand::Previous => {
                        self.player.prev_track();
                        if self.player.is_playing { self.player.fetch_lyrics_for_current(); }
                    }
                    SmtcCommand::Seek(t) => { self.player.seek(t); }
                }
            }
        }

        // smtc: push metadata / play-state when something changed
        let track_changed = self.player.current_track != self.smtc_last_track;
        let cover_changed = self.player.current_cover_url != self.smtc_last_cover;
        if track_changed || cover_changed {
            self.smtc_last_track = self.player.current_track;
            self.smtc_last_cover = self.player.current_cover_url.clone();
            self.smtc_update_metadata();
            self.smtc_send_position();
        } else if self.player.is_playing != self.smtc_was_playing {
            self.smtc_send_position();
        }
        self.smtc_was_playing = self.player.is_playing;

        // macos: pump the cocoa run loop so MPRemoteCommandCenter delivers
        crate::smtc::pump_runloop();

        // cover art: reload image when url changes
        if self.player.current_cover_url != self.cover_loaded_url {
            self.load_cover();
        }

        // periodic smtc position refresh while playing (keeps now playing scrubber accurate).
        if self.player.is_playing && now.duration_since(self.last_smtc_update) > Duration::from_secs(3) {
            self.smtc_send_position();
            self.last_smtc_update = now;
        }

        // discord rich presence: update every tick (internal dedup avoids ipc spam)
        // when the setting is off, push None so any existing presence is cleared.
        if let Some(ref mut discord) = self.discord {
            let info = if self.player.current_track >= 0 && self.library.settings.discord_rpc {
                let i = self.player.current_track as usize;
                let title      = self.player.track_titles.get(i).cloned().unwrap_or_default();
                let raw_artist = self.player.track_artists.get(i).cloned().unwrap_or_default();
                let artist     = if raw_artist.is_empty() { self.player.album_artist.clone() } else { raw_artist };
                let album      = self.player.album_title.clone();
                let cover_url  = self.player.current_cover_url.clone();
                let pos        = self.player.current_time();
                let duration   = self.player.total_time;
                Some(crate::discord::TrackInfo {
                    title,
                    artist,
                    album,
                    cover_url,
                    position_secs: pos,
                    duration_secs: duration,
                    is_playing: self.player.is_playing,
                })
            } else {
                None
            };
            discord.update(info);
        }
    }

    fn smtc_update_metadata(&mut self) {
        let smtc = match self.smtc.as_mut() { Some(h) => h, None => return };
        if self.player.current_track < 0 {
            smtc.update(crate::smtc::SmtcUpdate::Stopped);
            return;
        }
        let i = self.player.current_track as usize;
        let title      = self.player.track_titles.get(i).cloned().unwrap_or_default();
        let raw_artist = self.player.track_artists.get(i).cloned().unwrap_or_default();
        let artist     = if raw_artist.is_empty() { self.player.album_artist.clone() } else { raw_artist };
        let album      = self.player.album_title.clone();
        // smtc.rs normalises /path -> file:///path; strip our file:// prefix first
        let cover_url = if self.player.current_cover_url.is_empty() {
            None
        } else {
            let u = &self.player.current_cover_url;
            if let Some(p) = u.strip_prefix("file://") {
                Some(format!("/{}", p.trim_start_matches('/')))
            } else {
                Some(u.clone())
            }
        };
        let duration = if self.player.total_time > 0.0 {
            Some(std::time::Duration::from_secs_f64(self.player.total_time))
        } else {
            None
        };
        smtc.update(crate::smtc::SmtcUpdate::Metadata { title, artist, album, cover_url, duration });
    }

    fn smtc_send_position(&mut self) {
        let smtc = match self.smtc.as_mut() { Some(h) => h, None => return };
        let pos = std::time::Duration::from_secs_f64(self.player.current_time());
        let update = if self.player.is_playing {
            crate::smtc::SmtcUpdate::Playing { progress: pos }
        } else if self.player.current_track >= 0 {
            crate::smtc::SmtcUpdate::Paused { progress: pos }
        } else {
            crate::smtc::SmtcUpdate::Stopped
        };
        smtc.update(update);
    }

    fn effective_tracklist(&self) -> Vec<(String, String, String)> {
        if let Some(ref _dir) = self.browse_dir {
            self.library.browse_tracks.iter()
                .map(|t| (t.title.clone(), t.artist.clone(), t.duration.clone()))
                .collect()
        } else if self.player.total_tracks > 0 {
            (0..self.player.total_tracks as usize).map(|i| (
                self.player.track_titles.get(i).cloned().unwrap_or_default(),
                self.player.track_artists.get(i).cloned().unwrap_or_default(),
                self.player.track_durations.get(i).cloned().unwrap_or_default(),
            )).collect()
        } else { vec![] }
    }

    /// paths the 'a' key should enqueue: the selected album/folder in the
    /// library, or the selected track in an album view. cd tracks have no file
    /// path, so they yield nothing.
    fn queueable_selection(&self) -> Vec<String> {
        match self.view {
            View::Library => match self.visible_nodes().get(self.content_selected) {
                Some(TuiLibraryNode::Album { album }) => album.track_paths.iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect(),
                Some(TuiLibraryNode::Folder { path, .. }) => vec![path.to_string_lossy().into_owned()],
                _ => vec![],
            },
            View::Album => {
                if self.browse_dir.is_some() {
                    self.library.browse_tracks.get(self.content_selected)
                        .map(|t| vec![t.path.to_string_lossy().into_owned()])
                        .unwrap_or_default()
                } else if self.player.is_file_mode {
                    self.player.file_tracks.get(self.content_selected)
                        .map(|t| vec![t.path.to_string_lossy().into_owned()])
                        .unwrap_or_default()
                } else {
                    vec![]
                }
            }
            View::Settings => vec![],
        }
    }

    fn open_album_view_for_library_item(&mut self, idx: usize) {
        self.album_return = (idx, self.content_scroll);
        match self.visible_nodes().get(idx).cloned() {
            Some(TuiLibraryNode::Folder { path, .. }) => {
                // opening a folder from search results drops the search.
                self.search = None;
                self.library.navigate_to(path);
                self.library.nodes_dirty = true;
                self.reset_content_cursor();
            }
            Some(TuiLibraryNode::Album { album }) => {
                // if this album is already loaded in the player, show it without re-browsing.
                if self.player.is_file_mode && self.playing_album_dir.as_ref() == Some(&album.id) {
                    self.browse_dir        = None;
                    self.browse_album_name = album.album.clone();
                    self.browse_album      = Some(album.clone());
                    self.file_mode_active  = true;
                    self.cd_view_active    = false;
                    self.view              = View::Album;
                    let cur = self.player.current_track.max(0) as usize;
                    self.content_selected  = cur;
                    self.content_scroll    = 0;
                } else {
                    self.library.browse_album(&album.id);
                    self.browse_dir        = Some(album.id.clone());
                    self.browse_album_name = album.album.clone();
                    self.browse_album      = Some(album.clone());
                    self.file_mode_active  = false;
                    self.cd_view_active    = false;
                    self.view              = View::Album;
                    self.content_selected  = 0;
                    self.content_scroll    = 0;
                }
            }
            Some(TuiLibraryNode::Cd { .. }) => {
                self.cd_view_active = true;
                self.file_mode_active = false;
                self.browse_dir = None;
                self.browse_album_name = String::new();
                self.browse_album = None;
                self.view = View::Album;
                self.content_selected = 0;
                self.content_scroll = 0;
            }
            None => {}
        }
    }

    fn play_browsed_track(&mut self, idx: usize) {
        if let Some(ref dir) = self.browse_dir.clone() {
            let paths: Vec<String> = self.library.browse_tracks.iter()
                .map(|t| t.path.to_string_lossy().into_owned())
                .collect();
            self.playing_album_dir = Some(dir.clone());
            self.browse_dir = None;
            self.browse_album_name = String::new();
            self.is_external_file = false;
            self.player.load_file_tracks(paths);
            self.player.load_track(idx as i32);
            self.player.start_playback();
            self.player.fetch_lyrics_for_current();
            self.file_mode_active = true;
        } else {
            self.player.load_track(idx as i32);
            self.player.start_playback();
            self.player.fetch_lyrics_for_current();
        }
    }
}

fn rect_contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

// stderr suppression (keep tui display clean)

#[cfg(unix)]
unsafe fn suppress_stderr() -> i32 {
    extern "C" {
        fn open(path: *const u8, flags: i32, ...) -> i32;
        fn dup(fd: i32) -> i32;
        fn dup2(oldfd: i32, newfd: i32) -> i32;
        fn close(fd: i32) -> i32;
    }
    let saved = dup(2);
    let devnull = open(b"/dev/null\0".as_ptr(), 1 /* O_WRONLY */);
    if devnull >= 0 { dup2(devnull, 2); close(devnull); }
    saved
}

#[cfg(unix)]
unsafe fn restore_stderr(saved: i32) {
    extern "C" {
        fn dup2(oldfd: i32, newfd: i32) -> i32;
        fn close(fd: i32) -> i32;
    }
    if saved >= 0 { dup2(saved, 2); close(saved); }
}

#[cfg(windows)]
unsafe fn suppress_stderr() -> *mut std::ffi::c_void {
    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> *mut std::ffi::c_void;
        fn SetStdHandle(nStdHandle: u32, hHandle: *mut std::ffi::c_void) -> i32;
        fn CreateFileA(
            lpFileName: *const u8,
            dwDesiredAccess: u32,
            dwShareMode: u32,
            lpSecurityAttributes: *const std::ffi::c_void,
            dwCreationDisposition: u32,
            dwFlagsAndAttributes: u32,
            hTemplateFile: *mut std::ffi::c_void,
        ) -> *mut std::ffi::c_void;
    }
    const STD_ERROR_HANDLE: u32 = 0xFFFFFFF4; // (DWORD)(-12)
    const GENERIC_WRITE: u32 = 0x40000000;
    const FILE_SHARE_WRITE: u32 = 2;
    const OPEN_EXISTING: u32 = 3;
    const FILE_ATTRIBUTE_NORMAL: u32 = 0x80;
    const INVALID_HANDLE_VALUE: usize = usize::MAX;

    let saved = GetStdHandle(STD_ERROR_HANDLE);
    let nul = CreateFileA(
        b"NUL\0".as_ptr(),
        GENERIC_WRITE,
        FILE_SHARE_WRITE,
        std::ptr::null(),
        OPEN_EXISTING,
        FILE_ATTRIBUTE_NORMAL,
        std::ptr::null_mut(),
    );
    if nul as usize != INVALID_HANDLE_VALUE {
        SetStdHandle(STD_ERROR_HANDLE, nul);
    }
    saved
}

#[cfg(windows)]
unsafe fn restore_stderr(saved: *mut std::ffi::c_void) {
    extern "system" {
        fn GetStdHandle(nStdHandle: u32) -> *mut std::ffi::c_void;
        fn SetStdHandle(nStdHandle: u32, hHandle: *mut std::ffi::c_void) -> i32;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    }
    const STD_ERROR_HANDLE: u32 = 0xFFFFFFF4;

    let current = GetStdHandle(STD_ERROR_HANDLE);
    SetStdHandle(STD_ERROR_HANDLE, saved);
    if !current.is_null() {
        CloseHandle(current);
    }
}

// terminal colours

type Rgb = (u8, u8, u8);

/// the terminal's default foreground and background, asked with osc 10/11. a
/// device-attributes query goes last: every terminal answers it, so the read
/// ends even when the colour queries are ignored, and nothing is left reading
/// stdin behind crossterm's back.
#[cfg(unix)]
fn query_terminal_colors() -> Option<(Rgb, Rgb)> {
    use std::io::{Read, Write};
    let mut out = io::stdout();
    out.write_all(b"\x1b]10;?\x1b\\\x1b]11;?\x1b\\\x1b[c").ok()?;
    out.flush().ok()?;
    let (tx, rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        let mut stdin = io::stdin();
        while let Ok(1) = stdin.read(&mut byte) {
            buf.push(byte[0]);
            // the da1 reply ("ESC [ ? ... c") comes last and ends the exchange.
            if byte[0] == b'c' && buf.windows(3).any(|w| w == b"\x1b[?") { break; }
            if buf.len() > 1024 { break; }
        }
        let _ = tx.send(buf);
    });
    let buf = rx.recv_timeout(Duration::from_millis(500)).ok()?;
    let text = String::from_utf8_lossy(&buf);
    Some((parse_osc_color(&text, "10")?, parse_osc_color(&text, "11")?))
}

#[cfg(not(unix))]
fn query_terminal_colors() -> Option<(Rgb, Rgb)> {
    None
}

/// pull "rgb:RRRR/GGGG/BBBB" out of an osc `code` reply; each channel has 1-4
/// hex digits.
fn parse_osc_color(text: &str, code: &str) -> Option<Rgb> {
    let start = text.find(&format!("]{};rgb:", code))? + code.len() + 6;
    let rest = &text[start..];
    let end = rest.find(|c: char| c == '\x07' || c == '\x1b').unwrap_or(rest.len());
    let mut parts = rest[..end].split('/').map(|h| {
        let v = u32::from_str_radix(h, 16).ok()?;
        let max = (1u32 << (4 * h.len().clamp(1, 4))) - 1;
        Some((v * 255 / max) as u8)
    });
    Some((parts.next()??, parts.next()??, parts.next()??))
}

// image protocol support

/// protocols the terminal reported during the startup query. Halfblocks is
/// plain text so it always works; iterm2 is env-detected rather than queried,
/// so it counts as supported only when detection actually picked it.
fn supported_protocols(picker: &Picker) -> Vec<ProtocolType> {
    use ratatui_image::picker::Capability;
    let mut out = vec![ProtocolType::Halfblocks];
    for cap in picker.capabilities() {
        match cap {
            Capability::Kitty => out.push(ProtocolType::Kitty),
            Capability::Sixel => out.push(ProtocolType::Sixel),
            _ => {}
        }
    }
    if picker.protocol_type() == ProtocolType::Iterm2 {
        out.push(ProtocolType::Iterm2);
    }
    out
}

/// the protocol an image-method setting asks for. forcing one is an explicit
/// override, so it is always honoured even when the startup query didn't list
/// it - detection misses terminals that render fine. `supported_protocols` only
/// drives the advisory note in the settings screen.
fn resolve_protocol(method: TuiImageMethod) -> ProtocolType {
    match method {
        TuiImageMethod::Sixel  => ProtocolType::Sixel,
        TuiImageMethod::Kitty  => ProtocolType::Kitty,
        TuiImageMethod::Iterm2 => ProtocolType::Iterm2,
        _                      => ProtocolType::Halfblocks,
    }
}

// entry point

pub fn run_tui() -> io::Result<()> {
    // suppress stderr so stray debug prints don't corrupt the tui display.
    #[cfg(any(unix, windows))]
    let saved_stderr = unsafe { suppress_stderr() };

    // a panic would otherwise leave the shell in raw mode on the alternate screen.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture, DisableBracketedPaste);
        default_hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture, EnableBracketedPaste, SetTitle("Kanae"))?;

    let backend  = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;
    term.clear()?;

    // query terminal for image protocol support (must be after entering alt screen,
    // before reading terminal events).
    let tui_settings = crate::library_cache::load_tui_settings();
    // always query, even when a protocol is forced: `Picker::halfblocks()` carries an
    // arbitrary 10x20 cell size and no capability list, so a sixel or kitty picker
    // built on top of it encodes at the wrong scale and emits graphics escapes blind,
    // which shreds the layout on terminals that do not understand them. the query also
    // reads stdin, and a terminal that never answers leaves ratatui-image with a thread
    // parked on `stdin().read()` that swallows keystrokes, so skip it entirely when
    // covers are switched off.
    let queried = if tui_settings.image_method == TuiImageMethod::None {
        None
    } else {
        Picker::from_query_stdio().ok()
    };
    // the colour query reads stdin the same way, so it only runs once the
    // terminal has proven it answers queries at all.
    let term_colors = if queried.is_some() { query_terminal_colors() } else { None };
    let queried = queried.unwrap_or_else(Picker::halfblocks);
    let supported = supported_protocols(&queried);
    let auto_detected_proto = Some(queried.protocol_type());

    let maybe_picker: Option<Picker> = match tui_settings.image_method {
        TuiImageMethod::None => None,
        TuiImageMethod::Auto => Some(queried.clone()),
        method => {
            let mut p = queried.clone();
            p.set_protocol_type(resolve_protocol(method));
            Some(p)
        }
    };

    let mut app = TuiApp::new();
    app.term_colors = term_colors;
    app.apply_theme();
    app.picker = maybe_picker;
    app.auto_detected_proto = auto_detected_proto;
    app.supported_protos = supported;
    app.base_picker = Some(queried);
    let tick_rate = Duration::from_millis(50);

    loop {
        app.tick();

        // a sixel cover lives entirely in one cell's symbol, and the cells it
        // covers are marked skip, so ratatui only emits it when that symbol
        // changes. anything that wipes the terminal behind our back - a resize,
        // or another view painting over the sidebar - leaves the buffer looking
        // unchanged, and the image plus the static rows around it never get
        // written again. redraw in full whenever that can have happened.
        if app.view != app.last_drawn_view {
            app.needs_full_redraw = true;
            app.last_drawn_view = app.view;
        }
        if app.needs_full_redraw {
            term.clear()?;
            app.needs_full_redraw = false;
        }

        term.draw(|f| render::render(&mut app, f))?;
        // the frame itself can discover that graphics moved (grid tiles
        // scrolled); repaint in full right away rather than a tick later.
        if app.needs_full_redraw {
            term.clear()?;
            app.needs_full_redraw = false;
            term.draw(|f| render::render(&mut app, f))?;
        }

        let title = app.terminal_title();
        if title != app.last_terminal_title {
            execute!(term.backend_mut(), SetTitle(&title))?;
            app.last_terminal_title = title;
        }

        if app.quitting { break; }

        // handle everything that queued up (fast typing, wheel bursts) before
        // the next frame instead of one event per tick.
        let mut timeout = tick_rate;
        while event::poll(timeout)? {
            match event::read()? {
                // windows terminals report both Press and Release key events -
                // only act on Press so keys don't register twice.
                Event::Key(key) if key.kind != crossterm::event::KeyEventKind::Release =>
                    input::handle_key(&mut app, key.code, key.modifiers),
                Event::Key(_) => {}
                Event::Mouse(m) => input::handle_mouse(&mut app, m),
                // a resize invalidates the graphic the terminal is holding, but
                // our buffer cell for it is unchanged, so the diff would never
                // re-emit it.
                Event::Resize(_, _) => app.needs_full_redraw = true,
                Event::Paste(text) => input::handle_paste(&mut app, &text),
                _ => {}
            }
            if app.quitting { break; }
            timeout = Duration::ZERO;
        }
    }

    app.player.stop_playback();
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen, DisableMouseCapture, DisableBracketedPaste, SetTitle(""))?;
    term.show_cursor()?;

    // restore stderr now that the tui is gone.
    #[cfg(any(unix, windows))]
    unsafe { restore_stderr(saved_stderr); }

    Ok(())
}

#[cfg(test)]
mod tests;
