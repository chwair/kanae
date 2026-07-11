// On Windows, hide the console window in release GUI builds.
// TUI and hybrid builds keep console access (hybrid falls back to GUI when not in a terminal).
#![cfg_attr(all(not(debug_assertions), feature = "gui"), windows_subsystem = "windows")]

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

mod smtc;
mod discord;
#[cfg(feature = "gui")]
mod player;
#[cfg(feature = "gui")]
mod library_controller;

#[cfg(feature = "tui")]
mod tui;

#[cfg(feature = "gui")]
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

// Needed to call is_terminal() in hybrid mode.
#[cfg(all(feature = "gui", feature = "tui"))]
use std::io::IsTerminal;

/// Give the process an explicit AppUserModelID and register a display name
/// for it, so the Windows media flyout (SMTC) shows "Kanae" instead of
/// "Unknown app". Must run before any window is created.
#[cfg(windows)]
fn setup_windows_app_identity() {
    const AUMID: &str = "Kanae.Player";
    // Per-user registration (no admin): maps the AUMID to a display name.
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

fn main() {
    #[cfg(windows)]
    setup_windows_app_identity();

    // ── Hybrid: prefer TUI when running in a terminal; --gui overrides ────
    #[cfg(all(feature = "gui", feature = "tui"))]
    if !std::env::args().any(|a| a == "--gui" || a == "-g")
        && std::io::stdout().is_terminal()
    {
        if let Err(e) = tui::run_tui() {
            eprintln!("TUI error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // ── TUI-only build ────────────────────────────────────────────────────
    #[cfg(all(not(feature = "gui"), feature = "tui"))]
    {
        if let Err(e) = tui::run_tui() {
            eprintln!("TUI error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    // ── GUI (gui-only build, or hybrid falling through to Qt) ─────────────
    #[cfg(feature = "gui")]
    {
        let mut app = QGuiApplication::new();
        let mut engine = QQmlApplicationEngine::new();

        if let Some(engine) = engine.as_mut() {
            engine.load(&QUrl::from("qrc:/qt/qml/com/kdab/kanae/qml/main.qml"));
        }

        if let Some(app) = app.as_mut() {
            app.exec();
        }
    }
}

// Pull in the generated CXX-Qt code for the library controller.
#[cfg(feature = "gui")]
use library_controller::library_bridge;
