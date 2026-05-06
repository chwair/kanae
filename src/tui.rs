use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::{
    event::{
        self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers,
        MouseButton, MouseEvent, MouseEventKind,
    },
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

use unicode_width::UnicodeWidthChar;
use ratatui_image::{picker::Picker, StatefulImage, protocol::StatefulProtocol};
use crate::cd_reader::{DriveInfo, TrackInfo};
use crate::file_player::LocalTrack;
use crate::library::{LibraryAlbum, LibraryNode, LibraryScanResult, LibrarySettings};
use crate::player::PendingDiscResult;

// ─── Colour palette (mirrors the QML palette) ────────────────────────────────
const CLR_SURF2: Color = Color::Rgb(30, 30, 30);
const CLR_BORDER: Color = Color::Rgb(40, 40, 40);
const CLR_BG: Color = Color::Rgb(10, 10, 10);
const CLR_TEXT: Color = Color::Rgb(223, 223, 223);
const CLR_TEXT2: Color = Color::Rgb(104, 104, 104);
const CLR_MUTED: Color = Color::Rgb(64, 64, 64);
const CLR_ACCENT: Color = Color::Rgb(191, 191, 191);

// ─── Library node for TUI ─────────────────────────────────────────────────────

#[derive(Clone)]
enum TuiLibraryNode {
    Folder { path: PathBuf, name: String },
    Album  { album: LibraryAlbum },
    Cd,
}

impl TuiLibraryNode {
    fn name(&self) -> &str {
        match self {
            TuiLibraryNode::Folder { name, .. } => name,
            TuiLibraryNode::Album  { album }     => &album.album,
            TuiLibraryNode::Cd                   => "Audio CD",
        }
    }
    fn sub(&self) -> &str {
        match self {
            TuiLibraryNode::Folder { .. }    => "",
            TuiLibraryNode::Album  { album } => &album.album_artist,
            TuiLibraryNode::Cd               => "",
        }
    }
    fn year(&self) -> &str {
        match self {
            TuiLibraryNode::Album { album } => &album.year,
            _                               => "",
        }
    }
    fn icon(&self) -> &'static str {
        match self {
            TuiLibraryNode::Folder { .. } => "▶",
            TuiLibraryNode::Album  { .. } => "♪",
            TuiLibraryNode::Cd            => "⊙",
        }
    }
}

// ─── Player state ─────────────────────────────────────────────────────────────

struct TuiPlayerState {
    // Drive / CD
    drives:             Vec<DriveInfo>,
    current_drive_idx:  Option<usize>,
    cd_tracks:          Vec<TrackInfo>,
    disc_status:        String,
    is_loading:         bool,
    disc_load_result:   Arc<Mutex<Option<PendingDiscResult>>>,
    disc_load_thread:   Option<thread::JoinHandle<()>>,
    current_disc_id:    String,
    metadata_loaded:    bool,

    // File mode
    is_file_mode:       bool,
    file_tracks:        Vec<LocalTrack>,

    // Playback
    current_track:      i32,
    total_tracks:       i32,
    is_playing:         bool,
    total_time:         f64,
    playback_start_offset: f64,

    // Shared atomics (with playback thread)
    stop_flag:          Arc<AtomicBool>,
    current_position:   Arc<AtomicU64>,
    heard_position:     Arc<AtomicU64>,
    playback_ended:     Arc<AtomicBool>,
    playback_error:     Arc<AtomicBool>,
    volume:             Arc<AtomicU64>,
    playback_thread:    Option<thread::JoinHandle<()>>,

    // Metadata
    album_title:        String,
    album_artist:       String,
    album_year:         String,
    current_cover_url:  String,

    // Track lists (parallel to cd_tracks / file_tracks)
    track_titles:       Vec<String>,
    track_artists:      Vec<String>,
    track_durations:    Vec<String>,

    // Lyrics
    lyric_lines:        Vec<String>,
    lyric_times:        Vec<f64>,
    lyrics_fetch_done:  bool,
    lyric_result:       Arc<Mutex<Option<Option<Vec<crate::lrclib::LyricLine>>>>>,
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
            disc_load_result:    Arc::new(Mutex::new(None)),
            disc_load_thread:    None,
            current_disc_id:     String::new(),
            metadata_loaded:     false,
            is_file_mode:        false,
            file_tracks:         vec![],
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
            playback_thread:     None,
            album_title:         "Unknown Album".into(),
            album_artist:        "Unknown Artist".into(),
            album_year:          String::new(),
            current_cover_url:   String::new(),
            track_titles:        vec![],
            track_artists:       vec![],
            track_durations:     vec![],
            lyric_lines:         vec![],
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

    fn load_disc(&mut self) {
        if self.is_loading { return; }
        let drive_path = match self.current_drive_idx {
            Some(i) if i < self.drives.len() => self.drives[i].path.clone(),
            _ => return,
        };
        // Join old thread first.
        if let Some(old) = self.disc_load_thread.take() {
            let _ = old.join();
        }
        *self.disc_load_result.lock().unwrap() = None;
        let result_slot = self.disc_load_result.clone();
        let meta_loaded = self.metadata_loaded;
        let handle = thread::spawn(move || {
            let result = match crate::cd_reader::open_drive(&drive_path) {
                Err(_) => PendingDiscResult::Unavailable { status: "Drive unavailable".into() },
                Ok(reader) => match crate::cd_reader::read_toc(&reader) {
                    Ok(toc) => {
                        let tracks  = crate::cd_reader::get_track_info(&toc);
                        let durations = tracks.iter()
                            .map(|t| crate::cd_reader::format_duration(t.duration_seconds))
                            .collect();
                        let metadata = if meta_loaded { None } else { crate::musicbrainz::lookup_metadata(&toc) };
                        let disc_id  = crate::musicbrainz::calculate_disc_id(&toc);
                        PendingDiscResult::Loaded { tracks, durations, metadata, disc_id }
                    }
                    Err(_) => PendingDiscResult::Empty { status: "No disc inserted".into() },
                },
            };
            *result_slot.lock().unwrap() = Some(result);
        });
        self.disc_load_thread = Some(handle);
        self.is_loading = true;
    }

