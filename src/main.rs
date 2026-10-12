// on windows, hide the console window in release gui-only builds.
// tui and hybrid builds stay console-subsystem: hybrid needs a console to detect a
// terminal launch (see detach_own_console).
#![cfg_attr(
    all(not(debug_assertions), feature = "gui", not(feature = "tui")),
    windows_subsystem = "windows"
)]

#[cfg(not(any(feature = "gui", feature = "tui")))]
compile_error!("At least one of the 'gui' or 'tui' features must be enabled.");

mod cd_reader;
mod audio_player;
mod file_player;
mod musicbrainz;
mod lrclib;
mod romaji;
mod lyric_cache;
mod library;
mod library_cache;
mod queue;

mod smtc;
mod discord;
#[cfg(feature = "gui")]
mod player;
#[cfg(feature = "gui")]
mod library_controller;
#[cfg(feature = "gui")]
mod cover_blur;
#[cfg(all(feature = "gui", windows))]
mod win_frame;

#[cfg(feature = "tui")]
mod tui;

#[cfg(feature = "gui")]
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

// needed to call is_terminal() in hybrid mode.
#[cfg(all(feature = "gui", feature = "tui"))]
use std::io::IsTerminal;

/// give the process an explicit appusermodelid and register a display name
/// for it, so the windows media flyout (smtc) shows "Kanae" instead of
/// "unknown app". must run before any window is created.
#[cfg(windows)]
fn setup_windows_app_identity() {
    const AUMID: &str = "Kanae.Player";
    // per-user registration (no admin): maps the AUMID to a display name.
    let _ = (|| -> std::io::Result<()> {
        use winreg::{enums::HKEY_CURRENT_USER, RegKey};
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(format!(r"Software\Classes\AppUserModelId\{}", AUMID))?;
        key.set_value("DisplayName", &"Kanae")
    })();
    #[link(name = "shell32")]
    extern "system" {
        fn SetCurrentProcessExplicitAppUserModelID(appid: *const u16) -> i32;
    }
    let wide: Vec<u16> = AUMID.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { SetCurrentProcessExplicitAppUserModelID(wide.as_ptr()); }
}

// hybrid helpers: decide desktop vs terminal at runtime

/// true when the user launched us from an interactive terminal (as opposed to
/// a double-click / desktop launcher / finder / explorer).
#[cfg(all(feature = "gui", feature = "tui"))]
fn launched_from_terminal() -> bool {
    if !std::io::stdout().is_terminal() {
        return false;
    }
    // on windows a double-clicked console-subsystem exe is given a brand-new
    // console, so stdout still looks like a terminal. if we are the only
    // process attached to the console, it was created for us -> desktop launch.
    #[cfg(windows)]
    unsafe {
        let mut pids = [0u32; 2];
        if winapi::um::wincon::GetConsoleProcessList(pids.as_mut_ptr(), 2) <= 1 {
            return false;
        }
    }
    true
}

/// close the auto-allocated console before showing the gui so the flash window
/// from a double-click launch disappears. a console shared with a shell (e.g.
/// `kanae --gui` from cmd) is kept, so logs stay visible there.
#[cfg(all(feature = "gui", feature = "tui", windows))]
fn detach_own_console() {
    unsafe {
        let mut pids = [0u32; 2];
        if winapi::um::wincon::GetConsoleProcessList(pids.as_mut_ptr(), 2) > 1 {
            return;
        }
        let was_console = console_backed_std_handles();
        winapi::um::wincon::FreeConsole();
        redirect_std_to_nul(was_console);
    }
}

/// point the console-backed standard handles at the nul device. FreeConsole
/// leaves them pointing at the console it just closed; writing to one of those
/// fails, and print!/eprintln! panic when the write fails, so the first log
/// line after the gui comes up would kill the process. (a windows-subsystem
/// build has null std handles, which std silently discards writes to - this
/// restores the same behaviour for a detached console build.) handles the
/// parent redirected to a file or pipe are left alone, so `kanae > log.txt`
/// still collects logs.
#[cfg(all(feature = "gui", feature = "tui", windows))]
unsafe fn redirect_std_to_nul(was_console: [bool; 3]) {
    use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::processenv::SetStdHandle;
    use winapi::um::winbase::{STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE};

    let nul: Vec<u16> = "NUL\0".encode_utf16().collect();
    let ids = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
    for (i, id) in ids.into_iter().enumerate() {
        if !was_console[i] {
            continue;
        }
        // one handle per slot, so closing one later can't affect the others
        let h = CreateFileW(
            nul.as_ptr(),
            GENERIC_READ | GENERIC_WRITE,
            FILE_SHARE_READ | FILE_SHARE_WRITE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            0,
            std::ptr::null_mut(),
        );
        if h != INVALID_HANDLE_VALUE {
            SetStdHandle(id, h);
        }
    }
}

/// which of stdin/stdout/stderr are backed by the console we are about to free.
#[cfg(all(feature = "gui", feature = "tui", windows))]
unsafe fn console_backed_std_handles() -> [bool; 3] {
    use winapi::um::consoleapi::GetConsoleMode;
    use winapi::um::processenv::GetStdHandle;
    use winapi::um::winbase::{STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};

    let ids = [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE];
    let mut out = [false; 3];
    for (i, id) in ids.into_iter().enumerate() {
        let h = GetStdHandle(id);
        let mut mode = 0u32;
        out[i] = !h.is_null() && GetConsoleMode(h, &mut mode) != 0;
    }
    out
}

fn main() {
    #[cfg(windows)]
    setup_windows_app_identity();

    // hybrid: tui in a terminal, gui otherwise; --gui / --tui override
    #[cfg(all(feature = "gui", feature = "tui"))]
    {
        let force_gui = std::env::args().any(|a| a == "--gui" || a == "-g");
        let force_tui = std::env::args().any(|a| a == "--tui" || a == "-t");
        if !force_gui && (force_tui || launched_from_terminal()) {
            if let Err(e) = tui::run_tui() {
                eprintln!("TUI error: {}", e);
                std::process::exit(1);
            }
            return;
        }
        #[cfg(windows)]
        detach_own_console();
    }

    // tui-only build
    #[cfg(all(not(feature = "gui"), feature = "tui"))]
    {
        if let Err(e) = tui::run_tui() {
            eprintln!("TUI error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // gui (gui-only build, or hybrid falling through to qt)
    #[cfg(feature = "gui")]
    {
        let mut app = QGuiApplication::new();
        let mut engine = QQmlApplicationEngine::new();

        if let Some(engine) = engine.as_mut() {
            engine.load(&QUrl::from("qrc:/qt/qml/com/kdab/kanae/qml/main.qml"));
        }
        #[cfg(windows)]
        win_frame::install();

        if let Some(app) = app.as_mut() {
            app.exec();
        }
    }
}

// pull in the generated cxx-qt code for the library controller.
#[cfg(feature = "gui")]
use library_controller::library_bridge;
