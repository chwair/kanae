/// scans library roots for audio files grouped by album, streaming progress to the ui.
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::collections::HashMap;
use rayon::prelude::*;
use crate::file_player::{cover_for, is_audio_file, read_file_metadata_light};

// public types

/// a scanned album: one or more audio files in the same directory, grouped by album
/// tag. untagged tracks each become their own single, never merged together.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LibraryAlbum {
    /// stable unique identifier for this album. distinguishes multiple albums
    /// (or singles) that live in the same directory, where `dir` alone is
    /// ambiguous. used as the navigation/browse key by both frontends.
    #[serde(default)]
    pub id: String,
    /// absolute path of the containing directory.
    pub dir: PathBuf,
    pub album:        String,
    pub album_artist: String,
    pub year:         String,
    pub cover_url:    Option<String>,
    pub track_paths:  Vec<PathBuf>,
    /// true when this album is a standalone single (an untagged track shown on
    /// its own rather than grouped with siblings).
    #[serde(default)]
    pub is_single:    bool,
}

/// a node in the library tree that the ui navigates.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub enum LibraryNode {
    Folder(PathBuf),
    Album(LibraryAlbum),
}

fn default_true() -> bool { true }
fn default_crossfade_secs() -> f64 { 4.0 }
/// bounds the ui and playback both clamp to.
pub const CROSSFADE_MIN_SECS: f64 = 1.0;
pub const CROSSFADE_MAX_SECS: f64 = 12.0;

/// settings that control scanning behaviour.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LibrarySettings {
    /// root directories to scan.
    pub search_paths:  Vec<PathBuf>,
    /// folders to treat as if their contents were in the parent ("merge").
    pub merged_folders: Vec<PathBuf>,
    /// folders to hide entirely.
    pub ignored_folders: Vec<PathBuf>,
    /// pinned folders/albums (always shown first).
    pub pinned_paths:  Vec<PathBuf>,
    /// when true, only albums are shown - sub-folders are traversed silently.
    #[serde(default)]
    pub merge_all_folders: bool,
    /// when true the lyric content cache has no entry limit.
    #[serde(default)]
    pub lrc_limit_disabled: bool,
    /// when true, japanese lyric lines are romanized (kana + kanji -> romaji).
    #[serde(default)]
    pub romanize_lyrics: bool,
    /// when true, publish discord rich presence. on by default.
    #[serde(default = "default_true")]
    pub discord_rpc: bool,
    /// when true, draw the spectrum visualizer in the gui. on by default.
    #[serde(default = "default_true")]
    pub visualizer: bool,
    /// when true, overlap the end of a track with the start of the next.
    #[serde(default)]
    pub crossfade: bool,
    /// length of that overlap in seconds.
    #[serde(default = "default_crossfade_secs")]
    pub crossfade_secs: f64,
    /// album sort order: "artist" (default), "artist_desc", "album", "album_desc",
    /// "year_desc", "year_asc". empty string means the default.
    #[serde(default)]
    pub album_sort: String,
}

impl Default for LibrarySettings {
    fn default() -> Self {
        Self {
            search_paths:      Vec::new(),
            merged_folders:    Vec::new(),
            ignored_folders:   Vec::new(),
            pinned_paths:      Vec::new(),
            merge_all_folders: false,
            lrc_limit_disabled: false,
            romanize_lyrics:   false,
            discord_rpc:       true,
            visualizer:        true,
            crossfade:         false,
            crossfade_secs:    default_crossfade_secs(),
            album_sort:        String::new(),
        }
    }
}

// album sorting

/// the album sort modes, in the order the uis cycle through them.
pub const ALBUM_SORT_MODES: [&str; 6] =
    ["artist", "artist_desc", "album", "album_desc", "year_desc", "year_asc"];

