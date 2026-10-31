import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Results table: a column header and the list beneath it, kept in one item so
// the header always spans exactly the list's width (the preview pane can take
// part of the window). Columns resize the way Dolphin's do: a dragged
// separator follows the pointer and only the columns to its right move.
// Name takes whatever width the others and the free space at the right
// leave, so it grows and shrinks with the window and the preview pane while
// the other columns keep their widths. Path, Size, and Modified can be
// hidden from the header's right-click menu.
ColumnLayout {
    id: root
    spacing: 0

    property alias model: listView.model
    property alias currentIndex: listView.currentIndex
    readonly property alias count: listView.count

    // Sort state per column, owned by Main: 0 = none, 1 = ascending, 2 = descending
    property int nameSortState: 0
    property int pathSortState: 0
    property int sizeSortState: 0
    property int dateSortState: 0

    // Shown in place of the list while it is empty
    property string placeholderIcon: ""
    property string placeholderText: ""
    property string placeholderExplanation: ""

    // Keeps the selection: the engine, or anything with its methods
    // (selection_count, is_selected, select_range, toggle_selected,
    // select_all, clear_selection, selected_paths)
    property var selector: null
    // Changes with every selection edit, so row highlights follow
    property int selectionRevision: 0
    // Where a Shift range starts: the row last clicked or moved to without Shift
    property int anchorRow: 0

    signal itemDoubleClicked()
    // A key typed while the list has focus, for the search field: `text`
    // to add, or `erase` for Backspace
    signal typedToSearch(string text, bool erase)
    signal contextMenuRequested(var paths, int globalX, int globalY)
    signal openFolderRequested(string path)
    signal headerClicked(string key)
    signal copyRequested(var paths)
    signal copyLocationRequested(var paths)
    // Main runs the drag, which returns after the drop
    signal dragRequested(var paths)
    // The user resized or hid a column; Main persists layoutJson()
    signal layoutEdited()

    // Column layout. Widths are unzoomed pixels so they survive a zoom change.
    // `free` is the empty space right of the last column, left there by
    // narrowing a column; Name has no stored width.
    readonly property var columnKeys: ["path", "size", "date"]
    readonly property var defaultWidths: ({ path: 300, size: 80, date: 140, free: 0 })
    readonly property var minWidths: ({ name: Kirigami.Units.gridUnit * 8, path: 100, size: 60, date: 100 })
    property var widths: defaultWidths
    property var hidden: []

    function shown(key) {
        return hidden.indexOf(key) < 0
    }

    readonly property int freeWidth: Math.round((widths.free || 0) * Style.zoomFactor)

    // Shown widths of Path, Size, and Modified: their own, unless the list is
    // too narrow for them and Name at its minimum, when Path gives way first,
    // then Modified, then Size, each down to its minimum. Whatever still does
    // not fit is clipped at the right edge.
    readonly property var shownWidths: {
        var w = {}
        var total = 0
        for (var i = 0; i < columnKeys.length; i++) {
            var key = columnKeys[i]
            w[key] = shown(key) ? Math.round(widths[key] * Style.zoomFactor) : 0
            total += w[key]
        }
        var over = nameMinWidth + total - availableWidth
        var order = ["path", "date", "size"]
        for (var j = 0; j < order.length && over > 0; j++) {
            var k = order[j]
            if (!shown(k))
                continue
            var take = Math.min(over, w[k] - minPx(k))
            if (take > 0) {
                w[k] -= take
                over -= take
            }
        }
        return w
    }

    function colWidth(key) {
        return shownWidths[key]
    }

    // Shown width of any column, Name included
    function widthOf(key) {
        return key === "name" ? nameColumnWidth : colWidth(key)
    }

    // Name first, then the shown optional columns, in display order
    readonly property var visibleKeys: ["name"].concat(columnKeys.filter(function(k) { return hidden.indexOf(k) < 0 }))

    // Distance from the header's left content edge to `key`'s right separator
    function edgeOf(key) {
        var x = 0
        for (var i = 0; i < visibleKeys.length; i++) {
            x += widthOf(visibleKeys[i])
            if (visibleKeys[i] === key)
                break
        }
        return x
    }

    function minPx(key) {
        return Math.round(minWidths[key] * Style.zoomFactor)
    }

    // Shown widths when a separator drag starts, and the free space then
    property var dragStart: ({})
    property int dragRoom: 0

    function beginResize() {
        var start = {}
        for (var i = 0; i < visibleKeys.length; i++)
            start[visibleKeys[i]] = widthOf(visibleKeys[i])
        dragStart = start
        dragRoom = Math.max(0, availableWidth - nameColumnWidth - othersWidth)
    }

    // Size `key` so its right separator sits `delta` px from where the drag
    // began. Growing uses the free space first, then narrows the columns
    // after it from the rightmost inward, each down to its minimum, so the
    // dragged separator follows the pointer and nothing is pushed out of view.
    function resizeTo(key, delta) {
        var start = dragStart[key]
        var desired = Math.max(minPx(key), start + delta)
        var after = visibleKeys.slice(visibleKeys.indexOf(key) + 1)
        var px = {}
        px[key] = desired
        for (var i = 0; i < after.length; i++)
            px[after[i]] = dragStart[after[i]]
        var overflow = Math.max(0, desired - start - dragRoom)
        for (var j = after.length - 1; j >= 0 && overflow > 0; j--) {
            var take = Math.min(overflow, dragStart[after[j]] - minPx(after[j]))
            px[after[j]] -= take
            overflow -= take
        }
        px[key] -= overflow
        // Name keeps its width unless it is the column dragged, so what the
        // others give or take comes out of the free space
        var namePx = key === "name" ? px.name : dragStart.name
        var used = namePx
        var next = Object.assign({}, widths)
        for (var k in px) {
            if (k === "name")
                continue
            next[k] = Math.max(minWidths[k], Math.round(px[k] / Style.zoomFactor))
            used += px[k]
        }
        for (var c = 0; c < columnKeys.length; c++) {
            if (px[columnKeys[c]] === undefined)
                used += colWidth(columnKeys[c])
        }
        next.free = Math.max(0, Math.round((availableWidth - used) / Style.zoomFactor))
        widths = next
        layoutEdited()
    }

    // Size `key` to fit its title and the rows on screen, as a double click
    // on a separator does in Dolphin. Rows off screen are not measured: a
    // result set can hold a million paths.
    function fitColumn(key) {
        var cells = { name: nameHeader, path: pathHeader, size: sizeHeader, date: dateHeader }
        var px = cells[key].implicitWidth
        var first = Math.max(0, listView.indexAt(0, listView.contentY))
        var last = listView.indexAt(0, listView.contentY + listView.height - 1)
        if (last < 0)
            last = listView.count - 1
        for (var i = first; i <= last; i++) {
            var row = listView.itemAtIndex(i)
            if (row)
                px = Math.max(px, row.fitWidth(key))
        }
        var next = Object.assign({}, widths)
        if (key === "name") {
            // Name's width is what the others and the free space leave
            next.free = Math.max(0, Math.round((availableWidth - px - othersWidth) / Style.zoomFactor))
        } else {
            var fitted = Math.max(minWidths[key], Math.ceil(px / Style.zoomFactor))
            // A wider column takes the free space first, then from Name
            var growth = Math.round((fitted - widths[key]) * Style.zoomFactor)
            next.free = Math.max(0, Math.round((freeWidth - Math.max(0, growth)) / Style.zoomFactor))
            next[key] = fitted
        }
        widths = next
        layoutEdited()
    }

    function toggleColumn(key) {
        var next = hidden.slice()
        var i = next.indexOf(key)
        if (i >= 0)
            next.splice(i, 1)
        else
            next.push(key)
        hidden = next
        layoutEdited()
    }

    function resetColumns() {
        widths = defaultWidths
        hidden = []
        layoutEdited()
    }

    // Restore a layout saved by layoutJson(); unknown keys and bad widths are
    // dropped so a hand-edited config cannot break the table.
    function applyLayout(json) {
        var saved
        try {
            saved = JSON.parse(json)
        } catch (e) {
            return
        }
        var next = Object.assign({}, defaultWidths)
        var savedWidths = saved.widths || {}
        for (var i = 0; i < columnKeys.length; i++) {
            var key = columnKeys[i]
            if (typeof savedWidths[key] === "number")
                next[key] = Math.max(minWidths[key], savedWidths[key])
        }
        if (typeof savedWidths.free === "number")
            next.free = Math.max(0, savedWidths.free)
        widths = next
        hidden = (saved.hidden || []).filter(function(k) { return columnKeys.indexOf(k) >= 0 })
    }

    function layoutJson() {
        return JSON.stringify({ widths: widths, hidden: hidden })
    }

    readonly property int rowPadding: Style.marginSmall
    readonly property real scrollBarWidth: listView.ScrollBar.vertical && listView.ScrollBar.vertical.visible
        ? listView.ScrollBar.vertical.width : 0
    readonly property int availableWidth: listView.width - 2 * rowPadding - scrollBarWidth
    readonly property int othersWidth: colWidth("path") + colWidth("size") + colWidth("date")
    readonly property int nameMinWidth: Math.round(minWidths.name * Style.zoomFactor)
    // Everything the other columns and the free space leave, down to the
    // minimum (then the free space goes first)
    readonly property int nameColumnWidth: Math.max(nameMinWidth, availableWidth - othersWidth - freeWidth)

    // Whether row `i` is selected; with no selection, the current row is
    function isSelected(i) {
        if (selector && selector.selection_count > 0)
            return selectionRevision >= 0 && selector.is_selected(i)
        return i === listView.currentIndex
    }

    // Paths of the selected rows, or of the current row
    function selectedPaths() {
        if (selector && selector.selection_count > 0)
            return selector.selected_paths()
        var item = listView.currentItem
        return item && item.filePath ? [item.filePath] : []
    }

    // Make row `i` current and scroll it into view. With `extend`, select
    // from the anchor to it; otherwise the selection is just this row again.
    function moveTo(i, extend) {
        if (listView.count === 0)
            return
        i = Math.max(0, Math.min(listView.count - 1, i))
        listView.currentIndex = i
        listView.positionViewAtIndex(i, ListView.Contain)
        if (extend && selector) {
            selector.select_range(anchorRow, i, false)
        } else {
            anchorRow = i
            if (selector)
                selector.clear_selection()
        }
    }

    // Ctrl+click: add or remove row `i`, keeping the rest
    function toggleRow(i) {
        if (!selector)
            return
        // Until now the current row stood for the selection; keep it
        if (selector.selection_count === 0 && listView.currentIndex >= 0)
            selector.select_range(listView.currentIndex, listView.currentIndex, false)
        selector.toggle_selected(i)
        listView.currentIndex = i
        anchorRow = i
    }

    // Rows a page moves: what fits in the view, less one kept for context
    readonly property int pageRows: Math.max(1, Math.floor(listView.height / Style.rowHeight) - 1)

    // Keys for the results, from the search field `textField`, or from the
    // list (`textField` null), which takes focus when a row is clicked.
    // Arrows, Page Up/Down, and Home/End move, Shift extending the
    // selection; in the search field Shift+Home/End select its text, and with
    // no results Home/End move its cursor. Ctrl+C copies the selected files
    // unless search text is selected, and Ctrl+Alt+C copies their locations.
    // Returns whether the key was used.
    function handleKey(event, textField) {
        var mods = event.modifiers & (Qt.ShiftModifier | Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)
        if (event.matches(StandardKey.Copy) && !(textField && textField.selectedText !== "")) {
            var files = selectedPaths()
            if (files.length > 0)
                copyRequested(files)
            return files.length > 0
        }
        if (event.key === Qt.Key_C && mods === (Qt.ControlModifier | Qt.AltModifier)) {
            var locations = selectedPaths()
            if (locations.length > 0)
                copyLocationRequested(locations)
            return locations.length > 0
        }
        var shift = mods === Qt.ShiftModifier
        if (mods !== Qt.NoModifier && !shift)
            return false
        var current = listView.currentIndex
        switch (event.key) {
        case Qt.Key_Down:
            moveTo(current + 1, shift)
            return true
        case Qt.Key_Up:
            moveTo(current - 1, shift)
            return true
        case Qt.Key_PageDown:
            moveTo(current + pageRows, shift)
            return true
        case Qt.Key_PageUp:
            moveTo(current - pageRows, shift)
            return true
        case Qt.Key_Home:
        case Qt.Key_End:
            if (listView.count === 0 || (shift && textField))
                return false
            moveTo(event.key === Qt.Key_Home ? 0 : listView.count - 1, shift)
            return true
        }
        return false
    }

    // Keys while the list has focus: the results keys, Enter to open, Ctrl+A
    // to select every result, and typing or Backspace, which go back to the
    // search field
    function handleListKey(event) {
        var mods = event.modifiers & (Qt.ShiftModifier | Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)
        if (event.key === Qt.Key_Return || event.key === Qt.Key_Enter) {
            itemDoubleClicked()
            return true
        }
        if (event.key === Qt.Key_A && mods === Qt.ControlModifier) {
            if (selector && listView.count > 0)
                selector.select_all()
            return true
        }
        if (handleKey(event, null))
            return true
        if (event.key === Qt.Key_Backspace && mods === Qt.NoModifier) {
            typedToSearch("", true)
            return true
        }
        if (event.text.length > 0 && event.text.charCodeAt(0) >= 32
                && (mods & (Qt.ControlModifier | Qt.AltModifier | Qt.MetaModifier)) === 0) {
            typedToSearch(event.text, false)
            return true
        }
        return false
    }

    // A click on a row takes focus to the list, so Ctrl+A selects results
    function focusList() {
        listView.forceActiveFocus()
    }

    // The selection's context menu, at (x, y) in `item`
    function requestContextMenu(item, x, y) {
        var paths = selectedPaths()
        if (paths.length === 0)
            return
        var p = item.mapToGlobal(x, y)
        contextMenuRequested(paths, p.x, p.y)
    }

    // From the keyboard: below the current row's name
    function openContextMenu() {
        if (listView.currentIndex < 0)
            return
        listView.positionViewAtIndex(listView.currentIndex, ListView.Contain)
        var item = listView.currentItem
        if (item)
            requestContextMenu(item, rowPadding + Style.iconSize, item.height)
    }

    // A drag from row `i` carries the selection when `i` is in it, else `i`
    // alone. The drag runs its own event loop until the drop and takes the
    // release with it, so it starts after this handler returns, and the
    // handler is reset afterwards.
    function startDrag(i, handler) {
        if (!isSelected(i))
            moveTo(i, false)
        var paths = selectedPaths()
        Qt.callLater(function() {
            dragRequested(paths)
            if (handler) {
                handler.enabled = false
                handler.enabled = true
            }
        })
    }

    // Column header
    Item {
        Layout.fillWidth: true
        Layout.preferredHeight: Style.headerHeight

        Kirigami.Theme.colorSet: Kirigami.Theme.Header
        Kirigami.Theme.inherit: false

        Rectangle {
            anchors.fill: parent
            color: Kirigami.Theme.backgroundColor
        }

        Kirigami.Separator {
            anchors.bottom: parent.bottom
            width: parent.width
        }

        RowLayout {
            anchors.fill: parent
            anchors.leftMargin: root.rowPadding
            anchors.rightMargin: root.rowPadding + root.scrollBarWidth
            spacing: 0
            // Columns that do not fit stop at the edge
            clip: true

            HeaderCell {
                id: nameHeader
                title: "Name"
                Layout.preferredWidth: root.nameColumnWidth
                Layout.minimumWidth: root.nameColumnWidth
                Layout.maximumWidth: root.nameColumnWidth
                sortState: root.nameSortState
                onClicked: root.headerClicked("name")
            }
            HeaderCell {
                id: pathHeader
                title: "Path"
                visible: root.shown("path")
                Layout.preferredWidth: root.colWidth("path")
                Layout.minimumWidth: root.colWidth("path")
                Layout.maximumWidth: root.colWidth("path")
                sortState: root.pathSortState
                onClicked: root.headerClicked("path")
            }
            HeaderCell {
                id: sizeHeader
                title: "Size"
                visible: root.shown("size")
                Layout.preferredWidth: root.colWidth("size")
                Layout.minimumWidth: root.colWidth("size")
                Layout.maximumWidth: root.colWidth("size")
                alignRight: true
                sortState: root.sizeSortState
                onClicked: root.headerClicked("size")
            }
            HeaderCell {
                id: dateHeader
                title: "Modified"
                visible: root.shown("date")
                Layout.preferredWidth: root.colWidth("date")
                Layout.minimumWidth: root.colWidth("date")
                Layout.maximumWidth: root.colWidth("date")
                alignRight: true
                sortState: root.dateSortState
                onClicked: root.headerClicked("date")
            }
            Item { Layout.fillWidth: true }
        }

        // One drag handle centered on the separator after each shown column,
        // above the cells so a press there never reaches a header button.
        // Double-clicking it fits the column to its left.
        Repeater {
            model: root.visibleKeys

            MouseArea {
                required property string modelData

                x: root.rowPadding + root.edgeOf(modelData) - width / 2
                width: Kirigami.Units.largeSpacing
                height: parent.height
                z: 1
                cursorShape: Qt.SplitHCursor
                // Take the hover so the header button under the separator
                // does not light up while the pointer is on the handle
                hoverEnabled: true
                preventStealing: true

                property real startX: 0

                onPressed: function(mouse) {
                    startX = mapToGlobal(mouse.x, 0).x
                    root.beginResize()
                }
                onPositionChanged: function(mouse) {
                    if (pressed)
                        root.resizeTo(modelData, mapToGlobal(mouse.x, 0).x - startX)
                }
                onDoubleClicked: function(mouse) {
                    root.fitColumn(modelData)
                    // A pixel of movement before the release resizes from
                    // the fitted width, not the one the press recorded
                    startX = mapToGlobal(mouse.x, 0).x
                    root.beginResize()
                }
            }
        }

        TapHandler {
            acceptedButtons: Qt.RightButton
            onTapped: columnsMenu.popup()
        }
    }

    Menu {
        id: columnsMenu
        onAboutToShow: {
            pathItem.checked = root.shown("path")
            sizeItem.checked = root.shown("size")
            dateItem.checked = root.shown("date")
        }
        MenuItem { id: pathItem; text: "Path"; checkable: true; onTriggered: root.toggleColumn("path") }
        MenuItem { id: sizeItem; text: "Size"; checkable: true; onTriggered: root.toggleColumn("size") }
        MenuItem { id: dateItem; text: "Modified"; checkable: true; onTriggered: root.toggleColumn("date") }
        MenuSeparator {}
        MenuItem { text: "Reset Columns"; onTriggered: root.resetColumns() }
    }

    ListView {
        id: listView
        Layout.fillWidth: true
        Layout.fillHeight: true
        clip: true
        currentIndex: -1
        boundsBehavior: Flickable.StopAtBounds
        highlightMoveDuration: 0

        Keys.onPressed: function(event) {
            event.accepted = root.handleListKey(event)
        }

        // A content view: the View color set, as Dolphin's file view uses
        Kirigami.Theme.colorSet: Kirigami.Theme.View
        Kirigami.Theme.inherit: false

        Rectangle {
            parent: listView
            anchors.fill: parent
            z: -1
            color: Kirigami.Theme.backgroundColor
        }

        delegate: ItemDelegate {
            id: row
            width: listView.width
            height: Style.rowHeight
            leftPadding: root.rowPadding
            rightPadding: root.rowPadding + root.scrollBarWidth
            topPadding: 0
            bottomPadding: 0
            highlighted: root.isSelected(index)
            Kirigami.StyleHints.useAlternateBackgroundColor: true

            readonly property color textColor: highlighted
                ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
            // Secondary columns use the theme's dim (0.75, as Plasma does),
            // except on the selected row where full contrast reads better
            readonly property real dim: highlighted ? 1 : 0.75
            readonly property string filePath: model.path || ""

            // Width `key`'s cell needs to show this row without shortening
            function fitWidth(key) {
                switch (key) {
                case "name":
                    return nameLabel.implicitWidth + Style.iconSize + 3 * Style.marginSmall
                case "path":
                    return pathLabel.implicitWidth + 2 * Style.marginSmall
                case "size":
                    return sizeLabel.implicitWidth + Style.marginSmall
                case "date":
                    return dateLabel.implicitWidth + Style.marginSmall
                }
                return 0
            }
            readonly property string folder: {
                var p = model.path || ""
                var fn = model.filename || ""
                return p.endsWith("/" + fn) ? p.substring(0, p.length - fn.length - 1) : p
            }

            // Whether `point` (in the row) is on the path text, where a double
            // click opens the folder rather than the file
            function onPath(point) {
                if (!root.shown("path"))
                    return false
                var p = pathLabel.mapFromItem(row, point.x, point.y)
                return p.x >= 0 && p.x <= Math.min(pathLabel.paintedWidth, pathLabel.width)
            }

            // A double click opens the file, or on the path text, its folder
            onDoubleClicked: {
                if (onPath(Qt.point(pressX, pressY)))
                    root.openFolderRequested(filePath)
                else
                    root.itemDoubleClicked()
            }

            // A click selects the row, Ctrl+click adds or removes it, and
            // Shift+click selects from the anchor
            TapHandler {
                acceptedButtons: Qt.LeftButton
                acceptedModifiers: Qt.NoModifier
                onTapped: {
                    root.focusList()
                    root.moveTo(index, false)
                }
            }
            TapHandler {
                acceptedButtons: Qt.LeftButton
                acceptedModifiers: Qt.ControlModifier
                onTapped: {
                    root.focusList()
                    root.toggleRow(index)
                }
            }
            TapHandler {
                acceptedButtons: Qt.LeftButton
                acceptedModifiers: Qt.ShiftModifier
                onTapped: {
                    root.focusList()
                    root.moveTo(index, true)
                }
            }

            TapHandler {
                acceptedButtons: Qt.RightButton
                onTapped: function(eventPoint) {
                    root.focusList()
                    // On a row outside the selection, the menu is for that row alone
                    if (!root.isSelected(index))
                        root.moveTo(index, false)
                    root.requestContextMenu(row, eventPoint.position.x, eventPoint.position.y)
                }
            }

            // Dragging rows hands the files to another app: Dolphin, a
            // browser, a chat. It takes over from the list's flicking, which
            // a mouse has no use for.
            DragHandler {
                id: dragHandler
                target: null
                acceptedButtons: Qt.LeftButton
                acceptedDevices: PointerDevice.Mouse | PointerDevice.TouchPad | PointerDevice.Stylus
                grabPermissions: PointerHandler.CanTakeOverFromAnything
                onActiveChanged: {
                    if (active)
                        root.startDrag(index, dragHandler)
                }
            }

            contentItem: RowLayout {
                spacing: 0
                // Columns that do not fit stop short of the scroll bar
                clip: true

                // Name: file type icon and name, shortened in the middle so
                // the extension stays visible
                RowLayout {
                    Layout.preferredWidth: root.nameColumnWidth
                    Layout.minimumWidth: root.nameColumnWidth
                    Layout.maximumWidth: root.nameColumnWidth
                    spacing: Style.marginSmall

                    Kirigami.Icon {
                        source: model.iconName
                        selected: row.highlighted
                        Layout.preferredWidth: Style.iconSize
                        Layout.preferredHeight: Style.iconSize
                        Layout.leftMargin: Style.marginSmall
                    }
                    Label {
                        id: nameLabel
                        // Bold where the search matched
                        text: model.nameMarkup || model.filename || ""
                        textFormat: model.nameMarkup ? Text.StyledText : Text.PlainText
                        color: row.textColor
                        font.pointSize: Style.fontSizeNormal
                        elide: Text.ElideMiddle
                        Layout.fillWidth: true
                        Layout.rightMargin: Style.marginSmall

                        // The full name when it is shortened
                        HoverHandler {
                            id: nameHover
                        }
                        ToolTip.visible: nameHover.hovered && truncated
                        ToolTip.delay: Kirigami.Units.toolTipDelay
                        ToolTip.text: model.filename || ""
                    }
                }

                // Path: the containing folder, shortened from the start so its
                // deepest folders stay visible; double-clicking the text opens it
                Item {
                    visible: root.shown("path")
                    Layout.preferredWidth: root.colWidth("path")
                    Layout.minimumWidth: root.colWidth("path")
                    Layout.maximumWidth: root.colWidth("path")
                    Layout.fillHeight: true

                    Label {
                        id: pathLabel
                        anchors.fill: parent
                        anchors.leftMargin: Style.marginSmall
                        anchors.rightMargin: Style.marginSmall
                        text: row.folder
                        color: linkHover.hovered && !row.highlighted
                            ? Kirigami.Theme.linkColor : row.textColor
                        opacity: linkHover.hovered ? 1 : row.dim
                        font.pointSize: Style.fontSizeNormal
                        font.underline: linkHover.hovered
                        elide: Text.ElideLeft
                        textFormat: Text.PlainText
                        verticalAlignment: Text.AlignVCenter
                    }
                    // Hover only: clicks go to the row, so a stray click
                    // selects it rather than opening the folder
                    Item {
                        x: pathLabel.x
                        width: Math.min(pathLabel.paintedWidth, pathLabel.width)
                        height: parent.height

                        HoverHandler {
                            id: linkHover
                        }
                        ToolTip.visible: linkHover.hovered
                        ToolTip.delay: Kirigami.Units.toolTipDelay
                        ToolTip.text: "Double-click to open " + row.folder
                    }
                }

                Label {
                    id: sizeLabel
                    visible: root.shown("size")
                    Layout.preferredWidth: root.colWidth("size")
                    Layout.minimumWidth: root.colWidth("size")
                    Layout.maximumWidth: root.colWidth("size")
                    rightPadding: Style.marginSmall
                    text: model.isDir ? "" : Style.formatSize(model.fileSize)
                    color: row.textColor
                    opacity: row.dim
                    font.pointSize: Style.fontSizeNormal
                    horizontalAlignment: Text.AlignRight
                    elide: Text.ElideRight
                }

                Label {
                    id: dateLabel
                    visible: root.shown("date")
                    Layout.preferredWidth: root.colWidth("date")
                    Layout.minimumWidth: root.colWidth("date")
                    Layout.maximumWidth: root.colWidth("date")
                    rightPadding: Style.marginSmall
                    text: Style.formatDate(model.mtime)
                    color: row.textColor
                    opacity: row.dim
                    font.pointSize: Style.fontSizeNormal
                    horizontalAlignment: Text.AlignRight
                    elide: Text.ElideRight
                }

                Item { Layout.fillWidth: true }
            }
        }

        ScrollBar.vertical: ScrollBar {
            policy: ScrollBar.AsNeeded
            // The handle's share of the bar is the share of rows on screen,
            // a sliver for a hundred thousand results; keep it long enough
            // to see and grab, as Dolphin's is in an ordinary folder
            minimumSize: height > 0 ? Math.min(0.5, Kirigami.Units.gridUnit * 5 / height) : 0
        }

        Kirigami.PlaceholderMessage {
            parent: listView
            anchors.centerIn: parent
            width: parent.width - Kirigami.Units.gridUnit * 4
            visible: listView.count === 0 && text !== ""
            icon.name: root.placeholderIcon
            text: root.placeholderText
            explanation: root.placeholderExplanation
        }
    }

    // Clickable column header: a flat tool button for Breeze's hover and press
    // feedback, a sort arrow, and a separator on its right edge
    component HeaderCell: ToolButton {
        id: cell
        property string title: ""
        property int sortState: 0
        property bool sortable: true
        property bool alignRight: false

        Layout.fillHeight: true
        hoverEnabled: sortable
        down: sortable && pressed
        focusPolicy: Qt.NoFocus
        leftPadding: Style.marginSmall
        rightPadding: Style.marginSmall

        contentItem: RowLayout {
            spacing: Style.marginSmall

            Item {
                visible: cell.alignRight
                Layout.fillWidth: true
            }
            Label {
                text: cell.title
                font.pointSize: Style.fontSizeNormal
                font.bold: cell.sortState !== 0
                elide: Text.ElideRight
                Layout.fillWidth: !cell.alignRight
            }
            Kirigami.Icon {
                visible: cell.sortState !== 0
                source: cell.sortState === 1 ? "arrow-up" : "arrow-down"
                Layout.preferredWidth: Style.sortIconSize
                Layout.preferredHeight: Style.sortIconSize
            }
        }

        Kirigami.Separator {
            anchors.right: parent.right
            anchors.verticalCenter: parent.verticalCenter
            height: parent.height - 2 * Kirigami.Units.smallSpacing
        }
    }
}
