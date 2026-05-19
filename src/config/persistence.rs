use super::PersistentState;
use crate::consts::STATE_FILE_NAME;
use crate::utils::get_executable_directory;
use anyhow::Context;
use std::fs;
use std::path::PathBuf;

fn get_state_file_path() -> anyhow::Result<PathBuf> {
    Ok(get_executable_directory()?.join(STATE_FILE_NAME))
}

pub fn save_state(state: &PersistentState) -> anyhow::Result<()> {
    save_state_to(&get_state_file_path()?, state)
}

pub fn load_state() -> anyhow::Result<PersistentState> {
    load_state_from(&get_state_file_path()?)
}

/// Writes `state` to `path` via a temp file + rename for crash safety.
pub(crate) fn save_state_to(
    path: &std::path::Path,
    state: &PersistentState,
) -> anyhow::Result<()> {
    let tmp_path = path.with_extension("json.tmp");

    let json = serde_json::to_string_pretty(state).context("failed to serialize state")?;

    // Write to a temporary file first, then atomically rename to the target.
    // This prevents corruption if the process is interrupted mid-write.
    fs::write(&tmp_path, &json).with_context(|| {
        format!(
            "failed to write temporary state file '{}'",
            tmp_path.display()
        )
    })?;

    if let Err(e) = fs::rename(&tmp_path, path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(anyhow::anyhow!(e).context(format!(
            "failed to rename temporary state file '{}' to '{}'",
            tmp_path.display(),
            path.display()
        )));
    }

    Ok(())
}

/// Loads state from `path`, returning defaults if the file doesn't exist.
pub(crate) fn load_state_from(path: &std::path::Path) -> anyhow::Result<PersistentState> {
    let data = match fs::read_to_string(path) {
        Ok(data) => data,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // First run or file was deleted intentionally — use defaults
            return Ok(PersistentState::default());
        }
        Err(e) => {
            return Err(anyhow::anyhow!(e))
                .with_context(|| format!("failed to read state file '{}'", path.display()));
        }
    };

    serde_json::from_str(&data)
        .with_context(|| format!("failed to parse state file '{}'", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_creates_default_if_missing() {
        let dir = std::env::temp_dir().join("clipboard_sync_test_missing");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("state.json");

        let state = load_state_from(&path).unwrap();
        assert!(state.folder.is_none());
        assert!(state.send_texts);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join("clipboard_sync_test_roundtrip");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("state.json");

        let mut state = PersistentState::default();
        state.folder = Some("/tmp/sync".to_string());
        state.check_updates_on_launch = false;

        save_state_to(&path, &state).unwrap();
        let loaded = load_state_from(&path).unwrap();

        assert_eq!(loaded.folder, Some("/tmp/sync".to_string()));
        assert!(!loaded.check_updates_on_launch);

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_state_overwrites_existing_file() {
        let dir = std::env::temp_dir().join("clipboard_sync_test_overwrite");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("state.json");

        let mut state1 = PersistentState::default();
        state1.folder = Some("/first".to_string());
        save_state_to(&path, &state1).unwrap();

        let mut state2 = PersistentState::default();
        state2.folder = Some("/second".to_string());
        save_state_to(&path, &state2).unwrap();

        let loaded = load_state_from(&path).unwrap();
        assert_eq!(loaded.folder, Some("/second".to_string()));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn load_state_malformed_file_returns_error_with_context() {
        let dir = std::env::temp_dir().join("clipboard_sync_test_malformed");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("state.json");

        fs::write(&path, "not json at all").unwrap();

        let result = load_state_from(&path);
        assert!(result.is_err());
        let err_msg = format!("{:#}", result.unwrap_err());
        assert!(err_msg.contains("failed to parse state file"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_state_uses_atomic_write() {
        let dir = std::env::temp_dir().join("clipboard_sync_test_atomic");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("state.json");
        let tmp_path = path.with_extension("json.tmp");

        let state = PersistentState::default();
        save_state_to(&path, &state).unwrap();

        assert!(
            !tmp_path.exists(),
            "temp file should be cleaned up after successful write"
        );
        assert!(path.exists(), "target file should exist after save");

        let _ = fs::remove_dir_all(&dir);
    }
}