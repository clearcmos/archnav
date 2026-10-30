import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import QtQuick.Dialogs
import org.kde.kirigami as Kirigami

import org.archnav.app

Dialog {
    id: dialog
    title: "Manage Bookmarks"
    modal: true
    width: Math.round(Kirigami.Units.gridUnit * 28 * Style.zoomFactor)
    height: Math.round(Kirigami.Units.gridUnit * 22 * Style.zoomFactor)
    anchors.centerIn: parent
    standardButtons: Dialog.Close

    property SearchEngine searchEngine: null

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

    // Bookmark mutations run on background threads; refresh when they land
    Connections {
        target: dialog.searchEngine
        function onBookmarksChanged() {
            if (dialog.visible) dialog.refreshBookmarks()
        }
    }

    ListModel {
        id: bookmarkModel
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Style.marginNormal

        // Bookmark list
        Frame {
            Layout.fillWidth: true
            Layout.fillHeight: true
            padding: 0

            ListView {
                id: bookmarkList
                anchors.fill: parent
                clip: true
                model: bookmarkModel
                currentIndex: -1

                delegate: ItemDelegate {
                    id: bmDelegate
                    width: bookmarkList.width
                    highlighted: ListView.isCurrentItem

                    contentItem: ColumnLayout {
                        spacing: 0

                        Label {
                            text: model.name
                            color: bmDelegate.highlighted
                                ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                            font.pointSize: Style.fontSizeNormal
                            font.bold: true
                            textFormat: Text.PlainText
                            Layout.fillWidth: true
                        }

                        Label {
                            text: model.path
                            color: bmDelegate.highlighted
                                ? Kirigami.Theme.highlightedTextColor : Kirigami.Theme.textColor
                            opacity: 0.75
                            font.pointSize: Style.fontSizeSmall
                            elide: Text.ElideMiddle
                            textFormat: Text.PlainText
                            Layout.fillWidth: true
                        }
                    }

                    onClicked: bookmarkList.currentIndex = index
                }

                Kirigami.PlaceholderMessage {
                    parent: bookmarkList
                    anchors.centerIn: parent
                    width: parent.width - Kirigami.Units.gridUnit * 2
                    visible: bookmarkList.count === 0
                    icon.name: "bookmarks"
                    text: "No bookmarks"
                    explanation: "Add a folder to search it."
                }
            }
        }

        // Action buttons
        RowLayout {
            Layout.fillWidth: true
            spacing: Kirigami.Units.mediumSpacing

            Button {
                text: "Add…"
                icon.name: "list-add"
                onClicked: folderDialog.open()
            }

            Button {
                text: "Rename…"
                icon.name: "edit-rename"
                enabled: bookmarkList.currentIndex >= 0
                onClicked: renameDialog.open()
            }

            Button {
                text: "Remove"
                icon.name: "list-remove"
                enabled: bookmarkList.currentIndex >= 0
                onClicked: {
                    var item = bookmarkModel.get(bookmarkList.currentIndex)
                    if (item && searchEngine)
                        searchEngine.remove_bookmark(item.name)
                }
            }

            Item { Layout.fillWidth: true }
        }

        // Tip text
        Label {
            text: "All bookmarks are searched together. Changes are saved to ~/.config/archnav/config.json."
            opacity: 0.75
            font.pointSize: Style.fontSizeSmall
            wrapMode: Text.Wrap
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
        width: Math.round(Kirigami.Units.gridUnit * 18 * Style.zoomFactor)
        anchors.centerIn: parent
        standardButtons: Dialog.Ok | Dialog.Cancel

        // Fill the field with the selected bookmark each time, not only on
        // first creation, so a second rename starts from the right name
        onOpened: {
            var item = bookmarkModel.get(bookmarkList.currentIndex)
            renameField.text = item ? item.name : ""
            renameField.selectAll()
            renameField.forceActiveFocus()
        }

        onAccepted: {
            var item = bookmarkModel.get(bookmarkList.currentIndex)
            if (item && searchEngine && renameField.text.length > 0)
                searchEngine.rename_bookmark(item.name, renameField.text)
        }

        TextField {
            id: renameField
            anchors.left: parent.left
            anchors.right: parent.right
            placeholderText: "New name"
            onAccepted: renameDialog.accept()
        }
    }
}
