pragma Singleton
import QtQuick

QtObject {
    id: style

    // User-adjustable zoom factor (1.0 = 100%)
    property real zoomFactor: 1.0

    // System palette - follows KDE/Qt theme automatically
    readonly property SystemPalette palette: SystemPalette { colorGroup: SystemPalette.Active }
    readonly property SystemPalette disabledPalette: SystemPalette { colorGroup: SystemPalette.Disabled }

    // Background colors (from system palette)
    readonly property color bgPrimary: palette.window
    readonly property color bgSecondary: palette.base
    readonly property color bgTertiary: palette.alternateBase
    readonly property color bgList: palette.base
    readonly property color bgSelected: palette.highlight
    readonly property color bgSelectedHover: Qt.lighter(palette.highlight, 1.1)
    readonly property color bgHover: Qt.lighter(palette.base, 1.15)

    // Text colors (from system palette)
    readonly property color textPrimary: palette.windowText
    readonly property color textSecondary: palette.text
    readonly property color textPreview: palette.text
    readonly property color textTertiary: Qt.darker(palette.text, 1.5)
    readonly property color textDim: disabledPalette.text
    readonly property color textMuted: disabledPalette.windowText
    readonly property color textHelp: disabledPalette.text
    readonly property color textHighlighted: palette.highlightedText

    // Accent colors
    readonly property color accentBlue: palette.highlight
    readonly property color borderDefault: Qt.darker(palette.window, 1.3)
    readonly property color borderFocus: palette.highlight
    readonly property color buttonBg: palette.button
    readonly property color buttonBorder: Qt.darker(palette.button, 1.3)
    readonly property color buttonHover: Qt.lighter(palette.button, 1.1)
    readonly property color buttonText: palette.buttonText

    // Header colors
    readonly property color headerBg: Qt.lighter(palette.window, 1.08)
    readonly property color headerText: palette.windowText
    readonly property color headerBorder: Qt.rgba(1, 1, 1, 0.15)  // Semi-transparent white for visible separators

    // Status colors (keep fixed for visibility)
    readonly property color statusOrange: "#e6a855"
    readonly property color statusGreen: "#66bb6a"
    readonly property color statusRed: "#ef5350"

    // Base font sizes (before zoom)
    readonly property int baseFontSizeSmall: 11
    readonly property int baseFontSizeNormal: 13
    readonly property int baseFontSizeLarge: 15
    readonly property int baseFontSizePreview: 12

    // Scaled font sizes
    readonly property int fontSizeSmall: Math.round(baseFontSizeSmall * zoomFactor)
    readonly property int fontSizeNormal: Math.round(baseFontSizeNormal * zoomFactor)
    readonly property int fontSizeLarge: Math.round(baseFontSizeLarge * zoomFactor)
    readonly property int fontSizePreview: Math.round(baseFontSizePreview * zoomFactor)

    // Fonts - use system default
    readonly property string defaultFont: Qt.application.font.family
    readonly property string monoFont: "monospace"

    // Base spacing (before zoom)
    readonly property int baseMarginSmall: 4
    readonly property int baseMarginNormal: 8
    readonly property int baseMarginLarge: 12

    // Scaled spacing
    readonly property int marginSmall: Math.round(baseMarginSmall * zoomFactor)
    readonly property int marginNormal: Math.round(baseMarginNormal * zoomFactor)
    readonly property int marginLarge: Math.round(baseMarginLarge * zoomFactor)

    // Table/list dimensions (scaled)
    readonly property int rowHeight: Math.round(28 * zoomFactor)
    readonly property int headerHeight: Math.round(32 * zoomFactor)

    // Timing
    readonly property int searchDebounceMs: 100
    readonly property int resizeDebounceMs: 150

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
        return d.toLocaleDateString(Qt.locale(), "yyyy-MM-dd h:mm ap")
    }
}
