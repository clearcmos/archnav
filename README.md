# archnav

Keyboard-centric file navigator for KDE Plasma on Wayland. It keeps a trigram
index of every path under your bookmarks and searches it as you type.

![archnav Icon](archnav.svg)

## Features

- **Trigram-indexed search** - On a 1.46M-file index (i7-13700K, 24 threads), specific queries take under a millisecond, very common words like `config` (146,944 matches) about 6 ms, and a query with no three-character run (`ab`, or `*.rs` alone) about 20 ms, since it checks every path on all cores. The loaded index takes about 600 MB of memory. When a keystroke only extends the query and the previous results were complete, archnav filters them instead of searching again. `archnav --benchmark` measures your own index
- **Query syntax** - words match in any order (`left all` finds "All That's Left of You"), `!trailer` leaves out paths containing a word, `size:>1G` and `dm:thisweek` filter by size and date, plus regex, glob, fuzzy, and path-aware search (see Search Syntax)
- **Previews** - Text, rendered Markdown, images, PDF and audio/video metadata, archive contents
- **Unified search** - Search all bookmarks simultaneously; every match is listed, and the list creates rows only as they scroll into view
- **Extension filter** - Use `*.py query` to filter by file type
- **Sort options** - Recent, Oldest, Name, Size, Path, Frecency
- **Results table** - File-type icons from the icon theme, click a header to sort, drag its edge to resize, right-click it to show or hide Path, Size, and Modified; widths and hidden columns are remembered. Click a path to open its folder
- **Live updates** - inotify applies changes under local bookmarks as they happen (folders it cannot read are skipped); network mounts are rescanned every 5 minutes
- **Keyboard-centric** - Focus stays in the search field: arrows, Page Up/Down, and Home/End move through the results, Enter opens, the Menu key opens the context menu, Esc hides to the tray
- **System tray** - Persists in tray when closed, toggle with click or global hotkey
- **Context menu** - Right-click (or the Menu key) for Open With, Copy, Cut, Rename, Delete, Properties
- **Zoom support** - Ctrl+=/- to scale UI
- **Wayland native** - Works on KDE Plasma 6 Wayland; colors, fonts, and spacing follow the KDE theme (Kirigami and the Breeze desktop style)

## Dependencies

Install from the Arch repos:

```bash
pacman -S qt6-base qt6-declarative kirigami qqc2-desktop-style kio kservice kcoreaddons
pacman -S ffmpeg poppler xdg-utils dolphin systemsettings p7zip
```

Build dependencies:

```bash
pacman -S rust pkg-config
```

## Building

```bash
# Build and run
cargo run

# Build release
cargo build --release

# Toggle existing instance (or start one)
cargo run -- --toggle

# Start in the tray and load the index without showing the window
# (what a login autostart entry should run)
cargo run -- --hidden
```

## Installation

Install as an Arch package via the bundled PKGBUILD:

```bash
makepkg -si
```

This installs `archnav` to `/usr/bin/`, registers the desktop file, and ships the icon under `hicolor/scalable/apps/`. The package is tracked by pacman as `archnav-git`.

## Usage

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Arrow Up/Down` | Navigate results |
| `Page Up/Down` | Move a page of results |
| `Home` / `End` | First / last result (with no results, or with Shift, they act on the search text) |
| `Enter` | Open file or folder |
| `Ctrl+O` | Open containing folder |
| `Ctrl+P` | Toggle preview pane |
| `Ctrl+R` | Rescan all bookmarks |
| `Ctrl+B` | Manage bookmarks |
| `Ctrl+Shift+F` | Toggle frecency sort |
| `Ctrl+=` / `Ctrl+-` / `Ctrl+Scroll` | Zoom in / out |
| `Ctrl+0` | Reset zoom |
| `F1` | Toggle keyboard shortcuts help |
| `Right-click`, `Menu`, `Shift+F10` | Context menu |
| `Esc` | Hide to tray |

### Search Syntax

- **Words**: `left all` - every word must appear somewhere in the path, in any order (case-insensitive)
- **Phrase**: `"all that's"` - quotes keep words together, in order
- **Exclude**: `movies !trailer` - leave out paths containing a word (`!"bonus disc"` for a phrase)
- **Size**: `size:>1G`, `size:<=500k`, `size:100M..1G`, `size:4096` - binary units k, m, g, t; folders never match
- **Date modified**: `dm:today`, `dm:yesterday`, `dm:thisweek`, `dm:lastmonth`, `dm:thisyear`, `dm:last7days`, `dm:last24h`, `dm:2026`, `dm:2026-10`, `dm:2026-10-29`; with `>`, `>=`, `<`, `<=`, or `dm:2026-01..2026-03` (weeks start on Monday, local time)
- **Extension filter**: `*.rs query` - filter by file extension (position-independent)
- **Regex**: `/^README` - regex search (prefix with `/`)
- **Glob**: `foo*bar` - glob pattern (contains `*` or `?`)
- **Fuzzy**: `~confg` - fuzzy match with typo tolerance (prefix with `~`)
- **Path-aware**: `src/config` - match "config" under a "src" directory
- **Folders only**: `folder:movies` or `folder: movies` - restrict results to directories (substring match)

### System Tray

archnav runs in the system tray:
- **Left-click** tray icon to toggle window visibility
- **Right-click** for menu: Show/Hide, Configure Shortcut, Exit
- Closing the window (X or Esc) hides to tray instead of quitting
- Use "Exit" from tray menu to fully quit

### Global Hotkey

Set up a global shortcut for `archnav --toggle`:

1. System Settings - Shortcuts - click "Add New" - "Command or Script..."
2. Name: `archnav Toggle`
3. Command: `archnav --toggle`
4. Set your preferred key (e.g., `Alt+``)

