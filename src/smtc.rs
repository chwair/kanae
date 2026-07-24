use std::sync::{Arc, Mutex};
use std::time::Duration;

pub use souvlaki::{MediaControlEvent, MediaControls, MediaMetadata, MediaPlayback, MediaPosition};
use souvlaki::PlatformConfig;

#[derive(Debug)]
pub enum SmtcCommand {
    Toggle,
    Next,
    Previous,
    Seek(f64),
}

#[allow(dead_code)]
pub enum SmtcUpdate {
    Metadata {
        title:     String,
        artist:    String,
        album:     String,
        cover_url: Option<String>,
        duration:  Option<Duration>,
    },
    Playing  { progress: Duration },
    Paused   { progress: Duration },
    Stopped,
}

pub struct SmtcHandle {
    controls: MediaControls,
    commands: Arc<Mutex<Vec<SmtcCommand>>>,
}

// SAFETY: souvlaki's Windows backend uses IAgileObject COM — safe as long as
// all methods are called on the Qt main thread.
unsafe impl Send for SmtcHandle {}
unsafe impl Sync for SmtcHandle {}

impl SmtcHandle {
    pub fn update(&mut self, u: SmtcUpdate) {
        match u {
            SmtcUpdate::Metadata { title, artist, album, cover_url, duration } => {
                eprintln!("[smtc] set_metadata {:?} – {:?}", artist, title);
                // Pass http(s) URLs as-is. Local covers need per-platform shapes
                // (souvlaki 0.8.3 behaviour):
                // - Windows strips "file://" and feeds the rest to
                //   GetFileFromPathAsync, so it must be "file://" + a native
                //   C:\ path — "file:///C:/…" yields the bogus "path too long".
                // - macOS + Linux (MPRIS) both feed the string to
                //   `[NSURL URLWithString:]` / GIO, which require a real,
                //   percent-encoded `file://` URI *with* a scheme. A bare path
                //   (`/Users/…`) produces a nil NSURL → nil NSImage, and
                //   souvlaki then aborts the process from its async artwork task
                //   (non-unwinding panic in `msg_send!(image, size)`). We also
                //   only emit the URL when the file actually exists, so a missing
                //   cover can never reach that nil-image abort path.
                let normalised: Option<String> = cover_url.as_deref().and_then(|u| {
                    if u.starts_with("http") {
                        Some(u.to_string())
                    } else if let Some(p) = u.strip_prefix("file:///") {
                        #[cfg(target_os = "windows")]
                        { Some(format!("file://{}", p.replace('/', "\\"))) }
                        #[cfg(not(target_os = "windows"))]
                        { file_uri_if_exists(&format!("/{}", p)) }
                    } else if u.starts_with('/') {
                        // Bare absolute POSIX path.
                        #[cfg(target_os = "windows")]
                        { None }
                        #[cfg(not(target_os = "windows"))]
                        { file_uri_if_exists(u) }
                    } else {
                        None
                    }
                });
                eprintln!("[smtc] cover_url={:?}", normalised);
                let res = self.controls.set_metadata(MediaMetadata {
                    title:     Some(title.as_str()),
                    artist:    Some(artist.as_str()),
                    album:     Some(album.as_str()),
                    cover_url: normalised.as_deref(),
                    duration,
                });
                match res {
                    Ok(()) => eprintln!("[smtc] set_metadata OK"),
                    Err(e) => {
                        eprintln!("[smtc] set_metadata ERR: {:?}", e);
                        // If cover loading caused the failure, retry without it so
                        // text metadata still appears in SMTC.
                        if normalised.is_some() {
                            eprintln!("[smtc] retrying without cover art");
                            if let Err(e2) = self.controls.set_metadata(MediaMetadata {
                                title:     Some(title.as_str()),
                                artist:    Some(artist.as_str()),
                                album:     Some(album.as_str()),
                                cover_url: None,
                                duration,
                            }) {
                                eprintln!("[smtc] retry ERR: {:?}", e2);
                            }
                        }
                    }
                }
            }
            SmtcUpdate::Playing { progress } => {
                if let Err(e) = self.controls.set_playback(MediaPlayback::Playing {
                    progress: Some(MediaPosition(progress)),
                }) { eprintln!("[smtc] Playing ERR: {:?}", e); }
            }
            SmtcUpdate::Paused { progress } => {
                eprintln!("[smtc] Paused {:.1}s", progress.as_secs_f64());
                if let Err(e) = self.controls.set_playback(MediaPlayback::Paused {
                    progress: Some(MediaPosition(progress)),
                }) { eprintln!("[smtc] Paused ERR: {:?}", e); }
            }
            SmtcUpdate::Stopped => {
                eprintln!("[smtc] Stopped");
                if let Err(e) = self.controls.set_playback(MediaPlayback::Stopped) {
                    eprintln!("[smtc] Stopped ERR: {:?}", e);
                }
            }
        }
    }

