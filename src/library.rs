/// Music library scanner.
///
/// Scans one or more root paths, discovers audio files grouped by album,
/// and delivers incremental progress via a channel so the UI can show a toast.
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use crate::file_player::{is_audio_file, read_file_metadata};

// ─── Public types ────────────────────────────────────────────────────────────

/// A scanned album: one or more audio files that share the same album tag,
/// found inside the same directory.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LibraryAlbum {
    /// Absolute path of the containing directory.
    pub dir: PathBuf,
    pub album:        String,
    pub album_artist: String,
    pub year:         String,
    pub cover_url:    Option<String>,
    pub track_paths:  Vec<PathBuf>,
}

/// A node in the library tree that the UI navigates.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum LibraryNode {
    Folder(PathBuf),
    Album(LibraryAlbum),
}

/// Settings that control scanning behaviour.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LibrarySettings {
    /// Root directories to scan.
    pub search_paths:  Vec<PathBuf>,
    /// Folders to treat as if their contents were in the parent ("merge").
    pub merged_folders: Vec<PathBuf>,
    /// Folders to hide entirely.
    pub ignored_folders: Vec<PathBuf>,
    /// Pinned folders/albums (always shown first).
    pub pinned_paths:  Vec<PathBuf>,
    /// When true, only albums are shown – sub-folders are traversed silently.
    pub merge_all_folders: bool,
    /// When true the lyric content cache has no entry limit.
    #[serde(default)]
    pub lrc_limit_disabled: bool,
}

/// Progress snapshot delivered during a scan.
#[derive(Clone, Debug)]
pub struct ScanProgress {
    pub files_found: usize,
    pub dirs_visited: usize,
    pub current_dir: PathBuf,
    /// Albums newly discovered since the last progress event (may be empty).
    pub new_albums: Vec<LibraryAlbum>,
}

/// Final result of a completed scan.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LibraryScanResult {
    pub albums: Vec<LibraryAlbum>,
    /// All directories that were visited (including leaf dirs with no tracks).
    pub dirs:   Vec<PathBuf>,
    /// Mtime (Unix seconds) of each scanned directory, captured at scan time.
    /// Used by `library_cache::needs_rescan` to detect filesystem changes without
    /// re-walking the tree on every startup.
    #[serde(default)]
    pub dir_mtimes: std::collections::HashMap<PathBuf, u64>,
}

// ─── Scanner ─────────────────────────────────────────────────────────────────

/// Scan all `search_paths` and return a `LibraryScanResult`.
/// Calls `progress_cb` periodically with incremental progress.
/// Respects `stop_flag` for early cancellation.
pub fn scan(
    settings: &LibrarySettings,
    stop_flag: Arc<AtomicBool>,
    progress_tx: std::sync::mpsc::SyncSender<ScanProgress>,
) -> LibraryScanResult {
    let mut result = LibraryScanResult::default();
    let counter = Arc::new(Mutex::new((0usize, 0usize))); // (files, dirs)

    for root in &settings.search_paths {
        if stop_flag.load(Ordering::Relaxed) { break; }
        scan_dir(
            root,
            root,
            settings,
            &stop_flag,
            &progress_tx,
            &counter,
            &mut result,
        );
    }

    // Sort albums: pinned first, then by artist/album.
    result.albums.sort_by(|a, b| {
        let a_pin = settings.pinned_paths.contains(&a.dir);
        let b_pin = settings.pinned_paths.contains(&b.dir);
        if a_pin != b_pin { return b_pin.cmp(&a_pin); }
        a.album_artist.to_lowercase().cmp(&b.album_artist.to_lowercase())
            .then(a.album.to_lowercase().cmp(&b.album.to_lowercase()))
    });
    result.dirs.sort();
    result.dirs.dedup();

    // Snapshot directory mtimes so needs_rescan() can detect changes in O(n_dirs)
    // without re-walking the filesystem tree on the next startup.
    result.dir_mtimes.clear();
    for dir in &result.dirs {
        if let Ok(meta) = std::fs::metadata(dir) {
            if let Ok(mtime) = meta.modified() {
                let secs = mtime
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                result.dir_mtimes.insert(dir.clone(), secs);
            }
        }
    }

    result
}

