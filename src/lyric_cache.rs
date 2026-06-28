use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Default maximum combined entries (LRC + no-lyrics) kept in the cache.
pub const MAX_ENTRIES: usize = 100;

// ─── Entry types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LrcEntry {
    pub key:         String,
    /// Raw LRC text (the `[mm:ss.xx] line` format returned by lrclib).
    pub lrc_text:    String,
    /// Unix milliseconds of last access, used for LRU eviction.
    pub accessed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoLyricsEntry {
    pub key:         String,
    pub accessed_ms: u64,
}

// ─── Serialised store ─────────────────────────────────────────────────────────

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    #[serde(default)]
    lrc_entries:       Vec<LrcEntry>,
    #[serde(default)]
    no_lyrics_entries: Vec<NoLyricsEntry>,
}

// ─── Public API ───────────────────────────────────────────────────────────────

pub struct LyricContentCache {
    path:              PathBuf,
    lrc_entries:       Vec<LrcEntry>,
    no_lyrics_entries: Vec<NoLyricsEntry>,
}

impl LyricContentCache {
    pub fn load() -> Self {
        let path = cache_path();
        let file: CacheFile = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default()
        } else {
            CacheFile::default()
        };
        Self {
            path,
            lrc_entries:       file.lrc_entries,
            no_lyrics_entries: file.no_lyrics_entries,
        }
    }

    // ── Counts ────────────────────────────────────────────────────────────

    pub fn lrc_count(&self) -> usize       { self.lrc_entries.len() }
    pub fn no_lyrics_count(&self) -> usize { self.no_lyrics_entries.len() }

    // ── Lookup ────────────────────────────────────────────────────────────

    /// Returns the raw LRC text if a previous successful fetch is cached.
    pub fn get_lrc(&mut self, key: &str) -> Option<String> {
        let now = unix_ms();
        if let Some(e) = self.lrc_entries.iter_mut().find(|e| e.key == key) {
            e.accessed_ms = now;
            Some(e.lrc_text.clone())
        } else {
            None
        }
    }

    /// Returns `true` if we previously determined this track has no lyrics.
    pub fn has_no_lyrics(&mut self, key: &str) -> bool {
        let now = unix_ms();
        if let Some(e) = self.no_lyrics_entries.iter_mut().find(|e| e.key == key) {
            e.accessed_ms = now;
            true
        } else {
            false
        }
    }

    // ── Insert ────────────────────────────────────────────────────────────

    /// Cache a successful LRC fetch.  `limit_disabled` comes from `LibrarySettings`.
    pub fn insert_lrc(&mut self, key: &str, lrc_text: &str, limit_disabled: bool) {
        // Update existing entry if present.
        if let Some(e) = self.lrc_entries.iter_mut().find(|e| e.key == key) {
            e.lrc_text    = lrc_text.to_string();
            e.accessed_ms = unix_ms();
            return;
        }
        self.lrc_entries.push(LrcEntry {
            key:         key.to_string(),
            lrc_text:    lrc_text.to_string(),
            accessed_ms: unix_ms(),
        });
        if !limit_disabled {
            self.evict_to_limit();
        }
    }

    /// Cache a "no lyrics" determination for this track.
    pub fn insert_no_lyrics(&mut self, key: &str, limit_disabled: bool) {
        if self.no_lyrics_entries.iter().any(|e| e.key == key) {
            return;
        }
        self.no_lyrics_entries.push(NoLyricsEntry {
            key:         key.to_string(),
            accessed_ms: unix_ms(),
        });
        if !limit_disabled {
            self.evict_to_limit();
        }
    }

    // ── Purge ─────────────────────────────────────────────────────────────

    /// Remove all cached LRC entries.
    pub fn purge_lrc(&mut self) {
        self.lrc_entries.clear();
    }

    /// Remove all "no lyrics" entries.
    pub fn purge_no_lyrics(&mut self) {
        self.no_lyrics_entries.clear();
    }

    // ── Persist ───────────────────────────────────────────────────────────

    pub fn save(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = CacheFile {
            lrc_entries:       self.lrc_entries.clone(),
            no_lyrics_entries: self.no_lyrics_entries.clone(),
        };
        if let Ok(json) = serde_json::to_string_pretty(&file) {
            let _ = std::fs::write(&self.path, json);
        }
    }

    // ── LRU eviction ──────────────────────────────────────────────────────

    fn evict_to_limit(&mut self) {
        let total = self.lrc_entries.len() + self.no_lyrics_entries.len();
        if total <= MAX_ENTRIES { return; }

        // Evict oldest entries across both lists until we are at the limit.
        let overflow = total - MAX_ENTRIES;
        for _ in 0..overflow {
            let oldest_lrc = self.lrc_entries.iter()
                .enumerate()
                .min_by_key(|(_, e)| e.accessed_ms)
                .map(|(i, e)| (i, e.accessed_ms));
            let oldest_no = self.no_lyrics_entries.iter()
                .enumerate()
                .min_by_key(|(_, e)| e.accessed_ms)
                .map(|(i, e)| (i, e.accessed_ms));

            match (oldest_lrc, oldest_no) {
                (Some((li, lt)), Some((ni, nt))) => {
                    if lt <= nt { self.lrc_entries.remove(li); }
                    else        { self.no_lyrics_entries.remove(ni); }
                }
                (Some((li, _)), None) => { self.lrc_entries.remove(li); }
                (None, Some((ni, _))) => { self.no_lyrics_entries.remove(ni); }
                (None, None)          => break,
            }
        }
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Build the lookup key for a CD track.
pub fn cd_key(disc_id: &str, track_num: i32) -> String {
    format!("cd:{}:{}", disc_id, track_num)
}

/// Build the lookup key for a file track (prefer path; fall back to title+artist).
pub fn file_key(path: &str, title: &str, artist: &str) -> String {
    if !path.is_empty() {
        format!("file:{}", path)
    } else {
        format!("track:{}:{}", title.trim().to_lowercase(), artist.trim().to_lowercase())
    }
}

fn cache_path() -> PathBuf {
    config_dir().join("kanae").join("lyric_content_cache.json")
}

fn config_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    if let Ok(v) = std::env::var("APPDATA") {
        return PathBuf::from(v);
    }

    #[cfg(target_os = "macos")]
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home)
            .join("Library")
            .join("Application Support");
    }

    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        return PathBuf::from(xdg);
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".config");
    }

    PathBuf::from(".")
}