/// human-readable label for an album sort mode.
pub fn album_sort_label(mode: &str) -> &'static str {
    match mode {
        "artist_desc" => "Artist (Z-A)",
        "album"       => "Album (A-Z)",
        "album_desc"  => "Album (Z-A)",
        "year_desc"   => "Year (newest)",
        "year_asc"    => "Year (oldest)",
        _             => "Artist (A-Z)",
    }
}

/// next / previous mode in the cycle (unknown or empty modes count as "artist").
pub fn album_sort_next(mode: &str) -> &'static str {
    let i = ALBUM_SORT_MODES.iter().position(|m| *m == mode).unwrap_or(0);
    ALBUM_SORT_MODES[(i + 1) % ALBUM_SORT_MODES.len()]
}
pub fn album_sort_prev(mode: &str) -> &'static str {
    let i = ALBUM_SORT_MODES.iter().position(|m| *m == mode).unwrap_or(0);
    ALBUM_SORT_MODES[(i + ALBUM_SORT_MODES.len() - 1) % ALBUM_SORT_MODES.len()]
}

/// leading numeric part of a year tag ("2003-05-01" -> 2003). none when absent.
fn year_num(y: &str) -> Option<i64> {
    let digits: String = y.trim().chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() { None } else { digits.parse().ok() }
}

/// compare two albums for display under the given sort mode. each side is
/// `(album_artist, album_name, year)`. albums with an unknown year sort after
/// dated ones in both year modes.
pub fn compare_albums(
    mode: &str,
    a: (&str, &str, &str),
    b: (&str, &str, &str),
) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    let artist = || a.0.to_lowercase().cmp(&b.0.to_lowercase());
    let name   = || a.1.to_lowercase().cmp(&b.1.to_lowercase());
    match mode {
        "artist_desc" => artist().reverse().then_with(name),
        "album"       => name().then_with(artist),
        "album_desc"  => name().reverse().then_with(artist),
        "year_desc" | "year_asc" => {
            let ord = match (year_num(a.2), year_num(b.2)) {
                (Some(x), Some(y)) => if mode == "year_desc" { y.cmp(&x) } else { x.cmp(&y) },
                (Some(_), None)    => Ordering::Less,
                (None, Some(_))    => Ordering::Greater,
                (None, None)       => Ordering::Equal,
            };
            ord.then_with(artist).then_with(name)
        }
        _ => artist().then_with(name),
    }
}

/// progress snapshot delivered during a scan.
#[derive(Clone, Debug)]
pub struct ScanProgress {
    pub files_found: usize,
    pub dirs_visited: usize,
    pub current_dir: PathBuf,
    /// albums newly discovered since the last progress event (may be empty).
    pub new_albums: Vec<LibraryAlbum>,
}

/// final result of a completed scan.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LibraryScanResult {
    pub albums: Vec<LibraryAlbum>,
    /// all directories that were visited (including leaf dirs with no tracks).
    pub dirs:   Vec<PathBuf>,
    /// mtime (unix seconds) of each scanned directory, captured at scan time.
    /// used by `library_cache::needs_rescan` to detect filesystem changes without
    /// re-walking the tree on every startup.
    #[serde(default)]
    pub dir_mtimes: std::collections::HashMap<PathBuf, u64>,
}

// scanner

/// result of the cheap directory-tree walk (phase 1 of a scan).
#[derive(Default)]
struct WalkAccum {
    /// every navigable directory (has audio and/or sub-directories).
    dirs: Vec<PathBuf>,
    /// directories that directly contain audio files, with those files.
    audio_dirs: Vec<(PathBuf, Vec<PathBuf>)>,
}

/// read a directory's modification time as unix seconds, if available.
fn dir_mtime(dir: &Path) -> Option<u64> {
    std::fs::metadata(dir)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs())
}