    fn poll_load(&mut self) {
        let result = self.disc_load_result.lock().unwrap().take();
        let Some(result) = result else { return };
        if let Some(t) = self.disc_load_thread.take() { let _ = t.join(); }
        self.is_loading = false;

        match result {
            PendingDiscResult::Loaded { tracks, durations, metadata, disc_id } => {
                self.current_disc_id = disc_id;
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
                } else {
                    if !self.metadata_loaded {
                        self.album_title  = "Unknown Album".into();
                        self.album_artist = "Unknown Artist".into();
                        self.album_year   = String::new();
                    }
                }
                self.cd_tracks  = tracks;
                self.disc_status = String::new();
                if self.current_track < 0 { self.current_track = -1; }
            }
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
                self.current_cover_url.clear();
                self.lyric_lines.clear();
                self.lyric_times.clear();
                self.lyrics_fetch_done = false;
            }
        }
    }

    fn load_file_tracks(&mut self, paths: Vec<String>) {
        self.stop_playback();
        self.current_disc_id.clear();
        let tracks = crate::file_player::collect_files_from_paths(&paths);
        if tracks.is_empty() { return; }

        let n = tracks.len();
        self.track_durations = tracks.iter().map(|t| t.display_duration()).collect();
        self.track_titles    = tracks.iter().map(|t| t.title.clone()).collect();
        self.track_artists   = tracks.iter().map(|t| t.artist.clone()).collect();

        // Album-level metadata
        let first_album = tracks[0].album.clone();
        let all_same = tracks.iter().all(|t| t.album == first_album);
        self.album_title = if all_same && !first_album.is_empty() {
            first_album
        } else {
            paths.first()
                .and_then(|p| std::path::Path::new(p).file_name())
                .and_then(|n| n.to_str())
                .unwrap_or("Files")
                .into()
        };
        let first_aa = tracks[0].album_artist.clone();
        self.album_artist = if tracks.iter().all(|t| t.album_artist == first_aa) && !first_aa.is_empty() {
            first_aa
        } else { String::new() };
        let first_year = tracks[0].year.clone();
        self.album_year = if tracks.iter().all(|t| t.year == first_year) { first_year } else { String::new() };

        self.current_cover_url = tracks.first()
            .and_then(|t| t.cover_art_path.clone())
            .unwrap_or_default();
        self.file_tracks   = tracks;
        self.is_file_mode  = true;
        self.total_tracks  = n as i32;
        self.current_track = -1;
        self.total_time    = 0.0;
        self.is_playing    = false;
        self.metadata_loaded = true;
        self.lyric_lines.clear();
        self.lyric_times.clear();
        self.lyrics_fetch_done = false;
        // Reset atomic positions
        self.heard_position.store(0u64, Ordering::Relaxed);
        self.current_position.store(0u64, Ordering::Relaxed);
    }

    fn load_track(&mut self, idx: i32) {
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
            let handle = thread::spawn(move || {
                crate::player::play_local_file(file_path, offset, stop_flag, vol, heard, pos, ended);
            });
            self.playback_thread = Some(handle);
            self.is_playing = true;
            return;
        }

        // CD mode
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
            // Open drive with retries
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
        if was_playing { self.start_playback(); }
    }

    fn next_track(&mut self) {
        let n = self.current_track;
        let total = self.total_tracks;
        if n + 1 < total {
            let was_playing = self.is_playing;
            self.load_track(n + 1);
            if was_playing { self.start_playback(); }
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
                self.lyric_times.clear();
                self.lyrics_fetch_done = false;
                return;
            }

            // Auto-advance
            let next = self.current_track + 1;
            if next < self.total_tracks {
                self.load_track(next);
                self.start_playback();
                self.fetch_lyrics_for_current();
            }
        }
    }

    fn fetch_lyrics_for_current(&mut self) {
        let idx = self.current_track as usize;
        let title  = self.track_titles.get(idx).cloned().unwrap_or_default();
        let raw_ar = self.track_artists.get(idx).cloned().unwrap_or_default();
        let artist = if raw_ar.is_empty() { self.album_artist.clone() } else { raw_ar };
        let dur    = self.total_time;
        let disc_id     = self.current_disc_id.clone();
        let track_idx   = self.current_track;

        if title.is_empty() { return; }

        self.lyric_lines.clear();
        self.lyric_times.clear();
        self.lyrics_fetch_done = false;

        let gen = self.lyric_fetch_gen.fetch_add(1, Ordering::SeqCst) + 1;
        let gen_arc      = self.lyric_fetch_gen.clone();
        let result_slot  = { *self.lyric_result.lock().unwrap() = None; self.lyric_result.clone() };
        if let Some(old) = self.lyric_fetch_thread.take() { drop(old); }

        let handle = thread::spawn(move || {
            let mut cache = crate::lyric_cache::LyricCache::load();
            let cached_id = if !disc_id.is_empty() && track_idx >= 0 {
                cache.lookup(&disc_id, track_idx as u8)
            } else { None };
            let result = if let Some(id) = cached_id {
                crate::lrclib::fetch_by_id(id)
            } else {
                crate::lrclib::fetch_synced_lyrics(&title, &artist, dur).map(|(id, lines)| {
                    if !disc_id.is_empty() && track_idx >= 0 {
                        cache.insert(&disc_id, track_idx as u8, id);
                    }
                    lines
                })
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
            if let Some(lines) = maybe_lines {
                self.lyric_lines = lines.iter().map(|l| l.text.clone()).collect();
                self.lyric_times = lines.iter().map(|l| l.time_secs).collect();
            }
        }
    }

    fn active_lyric_idx(&self) -> i32 {
        let t = self.current_time();
        let mut best = -1i32;
        for (i, &ts) in self.lyric_times.iter().enumerate() {
            if ts <= t + 0.05 { best = i as i32; } else { break; }
        }
        best
    }
}

// ─── Library state ────────────────────────────────────────────────────────────

struct TuiLibraryState {
    settings:      LibrarySettings,
    scan_result:   Option<LibraryScanResult>,
    stop_scan:     Arc<AtomicBool>,
    scan_thread:   Option<thread::JoinHandle<()>>,
    progress_rx:   Option<std::sync::mpsc::Receiver<crate::library::ScanProgress>>,
    done_result:   Arc<Mutex<Option<LibraryScanResult>>>,
    is_scanning:   bool,
    scan_message:  String,
    // Navigation
    nav_stack:     Vec<PathBuf>,
    nav_idx:       usize,
    nodes:         Vec<TuiLibraryNode>,
    // Album browse (previewing an album before loading it)
    browse_album_path: Option<PathBuf>,
    browse_tracks:     Vec<BrowseTrack>,
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
            browse_album_path: None,
            browse_tracks:     vec![],
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
        let stop2 = stop.clone(); let done2 = done.clone();
        let handle = thread::spawn(move || {
            let r = crate::library::scan(&settings, stop2, tx);
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
                    let result = self.scan_result.get_or_insert_with(|| crate::library::LibraryScanResult { albums: vec![], dirs: vec![] });
                    result.albums.extend(p.new_albums);
                    has_new = true;
                }
            }
        }
        if has_new {
            self.refresh_nodes(None);
        }
        let done = { let mut g = self.done_result.lock().unwrap(); g.take() };
        if let Some(result) = done {
            crate::library_cache::save_cache(&result);
            self.scan_result  = Some(result);
            self.progress_rx  = None;
            self.is_scanning  = false;
            self.scan_message = String::new();
            self.refresh_nodes(None);
        }
    }

    fn init(&mut self) {
        if let Some(result) = crate::library_cache::load_cache() {
            let rescan = crate::library_cache::needs_rescan(&self.settings, &result);
            self.scan_result = Some(result);
            self.refresh_nodes(None);
            if rescan { self.start_scan(); }
        } else if !self.settings.search_paths.is_empty() {
            self.start_scan();
        }
    }

    fn current_dir(&self) -> Option<&PathBuf> {
        if self.nav_stack.is_empty() { return None; }
        self.nav_stack.get(self.nav_idx)
    }

    fn refresh_nodes(&mut self, cd_info: Option<(String, String)>) {
        let scan = match &self.scan_result { Some(s) => s, None => { self.nodes.clear(); return; } };
        let dir  = self.current_dir().cloned();

        let mut nodes: Vec<TuiLibraryNode> = Vec::new();

        if let Some(ref cd) = cd_info {
            if dir.is_none() { nodes.push(TuiLibraryNode::Cd); }
            let _ = cd; // used for CD tile
        }

        match dir {
            None => {
                // Root: show top-level dirs/albums
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
                        // Also albums directly in root
                        for album in scan.albums.iter().filter(|a| a.dir.parent() == Some(root.as_path())) {
                            if !nodes.iter().any(|n| matches!(n, TuiLibraryNode::Album { album: a } if a.dir == album.dir)) {
                                nodes.push(TuiLibraryNode::Album { album: album.clone() });
                            }
                        }
                    }
                    // Deduplicate
                    if nodes.is_empty() {
                        for album in &scan.albums {
                            nodes.push(TuiLibraryNode::Album { album: album.clone() });
                        }
                    }
                }
            }
            Some(ref current) => {
                // Sub-directory: show children folders + albums in this dir
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
                    // Direct match on albums at this path
                    for album in scan.albums.iter().filter(|a| a.dir == *current) {
                        nodes.push(TuiLibraryNode::Album { album: album.clone() });
                    }
                }
            }
        }
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
            // Go back to library root from first subfolder
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

    fn browse_album(&mut self, path: &PathBuf) {
        self.browse_album_path = Some(path.clone());
        let tracks = self.scan_result.as_ref()
            .and_then(|r| r.albums.iter().find(|a| &a.dir == path))
            .map(|album| album.track_paths.iter().map(|p| {
                let meta = crate::file_player::read_file_metadata(p);
                let secs = meta.duration_secs as u64;
                BrowseTrack {
                    title:    meta.title,
                    artist:   meta.artist,
                    duration: format!("{:02}:{:02}", secs / 60, secs % 60),
                    path:     p.clone(),
                }
            }).collect())
            .unwrap_or_default();
        self.browse_tracks = tracks;
    }
}

// ─── View / focus state ───────────────────────────────────────────────────────

#[derive(Clone, PartialEq)]
enum View {
    Library,
    Album,
    Settings,
}

#[derive(Clone, PartialEq)]
enum Focus {
    Content,
    Sidebar,
    Controls,
}

// ─── Main app struct ──────────────────────────────────────────────────────────

struct TuiApp {
    player:  TuiPlayerState,
    library: TuiLibraryState,

    view:    View,
    focus:   Focus,

    // For album view when previewing (not yet loaded into player)
    browse_dir:        Option<PathBuf>,
    browse_album_name: String,
    file_mode_active:  bool,
    cd_view_active:    bool,

