use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheEntry {
    pub disc_id:      String,
    pub track_number: u8,
    pub lrclib_id:    u64,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct CacheFile {
    entries: Vec<CacheEntry>,
}

pub struct LyricCache {
    path:    PathBuf,
    entries: Vec<CacheEntry>,
}

impl LyricCache {
    pub fn load() -> Self {
        let path = cache_path();
        let entries = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(s) => serde_json::from_str::<CacheFile>(&s)
                    .map(|f| f.entries)
                    .unwrap_or_default(),
                Err(e) => {
                    eprintln!("[lyric_cache] failed to read cache: {}", e);
                    Vec::new()
                }
            }
        } else {
            Vec::new()
        };
        eprintln!(
            "[lyric_cache] loaded {} entry/entries from {}",
            entries.len(),
            path.display()
        );
        Self { path, entries }
    }

    pub fn lookup(&self, disc_id: &str, track_number: u8) -> Option<u64> {
        self.entries
            .iter()
            .find(|e| e.disc_id == disc_id && e.track_number == track_number)
            .map(|e| e.lrclib_id)
    }

    pub fn insert(&mut self, disc_id: &str, track_number: u8, lrclib_id: u64) {
        if let Some(e) = self
            .entries
            .iter_mut()
            .find(|e| e.disc_id == disc_id && e.track_number == track_number)
        {
            e.lrclib_id = lrclib_id;
        } else {
            self.entries.push(CacheEntry {
                disc_id:      disc_id.to_string(),
                track_number,
                lrclib_id,
            });
        }
        self.save();
    }

    fn save(&self) {
        if let Some(parent) = self.path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("[lyric_cache] could not create cache dir: {}", e);
                return;
            }
        }
        let f = CacheFile { entries: self.entries.clone() };
        match serde_json::to_string_pretty(&f) {
            Ok(s) => {
                if let Err(e) = std::fs::write(&self.path, s) {
                    eprintln!("[lyric_cache] failed to write cache: {}", e);
                }
            }
            Err(e) => eprintln!("[lyric_cache] failed to serialise cache: {}", e),
        }
    }
}

fn cache_path() -> PathBuf {
    config_dir().join("kanae").join("lyric_cache.json")
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