/// scan all `search_paths` and return a `LibraryScanResult`. phase 1 walks the
/// directory tree (cheap `read_dir` only), phase 2 reads file metadata in parallel
/// via rayon, streaming each directory's albums to the ui. when `prev` is supplied,
/// directories whose mtime is unchanged reuse their cached albums instead of
/// re-reading metadata, which makes startup and live rescans near-instant. respects
/// `stop_flag` for early cancellation.
pub fn scan(
    settings: &LibrarySettings,
    stop_flag: Arc<AtomicBool>,
    progress_tx: std::sync::mpsc::SyncSender<ScanProgress>,
    prev: Option<&LibraryScanResult>,
) -> LibraryScanResult {
    // phase 1: walk the tree (serial, just read_dir - fast).
    let mut walk = WalkAccum::default();
    let dirs_visited = Arc::new(AtomicUsize::new(0));
    for root in &settings.search_paths {
        if stop_flag.load(Ordering::Relaxed) { break; }
        walk_dir(root, settings, &stop_flag, &progress_tx, &dirs_visited, &mut walk);
        // always record an existing root, even if it currently holds no music or
        // sub-folders, so the live-change check treats it as "covered" rather
        // than perpetually out of date.
        let ignored = settings.ignored_folders.iter().any(|ig| root.starts_with(ig) || root == ig);
        if root.exists() && !ignored {
            walk.dirs.push(root.clone());
        }
    }

    // group previous albums by directory so unchanged dirs can be reused
    // without re-reading any metadata. keyed by dir, o(albums) to build.
    let mut prev_by_dir: HashMap<PathBuf, Vec<LibraryAlbum>> = HashMap::new();
    if let Some(p) = prev {
        for album in &p.albums {
            // never reuse id-less albums (legacy cache) - they'd carry empty ids
            // forward and break browsing. re-reading the dir assigns proper ids.
            if album.id.is_empty() { continue; }
            prev_by_dir.entry(album.dir.clone()).or_default().push(album.clone());
        }
    }

    let files_found = Arc::new(AtomicUsize::new(0));
    let dirs_total = walk.dirs.len();

    // phase 2: read metadata + group, in parallel across directories.
    let albums: Vec<LibraryAlbum> = walk.audio_dirs
        .par_iter()
        .flat_map(|(dir, files)| {
            if stop_flag.load(Ordering::Relaxed) { return Vec::new(); }

            // reuse cached albums when this dir's mtime hasn't changed.
            let reused = prev.and_then(|p| {
                let cur = dir_mtime(dir)?;
                let was = p.dir_mtimes.get(dir).copied()?;
                if cur == was { prev_by_dir.get(dir).cloned() } else { None }
            });

            let (albums, was_reused) = match reused {
                Some(cached) => (cached, true),
                None => {
                    // cover art is read separately below, so the per-track pass
                    // skips it: every file of an album carries the same picture
                    // and grouping keeps only one of them.
                    let tracks: Vec<_> = files.par_iter().map(|p| read_file_metadata_light(p)).collect();
                    let mut albums = group_into_albums(dir, &tracks);
                    for a in &mut albums {
                        a.cover_url = a.track_paths.iter().find_map(|p| cover_for(p));
                    }
                    (albums, false)
                }
            };

            // only stream freshly-read directories. on an incremental rescan the
            // ui already holds the reused (unchanged) albums, so re-streaming them
            // would just churn the node list for no benefit.
            if !was_reused {
                let fc = files_found.fetch_add(files.len(), Ordering::Relaxed) + files.len();
                let _ = progress_tx.try_send(ScanProgress {
                    files_found: fc,
                    dirs_visited: dirs_total,
                    current_dir: dir.clone(),
                    new_albums: albums.clone(),
                });
            }
            albums
        })
        .collect();

    let mut result = LibraryScanResult { albums, dirs: walk.dirs, dir_mtimes: HashMap::new() };
    let _ = dirs_visited; // counter is only used for live progress messages

    result.albums.sort_by(|a, b| {
        let a_pin = settings.pinned_paths.contains(&a.dir);
        let b_pin = settings.pinned_paths.contains(&b.dir);
        if a_pin != b_pin { return b_pin.cmp(&a_pin); }
        a.album_artist.to_lowercase().cmp(&b.album_artist.to_lowercase())
            .then(a.album.to_lowercase().cmp(&b.album.to_lowercase()))
            .then(a.id.cmp(&b.id))
    });
    result.dirs.sort();
    result.dirs.dedup();

    // snapshot directory mtimes so needs_rescan() can detect changes in o(n_dirs)
    // without re-walking the filesystem tree on the next startup.
    for dir in &result.dirs {
        if let Some(secs) = dir_mtime(dir) {
            result.dir_mtimes.insert(dir.clone(), secs);
        }
    }

    result
}