    // List scroll state
    content_selected: usize,   // cursor / selected item index
    content_scroll:   usize,   // first visible row (auto-adjusted by render)
    sidebar_scroll:   usize,
    // Seek bar drag
    seek_dragging:    bool,
    seek_drag_x:      u16,
    seek_bar_rect:    Rect,
    // Volume drag
    vol_dragging:     bool,
    vol_bar_rect:     Rect,
    // Control button rects (updated each frame)
    btn_prev:   Rect,
    btn_play:   Rect,
    btn_next:   Rect,
    btn_back:   Rect,
    btn_fwd:    Rect,
    btn_lib:    Rect,
    // Content item rects (updated each frame for mouse hit-testing)
    content_item_rects: Vec<Rect>,
    // Sidebar lyric rects + mapping back to lyric index
    lyric_item_rects:   Vec<Rect>,
    lyric_row_lyric_idx: Vec<usize>,
    // Periodic timers
    last_position_poll: Instant,
    last_drive_scan:    Instant,
    last_disc_check:    Instant,
    last_lyrics_poll:   Instant,
    // Toast messages
    toast_msg: Option<(String, Instant)>,
    // Whether needs_rescan was triggered during init
    quitting: bool,
    // Draggable sidebar width
    sidebar_w: u16,
    divider_dragging: bool,
    divider_rect: Rect,
    // Marquee animation phase (incremented ~every 350 ms)
    marquee_phase: usize,
    last_marquee: Instant,
    // Lyrics loading shimmer phase (incremented ~every 80 ms)
    lyric_shimmer_phase: usize,
    last_lyric_shimmer: Instant,
    // Album art via ratatui-image
    picker:            Option<Picker>,
    cover_protocol:    Option<StatefulProtocol>,
    cover_loaded_url:  String,
    cover_aspect:      f32,   // width/height ratio; 1.0 = square (default)
    // OS media transport controls (SMTC / MPRIS / Now Playing)
    smtc:             Option<crate::smtc::SmtcHandle>,
    smtc_last_track:  i32,
    smtc_last_cover:  String,
    smtc_was_playing: bool,
    last_smtc_update: Instant,
    // Clickable breadcrumb segments in the path bar
    breadcrumb_rects:       Vec<Rect>,
    breadcrumb_nav_targets: Vec<Option<usize>>,
    // True only when tracks were loaded externally (drag-drop), not from library
    is_external_file: bool,
    // Dir of the album currently loaded in the player (from library browse)
    playing_album_dir: Option<PathBuf>,
    // Settings view state
    settings_selected:   usize,
    settings_input_mode: bool,
    settings_input_text: String,
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
        Self {
            player,
            library,
            view:   View::Library,
            focus:  Focus::Content,
            browse_dir:        None,
            browse_album_name: String::new(),
            file_mode_active:  false,
            cd_view_active:    false,
            content_selected:    0,
            content_scroll:    0,
            sidebar_scroll:    0,
            seek_dragging:     false,
            seek_drag_x:       0,
            seek_bar_rect:     Rect::default(),
            vol_dragging:      false,
            vol_bar_rect:      Rect::default(),
            btn_prev: Rect::default(),
            btn_play: Rect::default(),
            btn_next: Rect::default(),
            btn_back: Rect::default(),
            btn_fwd:  Rect::default(),
            btn_lib:  Rect::default(),
            content_item_rects:   vec![],
            lyric_item_rects:     vec![],
            lyric_row_lyric_idx: vec![],
            last_position_poll: Instant::now(),
            last_drive_scan:    Instant::now(),
            last_disc_check:    Instant::now(),
            last_lyrics_poll:   Instant::now(),
            toast_msg: None,
            quitting:  false,
            sidebar_w: 28,
            divider_dragging: false,
            divider_rect: Rect::default(),
            marquee_phase: 0,
            last_marquee: Instant::now(),
            lyric_shimmer_phase: 0,
            last_lyric_shimmer: Instant::now(),
            picker:           None,
            cover_protocol:   None,
            cover_loaded_url: String::new(),
            cover_aspect:     1.0,
            smtc:             crate::smtc::init_for_tui(),
            smtc_last_track:  -2,
            smtc_last_cover:  String::new(),
            smtc_was_playing: false,
            last_smtc_update: Instant::now(),
            breadcrumb_rects:       vec![],
            breadcrumb_nav_targets: vec![],
            is_external_file: false,
            playing_album_dir:   None,
            settings_selected:   0,
            settings_input_mode: false,
            settings_input_text: String::new(),
        }
    }

    /// Load (or clear) the cover image whenever `current_cover_url` changes.
    fn load_cover(&mut self) {
        let url = self.player.current_cover_url.clone();
        self.cover_loaded_url = url.clone();

        if url.is_empty() {
            self.cover_protocol = None;
            self.cover_aspect   = 1.0;
            return;
        }

        // Convert file:/// URL to a plain filesystem path.
        let path: String = if let Some(p) = url.strip_prefix("file:///") {
            format!("/{}", p)
        } else {
            url.clone()
        };

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

    fn tick(&mut self) {
        let now = Instant::now();

        // Update playback position + auto-advance
        if now.duration_since(self.last_position_poll) > Duration::from_millis(100) {
            self.player.update_position();
            self.last_position_poll = now;
        }

        // Poll disc load result
        self.player.poll_load();

        // Poll lyrics
        if now.duration_since(self.last_lyrics_poll) > Duration::from_millis(300) {
            self.player.poll_lyrics();
            self.last_lyrics_poll = now;
        }

        // Poll library scan
        self.library.poll_scan();

        // Refresh library nodes (in case CD state changed)
        let cd_info = if !self.player.is_file_mode && self.player.total_tracks > 0 {
            Some((self.player.album_title.clone(), self.player.album_artist.clone()))
        } else { None };
        self.library.refresh_nodes(cd_info);

        // Periodic drive scan (every 3s)
        let drive_interval = if self.player.total_tracks > 0 { Duration::from_secs(1) } else { Duration::from_secs(3) };
        if !self.player.is_file_mode && now.duration_since(self.last_drive_scan) > drive_interval {
            self.player.scan_drives();
            if !self.player.drives.is_empty() && self.player.current_drive_idx.is_none() {
                self.player.current_drive_idx = Some(0);
            }
            self.last_drive_scan = now;
        }

        // Periodic disc check (not while playing or loading)
        if !self.player.is_playing && !self.player.is_loading && !self.player.is_file_mode {
            if now.duration_since(self.last_disc_check) > Duration::from_secs(2) {
                if self.player.current_drive_idx.is_some() {
                    self.player.load_disc();
                }
                self.last_disc_check = now;
            }
        }

        // Marquee animation
        if now.duration_since(self.last_marquee) > Duration::from_millis(350) {
            self.marquee_phase = self.marquee_phase.wrapping_add(1);
            self.last_marquee = now;
        }

        // Lyrics shimmer animation
        if now.duration_since(self.last_lyric_shimmer) > Duration::from_millis(80) {
            self.lyric_shimmer_phase = self.lyric_shimmer_phase.wrapping_add(1);
            self.last_lyric_shimmer = now;
        }

        // Auto-fetch lyrics on track change
        if self.player.is_playing && self.player.lyric_lines.is_empty()
            && !self.player.lyrics_fetch_done
            && self.player.lyric_result.lock().unwrap().is_none()
            && self.player.lyric_fetch_thread.is_none()
        {
            self.player.fetch_lyrics_for_current();
        }

        // ── SMTC: drain OS media-key commands ─────────────────────────────
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

        // ── SMTC: push metadata / play-state when something changed ───────
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

        // ── macOS: pump the Cocoa run loop so MPRemoteCommandCenter delivers ──
        crate::smtc::pump_runloop();

        // ── Cover art: reload image when URL changes ─────────────────────
        if self.player.current_cover_url != self.cover_loaded_url {
            self.load_cover();
        }

        // Periodic SMTC position refresh while playing (keeps NowPlaying scrubber accurate).
        if self.player.is_playing && now.duration_since(self.last_smtc_update) > Duration::from_secs(3) {
            self.smtc_send_position();
            self.last_smtc_update = now;
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
        // smtc.rs normalises /path → file:///path; strip our file:// prefix first
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

    fn open_album_view_for_library_item(&mut self, idx: usize) {
        match self.library.nodes.get(idx).cloned() {
            Some(TuiLibraryNode::Folder { path, .. }) => {
                self.library.navigate_to(path);
                self.library.refresh_nodes(None);
                self.content_selected = 0;
                self.content_scroll = 0;
            }
            Some(TuiLibraryNode::Album { album }) => {
                // If this album is already loaded in the player, show it without re-browsing.
                if self.player.is_file_mode && self.playing_album_dir.as_ref() == Some(&album.dir) {
                    self.browse_dir        = None;
                    self.browse_album_name = album.album.clone();
                    self.file_mode_active  = true;
                    self.cd_view_active    = false;
                    self.view              = View::Album;
                    let cur = self.player.current_track.max(0) as usize;
                    self.content_selected  = cur;
                    self.content_scroll    = cur;
                } else {
                    self.library.browse_album(&album.dir);
                    self.browse_dir        = Some(album.dir.clone());
                    self.browse_album_name = album.album.clone();
                    self.file_mode_active  = false;
                    self.cd_view_active    = false;
                    self.view              = View::Album;
                    self.content_selected  = 0;
                    self.content_scroll    = 0;
                }
            }
            Some(TuiLibraryNode::Cd) => {
                if self.player.is_file_mode { /* switch back to CD mode */ }
                self.cd_view_active = true;
                self.file_mode_active = false;
                self.browse_dir = None;
                self.browse_album_name = String::new();
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

// ─── Rendering ───────────────────────────────────────────────────────────────

fn fmt_time(secs: f64) -> String {
    let s = secs.max(0.0) as u64;
    format!("{:02}:{:02}", s / 60, s % 60)
}

/// Display width of a string (CJK chars count as 2 columns).
fn display_width(text: &str) -> usize {
    text.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(0)).sum()
}

/// Truncate a string to at most `max_cols` display columns.
fn truncate_to_cols(text: &str, max_cols: usize) -> String {
    let mut result = String::new();
    let mut col = 0usize;
    for ch in text.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
        if col + cw > max_cols { break; }
        result.push(ch);
        col += cw;
    }
    result
}

/// Scrolling marquee: if text fits in width return padded; otherwise scroll char-by-char.
/// Uses display width so CJK characters are counted as 2 columns.
fn marquee(text: &str, width: usize, phase: usize) -> String {
    if width == 0 { return String::new(); }
    let dw = display_width(text);
    if dw <= width {
        // Pad to exactly `width` display columns with spaces.
        return format!("{}{}", text, " ".repeat(width - dw));
    }
    // Build (char, col_width) pairs for the text + a 3-space gap.
    let chars: Vec<(char, usize)> = text.chars()
        .chain("   ".chars())
        .map(|c| (c, UnicodeWidthChar::width(c).unwrap_or(1)))
        .collect();
    let total_dw: usize = chars.iter().map(|(_, w)| w).sum();
    // Advance phase in char-index units (cycling over the char array length).
    let start = phase % chars.len();
    let mut result = String::new();
    let mut col = 0usize;
    for &(ch, cw) in chars.iter().cycle().skip(start) {
        if col + cw > width { break; }
        result.push(ch);
        col += cw;
        if col >= width { break; }
    }
    // Pad any remaining columns (can happen with wide chars near the edge).
    if col < width { result.push_str(&" ".repeat(width - col)); }
    let _ = total_dw; // suppress unused warning
    result
}

/// Centre a string within `width` display columns.
fn center_text(text: &str, width: usize) -> String {
    let dw = display_width(text);
    if dw >= width { return truncate_to_cols(text, width); }
    let pad = (width - dw) / 2;
    format!("{}{}", " ".repeat(pad), text)
}

/// Hard-wrap `text` into chunks of at most `width` display columns.
/// Splits on spaces where possible; hard-wraps lone long words.
fn wrap_to_width(text: &str, width: usize) -> Vec<String> {
    if width == 0 { return vec![String::new()]; }
    if display_width(text) <= width { return vec![text.to_owned()]; }
    let mut rows: Vec<String> = Vec::new();
    let mut cur  = String::new();
    let mut col  = 0usize;
    let words: Vec<&str> = text.split(' ').collect();
    for word in words {
        let ww = display_width(word);
        if cur.is_empty() {
            if ww <= width {
                cur.push_str(word);
                col = ww;
            } else {
                hard_push_chars(word, width, &mut rows, &mut cur, &mut col);
            }
        } else if col + 1 + ww <= width {
            cur.push(' ');
            cur.push_str(word);
            col += 1 + ww;
        } else {
            rows.push(std::mem::take(&mut cur));
            col = 0;
            if ww <= width {
                cur.push_str(word);
                col = ww;
            } else {
                hard_push_chars(word, width, &mut rows, &mut cur, &mut col);
            }
        }
    }
    if !cur.is_empty() { rows.push(cur); }
    if rows.is_empty()  { rows.push(String::new()); }
    rows
}

/// Append `text` char-by-char into `cur`, flushing to `rows` when `width` is reached.
fn hard_push_chars(text: &str, width: usize, rows: &mut Vec<String>, cur: &mut String, col: &mut usize) {
    for ch in text.chars() {
        let cw = UnicodeWidthChar::width(ch).unwrap_or(1);
        if *col + cw > width {
            rows.push(std::mem::take(cur));
            *col = 0;
        }
        cur.push(ch);
        *col += cw;
    }
}

fn render(app: &mut TuiApp, frame: &mut Frame) {
    let area = frame.area();

    // Overall vertical split:
    //   [title bar 1]
    //   [main body]
    //   [seek bar 2]
    //   [controls 3]
    let vert = Layout::vertical([
        Constraint::Length(1), // title bar
        Constraint::Min(0),    // main body
        Constraint::Length(1), // seek bar
        Constraint::Length(2), // controls
    ]).split(area);

    render_title_bar(app, frame, vert[0]);
    render_main(app, frame, vert[1]);
    render_seek_bar(app, frame, vert[2]);
    render_controls(app, frame, vert[3]);
    render_scan_toast(app, frame, vert[1]);
    render_toast(app, frame, area);
}

fn render_title_bar(app: &TuiApp, frame: &mut Frame, area: Rect) {
    let now_playing = if app.player.is_playing && app.player.current_track >= 0 {
        let i = app.player.current_track as usize;
        let num = format!("{:02}", i + 1);
        let title  = app.player.track_titles.get(i).map(|s| s.as_str()).unwrap_or("");
        let artist = {
            let a = app.player.track_artists.get(i).map(|s| s.as_str()).unwrap_or("");
            if a.is_empty() { app.player.album_artist.as_str() } else { a }
        };
        if artist.is_empty() {
            format!("▶  {}  ·  {}", num, title)
        } else {
            format!("▶  {}  ·  {}  —  {}", num, artist, title)
        }
    } else { String::new() };

    let w = area.width as usize;
    let display = marquee(&now_playing, w, app.marquee_phase);
    let line = Line::from(Span::styled(display, Style::default().fg(CLR_TEXT2)));
    frame.render_widget(Paragraph::new(line), area);
}

fn render_main(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let sw = app.sidebar_w.max(12).min(area.width.saturating_sub(20));
    let horiz = Layout::horizontal([
        Constraint::Length(sw),
        Constraint::Length(1), // draggable divider
        Constraint::Min(0),
    ]).split(area);

    app.divider_rect = horiz[1];

    // Render the vertical divider
    let div_lines: Vec<Line> = (0..horiz[1].height)
        .map(|_| Line::from(Span::styled("│", Style::default().fg(CLR_BORDER))))
        .collect();
    frame.render_widget(Paragraph::new(div_lines), horiz[1]);

    render_sidebar(app, frame, horiz[0]);
    render_right_panel(app, frame, horiz[2]);
}

fn render_sidebar(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    // Vertical split: cover art + meta block + lyrics
    //
    // Terminal cells are NOT square.  To make the rendered image visually correct we
    // need to convert between column/row counts and pixels:
    //   displayed_px_w = cols  × cell_px_w
    //   displayed_px_h = rows  × cell_px_h
    //   rows = cols × (cell_px_w / cell_px_h) / cover_aspect
    //
    // Picker::font_size() returns (cell_px_w, cell_px_h).  Fall back to 8×16 (the
    // most common terminal default) when a picker is unavailable.
    let (cell_pw, cell_ph) = app.picker
        .as_ref()
        .map(|p| { let s = p.font_size(); (s.0 as f32, s.1 as f32) })
        .unwrap_or((8.0, 16.0));
    let inner_w = area.width.saturating_sub(2) as f32;
    let cover_inner_h = (inner_w * (cell_pw / cell_ph) / app.cover_aspect).round() as u16;
    let cover_h = (cover_inner_h + 2).min(area.height.saturating_sub(8)); // leave room for meta+lyrics
    let meta_h    = 4u16;
    let lyrics_h  = area.height.saturating_sub(cover_h + meta_h + 2);

    let vert = Layout::vertical([
        Constraint::Length(cover_h),
        Constraint::Length(1),        // separator
        Constraint::Length(meta_h),
        Constraint::Length(1),        // separator
        Constraint::Min(lyrics_h),
    ]).split(area);

    // Cover art placeholder (empty space for image)
    let cover_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(CLR_BORDER));
    let inner = cover_block.inner(vert[0]);
    frame.render_widget(cover_block, vert[0]);
    // Render cover art if available, otherwise show placeholder icon.
    // During a sidebar drag, skip the stateful widget to avoid re-encoding on
    // every intermediate size; ratatui-image will re-encode once drag ends.
    if let Some(ref mut proto) = app.cover_protocol {
        if !app.divider_dragging {
            frame.render_stateful_widget(StatefulImage::new(), inner, proto);
        }
    } else if inner.height >= 3 && inner.width >= 3 {
        let mid_y = inner.y + inner.height / 2;
        let mid_x = inner.x + inner.width / 2 - 1;
        let icon_area = Rect { x: mid_x.saturating_sub(1), y: mid_y, width: 4, height: 1 };
        frame.render_widget(
            Paragraph::new(" ⊙ ").style(Style::default().fg(CLR_MUTED)),
            icon_area,
        );
    }

    // Separator
    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(CLR_BORDER)),
        vert[1],
    );

    // Album metadata
    let title_style  = Style::default().fg(CLR_TEXT).add_modifier(Modifier::BOLD);
    let artist_style = Style::default().fg(CLR_TEXT2);
    let year_style   = Style::default().fg(CLR_MUTED);
    let w = vert[2].width as usize;
    let meta_para = Paragraph::new(vec![
        Line::from(Span::styled(marquee(&app.player.album_title,  w, app.marquee_phase), title_style)),
        Line::from(Span::styled(marquee(&app.player.album_artist, w, app.marquee_phase), artist_style)),
        Line::from(Span::styled(marquee(&app.player.album_year,   w.min(10), 0),         year_style)),
        Line::from(""),
    ]);
    frame.render_widget(meta_para, vert[2]);

    // Separator
    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(CLR_BORDER)),
        vert[3],
    );

    // Lyrics
    render_lyrics(app, frame, vert[4]);
}

