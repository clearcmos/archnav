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
│   SearchEngine (the results list model): initialize, search, roles       │
│     (path, name, bold matches, size, mtime, icon), selection, copy,      │
│     drag, bookmarks, columns, window size, open, menus                   │
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
component. The trigrams of every query word select candidates by intersecting
posting lists, and the candidates are then checked against the full query:

```
Query: "test"
Trigrams: ["tes", "est"]

Posting lists (in memory):
  "tes" -> {1, 5, 99, ...}
  "est" -> {1, 12, 99, ...}

Intersection: {1, 99}  <- candidates
Final filter: the query must match the path (or, for folder:, the folder name)
```

Posting lists are Roaring bitmaps (the `roaring` crate): compressed sets of
file ids that intersect container by container. The intersection starts from
the shortest list, so each step works on the smallest set so far. The candidate
checks (words, `!words`, extension, size and date filters) run on all cores
with rayon. A query with no trigram to narrow with skips candidate collection
and checks every entry in parallel. Matches are sorted on a key computed once
per entry (a file name or frecency score is too slow to work out again in every
comparison), and every match is returned. When `max_results` is set, a linear
selection picks the top ones before sorting, and the status bar shows the full
count ("500 of 146,944 results").

plocate and Google Code Search use the same approach. The cost is set by the
posting lists the query touches: a rare run of characters narrows the
candidates to a handful of files, while a very common word leaves many to
check, and a query with no three-character run (`ab`, or `*.rs` alone) has no
trigram to narrow with and checks every path.

`archnav --benchmark` measures this against the real index, with the search
cache cleared between runs, and prints the resident memory once the index is
loaded. On 1.46M files (i7-13700K, 24 threads), medians were:

| Query | Median | Matches |
|---|---|---|
| `PKGBUILD` | 55 µs | 57 |
| `report pdf` | 54 µs | 17 |
| `archnav` | 398 µs | 1,364 |
| `package.json` | 405 µs | 885 |
| `screenshot` | 517 µs | 1,660 |
| `main` | 984 µs | 12,307 |
| `test` | 3.2 ms | 85,903 |
| `config` | 5.5 ms | 146,944 |
| `size:>1G` | 8.3 ms | 1,938 |
| `dm:thisweek` | 10.1 ms | 32,323 |
| `*.rs` | 15.9 ms | 7,603 |
| `ab` | 21.1 ms | 153,454 |

A single letter matches nearly every path: `a` returns 1.44M results in
about 50 ms sorted by date and about 120 ms sorted by name.

The results go to QML as a list model (`SearchEngine` derives from
`QAbstractListModel`), so the view creates delegates only for the rows on
screen, and file-type icons are looked up as rows scroll into view. The
engine's cache and the model share one `Arc` of the results, and a replaced
result set is freed on a pool thread, since releasing a million shared paths
takes longer than the search that replaced them.

`SearchCache` in `engine.rs` keeps the previous query's results. When the new
query can only remove matches (typing `con`, then `conf`; adding a word or a
`!word`), it filters those results in parallel instead of searching again,
unless they were truncated at a result cap. `ParsedQuery::narrows` decides: every old word must
sit inside a new word, every old `!word` must contain a new one (excluding "ba"
drops fewer paths than excluding "b"), and every filter must be unchanged.

## Persistence

SQLite (`~/.local/share/archnav/index.db`, WAL mode) holds:
- `files`: id, path (unique), is_dir, mtime, size
- `posting_lists`: trigram, serialized Roaring bitmap of file ids; the
  startup cache (about 70 MB for 1.46M files)
- `meta`: `posting_lists_count` and `posting_lists_next_id` (cache
  validation), `max_file_id` (ids are never reused), `trigram_schema_version`
- `bookmarks`, and `file_access` (open counts for frecency sorting)

Trigrams are not stored per file: they are a pure function of the path.

On startup:
1. If the cached posting-list count is within 1% of the file count, load the
   files and posting lists directly (the fast path, under 1 s for 1.46M
   files), then compute trigrams for files added since the cache was built.
2. Otherwise compute every file's trigrams from its path, rebuild the posting
   lists, and save them as the new cache.

An older database is migrated once when opened: the per-file trigram table and
two unused indexes are dropped and the file is compacted with VACUUM. A new
posting-list format (`TRIGRAM_SCHEMA_VERSION`) drops the cache and its count,
compacts the file, and rebuilds the cache on that start (about 10 s for 1.46M
files). The WAL is truncated to 64 MiB after checkpoints.

In memory, each path is stored once as an `Arc<str>` shared by `files`,
`path_to_id`, and the results, and the posting lists' ~100M entries take about
70 MB as Roaring bitmaps. The loaded index of 1.46M files is about 600 MB
resident.

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
- `left all`: every word must appear in the full path, in any order
  (case-insensitive); the longest word leads the trigram lookup
- `"all that's"`: quotes keep a phrase together
- `movies !trailer`: `!word` leaves out paths containing the word
- `size:>1G`, `size:100M..1G`: size in bytes with binary units; folders,
  which have no indexed size, never match
- `dm:thisweek`, `dm:last7days`, `dm:2026-10`, `dm:>2025`: modification time,
  in local time, with weeks starting on Monday; an incomplete value such as
  `size:>` is ignored while it is being typed
- `*.rs config`: extension filter, anywhere in the query
- `/^README`: regular expression; the rest of the query is the pattern, so
  `!`, quotes, and `size:` inside it stay literal
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
  `SEARCH` and `SEARCH_ALL` return at most the request's `limit` results
  (500 when absent, every match when 0).

## Context menu

`context_menu.cpp` builds a QMenu with Dolphin-style actions for one file or
a selection: Open, Open With (from `mimeapps.list`, through
`KIO::ApplicationLauncherJob`), Cut, Copy, Copy Location, Duplicate Here,
Rename, Move to Trash, Delete (confirmed), Move to New Folder, Open Terminal
Here, Open as Administrator, Compress, and Properties, the last through the
freedesktop `FileManager1.ShowItemProperties` D-Bus call, which also works on
network mounts. QML asks for it through `SearchEngine::show_context_menu`.
The same file holds the clipboard copy (Ctrl+C, Ctrl+Alt+C) and the `QDrag`
that carries the selected files out of the window.

## Selection and highlighting

The selected rows live in Rust (`search/selection.rs`, a Roaring bitmap of
row numbers, so a Shift+click range over a million rows stays small); with none
selected, the current row stands for the selection. Results keys work from
the search field and, after a click on a row moves focus there, from the
list, where typing goes back to the search field. Each
search also builds a `Highlight` from its query (`search/highlight.rs`), and
the model's `nameMarkup` role gives the file name as Qt StyledText with the
matched words, or regex matches, in bold.

## Build

`build.rs` uses cxx-qt-build to:
1. Register the QML module `org.archnav.app`, with `Style.qml` as a singleton
2. Generate the C++ side of the Rust bridge objects
3. Run MOC on `system_tray.cpp`
4. Compile `context_menu.cpp`, `file_opener.cpp`, `mime_icon.cpp`,
   `qt_app.cpp`, `qt_debug_handler.cpp`, and `system_tray.cpp`
5. Link the Qml, Quick, QuickControls2, Widgets, DBus, and Network Qt modules
   and the KDE Frameworks (KIO with KIOWidgets for the Open With dialog,
   KService, KCoreAddons)
