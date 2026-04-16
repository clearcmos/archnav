import QtQuick
import QtQuick.Controls

TextField {
    id: searchField
    placeholderText: "Search files..."
    placeholderTextColor: Style.textDim
    font.pixelSize: Style.fontSizeLarge
    color: Style.textPrimary
    padding: Style.marginNormal
    leftPadding: Style.marginLarge
    selectByMouse: true

    background: Rectangle {
        color: Style.bgSecondary
        border.color: searchField.activeFocus ? Style.borderFocus : Style.borderDefault
        border.width: searchField.activeFocus ? 2 : 1
        radius: 4
    }
}
