/// blurhash placeholders for cover art: the qml item that paints one, plus the
/// cache and background worker that generate them.
use core::pin::Pin;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use cxx_qt_lib::{QImage, QImageFormat, QPainterRenderHint, QRect, QString};

/// side length of a decoded placeholder. the item is stretched by the gpu, so
/// this only has to be enough for the few low frequencies a blurhash carries.
const BLUR_PX: u32 = 32;
/// breather between two covers so the backlog never competes with playback
/// or with qml decoding the covers that are on screen.
const WORKER_PAUSE: Duration = Duration::from_millis(50);
/// new hashes between two writes of the cache file while the backlog drains.
const SAVE_EVERY: usize = 32;

#[cxx_qt::bridge]
pub mod blur_bridge {
    unsafe extern "C++" {
        include!("cxx-qt-lib/qstring.h");
        type QString = cxx_qt_lib::QString;
        include!("cxx-qt-lib/qsizef.h");
        type QSizeF = cxx_qt_lib::QSizeF;
        include!("cxx-qt-lib/qpainter.h");
        type QPainter = cxx_qt_lib::QPainter;
        include!(<QtQuick/QQuickPaintedItem>);
        type QQuickPaintedItem;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[base = QQuickPaintedItem]
        #[qml_element]
        #[qproperty(QString, source)]
        #[qproperty(bool, ready)]
        type BlurhashItem = super::BlurhashItemRust;

        #[cxx_override]
        unsafe fn paint(self: Pin<&mut Self>, painter: *mut QPainter);

        #[inherit]
        fn size(self: &Self) -> QSizeF;
        #[inherit]
        fn update(self: Pin<&mut Self>);
    }

    impl cxx_qt::Initialize for BlurhashItem {}
}

/// paints the blurhash of the cover at `source` (a `file://` url). `ready` is
/// false while no hash exists for it yet, in which case nothing is drawn.
#[derive(Default)]
pub struct BlurhashItemRust {
    source: QString,
    ready:  bool,
    image:  Option<QImage>,
}

impl cxx_qt::Initialize for blur_bridge::BlurhashItem {
    fn initialize(self: Pin<&mut Self>) {
        self.on_source_changed(|item| item.reload()).release();
    }
}

impl blur_bridge::BlurhashItem {
    fn reload(mut self: Pin<&mut Self>) {
        use cxx_qt::CxxQtType;
        let url = self.source().to_string();
        let image = lookup(&url)
            .and_then(|hash| blurhash::decode(&hash, BLUR_PX, BLUR_PX, 1.0).ok())
            // safety: decode returns exactly BLUR_PX * BLUR_PX rgba pixels
            .map(|px| unsafe {
                QImage::from_raw_bytes(px, BLUR_PX as i32, BLUR_PX as i32, QImageFormat::Format_RGBA8888)
            });
        self.as_mut().set_ready(image.is_some());
        self.as_mut().rust_mut().image = image;
        self.update();
    }

