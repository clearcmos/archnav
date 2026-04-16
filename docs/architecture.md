# Architecture

## Overview

ArchNav is a unified Rust application with Qt/QML GUI using cxx-qt for bindings. It provides instant file search across large filesystems via trigram indexing.

The architecture prioritizes:
- **Instant response** - Trigram index enables sub-10ms search
- **Fast startup** - Posting list cache in SQLite, async watcher setup
- **Real-time updates** - inotify for local paths, periodic scan for network mounts
- **Single binary** - No separate daemon; everything in one process

## Component Diagram

```
┌─────────────────────────────────────────────────────────────────────────────────┐
│                           archnav (Rust + Qt/QML)                                 │
│                                                                                   │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │                        Qt/QML UI Layer (qml/)                                │ │
│  │  Main.qml ─── SearchBar.qml ─── ResultsList.qml ─── PreviewPanel.qml        │ │
│  │      │                                                                       │ │
│  │      └── BookmarkDialog.qml ─── Style.qml (singleton)                        │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                    │                                              │
│                         QObject properties/signals                               │
│                                    │                                              │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │                      Bridge Layer (src/bridge/)                              │ │
│  │  SearchEngine ─────────────────────────── PreviewBridge                     │ │
│  │  - engine_ready, result_count            - preview_text, preview_type       │ │
│  │  - search(), initialize()                - request_preview()                │ │
│  │  - open_file(), rescan_all()             - clear_preview()                  │ │
│  │  - show_context_menu()                   - Signals: previewReady            │ │
│  │  - Signals: resultsReady,                                                   │ │
│  │             bookmarksChanged                                                 │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                    │                                              │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │                     Core Engine (src/search/)                                │ │
│  │                                                                               │ │
│  │  ┌────────────────┐  ┌────────────────┐  ┌────────────────┐                  │ │
│  │  │ TrigramIndex   │  │    Database    │  │   FileWatcher  │                  │ │
│  │  │                │  │   (SQLite)     │  │   (inotify)    │                  │ │
│  │  │ trigrams: Map  │  │                │  │                │                  │ │
│  │  │ files: Map     │  │ ~/.local/share │  │ Real-time      │                  │ │
│  │  │ path_to_id     │  │ /archnav/       │  │ file changes   │                  │ │
│  │  │                │  │ index.db       │  │ (async setup)  │                  │ │
│  │  └────────────────┘  └────────────────┘  └────────────────┘                  │ │
│  │         │                    │                   │                            │ │
│  │         └────────────────────┴───────────────────┘                            │ │
│  │                              │                                                 │ │
│  │  ┌────────────────┐  ┌────────────────┐                                      │ │
│  │  │ IntegrityCheck │  │ NetworkScanner │  Background threads                  │ │
│  │  │ (every 60s)    │  │ (every 5min)   │                                      │ │
│  │  └────────────────┘  └────────────────┘                                      │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                   │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │                     Preview System (src/preview/)                            │ │
│  │  text.rs ── media.rs (ffprobe) ── archive.rs (zip/tar) ── directory.rs      │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
│                                                                                   │
│  ┌─────────────────────────────────────────────────────────────────────────────┐ │
│  │                     IPC (src/ipc.rs, src/toggle.rs)                          │ │
│  │  Unix socket server at $XDG_RUNTIME_DIR/archnav.sock                          │ │
│  │  Accepts "toggle" command to show/hide window                                │ │
│  └─────────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────────┘
```

## Trigram Index

The search engine uses trigram posting lists for instant substring matching:

```
Query: "test"
Trigrams: ["tes", "est"]

Posting lists (in memory):
  "tes" -> {file_id_1, file_id_5, file_id_99, ...}
  "est" -> {file_id_1, file_id_12, file_id_99, ...}

Intersection: {file_id_1, file_id_99}  <- candidates
Final filter: verify actual substring match in filename
```

This approach (used by plocate, Google Code Search) enables O(1) candidate lookup regardless of index size.

### Persistence

SQLite stores:
- `files` table: id, path, is_dir, mtime, size
- `file_trigrams` table: file_id, trigrams (blob)
- `posting_lists` table: trigram (blob), file_ids (blob)
- `meta` table: posting_lists_count (for cache validation)

On startup:
1. Check if `posting_lists_count` matches actual file count
2. If match: load posting lists directly (fast path, ~2-3s for 760k files)
3. If mismatch: rebuild posting lists from per-file trigrams (slow path, ~7s)

## Threading Model

