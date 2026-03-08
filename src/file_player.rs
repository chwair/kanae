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
    use lofty::prelude::{Accessor, AudioFile, TaggedFileExt};

    let mut track = LocalTrack {
        path: path.to_path_buf(),
        title: path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("Unknown")
            .to_string(),
        ..Default::default()
    };

    let tagged = match lofty::probe::Probe::open(path)
        .ok()
        .and_then(|p| p.read().ok())
    {
        Some(t) => t,
        None => {
            eprintln!("[file] failed to read metadata: {}", path.display());
            return track;
        }
    };

    track.duration_secs = tagged.properties().duration().as_secs_f64();
    let tag = match tagged.primary_tag().or_else(|| tagged.first_tag()) {
        Some(t) => t,
        None => return track,
    };

    if let Some(t) = tag.title()  { track.title  = t.into_owned(); }
    if let Some(a) = tag.artist() { track.artist  = a.into_owned(); }
    if let Some(al) = tag.album() { track.album   = al.into_owned(); }
    if let Some(y)  = tag.year()  { track.year    = y.to_string(); }

    use lofty::tag::ItemKey;
    if let Some(aa) = tag.get_string(&ItemKey::AlbumArtist) {
        track.album_artist = aa.to_string();
    }
    if track.album_artist.is_empty() {
        track.album_artist = track.artist.clone();
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
        let dest = std::env::temp_dir().join(format!("kanae_embed_cover.{}", ext));
        if std::fs::write(&dest, pic.data()).is_ok() {
            let url_path = dest.to_string_lossy().replace('\\', "/");
            track.cover_art_path = Some(format!("file:///{}", url_path));
        }
    }

    // Fallback: look for cover art image next to the file.
    if track.cover_art_path.is_none() {
        if let Some(dir) = path.parent() {
            for name in &["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "front.png"] {
                let candidate = dir.join(name);
                if candidate.exists() {
                    let url_path = candidate.to_string_lossy().replace('\\', "/");
                    track.cover_art_path = Some(format!("file:///{}", url_path));
                    break;
                }
            }
        }
    }

    track
}
