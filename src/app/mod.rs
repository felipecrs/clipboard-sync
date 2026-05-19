mod fs_watcher;
mod tray;

use crate::clipboard::{
    ClipboardDedupState, clean_files, now_ms, parse_clipboard_filename, read_clipboard_from_file,
    write_clipboard_to_file,
};
use crate::config::{PersistentState, WatchMode, save_state};
use crate::consts::{
    APP_NAME, APP_UID, CLIPBOARD_DEBOUNCE_MS, CLIPBOARD_WRITE_DELAY_MS, CURRENT_VERSION,
    ICON_FLASH_DURATION_SECS, IDLE_TIMEOUT_SECS,
    IS_RECEIVING_FILE_SUFFIX, KEEP_ALIVE_INTERVAL_SECS, SYNC_COMMAND_WAIT_SECS,
};
use crate::notification::log_and_notify_error;
use crate::platform::{NotificationDuration, send_notification};
use crate::sync_command::SyncCommand;
use crate::types::{ClipboardOrigin, TrayIconState, UserEvent};
use crate::ui::{MenuIdMap, UpdateAction, handle_menu_event, rebuild_tray_menu};
use crate::update::{self, UpdateInfo};
use crate::EXECUTOR;
use clipboard_rs::{ClipboardHandler, ClipboardWatcher, ClipboardWatcherContext, WatcherShutdown};
use notify::Watcher;
use smol::Task;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tao::event_loop::EventLoopProxy;
use tray_icon::TrayIconBuilder;
use tray_icon::menu::Menu;

// --- Clipboard watcher handler ---

struct ClipboardChangeHandler {
    proxy: EventLoopProxy<UserEvent>,
}

impl ClipboardHandler for ClipboardChangeHandler {
    fn on_clipboard_change(&mut self) {
        let _ = self.proxy.send_event(UserEvent::ClipboardChanged);
    }
}

// --- Application state ---

pub struct AppState {
    pub persistent_state: PersistentState,
    pub hostname: String,
    pub sync_folder: Option<PathBuf>,

    pub initialized: bool,

    // Clipboard dedup state
    pub dedup: ClipboardDedupState,

    // Clipboard watcher
    clipboard_watcher_shutdown: Option<WatcherShutdown>,

    // File system watcher (kept alive to maintain the watch)
    _fs_watcher: Option<Box<dyn Watcher + Send>>,

    // Sync command
    pub sync_command: SyncCommand,

    // Auto-launch
    pub auto_launch: auto_launch::AutoLaunch,

    // Update
    pub update_info: Option<UpdateInfo>,

    // Icon state
    current_icon: TrayIconState,

    // For clipboard change debouncing
    last_clipboard_event: Option<u64>,

    // Idle suspension tracking
    suspended_by_idle: bool,

    // Folder check timing (dynamic interval)
    last_folder_check: Option<Instant>,
    sync_command_started_at: Option<Instant>,

    // Tray menu (kept alive; rebuilt in place when state changes)
    pub tray_menu: Menu,

    // Menu action map
    pub menu_actions: MenuIdMap,

    // Async task handles - tasks created during initialize(), cancelled during uninitialize()
    init_tasks: Vec<Task<()>>,

    // One-shot async task handles
    icon_revert_task: Option<Task<()>>,
    clipboard_write_task: Option<Task<()>>,

    // Tray icon handle
    pub tray_icon: Option<tray_icon::TrayIcon>,
}

impl AppState {
    pub fn new(
        persistent_state: PersistentState,
        hostname: String,
        tray_menu: Menu,
        menu_actions: MenuIdMap,
        auto_launch: auto_launch::AutoLaunch,
    ) -> Self {
        let sync_folder = persistent_state.folder.clone();
        Self {
            hostname,
            sync_folder,
            persistent_state,
            initialized: false,
            dedup: ClipboardDedupState::default(),
            clipboard_watcher_shutdown: None,
            _fs_watcher: None,
            sync_command: SyncCommand::new(),
            auto_launch,
            update_info: None,
            current_icon: TrayIconState::Suspended,
            last_clipboard_event: None,
            suspended_by_idle: false,
            last_folder_check: None,
            sync_command_started_at: None,
            tray_menu,
            menu_actions,
            init_tasks: Vec::new(),
            icon_revert_task: None,
            clipboard_write_task: None,
            tray_icon: None,
        }
    }

