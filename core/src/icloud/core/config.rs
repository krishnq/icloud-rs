use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use directories::ProjectDirs;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AppConfig {
    pub accounts: HashMap<String, AccountConfig>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct AccountConfig {
    #[serde(rename = "type")]
    pub account_type: String, // "icloud", "google", etc.
    pub mount_drive: Option<String>,
    pub mount_photos: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        let mut accounts = HashMap::new();
        accounts.insert(
            "default_icloud".to_string(),
            AccountConfig {
                account_type: "icloud".to_string(),
                mount_drive: Some("/data/icloud/drive".to_string()),
                mount_photos: Some("/data/icloud/photos".to_string()),
            },
        );
        Self { accounts }
    }
}

pub fn get_config_dir() -> PathBuf {
    if let Some(proj_dirs) = ProjectDirs::from("com", "antigravity", "icloud-rs") {
        proj_dirs.config_dir().to_path_buf()
    } else {
        PathBuf::from("~/.config/icloud-rs")
    }
}

pub fn load_config() -> AppConfig {
    let config_dir = get_config_dir();
    let config_path = config_dir.join("config.toml");

    if !config_path.exists() {
        // Create default config
        if let Err(e) = std::fs::create_dir_all(&config_dir) {
            eprintln!("Failed to create config dir: {}", e);
        }
        let default_cfg = AppConfig::default();
        if let Ok(toml_str) = toml::to_string_pretty(&default_cfg) {
            if let Err(e) = std::fs::write(&config_path, toml_str) {
                eprintln!("Failed to write default config: {}", e);
            }
        }
        return default_cfg;
    }

    match std::fs::read_to_string(&config_path) {
        Ok(content) => match toml::from_str(&content) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!("Failed to parse config.toml: {}", e);
                AppConfig::default()
            }
        },
        Err(e) => {
            eprintln!("Failed to read config.toml: {}", e);
            AppConfig::default()
        }
    }
}
