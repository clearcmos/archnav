import QtQuick
import org.kde.kirigami as Kirigami

// Kirigami's search field: Breeze frame, search icon, and a clear button.
// Main runs the search on its own debounce, so the field's accepted signal
// is not used.
Kirigami.SearchField {
    placeholderText: "Search files"
    font.pointSize: Style.fontSizeLarge
    autoAccept: false
    focusSequence: ""
}