    pub fn handle_init(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        let tooltip = format!("{APP_NAME} v{CURRENT_VERSION}");
        match TrayIconBuilder::new()
            .with_menu(Box::new(self.tray_menu.clone()))
            .with_tooltip(&tooltip)
            .with_icon(tray::get_tray_icon(TrayIconState::Suspended))
            .with_id(APP_UID)
            .build()
        {
            Ok(icon) => self.tray_icon = Some(icon),
            Err(e) => log::error!("Failed to build tray icon: {e:#}"),
        }

        // Async update check (non-blocking)
        if self.persistent_state.check_updates_on_launch {
            let p = proxy.clone();
            EXECUTOR
                .spawn(async move {
                    let info = smol::unblock(|| update::check_for_update(false)).await;
                    let _ = p.send_event(UserEvent::UpdateCheckComplete(info.unwrap_or(None)));
                })
                .detach();
        }

        // Initialize
        self.initialize(proxy);
    }

    pub fn initialize(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        // Start sync command if configured (may create the sync folder)
        if !self.persistent_state.sync_command.is_empty() {
            log::info!("Starting sync command...");
            if self
                .sync_command
                .start(&self.persistent_state.sync_command)
            {
                self.sync_command_started_at = Some(Instant::now());
            }
        }

        if self.sync_folder.is_none() {
            self.sync_folder = self.persistent_state.folder.clone();
        }

        let sync_folder = match &self.sync_folder {
            Some(f) => f.clone(),
            None => {
                log::warn!("No sync folder configured.");
                self.set_tray_tooltip("Please set a sync folder");
                self.rebuild_menu();
                return;
            }
        };

        // Check if folder is accessible
        if !sync_folder.is_dir() {
            log::warn!(
                "Sync folder is not accessible: {}. Waiting for it...",
                sync_folder.display()
            );
            self.set_tray_tooltip("Waiting for folder...");
            return;
        }

        // Start clipboard watcher (for sending)
        if self.persistent_state.is_sending_anything() {
            log::info!("Starting clipboard watcher...");
            let p = proxy.clone();
            let mut watcher_ctx = match ClipboardWatcherContext::new() {
                Ok(ctx) => ctx,
                Err(e) => {
                    log::error!("Failed to create clipboard watcher: {e}");
                    return;
                }
            };
            let handler = ClipboardChangeHandler { proxy: p };
            let shutdown = watcher_ctx.add_handler(handler).get_shutdown_channel();

            std::thread::spawn(move || {
                watcher_ctx.start_watch();
            });

            self.clipboard_watcher_shutdown = Some(shutdown);
        }

        // Start file watcher (for receiving)
        if self.persistent_state.is_receiving_anything() {
            let watch_mode: WatchMode = self.persistent_state.watch_mode.clone();
            log::info!("Starting file watcher...");
            log::info!("Watch mode: {:?}", watch_mode);

            self.start_fs_watcher(proxy, &sync_folder, &watch_mode);

            // Write the initial keep-alive file
            log::info!("Writing keep-alive file...");
            write_keep_alive(&sync_folder, &self.hostname);

            // Spawn periodic keep-alive async task
            let p = proxy.clone();
            self.init_tasks.push(EXECUTOR.spawn(async move {
                loop {
                    smol::Timer::after(Duration::from_secs(KEEP_ALIVE_INTERVAL_SECS)).await;
                    let _ = p.send_event(UserEvent::KeepAlive);
                }
            }));
        }

        // Initial auto-cleanup + periodic cleanup task
        if self.persistent_state.auto_cleanup {
            log::info!("Performing initial cleanup...");
            clean_files(&sync_folder, &self.hostname);

            let p = proxy.clone();
            self.init_tasks.push(EXECUTOR.spawn(async move {
                loop {
                    smol::Timer::after(Duration::from_secs(60)).await;
                    let _ = p.send_event(UserEvent::Cleanup);
                }
            }));
        }

        self.initialized = true;
        self.update_tray_icon(TrayIconState::Working);
        self.set_tray_tooltip("");
        self.rebuild_menu();
        log::info!("Clipboard Sync initialized successfully.");
    }

    fn start_fs_watcher(
        &mut self,
        proxy: &EventLoopProxy<UserEvent>,
        sync_folder: &Path,
        watch_mode: &WatchMode,
    ) {
        self._fs_watcher =
            fs_watcher::create_watcher(proxy, sync_folder, &self.hostname, watch_mode);
    }

