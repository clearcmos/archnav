import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import org.archnav.app

// Ctrl+T tag editor for the selected file. Tags are comma-separated in the
// field; saving replaces the file's full tag set via the tagdex CLI.
Dialog {
    id: dialog
    title: "Edit Tags"
    modal: true
    width: Math.round(460 * Style.zoomFactor)
    anchors.centerIn: parent

    property TagBridge tagBridge: null
    property string filePath: ""
    property string fileName: ""

    background: Rectangle {
        color: Style.bgSecondary
        border.color: Style.borderDefault
        border.width: 1
        radius: 4
    }

    function save() {
        if (!tagBridge || tagBridge.is_saving) return
        tagBridge.save_tags(filePath, tagField.text)
    }

    onOpened: {
        tagField.text = tagBridge ? tagBridge.tags : ""
        tagField.forceActiveFocus()
        tagField.selectAll()
    }

    Connections {
        target: dialog.tagBridge
        function onTagsSaved() {
            if (dialog.visible) dialog.close()
        }
    }

    ColumnLayout {
        anchors.fill: parent
        spacing: Style.marginNormal

        Label {
            text: dialog.fileName
            color: Style.textDim
            font.family: Style.monoFont
            font.pixelSize: Style.fontSizeSmall
            elide: Text.ElideMiddle
            Layout.fillWidth: true
        }

        TextField {
            id: tagField
            Layout.fillWidth: true
            enabled: !(tagBridge && tagBridge.is_saving)
            placeholderText: "tag1, tag2, tag3 (empty clears all tags)"
            placeholderTextColor: Style.textDim
            color: Style.textPrimary
            font.family: Style.monoFont
            font.pixelSize: Style.fontSizeNormal
            background: Rectangle {
                color: Style.bgPrimary
                border.color: tagField.activeFocus ? Style.borderFocus : Style.borderDefault
                radius: 3
            }
            Keys.onReturnPressed: dialog.save()
            Keys.onEnterPressed: dialog.save()
        }

        Label {
            visible: tagBridge && !tagBridge.has_store && tagBridge.error_text.toString() === ""
            text: "No tag store found above this file. Run: tagdex init <tree root>"
            color: Style.statusOrange
            font.pixelSize: Style.fontSizeSmall
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        Label {
            visible: tagBridge && tagBridge.error_text.toString() !== ""
            text: tagBridge ? tagBridge.error_text : ""
            color: Style.statusRed
            font.pixelSize: Style.fontSizeSmall
            wrapMode: Text.Wrap
            Layout.fillWidth: true
        }

        RowLayout {
            Layout.fillWidth: true
            spacing: Style.marginNormal

            Item { Layout.fillWidth: true }

            Button {
                text: tagBridge && tagBridge.is_saving ? "Saving..." : "Save"
                enabled: !(tagBridge && tagBridge.is_saving)
                onClicked: dialog.save()
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
                text: "Cancel"
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
    }
}
