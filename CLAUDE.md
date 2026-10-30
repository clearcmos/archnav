# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

archnav is a keyboard-centric file navigator for KDE Wayland on Arch Linux. It searches every path under the configured bookmarks through an in-memory trigram index, with a Qt/QML GUI.

## Architecture

**Unified Rust application** using cxx-qt 0.9 for Qt/QML bindings:

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
│   └── query.rs         # Query parsing (words in any order, !exclude, "phrase", size:, dm:, *.ext, /regex, glob, ~fuzzy, path/aware, folder:)
├── preview/             # Preview generators
│   ├── mod.rs           # Preview dispatch by extension (images, PDF info, binaries)
│   ├── text.rs          # Text file preview (with size limit), Markdown to HTML
│   ├── media.rs         # Audio/video metadata via ffprobe
│   ├── archive.rs       # ZIP/TAR contents listing
│   └── directory.rs     # Directory listing
├── config.rs            # JSON config load/save (~/.config/archnav/config.json)
├── ipc.rs               # Line-protocol IPC server for scripts ($XDG_RUNTIME_DIR/archnav-daemon.sock)
├── toggle.rs            # Single-instance toggle socket: server in the app, client for --toggle
├── context_menu.rs      # Rust FFI wrapper for C++ context menu
├── context_menu.cpp     # KDE-style right-click context menu (Qt/C++)
├── context_menu.h       # Context menu header
├── system_tray.rs       # Rust FFI wrapper for system tray
├── system_tray.cpp      # QSystemTrayIcon with menu (Qt/C++)
├── system_tray.h        # System tray header
├── file_opener.rs       # Rust FFI wrapper for KIO file opener
├── file_opener.cpp      # KIO::OpenUrlJob file opener (proper Wayland focus)
├── file_opener.h        # File opener header
├── mime_icon.rs         # Rust FFI wrapper: theme icon name for a result's file type
├── mime_icon.cpp        # QMimeDatabase name-only lookup + QIcon theme check
├── mime_icon.h          # MIME icon header
├── qt_debug_handler.cpp # Qt message handler redirecting to Rust tracing
└── qt_app.cpp           # QApplication wrapper for QtWidgets (needed for QMenu)

data/
└── archnav.desktop      # Desktop file with Toggle action for KDE integration

qml/
├── Main.qml             # Main window, keyboard shortcuts, layout
├── SearchBar.qml        # Kirigami.SearchField
├── ResultsList.qml      # Column header + results list (columns, sort arrows, path links, placeholder)
├── PreviewPanel.qml     # Preview pane (text, Markdown, images, audio art, metadata)
├── BookmarkDialog.qml   # Bookmark management dialog (Ctrl+B)
└── Style.qml            # Singleton: theme colors, fonts, spacing scaled by zoom
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

# Headless search against the real index, and a search benchmark
cargo run --release -- --test-search "query one" "query two"
cargo run --release -- --benchmark --iterations 100

# Test / lint / format (all enforced by CI on push and PR)
cargo test
cargo clippy --all-targets -- -D warnings
cargo fmt

