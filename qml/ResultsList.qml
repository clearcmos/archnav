import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

// Results table: a column header and the list beneath it, kept in one item so
// the header always spans exactly the list's width (the preview pane can take
// part of the window). Columns resize the way Dolphin's do: until the user
// drags a separator, Name fills whatever width the others leave; the first
// drag fixes Name at its current width, and from then on a dragged separator
// follows the pointer and only the columns to its right move. Path, Size,
// Modified, and Tags can be hidden from the header's right-click menu.
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

    signal itemDoubleClicked()
    signal contextMenuRequested(string path, int globalX, int globalY)
    signal openFolderRequested(string path)
    signal headerClicked(string key)
    // The user resized or hid a column; Main persists layoutJson()
    signal layoutEdited()

    // Column layout. Widths are unzoomed pixels so they survive a zoom change.
    // `widths` has no "name" entry while Name fills automatically.
    readonly property var columnKeys: ["path", "size", "date", "tags"]
    readonly property var defaultWidths: ({ path: 300, size: 80, date: 140, tags: 120 })
    readonly property var minWidths: ({ name: Kirigami.Units.gridUnit * 8, path: 100, size: 60, date: 100, tags: 60 })
    property var widths: defaultWidths
    property var hidden: []

    function shown(key) {
        return hidden.indexOf(key) < 0
    }

    function colWidth(key) {
        return shown(key) ? Math.round(widths[key] * Style.zoomFactor) : 0
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

    // Stop Name filling automatically: from now on it keeps its width
    function freezeName() {
        if (widths.name === undefined) {
            var next = Object.assign({}, widths)
            next.name = Math.round(nameColumnWidth / Style.zoomFactor)
            widths = next
        }
    }

    function minPx(key) {
        return Math.round(minWidths[key] * Style.zoomFactor)
    }

    // Shown widths when a separator drag starts, and the free space then
    property var dragStart: ({})
    property int dragRoom: 0

    function beginResize() {
        freezeName()
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
        var next = Object.assign({}, widths)
        for (var k in px)
            next[k] = Math.max(minWidths[k], Math.round(px[k] / Style.zoomFactor))
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
        var keys = ["name"].concat(columnKeys)
        for (var i = 0; i < keys.length; i++) {
            var key = keys[i]
            if (typeof savedWidths[key] === "number")
                next[key] = Math.max(minWidths[key], savedWidths[key])
        }
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
    readonly property int othersWidth: colWidth("path") + colWidth("size") + colWidth("date") + colWidth("tags")
    readonly property int nameMinWidth: Math.round(minWidths.name * Style.zoomFactor)
    // Auto: everything the other columns leave. Fixed: the user's width, but
    // given back down to the minimum when the list narrows (preview pane),
    // so the columns to the right stay visible as long as possible.
    readonly property int nameColumnWidth: widths.name === undefined
        ? Math.max(nameMinWidth, availableWidth - othersWidth)
        : Math.max(nameMinWidth, Math.min(Math.round(widths.name * Style.zoomFactor), availableWidth - othersWidth))

    function moveDown() {
        if (listView.currentIndex < listView.count - 1) {
            listView.currentIndex++
            listView.positionViewAtIndex(listView.currentIndex, ListView.Contain)
        }
    }

    function moveUp() {
        if (listView.currentIndex > 0) {
            listView.currentIndex--
            listView.positionViewAtIndex(listView.currentIndex, ListView.Contain)
        }
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

            HeaderCell {
                title: "Name"
                Layout.preferredWidth: root.nameColumnWidth
                Layout.minimumWidth: root.nameColumnWidth
                Layout.maximumWidth: root.nameColumnWidth
                sortState: root.nameSortState
                onClicked: root.headerClicked("name")
            }
            HeaderCell {
                title: "Path"
                visible: root.shown("path")
                Layout.preferredWidth: root.colWidth("path")
                Layout.minimumWidth: root.colWidth("path")
                Layout.maximumWidth: root.colWidth("path")
                sortState: root.pathSortState
                onClicked: root.headerClicked("path")
            }
            HeaderCell {
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
                title: "Modified"
                visible: root.shown("date")
                Layout.preferredWidth: root.colWidth("date")
                Layout.minimumWidth: root.colWidth("date")
                Layout.maximumWidth: root.colWidth("date")
                alignRight: true
                sortState: root.dateSortState
                onClicked: root.headerClicked("date")
            }
            // Tags live in the tag stores, outside the engine's sorters
            HeaderCell {
                title: "Tags"
                visible: root.shown("tags")
                Layout.preferredWidth: root.colWidth("tags")
                Layout.minimumWidth: root.colWidth("tags")
                Layout.maximumWidth: root.colWidth("tags")
                sortable: false
            }
            Item { Layout.fillWidth: true }
        }

        // One drag handle centered on the separator after each shown column,
        // above the cells so a press there never reaches a header button
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
            tagsItem.checked = root.shown("tags")
        }
        MenuItem { id: pathItem; text: "Path"; checkable: true; onTriggered: root.toggleColumn("path") }
        MenuItem { id: sizeItem; text: "Size"; checkable: true; onTriggered: root.toggleColumn("size") }
        MenuItem { id: dateItem; text: "Modified"; checkable: true; onTriggered: root.toggleColumn("date") }
        MenuItem { id: tagsItem; text: "Tags"; checkable: true; onTriggered: root.toggleColumn("tags") }
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
            highlighted: ListView.isCurrentItem
            Kirigami.StyleHints.useAlternateBackgroundColor: true

            readonly property color textColor: highlighted
                ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
            // Secondary columns use the theme's dim (0.75, as Plasma does),
            // except on the selected row where full contrast reads better
            readonly property real dim: highlighted ? 1 : 0.75
            readonly property string folder: {
                var p = model.path || ""
                var fn = model.filename || ""
                return p.endsWith("/" + fn) ? p.substring(0, p.length - fn.length - 1) : p
            }

            onClicked: listView.currentIndex = index
            onDoubleClicked: {
                listView.currentIndex = index
                root.itemDoubleClicked()
            }

            TapHandler {
                acceptedButtons: Qt.RightButton
                onTapped: function(eventPoint) {
                    listView.currentIndex = index
                    var p = row.mapToGlobal(eventPoint.position.x, eventPoint.position.y)
                    root.contextMenuRequested(model.path, p.x, p.y)
                }
            }

            contentItem: RowLayout {
                spacing: 0

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
                        text: model.filename || ""
                        color: row.textColor
                        font.pointSize: Style.fontSizeNormal
                        elide: Text.ElideMiddle
                        textFormat: Text.PlainText
                        Layout.fillWidth: true
                        Layout.rightMargin: Style.marginSmall
                    }
                }

                // Path: the containing folder, shortened from the start so its
                // deepest folders stay visible; clicking the text opens it
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
                        color: linkArea.containsMouse && !row.highlighted
                            ? Kirigami.Theme.linkColor : row.textColor
                        opacity: linkArea.containsMouse ? 1 : row.dim
                        font.pointSize: Style.fontSizeNormal
                        font.underline: linkArea.containsMouse
                        elide: Text.ElideLeft
                        textFormat: Text.PlainText
                        verticalAlignment: Text.AlignVCenter
                    }
                    MouseArea {
                        id: linkArea
                        x: pathLabel.x
                        width: Math.min(pathLabel.paintedWidth, pathLabel.width)
                        height: parent.height
                        hoverEnabled: true
                        cursorShape: Qt.PointingHandCursor
                        onClicked: {
                            listView.currentIndex = index
                            root.openFolderRequested(model.path)
                        }
                        ToolTip.visible: containsMouse
                        ToolTip.delay: Kirigami.Units.toolTipDelay
                        ToolTip.text: "Open " + row.folder
                    }
                }

                Label {
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

                // Tags from the file's tag store, empty when untagged
                Label {
                    visible: root.shown("tags")
                    Layout.preferredWidth: root.colWidth("tags")
                    Layout.minimumWidth: root.colWidth("tags")
                    Layout.maximumWidth: root.colWidth("tags")
                    leftPadding: Style.marginSmall
                    rightPadding: Style.marginSmall
                    text: model.tags || ""
                    color: row.textColor
                    font.pointSize: Style.fontSizeNormal
                    elide: Text.ElideRight
                    textFormat: Text.PlainText
                }

                Item { Layout.fillWidth: true }
            }
        }

        ScrollBar.vertical: ScrollBar {
            policy: ScrollBar.AsNeeded
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