fn render_lyrics(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    if area.height == 0 { return; }
    let active_idx = app.player.active_lyric_idx();
    let lines      = &app.player.lyric_lines;
    let w          = area.width as usize;

    app.lyric_item_rects.clear();
    app.lyric_row_lyric_idx.clear();

    if lines.is_empty() {
        let is_loading = app.player.lyric_fetch_thread.is_some();
        if is_loading {
            // Per-character shimmer: highlight sweeps left to right
            const LOADING_TEXT: &str = "Loading lyrics\u{2026}";
            let n = LOADING_TEXT.chars().count();
            let peak = app.lyric_shimmer_phase % n;
            let spans: Vec<Span> = LOADING_TEXT.chars().enumerate().map(|(i, ch)| {
                let dist = {
                    let d = (i as isize - peak as isize).unsigned_abs();
                    d.min(n.saturating_sub(d))
                };
                let color = match dist {
                    0 => CLR_TEXT,
                    1 => Color::Rgb(191, 191, 191),
                    2 => Color::Rgb(150, 150, 150),
                    _ => CLR_TEXT2,
                };
                Span::styled(ch.to_string(), Style::default().fg(color))
            }).collect();
            let padding = (w.saturating_sub(n)) / 2;
            let mut padded: Vec<Span> = vec![Span::raw(" ".repeat(padding))];
            padded.extend(spans);
            frame.render_widget(
                Paragraph::new(Line::from(padded)),
                area,
            );
        } else if app.player.lyrics_fetch_done && app.player.total_tracks > 0 {
            frame.render_widget(
                Paragraph::new(center_text("No lyrics found.", w))
                    .style(Style::default().fg(CLR_MUTED)),
                area,
            );
        }
        return;
    }

    // Wrap every lyric line into display-column-bounded chunks.
    // rows: (lyric_idx, chunk_text)
    let rows: Vec<(usize, String)> = lines.iter().enumerate()
        .flat_map(|(li, line)| wrap_to_width(line, w).into_iter().map(move |chunk| (li, chunk)))
        .collect();

    // Auto-scroll: keep first wrapped row of active lyric near centre.
    if active_idx >= 0 {
        if let Some(first_row) = rows.iter().position(|(li, _)| *li == active_idx as usize) {
            let half = (area.height as usize / 2).max(0);
            app.sidebar_scroll = first_row.saturating_sub(half);
        }
    }

    let scroll    = app.sidebar_scroll.min(rows.len().saturating_sub(1));
    let visible_h = area.height as usize;
    let para_lines: Vec<Line> = rows.iter().skip(scroll).take(visible_h).map(|(li, chunk)| {
        let is_active = *li as i32 == active_idx;
        let style = if is_active {
            Style::default().fg(CLR_TEXT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CLR_TEXT2)
        };
        Line::from(Span::styled(center_text(chunk, w), style))
    }).collect();
    frame.render_widget(Paragraph::new(para_lines), area);

    // Track rects + lyric-index mapping for mouse click.
    for (screen_row, (li, _)) in rows.iter().skip(scroll).take(visible_h).enumerate() {
        app.lyric_item_rects.push(Rect {
            x: area.x, y: area.y + screen_row as u16, width: area.width, height: 1,
        });
        app.lyric_row_lyric_idx.push(*li);
    }
}

