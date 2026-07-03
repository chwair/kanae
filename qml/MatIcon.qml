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
        "settings":      "\ue8b8",
        "close":         "\ue5cd",
        "minimize":      "\ue15b",
        "maximize":      "\ue3c6",
        "restore":       "\ue3e0",
        "chevron-left":  "\ue5cb",
        "chevron-right": "\ue5cc",
        "grid":          "\ue9b0",
        "list":          "\ue896",
        "folder":        "\ue2c7",
        "folder-plus":   "\ue2cc",
        "trash":         "\ue92e",
        "refresh":       "\ue5d5",
        "prev":          "\ue045",
        "play":          "\ue037",
        "pause":         "\ue034",
        "next":          "\ue044",
        "volume":        "\ue050",
        "volume-low":    "\ue04d",
        "volume-mute":   "\ue04f",
        "eject":         "\ue8fb",
        "album":         "\ue019"
    })
}