    pub fn drain_commands(&self) -> Vec<SmtcCommand> {
        std::mem::take(&mut self.commands.lock().unwrap())
    }
}

/// Build a `file://` URI for an absolute path, but only if the file exists.
///
/// Returning `None` for a missing file is a safety guard: souvlaki 0.8.3's
/// macOS backend loads cover art on a background GCD queue and does not
/// null-check the resulting `NSImage`, so a path it can't read leads to a
/// `msg_send!(nil, size)` that aborts the whole process (non-unwinding panic).
#[cfg(not(target_os = "windows"))]
fn file_uri_if_exists(abs_path: &str) -> Option<String> {
    if std::path::Path::new(abs_path).is_file() {
        Some(format!("file://{}", encode_uri_path(abs_path)))
    } else {
        eprintln!("[smtc] cover file missing, skipping artwork: {}", abs_path);
        None
    }
}

/// Percent-encode a filesystem path for use inside a file:// URI.
/// Keeps `/` and `:`, encodes everything else outside the RFC 3986 unreserved
/// set (cover paths are never pre-encoded, so `%` is encoded too).
#[cfg(not(target_os = "windows"))]
fn encode_uri_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for b in path.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9'
            | b'-' | b'.' | b'_' | b'~' | b'/' | b':' => out.push(b as char),
            _ => out.push_str(&format!("%{:02X}", b)),
        }
    }
    out
}

/// On macOS TUI mode: briefly pump the main run loop so MPRemoteCommandCenter
/// can deliver queued media-key callbacks. Safe to call every tick; returns
/// immediately when there is nothing pending.
pub fn pump_runloop() {
    #[cfg(target_os = "macos")]
    unsafe {
        use objc::{class, msg_send, sel, sel_impl};
        use objc::runtime::Object;
        // [NSDate dateWithTimeIntervalSinceNow: 0.0]  →  returns "now"
        let date_cls = class!(NSDate);
        let now: *mut Object = msg_send![date_cls, dateWithTimeIntervalSinceNow: 0.0f64];
        // [[NSRunLoop mainRunLoop] runUntilDate: now]  →  drains pending work, non-blocking
        let rl_cls = class!(NSRunLoop);
        let rl: *mut Object = msg_send![rl_cls, mainRunLoop];
        let _: () = msg_send![rl, runUntilDate: now];
    }
}

/// On macOS: initialise NSApplication once so the process is registered with
/// the system media server (required for Now Playing / MPRemoteCommandCenter).
#[cfg(target_os = "macos")]
fn ensure_nsapplication(regular_app: bool) {
    use std::sync::Once;
    static ONCE: Once = Once::new();
    ONCE.call_once(|| unsafe {
        use objc::{class, msg_send, sel, sel_impl};
        use objc::runtime::Object;
        let cls = class!(NSApplication);
        let app: *mut Object = msg_send![cls, sharedApplication];
        // Regular (0): normal GUI app, visible in Dock.
        // Accessory (1): background/accessory app, no Dock icon/window binding.
        let policy = if regular_app { 0i64 } else { 1i64 };
        let _: () = msg_send![app, setActivationPolicy: policy];
        eprintln!(
            "[smtc] NSApplication initialized as {} app",
            if regular_app { "regular" } else { "accessory" }
        );
    });
}

fn init_with_mode(regular_app_on_macos: bool) -> Option<SmtcHandle> {
    eprintln!("[smtc] init() called");

    #[cfg(target_os = "macos")]
    ensure_nsapplication(regular_app_on_macos);

    #[cfg(target_os = "windows")]
    set_app_user_model_id("com.chair.kanae");
    #[cfg(target_os = "windows")]
    let hwnd_raw: *mut std::ffi::c_void = {
        let mut h = find_main_hwnd();
        for _ in 0..20 {
            if h != 0 { break; }
            std::thread::sleep(Duration::from_millis(50));
            h = find_main_hwnd();
        }
        if h == 0 {
            eprintln!("[smtc] no HWND found; SMTC disabled");
            return None;
        }
        eprintln!("[smtc] using HWND 0x{:X}", h);
        h as *mut std::ffi::c_void
    };

    let config = PlatformConfig {
        dbus_name:    "com.chair.kanae",
        display_name: "Kanae",
        #[cfg(target_os = "windows")]
        hwnd: Some(hwnd_raw),
        #[cfg(not(target_os = "windows"))]
        hwnd: None,
    };

    let mut controls = match MediaControls::new(config) {
        Ok(c)  => { eprintln!("[smtc] MediaControls::new OK"); c }
        Err(e) => { eprintln!("[smtc] MediaControls::new ERR: {:?}", e); return None; }
    };

    let commands = Arc::new(Mutex::new(Vec::<SmtcCommand>::new()));
    let cmds = commands.clone();

    match controls.attach(move |event: MediaControlEvent| {
        eprintln!("[smtc] event: {:?}", event);
        let cmd = match event {
            MediaControlEvent::Play
            | MediaControlEvent::Pause
            | MediaControlEvent::Toggle  => Some(SmtcCommand::Toggle),
            MediaControlEvent::Next      => Some(SmtcCommand::Next),
            MediaControlEvent::Previous  => Some(SmtcCommand::Previous),
            MediaControlEvent::SetPosition(pos) => {
                Some(SmtcCommand::Seek(pos.0.as_secs_f64()))
            }
            _ => None,
        };
        if let Some(c) = cmd { cmds.lock().unwrap().push(c); }
    }) {
        Ok(()) => eprintln!("[smtc] attach OK"),
        Err(e) => { eprintln!("[smtc] attach ERR: {:?}", e); return None; }
    }

    match controls.set_playback(MediaPlayback::Stopped) {
        Ok(()) => eprintln!("[smtc] initial Stopped OK"),
        Err(e) => eprintln!("[smtc] initial Stopped ERR: {:?}", e),
    }

    eprintln!("[smtc] ready");
    Some(SmtcHandle {
        controls,
        commands,
    })
}

