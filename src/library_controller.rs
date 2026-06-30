/// Library controller — a QML-exposed QObject that manages scanning,
/// navigation, settings and CD tile state.
use cxx_qt_lib::{QStringList, QString};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use crate::library::{LibraryScanResult, LibrarySettings, ScanProgress};
use crate::library_cache;

// DTOs serialised to JSON for QML
#[derive(serde::Serialize, Clone)]
struct LibraryNodeDto {
    kind:         String,
    /// Real filesystem path. For folders this is the directory to navigate into;
    /// for albums it is the containing directory (used by the context menu for
    /// pin/merge/ignore, which operate on directories).
    path:         String,
    /// Stable album identity used for browsing. Distinguishes multiple albums or
    /// singles that share a directory. Empty for folders/CD nodes.
    id:           String,
    name:         String,
    album_artist: String,
    year:         String,
    cover_url:    String,
    pinned:       bool,
}

#[cxx_qt::bridge]
pub mod library_bridge {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qstringlist.h");
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qproperty(QString, library_nodes)]
        #[qproperty(QString, scan_message)]
        #[qproperty(bool, is_scanning)]
        #[qproperty(bool, music_dir_set)]
        #[qproperty(QString, settings_json)]
        #[qproperty(QString, current_path)]
        #[qproperty(bool, can_go_back)]
        #[qproperty(bool, can_go_forward)]
        #[qproperty(QString, pending_open_dir)]
        #[qproperty(QString, album_tracks_json)]
        type LibraryController = super::LibraryControllerRust;

        #[qinvokable] #[cxx_name = "startScan"]     fn start_scan(self: Pin<&mut Self>);
        #[qinvokable] #[cxx_name = "navigateTo"]    fn navigate_to(self: Pin<&mut Self>, path: QString);
        #[qinvokable] #[cxx_name = "navigateBack"]  fn navigate_back(self: Pin<&mut Self>);
        #[qinvokable] #[cxx_name = "navigateForward"] fn navigate_forward(self: Pin<&mut Self>);
        #[qinvokable] #[cxx_name = "openAlbum"]     fn open_album(self: Pin<&mut Self>, dir: QString);
        #[qinvokable] #[cxx_name = "setMusicDir"]   fn set_music_dir(self: Pin<&mut Self>, path: QString);
        #[qinvokable] #[cxx_name = "openFolderPicker"] fn open_folder_picker(self: Pin<&mut Self>);
        #[qinvokable] #[cxx_name = "saveSettings"]  fn save_settings_from_qml(self: Pin<&mut Self>, json: QString);
        #[qinvokable] #[cxx_name = "setFolderOption"] fn set_folder_option(self: Pin<&mut Self>, path: QString, option: QString);
        #[qinvokable] #[cxx_name = "pollScan"]      fn poll_scan(self: Pin<&mut Self>);
        #[qinvokable] #[cxx_name = "addSearchPath"] fn add_search_path(self: Pin<&mut Self>, path: QString);
        #[qinvokable] #[cxx_name = "removeSearchPath"] fn remove_search_path(self: Pin<&mut Self>, path: QString);
        #[qinvokable] #[cxx_name = "setMergeAll"]      fn set_merge_all(self: Pin<&mut Self>, value: bool);
        #[qinvokable] #[cxx_name = "browseAlbum"]      fn browse_album(self: Pin<&mut Self>, dir: QString);
        #[qinvokable] #[cxx_name = "navigateToRoot"]   fn navigate_to_root(self: Pin<&mut Self>);
        #[qinvokable]                                   fn init(self: Pin<&mut Self>);
        #[qinvokable] #[cxx_name = "purgeLrcCache"]        fn purge_lrc_cache(self: Pin<&mut Self>);
        #[qinvokable] #[cxx_name = "purgeNoLyricsCache"]   fn purge_no_lyrics_cache(self: Pin<&mut Self>);
        #[qinvokable] #[cxx_name = "setLrcLimitDisabled"]  fn set_lrc_limit_disabled(self: Pin<&mut Self>, value: bool);
        #[qinvokable] #[cxx_name = "setRomanizeLyrics"]    fn set_romanize_lyrics(self: Pin<&mut Self>, value: bool);
        #[qinvokable] #[cxx_name = "setDiscordRpc"]        fn set_discord_rpc(self: Pin<&mut Self>, value: bool);
    }
}

