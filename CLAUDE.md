# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

archnav is a fast, keyboard-centric file navigator for KDE Wayland on Arch Linux. It provides instant search across hundreds of thousands of files using trigram indexing, with a Qt/QML GUI.

## Architecture

**Unified Rust application** using cxx-qt 0.9 for Qt/QML bindings:

```
src/
├── main.rs              # Application entry, Qt setup
├── bridge/              # Qt/QML bridge objects (cxx-qt)
│   ├── search_engine.rs # SearchEngine QObject - search, bookmarks, results, context menu
│   ├── preview_bridge.rs # PreviewBridge QObject - file preview generation
│   └── tag_bridge.rs    # TagBridge QObject - tagdex tag lookup and editing
├── search/              # Core search engine
│   ├── engine.rs        # CoreEngine - owns index, database, background threads
│   ├── trigram.rs       # TrigramIndex - in-memory trigram posting lists
│   ├── database.rs      # SQLite persistence for index + posting list cache
│   ├── scanner.rs       # Directory scanner using walkdir
│   ├── watcher.rs       # inotify file watcher for real-time updates
│   ├── integrity.rs     # Periodic integrity checker + network scanner
│   └── query.rs         # Query parsing (*.ext, /regex, glob, ~fuzzy, path/aware, folder:)
├── preview/             # Preview generators
│   ├── text.rs          # Text file preview (with size limit)
│   ├── media.rs         # Audio/video metadata via ffprobe
│   ├── archive.rs       # ZIP/TAR contents listing
│   └── directory.rs     # Directory listing
├── tagstore.rs          # Read-only tagdex index parser + tagdex CLI writer (see Tagging)
├── config.rs            # JSON config load/save (~/.config/archnav/config.json)
├── ipc.rs               # Unix socket IPC server for toggle command
├── toggle.rs            # Toggle client (--toggle flag)
├── context_menu.rs      # Rust FFI wrapper for C++ context menu
├── context_menu.cpp     # KDE-style right-click context menu (Qt/C++)
├── context_menu.h       # Context menu header
├── system_tray.rs       # Rust FFI wrapper for system tray
├── system_tray.cpp      # QSystemTrayIcon with menu (Qt/C++)
├── system_tray.h        # System tray header
├── file_opener.rs       # Rust FFI wrapper for KIO file opener
├── file_opener.cpp      # KIO::OpenUrlJob file opener (proper Wayland focus)
├── file_opener.h        # File opener header
├── qt_debug_handler.cpp # Qt message handler redirecting to Rust tracing
└── qt_app.cpp           # QApplication wrapper for QtWidgets (needed for QMenu)

data/
└── archnav.desktop      # Desktop file with Toggle action for KDE integration

qml/
├── Main.qml             # Main window, keyboard shortcuts, layout
├── SearchBar.qml        # Search input field
├── ResultsList.qml      # File results ListView with delegates
├── PreviewPanel.qml     # Preview pane (text, images, audio art, metadata)
├── BookmarkDialog.qml   # Bookmark management dialog (Ctrl+B)
├── TagDialog.qml        # Tag editor dialog (Ctrl+T)
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

# Start hidden in the tray, preloading the index (used by the login autostart entry)
cargo run -- --hidden

# Build release
cargo build --release

# Run with debug logging
RUST_LOG=archnav=debug cargo run

# Test / lint / format (all three are enforced by CI on push and PR)
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt
```

## Dependencies

Runtime: `qt6-base qt6-declarative kio kservice kcoreaddons ffmpeg poppler xdg-utils dolphin systemsettings p7zip`

Build: `rust pkg-config`

## Packaging

A root-level `PKGBUILD` (`pkgname=archnav-git`) pulls from the github remote, builds with `cargo build --release --locked`, and installs the binary, desktop file, and icon. `options=('!lto' '!debug')` is required:
- `!lto`: makepkg's default `-flto=auto` in `CFLAGS` produces GCC LTO bitcode in C/C++ objects that rust-lld cannot resolve. Rust-side LTO stays on via `[profile.release] lto = true`.
- `!debug`: the cargo release profile produces a stripped binary, so makepkg's debug-package split is empty and `gdb-add-index` errors out trying to index symbols that don't exist.

