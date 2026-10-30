import QtQuick
import QtQuick.Controls
import QtQuick.Layouts
import org.kde.kirigami as Kirigami

import org.archnav.app

// Ctrl+T tag editor for the selected file. Tags are comma-separated in the
// field; saving replaces the file's full tag set via the tag store.
Dialog {
    id: dialog
    title: "Edit Tags"
    modal: true
    width: Math.round(Kirigami.Units.gridUnit * 26 * Style.zoomFactor)
    anchors.centerIn: parent

    property TagBridge tagBridge: null
    property string filePath: ""
    property string fileName: ""

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

    // As the contentItem (not a child filling it) so the dialog grows when
    // an inline message appears instead of letting it run under the buttons
    contentItem: ColumnLayout {
        spacing: Style.marginNormal

        Label {
            text: dialog.fileName
            opacity: 0.75
            font.pointSize: Style.fontSizeSmall
            elide: Text.ElideMiddle
            textFormat: Text.PlainText
            Layout.fillWidth: true
        }

        TextField {
            id: tagField
            Layout.fillWidth: true
            enabled: !(tagBridge && tagBridge.is_saving)
            placeholderText: "tag1, tag2, tag3 (empty clears all tags)"
            font.pointSize: Style.fontSizeNormal
            Keys.onReturnPressed: dialog.save()
            Keys.onEnterPressed: dialog.save()
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            type: Kirigami.MessageType.Warning
            visible: tagBridge && !tagBridge.has_store && tagBridge.error_text.toString() === ""
            text: "No tag store found above this file. Run: archnav tag init <tree root>"
        }

        Kirigami.InlineMessage {
            Layout.fillWidth: true
            type: Kirigami.MessageType.Error
            visible: tagBridge && tagBridge.error_text.toString() !== ""
            text: tagBridge ? tagBridge.error_text : ""
        }
    }

    footer: DialogButtonBox {
        Button {
            text: tagBridge && tagBridge.is_saving ? "Saving…" : "Save"
            icon.name: "document-save"
            enabled: !(tagBridge && tagBridge.is_saving)
            DialogButtonBox.buttonRole: DialogButtonBox.ApplyRole
            onClicked: dialog.save()
        }
        Button {
            text: "Cancel"
            icon.name: "dialog-cancel"
            DialogButtonBox.buttonRole: DialogButtonBox.RejectRole
        }
    }
}
