import QtQuick 2.15

Text {
    id: root

    property string name: ""
    property real   size: 12

    width: size
    height: size
    font.family: _matFont.status === FontLoader.Ready ? _matFont.name : "Material Symbols Sharp"
    font.pixelSize: size
    color: "#dfdfdf"
    text: _glyphs[name] || ""
    horizontalAlignment: Text.AlignHCenter
    verticalAlignment: Text.AlignVCenter
    renderType: Text.QtRendering
    antialiasing: true

    FontLoader { id: _matFont; source: "qrc:/fonts/MaterialSymbolsSharp-Filled.ttf" }

    // Internal icon name → Material Symbols codepoint.
    readonly property var _glyphs: ({
        "settings":      "",
        "close":         "",
        "minimize":      "",
        "maximize":      "",
        "restore":       "",
        "chevron-left":  "",
        "chevron-right": "",
        "grid":          "",
        "list":          "",
        "folder":        "",
        "folder-plus":   "",
        "trash":         "",
        "refresh":       "",
        "prev":          "",
        "play":          "",
        "pause":         "",
        "next":          "",
        "volume":        "",
        "volume-low":    "",
        "volume-mute":   ""
    })
}