struct LibraryState {
    settings:    LibrarySettings,
    scan_result: Option<LibraryScanResult>,
    stop_scan:   Arc<AtomicBool>,
    scan_thread: Option<thread::JoinHandle<()>>,
    progress_rx: Option<std::sync::mpsc::Receiver<ScanProgress>>,
    done_result: Arc<Mutex<Option<LibraryScanResult>>>,
    nav_stack:   Vec<std::path::PathBuf>,
    nav_idx:     usize,
    picker_result: Arc<Mutex<Option<Option<std::path::PathBuf>>>>,
    /// Set by the background watcher thread when the filesystem changed.
    fs_dirty:    Arc<AtomicBool>,
    /// Whether the live-change watcher thread has been started.
    watcher_started: bool,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            settings:    library_cache::load_settings(),
            scan_result: None,
            stop_scan:   Arc::new(AtomicBool::new(false)),
            scan_thread: None,
            progress_rx: None,
            done_result: Arc::new(Mutex::new(None)),
            nav_stack:   vec![],
            nav_idx:     0,
            picker_result: Arc::new(Mutex::new(None)),
            fs_dirty:    Arc::new(AtomicBool::new(false)),
            watcher_started: false,
        }
    }
}

pub struct LibraryControllerRust {
    library_nodes:    QString,
    scan_message:     QString,
    is_scanning:      bool,
    music_dir_set:    bool,
    settings_json:    QString,
    current_path:     QString,
    can_go_back:      bool,
    can_go_forward:   bool,
    pending_open_dir: QString,
    album_tracks_json: QString,
    state: Arc<Mutex<LibraryState>>,
}

impl Default for LibraryControllerRust {
    fn default() -> Self {
        let settings = library_cache::load_settings();
        let music_dir_set = !settings.search_paths.is_empty();
        let settings_json = serde_json::to_string(&settings).unwrap_or_default();
        Self {
            library_nodes: QString::from("[]"),
            scan_message:  QString::from(""),
            is_scanning:   false,
            music_dir_set,
            settings_json: QString::from(settings_json.as_str()),
            current_path:  QString::from("Library"),
            can_go_back:   false,
            can_go_forward: false,
            pending_open_dir: QString::from(""),
            album_tracks_json: QString::from("[]"),
            state: Arc::new(Mutex::new(LibraryState::default())),
        }
    }
}

impl library_bridge::LibraryController {
    pub fn init(mut self: Pin<&mut Self>) {
        let (music_set, settings, cached) = {
            let state = self.state.lock().unwrap();
            (
                !state.settings.search_paths.is_empty(),
                state.settings.clone(),
                library_cache::load_cache(),
            )
        };
        self.as_mut().set_music_dir_set(music_set);
        if !music_set {
            if let Some(d) = crate::library::default_music_dir() {
                let s = d.to_string_lossy().to_string();
                self.as_mut().set_music_dir(QString::from(s.as_str()));
            }
            return;
        }
        if let Some(result) = cached {
            if library_cache::is_legacy_cache(&result) {
                // Old cache (pre-id): discard it and full-rescan so albums get
                // stable ids, singles split, and covers re-extract to the
                // persistent cache dir. Leaving scan_result unset → prev = None.
                self.as_mut().start_scan();
            } else {
                let rescan = library_cache::needs_rescan(&settings, &result);
                { self.state.lock().unwrap().scan_result = Some(result); }
                self.as_mut().refresh_nodes();
                if rescan { self.as_mut().start_scan(); }
                else { self.as_mut().ensure_watcher(); }
            }
        } else {
            self.as_mut().start_scan();
        }
    }

