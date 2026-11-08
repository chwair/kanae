use std::collections::HashMap;
use std::path::PathBuf;

use crate::library::{LibraryScanResult, LibrarySettings};
use super::TuiLibraryNode;

/// results past this are not materialised; the header says "N+" instead.
pub(super) const MAX_RESULTS: usize = 500;

/// the library search bar and its current results.
#[derive(Default)]
pub(super) struct Search {
    pub query:   String,
    /// true while keystrokes go into the query rather than the results.
    pub editing: bool,
    pub results: Vec<TuiLibraryNode>,
    /// total matches, which can exceed `results.len()`.
    pub total:   usize,
    computed:    Option<(String, u64)>,
}

impl Search {
    pub fn new() -> Self {
        Self { editing: true, ..Default::default() }
    }

    /// recompute results when the query or the library changed.
    pub fn refresh(&mut self, index: &mut SearchIndex, scan: Option<&LibraryScanResult>,
                   settings: &LibrarySettings, gen: u64) -> bool {
        if self.computed.as_ref().is_some_and(|(q, g)| *q == self.query && *g == gen) {
            return false;
        }
        self.computed = Some((self.query.clone(), gen));
        let Some(scan) = scan else {
            self.results.clear();
            self.total = 0;
            return true;
        };
        index.rebuild_if_stale(scan, settings, gen);
        let (results, total) = index.query(&self.query, scan, settings);
        self.results = results;
        self.total = total;
        true
    }
}

struct AlbumHay {
    title:  String,
    artist: String,
    /// year, folder name and track file names.
    rest:   String,
}

/// lowercased (and romanized, for japanese) text per album and folder, built
/// lazily the first time a search runs and again whenever the library changes.
#[derive(Default)]
pub(super) struct SearchIndex {
    gen:     Option<u64>,
    albums:  Vec<AlbumHay>,
    folders: Vec<(PathBuf, String)>,
    /// romanizations survive rebuilds; kakasi is the slow part.
    romaji:  HashMap<String, String>,
}

impl SearchIndex {
    fn fold(&mut self, s: &str) -> String {
        let lc = s.to_lowercase();
        if lc.is_ascii() { return lc; }
        // kakasi splits words unpredictably ("u tada hikaru"), so the romaji is
        // indexed both spaced and joined.
        let rom = self.romaji.entry(lc.clone())
            .or_insert_with(|| {
                let r = crate::romaji::romanize_line(&lc).to_lowercase();
                let joined: String = r.split_whitespace().collect();
                format!("{} {}", r, joined)
            })
            .clone();
        if rom.starts_with(lc.as_str()) { lc } else { format!("{} {}", lc, rom) }
    }

    fn rebuild_if_stale(&mut self, scan: &LibraryScanResult, settings: &LibrarySettings, gen: u64) {
        if self.gen == Some(gen) { return; }
        self.gen = Some(gen);
        let mut albums = Vec::with_capacity(scan.albums.len());
        for a in &scan.albums {
            let title  = self.fold(&a.album);
            let artist = self.fold(&a.album_artist);
            let mut rest = a.year.clone();
            if let Some(n) = a.dir.file_name().and_then(|n| n.to_str()) {
                rest.push(' ');
                rest.push_str(&n.to_lowercase());
            }
            for p in &a.track_paths {
                if let Some(stem) = p.file_stem().and_then(|n| n.to_str()) {
                    rest.push(' ');
                    rest.push_str(&stem.to_lowercase());
                }
            }
            albums.push(AlbumHay { title, artist, rest });
        }
        self.albums = albums;

        let mut folders = Vec::new();
        if !settings.merge_all_folders {
            for d in &scan.dirs {
                if settings.ignored_folders.contains(d) || settings.search_paths.contains(d) { continue; }
                if let Some(n) = d.file_name().and_then(|n| n.to_str()) {
                    let hay = self.fold(n);
                    folders.push((d.clone(), hay));
                }
            }
        }
        self.folders = folders;
    }

    /// every whitespace-separated term must appear somewhere. albums whose
    /// title matches come first, then artist matches, then the rest (track or
    /// folder names), each group in the configured sort order.
    fn query(&self, q: &str, scan: &LibraryScanResult, settings: &LibrarySettings)
        -> (Vec<TuiLibraryNode>, usize)
    {
        let q = q.trim().to_lowercase();
        let terms: Vec<&str> = q.split_whitespace().collect();
        if terms.is_empty() { return (vec![], 0); }

        let mut hits: Vec<(u8, usize)> = Vec::new();
        for (i, h) in self.albums.iter().enumerate() {
            let all = terms.iter().all(|t| h.title.contains(t) || h.artist.contains(t) || h.rest.contains(t));
            if !all { continue; }
            let rank = if h.title.contains(q.as_str()) { 0 }
                else if h.artist.contains(q.as_str()) { 1 }
                else { 2 };
            hits.push((rank, i));
        }
        let folder_hits: Vec<&(PathBuf, String)> = self.folders.iter()
            .filter(|(_, hay)| terms.iter().all(|t| hay.contains(t)))
            .collect();

        let total = hits.len() + folder_hits.len();
        let mode = &settings.album_sort;
        hits.sort_by(|(ra, a), (rb, b)| {
            let (x, y) = (&scan.albums[*a], &scan.albums[*b]);
            ra.cmp(rb).then_with(|| crate::library::compare_albums(
                mode,
                (&x.album_artist, &x.album, &x.year),
                (&y.album_artist, &y.album, &y.year),
            ))
        });

        let mut out: Vec<TuiLibraryNode> = Vec::new();
        for (path, _) in folder_hits.iter().take(MAX_RESULTS / 5) {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("").to_string();
            out.push(TuiLibraryNode::Folder { path: path.clone(), name });
        }
        for (_, i) in hits.iter() {
            if out.len() >= MAX_RESULTS { break; }
            out.push(TuiLibraryNode::Album { album: scan.albums[*i].clone() });
        }
        (out, total)
    }
}
