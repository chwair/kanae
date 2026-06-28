use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

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
                let dest = crate::library_cache::cover_cache_dir().join(format!("kanae_cover_{:016x}.{}",
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

/// Decode and play back a local audio file on a background thread.
/// Progress is reported via `heard_position_arc` and `current_position`.
pub fn play_local_file(
    file_path: PathBuf,
    start_offset: f64,
    stop_flag: Arc<AtomicBool>,
    volume_arc: Arc<AtomicU64>,
    heard_position_arc: Arc<AtomicU64>,
    current_position: Arc<AtomicU64>,
    playback_ended_arc: Arc<AtomicBool>,
) {
    use crate::audio_player::AudioController;
    use symphonia::core::{
        audio::SampleBuffer,
        codecs::DecoderOptions,
        formats::{SeekMode, SeekTo},
        io::MediaSourceStream,
        meta::MetadataOptions,
        probe::Hint,
        units::Time,
    };

    let audio_controller = match AudioController::new() {
        Ok(c)  => c,
        Err(e) => { eprintln!("[file] audio init: {}", e); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    let file = match std::fs::File::open(&file_path) {
        Ok(f)  => f,
        Err(e) => { eprintln!("[file] open {}: {}", file_path.display(), e); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    let mss = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = file_path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }

    let fmt_opts = symphonia::core::formats::FormatOptions { enable_gapless: true, ..Default::default() };
    let probed = match symphonia::default::get_probe().format(
        &hint, mss, &fmt_opts, &MetadataOptions::default(),
    ) {
        Ok(p)  => p,
        Err(e) => { eprintln!("[file] probe {}: {}", file_path.display(), e); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    let mut format = probed.format;
    let track = match format.default_track() {
        Some(t) => t,
        None    => { eprintln!("[file] no audio track in {}", file_path.display()); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    let sample_rate = track.codec_params.sample_rate.unwrap_or(44100);
    let n_channels  = track.codec_params.channels.map(|c| c.count()).unwrap_or(2);
    let track_id    = track.id;
    let time_base   = track.codec_params.time_base;

    let mut decoder = match symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
    {
        Ok(d)  => d,
        Err(e) => { eprintln!("[file] decoder: {}", e); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    if start_offset > 0.1 {
        let _ = format.seek(SeekMode::Accurate, SeekTo::Time {
            time: Time { seconds: start_offset as u64, frac: start_offset.fract() },
            track_id: Some(track_id),
        });
    }

    let mut current_vol = f64::from_bits(volume_arc.load(Ordering::Relaxed)) as f32;
    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut fade_first_packet = start_offset < 0.05;

    let (stream_sender, samples_emitted_arc) = audio_controller.begin_stream(
        sample_rate, n_channels as u16, stop_flag.clone(), 4,
    );

    loop {
        if stop_flag.load(Ordering::Relaxed) { break; }

        let packet = match format.next_packet() {
            Ok(p) => p,
            Err(symphonia::core::errors::Error::IoError(ref e))
                if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(symphonia::core::errors::Error::ResetRequired) => { decoder.reset(); continue; }
            Err(e) => { eprintln!("[file] packet: {}", e); break; }
        };
        if packet.track_id() != track_id { continue; }

        let chunk_start = if let Some(tb) = time_base {
            let t = tb.calc_time(packet.ts());
            t.seconds as f64 + t.frac
        } else {
            packet.ts() as f64 / sample_rate as f64
        };

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(ref msg)) => { eprintln!("[file] decode: {}", msg); continue; }
            Err(e) => { eprintln!("[file] decode fatal: {}", e); break; }
        };

        if sample_buf.as_ref().map_or(true, |b| b.capacity() < decoded.capacity()) {
            sample_buf = Some(SampleBuffer::<f32>::new(decoded.capacity() as u64, *decoded.spec()));
        }
        let sb = sample_buf.as_mut().unwrap();
        sb.copy_interleaved_ref(decoded);

        let target_vol = f64::from_bits(volume_arc.load(Ordering::Relaxed)) as f32;
        let raw = sb.samples();
        let n   = raw.len() as f32;
        let fade = fade_first_packet;
        fade_first_packet = false;
        let samples: Vec<f32> = raw.iter().enumerate().map(|(i, &s)| {
            let t = i as f32 / n;
            let fi = if fade { t } else { 1.0 };
            (s * (current_vol + (target_vol - current_vol) * t) * fi).clamp(-1.0, 1.0)
        }).collect();
        current_vol = target_vol;

        if !stream_sender.send(samples) { break; }

        let denom = sample_rate as f64 * (n_channels as f64).max(1.0);
        let heard_pos = start_offset
            + samples_emitted_arc.load(Ordering::Relaxed) as f64 / denom;
        heard_position_arc.store(heard_pos.to_bits(), Ordering::Relaxed);
        current_position.store(chunk_start.to_bits(), Ordering::Relaxed);
    }

    if stop_flag.load(Ordering::Relaxed) {
        audio_controller.stop();
        return;
    }

    stream_sender.finish();
    while !audio_controller.is_empty() {
        if stop_flag.load(Ordering::Relaxed) { audio_controller.stop(); return; }
        let denom = sample_rate as f64 * (n_channels as f64).max(1.0);
        let heard_pos = start_offset
            + samples_emitted_arc.load(Ordering::Relaxed) as f64 / denom;
        heard_position_arc.store(heard_pos.to_bits(), Ordering::Relaxed);
        thread::sleep(std::time::Duration::from_millis(50));
    }

    playback_ended_arc.store(true, Ordering::Relaxed);
}