    unsafe fn paint(self: Pin<&mut Self>, painter: *mut blur_bridge::QPainter) {
        let Some(painter) = painter.as_mut() else { return };
        let mut painter = Pin::new_unchecked(painter);
        if let Some(image) = &self.image {
            let size = self.size();
            painter.as_mut().set_render_hint(QPainterRenderHint::SmoothPixmapTransform, true);
            painter.draw_image(&QRect::new(0, 0, size.width().ceil() as i32, size.height().ceil() as i32), image);
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct Entry {
    /// empty when the cover could not be decoded, so it is not retried.
    hash:  String,
    /// mtime of the cover the hash was made from, to notice a replaced image.
    mtime: u64,
}

#[derive(Default)]
struct Store {
    entries: HashMap<String, Entry>,
    queue:   VecDeque<String>,
    /// urls already queued this session.
    seen:    HashSet<String>,
    worker_running: bool,
    unsaved: usize,
}

fn store() -> &'static Mutex<Store> {
    static STORE: OnceLock<Mutex<Store>> = OnceLock::new();
    STORE.get_or_init(|| {
        let entries = std::fs::read_to_string(crate::library_cache::blurhash_cache_path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Mutex::new(Store { entries, ..Default::default() })
    })
}

/// queue covers for hashing behind everything already waiting. cheap to call
/// repeatedly: a url is only ever queued once per session.
pub fn enqueue(urls: impl IntoIterator<Item = String>) {
    let mut st = store().lock().unwrap();
    for url in urls {
        if url.starts_with("file://") && !st.seen.contains(&url) {
            st.seen.insert(url.clone());
            st.queue.push_back(url);
        }
    }
    ensure_worker(&mut st);
}

/// the cached hash for a cover, if one exists yet. a cover not seen before is
/// on screen right now, so it jumps ahead of the library backlog.
fn lookup(url: &str) -> Option<String> {
    if !url.starts_with("file://") { return None; }
    let mut st = store().lock().unwrap();
    if !st.seen.contains(url) {
        st.seen.insert(url.to_string());
        st.queue.push_front(url.to_string());
        ensure_worker(&mut st);
    }
    st.entries.get(url).filter(|e| !e.hash.is_empty()).map(|e| e.hash.clone())
}

fn ensure_worker(st: &mut Store) {
    if st.worker_running || st.queue.is_empty() { return; }
    st.worker_running = true;
    std::thread::spawn(worker);
}

/// drains the queue one cover at a time, then exits; `ensure_worker` starts a
/// fresh one when more work shows up.
fn worker() {
    lower_thread_priority();
    loop {
        let url = {
            let mut st = store().lock().unwrap();
            match st.queue.pop_front() {
                Some(url) => url,
                None => {
                    st.worker_running = false;
                    let json = take_unsaved(&mut st, 1);
                    drop(st);
                    write_cache(json);
                    return;
                }
            }
        };
        let path = url_to_path(&url);
        let Some(mtime) = mtime_secs(&path) else { continue };
        // an up-to-date entry costs one stat and no pause
        if store().lock().unwrap().entries.get(&url).is_some_and(|e| e.mtime == mtime) { continue; }

        let hash = hash_cover(&path).unwrap_or_default();
        let json = {
            let mut st = store().lock().unwrap();
            st.entries.insert(url, Entry { hash, mtime });
            st.unsaved += 1;
            take_unsaved(&mut st, SAVE_EVERY)
        };
        write_cache(json);
        std::thread::sleep(WORKER_PAUSE);
    }
}

/// the serialised cache once at least `threshold` entries are unsaved. the
/// caller writes it after releasing the lock.
fn take_unsaved(st: &mut Store, threshold: usize) -> Option<String> {
    if st.unsaved < threshold { return None; }
    st.unsaved = 0;
    serde_json::to_string(&st.entries).ok()
}

fn write_cache(json: Option<String>) {
    if let Some(json) = json {
        let _ = std::fs::write(crate::library_cache::blurhash_cache_path(), json);
    }
}

fn hash_cover(path: &Path) -> Option<String> {
    let img = image::ImageReader::open(path).ok()?.with_guessed_format().ok()?.decode().ok()?;
    // a blurhash only keeps a handful of frequencies, so a thumbnail is plenty
    // and keeps the encode itself to a few thousand pixels.
    let small = img.thumbnail(BLUR_PX, BLUR_PX).to_rgba8();
    blurhash::encode(4, 3, small.width(), small.height(), small.as_raw()).ok()
}

fn mtime_secs(path: &Path) -> Option<u64> {
    let mtime = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(mtime.duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0))
}

fn url_to_path(url: &str) -> PathBuf {
    match url.strip_prefix("file:///") {
        // windows:  file:///C:/...  -> C:/...
        // unix:     file:///home/... -> /home/...  (re-add the leading /)
        #[cfg(target_os = "windows")]
        Some(p) => PathBuf::from(p),
        #[cfg(not(target_os = "windows"))]
        Some(p) => PathBuf::from(format!("/{}", p)),
        None => PathBuf::from(url.strip_prefix("file://").unwrap_or(url)),
    }
}

/// decoding a large cover pegs a core for a moment; keep that below the audio
/// and ui threads.
fn lower_thread_priority() {
    #[cfg(windows)]
    unsafe {
        use winapi::um::processthreadsapi::{GetCurrentThread, SetThreadPriority};
        SetThreadPriority(GetCurrentThread(), winapi::um::winbase::THREAD_PRIORITY_LOWEST as i32);
    }
}
