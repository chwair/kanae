use discordipc::{
    activity::{Activity, ActivityType, Assets, Timestamps},
    packet::Packet,
    Client, InnerClient,
};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const CLIENT_ID: &str = "1513290345322905781";
/// cloudflare worker that holds a 128x128 cover for an hour so discord can fetch it.
const COVER_UPLOAD_URL: &str = "https://cover.kanaefm.workers.dev/upload";
/// slightly under the host's one-hour expiry, so a cached url never outlives the image.
const COVER_CACHE_TTL: Duration = Duration::from_secs(58 * 60);
const SEEK_THRESHOLD_SECS: f64 = 3.0;
/// backoff before each retry of a failed cover upload. length caps the attempts.
const COVER_RETRY_BACKOFF_SECS: [u64; 3] = [5, 20, 60];

/// a finished upload, tagged with the request it answers.
struct CoverResult {
    gen: u64,
    url: Option<String>,
}

pub struct TrackInfo {
    pub title:         String,
    pub artist:        String,
    pub album:         String,
    /// local filesystem path or http(s):// url for cover art.
    pub cover_url:     String,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub is_playing:    bool,
}

pub struct DiscordPresence {
    client:              Client<Arc<InnerClient>>,
    connected:           bool,
    last_reconnect:      Instant,
    // cover art upload + 1-hour cache
    cached_cover_src: String,
    cached_cover_url: String,
    cover_cache:      HashMap<String, (String, Instant)>,
    cover_slot:       Arc<Mutex<Option<CoverResult>>>,
    /// bumped per upload request; results carrying a stale gen are discarded.
    cover_gen:        u64,
    cover_inflight:   bool,
    cover_attempts:   usize,
    cover_retry_at:   Option<Instant>,
    // change detection - only call ipc when something actually changes
    last_title:          String,
    last_artist:         String,
    last_album:          String,
    last_is_playing:     bool,
    last_pushed_cover:   String,
    // seek detection
    last_push_time:      Instant,
    last_pushed_position: f64,
}

impl DiscordPresence {
    pub fn new() -> Option<Self> {
        Some(Self {
            client:              Client::new(CLIENT_ID),
            connected:           false,
            last_reconnect:      Instant::now() - Duration::from_secs(30),
            cached_cover_src:    String::new(),
            cached_cover_url:    String::new(),
            cover_cache:         HashMap::new(),
            cover_slot:          Arc::new(Mutex::new(None)),
            cover_gen:           0,
            cover_inflight:      false,
            cover_attempts:      0,
            cover_retry_at:      None,
            last_title:          String::new(),
            last_artist:         String::new(),
            last_album:          String::new(),
            last_is_playing:     false,
            last_pushed_cover:   String::new(),
            last_push_time:      Instant::now(),
            last_pushed_position: 0.0,
        })
    }

    fn ensure_connected(&mut self) -> bool {
        if self.connected { return true; }
        if self.last_reconnect.elapsed() < Duration::from_secs(15) { return false; }
        self.last_reconnect = Instant::now();
        match self.client.connect_and_wait() {
            Ok(_)  => { eprintln!("[discord] connected"); self.connected = true; true }
            Err(e) => { eprintln!("[discord] connect failed: {}", e); false }
        }
    }

    /// upload the current cover in the background, tagged with a fresh gen.
    fn spawn_cover_upload(&mut self) {
        let src = self.cached_cover_src.clone();
        if src.is_empty() { return; }
        self.cover_gen     += 1;
        self.cover_attempts += 1;
        self.cover_inflight = true;
        self.cover_retry_at = None;
        let gen  = self.cover_gen;
        let slot = self.cover_slot.clone();
        thread::spawn(move || {
            let url = upload_cover_art(&src);
            *slot.lock().unwrap() = Some(CoverResult { gen, url });
        });
    }

