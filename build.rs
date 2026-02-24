use cxx_qt_build::{CxxQtBuilder, QmlModule};

fn main() {
    CxxQtBuilder::new_qml_module(
        QmlModule::new("com.kdab.kanae")
            .qml_file("qml/main.qml")
    )
    .qt_module("Network")
    .file("src/player.rs")
    .build();
}