    pub fn start_scan(mut self: Pin<&mut Self>) {
        let has_paths = !self.state.lock().unwrap().settings.search_paths.is_empty();
        if !has_paths { self.as_mut().set_music_dir_set(false); return; }

        self.state.lock().unwrap().stop_scan.store(true, Ordering::Relaxed);
        let old = self.state.lock().unwrap().scan_thread.take();
        if let Some(t) = old { let _ = t.join(); }

        let (prog_tx, prog_rx) = std::sync::mpsc::sync_channel(64);
        let done_result: Arc<Mutex<Option<LibraryScanResult>>> = Arc::new(Mutex::new(None));
        let stop_flag = Arc::new(AtomicBool::new(false));
        let (settings_clone, prev) = {
            let st = self.state.lock().unwrap();
            (st.settings.clone(), st.scan_result.clone())
        };
        let stop_clone = stop_flag.clone();
        let done_clone = done_result.clone();

        let handle = thread::spawn(move || {
            // Reuse the previous result so unchanged directories skip re-reading.
            let r = crate::library::scan(&settings_clone, stop_clone, prog_tx, prev.as_ref());
            *done_clone.lock().unwrap() = Some(r);
        });

        {
            let mut st = self.state.lock().unwrap();
            st.stop_scan = stop_flag;
            st.scan_thread = Some(handle);
            st.progress_rx = Some(prog_rx);
            st.done_result = done_result;
            st.fs_dirty.store(false, Ordering::Relaxed);
        }
        self.as_mut().set_is_scanning(true);
        self.as_mut().set_scan_message(QString::from("Scanning\u{2026}"));
        self.as_mut().ensure_watcher();
    }

    /// Spawn the live-change watcher thread (idempotent). It periodically checks
    /// the filesystem for added/removed/changed files and flags `fs_dirty`, which
    /// `poll_scan` consumes to kick off an incremental rescan.
    fn ensure_watcher(self: Pin<&mut Self>) {
        let (state, dirty) = {
            let mut st = self.state.lock().unwrap();
            if st.watcher_started { return; }
            st.watcher_started = true;
            (self.state.clone(), st.fs_dirty.clone())
        };
        thread::spawn(move || loop {
            thread::sleep(std::time::Duration::from_secs(3));
            // Snapshot only the dir list + mtimes (not the albums) under the lock.
            let snapshot = {
                let st = state.lock().unwrap();
                // Skip while a scan is in flight or nothing scanned yet.
                if st.progress_rx.is_some() { None }
                else { st.scan_result.as_ref().map(|r|
                    (st.settings.search_paths.clone(), r.dirs.clone(), r.dir_mtimes.clone())) }
            };
            if let Some((paths, dirs, mtimes)) = snapshot {
                if library_cache::dirs_changed(&paths, &dirs, &mtimes) {
                    dirty.store(true, Ordering::Relaxed);
                }
            }
        });
    }

