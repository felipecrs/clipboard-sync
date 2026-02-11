pub const APP_NAME: &str = "Clipboard Sync";
pub const APP_AUMID: &str = "FelipeSantos.ClipboardSync";
pub const APP_UID: &str = "72812af2-6bcc-40d9-b35d-0b43e72ac346";
pub const CONFIG_FILE_NAME: &str = "ClipboardSyncConfig.json";
pub const LOG_FILE_NAME: &str = "ClipboardSync.log";
pub const PNG_ICON_BYTES: &[u8] = include_bytes!("../resources/trayicons/png/working.png");
pub const PNG_ICON_FILE_NAME: &str = "ClipboardSync.png";

pub const GITHUB_REPO_URL: &str = "https://github.com/felipecrs/clipboard-sync";
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
pub const IDLE_TIMEOUT_SECS: u64 = 30;

/// Max time to wait for sync folder after starting sync command (seconds).
pub const SYNC_COMMAND_WAIT_SECS: u64 = 15;
