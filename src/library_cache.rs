/// Persistence for library scan results and settings.
///
/// Stored as JSON in the OS app-data directory so they survive restarts
/// without a full rescan.  On the next boot we use the cache and only do an
/// incremental check for new / removed files.
use std::path::{Path, PathBuf};
use crate::library::{LibraryScanResult, LibrarySettings};

fn cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    let base = std::env::var("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());
    #[cfg(target_os = "macos")]
    let base = {
        let home = std::env::var("HOME").unwrap_or_default();
        PathBuf::from(home).join("Library/Application Support")
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let base = std::env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_default();
            PathBuf::from(home).join(".cache")
        });
    base.join("Kanae")
}

fn ensure_dir() -> PathBuf {
    let dir = cache_dir();
    let _ = std::fs::create_dir_all(&dir);
    dir
}

pub fn load_settings() -> LibrarySettings {
    let path = ensure_dir().join("library_settings.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_settings(s: &LibrarySettings) {
    if let Ok(json) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(ensure_dir().join("library_settings.json"), json);
    }
}

pub fn load_cache() -> Option<LibraryScanResult> {
    let path = ensure_dir().join("library_cache.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
}

pub fn save_cache(r: &LibraryScanResult) {
    if let Ok(json) = serde_json::to_string(r) {
        let _ = std::fs::write(ensure_dir().join("library_cache.json"), json);
    }
}

/// True if any file in `result` no longer exists on disk, or any directory in
/// the search paths has an mtime newer than the youngest cached mtime.
/// Fast O(n_dirs) check so the UI can decide whether to rescan silently.
pub fn needs_rescan(settings: &LibrarySettings, result: &LibraryScanResult) -> bool {
    // Quick check: do any dirs have new mtimes?
    for dir in &result.dirs {
        if let Ok(meta) = std::fs::metadata(dir) {
            if let Ok(modified) = meta.modified() {
                // Check each entry in the dir.
                if let Ok(entries) = std::fs::read_dir(dir) {
                    for e in entries.flatten() {
                        if let Ok(em) = e.metadata() {
                            if let Ok(em_mod) = em.modified() {
                                if em_mod > modified {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // Dir was removed.
            return true;
        }
    }
    false
}
