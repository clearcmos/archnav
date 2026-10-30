# Architecture

## Overview

archnav is a single Rust binary with a Qt/QML interface bound through cxx-qt.
It keeps a trigram index of every path under the configured bookmarks in
memory, persists it in SQLite, and keeps it current with inotify for local
paths and periodic rescans for network mounts.

The design aims for:
- **Selective search**: a query's trigrams pick the candidate files, so a
  specific query checks few paths (measured numbers below)
- **Short startup**: the posting lists are cached in SQLite and loaded directly
- **Few disk writes**: rescans and file events write only entries that changed
- **One process**: no separate daemon; the window, index, and IPC servers share
  it, and the window can start hidden in the tray

## Components

```
┌──────────────────────────────────────────────────────────────────────────┐
│ QML UI (qml/)                                                            │
│   Main.qml: window, shortcuts, status bar, F1 help popup                 │
│   SearchBar.qml (Kirigami.SearchField)   ResultsList.qml (header + rows) │
│   PreviewPanel.qml   BookmarkDialog.qml   Style.qml (theme singleton)    │
└──────────────────────────────┬───────────────────────────────────────────┘
                               │ QObject properties, invokables, signals
┌──────────────────────────────▼───────────────────────────────────────────┐
│ Bridge (src/bridge/)                                                     │
│   SearchEngine: initialize, search, result accessors (path, size, mtime, │
│     icon name), bookmarks, column layout, open file/folder, context menu │
│   PreviewBridge: request_preview, clear_preview, preview text/image      │
└──────────────────────────────┬───────────────────────────────────────────┘
┌──────────────────────────────▼───────────────────────────────────────────┐
│ Core engine (src/search/)                                                │
│   CoreEngine ── TrigramIndex (in memory, behind an RwLock)               │
│       │      └─ Database writer thread (SQLite, batched transactions)    │
│       ├── scanner: initial scan, startup reconcile, index_path           │
│       ├── watcher: inotify for local bookmarks                           │
│       └── integrity: 60 s stale-entry sweep, 5 min network rescan        │
├──────────────────────────────────────────────────────────────────────────┤
│ Previews (src/preview/)   text/Markdown, media (ffprobe), archives, dirs │
├──────────────────────────────────────────────────────────────────────────┤
│ Native glue (C++)   context menu (QMenu), tray icon, KIO file opener,    │
│                     MIME icon lookup, QApplication wrapper               │
├──────────────────────────────────────────────────────────────────────────┤
│ Sockets   toggle.rs: archnav.sock        ipc.rs: archnav-daemon.sock     │
└──────────────────────────────────────────────────────────────────────────┘
```

## Trigram index

Every indexed path contributes the trigrams of its file name and of each path
component. A query's trigrams select candidates by intersecting posting lists,
and the candidates are then checked against the full query:

```
Query: "test"
Trigrams: ["tes", "est"]

Posting lists (in memory):
  "tes" -> {1, 5, 99, ...}
  "est" -> {1, 12, 99, ...}

Intersection: {1, 99}  <- candidates
Final filter: the query must match the path (or, for folder:, the folder name)
```

plocate and Google Code Search use the same approach. The cost is set by the
posting lists the query touches: a rare run of characters narrows the
candidates to a handful of files, while a very common word leaves many to
check, and a query with no three-character run (`ab`, or `*.rs` alone) has no
trigram to narrow with and checks every path.

`archnav --benchmark` measures this against the real index, with the search
cache cleared between runs. On 1.44M files (i7-13700K), medians were:

| Query | Median | Results |
|---|---|---|
| `PKGBUILD` | 31 µs | 57 |
| `screenshot` | 1.0 ms | 500 |
| `main` | 2.7 ms | 500 |
| `archnav` | 4.7 ms | 500 |
| `package.json` | 29 ms | 500 |
| `test` | 38 ms | 500 |
| `config` | 72 ms | 500 |
| `ab` | 401 ms | 500 |
| `*.rs` | 406 ms | 500 |

Results are capped at `max_results` (500 by default).

`SearchCache` in `engine.rs` keeps the previous query's results. When the new
query only extends the old one (typing `con`, then `conf`), it filters those
results instead of searching again, unless they were truncated at the result
cap.

## Persistence

SQLite (`~/.local/share/archnav/index.db`, WAL mode) holds:
- `files`: id, path (unique), is_dir, mtime, size
- `posting_lists`: trigram, packed file ids; the startup cache
- `meta`: `posting_lists_count` and `posting_lists_next_id` (cache
  validation), `max_file_id` (ids are never reused), `trigram_schema_version`
- `bookmarks`, and `file_access` (open counts for frecency sorting)

Trigrams are not stored per file: they are a pure function of the path.

On startup:
1. If the cached posting-list count is within 1% of the file count, load the
   files and posting lists directly (the fast path, about 2-5 s for 1.4M
   files), then compute trigrams for files added since the cache was built.
2. Otherwise compute every file's trigrams from its path, rebuild the posting
   lists, and save them as the new cache.

An older database is migrated once when opened: the per-file trigram table and
two unused indexes are dropped and the file is compacted with VACUUM. The WAL
is truncated to 64 MiB after checkpoints.