```
Main Thread (Qt event loop)
    │
    ├── QML UI rendering
    │
    └── SearchEngine QObject
            │
            ├── CoreEngine (created in background thread)
            │       │
            │       ├── Database load (blocking, ~2-3s)
            │       │
            │       └── Background thread (spawned after load)
            │               ├── inotify Watcher setup (slow, 5-16s)
            │               ├── Integrity Checker (every 60s)
            │               └── Network Scanner (every 5min)
            │
            └── Search (read lock on index, very fast)

Database Writer Thread
    └── Serializes all SQLite writes via channel
```

Key optimization: Engine reports "ready" immediately after index load. Watcher setup continues in background, so users can search while watchers are still being configured.

## Query Parsing

The `query.rs` module parses search strings:

```
Input: "home:*.rs config"

Parsed:
  - bookmark_path: "/home/user" (from bookmark named "home")
  - extension: "rs"
  - terms: ["config"]
  - sort_order: Recent (default)
```

Supports:
- `bookmark:query` - filter to specific bookmark
- `*.ext query` - filter by extension
- Sort options: Recent, Oldest, Name A-Z/Z-A, Largest, Smallest, Path

## Smart Preview System

Preview type detection (in Rust):

```rust
match extension {
    "mp3" | "flac" | "ogg" | ... => PreviewType::Audio,
    "mp4" | "mkv" | "avi" | ... => PreviewType::Video,
    "zip" | "tar" | "gz" | ...  => PreviewType::Archive,
    "png" | "jpg" | "gif" | ... => PreviewType::Image,
    _ if is_binary(path)        => PreviewType::Binary,
    _                           => PreviewType::Text,
}
```

Preview handlers:
- **text**: Read first 50KB of file
- **image**: Return path for QML Image element
- **audio/video**: Run `ffprobe -print_format json` to extract metadata
- **archive**: Use `zip` or `tar` crate to list contents
- **directory**: List directory contents with `std::fs::read_dir`

## IPC Protocol

Single socket for window toggle:

```
Path: $XDG_RUNTIME_DIR/archnav.sock
Protocol: Raw bytes
Command: "toggle" -> Show/hide window
```

The `--toggle` flag sends this command to existing instance, or starts a new one if none running.

## Startup Sequence

1. Check `--toggle` flag; if set and instance exists, send toggle and exit
2. Initialize Qt application and QML engine
3. Load QML from embedded QRC resources
4. SearchEngine::initialize() spawns background thread:
   a. Open SQLite database
   b. Load index (fast path if cache valid)
   c. Report "engine ready" via signal
   d. Spawn async thread for watcher setup + integrity checker
5. User can search immediately after step 4c

## Exclude Patterns

Hardcoded in `scanner.rs`:

```rust
const EXCLUDE_DIRS: &[&str] = &[
    ".git", "node_modules", "__pycache__", ".cache", ".npm", ".cargo",
    "target", "build", "dist", ".next", ".nuxt",
];

const EXCLUDE_PATTERNS: &[&str] = &[".Trash", "Trash"];
```

## Context Menu

The context menu (`src/context_menu.cpp`) provides Dolphin-style file operations:

```
┌─────────────────────────────────────────┐
│ QML (ResultsList.qml)                   │
│   onContextMenuRequested(path, x, y)    │
│               │                         │
└───────────────│─────────────────────────┘
                │
┌───────────────▼─────────────────────────┐
│ Rust (search_engine.rs)                 │
│   show_context_menu(path, x, y)         │
│               │                         │
└───────────────│─────────────────────────┘
                │ FFI call
┌───────────────▼─────────────────────────┐
│ C++ (context_menu.cpp)                  │
│   ContextMenuHandler::showContextMenu() │
│   - QMenu with QActions                 │
│   - Reads mimeapps.list for "Open With" │
│   - DBus for Properties dialog          │
└─────────────────────────────────────────┘
```

Uses Qt Widgets (QMenu) for native context menu appearance.
Uses Qt DBus for freedesktop FileManager1.ShowItemProperties (works on network mounts).

## Build System

`build.rs` uses cxx-qt-build to:
1. Register QML module `org.archnav.app`
2. Register all QML files, with Style.qml as singleton
3. Compile Rust bridge files to MOC-compatible C++
4. Compile C++ files: `context_menu.cpp`, `qt_app.cpp`, `qt_debug_handler.cpp`
5. Link Qt modules: Qml, Quick, QuickControls2, Widgets, DBus
6. Link KDE Frameworks 6 libraries from standard system paths
