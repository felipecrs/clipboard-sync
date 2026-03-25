use crate::consts::STATE_FILE_NAME;
use crate::utils::get_executable_directory;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

/// Watch mode for detecting incoming clipboard files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[derive(Default)]
pub enum WatchMode {
    #[default]
    Native,
    Polling,
}

/// Persistent application state.
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistentState {
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
    pub check_updates_on_launch: bool,
}

impl Default for PersistentState {
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
            check_updates_on_launch: true,
        }
    }
}

impl PersistentState {
    pub fn is_sending_anything(&self) -> bool {
        self.send_texts || self.send_images || self.send_files
    }

    pub fn is_receiving_anything(&self) -> bool {
        self.receive_texts || self.receive_images || self.receive_files
    }
}

fn get_state_file_path() -> PathBuf {
    get_executable_directory().join(STATE_FILE_NAME)
}

pub fn save_state(state: &PersistentState) {
    let path = get_state_file_path();
    let tmp_path = path.with_extension("json.tmp");

    let json = match serde_json::to_string_pretty(state) {
        Ok(json) => json,
        Err(e) => {
            log::error!("Failed to serialize state: {e}");
            return;
        }
    };

    // Write to a temporary file first, then atomically rename to the target.
    // This prevents corruption if the process is interrupted mid-write.
    if let Err(e) = fs::write(&tmp_path, &json) {
        log::error!(
            "Failed to write temporary state file '{}': {e}",
            tmp_path.display()
        );
        return;
    }

    if let Err(e) = fs::rename(&tmp_path, &path) {
        log::error!(
            "Failed to rename temporary state file '{}' to '{}': {e}",
            tmp_path.display(),
            path.display()
        );
        // Try to clean up the temporary file
        let _ = fs::remove_file(&tmp_path);
    }
}

pub fn load_state() -> anyhow::Result<PersistentState> {
    let path = get_state_file_path();

    let data = match fs::read_to_string(&path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // First run or file was deleted intentionally — use defaults
            return Ok(PersistentState::default());
        }
        Err(e) => {
            return Err(anyhow::anyhow!(e))
                .with_context(|| format!("Failed to read state file '{}'", path.display()));
        }
    };

    serde_json::from_str(&data)
        .with_context(|| format!("Failed to parse state file '{}'", path.display()))
}