### Previews

| File Type | Preview Shows |
|-----------|--------------|
| Text files | File contents (first 50 KB) |
| Markdown | Rendered, with images sized to the pane |
| Directories | Folder listing |
| Images | Scaled image display |
| PDF | Document metadata from `pdfinfo` (pages, title, producer) |
| Audio (MP3, FLAC, ...) | Album art, ID3 tags, duration, bitrate |
| Video (MKV, MP4, ...) | Resolution, duration, audio tracks, subtitles |
| Archives (ZIP, TAR, gz, bz2, xz) | Contents listing with sizes |
| Archives (7z, rar, zst) | Contents listing via `7z`, `unrar`, or `zstd` when installed |
| Binaries, fonts, databases | File name and size |

## Configuration

Config stored at `~/.config/archnav/config.json`:

```json
{
  "bookmarks": [
    {"name": "home", "path": "/home/user", "is_network": false},
    {"name": "projects", "path": "/home/user/projects", "is_network": false},
    {"name": "nas", "path": "/mnt/nas", "is_network": true}
  ],
  "toggle_hotkey": "Alt+`",
  "exclude_paths": ["~/Downloads", "/mnt/scratch"],
  "columns": {"widths": {"path": 300, "size": 80, "date": 140}, "hidden": []}
}
```

- `max_results` (optional) caps how many results the window lists; absent or
  0 lists every match
- `columns` is written by the window when a column is resized or hidden. Name
  fills the width the others leave until a separator is dragged, then keeps
  its own width; Reset Columns in the header's right-click menu restores that
- `exclude_paths` lists locations to exclude from indexing, recursively;
  entries are absolute paths with an optional leading `~`
- A config file with a syntax error is left untouched at startup (defaults are
  used for that session) so a hand-editing typo cannot wipe your bookmarks

### Bookmark Management

- Press `Ctrl+B` to open the management dialog (add, rename, delete)
- Changes are written back to `config.json`
- Mark network mounts with `is_network: true` (uses periodic rescan instead of inotify); cifs/nfs/sshfs mounts are also detected automatically

## Architecture

Single unified Rust application using cxx-qt for Qt/QML bindings:

- **Search engine**: Trigram-based posting lists with SQLite persistence
- **File watcher**: inotify for real-time local filesystem updates
- **Preview system**: Text, media (via ffprobe), archives, directories
- **Qt/QML GUI**: Kirigami on the Breeze desktop style, so it follows the KDE color scheme and fonts; results table with a split preview pane

## Data Locations

| Data | Location |
|------|----------|
| Config | `~/.config/archnav/config.json` |
| Index | `~/.local/share/archnav/index.db` |
| Toggle socket | `$XDG_RUNTIME_DIR/archnav.sock` |
| IPC socket for scripts | `$XDG_RUNTIME_DIR/archnav-daemon.sock` (line commands: `SEARCH`, `SEARCH_ALL`, `ADD_BOOKMARK`, `RESCAN`, `STATS`, `PING`) |

`SEARCH` and `SEARCH_ALL` return at most `limit` results from their JSON
request: 500 when it is absent, every match when it is 0.

## License

MIT