    pub fn poll_scan(mut self: Pin<&mut Self>) {
        let (new_albums, last_msg) = {
            let st = self.state.lock().unwrap();
            if let Some(ref rx) = st.progress_rx {
                let mut all_new: Vec<crate::library::LibraryAlbum> = Vec::new();
                let mut last_msg: Option<String> = None;
                while let Ok(p) = rx.try_recv() {
                    all_new.extend(p.new_albums);
                    if p.files_found > 0 || p.dirs_visited > 0 {
                        last_msg = Some(format!("Scanning\u{2026} {} files, {} folders", p.files_found, p.dirs_visited));
                    }
                }
                (all_new, last_msg)
            } else { (vec![], None) }
        };
        if let Some(msg) = last_msg {
            self.as_mut().set_scan_message(QString::from(msg.as_str()));
        }
        // Merge partial albums into the scan result and refresh nodes incrementally.
        // Upsert by album id so incremental rescans (which re-stream unchanged
        // directories) don't create duplicate tiles.
        if !new_albums.is_empty() {
            {
                let mut st = self.state.lock().unwrap();
                let result = st.scan_result.get_or_insert_with(|| LibraryScanResult { albums: vec![], dirs: vec![], dir_mtimes: Default::default() });
                let incoming: std::collections::HashSet<String> =
                    new_albums.iter().map(|a| a.id.clone()).collect();
                result.albums.retain(|a| !incoming.contains(&a.id));
                result.albums.extend(new_albums);
            }
            self.as_mut().refresh_nodes();
        }

        // Live rescan: the watcher flagged a filesystem change and no scan is
        // currently running — kick off an incremental rescan.
        let should_rescan = {
            let st = self.state.lock().unwrap();
            st.progress_rx.is_none() && st.fs_dirty.swap(false, Ordering::Relaxed)
        };
        if should_rescan {
            self.as_mut().start_scan();
            return;
        }

        let picker = {
            let picker_arc = self.state.lock().unwrap().picker_result.clone();
            let mut g = picker_arc.lock().unwrap();
            g.take()
        };
        if let Some(path_opt) = picker {
            if let Some(p) = path_opt {
                let s = p.to_string_lossy().to_string();
                self.as_mut().set_music_dir(QString::from(s.as_str()));
            }
        }

        let done = {
            let done_arc = self.state.lock().unwrap().done_result.clone();
            let mut g = done_arc.lock().unwrap();
            g.take()
        };
        if let Some(result) = done {
            library_cache::save_cache(&result);
            {
                let mut st = self.state.lock().unwrap();
                st.scan_result = Some(result);
                st.progress_rx = None;
            }
            self.as_mut().set_is_scanning(false);
            self.as_mut().set_scan_message(QString::from(""));
            self.as_mut().refresh_nodes();
        }
    }

    pub fn navigate_to(mut self: Pin<&mut Self>, path: QString) {
        let nav = std::path::PathBuf::from(path.to_string());
        {
            let mut st = self.state.lock().unwrap();
            let idx = st.nav_idx;
            let len = st.nav_stack.len();
            if idx + 1 < len { st.nav_stack.truncate(idx + 1); }
            st.nav_stack.push(nav);
            st.nav_idx = st.nav_stack.len() - 1;
        }
        self.as_mut().refresh_nav();
        self.as_mut().refresh_nodes();
    }

    pub fn navigate_back(mut self: Pin<&mut Self>) {
        { let mut st = self.state.lock().unwrap(); if st.nav_idx > 0 { st.nav_idx -= 1; } }
        self.as_mut().refresh_nav();
        self.as_mut().refresh_nodes();
    }

    pub fn navigate_forward(mut self: Pin<&mut Self>) {
        { let mut st = self.state.lock().unwrap(); let l = st.nav_stack.len(); if st.nav_idx + 1 < l { st.nav_idx += 1; } }
        self.as_mut().refresh_nav();
        self.as_mut().refresh_nodes();
    }

    pub fn open_album(mut self: Pin<&mut Self>, dir: QString) {
        self.as_mut().set_pending_open_dir(dir);
    }

    pub fn navigate_to_root(mut self: Pin<&mut Self>) {
        {
            let mut st = self.state.lock().unwrap();
            st.nav_stack.clear();
            st.nav_idx = 0;
        }
        self.as_mut().refresh_nav();
        self.as_mut().refresh_nodes();
    }

    pub fn browse_album(mut self: Pin<&mut Self>, id: QString) {
        use crate::file_player::read_file_metadata;
        let album_id = id.to_string();
        // Collect track paths from the scan result while holding the lock briefly.
        // Match on the stable album id so multiple albums/singles sharing a
        // directory each resolve to their own track list.
        let track_paths: Vec<std::path::PathBuf> = {
            let state = self.state.lock().unwrap();
            match &state.scan_result {
                Some(result) => result.albums.iter()
                    .find(|a| a.id == album_id)
                    .map(|album| album.track_paths.clone())
                    .unwrap_or_default(),
                None => vec![],
            }
        };
        let tracks: Vec<serde_json::Value> = track_paths.iter()
            .map(|p| {
                let meta = read_file_metadata(p);
                let secs = meta.duration_secs as u64;
                let dur = format!("{:02}:{:02}", secs / 60, secs % 60);
                serde_json::json!({ "title": meta.title, "artist": meta.artist, "duration": dur, "path": p.to_string_lossy() })
            })
            .collect();
        let json = serde_json::to_string(&tracks).unwrap_or_else(|_| "[]".to_string());
        self.as_mut().set_album_tracks_json(QString::from(json.as_str()));
    }

