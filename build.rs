fn main() {
    // only link qt and generate cxx-qt bindings when the 'gui' feature is active.
    if std::env::var("CARGO_FEATURE_GUI").is_ok() {
        let mut builder = cxx_qt_build::CxxQtBuilder::new_qml_module(
            cxx_qt_build::QmlModule::new("com.kdab.kanae")
                .qml_file("qml/main.qml")
                .qml_file("qml/MatIcon.qml"),
        )
        .qrc("qml/resources.qrc")
        .qt_module("Network")
        .qt_module("Quick")
        .file("src/player.rs")
        .file("src/library_controller.rs")
        .file("src/cover_blur.rs");
        // msvc defaults to the system codepage (e.g. windows-1252) for source
        // files, which corrupts non-ascii text embedded in the c++ that
        // qmlcachegen generates from the .qml files. force utf-8.
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            builder = unsafe { builder.cc_builder(|cc| { cc.flag("/utf-8"); }) };
        }
        builder.build();
    }
}