fn render_right_panel(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    // Path bar at top, then content
    let vert = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1), // separator
        Constraint::Min(0),
    ]).split(area);

    render_path_bar(app, frame, vert[0]);
    frame.render_widget(
        Paragraph::new("─".repeat(area.width as usize)).style(Style::default().fg(CLR_BORDER)),
        vert[1],
    );

    match app.view {
        View::Library  => render_library(app, frame, vert[2]),
        View::Album    => render_track_list(app, frame, vert[2]),
        View::Settings => render_settings(app, frame, vert[2]),
    }
}

fn render_path_bar(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let can_back = app.library.can_go_back() || app.view == View::Album;
    let can_fwd  = app.library.can_go_forward();

    let back_style = if can_back { Style::default().fg(CLR_ACCENT) } else { Style::default().fg(CLR_MUTED) };
    let fwd_style  = if can_fwd  { Style::default().fg(CLR_ACCENT) } else { Style::default().fg(CLR_MUTED) };

    // Save button rects
    app.btn_back = Rect { x: area.x,     y: area.y, width: 3, height: 1 };
    app.btn_fwd  = Rect { x: area.x + 3, y: area.y, width: 2, height: 1 };

    // Build breadcrumb spans + track click rects
    app.breadcrumb_rects.clear();
    app.breadcrumb_nav_targets.clear();

    let link_style    = Style::default().fg(CLR_ACCENT);
    let sep_style     = Style::default().fg(CLR_MUTED);
    let current_style = Style::default().fg(CLR_TEXT2);

    let mut spans: Vec<Span> = vec![
        Span::styled(" ← ", back_style),
        Span::styled("→ ", fwd_style),
    ];

    // Running X position (buttons consume 5 display cols: 3 + 2)
    let mut cur_x = area.x + 5u16;

    // "Library" root segment — always shown, always clickable
    let lib_text = "Library";
    let lib_dw   = display_width(lib_text) as u16;
    app.breadcrumb_rects.push(Rect { x: cur_x, y: area.y, width: lib_dw, height: 1 });
    app.breadcrumb_nav_targets.push(None); // None = go to library root
    cur_x += lib_dw;
    spans.push(Span::styled(lib_text, link_style));

    match app.view {
        View::Library => {
            // Show each folder in the nav stack up to nav_idx
            for i in 0..=app.library.nav_idx {
                if i >= app.library.nav_stack.len() { break; }
                let sep    = " ⟋ ";
                let sep_dw = display_width(sep) as u16;
                spans.push(Span::styled(sep, sep_style));
                cur_x += sep_dw;

                let path    = &app.library.nav_stack[i];
                let name    = path.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string();
                let name_dw = display_width(&name) as u16;
                let style   = if i == app.library.nav_idx { current_style } else { link_style };
                app.breadcrumb_rects.push(Rect { x: cur_x, y: area.y, width: name_dw, height: 1 });
                app.breadcrumb_nav_targets.push(Some(i));
                cur_x += name_dw;
                spans.push(Span::styled(name, style));
            }
        }
        View::Album => {
            if app.cd_view_active {
                spans.push(Span::styled(" ⟋ ", sep_style));
                spans.push(Span::styled("Audio CD", current_style));
            } else {
                let album = if app.browse_dir.is_some() { app.browse_album_name.clone() }
                            else                        { app.player.album_title.clone() };
                if !album.is_empty() {
                    // Only show "Files" for externally loaded tracks (drag-drop), not library ones
                    if app.is_external_file {
                        spans.push(Span::styled(" ⟋ Files ⟋ ", sep_style));
                    } else {
                        spans.push(Span::styled(" ⟋ ", sep_style));
                    }
                    spans.push(Span::styled(album, current_style));
                }
            }
        }
        View::Settings => {
            spans.push(Span::styled(" ⟋ Settings", current_style));
        }
    }

    let _ = cur_x; // used for rect tracking above
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_library(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    app.content_item_rects.clear();

    if app.library.is_scanning && app.library.nodes.is_empty() {
        frame.render_widget(
            Paragraph::new(format!(" ⟳ {}", app.library.scan_message))
                .style(Style::default().fg(CLR_TEXT2)),
            area,
        );
        return;
    }

    if app.library.nodes.is_empty() {
        let msg = if app.library.settings.search_paths.is_empty() {
            "No music folder configured.\nPress 's' to open settings."
        } else {
            "No music found."
        };
        frame.render_widget(
            Paragraph::new(msg).style(Style::default().fg(CLR_TEXT2)),
            area,
        );
        return;
    }

    let sel = app.content_selected;
    let visible_rows = area.height as usize;
    let nodes = &app.library.nodes;
    // Clamp selection
    let sel = sel.min(nodes.len().saturating_sub(1));
    app.content_selected = sel;
    // Auto-scroll to keep selection visible
    if sel < app.content_scroll { app.content_scroll = sel; }
    if sel >= app.content_scroll + visible_rows { app.content_scroll = sel + 1 - visible_rows; }
    let scroll = app.content_scroll;

    for (row, node) in nodes.iter().enumerate().skip(scroll).take(visible_rows) {
        let y = area.y + (row - scroll) as u16;
        let row_rect = Rect { x: area.x, y, width: area.width, height: 1 };
        app.content_item_rects.push(row_rect);

        let is_selected = row == sel;
        let w = area.width as usize;
        let icon   = node.icon();
        let name   = node.name();
        let sub    = node.sub();
        let yr     = node.year();

        // Keep a fixed year column so rows align even when year is missing.
        let year_col_w = 6usize;
        let base_fixed = 2 + year_col_w + if sub.is_empty() { 0 } else { 2 };
        let avail  = w.saturating_sub(base_fixed);
        let name_w = if sub.is_empty() { avail } else { avail / 2 };
        let sub_w  = avail.saturating_sub(name_w);

        let name_display = marquee(name, name_w, app.marquee_phase);
        let sub_display  = if sub.is_empty() { String::new() } else { marquee(sub, sub_w, app.marquee_phase) };

        let (name_style, sub_style, yr_style) = if is_selected {
            (Style::default().fg(CLR_TEXT).add_modifier(Modifier::BOLD),
             Style::default().fg(CLR_TEXT2).add_modifier(Modifier::BOLD),
             Style::default().fg(CLR_MUTED).add_modifier(Modifier::BOLD))
        } else {
            (Style::default().fg(CLR_TEXT),
             Style::default().fg(CLR_TEXT2),
             Style::default().fg(CLR_MUTED))
        };
        let icon_style = if is_selected { Style::default().fg(CLR_ACCENT) } else { Style::default().fg(CLR_TEXT2) };

        let row_line = Line::from(vec![
            Span::styled(format!("{} ", icon), icon_style),
            Span::styled(name_display, name_style),
            if sub.is_empty() { Span::raw("") } else {
                Span::styled(format!("  {}", sub_display), sub_style)
            },
            Span::styled(
                if yr.is_empty() { "      ".to_string() } else { format!("  {:>4}", yr) },
                yr_style,
            ),
        ]);
        let para = Paragraph::new(row_line);
        if is_selected {
            frame.render_widget(para.style(Style::default().bg(CLR_SURF2)), row_rect);
        } else {
            frame.render_widget(para, row_rect);
        }
    }
}

fn render_track_list(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    app.content_item_rects.clear();

    let tracks = app.effective_tracklist();

    // Loading indicator
    if app.player.is_loading && app.browse_dir.is_none() {
        frame.render_widget(
            Paragraph::new(" ⟳ Reading disc…").style(Style::default().fg(CLR_TEXT2)),
            area,
        );
        return;
    }

    // No disc / empty
    if tracks.is_empty() {
        let msg = if app.cd_view_active {
            app.player.disc_status.clone()
        } else { String::new() };
        frame.render_widget(
            Paragraph::new(msg).style(Style::default().fg(CLR_TEXT2)),
            area,
        );
        return;
    }

    let sel         = app.content_selected;
    let visible_rows = area.height as usize;
    let cur         = app.player.current_track;
    let has_browse  = app.browse_dir.is_some();
    // Clamp selection
    let sel = sel.min(tracks.len().saturating_sub(1));
    app.content_selected = sel;
    // Auto-scroll to keep selection visible
    if sel < app.content_scroll { app.content_scroll = sel; }
    if sel >= app.content_scroll + visible_rows { app.content_scroll = sel + 1 - visible_rows; }
    let scroll = app.content_scroll;

    for (row, (title, artist, dur)) in tracks.iter().enumerate().skip(scroll).take(visible_rows) {
        let y = area.y + (row - scroll) as u16;
        let row_rect = Rect { x: area.x, y, width: area.width, height: 1 };
        app.content_item_rects.push(row_rect);

        let is_current  = !has_browse && row as i32 == cur;
        let is_selected = row == sel;
        // 2-char play indicator instead of row background
        let ind_w    = 2usize;
        let num_w    = 3usize;
        // Budget: ind + num + avail_for_text + "  " + dur + "  " (trailing sep) = width
        // sep_dur=2 always present; sep_art=2 only when artist is non-empty
        let dur_dw   = display_width(dur);
        let sep_art  = if artist.is_empty() { 0 } else { 2 };
        let avail    = (area.width as usize).saturating_sub(ind_w + num_w + sep_art + 2 + dur_dw);
        let title_w  = if artist.is_empty() { avail } else { avail * 6 / 10 };
        let artist_w = avail.saturating_sub(title_w);

        let indicator = if is_current { "\u{25b6} " } else { "  " };
        let num_str   = format!("{:>2} ", row + 1);
        let title_display  = marquee(title,  title_w,  app.marquee_phase);
        let artist_display = if artist.is_empty() { String::new() } else { marquee(artist, artist_w, app.marquee_phase) };

        let ind_style = Style::default().fg(CLR_ACCENT);
        let num_style = if is_current {
            Style::default().fg(CLR_ACCENT).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(CLR_TEXT2).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CLR_MUTED)
        };
        let title_style = if is_current {
            Style::default().fg(CLR_TEXT).add_modifier(Modifier::BOLD)
        } else if is_selected {
            Style::default().fg(CLR_TEXT).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(CLR_TEXT2)
        };
        let artist_style = if is_selected { Style::default().fg(CLR_TEXT2) } else { Style::default().fg(CLR_MUTED) };
        let dur_style    = if is_selected { Style::default().fg(CLR_TEXT2) } else { Style::default().fg(CLR_MUTED) };

        let mut spans = vec![
            Span::styled(indicator, ind_style),
            Span::styled(num_str, num_style),
            Span::styled(title_display, title_style),
        ];
        if !artist.is_empty() {
            spans.push(Span::styled(format!("  {}", artist_display), artist_style));
        }
        spans.push(Span::styled(format!("  {}", dur), dur_style));

        let para = Paragraph::new(Line::from(spans));
        if is_selected {
            frame.render_widget(para.style(Style::default().bg(CLR_SURF2)), row_rect);
        } else {
            frame.render_widget(para, row_rect);
        }
    }
}