    /// poll the background upload thread; returns true if the cover url changed.
    fn poll_cover(&mut self) -> bool {
        let Some(result) = self.cover_slot.lock().unwrap().take() else { return false };
        // an upload for a cover we've since moved off of. dropping it keeps its
        // url from being cached under - or pushed for - the current track.
        if result.gen != self.cover_gen { return false; }
        self.cover_inflight = false;

        let Some(url) = result.url else {
            // failed. schedule a retry so one flaky upload doesn't cost the
            // cover for as long as this track stays loaded.
            self.cover_retry_at = COVER_RETRY_BACKOFF_SECS
                .get(self.cover_attempts - 1)
                .map(|secs| Instant::now() + Duration::from_secs(*secs));
            eprintln!(
                "[discord] cover upload failed (attempt {}); retry: {}",
                self.cover_attempts,
                if self.cover_retry_at.is_some() { "scheduled" } else { "gave up" },
            );
            return false;
        };

        eprintln!("[discord] cover url: {:?}", url);
        self.cover_cache.insert(self.cached_cover_src.clone(), (url.clone(), Instant::now()));
        self.cached_cover_url = url;
        self.cover_attempts = 0;
        self.cover_retry_at = None;
        true
    }

    /// call every tick with current player state.
    pub fn update(&mut self, info: Option<TrackInfo>) {
        let cover_arrived = self.poll_cover();

        if !self.ensure_connected() { return; }

        let Some(info) = info else {
            if self.last_title.is_empty() && !self.last_is_playing { return; }
            self.last_title.clear();
            self.last_artist.clear();
            self.last_album.clear();
            self.last_is_playing = false;
            self.last_pushed_cover.clear();
            self.last_push_time = Instant::now();
            self.last_pushed_position = 0.0;
            if let Err(e) = self.client.send(Packet::new_activity(None, None)) {
                eprintln!("[discord] clear failed: {}", e);
                self.connected = false;
            }
            return;
        };

        let cover_src = normalize_cover_path(&info.cover_url);
        if cover_src != self.cached_cover_src {
            self.cached_cover_src = cover_src.clone();
            self.cached_cover_url = String::new();
            self.cover_gen       += 1; // orphan any in-flight upload
            self.cover_inflight   = false;
            self.cover_attempts   = 0;
            self.cover_retry_at   = None;
            *self.cover_slot.lock().unwrap() = None;
            if !cover_src.is_empty() {
                if let Some((cached_url, upload_time)) = self.cover_cache.get(&cover_src) {
                    if upload_time.elapsed() < COVER_CACHE_TTL {
                        eprintln!("[discord] cover cache hit: {}", cached_url);
                        self.cached_cover_url = cached_url.clone();
                    }
                }
                if self.cached_cover_url.is_empty() { self.spawn_cover_upload(); }
            }
        } else if self.cached_cover_url.is_empty() && !self.cover_inflight {
            // retry an earlier failure once its backoff expires.
            if self.cover_retry_at.is_some_and(|at| Instant::now() >= at) {
                self.spawn_cover_upload();
            }
        }

        // detect seeks: position jumped more than threshold from expected progression.
        let seek_detected = info.is_playing
            && !self.last_title.is_empty()
            && info.title == self.last_title
            && {
                let elapsed = self.last_push_time.elapsed().as_secs_f64();
                let expected = self.last_pushed_position + elapsed;
                (info.position_secs - expected).abs() > SEEK_THRESHOLD_SECS
            };

        // only push ipc when something meaningful changed.
        let dirty = seek_detected
            || cover_arrived
            || info.title      != self.last_title
            || info.artist     != self.last_artist
            || info.album      != self.last_album
            || info.is_playing != self.last_is_playing
            || self.cached_cover_url != self.last_pushed_cover;

        if !dirty { return; }

        self.last_title          = info.title.clone();
        self.last_artist         = info.artist.clone();
        self.last_album          = info.album.clone();
        self.last_is_playing     = info.is_playing;
        self.last_pushed_cover   = self.cached_cover_url.clone();
        self.last_push_time      = Instant::now();
        self.last_pushed_position = info.position_secs;

        let title = if info.title.is_empty() { "Unknown Track".to_string() } else { info.title.clone() };
        let mut state = if !info.artist.is_empty() {
            info.artist.clone()
        } else if !info.album.is_empty() {
            info.album.clone()
        } else {
            "Kanae".to_string()
        };
        // discord rich presence has no dedicated "paused" state, so we mark it
        // two ways: drop the progress-bar timestamps (below) and tag the state
        // line - otherwise a paused track is indistinguishable from one that
        // simply has no known duration.
        if !info.is_playing {
            state.push_str(" • Paused");
        }

        let mut activity = Activity::new()
            .kind(ActivityType::Listening)
            .details(title)
            .state(state);

        // timestamps: start = when this track "began" in unix time,
        // end = when it will finish. both together give discord a progress bar.
        // only sent while playing - a live countdown next to a paused track would
        // keep ticking, so paused presence carries no timestamps at all.
        if info.is_playing && info.duration_secs > 0.0 {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs() as i64;
            let start = now - info.position_secs as i64;
            let end   = start + info.duration_secs as i64;
            activity = activity.timestamps(Timestamps::new().start(start).end(end));
        }

        if !self.cached_cover_url.is_empty() {
            let tooltip = if info.album.is_empty() { None } else { Some(info.album.as_str()) };
            let mut assets = Assets::new().large_image(self.cached_cover_url.as_str(), tooltip);
            // a small pause badge over the cover reinforces the paused state for
            // viewers. "paused" is an asset key registered in the discord app's
            // rich presence art assets; if it isn't present discord just ignores
            // it, and the "Paused" state tag still conveys the status.
            if !info.is_playing {
                assets = assets.small_image("paused", Some("Paused"));
            }
            activity = activity.assets(assets);
        }

        if let Err(e) = self.client.send(Packet::new_activity(Some(&activity), None)) {
            eprintln!("[discord] set_activity failed: {} — will reconnect", e);
            self.connected = false;
        }
    }
}