    pub fn set_music_dir(mut self: Pin<&mut Self>, path: QString) {
        let p = std::path::PathBuf::from(path.to_string());
        let json = {
            let mut st = self.state.lock().unwrap();
            if !st.settings.search_paths.contains(&p) { st.settings.search_paths.push(p); library_cache::save_settings(&st.settings); }
            serde_json::to_string(&st.settings).unwrap_or_default()
        };
        self.as_mut().set_settings_json(QString::from(json.as_str()));
        self.as_mut().set_music_dir_set(true);
        self.as_mut().start_scan();
    }

    pub fn open_folder_picker(self: Pin<&mut Self>) {
        let slot = self.state.lock().unwrap().picker_result.clone();
        thread::spawn(move || { *slot.lock().unwrap() = Some(rfd::FileDialog::new().pick_folder()); });
    }

    pub fn save_settings_from_qml(mut self: Pin<&mut Self>, json: QString) {
        let s = json.to_string();
        if let Ok(settings) = serde_json::from_str::<LibrarySettings>(&s) {
            library_cache::save_settings(&settings);
            let music_set = !settings.search_paths.is_empty();
            { self.state.lock().unwrap().settings = settings; }
            self.as_mut().set_settings_json(json);
            self.as_mut().set_music_dir_set(music_set);
            self.as_mut().start_scan();
        }
    }

    pub fn set_folder_option(mut self: Pin<&mut Self>, path: QString, option: QString) {
        let p = std::path::PathBuf::from(path.to_string());
        let opt = option.to_string();
        let json = {
            let mut st = self.state.lock().unwrap();
            match opt.as_str() {
                "merge"         => { if !st.settings.merged_folders.contains(&p) { st.settings.merged_folders.push(p); } }
                "merge_remove"  => { st.settings.merged_folders.retain(|x| x != &p); }
                "ignore"        => { if !st.settings.ignored_folders.contains(&p) { st.settings.ignored_folders.push(p); } }
                "ignore_remove" => { st.settings.ignored_folders.retain(|x| x != &p); }
                "pin"           => { if !st.settings.pinned_paths.contains(&p) { st.settings.pinned_paths.push(p); } }
                "unpin"         => { st.settings.pinned_paths.retain(|x| x != &p); }
                _ => {}
            }
            library_cache::save_settings(&st.settings);
            serde_json::to_string(&st.settings).unwrap_or_default()
        };
        self.as_mut().set_settings_json(QString::from(json.as_str()));
        self.as_mut().refresh_nodes();
    }

    pub fn add_search_path(mut self: Pin<&mut Self>, path: QString) {
        let p = std::path::PathBuf::from(path.to_string());
        if !p.exists() { return; }
        let json = {
            let mut st = self.state.lock().unwrap();
            if st.settings.search_paths.contains(&p) { return; }
            st.settings.search_paths.push(p);
            library_cache::save_settings(&st.settings);
            serde_json::to_string(&st.settings).unwrap_or_default()
        };
        self.as_mut().set_settings_json(QString::from(json.as_str()));
        self.as_mut().set_music_dir_set(true);
        self.as_mut().start_scan();
    }

    pub fn remove_search_path(mut self: Pin<&mut Self>, path: QString) {
        let p = std::path::PathBuf::from(path.to_string());
        let (json, music_set) = {
            let mut st = self.state.lock().unwrap();
            st.settings.search_paths.retain(|x| x != &p);
            library_cache::save_settings(&st.settings);
            (serde_json::to_string(&st.settings).unwrap_or_default(), !st.settings.search_paths.is_empty())
        };
        self.as_mut().set_settings_json(QString::from(json.as_str()));
        self.as_mut().set_music_dir_set(music_set);
        self.as_mut().start_scan();
    }

