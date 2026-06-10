use serde::{Deserialize, Serialize};
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
    /// Per-search result cap. Hard-capped at the engine's MAX_RESULTS (2000).
    #[serde(default = "default_max_results")]
    pub max_results: i32,
    #[serde(default = "default_toggle_hotkey")]
    pub toggle_hotkey: String,
    /// Locations to exclude from indexing, recursively. Absolute paths, with an
    /// optional leading `~` for the home directory. A file is skipped if its
    /// path equals or sits under any entry. Empty by default.
    #[serde(default)]
    pub exclude_paths: Vec<String>,
}

fn default_max_results() -> i32 {
    500
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
            max_results: default_max_results(),
            toggle_hotkey: default_toggle_hotkey(),
            exclude_paths: Vec::new(),
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

    /// The result cap to apply per search: max_results clamped to a sane
    /// range, never above the engine's hard cap.
    pub fn effective_max_results(&self) -> usize {
        self.max_results
            .clamp(1, crate::search::trigram::MAX_RESULTS as i32) as usize
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
    fn leaves_plain_absolute_paths_untouched() {
        let out = cfg_with(&["/home/u/Videos"]).expanded_exclude_paths();
        assert_eq!(out, vec!["/home/u/Videos".to_string()]);
    }
}
