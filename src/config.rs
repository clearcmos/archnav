use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;
use tracing::{info, warn};

const CONFIG_DIR: &str = ".config/archnav";
const CONFIG_FILE: &str = "config.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BookmarkConfig {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub is_network: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub bookmarks: Vec<BookmarkConfig>,
    /// Per-search result cap for the window; 0 (the default) shows every match.
    #[serde(default)]
    pub max_results: i32,
    #[serde(default = "default_toggle_hotkey")]
    pub toggle_hotkey: String,
    /// Locations to exclude from indexing, recursively. Absolute paths, with an
    /// optional leading `~` for the home directory. A file is skipped if its
    /// path equals or sits under any entry. Empty by default.
    #[serde(default)]
    pub exclude_paths: Vec<String>,
    /// Results table layout, restored when the window starts.
    #[serde(default)]
    pub columns: ColumnLayout,
    /// Window size in logical pixels, restored when the window starts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowSize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WindowSize {
    pub width: i32,
    pub height: i32,
}

/// Widths and visibility of the results columns, keyed by column ("name",
/// "path", "size", "date"). Widths are unzoomed pixels. Name is always shown
/// and has no width entry while it fills the width the others leave; the
/// first separator drag stores one.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ColumnLayout {
    #[serde(default)]
    pub widths: BTreeMap<String, i32>,
    #[serde(default)]
    pub hidden: Vec<String>,
}

fn default_toggle_hotkey() -> String {
    "Alt+`".to_string()
}

impl Default for AppConfig {
    fn default() -> Self {
        let home = dirs::home_dir()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|| "/home".to_string());

        Self {
            bookmarks: vec![BookmarkConfig {
                name: "home".to_string(),
                path: home,
                is_network: false,
            }],
            max_results: 0,
            toggle_hotkey: default_toggle_hotkey(),
            exclude_paths: Vec::new(),
            columns: ColumnLayout::default(),
            window: None,
        }
    }
}

impl AppConfig {
    fn config_path() -> PathBuf {
        let home = dirs::home_dir().expect("No home directory");
        home.join(CONFIG_DIR).join(CONFIG_FILE)
    }

