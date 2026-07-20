# archnav

Fast, keyboard-centric file navigator for KDE Wayland with instant trigram-indexed search.

![archnav Icon](archnav.svg)

## Features

- **Instant search** - Trigram-indexed engine provides sub-10ms search across 600k+ files
- **Smart previews** - Text files, images, audio/video metadata, archive contents
- **Unified search** - Search all bookmarks simultaneously
- **Extension filter** - Use `*.py query` to filter by file type
- **Sort options** - Recent, Oldest, Name, Size, Path, Frecency
- **File tags** - built-in tag system that works where xattrs do not (NAS/CIFS mounts): tags column in results, Ctrl+T tag editor, `t:` search filter, and a full `archnav tag` CLI with rename-surviving repair
- **Real-time updates** - inotify watches for instant file change detection
- **Keyboard-centric** - Arrow keys navigate, Enter opens, Esc closes
- **System tray** - Persists in tray when closed, toggle with click or global hotkey
- **Context menu** - Right-click for Open With, Copy, Cut, Rename, Delete, Properties
- **Zoom support** - Ctrl+=/- to scale UI
- **Wayland native** - Works on KDE Plasma 6 Wayland

## Dependencies

Install from the Arch repos:

```bash
pacman -S qt6-base qt6-declarative kio kservice kcoreaddons
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

# Toggle existing instance
cargo run -- --toggle
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
| `Enter` | Open file or folder |
| `Ctrl+O` | Open containing folder |
| `Ctrl+P` | Toggle preview pane |
| `Ctrl+R` | Refresh index |
| `Ctrl+B` | Manage bookmarks |
| `Ctrl+T` | Edit tags of selected file |
| `Ctrl+Shift+F` | Toggle frecency sort |
| `Ctrl+=` / `Ctrl+-` | Zoom in / out |
| `Ctrl+0` | Reset zoom |
| `F1` | Toggle keyboard shortcuts help |
| `Right-click` | Context menu |
| `Esc` | Hide to tray |

### Search Syntax

- **Simple**: `query` - substring search across all bookmarks
- **Extension filter**: `*.rs query` - filter by file extension (position-independent)
- **Regex**: `/^README` - regex search (prefix with `/`)
- **Glob**: `foo*bar` - glob pattern (contains `*` or `?`)
- **Fuzzy**: `~confg` - fuzzy match with typo tolerance (prefix with `~`)
- **Path-aware**: `src/config` - match "config" under a "src" directory
- **Folders only**: `folder:movies` or `folder: movies` - restrict results to directories (substring match)
- **Tags**: `t:coffee` or `t: coffee outdoor` (OR) or `t: coffee&outdoor` / `t:coffee AND outdoor` (AND); bare `t:` lists every tagged file. Search text goes before a detached `t:` group.

### File Tags

archnav includes a tag system for organizing files into overlapping
categories, designed for filesystems where `user.xdg.tags` xattrs fail
(CIFS/SMB NAS mounts). Tags live in one portable JSON index per directory
tree (`.tagstore/index.json`); content fingerprints let `repair` relink
files that were renamed or moved outside the tool. The format is specified
in [docs/tagstore-format.md](docs/tagstore-format.md).

In the GUI: a Tags column in results, the tags of the selected file in the
status bar, Ctrl+T to edit, and the `t:` search filter above.

From the terminal:

```
archnav tag init /mnt/nas/photos          # once per tree
archnav tag add "beach day.jpg" vacation 2026
archnav tag add -t vacation pic1.jpg pic2.jpg pic3.jpg
archnav tag ls                            # ls -lh style listing with a tags column
archnav tag find vacation --not 2025      # query by tags
archnav tag mv old.jpg archive/new.jpg    # move + reindex in one step
archnav tag repair                        # relink after external renames/moves
archnav tag check --verify                # read-only fsck of the index
```

On filesystems that do support user xattrs, tags are additionally mirrored
to `user.xdg.tags` so KDE Dolphin and Baloo see them; the index remains the
source of truth.

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

### Smart Previews

| File Type | Preview Shows |
|-----------|--------------|
| Text files | File contents (up to 50KB) |
| Directories | Folder listing |
| Images | Scaled image display |
| Audio (MP3, FLAC) | Album art, ID3 tags, duration, bitrate |
| Video (MKV, MP4) | Resolution, duration, audio tracks, subtitles |
| Archives (ZIP, TAR) | Contents listing with sizes |

## Configuration

Config stored at `~/.config/archnav/config.json`:

```json
{
  "bookmarks": [
    {"name": "home", "path": "/home/user", "is_network": false},
    {"name": "projects", "path": "/home/user/projects", "is_network": false},
    {"name": "nas", "path": "/mnt/nas", "is_network": true}
  ],
  "max_results": 500,
  "toggle_hotkey": "Alt+`",
  "exclude_paths": ["~/Downloads", "/mnt/scratch"]
}
```

- `max_results` caps how many results a search returns (hard cap 2000)
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
- **Tag store**: Native tag engine + `archnav tag` CLI ([format spec](docs/tagstore-format.md))
- **Qt/QML GUI**: Dark theme, responsive layout with split view

## Data Locations

| Data | Location |
|------|----------|
| Config | `~/.config/archnav/config.json` |
| Index | `~/.local/share/archnav/index.db` |
| Toggle Socket | `$XDG_RUNTIME_DIR/archnav.sock` |

## License

MIT