    pub fn uninitialize(&mut self, reason: &str) {
        log::info!("Uninitializing Clipboard Sync...");

        self.update_tray_icon(TrayIconState::Suspended);
        self.set_tray_tooltip(reason);

        // Cancel all async init tasks (dropping cancels them)
        self.init_tasks.clear();
        self.icon_revert_task = None;
        self.clipboard_write_task = None;

        // Stop clipboard watcher
        if let Some(shutdown) = self.clipboard_watcher_shutdown.take() {
            log::info!("Stopping clipboard watcher...");
            shutdown.stop();
        }

        // Stop file watcher
        if self._fs_watcher.is_some() {
            log::info!("Stopping file watcher...");
        }
        self._fs_watcher = None;

        // Remove keep-alive file
        if let Some(ref sync_folder) = self.sync_folder {
            log::info!("Removing keep-alive file...");
            let keep_alive_path =
                sync_folder.join(format!("{}{}", self.hostname, IS_RECEIVING_FILE_SUFFIX));
            let _ = std::fs::remove_file(keep_alive_path);
        }

        // Stop sync command
        self.sync_command.stop();

        self.initialized = false;
        log::info!("Clipboard Sync uninitialized.");
    }

    pub fn handle_clipboard_changed(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        if !self.initialized {
            return;
        }

        if self.sync_folder.is_none() {
            return;
        }

        // Clipboard debounce (leading-edge: ignore events too close together)
        let now = now_ms();
        if let Some(last) = self.last_clipboard_event
            && now - last < CLIPBOARD_DEBOUNCE_MS
        {
            return;
        }
        self.last_clipboard_event = Some(now);

        // Cancel any pending write task
        self.clipboard_write_task = None;

        // Schedule delayed write via async timer (replaces blocking sleep)
        let p = proxy.clone();
        self.clipboard_write_task = Some(EXECUTOR.spawn(async move {
            smol::Timer::after(Duration::from_millis(CLIPBOARD_WRITE_DELAY_MS)).await;
            let _ = p.send_event(UserEvent::ClipboardReady);
        }));
    }

    pub fn handle_clipboard_ready(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        if !self.initialized {
            return;
        }

        let sync_folder = match &self.sync_folder {
            Some(f) => f.clone(),
            None => return,
        };

        let sent = write_clipboard_to_file(
            &sync_folder,
            &self.hostname,
            &self.persistent_state,
            &mut self.dedup,
        );

        if sent {
            self.set_icon_for_duration(proxy, TrayIconState::Sent);
        }
    }

    pub fn handle_clipboard_file_detected(&self, path: PathBuf, proxy: &EventLoopProxy<UserEvent>) {
        if self.initialized {
            // Async delay to let the file be fully written
            let p = proxy.clone();
            EXECUTOR
                .spawn(async move {
                    smol::Timer::after(Duration::from_millis(200)).await;
                    let _ = p.send_event(UserEvent::ClipboardFileReady(path));
                })
                .detach();
        }
    }

    pub fn handle_clipboard_file_ready(
        &mut self,
        path: &Path,
        proxy: &EventLoopProxy<UserEvent>,
    ) {
        if !self.initialized {
            return;
        }

        let sync_folder = match &self.sync_folder {
            Some(f) => f.clone(),
            None => return,
        };

        let parsed = parse_clipboard_filename(
            path,
            &sync_folder,
            &self.hostname,
            Some(ClipboardOrigin::Others),
        );

        if let Some(parsed) = parsed {
            let received =
                read_clipboard_from_file(&parsed, &self.persistent_state, &mut self.dedup);

            if received {
                self.set_icon_for_duration(proxy, TrayIconState::Received);
            }
        }
    }

    pub fn handle_reload(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        self.uninitialize("Reloading...");
        self.initialize(proxy);
    }

