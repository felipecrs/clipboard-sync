#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod clipboard;
mod config;
mod consts;
mod platform;
mod sync_command;
mod types;
mod ui;
mod update;
mod utils;

use crate::clipboard::{
    clean_files, now_ms, parse_clipboard_filename, read_clipboard_from_file,
    write_clipboard_to_file,
};
use crate::config::{load_config, save_config, Config, WatchMode};
use crate::consts::*;
use crate::platform::{init_platform, send_notification, NotificationDuration};
use crate::sync_command::SyncCommand;
use crate::types::*;
use crate::ui::{build_tray_menu, MenuAction};
use crate::update::UpdateInfo;
use crate::utils::{get_executable_directory, get_executable_path, get_hostname, open_path, open_url};

use auto_launch::AutoLaunchBuilder;
use clipboard_rs::{ClipboardHandler, ClipboardWatcher, ClipboardWatcherContext, WatcherShutdown};
use faccess::PathExt;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use simplelog::*;
use single_instance::SingleInstance;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{MenuEvent, MenuId};
use tray_icon::{TrayIconBuilder, TrayIconEvent};

// --- Clipboard watcher handler ---

struct ClipboardChangeHandler {
    proxy: tao::event_loop::EventLoopProxy<UserEvent>,
}

impl ClipboardHandler for ClipboardChangeHandler {
    fn on_clipboard_change(&mut self) {
        let _ = self.proxy.send_event(UserEvent::ClipboardChanged);
    }
}

// --- Application state ---

struct AppState {
    config: Config,
    hostname: String,
    sync_folder: Option<PathBuf>,

    initialized: bool,

    // Clipboard dedup state
    last_beat: Option<u64>,
    last_text_written: Option<ClipboardText>,
    last_text_read: Option<ClipboardText>,
    last_image_sha256_written: Option<String>,
    last_image_sha256_read: Option<String>,
    last_file_paths_read: Option<Vec<String>>,

    // Clipboard watcher
    clipboard_watcher_shutdown: Option<WatcherShutdown>,

    // File system watcher (kept alive to maintain the watch)
    _fs_watcher: Option<Box<dyn Watcher + Send>>,

    // Sync command
    sync_command: SyncCommand,

    // Auto-launch
    auto_launch_enabled: bool,

    // Update
    update_info: Option<UpdateInfo>,

    // Icon state
    current_icon: TrayIconState,
    icon_revert_time: Option<Instant>,

    // For clipboard change debouncing
    last_clipboard_event: Option<u64>,

    // Idle suspension tracking
    suspended_by_idle: bool,

    // Timer state for periodic tasks
    last_keep_alive: Option<Instant>,
    last_clean: Option<Instant>,
    last_folder_check: Option<Instant>,
    sync_command_started_at: Option<Instant>,

    // Menu action map
    menu_actions: HashMap<MenuId, MenuAction>,
}