fn render_seek_bar(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    if area.height == 0 { return; }

    let cur   = app.player.current_time();
    let total = app.player.total_time.max(1.0);
    let frac  = (cur / total).clamp(0.0, 1.0);

    let w = area.width as usize;
    let inner_w = w.saturating_sub(2);
    let time_w = 5usize; // "00:00"
    let bar_w  = inner_w.saturating_sub(time_w * 2 + 2);

    let filled = ((frac * bar_w as f64) as usize).min(bar_w);
    // Save rect for click handling
    let bar_x = area.x + 1 + time_w as u16 + 1;
    app.seek_bar_rect = Rect { x: bar_x, y: area.y, width: bar_w as u16, height: 1 };

    let line = if app.player.total_tracks > 0 {
        let f = "─".repeat(filled);
        let (head, tail) = if filled < bar_w {
            ("╸".to_string(), "─".repeat(bar_w - filled - 1))
        } else {
            (String::new(), String::new())
        };
        Line::from(vec![
            Span::styled(" ",             Style::default()),
            Span::styled(fmt_time(cur),   Style::default().fg(CLR_TEXT)),
            Span::styled(" ",             Style::default()),
            Span::styled(f,               Style::default().fg(Color::White)),
            Span::styled(head,            Style::default().fg(Color::White)),
            Span::styled(tail,            Style::default().fg(CLR_BORDER)),
            Span::styled(" ",             Style::default()),
            Span::styled(fmt_time(total), Style::default().fg(CLR_TEXT2)),
            Span::styled(" ",             Style::default()),
        ])
    } else {
        Line::from(vec![
            Span::styled(" ",                   Style::default()),
            Span::styled(fmt_time(cur),         Style::default().fg(CLR_TEXT)),
            Span::styled(" ",                   Style::default()),
            Span::styled("─".repeat(bar_w),     Style::default().fg(CLR_BORDER)),
            Span::styled(" ",                   Style::default()),
            Span::styled(fmt_time(total),       Style::default().fg(CLR_TEXT2)),
            Span::styled(" ",                   Style::default()),
        ])
    };
    frame.render_widget(Paragraph::new(line), area);
}

fn render_controls(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    if area.height == 0 { return; }

    let vol     = app.player.volume_f64();
    let vol_pct = (vol * 100.0) as u32;

    // Volume bar (10 chars), no icon
    let vol_bar_w = 10usize;
    let vol_filled = ((vol * vol_bar_w as f64) as usize).min(vol_bar_w);
    let vol_bar: String = format!("{}{}",
        "█".repeat(vol_filled),
        "░".repeat(vol_bar_w - vol_filled),
    );
    let pct_str = format!(" {:3}%", vol_pct); // " 80%"
    let vol_total_w = vol_bar_w + pct_str.len(); // 10 + 4 = 14

    // Transport buttons: |◀ ▶ ▶| (1 space each side of play/pause)
    // |◀=2, space=1, pp=1-2, space=1, ▶|=2, space=1 => ~8-9 chars
    let has_tracks  = app.player.total_tracks > 0 && app.player.current_track >= 0;
    let prev_style  = if app.player.current_track > 0 || app.player.current_time() > 4.0 {
        Style::default().fg(CLR_TEXT) } else { Style::default().fg(CLR_MUTED) };
    let pp_char     = if app.player.is_playing { "⏸" } else { "▶" };
    let next_style  = if app.player.current_track < app.player.total_tracks - 1 {
        Style::default().fg(CLR_TEXT) } else { Style::default().fg(CLR_MUTED) };
    let pp_style    = if has_tracks { Style::default().fg(CLR_ACCENT) } else { Style::default().fg(CLR_MUTED) };

    // transport_w: "|◀"(2) + " "(1) + pp(1) + " "(1) + "▶|"(2) + " "(1) = 8
    let transport_w = 8usize;
    // Track label fills space between transport and volume (right-aligned)
    let centre_w = (area.width as usize).saturating_sub(transport_w + vol_total_w);
    let centre_label = if has_tracks {
        let i = app.player.current_track as usize;
        let title = app.player.track_titles.get(i).cloned().unwrap_or_default();
        let artist = app.player
            .track_artists
            .get(i)
            .filter(|a| !a.is_empty())
            .cloned()
            .unwrap_or_else(|| app.player.album_artist.clone());
        if artist.is_empty() { title } else { format!("{} — {}", title, artist) }
    } else { String::new() };
    let label = marquee(&centre_label, centre_w, app.marquee_phase);

    // Button rects
    app.btn_prev = Rect { x: area.x,     y: area.y, width: 2, height: 1 };
    app.btn_play = Rect { x: area.x + 3, y: area.y, width: 1, height: 1 };
    app.btn_next = Rect { x: area.x + 5, y: area.y, width: 2, height: 1 };

    // Volume bar rect at the far right
    let vol_bar_x = area.x + area.width.saturating_sub(vol_total_w as u16);
    app.vol_bar_rect = Rect { x: vol_bar_x, y: area.y, width: vol_bar_w as u16, height: 1 };

    let line = Line::from(vec![
        Span::styled("|◀", prev_style),
        Span::styled(" ", Style::default()),
        Span::styled(pp_char, pp_style),
        Span::styled(" ", Style::default()),
        Span::styled("▶|", next_style),
        Span::styled(" ", Style::default()),
        Span::styled(label, Style::default().fg(CLR_TEXT2)),
        Span::styled(vol_bar, Style::default().fg(CLR_TEXT2)),
        Span::styled(pct_str, Style::default().fg(CLR_MUTED)),
    ]);
    frame.render_widget(Paragraph::new(line), area);

    // Second line: hint text
    if area.height > 1 {
        let hints = " Space:⏯  ←→:seek  Shift←→:navigate  n/p:track  +/-:vol  s:settings  q:quit";
        let hint_area = Rect { x: area.x, y: area.y + 1, width: area.width, height: 1 };
        frame.render_widget(
            Paragraph::new(hints).style(Style::default().fg(CLR_MUTED)),
            hint_area,
        );
    }
}

