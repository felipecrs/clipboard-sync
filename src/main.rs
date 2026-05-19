#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod app;
mod clipboard;
mod config;
mod consts;
mod notification;
mod platform;
mod sync_command;
mod types;
mod ui;
mod update;
mod utils;

use crate::app::{AppState, spawn_periodic_tasks};
use crate::config::load_state;
use crate::consts::{APP_NAME, APP_UID, LOG_FILE_NAME};
use crate::platform::{NotificationDuration, init_platform, is_directory_writable, send_notification};
use crate::types::UserEvent;
use crate::ui::rebuild_tray_menu;
use crate::utils::{get_executable_directory, get_executable_path_str, get_hostname};

use anyhow::Context;
use auto_launch::AutoLaunchBuilder;
#[cfg(debug_assertions)]
use simplelog::{ColorChoice, TermLogger, TerminalMode};
use simplelog::{CombinedLogger, Config, LevelFilter, SharedLogger, WriteLogger};
use single_instance::SingleInstance;
use std::fs::File;
use tao::event::Event;
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::TrayIconEvent;
use tray_icon::menu::{Menu, MenuEvent};

/// Global async executor for background tasks.
static EXECUTOR: smol::Executor<'static> = smol::Executor::new();

fn main() -> std::process::ExitCode {
    if let Err(e) = run() {
        eprintln!("Fatal error: {e:#}");
        log::error!("Fatal error: {e:#}");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}

fn setup_logging(executable_directory: &std::path::Path) -> anyhow::Result<()> {
    let log_path = executable_directory.join(LOG_FILE_NAME);
    let loggers: Vec<Box<dyn SharedLogger>> = vec![
        WriteLogger::new(
            LevelFilter::Info,
            Config::default(),
            File::create(&log_path).context("failed to create log file")?,
        ),
        #[cfg(debug_assertions)]
        TermLogger::new(
            LevelFilter::Info,
            Config::default(),
            TerminalMode::Stderr,
            ColorChoice::Auto,
        ),
    ];
    CombinedLogger::init(loggers).context("failed to init logger")?;

    // windows_subsystem = "windows" suppresses stderr, so log panics before exit
    std::panic::set_hook(Box::new(|panic_info| {
        log::error!("Panic occurred: {panic_info}");
    }));

    Ok(())
}

fn ensure_writable_directory(executable_directory: &std::path::Path) -> anyhow::Result<()> {
    if !is_directory_writable(executable_directory) {
        let error_title = "Clipboard Sync Directory Not Writable";
        let error_message = format!(
            "Please move Clipboard Sync to a directory that is writable or fix the permissions of '{}'.",
            executable_directory.display(),
        );
        let _ = send_notification(error_title, &error_message, NotificationDuration::Long);
        anyhow::bail!("{error_title}: {error_message}");
    }
    Ok(())
}

fn wire_event_proxies(event_loop: &tao::event_loop::EventLoop<UserEvent>) {
    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |event| {
        if let Err(e) = proxy.send_event(UserEvent::TrayIcon(event)) {
            log::warn!("Failed to send TrayIcon event: {e:#}");
        }
    }));
    TrayIconEvent::receiver();

    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |event| {
        if let Err(e) = proxy.send_event(UserEvent::Menu(event)) {
            log::warn!("Failed to send Menu event: {e:#}");
        }
    }));
    MenuEvent::receiver();
}

fn create_auto_launch() -> anyhow::Result<auto_launch::AutoLaunch> {
    let app_path = get_executable_path_str()?;
    AutoLaunchBuilder::new()
        .set_app_name(APP_NAME)
        .set_app_path(&app_path)
        .build()
        .context("failed to build auto-launch")
}

fn run() -> anyhow::Result<()> {
    let executable_directory = get_executable_directory()?;
    setup_logging(&executable_directory)?;

    init_platform(&executable_directory)?;
    ensure_writable_directory(&executable_directory)?;

    // Only allow one instance
    let _instance = SingleInstance::new(APP_UID).context("failed to create single instance")?;
    if !_instance.is_single() {
        anyhow::bail!("Another instance is already running.");
    }

    let hostname = get_hostname();
    log::info!("Hostname: {hostname}");

    let persistent_state = load_state()
        .context("failed to load preferences — exiting to prevent overwriting your preferences")?;
    log::info!("Loaded: {persistent_state:?}");

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

    wire_event_proxies(&event_loop);

    let auto_launch = create_auto_launch()?;

    // Build initial menu (auto-launch status will be checked when initializing)
    let tray_menu = Menu::new();
    let menu_actions = rebuild_tray_menu(&tray_menu, &persistent_state, false, &None);

    let mut app = AppState::new(persistent_state, hostname, tray_menu, menu_actions, auto_launch);

    let main_proxy = event_loop.create_proxy();

    // Spawn always-running periodic async tasks
    spawn_periodic_tasks(&main_proxy);

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::NewEvents(tao::event::StartCause::Init) => {
                app.handle_init(&main_proxy);
            }

            Event::UserEvent(UserEvent::ClipboardChanged) => {
                app.handle_clipboard_changed(&main_proxy);
            }

            Event::UserEvent(UserEvent::ClipboardReady) => {
                app.handle_clipboard_ready(&main_proxy);
            }

            Event::UserEvent(UserEvent::ClipboardFileDetected(path)) => {
                app.handle_clipboard_file_detected(path, &main_proxy);
            }

            Event::UserEvent(UserEvent::ClipboardFileReady(path)) => {
                app.handle_clipboard_file_ready(&path, &main_proxy);
            }

            Event::UserEvent(UserEvent::Reload) => {
                app.handle_reload(&main_proxy);
            }

            Event::UserEvent(UserEvent::Menu(menu_event)) => {
                app.handle_menu_click(&menu_event, &main_proxy);
            }

            Event::UserEvent(UserEvent::TrayIcon(_tray_event)) => {
                // Tray icon events (click, double-click) can be handled here if needed
            }

            Event::UserEvent(UserEvent::RevertIcon) => {
                app.handle_revert_icon();
            }

            Event::UserEvent(UserEvent::KeepAlive) => {
                app.handle_keep_alive();
            }

            Event::UserEvent(UserEvent::Cleanup) => {
                app.handle_cleanup();
            }

            Event::UserEvent(UserEvent::CheckFolderAccess) => {
                app.handle_folder_check(&main_proxy);
            }

            Event::UserEvent(UserEvent::CheckIdleState) => {
                app.handle_idle_check(&main_proxy);
            }

            Event::UserEvent(UserEvent::CheckSyncCommand) => {
                app.handle_sync_command_check();
            }

            Event::UserEvent(UserEvent::UpdateCheckComplete(info)) => {
                app.handle_update_check_complete(info);
            }

            _ => {}
        }
    });
}