    pub fn handle_menu_click(&mut self, menu_event: &tray_icon::menu::MenuEvent, proxy: &EventLoopProxy<UserEvent>) {
        let result = handle_menu_event(
            &menu_event.id,
            &self.menu_actions,
            &mut self.persistent_state,
            &self.sync_folder,
            &self.auto_launch,
            &self.update_info,
        );

        if result.save_and_reload {
            // Sync folder may have changed via ChangeFolder action
            self.sync_folder = self.persistent_state.folder.clone();
            if let Err(e) = save_state(&self.persistent_state) {
                log_and_notify_error(
                    "Failed to Save State",
                    &format!("Failed to save state: {e:#}"),
                );
                return;
            }
            let _ = proxy.send_event(UserEvent::Reload);
        }

        if result.rebuild_menu {
            self.rebuild_menu();
        }

        #[cfg(target_os = "windows")]
        if result.restart_onedrive {
            EXECUTOR
                .spawn(async {
                    smol::unblock(crate::platform::restart_onedrive).await;
                })
                .detach();
        }

        match result.update_action {
            UpdateAction::Check => {
                let p = proxy.clone();
                EXECUTOR
                    .spawn(async move {
                        let info = smol::unblock(|| update::check_for_update(true)).await;
                        let _ = p.send_event(UserEvent::UpdateCheckComplete(info.unwrap_or(None)));
                    })
                    .detach();
            }
            UpdateAction::Perform(info) => {
                match update::install_update(&info) {
                    Ok(()) => {
                        self.uninitialize("Updating...");
                        self.tray_icon.take();
                        std::process::exit(0);
                    }
                    Err(e) => {
                        log_and_notify_error("Update Failed", &format!("Update failed: {e:#}"));
                    }
                }
            }
            UpdateAction::None => {}
        }

        if result.quit {
            self.uninitialize("Exiting...");
            self.tray_icon.take();
            std::process::exit(0);
        }
    }

    pub fn handle_revert_icon(&mut self) {
        if self.initialized {
            self.update_tray_icon(TrayIconState::Working);
        }
    }

    pub fn handle_keep_alive(&self) {
        if self.initialized
            && self.persistent_state.is_receiving_anything()
            && let Some(ref sf) = self.sync_folder
        {
            write_keep_alive(sf, &self.hostname);
        }
    }

    pub fn handle_cleanup(&self) {
        if self.initialized
            && self.persistent_state.auto_cleanup
            && let Some(ref sf) = self.sync_folder
        {
            clean_files(sf, &self.hostname);
        }
    }

    pub fn handle_folder_check(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        let now = Instant::now();

        let (check_interval, clear_started) =
            folder_check_interval(self.sync_command_started_at, now);
        if clear_started {
            self.sync_command_started_at = None;
        }
        let folder_check_interval = check_interval;

        let should_check = self
            .last_folder_check
            .map(|t| now.duration_since(t) >= folder_check_interval)
            .unwrap_or(true);

        if !should_check {
            return;
        }

        self.last_folder_check = Some(now);

        if let Some(ref sync_folder) = self.sync_folder {
            let accessible = sync_folder.is_dir();

            if !self.initialized && accessible {
                log::info!("Sync folder is now accessible. Starting Clipboard Sync...");
                self.initialize(proxy);
            } else if self.initialized && !accessible {
                log::info!("Sync folder is no longer accessible. Waiting for it...");
                self.uninitialize("Folder unavailable");
            }
        }
    }

    pub fn handle_idle_check(&mut self, proxy: &EventLoopProxy<UserEvent>) {
        let idle_secs = match user_idle2::UserIdle::get_time() {
            Ok(idle) => idle.as_seconds(),
            Err(e) => {
                log::error!("Failed to get idle time: {e}");
                return;
            }
        };

        if idle_secs >= IDLE_TIMEOUT_SECS {
            if self.initialized {
                log::info!("System is idle ({idle_secs}s). Suspending...");
                self.suspended_by_idle = true;
                self.uninitialize("System is idle");
            }
        } else if self.suspended_by_idle {
            log::info!("System is no longer idle. Resuming...");
            self.suspended_by_idle = false;
            self.initialize(proxy);
        }
    }

    pub fn handle_sync_command_check(&mut self) {
        if let Some(status) = self.sync_command.check() {
            let msg = format!("The sync command exited unexpectedly with status: {status}");
            if let Err(e) = send_notification("Sync command failed", &msg, NotificationDuration::Short) {
                log::error!("Failed to send sync command notification: {e:#}");
            }
            self.uninitialize("Sync command failed");
        }
    }

    pub fn handle_update_check_complete(&mut self, info: Option<UpdateInfo>) {
        self.update_info = info;
        self.rebuild_menu();
    }

    fn set_icon_for_duration(
        &mut self,
        proxy: &EventLoopProxy<UserEvent>,
        icon: TrayIconState,
    ) {
        self.update_tray_icon(icon);

        // Cancel any existing revert task
        self.icon_revert_task = None;

        // Spawn one-shot async timer to revert icon
        let p = proxy.clone();
        self.icon_revert_task = Some(EXECUTOR.spawn(async move {
            smol::Timer::after(Duration::from_secs(ICON_FLASH_DURATION_SECS)).await;
            let _ = p.send_event(UserEvent::RevertIcon);
        }));
    }