All writes go through one database thread over a channel, batched up to 1000
operations per transaction. Indexing goes through `scanner::index_path`, which
queues nothing for an entry whose type, mtime, and size are unchanged, an
in-place `UPDATE` for changed metadata, and an insert only for a new path, so
a rescan of an unchanged tree writes nothing.

## Keeping the index current

- **Startup**: an empty index gets a full scan of every bookmark; otherwise a
  reconcile walks the bookmarks and adds only paths the index lacks.
- **Local bookmarks**: inotify (the `notify` crate) applies creates, changes,
  removals, and renames as they happen.
- **Network bookmarks** (`is_network` in the config, or an nfs/cifs/sshfs
  mount found in `/proc/mounts`): rescanned every 5 minutes, since inotify
  cannot see remote changes.
- **Integrity sweep**: every 60 s, 5000 index entries are checked and those
  whose files are gone are removed; entries under a bookmark that is currently
  unreachable (an unmounted share) are left alone.
- **Excludes**: built-in directory names (`.git`, `node_modules`, `target`,
  `.cache`, and others in `trigram.rs`) plus the config's `exclude_paths`, all
  through the single `should_exclude` check in `scanner.rs`.

## Threading

```
Main thread (Qt event loop)
    ├── QML UI
    └── SearchEngine QObject
            ├── initialize(): background thread opens the database, loads the
            │   index, then reports "ready"; the user can search from here
            │       └── background thread: inotify setup (can take seconds),
            │           startup scan or reconcile, integrity sweep, network rescan
            └── search(): background thread per query, read lock on the index;
                results are queued back to the Qt thread, stale ones dropped

Database writer thread
    └── serializes every SQLite write
```

## Query syntax

`query.rs` turns the search text into a `ParsedQuery`:
- `config`: substring over the full path
- `*.rs config`: extension filter, anywhere in the query
- `/^README`: regular expression
- `foo*bar`: glob over the file name or path
- `~confg`: fuzzy match on the file name, with an edit-distance budget that
  grows with query length
- `src/config`: path-aware; `config` must sit under a `src` component
- `folder:movies`: directories whose own name matches

Sort orders: most recent, oldest, name A-Z and Z-A, largest, smallest, path,
and frecency (Ctrl+Shift+F).

## Previews

`preview/mod.rs` picks a preview by extension:
- **Images**: shown by QML from the file path
- **PDF**: `pdfinfo` metadata
- **Audio and video**: `ffprobe -print_format json` metadata, plus album art
- **Archives**: zip and tar (plain, gz, bz2, xz) listed in-process; 7z, rar, and
  zst through `7z`, `unrar`, or `zstd`
- **Markdown**: rendered to HTML with pulldown-cmark, images sized to the pane
  and resolved relative to the file
- **Binaries, fonts, databases**: name and size
- **Everything else**: the first 50 KB as text, with binary detection
- **Directories**: a listing

## Interface

The QML follows KDE's Breeze design: colors, fonts, and spacing come from
Kirigami and the system palette, list rows use the desktop style's own
delegates, and file-type icons come from the icon theme through a name-only
MIME lookup (no filesystem access, which matters on network mounts). The
results table resizes its columns the way Dolphin does. `CLAUDE.md` records
the conventions and the reasons behind them.

## Sockets

- **Toggle** (`$XDG_RUNTIME_DIR/archnav.sock`, `toggle.rs`): the running
  window listens; `archnav --toggle` writes `toggle` to show or hide it. A
  plain second launch toggles the running window and exits; `--hidden` exits
  quietly if one is running.
- **Scripts** (`$XDG_RUNTIME_DIR/archnav-daemon.sock`, `ipc.rs`): one command
  per line, each answered with one JSON line: `SEARCH <json>`,
  `SEARCH_ALL <json>`, `ADD_BOOKMARK <json>`, `RESCAN <path>`, `STATS`,
  `PING`. It keeps the protocol of the standalone daemon archnav replaced.

## Context menu

`context_menu.cpp` builds a QMenu with Dolphin-style actions: Open, Open With
(from `mimeapps.list`), Cut, Copy, Copy Location, Duplicate Here, Rename, Move
to Trash, Delete (confirmed), Move to New Folder, Open Terminal Here, Open as
Administrator, Compress, and Properties, the last through the freedesktop
`FileManager1.ShowItemProperties` D-Bus call, which also works on network
mounts. QML asks for it through `SearchEngine::show_context_menu`.

## Build

`build.rs` uses cxx-qt-build to:
1. Register the QML module `org.archnav.app`, with `Style.qml` as a singleton
2. Generate the C++ side of the Rust bridge objects
3. Run MOC on `system_tray.cpp`
4. Compile `context_menu.cpp`, `file_opener.cpp`, `mime_icon.cpp`,
   `qt_app.cpp`, `qt_debug_handler.cpp`, and `system_tray.cpp`
5. Link the Qml, Quick, QuickControls2, Widgets, and DBus Qt modules and the
   KDE Frameworks (KIO, KService, KCoreAddons)
