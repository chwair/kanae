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

use cxx_qt_lib::{QGuiApplication, QQmlApplicationEngine, QUrl};

fn main() {
    let mut app = QGuiApplication::new();
    let mut engine = QQmlApplicationEngine::new();

    if let Some(engine) = engine.as_mut() {
        engine.load(&QUrl::from("qrc:/qt/qml/com/kdab/kanae/qml/main.qml"));
    }

    if let Some(app) = app.as_mut() {
        app.exec();
    }
}
