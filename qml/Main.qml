import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Window
import org.kde.kirigami as Kirigami

import org.archnav.app

ApplicationWindow {
    id: root
    width: 1000
    height: 700
    // Shown explicitly in Component.onCompleted unless launched with --hidden,
    // so the autostart instance can preload the index and sit in the tray.
    visible: false
    title: "archnav"
    color: Style.bgPrimary

    // Hide to tray instead of quitting when window is closed
    onClosing: function(close) {
        close.accepted = false
        saveLayoutNow()
        root.hide()
    }

    // The window's size persists to config.json once a resize settles, so
    // it survives a restart (Wayland leaves the position to KWin)
    onWidthChanged: windowSaveTimer.restart()
    onHeightChanged: windowSaveTimer.restart()
    Timer {
        id: windowSaveTimer
        interval: Kirigami.Units.humanMoment
        onTriggered: {
            if (root.visible && root.visibility === Window.Windowed)
                engine.save_window_size(root.width, root.height)
        }
    }

    // Keep the tray menu text ("Show archnav" / "Hide archnav") in sync
    onVisibleChanged: engine.set_window_visible(root.visible)

    property bool previewVisible: false
    // True from a keystroke until its results land, so the empty-list
    // placeholder does not flash "No matching files" while a search runs
    property bool searching: false
    property int currentSortIndex: 0  // 0=MtimeDesc, 1=MtimeAsc, 2=NameAsc, 3=NameDesc, 4=SizeDesc, 5=SizeAsc, 6=PathAsc, 7=Frecency
    property bool frecencyMode: false  // When true, sort by frecency (most used first)

    // Column sort states: 0=none, 1=asc, 2=desc
    property int nameSortState: 0
    property int pathSortState: 0
    property int sizeSortState: 0
    property int dateSortState: 2  // Default: date descending (recent first)

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

    BookmarkDialog {
        id: bookmarkDialog
        searchEngine: engine
    }

    // The engine is the results model; it has reloaded when this runs
    function updateResults() {
        searching = false
        resultsList.currentIndex = engine.result_count > 0 ? 0 : -1
        resultsList.anchorRow = 0
        // currentIndex may not change value across searches (0 -> 0), so the
        // onCurrentIndexChanged hook alone would leave a stale preview.
        updatePreview()
    }

    function openSelected() {
        if (resultsList.currentIndex < 0) return
        engine.open_file(engine.result_path_at(resultsList.currentIndex))
    }

    function openContainingFolder() {
        if (resultsList.currentIndex < 0) return
        engine.open_folder(engine.result_path_at(resultsList.currentIndex))
    }

    function updatePreview() {
        if (!previewVisible) return
        if (resultsList.currentIndex < 0) {
            preview.clear_preview()
            return
        }
        var i = resultsList.currentIndex
        preview.request_preview(engine.result_path_at(i), engine.result_is_dir_at(i),
                                previewPanel.contentWidth)
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

    function sortByColumn(key) {
        if (key === "name") sortByName()
        else if (key === "path") sortByPath()
        else if (key === "size") sortBySize()
        else if (key === "date") sortByDate()
    }

    // Column widths and visibility persist to config.json, debounced so a
    // drag writes once when it ends rather than on every pixel
    Timer {
        id: layoutSaveTimer
        interval: Kirigami.Units.humanMoment
        onTriggered: engine.save_column_layout(resultsList.layoutJson())
    }

    function saveLayoutNow() {
        if (layoutSaveTimer.running) {
            layoutSaveTimer.stop()
            engine.save_column_layout(resultsList.layoutJson())
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
                searching = false
                engine.clear_results()
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

        SearchBar {
            id: searchBar
            Layout.fillWidth: true
            Layout.margins: Style.marginNormal
            onTextChanged: {
                searching = text.length > 0
                searchDebounce.restart()
            }

            Keys.onReturnPressed: openSelected()
            Keys.onEnterPressed: openSelected()
            Keys.onPressed: function(event) {
                event.accepted = resultsList.handleKey(event, searchBar)
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
                SplitView.minimumWidth: Kirigami.Units.gridUnit * 22
                model: engine
                selector: engine
                selectionRevision: engine.selection_revision
                nameSortState: root.nameSortState
                pathSortState: root.pathSortState
                sizeSortState: root.sizeSortState
                dateSortState: root.dateSortState

                placeholderIcon: !engine.engine_ready || searching ? ""
                    : searchBar.text.length === 0 ? "search" : "edit-none"
                placeholderText: !engine.engine_ready ? "Loading index…"
                    : searching ? ""
                    : searchBar.text.length === 0 ? "Type to search" : "No matching files"
                placeholderExplanation: engine.engine_ready && !searching && searchBar.text.length === 0
                    ? Number(engine.total_indexed).toLocaleString(Qt.locale(), "f", 0) + " files indexed"
                    : ""

                onCurrentIndexChanged: updatePreview()
                onItemDoubleClicked: openSelected()
                onContextMenuRequested: function(paths, globalX, globalY) {
                    engine.show_context_menu(paths, globalX, globalY)
                }
                onTypedToSearch: function(text, erase) {
                    searchBar.forceActiveFocus()
                    searchBar.cursorPosition = searchBar.text.length
                    if (erase)
                        searchBar.remove(searchBar.text.length - 1, searchBar.text.length)
                    else
                        searchBar.insert(searchBar.text.length, text)
                }
                onCopyRequested: function(paths) {
                    engine.copy_files(paths)
                }
                onCopyLocationRequested: function(paths) {
                    engine.copy_locations(paths)
                }
                onDragRequested: function(paths) {
                    engine.start_drag(paths)
                }
                onOpenFolderRequested: function(path) {
                    engine.open_folder(path)
                }
                onHeaderClicked: function(key) {
                    sortByColumn(key)
                }
                onLayoutEdited: layoutSaveTimer.restart()
            }

            PreviewPanel {
                id: previewPanel
                SplitView.preferredWidth: parent.width * 0.45
                SplitView.minimumWidth: Kirigami.Units.gridUnit * 11
                visible: previewVisible
                previewBridge: preview
            }
        }

        // Status bar
        Kirigami.Separator {
            Layout.fillWidth: true
        }

        RowLayout {
            Layout.fillWidth: true
            Layout.leftMargin: Style.marginNormal
            Layout.rightMargin: Style.marginNormal
            Layout.topMargin: Style.marginSmall
            Layout.bottomMargin: Style.marginSmall
            spacing: Style.marginLarge

            Label {
                text: "Frecency"
                color: Style.accentBlue
                font.pointSize: Style.fontSizeSmall
                font.bold: true
                visible: frecencyMode
            }

            Item { Layout.fillWidth: true }

            Label {
                text: engine.status_text
                opacity: 0.75
                font.pointSize: Style.fontSizeSmall
            }

            Label {
                text: Math.round(Style.zoomFactor * 100) + "%"
                opacity: 0.75
                font.pointSize: Style.fontSizeSmall
                visible: Style.zoomFactor !== 1.0
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
        sequence: "Ctrl+B"
        onActivated: bookmarkDialog.open()
    }
    Shortcut {
        sequence: "Ctrl+O"
        onActivated: openContainingFolder()
    }
    // Back to the search field from the list, its text selected
    Shortcut {
        sequences: ["Ctrl+L", "Ctrl+F"]
        onActivated: {
            searchBar.forceActiveFocus()
            searchBar.selectAll()
        }
    }
    // The selected result's menu. A shortcut takes the key before the search
    // field sees it, which would otherwise open its own edit menu as well.
    Shortcut {
        sequences: ["Menu", "Shift+F10"]
        onActivated: resultsList.openContextMenu()
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

    // Sort by a column, as a click on its header does (again to reverse)
    Shortcut {
        sequence: "Ctrl+1"
        onActivated: sortByColumn("name")
    }
    Shortcut {
        sequence: "Ctrl+2"
        onActivated: sortByColumn("path")
    }
    Shortcut {
        sequence: "Ctrl+3"
        onActivated: sortByColumn("size")
    }
    Shortcut {
        sequence: "Ctrl+4"
        onActivated: sortByColumn("date")
    }

    // Help overlay
    Shortcut {
        sequence: "F1"
        onActivated: helpPopup.visible ? helpPopup.close() : helpPopup.open()
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
        resultsList.applyLayout(engine.column_layout())
        try {
            var size = JSON.parse(engine.window_size() || "{}")
            if (size.width > 0 && size.height > 0) {
                root.width = size.width
                root.height = size.height
            }
        } catch (e) {
        }
        engine.initialize()
        // In --hidden mode (login autostart), stay in the tray: the index still
        // loads in the background and the toggle socket still comes up, but no
        // window appears until the user toggles it.
        if (!engine.start_hidden()) {
            root.show()
            searchBar.forceActiveFocus()
        }
    }

    // Keyboard shortcuts (F1); any key closes it
    Popup {
        id: helpPopup
        parent: Overlay.overlay
        anchors.centerIn: parent
        modal: true
        focus: true
        padding: Kirigami.Units.gridUnit
        closePolicy: Popup.CloseOnEscape | Popup.CloseOnPressOutside

        contentItem: ColumnLayout {
            focus: true
            spacing: Style.marginLarge

            Keys.onPressed: function(event) {
                if (event.key !== Qt.Key_F1) {
                    helpPopup.close()
                    event.accepted = true
                }
            }

            Label {
                text: "Keyboard Shortcuts"
                font.pointSize: Style.fontSizeLarge
                font.bold: true
                Layout.alignment: Qt.AlignHCenter
            }

            GridLayout {
                columns: 2
                columnSpacing: Math.round(Kirigami.Units.gridUnit * Style.zoomFactor)
                rowSpacing: Style.marginSmall
                Layout.fillWidth: true

                // Navigation
                Label { text: "Up / Down"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Navigate results"; font.pointSize: Style.fontSizeNormal }
                Label { text: "Shift+Up / Down"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Extend the selection"; font.pointSize: Style.fontSizeNormal }
                Label { text: "Ctrl+Click"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Add or remove a result"; font.pointSize: Style.fontSizeNormal }
                Label { text: "Ctrl+A"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Select all results (after clicking one)"; font.pointSize: Style.fontSizeNormal }
                Label { text: "Ctrl+L / Ctrl+F"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Back to the search field"; font.pointSize: Style.fontSizeNormal }

                Label { text: "PgUp / PgDn"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Move a page of results"; font.pointSize: Style.fontSizeNormal }

                Label { text: "Home / End"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "First / last result"; font.pointSize: Style.fontSizeNormal }

                Label { text: "Enter"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Open file or folder"; font.pointSize: Style.fontSizeNormal }
                Label { text: "Double-click path"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Open the containing folder"; font.pointSize: Style.fontSizeNormal }

                Label { text: "Ctrl+O"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Open containing folder"; font.pointSize: Style.fontSizeNormal }

                Label { text: "Menu / Shift+F10"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Context menu"; font.pointSize: Style.fontSizeNormal }
                Label { text: "Ctrl+C"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Copy the selected files"; font.pointSize: Style.fontSizeNormal }
                Label { text: "Ctrl+Alt+C"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Copy their locations"; font.pointSize: Style.fontSizeNormal }
                Label { text: "Drag"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Move or copy files to another app"; font.pointSize: Style.fontSizeNormal }

                // Separator
                Item { Layout.columnSpan: 2; Layout.preferredHeight: Style.marginSmall }

                // Search
                Label { text: "a b"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "All words, in any order (\"a b\" for a phrase)"; font.pointSize: Style.fontSizeNormal }

                Label { text: "!word"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Leave out paths containing a word"; font.pointSize: Style.fontSizeNormal }

                Label { text: "size:>1G"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Size: >, <, =, or 100M..1G; k, m, g, t"; font.pointSize: Style.fontSizeNormal }

                Label { text: "dm:thisweek"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Modified: today, last7days, 2026-10, >2025"; font.pointSize: Style.fontSizeNormal }

                // Separator
                Item { Layout.columnSpan: 2; Layout.preferredHeight: Style.marginSmall }

                // View
                Label { text: "Ctrl+P"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Toggle preview pane"; font.pointSize: Style.fontSizeNormal }

                Label { text: "Ctrl+1 to 4"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Sort by Name, Path, Size, Modified"; font.pointSize: Style.fontSizeNormal }
                Label { text: "Ctrl+Shift+F"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Toggle frecency sort"; font.pointSize: Style.fontSizeNormal }

                Label { text: "Ctrl+R"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Rescan all bookmarks"; font.pointSize: Style.fontSizeNormal }

                Label { text: "Ctrl+B"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Manage bookmarks"; font.pointSize: Style.fontSizeNormal }

                // Separator
                Item { Layout.columnSpan: 2; Layout.preferredHeight: Style.marginSmall }

                // Zoom
                Label { text: "Ctrl+= / Ctrl+-"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Zoom in / out"; font.pointSize: Style.fontSizeNormal }

                Label { text: "Ctrl+0"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Reset zoom"; font.pointSize: Style.fontSizeNormal }

                Label { text: "Ctrl+Scroll"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Zoom in / out"; font.pointSize: Style.fontSizeNormal }

                // Separator
                Item { Layout.columnSpan: 2; Layout.preferredHeight: Style.marginSmall }

                // Window
                Label { text: "Esc"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Hide to tray"; font.pointSize: Style.fontSizeNormal }

                Label { text: "F1"; color: Style.accentBlue; font.pointSize: Style.fontSizeNormal; font.family: Style.monoFont; Layout.alignment: Qt.AlignRight }
                Label { text: "Toggle this help"; font.pointSize: Style.fontSizeNormal }
            }

            Label {
                text: "Press any key to close"
                opacity: 0.75
                font.pointSize: Style.fontSizeSmall
                Layout.alignment: Qt.AlignHCenter
            }
        }
    }
}
