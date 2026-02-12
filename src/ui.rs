use crate::config::{Config, WatchMode};
use crate::update::UpdateInfo;
use std::collections::HashMap;
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
    SetWatchModePollingHarder,
    ToggleAutoCleanup,
    ToggleAutoStart,
    ChangeFolder,
    SetSyncCommand,
    OpenFolder,
    RestartOneDrive,
    CheckForUpdates,
    OpenGitHub,
    Quit,
}

/// Build the tray context menu, returning the menu and a mapping of MenuId -> MenuAction.
pub fn build_tray_menu(
    config: &Config,
    auto_launch_enabled: bool,
    update_info: &Option<UpdateInfo>,
    sync_folder: &Option<String>,
) -> (Menu, HashMap<MenuId, MenuAction>) {
    let menu = Menu::new();
    let mut actions: HashMap<MenuId, MenuAction> = HashMap::new();

    // Send submenu
    let send_submenu = Submenu::new("Send", true);
    let send_texts = CheckMenuItem::new("Texts", true, config.send_texts, None);
    actions.insert(send_texts.id().clone(), MenuAction::ToggleSendTexts);
    send_submenu.append(&send_texts).unwrap();

    let send_images = CheckMenuItem::new("Images", true, config.send_images, None);
    actions.insert(send_images.id().clone(), MenuAction::ToggleSendImages);
    send_submenu.append(&send_images).unwrap();

    let send_files = CheckMenuItem::new("Files", true, config.send_files, None);
    actions.insert(send_files.id().clone(), MenuAction::ToggleSendFiles);
    send_submenu.append(&send_files).unwrap();
    menu.append(&send_submenu).unwrap();

    // Receive submenu
    let receive_submenu = Submenu::new("Receive", true);
    let recv_texts = CheckMenuItem::new("Texts", true, config.receive_texts, None);
    actions.insert(recv_texts.id().clone(), MenuAction::ToggleReceiveTexts);
    receive_submenu.append(&recv_texts).unwrap();

    let recv_images = CheckMenuItem::new("Images", true, config.receive_images, None);
    actions.insert(recv_images.id().clone(), MenuAction::ToggleReceiveImages);
    receive_submenu.append(&recv_images).unwrap();

    let recv_files = CheckMenuItem::new("Files", true, config.receive_files, None);
    actions.insert(recv_files.id().clone(), MenuAction::ToggleReceiveFiles);
    receive_submenu.append(&recv_files).unwrap();
    menu.append(&receive_submenu).unwrap();

    menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Watch mode submenu
    let watch_submenu = Submenu::new("Watch mode", true);
    let wm_native = CheckMenuItem::new(
        "Native",
        true,
        config.watch_mode == WatchMode::Native,
        None,
    );
    actions.insert(wm_native.id().clone(), MenuAction::SetWatchModeNative);
    watch_submenu.append(&wm_native).unwrap();

    let wm_polling = CheckMenuItem::new(
        "Polling",
        true,
        config.watch_mode == WatchMode::Polling,
        None,
    );
    actions.insert(wm_polling.id().clone(), MenuAction::SetWatchModePolling);
    watch_submenu.append(&wm_polling).unwrap();

    let wm_polling_harder = CheckMenuItem::new(
        "Polling harder",
        true,
        config.watch_mode == WatchMode::PollingHarder,
        None,
    );
    actions.insert(wm_polling_harder.id().clone(), MenuAction::SetWatchModePollingHarder);
    watch_submenu.append(&wm_polling_harder).unwrap();
    menu.append(&watch_submenu).unwrap();

    // Auto-clean
    let auto_clean = CheckMenuItem::new("Auto-clean", true, config.auto_cleanup, None);
    actions.insert(auto_clean.id().clone(), MenuAction::ToggleAutoCleanup);
    menu.append(&auto_clean).unwrap();

    // Sync command
    let sync_cmd_item = CheckMenuItem::new(
        "Sync command",
        true,
        !config.sync_command.is_empty(),
        None,
    );
    actions.insert(sync_cmd_item.id().clone(), MenuAction::SetSyncCommand);
    menu.append(&sync_cmd_item).unwrap();

    // Auto-start on login
    #[cfg(not(target_os = "linux"))]
    {
        let auto_start = CheckMenuItem::new("Auto-start on login", true, auto_launch_enabled, None);
        actions.insert(auto_start.id().clone(), MenuAction::ToggleAutoStart);
        menu.append(&auto_start).unwrap();
    }

    menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Change folder
    let change_folder = MenuItem::new("Change folder", true, None);
    actions.insert(change_folder.id().clone(), MenuAction::ChangeFolder);
    menu.append(&change_folder).unwrap();

    // Open folder
    let open_folder = MenuItem::new("Open folder", sync_folder.is_some(), None);
    actions.insert(open_folder.id().clone(), MenuAction::OpenFolder);
    menu.append(&open_folder).unwrap();

    // Restart OneDrive (Windows only)
    #[cfg(target_os = "windows")]
    {
        menu.append(&PredefinedMenuItem::separator()).unwrap();
        let restart_od = MenuItem::new("Restart OneDrive", true, None);
        actions.insert(restart_od.id().clone(), MenuAction::RestartOneDrive);
        menu.append(&restart_od).unwrap();
    }

    menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Check for updates
    #[cfg(not(target_os = "linux"))]
    {
        let update_label = if update_info.is_some() {
            "Download update"
        } else {
            "Check for updates"
        };
        let update_item = MenuItem::new(update_label, true, None);
        actions.insert(update_item.id().clone(), MenuAction::CheckForUpdates);
        menu.append(&update_item).unwrap();
    }

    // GitHub
    let github_item = MenuItem::new("GitHub", true, None);
    actions.insert(github_item.id().clone(), MenuAction::OpenGitHub);
    menu.append(&github_item).unwrap();

    menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Quit
    let quit_item = MenuItem::new("Exit", true, None);
    actions.insert(quit_item.id().clone(), MenuAction::Quit);
    menu.append(&quit_item).unwrap();

    (menu, actions)
}
