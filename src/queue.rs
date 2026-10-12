//! in-memory play queue shared by the gui and tui frontends.
//! not persisted; playback returns to the interrupted album once it drains.

use std::path::PathBuf;

use crate::file_player::LocalTrack;

/// one queued song. only local files can be queued; a cd track has no path
/// that survives the disc being swapped.
#[derive(Clone)]
pub struct QueueEntry {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    /// pre-formatted mm:ss, so both frontends render the same string.
    pub duration: String,
}

impl QueueEntry {
    pub fn from_track(t: &LocalTrack) -> Self {
        Self {
            path: t.path.clone(),
            title: if t.title.is_empty() {
                t.path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
            } else {
                t.title.clone()
            },
            artist: t.artist.clone(),
            duration: t.display_duration(),
        }
    }
}

#[derive(Default)]
pub struct Queue {
    entries: Vec<QueueEntry>,
}

impl Queue {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn entries(&self) -> &[QueueEntry] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// append every audio file found under `paths`. accepts files and folders
    /// alike, so an album, a folder or a single song all go through here.
    /// returns how many entries were added.
    pub fn extend_from_paths(&mut self, paths: &[String]) -> usize {
        let tracks = crate::file_player::collect_files_from_paths(paths);
        let added = tracks.len();
        self.entries.extend(tracks.iter().map(QueueEntry::from_track));
        added
    }

    pub fn pop_front(&mut self) -> Option<QueueEntry> {
        if self.entries.is_empty() { None } else { Some(self.entries.remove(0)) }
    }

    /// remove and return one entry, ignoring an out-of-range index.
    pub fn remove(&mut self, index: usize) -> Option<QueueEntry> {
        if index < self.entries.len() { Some(self.entries.remove(index)) } else { None }
    }

    /// move an entry to another position, clamping the destination. used by the
    /// gui's drag-and-drop and the tui's move up/down keys.
    pub fn move_entry(&mut self, from: usize, to: usize) {
        if from >= self.entries.len() || from == to {
            return;
        }
        let to = to.min(self.entries.len() - 1);
        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// the queue as json for the gui, mirroring `album_tracks_json`.
    pub fn to_json(&self) -> String {
        let rows: Vec<serde_json::Value> = self.entries.iter()
            .map(|e| serde_json::json!({
                "title":    e.title,
                "artist":   e.artist,
                "duration": e.duration,
                "path":     e.path.to_string_lossy(),
            }))
            .collect();
        serde_json::to_string(&rows).unwrap_or_else(|_| "[]".to_string())
    }
}
