use crate::config::{PersistentState, WatchMode};
use crate::consts::GITHUB_REPO_URL;
use crate::update::UpdateInfo;
use crate::utils::{get_executable_directory, open_path, open_url};
use std::collections::HashMap;
use std::path::PathBuf;
use tray_icon::menu::{CheckMenuItem, Menu, MenuId, MenuItem, PredefinedMenuItem, Submenu};

/// Identifies what a menu item does when clicked.
#[derive(Debug)]
pub enum MenuAction {
    ToggleSendTexts,
    ToggleSendImages,
    ToggleSendFiles,
    ToggleReceiveTexts,
    ToggleReceiveImages,
    ToggleReceiveFiles,
    SetWatchModeNative,
    SetWatchModePolling,
    ToggleAutoCleanup,
    ToggleCheckUpdatesOnLaunch,
    ToggleAutoStart,
    ChangeFolder,
    SetSyncCommand,
    OpenSyncFolder,
    OpenAppFolder,
    #[cfg(target_os = "windows")]
    RestartOneDrive,
    Reinitialize,
    CheckForUpdates,
    PerformUpdate,
    OpenGitHub,
    Quit,
}

pub enum UpdateAction {
    None,
    Check,
    Perform(UpdateInfo),
}

pub struct MenuEventResult {
    pub save_and_reload: bool,
    pub rebuild_menu: bool,
    pub quit: bool,
    pub update_action: UpdateAction,
    #[cfg(target_os = "windows")]
    pub restart_onedrive: bool,
}

/// Handle a menu event and return the result for the main event loop to act on.
pub fn handle_menu_event(
    menu_id: &MenuId,
    menu_actions: &HashMap<MenuId, MenuAction>,
    state: &mut PersistentState,
    sync_folder: &Option<PathBuf>,
    auto_launch: &auto_launch::AutoLaunch,
    update_info: &Option<UpdateInfo>,
) -> MenuEventResult {
    let mut result = MenuEventResult {
        save_and_reload: false,
        rebuild_menu: false,
        quit: false,
        update_action: UpdateAction::None,
        #[cfg(target_os = "windows")]
        restart_onedrive: false,
    };

    let action = match menu_actions.get(menu_id) {
        Some(a) => a,
        None => return result,
    };

    match action {
        MenuAction::ToggleSendTexts => {
            state.send_texts = !state.send_texts;
            result.save_and_reload = true;
        }
        MenuAction::ToggleSendImages => {
            state.send_images = !state.send_images;
            result.save_and_reload = true;
        }
        MenuAction::ToggleSendFiles => {
            state.send_files = !state.send_files;
            result.save_and_reload = true;
        }
        MenuAction::ToggleReceiveTexts => {
            state.receive_texts = !state.receive_texts;
            result.save_and_reload = true;
        }
        MenuAction::ToggleReceiveImages => {
            state.receive_images = !state.receive_images;
            result.save_and_reload = true;
        }
        MenuAction::ToggleReceiveFiles => {
            state.receive_files = !state.receive_files;
            result.save_and_reload = true;
        }
        MenuAction::SetWatchModeNative => {
            state.watch_mode = WatchMode::Native;
            result.save_and_reload = true;
        }
        MenuAction::SetWatchModePolling => {
            state.watch_mode = WatchMode::Polling;
            result.save_and_reload = true;
        }
        MenuAction::ToggleAutoCleanup => {
            state.auto_cleanup = !state.auto_cleanup;
            result.save_and_reload = true;
        }
        MenuAction::ToggleCheckUpdatesOnLaunch => {
            state.check_updates_on_launch = !state.check_updates_on_launch;
            result.save_and_reload = true;
        }
        MenuAction::ToggleAutoStart => {
            let new_state = !auto_launch.is_enabled().unwrap_or(false);
            if new_state {
                let _ = auto_launch.enable();
            } else {
                let _ = auto_launch.disable();
            }
            result.rebuild_menu = true;
        }
        MenuAction::SetSyncCommand => {
            let current = &state.sync_command;
            let default = if current.is_empty() {
                ""
            } else {
                current.as_str()
            };
            if let Some(cmd) = tinyfiledialogs::input_box(
                "Sync command",
                "Enter a command to run before syncing (leave empty to disable):",
                default,
            ) {
                state.sync_command = cmd;
                result.save_and_reload = true;
            }
        }
        MenuAction::ChangeFolder => {
            if let Some(folder) = pick_folder() {
                state.folder = Some(folder);
                result.save_and_reload = true;
            }
        }
        MenuAction::OpenSyncFolder => {
            if let Some(folder) = sync_folder {
                open_path(folder);
            }
        }
        MenuAction::OpenAppFolder => {
            open_path(&get_executable_directory());
        }
        MenuAction::Reinitialize => {
            result.save_and_reload = true;
        }
        #[cfg(target_os = "windows")]
        MenuAction::RestartOneDrive => {
            result.restart_onedrive = true;
        }
        MenuAction::CheckForUpdates => {
            result.update_action = UpdateAction::Check;
        }
        MenuAction::PerformUpdate => {
            if let Some(info) = update_info {
                result.update_action = UpdateAction::Perform(info.clone());
            }
        }
        MenuAction::OpenGitHub => {
            open_url(GITHUB_REPO_URL);
        }
        MenuAction::Quit => {
            result.quit = true;
        }
    }

    result
}

