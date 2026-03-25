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
    ClipboardDedupState, clean_files, now_ms, parse_clipboard_filename, read_clipboard_from_file,
    write_clipboard_to_file,
};
use crate::config::{PersistentState, WatchMode, load_state, save_state};
use crate::consts::*;
use crate::platform::{NotificationDuration, init_platform, send_notification};
use crate::sync_command::SyncCommand;
use crate::types::*;
use crate::ui::{MenuAction, UpdateAction, rebuild_tray_menu};
use crate::update::UpdateInfo;
use crate::utils::{
    get_executable_directory, get_executable_path_str, get_hostname, log_and_notify_error,
};

use anyhow::Context;
use auto_launch::AutoLaunchBuilder;
use clipboard_rs::{ClipboardHandler, ClipboardWatcher, ClipboardWatcherContext, WatcherShutdown};
use faccess::PathExt;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use simplelog::*;
use single_instance::SingleInstance;
use smol::Task;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuId};
use tray_icon::{TrayIconBuilder, TrayIconEvent};

/// Global async executor for background tasks.
static EXECUTOR: smol::Executor<'static> = smol::Executor::new();

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
    persistent_state: PersistentState,
    hostname: String,
    sync_folder: Option<PathBuf>,

    initialized: bool,

    // Clipboard dedup state
    dedup: ClipboardDedupState,

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

    // For clipboard change debouncing
    last_clipboard_event: Option<u64>,

    // Idle suspension tracking
    suspended_by_idle: bool,

    // Folder check timing (dynamic interval)
    last_folder_check: Option<Instant>,
    sync_command_started_at: Option<Instant>,

    // Tray menu (kept alive; rebuilt in place when state changes)
    tray_menu: Menu,

    // Menu action map
    menu_actions: HashMap<MenuId, MenuAction>,

    // Async task handles - tasks created during initialize(), cancelled during uninitialize()
    init_tasks: Vec<Task<()>>,

    // One-shot async task handles
    icon_revert_task: Option<Task<()>>,
    clipboard_write_task: Option<Task<()>>,
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
    if let Err(error) = run() {
        eprintln!("Fatal error: {error:#}");
        log::error!("Fatal error: {error:#}");
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    let executable_directory = get_executable_directory();

    init_platform(&executable_directory)?;

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
            File::create(&log_path).context("failed to create log file")?,
        ),
        #[cfg(debug_assertions)]
        TermLogger::new(
            LevelFilter::Info,
            simplelog::Config::default(),
            TerminalMode::Stderr,
            ColorChoice::Auto,
        ),
    ];

    CombinedLogger::init(loggers).context("failed to init logger")?;

    // Set panic hook to log panic info
    std::panic::set_hook(Box::new(|panic_info| {
        log::error!("Panic occurred: {panic_info}");
    }));

    // Only allow one instance
    let instance = SingleInstance::new(APP_UID).context("failed to create single instance")?;
    if !instance.is_single() {
        log::error!("Another instance is already running.");
        std::process::exit(1);
    }

    let hostname = get_hostname();
    log::info!("Hostname: {hostname}");

    let persistent_state = match load_state() {
        Ok(persistent_state) => persistent_state,
        Err(e) => {
            let message = format!(
                "{e}\n\nExiting to prevent overwriting your preferences. Please check the state file."
            );
            log_and_notify_error("Failed to Load Preferences", &message);
            std::process::exit(1);
        }
    };
    log::info!("Loaded: {:?}", persistent_state);

    // Start async executor worker thread
    std::thread::spawn(|| smol::block_on(EXECUTOR.run(smol::future::pending::<()>())));

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    // Set up a Win32 job object so all child processes are killed when we exit
    #[cfg(target_os = "windows")]
    let _job = {
        let job = win32job::Job::create().context("failed to create Win32 job object")?;
        let mut info = job
            .query_extended_limit_info()
            .context("failed to query Win32 job limits")?;
        info.limit_kill_on_job_close();
        job.set_extended_limit_info(&info)
            .context("failed to set Win32 job limits")?;
        job.assign_current_process()
            .context("failed to assign process to Win32 job")?;
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
    let tray_menu = Menu::new();
    let menu_actions = rebuild_tray_menu(&tray_menu, &persistent_state, false, &None);

    let mut tray_icon_handle = None;

    let mut state = AppState {
        hostname,
        sync_folder: persistent_state.folder.as_ref().map(PathBuf::from),
        persistent_state,
        initialized: false,
        dedup: ClipboardDedupState::default(),
        clipboard_watcher_shutdown: None,
        _fs_watcher: None,
        sync_command: SyncCommand::new(),
        auto_launch_enabled: false,
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
    };

    let main_proxy = event_loop.create_proxy();

    // Spawn always-running periodic async tasks
    spawn_periodic_tasks(&main_proxy);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(tao::event::StartCause::Init) => {
                let tooltip = format!("{APP_NAME} v{CURRENT_VERSION}");
                tray_icon_handle = Some(
                    TrayIconBuilder::new()
                        .with_menu(Box::new(state.tray_menu.clone()))
                        .with_tooltip(&tooltip)
                        .with_icon(get_tray_icon(TrayIconState::Suspended))
                        .with_id(APP_UID)
                        .build()
                        .expect("Failed to build tray icon"),
                );

                // Async update check (non-blocking)
                if state.persistent_state.check_updates_on_launch {
                    let p = main_proxy.clone();
                    EXECUTOR
                        .spawn(async move {
                            let info = smol::unblock(|| update::check(false)).await;
                            let _ = p.send_event(UserEvent::UpdateCheckComplete(info));
                        })
                        .detach();
                }

                // Initialize
                initialize(&mut state, &main_proxy, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::ClipboardChanged) => {
                handle_clipboard_changed(&mut state, &main_proxy);
            }

            Event::UserEvent(UserEvent::ClipboardReady) => {
                handle_clipboard_ready(&mut state, &main_proxy, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::ClipboardFileDetected(path)) => {
                if state.initialized {
                    // Async delay to let the file be fully written
                    let p = main_proxy.clone();
                    EXECUTOR
                        .spawn(async move {
                            smol::Timer::after(Duration::from_millis(200)).await;
                            let _ = p.send_event(UserEvent::ClipboardFileReady(path));
                        })
                        .detach();
                }
            }

            Event::UserEvent(UserEvent::ClipboardFileReady(path)) => {
                handle_clipboard_file_ready(&mut state, &path, &main_proxy, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::Reload) => {
                uninitialize(&mut state, &tray_icon_handle, "Reloading...");
                initialize(&mut state, &main_proxy, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::Menu(menu_event)) => {
                let result = ui::handle_menu_event(
                    &menu_event.id,
                    &state.menu_actions,
                    &mut state.persistent_state,
                    &state.sync_folder,
                    &mut state.auto_launch_enabled,
                    &state.update_info,
                );

                if result.save_and_reload {
                    // Sync folder may have changed via ChangeFolder action
                    state.sync_folder = state.persistent_state.folder.as_ref().map(PathBuf::from);
                    save_state(&state.persistent_state);
                    let _ = main_proxy.send_event(UserEvent::Reload);
                }

                if result.rebuild_menu {
                    rebuild_menu(&mut state);
                }

                if result.restart_onedrive {
                    #[cfg(target_os = "windows")]
                    {
                        EXECUTOR
                            .spawn(async {
                                smol::unblock(crate::platform::restart_onedrive).await;
                            })
                            .detach();
                    }
                }

                match result.update_action {
                    UpdateAction::Check => {
                        let p = main_proxy.clone();
                        EXECUTOR
                            .spawn(async move {
                                let info = smol::unblock(|| update::check(true)).await;
                                let _ = p.send_event(UserEvent::UpdateCheckComplete(info));
                            })
                            .detach();
                    }
                    UpdateAction::Perform(info) => {
                        update::perform(&info);
                    }
                    UpdateAction::None => {}
                }

                if result.quit {
                    uninitialize(&mut state, &tray_icon_handle, "Exiting...");
                    std::process::exit(0);
                }
            }

            Event::UserEvent(UserEvent::TrayIcon(_tray_event)) => {
                // Tray icon events (click, double-click) can be handled here if needed
            }

            Event::UserEvent(UserEvent::RevertIcon) => {
                if state.initialized {
                    update_tray_icon(&mut state, &tray_icon_handle, TrayIconState::Working);
                }
            }

            Event::UserEvent(UserEvent::KeepAlive) => {
                if state.initialized
                    && state.persistent_state.is_receiving_anything()
                    && let Some(ref sf) = state.sync_folder
                {
                    write_keep_alive(sf, &state.hostname);
                }
            }

            Event::UserEvent(UserEvent::Cleanup) => {
                if state.initialized
                    && state.persistent_state.auto_cleanup
                    && let Some(ref sf) = state.sync_folder
                {
                    clean_files(sf, &state.hostname);
                }
            }

            Event::UserEvent(UserEvent::CheckFolderAccess) => {
                handle_folder_check(&mut state, &main_proxy, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::CheckIdleState) => {
                check_idle_state(&mut state, &main_proxy, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::CheckSyncCommand) => {
                handle_sync_command_check(&mut state, &tray_icon_handle);
            }

            Event::UserEvent(UserEvent::UpdateCheckComplete(info)) => {
                state.update_info = info;
                rebuild_menu(&mut state);
            }

            _ => {}
        }
    });
}

/// Spawn always-running periodic async tasks that send events to the main event loop.
fn spawn_periodic_tasks(proxy: &tao::event_loop::EventLoopProxy<UserEvent>) {
    // Folder access check (every 1s)
    let p = proxy.clone();
    EXECUTOR
        .spawn(async move {
            loop {
                smol::Timer::after(Duration::from_secs(1)).await;
                let _ = p.send_event(UserEvent::CheckFolderAccess);
            }
        })
        .detach();

    // Idle detection (every 1s)
    let p = proxy.clone();
    EXECUTOR
        .spawn(async move {
            loop {
                smol::Timer::after(Duration::from_secs(1)).await;
                let _ = p.send_event(UserEvent::CheckIdleState);
            }
        })
        .detach();

    // Sync command health check (every 1s)
    let p = proxy.clone();
    EXECUTOR
        .spawn(async move {
            loop {
                smol::Timer::after(Duration::from_secs(1)).await;
                let _ = p.send_event(UserEvent::CheckSyncCommand);
            }
        })
        .detach();
}

fn initialize(
    state: &mut AppState,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    // Start sync command if configured (may create the sync folder)
    if !state.persistent_state.sync_command.is_empty() {
        log::info!("Starting sync command...");
        if state
            .sync_command
            .start(&state.persistent_state.sync_command)
        {
            state.sync_command_started_at = Some(Instant::now());
        }
    }

    if state.sync_folder.is_none()
        && let Some(ref folder) = state.persistent_state.folder
    {
        state.sync_folder = Some(PathBuf::from(folder));
    }

    let sync_folder = match &state.sync_folder {
        Some(f) => f.clone(),
        None => {
            log::warn!("No sync folder configured.");
            set_tray_tooltip(tray_icon_handle, "Please set a sync folder");
            rebuild_menu(state);
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
    if state.persistent_state.is_sending_anything() {
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
    if state.persistent_state.is_receiving_anything() {
        let watch_mode: WatchMode = state.persistent_state.watch_mode.clone();
        log::info!("Starting file watcher...");
        log::info!("Watch mode: {:?}", watch_mode);

        start_fs_watcher(state, proxy, &sync_folder, &watch_mode);

        // Write the initial keep-alive file
        log::info!("Writing keep-alive file...");
        write_keep_alive(&sync_folder, &state.hostname);

        // Spawn periodic keep-alive async task
        let p = proxy.clone();
        state.init_tasks.push(EXECUTOR.spawn(async move {
            loop {
                smol::Timer::after(Duration::from_secs(KEEP_ALIVE_INTERVAL_SECS)).await;
                let _ = p.send_event(UserEvent::KeepAlive);
            }
        }));
    }

    // Initial auto-cleanup + periodic cleanup task
    if state.persistent_state.auto_cleanup {
        log::info!("Performing initial cleanup...");
        clean_files(&sync_folder, &state.hostname);

        let p = proxy.clone();
        state.init_tasks.push(EXECUTOR.spawn(async move {
            loop {
                smol::Timer::after(Duration::from_secs(60)).await;
                let _ = p.send_event(UserEvent::Cleanup);
            }
        }));
    }

    state.initialized = true;
    update_tray_icon(state, tray_icon_handle, TrayIconState::Working);
    set_tray_tooltip(tray_icon_handle, "");
    rebuild_menu(state);
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
        let config = notify::Config::default()
            .with_poll_interval(Duration::from_secs(FS_WATCHER_POLL_INTERVAL_SECS));
        match notify::PollWatcher::new(event_handler, config) {
            Ok(mut w) => {
                if let Err(e) = w.watch(sync_folder, RecursiveMode::Recursive) {
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
                if let Err(e) = w.watch(sync_folder, RecursiveMode::Recursive) {
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

    // Cancel all async init tasks (dropping cancels them)
    state.init_tasks.clear();
    state.icon_revert_task = None;
    state.clipboard_write_task = None;

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
        let keep_alive_path =
            sync_folder.join(format!("{}{}", state.hostname, IS_RECEIVING_FILE_SUFFIX));
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
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
) {
    if !state.initialized {
        return;
    }

    if state.sync_folder.is_none() {
        return;
    }

    // Clipboard debounce (leading-edge: ignore events too close together)
    let now = now_ms();
    if let Some(last) = state.last_clipboard_event
        && now - last < CLIPBOARD_DEBOUNCE_MS
    {
        return;
    }
    state.last_clipboard_event = Some(now);

    // Cancel any pending write task
    state.clipboard_write_task = None;

    // Schedule delayed write via async timer (replaces blocking sleep)
    let p = proxy.clone();
    state.clipboard_write_task = Some(EXECUTOR.spawn(async move {
        smol::Timer::after(Duration::from_millis(CLIPBOARD_WRITE_DELAY_MS)).await;
        let _ = p.send_event(UserEvent::ClipboardReady);
    }));
}

fn handle_clipboard_ready(
    state: &mut AppState,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    if !state.initialized {
        return;
    }

    let sync_folder = match &state.sync_folder {
        Some(f) => f.clone(),
        None => return,
    };

    let sent = write_clipboard_to_file(
        &sync_folder,
        &state.hostname,
        &state.persistent_state,
        &mut state.dedup,
    );

    if sent {
        set_icon_for_duration(state, tray_icon_handle, proxy, TrayIconState::Sent);
    }
}

fn handle_clipboard_file_ready(
    state: &mut AppState,
    path: &Path,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    if !state.initialized {
        return;
    }

    let sync_folder = match &state.sync_folder {
        Some(f) => f.clone(),
        None => return,
    };

    let parsed = parse_clipboard_filename(
        path,
        &sync_folder,
        &state.hostname,
        Some(ClipboardOrigin::Others),
    );

    if let Some(parsed) = parsed {
        let received = read_clipboard_from_file(&parsed, &state.persistent_state, &mut state.dedup);

        if received {
            set_icon_for_duration(state, tray_icon_handle, proxy, TrayIconState::Received);
        }
    }
}

fn handle_folder_check(
    state: &mut AppState,
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    tray_icon_handle: &Option<tray_icon::TrayIcon>,
) {
    let now = Instant::now();

    // Dynamic interval: check every 1s while waiting for sync command, then every 30s
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

    let should_check = state
        .last_folder_check
        .map(|t| now.duration_since(t) >= folder_check_interval)
        .unwrap_or(true);

    if !should_check {
        return;
    }

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

fn handle_sync_command_check(state: &mut AppState, tray_icon_handle: &Option<tray_icon::TrayIcon>) {
    if let Some(status) = state.sync_command.check() {
        let msg = format!("The sync command exited unexpectedly with status: {status}");
        let _ = send_notification("Sync command failed", &msg, NotificationDuration::Short);
        uninitialize(state, tray_icon_handle, "Sync command failed");
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
    proxy: &tao::event_loop::EventLoopProxy<UserEvent>,
    icon: TrayIconState,
) {
    update_tray_icon(state, tray_icon_handle, icon);

    // Cancel any existing revert task
    state.icon_revert_task = None;

    // Spawn one-shot async timer to revert icon
    let p = proxy.clone();
    state.icon_revert_task = Some(EXECUTOR.spawn(async move {
        smol::Timer::after(Duration::from_secs(ICON_FLASH_DURATION_SECS)).await;
        let _ = p.send_event(UserEvent::RevertIcon);
    }));
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

fn set_tray_tooltip(tray_icon_handle: &Option<tray_icon::TrayIcon>, status: &str) {
    let tooltip = if status.is_empty() {
        format!("{APP_NAME} v{CURRENT_VERSION}")
    } else {
        format!("{APP_NAME} - {status}")
    };
    if let Some(handle) = tray_icon_handle {
        let _ = handle.set_tooltip(Some(&tooltip));
    }
}

fn rebuild_menu(state: &mut AppState) {
    let app_path = get_executable_path_str();
    let auto_launch = AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&app_path)
        .build()
        .expect("Failed to build auto-launch");

    state.auto_launch_enabled = auto_launch.is_enabled().unwrap_or(false);

    let new_actions = {
        let auto_launch_enabled = state.auto_launch_enabled;
        rebuild_tray_menu(
            &state.tray_menu,
            &state.persistent_state,
            auto_launch_enabled,
            &state.update_info,
        )
    };
    state.menu_actions = new_actions;
}
