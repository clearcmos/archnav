import QtQuick
import QtQuick.Controls

ApplicationWindow {
    width: 400
    height: 300
    visible: true
    title: "Test Window"
    color: "#141414"

    Label {
        anchors.centerIn: parent
        text: "Hello from archnav!"
        color: "white"
        font.pixelSize: 24
    }

    Component.onCompleted: console.log("QML: Window completed!")
}