    fn update_tray_icon(&mut self, icon: TrayIconState) {
        if self.current_icon == icon {
            return;
        }
        self.current_icon = icon;
        if let Some(handle) = &self.tray_icon {
            let _ = handle.set_icon(Some(tray::get_tray_icon(icon)));
        }
    }

    fn set_tray_tooltip(&self, status: &str) {
        let tooltip = format_tooltip(status);
        if let Some(handle) = &self.tray_icon {
            let _ = handle.set_tooltip(Some(&tooltip));
        }
    }

    pub fn rebuild_menu(&mut self) {
        let auto_launch_enabled = self.auto_launch.is_enabled().unwrap_or(false);

        let new_actions = rebuild_tray_menu(
            &self.tray_menu,
            &self.persistent_state,
            auto_launch_enabled,
            &self.update_info,
        );
        self.menu_actions = new_actions;
    }
}



fn write_keep_alive(sync_folder: &Path, hostname: &str) {
    let path = sync_folder.join(format!("{hostname}{IS_RECEIVING_FILE_SUFFIX}"));
    let _ = std::fs::write(&path, format!("{}", now_ms()));
}

/// Compute the folder check interval based on how long the sync command has been running.
///
/// Returns `(interval, should_clear_started_at)`.
fn folder_check_interval(
    sync_command_started_at: Option<Instant>,
    now: Instant,
) -> (Duration, bool) {
    match sync_command_started_at {
        Some(t) if now.duration_since(t) < Duration::from_secs(SYNC_COMMAND_WAIT_SECS) => {
            (Duration::from_secs(1), false)
        }
        Some(_) => (Duration::from_secs(30), true),
        None => (Duration::from_secs(30), false),
    }
}

/// Format the tray icon tooltip text.
fn format_tooltip(status: &str) -> String {
    if status.is_empty() {
        format!("{APP_NAME} v{CURRENT_VERSION}")
    } else {
        format!("{APP_NAME} - {status}")
    }
}


/// Spawn a detached async task that sends an event every `interval`.
fn spawn_periodic_event(
    proxy: &EventLoopProxy<UserEvent>,
    interval: Duration,
    make_event: fn() -> UserEvent,
) {
    let p = proxy.clone();
    EXECUTOR
        .spawn(async move {
            loop {
                smol::Timer::after(interval).await;
                let _ = p.send_event(make_event());
            }
        })
        .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn folder_check_interval_no_sync_command() {
        let (interval, clear) = folder_check_interval(None, Instant::now());
        assert_eq!(interval, Duration::from_secs(30));
        assert!(!clear);
    }

    #[test]
    fn folder_check_interval_during_wait() {
        let started = Instant::now();
        let now = started + Duration::from_secs(1);
        let (interval, clear) = folder_check_interval(Some(started), now);
        assert_eq!(interval, Duration::from_secs(1));
        assert!(!clear);
    }

    #[test]
    fn folder_check_interval_after_wait_expired() {
        let started = Instant::now() - Duration::from_secs(SYNC_COMMAND_WAIT_SECS + 1);
        let (interval, clear) = folder_check_interval(Some(started), Instant::now());
        assert_eq!(interval, Duration::from_secs(30));
        assert!(clear);
    }

    #[test]
    fn format_tooltip_empty_shows_version() {
        let tooltip = format_tooltip("");
        assert!(tooltip.contains(APP_NAME));
        assert!(tooltip.contains(CURRENT_VERSION));
    }

    #[test]
    fn format_tooltip_with_status() {
        let tooltip = format_tooltip("Waiting for folder...");
        assert!(tooltip.contains(APP_NAME));
        assert!(tooltip.contains("Waiting for folder..."));
        assert!(!tooltip.contains(CURRENT_VERSION));
    }

    #[test]
    fn write_keep_alive_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        write_keep_alive(dir.path(), "testhost");
        let expected = dir.path().join(format!("testhost{IS_RECEIVING_FILE_SUFFIX}"));
        assert!(expected.exists());
        let content = std::fs::read_to_string(&expected).unwrap();
        // Content should be a numeric timestamp
        assert!(content.parse::<u64>().is_ok());
    }
}

/// Spawn always-running periodic async tasks that send events to the main event loop.
pub fn spawn_periodic_tasks(proxy: &EventLoopProxy<UserEvent>) {
    spawn_periodic_event(proxy, Duration::from_secs(1), || UserEvent::CheckFolderAccess);
    spawn_periodic_event(proxy, Duration::from_secs(1), || UserEvent::CheckIdleState);
    spawn_periodic_event(proxy, Duration::from_secs(1), || UserEvent::CheckSyncCommand);
}