    pub fn set_merge_all(mut self: Pin<&mut Self>, value: bool) {
        let json = {
            let mut st = self.state.lock().unwrap();
            st.settings.merge_all_folders = value;
            library_cache::save_settings(&st.settings);
            serde_json::to_string(&st.settings).unwrap_or_default()
        };
        self.as_mut().set_settings_json(QString::from(json.as_str()));
        self.as_mut().refresh_nodes();
    }

    pub fn purge_lrc_cache(self: Pin<&mut Self>) {
        let mut c = crate::lyric_cache::LyricContentCache::load();
        c.purge_lrc();
        c.save();
    }

    pub fn purge_no_lyrics_cache(self: Pin<&mut Self>) {
        let mut c = crate::lyric_cache::LyricContentCache::load();
        c.purge_no_lyrics();
        c.save();
    }

    pub fn set_lrc_limit_disabled(mut self: Pin<&mut Self>, value: bool) {
        let json = {
            let mut st = self.state.lock().unwrap();
            st.settings.lrc_limit_disabled = value;
            library_cache::save_settings(&st.settings);
            serde_json::to_string(&st.settings).unwrap_or_default()
        };
        self.as_mut().set_settings_json(QString::from(json.as_str()));
    }

    pub fn set_romanize_lyrics(mut self: Pin<&mut Self>, value: bool) {
        let json = {
            let mut st = self.state.lock().unwrap();
            st.settings.romanize_lyrics = value;
            library_cache::save_settings(&st.settings);
            serde_json::to_string(&st.settings).unwrap_or_default()
        };
        self.as_mut().set_settings_json(QString::from(json.as_str()));
    }

    pub fn set_discord_rpc(mut self: Pin<&mut Self>, value: bool) {
        let json = {
            let mut st = self.state.lock().unwrap();
            st.settings.discord_rpc = value;
            library_cache::save_settings(&st.settings);
            serde_json::to_string(&st.settings).unwrap_or_default()
        };
        self.as_mut().set_settings_json(QString::from(json.as_str()));
    }

    fn refresh_nav(mut self: Pin<&mut Self>) {
        let (back, fwd, path) = {
            let st = self.state.lock().unwrap();
            (
                st.nav_idx > 0,
                st.nav_idx + 1 < st.nav_stack.len(),
                if st.nav_stack.is_empty() { "Library".to_string() }
                else { st.nav_stack[st.nav_idx].to_string_lossy().to_string() },
            )
        };
        self.as_mut().set_can_go_back(back);
        self.as_mut().set_can_go_forward(fwd);
        self.as_mut().set_current_path(QString::from(path.as_str()));
    }

    fn refresh_nodes(mut self: Pin<&mut Self>) {
        let json = {
            let st = self.state.lock().unwrap();
            let result = match st.scan_result { Some(ref r) => r, None => return };
            let dir = if st.nav_stack.is_empty() { None } else { Some(st.nav_stack[st.nav_idx].clone()) };
            build_nodes_json(dir.as_deref(), result, &st.settings)
        };
        self.as_mut().set_library_nodes(QString::from(json.as_str()));
    }
}