/// Pick a folder using a cross-platform dialog.
fn pick_folder() -> Option<String> {
    tinyfiledialogs::select_folder_dialog("Select folder to save and read clipboard files", "")
}

/// Rebuild the tray context menu in place, returning a mapping of MenuId -> MenuAction.
pub fn rebuild_tray_menu(
    tray_menu: &Menu,
    state: &PersistentState,
    auto_launch_enabled: bool,
    update_info: &Option<UpdateInfo>,
) -> HashMap<MenuId, MenuAction> {
    // Clear the existing menu
    for _ in 0..tray_menu.items().len() {
        tray_menu.remove_at(0);
    }

    let mut actions: HashMap<MenuId, MenuAction> = HashMap::new();

    // Clipboard section
    tray_menu
        .append(&MenuItem::new("Clipboard", false, None))
        .unwrap();

    let send_submenu = Submenu::new("Send", true);
    let send_texts = CheckMenuItem::new("Texts", true, state.send_texts, None);
    actions.insert(send_texts.id().clone(), MenuAction::ToggleSendTexts);
    send_submenu.append(&send_texts).unwrap();
    let send_images = CheckMenuItem::new("Images", true, state.send_images, None);
    actions.insert(send_images.id().clone(), MenuAction::ToggleSendImages);
    send_submenu.append(&send_images).unwrap();
    let send_files = CheckMenuItem::new("Files", true, state.send_files, None);
    actions.insert(send_files.id().clone(), MenuAction::ToggleSendFiles);
    send_submenu.append(&send_files).unwrap();
    tray_menu.append(&send_submenu).unwrap();

    let receive_submenu = Submenu::new("Receive", true);
    let recv_texts = CheckMenuItem::new("Texts", true, state.receive_texts, None);
    actions.insert(recv_texts.id().clone(), MenuAction::ToggleReceiveTexts);
    receive_submenu.append(&recv_texts).unwrap();
    let recv_images = CheckMenuItem::new("Images", true, state.receive_images, None);
    actions.insert(recv_images.id().clone(), MenuAction::ToggleReceiveImages);
    receive_submenu.append(&recv_images).unwrap();
    let recv_files = CheckMenuItem::new("Files", true, state.receive_files, None);
    actions.insert(recv_files.id().clone(), MenuAction::ToggleReceiveFiles);
    receive_submenu.append(&recv_files).unwrap();
    tray_menu.append(&receive_submenu).unwrap();

    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Sync section
    tray_menu
        .append(&MenuItem::new("Sync", false, None))
        .unwrap();

    let change_folder = MenuItem::new("Change sync folder...", true, None);
    actions.insert(change_folder.id().clone(), MenuAction::ChangeFolder);
    tray_menu.append(&change_folder).unwrap();

    let sync_cmd_label = if state.sync_command.is_empty() {
        "Set sync command..."
    } else {
        "Change sync command..."
    };
    let sync_cmd_item = MenuItem::new(sync_cmd_label, true, None);
    actions.insert(sync_cmd_item.id().clone(), MenuAction::SetSyncCommand);
    tray_menu.append(&sync_cmd_item).unwrap();

    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Preferences section
    tray_menu
        .append(&MenuItem::new("Preferences", false, None))
        .unwrap();

    let watch_submenu = Submenu::new("Watch mode", true);
    let wm_native = CheckMenuItem::new("Native", true, state.watch_mode == WatchMode::Native, None);
    actions.insert(wm_native.id().clone(), MenuAction::SetWatchModeNative);
    watch_submenu.append(&wm_native).unwrap();
    let wm_polling = CheckMenuItem::new(
        "Polling",
        true,
        state.watch_mode == WatchMode::Polling,
        None,
    );
    actions.insert(wm_polling.id().clone(), MenuAction::SetWatchModePolling);
    watch_submenu.append(&wm_polling).unwrap();
    tray_menu.append(&watch_submenu).unwrap();

    let auto_clean = CheckMenuItem::new("Auto-clean", true, state.auto_cleanup, None);
    actions.insert(auto_clean.id().clone(), MenuAction::ToggleAutoCleanup);
    tray_menu.append(&auto_clean).unwrap();

    let auto_start = CheckMenuItem::new("Auto-launch on startup", true, auto_launch_enabled, None);
    actions.insert(auto_start.id().clone(), MenuAction::ToggleAutoStart);
    tray_menu.append(&auto_start).unwrap();

    let check_updates_on_launch = CheckMenuItem::new(
        "Check for updates on launch",
        true,
        state.check_updates_on_launch,
        None,
    );
    actions.insert(
        check_updates_on_launch.id().clone(),
        MenuAction::ToggleCheckUpdatesOnLaunch,
    );
    tray_menu.append(&check_updates_on_launch).unwrap();

    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Troubleshooting section
    tray_menu
        .append(&MenuItem::new("Troubleshooting", false, None))
        .unwrap();

    let reinitialize = MenuItem::new("Reinitialize", true, None);
    actions.insert(reinitialize.id().clone(), MenuAction::Reinitialize);
    tray_menu.append(&reinitialize).unwrap();

    let open_sync_folder = MenuItem::new("Open sync folder...", state.folder.is_some(), None);
    actions.insert(open_sync_folder.id().clone(), MenuAction::OpenSyncFolder);
    tray_menu.append(&open_sync_folder).unwrap();

    let open_app_folder = MenuItem::new("Open app folder...", true, None);
    actions.insert(open_app_folder.id().clone(), MenuAction::OpenAppFolder);
    tray_menu.append(&open_app_folder).unwrap();

    #[cfg(target_os = "windows")]
    {
        let restart_od = MenuItem::new("Restart OneDrive...", true, None);
        actions.insert(restart_od.id().clone(), MenuAction::RestartOneDrive);
        tray_menu.append(&restart_od).unwrap();
    }

    // Update section
    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    let github_item = MenuItem::new("GitHub...", true, None);
    actions.insert(github_item.id().clone(), MenuAction::OpenGitHub);
    tray_menu.append(&github_item).unwrap();

    let (update_label, update_action) = match update_info {
        Some(info) => (
            format!("Update to {}...", info.latest_version),
            MenuAction::PerformUpdate,
        ),
        None => ("Check for updates".to_string(), MenuAction::CheckForUpdates),
    };
    let update_item = MenuItem::new(&update_label, true, None);
    actions.insert(update_item.id().clone(), update_action);
    tray_menu.append(&update_item).unwrap();

    // Quit section
    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    let quit_item = MenuItem::new("Quit", true, None);
    actions.insert(quit_item.id().clone(), MenuAction::Quit);
    tray_menu.append(&quit_item).unwrap();

    actions
}
