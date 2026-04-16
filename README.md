# archnav

Fast, keyboard-centric file navigator for KDE Wayland with instant trigram-indexed search.

![archnav Icon](archnav.svg)

## Features

- **Instant search** - Trigram-indexed engine provides sub-10ms search across 600k+ files
- **Smart previews** - Text files, images, audio/video metadata, archive contents
- **Unified search** - Search all bookmarks simultaneously
- **Extension filter** - Use `*.py query` to filter by file type
- **Sort options** - Recent, Oldest, Name, Size, Path, Frecency
- **Real-time updates** - inotify watches for instant file change detection
- **Keyboard-centric** - Arrow keys navigate, Enter opens, Esc closes
- **System tray** - Persists in tray when closed, toggle with click or global hotkey
- **Context menu** - Right-click for Open With, Copy, Cut, Rename, Delete, Properties
- **Zoom support** - Ctrl+=/- to scale UI
- **Wayland native** - Works on KDE Plasma 6 Wayland

## Dependencies

Install from the Arch repos:

```bash
pacman -S qt6-base qt6-declarative qt6-quickcontrols2 kio kservice kcoreaddons
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

Copy the release binary and desktop file:

```bash
cargo build --release
install -Dm755 target/release/archnav /usr/local/bin/archnav
install -Dm644 data/archnav.desktop /usr/share/applications/archnav.desktop
```

## Usage

### Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `Arrow Up/Down` | Navigate results |
| `Enter` | Open file or folder |
| `Ctrl+O` | Open containing folder |
| `Ctrl+P` | Toggle preview pane |
| `Ctrl+R` | Refresh index |
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
  "toggle_hotkey": "Alt+`"
}
```

### Bookmark Management

- Click **Bookmarks** button to open management dialog
- Add directories to index
- Mark network mounts with `is_network: true` (uses periodic rescan instead of inotify)

## Architecture

Single unified Rust application using cxx-qt for Qt/QML bindings:

- **Search engine**: Trigram-based posting lists with SQLite persistence
- **File watcher**: inotify for real-time local filesystem updates
- **Preview system**: Text, media (via ffprobe), archives, directories
- **Qt/QML GUI**: Dark theme, responsive layout with split view

## Data Locations

| Data | Location |
|------|----------|
| Config | `~/.config/archnav/config.json` |
| Index | `~/.local/share/archnav/index.db` |
| Toggle Socket | `$XDG_RUNTIME_DIR/archnav.sock` |

## License

MIT
