import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

ListView {
    id: listView
    clip: true
    currentIndex: -1
    boundsBehavior: Flickable.StopAtBounds
    highlightMoveDuration: 0

    signal itemDoubleClicked()
    signal contextMenuRequested(string path, int globalX, int globalY)

    // Column widths passed from parent
    property int nameColumnWidth: 280
    property int pathColumnWidth: 400
    property int sizeColumnWidth: 90
    property int dateColumnWidth: 180

    delegate: ItemDelegate {
        id: delegateItem
        width: listView.width
        height: Style.rowHeight
        padding: 0
        topPadding: 0
        bottomPadding: 0

        // Handle both clicks and right-clicks
        MouseArea {
            anchors.fill: parent
            acceptedButtons: Qt.LeftButton | Qt.RightButton

            onClicked: function(mouse) {
                listView.currentIndex = index
                if (mouse.button === Qt.RightButton) {
                    var globalPos = mapToGlobal(mouse.x, mouse.y)
                    listView.contextMenuRequested(model.path, globalPos.x, globalPos.y)
                }
            }

            onDoubleClicked: function(mouse) {
                if (mouse.button === Qt.LeftButton) {
                    listView.currentIndex = index
                    listView.itemDoubleClicked()
                }
            }
        }

        contentItem: RowLayout {
            anchors.leftMargin: Style.marginNormal
            anchors.rightMargin: Style.marginNormal
            spacing: 0

            // Name column
            Item {
                Layout.preferredWidth: listView.nameColumnWidth
                Layout.fillHeight: true

                RowLayout {
                    anchors.fill: parent
                    anchors.leftMargin: Style.marginSmall
                    anchors.rightMargin: Style.marginSmall
                    spacing: Style.marginNormal

                    // File/folder icon
                    Label {
                        text: model.isDir ? "\uD83D\uDCC1" : "\uD83D\uDCC4"
                        font.pixelSize: Style.fontSizeNormal
                        Layout.alignment: Qt.AlignVCenter
                    }

                    // Filename
                    Label {
                        text: model.filename || ""
                        color: delegateItem.ListView.isCurrentItem
                            ? Style.textHighlighted
                            : (model.isDir ? Style.accentBlue : Style.textSecondary)
                        font.pixelSize: Style.fontSizeNormal
                        font.bold: model.isDir
                        elide: Text.ElideRight
                        Layout.fillWidth: true
                        Layout.alignment: Qt.AlignVCenter
                    }
                }
            }

            // Path column
            Item {
                Layout.preferredWidth: listView.pathColumnWidth
                Layout.fillHeight: true

                Label {
                    anchors.fill: parent
                    anchors.leftMargin: Style.marginSmall
                    anchors.rightMargin: Style.marginSmall
                    text: {
                        var p = model.path || ""
                        var fn = model.filename || ""
                        if (p.endsWith("/" + fn))
                            return p.substring(0, p.length - fn.length - 1)
                        return p
                    }
                    color: delegateItem.ListView.isCurrentItem ? Style.textHighlighted : Style.textTertiary
                    font.pixelSize: Style.fontSizeNormal
                    elide: Text.ElideMiddle
                    verticalAlignment: Text.AlignVCenter
                }
            }

            // Size column
            Item {
                Layout.preferredWidth: listView.sizeColumnWidth
                Layout.fillHeight: true

                Label {
                    anchors.fill: parent
                    anchors.leftMargin: Style.marginSmall
                    anchors.rightMargin: Style.marginSmall
                    text: model.isDir ? "" : Style.formatSize(model.fileSize)
                    color: delegateItem.ListView.isCurrentItem ? Style.textHighlighted : Style.textTertiary
                    font.pixelSize: Style.fontSizeNormal
                    horizontalAlignment: Text.AlignRight
                    verticalAlignment: Text.AlignVCenter
                }
            }

            // Date Modified column
            Item {
                Layout.preferredWidth: listView.dateColumnWidth
                Layout.fillHeight: true

                Label {
                    anchors.fill: parent
                    anchors.leftMargin: Style.marginSmall
                    anchors.rightMargin: Style.marginSmall
                    text: Style.formatDate(model.mtime)
                    color: delegateItem.ListView.isCurrentItem ? Style.textHighlighted : Style.textTertiary
                    font.pixelSize: Style.fontSizeNormal
                    horizontalAlignment: Text.AlignRight
                    verticalAlignment: Text.AlignVCenter
                }
            }

            // Flexible spacer to fill remaining space after all columns
            Item {
                Layout.fillWidth: true
                Layout.fillHeight: true
            }
        }

        background: Rectangle {
            color: delegateItem.ListView.isCurrentItem ? Style.bgSelected
                 : delegateItem.hovered ? Style.bgHover
                 : (index % 2 === 0 ? "transparent" : Style.bgTertiary)
        }
    }

    function moveDown() {
        if (currentIndex < count - 1) {
            currentIndex++
            positionViewAtIndex(currentIndex, ListView.Contain)
        }
    }

    function moveUp() {
        if (currentIndex > 0) {
            currentIndex--
            positionViewAtIndex(currentIndex, ListView.Contain)
        }
    }

    ScrollBar.vertical: ScrollBar {
        active: true
        policy: ScrollBar.AsNeeded
    }
}