# Coverage (gated in CI; env vars point cargo-llvm-cov at the system LLVM,
# needed because rust is installed via pacman rather than rustup)
LLVM_COV=/usr/bin/llvm-cov LLVM_PROFDATA=/usr/bin/llvm-profdata cargo llvm-cov --summary-only
```

## Dependencies

Runtime: `qt6-base qt6-declarative kirigami qqc2-desktop-style kio kservice kcoreaddons ffmpeg poppler xdg-utils dolphin systemsettings p7zip`

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
- **walkdir**: Recursive directory traversal
- **QML/Qt Quick + Kirigami**: Declarative UI on the Breeze desktop style (see UI: Breeze)
- **Qt DBus**: For freedesktop FileManager1 integration (file properties dialog)
- **Qt Widgets**: For native context menus (QMenu) and system tray (QSystemTrayIcon)

## Search Engine

The search engine uses **trigram indexing** for substring matching:

1. **Trigrams**: Each filename is split into 3-character overlapping sequences
2. **Posting lists**: Map trigram -> set of file IDs containing it
3. **Search**: Intersect the posting lists of every query word's trigrams (walking the shortest list and checking the others), then filter the candidates on all cores (rayon) and keep the top `max_results` with a linear selection before sorting; the full match count is reported alongside. A query with no trigram to narrow with (`ab`, `*.rs` alone, `size:>1G` alone) skips candidate collection and scans every entry in parallel
4. **Persistence**: SQLite stores files + posting lists; fast load in about 2-5 s for 1.4M files

### Performance Optimizations

- **Posting list cache**: Pre-built posting lists stored in SQLite, so startup loads them instead of rebuilding
- **Async watcher setup**: Engine reports "ready" immediately after index load; inotify watches are set up in background thread
- **Count-based cache validation**: Compares cached file count vs actual to detect staleness
- **Write only what changed**: every scan path (initial scan, reconcile, network rescan, inotify) goes through `scanner::index_path`. An entry whose `is_dir`/mtime/size match the index costs one stat and a read-locked lookup, with no write lock and no DB op; changed metadata is an in-place `UPDATE` (`DbOp::UpdateFile`), and only new paths are inserted. Re-saving every entry on each 5-minute NAS rescan used to write about 500 MiB per pass.
- **No stored per-file trigrams**: trigrams are a pure function of the path, so the slow load and the fast-load backfill recompute them (about 3 s for 1.4M paths). `Database::open_at` drops the legacy `file_trigrams` table and the unused `idx_files_path` (a duplicate of the `UNIQUE` index) and `idx_files_mtime`, then VACUUMs once. `journal_size_limit` caps the WAL at 64 MiB after checkpoints.

## Engineering Standard (tier 2: public repo)

- **Tests**: every pure-logic module carries an in-module `#[cfg(test)]` block. Runtime-bound modules are exempt from direct tests and excluded from the coverage gate: `bridge/*` (Qt objects need a running QML engine), `main.rs` (entry point), `ipc.rs` and `toggle.rs` (live unix socket + engine), `search/watcher.rs` (inotify threads), `search/integrity.rs` (periodic threads), `context_menu`/`system_tray`/`file_opener`/`mime_icon`/`qt_*` (C++/FFI glue). `preview/media.rs` splits parse-vs-run: JSON formatting is tested against captured ffprobe output; the subprocess paths are exempt.
- **Coverage**: cargo-llvm-cov, gated in CI over the non-exempt set (`--ignore-filename-regex` in ci.yml mirrors the exemption list above). Gate: 70% lines (introduced at 60 with 66% measured, 2026-07-19; raised to 70 with 74% measured, 2026-07-20). Ratchets upward, never down. Biggest headroom: `search/engine.rs` (thread/db orchestration).
- **Supply chain**: Cargo.lock committed; cargo dependency updates are deliberate and manual, advisory-driven (see the chore(deps) commit trail). dependabot only maintains the pinned GitHub Actions SHAs.
- **Changelog**: none; git history is the record.
- **License**: MIT (LICENSE file; matches Cargo.toml).

## Key Features

- **Trigram search**: measured with `--benchmark` on a 1.4M-file index (i7-13700K, 24 threads): most queries under 1 ms, very common words (`config`, 146,630 matches) about 13 ms, and queries without a 3-character run (`ab`, `*.rs` alone) about 20 ms on all cores. Don't quote a single latency figure; it depends on the query
- **Words, exclusions, filters**: plain words must all appear, in any order; `"..."` keeps a phrase; `!word` excludes; `size:` and `dm:` filter on size (files only) and modification time. Regex queries (`/...`) keep `!`, quotes, and `size:` literal. The result cache (`SearchCache`) reuses earlier results only when `ParsedQuery::narrows` proves the new query can only remove matches: every old word inside a new word, every old `!word` containing a new one, and identical filters
- **Unified search**: Searches all bookmarks simultaneously by default
- **Regex search**: `/pattern` for regex matching
- **Fuzzy search**: `~query` for typo-tolerant matching
- **Path-aware search**: `src/config` to match files under specific directories
- **Folders-only filter**: `folder:movies` or `folder: movies` to show directories whose own name matches (folders nested under a matching directory are excluded)
- **Extension filtering**: `*.py query` to filter by file extension
- **Sort options**: Recent, Oldest, Name A-Z/Z-A, Largest, Smallest, Path
- **Previews**: Text, rendered Markdown, images, PDF and audio/video metadata, archive contents
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
| `Ctrl+Scroll` | Zoom in / out |
| `Ctrl+0` | Reset zoom |
| `Esc` | Hide to tray |
| `F1` | Toggle keyboard shortcuts help |
| `Right-click` | Context menu |

## Data Locations

