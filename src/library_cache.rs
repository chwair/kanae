/// Persistence for library scan results and settings.
///
/// Stored as JSON in the OS app-data directory so they survive restarts
/// without a full rescan.  On the next boot we use the cache and only do an
/// incremental check for new / removed files.
use std::path::{Path, PathBuf};
use crate::library::{LibraryScanResult, LibrarySettings};

// ─── TUI-specific settings ────────────────────────────────────────────────────

/// How album-art images are rendered in the TUI.
#[derive(Clone, Copy, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TuiImageMethod {
    /// Automatically detect the best supported protocol at startup.
    #[default]
    Auto,
    /// Unicode half-block / chafa character-art (works everywhere; uses chafa when available).
    Halfblocks,
    /// Sixel graphics (xterm, mlterm, etc.).
    Sixel,
    /// Kitty graphics protocol.
    Kitty,
    /// iTerm2 inline images (iTerm2, WezTerm, etc.).
    Iterm2,
    /// Disable album-art display entirely.
    None,
}

impl TuiImageMethod {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto       => "Auto",
            Self::Halfblocks => "Halfblocks",
            Self::Sixel      => "Sixel",
            Self::Kitty      => "Kitty",
            Self::Iterm2     => "iTerm2",
            Self::None       => "None",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Auto       => Self::Halfblocks,
            Self::Halfblocks => Self::Sixel,
            Self::Sixel      => Self::Kitty,
            Self::Kitty      => Self::Iterm2,
            Self::Iterm2     => Self::None,
            Self::None       => Self::Auto,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            Self::Auto       => Self::None,
            Self::Halfblocks => Self::Auto,
            Self::Sixel      => Self::Halfblocks,
            Self::Kitty      => Self::Sixel,
            Self::Iterm2     => Self::Kitty,
            Self::None       => Self::Iterm2,
        }
    }
}

/// Which icon style to use in the TUI.
#[derive(Clone, Copy, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TuiIconSet {
    /// Nerd Fonts patched font icons (requires a Nerd Fonts terminal font).
    #[default]
    NerdFonts,
    /// Standard Unicode symbols (works on most terminals).
    Unicode,
    /// Plain ASCII text fallback (works on any terminal).
    PlainText,
}

impl TuiIconSet {
    pub fn label(self) -> &'static str {
        match self {
            Self::NerdFonts => "Nerd Fonts",
            Self::Unicode   => "Unicode",
            Self::PlainText => "Plain Text",
        }
    }
    pub fn next(self) -> Self {
        match self {
            Self::NerdFonts => Self::Unicode,
            Self::Unicode   => Self::PlainText,
            Self::PlainText => Self::NerdFonts,
        }
    }
    pub fn prev(self) -> Self {
        match self {
            Self::NerdFonts => Self::PlainText,
            Self::Unicode   => Self::NerdFonts,
            Self::PlainText => Self::Unicode,
        }
    }
}

/// TUI-specific persistent settings.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct TuiSettings {
    #[serde(default)]
    pub image_method: TuiImageMethod,
    #[serde(default)]
    pub icon_set: TuiIconSet,
}

pub fn load_tui_settings() -> TuiSettings {
    let path = ensure_dir().join("tui_settings.json");
    std::fs::read_to_string(&path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_tui_settings(s: &TuiSettings) {
    if let Ok(json) = serde_json::to_string_pretty(s) {
        let _ = std::fs::write(ensure_dir().join("tui_settings.json"), json);
    }
}

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

/// Returns true if the cached scan result is out of date and a rescan is needed.
///
/// Fast O(n_dirs) check: each directory in the cache is stat()ed and its current
/// mtime is compared against the mtime stored at scan time.  A changed mtime means
/// files were added or removed from that directory.  If the stored mtime map is
/// absent (old cache format) the check falls back to verifying directories exist.
pub fn needs_rescan(settings: &LibrarySettings, result: &LibraryScanResult) -> bool {
    // Any search path that produced no dirs in the cache needs scanning.
    for root in &settings.search_paths {
        let covered = result.dirs.iter().any(|d| d.starts_with(root));
        if !covered { return true; }
    }

    // Check every cached directory: still exists, and mtime is unchanged.
    for dir in &result.dirs {
        match std::fs::metadata(dir) {
            Err(_) => return true, // directory was removed
            Ok(meta) => {
                if let Ok(mtime) = meta.modified() {
                    let now_secs = mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    if let Some(&stored_secs) = result.dir_mtimes.get(dir) {
                        if now_secs != stored_secs { return true; }
                    }
                    // If dir_mtimes is absent (old cache), just confirm the dir exists.
                }
            }
        }
    }
    false
}
