#[derive(Clone, Copy)]
pub enum NotificationDuration {
    Short,
    Long,
}

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "windows")]
pub use self::windows::{init_platform, is_directory_writable, restart_onedrive};

#[cfg(not(target_os = "windows"))]
pub fn init_platform(_executable_directory: &std::path::Path) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub fn is_directory_writable(dir: &std::path::Path) -> bool {
    let test_path = dir.join(".clipboard_sync_write_test");
    match std::fs::write(&test_path, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&test_path);
            true
        }
        Err(_) => false,
    }
}

pub fn send_notification(
    title: &str,
    message: &str,
    duration: NotificationDuration,
) -> anyhow::Result<()> {
    let timeout = match duration {
        NotificationDuration::Short => notify_rust::Timeout::Default,
        NotificationDuration::Long => notify_rust::Timeout::Milliseconds(25_000),
    };

    let mut notification = notify_rust::Notification::new();
    notification.summary(title).body(message).timeout(timeout);

    #[cfg(target_os = "windows")]
    notification.app_id(crate::consts::APP_AUMID);

    notification
        .show()
        .map_err(|e| anyhow::anyhow!("failed to show notification: {e:#}"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_directory_writable_with_writable_dir() {
        let dir = tempfile::tempdir().unwrap();
        assert!(is_directory_writable(dir.path()));
    }

    #[test]
    fn is_directory_writable_with_nonexistent_dir() {
        let path = std::path::Path::new("/tmp/nonexistent_desloppify_dir_test_xyz");
        assert!(!is_directory_writable(path));
    }

    #[test]
    fn init_platform_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        assert!(init_platform(dir.path()).is_ok());
    }
}
