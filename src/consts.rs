pub const APP_NAME: &str = "Clipboard Sync";
pub const APP_UID: &str = "72812af2-6bcc-40d9-b35d-0b43e72ac346";
pub const STATE_FILE_NAME: &str = "ClipboardSyncState.json";
pub const LOG_FILE_NAME: &str = "ClipboardSync.log";

// App identity and icon constants used for Windows AUMID / toast notifications.
#[cfg(target_os = "windows")]
pub const APP_AUMID: &str = "FelipeSantos.ClipboardSync";
#[cfg(target_os = "windows")]
pub const APP_ICON_PNG_BYTES: &[u8] = include_bytes!("../resources/appicons/png/icon.png");
#[cfg(target_os = "windows")]
pub const APP_ICON_PNG_FILE_NAME: &str = "ClipboardSync.png";

// Embedded tray icons for non-Windows.
#[cfg(not(target_os = "windows"))]
pub const WORKING_TRAY_ICON_BYTES: &[u8] = include_bytes!("../resources/trayicons/png/working.png");
#[cfg(not(target_os = "windows"))]
pub const SENT_TRAY_ICON_BYTES: &[u8] = include_bytes!("../resources/trayicons/png/sent.png");
#[cfg(not(target_os = "windows"))]
pub const RECEIVED_TRAY_ICON_BYTES: &[u8] =
    include_bytes!("../resources/trayicons/png/received.png");
#[cfg(not(target_os = "windows"))]
pub const SUSPENDED_TRAY_ICON_BYTES: &[u8] =
    include_bytes!("../resources/trayicons/png/suspended.png");

pub const GITHUB_REPO_URL: &str = "https://github.com/felipecrs/clipboard-sync";
pub const GITHUB_RELEASE_ASSET: &str = "ClipboardSync.exe";
pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Suffix for "is-receiving" marker files.
pub const IS_RECEIVING_FILE_SUFFIX: &str = ".is-receiving.txt";

/// Interval at which the keep-alive file is renewed (4 minutes).
pub const KEEP_ALIVE_INTERVAL_SECS: u64 = 4 * 60;

/// Files from other computers are considered stale after 10 minutes.
pub const STALE_THRESHOLD_SECS: u64 = 10 * 60;

/// Files from ourselves are cleaned after 5 minutes.
pub const SELF_CLEAN_THRESHOLD_SECS: u64 = 5 * 60;

/// Files from others are cleaned after 10 minutes.
pub const OTHERS_CLEAN_THRESHOLD_SECS: u64 = 10 * 60;

/// Maximum file size in MB for sending clipboard files.
pub const MAX_FILES_SIZE_MB: f64 = 100.0;

/// Duration to show sent/received icon before reverting to working.
pub const ICON_FLASH_DURATION_SECS: u64 = 5;

/// Debounce time for clipboard change events in milliseconds.
pub const CLIPBOARD_DEBOUNCE_MS: u64 = 500;

/// Delay after clipboard change to let clipboard be fully written.
pub const CLIPBOARD_WRITE_DELAY_MS: u64 = 100;

/// Time window in which recent clipboards are skipped as duplicates.
pub const DUPLICATE_WINDOW_MS: u64 = 15_000;

/// Idle timeout in seconds (15 minutes).
pub const IDLE_TIMEOUT_SECS: u64 = 15 * 60;

/// Max time to wait for sync folder after starting sync command (seconds).
pub const SYNC_COMMAND_WAIT_SECS: u64 = 15;

/// Polling interval for file system watcher in polling mode (seconds).
pub const FS_WATCHER_POLL_INTERVAL_SECS: u64 = 1;