## Key Technologies

- **cxx-qt 0.9**: Rust-Qt bindings for QObjects and QML integration. The `cxx`
  and `cxx-gen` crates stamp their exact version into the generated bridge
  symbols (`cxxbridge1$NNN$...`) and must stay in lockstep, or the final link
  fails with undefined `cxxbridge1$...` symbols. Update them together:
  `cargo update -p cxx -p cxx-gen`
- **SQLite (rusqlite)**: Persistent index storage with posting list cache
- **notify 8**: Cross-platform filesystem watcher (inotify on Linux)
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

## Tagging (tagdex integration)

archnav surfaces file tags from [tagdex](https://github.com/clearcmos/tagdex) stores (a `.tagstore/index.json` at a tree root, designed for filesystems without xattr support such as CIFS NAS mounts).

- **Single-writer principle**: archnav parses `.tagstore/index.json` directly for display (`src/tagstore.rs`, cached by index mtime+size plus a 5s-TTL store-root discovery cache keyed by directory), but every mutation shells out to the `tagdex` CLI so the store lock, atomic index writes, content fingerprints, and xattr mirroring stay in one implementation. Never write the index JSON from archnav.
- **UI**: Tags column in results (rightmost), tags of the selected file in the status bar, Ctrl+T edit dialog (comma-separated, empty clears). The tagdex binary is resolved at `~/.local/bin/tagdex` first (KDE autostart PATH lacks it), `TAGDEX_BIN` overrides.
- **`t:` search filter**: `t: a b` = a OR b; `&` or uppercase `AND` join (`t: a&b`, `t:a AND b`); uppercase `OR` is an explicit separator; lowercase `and` stays a tag name; `t:` alone = any tagged file; text must precede a detached `t:` group. Matching is case-insensitive substring.
- **Engine path**: tag queries invert the search - candidates come from the tag stores (roots discovered via the `.tagstore` path component in the trigram index), so a `t:`-only query costs O(tagged files), not a 600k-file scan. Tag queries bypass the incremental search cache entirely: cache refinement filters with `matches_path()`, which is tag-blind, and would serve wrong results for refined tag queries.

## Key Features

- **Instant search**: Sub-10ms search across 600k+ files via trigram index
- **Unified search**: Searches all bookmarks simultaneously by default
- **Regex search**: `/pattern` for regex matching
- **Fuzzy search**: `~query` for typo-tolerant matching
- **Path-aware search**: `src/config` to match files under specific directories
- **Folders-only filter**: `folder:movies` or `folder: movies` to show directories whose own name matches (folders nested under a matching directory are excluded)
- **Tag filter**: `t:coffee`, `t: coffee outdoor` (OR), `t: coffee&outdoor` / `t:coffee AND outdoor` (AND) - filters by tagdex tags (see Tagging)
- **Extension filtering**: `*.py query` to filter by file extension
- **Sort options**: Recent, Oldest, Name A-Z/Z-A, Largest, Smallest, Path
- **Smart previews**: Text, images, audio/video metadata, archive contents
- **Real-time updates**: inotify watches for file changes (local paths only)
- **Network mount support**: Periodic rescanning for network paths (marked `is_network`)
- **Keyboard-centric**: Arrow keys navigate, Enter opens, Esc hides to tray
- **Bookmark dialog**: Ctrl+B to add/rename/delete bookmarks; changes persist to config.json
- **System tray**: Persists in tray when closed, left-click to toggle, right-click for menu
- **Context menu**: Right-click for Open With, Cut, Copy, Rename, Delete (confirmed), Properties, etc.
- **Zoom support**: Ctrl+=/- to zoom in/out, Ctrl+0 to reset
- **Single instance**: Launching archnav while one is running toggles the existing window (a `--hidden` launch exits quietly instead)

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Arrow Up/Down` | Navigate results |
| `Enter` | Open file (xdg-open) or folder |
| `Ctrl+O` | Open containing folder |
| `Ctrl+P` | Toggle preview pane |
| `Ctrl+R` | Rescan all bookmarks |
| `Ctrl+B` | Manage bookmarks |
| `Ctrl+Shift+F` | Toggle frecency sort |
| `Ctrl+=` / `Ctrl++` | Zoom in |
| `Ctrl+-` | Zoom out |
| `Ctrl+0` | Reset zoom |
| `Esc` | Hide to tray |
| `F1` | Toggle keyboard shortcuts help |
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
  "max_results": 500,
  "toggle_hotkey": "Alt+`",
  "exclude_paths": ["~/Downloads", "/mnt/scratch"]
}
```

`max_results` caps how many results a search returns (default 500, hard cap
2000 via `MAX_RESULTS` in trigram.rs).

`exclude_paths` is an optional list of locations to exclude from indexing,
recursively. Entries are absolute paths with an optional leading `~` (home);
a file is skipped if its path equals or sits under any entry. Edits take
effect on restart - the scanner skips these going forward, and any files
indexed before a path was added are purged on the next launch.

A config file that exists but fails to parse is never overwritten: defaults
are used for that session and a warning is logged, so a hand-editing typo
cannot destroy bookmarks or excludes.

Note: `toggle_hotkey` is stored for reference but global shortcuts must be configured manually in KDE System Settings.

## Exclude Patterns

Built-in name patterns, hardcoded in `trigram.rs` (matched against directory
names only - a plain file that shares one of these names stays indexed - and
against any path component, so a dir of this name is skipped anywhere it
appears):
- `.git`, `node_modules`, `__pycache__`, `.cache`, `.npm`, `.cargo`
- `target`, `build`, `dist`, `.next`, `.nuxt`
- `.Trash`, `Trash`, `.steam`, `dosdevices`

User-configurable recursive locations via `exclude_paths` in `config.json`
(see Config Structure). Both feed the single `should_exclude` chokepoint in
`scanner.rs`, which gates the initial scan, reconcile, network rescans, and the
live inotify watcher. The user list is installed once at startup via
`scanner::set_exclude_paths` and matched by `is_user_excluded`.

## QML/Rust Bridge

The bridge uses cxx-qt macros to expose Rust types to QML:

- **SearchEngine**: Main QObject with properties (`engine_ready`, `result_count`, `status_text`) and methods (`search()`, `initialize()`, `open_file()`, `show_context_menu()`, etc.)
- **PreviewBridge**: Preview QObject with `preview_text`, `preview_type`, `item_count` properties

Signals are defined in Rust and connected in QML (e.g., `onResultsReady`, `onEngine_readyChanged`).

## Context Menu

The context menu (`context_menu.cpp`) provides Dolphin-like file operations:
- Open / Open With (reads from `mimeapps.list`)
- Cut, Copy, Copy Location
- Duplicate Here
- Rename, Delete (permanent, behind a confirmation dialog), Move to Trash
- Move to New Folder
- Open Terminal Here
- Open as Administrator
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
- **Right-click menu**: Show/Hide archnav, Configure Shortcut..., Exit
- **Configure Shortcut**: Opens KDE System Settings (kcm_keys) for manual shortcut setup

Window close behavior:
- Closing via X button or Esc hides the window instead of quitting
- Use "Exit" from tray menu to fully quit the application

## Known Behaviors

- **Max results**: `max_results` from config (default 500), hard cap 2000
- **Integrity check**: Every 60s, checks 5000 files per cycle; entries under a
  bookmark root that is currently unreachable (e.g. an unmounted share) are
  skipped, not purged
- **Network mounts**: Rescanned every 5 minutes (no inotify). A bookmark is
  treated as network if its config `is_network` flag is set or its mount is
  detected as nfs/cifs/sshfs in /proc/mounts (longest mount-point match)
- **Single instance**: A second plain launch toggles the running instance and
  exits; a second `--hidden` launch exits without toggling
- **Wayland**: Cannot set window position; centers on show
