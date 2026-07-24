import QtQuick 2.15
import QtQuick.Controls.Basic 2.15
import QtQuick.Layouts 1.15
import QtQuick.Shapes 1.15
import com.kdab.kanae 1.0

ApplicationWindow {
    id: window
    visible: true
    width: 720
    height: 580
    minimumWidth: 720
    minimumHeight: 540
    maximumWidth: 1920
    maximumHeight: 1080
    title: "Kanae"
    flags: Qt.FramelessWindowHint | Qt.Window
    color: "transparent"

    // ── Palette ────────────────────────────────────────────────────────────
    readonly property color clrBg:      "#0f0f0f"
    readonly property color clrSurface: "#161616"
    readonly property color clrSurf2:   "#1e1e1e"
    readonly property color clrBorder:  "#282828"
    readonly property color clrText:    "#dfdfdf"
    readonly property color clrText2:   "#686868"
    readonly property color clrMuted:   "#404040"
    readonly property color clrAccent:  "#bfbfbf"

    property bool coverOnSide: false
    property bool _resizing: false
    Timer { id: resizeEndTimer; interval: 50; onTriggered: window._resizing = false }
    onWidthChanged:  { _resizing = true; resizeEndTimer.restart() }
    onHeightChanged: { _resizing = true; resizeEndTimer.restart() }

    // ── Backends ───────────────────────────────────────────────────────────
    PlayerController   { id: player }
    LibraryController  { id: library }

    // ── View state ─────────────────────────────────────────────────────────
    // "library" – browsable library grid in right panel
    // "album"   – track list in right panel
    property string _view:          "library"
    property bool   _fileMode:      false
    property bool   _showingCdView: false  // true when album view should show CD state (not file tracks)

    // When browsing a library album without yet loading it into the player:
    property string _browseDir: ""        // dir being previewed; "" = player owns the track list
    property string _browseAlbumName: "" // display name for the breadcrumb

    // Tracks for the currently browsed library album (populated by library.browseAlbum).
    property var _browseTracks: {
        var j = library.album_tracks_json.toString()
        if (!j || j === "[]") return []
        try { return JSON.parse(j) } catch(e) { return [] }
    }

    // Unified track list model: use browse data when previewing, player data when playing.
    property var _effectiveTracklist: {
        if (_browseDir !== "" && _browseTracks.length > 0) return _browseTracks
        if (player.total_tracks <= 0) return []
        var arr = []; var n = player.total_tracks
        for (var i = 0; i < n; i++) {
            arr.push({
                title:    (player.track_titles[i]  || "").toString(),
                artist:   (player.track_artists[i] || "").toString(),
                duration: (player.track_names[i]   || "").toString()
            })
        }
        return arr
    }

    // Grid/list toggle for library browser
    property bool _libUseGrid: true

    // Navigation state for forward-to-album
    property string _prevBrowseDir: ""
    property string _prevBrowseAlbumName: ""
    property bool   _prevFileMode: false
    property bool   _prevShowingCdView: false
    property bool   _canGoForwardToAlbum: false

    // Mouse back/forward navigation
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.BackButton | Qt.ForwardButton
        onPressed: function(mouse) {
            if (mouse.button === Qt.BackButton)    { goBack();    mouse.accepted = true }
            if (mouse.button === Qt.ForwardButton) { goForward(); mouse.accepted = true }
        }
    }

    function goBack() {
        if (_view === "album") {
            _prevFileMode = _fileMode; _prevBrowseDir = _browseDir; _prevBrowseAlbumName = _browseAlbumName
            _prevShowingCdView = _showingCdView
            _canGoForwardToAlbum = true
            _browseDir = ""; _browseAlbumName = ""
            _showingCdView = false
            _view = "library"
        } else {
            library.navigateBack()
        }
    }

    function goForward() {
        if (_view === "library" && _canGoForwardToAlbum) {
            _canGoForwardToAlbum = false
            _browseDir = _prevBrowseDir; _browseAlbumName = _prevBrowseAlbumName
            _fileMode = _prevFileMode; _showingCdView = _prevShowingCdView
            _view = "album"
            if (_browseDir !== "") library.browseAlbum(_browseDir)
        } else {
            library.navigateForward()
        }
    }

    function openAlbumDir(dir) {
        _canGoForwardToAlbum = false
        player.openDroppedPaths([dir])
        _view = "album"
    }

    // Called from the track list delegate to start playing a browsed library album.
    // All window-property mutations live here so delegate scope issues are avoided.
    property bool   _suppressFileModeFlag: false
    property string _playingAlbumDir: ""   // album dir currently loaded in the player from library
    function playBrowsedTrack(idx) {
        if (_browseDir !== "") {
            var paths = _browseTracks.map(function(t){ return t.path })
            _playingAlbumDir = _browseDir
            _browseDir = ""; _browseAlbumName = ""
            _suppressFileModeFlag = true
            player.openDroppedPaths(paths)
            _suppressFileModeFlag = false
            Qt.callLater(function() { player.loadTrack(idx); player.playPause() })
        } else {
            player.loadTrack(idx); player.playPause()
        }
    }

    // Breadcrumbs computed relative to library search roots;
    // appends album name when viewing an album's track list
    property var _crumbs: {
        var p = library.current_path.toString()
        var base
        if (p === "Library" || p === "") {
            base = [{name:"Library", path:""}]
        } else {
            try {
                var sj = JSON.parse(library.settings_json.toString())
                var roots = sj.search_paths || []
                var pp = p.replace(/\\/g, "/")
                var bestRoot = ""
                for (var i = 0; i < roots.length; i++) {
                    var r = roots[i].replace(/\\/g, "/")
                    if ((pp.startsWith(r + "/") || pp === r) && r.length > bestRoot.length) bestRoot = r
                }
                var result = [{name:"Library", path:""}]
                if (bestRoot.length > 0) {
                    var rest = pp.substring(bestRoot.length).replace(/^\//, "")
                    if (rest.length > 0) {
                        var segs = rest.split("/").filter(function(s){ return s.length > 0 })
                        var accum = bestRoot
                        segs.forEach(function(seg){ accum = accum + "/" + seg; result.push({name:seg, path:accum}) })
                    }
                } else {
                    var segs2 = pp.split(/[\/\\]/).filter(function(s){ return s.length > 0 })
                    var accum2 = ""
                    segs2.forEach(function(seg){ accum2 = accum2 + "/" + seg; result.push({name:seg, path:accum2}) })
                }
                base = result
            } catch(e) { base = [{name:"Library", path:""}] }
        }
        if (_view === "album") {
            var aName = _browseDir !== "" ? _browseAlbumName : player.album_title.toString()
            // For drag-and-drop / file mode, insert an unclickable 'Files' segment
            // between Library and the album name.
            if (_fileMode) {
                base = base.concat([{name: "Files", path: "__files__"}])
            }
            if (aName.length > 0) {
                return base.concat([{name: aName, path: "__album__"}])
            }
        }
        return base
    }

    // When pending_open_dir is set by the library controller
    Connections {
        target: library
        function onPending_open_dirChanged() {
            var d = library.pending_open_dir.toString()
            if (d.length > 0) { openAlbumDir(d); library.openAlbum("") }
        }
    }

    Connections {
        target: player
        function onIs_file_modeChanged() {
            if (player.is_file_mode && !window._suppressFileModeFlag) { _fileMode = true; _browseDir = ""; _browseAlbumName = ""; _view = "album" }
        }
    }

    // 100 ms position updates are only needed while audio is actually playing;
    // when paused/idle the tick just drains SMTC commands, so 300 ms is plenty.
    Timer { interval: player.is_playing ? 100 : 300; repeat: true; running: true; onTriggered: player.updatePosition() }
    Timer { interval: player.total_tracks > 0 ? 1000 : 3000; repeat: true
            running: !player.is_loading
            onTriggered: { if (player.drive_list.length === 0) player.scanDrives(); else player.checkDrive() } }
    // Fast polling only matters while a disc load or library scan is in flight.
    Timer { interval: player.is_loading || library.is_scanning ? 150 : 500
            repeat: true; running: true; onTriggered: { player.pollLoad(); library.pollScan() } }
    // Lyric results only arrive while a fetch is in flight.
    Timer { interval: 300; repeat: true; running: player.lyrics_loading; onTriggered: player.pollLyrics() }

    DropArea {
        anchors.fill: parent; keys: ["text/uri-list"]
        onDropped: {
            var urls = []
            for (var i = 0; i < drop.urls.length; i++) urls.push(drop.urls[i].toString())
            if (urls.length > 0) { player.openDroppedPaths(urls); drop.accept() }
        }
    }

    // ── Keyboard shortcuts ─────────────────────────────────────────────────
    property real _volBeforeMute: 1.0
    function togglePlayPause() {
        if (player.total_tracks > 0 && player.current_track >= 0) player.playPause()
    }
    // Mirrors the prev button: restart the track past 4s, otherwise skip back.
    function prevTrackOrRestart() {
        if (!(player.current_track > 0 || player.current_time > 4)) return
        if (player.current_time > 4) { player.seek(0) }
        else { var wp = player.is_playing; player.previousTrack(); if (wp) player.playPause() }
    }
    function nextTrackShortcut() {
        if (!(player.current_track >= 0 && player.current_track < player.total_tracks - 1)) return
        var wp = player.is_playing; player.nextTrack(); if (wp) player.playPause()
    }
    function volumeBy(delta) {
        var v = Math.max(0, Math.min(1, volSlider.value + delta))
        volSlider.value = v; player.setVolumeLevel(v)
    }
    function toggleMute() {
        if (volSlider.value > 0.001) { _volBeforeMute = volSlider.value; volSlider.value = 0; player.setVolumeLevel(0) }
        else { var v = _volBeforeMute > 0.001 ? _volBeforeMute : 1.0; volSlider.value = v; player.setVolumeLevel(v) }
    }

    // Disabled while the first-run "where's your music" field has focus so
    // typing a path doesn't trigger playback/volume shortcuts.
    property bool _shortcutsEnabled: !musicDirInput.activeFocus
    Shortcut { sequence: "Space"; enabled: window._shortcutsEnabled; onActivated: togglePlayPause() }
    Shortcut { sequence: "Left";  enabled: window._shortcutsEnabled; onActivated: prevTrackOrRestart() }
    Shortcut { sequence: "Right"; enabled: window._shortcutsEnabled; onActivated: nextTrackShortcut() }
    Shortcut { sequence: "Up";    enabled: window._shortcutsEnabled; onActivated: volumeBy(0.05) }
    Shortcut { sequence: "Down";  enabled: window._shortcutsEnabled; onActivated: volumeBy(-0.05) }
    Shortcut { sequence: "M";     enabled: window._shortcutsEnabled; onActivated: toggleMute() }
    Shortcut { sequence: "Ctrl+Left";  onActivated: goBack() }
    Shortcut { sequence: "Ctrl+Right"; onActivated: goForward() }
    Shortcut { sequence: "Ctrl+,"; onActivated: settingsWindow.show() }

    property int _lyricsTrackIdx: player.current_track
    on_LyricsTrackIdxChanged: {
        Qt.callLater(function() {
            var title  = (player.track_titles[player.current_track] || "").trim()
            var rawArt = (player.track_artists[player.current_track] || "").trim()
            var artist = rawArt.length > 0 ? rawArt : (player.album_artist || "").trim()
            if (title.length > 0 && player.album_title !== "Unknown Album")
                player.fetchLyrics(title, artist, player.album_title, player.total_time)
        })
    }

    // ── ScrollText component ──────────────────────────────────────────────
    component ScrollText: Item {
        id: stRoot
        implicitHeight: stLabel.implicitHeight
        clip: true
        onWidthChanged: _resetX()
        property string text: ""
        property color  textColor: clrText
        property real   pixelSize: 13
        property string fontFamily: "Segoe UI"
        property bool   bold: false
        property bool   centered: false
        readonly property bool scrolling: stLabel.implicitWidth > stRoot.width
        function _resetX() {
            stLabel.x = (stRoot.centered && !stRoot.scrolling)
                        ? Math.round((stRoot.width - stLabel.implicitWidth) / 2) : 0
        }
        Text {
            id: stLabel; text: stRoot.text; color: stRoot.textColor
            font.pixelSize: stRoot.pixelSize; font.family: stRoot.fontFamily; font.bold: stRoot.bold
            onImplicitWidthChanged: if (!stRoot.scrolling) stRoot._resetX()
            SequentialAnimation on x {
                loops: Animation.Infinite; running: stRoot.scrolling
                onRunningChanged: if (!running) stRoot._resetX()
                PauseAnimation  { duration: 2200 }
                NumberAnimation { to: -(stLabel.implicitWidth - stRoot.width + 8)
                                  duration: Math.max(500, (stLabel.implicitWidth - stRoot.width) * 32)
                                  easing.type: Easing.Linear }
                PauseAnimation  { duration: 1600 }
                NumberAnimation { to: 0; duration: 1100; easing.type: Easing.InOutQuart }
            }
        }
    }

    // ── WaveText: per-character colour sweep (the lyrics loading effect) ──
    component WaveText: Row {
        id: wtRoot
        property string text: ""
        property real   pixelSize: 11
        property bool   animating: true
        spacing: 0
        Repeater {
            model: wtRoot.text.length
            delegate: Text {
                text: wtRoot.text[index]
                font.pixelSize: wtRoot.pixelSize; font.family: "Segoe UI"
                color: clrText2
                SequentialAnimation on color {
                    loops: Animation.Infinite
                    running: wtRoot.animating && wtRoot.visible
                    PauseAnimation  { duration: index * 70 }
                    ColorAnimation  { to: clrText;  duration: 300; easing.type: Easing.InOutSine }
                    ColorAnimation  { to: clrText2; duration: 300; easing.type: Easing.InOutSine }
                    PauseAnimation  { duration: (wtRoot.text.length - 1 - index) * 70 }
                }
            }
        }
    }

    // True only after the player has been reading the disc for >= 1s straight,
    // so the periodic empty-drive poll doesn't flash a "Reading CD" state.
    property bool _cdLoading: false
    readonly property bool _cdLoadingRaw: player.is_loading && !player.is_file_mode
    on_CdLoadingRawChanged: {
        if (_cdLoadingRaw) cdLoadingDelay.restart()
        else { cdLoadingDelay.stop(); _cdLoading = false }
    }
    Timer { id: cdLoadingDelay; interval: 1000; onTriggered: window._cdLoading = true }
    readonly property bool _cdLoaded:  player.total_tracks > 0 && !player.is_file_mode
    // "CD" badge label; shows the disc position within a multi-CD release
    // (from MusicBrainz), e.g. "CD 2/3".
    readonly property string _cdBadgeLabel:
        _cdLoaded && player.cd_disc_count > 1 && player.cd_disc_number > 0
        ? "CD " + player.cd_disc_number + "/" + player.cd_disc_count : "CD"

    // ── Settings building blocks ──────────────────────────────────────────
    // Grouped "card" container with an optional heading + caption.
    component SettingsCard: Rectangle {
        default property alias content: cardCol.data
        property string heading: ""
        property string caption: ""
        Layout.fillWidth: true
        color: clrSurface; radius: 8
        border.color: clrBorder; border.width: 1
        implicitHeight: cardCol.implicitHeight + 32
        ColumnLayout {
            id: cardCol
            anchors.left: parent.left; anchors.right: parent.right; anchors.top: parent.top
            anchors.margins: 16; spacing: 12
            ColumnLayout {
                Layout.fillWidth: true; spacing: 3
                visible: heading.length > 0
                Text { text: heading; color: clrText; font.pixelSize: 13; font.family: "Segoe UI"; font.bold: true }
                Text { visible: caption.length > 0; text: caption; color: clrText2; font.pixelSize: 11
                    font.family: "Segoe UI"; Layout.fillWidth: true; wrapMode: Text.WordWrap }
            }
        }
    }

    // Sliding pill switch.
    component PillToggle: Item {
        id: tg
        property bool checked: false
        signal toggled()
        implicitWidth: 34; implicitHeight: 20
        Rectangle {
            anchors.fill: parent; radius: height / 2
            color: tg.checked ? clrAccent : clrSurf2
            border.color: tg.checked ? clrAccent : clrBorder; border.width: 1
            Behavior on color { ColorAnimation { duration: 140 } }
            Rectangle {
                width: 14; height: 14; radius: 7; y: 3
                x: tg.checked ? parent.width - 17 : 3
                color: tg.checked ? clrBg : clrText2
                Behavior on x { NumberAnimation { duration: 150; easing.type: Easing.OutCubic } }
                Behavior on color { ColorAnimation { duration: 140 } }
            }
        }
        MouseArea { anchors.fill: parent; cursorShape: Qt.PointingHandCursor; onClicked: tg.toggled() }
    }

    // Bordered icon + label button.
    component TextButton: Rectangle {
        id: btn
        property string label: ""
        property string icon: ""
        property bool danger: false
        signal clicked()
        implicitWidth: btnRow.implicitWidth + 24; implicitHeight: 30; radius: 6
        color: btnMa.containsMouse ? (danger ? "#3c1a1a" : clrSurf2) : "transparent"
        border.color: clrBorder; border.width: 1
        Behavior on color { ColorAnimation { duration: 100 } }
        Row {
            id: btnRow; anchors.centerIn: parent; spacing: 7
            MatIcon {
                anchors.verticalCenter: parent.verticalCenter; visible: btn.icon.length > 0
                name: btn.icon; size: 13
                color: btn.danger && btnMa.containsMouse ? "#d07070" : clrText2
            }
            Text {
                anchors.verticalCenter: parent.verticalCenter; text: btn.label
                color: btn.danger && btnMa.containsMouse ? "#d07070" : clrText2
                font.pixelSize: 11; font.family: "Segoe UI"
            }
        }
        MouseArea { id: btnMa; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: btn.clicked() }
    }

    // ── Resize handles ────────────────────────────────────────────────────
    readonly property bool _resizable: Qt.platform.os !== "osx" && window.visibility !== Window.Maximized
    Item {
        anchors.fill: parent; z: 100; visible: window._resizable
        MouseArea { width:6;height:6;anchors.top:parent.top;anchors.left:parent.left;cursorShape:Qt.SizeFDiagCursor;onPressed:window.startSystemResize(Qt.TopEdge|Qt.LeftEdge) }
        MouseArea { width:6;height:6;anchors.top:parent.top;anchors.right:parent.right;cursorShape:Qt.SizeBDiagCursor;onPressed:window.startSystemResize(Qt.TopEdge|Qt.RightEdge) }
        MouseArea { width:6;height:6;anchors.bottom:parent.bottom;anchors.left:parent.left;cursorShape:Qt.SizeBDiagCursor;onPressed:window.startSystemResize(Qt.BottomEdge|Qt.LeftEdge) }
        MouseArea { width:6;height:6;anchors.bottom:parent.bottom;anchors.right:parent.right;cursorShape:Qt.SizeFDiagCursor;onPressed:window.startSystemResize(Qt.BottomEdge|Qt.RightEdge) }
        MouseArea { height:6;anchors.top:parent.top;anchors.left:parent.left;anchors.right:parent.right;anchors.leftMargin:6;anchors.rightMargin:6;cursorShape:Qt.SizeVerCursor;onPressed:window.startSystemResize(Qt.TopEdge) }
        MouseArea { height:6;anchors.bottom:parent.bottom;anchors.left:parent.left;anchors.right:parent.right;anchors.leftMargin:6;anchors.rightMargin:6;cursorShape:Qt.SizeVerCursor;onPressed:window.startSystemResize(Qt.BottomEdge) }
        MouseArea { width:6;anchors.top:parent.top;anchors.bottom:parent.bottom;anchors.left:parent.left;anchors.topMargin:6;anchors.bottomMargin:6;cursorShape:Qt.SizeHorCursor;onPressed:window.startSystemResize(Qt.LeftEdge) }
        MouseArea { width:6;anchors.top:parent.top;anchors.bottom:parent.bottom;anchors.right:parent.right;anchors.topMargin:6;anchors.bottomMargin:6;cursorShape:Qt.SizeHorCursor;onPressed:window.startSystemResize(Qt.RightEdge) }
    }

    // ── Settings window ───────────────────────────────────────────────────
    Window {
        id: settingsWindow
        title: "Settings \u2013 Kanae"
        width: 520; height: 600
        minimumWidth: 440; minimumHeight: 420
        color: "transparent"
        flags: Qt.FramelessWindowHint | Qt.Dialog | Qt.Window

        property var settingsObj: {
            try { return JSON.parse(library.settings_json.toString()) }
            catch(e) { return {search_paths:[],merged_folders:[],ignored_folders:[],pinned_paths:[],merge_all_folders:false} }
        }

        Rectangle {
            anchors.fill: parent; color: "#0f0f0f"; radius: 6; clip: true

            // Title bar
            Rectangle {
                id: swTitleBar
                anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right
                height: 30; color: "transparent"; z: 10; radius: 6
                // Square off the bottom corners
                Rectangle { anchors.left:parent.left;anchors.right:parent.right;anchors.bottom:parent.bottom;height:parent.height/2;color:"transparent" }
                Rectangle { anchors.bottom:parent.bottom;anchors.left:parent.left;anchors.right:parent.right;height:1;color:"#282828" }

                MouseArea {
                    anchors.fill: parent; acceptedButtons: Qt.LeftButton
                    propagateComposedEvents: true
                    onPressed: function(mouse) { settingsWindow.startSystemMove(); mouse.accepted = false }
                }

                Text {
                    anchors.centerIn: parent
                    text: "Settings"; color: clrText2; font.pixelSize: 11; font.family: "Segoe UI"
                }

                // Close button
                Rectangle {
                    width: 30; height: parent.height
                    anchors.right: parent.right; anchors.top: parent.top
                    color: swClsHov.containsMouse ? "#3c1a1a" : "transparent"
                    radius: 6
                    // Square off the left + bottom-left corner
                    Rectangle { anchors.left:parent.left;anchors.top:parent.top;anchors.bottom:parent.bottom;width:parent.radius;color:parent.color }
                    Rectangle { anchors.left:parent.left;anchors.right:parent.right;anchors.bottom:parent.bottom;height:parent.radius;color:parent.color }
                    Behavior on color { ColorAnimation { duration: 100 } }
                    MatIcon {
                        anchors.centerIn: parent; name: "close"; size: 11
                        color: swClsHov.containsMouse ? "#d07070" : "#686868"
                    }
                    MouseArea { id: swClsHov; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor; onClicked: settingsWindow.close() }
                }
            }

            Flickable {
                anchors.top: swTitleBar.bottom; anchors.topMargin: 0
                anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
                contentHeight: swCol.implicitHeight + 32; clip: true

                ScrollBar.vertical: ScrollBar {
                    policy: ScrollBar.AsNeeded
                    contentItem: Rectangle { implicitWidth:4;radius:2;color:"#404040";opacity:parent.active?0.85:0.3 }
                    background: Rectangle { color:"transparent" }
                }

                ColumnLayout {
                    id: swCol
                    anchors.left: parent.left; anchors.right: parent.right
                    anchors.margins: 20; spacing: 16

                    Item { height: 4 }

                    // ── Search paths ──────────────────────────────────────
                    SettingsCard {
                        heading: "Search Paths"
                        caption: "Folders that Kanae scans for music"

                        Repeater {
                            model: settingsWindow.settingsObj.search_paths || []
                            RowLayout {
                                Layout.fillWidth: true; spacing: 6
                                Rectangle {
                                    Layout.fillWidth: true; height: 30; radius: 5; color: clrBg
                                    border.color: clrBorder; border.width: 1; clip: true
                                    MatIcon { anchors.left: parent.left; anchors.leftMargin: 9; anchors.verticalCenter: parent.verticalCenter
                                        name: "folder"; size: 13; color: clrMuted }
                                    Text { anchors.verticalCenter: parent.verticalCenter; anchors.left: parent.left; anchors.right: parent.right
                                        anchors.leftMargin: 30; anchors.rightMargin: 10; text: modelData; color: clrText2
                                        font.pixelSize: 11; font.family: "Segoe UI"; elide: Text.ElideRight }
                                }
                                Rectangle {
                                    width: 30; height: 30; radius: 5
                                    color: swRmHov.containsMouse ? "#3c1a1a" : clrBg
                                    border.color: clrBorder; border.width: 1
                                    Behavior on color { ColorAnimation { duration: 100 } }
                                    MatIcon { anchors.centerIn: parent; name: "trash"; size: 14
                                        color: swRmHov.containsMouse ? "#d07070" : clrText2 }
                                    MouseArea { id: swRmHov; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                                        property string pv: modelData; onClicked: library.removeSearchPath(pv) }
                                }
                            }
                        }

                        TextButton { label: "Add Folder"; icon: "folder-plus"; onClicked: library.openFolderPicker() }
                    }

                    // ── Display ───────────────────────────────────────────
                    SettingsCard {
                        heading: "Display"
                        RowLayout {
                            Layout.fillWidth: true; spacing: 12
                            Column {
                                Layout.fillWidth: true; spacing: 3
                                Text { text: "Merge all folders"; color: clrText; font.pixelSize: 12; font.family: "Segoe UI" }
                                Text { text: "Show only albums, hide folder tiles"; color: clrText2; font.pixelSize: 10; font.family: "Segoe UI" }
                            }
                            PillToggle {
                                checked: settingsWindow.settingsObj.merge_all_folders === true
                                onToggled: library.setMergeAll(!(settingsWindow.settingsObj.merge_all_folders === true))
                            }
                        }
                    }

                    // ── Merged / ignored folders ──────────────────────────
                    SettingsCard {
                        heading: "Merged Folders"
                        visible: (settingsWindow.settingsObj.merged_folders || []).length > 0
                        Repeater {
                            model: settingsWindow.settingsObj.merged_folders || []
                            RowLayout {
                                Layout.fillWidth: true; spacing: 6
                                Text { Layout.fillWidth: true; text: modelData; color: clrText2; font.pixelSize: 11; font.family: "Segoe UI"; elide: Text.ElideRight }
                                TextButton { label: "Unmerge"; property string pv: modelData; onClicked: library.setFolderOption(pv, "merge_remove") }
                            }
                        }
                    }

                    SettingsCard {
                        heading: "Ignored Folders"
                        visible: (settingsWindow.settingsObj.ignored_folders || []).length > 0
                        Repeater {
                            model: settingsWindow.settingsObj.ignored_folders || []
                            RowLayout {
                                Layout.fillWidth: true; spacing: 6
                                Text { Layout.fillWidth: true; text: modelData; color: clrText2; font.pixelSize: 11; font.family: "Segoe UI"; elide: Text.ElideRight }
                                TextButton { label: "Show"; property string pv: modelData; onClicked: library.setFolderOption(pv, "ignore_remove") }
                            }
                        }
                    }

                    // ── Lyrics ────────────────────────────────────────────
                    SettingsCard {
                        heading: "Lyrics"
                        RowLayout {
                            Layout.fillWidth: true; spacing: 12
                            Column {
                                Layout.fillWidth: true; spacing: 3
                                Text { text: "Romanize Japanese lyrics"; color: clrText; font.pixelSize: 12; font.family: "Segoe UI" }
                                Text { text: "Convert kana and kanji to romaji"; color: clrText2; font.pixelSize: 10; font.family: "Segoe UI" }
                            }
                            PillToggle {
                                checked: settingsWindow.settingsObj.romanize_lyrics === true
                                onToggled: {
                                    library.setRomanizeLyrics(!(settingsWindow.settingsObj.romanize_lyrics === true))
                                    player.reapplyLyrics()
                                }
                            }
                        }
                        Rectangle { Layout.fillWidth: true; height: 1; color: clrBorder }
                        RowLayout {
                            Layout.fillWidth: true; spacing: 12
                            Column {
                                Layout.fillWidth: true; spacing: 3
                                Text { text: "Disable 100-entry limit"; color: clrText; font.pixelSize: 12; font.family: "Segoe UI" }
                                Text { text: "Keep all cached lyrics indefinitely"; color: clrText2; font.pixelSize: 10; font.family: "Segoe UI" }
                            }
                            PillToggle {
                                checked: settingsWindow.settingsObj.lrc_limit_disabled === true
                                onToggled: library.setLrcLimitDisabled(!(settingsWindow.settingsObj.lrc_limit_disabled === true))
                            }
                        }
                        RowLayout {
                            Layout.fillWidth: true; spacing: 8
                            TextButton { label: "Purge LRC cache"; icon: "refresh"; onClicked: library.purgeLrcCache() }
                            TextButton { label: "Purge no-lyrics cache"; icon: "refresh"; onClicked: library.purgeNoLyricsCache() }
                        }
                    }

                    // ── Integrations ──────────────────────────────────────
                    SettingsCard {
                        heading: "Integrations"
                        RowLayout {
                            Layout.fillWidth: true; spacing: 12
                            Column {
                                Layout.fillWidth: true; spacing: 3
                                Text { text: "Discord Rich Presence"; color: clrText; font.pixelSize: 12; font.family: "Segoe UI" }
                                Text { text: "Show what you're listening to on Discord"; color: clrText2; font.pixelSize: 10; font.family: "Segoe UI" }
                            }
                            PillToggle {
                                checked: settingsWindow.settingsObj.discord_rpc !== false
                                onToggled: {
                                    var v = !(settingsWindow.settingsObj.discord_rpc !== false)
                                    library.setDiscordRpc(v)
                                    player.setDiscordEnabled(v)
                                }
                            }
                        }
                    }

                    // ── Library maintenance ───────────────────────────────
                    SettingsCard {
                        heading: "Library"
                        TextButton { label: "Rescan Library"; icon: "refresh"; onClicked: library.startScan() }
                    }

                    Item { height: 4 }
                }
            }
        }
    }

    // ── Root background ───────────────────────────────────────────────────
    Rectangle {
        id: rootBg
        anchors.fill: parent
        color: clrBg; radius: 6; clip: true

        // ── Titlebar ──────────────────────────────────────────────────────
        Rectangle {
            id: titleBar
            anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right
            height: 30; color: "transparent"; z: 10

            MouseArea {
                anchors.fill: parent; acceptedButtons: Qt.LeftButton
                propagateComposedEvents: true
                onPressed: function(mouse) { window.startSystemMove(); mouse.accepted = false }
            }

            // macOS traffic lights
            Item {
                id: trafficLights; visible: Qt.platform.os === "osx"
                anchors.left: parent.left; anchors.leftMargin: 12
                anchors.verticalCenter: parent.verticalCenter; width: 52; height: 12
                HoverHandler { id: tlHover }
                Rectangle { x:0;width:12;height:12;radius:6;color:tlHover.hovered?"#FF5F57":"#606060"
                    Behavior on color{ColorAnimation{duration:120}}
                    MouseArea{anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:Qt.quit()} }
                Rectangle { x:20;width:12;height:12;radius:6;color:tlHover.hovered?"#FEBC2E":"#606060"
                    Behavior on color{ColorAnimation{duration:120}}
                    MouseArea{anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:window.showMinimized()} }
                Rectangle { x:40;width:12;height:12;radius:6;color:tlHover.hovered?"#28C840":"#606060"
                    Behavior on color{ColorAnimation{duration:120}}
                    MouseArea{anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor
                        onClicked: window.visibility===Window.FullScreen?window.showNormal():window.showFullScreen()} }
            }

            // macOS settings button (right side of titlebar)
            Item {
                id: macSettingsBtn
                visible: Qt.platform.os === "osx"
                anchors.right: parent.right; anchors.rightMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                width: 22; height: 22

                Rectangle {
                    anchors.fill: parent; radius: 4
                    color: macSettingsHov.containsMouse ? clrSurf2 : "transparent"
                    border.color: macSettingsHov.containsMouse ? clrBorder : "transparent"
                    border.width: 1
                    Behavior on color { ColorAnimation { duration: 100 } }
                }
                MatIcon {
                    anchors.centerIn: parent; name: "settings"; size: 13
                    color: macSettingsHov.containsMouse ? clrText : clrText2
                }
                MouseArea { id: macSettingsHov; anchors.fill: parent; hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor; onClicked: settingsWindow.show() }
            }

            // Now playing (left-aligned): \u25B6 Artist \u2014 Title, title brightest.
            MatIcon {
                id: npTitleIcon
                visible: player.is_playing && player.total_tracks > 0
                anchors.left: Qt.platform.os === "osx" ? trafficLights.right : parent.left
                anchors.leftMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                name: "play"; size: 13; color: "#5a5a5a"
            }
            Text {
                anchors.left: npTitleIcon.right
                anchors.right: Qt.platform.os === "osx" ? macSettingsBtn.left : winButtons.left
                anchors.leftMargin: 6; anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                elide: Text.ElideRight
                textFormat: Text.StyledText
                text: {
                    if (!player.is_playing || player.total_tracks === 0) return ""
                    function esc(s){ return s.replace(/&/g,"&amp;").replace(/</g,"&lt;").replace(/>/g,"&gt;") }
                    var title  = esc((player.track_titles[player.current_track] || "").trim())
                    var rawArt = (player.track_artists[player.current_track] || "").trim()
                    var artist = esc(rawArt.length > 0 ? rawArt : (player.album_artist || "").trim())
                    var head = artist.length > 0
                        ? "<font color='#7a7a7a'>" + artist + "</font>&nbsp;&nbsp;<font color='#3a3a3a'>-</font>&nbsp;&nbsp;"
                        : ""
                    return head + "<font color='#a8a8a8'>" + title + "</font>"
                }
                font.pixelSize: 11; font.family: "Segoe UI"
            }

            // Windows titlebar buttons
            Row {
                id: winButtons; visible: Qt.platform.os !== "osx"
                anchors.right: parent.right; anchors.top: parent.top; height: titleBar.height

                // Settings button (cog icon)
                Rectangle {
                    width: 32; height: parent.height; color: "transparent"
                    Rectangle { anchors.fill:parent; color:settingsHov.containsMouse?clrSurf2:"transparent"; Behavior on color{ColorAnimation{duration:100}} }
                    MatIcon {
                        anchors.centerIn: parent; name: "settings"; size: 13
                        color: settingsHov.containsMouse ? clrText : clrText2
                    }
                    MouseArea { id:settingsHov; anchors.fill:parent; hoverEnabled:true
                        cursorShape:Qt.PointingHandCursor
                        onClicked: settingsWindow.show() }
                }
                Rectangle { width:1; height:parent.height; color:clrBorder }
                Rectangle {
                    width:32;height:parent.height;color:"transparent"
                    Rectangle{anchors.fill:parent;color:minHov.containsMouse?clrSurf2:"transparent";Behavior on color{ColorAnimation{duration:100}}}
                    MatIcon{anchors.centerIn:parent;name:"minimize";size:11;color:minHov.containsMouse?clrText:clrText2}
                    MouseArea{id:minHov;anchors.fill:parent;hoverEnabled:true;onClicked:window.showMinimized()}
                }
                Rectangle {
                    width:32;height:parent.height;color:"transparent"
                    Rectangle{anchors.fill:parent;color:maxHov.containsMouse?clrSurf2:"transparent";Behavior on color{ColorAnimation{duration:100}}}
                    // Drawn empty square: the icon font is a FILL=1 subset, so the
                    // crop_square glyph renders as a solid block.
                    Rectangle{anchors.centerIn:parent;width:8;height:8
                        visible:window.visibility!==Window.Maximized
                        color:"transparent";border.width:1
                        border.color:maxHov.containsMouse?clrText:clrText2}
                    MatIcon{anchors.centerIn:parent;size:11;name:"restore"
                        visible:window.visibility===Window.Maximized
                        color:maxHov.containsMouse?clrText:clrText2}
                    MouseArea{id:maxHov;anchors.fill:parent;hoverEnabled:true;onClicked:window.visibility===Window.Maximized?window.showNormal():window.showMaximized()}
                }
                Rectangle {
                    width:32;height:parent.height;color:"transparent"
                    Rectangle{anchors.fill:parent;color:clsHov.containsMouse?"#3c1a1a":"transparent";Behavior on color{ColorAnimation{duration:100}}}
                    MatIcon{anchors.centerIn:parent;name:"close";size:11;color:clsHov.containsMouse?"#d07070":clrText2}
                    MouseArea{id:clsHov;anchors.fill:parent;hoverEnabled:true;onClicked:Qt.quit()}
                }
            }
            Rectangle { anchors.bottom:parent.bottom;anchors.left:parent.left;anchors.right:parent.right;height:1;color:clrBorder }
        }

        // ── Content ───────────────────────────────────────────────────────
        ColumnLayout {
            anchors.top: titleBar.bottom
            anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
            spacing: 0

            // ── Main SplitView (always visible: sidebar + right panel) ────
            SplitView {
                Layout.fillWidth: true; Layout.fillHeight: true
                orientation: Qt.Horizontal

                handle: Item {
                    implicitWidth: 1; implicitHeight: 1
                    Rectangle {
                        anchors.centerIn:parent;width:1;height:parent.height
                        color:SplitHandle.pressed?clrAccent:SplitHandle.hovered?clrMuted:clrBorder
                        Behavior on color{ColorAnimation{duration:100}}
                    }
                }

                // ── Left sidebar (always visible) ─────────────────────────
                ColumnLayout {
                    id: sidebarColumn
                    property real _ratio: 200.0/880.0
                    SplitView.preferredWidth: Math.round(window.width * _ratio)
                    SplitView.minimumWidth: 200; SplitView.maximumWidth: Math.min(400, Math.round(window.width*0.45))
                    spacing: 0; clip: true
                    onWidthChanged: {
                        if(!window._resizing) {
                            var mxW=Math.min(400,Math.round(window.width*0.45))
                            _ratio=Math.max(200/window.width,Math.min(mxW/window.width,width/window.width))
                        }
                    }

                    readonly property real _naturalCoverH:
                        (coverImg.status===Image.Ready&&coverImg.implicitWidth>0)
                        ? sidebarColumn.width*coverImg.implicitHeight/coverImg.implicitWidth
                        : sidebarColumn.width
                    property real _curtainH: _naturalCoverH
                    property real _topSepH: 1
                    property real _slideX: -200
                    property real _coverTarget: _naturalCoverH
                    property bool _toTopPending: false
                    readonly property real _thumbW:
                        (coverImg.status===Image.Ready&&coverImg.implicitHeight>0)
                        ? Math.round(70*coverImg.implicitWidth/coverImg.implicitHeight) : 70
                    property real _thumbTarget: 70

                    Binding {
                        when:!window.coverOnSide&&!toSideAnim.running&&!toTopAnim.running&&!sidebarColumn._toTopPending
                        target:sidebarColumn;property:"_curtainH";value:sidebarColumn._naturalCoverH
                    }
                    SequentialAnimation {
                        id:toSideAnim
                        ScriptAction{script:{sidebarColumn._thumbTarget=sidebarColumn._thumbW;sidebarColumn._slideX=-(sidebarColumn._thumbTarget+4)}}
                        ParallelAnimation{
                            NumberAnimation{target:sidebarColumn;property:"_curtainH";to:0;duration:220;easing.type:Easing.OutCubic}
                            NumberAnimation{target:sidebarColumn;property:"_topSepH";to:0;duration:220;easing.type:Easing.OutCubic}
                            NumberAnimation{target:sidebarColumn;property:"_slideX";to:0;duration:220;easing.type:Easing.OutCubic}
                        }
                    }
                    SequentialAnimation {
                        id:toTopAnim
                        ScriptAction{script:{sidebarColumn._toTopPending=false;sidebarColumn._coverTarget=sidebarColumn._naturalCoverH;sidebarColumn._thumbTarget=sidebarColumn._thumbW;sidebarColumn._topSepH=1;sidebarColumn._curtainH=0}}
                        ParallelAnimation{
                            NumberAnimation{target:sidebarColumn;property:"_slideX";to:-(sidebarColumn._thumbTarget+4);duration:220;easing.type:Easing.OutCubic}
                            NumberAnimation{target:sidebarColumn;property:"_curtainH";to:sidebarColumn._coverTarget;duration:220;easing.type:Easing.OutCubic}
                        }
                    }
                    Connections{target:window;function onCoverOnSideChanged(){
                        if(window.coverOnSide){toTopAnim.stop();toSideAnim.restart()}
                        else{sidebarColumn._toTopPending=true;toSideAnim.stop();toTopAnim.restart()}
                    }}

                    Item {
                        id:coverTopItem;Layout.fillWidth:true;Layout.preferredHeight:sidebarColumn._curtainH;clip:true
                        Rectangle {
                            id:coverRect;anchors.left:parent.left;anchors.right:parent.right;anchors.top:parent.top
                            height:sidebarColumn._naturalCoverH;color:clrSurf2
                            border.color:coverImg.status!==Image.Ready?clrBorder:"transparent";border.width:1
                            // Cap the decode size: embedded art is often 1500–3000 px and a
                            // full-res RGBA texture of that costs tens of MB. 1024 covers the
                            // sidebar's 400 px max width even on 2x-DPI screens.
                            Image{id:coverImg;anchors.fill:parent;source:player.cover_art_path;fillMode:Image.Stretch;smooth:true;mipmap:true;visible:status===Image.Ready
                                asynchronous:true;sourceSize.width:1024;sourceSize.height:1024}
                            Canvas{anchors.centerIn:parent;width:44;height:44;opacity:0.3;visible:coverImg.status!==Image.Ready
                                onPaint:{var c=getContext("2d");c.clearRect(0,0,44,44);c.beginPath();c.arc(22,22,20,0,2*Math.PI);c.strokeStyle="#888";c.lineWidth=1.5;c.stroke();c.beginPath();c.arc(22,22,4,0,2*Math.PI);c.fillStyle="#666";c.fill()}}
                            MouseArea{anchors.fill:parent;cursorShape:Qt.PointingHandCursor;onClicked:coverOnSide=true}
                        }
                    }
                    Rectangle{Layout.fillWidth:true;height:sidebarColumn._topSepH;color:clrBorder}
                    Item{id:metaBlock;Layout.fillWidth:true;Layout.preferredHeight:70;clip:true
                        Rectangle{id:thumbRect;x:sidebarColumn._slideX;y:0;height:70;width:sidebarColumn._thumbW;color:clrSurf2
                            border.color:coverImg.status!==Image.Ready?clrBorder:"transparent";border.width:1;clip:true
                            // Same source + sourceSize as coverImg so both share one cache entry.
                            Image{anchors.fill:parent;source:player.cover_art_path;fillMode:Image.Stretch;smooth:true;mipmap:true;visible:coverImg.status===Image.Ready
                                asynchronous:true;sourceSize.width:1024;sourceSize.height:1024}
                            MouseArea{anchors.fill:parent;cursorShape:Qt.PointingHandCursor;onClicked:coverOnSide=false}}
                        Column{
                            anchors.left:parent.left;anchors.right:parent.right;anchors.verticalCenter:parent.verticalCenter
                            anchors.leftMargin:Math.max(12,sidebarColumn._slideX+sidebarColumn._thumbW+10);anchors.rightMargin:10;spacing:3
                            ScrollText{width:parent.width;text:player.album_title;textColor:clrText;pixelSize:12;bold:true}
                            ScrollText{width:parent.width;text:player.album_artist;textColor:clrText2;pixelSize:11}
                            ScrollText{width:parent.width;visible:player.album_year.length>0;text:player.album_year;textColor:"#3a3a3a";pixelSize:10}
                        }
                    }
                    Rectangle{Layout.fillWidth:true;height:1;color:clrBorder}

                    // ── Lyrics ────────────────────────────────────────────
                    Item {
                        id:lyricsArea;Layout.fillWidth:true;Layout.fillHeight:true;clip:true
                        property var timesArr:{var a=[];for(var i=0;i<player.lyric_times.length;i++)a.push(parseFloat(player.lyric_times[i]));return a}
                        property int activeIdx:{var t=player.current_time;var times=timesArr;var best=-1;for(var i=0;i<times.length;i++){if(times[i]<=t+0.05)best=i;else break};return best}
                        property bool userScrolled:false;property bool _autoScrolling:false
                        Timer{id:resyncGuardTimer;interval:800;repeat:false;onTriggered:lyricsArea._autoScrolling=false}
                        function syncScroll(idx){if(idx<0||lyricsView.count===0)return;lyricsArea._autoScrolling=true;lyricsView.positionViewAtIndex(idx,ListView.Center);resyncGuardTimer.restart()}
                        property var _lyricsWatch:player.lyric_lines
                        on_LyricsWatchChanged:{lyricsArea.userScrolled=false;syncScroll(lyricsArea.activeIdx)}
                        onActiveIdxChanged:{if(!userScrolled)syncScroll(activeIdx)}
                        ListView{id:lyricsView;anchors.fill:parent;clip:true;model:player.lyric_lines;spacing:0;flickDeceleration:600;maximumFlickVelocity:6000
                            Behavior on contentY{NumberAnimation{duration:700;easing.type:Easing.InOutQuart}}
                            ScrollBar.vertical:ScrollBar{policy:ScrollBar.AlwaysOff}
                            onMovementStarted:{if(!lyricsArea._autoScrolling)lyricsArea.userScrolled=true}
                            header:Item{width:lyricsView.width;height:Math.max(0,lyricsView.height/2-24)}
                            footer:Item{width:lyricsView.width;height:Math.max(0,lyricsView.height/2)}
                            delegate:MouseArea{id:lyricRow;width:lyricsView.width;height:lyricRowText.implicitHeight+16;hoverEnabled:true;preventStealing:false
                                onClicked:{var times=lyricsArea.timesArr;if(index<times.length){player.seek(times[index]);lyricsArea.userScrolled=false;lyricsArea.syncScroll(index)}}
                                Text{id:lyricRowText;anchors.centerIn:parent;width:parent.width-24;text:modelData
                                    color:{if(index===lyricsArea.activeIdx)return lyricRow.containsMouse?clrAccent:clrText;return lyricRow.containsMouse?clrText:clrText2}
                                    font.pixelSize:index===lyricsArea.activeIdx?12:11;font.bold:index===lyricsArea.activeIdx;font.family:"Segoe UI"
                                    wrapMode:Text.WordWrap;horizontalAlignment:Text.AlignHCenter
                                    Behavior on color{ColorAnimation{duration:150}}
                                    Behavior on font.pixelSize{NumberAnimation{duration:150}}
                                }
                            }
                        }
                        Rectangle{anchors.left:parent.left;anchors.right:parent.right;anchors.top:parent.top;height:40;z:1
                            gradient:Gradient{
                                GradientStop{position:0;color:clrBg}
                                GradientStop{position:1;color:"transparent"}
                            }}
                        Rectangle{anchors.left:parent.left;anchors.right:parent.right;anchors.bottom:parent.bottom;height:40;z:1
                            gradient:Gradient{
                                GradientStop{position:0;color:"transparent"}
                                GradientStop{position:1;color:clrBg}
                            }}
                        Rectangle{visible:lyricsArea.userScrolled&&player.lyric_lines.length>0;anchors.bottom:parent.bottom;anchors.left:parent.left;anchors.margins:8
                            width:resyncLabel.implicitWidth+16;height:20;radius:3;color:clrSurf2;border.color:clrBorder;border.width:1;z:2
                            Text{id:resyncLabel;anchors.centerIn:parent;text:"\u21A9 resync";color:clrText2;font.pixelSize:10;font.family:"Segoe UI"}
                            MouseArea{anchors.fill:parent;cursorShape:Qt.PointingHandCursor;onClicked:{lyricsArea.userScrolled=false;lyricsArea.syncScroll(lyricsArea.activeIdx)}}
                        }

                        // ── Loading / empty overlay ───────────────────────
                        Item {
                            anchors.centerIn: parent; z: 3
                            visible: player.lyrics_loading || (!player.lyrics_loading && player.lyric_lines.length === 0 && player.total_tracks > 0)
                            width: parent.width; height: 40

                            Text {
                                id: noLyricsText
                                anchors.centerIn: parent
                                text: "No lyrics found."
                                font.pixelSize: 11; font.family: "Segoe UI"
                                visible: !player.lyrics_loading
                                color: clrText2
                            }

                            WaveText {
                                anchors.centerIn: parent
                                visible: player.lyrics_loading
                                animating: player.lyrics_loading
                                text: "Loading lyrics..."
                            }
                        }
                    }
                }

                // ── Right panel ───────────────────────────────────────────
                Item {
                    SplitView.fillWidth: true; SplitView.minimumWidth: 220

                    // ── Path bar (always visible) ─────────────────────────
                    Rectangle {
                        id: rightPathBar
                        anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right
                        height: 34; color: clrSurface
                        Rectangle { anchors.bottom:parent.bottom;anchors.left:parent.left;anchors.right:parent.right;height:1;color:clrBorder }

                        RowLayout {
                            anchors.fill: parent; anchors.leftMargin: 10; anchors.rightMargin: 10; spacing: 4

                            // Back
                            Rectangle {
                                width:22;height:22;radius:3
                                color: backPathHov.containsMouse && (library.can_go_back || _view === "album") ? clrSurf2 : "transparent"
                                opacity: library.can_go_back || _view === "album" ? 1 : 0.3
                                Behavior on color { ColorAnimation { duration: 80 } }
                                MatIcon { anchors.centerIn:parent; name:"chevron-left"; size:11; color:clrText2 }
                                MouseArea { id:backPathHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor
                                    onClicked: if(library.can_go_back || window._view === "album") window.goBack() }
                            }

                            // Forward
                            Rectangle {
                                width:22;height:22;radius:3
                                color: fwdPathHov.containsMouse && (library.can_go_forward || (_view === "library" && _canGoForwardToAlbum)) ? clrSurf2 : "transparent"
                                opacity: library.can_go_forward || (_view === "library" && _canGoForwardToAlbum) ? 1 : 0.3
                                Behavior on color { ColorAnimation { duration: 80 } }
                                MatIcon { anchors.centerIn:parent; name:"chevron-right"; size:11; color:clrText2 }
                                MouseArea { id:fwdPathHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor
                                    onClicked: if(library.can_go_forward || (window._view === "library" && window._canGoForwardToAlbum)) window.goForward() }
                            }

                            // Breadcrumbs (relative to library root)
                            Row {
                                id: crumbsRow
                                Layout.fillWidth: true; spacing: 0; clip: true

                                function navigateCrumb(itemPath) {
                                    if (itemPath === "__album__" || itemPath === "__files__") return
                                    window._canGoForwardToAlbum = false
                                    window._browseDir = ""; window._browseAlbumName = ""; window._fileMode = false; window._showingCdView = false
                                    window._view = "library"
                                    if (itemPath === "") library.navigateToRoot()
                                    else library.navigateTo(itemPath)
                                }

                                Repeater {
                                    model: window._crumbs
                                    delegate: Item {
                                        id: crumbDelegate
                                        required property var modelData
                                        required property int index
                                        property bool crumbIsLast: window._view === "library"
                                            ? crumbDelegate.index === window._crumbs.length - 1
                                            : (crumbDelegate.modelData.path === "__album__" || crumbDelegate.modelData.path === "__files__")
                                        height: 34
                                        width: crumbInner.implicitWidth

                                        Row {
                                            id: crumbInner
                                            anchors.verticalCenter: parent.verticalCenter
                                            spacing: 0

                                            // Fraction slash separator (not before first item)
                                            Text {
                                                visible: crumbDelegate.index > 0
                                                text: "\u2044"
                                                color: clrMuted; font.pixelSize: 13; font.family: "Segoe UI"
                                                anchors.verticalCenter: parent.verticalCenter
                                                leftPadding: 4; rightPadding: 2
                                            }

                                            Text {
                                                anchors.verticalCenter: parent.verticalCenter
                                                text: crumbDelegate.modelData.name
                                                font.pixelSize: 12; font.family: "Segoe UI"
                                                color: crumbDelegate.crumbIsLast ? clrText
                                                     : crumbMa.containsMouse       ? clrAccent
                                                     : clrText2
                                                Behavior on color { ColorAnimation { duration: 80 } }
                                            }
                                        }

                                        MouseArea {
                                            id: crumbMa
                                            anchors.fill: parent
                                            hoverEnabled: true
                                            enabled: !crumbDelegate.crumbIsLast
                                            cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                                            onClicked: crumbsRow.navigateCrumb(crumbDelegate.modelData.path)
                                        }
                                    }
                                }
                            }

                            // Eject (visible when viewing the audio CD)
                            Rectangle {
                                width: 22; height: 22; radius: 3
                                visible: player.drive_list.length > 0 && (_view === "library" || (_view === "album" && _showingCdView))
                                color: pathEjectHov.containsMouse ? clrSurf2 : "transparent"
                                Behavior on color { ColorAnimation { duration: 80 } }
                                MatIcon { anchors.centerIn: parent; name: "eject"; size: 12
                                    color: pathEjectHov.containsMouse ? clrText : clrText2 }
                                MouseArea { id: pathEjectHov; anchors.fill: parent; hoverEnabled: true
                                    cursorShape: Qt.PointingHandCursor; onClicked: player.ejectDisc() }
                            }

                            // Grid/List toggle (only relevant in library view)
                            Row {
                                spacing: 4; visible: _view === "library"
                                // Album sort mode (click to cycle). Persisted via
                                // library.setAlbumSort \u2192 settings.json; _cur re-reads it.
                                Rectangle {
                                    id: sortToggle
                                    width: sortRow.implicitWidth + 16; height: 22; radius: 3
                                    color: sortHov.containsMouse ? clrSurf2 : "transparent"
                                    Behavior on color { ColorAnimation { duration: 80 } }
                                    readonly property var _modes: ["artist", "album", "year_desc", "year_asc"]
                                    readonly property var _labels: ({"artist": "Artist", "album": "Album",
                                                                     "year_desc": "Year", "year_asc": "Year"})
                                    readonly property bool _desc: _cur === "year_desc"
                                    property string _cur: {
                                        try {
                                            var s = JSON.parse(library.settings_json)
                                            return s.album_sort && _modes.indexOf(s.album_sort) >= 0 ? s.album_sort : "artist"
                                        } catch (e) { return "artist" }
                                    }
                                    Row {
                                        id: sortRow; anchors.centerIn: parent; spacing: 4
                                        // Direction arrow: chevron glyph rotated up (ascending /
                                        // A-Z) or down (newest first), flipping with a short spin.
                                        MatIcon {
                                            anchors.verticalCenter: parent.verticalCenter
                                            name: "chevron-right"; size: 11
                                            rotation: sortToggle._desc ? 90 : -90
                                            Behavior on rotation { NumberAnimation { duration: 160; easing.type: Easing.OutCubic } }
                                            color: sortHov.containsMouse ? clrText : clrText2
                                        }
                                        Text {
                                            anchors.verticalCenter: parent.verticalCenter
                                            text: sortToggle._labels[sortToggle._cur]
                                            color: sortHov.containsMouse ? clrText : clrText2
                                            font.pixelSize: 10; font.family: "Segoe UI"
                                        }
                                    }
                                    MouseArea {
                                        id: sortHov; anchors.fill: parent; hoverEnabled: true
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: {
                                            var i = sortToggle._modes.indexOf(sortToggle._cur)
                                            library.setAlbumSort(sortToggle._modes[(i + 1) % sortToggle._modes.length])
                                        }
                                    }
                                }
                                Rectangle {
                                    width:22;height:22;radius:3
                                    color: gridToggleHov.containsMouse || window._libUseGrid ? clrSurf2 : "transparent"
                                    Behavior on color { ColorAnimation { duration: 80 } }
                                    MatIcon { anchors.centerIn:parent; name:"grid"; size:11; color:window._libUseGrid?clrText:clrText2 }
                                    MouseArea{id:gridToggleHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:window._libUseGrid=true}
                                }
                                Rectangle {
                                    width:22;height:22;radius:3
                                    color: listToggleHov.containsMouse || !window._libUseGrid ? clrSurf2 : "transparent"
                                    Behavior on color { ColorAnimation { duration: 80 } }
                                    MatIcon { anchors.centerIn:parent; name:"list"; size:11; color:!window._libUseGrid?clrText:clrText2 }
                                    MouseArea{id:listToggleHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:window._libUseGrid=false}
                                }
                            }
                        }
                    }

                    // ── Library browser (visible when _view === "library") ─
                    ColumnLayout {
                        anchors.top: rightPathBar.bottom
                        anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
                        spacing: 0
                        visible: _view === "library"

                        // ── Library grid / list ───────────────────────────
                        Item {
                            id: libBrowserArea
                            Layout.fillWidth: true; Layout.fillHeight: true

                            // Empty state
                            Column {
                                anchors.centerIn: parent; spacing: 12
                                visible: !library.is_scanning && libBrowserArea.nodes.length === 0

                                Canvas {
                                    width:48;height:48;anchors.horizontalCenter:parent.horizontalCenter;opacity:0.4
                                    onPaint:{ var c=getContext("2d");c.clearRect(0,0,48,48)
                                        c.beginPath();c.arc(24,24,21,0,2*Math.PI);c.strokeStyle="#555";c.lineWidth=1.5;c.stroke()
                                        c.beginPath();c.arc(24,24,4,0,2*Math.PI);c.fillStyle="#555";c.fill() }
                                }
                                Text { anchors.horizontalCenter:parent.horizontalCenter; text:"No music found"
                                    color:clrText2;font.pixelSize:13;font.family:"Segoe UI" }
                                Row {
                                    anchors.horizontalCenter:parent.horizontalCenter;spacing:8
                                    Rectangle {
                                        width:addFolderLbl.implicitWidth+20;height:26;radius:3
                                        color:addFolHov.containsMouse?clrSurf2:"transparent"
                                        border.color:clrBorder;border.width:1
                                        Behavior on color{ColorAnimation{duration:100}}
                                        Text{id:addFolderLbl;anchors.centerIn:parent;text:"Add Folder";color:clrText2;font.pixelSize:11;font.family:"Segoe UI"}
                                        MouseArea{id:addFolHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:library.openFolderPicker()}
                                    }
                                }
                            }

                            // Scanning spinner
                            Column {
                                anchors.centerIn: parent; spacing: 10
                                visible: library.is_scanning && libBrowserArea.nodes.length === 0
                                Shape {
                                    anchors.horizontalCenter:parent.horizontalCenter;width:26;height:26
                                    RotationAnimator on rotation { from:0;to:360;duration:800;loops:Animation.Infinite;running:library.is_scanning }
                                    ShapePath { strokeColor:clrText2;strokeWidth:2;fillColor:"transparent";capStyle:ShapePath.RoundCap
                                        PathAngleArc{centerX:13;centerY:13;radiusX:10;radiusY:10;startAngle:-90;sweepAngle:250} }
                                }
                                Text { anchors.horizontalCenter:parent.horizontalCenter;text:library.scan_message;color:clrText2;font.pixelSize:11;font.family:"Segoe UI" }
                            }

                            property bool useGrid: window._libUseGrid
                            property var nodes: {
                                try {
                                    var base = JSON.parse(library.library_nodes)
                                    var atRoot = library.current_path.toString() === "Library" || library.current_path.toString() === ""
                                    if (atRoot && player.drive_list.length > 0) {
                                        var cdLoaded  = window._cdLoaded
                                        var cdLoading = window._cdLoading
                                        var cdNode = {
                                            kind: "cd", path: "__cd__", id: "__cd__",
                                            name: cdLoaded ? (player.album_title || "Audio CD") : "Audio CD",
                                            album_artist: cdLoaded ? (player.album_artist || "")
                                                        : (cdLoading ? "" : "No disc"),
                                            year: cdLoaded ? (player.album_year || "") : "",
                                            cover_url: cdLoaded ? (player.cover_art_path || "") : "",
                                            pinned: false,
                                            loading: cdLoading
                                        }
                                        return [cdNode].concat(base)
                                    }
                                    return base
                                } catch(e) { return [] }
                            }

                            // Grid view — flush tiles, 0 padding
                            GridView {
                                id: libGridView
                                anchors.fill: parent
                                visible: parent.useGrid && parent.nodes.length > 0
                                clip: true
                                property int numCols: Math.max(1, Math.floor(width / 150))
                                cellWidth: Math.floor(width / numCols)
                                cellHeight: cellWidth + 60
                                model: parent.nodes

                                ScrollBar.vertical: ScrollBar {
                                    policy: ScrollBar.AsNeeded
                                    contentItem: Rectangle { implicitWidth:4;radius:2;color:clrMuted;opacity:parent.active?0.85:0.3 }
                                    background: Rectangle { color:"transparent" }
                                }

                                delegate: Item {
                                    width: libGridView.cellWidth; height: libGridView.cellHeight
                                    required property var modelData

                                    // Hover tint
                                    Rectangle {
                                        anchors.fill: parent
                                        color: gridItemHov.containsMouse ? "#18ffffff" : "transparent"
                                        Behavior on color { ColorAnimation { duration: 100 } }
                                    }

                                    // Tile borders
                                    Rectangle { anchors.right:parent.right; width:1; height:parent.height; color:clrBorder; z:1 }
                                    Rectangle { anchors.bottom:parent.bottom; height:1; width:parent.width; color:clrBorder; z:1 }

                                    // Cover art (square)
                                    Rectangle {
                                        id: tileArt
                                        anchors.top: parent.top; anchors.left: parent.left; anchors.right: parent.right
                                        height: parent.width; color: clrSurf2
                                        Image {
                                            // No mipmap: at 320 px the texture is already ~1:1 with the
                                            // tile (cells are 150–300 px), so mipmaps would only add VRAM.
                                            anchors.fill: parent; fillMode: Image.Stretch; smooth: true
                                            source: modelData.cover_url || ""; visible: status === Image.Ready
                                            // Decode off the UI thread at thumbnail size — full-size
                                            // covers otherwise cost several MB of texture per tile.
                                            asynchronous: true
                                            sourceSize.width: 320; sourceSize.height: 320
                                        }
                                        Canvas {
                                            anchors.centerIn: parent; width:40;height:40;opacity:0.35
                                            visible: modelData.kind === "folder" || modelData.cover_url === ""
                                            onPaint: {
                                                var c=getContext("2d");c.clearRect(0,0,40,40)
                                                if(modelData.kind==="folder"){
                                                    c.fillStyle="#555";c.beginPath();c.moveTo(0,8);c.lineTo(14,8);c.lineTo(18,3);c.lineTo(40,3);c.lineTo(40,32);c.lineTo(0,32);c.closePath();c.fill()
                                                    c.fillStyle=clrBg;c.fillRect(3,10,34,20)
                                                } else {
                                                    c.beginPath();c.arc(20,20,16,0,2*Math.PI);c.strokeStyle="#666";c.lineWidth=2;c.stroke()
                                                    c.beginPath();c.arc(20,20,4,0,2*Math.PI);c.fillStyle="#444";c.fill()
                                                }
                                            }
                                            property string kk: modelData.kind; onKkChanged: requestPaint()
                                        }
                                    }

                                    // CD badge — marks the audio-CD tile apart from library albums
                                    Rectangle {
                                        visible: modelData.kind === "cd"
                                        anchors.left: tileArt.left; anchors.bottom: tileArt.bottom; anchors.margins: 6
                                        width: cdBadgeRow.implicitWidth + 12; height: 18; radius: 3
                                        color: "#cc161616"; border.color: clrBorder; border.width: 1
                                        Row {
                                            id: cdBadgeRow; anchors.centerIn: parent; spacing: 4
                                            MatIcon { anchors.verticalCenter: parent.verticalCenter; name: "album"; size: 11; color: clrAccent }
                                            Text {
                                                anchors.verticalCenter: parent.verticalCenter
                                                text: window._cdBadgeLabel
                                                color: clrAccent; font.pixelSize: 9; font.bold: true; font.family: "Segoe UI"
                                            }
                                        }
                                    }

                                    // Text area
                                    Column {
                                        anchors.top: tileArt.bottom; anchors.left: parent.left; anchors.right: parent.right
                                        anchors.bottom: parent.bottom; anchors.topMargin: 6
                                        anchors.leftMargin: 7; anchors.rightMargin: 7; spacing: 2
                                        WaveText { visible: modelData.loading === true; animating: visible; text: "Reading CD..."; pixelSize: 11 }
                                        Text { width:parent.width;visible:modelData.loading!==true;text:modelData.name;color:clrText;font.pixelSize:11;font.family:"Segoe UI";elide:Text.ElideRight;wrapMode:Text.NoWrap }
                                        Text { width:parent.width;text:modelData.album_artist.length>0?modelData.album_artist:(modelData.year.length>0?modelData.year:"");color:clrText2;font.pixelSize:10;font.family:"Segoe UI";elide:Text.ElideRight }
                                        Text { width:parent.width;visible:modelData.album_artist.length>0&&modelData.year.length>0;text:modelData.year;color:clrMuted;font.pixelSize:9;font.family:"Segoe UI" }
                                    }

                                    // Pin dot
                                    Rectangle { visible:modelData.pinned;width:5;height:5;radius:2.5;color:clrAccent;anchors.top:parent.top;anchors.right:parent.right;anchors.margins:5 }

                                    MouseArea {
                                        id: gridItemHov; anchors.fill: parent; hoverEnabled: true
                                        acceptedButtons: Qt.LeftButton | Qt.RightButton
                                        cursorShape: Qt.PointingHandCursor
                                        onClicked: function(mouse) {
                                            if (mouse.button === Qt.RightButton) {
                                                if (modelData.kind !== "cd") {
                                                    var mp = gridItemHov.mapToItem(rootBg, mouse.x, mouse.y)
                                                    itemCtxMenu.openAt(modelData.path, modelData.kind, mp.x, mp.y)
                                                }
                                                return
                                            }
                                            if (modelData.kind === "cd") { if (player.is_file_mode) player.loadDisc(); window._canGoForwardToAlbum = false; window._showingCdView = true; window._browseDir = ""; window._browseAlbumName = ""; window._fileMode = false; window._view = "album" }
                                            else if (modelData.kind === "folder") library.navigateTo(modelData.path)
                                            else {
                                                window._canGoForwardToAlbum = false; window._showingCdView = false; window._fileMode = false
                                                if (modelData.id === window._playingAlbumDir && player.is_file_mode) {
                                                    window._browseDir = ""; window._browseAlbumName = modelData.name; window._view = "album"
                                                    Qt.callLater(function() { if (player.current_track >= 0 && trackList.count > player.current_track) trackList.positionViewAtIndex(player.current_track, ListView.Center) })
                                                } else { library.browseAlbum(modelData.id); window._browseDir = modelData.id; window._browseAlbumName = modelData.name; window._view = "album" }
                                            }
                                        }
                                        onPressAndHold: {
                                            if (modelData.kind !== "cd") {
                                                var mp = gridItemHov.mapToItem(rootBg, gridItemHov.mouseX, gridItemHov.mouseY)
                                                itemCtxMenu.openAt(modelData.path, modelData.kind, mp.x, mp.y)
                                            }
                                        }
                                    }
                                }
                            }

                            // List view
                            ListView {
                                id: libListView
                                anchors.fill: parent
                                visible: !parent.useGrid && parent.nodes.length > 0
                                clip: true; spacing: 0
                                model: parent.nodes
                                ScrollBar.vertical: ScrollBar {
                                    policy: ScrollBar.AsNeeded
                                    contentItem: Rectangle { implicitWidth:4;radius:2;color:clrMuted;opacity:parent.active?0.85:0.3 }
                                    background: Rectangle { color:"transparent" }
                                }
                                delegate: Rectangle {
                                    width: libListView.width; height: 44
                                    required property var modelData
                                    color: listItemHov.containsMouse ? "#141414" : "transparent"
                                    Behavior on color { ColorAnimation { duration: 100 } }
                                    Rectangle { anchors.bottom:parent.bottom;anchors.left:parent.left;anchors.right:parent.right;height:1;color:clrBorder;opacity:0.5 }
                                    RowLayout {
                                        anchors.fill:parent;anchors.leftMargin:12;anchors.rightMargin:12;spacing:10
                                        Rectangle {
                                            width:32;height:32;radius:3;color:clrSurf2;clip:true
                                            Image{anchors.fill:parent;fillMode:Image.Stretch;smooth:true;mipmap:true;source:modelData.cover_url||"";visible:status===Image.Ready
                                                asynchronous:true;sourceSize.width:64;sourceSize.height:64}
                                            Canvas{anchors.centerIn:parent;width:16;height:16;opacity:0.6;visible:modelData.kind==="folder"||modelData.cover_url===""
                                                onPaint:{var c=getContext("2d");c.clearRect(0,0,16,16);if(modelData.kind==="folder"){c.fillStyle="#555";c.fillRect(0,3,16,11)}else{c.beginPath();c.arc(8,8,6,0,2*Math.PI);c.strokeStyle="#555";c.lineWidth=1.5;c.stroke()}}
                                                property string kk:modelData.kind;onKkChanged:requestPaint()}
                                        }
                                        Column{Layout.fillWidth:true;spacing:1
                                            WaveText{visible:modelData.loading===true;animating:visible;text:"Reading CD...";pixelSize:12}
                                            Text{width:parent.width;visible:modelData.loading!==true;text:modelData.name;color:clrText;font.pixelSize:12;font.family:"Segoe UI";elide:Text.ElideRight}
                                            Text{width:parent.width;visible:modelData.album_artist.length>0;text:modelData.album_artist;color:clrText2;font.pixelSize:10;font.family:"Segoe UI";elide:Text.ElideRight}
                                        }
                                        // CD badge — marks the audio-CD row apart from library albums
                                        Rectangle{
                                            visible: modelData.kind === "cd"
                                            width: listCdBadgeRow.implicitWidth + 12; height: 18; radius: 3
                                            color: "transparent"; border.color: clrBorder; border.width: 1
                                            Row {
                                                id: listCdBadgeRow; anchors.centerIn: parent; spacing: 4
                                                MatIcon { anchors.verticalCenter: parent.verticalCenter; name: "album"; size: 11; color: clrAccent }
                                                Text {
                                                    anchors.verticalCenter: parent.verticalCenter
                                                    text: window._cdBadgeLabel
                                                    color: clrAccent; font.pixelSize: 9; font.bold: true; font.family: "Segoe UI"
                                                }
                                            }
                                        }
                                        Text{text:modelData.year;color:clrText2;font.pixelSize:11;font.family:"Segoe UI";visible:modelData.year.length>0}
                                        Rectangle{width:6;height:6;radius:3;color:clrAccent;visible:modelData.pinned}
                                    }
                                    MouseArea {
                                        id:listItemHov;anchors.fill:parent;hoverEnabled:true
                                        acceptedButtons:Qt.LeftButton|Qt.RightButton;cursorShape:Qt.PointingHandCursor
                                        onClicked: function(mouse) {
                                            if(mouse.button===Qt.RightButton){
                                                if(modelData.kind!=="cd"){var mp=listItemHov.mapToItem(rootBg,mouse.x,mouse.y);itemCtxMenu.openAt(modelData.path,modelData.kind,mp.x,mp.y)}
                                                return
                                            }
                                            if(modelData.kind==="cd"){if(player.is_file_mode)player.loadDisc();window._canGoForwardToAlbum=false;window._showingCdView=true;window._browseDir="";window._browseAlbumName="";window._fileMode=false;window._view="album"}
                                            else if(modelData.kind==="folder") library.navigateTo(modelData.path)
                                            else {
                                                window._canGoForwardToAlbum=false; window._showingCdView=false; window._fileMode=false
                                                if (modelData.id === window._playingAlbumDir && player.is_file_mode) {
                                                    window._browseDir=""; window._browseAlbumName=modelData.name; window._view="album"
                                                    Qt.callLater(function() { if (player.current_track >= 0 && trackList.count > player.current_track) trackList.positionViewAtIndex(player.current_track, ListView.Center) })
                                                } else { library.browseAlbum(modelData.id); window._browseDir = modelData.id; window._browseAlbumName = modelData.name; window._view = "album" }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // ── Track list (visible when _view === "album") ────────
                    Item {
                        anchors.top: rightPathBar.bottom
                        anchors.left: parent.left; anchors.right: parent.right; anchors.bottom: parent.bottom
                        visible: _view === "album"

                        Item{anchors.fill:parent;visible:window._cdLoading&&_browseDir==="";z:5
                            Shape{anchors.centerIn:parent;width:26;height:26
                                RotationAnimator on rotation{from:0;to:360;duration:800;loops:Animation.Infinite;running:window._cdLoading}
                                ShapePath{strokeColor:clrText2;strokeWidth:2;fillColor:"transparent";capStyle:ShapePath.RoundCap
                                    PathAngleArc{centerX:13;centerY:13;radiusX:10;radiusY:10;startAngle:-90;sweepAngle:250}}}
                            WaveText{anchors.horizontalCenter:parent.horizontalCenter;anchors.top:parent.verticalCenter;anchors.topMargin:24
                                visible:parent.visible;animating:visible
                                text:"Reading CD...";pixelSize:12}
                        }

                        Item{anchors.fill:parent;visible:!window._cdLoading&&_browseDir===""&&(player.total_tracks===0||(_showingCdView&&player.is_file_mode))
                            Column{anchors.centerIn:parent;spacing:12
                                Canvas{width:48;height:48;anchors.horizontalCenter:parent.horizontalCenter;opacity:0.45;onPaint:{var c=getContext("2d");c.clearRect(0,0,48,48);c.beginPath();c.arc(24,24,21,0,2*Math.PI);c.strokeStyle="#555";c.lineWidth=1.5;c.stroke();c.beginPath();c.arc(24,24,4,0,2*Math.PI);c.fillStyle="#3a3a3a";c.fill()}}
                                Text{anchors.horizontalCenter:parent.horizontalCenter;text:player.drive_status.length>0?player.drive_status:"No disc inserted";color:clrText2;font.pixelSize:13;font.family:"Segoe UI"}
                            }
                        }

                        ListView{id:trackList;anchors.fill:parent;visible:(_browseDir!==""?_browseTracks.length>0:!player.is_loading&&player.total_tracks>0&&!(_showingCdView&&player.is_file_mode));clip:true;spacing:0
                            boundsBehavior:Flickable.StopAtBounds;flickDeceleration:600;maximumFlickVelocity:6000;model:_effectiveTracklist
                            add:Transition{NumberAnimation{property:"opacity";from:0;to:1;duration:180}}
                            delegate:Rectangle{
                                readonly property bool isCurrent:_browseDir===""&&index===player.current_track
                                width:trackList.width;height:42
                                color:isCurrent?clrSurf2:rowMs.containsMouse?"#141414":"transparent"
                                Behavior on color{ColorAnimation{duration:110}}
                                Rectangle{visible:isCurrent;width:2;height:18;anchors.left:parent.left;anchors.verticalCenter:parent.verticalCenter;color:clrAccent}
                                Rectangle{anchors.bottom:parent.bottom;anchors.left:parent.left;anchors.right:parent.right;height:1;color:clrBorder;opacity:0.5}
                                RowLayout{anchors.fill:parent;anchors.leftMargin:14;anchors.rightMargin:14;spacing:10
                                    Text{Layout.preferredWidth:22;text:(index+1).toString().padStart(2,"0");color:isCurrent?clrAccent:"#3a3a3a";font.pixelSize:11;font.bold:true;font.family:"Segoe UI";Behavior on color{ColorAnimation{duration:110}}}
                                    Column{Layout.fillWidth:true;spacing:1
                                        ScrollText{width:parent.width;text:modelData.title;textColor:isCurrent?clrText:clrText2;pixelSize:13}
                                        Text{width:parent.width;visible:modelData.artist!=="";text:modelData.artist;color:isCurrent?"#888":"#3a3a3a";font.pixelSize:10;font.family:"Segoe UI";elide:Text.ElideRight}
                                    }
                                    Text{text:modelData.duration;color:isCurrent?"#888":"#3a3a3a";font.pixelSize:11;font.family:"Consolas, monospace";Behavior on color{ColorAnimation{duration:110}}}
                                }
                                MouseArea{id:rowMs;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:window.playBrowsedTrack(index)}
                            }
                            ScrollBar.vertical:ScrollBar{id:vScrollBar;policy:ScrollBar.AsNeeded
                                contentItem:Rectangle{implicitWidth:4;radius:2;color:clrMuted;visible:vScrollBar.size<1.0;opacity:vScrollBar.active?0.85:0.3;Behavior on opacity{NumberAnimation{duration:200}}}
                                background:Rectangle{color:"transparent"}}
                        }
                    }
                }
            }

            // ── Seek section ──────────────────────────────────────────────
            Rectangle{Layout.fillWidth:true;height:1;color:clrBorder;visible:_view==="album"||player.is_playing}

            TextMetrics{id:timeMetrics;font.pixelSize:11;font.family:Qt.platform.os==="osx"?"Menlo":"Consolas";text:"00:00"}

            // Single compact row: current time \u00B7 seek slider \u00B7 total time
            RowLayout{
                Layout.fillWidth:true;Layout.leftMargin:14;Layout.rightMargin:14;Layout.topMargin:8;Layout.bottomMargin:6
                spacing:10;visible:_view==="album"||player.is_playing
                Text{text:formatTime(player.current_time);color:clrText;font.pixelSize:11;font.family:Qt.platform.os==="osx"?"Menlo":"Consolas";Layout.preferredWidth:timeMetrics.advanceWidth;horizontalAlignment:Text.AlignLeft}
                Slider{id:seekSlider;Layout.fillWidth:true
                    implicitHeight:20;padding:0;from:0;to:Math.max(player.total_time,1);value:pressed?value:player.current_time
                    enabled:player.total_tracks>0;onPressedChanged:{if(!pressed)player.seek(value)}
                    background:Item{implicitHeight:20
                        Rectangle{anchors.verticalCenter:parent.verticalCenter;width:parent.width;height:3;radius:1;color:clrSurf2
                            Rectangle{id:seekFill;width:parent.width*seekSlider.visualPosition;height:parent.height;radius:1;color:clrAccent
                                Behavior on width{enabled:!seekSlider.pressed;NumberAnimation{duration:60;easing.type:Easing.OutSine}}
                                Rectangle{anchors.top:parent.top;anchors.left:parent.left;anchors.right:parent.right;height:1;radius:1;color:"#ffffff";opacity:0.08}}}}
                    handle:Rectangle{x:seekSlider.visualPosition*seekSlider.availableWidth-width/2;y:seekSlider.availableHeight/2-height/2
                        width:11;height:11;radius:5.5;color:seekSlider.pressed?"#ffffff":clrAccent;visible:player.total_tracks>0
                        opacity:seekSlider.hovered||seekSlider.pressed?1.0:0.0
                        Behavior on opacity{NumberAnimation{duration:130}}
                        Behavior on color{ColorAnimation{duration:80}}
                        Rectangle{anchors.fill:parent;anchors.margins:-1;radius:parent.radius+1;color:"transparent";border.color:"#ffffff";border.width:1;opacity:0.08}}
                }
                Text{text:formatTime(player.total_time);color:clrText2;font.pixelSize:11;font.family:Qt.platform.os==="osx"?"Menlo":"Consolas";Layout.preferredWidth:timeMetrics.advanceWidth;horizontalAlignment:Text.AlignRight}
            }

            // ── Transport + volume ────────────────────────────────────────
            Rectangle{Layout.fillWidth:true;height:1;color:clrBorder}
            RowLayout{Layout.fillWidth:true;Layout.leftMargin:14;Layout.rightMargin:14;Layout.topMargin:10;Layout.bottomMargin:12;spacing:2
                Item{width:30;height:30;opacity:(player.current_track>0||player.current_time>4)?1.0:0.26;Behavior on opacity{NumberAnimation{duration:160}}
                    MatIcon{anchors.centerIn:parent;name:"prev";size:14;color:clrText}
                    MouseArea{anchors.fill:parent;enabled:player.current_track>0||player.current_time>4;cursorShape:enabled?Qt.PointingHandCursor:Qt.ArrowCursor
                        onClicked:{if(player.current_time>4){player.seek(0)}else{var wp=player.is_playing;player.previousTrack();if(wp)player.playPause()}}}}
                Rectangle{id:ppBtn;width:38;height:38;radius:4;color:ppMs.pressed?clrSurf2:ppMs.containsMouse?"#1c1c1c":clrSurface
                    opacity:player.total_tracks>0&&player.current_track>=0?1.0:0.32;border.color:clrBorder;border.width:1
                    Behavior on color{ColorAnimation{duration:90}}
                    Behavior on opacity{NumberAnimation{duration:150}}
                    MatIcon{anchors.centerIn:parent;size:14;color:clrText;name:player.is_playing?"pause":"play"}
                    MouseArea{id:ppMs;anchors.fill:parent;hoverEnabled:true;enabled:player.total_tracks>0&&player.current_track>=0;cursorShape:enabled?Qt.PointingHandCursor:Qt.ArrowCursor;onClicked:player.playPause()}}
                Item{width:30;height:30;opacity:player.current_track>=0&&player.current_track<player.total_tracks-1?1.0:0.26;Behavior on opacity{NumberAnimation{duration:160}}
                    MatIcon{anchors.centerIn:parent;name:"next";size:14;color:clrText}
                    MouseArea{anchors.fill:parent;enabled:player.current_track>=0&&player.current_track<player.total_tracks-1;cursorShape:enabled?Qt.PointingHandCursor:Qt.ArrowCursor
                        onClicked:{var wp=player.is_playing;player.nextTrack();if(wp)player.playPause()}}}

                // ── Now playing: centered title / artist; click → current song ──
                Item{
                    Layout.fillWidth:true; Layout.preferredHeight:38
                    Column{
                        anchors.verticalCenter:parent.verticalCenter
                        anchors.left:parent.left; anchors.right:parent.right
                        anchors.leftMargin:12; anchors.rightMargin:12; spacing:2
                        visible: player.total_tracks>0 && player.current_track>=0
                        ScrollText{
                            width:parent.width; centered:true
                            text:(player.track_titles[player.current_track]||"").trim()
                            textColor:npMa.containsMouse?clrAccent:clrText; pixelSize:12; bold:true
                        }
                        ScrollText{
                            width:parent.width; centered:true
                            text:{
                                var rawArt=(player.track_artists[player.current_track]||"").trim()
                                var artist=rawArt.length>0?rawArt:(player.album_artist||"").trim()
                                if(player.is_single_file) return artist
                                var num=(player.current_track+1).toString().padStart(2,"0")
                                return artist.length>0 ? num+"  ·  "+artist : num+"  ·  "+(player.album_title||"").trim()
                            }
                            textColor:clrText2; pixelSize:10
                        }
                    }
                    MouseArea{
                        id:npMa; anchors.fill:parent; hoverEnabled:true
                        enabled: player.total_tracks>0 && player.current_track>=0
                        cursorShape: enabled?Qt.PointingHandCursor:Qt.ArrowCursor
                        onClicked:{
                            window._browseDir=""; window._browseAlbumName=""; window._view="album"
                            Qt.callLater(function(){
                                if(player.current_track>=0 && trackList.count>player.current_track)
                                    trackList.positionViewAtIndex(player.current_track, ListView.Center)
                            })
                        }
                    }
                }
                MatIcon{size:16;color:clrText2
                    name:volSlider.value<=0.001?"volume-mute":(volSlider.value<0.5?"volume-low":"volume")}
                Slider{id:volSlider;Layout.preferredWidth:88;implicitHeight:30;padding:0;from:0;to:1;value:1.0
                    Component.onCompleted:player.setVolumeLevel(1.0);onMoved:player.setVolumeLevel(value)
                    background:Item{implicitHeight:30;Rectangle{anchors.verticalCenter:parent.verticalCenter;width:parent.width;height:3;radius:1;color:clrSurf2
                        Rectangle{width:parent.width*volSlider.value;height:parent.height;radius:1;color:clrText2
                            Rectangle{anchors.top:parent.top;anchors.left:parent.left;anchors.right:parent.right;height:1;radius:1;color:"#ffffff";opacity:0.08}}}}
                    handle:Rectangle{x:volSlider.visualPosition*volSlider.availableWidth-width/2;y:volSlider.availableHeight/2-height/2;width:9;height:9;radius:4.5
                        color:volSlider.pressed?"#ffffff":clrText2;opacity:volSlider.hovered||volSlider.pressed?1.0:0.0
                        Behavior on opacity{NumberAnimation{duration:130}}
                        Behavior on color{ColorAnimation{duration:80}}}}
            }
        }

        // ── Item context menu (right-click on albums & folders) ─────────
        Rectangle {
            id: itemCtxMenu
            visible: false; z: 200
            width: 150; radius: 4; clip: true
            color: clrSurf2; border.color: clrBorder; border.width: 1
            height: ctxMenuCol.implicitHeight + 8
            property string targetPath: ""
            property var menuItems: []

            function openAt(path, kind, mx, my) {
                targetPath = path
                var sj; try { sj = JSON.parse(library.settings_json.toString()) } catch(e) { sj = {} }
                var pinned  = (sj.pinned_paths    || []).some(function(p){ return p === path })
                var merged  = (sj.merged_folders  || []).some(function(p){ return p === path })
                var ign     = (sj.ignored_folders || []).some(function(p){ return p === path })
                var items = []
                items.push(pinned ? {label: "Unpin",   action: "unpin"} : {label: "Pin",    action: "pin"})
                if (kind === "folder") {
                    items.push(merged ? {label: "Unmerge",  action: "merge_remove"}  : {label: "Merge",  action: "merge"})
                    items.push(ign   ? {label: "Unignore", action: "ignore_remove"} : {label: "Ignore", action: "ignore"})
                }
                menuItems = items
                var h = items.length * 28 + 8
                var fx = mx; if (fx + width  > rootBg.width  - 4) fx = Math.max(4, mx - width)
                var fy = my; if (fy + h > rootBg.height - 4) fy = Math.max(4, my - h)
                x = fx; y = fy
                visible = true
            }

            Column {
                id: ctxMenuCol
                anchors.left: parent.left; anchors.right: parent.right
                anchors.top: parent.top; anchors.topMargin: 4
                Repeater {
                    model: itemCtxMenu.menuItems
                    Rectangle {
                        width: ctxMenuCol.width; height: 28
                        color: ctxRowMa.containsMouse ? clrSurface : "transparent"
                        Behavior on color { ColorAnimation { duration: 80 } }
                        Text { anchors.verticalCenter: parent.verticalCenter; anchors.left: parent.left; anchors.leftMargin: 12
                            text: modelData.label; color: clrText; font.pixelSize: 12; font.family: "Segoe UI" }
                        MouseArea { id: ctxRowMa; anchors.fill: parent; hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                            onClicked: { itemCtxMenu.visible = false; library.setFolderOption(itemCtxMenu.targetPath, modelData.action) }
                        }
                    }
                }
            }
            MouseArea { anchors.fill: parent; acceptedButtons: Qt.NoButton }
        }
        MouseArea {
            anchors.fill: parent; z: 199; visible: itemCtxMenu.visible
            acceptedButtons: Qt.LeftButton | Qt.RightButton; onClicked: itemCtxMenu.visible = false
        }

        // ── No disc popup ─────────────────────────────────────────────────
        Popup {
            id: noDiscPopup
            anchors.centerIn: parent; z: 300
            width: 260; height: 80; modal: true; focus: true
            background: Rectangle { color:clrSurf2; border.color:clrBorder; border.width:1; radius:6 }
            Column { anchors.centerIn:parent; spacing:8
                Text { anchors.horizontalCenter:parent.horizontalCenter; text:"No disc in drive"; color:clrText; font.pixelSize:13; font.family:"Segoe UI"; font.bold:true }
                Text { anchors.horizontalCenter:parent.horizontalCenter; text:"Insert a CD and try again"; color:clrText2; font.pixelSize:11; font.family:"Segoe UI" }
            }
        }

        // ── Music dir popup ───────────────────────────────────────────────
        Rectangle {
            id: musicDirPopup
            anchors.centerIn: parent; z: 300
            visible: !library.music_dir_set
            width: 380; height: 160; radius: 6
            color: clrSurf2; border.color: clrBorder; border.width: 1

            ColumnLayout {
                anchors.fill: parent; anchors.margins: 24; spacing: 14
                Text { text:"Where is your music?"; color:clrText; font.pixelSize:14; font.family:"Segoe UI"; font.bold:true; Layout.alignment:Qt.AlignHCenter }
                Text { text:"Kanae couldn't find a Music folder on this system."; color:clrText2; font.pixelSize:11; font.family:"Segoe UI"; Layout.alignment:Qt.AlignHCenter; wrapMode:Text.WordWrap; horizontalAlignment:Text.AlignHCenter; Layout.fillWidth:true }
                RowLayout { Layout.fillWidth:true; spacing:8
                    Rectangle {
                        Layout.fillWidth:true; height:28; radius:3; color:clrBg; border.color:clrBorder; border.width:1
                        TextInput {
                            id:musicDirInput; anchors.fill:parent; anchors.leftMargin:8; anchors.rightMargin:8
                            verticalAlignment:TextInput.AlignVCenter; color:clrText; font.pixelSize:11; font.family:"Segoe UI"; clip:true
                        }
                        Text { anchors.verticalCenter:parent.verticalCenter; anchors.left:parent.left; anchors.leftMargin:8
                            text:"Path to music folder"; color:clrText2; font.pixelSize:11; font.family:"Segoe UI"
                            visible:musicDirInput.text.length===0; opacity:0.5 }
                    }
                    Rectangle { width:28;height:28;radius:3;color:browseHov.containsMouse?clrSurf2:"transparent";border.color:clrBorder;border.width:1
                        Behavior on color{ColorAnimation{duration:100}}
                        Text{anchors.centerIn:parent;text:"\u2026";color:clrText2;font.pixelSize:14;font.family:"Segoe UI"}
                        MouseArea{id:browseHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:library.openFolderPicker()} }
                    Rectangle { width:52;height:28;radius:3;color:confirmHov.containsMouse?clrSurface:"#1a1a1a";border.color:clrBorder;border.width:1
                        Behavior on color{ColorAnimation{duration:100}}
                        Text{anchors.centerIn:parent;text:"OK";color:clrText;font.pixelSize:11;font.family:"Segoe UI"}
                        MouseArea{id:confirmHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor
                            onClicked:{ if(musicDirInput.text.length>0) library.setMusicDir(musicDirInput.text) }} }
                }
            }
        }

        // ── Scan progress toast ───────────────────────────────────────────
        Rectangle {
            anchors.left: parent.left; anchors.bottom: parent.bottom; anchors.margins: 12; z: 250
            visible: library.is_scanning && library.scan_message.length > 0
            width: toastText.implicitWidth + 24; height: 28; radius: 4
            color: clrSurface; border.color: clrBorder; border.width: 1
            opacity: visible ? 0.92 : 0
            Behavior on opacity { NumberAnimation { duration: 200 } }
            Row { anchors.centerIn:parent; spacing:8
                Shape {
                    width:14;height:14;anchors.verticalCenter:parent.verticalCenter
                    RotationAnimator on rotation{from:0;to:360;duration:900;loops:Animation.Infinite;running:library.is_scanning}
                    ShapePath{strokeColor:clrAccent;strokeWidth:1.5;fillColor:"transparent";capStyle:ShapePath.RoundCap
                        PathAngleArc{centerX:7;centerY:7;radiusX:5;radiusY:5;startAngle:-90;sweepAngle:250}}
                }
                Text{id:toastText;text:library.scan_message;color:clrAccent;font.pixelSize:11;font.family:"Segoe UI";anchors.verticalCenter:parent.verticalCenter}
            }
        }


    }

    Timer{id:smtcInitTimer;interval:500;repeat:false;running:false;onTriggered:player.initSmtc()}
    Component.onCompleted:{player.scanDrives();player.setVolumeLevel(1.0);smtcInitTimer.start();library.init()}

    function formatTime(s){if(s<0)s=0;var m=Math.floor(s/60);var sec=Math.floor(s%60);return(m<10?"0":"")+m+":"+(sec<10?"0":"")+sec}
}