    /// Load config from disk, or create default. A config file that exists
    /// but cannot be read or parsed is left untouched: overwriting it with
    /// defaults would silently destroy the user's bookmarks and excludes over
    /// a hand-editing typo.
    pub fn load() -> Self {
        let path = Self::config_path();

        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str(&contents) {
                    Ok(config) => {
                        info!("Loaded config from {}", path.display());
                        return config;
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse config ({}); using defaults without overwriting {}",
                            e,
                            path.display()
                        );
                        return Self::default();
                    }
                },
                Err(e) => {
                    warn!(
                        "Failed to read config ({}); using defaults without overwriting {}",
                        e,
                        path.display()
                    );
                    return Self::default();
                }
            }
        }

        let config = Self::default();
        config.save();
        config
    }

    /// Store a new column layout.
    pub fn save_columns(columns: ColumnLayout) {
        Self::update_saved("column layout", |config| {
            let changed = config.columns != columns;
            config.columns = columns;
            changed
        });
    }

    /// Store the window size.
    pub fn save_window(window: WindowSize) {
        Self::update_saved("window size", |config| {
            let changed = config.window != Some(window);
            config.window = Some(window);
            changed
        });
    }

    /// Apply `change` to the saved config and write it if `change` reports a
    /// difference. Skipped, with a warning, when the config file exists but
    /// does not parse: saving over it would replace the user's bookmarks and
    /// excludes with defaults, which `load` avoids too.
    fn update_saved(what: &str, change: impl FnOnce(&mut Self) -> bool) {
        let path = Self::config_path();
        let mut config = if path.exists() {
            match std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|s| serde_json::from_str::<Self>(&s).map_err(|e| e.to_string()))
            {
                Ok(config) => config,
                Err(e) => {
                    warn!("Not saving {}: config unreadable ({})", what, e);
                    return;
                }
            }
        } else {
            Self::default()
        };
        if change(&mut config) {
            config.save();
        }
    }

    /// Save config to disk.
    pub fn save(&self) {
        let path = Self::config_path();

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }

        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&path, json) {
                    warn!("Failed to write config: {}", e);
                }
            }
            Err(e) => {
                warn!("Failed to serialize config: {}", e);
            }
        }
    }

    /// The result cap to apply per search, 0 for none. A negative value
    /// means none too.
    pub fn effective_max_results(&self) -> usize {
        self.max_results.max(0) as usize
    }

    /// Convert bookmarks to the search engine format.
    pub fn to_bookmarks(&self) -> Vec<crate::search::trigram::Bookmark> {
        self.bookmarks
            .iter()
            .map(|b| crate::search::trigram::Bookmark {
                name: b.name.clone(),
                path: b.path.clone(),
                is_network: b.is_network,
            })
            .collect()
    }

    /// Exclude paths normalized for matching: blanks dropped, a leading `~`
    /// expanded to `$HOME`, and trailing slashes trimmed. The scanner compares
    /// indexed paths against these, so they must be absolute and slash-clean.
    pub fn expanded_exclude_paths(&self) -> Vec<String> {
        let home = dirs::home_dir();
        self.exclude_paths
            .iter()
            .filter_map(|raw| {
                let p = raw.trim();
                if p.is_empty() {
                    return None;
                }
                let expanded = if p == "~" {
                    home.as_ref()?.to_string_lossy().into_owned()
                } else if let Some(rest) = p.strip_prefix("~/") {
                    home.as_ref()?.join(rest).to_string_lossy().into_owned()
                } else {
                    p.to_string()
                };
                let trimmed = expanded.trim_end_matches('/');
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg_with(paths: &[&str]) -> AppConfig {
        AppConfig {
            exclude_paths: paths.iter().map(|s| s.to_string()).collect(),
            ..AppConfig::default()
        }
    }

    #[test]
    fn expands_tilde_to_home() {
        let home = dirs::home_dir().unwrap().to_string_lossy().into_owned();
        let out = cfg_with(&["~/Downloads", "~"]).expanded_exclude_paths();
        assert_eq!(out, vec![format!("{home}/Downloads"), home]);
    }

    #[test]
    fn trims_trailing_slashes_and_blanks() {
        let out = cfg_with(&["/mnt/scratch/", "  ", "/data//"]).expanded_exclude_paths();
        assert_eq!(out, vec!["/mnt/scratch".to_string(), "/data".to_string()]);
    }

    #[test]
    fn max_results_defaults_to_no_cap() {
        let with = |max_results| AppConfig {
            max_results,
            ..AppConfig::default()
        };
        assert_eq!(AppConfig::default().effective_max_results(), 0);
        let parsed: AppConfig = serde_json::from_str(r#"{"bookmarks": []}"#).unwrap();
        assert_eq!(parsed.effective_max_results(), 0);
        assert_eq!(with(-5).effective_max_results(), 0);
        assert_eq!(with(250).effective_max_results(), 250);
    }

    #[test]
    fn bookmarks_convert_to_engine_bookmarks() {
        let config = AppConfig {
            bookmarks: vec![BookmarkConfig {
                name: "nas".to_string(),
                path: "/mnt/nas".to_string(),
                is_network: true,
            }],
            ..AppConfig::default()
        };
        let bookmarks = config.to_bookmarks();
        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].name, "nas");
        assert_eq!(bookmarks[0].path, "/mnt/nas");
        assert!(bookmarks[0].is_network);
    }

    #[test]
    fn window_size_round_trips_and_is_omitted_until_saved() {
        let json = serde_json::to_string(&AppConfig::default()).unwrap();
        assert!(!json.contains("window"));
        let config: AppConfig =
            serde_json::from_str(r#"{"window": {"width": 1200, "height": 800}}"#).unwrap();
        assert_eq!(
            config.window,
            Some(WindowSize {
                width: 1200,
                height: 800
            })
        );
    }

    #[test]
    fn config_without_columns_parses_with_an_empty_layout() {
        let config: AppConfig = serde_json::from_str(r#"{"bookmarks": []}"#).unwrap();
        assert_eq!(config.columns, ColumnLayout::default());
    }

    #[test]
    fn column_layout_round_trips_through_json() {
        let layout: ColumnLayout =
            serde_json::from_str(r#"{"widths": {"path": 320, "size": 90}, "hidden": ["size"]}"#)
                .unwrap();
        assert_eq!(layout.widths.get("path"), Some(&320));
        assert_eq!(layout.hidden, vec!["size".to_string()]);
        let json = serde_json::to_string(&layout).unwrap();
        assert_eq!(serde_json::from_str::<ColumnLayout>(&json).unwrap(), layout);
    }

    #[test]
    fn leaves_plain_absolute_paths_untouched() {
        let out = cfg_with(&["/home/u/Videos"]).expanded_exclude_paths();
        assert_eq!(out, vec!["/home/u/Videos".to_string()]);
    }
}
