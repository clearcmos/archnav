# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

ArchNav is a fast, keyboard-centric file navigator for KDE Wayland on Arch Linux. It provides instant search across hundreds of thousands of files using trigram indexing, with a Qt/QML GUI.

## Architecture

**Unified Rust application** using cxx-qt 0.8 for Qt/QML bindings:

```
src/
├── main.rs              # Application entry, Qt setup
├── bridge/              # Qt/QML bridge objects (cxx-qt)
│   ├── search_engine.rs # SearchEngine QObject - search, bookmarks, results, context menu
│   └── preview_bridge.rs # PreviewBridge QObject - file preview generation
├── search/              # Core search engine
│   ├── engine.rs        # CoreEngine - owns index, database, background threads
│   ├── trigram.rs       # TrigramIndex - in-memory trigram posting lists
│   ├── database.rs      # SQLite persistence for index + posting list cache
│   ├── scanner.rs       # Directory scanner using walkdir
│   ├── watcher.rs       # inotify file watcher for real-time updates
│   ├── integrity.rs     # Periodic integrity checker + network scanner
│   └── query.rs         # Query parsing (bookmark:, *.ext, sort order)
├── preview/             # Preview generators
│   ├── text.rs          # Text file preview (with size limit)
│   ├── media.rs         # Audio/video metadata via ffprobe
│   ├── archive.rs       # ZIP/TAR contents listing
│   └── directory.rs     # Directory listing
├── config.rs            # JSON config load/save (~/.config/archnav/config.json)
├── ipc.rs               # Unix socket IPC server for toggle command
├── toggle.rs            # Toggle client (--toggle flag)
├── context_menu.rs      # Rust FFI wrapper for C++ context menu
├── context_menu.cpp     # KDE-style right-click context menu (Qt/C++)
├── context_menu.h       # Context menu header
├── system_tray.rs       # Rust FFI wrapper for system tray
├── system_tray.cpp      # QSystemTrayIcon with menu (Qt/C++)
├── system_tray.h        # System tray header
└── qt_app.cpp           # QApplication wrapper for QtWidgets (needed for QMenu)

data/
└── archnav.desktop      # Desktop file with Toggle action for KDE integration

qml/
├── Main.qml             # Main window, keyboard shortcuts, layout
├── SearchBar.qml        # Search input field
├── ResultsList.qml      # File results ListView with delegates
├── PreviewPanel.qml     # Preview pane (text, images, metadata)
├── BookmarkDialog.qml   # Bookmark management dialog
└── Style.qml            # Singleton with colors, fonts, spacing
```

## Development Commands

```bash
# Build
cargo build

# Run
cargo run

# Toggle existing instance (or start new one)
cargo run -- --toggle

# Build release
cargo build --release

# Run with debug logging
RUST_LOG=archnav=debug cargo run
```

## Dependencies

Runtime: `qt6-base qt6-declarative qt6-quickcontrols2 kio kservice kcoreaddons ffmpeg poppler xdg-utils dolphin systemsettings p7zip`

Build: `rust pkg-config`

## Key Technologies

- **cxx-qt 0.8**: Rust-Qt bindings for QObjects and QML integration
- **SQLite (rusqlite)**: Persistent index storage with posting list cache
- **notify 6**: Cross-platform filesystem watcher (inotify on Linux)
- **walkdir**: Fast recursive directory traversal
- **QML/Qt Quick**: Declarative UI with dark theme
- **Qt DBus**: For freedesktop FileManager1 integration (file properties dialog)
- **Qt Widgets**: For native context menus (QMenu) and system tray (QSystemTrayIcon)

## Search Engine

The search engine uses **trigram indexing** for instant substring matching:

1. **Trigrams**: Each filename is split into 3-character overlapping sequences
2. **Posting lists**: Map trigram -> set of file IDs containing it
3. **Search**: Intersect posting lists for all query trigrams, then filter by substring match
4. **Persistence**: SQLite stores files + posting lists; fast load (~2-3s for 760k files)

### Performance Optimizations

- **Posting list cache**: Pre-built posting lists stored in SQLite for instant startup
- **Async watcher setup**: Engine reports "ready" immediately after index load; inotify watches are set up in background thread
- **Count-based cache validation**: Compares cached file count vs actual to detect staleness

## Key Features