fn build_nodes_json(cd: Option<&std::path::Path>, result: &LibraryScanResult, settings: &LibrarySettings) -> String {
    use std::collections::BTreeSet;
    let mut dtos: Vec<LibraryNodeDto> = Vec::new();
    let ignored = |p: &std::path::Path| settings.ignored_folders.iter().any(|ig| p.starts_with(ig) || p == ig);

    let push_album = |dtos: &mut Vec<LibraryNodeDto>, album: &crate::library::LibraryAlbum, settings: &LibrarySettings| {
        let name = if album.album.is_empty() {
            album.dir.file_name().and_then(|n| n.to_str()).unwrap_or("Unknown").to_string()
        } else { album.album.clone() };
        dtos.push(LibraryNodeDto {
            kind: "album".into(),
            path: album.dir.to_string_lossy().to_string(),
            id: album.id.clone(),
            name,
            album_artist: album.album_artist.clone(),
            year: album.year.clone(),
            cover_url: album.cover_url.clone().unwrap_or_default(),
            pinned: settings.pinned_paths.contains(&album.dir),
        });
    };

    // Helper: promote folder to album if it has exactly one album descendant
    let promote_folder = |dtos: &mut Vec<LibraryNodeDto>, dir: &std::path::Path,
                          result: &LibraryScanResult, settings: &LibrarySettings,
                          used: &mut BTreeSet<std::path::PathBuf>| {
        let subtree: Vec<_> = result.albums.iter()
            .filter(|a| a.dir.starts_with(dir) && !ignored(&a.dir) && !used.contains(&a.dir))
            .collect();
        if subtree.len() == 1 {
            used.insert(subtree[0].dir.clone());
            push_album(dtos, subtree[0], settings);
            true
        } else if subtree.len() > 1 {
            let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            dtos.push(LibraryNodeDto {
                kind: "folder".into(),
                path: dir.to_string_lossy().to_string(),
                id: String::new(),
                name, album_artist: String::new(), year: String::new(),
                cover_url: String::new(), pinned: settings.pinned_paths.contains(&dir.to_path_buf()),
            });
            false
        } else {
            false // empty subtree, skip
        }
    };

    match cd {
        None => {
            let mut used: BTreeSet<std::path::PathBuf> = BTreeSet::new();
            for album in &result.albums {
                if ignored(&album.dir) { continue; }
                let at_root = settings.merge_all_folders
                    || settings.search_paths.iter().any(|sp| album.dir == *sp || album.dir.parent() == Some(sp.as_path()))
                    || settings.merged_folders.iter().any(|m| album.dir.starts_with(m));
                if !at_root { continue; }
                used.insert(album.dir.clone());
                push_album(&mut dtos, album, settings);
            }
            if !settings.merge_all_folders {
                for dir in &result.dirs {
                    if ignored(dir) || used.contains(dir) { continue; }
                    if settings.merged_folders.iter().any(|m| dir == m) { continue; }
                    let direct = settings.search_paths.iter().any(|sp| dir == sp.as_path() || dir.parent() == Some(sp.as_path()));
                    if !direct { continue; }
                    promote_folder(&mut dtos, dir, result, settings, &mut used);
                }
            }
        }
        Some(cur) => {
            let mut used: BTreeSet<std::path::PathBuf> = BTreeSet::new();
            if settings.merge_all_folders {
                for album in &result.albums {
                    if !album.dir.starts_with(cur) || ignored(&album.dir) { continue; }
                    used.insert(album.dir.clone());
                    push_album(&mut dtos, album, settings);
                }
            } else {
                // Direct-child albums
                for album in &result.albums {
                    if album.dir != cur || ignored(&album.dir) { continue; }
                    used.insert(album.dir.clone());
                    push_album(&mut dtos, album, settings);
                }
                // Sub-dirs: promote single-album folders
                for dir in &result.dirs {
                    if used.contains(dir) || dir.parent() != Some(cur) || ignored(dir) { continue; }
                    promote_folder(&mut dtos, dir, result, settings, &mut used);
                }
            }
        }
    }

    dtos.sort_by(|a, b| {
        if a.pinned != b.pinned { return b.pinned.cmp(&a.pinned); }
        if a.kind != b.kind { return if a.kind == "folder" { std::cmp::Ordering::Less } else { std::cmp::Ordering::Greater }; }
        let ak = format!("{} {}", a.album_artist.to_lowercase(), a.name.to_lowercase());
        let bk = format!("{} {}", b.album_artist.to_lowercase(), b.name.to_lowercase());
        ak.cmp(&bk)
    });

    serde_json::to_string(&dtos).unwrap_or_else(|_| "[]".to_string())
}
