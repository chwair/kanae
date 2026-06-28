fn main() {
    // Only link Qt and generate CXX-Qt bindings when the 'gui' feature is active.
    if std::env::var("CARGO_FEATURE_GUI").is_ok() {
        cxx_qt_build::CxxQtBuilder::new_qml_module(
            cxx_qt_build::QmlModule::new("com.kdab.kanae")
                .qml_file("qml/main.qml")
                .qml_file("qml/MatIcon.qml"),
        )
        .qrc("qml/resources.qrc")
        .qt_module("Network")
        .file("src/player.rs")
        .file("src/library_controller.rs")
        .build();
    }
}