fn scan_dir(
    dir: &Path,
    root: &Path,
    settings: &LibrarySettings,
    stop_flag: &Arc<AtomicBool>,
    progress_tx: &std::sync::mpsc::SyncSender<ScanProgress>,
    counter: &Arc<Mutex<(usize, usize)>>,
    result: &mut LibraryScanResult,
) {
    if stop_flag.load(Ordering::Relaxed) { return; }
    if settings.ignored_folders.iter().any(|ig| dir.starts_with(ig) || dir == ig) { return; }

    let (files_count, dirs_count) = {
        let mut lock = counter.lock().unwrap();
        lock.1 += 1;
        (lock.0, lock.1)
    };

    let _ = progress_tx.try_send(ScanProgress {
        files_found: files_count,
        dirs_visited: dirs_count,
        current_dir: dir.to_path_buf(),
        new_albums: vec![],
    });

    let entries: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(it) => it.flatten().map(|e| e.path()).collect(),
        Err(_) => return,
    };

    // Separate audio files from sub-directories.
    let mut audio_files: Vec<PathBuf> = entries.iter()
        .filter(|p| p.is_file() && is_audio_file(p))
        .cloned().collect();
    let mut sub_dirs: Vec<PathBuf> = entries.iter()
        .filter(|p| p.is_dir())
        .cloned().collect();
    audio_files.sort();
    sub_dirs.sort();

    if !audio_files.is_empty() {
        {
            let mut lock = counter.lock().unwrap();
            lock.0 += audio_files.len();
        }
        // Group audio files in this directory by album tag.
        let tracks: Vec<_> = audio_files.iter().map(|p| read_file_metadata(p)).collect();
        let new_albums = group_into_albums(dir, &tracks);
        for album in &new_albums {
            result.albums.push(album.clone());
        }
        result.dirs.push(dir.to_path_buf());
        // Deliver newly found albums to the UI incrementally.
        let (fc, dc) = { let lock = counter.lock().unwrap(); (lock.0, lock.1) };
        let _ = progress_tx.try_send(ScanProgress {
            files_found: fc,
            dirs_visited: dc,
            current_dir: dir.to_path_buf(),
            new_albums,
        });
    } else if sub_dirs.is_empty() {
        // Empty directory — don't add to dirs (no point navigating here).
    } else if !audio_files.is_empty() || !sub_dirs.is_empty() {
        result.dirs.push(dir.to_path_buf());
    }

    // Also track dirs that have sub-dirs even if they have no audio files,
    // so navigation works.
    if audio_files.is_empty() && !sub_dirs.is_empty() {
        result.dirs.push(dir.to_path_buf());
    }

    // Determine whether to merge sub-dirs or show them as folders.
    let do_merge_all = settings.merge_all_folders;

    for sub in &sub_dirs {
        if stop_flag.load(Ordering::Relaxed) { break; }
        let is_merged = do_merge_all
            || settings.merged_folders.iter().any(|m| sub == m || sub.starts_with(m));
        // Recurse regardless — merging just affects whether sub-dir shows as
        // a folder node (handled by the UI / LibraryController).
        scan_dir(sub, root, settings, stop_flag, progress_tx, counter, result);
    }
}

fn group_into_albums(dir: &Path, tracks: &[crate::file_player::LocalTrack]) -> Vec<LibraryAlbum> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, LibraryAlbum> = BTreeMap::new();

    for t in tracks {
        let key = if t.album.is_empty() { "__no_album__".to_string() } else { t.album.clone() };
        let entry = map.entry(key.clone()).or_insert_with(|| LibraryAlbum {
            dir: dir.to_path_buf(),
            album: t.album.clone(),
            album_artist: if !t.album_artist.is_empty() { t.album_artist.clone() } else { t.artist.clone() },
            year: t.year.clone(),
            cover_url: t.cover_art_path.clone(),
            track_paths: Vec::new(),
        });
        if entry.cover_url.is_none() {
            entry.cover_url = t.cover_art_path.clone();
        }
        entry.track_paths.push(t.path.clone());
    }

    map.into_values().collect()
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// Return the OS default music directory, or None if unavailable.
pub fn default_music_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(docs) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(&docs).join("Music");
            if p.exists() { return Some(p); }
        }
        // SHGetKnownFolderPath for FOLDERID_Music via winapi would be ideal but
        // the env var is reliable on all supported Windows versions.
        None
    }
    #[cfg(target_os = "macos")]
    {
        if let Ok(home) = std::env::var("HOME") {
            let p = PathBuf::from(&home).join("Music");
            if p.exists() { return Some(p); }
        }
        None
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    {
        // XDG_MUSIC_DIR
        if let Ok(xdg_music) = std::env::var("XDG_MUSIC_DIR") {
            let p = PathBuf::from(&xdg_music);
            if p.exists() { return Some(p); }
        }
        if let Ok(home) = std::env::var("HOME") {
            let p = PathBuf::from(&home).join("Music");
            if p.exists() { return Some(p); }
        }
        None
    }
}
