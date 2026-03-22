import QtQuick 2.15
import QtQuick.Controls.Basic 2.15
import QtQuick.Layouts 1.15
import QtQuick.Shapes 1.15
import com.kdab.kanae 1.0

ApplicationWindow {
    id: window
    visible: true
    width: 660
    height: 540
    minimumWidth: 660
    minimumHeight: 540
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

    // Toggles cover art position: false = above metadata, true = left of metadata
    property bool coverOnSide: false

    // ── Backend ────────────────────────────────────────────────────────────
    PlayerController { id: player }

    Timer { interval: 500; repeat: true; running: true
            onTriggered: player.updatePosition() }
    Timer { interval: player.total_tracks > 0 ? 1000 : 3000; repeat: true; running: !player.is_loading && !player.is_file_mode
            onTriggered: {
                if (player.drive_list.length === 0)
                    player.scanDrives()
                else
                    player.checkDrive()
            } }
    Timer { interval: 200; repeat: true; running: true
            onTriggered: player.pollLoad() }
    Timer { interval: 300; repeat: true; running: true
            onTriggered: player.pollLyrics() }

    // ── Drag-and-drop file loading ─────────────────────────────────────────
    DropArea {
        anchors.fill: parent
        keys: ["text/uri-list"]
        onDropped: {
            var urls = []
            for (var i = 0; i < drop.urls.length; i++)
                urls.push(drop.urls[i].toString())
            if (urls.length > 0) {
                player.openDroppedPaths(urls)
                drop.accept()
            }
        }
    }

    property int _lyricsTrackIdx: player.current_track
    on_LyricsTrackIdxChanged: {
        Qt.callLater(function() {
            var title  = (player.track_titles[player.current_track] || "").trim()
            var rawArt = (player.track_artists[player.current_track] || "").trim()
            var artist = rawArt.length > 0 ? rawArt : (player.album_artist || "").trim()
            // Skip lookup when we have no real metadata (fallback "Track N" titles).
            var hasMetadata = player.album_title !== "Unknown Album"
            if (title.length > 0 && hasMetadata)
                player.fetchLyrics(title, artist, player.total_time)
        })
    }

    // ── Scrolling-text helper ─────────────────────────────────────────────────
    component ScrollText: Item {
        id: stRoot
        implicitHeight: stLabel.implicitHeight
        clip: true

        property string text:       ""
        property color  textColor:  clrText
        property real   pixelSize:  13
        property string fontFamily: "Segoe UI"
        property bool   bold:       false
        property bool   centered:   false

        readonly property bool scrolling: stLabel.implicitWidth > stRoot.width

        function _resetX() {
            stLabel.x = (stRoot.centered && !stRoot.scrolling)
                        ? Math.round((stRoot.width - stLabel.implicitWidth) / 2)
                        : 0
        }

        Text {
            id: stLabel
            text:            stRoot.text
            color:           stRoot.textColor
            font.pixelSize:  stRoot.pixelSize
            font.family:     stRoot.fontFamily
            font.bold:       stRoot.bold

            onImplicitWidthChanged: if (!stRoot.scrolling) stRoot._resetX()

            SequentialAnimation on x {
                loops: Animation.Infinite
                running: stRoot.scrolling
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

    // ── Rounded-window root background ─────────────────────────────────────
    Rectangle {
        id: rootBg
        anchors.fill: parent
        color: clrBg
        radius: 6
        clip: true

        // ── Frameless titlebar ──────────────────────────────────────────────
        Rectangle {
            id: titleBar
            anchors { top: parent.top; left: parent.left; right: parent.right }
            height: 30
            color: "transparent"
            z: 10

            // ── Drag handler — must be declared first so child items sit above it.
            // On macOS, startSystemMove() must be called immediately on mouseDown
            // (the system tracks whether a drag follows); a DragHandler's threshold
            // delay causes the spotty behaviour. mouse.accepted = false lets the
            // press propagate to traffic-light buttons underneath.
            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton
                propagateComposedEvents: true
                onPressed: function(mouse) {
                    window.startSystemMove()
                    mouse.accepted = false
                }
            }

            // ── macOS traffic lights ────────────────────────────────────────
            Item {
                id: trafficLights
                visible: Qt.platform.os === "osx"
                anchors.left: parent.left
                anchors.leftMargin: 12
                anchors.verticalCenter: parent.verticalCenter
                width: 52; height: 12

                // Single hover handler covering the whole group.
                HoverHandler { id: tlHover }

                // Close (red)
                Rectangle {
                    x: 0; width: 12; height: 12; radius: 6
                    color: tlHover.hovered ? "#FF5F57" : "#606060"
                    Behavior on color { ColorAnimation { duration: 120 } }
                    Canvas {
                        anchors.centerIn: parent; width: 8; height: 8
                        visible: tlCloseArea.containsMouse
                        onVisibleChanged: if (visible) requestPaint()
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.strokeStyle = "#5a0000"
                            ctx.lineWidth = 1.2; ctx.lineCap = "round"
                            ctx.beginPath(); ctx.moveTo(2, 2); ctx.lineTo(6, 6); ctx.stroke()
                            ctx.beginPath(); ctx.moveTo(6, 2); ctx.lineTo(2, 6); ctx.stroke()
                        }
                    }
                    MouseArea {
                        id: tlCloseArea; anchors.fill: parent; hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: Qt.quit()
                    }
                }

                // Minimize (yellow)
                Rectangle {
                    x: 20; width: 12; height: 12; radius: 6
                    color: tlHover.hovered ? "#FEBC2E" : "#606060"
                    Behavior on color { ColorAnimation { duration: 120 } }
                    Canvas {
                        anchors.centerIn: parent; width: 8; height: 2
                        visible: tlMinArea.containsMouse
                        onVisibleChanged: if (visible) requestPaint()
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.fillStyle = "#4b3300"
                            ctx.fillRect(0, 0, 8, 2)
                        }
                    }
                    MouseArea {
                        id: tlMinArea; anchors.fill: parent; hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: window.showMinimized()
                    }
                }

                // Maximize / restore (green)
                Rectangle {
                    x: 40; width: 12; height: 12; radius: 6
                    color: tlHover.hovered ? "#28C840" : "#606060"
                    Behavior on color { ColorAnimation { duration: 120 } }
                    Canvas {
                        id: tlMaxCanvas
                        anchors.centerIn: parent; width: 8; height: 8
                        visible: tlMaxArea.containsMouse
                        onVisibleChanged: if (visible) requestPaint()
                        property bool isFs: window.visibility === Window.FullScreen
                        onIsFsChanged: if (visible) requestPaint()
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.fillStyle = "#003a00"
                            if (isFs) {
                                // Two inward-pointing arrows → "exit fullscreen"
                                ctx.beginPath(); ctx.moveTo(0,3); ctx.lineTo(3,0); ctx.lineTo(3,3); ctx.closePath(); ctx.fill()
                                ctx.beginPath(); ctx.moveTo(8,5); ctx.lineTo(5,8); ctx.lineTo(5,5); ctx.closePath(); ctx.fill()
                                ctx.beginPath(); ctx.moveTo(5,0); ctx.lineTo(8,3); ctx.lineTo(5,3); ctx.closePath(); ctx.fill()
                                ctx.beginPath(); ctx.moveTo(3,8); ctx.lineTo(0,5); ctx.lineTo(3,5); ctx.closePath(); ctx.fill()
                            } else {
                                // Two outward-pointing arrows → "enter fullscreen"
                                ctx.beginPath(); ctx.moveTo(0,0); ctx.lineTo(3,0); ctx.lineTo(0,3); ctx.closePath(); ctx.fill()
                                ctx.beginPath(); ctx.moveTo(8,8); ctx.lineTo(5,8); ctx.lineTo(8,5); ctx.closePath(); ctx.fill()
                                ctx.beginPath(); ctx.moveTo(8,0); ctx.lineTo(8,3); ctx.lineTo(5,0); ctx.closePath(); ctx.fill()
                                ctx.beginPath(); ctx.moveTo(0,8); ctx.lineTo(0,5); ctx.lineTo(3,8); ctx.closePath(); ctx.fill()
                            }
                        }
                    }
                    MouseArea {
                        id: tlMaxArea; anchors.fill: parent; hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            if (window.visibility === Window.FullScreen)
                                window.showNormal()
                            else
                                window.showFullScreen()
                        }
                    }
                }
            }

            // ── macOS eject button (right side, only when something is loaded) ──
            Rectangle {
                id: macEjectBtn
                visible: Qt.platform.os === "osx" && (player.is_file_mode || player.total_tracks > 0)
                anchors.right: parent.right
                anchors.rightMargin: 10
                anchors.verticalCenter: parent.verticalCenter
                width: macEjectArea.containsMouse ? macEjectLabel.implicitWidth + 16 : 22
                height: 18; radius: 3
                color: macEjectArea.containsMouse ? clrSurf2 : "transparent"
                border.color: macEjectArea.containsMouse ? clrBorder : "transparent"
                border.width: 1
                Behavior on width { NumberAnimation { duration: 120; easing.type: Easing.OutCubic } }
                Behavior on color { ColorAnimation { duration: 100 } }
                clip: true

                Row {
                    anchors.centerIn: parent; spacing: 5

                    Canvas {
                        anchors.verticalCenter: parent.verticalCenter
                        width: 8; height: 8
                        property color ic: macEjectArea.containsMouse ? clrText : clrText2
                        onIcChanged: requestPaint()
                        Component.onCompleted: requestPaint()
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.fillStyle = ic
                            ctx.beginPath()
                            ctx.moveTo(4, 0); ctx.lineTo(8, 5); ctx.lineTo(0, 5)
                            ctx.closePath(); ctx.fill()
                            ctx.fillRect(0, 6.5, 8, 1.5)
                        }
                    }
                    Text {
                        id: macEjectLabel
                        anchors.verticalCenter: parent.verticalCenter
                        visible: macEjectArea.containsMouse
                        text: player.is_file_mode ? "Close" : "Eject"
                        color: clrText2; font.pixelSize: 10; font.family: "Segoe UI"
                    }
                }
                MouseArea {
                    id: macEjectArea; anchors.fill: parent; hoverEnabled: true
                    cursorShape: Qt.PointingHandCursor
                    onClicked: player.ejectOrClose()
                }
            }

            Text {
                id: titleText
                anchors.left: Qt.platform.os === "osx" ? trafficLights.right : parent.left
                anchors.right: Qt.platform.os === "osx" ? macEjectBtn.left : winButtons.left
                anchors.leftMargin: 8
                anchors.rightMargin: 8
                anchors.verticalCenter: parent.verticalCenter
                elide: Text.ElideRight
                text: {
                    if (!player.is_playing || player.total_tracks === 0) return ""
                    var num    = (player.current_track + 1).toString().padStart(2, "0")
                    var title  = (player.track_titles[player.current_track]  || "").trim()
                    var artist = (player.track_artists[player.current_track] || "").trim()
                    var sep1 = "  \u00B7  "
                    var sep2 = "  \u2014  "
                    return "\u25B6  " + num + sep1 + (artist.length > 0 ? artist + sep2 : "") + title
                }
                color: clrText2; font.pixelSize: 11; font.family: "Segoe UI"
            }

            // ── Windows-style controls (non-macOS) ──────────────────────────
            Row {
                id: winButtons
                visible: Qt.platform.os !== "osx"
                anchors.right: parent.right
                anchors.top: parent.top
                height: titleBar.height

                Rectangle {
                    width: 32; height: parent.height; color: "transparent"
                    visible: player.is_file_mode || player.total_tracks > 0
                    Rectangle {
                        anchors.fill: parent
                        color: ejectHov.containsMouse ? clrSurf2 : "transparent"
                        Behavior on color { ColorAnimation { duration: 100 } }
                    }
                    Canvas {
                        anchors.centerIn: parent; width: 8; height: 8
                        property color ic: ejectHov.containsMouse ? clrText : clrText2
                        onIcChanged: requestPaint()
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.fillStyle = ic
                            ctx.beginPath()
                            ctx.moveTo(4, 0); ctx.lineTo(8, 5); ctx.lineTo(0, 5)
                            ctx.closePath(); ctx.fill()
                            ctx.fillRect(0, 6, 8, 2)
                        }
                    }
                    MouseArea { id: ejectHov; anchors.fill: parent; hoverEnabled: true
                                cursorShape: Qt.PointingHandCursor
                                onClicked: player.ejectOrClose() }
                }

                Rectangle {
                    width: 1; height: parent.height
                    visible: player.is_file_mode || player.total_tracks > 0
                    color: clrBorder
                }

                Rectangle {
                    width: 32; height: parent.height; color: "transparent"
                    Rectangle {
                        anchors.fill: parent
                        color: minHov.containsMouse ? clrSurf2 : "transparent"
                        Behavior on color { ColorAnimation { duration: 100 } }
                    }
                    Canvas {
                        anchors.centerIn: parent; width: 8; height: 1
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.fillStyle = clrText2
                            ctx.fillRect(0, 0, width, height)
                        }
                    }
                    MouseArea { id: minHov; anchors.fill: parent; hoverEnabled: true
                                onClicked: window.showMinimized() }
                }
                Rectangle {
                    width: 32; height: parent.height; color: "transparent"
                    Rectangle {
                        anchors.fill: parent
                        color: clsHov.containsMouse ? "#3c1a1a" : "transparent"
                        Behavior on color { ColorAnimation { duration: 100 } }
                    }
                    Canvas {
                        anchors.centerIn: parent; width: 8; height: 8
                        property color ic: clsHov.containsMouse ? "#d07070" : clrText2
                        onIcChanged: requestPaint()
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.strokeStyle = ic
                            ctx.lineWidth = 1.5
                            ctx.lineCap = "round"
                            ctx.beginPath(); ctx.moveTo(0, 0); ctx.lineTo(8, 8); ctx.stroke()
                            ctx.beginPath(); ctx.moveTo(8, 0); ctx.lineTo(0, 8); ctx.stroke()
                        }
                    }
                    MouseArea { id: clsHov; anchors.fill: parent; hoverEnabled: true
                                onClicked: Qt.quit() }
                }
            }

            Rectangle { anchors.bottom: parent.bottom
                        anchors.left: parent.left; anchors.right: parent.right
                        height: 1; color: clrBorder }
        }

        // ── Content column ──────────────────────────────────────────────────
        ColumnLayout {
            anchors.top: titleBar.bottom
            anchors.left: parent.left
            anchors.right: parent.right
            anchors.bottom: parent.bottom
            spacing: 0

            // ── Main area: metadata sidebar + track list ──────────────────
            SplitView {
                Layout.fillWidth: true
                Layout.fillHeight: true
                orientation: Qt.Horizontal

                handle: Item {
                    implicitWidth: 5
                    implicitHeight: 1
                    Rectangle {
                        anchors.centerIn: parent
                        width: 1; height: parent.height
                        color: SplitHandle.pressed ? clrAccent
                             : SplitHandle.hovered  ? clrMuted
                             : clrBorder
                        Behavior on color { ColorAnimation { duration: 100 } }
                    }
                }

                // ─── Left metadata panel ──────────────────────────────────
                ColumnLayout {
                    id: sidebarColumn
                    SplitView.preferredWidth: 200
                    SplitView.minimumWidth: 140
                    SplitView.maximumWidth: 400
                    spacing: 0
                    clip: true

                    // ── Animation state ───────────────────────────────────
                    readonly property real _naturalCoverH:
                        (coverImg.status === Image.Ready && coverImg.implicitWidth > 0)
                        ? sidebarColumn.width * coverImg.implicitHeight / coverImg.implicitWidth
                        : sidebarColumn.width

                    property real _curtainH:   _naturalCoverH
                    property real _topSepH:    1
                    property real _slideX:     -200
                    property real _coverTarget: _naturalCoverH
                    // Prevents Binding from snapping _curtainH before toTopAnim's ScriptAction.
                    property bool _toTopPending: false

                    readonly property real _thumbW:
                        (coverImg.status === Image.Ready && coverImg.implicitHeight > 0)
                        ? Math.round(70 * coverImg.implicitWidth / coverImg.implicitHeight)
                        : 70
                    property real _thumbTarget: 70

                    Binding {
                        when: !window.coverOnSide && !toSideAnim.running && !toTopAnim.running
                              && !sidebarColumn._toTopPending
                        target: sidebarColumn
                        property: "_curtainH"
                        value: sidebarColumn._naturalCoverH
                    }

                    SequentialAnimation {
                        id: toSideAnim
                        ScriptAction { script: {
                            sidebarColumn._thumbTarget = sidebarColumn._thumbW
                            sidebarColumn._slideX = -(sidebarColumn._thumbTarget + 4)
                        } }
                        ParallelAnimation {
                            NumberAnimation { target: sidebarColumn; property: "_curtainH"
                                             to: 0; duration: 220; easing.type: Easing.OutCubic }
                            NumberAnimation { target: sidebarColumn; property: "_topSepH"
                                             to: 0; duration: 220;  easing.type: Easing.OutCubic }
                            NumberAnimation { target: sidebarColumn; property: "_slideX"
                                             to: 0; duration: 220; easing.type: Easing.OutCubic }
                        }
                    }

                    SequentialAnimation {
                        id: toTopAnim
                        ScriptAction { script: {
                            sidebarColumn._toTopPending = false
                            sidebarColumn._coverTarget = sidebarColumn._naturalCoverH
                            sidebarColumn._thumbTarget = sidebarColumn._thumbW
                            sidebarColumn._topSepH = 1
                            sidebarColumn._curtainH = 0
                        } }
                        ParallelAnimation {
                            NumberAnimation { target: sidebarColumn; property: "_slideX"
                                             to: -(sidebarColumn._thumbTarget + 4)
                                             duration: 220; easing.type: Easing.OutCubic }
                            NumberAnimation { target: sidebarColumn; property: "_curtainH"
                                             to: sidebarColumn._coverTarget
                                             duration: 220; easing.type: Easing.OutCubic }
                        }
                    }

                    Connections {
                        target: window
                        function onCoverOnSideChanged() {
                            if (window.coverOnSide) {
                                toTopAnim.stop(); toSideAnim.restart()
                            } else {
                                sidebarColumn._toTopPending = true
                                toSideAnim.stop(); toTopAnim.restart()
                            }
                        }
                    }

                    // ── Cover art (squishes away on switch) ───────────────
                    Item {
                        id: coverTopItem
                        Layout.fillWidth: true
                        Layout.preferredHeight: sidebarColumn._curtainH
                        clip: true

                        Rectangle {
                            id: coverRect
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.top: parent.top
                            height: sidebarColumn._naturalCoverH
                            color: clrSurf2
                            border.color: coverImg.status !== Image.Ready ? clrBorder : "transparent"
                            border.width: 1

                            Image {
                                id: coverImg
                                anchors.fill: parent
                                source: player.cover_art_path
                                fillMode: Image.Stretch
                                smooth: true; mipmap: true
                                visible: status === Image.Ready
                            }

                            Canvas {
                                anchors.centerIn: parent
                                width: 44; height: 44; opacity: 0.3
                                visible: coverImg.status !== Image.Ready
                                onPaint: {
                                    var ctx = getContext("2d")
                                    ctx.clearRect(0, 0, width, height)
                                    ctx.beginPath(); ctx.arc(22, 22, 20, 0, 2*Math.PI)
                                    ctx.strokeStyle = "#888"; ctx.lineWidth = 1.5; ctx.stroke()
                                    ctx.beginPath(); ctx.arc(22, 22, 13, 0, 2*Math.PI)
                                    ctx.strokeStyle = "#666"; ctx.lineWidth = 1.5; ctx.stroke()
                                    ctx.beginPath(); ctx.arc(22, 22, 4, 0, 2*Math.PI)
                                    ctx.fillStyle = "#666"; ctx.fill()
                                }
                            }

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: coverOnSide = true
                            }
                        }
                    }

                    Rectangle {
                        Layout.fillWidth: true
                        height: sidebarColumn._topSepH
                        color: clrBorder
                    }

                    Item {
                        id: metaBlock
                        Layout.fillWidth: true
                        Layout.preferredHeight: 70
                        clip: true

                        Rectangle {
                            id: thumbRect
                            x: sidebarColumn._slideX
                            y: 0
                            height: 70
                            width: sidebarColumn._thumbW
                            color: clrSurf2
                            border.color: coverImg.status !== Image.Ready ? clrBorder : "transparent"
                            border.width: 1
                            clip: true

                            Image {
                                anchors.fill: parent
                                source: player.cover_art_path
                                fillMode: Image.Stretch
                                smooth: true; mipmap: true
                                visible: coverImg.status === Image.Ready
                            }

                            Canvas {
                                anchors.centerIn: parent
                                width: 28; height: 28; opacity: 0.3
                                visible: coverImg.status !== Image.Ready
                                onPaint: {
                                    var ctx = getContext("2d")
                                    ctx.clearRect(0, 0, width, height)
                                    ctx.beginPath(); ctx.arc(14, 14, 12, 0, 2*Math.PI)
                                    ctx.strokeStyle = "#888"; ctx.lineWidth = 1.5; ctx.stroke()
                                    ctx.beginPath(); ctx.arc(14, 14, 8, 0, 2*Math.PI)
                                    ctx.strokeStyle = "#666"; ctx.lineWidth = 1.5; ctx.stroke()
                                    ctx.beginPath(); ctx.arc(14, 14, 3, 0, 2*Math.PI)
                                    ctx.fillStyle = "#666"; ctx.fill()
                                }
                            }

                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: coverOnSide = false
                            }
                        }

                        Column {
                            anchors.left: parent.left
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            anchors.leftMargin: Math.max(12, sidebarColumn._slideX + sidebarColumn._thumbW + 10)
                            anchors.rightMargin: 10
                            spacing: 3

                            ScrollText {
                                width: parent.width
                                text: player.album_title
                                textColor: clrText; pixelSize: 12; bold: true
                            }
                            ScrollText {
                                width: parent.width
                                text: player.album_artist
                                textColor: clrText2; pixelSize: 11
                            }
                            ScrollText {
                                width: parent.width
                                visible: player.album_year.length > 0
                                text: player.album_year
                                textColor: "#3a3a3a"; pixelSize: 10
                            }
                        }
                    }

                    Rectangle { Layout.fillWidth: true; height: 1; color: clrBorder }

                    Item {
                        id: lyricsArea
                        Layout.fillWidth: true
                        Layout.fillHeight: true
                        clip: true

                        property var timesArr: {
                            var arr = []
                            for (var i = 0; i < player.lyric_times.length; i++)
                                arr.push(parseFloat(player.lyric_times[i]))
                            return arr
                        }

                        property int activeIdx: {
                            var t = player.current_time
                            var times = timesArr
                            var best = -1
                            for (var i = 0; i < times.length; i++) {
                                if (times[i] <= t + 0.05) best = i
                                else break
                            }
                            return best
                        }

                        property bool userScrolled: false
                        property bool _autoScrolling: false

                        Timer {
                            id: resyncGuardTimer
                            interval: 800
                            repeat: false
                            onTriggered: lyricsArea._autoScrolling = false
                        }

                        function syncScroll(idx) {
                            if (idx < 0 || lyricsView.count === 0) return
                            lyricsArea._autoScrolling = true
                            lyricsView.positionViewAtIndex(idx, ListView.Center)
                            resyncGuardTimer.restart()
                        }

                        property var _lyricsWatch: player.lyric_lines
                        on_LyricsWatchChanged: {
                            lyricsArea.userScrolled = false
                            syncScroll(lyricsArea.activeIdx)
                        }

                        onActiveIdxChanged: {
                            if (!userScrolled) syncScroll(activeIdx)
                        }

                        ListView {
                            id: lyricsView
                            anchors.fill: parent
                            clip: true
                            model: player.lyric_lines
                            spacing: 0
                            flickDeceleration: 600
                            maximumFlickVelocity: 6000

                            Behavior on contentY {
                                NumberAnimation { duration: 700; easing.type: Easing.InOutQuart }
                            }

                            ScrollBar.vertical: ScrollBar { policy: ScrollBar.AlwaysOff }

                            onMovementStarted: {
                                if (!lyricsArea._autoScrolling) lyricsArea.userScrolled = true
                            }

                            header: Item { width: lyricsView.width; height: Math.max(0, lyricsView.height / 2 - 24) }
                            footer: Item { width: lyricsView.width; height: Math.max(0, lyricsView.height / 2) }

                            delegate: MouseArea {
                                id: lyricRow
                                width: lyricsView.width
                                height: lyricRowText.implicitHeight + 16
                                hoverEnabled: true
                                preventStealing: false

                                onClicked: {
                                    var times = lyricsArea.timesArr
                                    if (index < times.length) {
                                        player.seek(times[index])
                                        lyricsArea.userScrolled = false
                                        lyricsArea.syncScroll(index)
                                    }
                                }

                                Text {
                                    id: lyricRowText
                                    anchors.centerIn: parent
                                    width: parent.width - 24
                                    text: modelData
                                    color: {
                                        if (index === lyricsArea.activeIdx)
                                            return lyricRow.containsMouse ? clrAccent : clrText
                                        return lyricRow.containsMouse ? clrText : clrText2
                                    }
                                    font.pixelSize: index === lyricsArea.activeIdx ? 12 : 11
                                    font.bold: index === lyricsArea.activeIdx
                                    font.family: "Segoe UI"
                                    wrapMode: Text.WordWrap
                                    horizontalAlignment: Text.AlignHCenter
                                    Behavior on color      { ColorAnimation  { duration: 150 } }
                                    Behavior on font.pixelSize { NumberAnimation { duration: 150 } }
                                }
                            }
                        }

                        Rectangle {
                            anchors { left: parent.left; right: parent.right; top: parent.top }
                            height: 40; z: 1
                            gradient: Gradient {
                                GradientStop { position: 0.0; color: clrBg }
                                GradientStop { position: 1.0; color: "transparent" }
                            }
                        }
                        Rectangle {
                            anchors { left: parent.left; right: parent.right; bottom: parent.bottom }
                            height: 40; z: 1
                            gradient: Gradient {
                                GradientStop { position: 0.0; color: "transparent" }
                                GradientStop { position: 1.0; color: clrBg }
                            }
                        }

                        Rectangle {
                            visible: lyricsArea.userScrolled && player.lyric_lines.length > 0
                            anchors.bottom: parent.bottom
                            anchors.left: parent.left
                            anchors.margins: 8
                            width: resyncLabel.implicitWidth + 16; height: 20; radius: 3
                            color: clrSurf2
                            border.color: clrBorder; border.width: 1
                            z: 2
                            Text {
                                id: resyncLabel
                                anchors.centerIn: parent
                                text: "\u21A9 resync"
                                color: clrText2; font.pixelSize: 10; font.family: "Segoe UI"
                            }
                            MouseArea {
                                anchors.fill: parent
                                cursorShape: Qt.PointingHandCursor
                                onClicked: {
                                    lyricsArea.userScrolled = false
                                    lyricsArea.syncScroll(lyricsArea.activeIdx)
                                }
                            }
                        }
                    }
                }

                // ─── Track list panel ─────────────────────────────────────
                Item {
                    SplitView.fillWidth: true
                    SplitView.minimumWidth: 220

                Item {
                    anchors.fill: parent
                    visible: player.is_loading
                    z: 5

                    Shape {
                        anchors.centerIn: parent
                        width: 26; height: 26
                        RotationAnimator on rotation {
                            from: 0; to: 360; duration: 800
                            loops: Animation.Infinite; running: player.is_loading
                        }
                        ShapePath {
                            strokeColor: clrText2; strokeWidth: 2
                            fillColor: "transparent"; capStyle: ShapePath.RoundCap
                            PathAngleArc {
                                centerX: 13; centerY: 13
                                radiusX: 10; radiusY: 10
                                startAngle: -90; sweepAngle: 250
                            }
                        }
                    }

                    Text {
                        anchors.horizontalCenter: parent.horizontalCenter
                        anchors.top: parent.verticalCenter
                        anchors.topMargin: 24
                        text: "Reading disc"
                        color: clrText2; font.pixelSize: 12; font.family: "Segoe UI"
                    }
                }

                Item {
                    anchors.fill: parent
                    visible: !player.is_loading && player.total_tracks === 0

                    Column {
                        anchors.centerIn: parent; spacing: 12

                        Canvas {
                            width: 48; height: 48
                            anchors.horizontalCenter: parent.horizontalCenter
                            opacity: 0.45
                            onPaint: {
                                var ctx = getContext("2d")
                                ctx.clearRect(0, 0, width, height)
                                var cx = 24, cy = 24
                                ctx.beginPath(); ctx.arc(cx, cy, 21, 0, 2*Math.PI)
                                ctx.strokeStyle = "#555"; ctx.lineWidth = 1.5; ctx.stroke()
                                ctx.beginPath(); ctx.arc(cx, cy, 14, 0, 2*Math.PI)
                                ctx.strokeStyle = "#3a3a3a"; ctx.lineWidth = 1.5; ctx.stroke()
                                ctx.beginPath(); ctx.arc(cx, cy, 4, 0, 2*Math.PI)
                                ctx.fillStyle = "#3a3a3a"; ctx.fill()
                            }
                        }
                        Text {
                            anchors.horizontalCenter: parent.horizontalCenter
                            text: player.drive_status.length > 0
                                  ? player.drive_status : "No disc inserted"
                            color: clrText2; font.pixelSize: 13; font.family: "Segoe UI"
                        }

                        Row {
                            anchors.horizontalCenter: parent.horizontalCenter
                            spacing: 8

                            Rectangle {
                                width: openFilesLbl.implicitWidth + 20; height: 26
                                radius: 3
                                color: openFilesHov.containsMouse ? clrSurf2 : "transparent"
                                border.color: clrBorder; border.width: 1
                                Behavior on color { ColorAnimation { duration: 100 } }
                                Text {
                                    id: openFilesLbl
                                    anchors.centerIn: parent
                                    text: "Open Files"
                                    color: clrText2; font.pixelSize: 11; font.family: "Segoe UI"
                                }
                                MouseArea {
                                    id: openFilesHov; anchors.fill: parent
                                    hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                                    onClicked: player.openFilesDialog()
                                }
                            }

                            Rectangle {
                                width: openFolderLbl.implicitWidth + 20; height: 26
                                radius: 3
                                color: openFolderHov.containsMouse ? clrSurf2 : "transparent"
                                border.color: clrBorder; border.width: 1
                                Behavior on color { ColorAnimation { duration: 100 } }
                                Text {
                                    id: openFolderLbl
                                    anchors.centerIn: parent
                                    text: "Open Folder"
                                    color: clrText2; font.pixelSize: 11; font.family: "Segoe UI"
                                }
                                MouseArea {
                                    id: openFolderHov; anchors.fill: parent
                                    hoverEnabled: true; cursorShape: Qt.PointingHandCursor
                                    onClicked: player.openFolderDialog()
                                }
                            }
                        }
                    }
                }

                ListView {
                    id: trackList
                    anchors.fill: parent
                    visible: !player.is_loading && player.total_tracks > 0
                    clip: true; spacing: 0
                    boundsBehavior: Flickable.StopAtBounds
                    flickDeceleration: 600
                    maximumFlickVelocity: 6000
                    model: player.track_titles

                    add: Transition { NumberAnimation { property: "opacity"; from: 0; to: 1; duration: 180 } }

                    delegate: Rectangle {
                        readonly property bool isCurrent: index === player.current_track
                        width: trackList.width; height: 42
                        color: isCurrent           ? clrSurf2
                             : rowMs.containsMouse ? "#141414"
                             : "transparent"
                        Behavior on color { ColorAnimation { duration: 110 } }

                        Rectangle {
                            visible: isCurrent
                            width: 2; height: 18
                            anchors.left: parent.left
                            anchors.verticalCenter: parent.verticalCenter
                            color: clrAccent
                            Behavior on opacity { NumberAnimation { duration: 120 } }
                        }

                        Rectangle {
                            anchors.bottom: parent.bottom
                            anchors.left: parent.left; anchors.right: parent.right
                            height: 1; color: clrBorder; opacity: 0.5
                        }

                        RowLayout {
                            anchors.fill: parent
                            anchors.leftMargin: 14; anchors.rightMargin: 14
                            spacing: 10

                            Text {
                                Layout.preferredWidth: 22
                                text: (index + 1).toString().padStart(2, "0")
                                color: isCurrent ? clrAccent : "#3a3a3a"
                                font.pixelSize: 11; font.bold: true; font.family: "Segoe UI"
                                Behavior on color { ColorAnimation { duration: 110 } }
                            }
                            Column {
                                Layout.fillWidth: true
                                spacing: 1

                                ScrollText {
                                    width: parent.width
                                    text: modelData
                                    textColor: isCurrent ? clrText : clrText2
                                    pixelSize: 13
                                }
                                Text {
                                    width: parent.width
                                    visible: (player.track_artists[index] || "") !== ""
                                    text: player.track_artists[index] || ""
                                    color: isCurrent ? "#888" : "#3a3a3a"
                                    font.pixelSize: 10; font.family: "Segoe UI"
                                    elide: Text.ElideRight
                                }
                            }
                            Text {
                                text: player.track_names[index] || ""
                                color: isCurrent ? "#888" : "#3a3a3a"
                                font.pixelSize: 11; font.family: "Consolas, monospace"
                                Behavior on color { ColorAnimation { duration: 110 } }
                            }
                        }

                        MouseArea {
                            id: rowMs; anchors.fill: parent; hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: { player.loadTrack(index); player.playPause() }
                        }
                    }

                    ScrollBar.vertical: ScrollBar {
                        id: vScrollBar
                        policy: ScrollBar.AsNeeded

                        contentItem: Rectangle {
                            implicitWidth: 4
                            radius: 2
                            color: clrMuted
                            // Hide completely when all tracks fit without scrolling.
                            visible: vScrollBar.size < 1.0
                            opacity: vScrollBar.active ? 0.85 : 0.3
                            Behavior on opacity { NumberAnimation { duration: 200 } }
                        }
                        background: Rectangle { color: "transparent" }
                    }
                }
                }
            }

            // ── Seek section ──────────────────────────────────────────────
            Rectangle { Layout.fillWidth: true; height: 1; color: clrBorder }

            // Measures the widest possible time string once so both flanking
            // time labels stay a fixed, equal width — keeping the center label
            // truly centered regardless of which digits are on screen.
            TextMetrics {
                id: timeMetrics
                font.pixelSize: 11
                font.family: Qt.platform.os === "osx" ? "Menlo" : "Consolas"
                text: "00:00"
            }

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 14; Layout.rightMargin: 14
                Layout.topMargin: 7; Layout.bottomMargin: 4
                spacing: 0

                Text { text: formatTime(player.current_time)
                       color: clrText; font.pixelSize: 11
                       font.family: Qt.platform.os === "osx" ? "Menlo" : "Consolas"
                       Layout.preferredWidth: timeMetrics.advanceWidth
                       horizontalAlignment: Text.AlignLeft }
                Item { Layout.preferredWidth: 8 }
                Item {
                    Layout.fillWidth: true
                    height: 20

                    ScrollText {
                        anchors.fill: parent
                        visible: player.total_tracks > 0 && player.current_track >= 0 && !player.is_single_file
                        centered: true
                        text: {
                            if (player.total_tracks === 0 || player.current_track < 0) return ""
                            var num    = (player.current_track + 1).toString().padStart(2, "0")
                            var title  = (player.track_titles[player.current_track]  || "").trim()
                            var rawArt = (player.track_artists[player.current_track] || "").trim()
                            var artist = rawArt.length > 0 ? rawArt : (player.album_artist || "").trim()
                            return num + "  \u00B7  " + artist + "  \u2014  " + title
                        }
                        textColor: clrText2
                        pixelSize: 11
                        fontFamily: "Segoe UI"
                    }
                }
                Item { Layout.preferredWidth: 8 }
                Text { text: formatTime(player.total_time)
                       color: clrText2; font.pixelSize: 11
                       font.family: Qt.platform.os === "osx" ? "Menlo" : "Consolas"
                       Layout.preferredWidth: timeMetrics.advanceWidth
                       horizontalAlignment: Text.AlignRight }
            }

            Slider {
                id: seekSlider
                Layout.fillWidth: true
                Layout.leftMargin: 14; Layout.rightMargin: 14
                Layout.bottomMargin: 10
                implicitHeight: 20
                padding: 0
                from: 0; to: Math.max(player.total_time, 1)
                value: pressed ? value : player.current_time
                enabled: player.total_tracks > 0
                onPressedChanged: { if (!pressed) player.seek(value) }

                background: Item {
                    implicitHeight: 20
                    Rectangle {
                        anchors.verticalCenter: parent.verticalCenter
                        width: parent.width; height: 3; radius: 1
                        color: clrSurf2
                        Rectangle {
                            id: seekFill
                            width: parent.width * seekSlider.visualPosition
                            height: parent.height; radius: 1
                            color: clrAccent
                            Behavior on width {
                                enabled: !seekSlider.pressed
                                NumberAnimation { duration: 60; easing.type: Easing.OutSine }
                            }
                            Rectangle {
                                anchors.top: parent.top; anchors.left: parent.left
                                anchors.right: parent.right
                                height: 1; radius: 1
                                color: "#ffffff"; opacity: 0.08
                            }
                        }
                    }
                }
                handle: Rectangle {
                    x: seekSlider.visualPosition * seekSlider.availableWidth - width / 2
                    y: seekSlider.availableHeight / 2 - height / 2
                    width: 11; height: 11; radius: 5.5
                    color: seekSlider.pressed ? "#ffffff" : clrAccent
                    visible: player.total_tracks > 0
                    opacity: seekSlider.hovered || seekSlider.pressed ? 1.0 : 0.0
                    Behavior on opacity { NumberAnimation { duration: 130 } }
                    Behavior on color { ColorAnimation { duration: 80 } }
                    Rectangle {
                        anchors.fill: parent; anchors.margins: -1
                        radius: parent.radius + 1
                        color: "transparent"
                        border.color: "#ffffff"; border.width: 1; opacity: 0.08
                    }
                }
            }

            // ── Transport + volume ────────────────────────────────────────
            Rectangle { Layout.fillWidth: true; height: 1; color: clrBorder }

            RowLayout {
                Layout.fillWidth: true
                Layout.leftMargin: 14; Layout.rightMargin: 14
                Layout.topMargin: 10; Layout.bottomMargin: 12
                spacing: 2

                Item {
                    width: 30; height: 30
                    opacity: player.current_track > 0 ? 1.0 : 0.26
                    Behavior on opacity { NumberAnimation { duration: 160 } }

                    Canvas {
                        anchors.centerIn: parent; width: 13; height: 13
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.fillStyle = clrText
                            ctx.fillRect(0, 0, 2, height)
                            ctx.beginPath()
                            ctx.moveTo(12, 0); ctx.lineTo(2, 6.5); ctx.lineTo(12, 13)
                            ctx.closePath(); ctx.fill()
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        enabled: player.current_track > 0
                        cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                        onClicked: {
                            var wp = player.is_playing
                            player.previousTrack()
                            if (wp) player.playPause()
                        }
                    }
                }

                Rectangle {
                    id: ppBtn
                    width: 38; height: 38; radius: 4
                    color: ppMs.pressed       ? clrSurf2
                         : ppMs.containsMouse ? "#1c1c1c"
                         : clrSurface
                    opacity: player.total_tracks > 0 && player.current_track >= 0 ? 1.0 : 0.32
                    border.color: clrBorder; border.width: 1
                    Behavior on color { ColorAnimation { duration: 90 } }
                    Behavior on opacity { NumberAnimation { duration: 150 } }

                    Canvas {
                        id: ppCanvas
                        anchors.centerIn: parent; width: 13; height: 13
                        Connections {
                            target: player
                            function onIs_playingChanged() { ppCanvas.requestPaint() }
                        }
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.fillStyle = clrText
                            if (player.is_playing) {
                                ctx.fillRect(0, 0, 4, height)
                                ctx.fillRect(8, 0, 4, height)
                            } else {
                                ctx.beginPath()
                                ctx.moveTo(2, 0); ctx.lineTo(13, 6.5); ctx.lineTo(2, 13)
                                ctx.closePath(); ctx.fill()
                            }
                        }
                    }
                    MouseArea {
                        id: ppMs; anchors.fill: parent; hoverEnabled: true
                        enabled: player.total_tracks > 0 && player.current_track >= 0
                        cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                        onClicked: player.playPause()
                    }
                }

                Item {
                    width: 30; height: 30
                    opacity: player.current_track >= 0 && player.current_track < player.total_tracks - 1 ? 1.0 : 0.26
                    Behavior on opacity { NumberAnimation { duration: 160 } }

                    Canvas {
                        anchors.centerIn: parent; width: 13; height: 13
                        onPaint: {
                            var ctx = getContext("2d")
                            ctx.clearRect(0, 0, width, height)
                            ctx.fillStyle = clrText
                            ctx.beginPath()
                            ctx.moveTo(0, 0); ctx.lineTo(10, 6.5); ctx.lineTo(0, 13)
                            ctx.closePath(); ctx.fill()
                            ctx.fillRect(11, 0, 2, height)
                        }
                    }
                    MouseArea {
                        anchors.fill: parent
                        enabled: player.current_track >= 0 && player.current_track < player.total_tracks - 1
                        cursorShape: enabled ? Qt.PointingHandCursor : Qt.ArrowCursor
                        onClicked: {
                            var wp = player.is_playing
                            player.nextTrack()
                            if (wp) player.playPause()
                        }
                    }
                }

                Item { Layout.fillWidth: true }

                Canvas {
                    id: volIconCanvas
                    width: 16; height: 13
                    property real lvl: volSlider.value
                    onLvlChanged: requestPaint()
                    onPaint: {
                        var ctx = getContext("2d")
                        ctx.clearRect(0, 0, width, height)
                        ctx.fillStyle = clrText2
                        ctx.fillRect(0, 4, 4, 5)
                        ctx.beginPath()
                        ctx.moveTo(4, 4); ctx.lineTo(8, 1); ctx.lineTo(8, 12); ctx.lineTo(4, 9)
                        ctx.closePath(); ctx.fill()
                        ctx.strokeStyle = clrText2; ctx.lineWidth = 1.3
                        if (lvl > 0.05) {
                            ctx.beginPath(); ctx.arc(8, 6.5, 3.2, -0.7, 0.7); ctx.stroke()
                        }
                        if (lvl > 0.5) {
                            ctx.beginPath(); ctx.arc(8, 6.5, 5.5, -0.7, 0.7); ctx.stroke()
                        }
                    }
                }

                Slider {
                    id: volSlider
                    Layout.preferredWidth: 88
                    implicitHeight: 30
                    padding: 0
                    from: 0; to: 1; value: 1.0
                    Component.onCompleted: player.setVolumeLevel(1.0)
                    onMoved: player.setVolumeLevel(value)

                    background: Item {
                        implicitHeight: 30
                        Rectangle {
                            anchors.verticalCenter: parent.verticalCenter
                            width: parent.width; height: 3; radius: 1
                            color: clrSurf2
                            Rectangle {
                                width: parent.width * volSlider.value
                                height: parent.height; radius: 1
                                color: clrText2
                                Behavior on width { NumberAnimation { duration: 40; easing.type: Easing.OutSine } }
                                Rectangle {
                                    anchors.top: parent.top; anchors.left: parent.left
                                    anchors.right: parent.right
                                    height: 1; radius: 1
                                    color: "#ffffff"; opacity: 0.08
                                }
                            }
                        }
                    }
                    handle: Rectangle {
                        x: volSlider.visualPosition * volSlider.availableWidth - width / 2
                        y: volSlider.availableHeight / 2 - height / 2
                        width: 9; height: 9; radius: 4.5
                        color: volSlider.pressed ? "#ffffff" : clrText2
                        opacity: volSlider.hovered || volSlider.pressed ? 1.0 : 0.0
                        Behavior on opacity { NumberAnimation { duration: 130 } }
                        Behavior on color { ColorAnimation { duration: 80 } }
                    }
                }
            }
        }
    }

    Timer { id: smtcInitTimer; interval: 500; repeat: false; running: false
            onTriggered: player.initSmtc() }

    Component.onCompleted: {
        player.scanDrives()
        player.setVolumeLevel(0.8)
        smtcInitTimer.start()
    }

    function formatTime(s) {
        if (s < 0) s = 0
        var m = Math.floor(s / 60)
        var sec = Math.floor(s % 60)
        return (m < 10 ? "0" : "") + m + ":" + (sec < 10 ? "0" : "") + sec
    }
}