fn render_scan_toast(app: &TuiApp, frame: &mut Frame, body_area: Rect) {
    if !app.library.is_scanning || app.library.scan_message.is_empty() { return; }
    let msg = format!(" ● {} ", &app.library.scan_message);
    let w = (msg.len() as u16).min(body_area.width.saturating_sub(2)).max(1);
    let toast_rect = Rect {
        x: body_area.x + 1,
        y: body_area.y + body_area.height.saturating_sub(2),
        width: w,
        height: 1,
    };
    frame.render_widget(
        Paragraph::new(msg).style(Style::default().fg(CLR_ACCENT).bg(CLR_SURF2)),
        toast_rect,
    );
}

fn render_settings(app: &mut TuiApp, frame: &mut Frame, area: Rect) {
    let paths     = app.library.settings.search_paths.clone();
    let merge_all = app.library.settings.merge_all_folders;
    let n_paths   = paths.len();
    let idx_add    = n_paths;
    let idx_merge  = n_paths + 1;
    let idx_rescan = n_paths + 2;
    let total      = n_paths + 3;
    if app.settings_selected >= total { app.settings_selected = total.saturating_sub(1); }
    let sel = app.settings_selected;

    let [header_area, list_area, hint_area] = Layout::vertical([
        Constraint::Length(2),
        Constraint::Min(0),
        Constraint::Length(1),
    ]).areas(area);

    frame.render_widget(
        Paragraph::new(" Settings").style(Style::default().fg(CLR_TEXT).bold()),
        header_area,
    );

    let mut items: Vec<Line> = Vec::new();
    for (i, p) in paths.iter().enumerate() {
        let name = p.to_string_lossy();
        let (marker, style) = if sel == i {
            ("▸", Style::default().fg(CLR_ACCENT))
        } else {
            (" ", Style::default().fg(CLR_TEXT2))
        };
        items.push(Line::styled(format!("{} {}", marker, name), style));
    }
    // Add folder row
    if app.settings_input_mode && sel == idx_add {
        items.push(Line::styled(
            format!("▸ Path: {}_", app.settings_input_text),
            Style::default().fg(CLR_ACCENT),
        ));
    } else {
        let (marker, style) = if sel == idx_add {
            ("▸", Style::default().fg(CLR_ACCENT))
        } else {
            (" ", Style::default().fg(CLR_TEXT2))
        };
        items.push(Line::styled(format!("{} [+] Add folder...", marker), style));
    }
    // Merge all toggle
    let check = if merge_all { "■" } else { "□" };
    let (marker, style) = if sel == idx_merge {
        ("▸", Style::default().fg(CLR_ACCENT))
    } else {
        (" ", Style::default().fg(CLR_TEXT2))
    };
    items.push(Line::styled(format!("{} {} Merge all folders", marker, check), style));
    // Rescan
    let (marker, style) = if sel == idx_rescan {
        ("▸", Style::default().fg(CLR_ACCENT))
    } else {
        (" ", Style::default().fg(CLR_TEXT2))
    };
    items.push(Line::styled(format!("{} [↺] Rescan library", marker), style));

    frame.render_widget(Paragraph::new(items), list_area);
    frame.render_widget(
        Paragraph::new(" ↑↓:navigate  Enter:activate  Del:remove path  s/Esc:close")
            .style(Style::default().fg(CLR_MUTED)),
        hint_area,
    );
}

fn render_toast(app: &TuiApp, frame: &mut Frame, area: Rect) {
    if let Some((ref msg, t)) = app.toast_msg {
        if t.elapsed() < Duration::from_secs(3) {
            let toast_w = (msg.len() + 2).min(area.width as usize) as u16;
            let toast_rect = Rect {
                x: area.x + area.width.saturating_sub(toast_w + 2),
                y: area.y + area.height.saturating_sub(4),
                width: toast_w,
                height: 1,
            };
            frame.render_widget(
                Paragraph::new(format!(" {} ", msg))
                    .style(Style::default().fg(CLR_ACCENT).bg(CLR_SURF2)),
                toast_rect,
            );
        }
    }
}

// ─── Input handling ───────────────────────────────────────────────────────────

fn handle_key(app: &mut TuiApp, code: KeyCode, modifiers: KeyModifiers) {
    match code {
        KeyCode::Char('q') | KeyCode::Char('Q') => { app.quitting = true; }
        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => { app.quitting = true; }

        // Playback
        KeyCode::Char(' ') => {
            if app.player.total_tracks > 0 && app.player.current_track >= 0 {
                app.player.play_pause();
            } else if app.player.total_tracks > 0 {
                app.player.load_track(0);
                app.player.start_playback();
                app.player.fetch_lyrics_for_current();
            }
        }
        KeyCode::Char('n') | KeyCode::Char(']') => {
            let was = app.player.is_playing;
            app.player.next_track();
            if was { app.player.fetch_lyrics_for_current(); }
        }
        KeyCode::Char('p') | KeyCode::Char('[') => {
            let was = app.player.is_playing;
            app.player.prev_track();
            if was { app.player.fetch_lyrics_for_current(); }
        }
        // Settings input: capture all printable chars before other bindings
        KeyCode::Char(c) if app.view == View::Settings && app.settings_input_mode => {
            app.settings_input_text.push(c);
        }

        // Settings toggle
        KeyCode::Char('s') => {
            if app.view == View::Settings {
                app.view = View::Library;
                app.settings_input_mode = false;
                app.settings_input_text.clear();
            } else {
                app.view = View::Settings;
            }
        }

        // Seek with Left/Right
        KeyCode::Left if modifiers.is_empty() => {
            app.player.seek((app.player.current_time() - 5.0).max(0.0));
        }
        KeyCode::Right if modifiers.is_empty() => {
            app.player.seek((app.player.current_time() + 5.0).min(app.player.total_time));
        }
        // Path/history navigation with Shift+Left/Right
        KeyCode::Left if modifiers.contains(KeyModifiers::SHIFT) => {
            if app.view == View::Album {
                app.browse_dir = None; app.browse_album_name = String::new();
                app.view = View::Library; app.content_selected = 0; app.content_scroll = 0;
            } else if app.library.can_go_back() {
                app.library.navigate_back(); app.library.refresh_nodes(None); app.content_selected = 0; app.content_scroll = 0;
            }
        }
        KeyCode::Right if modifiers.contains(KeyModifiers::SHIFT) => {
            if app.library.can_go_forward() {
                app.library.navigate_forward(); app.library.refresh_nodes(None); app.content_selected = 0; app.content_scroll = 0;
            }
        }

        KeyCode::Char('+') | KeyCode::Char('=') => {
            let v = (app.player.volume_f64() + 0.05).min(1.0);
            app.player.set_volume(v);
        }
        KeyCode::Char('-') => {
            let v = (app.player.volume_f64() - 0.05).max(0.0);
            app.player.set_volume(v);
        }

        // Backspace: text input removal, or navigate back
        KeyCode::Backspace => {
            if app.view == View::Settings && app.settings_input_mode {
                app.settings_input_text.pop();
            } else if app.view == View::Album {
                app.browse_dir = None;
                app.browse_album_name = String::new();
                app.view = View::Library;
                app.content_selected = 0;
                app.content_scroll = 0;
            } else if app.library.can_go_back() {
                app.library.navigate_back();
                app.library.refresh_nodes(None);
                app.content_selected = 0;
                app.content_scroll = 0;
            }
        }

        // Esc: close settings / cancel input
        KeyCode::Esc => {
            if app.view == View::Settings {
                if app.settings_input_mode {
                    app.settings_input_mode = false;
                    app.settings_input_text.clear();
                } else {
                    app.view = View::Library;
                }
            }
        }

        // Delete: remove selected path in settings
        KeyCode::Delete if app.view == View::Settings => {
            let paths_len = app.library.settings.search_paths.len();
            let sel = app.settings_selected;
            if sel < paths_len {
                app.library.settings.search_paths.remove(sel);
                crate::library_cache::save_settings(&app.library.settings);
                if app.settings_selected > 0 { app.settings_selected -= 1; }
                app.library.start_scan();
            }
        }

        // Enter / select / activate
        KeyCode::Enter => {
            match app.view {
                View::Settings => {
                    let paths_len  = app.library.settings.search_paths.len();
                    let idx_add    = paths_len;
                    let idx_merge  = paths_len + 1;
                    let idx_rescan = paths_len + 2;
                    let sel = app.settings_selected;
                    if app.settings_input_mode {
                        // Confirm typed path
                        let path = app.settings_input_text.trim().to_string();
                        if !path.is_empty() {
                            let p = std::path::PathBuf::from(&path);
                            if p.exists() && !app.library.settings.search_paths.contains(&p) {
                                app.library.settings.search_paths.push(p);
                                crate::library_cache::save_settings(&app.library.settings);
                                app.library.start_scan();
                            }
                        }
                        app.settings_input_mode = false;
                        app.settings_input_text.clear();
                    } else if sel < paths_len {
                        // Remove selected path
                        app.library.settings.search_paths.remove(sel);
                        crate::library_cache::save_settings(&app.library.settings);
                        if app.settings_selected > 0 { app.settings_selected -= 1; }
                        app.library.start_scan();
                    } else if sel == idx_add {
                        app.settings_input_mode = true;
                        app.settings_input_text.clear();
                    } else if sel == idx_merge {
                        app.library.settings.merge_all_folders = !app.library.settings.merge_all_folders;
                        crate::library_cache::save_settings(&app.library.settings);
                        app.library.refresh_nodes(None);
                    } else if sel == idx_rescan {
                        app.library.start_scan();
                    }
                }
                _ => {
                    let n = match app.view {
                        View::Library  => app.library.nodes.len(),
                        View::Album    => app.effective_tracklist().len(),
                        View::Settings => 0,
                    };
                    let sel = app.content_selected;
                    if sel < n {
                        match app.view {
                            View::Library  => app.open_album_view_for_library_item(sel),
                            View::Album    => app.play_browsed_track(sel),
                            View::Settings => {}
                        }
                    }
                }
            }
        }

        // List navigation (Up/Down)
        KeyCode::Up => {
            match app.view {
                View::Settings => { app.settings_selected = app.settings_selected.saturating_sub(1); }
                _ => { app.content_selected = app.content_selected.saturating_sub(1); }
            }
        }
        KeyCode::Down => {
            match app.view {
                View::Settings => {
                    let max = app.library.settings.search_paths.len() + 2;
                    if app.settings_selected < max { app.settings_selected += 1; }
                }
                _ => {
                    let max = match app.view {
                        View::Library  => app.library.nodes.len().saturating_sub(1),
                        View::Album    => app.effective_tracklist().len().saturating_sub(1),
                        View::Settings => 0,
                    };
                    if app.content_selected < max { app.content_selected += 1; }
                }
            }
        }

        _ => {}
    }
}

