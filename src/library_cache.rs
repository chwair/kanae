/// persistence for library scan results and settings, so restarts skip a full rescan.
use std::path::PathBuf;
use crate::library::{LibraryScanResult, LibrarySettings};

// tui-specific settings

/// how album-art images are rendered in the tui.
#[derive(Clone, Copy, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TuiImageMethod {
    /// automatically detect the best supported protocol at startup.
    #[default]
    Auto,
    /// unicode half-block / chafa character-art (works everywhere; uses chafa when available).
    Halfblocks,
    /// sixel graphics (xterm, mlterm, etc.).
    Sixel,
    /// kitty graphics protocol.
    Kitty,
    /// iterm2 inline images (iterm2, wezterm, etc.).
    Iterm2,
    /// disable album-art display entirely.
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

/// which icon style to use in the tui.
#[derive(Clone, Copy, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TuiIconSet {
    /// nerd fonts patched font icons (requires a nerd fonts terminal font).
    #[default]
    NerdFonts,
    /// standard unicode symbols (works on most terminals).
    Unicode,
    /// plain ascii text fallback (works on any terminal).
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

/// step `cur` by `step` places through `all`, wrapping at either end.
fn cycle<T: Copy + PartialEq>(all: &[T], cur: T, step: isize) -> T {
    let i = all.iter().position(|v| *v == cur).unwrap_or(0) as isize;
    let n = all.len() as isize;
    all[(i + step).rem_euclid(n) as usize]
}

/// colour scheme for the tui.
#[derive(Clone, Copy, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TuiTheme {
    /// the terminal's own palette and default colours, so light and dark
    /// terminal schemes both keep their contrast.
    #[default]
    Terminal,
    /// kanae's fixed dark palette (mirrors the gui), painted over the terminal background.
    Kanae,
}

impl TuiTheme {
    const ALL: [Self; 2] = [Self::Terminal, Self::Kanae];
    pub fn label(self) -> &'static str {
        match self {
            Self::Terminal => "Terminal",
            Self::Kanae    => "Kanae Dark",
        }
    }
    pub fn next(self) -> Self { cycle(&Self::ALL, self, 1) }
    pub fn prev(self) -> Self { cycle(&Self::ALL, self, -1) }
}

/// highlight colour for the tui, taken from the terminal's ansi palette.
#[derive(Clone, Copy, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TuiAccent {
    /// no colour, emphasis by weight only (the gui's neutral accent).
    #[default]
    Mono,
    Blue,
    Cyan,
    Green,
    Yellow,
    Magenta,
    Red,
}

impl TuiAccent {
    const ALL: [Self; 7] = [Self::Mono, Self::Blue, Self::Cyan, Self::Green, Self::Yellow, Self::Magenta, Self::Red];
    pub fn label(self) -> &'static str {
        match self {
            Self::Blue    => "Blue",
            Self::Cyan    => "Cyan",
            Self::Green   => "Green",
            Self::Yellow  => "Yellow",
            Self::Magenta => "Magenta",
            Self::Red     => "Red",
            Self::Mono    => "Monochrome",
        }
    }
    pub fn next(self) -> Self { cycle(&Self::ALL, self, 1) }
    pub fn prev(self) -> Self { cycle(&Self::ALL, self, -1) }
}

/// how the library is laid out in the tui.
#[derive(Clone, Copy, PartialEq, Debug, Default, serde::Serialize, serde::Deserialize)]
pub enum TuiLibraryView {
    /// cover tiles, like the gui.
    #[default]
    Grid,
    /// one row per item.
    List,
}

impl TuiLibraryView {
    const ALL: [Self; 2] = [Self::Grid, Self::List];
    pub fn label(self) -> &'static str {
        match self {
            Self::Grid => "Grid",
            Self::List => "List",
        }
    }
    pub fn next(self) -> Self { cycle(&Self::ALL, self, 1) }
    pub fn prev(self) -> Self { cycle(&Self::ALL, self, -1) }
}

