use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
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

/// collect audio tracks from a mixed list of file and folder paths.
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

/// the album a file belongs to, as the sibling audio files in its directory
/// that share its album tag, plus the file's index within them. falls back to
/// the file on its own when it has no album siblings, so an untagged loose
/// file still plays.
pub fn album_context_for(path: &Path) -> (Vec<LocalTrack>, i32) {
    let this = read_file_metadata(path);
    let Some(dir) = path.parent() else { return (vec![this], 0) };

    let siblings = collect_files_from_paths(&[dir.to_string_lossy().into_owned()]);
    // directories can hold several albums; keep only the matching one. an empty
    // album tag means the track is a standalone single.
    let album: Vec<_> = if this.album.is_empty() {
        Vec::new()
    } else {
        siblings.into_iter().filter(|t| t.album == this.album).collect()
    };
    match album.iter().position(|t| t.path == this.path) {
        Some(i) if album.len() > 1 => (album, i as i32),
        _ => (vec![this], 0),
    }
}

/// track metadata without any cover-art work. symphonia handles the common
/// formats on its own, so lofty is only opened when symphonia came back without
/// a duration or a title, keeping the usual case to a single parse per file.
pub fn read_file_metadata_light(path: &Path) -> LocalTrack {
    use symphonia::core::{
        formats::{probe::Hint, FormatOptions, TrackType},
        io::MediaSourceStream,
        meta::{MetadataOptions, StandardTag},
        units::Timestamp,
    };

    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("Unknown").to_string();
    let mut track = LocalTrack {
        path: path.to_path_buf(),
        title: stem.clone(),
        ..Default::default()
    };

    // symphonia: duration + text tags
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

            // consume media-level metadata (id3, vorbis comment, ape, etc.)
            if let Some(rev) = reader.metadata().skip_to_latest() {
                for tag in &rev.media.tags {
                    match &tag.std {
                        Some(StandardTag::TrackTitle(v))  => track.title        = v.to_string(),
                        Some(StandardTag::Artist(v))      => track.artist       = v.to_string(),
                        Some(StandardTag::Album(v))       => track.album        = v.to_string(),
                        Some(StandardTag::AlbumArtist(v)) => track.album_artist = v.to_string(),
                        // take just the year portion (first 4 chars).
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

    // lofty: only for what symphonia could not read
    let needs_lofty = track.duration_secs == 0.0 || track.title == stem;
    if needs_lofty {
        if let Some(tagged) = lofty::probe::Probe::open(path).ok().and_then(|p| p.read().ok()) {
            use lofty::prelude::{Accessor, AudioFile, TaggedFileExt};
            if track.duration_secs == 0.0 {
                track.duration_secs = tagged.properties().duration().as_secs_f64();
            }
            if track.title == stem {
                if let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) {
                    if let Some(t) = tag.title()  { track.title   = t.into_owned(); }
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
            }
        }
    }

    if track.album_artist.is_empty() {
        track.album_artist = track.artist.clone();
    }

    track
}

/// cover art embedded in a file, written to the cover cache and returned as a
/// `file://` url. None when the file carries no picture.
pub fn embedded_cover(path: &Path) -> Option<String> {
    use lofty::prelude::TaggedFileExt;
    let tagged = lofty::probe::Probe::open(path).ok()?.read().ok()?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;

    // prefer CoverFront picture, fall back to first picture.
    use lofty::picture::PictureType;
    let pic = tag.pictures().iter()
        .find(|p| p.pic_type() == PictureType::CoverFront)
        .or_else(|| tag.pictures().first())?;

    use lofty::picture::MimeType;
    let ext = match pic.mime_type() {
        Some(MimeType::Png) => "png",
        _ => "jpg",
    };
    // key the cache file by picture *content*, not source path, so the same art
    // embedded in every track of an album (or shared across albums) dedupes to
    // a single file instead of one copy per track.
    let hash = {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};
        let mut h = DefaultHasher::new();
        pic.data().hash(&mut h);
        h.finish()
    };
    let dest = crate::library_cache::cover_cache_dir()
        .join(format!("kanae_cover_{:016x}.{}", hash, ext));
    if dest.exists() || std::fs::write(&dest, pic.data()).is_ok() {
        let url_path = dest.to_string_lossy().replace('\\', "/");
        // trim any leading slash so format!() produces exactly 3 slashes.
        Some(format!("file:///{}", url_path.trim_start_matches('/')))
    } else {
        None
    }
}

/// cover image sitting next to a file, as a `file://` url.
pub fn folder_cover(dir: &Path) -> Option<String> {
    for name in &["cover.jpg", "cover.png", "folder.jpg", "folder.png", "front.jpg", "front.png"] {
        let candidate = dir.join(name);
        if candidate.exists() {
            let url_path = candidate.to_string_lossy().replace('\\', "/");
            return Some(format!("file:///{}", url_path.trim_start_matches('/')));
        }
    }
    None
}

/// embedded art if the file has any, otherwise the containing folder's image.
pub fn cover_for(path: &Path) -> Option<String> {
    embedded_cover(path).or_else(|| path.parent().and_then(folder_cover))
}

/// memoised metadata, keyed on the file's (mtime, size) so an edited file is
/// re-read but an unchanged one is not.
fn meta_cache() -> &'static std::sync::Mutex<HashMap<PathBuf, (u64, u64, LocalTrack)>> {
    static CACHE: OnceLock<std::sync::Mutex<HashMap<PathBuf, (u64, u64, LocalTrack)>>> = OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

/// dropped wholesale once it grows past this; a browse view only ever needs the
/// albums the user actually visited.
const META_CACHE_MAX: usize = 4096;

fn file_stamp(path: &Path) -> (u64, u64) {
    match std::fs::metadata(path) {
        Ok(m) => {
            let mtime = m.modified().ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (mtime, m.len())
        }
        Err(_) => (0, 0),
    }
}

/// track rows for a browse listing: read in parallel, cover art skipped (the
/// list only shows title / artist / duration) and cached so revisiting an album
/// costs nothing.
pub fn read_album_metadata(paths: &[PathBuf]) -> Vec<LocalTrack> {
    use rayon::prelude::*;

    let stamps: Vec<(u64, u64)> = paths.iter().map(|p| file_stamp(p)).collect();
    let mut out: Vec<Option<LocalTrack>> = {
        let cache = meta_cache().lock().unwrap();
        paths.iter().zip(&stamps)
            .map(|(p, st)| match cache.get(p) {
                Some((mtime, len, t)) if (*mtime, *len) == *st => Some(t.clone()),
                _ => None,
            })
            .collect()
    };

    let fresh: Vec<(usize, LocalTrack)> = out.par_iter()
        .enumerate()
        .filter(|(_, slot)| slot.is_none())
        .map(|(i, _)| (i, read_file_metadata_light(&paths[i])))
        .collect();

    if !fresh.is_empty() {
        let mut cache = meta_cache().lock().unwrap();
        if cache.len() + fresh.len() > META_CACHE_MAX { cache.clear(); }
        for (i, track) in fresh {
            cache.insert(paths[i].clone(), (stamps[i].0, stamps[i].1, track.clone()));
            out[i] = Some(track);
        }
    }

    out.into_iter().zip(paths)
        .map(|(t, p)| t.unwrap_or_else(|| LocalTrack { path: p.clone(), ..Default::default() }))
        .collect()
}

/// full metadata for one track, cover art included.
pub fn read_file_metadata(path: &Path) -> LocalTrack {
    let mut track = read_file_metadata_light(path);
    track.cover_art_path = cover_for(path);
    track
}

/// decode and play back a local audio file on a background thread.
/// progress is reported via `heard_position_arc` and `current_position`.
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
    // fade_out_secs: set by the player to hand this track over to the next one -
    // the bits of an f64 giving how long to ramp down to silence, after which the
    // stream ends. zero means "play to the end normally".
    fade_out_secs: Arc<AtomicU64>,
    // seeks while playing arrive here rather than restarting the thread;
    // seek_gen is the generation start_offset already accounts for.
    seek: Arc<crate::audio_player::SeekSignal>,
    seek_gen: u64,
) {
    use crate::audio_player::AudioController;
    use symphonia::core::{
        codecs::{audio::{AudioDecoder, AudioDecoderOptions}, CodecParameters},
        formats::{probe::Hint, FormatOptions, FormatReader, SeekMode, SeekTo, TrackType},
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

    // gapless is decoder-side in 0.6: packets carry trim info and the decoder
    // strips encoder delay/padding when this option is set (default: on).
    let mut dec_opts = AudioDecoderOptions::default();
    dec_opts.gapless = true;
    let mut decoder = match symphonia::default::get_codecs().make_audio_decoder(&params, &dec_opts) {
        Ok(d)  => d,
        Err(e) => { eprintln!("[file] decoder: {}", e); playback_ended_arc.store(true, Ordering::Relaxed); return; }
    };

    // an accurate seek lands on the packet holding the target, so decoded audio
    // before the target is dropped here to make the seek sample-exact.
    let seek_to = |format: &mut Box<dyn FormatReader>, decoder: &mut Box<dyn AudioDecoder>, secs: f64| -> Result<f64, String> {
        let seeked = format.seek(SeekMode::Accurate, SeekTo::Time {
            time: Time::try_from_secs_f64(secs.max(0.0)).unwrap_or(Time::ZERO),
            track_id: Some(track_id),
        }).map_err(|e| e.to_string())?;
        decoder.reset();
        Ok(time_base
            .and_then(|tb| tb.calc_time(seeked.required_ts))
            .map(|t| t.as_secs_f64())
            .unwrap_or(secs))
    };

    // track position the stream's consumed-sample counter is relative to.
    let mut base_secs = start_offset;
    let mut skip_until: Option<f64> = None;
    if start_offset > 0.1 {
        match seek_to(&mut format, &mut decoder, start_offset) {
            Ok(t)  => skip_until = Some(t),
            Err(e) => eprintln!("[file] seek: {}", e),
        }
    }

    // output seconds emitted so far, used to place both fade ramps.
    let mut emitted_secs = 0.0f64;
    // where the fade-out was triggered, once it has been.
    let mut fade_out_from: Option<f64> = None;
    let mut fade_in = if start_offset < 0.05 { fade_in_secs.max(0.0) } else { 0.0 };

    // volume is applied by the stream itself, at the device end of the buffer.
    let mut my_gen = seek_gen;
    let (stream_sender, samples_emitted_arc, emitted_gen_arc) = audio_controller.begin_stream(
        sample_rate, n_channels as u16, stop_flag.clone(), 4, volume_arc, seek.clone(), my_gen,
    );
    // samples sent since the last seek, to tell when the tail has been heard.
    let mut sent: u64 = 0;

    // bind the visualizer to the device's consumed-sample counter so it reads
    // the window being heard, not the one being decoded.
    if let Ok(mut tap) = vis_tap.lock() {
        tap.attach(samples_emitted_arc.clone(), n_channels as u16);
    }

    let denom = sample_rate as f64 * (n_channels as f64).max(1.0);
    // the counter restarts at every seek; until the device has switched over it
    // still counts the old position, so it is only trusted once it matches. a
    // seek requested but not yet picked up must not be overwritten either.
    let publish_heard = |base: f64, gen: u64| {
        if emitted_gen_arc.load(Ordering::Acquire) != gen || seek.generation() != gen { return None; }
        let played = samples_emitted_arc.load(Ordering::Relaxed);
        heard_position_arc.store((base + played as f64 / denom).to_bits(), Ordering::Relaxed);
        Some(played)
    };

    loop {
        if stop_flag.load(Ordering::Relaxed) { break; }

        let gen = seek.generation();
        if gen != my_gen {
            my_gen = gen;
            let target = seek.target();
            match seek_to(&mut format, &mut decoder, target) {
                Ok(t) => { skip_until = Some(t); base_secs = target; }
                // past the end, most likely: let the track finish
                Err(e) => { eprintln!("[file] seek: {}", e); break; }
            }
            sent = 0;
            // a seek lands mid-track, so a crossfade ramp-in no longer applies
            fade_in = 0.0;
            if let Ok(mut tap) = vis_tap.lock() {
                tap.attach(samples_emitted_arc.clone(), n_channels as u16);
            }
        }

        let packet = match format.next_packet() {
            Ok(Some(p)) => p,
            Ok(None) => {
                // keep the stream open until the tail has been heard, so a seek
                // back from the last moments of the track still works.
                let mut drained = false;
                while !stop_flag.load(Ordering::Relaxed) && seek.generation() == my_gen {
                    if publish_heard(base_secs, my_gen).map_or(false, |p| p >= sent) {
                        drained = true;
                        break;
                    }
                    thread::sleep(std::time::Duration::from_millis(10));
                }
                if drained || stop_flag.load(Ordering::Relaxed) { break; }
                continue;
            }
            Err(symphonia::core::errors::Error::ResetRequired) => { decoder.reset(); continue; }
            Err(e) => { eprintln!("[file] packet: {}", e); break; }
        };
        if packet.track_id != track_id { continue; }

        let mut chunk_start = time_base
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

        if let Some(target) = skip_until {
            let ch = n_channels.max(1);
            let skip = ((target - chunk_start) * sample_rate as f64).round().max(0.0) as usize * ch;
            if skip >= samples.len() { continue; }
            samples.drain(..skip);
            chunk_start = target;
            skip_until = None;
        }

        // feed the visualizer here, before volume scaling, so the spectrum is
        // independent of the playback volume.
        if let Ok(mut tap) = vis_tap.lock() { tap.push(&samples, n_channels as u16); }

        let count  = samples.len();
        let frames = count as f64 / (sample_rate as f64 * (n_channels as f64).max(1.0));

        // pick up a crossfade handover requested since the last packet.
        let requested = f64::from_bits(fade_out_secs.load(Ordering::Relaxed));
        if fade_out_from.is_none() && requested > 0.0 {
            fade_out_from = Some(emitted_secs);
        }
        let fade_len = requested.max(0.0);
        let fade_start = fade_out_from;

        // only the crossfade ramps are applied here: they are positions on the
        // track's own timeline, so they have to ride along with the samples.
        if fade_in > 0.0 || fade_len > 0.0 {
            for (i, s) in samples.iter_mut().enumerate() {
                // where this sample sits on the track's output timeline.
                let at = emitted_secs + frames * (i as f64 / count.max(1) as f64);
                let mut g = 1.0f32;
                if fade_in > 0.0 && at < fade_in {
                    g *= (at / fade_in) as f32;
                }
                if let (Some(from), true) = (fade_start, fade_len > 0.0) {
                    g *= (1.0 - ((at - from) / fade_len).clamp(0.0, 1.0)) as f32;
                }
                *s *= g;
            }
        }
        emitted_secs += frames;

        if !stream_sender.send(my_gen, samples) { break; }
        sent += count as u64;

        // the handover is complete once the ramp has run its course.
        if let (Some(from), true) = (fade_start, fade_len > 0.0) {
            if emitted_secs - from >= fade_len { break; }
        }

        publish_heard(base_secs, my_gen);
        current_position.store(chunk_start.to_bits(), Ordering::Relaxed);
    }

    if stop_flag.load(Ordering::Relaxed) {
        audio_controller.stop();
        return;
    }

    stream_sender.finish();
    while !audio_controller.is_empty() {
        if stop_flag.load(Ordering::Relaxed) { audio_controller.stop(); return; }
        publish_heard(base_secs, my_gen);
        thread::sleep(std::time::Duration::from_millis(50));
    }

    playback_ended_arc.store(true, Ordering::Relaxed);
}