| Data | Location |
|------|----------|
| Config | `~/.config/archnav/config.json` |
| Index | `~/.local/share/archnav/index.db` |
| Toggle socket | `$XDG_RUNTIME_DIR/archnav.sock` |
| IPC socket | `$XDG_RUNTIME_DIR/archnav-daemon.sock` |

The IPC socket (`ipc.rs`) keeps the protocol of the old standalone daemon for
scripts: one command per line, `SEARCH <json>`, `SEARCH_ALL <json>`,
`ADD_BOOKMARK <json>`, `RESCAN <path>`, `STATS`, `PING`, each answered with
one JSON line.

## Config Structure

```json
{
  "bookmarks": [
    {"name": "home", "path": "/home/user", "is_network": false}
  ],
  "max_results": 500,
  "toggle_hotkey": "Alt+`",
  "exclude_paths": ["~/Downloads", "/mnt/scratch"],
  "columns": {"widths": {"path": 300, "size": 80, "date": 140}, "hidden": ["path"]}
}
```

`columns` is written by the window (debounced, and on hide) when a column is
resized or hidden from the header's right-click menu. Widths are unzoomed
pixels. Name has no entry while it fills the width the others leave (the
default, and after Reset Columns); the first separator drag stores its
width, the way Dolphin stops sizing columns automatically once one is
resized by hand. `AppConfig::save_columns` refuses to write over a config that
does not parse, like `load`.

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

## UI: Breeze

The window follows KDE's Breeze language and Human Interface Guidelines:
every color, font, size, and spacing comes from the theme, never a literal,
and archnav's zoom (Ctrl+=/-) only multiplies them.

- `Style.qml` derives its tokens from `Kirigami.Theme`, `Kirigami.Units`, and
  `SystemPalette`. A singleton `QtObject` has no item for Kirigami's attached
  theme to bind to, so it reads the theme through `themeProbe`, an unparented
  `Item`. Fonts are point sizes (`font.pointSize: Style.fontSize*`), so the
  system font settings apply.
- Use `Label`, never `QtQuick.Text` (it ignores the system font). Keep the
  controls' own backgrounds; replace only a `contentItem` when a control does
  not fit (the header cells are flat `ToolButton`s with a custom content item).
- Result rows are `ItemDelegate`s with the desktop style's own background
  (`highlighted` for the selection, `Kirigami.StyleHints.useAlternateBackgroundColor`
  for alternating rows), inside a list using the `View` color set.
- File-type icons come from `mime_icon.cpp`: `QMimeDatabase::mimeTypesForFileName`
  matches on the name only. `mimeTypeForFile` would stat and may read every
  result, a network round trip each on a NAS mount. Breeze draws file-type
  icons in color from 22 px (`smallMedium`); its folder icons stay monochrome
  below 32 px, as in Dolphin.
- Column resizing follows Dolphin: a drag moves only the separator under the
  pointer, and the columns after it shift. The drag handles live in a layer
  above the header cells, centered on each separator. With Name filling on
  the left, resizing a column by changing its own width leaves the dragged
  separator still and moves the one on its left instead, which is why the
  first drag fixes Name's width. There is no horizontal scrolling, so
  widening uses free space first, then narrows the columns after it from
  the rightmost inward, each down to its minimum.
- Header and row cells pin `Layout.minimumWidth`/`maximumWidth` to their
  preferred width. Without that, a list narrower than its columns (preview
  pane open) shrinks header cells and row cells differently and the columns
  stop lining up.
- A `Dialog` whose content changes height (an `InlineMessage` appearing) takes
  the layout as its `contentItem`; a child with `anchors.fill: parent` does not
  resize the dialog and the message runs under the buttons.
- Check a UI change under Breeze Light as well as Breeze Dark
  (`plasma-apply-colorscheme BreezeLight`, then back).

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
5. Compile C++ files (`context_menu.cpp`, `file_opener.cpp`, `mime_icon.cpp`, `qt_app.cpp`, `qt_debug_handler.cpp`, `system_tray.cpp`)
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
- **Network mounts**: Rescanned every 5 minutes (no inotify); a rescan writes
  only new or changed entries to the database. A bookmark is
  treated as network if its config `is_network` flag is set or its mount is
  detected as nfs/cifs/sshfs in /proc/mounts (longest mount-point match)
- **Single instance**: A second plain launch toggles the running instance and
  exits; a second `--hidden` launch exits without toggling
- **Wayland**: Cannot set window position; centers on show
