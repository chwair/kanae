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
        try {
            var parsed = JSON.parse(j)
            if (parsed.length > 0) console.log("[dbg] _browseTracks updated: " + parsed.length + " tracks, first=" + JSON.stringify(parsed[0]))
            return parsed
        } catch(e) { return [] }
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
    property bool _suppressFileModeFlag: false
    function playBrowsedTrack(idx) {
        if (_browseDir !== "") {
            var paths = _browseTracks.map(function(t){ return t.path })
            console.log("[dbg] playBrowsedTrack: idx=" + idx + " paths=" + paths.length)
            _browseDir = ""; _browseAlbumName = ""
            _suppressFileModeFlag = true
            player.openDroppedPaths(paths)
            _suppressFileModeFlag = false
            console.log("[dbg] after openDroppedPaths: total_tracks=" + player.total_tracks + " is_file_mode=" + player.is_file_mode)
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

    Timer { interval: 100; repeat: true; running: true; onTriggered: player.updatePosition() }
    Timer { interval: player.total_tracks > 0 ? 1000 : 3000; repeat: true
            running: !player.is_loading
            onTriggered: { if (player.drive_list.length === 0) player.scanDrives(); else player.checkDrive() } }
    Timer { interval: 200; repeat: true; running: true; onTriggered: { player.pollLoad(); library.pollScan() } }
    Timer { interval: 300; repeat: true; running: true; onTriggered: player.pollLyrics() }

    DropArea {
        anchors.fill: parent; keys: ["text/uri-list"]
        onDropped: {
            var urls = []
            for (var i = 0; i < drop.urls.length; i++) urls.push(drop.urls[i].toString())
            if (urls.length > 0) { player.openDroppedPaths(urls); drop.accept() }
        }
    }

    property int _lyricsTrackIdx: player.current_track
    on_LyricsTrackIdxChanged: {
        Qt.callLater(function() {
            var title  = (player.track_titles[player.current_track] || "").trim()
            var rawArt = (player.track_artists[player.current_track] || "").trim()
            var artist = rawArt.length > 0 ? rawArt : (player.album_artist || "").trim()
            if (title.length > 0 && player.album_title !== "Unknown Album")
                player.fetchLyrics(title, artist, player.total_time)
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
        title: "Settings – Kanae"
        width: 500; height: 560
        minimumWidth: 400; minimumHeight: 400
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
                    Canvas {
                        anchors.centerIn: parent; width: 8; height: 8
                        property color ic: swClsHov.containsMouse ? "#d07070" : "#686868"
                        onIcChanged: requestPaint()
                        Component.onCompleted: requestPaint()
                        onPaint: {
                            var c = getContext("2d"); c.clearRect(0,0,8,8)
                            c.strokeStyle = ic; c.lineWidth = 1.5; c.lineCap = "round"
                            c.beginPath(); c.moveTo(0,0); c.lineTo(8,8); c.stroke()
                            c.beginPath(); c.moveTo(8,0); c.lineTo(0,8); c.stroke()
                        }
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
                    anchors.margins: 28; spacing: 24

                    Item { height: 8 }

                    // Search paths
                    ColumnLayout {
                        Layout.fillWidth: true; spacing: 8
                        Text { text:"Search Paths"; color:"#dfdfdf"; font.pixelSize:12; font.family:"Segoe UI"; font.bold:true }
                        Text { text:"Folders that Kanae will scan for music"; color:"#686868"; font.pixelSize:10; font.family:"Segoe UI" }

                        Repeater {
                            model: settingsWindow.settingsObj.search_paths || []
                            RowLayout {
                                Layout.fillWidth: true; spacing: 6
                                Rectangle {
                                    Layout.fillWidth:true;height:30;radius:4;color:"#161616";border.color:"#282828";border.width:1;clip:true
                                    Text{anchors.verticalCenter:parent.verticalCenter;anchors.left:parent.left;anchors.right:parent.right;anchors.leftMargin:10;anchors.rightMargin:10;text:modelData;color:"#686868";font.pixelSize:11;font.family:"Segoe UI";elide:Text.ElideRight}
                                }
                                Rectangle {
                                    width:30;height:30;radius:4;color:swRmHov.containsMouse?"#3c1a1a":"#161616";border.color:"#282828";border.width:1
                                    Behavior on color{ColorAnimation{duration:100}}
                                    Canvas{anchors.centerIn:parent;width:8;height:8;onPaint:{var c=getContext("2d");c.clearRect(0,0,8,8);c.strokeStyle=swRmHov.containsMouse?"#d07070":"#686868";c.lineWidth=1.5;c.lineCap="round";c.beginPath();c.moveTo(0,0);c.lineTo(8,8);c.stroke();c.beginPath();c.moveTo(8,0);c.lineTo(0,8);c.stroke()}
                                        property bool d: swRmHov.containsMouse; onDChanged: requestPaint()}
                                    MouseArea{id:swRmHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;property string pv:modelData;onClicked:library.removeSearchPath(pv)}
                                }
                            }
                        }

                        Rectangle {
                            width:swAddLbl.implicitWidth+20;height:28;radius:4
                            color:swAddHov.containsMouse?"#1e1e1e":"transparent";border.color:"#282828";border.width:1
                            Behavior on color{ColorAnimation{duration:100}}
                            Text{id:swAddLbl;anchors.centerIn:parent;text:"+ Add Folder";color:"#686868";font.pixelSize:11;font.family:"Segoe UI"}
                            MouseArea{id:swAddHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:library.openFolderPicker()}
                        }
                    }

                    Rectangle { Layout.fillWidth:true; height:1; color:"#282828" }

                    // Merge all toggle
                    RowLayout {
                        Layout.fillWidth: true; spacing: 12
                        Rectangle {
                            width:18;height:18;radius:4;color:"#161616";border.color:"#282828";border.width:1
                            Rectangle{anchors.fill:parent;anchors.margins:4;radius:2;color:"#bfbfbf";visible:settingsWindow.settingsObj.merge_all_folders===true}
                            MouseArea{anchors.fill:parent;cursorShape:Qt.PointingHandCursor;onClicked:library.setMergeAll(!(settingsWindow.settingsObj.merge_all_folders===true))}
                        }
                        Column {
                            Layout.fillWidth: true; spacing: 3
                            Text{text:"Merge all folders";color:"#dfdfdf";font.pixelSize:12;font.family:"Segoe UI"}
                            Text{text:"Show only albums, hide folder tiles";color:"#686868";font.pixelSize:10;font.family:"Segoe UI"}
                        }
                    }

                    Rectangle { Layout.fillWidth:true; height:1; color:"#282828" }

                    // Merged folders
                    ColumnLayout {
                        Layout.fillWidth: true; spacing: 8
                        visible: (settingsWindow.settingsObj.merged_folders || []).length > 0
                        Text{text:"Merged Folders";color:"#dfdfdf";font.pixelSize:12;font.family:"Segoe UI";font.bold:true}
                        Repeater {
                            model: settingsWindow.settingsObj.merged_folders || []
                            RowLayout { Layout.fillWidth:true;spacing:6
                                Text{Layout.fillWidth:true;text:modelData;color:"#686868";font.pixelSize:11;font.family:"Segoe UI";elide:Text.ElideRight}
                                Rectangle{width:70;height:26;radius:4;color:swUnmergeHov.containsMouse?"#1e1e1e":"transparent";border.color:"#282828";border.width:1;Behavior on color{ColorAnimation{duration:100}}
                                    Text{anchors.centerIn:parent;text:"Unmerge";color:"#686868";font.pixelSize:10;font.family:"Segoe UI"}
                                    MouseArea{id:swUnmergeHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;property string pv:modelData;onClicked:library.setFolderOption(pv,"merge_remove")}}
                            }
                        }
                    }

                    // Ignored folders
                    ColumnLayout {
                        Layout.fillWidth: true; spacing: 8
                        visible: (settingsWindow.settingsObj.ignored_folders || []).length > 0
                        Text{text:"Ignored Folders";color:"#dfdfdf";font.pixelSize:12;font.family:"Segoe UI";font.bold:true}
                        Repeater {
                            model: settingsWindow.settingsObj.ignored_folders || []
                            RowLayout { Layout.fillWidth:true;spacing:6
                                Text{Layout.fillWidth:true;text:modelData;color:"#686868";font.pixelSize:11;font.family:"Segoe UI";elide:Text.ElideRight}
                                Rectangle{width:70;height:26;radius:4;color:swUnignoreHov.containsMouse?"#1e1e1e":"transparent";border.color:"#282828";border.width:1;Behavior on color{ColorAnimation{duration:100}}
                                    Text{anchors.centerIn:parent;text:"Show";color:"#686868";font.pixelSize:10;font.family:"Segoe UI"}
                                    MouseArea{id:swUnignoreHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;property string pv:modelData;onClicked:library.setFolderOption(pv,"ignore_remove")}}
                            }
                        }
                    }

                    Item { height: 12 }
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
                Canvas {
                    anchors.centerIn: parent; width: 12; height: 12
                    property color ic: macSettingsHov.containsMouse ? clrText : clrText2
                    onIcChanged: requestPaint()
                    Component.onCompleted: requestPaint()
                    onPaint: {
                        var c = getContext("2d"); c.clearRect(0,0,12,12)
                        c.fillStyle = ic
                        var cx=6,cy=6,ri=2.6,ro=5.4,n=6
                        c.beginPath()
                        for(var i=0;i<n*2;i++){
                            var a=(i*Math.PI/n)-Math.PI/2
                            var r=i%2===0?ro:ri
                            if(i===0)c.moveTo(cx+r*Math.cos(a),cy+r*Math.sin(a))
                            else c.lineTo(cx+r*Math.cos(a),cy+r*Math.sin(a))
                        }
                        c.closePath(); c.fill()
                        c.globalCompositeOperation="destination-out"
                        c.beginPath(); c.arc(cx,cy,1.8,0,2*Math.PI); c.fill()
                        c.globalCompositeOperation="source-over"
                    }
                }
                MouseArea { id: macSettingsHov; anchors.fill: parent; hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor; onClicked: settingsWindow.show() }
            }

            // Now playing (centre)
            Text {
                anchors.left: Qt.platform.os === "osx" ? trafficLights.right : parent.left
                anchors.right: Qt.platform.os === "osx" ? macSettingsBtn.left : winButtons.left
                anchors.leftMargin: 8; anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                elide: Text.ElideRight
                text: {
                    if (!player.is_playing || player.total_tracks === 0) return ""
                    var num = (player.current_track+1).toString().padStart(2,"0")
                    var title = (player.track_titles[player.current_track] || "").trim()
                    var rawArt = (player.track_artists[player.current_track] || "").trim()
                    var artist = rawArt.length > 0 ? rawArt : (player.album_artist || "").trim()
                    return "\u25B6  " + num + "  \u00B7  " + (artist.length > 0 ? artist + "  \u2014  " : "") + title
                }
                color: clrText2; font.pixelSize: 11; font.family: "Segoe UI"
            }

            // Windows titlebar buttons
            Row {
                id: winButtons; visible: Qt.platform.os !== "osx"
                anchors.right: parent.right; anchors.top: parent.top; height: titleBar.height

                // Settings button (cog icon)
                Rectangle {
                    width: 32; height: parent.height; color: "transparent"
                    Rectangle { anchors.fill:parent; color:settingsHov.containsMouse?clrSurf2:"transparent"; Behavior on color{ColorAnimation{duration:100}} }
                    Canvas {
                        anchors.centerIn: parent; width: 12; height: 12
                        property color ic: settingsHov.containsMouse ? clrText : clrText2
                        onIcChanged: requestPaint()
                        Component.onCompleted: requestPaint()
                        onPaint: {
                            var c = getContext("2d"); c.clearRect(0,0,12,12)
                            c.fillStyle = ic
                            var cx=6,cy=6,ri=2.6,ro=5.4,n=6
                            c.beginPath()
                            for(var i=0;i<n*2;i++){
                                var a=(i*Math.PI/n)-Math.PI/2
                                var r=i%2===0?ro:ri
                                if(i===0)c.moveTo(cx+r*Math.cos(a),cy+r*Math.sin(a))
                                else c.lineTo(cx+r*Math.cos(a),cy+r*Math.sin(a))
                            }
                            c.closePath(); c.fill()
                            c.globalCompositeOperation="destination-out"
                            c.beginPath(); c.arc(cx,cy,1.8,0,2*Math.PI); c.fill()
                            c.globalCompositeOperation="source-over"
                        }
                    }
                    MouseArea { id:settingsHov; anchors.fill:parent; hoverEnabled:true
                        cursorShape:Qt.PointingHandCursor
                        onClicked: settingsWindow.show() }
                }
                Rectangle { width:1; height:parent.height; color:clrBorder }
                Rectangle {
                    width:32;height:parent.height;color:"transparent"
                    Rectangle{anchors.fill:parent;color:minHov.containsMouse?clrSurf2:"transparent";Behavior on color{ColorAnimation{duration:100}}}
                    Canvas{anchors.centerIn:parent;width:8;height:1;onPaint:{var c=getContext("2d");c.clearRect(0,0,8,1);c.fillStyle=clrText2;c.fillRect(0,0,8,1)}}
                    MouseArea{id:minHov;anchors.fill:parent;hoverEnabled:true;onClicked:window.showMinimized()}
                }
                Rectangle {
                    width:32;height:parent.height;color:"transparent"
                    Rectangle{anchors.fill:parent;color:maxHov.containsMouse?clrSurf2:"transparent";Behavior on color{ColorAnimation{duration:100}}}
                    Canvas{id:maxCanvas;anchors.centerIn:parent;width:8;height:8
                        property bool isMax:window.visibility===Window.Maximized
                        property color ic:maxHov.containsMouse?clrText:clrText2
                        onIsMaxChanged:requestPaint();onIcChanged:requestPaint();Component.onCompleted:requestPaint()
                        onPaint:{var c=getContext("2d");c.clearRect(0,0,8,8);c.strokeStyle=ic;c.lineWidth=1.2;c.lineCap="square"
                            if(isMax){c.beginPath();c.moveTo(2.5,0.5);c.lineTo(7.5,0.5);c.lineTo(7.5,5.5);c.stroke();c.strokeRect(0.5,2.5,5,5)}
                            else{c.strokeRect(0.5,0.5,7,7)}}}
                    MouseArea{id:maxHov;anchors.fill:parent;hoverEnabled:true;onClicked:window.visibility===Window.Maximized?window.showNormal():window.showMaximized()}
                }
                Rectangle {
                    width:32;height:parent.height;color:"transparent"
                    Rectangle{anchors.fill:parent;color:clsHov.containsMouse?"#3c1a1a":"transparent";Behavior on color{ColorAnimation{duration:100}}}
                    Canvas{anchors.centerIn:parent;width:8;height:8;property color ic:clsHov.containsMouse?"#d07070":clrText2
                        onIcChanged:requestPaint();onPaint:{var c=getContext("2d");c.clearRect(0,0,8,8);c.strokeStyle=ic;c.lineWidth=1.5;c.lineCap="round"
                            c.beginPath();c.moveTo(0,0);c.lineTo(8,8);c.stroke();c.beginPath();c.moveTo(8,0);c.lineTo(0,8);c.stroke()}}
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
                            Image{id:coverImg;anchors.fill:parent;source:player.cover_art_path;fillMode:Image.Stretch;smooth:true;mipmap:true;visible:status===Image.Ready}
                            Canvas{anchors.centerIn:parent;width:44;height:44;opacity:0.3;visible:coverImg.status!==Image.Ready
                                onPaint:{var c=getContext("2d");c.clearRect(0,0,44,44);c.beginPath();c.arc(22,22,20,0,2*Math.PI);c.strokeStyle="#888";c.lineWidth=1.5;c.stroke();c.beginPath();c.arc(22,22,4,0,2*Math.PI);c.fillStyle="#666";c.fill()}}
                            MouseArea{anchors.fill:parent;cursorShape:Qt.PointingHandCursor;onClicked:coverOnSide=true}
                        }
                    }
                    Rectangle{Layout.fillWidth:true;height:sidebarColumn._topSepH;color:clrBorder}
                    Item{id:metaBlock;Layout.fillWidth:true;Layout.preferredHeight:70;clip:true
                        Rectangle{id:thumbRect;x:sidebarColumn._slideX;y:0;height:70;width:sidebarColumn._thumbW;color:clrSurf2
                            border.color:coverImg.status!==Image.Ready?clrBorder:"transparent";border.width:1;clip:true
                            Image{anchors.fill:parent;source:player.cover_art_path;fillMode:Image.Stretch;smooth:true;mipmap:true;visible:coverImg.status===Image.Ready}
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
                                Canvas {
                                    anchors.centerIn:parent;width:7;height:7
                                    onPaint:{ var c=getContext("2d");c.clearRect(0,0,7,7);c.strokeStyle=clrText2;c.lineWidth=1.5;c.lineCap="round";c.lineJoin="round"
                                        c.beginPath();c.moveTo(5,1);c.lineTo(2,3.5);c.lineTo(5,6);c.stroke() }
                                }
                                MouseArea { id:backPathHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor
                                    onClicked: if(library.can_go_back || window._view === "album") window.goBack() }
                            }

                            // Forward
                            Rectangle {
                                width:22;height:22;radius:3
                                color: fwdPathHov.containsMouse && (library.can_go_forward || (_view === "library" && _canGoForwardToAlbum)) ? clrSurf2 : "transparent"
                                opacity: library.can_go_forward || (_view === "library" && _canGoForwardToAlbum) ? 1 : 0.3
                                Behavior on color { ColorAnimation { duration: 80 } }
                                Canvas {
                                    anchors.centerIn:parent;width:7;height:7
                                    onPaint:{ var c=getContext("2d");c.clearRect(0,0,7,7);c.strokeStyle=clrText2;c.lineWidth=1.5;c.lineCap="round";c.lineJoin="round"
                                        c.beginPath();c.moveTo(2,1);c.lineTo(5,3.5);c.lineTo(2,6);c.stroke() }
                                }
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

                            // Grid/List toggle (only relevant in library view)
                            Row {
                                spacing: 4; visible: _view === "library"
                                Rectangle {
                                    width:22;height:22;radius:3
                                    color: gridToggleHov.containsMouse || window._libUseGrid ? clrSurf2 : "transparent"
                                    Behavior on color { ColorAnimation { duration: 80 } }
                                    Canvas { anchors.centerIn:parent;width:9;height:9
                                        property bool dep: window._libUseGrid; onDepChanged: requestPaint()
                                        onPaint:{ var c=getContext("2d");c.clearRect(0,0,9,9)
                                            c.fillStyle=window._libUseGrid?clrText:clrText2
                                            c.fillRect(0,0,4,4);c.fillRect(5,0,4,4);c.fillRect(0,5,4,4);c.fillRect(5,5,4,4) } }
                                    MouseArea{id:gridToggleHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:window._libUseGrid=true}
                                }
                                Rectangle {
                                    width:22;height:22;radius:3
                                    color: listToggleHov.containsMouse || !window._libUseGrid ? clrSurf2 : "transparent"
                                    Behavior on color { ColorAnimation { duration: 80 } }
                                    Canvas { anchors.centerIn:parent;width:9;height:9
                                        property bool dep: window._libUseGrid; onDepChanged: requestPaint()
                                        onPaint:{ var c=getContext("2d");c.clearRect(0,0,9,9)
                                            c.fillStyle=!window._libUseGrid?clrText:clrText2
                                            c.fillRect(0,0,9,2);c.fillRect(0,3.5,9,2);c.fillRect(0,7,9,2) } }
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

                        // CD drives bar
                        Rectangle {
                            Layout.fillWidth: true
                            height: player.drive_list.length > 0 ? 64 : 0
                            visible: player.drive_list.length > 0
                            color: clrSurface; clip: true
                            Behavior on height { NumberAnimation { duration: 150 } }
                            Rectangle { anchors.bottom:parent.bottom;anchors.left:parent.left;anchors.right:parent.right;height:1;color:clrBorder }

                            ListView {
                                anchors.fill: parent; anchors.margins: 8
                                orientation: ListView.Horizontal; spacing: 8; clip: true
                                model: player.drive_list
                                delegate: Rectangle {
                                    width: 200; height: 48; radius: 4
                                    color: driveTileHov.containsMouse ? clrSurf2 : "#181818"
                                    border.color: clrBorder; border.width: 1
                                    Behavior on color { ColorAnimation { duration: 100 } }
                                    RowLayout {
                                        anchors.fill: parent; anchors.margins: 8; spacing: 8
                                        Canvas {
                                            width:28;height:28
                                            onPaint:{var c=getContext("2d");c.clearRect(0,0,28,28);c.beginPath();c.arc(14,14,12,0,2*Math.PI);c.strokeStyle="#555";c.lineWidth=1.5;c.stroke();c.beginPath();c.arc(14,14,8,0,2*Math.PI);c.strokeStyle="#3a3a3a";c.lineWidth=1.5;c.stroke();c.beginPath();c.arc(14,14,2,0,2*Math.PI);c.fillStyle=clrBg;c.fill()}
                                        }
                                        Column {
                                            Layout.fillWidth:true;spacing:2
                                            Text{width:parent.width;text:modelData;color:clrText;font.pixelSize:11;font.family:"Segoe UI";elide:Text.ElideRight}
                                            Text{width:parent.width;text:player.total_tracks>0&&player.selected_drive_index===index?player.album_title:"No disc";color:clrText2;font.pixelSize:10;font.family:"Segoe UI";elide:Text.ElideRight}
                                        }
                                    }
                                    MouseArea {
                                        id:driveTileHov;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor
                                        onClicked: { if (player.is_file_mode) player.loadDisc(); _showingCdView = true; _fileMode = false; _view = "album" }
                                    }
                                }
                            }
                        }

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
                                        var cdLoaded = player.total_tracks > 0 && !player.is_file_mode
                                        var cdNode = {
                                            kind: "cd", path: "__cd__",
                                            name: cdLoaded ? (player.album_title || "Audio CD") : "Audio CD",
                                            album_artist: cdLoaded ? (player.album_artist || "") : "",
                                            year: cdLoaded ? (player.album_year || "") : "",
                                            cover_url: cdLoaded ? (player.cover_art_path || "") : "",
                                            pinned: false
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
                                            anchors.fill: parent; fillMode: Image.Stretch; smooth: true; mipmap: true
                                            source: modelData.cover_url || ""; visible: status === Image.Ready
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

                                    // Text area
                                    Column {
                                        anchors.top: tileArt.bottom; anchors.left: parent.left; anchors.right: parent.right
                                        anchors.bottom: parent.bottom; anchors.topMargin: 6
                                        anchors.leftMargin: 7; anchors.rightMargin: 7; spacing: 2
                                        Text { width:parent.width;text:modelData.name;color:clrText;font.pixelSize:11;font.family:"Segoe UI";elide:Text.ElideRight;wrapMode:Text.NoWrap }
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
                                            else { window._canGoForwardToAlbum = false; window._showingCdView = false; window._fileMode = false; library.browseAlbum(modelData.path); window._browseDir = modelData.path; window._browseAlbumName = modelData.name; window._view = "album" }
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
                                            Image{anchors.fill:parent;fillMode:Image.Stretch;smooth:true;mipmap:true;source:modelData.cover_url||"";visible:status===Image.Ready}
                                            Canvas{anchors.centerIn:parent;width:16;height:16;opacity:0.6;visible:modelData.kind==="folder"||modelData.cover_url===""
                                                onPaint:{var c=getContext("2d");c.clearRect(0,0,16,16);if(modelData.kind==="folder"){c.fillStyle="#555";c.fillRect(0,3,16,11)}else{c.beginPath();c.arc(8,8,6,0,2*Math.PI);c.strokeStyle="#555";c.lineWidth=1.5;c.stroke()}}
                                                property string kk:modelData.kind;onKkChanged:requestPaint()}
                                        }
                                        Column{Layout.fillWidth:true;spacing:1
                                            Text{width:parent.width;text:modelData.name;color:clrText;font.pixelSize:12;font.family:"Segoe UI";elide:Text.ElideRight}
                                            Text{width:parent.width;visible:modelData.album_artist.length>0;text:modelData.album_artist;color:clrText2;font.pixelSize:10;font.family:"Segoe UI";elide:Text.ElideRight}
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
                                            else { window._canGoForwardToAlbum=false; window._showingCdView=false; window._fileMode=false; library.browseAlbum(modelData.path); window._browseDir = modelData.path; window._browseAlbumName = modelData.name; window._view = "album" }
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

                        Item{anchors.fill:parent;visible:player.is_loading&&_browseDir==="";z:5
                            Shape{anchors.centerIn:parent;width:26;height:26
                                RotationAnimator on rotation{from:0;to:360;duration:800;loops:Animation.Infinite;running:player.is_loading}
                                ShapePath{strokeColor:clrText2;strokeWidth:2;fillColor:"transparent";capStyle:ShapePath.RoundCap
                                    PathAngleArc{centerX:13;centerY:13;radiusX:10;radiusY:10;startAngle:-90;sweepAngle:250}}}
                            Text{anchors.horizontalCenter:parent.horizontalCenter;anchors.top:parent.verticalCenter;anchors.topMargin:24;text:"Reading disc";color:clrText2;font.pixelSize:12;font.family:"Segoe UI"}
                        }

                        Item{anchors.fill:parent;visible:!player.is_loading&&_browseDir===""&&(player.total_tracks===0||(_showingCdView&&player.is_file_mode))
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
                                MouseArea{id:rowMs;anchors.fill:parent;hoverEnabled:true;cursorShape:Qt.PointingHandCursor;onClicked:{
                                    console.log("[dbg] track clicked: idx=" + index)
                                    window.playBrowsedTrack(index)
                                }}
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

            RowLayout{
                Layout.fillWidth:true;Layout.leftMargin:14;Layout.rightMargin:14;Layout.topMargin:7;Layout.bottomMargin:4
                spacing:0;visible:_view==="album"||player.is_playing
                Text{text:formatTime(player.current_time);color:clrText;font.pixelSize:11;font.family:Qt.platform.os==="osx"?"Menlo":"Consolas";Layout.preferredWidth:timeMetrics.advanceWidth;horizontalAlignment:Text.AlignLeft}
                Item{Layout.preferredWidth:8}
                Item{Layout.fillWidth:true;height:20
                    ScrollText{anchors.fill:parent;visible:player.total_tracks>0&&player.current_track>=0&&!player.is_single_file;centered:true
                        text:{if(player.total_tracks===0||player.current_track<0)return "";var num=(player.current_track+1).toString().padStart(2,"0");var title=(player.track_titles[player.current_track]||"").trim();var rawArt=(player.track_artists[player.current_track]||"").trim();var artist=rawArt.length>0?rawArt:(player.album_artist||"").trim();return num+"  \u00B7  "+artist+"  \u2014  "+title}
                        textColor:clrText2;pixelSize:11;fontFamily:"Segoe UI"}}
                Item{Layout.preferredWidth:8}
                Text{text:formatTime(player.total_time);color:clrText2;font.pixelSize:11;font.family:Qt.platform.os==="osx"?"Menlo":"Consolas";Layout.preferredWidth:timeMetrics.advanceWidth;horizontalAlignment:Text.AlignRight}
            }

            Slider{id:seekSlider;Layout.fillWidth:true;Layout.leftMargin:14;Layout.rightMargin:14;Layout.bottomMargin:10;visible:_view==="album"||player.is_playing
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

            // ── Transport + volume ────────────────────────────────────────
            Rectangle{Layout.fillWidth:true;height:1;color:clrBorder}
            RowLayout{Layout.fillWidth:true;Layout.leftMargin:14;Layout.rightMargin:14;Layout.topMargin:10;Layout.bottomMargin:12;spacing:2
                Item{width:30;height:30;opacity:(player.current_track>0||player.current_time>4)?1.0:0.26;Behavior on opacity{NumberAnimation{duration:160}}
                    Canvas{anchors.centerIn:parent;width:13;height:13;onPaint:{var c=getContext("2d");c.clearRect(0,0,13,13);c.fillStyle=clrText;c.fillRect(0,0,2,13);c.beginPath();c.moveTo(12,0);c.lineTo(2,6.5);c.lineTo(12,13);c.closePath();c.fill()}}
                    MouseArea{anchors.fill:parent;enabled:player.current_track>0||player.current_time>4;cursorShape:enabled?Qt.PointingHandCursor:Qt.ArrowCursor
                        onClicked:{if(player.current_time>4){player.seek(0)}else{var wp=player.is_playing;player.previousTrack();if(wp)player.playPause()}}}}
                Rectangle{id:ppBtn;width:38;height:38;radius:4;color:ppMs.pressed?clrSurf2:ppMs.containsMouse?"#1c1c1c":clrSurface
                    opacity:player.total_tracks>0&&player.current_track>=0?1.0:0.32;border.color:clrBorder;border.width:1
                    Behavior on color{ColorAnimation{duration:90}}
                    Behavior on opacity{NumberAnimation{duration:150}}
                    Canvas{id:ppCanvas;anchors.centerIn:parent;width:13;height:13
                        Connections{target:player;function onIs_playingChanged(){ppCanvas.requestPaint()}}
                        onPaint:{var c=getContext("2d");c.clearRect(0,0,13,13);c.fillStyle=clrText;if(player.is_playing){c.fillRect(0,0,4,13);c.fillRect(8,0,4,13)}else{c.beginPath();c.moveTo(2,0);c.lineTo(13,6.5);c.lineTo(2,13);c.closePath();c.fill()}}}
                    MouseArea{id:ppMs;anchors.fill:parent;hoverEnabled:true;enabled:player.total_tracks>0&&player.current_track>=0;cursorShape:enabled?Qt.PointingHandCursor:Qt.ArrowCursor;onClicked:player.playPause()}}
                Item{width:30;height:30;opacity:player.current_track>=0&&player.current_track<player.total_tracks-1?1.0:0.26;Behavior on opacity{NumberAnimation{duration:160}}
                    Canvas{anchors.centerIn:parent;width:13;height:13;onPaint:{var c=getContext("2d");c.clearRect(0,0,13,13);c.fillStyle=clrText;c.beginPath();c.moveTo(0,0);c.lineTo(10,6.5);c.lineTo(0,13);c.closePath();c.fill();c.fillRect(11,0,2,13)}}
                    MouseArea{anchors.fill:parent;enabled:player.current_track>=0&&player.current_track<player.total_tracks-1;cursorShape:enabled?Qt.PointingHandCursor:Qt.ArrowCursor
                        onClicked:{var wp=player.is_playing;player.nextTrack();if(wp)player.playPause()}}}
                Item{Layout.fillWidth:true}
                Canvas{id:volIconCanvas;width:16;height:13;property real lvl:volSlider.value;onLvlChanged:requestPaint()
                    onPaint:{var c=getContext("2d");c.clearRect(0,0,16,13);c.fillStyle=clrText2;c.fillRect(0,4,4,5);c.beginPath();c.moveTo(4,4);c.lineTo(8,1);c.lineTo(8,12);c.lineTo(4,9);c.closePath();c.fill();c.strokeStyle=clrText2;c.lineWidth=1.3;if(lvl>0.05){c.beginPath();c.arc(8,6.5,3.2,-0.7,0.7);c.stroke()};if(lvl>0.5){c.beginPath();c.arc(8,6.5,5.5,-0.7,0.7);c.stroke()}}}
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
            anchors.right: parent.right; anchors.bottom: parent.bottom; anchors.margins: 12; z: 250
            visible: library.is_scanning && library.scan_message.length > 0
            width: toastText.implicitWidth + 24; height: 28; radius: 4
            color: clrSurf2; border.color: clrBorder; border.width: 1
            opacity: visible ? 0.92 : 0
            Behavior on opacity { NumberAnimation { duration: 200 } }
            Row { anchors.centerIn:parent; spacing:8
                Shape {
                    width:14;height:14;anchors.verticalCenter:parent.verticalCenter
                    RotationAnimator on rotation{from:0;to:360;duration:900;loops:Animation.Infinite;running:library.is_scanning}
                    ShapePath{strokeColor:clrText2;strokeWidth:1.5;fillColor:"transparent";capStyle:ShapePath.RoundCap
                        PathAngleArc{centerX:7;centerY:7;radiusX:5;radiusY:5;startAngle:-90;sweepAngle:250}}
                }
                Text{id:toastText;text:library.scan_message;color:clrText2;font.pixelSize:11;font.family:"Segoe UI";anchors.verticalCenter:parent.verticalCenter}
            }
        }


    }

    Timer{id:smtcInitTimer;interval:500;repeat:false;running:false;onTriggered:player.initSmtc()}
    Component.onCompleted:{player.scanDrives();player.setVolumeLevel(0.8);smtcInitTimer.start();library.init()}

    function formatTime(s){if(s<0)s=0;var m=Math.floor(s/60);var sec=Math.floor(s%60);return(m<10?"0":"")+m+":"+(sec<10?"0":"")+sec}
}