- **Instant search**: Sub-10ms search across 600k+ files via trigram index
- **Unified search**: Searches all bookmarks simultaneously by default
- **Bookmark filtering**: `bookmark-name:query` to search within specific bookmark
- **Extension filtering**: `*.py query` to filter by file extension
- **Sort options**: Recent, Oldest, Name A-Z/Z-A, Largest, Smallest, Path
- **Smart previews**: Text, images, audio/video metadata, archive contents
- **Real-time updates**: inotify watches for file changes (local paths only)
- **Network mount support**: Periodic rescanning for network paths (marked `is_network`)
- **Keyboard-centric**: Arrow keys navigate, Enter opens, Esc hides to tray
- **System tray**: Persists in tray when closed, left-click to toggle, right-click for menu
- **Context menu**: Right-click for Open With, Cut, Copy, Rename, Delete, Properties, etc.
- **Zoom support**: Ctrl+=/- to zoom in/out, Ctrl+0 to reset

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Arrow Up/Down` | Navigate results |
| `Enter` | Open file (xdg-open) or folder |
| `Ctrl+O` | Open containing folder |
| `Ctrl+P` | Toggle preview pane |
| `Ctrl+R` | Rescan all bookmarks |
| `Ctrl+=` / `Ctrl++` | Zoom in |
| `Ctrl+-` | Zoom out |
| `Ctrl+0` | Reset zoom |
| `Esc` | Hide to tray |
| `Right-click` | Context menu |

## Data Locations

| Data | Location |
|------|----------|
| Config | `~/.config/archnav/config.json` |
| Index | `~/.local/share/archnav/index.db` |
| Toggle Socket | `$XDG_RUNTIME_DIR/archnav.sock` |

## Config Structure

```json
{
  "bookmarks": [
    {"name": "home", "path": "/home/user", "is_network": false}
  ],
  "exclude_patterns": ["*.pyc", "__pycache__", ".git", "node_modules"],
  "max_results": 500,
  "toggle_hotkey": "Alt+`"
}
```

Note: `toggle_hotkey` is stored for reference but global shortcuts must be configured manually in KDE System Settings.

## Exclude Patterns

Hardcoded in `scanner.rs`:
- `.git`, `node_modules`, `__pycache__`, `.cache`, `.npm`, `.cargo`
- `target`, `build`, `dist`, `.next`, `.nuxt`
- `.Trash*`, `Trash`

## QML/Rust Bridge

The bridge uses cxx-qt macros to expose Rust types to QML:

- **SearchEngine**: Main QObject with properties (`engine_ready`, `result_count`, `status_text`) and methods (`search()`, `initialize()`, `open_file()`, `show_context_menu()`, etc.)
- **PreviewBridge**: Preview QObject with `preview_text`, `preview_type`, `item_count` properties

Signals are defined in Rust and connected in QML (e.g., `onResultsReady`, `onEngine_readyChanged`).

## Context Menu

The context menu (`context_menu.cpp`) provides Dolphin-like file operations:
- Open / Open With (reads from `mimeapps.list`)
- Cut, Copy, Copy Location
- Rename, Delete, Move to Trash
- Open Terminal Here
- Compress (ZIP, TAR.GZ)
- Properties (via freedesktop FileManager1 DBus interface)

## Build System

`build.rs` uses cxx-qt-build to:
1. Register QML module `org.archnav.app` with all QML files
2. Register `Style.qml` as a singleton
3. Compile bridge Rust files into Qt MOC-compatible C++
4. Run MOC on `system_tray.cpp` (contains Q_OBJECT for signal handling)
5. Compile C++ files (`context_menu.cpp`, `qt_app.cpp`, `qt_debug_handler.cpp`, `system_tray.cpp`)
6. Link Qt modules: Qml, Quick, QuickControls2, Widgets, DBus
7. Link KDE Frameworks 6 libraries from standard system paths

## System Tray

The system tray (`system_tray.cpp`) provides:
- **Tray icon**: Uses `system-file-manager` theme icon
- **Left-click**: Toggle window visibility
- **Right-click menu**: Show/Hide ArchNav, Configure Shortcut..., Exit
- **Configure Shortcut**: Opens KDE System Settings (kcm_keys) for manual shortcut setup

Window close behavior:
- Closing via X button or Esc hides the window instead of quitting
- Use "Exit" from tray menu to fully quit the application

## Known Behaviors

- **Max results**: Limited to 2000 items
- **Integrity check**: Every 60s, checks 5000 files per cycle
- **Network mounts**: Rescanned every 5 minutes (no inotify)
- **Wayland**: Cannot set window position; centers on show