fn default_true() -> bool { true }

/// tui-specific persistent settings.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct TuiSettings {
    #[serde(default)]
    pub image_method: TuiImageMethod,
    #[serde(default)]
    pub icon_set: TuiIconSet,
    #[serde(default)]
    pub theme: TuiTheme,
    /// stored under a new key: the first builds wrote a blue default to
    /// "accent" that nobody chose, which this leaves behind.
    #[serde(default, rename = "accent_color")]
    pub accent: TuiAccent,
    #[serde(default)]
    pub library_view: TuiLibraryView,
    /// draw album covers on grid tiles (off falls back to icons).
    #[serde(default = "default_true")]
    pub grid_covers: bool,
    /// show the key hint line under the controls.
    #[serde(default = "default_true")]
    pub show_hints: bool,
    /// show the now-playing sidebar (cover, details, lyrics).
    #[serde(default = "default_true")]
    pub show_sidebar: bool,
}

impl Default for TuiSettings {
    fn default() -> Self {
        Self {
            image_method: TuiImageMethod::default(),
            icon_set:     TuiIconSet::default(),
            theme:        TuiTheme::default(),
            accent:       TuiAccent::default(),
            library_view: TuiLibraryView::default(),
            grid_covers:  true,
            show_hints:   true,
            show_sidebar: true,
        }
    }
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

/// persistent directory for extracted album-art images.
/// cover art embedded in audio files is written here (keyed by a hash of the
/// source path) so the `cover_url` stored in the scan cache survives restarts.
/// using the temp dir instead would leave reused/cached albums pointing at files
/// the os has since cleared, showing missing covers.
pub fn cover_cache_dir() -> PathBuf {
    let dir = cache_dir().join("covers");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// where the cover blurhashes are persisted (see cover_blur.rs).
#[cfg(feature = "gui")]
pub fn blurhash_cache_path() -> PathBuf {
    ensure_dir().join("blurhash_cache.json")
}

/// true if the cached result predates the album `id` field (every album has an
/// empty id). such a cache must be fully re-scanned - not incrementally reused -
/// so albums get stable ids and untagged singles split apart correctly.
pub fn is_legacy_cache(result: &LibraryScanResult) -> bool {
    !result.albums.is_empty() && result.albums.iter().all(|a| a.id.is_empty())
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

/// returns true if the cached scan result is out of date and a rescan is needed.
/// fast o(n_dirs) check: each directory in the cache is stat()ed and its current
/// mtime is compared against the mtime stored at scan time.  a changed mtime means
/// files were added or removed from that directory.  if the stored mtime map is
/// absent (old cache format) the check falls back to verifying directories exist.
pub fn needs_rescan(settings: &LibrarySettings, result: &LibraryScanResult) -> bool {
    dirs_changed(&settings.search_paths, &result.dirs, &result.dir_mtimes)
}

/// lightweight filesystem-change check used by both `needs_rescan` and the live
/// library watcher. operates on plain slices/maps so callers can clone just the
/// directory list and mtimes (not the whole album set) for off-thread polling.
/// returns true if: a search root is uncovered, a known dir was removed, or any
/// known dir's mtime changed (files added/removed/renamed inside it).
pub fn dirs_changed(
    search_paths: &[PathBuf],
    dirs: &[PathBuf],
    dir_mtimes: &std::collections::HashMap<PathBuf, u64>,
) -> bool {
    for root in search_paths {
        if root.exists() && !dirs.iter().any(|d| d.starts_with(root)) {
            return true;
        }
    }

    for dir in dirs {
        match std::fs::metadata(dir) {
            Err(_) => return true, // directory was removed
            Ok(meta) => {
                if let Ok(mtime) = meta.modified() {
                    let now_secs = mtime
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    if let Some(&stored_secs) = dir_mtimes.get(dir) {
                        if now_secs != stored_secs { return true; }
                    }
                    // if dir_mtimes is absent (old cache), just confirm the dir exists.
                }
            }
        }
    }
    false
}
