use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;

const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "mp2", "mp1", "flac", "ogg", "opus", "m4a", "mp4", "aac", "alac",
    "wav", "aiff", "aif", "caf", "mka", "wma", "ape",
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
        formats::{probe::Hint, FormatOptions, TrackType},
        io::MediaSourceStream,
        meta::{MetadataOptions, StandardTag},
        units::Timestamp,
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
        // 0.6 is gapless-aware at the container level: `duration`/`num_frames`
        // already exclude encoder delay/padding, matching gapless playback.
        if let Ok(mut reader) = symphonia::default::get_probe().probe(
            &hint, mss, FormatOptions::default(), MetadataOptions::default(),
        ) {
            if let Some(t) = reader.default_track(TrackType::Audio) {
                if let (Some(tb), Some(dur)) = (t.time_base, t.duration) {
                    track.duration_secs =
                        tb.calc_time_saturating(Timestamp::new(dur.get() as i64)).as_secs_f64();
                }
            }

            // Consume media-level metadata (ID3, Vorbis Comment, APE, etc.)
            if let Some(rev) = reader.metadata().skip_to_latest() {
                for tag in &rev.media.tags {
                    match &tag.std {
                        Some(StandardTag::TrackTitle(v))  => track.title        = v.to_string(),
                        Some(StandardTag::Artist(v))      => track.artist       = v.to_string(),
                        Some(StandardTag::Album(v))       => track.album        = v.to_string(),
                        Some(StandardTag::AlbumArtist(v)) => track.album_artist = v.to_string(),
                        // Take just the year portion (first 4 chars).
                        Some(StandardTag::ReleaseDate(v)) => track.year = v.chars().take(4).collect(),
                        Some(StandardTag::RecordingDate(v)) if track.year.is_empty() => {
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
                // Key the cache file by picture *content*, not source path, so the
                // same art embedded in every track of an album (or shared across
                // albums) dedupes to a single file instead of one copy per track.
                let dest = crate::library_cache::cover_cache_dir().join(format!("kanae_cover_{:016x}.{}",
                    {
                        use std::collections::hash_map::DefaultHasher;
                        use std::hash::{Hash, Hasher};
                        let mut h = DefaultHasher::new();
                        pic.data().hash(&mut h);
                        h.finish()
                    }, ext));
                if dest.exists() || std::fs::write(&dest, pic.data()).is_ok() {
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
    vis_tap: Arc<std::sync::Mutex<crate::audio_player::VisTap>>,
    // fade_in_secs: seconds to ramp up from silence at the start (0 = full gain).
    fade_in_secs: f64,
    // fade_out_secs: set by the player to hand this track over to the next one —
    // the bits of an f64 giving how long to ramp down to silence, after which the
    // stream ends. Zero means "play to the end normally".
    fade_out_secs: Arc<AtomicU64>,
) {
    use crate::audio_player::AudioController;
    use symphonia::core::{
        codecs::{audio::AudioDecoderOptions, CodecParameters},
        formats::{probe::Hint, FormatOptions, SeekMode, SeekTo, TrackType},
        io::MediaSourceStream,
        meta::MetadataOptions,
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

    let mut format = match symphonia::default::get_probe().probe(
        &hint, mss, FormatOptions::default(), MetadataOptions::default(),
    ) {
        Ok(f)  => f,
        Err(e) => { eprintln!("[file] probe {}: {}", file_path.display(), e); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    let track = match format.default_track(TrackType::Audio) {
        Some(t) => t,
        None    => { eprintln!("[file] no audio track in {}", file_path.display()); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    let params = match &track.codec_params {
        Some(CodecParameters::Audio(p)) => p.clone(),
        _ => { eprintln!("[file] no audio codec params in {}", file_path.display()); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    let sample_rate = params.sample_rate.unwrap_or(44100);
    let n_channels  = params.channels.as_ref().map(|c| c.count()).unwrap_or(2);
    let track_id    = track.id;
    let time_base   = track.time_base;

    // Gapless is decoder-side in 0.6: packets carry trim info and the decoder
    // strips encoder delay/padding when this option is set (default: on).
    let mut dec_opts = AudioDecoderOptions::default();
    dec_opts.gapless = true;
    let mut decoder = match symphonia::default::get_codecs().make_audio_decoder(&params, &dec_opts) {
        Ok(d)  => d,
        Err(e) => { eprintln!("[file] decoder: {}", e); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    if start_offset > 0.1 {
        let _ = format.seek(SeekMode::Accurate, SeekTo::Time {
            time: Time::try_from_secs_f64(start_offset).unwrap_or(Time::ZERO),
            track_id: Some(track_id),
        });
    }

    let mut current_vol = f64::from_bits(volume_arc.load(Ordering::Relaxed)) as f32;
    // Output seconds emitted so far, used to place both fade ramps.
    let mut emitted_secs = 0.0f64;
    // Where the fade-out was triggered, once it has been.
    let mut fade_out_from: Option<f64> = None;
    let fade_in = if start_offset < 0.05 { fade_in_secs.max(0.0) } else { 0.0 };

    let (stream_sender, samples_emitted_arc) = audio_controller.begin_stream(
        sample_rate, n_channels as u16, stop_flag.clone(), 4,
    );

    // Bind the visualizer to the device's consumed-sample counter so it reads
    // the window being heard, not the one being decoded.
    if let Ok(mut tap) = vis_tap.lock() {
        tap.attach(samples_emitted_arc.clone(), n_channels as u16);
    }

    loop {
        if stop_flag.load(Ordering::Relaxed) { break; }

        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None)    => break, // end of stream
            Err(symphonia::core::errors::Error::ResetRequired) => { decoder.reset(); continue; }
            Err(e) => { eprintln!("[file] packet: {}", e); break; }
        };
        if packet.track_id != track_id { continue; }

        let chunk_start = time_base
            .and_then(|tb| tb.calc_time(packet.pts))
            .map(|t| t.as_secs_f64())
            .unwrap_or_else(|| packet.pts.get() as f64 / sample_rate as f64);

        let decoded = match decoder.decode(&packet) {
            Ok(d) => d,
            Err(symphonia::core::errors::Error::DecodeError(msg)) => { eprintln!("[file] decode: {}", msg); continue; }
            Err(e) => { eprintln!("[file] decode fatal: {}", e); break; }
        };

        let mut samples: Vec<f32> = Vec::new();
        decoded.copy_to_vec_interleaved(&mut samples);

        // Feed the visualizer here, BEFORE volume scaling, so the spectrum is
        // independent of the playback volume.
        if let Ok(mut tap) = vis_tap.lock() { tap.push(&samples, n_channels as u16); }

        let target_vol = f64::from_bits(volume_arc.load(Ordering::Relaxed)) as f32;
        let count  = samples.len();
        let n      = count as f32;
        let frames = count as f64 / (sample_rate as f64 * (n_channels as f64).max(1.0));

        // Pick up a crossfade handover requested since the last packet.
        let requested = f64::from_bits(fade_out_secs.load(Ordering::Relaxed));
        if fade_out_from.is_none() && requested > 0.0 {
            fade_out_from = Some(emitted_secs);
        }
        let fade_len = requested.max(0.0);
        let fade_start = fade_out_from;

        for (i, s) in samples.iter_mut().enumerate() {
            let t = i as f32 / n;
            // Where this sample sits on the track's output timeline.
            let at = emitted_secs + frames * (i as f64 / count.max(1) as f64);
            let mut g = current_vol + (target_vol - current_vol) * t;
            if fade_in > 0.0 && at < fade_in {
                g *= (at / fade_in) as f32;
            }
            if let (Some(from), true) = (fade_start, fade_len > 0.0) {
                g *= (1.0 - ((at - from) / fade_len).clamp(0.0, 1.0)) as f32;
            }
            *s = (*s * g).clamp(-1.0, 1.0);
        }
        current_vol = target_vol;
        emitted_secs += frames;

        if !stream_sender.send(samples) { break; }

        // The handover is complete once the ramp has run its course.
        if let (Some(from), true) = (fade_start, fade_len > 0.0) {
            if emitted_secs - from >= fade_len { break; }
        }

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