fn get_tray_icon(state: TrayIconState) -> tray_icon::Icon {
    #[cfg(target_os = "windows")]
    {
        let resource_name = match state {
            TrayIconState::Working => "working-icon",
            TrayIconState::Sent => "sent-icon",
            TrayIconState::Received => "received-icon",
            TrayIconState::Suspended => "suspended-icon",
        };
        tray_icon::Icon::from_resource_name(resource_name, None).unwrap()
    }
    #[cfg(not(target_os = "windows"))]
    {
        // On non-Windows, load from PNG files next to the executable
        let icon_name = match state {
            TrayIconState::Working => "working",
            TrayIconState::Sent => "sent",
            TrayIconState::Received => "received",
            TrayIconState::Suspended => "suspended",
        };
        let icon_path = get_executable_directory()
            .join("resources/trayicons/png")
            .join(format!("{icon_name}.png"));
        let bytes = std::fs::read(&icon_path).unwrap_or_else(|_| PNG_ICON_BYTES.to_vec());
        // Decode PNG to RGBA
        let decoder = png::Decoder::new(std::io::Cursor::new(&bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buf = vec![0; reader.output_buffer_size()];
        let info = reader.next_frame(&mut buf).unwrap();
        buf.truncate(info.buffer_size());
        tray_icon::Icon::from_rgba(buf, info.width, info.height).unwrap()
    }
}

fn main() {
    let executable_directory = get_executable_directory();

    init_platform(&executable_directory);

    if !executable_directory.writable() {
        let error_title = "Clipboard Sync Directory Not Writable";
        let error_message = format!(
            "Please move Clipboard Sync to a directory that is writable or fix the permissions of '{}'.",
            executable_directory.display(),
        );

        eprintln!("{error_title}: {error_message}");

        if let Err(e) = send_notification(error_title, &error_message, NotificationDuration::Long) {
            eprintln!("Failed to show {error_title} notification: {e}");
        }

        std::process::exit(1);
    }

    let log_path = executable_directory.join(LOG_FILE_NAME);
    let loggers: Vec<Box<dyn SharedLogger>> = vec![
        WriteLogger::new(
            LevelFilter::Info,
            simplelog::Config::default(),
            File::create(&log_path).expect("Failed to create log file"),
        ),
        #[cfg(debug_assertions)]
        TermLogger::new(
            LevelFilter::Info,
            simplelog::Config::default(),
            TerminalMode::Stderr,
            ColorChoice::Auto,
        ),
    ];

    CombinedLogger::init(loggers).expect("Failed to init logger");

    // Set panic hook to log panic info
    std::panic::set_hook(Box::new(|panic_info| {
        log::error!("Panic occurred: {panic_info}");
    }));

    // Only allow one instance
    let instance = SingleInstance::new(APP_UID).expect("Failed to create single instance");
    if !instance.is_single() {
        log::error!("Another instance is already running.");
        std::process::exit(1);
    }

    let hostname = get_hostname();
    log::info!("Hostname: {hostname}");

    let config = load_config();
    log::info!("Loaded config: {:?}", config);


    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    // Set up a Win32 job object so all child processes are killed when we exit
    #[cfg(target_os = "windows")]
    let _job = {
        let job = win32job::Job::create().expect("Failed to create Win32 job object");
        let mut info = job.query_extended_limit_info().unwrap();
        info.limit_kill_on_job_close();
        job.set_extended_limit_info(&mut info).unwrap();
        job.assign_current_process().unwrap();
        job // keep alive for the lifetime of the process
    };

    // Set up event handlers for tray icon and menu
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::TrayIcon(event));
    }));
    TrayIconEvent::receiver();

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        let _ = proxy.send_event(UserEvent::Menu(event));
    }));
    MenuEvent::receiver();

    // Build initial menu (auto-launch status will be checked when initializing)
    let (tray_menu, menu_actions) =
        build_tray_menu(&config, false, &None, &config.folder);

    let mut tray_icon_handle = None;

    let mut state = AppState {
        hostname,
        sync_folder: config.folder.as_ref().map(PathBuf::from),
        config,
        initialized: false,
        last_beat: None,
        last_text_written: None,
        last_text_read: None,
        last_image_sha256_written: None,
        last_image_sha256_read: None,
        last_file_paths_read: None,
        clipboard_watcher_shutdown: None,
        _fs_watcher: None,
        sync_command: SyncCommand::new(),
        auto_launch_enabled: false,
        update_info: None,
        current_icon: TrayIconState::Suspended,
        icon_revert_time: None,
        last_clipboard_event: None,
        suspended_by_idle: false,
        last_keep_alive: None,
        last_clean: None,
        last_folder_check: None,
        sync_command_started_at: None,
        menu_actions,
    };

    let main_proxy = event_loop.create_proxy();

    event_loop.run(move |event, _, control_flow| {
        // Use WaitUntil with a 1-second interval for timer-based tasks
        *control_flow = ControlFlow::WaitUntil(Instant::now() + Duration::from_secs(TIMER_TICK_INTERVAL_SECS));

        match event {
            Event::NewEvents(tao::event::StartCause::Init) => {
                let tooltip = format!("{APP_NAME} v{CURRENT_VERSION}");
                tray_icon_handle = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(tray_menu.clone()))
                        .with_tooltip(&tooltip)
                        .with_icon(get_tray_icon(TrayIconState::Suspended))
                        .with_id(APP_UID)
                        .build()
                        .expect("Failed to build tray icon"),
                );

                // Auto-check for updates before initializing so menu reflects update status
                state.update_info = update::check(true);

                // Initialize
                initialize(&mut state, &main_proxy, &tray_icon_handle);
            }

            Event::NewEvents(tao::event::StartCause::ResumeTimeReached { .. }) => {
                handle_timer_tick(&mut state, &main_proxy, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::ClipboardChanged) => {
                handle_clipboard_changed(&mut state, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::ClipboardFileDetected(path)) => {
                handle_clipboard_file_detected(&mut state, &path, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::Reload) => {
                uninitialize(&mut state, &tray_icon_handle, "Reloading...");
                initialize(&mut state, &main_proxy, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::Menu(menu_event)) => {
                handle_menu_event(
                    &menu_event.id,
                    &mut state,
                    &main_proxy,
                    &tray_icon_handle,
                );
            }

            Event::UserEvent(UserEvent::TrayIcon(_tray_event)) => {
                // Tray icon events (click, double-click) can be handled here if needed
            }

            _ => {}
        }
    });
}

fn initialize(
    state: &mut AppState,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    // Start sync command if configured (may create the sync folder)
    if !state.config.sync_command.is_empty() {
        log::info!("Starting sync command...");
        if state.sync_command.start(&state.config.sync_command) {
            state.sync_command_started_at = Some(Instant::now());
        }
    }

    if state.sync_folder.is_none() {
        if let Some(ref folder) = state.config.folder {
            state.sync_folder = Some(PathBuf::from(folder));
        }
    }

    let sync_folder = match &state.sync_folder {
        Some(f) => f.clone(),
        None => {
            log::warn!("No sync folder configured.");
            set_tray_tooltip(tray_icon_handle, "Please set a sync folder");
            return;
        }
    };

    // Check if folder is accessible
    if !sync_folder.is_dir() {
        log::warn!(
            "Sync folder is not accessible: {}. Waiting for it...",
            sync_folder.display()
        );
        set_tray_tooltip(tray_icon_handle, "Waiting for folder...");
        return;
    }

    // Start clipboard watcher (for sending)
    if state.config.is_sending_anything() {
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

        state.clipboard_watcher_shutdown = Some(shutdown);
    }

    // Start file watcher (for receiving)
    if state.config.is_receiving_anything() {
        let watch_mode: WatchMode = state.config.watch_mode.clone();
        log::info!("Starting file watcher...");
        log::info!("Watch mode: {:?}", watch_mode);

        start_fs_watcher(state, proxy, &sync_folder, &watch_mode);

        // Write the initial keep-alive file
        log::info!("Writing keep-alive file...");
        write_keep_alive(&sync_folder, &state.hostname);
        state.last_keep_alive = Some(Instant::now());
    }

    // Initial auto-cleanup
    if state.config.auto_cleanup {
        log::info!("Performing initial cleanup...");
        clean_files(&sync_folder, &state.hostname);
        state.last_clean = Some(Instant::now());
    }

    state.initialized = true;
    update_tray_icon(state, tray_icon_handle, TrayIconState::Working);
    set_tray_tooltip(tray_icon_handle, "");
    rebuild_menu(state, tray_icon_handle);
    log::info!("Clipboard Sync initialized successfully.");
}

fn start_fs_watcher(
    state: &mut AppState,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    sync_folder: &Path,
    watch_mode: &WatchMode,
) {
    let p = proxy.clone();
    let sf = sync_folder.to_path_buf();
    let hn = state.hostname.clone();

    let event_handler = move |res: Result<notify::Event, notify::Error>| {
        handle_fs_event(res, &sf, &hn, &p);
    };

    let watcher: Option<Box<dyn Watcher + Send>> = if *watch_mode == WatchMode::Polling {
        let config = notify::Config::default().with_poll_interval(Duration::from_secs(FS_WATCHER_POLL_INTERVAL_SECS));
        match notify::PollWatcher::new(event_handler, config) {
            Ok(mut w) => {
                if let Err(e) = w.watch(sync_folder, RecursiveMode::NonRecursive) {
                    log::error!("Failed to watch sync folder: {e}");
                }
                Some(Box::new(w))
            }
            Err(e) => {
                log::error!("Failed to create poll watcher: {e}");
                None
            }
        }
    } else {
        let config = notify::Config::default();
        match RecommendedWatcher::new(event_handler, config) {
            Ok(mut w) => {
                if let Err(e) = w.watch(sync_folder, RecursiveMode::NonRecursive) {
                    log::error!("Failed to watch sync folder: {e}");
                }
                Some(Box::new(w))
            }
            Err(e) => {
                log::error!("Failed to create native watcher: {e}");
                None
            }
        }
    };

    state._fs_watcher = watcher;
}

fn uninitialize(
    state: &mut AppState,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
    reason: &str,
) {
    log::info!("Uninitializing Clipboard Sync...");

    update_tray_icon(state, tray_icon_handle, TrayIconState::Suspended);
    set_tray_tooltip(tray_icon_handle, reason);

    // Stop clipboard watcher
    if let Some(shutdown) = state.clipboard_watcher_shutdown.take() {
        log::info!("Stopping clipboard watcher...");
        shutdown.stop();
    }

    // Stop file watcher
    if state._fs_watcher.is_some() {
        log::info!("Stopping file watcher...");
    }
    state._fs_watcher = None;

    // Remove keep-alive file
    if let Some(ref sync_folder) = state.sync_folder {
        log::info!("Removing keep-alive file...");
        let keep_alive_path = sync_folder.join(format!(
            "{}{}",
            state.hostname, IS_RECEIVING_FILE_SUFFIX
        ));
        let _ = std::fs::remove_file(keep_alive_path);
    }

    // Stop sync command
    state.sync_command.stop();

    state.initialized = false;
    log::info!("Clipboard Sync uninitialized.");
}

fn write_keep_alive(sync_folder: &Path, hostname: &str) {
    let path = sync_folder.join(format!("{hostname}{IS_RECEIVING_FILE_SUFFIX}"));
    let _ = std::fs::write(&path, format!("{}", now_ms()));
}

fn handle_fs_event(
    res: Result<notify::Event, notify::Error>,
    sync_folder: &Path,
    hostname: &str,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
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

fn handle_clipboard_changed(
    state: &mut AppState,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    if !state.initialized {
        return;
    }

    let sync_folder = match &state.sync_folder {
        Some(f) => f.clone(),
        None => return,
    };

    // Clipboard debounce
    let now = now_ms();
    if let Some(last) = state.last_clipboard_event {
        if now - last < CLIPBOARD_DEBOUNCE_MS {
            return;
        }
    }
    state.last_clipboard_event = Some(now);

    // Small delay to let clipboard be fully written
    std::thread::sleep(Duration::from_millis(CLIPBOARD_WRITE_DELAY_MS));

    let sent = write_clipboard_to_file(
        &sync_folder,
        &state.hostname,
        &state.config,
        &mut state.last_beat,
        &mut state.last_text_written,
        &mut state.last_image_sha256_written,
        &state.last_text_read,
        &state.last_image_sha256_read,
        &state.last_file_paths_read,
    );

    if sent {
        set_icon_for_duration(state, tray_icon_handle, TrayIconState::Sent);
    }
}

fn handle_clipboard_file_detected(
    state: &mut AppState,
    path: &Path,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    if !state.initialized {
        return;
    }

    let sync_folder = match &state.sync_folder {
        Some(f) => f.clone(),
        None => return,
    };

    // Small delay to let the file be fully written
    std::thread::sleep(Duration::from_millis(200));

    let parsed = parse_clipboard_filename(
        path,
        &sync_folder,
        &state.hostname,
        Some(ClipboardOrigin::Others),
    );

    if let Some(parsed) = parsed {
        let received = read_clipboard_from_file(
            &parsed,
            &state.config,
            &mut state.last_beat,
            &mut state.last_text_read,
            &mut state.last_image_sha256_read,
            &mut state.last_file_paths_read,
        );

        if received {
            set_icon_for_duration(state, tray_icon_handle, TrayIconState::Received);
        }
    }
}

fn handle_timer_tick(
    state: &mut AppState,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    let now = Instant::now();

    // Revert icon if needed
    if let Some(revert_time) = state.icon_revert_time {
        if now >= revert_time {
            state.icon_revert_time = None;
            if state.initialized {
                update_tray_icon(state, tray_icon_handle, TrayIconState::Working);
            }
        }
    }

    // Check sync command health
    if let Some(status) = state.sync_command.check() {
        let msg = format!("The sync command exited unexpectedly with status: {status}");
        let _ = send_notification("Sync command failed", &msg, NotificationDuration::Short);
        uninitialize(state, tray_icon_handle, "Sync command failed");
    }

    // Folder accessibility check
    // Check every 1s for SYNC_COMMAND_WAIT_SECS after starting a sync command, then every 30s
    let folder_check_interval = match state.sync_command_started_at {
        Some(t) if now.duration_since(t) < Duration::from_secs(SYNC_COMMAND_WAIT_SECS) => {
            Duration::from_secs(1)
        }
        Some(_) => {
            state.sync_command_started_at = None;
            Duration::from_secs(30)
        }
        None => Duration::from_secs(30),
    };
    let should_check_folder = state
        .last_folder_check
        .map(|t| now.duration_since(t) >= folder_check_interval)
        .unwrap_or(true);

    if should_check_folder {
        state.last_folder_check = Some(now);

        if let Some(ref sync_folder) = state.sync_folder {
            let accessible = sync_folder.is_dir();

            if !state.initialized && accessible {
                log::info!("Sync folder is now accessible. Starting Clipboard Sync...");
                initialize(state, proxy, tray_icon_handle);
            } else if state.initialized && !accessible {
                log::info!("Sync folder is no longer accessible. Waiting for it...");
                uninitialize(state, tray_icon_handle, "Folder unavailable");
            }
        }
    }

    // Idle detection (must run even when not initialized to detect system becoming active)
    check_idle_state(state, proxy, tray_icon_handle);

    if !state.initialized {
        return;
    }

    let sync_folder = match &state.sync_folder {
        Some(f) => f.clone(),
        None => return,
    };

    // Keep-alive (every 4 minutes)
    if state.config.is_receiving_anything() {
        let should_keep_alive = state
            .last_keep_alive
            .map(|t| now.duration_since(t) >= Duration::from_secs(KEEP_ALIVE_INTERVAL_SECS))
            .unwrap_or(true);

        if should_keep_alive {
            write_keep_alive(&sync_folder, &state.hostname);
            state.last_keep_alive = Some(now);
        }
    }

    // Auto-cleanup (every 1 minute)
    if state.config.auto_cleanup {
        let should_clean = state
            .last_clean
            .map(|t| now.duration_since(t) >= Duration::from_secs(60))
            .unwrap_or(true);

        if should_clean {
            clean_files(&sync_folder, &state.hostname);
            state.last_clean = Some(now);
        }
    }
}

fn check_idle_state(
    state: &mut AppState,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    let idle_secs = match user_idle2::UserIdle::get_time() {
        Ok(idle) => idle.as_seconds(),
        Err(e) => {
            log::error!("Failed to get idle time: {e}");
            return;
        }
    };

    if idle_secs >= IDLE_TIMEOUT_SECS {
        if state.initialized {
            log::info!("System is idle ({idle_secs}s). Suspending...");
            state.suspended_by_idle = true;
            uninitialize(state, tray_icon_handle, "System is idle");
        }
    } else if state.suspended_by_idle {
        log::info!("System is no longer idle. Resuming...");
        state.suspended_by_idle = false;
        initialize(state, proxy, tray_icon_handle);
    }
}

fn set_icon_for_duration(
    state: &mut AppState,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
    icon: TrayIconState,
) {
    update_tray_icon(state, tray_icon_handle, icon);
    state.icon_revert_time = Some(Instant::now() + Duration::from_secs(ICON_FLASH_DURATION_SECS));
}

fn update_tray_icon(
    state: &mut AppState,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
    icon: TrayIconState,
) {
    if state.current_icon == icon {
        return;
    }
    state.current_icon = icon;
    if let Some(handle) = tray_icon_handle {
        let _ = handle.set_icon(Some(get_tray_icon(icon)));
    }
}

fn set_tray_tooltip(
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
    status: &str,
) {
    let tooltip = if status.is_empty() {
        format!("{APP_NAME} v{CURRENT_VERSION}")
    } else {
        format!("{APP_NAME} - {status}")
    };
    if let Some(handle) = tray_icon_handle {
        let _ = handle.set_tooltip(Some(&tooltip));
    }
}

fn rebuild_menu(
    state: &mut AppState,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    let app_path = get_executable_path().to_str().unwrap().to_string();
    let auto_launch = AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&app_path)
        .build()
        .expect("Failed to build auto launch");

    state.auto_launch_enabled = auto_launch.is_enabled().unwrap_or(false);

    let (new_menu, new_actions) = build_tray_menu(
        &state.config,
        state.auto_launch_enabled,
        &state.update_info,
        &state.config.folder,
    );

    state.menu_actions = new_actions;

    if let Some(handle) = tray_icon_handle {
        let _ = handle.set_menu(Some(Box::new(new_menu)));
    }
}

fn handle_menu_event(
    menu_id: &MenuId,
    state: &mut AppState,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    // Clone the action to avoid borrowing issues
    let action = match state.menu_actions.get(menu_id) {
        Some(MenuAction::ToggleSendTexts) => MenuAction::ToggleSendTexts,
        Some(MenuAction::ToggleSendImages) => MenuAction::ToggleSendImages,
        Some(MenuAction::ToggleSendFiles) => MenuAction::ToggleSendFiles,
        Some(MenuAction::ToggleReceiveTexts) => MenuAction::ToggleReceiveTexts,
        Some(MenuAction::ToggleReceiveImages) => MenuAction::ToggleReceiveImages,
        Some(MenuAction::ToggleReceiveFiles) => MenuAction::ToggleReceiveFiles,
        Some(MenuAction::SetWatchModeNative) => MenuAction::SetWatchModeNative,
        Some(MenuAction::SetWatchModePolling) => MenuAction::SetWatchModePolling,
        Some(MenuAction::ToggleAutoCleanup) => MenuAction::ToggleAutoCleanup,
        Some(MenuAction::ToggleAutoStart) => MenuAction::ToggleAutoStart,
        Some(MenuAction::SetSyncCommand) => MenuAction::SetSyncCommand,
        Some(MenuAction::ChangeFolder) => MenuAction::ChangeFolder,
        Some(MenuAction::OpenFolder) => MenuAction::OpenFolder,
        Some(MenuAction::RestartOneDrive) => MenuAction::RestartOneDrive,
        Some(MenuAction::CheckForUpdates) => MenuAction::CheckForUpdates,
        Some(MenuAction::OpenGitHub) => MenuAction::OpenGitHub,
        Some(MenuAction::Quit) => MenuAction::Quit,
        None => return,
    };

    match action {
        MenuAction::ToggleSendTexts => {
            state.config.send_texts = !state.config.send_texts;
            save_config(&state.config);
            let _ = proxy.send_event(UserEvent::Reload);
        }
        MenuAction::ToggleSendImages => {
            state.config.send_images = !state.config.send_images;
            save_config(&state.config);
            let _ = proxy.send_event(UserEvent::Reload);
        }
        MenuAction::ToggleSendFiles => {
            state.config.send_files = !state.config.send_files;
            save_config(&state.config);
            let _ = proxy.send_event(UserEvent::Reload);
        }
        MenuAction::ToggleReceiveTexts => {
            state.config.receive_texts = !state.config.receive_texts;
            save_config(&state.config);
            let _ = proxy.send_event(UserEvent::Reload);
        }
        MenuAction::ToggleReceiveImages => {
            state.config.receive_images = !state.config.receive_images;
            save_config(&state.config);
            let _ = proxy.send_event(UserEvent::Reload);
        }
        MenuAction::ToggleReceiveFiles => {
            state.config.receive_files = !state.config.receive_files;
            save_config(&state.config);
            let _ = proxy.send_event(UserEvent::Reload);
        }
        MenuAction::SetWatchModeNative => {
            state.config.watch_mode = WatchMode::Native;
            save_config(&state.config);
            let _ = proxy.send_event(UserEvent::Reload);
        }
        MenuAction::SetWatchModePolling => {
            state.config.watch_mode = WatchMode::Polling;
            save_config(&state.config);
            let _ = proxy.send_event(UserEvent::Reload);
        }
        MenuAction::ToggleAutoCleanup => {
            state.config.auto_cleanup = !state.config.auto_cleanup;
            save_config(&state.config);
            let _ = proxy.send_event(UserEvent::Reload);
        }
        MenuAction::ToggleAutoStart => {
            let app_path = get_executable_path().to_str().unwrap().to_string();
            let auto_launch = AutoLaunchBuilder::new()
                .set_app_name(APP_NAME)
                .set_app_path(&app_path)
                .build()
                .expect("Failed to build auto launch");

            let new_state = !state.auto_launch_enabled;
            if new_state {
                let _ = auto_launch.enable();
            } else {
                let _ = auto_launch.disable();
            }
            state.auto_launch_enabled = new_state;
            rebuild_menu(state, tray_icon_handle);
        }
        MenuAction::SetSyncCommand => {
            let current = &state.config.sync_command;
            let default = if current.is_empty() { "" } else { current.as_str() };
            if let Some(cmd) = tinyfiledialogs::input_box(
                "Sync command",
                "Enter a command to run before syncing (leave empty to disable):",
                default,
            ) {
                state.config.sync_command = cmd;
                save_config(&state.config);
                let _ = proxy.send_event(UserEvent::Reload);
            }
        }
        MenuAction::ChangeFolder => {
            if let Some(folder) = pick_folder() {
                state.config.folder = Some(folder.clone());
                state.sync_folder = Some(PathBuf::from(&folder));
                save_config(&state.config);
                let _ = proxy.send_event(UserEvent::Reload);
            }
        }
        MenuAction::OpenFolder => {
            if let Some(ref folder) = state.sync_folder {
                open_path(folder);
            }
        }
        MenuAction::RestartOneDrive => {
            #[cfg(target_os = "windows")]
            {
                crate::platform::restart_onedrive();
            }
        }
        MenuAction::CheckForUpdates => {
            let update = update::check(false);
            if let Some(info) = update {
                let download_url = crate::update::get_download_url(&info);
                let _ = send_notification(
                    "Update available",
                    &format!("v{} is available. Opening download page...", info.latest_version),
                    NotificationDuration::Short,
                );
                open_url(&download_url);
                state.update_info = Some(info);
                rebuild_menu(state, tray_icon_handle);
            } else {
                let _ = send_notification(
                    "No updates found",
                    "You are already running the latest version.",
                    NotificationDuration::Short,
                );
            }
        }
        MenuAction::OpenGitHub => {
            open_url(GITHUB_REPO_URL);
        }
        MenuAction::Quit => {
            uninitialize(state, tray_icon_handle, "Exiting...");
            std::process::exit(0);
        }
    }
}

/// Pick a folder using a cross-platform dialog.
fn pick_folder() -> Option<String> {
    tinyfiledialogs::select_folder_dialog(
        "Select folder to save and read clipboard files",
        "",
    )
}