/// phase-1 walk: discover directories and the audio files they contain.
fn walk_dir(
    dir: &Path,
    settings: &LibrarySettings,
    stop_flag: &Arc<AtomicBool>,
    progress_tx: &std::sync::mpsc::SyncSender<ScanProgress>,
    dirs_visited: &Arc<AtomicUsize>,
    walk: &mut WalkAccum,
) {
    if stop_flag.load(Ordering::Relaxed) { return; }
    if settings.ignored_folders.iter().any(|ig| dir.starts_with(ig) || dir == ig) { return; }

    let visited = dirs_visited.fetch_add(1, Ordering::Relaxed) + 1;
    let _ = progress_tx.try_send(ScanProgress {
        files_found: 0,
        dirs_visited: visited,
        current_dir: dir.to_path_buf(),
        new_albums: vec![],
    });

    let entries: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(it) => it.flatten().map(|e| e.path()).collect(),
        Err(_) => return,
    };

    let mut audio_files: Vec<PathBuf> = entries.iter()
        .filter(|p| p.is_file() && is_audio_file(p))
        .cloned().collect();
    let mut sub_dirs: Vec<PathBuf> = entries.iter()
        .filter(|p| p.is_dir())
        .cloned().collect();
    audio_files.sort();
    sub_dirs.sort();

    // record this dir if it has audio or sub-directories (so navigation works).
    // empty leaf directories are skipped.
    if !audio_files.is_empty() {
        walk.dirs.push(dir.to_path_buf());
        walk.audio_dirs.push((dir.to_path_buf(), audio_files));
    } else if !sub_dirs.is_empty() {
        walk.dirs.push(dir.to_path_buf());
    }

    for sub in &sub_dirs {
        if stop_flag.load(Ordering::Relaxed) { break; }
        walk_dir(sub, settings, stop_flag, progress_tx, dirs_visited, walk);
    }
}

/// build a stable unique id for an album from its directory and a discriminator
/// (album tag, or `single:<filename>` for an untagged standalone track).
fn album_id(dir: &Path, key: &str) -> String {
    // U+001F (unit separator) cannot appear in a path or tag, so this is unambiguous.
    format!("{}\u{1f}{}", dir.to_string_lossy(), key)
}

fn group_into_albums(dir: &Path, tracks: &[crate::file_player::LocalTrack]) -> Vec<LibraryAlbum> {
    use std::collections::BTreeMap;
    let mut map: BTreeMap<String, LibraryAlbum> = BTreeMap::new();
    let mut singles: Vec<LibraryAlbum> = Vec::new();

    for t in tracks {
        // an untagged track is its own single - never merged with other untagged
        // tracks that happen to share a directory (e.g. loose files in Music/).
        if t.album.trim().is_empty() {
            let file_key = t.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let name = if t.title.trim().is_empty() {
                t.path.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string()
            } else { t.title.clone() };
            singles.push(LibraryAlbum {
                id: album_id(dir, &format!("single:{}", file_key)),
                dir: dir.to_path_buf(),
                album: name,
                album_artist: if !t.album_artist.is_empty() { t.album_artist.clone() } else { t.artist.clone() },
                year: t.year.clone(),
                cover_url: t.cover_art_path.clone(),
                track_paths: vec![t.path.clone()],
                is_single: true,
            });
            continue;
        }

        // tracks sharing an album tag in the same directory form one album.
        let key = t.album.clone();
        let entry = map.entry(key.clone()).or_insert_with(|| LibraryAlbum {
            id: album_id(dir, &key),
            dir: dir.to_path_buf(),
            album: t.album.clone(),
            album_artist: if !t.album_artist.is_empty() { t.album_artist.clone() } else { t.artist.clone() },
            year: t.year.clone(),
            cover_url: t.cover_art_path.clone(),
            track_paths: Vec::new(),
            is_single: false,
        });
        if entry.cover_url.is_none() {
            entry.cover_url = t.cover_art_path.clone();
        }
        entry.track_paths.push(t.path.clone());
    }

    let mut out: Vec<LibraryAlbum> = map.into_values().collect();
    out.extend(singles);
    out
}

