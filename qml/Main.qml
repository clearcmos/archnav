import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window

import org.archnav.app

ApplicationWindow {
    id: root
    width: 1000
    height: 700
    visible: true
    title: "archnav"
    color: Style.bgPrimary

    // Hide to tray instead of quitting when window is closed
    onClosing: function(close) {
        close.accepted = false
        root.hide()
    }

    property bool previewVisible: false
    property bool helpVisible: false
    property int currentSortIndex: 0  // 0=MtimeDesc, 1=MtimeAsc, 2=NameAsc, 3=NameDesc, 4=SizeDesc, 5=SizeAsc, 6=PathAsc, 7=Frecency
    property bool frecencyMode: false  // When true, sort by frecency (most used first)

    // Column sort states: 0=none, 1=asc, 2=desc
    property int nameSortState: 0
    property int pathSortState: 0
    property int sizeSortState: 0
    property int dateSortState: 2  // Default: date descending (recent first)

    // Scaled column widths (user-adjustable via drag)
    property int nameColumnWidth: Math.round(200 * Style.zoomFactor)
    property int pathColumnWidth: Math.round(400 * Style.zoomFactor)
    property int sizeColumnWidth: Math.round(80 * Style.zoomFactor)
    property int dateColumnWidth: Math.round(150 * Style.zoomFactor)

    // Rust bridge objects
    SearchEngine {
        id: engine
        onResultsReady: updateResults()
        onEngine_readyChanged: {
            if (engine_ready && searchBar.text.length > 0)
                engine.search(searchBar.text, currentSortIndex)
        }
        onRescanComplete: {
            if (searchBar.text.length > 0)
                engine.search(searchBar.text, currentSortIndex)
        }
        onToggleRequested: {
            if (root.visible) {
                root.hide()
            } else {
                root.show()
                root.raise()
                root.requestActivate()
                searchBar.forceActiveFocus()
            }
        }
    }

    PreviewBridge {
        id: preview
    }

    // Results data
    ListModel {
        id: resultsModel
    }

    function updateResults() {
        resultsModel.clear()
        for (var i = 0; i < engine.result_count; i++) {
            resultsModel.append({
                path: engine.result_path_at(i),
                isDir: engine.result_is_dir_at(i),
                filename: engine.result_filename_at(i),
                mtime: engine.result_mtime_at(i),
                fileSize: engine.result_size_at(i)
            })
        }
        resultsList.currentIndex = resultsModel.count > 0 ? 0 : -1
    }

    function openSelected() {
        if (resultsList.currentIndex < 0) return
        var item = resultsModel.get(resultsList.currentIndex)
        if (!item) return
        engine.open_file(item.path)
    }

    function openContainingFolder() {
        if (resultsList.currentIndex < 0) return
        var item = resultsModel.get(resultsList.currentIndex)
        if (!item) return
        engine.open_folder(item.path)
    }

    function updatePreview() {
        if (!previewVisible) return
        if (resultsList.currentIndex < 0) {
            preview.clear_preview()
            return
        }
        var item = resultsModel.get(resultsList.currentIndex)
        if (item) {
            preview.request_preview(item.path, item.isDir, previewPanel.contentWidth)
        }
    }

    function setSortOrder(sortIndex) {
        currentSortIndex = sortIndex
        if (searchBar.text.length > 0)
            engine.search(searchBar.text, currentSortIndex)
    }

    function sortByName() {
        pathSortState = 0
        sizeSortState = 0
        dateSortState = 0
        frecencyMode = false
        if (nameSortState === 0 || nameSortState === 2) {
            nameSortState = 1
            setSortOrder(2) // NameAsc
        } else {
            nameSortState = 2
            setSortOrder(3) // NameDesc
        }
    }

    function sortByPath() {
        nameSortState = 0
        sizeSortState = 0
        dateSortState = 0
        frecencyMode = false
        pathSortState = 1
        setSortOrder(6) // PathAsc
    }

    function sortBySize() {
        nameSortState = 0
        pathSortState = 0
        dateSortState = 0
        frecencyMode = false
        if (sizeSortState === 0 || sizeSortState === 1) {
            sizeSortState = 2
            setSortOrder(4) // SizeDesc (largest first)
        } else {
            sizeSortState = 1
            setSortOrder(5) // SizeAsc
        }
    }

    function sortByDate() {
        nameSortState = 0
        pathSortState = 0
        sizeSortState = 0
        frecencyMode = false
        if (dateSortState === 0 || dateSortState === 1) {
            dateSortState = 2
            setSortOrder(0) // MtimeDesc (recent first)
        } else {
            dateSortState = 1
            setSortOrder(1) // MtimeAsc
        }
    }

    function toggleFrecency() {
        frecencyMode = !frecencyMode
        if (frecencyMode) {
            // Clear column sort indicators
            nameSortState = 0
            pathSortState = 0
            sizeSortState = 0
            dateSortState = 0
            setSortOrder(7) // Frecency
        } else {
            // Revert to default (recent first)
            dateSortState = 2
            setSortOrder(0) // MtimeDesc
        }
    }

    // Search debounce timer
    Timer {
        id: searchDebounce
        interval: Style.searchDebounceMs
        onTriggered: {
            if (searchBar.text.length > 0) {
                engine.search(searchBar.text, currentSortIndex)
            } else {
                resultsModel.clear()
                resultsList.currentIndex = -1
                preview.clear_preview()
            }
        }
    }

    // Global mouse area for Ctrl+wheel zoom
    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.NoButton
        propagateComposedEvents: true
        onWheel: function(wheel) {
            if (wheel.modifiers & Qt.ControlModifier) {
                if (wheel.angleDelta.y > 0) {
                    Style.zoomIn()
                } else if (wheel.angleDelta.y < 0) {
                    Style.zoomOut()
                }
                wheel.accepted = true
            } else {
                wheel.accepted = false
            }
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: 0

        // Search bar area
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.round(48 * Style.zoomFactor)
            color: Style.bgPrimary

            RowLayout {
                anchors.fill: parent
                anchors.margins: Style.marginNormal
                spacing: Style.marginNormal

                SearchBar {
                    id: searchBar
                    Layout.fillWidth: true
                    onTextChanged: searchDebounce.restart()

                    Keys.onDownPressed: resultsList.moveDown()
                    Keys.onUpPressed: resultsList.moveUp()
                    Keys.onReturnPressed: openSelected()
                    Keys.onEnterPressed: openSelected()
                }
            }
        }

        // Header row
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: Style.headerHeight
            color: Style.headerBg

            Rectangle {
                anchors.bottom: parent.bottom
                width: parent.width
                height: 1
                color: Style.headerBorder
            }

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Style.marginNormal
                anchors.rightMargin: Style.marginNormal
                spacing: 0

                // Name header
                HeaderButton {
                    id: nameHeader
                    Layout.preferredWidth: nameColumnWidth
                    text: "Name"
                    sortState: nameSortState
                    resizable: true
                    minWidth: 80
                    onClicked: sortByName()
                    onWidthChangeRequested: function(newWidth) {
                        console.log("[HEADER] Name width change:", nameColumnWidth, "->", newWidth)
                        nameColumnWidth = newWidth
                    }
                }

                // Path header
                HeaderButton {
                    id: pathHeader
                    Layout.preferredWidth: pathColumnWidth
                    text: "Path"
                    sortState: pathSortState
                    resizable: true
                    minWidth: 100
                    onClicked: sortByPath()
                    onWidthChangeRequested: function(newWidth) {
                        console.log("[HEADER] Path width change:", pathColumnWidth, "->", newWidth)
                        pathColumnWidth = newWidth
                    }
                }

                // Size header
                HeaderButton {
                    id: sizeHeader
                    Layout.preferredWidth: sizeColumnWidth
                    text: "Size"
                    sortState: sizeSortState
                    alignment: Text.AlignRight
                    resizable: true
                    minWidth: 60
                    onClicked: sortBySize()
                    onWidthChangeRequested: function(newWidth) {
                        console.log("[HEADER] Size width change:", sizeColumnWidth, "->", newWidth)
                        sizeColumnWidth = newWidth
                    }
                }

                // Date Modified header (last column - not resizable)
                HeaderButton {
                    id: dateHeader
                    Layout.preferredWidth: dateColumnWidth
                    text: "Modified"
                    sortState: dateSortState
                    alignment: Text.AlignRight
                    resizable: false
                    onClicked: sortByDate()
                }

                // Flexible spacer to fill remaining space after all columns
                Item {
                    Layout.fillWidth: true
                }
            }
        }

        // Content area: results + preview
        SplitView {
            id: splitView
            Layout.fillWidth: true
            Layout.fillHeight: true
            orientation: Qt.Horizontal

            ResultsList {
                id: resultsList
                SplitView.preferredWidth: previewVisible ? parent.width * 0.55 : parent.width
                SplitView.minimumWidth: 400
                model: resultsModel
                nameColumnWidth: root.nameColumnWidth
                pathColumnWidth: root.pathColumnWidth
                sizeColumnWidth: root.sizeColumnWidth
                dateColumnWidth: root.dateColumnWidth

                onCurrentIndexChanged: updatePreview()
                onItemDoubleClicked: openSelected()
                onContextMenuRequested: function(path, globalX, globalY) {
                    engine.show_context_menu(path, globalX, globalY)
                }
            }

            PreviewPanel {
                id: previewPanel
                SplitView.preferredWidth: parent.width * 0.45
                SplitView.minimumWidth: 200
                visible: previewVisible
                previewBridge: preview
            }
        }

        // Status bar
        Rectangle {
            Layout.fillWidth: true
            Layout.preferredHeight: Math.round(26 * Style.zoomFactor)
            color: Style.bgPrimary

            Rectangle {
                anchors.top: parent.top
                width: parent.width
                height: 1
                color: Style.headerBorder
            }

            RowLayout {
                anchors.fill: parent
                anchors.leftMargin: Style.marginNormal
                anchors.rightMargin: Style.marginNormal

                Label {
                    text: engine.result_count + " items"
                    color: Style.textDim
                    font.pixelSize: Style.fontSizeSmall
                }

                Label {
                    text: "Frecency"
                    color: Style.accentBlue
                    font.pixelSize: Style.fontSizeSmall
                    font.bold: true
                    visible: frecencyMode
                }

                Item { Layout.fillWidth: true }

                Label {
                    text: engine.status_text
                    color: Style.textDim
                    font.pixelSize: Style.fontSizeSmall
                }

                Item { width: Style.marginLarge }

                Label {
                    text: Math.round(Style.zoomFactor * 100) + "%"
                    color: Style.textDim
                    font.pixelSize: Style.fontSizeSmall
                    visible: Style.zoomFactor !== 1.0
                }
            }
        }
    }

    // Keyboard shortcuts
    Shortcut {
        sequence: "Ctrl+P"
        onActivated: {
            previewVisible = !previewVisible
            if (previewVisible) {
                updatePreview()
            }
        }
    }
    Shortcut {
        sequence: "Ctrl+R"
        onActivated: engine.rescan_all()
    }
    Shortcut {
        sequence: "Ctrl+O"
        onActivated: openContainingFolder()
    }
    Shortcut {
        sequence: "Escape"
        onActivated: root.close()
    }

    // Frecency toggle
    Shortcut {
        sequence: "Ctrl+Shift+F"
        onActivated: toggleFrecency()
    }

    // Help overlay
    Shortcut {
        sequence: "F1"
        onActivated: helpVisible = !helpVisible
    }

    // Zoom controls
    Shortcut {
        sequence: "Ctrl+="
        onActivated: Style.zoomIn()
    }
    Shortcut {
        sequence: "Ctrl++"
        onActivated: Style.zoomIn()
    }
    Shortcut {
        sequence: "Ctrl+-"
        onActivated: Style.zoomOut()
    }
    Shortcut {
        sequence: "Ctrl+0"
        onActivated: Style.resetZoom()
    }

    Component.onCompleted: {
        searchBar.forceActiveFocus()
        engine.initialize()
    }

    // Help overlay - press F1 to toggle, any key to dismiss
    Rectangle {
        id: helpOverlay
        anchors.fill: parent
        color: Qt.rgba(0, 0, 0, 0.6)
        visible: helpVisible
        z: 100

        MouseArea {
            anchors.fill: parent
            onClicked: helpVisible = false
        }

        // Catch any key to dismiss
        Keys.onPressed: function(event) {
            if (event.key !== Qt.Key_F1) {
                helpVisible = false
                event.accepted = true
            }
        }

        focus: helpVisible

        Rectangle {
            anchors.centerIn: parent
            width: Math.round(520 * Style.zoomFactor)
            height: helpColumn.height + Math.round(48 * Style.zoomFactor)
            color: Style.bgPrimary
            border.color: Style.borderDefault
            border.width: 1
            radius: Math.round(8 * Style.zoomFactor)

            ColumnLayout {
                id: helpColumn
                anchors.top: parent.top
                anchors.left: parent.left
                anchors.right: parent.right
                anchors.margins: Math.round(24 * Style.zoomFactor)
                spacing: Math.round(16 * Style.zoomFactor)

                Label {
                    text: "Keyboard Shortcuts"
                    color: Style.textPrimary
                    font.pixelSize: Style.fontSizeLarge
                    font.bold: true
                    Layout.alignment: Qt.AlignHCenter
                }

                GridLayout {
                    columns: 2
                    columnSpacing: Math.round(24 * Style.zoomFactor)
                    rowSpacing: Math.round(6 * Style.zoomFactor)
                    Layout.fillWidth: true

                    // Navigation
                    Label { text: "Up / Down"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Navigate results"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    Label { text: "Enter"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Open file or folder"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    Label { text: "Ctrl+O"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Open containing folder"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    Label { text: "Right-click"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Context menu"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    // Separator
                    Item { Layout.columnSpan: 2; Layout.preferredHeight: Math.round(4 * Style.zoomFactor) }

                    // View
                    Label { text: "Ctrl+P"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Toggle preview pane"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    Label { text: "Ctrl+Shift+F"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Toggle frecency sort"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    Label { text: "Ctrl+R"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Rescan all bookmarks"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    // Separator
                    Item { Layout.columnSpan: 2; Layout.preferredHeight: Math.round(4 * Style.zoomFactor) }

                    // Zoom
                    Label { text: "Ctrl+= / Ctrl+-"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Zoom in / out"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    Label { text: "Ctrl+0"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Reset zoom"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    Label { text: "Ctrl+Scroll"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Zoom in / out"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    // Separator
                    Item { Layout.columnSpan: 2; Layout.preferredHeight: Math.round(4 * Style.zoomFactor) }

                    // Window
                    Label { text: "Esc"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Hide to tray"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }

                    Label { text: "F1"; color: Style.accentBlue; font.pixelSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                    Label { text: "Toggle this help"; color: Style.textPrimary; font.pixelSize: Style.fontSizeNormal }
                }

                Label {
                    text: "Press any key to close"
                    color: Style.textDim
                    font.pixelSize: Style.fontSizeSmall
                    Layout.alignment: Qt.AlignHCenter
                }
            }
        }
    }

    // HeaderButton component for clickable column headers
    component HeaderButton: Item {
        id: headerItem
        property string text: ""
        property int sortState: 0  // 0=none, 1=asc, 2=desc
        property int alignment: Text.AlignLeft
        property bool resizable: false
        property int minWidth: 60

        signal clicked()
        signal widthChangeRequested(int newWidth)

        implicitHeight: parent.height
        clip: false  // Allow resize handle to extend beyond bounds

        MouseArea {
            anchors.fill: parent
            anchors.rightMargin: resizable ? 8 : 0  // Leave space for resize handle
            hoverEnabled: true
            cursorShape: Qt.PointingHandCursor
            onClicked: headerItem.clicked()
            z: 1

            Rectangle {
                anchors.fill: parent
                color: parent.containsMouse ? Qt.rgba(1, 1, 1, 0.08) : "transparent"
            }
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: Style.marginSmall
            anchors.rightMargin: Style.marginSmall
            spacing: Style.marginSmall

            Item {
                Layout.fillWidth: alignment !== Text.AlignLeft
            }

            Label {
                text: headerItem.text
                color: Style.headerText
                font.pixelSize: Style.fontSizeNormal
                font.bold: sortState !== 0
            }

            Label {
                text: sortState === 1 ? "\u25B2" : (sortState === 2 ? "\u25BC" : "")
                color: Style.accentBlue
                font.pixelSize: Math.round(10 * Style.zoomFactor)
                visible: sortState !== 0
            }

            Item {
                Layout.fillWidth: alignment === Text.AlignLeft
            }
        }

        // Right border (separator line)
        Rectangle {
            id: separatorLine
            anchors.right: parent.right
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            width: 1
            color: Style.headerBorder
            z: 2
        }

        // Resize handle (draggable area centered on separator)
        MouseArea {
            id: resizeHandle
            visible: resizable
            anchors.horizontalCenter: separatorLine.horizontalCenter
            anchors.top: parent.top
            anchors.bottom: parent.bottom
            width: 10
            cursorShape: Qt.SplitHCursor
            hoverEnabled: true
            z: 3  // Above everything else

            property real startGlobalX: 0
            property int startWidth: 0

            onPressed: function(mouse) {
                var global = mapToGlobal(mouse.x, mouse.y)
                startGlobalX = global.x
                startWidth = headerItem.width
            }

            onPositionChanged: function(mouse) {
                if (pressed) {
                    var global = mapToGlobal(mouse.x, mouse.y)
                    var totalDelta = global.x - startGlobalX
                    var newWidth = Math.max(minWidth, startWidth + totalDelta)
                    headerItem.widthChangeRequested(newWidth)
                }
            }

            // Visual feedback on hover - centered highlight
            Rectangle {
                anchors.centerIn: parent
                width: 5
                height: parent.height
                color: parent.containsMouse || parent.pressed ? Qt.rgba(1, 1, 1, 0.25) : "transparent"
                radius: 2
            }
        }
    }
}
