use std::path::{Path, PathBuf};

const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "ogg", "opus", "m4a", "aac", "wav", "aiff", "aif", "wma", "ape",
];

pub fn is_audio_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| AUDIO_EXTENSIONS.contains(&e.to_lowercase().as_str()))
        .unwrap_or(false)
}

#[derive(Clone, Debug, Default)]
pub struct LocalTrack {
    pub path: PathBuf,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    pub year: String,
    pub duration_secs: f64,
    pub cover_art_path: Option<String>,
}

impl LocalTrack {
    pub fn display_duration(&self) -> String {
        let total = self.duration_secs as u64;
        format!("{:02}:{:02}", total / 60, total % 60)
    }
}

/// Collect audio tracks from a mixed list of file and folder paths.
pub fn collect_files_from_paths(input_paths: &[String]) -> Vec<LocalTrack> {
    let mut tracks = Vec::new();
    for p in input_paths {
        let path = PathBuf::from(p);
        if path.is_dir() {
            let mut dir_tracks = collect_dir(&path);
            dir_tracks.sort_by(|a, b| a.path.cmp(&b.path));
            tracks.extend(dir_tracks);
        } else if path.is_file() && is_audio_file(&path) {
            tracks.push(read_file_metadata(&path));
        }
    }
    tracks
}

fn collect_dir(dir: &Path) -> Vec<LocalTrack> {
    let mut files: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && is_audio_file(&p) {
                files.push(p);
            }
        }
    }
    files.sort();
    files.iter().map(|f| read_file_metadata(f)).collect()
}

pub fn read_file_metadata(path: &Path) -> LocalTrack {
    use symphonia::core::{
        formats::FormatOptions,
        io::MediaSourceStream,
        meta::{MetadataOptions, StandardTagKey},
        probe::Hint,
    };

    let mut track = LocalTrack {
        path: path.to_path_buf(),
        title: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string(),
        ..Default::default()
    };

    // ── Symphonia: duration + text tags ──────────────────────────────────
    if let Ok(file) = std::fs::File::open(path) {
        let mss = MediaSourceStream::new(Box::new(file), Default::default());
        let mut hint = Hint::new();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            hint.with_extension(ext);
        }
        let meta_opts = MetadataOptions { limit_metadata_bytes: symphonia::core::meta::Limit::Maximum(4 * 1024 * 1024), ..Default::default() };
        if let Ok(mut probed) = symphonia::default::get_probe().format(
            &hint, mss, &FormatOptions::default(), &meta_opts,
        ) {
            // Duration from the default track's codec params.
            if let Some(t) = probed.format.default_track() {
                if let (Some(tb), Some(n_frames)) = (t.codec_params.time_base, t.codec_params.n_frames) {
                    let time = tb.calc_time(n_frames);
                    track.duration_secs = time.seconds as f64 + time.frac;
                }
            }

            // Consume top-level metadata (ID3, Vorbis Comment, etc.)
            let metadata = probed.format.metadata();
            let revision = metadata.current();
            if let Some(rev) = revision {
                for tag in rev.tags() {
                    let v = tag.value.to_string();
                    match tag.std_key {
                        Some(StandardTagKey::TrackTitle)   => track.title        = v,
                        Some(StandardTagKey::Artist)       => track.artist       = v,
                        Some(StandardTagKey::Album)        => track.album        = v,
                        Some(StandardTagKey::AlbumArtist)  => track.album_artist = v,
                        Some(StandardTagKey::Date)         => {
                            // Take just the year portion (first 4 chars).
                            track.year = v.chars().take(4).collect();
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    if track.album_artist.is_empty() {
        track.album_artist = track.artist.clone();
    }

    // ── Lofty: cover art only ────────────────────────────────────────────
    if let Some(tagged) = lofty::probe::Probe::open(path)
        .ok()
        .and_then(|p| p.read().ok())
    {
        use lofty::prelude::{Accessor, AudioFile, TaggedFileExt};
        // Fill duration from lofty if symphonia didn't get it.
        if track.duration_secs == 0.0 {
            track.duration_secs = tagged.properties().duration().as_secs_f64();
        }

        let tag = tagged.primary_tag().or_else(|| tagged.first_tag());
        if let Some(tag) = tag {
            // Fill any text fields that symphonia left empty.
            if track.title == path.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown") {
                if let Some(t) = tag.title()  { track.title  = t.into_owned(); }
                if let Some(a) = tag.artist() { track.artist  = a.into_owned(); }
                if let Some(al) = tag.album() { track.album   = al.into_owned(); }
                if let Some(y)  = tag.year()  { track.year    = y.to_string(); }
                use lofty::tag::ItemKey;
                if track.album_artist.is_empty() {
                    if let Some(aa) = tag.get_string(&ItemKey::AlbumArtist) {
                        track.album_artist = aa.to_string();
                    }
                }
            }

            // Prefer CoverFront picture, fall back to first picture.
            let cover_pic = {
                use lofty::picture::PictureType;
                tag.pictures()
                    .iter()
                    .find(|p| p.pic_type() == PictureType::CoverFront)
                    .or_else(|| tag.pictures().first())
            };
            if let Some(pic) = cover_pic {
                use lofty::picture::MimeType;
                let ext = match pic.mime_type() {
                    Some(MimeType::Png) => "png",
                    _ => "jpg",
                };
                let dest = std::env::temp_dir().join(format!("kanae_cover_{:016x}.{}",
                    {
                        use std::collections::hash_map::DefaultHasher;
                        use std::hash::{Hash, Hasher};
                        let mut h = DefaultHasher::new();
                        path.hash(&mut h);
                        h.finish()
                    }, ext));
                if std::fs::write(&dest, pic.data()).is_ok() {
                    let url_path = dest.to_string_lossy().replace('\\', "/");
                    // Trim any leading slash so format!() produces exactly 3 slashes.
                    track.cover_art_path = Some(format!("file:///{}", url_path.trim_start_matches('/')));
                }
            }
        }
    }

    // Fallback: look for cover art image next to the file.
    if track.cover_art_path.is_none() {
        if let Some(dir) = path.parent() {
            for name in &["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "front.png"] {
                let candidate = dir.join(name);
                if candidate.exists() {
                    let url_path = candidate.to_string_lossy().replace('\\', "/");
                    track.cover_art_path = Some(format!("file:///{}", url_path.trim_start_matches('/')));
                    break;
                }
            }
        }
    }

    if track.album_artist.is_empty() {
        track.album_artist = track.artist.clone();
    }

    track
}
