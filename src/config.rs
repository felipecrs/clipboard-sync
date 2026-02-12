use crate::consts::CONFIG_FILE_NAME;
use crate::utils::get_executable_directory;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Watch mode for detecting incoming clipboard files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum WatchMode {
    Native,
    Polling,
    PollingHarder,
}

impl Default for WatchMode {
    fn default() -> Self {
        Self::Native
    }
}

/// Persistent application configuration.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Config {
    pub folder: Option<String>,
    pub send_texts: bool,
    pub send_images: bool,
    pub send_files: bool,
    pub receive_texts: bool,
    pub receive_images: bool,
    pub receive_files: bool,
    pub auto_cleanup: bool,
    pub watch_mode: WatchMode,
    pub sync_command: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            folder: None,
            send_texts: true,
            send_images: true,
            send_files: true,
            receive_texts: true,
            receive_images: true,
            receive_files: true,
            auto_cleanup: true,
            watch_mode: WatchMode::Native,
            sync_command: String::new(),
        }
    }
}

impl Config {
    pub fn is_sending_anything(&self) -> bool {
        self.send_texts || self.send_images || self.send_files
    }

    pub fn is_receiving_anything(&self) -> bool {
        self.receive_texts || self.receive_images || self.receive_files
    }
}

fn get_config_file_path() -> PathBuf {
    get_executable_directory().join(CONFIG_FILE_NAME)
}

pub fn save_config(config: &Config) {
    if let Ok(json) = serde_json::to_string_pretty(config) {
        let _ = fs::write(get_config_file_path(), json);
    }
}

pub fn load_config() -> Config {
    let config_path = get_config_file_path();
    fs::read_to_string(config_path)
        .ok()
        .and_then(|data| serde_json::from_str(&data).ok())
        .unwrap_or_default()
}
