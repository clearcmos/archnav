import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs

import org.archnav.app

Dialog {
    id: dialog
    title: "Manage Bookmarks"
    modal: true
    width: 500
    height: 400
    anchors.centerIn: parent

    property SearchEngine searchEngine: null

    background: Rectangle {
        color: Style.bgSecondary
        border.color: Style.borderDefault
        border.width: 1
        radius: 4
    }

    function refreshBookmarks() {
        bookmarkModel.clear()
        if (!searchEngine) return
        for (var i = 0; i < searchEngine.bookmark_count; i++) {
            bookmarkModel.append({
                name: searchEngine.bookmark_name_at(i),
                path: searchEngine.bookmark_path_at(i)
            })
        }
    }

    onOpened: refreshBookmarks()

    ListModel {
        id: bookmarkModel
    }

    ColumnLayout {
        anchors.fill: parent
        anchors.margins: Style.marginNormal
        spacing: Style.marginNormal

        // Bookmark list
        ListView {
            id: bookmarkList
            Layout.fillWidth: true
            Layout.fillHeight: true
            clip: true
            model: bookmarkModel
            currentIndex: -1

            delegate: ItemDelegate {
                id: bmDelegate
                width: bookmarkList.width
                height: 36

                contentItem: RowLayout {
                    spacing: Style.marginNormal

                    Text {
                        text: model.name
                        color: Style.accentBlue
                        font.family: Style.monoFont
                        font.pixelSize: Style.fontSizeNormal
                        font.bold: true
                        Layout.preferredWidth: 100
                    }

                    Text {
                        text: model.path
                        color: Style.textTertiary
                        font.family: Style.monoFont
                        font.pixelSize: Style.fontSizeSmall
                        elide: Text.ElideMiddle
                        Layout.fillWidth: true
                    }
                }

                background: Rectangle {
                    color: bmDelegate.ListView.isCurrentItem ? Style.bgSelected
                         : bmDelegate.hovered ? Style.bgHover
                         : "transparent"
                }

                onClicked: bookmarkList.currentIndex = index
            }

            Rectangle {
                anchors.fill: parent
                color: "transparent"
                border.color: Style.borderDefault
                border.width: 1
                radius: 3
                z: -1
            }
        }

        // Action buttons
        RowLayout {
            Layout.fillWidth: true
            spacing: Style.marginNormal

            Button {
                text: "Add..."
                onClicked: folderDialog.open()
                palette.buttonText: Style.textSecondary
                background: Rectangle {
                    color: parent.hovered ? Style.buttonHover : Style.buttonBg
                    border.color: Style.buttonBorder
                    border.width: 1
                    radius: 3
                }
            }

            Button {
                text: "Rename"
                enabled: bookmarkList.currentIndex >= 0
                onClicked: renameDialog.open()
                palette.buttonText: Style.textSecondary
                background: Rectangle {
                    color: parent.hovered ? Style.buttonHover : Style.buttonBg
                    border.color: Style.buttonBorder
                    border.width: 1
                    radius: 3
                    opacity: parent.enabled ? 1.0 : 0.5
                }
            }

            Button {
                text: "Delete"
                enabled: bookmarkList.currentIndex >= 0
                onClicked: {
                    var item = bookmarkModel.get(bookmarkList.currentIndex)
                    if (item && searchEngine)
                        searchEngine.remove_bookmark(item.name)
                }
                palette.buttonText: Style.statusRed
                background: Rectangle {
                    color: parent.hovered ? Style.buttonHover : Style.buttonBg
                    border.color: Style.buttonBorder
                    border.width: 1
                    radius: 3
                    opacity: parent.enabled ? 1.0 : 0.5
                }
            }

            Item { Layout.fillWidth: true }

            Button {
                text: "Close"
                onClicked: dialog.close()
                palette.buttonText: Style.textSecondary
                background: Rectangle {
                    color: parent.hovered ? Style.buttonHover : Style.buttonBg
                    border.color: Style.buttonBorder
                    border.width: 1
                    radius: 3
                }
            }
        }

        // Tip text
        Label {
            text: "Tip: Use 'bookmark-name:query' to search within a specific bookmark"
            color: Style.textDim
            font.pixelSize: Style.fontSizeSmall
            Layout.fillWidth: true
        }
    }

    // Folder picker dialog
    FolderDialog {
        id: folderDialog
        title: "Select folder to bookmark"
        onAccepted: {
            var folderPath = selectedFolder.toString().replace("file://", "")
            var name = folderPath.split("/").pop()
            if (name === "") name = "root"
            if (searchEngine) {
                searchEngine.add_bookmark(name, folderPath, false)
            }
        }
    }

    // Rename dialog
    Dialog {
        id: renameDialog
        title: "Rename Bookmark"
        modal: true
        width: 300
        height: 150
        anchors.centerIn: parent

        background: Rectangle {
            color: Style.bgSecondary
            border.color: Style.borderDefault
            border.width: 1
            radius: 4
        }

        ColumnLayout {
            anchors.fill: parent
            anchors.margins: Style.marginNormal
            spacing: Style.marginNormal

            TextField {
                id: renameField
                Layout.fillWidth: true
                placeholderText: "New name"
                placeholderTextColor: Style.textDim
                color: Style.textPrimary
                font.family: Style.monoFont
                background: Rectangle {
                    color: Style.bgPrimary
                    border.color: Style.borderDefault
                    radius: 3
                }
                Component.onCompleted: {
                    if (bookmarkList.currentIndex >= 0) {
                        var item = bookmarkModel.get(bookmarkList.currentIndex)
                        if (item) text = item.name
                    }
                }
            }

            RowLayout {
                Layout.fillWidth: true
                Item { Layout.fillWidth: true }
                Button {
                    text: "Rename"
                    onClicked: {
                        var item = bookmarkModel.get(bookmarkList.currentIndex)
                        if (item && searchEngine && renameField.text.length > 0) {
                            searchEngine.rename_bookmark(item.name, renameField.text)
                            renameDialog.close()
                        }
                    }
                }
                Button {
                    text: "Cancel"
                    onClicked: renameDialog.close()
                }
            }
        }
    }
}
