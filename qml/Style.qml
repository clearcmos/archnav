pragma Singleton
import QtQuick
import org.kde.kirigami as Kirigami

// Shared look for archnav's QML. Colors, fonts, and spacing all come from the
// KDE theme (Kirigami and the system palette), so the window follows the
// user's color scheme and font settings; archnav's own zoom (Ctrl+=/-) only
// scales them. Nothing here is a fixed color, font size, or pixel spacing.
QtObject {
    id: style

    // User-adjustable zoom factor (1.0 = 100%)
    property real zoomFactor: 1.0

    // An unparented item to read Kirigami's attached theme from: a singleton
    // QtObject has no item of its own for the attached property to bind to.
    readonly property Item themeProbe: Item {
        Kirigami.Theme.colorSet: Kirigami.Theme.View
        Kirigami.Theme.inherit: false
    }

    // System palette - follows the KDE color scheme
    readonly property SystemPalette palette: SystemPalette { colorGroup: SystemPalette.Active }
    readonly property SystemPalette disabledPalette: SystemPalette { colorGroup: SystemPalette.Disabled }

    // Backgrounds
    readonly property color bgPrimary: palette.window
    readonly property color bgSecondary: palette.base

    // Text
    readonly property color textPrimary: palette.windowText
    readonly property color textSecondary: palette.text
    readonly property color textPreview: palette.text
    readonly property color textDim: disabledPalette.text
    readonly property color textHighlighted: palette.highlightedText

    // Accent and status (status colors carry meaning, so they are paired
    // with text wherever they appear)
    readonly property color accentBlue: palette.highlight
    readonly property color statusOrange: themeProbe.Kirigami.Theme.neutralTextColor
    readonly property color statusGreen: themeProbe.Kirigami.Theme.positiveTextColor
    readonly property color statusRed: themeProbe.Kirigami.Theme.negativeTextColor

    // Fonts, in points, from the system font settings
    readonly property real fontSizeSmall: themeProbe.Kirigami.Theme.smallFont.pointSize * zoomFactor
    readonly property real fontSizeNormal: themeProbe.Kirigami.Theme.defaultFont.pointSize * zoomFactor
    readonly property real fontSizeLarge: themeProbe.Kirigami.Theme.defaultFont.pointSize * 1.2 * zoomFactor
    readonly property real fontSizePreview: themeProbe.Kirigami.Theme.fixedWidthFont.pointSize * zoomFactor
    readonly property string defaultFont: themeProbe.Kirigami.Theme.defaultFont.family
    readonly property string monoFont: themeProbe.Kirigami.Theme.fixedWidthFont.family

    // Spacing
    readonly property int marginSmall: Math.round(Kirigami.Units.smallSpacing * zoomFactor)
    readonly property int marginNormal: Math.round(Kirigami.Units.largeSpacing * zoomFactor)
    readonly property int marginLarge: Math.round((Kirigami.Units.smallSpacing + Kirigami.Units.largeSpacing) * zoomFactor)

    // Table/list dimensions, in grid units so they grow with the font
    readonly property int rowHeight: Math.round(Kirigami.Units.gridUnit * 1.6 * zoomFactor)
    readonly property int headerHeight: Math.round(Kirigami.Units.gridUnit * 1.75 * zoomFactor)
    // 22 px: Breeze draws file-type icons in color from this size up
    readonly property int iconSize: Math.round(Kirigami.Units.iconSizes.smallMedium * zoomFactor)
    readonly property int sortIconSize: Math.round(Kirigami.Units.iconSizes.small * zoomFactor)

    // Timing
    readonly property int searchDebounceMs: 100

    // Zoom limits
    readonly property real minZoom: 0.75
    readonly property real maxZoom: 2.0
    readonly property real zoomStep: 0.1

    function zoomIn() {
        zoomFactor = Math.min(maxZoom, zoomFactor + zoomStep)
    }

    function zoomOut() {
        zoomFactor = Math.max(minZoom, zoomFactor - zoomStep)
    }

    function resetZoom() {
        zoomFactor = 1.0
    }

    // Helper to format file size
    function formatSize(bytes) {
        if (bytes < 0) return ""
        if (bytes < 1024) return bytes + " B"
        if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + " KiB"
        if (bytes < 1024 * 1024 * 1024) return (bytes / (1024 * 1024)).toFixed(1) + " MiB"
        return (bytes / (1024 * 1024 * 1024)).toFixed(1) + " GiB"
    }

    // Helper to format timestamp
    function formatDate(timestamp) {
        if (timestamp <= 0) return ""
        var d = new Date(timestamp * 1000)
        var now = new Date()

        // Today: show time only
        if (d.toDateString() === now.toDateString()) {
            return d.toLocaleTimeString(Qt.locale(), "h:mm ap")
        }
        // This year: show date without year
        if (d.getFullYear() === now.getFullYear()) {
            return d.toLocaleDateString(Qt.locale(), "MMM d") + " " + d.toLocaleTimeString(Qt.locale(), "h:mm ap")
        }
        // Older: show full date
        return d.toLocaleDateString(Qt.locale(), "yyyy-MM-dd") + " " + d.toLocaleTimeString(Qt.locale(), "h:mm ap")
    }
}
