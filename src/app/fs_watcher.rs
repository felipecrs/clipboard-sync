use crate::clipboard::parse_clipboard_filename;
use crate::config::WatchMode;
use crate::consts::FS_WATCHER_POLL_INTERVAL_SECS;
use crate::types::{ClipboardOrigin, UserEvent};
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::path::Path;
use std::time::Duration;
use tao::event_loop::EventLoopProxy;

/// Create and start a filesystem watcher, returning it boxed.
pub fn create_watcher(
    proxy: &EventLoopProxy<UserEvent>,
    sync_folder: &Path,
    hostname: &str,
    watch_mode: &WatchMode,
) -> Option<Box<dyn Watcher + Send>> {
    let p = proxy.clone();
    let sf = sync_folder.to_path_buf();
    let hn = hostname.to_string();

    let event_handler = move |res: Result<notify::Event, notify::Error>| {
        handle_fs_event(res, &sf, &hn, &p);
    };

    let mut watcher: Box<dyn Watcher + Send> = if *watch_mode == WatchMode::Polling {
        let config = notify::Config::default()
            .with_poll_interval(Duration::from_secs(FS_WATCHER_POLL_INTERVAL_SECS));
        match notify::PollWatcher::new(event_handler, config) {
            Ok(w) => Box::new(w),
            Err(e) => {
                log::error!("Failed to create poll watcher: {e}");
                return None;
            }
        }
    } else {
        let config = notify::Config::default();
        match RecommendedWatcher::new(event_handler, config) {
            Ok(w) => Box::new(w),
            Err(e) => {
                log::error!("Failed to create native watcher: {e}");
                return None;
            }
        }
    };

    if let Err(e) = watcher.watch(sync_folder, RecursiveMode::Recursive) {
        log::error!("Failed to watch sync folder: {e}");
    }

    Some(watcher)
}

fn handle_fs_event(
    res: Result<notify::Event, notify::Error>,
    sync_folder: &Path,
    hostname: &str,
    proxy: &EventLoopProxy<UserEvent>,
) {
    match res {
        Ok(event) => {
            if event.kind.is_create() || event.kind.is_modify() {
                for path in event.paths {
                    // Skip temporary files (OneDrive creates ~RFxxxx.TMP files)
                    let name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    if name.contains("~RF") && name.ends_with(".TMP") {
                        continue;
                    }

                    if let Some(parsed) = parse_clipboard_filename(
                        &path,
                        sync_folder,
                        hostname,
                        Some(ClipboardOrigin::Others),
                    ) {
                        let _ = proxy.send_event(UserEvent::ClipboardFileDetected(parsed.path));
                    }
                }
            }
        }
        Err(e) => {
            log::error!("File watcher error: {e}");
        }
    }
}