/// convert a file:// url or plain path to a filesystem path; keep http(s) urls as-is.
fn normalize_cover_path(url: &str) -> String {
    if url.is_empty() { return String::new(); }
    if url.starts_with("http://") || url.starts_with("https://") {
        return url.to_string();
    }
    if let Some(p) = url.strip_prefix("file:///") {
        #[cfg(windows)]
        return p.to_string();
        #[cfg(not(windows))]
        return format!("/{}", p);
    }
    if let Some(p) = url.strip_prefix("file://") {
        return p.to_string();
    }
    url.to_string()
}

/// load cover art (file path or http url), resize to 128x128, upload to the
/// kanae image host, which serves it for one hour.
fn upload_cover_art(path_or_url: &str) -> Option<String> {
    eprintln!("[discord] uploading cover: {}", path_or_url);

    let img = if path_or_url.starts_with("http://") || path_or_url.starts_with("https://") {
        let resp = ureq::get(path_or_url).call().ok()?;
        let buf  = resp.into_body().read_to_vec().ok()?;
        image::load_from_memory(&buf).ok()?
    } else {
        image::open(path_or_url).ok()?
    };

    let resized = img.resize_exact(128, 128, image::imageops::FilterType::Lanczos3);

    let mut cursor = std::io::Cursor::new(Vec::<u8>::new());
    resized.write_to(&mut cursor, image::ImageFormat::Png).ok()?;
    let png_bytes = cursor.into_inner();

    let resp = ureq::post(COVER_UPLOAD_URL)
        .header("Content-Type", "image/png")
        .send(png_bytes.as_slice())
        .ok()?;

    let text = resp.into_body().read_to_string().ok()?;
    let url  = text.trim().to_string();
    eprintln!("[discord] imghost response: {:?}", url);
    if url.starts_with("https://") { Some(url) } else { None }
}
