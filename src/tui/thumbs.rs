use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;

use ratatui_image::{picker::Picker, protocol::StatefulProtocol};

/// longest edge of a decoded grid thumbnail, in pixels. big enough for a
/// ~30-column tile on high-dpi cell sizes, small enough to encode quickly.
const THUMB_PX: u32 = 512;
/// thumbnails kept around; the least recently drawn are dropped past this.
const MAX_THUMBS: usize = 192;
/// decodes waiting at most. scrolling fast through a big library would
/// otherwise queue every cover it passed; the oldest waiting ones are dropped
/// and simply asked for again if they come back on screen.
const MAX_WAITING: usize = 48;
const WORKERS: usize = 2;

pub(super) enum Thumb {
    Pending,
    Failed,
    Ready { proto: StatefulProtocol, aspect: f32 },
}

struct Entry {
    thumb:     Thumb,
    last_used: u64,
}

type Requests = Arc<(Mutex<VecDeque<String>>, Condvar)>;

/// grid cover thumbnails, decoded off the ui thread. requests are keyed by the
/// cover path; a tile asks every frame and draws whatever is ready.
pub(super) struct Thumbs {
    requests: Requests,
    res_rx:   Receiver<(String, Option<image::DynamicImage>)>,
    entries:  HashMap<String, Entry>,
    clock:    u64,
}

impl Thumbs {
    pub fn new() -> Self {
        let requests: Requests = Arc::new((Mutex::new(VecDeque::new()), Condvar::new()));
        let (res_tx, res_rx) = mpsc::channel();
        for _ in 0..WORKERS {
            let requests = requests.clone();
            let tx = res_tx.clone();
            thread::spawn(move || loop {
                let path = {
                    let (lock, cvar) = &*requests;
                    let mut q = lock.lock().unwrap();
                    loop {
                        // newest first: that is what is on screen right now.
                        if let Some(p) = q.pop_back() { break p; }
                        q = cvar.wait(q).unwrap();
                    }
                };
                let img = image::open(&path).ok().map(|i| i.thumbnail(THUMB_PX, THUMB_PX));
                // the receiver is gone once the app exits.
                if tx.send((path, img)).is_err() { return; }
            });
        }
        Self { requests, res_rx, entries: HashMap::new(), clock: 0 }
    }

    /// pull finished decodes in; returns true when a new thumbnail became ready.
    pub fn poll(&mut self, picker: Option<&Picker>) -> bool {
        let mut any = false;
        while let Ok((path, img)) = self.res_rx.try_recv() {
            // dropped by `clear` while decoding: the result is stale.
            let Some(entry) = self.entries.get_mut(&path) else { continue };
            entry.thumb = match (img, picker) {
                (Some(img), Some(p)) => {
                    let aspect = if img.height() > 0 { img.width() as f32 / img.height() as f32 } else { 1.0 };
                    any = true;
                    Thumb::Ready { proto: p.new_resize_protocol(img), aspect }
                }
                _ => Thumb::Failed,
            };
        }
        if any { self.evict(); }
        any
    }

    /// the thumbnail for `path`, queueing a decode the first time it is asked for.
    pub fn get(&mut self, path: &str) -> &mut Thumb {
        self.clock += 1;
        if !self.entries.contains_key(path) {
            self.request(path);
            self.entries.insert(path.to_string(), Entry { thumb: Thumb::Pending, last_used: 0 });
        }
        let entry = self.entries.get_mut(path).unwrap();
        entry.last_used = self.clock;
        &mut entry.thumb
    }

    fn request(&mut self, path: &str) {
        let (lock, cvar) = &*self.requests;
        let mut q = lock.lock().unwrap();
        q.push_back(path.to_string());
        while q.len() > MAX_WAITING {
            if let Some(old) = q.pop_front() {
                self.entries.remove(&old);
            }
        }
        cvar.notify_one();
    }

    /// forget everything, e.g. after the image protocol changed.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.requests.0.lock().unwrap().clear();
    }

    fn evict(&mut self) {
        while self.entries.len() > MAX_THUMBS {
            let oldest = self.entries.iter()
                .filter(|(_, e)| !matches!(e.thumb, Thumb::Pending))
                .min_by_key(|(_, e)| e.last_used)
                .map(|(k, _)| k.clone());
            match oldest {
                Some(k) => { self.entries.remove(&k); }
                None    => break,
            }
        }
    }
}

/// a cover url as stored by the scanner/metadata (file:/// or a bare path) to a
/// filesystem path image::open understands.
pub(super) fn cover_url_to_path(url: &str) -> String {
    #[cfg(windows)]
    {
        if let Some(p) = url.strip_prefix("file:///") { return p.replace('/', "\\"); }
    }
    #[cfg(not(windows))]
    {
        if let Some(p) = url.strip_prefix("file:///") { return format!("/{}", p); }
        if let Some(p) = url.strip_prefix("file://") { return p.to_string(); }
    }
    url.to_string()
}
