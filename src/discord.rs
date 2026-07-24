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
const COVER_CACHE_TTL: Duration = Duration::from_secs(58 * 60);
const SEEK_THRESHOLD_SECS: f64 = 3.0;

pub struct TrackInfo {
    pub title:         String,
    pub artist:        String,
    pub album:         String,
    /// Local filesystem path or http(s):// URL for cover art.
    pub cover_url:     String,
    pub position_secs: f64,
    pub duration_secs: f64,
    pub is_playing:    bool,
}

pub struct DiscordPresence {
    client:              Client<Arc<InnerClient>>,
    connected:           bool,
    last_reconnect:      Instant,
    // Cover art upload + 1-hour cache
    cached_cover_src:    String,
    cached_cover_litter: String,
    cover_cache:         HashMap<String, (String, Instant)>,
    cover_slot:          Arc<Mutex<Option<Option<String>>>>,
    cover_thread:        Option<thread::JoinHandle<()>>,
    // Change detection — only call IPC when something actually changes
    last_title:          String,
    last_artist:         String,
    last_album:          String,
    last_is_playing:     bool,
    last_pushed_cover:   String,
    // Seek detection
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
            cached_cover_litter: String::new(),
            cover_cache:         HashMap::new(),
            cover_slot:          Arc::new(Mutex::new(None)),
            cover_thread:        None,
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

    /// Poll the background upload thread; returns true if the cover URL changed.
    fn poll_cover(&mut self) -> bool {
        let result = self.cover_slot.lock().unwrap().take();
        if let Some(maybe_url) = result {
            if let Some(t) = self.cover_thread.take() { let _ = t.join(); }
            let url = maybe_url.unwrap_or_default();
            eprintln!("[discord] litterbox url: {:?}", url);
            if !url.is_empty() {
                self.cover_cache.insert(self.cached_cover_src.clone(), (url.clone(), Instant::now()));
            }
            self.cached_cover_litter = url;
            return true;
        }
        false
    }

    /// Call every tick with current player state.
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
            self.cached_cover_src    = cover_src.clone();
            self.cached_cover_litter = String::new();
            *self.cover_slot.lock().unwrap() = None;
            if !cover_src.is_empty() {
                if let Some((cached_url, upload_time)) = self.cover_cache.get(&cover_src) {
                    if upload_time.elapsed() < COVER_CACHE_TTL {
                        eprintln!("[discord] cover cache hit: {}", cached_url);
                        self.cached_cover_litter = cached_url.clone();
                    }
                }
                if self.cached_cover_litter.is_empty() {
                    let slot   = self.cover_slot.clone();
                    let handle = thread::spawn(move || {
                        *slot.lock().unwrap() = Some(upload_cover_art(&cover_src));
                    });
                    self.cover_thread = Some(handle);
                }
            }
        }

        // Detect seeks: position jumped more than threshold from expected progression.
        let seek_detected = info.is_playing
            && !self.last_title.is_empty()
            && info.title == self.last_title
            && {
                let elapsed = self.last_push_time.elapsed().as_secs_f64();
                let expected = self.last_pushed_position + elapsed;
                (info.position_secs - expected).abs() > SEEK_THRESHOLD_SECS
            };

        // Only push IPC when something meaningful changed.
        let dirty = seek_detected
            || cover_arrived
            || info.title      != self.last_title
            || info.artist     != self.last_artist
            || info.album      != self.last_album
            || info.is_playing != self.last_is_playing
            || self.cached_cover_litter != self.last_pushed_cover;

        if !dirty { return; }

        self.last_title          = info.title.clone();
        self.last_artist         = info.artist.clone();
        self.last_album          = info.album.clone();
        self.last_is_playing     = info.is_playing;
        self.last_pushed_cover   = self.cached_cover_litter.clone();
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
        // Discord Rich Presence has no dedicated "paused" state, so we mark it
        // two ways: drop the progress-bar timestamps (below) and tag the state
        // line — otherwise a paused track is indistinguishable from one that
        // simply has no known duration.
        if !info.is_playing {
            state.push_str(" • Paused");
        }

        let mut activity = Activity::new()
            .kind(ActivityType::Listening)
            .details(title)
            .state(state);

        // Timestamps: start = when this track "began" in Unix time,
        // end = when it will finish. Both together give Discord a progress bar.
        // Only sent while playing — a live countdown next to a paused track would
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

        if !self.cached_cover_litter.is_empty() {
            let tooltip = if info.album.is_empty() { None } else { Some(info.album.as_str()) };
            let mut assets = Assets::new().large_image(self.cached_cover_litter.as_str(), tooltip);
            // A small pause badge over the cover reinforces the paused state for
            // viewers. "paused" is an asset key registered in the Discord app's
            // Rich Presence art assets; if it isn't present Discord just ignores
            // it, and the " • Paused" state tag still conveys the status.
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

/// Convert a file:// URL or plain path to a filesystem path; keep http(s) URLs as-is.
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

/// Load cover art (file path or HTTP URL), resize to 128×128, upload to litterbox (1 h).
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

    let boundary = "KanaeDiscordBoundary42xYz";
    let mut body: Vec<u8> = Vec::new();
    multipart_field(&mut body, boundary, "reqtype", b"fileupload");
    multipart_field(&mut body, boundary, "time",    b"1h");
    multipart_file(&mut body, boundary, "fileToUpload", "cover.png", "image/png", &png_bytes);
    body.extend_from_slice(format!("--{}--\r\n", boundary).as_bytes());

    let ct   = format!("multipart/form-data; boundary={}", boundary);
    let resp = ureq::post("https://litterbox.catbox.moe/resources/internals/api.php")
        .header("Content-Type", &ct)
        .send(body.as_slice())
        .ok()?;

    let text = resp.into_body().read_to_string().ok()?;
    let url  = text.trim().to_string();
    eprintln!("[discord] litterbox response: {:?}", url);
    if url.starts_with("https://") { Some(url) } else { None }
}

fn multipart_field(body: &mut Vec<u8>, boundary: &str, name: &str, value: &[u8]) {
    body.extend_from_slice(
        format!("--{}\r\nContent-Disposition: form-data; name=\"{}\"\r\n\r\n", boundary, name).as_bytes(),
    );
    body.extend_from_slice(value);
    body.extend_from_slice(b"\r\n");
}

fn multipart_file(body: &mut Vec<u8>, boundary: &str, name: &str, filename: &str, mime: &str, data: &[u8]) {
    body.extend_from_slice(
        format!(
            "--{}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: {}\r\n\r\n",
            boundary, name, filename, mime
        )
        .as_bytes(),
    );
    body.extend_from_slice(data);
    body.extend_from_slice(b"\r\n");
}
