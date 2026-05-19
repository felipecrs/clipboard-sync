mod persistence;

pub use persistence::{load_state, save_state};

use serde::{Deserialize, Serialize};
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
    pub folder: Option<PathBuf>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persistent_state_default_values() {
        let state = PersistentState::default();
        assert!(state.folder.is_none());
        assert!(state.send_texts);
        assert!(state.send_images);
        assert!(state.send_files);
        assert!(state.receive_texts);
        assert!(state.receive_images);
        assert!(state.receive_files);
        assert!(state.auto_cleanup);
        assert_eq!(state.watch_mode, WatchMode::Native);
        assert!(state.sync_command.is_empty());
        assert!(state.check_updates_on_launch);
    }

    #[test]
    fn persistent_state_serialization_roundtrip() {
        let state = PersistentState {
            folder: Some(PathBuf::from("/tmp/sync")),
            send_texts: false,
            send_images: true,
            send_files: false,
            receive_texts: true,
            receive_images: false,
            receive_files: true,
            auto_cleanup: false,
            watch_mode: WatchMode::Polling,
            sync_command: "onedrive --sync".to_string(),
            check_updates_on_launch: false,
        };

        let json = serde_json::to_string_pretty(&state).unwrap();
        let loaded: PersistentState = serde_json::from_str(&json).unwrap();

        assert_eq!(loaded.folder, Some(PathBuf::from("/tmp/sync")));
        assert!(!loaded.send_texts);
        assert!(loaded.send_images);
        assert!(!loaded.send_files);
        assert!(loaded.receive_texts);
        assert!(!loaded.receive_images);
        assert!(loaded.receive_files);
        assert!(!loaded.auto_cleanup);
        assert_eq!(loaded.watch_mode, WatchMode::Polling);
        assert_eq!(loaded.sync_command, "onedrive --sync");
        assert!(!loaded.check_updates_on_launch);
    }

    #[test]
    fn persistent_state_deserialize_missing_fields_uses_defaults() {
        let json = r#"{}"#;
        let state: PersistentState = serde_json::from_str(json).unwrap();
        assert!(state.folder.is_none());
        assert!(state.send_texts);
        assert!(state.check_updates_on_launch);
    }

    #[test]
    fn is_sending_anything_all_false() {
        let state = PersistentState {
            send_texts: false,
            send_images: false,
            send_files: false,
            ..Default::default()
        };
        assert!(!state.is_sending_anything());
    }

    #[test]
    fn is_sending_anything_one_true() {
        let state = PersistentState {
            send_texts: false,
            send_images: true,
            send_files: false,
            ..Default::default()
        };
        assert!(state.is_sending_anything());
    }

    #[test]
    fn is_receiving_anything_all_false() {
        let state = PersistentState {
            receive_texts: false,
            receive_images: false,
            receive_files: false,
            ..Default::default()
        };
        assert!(!state.is_receiving_anything());
    }

    #[test]
    fn is_receiving_anything_one_true() {
        let state = PersistentState {
            receive_texts: true,
            receive_images: false,
            receive_files: false,
            ..Default::default()
        };
        assert!(state.is_receiving_anything());
    }
}