// helpers

/// return the os default music directory, or none if unavailable.
pub fn default_music_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        if let Ok(docs) = std::env::var("USERPROFILE") {
            let p = PathBuf::from(&docs).join("Music");
            if p.exists() { return Some(p); }
        }
        // SHGetKnownFolderPath for FOLDERID_Music via winapi would be ideal but
        // the env var is reliable on all supported windows versions.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file_player::LocalTrack;

    fn track(path: &str, album: &str, artist: &str) -> LocalTrack {
        LocalTrack {
            path: PathBuf::from(path),
            title: path.rsplit('/').next().unwrap_or(path).to_string(),
            artist: artist.to_string(),
            album: album.to_string(),
            album_artist: artist.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn untagged_tracks_become_distinct_singles() {
        let dir = Path::new("/music");
        let tracks = vec![
            track("/music/a.mp3", "", "Artist A"),
            track("/music/b.mp3", "", "Artist B"),
        ];
        let albums = group_into_albums(dir, &tracks);
        assert_eq!(albums.len(), 2, "two untagged files must be two singles");
        assert!(albums.iter().all(|a| a.is_single));
        assert_ne!(albums[0].id, albums[1].id, "singles need distinct ids");
        // each single owns exactly its own track.
        assert!(albums.iter().all(|a| a.track_paths.len() == 1));
    }

    #[test]
    fn differently_tagged_tracks_in_one_dir_are_separate_albums() {
        let dir = Path::new("/music");
        let tracks = vec![
            track("/music/x.mp3", "Album X", "Artist"),
            track("/music/y.mp3", "Album Y", "Artist"),
        ];
        let albums = group_into_albums(dir, &tracks);
        assert_eq!(albums.len(), 2);
        assert_ne!(albums[0].id, albums[1].id);
    }

    #[test]
    fn same_album_tag_groups_together() {
        let dir = Path::new("/music/album");
        let tracks = vec![
            track("/music/album/1.mp3", "One Album", "Artist"),
            track("/music/album/2.mp3", "One Album", "Artist"),
        ];
        let albums = group_into_albums(dir, &tracks);
        assert_eq!(albums.len(), 1);
        assert_eq!(albums[0].track_paths.len(), 2);
        assert!(!albums[0].is_single);
    }

    #[test]
    fn ids_are_stable_across_runs() {
        let dir = Path::new("/music");
        let tracks = vec![track("/music/a.mp3", "", "A")];
        let first = group_into_albums(dir, &tracks);
        let second = group_into_albums(dir, &tracks);
        assert_eq!(first[0].id, second[0].id, "ids must be deterministic");
    }

    #[test]
    fn desc_modes_reverse_only_the_primary_key() {
        use std::cmp::Ordering;
        let (a, b, c) = (("abba", "x", ""), ("zed", "a", ""), ("abba", "y", ""));
        assert_eq!(compare_albums("artist", a, b), Ordering::Less);
        assert_eq!(compare_albums("artist_desc", a, b), Ordering::Greater);
        assert_eq!(compare_albums("album", a, b), Ordering::Greater);
        assert_eq!(compare_albums("album_desc", a, b), Ordering::Less);
        // ties on the reversed key still fall back to ascending.
        assert_eq!(compare_albums("artist_desc", a, c), Ordering::Less);
    }
}
