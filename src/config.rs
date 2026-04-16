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
    #[serde(default)]
    pub last_bookmark: i32,
    #[serde(default = "default_max_results")]
    pub max_results: i32,
    #[serde(default)]
    pub window_width: i32,
    #[serde(default)]
    pub window_height: i32,
    #[serde(default = "default_splitter")]
    pub splitter_sizes: Vec<i32>,
    #[serde(default = "default_false")]
    pub preview_visible: bool,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_toggle_hotkey")]
    pub toggle_hotkey: String,
}

fn default_max_results() -> i32 {
    500
}

fn default_splitter() -> Vec<i32> {
    vec![500, 500]
}

fn default_false() -> bool {
    false
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
            last_bookmark: 0,
            max_results: 500,
            window_width: 1000,
            window_height: 650,
            splitter_sizes: vec![500, 500],
            preview_visible: false,
            sort_order: 0,
            toggle_hotkey: default_toggle_hotkey(),
        }
    }
}

impl AppConfig {
    fn config_path() -> PathBuf {
        let home = dirs::home_dir().expect("No home directory");
        home.join(CONFIG_DIR).join(CONFIG_FILE)
    }

    /// Load config from disk, or create default.
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
                        warn!("Failed to parse config: {}", e);
                    }
                },
                Err(e) => {
                    warn!("Failed to read config: {}", e);
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
}