fn handle_mouse(app: &mut TuiApp, event: MouseEvent) {
    let x = event.column;
    let y = event.row;

    match event.kind {
        MouseEventKind::Down(MouseButton::Left) => {
            // Divider drag
            if rect_contains(app.divider_rect, x, y) {
                app.divider_dragging = true;
                return;
            }
            // Seek bar
            if rect_contains(app.seek_bar_rect, x, y) && app.player.total_tracks > 0 {
                let frac = (x.saturating_sub(app.seek_bar_rect.x)) as f64 / app.seek_bar_rect.width.max(1) as f64;
                app.player.seek(frac * app.player.total_time);
                app.seek_dragging = true;
                app.seek_drag_x   = x;
                return;
            }
            // Volume bar
            if rect_contains(app.vol_bar_rect, x, y) {
                let frac = (x.saturating_sub(app.vol_bar_rect.x)) as f64 / app.vol_bar_rect.width.max(1) as f64;
                app.player.set_volume(frac);
                app.vol_dragging = true;
                return;
            }
            // Transport buttons
            if rect_contains(app.btn_prev, x, y) { app.player.prev_track(); return; }
            if rect_contains(app.btn_play, x, y) {
                if app.player.current_track >= 0 {
                    app.player.play_pause();
                } else if app.player.total_tracks > 0 {
                    app.player.load_track(0);
                    app.player.start_playback();
                    app.player.fetch_lyrics_for_current();
                }
                return;
            }
            if rect_contains(app.btn_next, x, y) { app.player.next_track(); return; }
            // Back / fwd buttons in path bar
            if rect_contains(app.btn_back, x, y) {
                if app.view == View::Album {
                    app.browse_dir = None; app.browse_album_name = String::new();
                    app.view = View::Library; app.content_scroll = 0;
                } else if app.library.can_go_back() {
                    app.library.navigate_back(); app.library.refresh_nodes(None); app.content_scroll = 0;
                }
                return;
            }
            if rect_contains(app.btn_fwd, x, y) {
                if app.library.can_go_forward() {
                    app.library.navigate_forward(); app.library.refresh_nodes(None); app.content_scroll = 0;
                }
                return;
            }
            // Breadcrumb path segments
            for (i, &rect) in app.breadcrumb_rects.iter().enumerate() {
                if rect_contains(rect, x, y) {
                    match app.breadcrumb_nav_targets.get(i) {
                        Some(None) => {
                            // "Library" root — go to library root
                            app.library.navigate_to_root();
                            app.library.refresh_nodes(None);
                            app.view = View::Library;
                            app.browse_dir = None;
                            app.browse_album_name = String::new();
                            app.content_scroll = 0;
                        }
                        Some(Some(nav_i)) => {
                            let nav_i = *nav_i;
                            if nav_i < app.library.nav_stack.len() {
                                app.library.nav_idx = nav_i;
                                app.library.refresh_nodes(None);
                                app.view = View::Library;
                                app.browse_dir = None;
                                app.browse_album_name = String::new();
                                app.content_selected = 0;
                                app.content_scroll = 0;
                            }
                        }
                        None => {}
                    }
                    return;
                }
            }
            // Content items
            for (i, &rect) in app.content_item_rects.iter().enumerate() {
                if rect_contains(rect, x, y) {
                    // Mouse click: set selection to the clicked visual row
                    let abs_idx = i + app.content_scroll;
                    app.content_selected = abs_idx;
                    match app.view {
                        View::Library  => app.open_album_view_for_library_item(abs_idx),
                        View::Album    => app.play_browsed_track(abs_idx),
                        View::Settings => {}
                    }
                    return;
                }
            }
            // Lyric items
            for (i, &rect) in app.lyric_item_rects.iter().enumerate() {
                if rect_contains(rect, x, y) {
                    let li = app.lyric_row_lyric_idx.get(i).copied().unwrap_or(i);
                    if let Some(&ts) = app.player.lyric_times.get(li) {
                        app.player.seek(ts);
                    }
                    return;
                }
            }
        }
        MouseEventKind::Drag(MouseButton::Left) => {
            if app.divider_dragging {
                app.sidebar_w = x.max(12);
            }
            if app.seek_dragging && app.player.total_tracks > 0 {
                let frac = (x.saturating_sub(app.seek_bar_rect.x)) as f64 / app.seek_bar_rect.width.max(1) as f64;
                app.player.seek(frac.clamp(0.0, 1.0) * app.player.total_time);
            }
            if app.vol_dragging {
                let frac = (x.saturating_sub(app.vol_bar_rect.x)) as f64 / app.vol_bar_rect.width.max(1) as f64;
                app.player.set_volume(frac.clamp(0.0, 1.0));
            }
        }
        MouseEventKind::Up(MouseButton::Left) => {
            app.seek_dragging = false;
            app.vol_dragging  = false;
            if app.divider_dragging {
                // Sidebar was resized — force cover reload at new size
                app.cover_loaded_url.clear();
            }
            app.divider_dragging = false;
        }
        MouseEventKind::ScrollDown => {
            if app.view == View::Settings {
                let max = app.library.settings.search_paths.len() + 2;
                if app.settings_selected < max { app.settings_selected += 1; }
            } else {
                let max = match app.view {
                    View::Library  => app.library.nodes.len().saturating_sub(1),
                    View::Album    => app.effective_tracklist().len().saturating_sub(1),
                    View::Settings => 0,
                };
                if app.content_selected < max { app.content_selected += 1; }
            }
        }
        MouseEventKind::ScrollUp => {
            if app.view == View::Settings {
                app.settings_selected = app.settings_selected.saturating_sub(1);
            } else {
                app.content_selected = app.content_selected.saturating_sub(1);
            }
        }
        _ => {}
    }
}

fn rect_contains(r: Rect, x: u16, y: u16) -> bool {
    x >= r.x && x < r.x + r.width && y >= r.y && y < r.y + r.height
}

// ─── Stderr suppression (keep TUI display clean) ─────────────────────────────

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

// ─── Entry point ─────────────────────────────────────────────────────────────

pub fn run_tui() -> io::Result<()> {
    // Suppress stderr so stray debug prints don't corrupt the TUI display.
    #[cfg(unix)]
    let saved_stderr = unsafe { suppress_stderr() };

    // Set up terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let backend  = CrosstermBackend::new(stdout);
    let mut term = Terminal::new(backend)?;
    term.clear()?;

    // Query terminal for image protocol support (must be after entering alt screen,
    // before reading terminal events).
    let picker = Picker::from_query_stdio()
        .unwrap_or_else(|_| Picker::halfblocks());

    let mut app = TuiApp::new();
    app.picker = Some(picker);
    let tick_rate = Duration::from_millis(50);

    loop {
        app.tick();
        term.draw(|f| render(&mut app, f))?;

        if app.quitting { break; }

        let timeout = tick_rate;
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) => handle_key(&mut app, key.code, key.modifiers),
                Event::Mouse(m) => handle_mouse(&mut app, m),
                Event::Resize(_, _) => {}
                _ => {}
            }
        }
    }

    // Restore terminal
    app.player.stop_playback();
    disable_raw_mode()?;
    execute!(term.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    term.show_cursor()?;

    // Restore stderr now that the TUI is gone.
    #[cfg(unix)]
    unsafe { restore_stderr(saved_stderr); }

    Ok(())
}
