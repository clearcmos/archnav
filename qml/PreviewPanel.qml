import QtQuick
import QtQuick.Controls
import QtQuick.Layouts

import org.archnav.app

Rectangle {
    id: previewRoot
    color: Style.bgSecondary

    property PreviewBridge previewBridge: null
    // Content width for image sizing: panel width - margins - scrollbar
    property int contentWidth: Math.max(200, width - 22)

    StackLayout {
        id: previewStack
        anchors.fill: parent
        anchors.margins: Style.marginSmall

        currentIndex: {
            if (!previewBridge) return 0
            if (previewBridge.is_loading) return 4
            switch (previewBridge.preview_type.toString()) {
                case "text":
                case "directory":
                    return 0
                case "image":
                    return 1
                case "none":
                    return 2
                case "markdown":
                    return 3
                default:
                    return 0
            }
        }

        // Index 0: Text/Directory preview
        ScrollView {
            id: textScroll
            clip: true

            TextArea {
                id: textArea
                readOnly: true
                text: previewBridge ? previewBridge.preview_text : "Select a file to preview"
                color: Style.textPreview
                font.family: Style.monoFont
                font.pixelSize: Style.fontSizePreview
                wrapMode: TextArea.NoWrap
                selectByMouse: true

                background: Rectangle {
                    color: "transparent"
                }
            }
        }

        // Index 1: Image preview
        Item {
            Image {
                id: imagePreview
                anchors.fill: parent
                anchors.margins: Style.marginSmall
                source: previewBridge && previewBridge.image_path.toString() !== ""
                    ? "file://" + previewBridge.image_path
                    : ""
                fillMode: Image.PreserveAspectFit
                asynchronous: true
                cache: false

                BusyIndicator {
                    anchors.centerIn: parent
                    running: imagePreview.status === Image.Loading
                    visible: running
                }
            }
        }

        // Index 2: No preview / empty state
        Item {
            Label {
                anchors.centerIn: parent
                text: "Select a file to preview"
                color: Style.textDim
                font.pixelSize: Style.fontSizeNormal
            }
        }

        // Index 3: Markdown preview (HTML via pulldown-cmark)
        Item {
            id: markdownContainer

            // Debounced width - text only reflows after resize stops
            property int stableWidth: width
            Timer {
                id: widthTimer
                interval: 150
                onTriggered: markdownContainer.stableWidth = markdownContainer.width
            }
            onWidthChanged: widthTimer.restart()
            Component.onCompleted: stableWidth = width

            // Ctrl+wheel zoom
            MouseArea {
                anchors.fill: parent
                z: 1
                acceptedButtons: Qt.NoButton
                onWheel: function(wheel) {
                    if (wheel.modifiers & Qt.ControlModifier) {
                        wheel.angleDelta.y > 0 ? Style.zoomIn() : Style.zoomOut()
                        wheel.accepted = true
                    } else {
                        wheel.accepted = false
                    }
                }
            }

            Flickable {
                id: markdownFlickable
                anchors.fill: parent
                clip: true
                contentHeight: markdownText.implicitHeight
                boundsBehavior: Flickable.StopAtBounds

                ScrollBar.vertical: ScrollBar {
                    id: markdownScrollBar
                    policy: ScrollBar.AsNeeded
                }

                Text {
                    id: markdownText
                    width: markdownContainer.stableWidth - 14
                    text: previewBridge ? previewBridge.preview_text : ""
                    textFormat: Text.RichText
                    color: Style.textPreview
                    font.family: Style.defaultFont
                    font.pixelSize: Style.fontSizeNormal
                    wrapMode: Text.Wrap
                    onLinkActivated: function(link) {
                        Qt.openUrlExternally(link)
                    }
                }
            }
        }

        // Index 4: Loading state
        Item {
            BusyIndicator {
                anchors.centerIn: parent
                running: true
            }
        }
    }
}
