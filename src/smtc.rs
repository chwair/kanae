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
                // Pass http(s) URLs as-is.
                // Windows: WinRT requires the three-slash form file:///C:/path — pass as-is.
                // macOS/Linux: souvlaki expects a file:// URI; convert bare absolute paths.
                let normalised: Option<String> = cover_url.as_deref().and_then(|u| {
                    if u.starts_with("http") {
                        Some(u.to_string())
                    } else if u.starts_with("file:///") {
                        Some(u.to_string())
                    } else if u.starts_with('/') {
                        // Bare absolute POSIX path → file:// URI (macOS/Linux only)
                        #[cfg(not(target_os = "windows"))]
                        { Some(format!("file://{}", u)) }
                        #[cfg(target_os = "windows")]
                        { None }
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
                eprintln!("[smtc] Playing {:.1}s", progress.as_secs_f64());
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

pub fn init() -> Option<SmtcHandle> {
    println!("[smtc] smtc::init() reached");
    eprintln!("[smtc] init() called");

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
    FOUND.load(Ordering::SeqCst)
}

