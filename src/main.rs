// Hide the console window in release builds; keep it in debug for logging.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cd_reader;
mod audio_player;
mod file_player;
mod musicbrainz;
mod lrclib;
mod lyric_cache;
mod smtc;
mod player;
mod library;
mod library_cache;
mod library_controller;
mod tui;

use std::io::IsTerminal;
use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let force_gui = args.iter().any(|a| a == "--gui" || a == "-g");

    // If stdout is a terminal and we haven't been asked to force the GUI, use TUI.
    if !force_gui && std::io::stdout().is_terminal() {
        if let Err(e) = tui::run_tui() {
            eprintln!("TUI error: {}", e);
            std::process::exit(1);
        }
        return;
    }

    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/com/kdab/kanae/qml/main.qml"));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}

// Pull in the generated CXX-Qt code for the library controller.
use library_controller::library_bridge;