pub fn init_for_gui() -> Option<SmtcHandle> {
    init_with_mode(true)
}

pub fn init_for_tui() -> Option<SmtcHandle> {
    init_with_mode(false)
}

pub fn init() -> Option<SmtcHandle> {
    init_for_gui()
}

#[cfg(target_os = "windows")]
fn set_app_user_model_id(id: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    let wide: Vec<u16> = OsStr::new(id).encode_wide().chain(std::iter::once(0)).collect();
    unsafe {
        #[link(name = "Shell32")]
        extern "system" {
            fn SetCurrentProcessExplicitAppUserModelID(id: *const u16) -> i32;
        }
        let hr = SetCurrentProcessExplicitAppUserModelID(wide.as_ptr());
        eprintln!("[smtc] SetCurrentProcessExplicitAppUserModelID hr=0x{:X}", hr);
    }

    let hkcu = winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER);
    let key_path = format!("Software\\Classes\\AppUserModelId\\{}", id);
    match hkcu.create_subkey(&key_path) {
        Ok((key, _)) => {
            match key.set_value("DisplayName", &"Kanae") {
                Ok(()) => eprintln!("[smtc] registry DisplayName written OK"),
                Err(e) => eprintln!("[smtc] registry DisplayName write ERR: {}", e),
            }
        }
        Err(e) => eprintln!("[smtc] registry create_subkey ERR: {}", e),
    }
}

#[cfg(target_os = "windows")]
fn find_main_hwnd() -> usize {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static FOUND: AtomicUsize = AtomicUsize::new(0);
    FOUND.store(0, Ordering::SeqCst);

    unsafe extern "system" fn enum_cb(
        hwnd:   winapi::shared::windef::HWND,
        _param: winapi::shared::minwindef::LPARAM,
    ) -> winapi::shared::minwindef::BOOL {
        let cur_pid = winapi::um::processthreadsapi::GetCurrentProcessId();
        let mut win_pid: u32 = 0;
        winapi::um::winuser::GetWindowThreadProcessId(hwnd, &mut win_pid);
        let visible  = winapi::um::winuser::IsWindowVisible(hwnd) != 0;
        let no_owner = winapi::um::winuser::GetWindow(
            hwnd, winapi::um::winuser::GW_OWNER).is_null();

        let mut tb = [0u16; 256];
        let tl = winapi::um::winuser::GetWindowTextW(hwnd, tb.as_mut_ptr(), 256);
        let title = if tl > 0 {
            OsString::from_wide(&tb[..tl as usize]).to_string_lossy().into_owned()
        } else { String::new() };
        eprintln!("[smtc] hwnd=0x{:X} pid={} cur={} vis={} no_owner={} title={:?}",
            hwnd as usize, win_pid, cur_pid, visible, no_owner, title);

        if win_pid == cur_pid && visible && no_owner {
            FOUND.store(hwnd as usize, Ordering::SeqCst);
            eprintln!("[smtc] -> selected");
            return 0;
        }
        1
    }

    unsafe { winapi::um::winuser::EnumWindows(Some(enum_cb), 0) };
    let found = FOUND.load(Ordering::SeqCst);
    if found != 0 {
        return found;
    }
    // Fallback for TUI mode (no Qt window): use the console window.
    let console_hwnd = unsafe { winapi::um::wincon::GetConsoleWindow() };
    if !console_hwnd.is_null() {
        eprintln!("[smtc] using console HWND 0x{:X} as fallback", console_hwnd as usize);
        return console_hwnd as usize;
    }
    0
}

