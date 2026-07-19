// On Windows, hide the console window in release GUI-only builds.
// TUI and hybrid builds stay console-subsystem: hybrid needs a console to
// detect being run from a terminal (it frees the console again before showing
// the GUI — see detach_own_console below).
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

// ── Hybrid helpers: decide desktop vs terminal at runtime ────────────────────

/// True when the user launched us from an interactive terminal (as opposed to
/// a double-click / desktop launcher / Finder / Explorer).
#[cfg(all(feature = "gui", feature = "tui"))]
fn launched_from_terminal() -> bool {
    if !std::io::stdout().is_terminal() {
        return false;
    }
    // On Windows a double-clicked console-subsystem exe is given a brand-new
    // console, so stdout still looks like a terminal. If we are the only
    // process attached to the console, it was created for us → desktop launch.
    #[cfg(windows)]
    unsafe {
        let mut pids = [0u32; 2];
        if winapi::um::wincon::GetConsoleProcessList(pids.as_mut_ptr(), 2) <= 1 {
            return false;
        }
    }
    true
}

/// Close the auto-allocated console before showing the GUI so the flash window
/// from a double-click launch disappears. A console shared with a shell (e.g.
/// `kanae --gui` from cmd) is kept, so logs stay visible there.
#[cfg(all(feature = "gui", feature = "tui", windows))]
fn detach_own_console() {
    unsafe {
        let mut pids = [0u32; 2];
        if winapi::um::wincon::GetConsoleProcessList(pids.as_mut_ptr(), 2) <= 1 {
            winapi::um::wincon::FreeConsole();
        }
    }
}

fn main() {
    // ── Hybrid: TUI in a terminal, GUI otherwise; --gui / --tui override ──
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
