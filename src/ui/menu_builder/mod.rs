use super::{MenuAction, MenuIdMap, MenuItemInfo};
use crate::config::{PersistentState, WatchMode};
use crate::update::UpdateInfo;
use tray_icon::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};

/// Rebuild the tray context menu in place, returning a mapping of MenuId -> MenuItemInfo.
pub fn rebuild_tray_menu(
    tray_menu: &Menu,
    state: &PersistentState,
    auto_launch_enabled: bool,
    update_info: &Option<UpdateInfo>,
) -> MenuIdMap {
    // Clear the existing menu
    for _ in 0..tray_menu.items().len() {
        tray_menu.remove_at(0);
    }

    let mut actions: MenuIdMap = MenuIdMap::new();

    // Helper to insert a menu item and record its action
    macro_rules! insert_action {
        ($item:expr, $name:expr, $action:expr) => {
            actions.insert(
                $item.id().clone(),
                MenuItemInfo {
                    name: $name.to_string(),
                    action: $action,
                },
            );
        };
    }

    // Clipboard section
    tray_menu
        .append(&MenuItem::new("Clipboard", false, None))
        .unwrap();

    let send_submenu = Submenu::new("Send", true);
    let send_texts = CheckMenuItem::new("Texts", true, state.send_texts, None);
    insert_action!(send_texts, "Send Texts", MenuAction::ToggleSendTexts);
    send_submenu.append(&send_texts).unwrap();
    let send_images = CheckMenuItem::new("Images", true, state.send_images, None);
    insert_action!(send_images, "Send Images", MenuAction::ToggleSendImages);
    send_submenu.append(&send_images).unwrap();
    let send_files = CheckMenuItem::new("Files", true, state.send_files, None);
    insert_action!(send_files, "Send Files", MenuAction::ToggleSendFiles);
    send_submenu.append(&send_files).unwrap();
    tray_menu.append(&send_submenu).unwrap();

    let receive_submenu = Submenu::new("Receive", true);
    let recv_texts = CheckMenuItem::new("Texts", true, state.receive_texts, None);
    insert_action!(
        recv_texts,
        "Receive Texts",
        MenuAction::ToggleReceiveTexts
    );
    receive_submenu.append(&recv_texts).unwrap();
    let recv_images = CheckMenuItem::new("Images", true, state.receive_images, None);
    insert_action!(
        recv_images,
        "Receive Images",
        MenuAction::ToggleReceiveImages
    );
    receive_submenu.append(&recv_images).unwrap();
    let recv_files = CheckMenuItem::new("Files", true, state.receive_files, None);
    insert_action!(
        recv_files,
        "Receive Files",
        MenuAction::ToggleReceiveFiles
    );
    receive_submenu.append(&recv_files).unwrap();
    tray_menu.append(&receive_submenu).unwrap();

    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Sync section
    tray_menu
        .append(&MenuItem::new("Sync", false, None))
        .unwrap();

    let change_folder = MenuItem::new("Change sync folder...", true, None);
    insert_action!(change_folder, "Change Folder", MenuAction::ChangeFolder);
    tray_menu.append(&change_folder).unwrap();

    let sync_cmd_label = if state.sync_command.is_empty() {
        "Set sync command..."
    } else {
        "Change sync command..."
    };
    let sync_cmd_item = MenuItem::new(sync_cmd_label, true, None);
    insert_action!(sync_cmd_item, "Sync Command", MenuAction::SetSyncCommand);
    tray_menu.append(&sync_cmd_item).unwrap();

    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Preferences section
    tray_menu
        .append(&MenuItem::new("Preferences", false, None))
        .unwrap();

    let watch_submenu = Submenu::new("Watch mode", true);
    let wm_native =
        CheckMenuItem::new("Native", true, state.watch_mode == WatchMode::Native, None);
    insert_action!(wm_native, "Watch Mode Native", MenuAction::SetWatchModeNative);
    watch_submenu.append(&wm_native).unwrap();
    let wm_polling = CheckMenuItem::new(
        "Polling",
        true,
        state.watch_mode == WatchMode::Polling,
        None,
    );
    insert_action!(
        wm_polling,
        "Watch Mode Polling",
        MenuAction::SetWatchModePolling
    );
    watch_submenu.append(&wm_polling).unwrap();
    tray_menu.append(&watch_submenu).unwrap();

    let auto_clean = CheckMenuItem::new("Auto-clean", true, state.auto_cleanup, None);
    insert_action!(auto_clean, "Auto-clean", MenuAction::ToggleAutoCleanup);
    tray_menu.append(&auto_clean).unwrap();

    let auto_start = CheckMenuItem::new("Auto-launch on startup", true, auto_launch_enabled, None);
    insert_action!(auto_start, "Auto-launch", MenuAction::ToggleAutoStart);
    tray_menu.append(&auto_start).unwrap();

    let check_updates_on_launch = CheckMenuItem::new(
        "Check for updates on launch",
        true,
        state.check_updates_on_launch,
        None,
    );
    insert_action!(
        check_updates_on_launch,
        "Check Updates on Launch",
        MenuAction::ToggleCheckUpdatesOnLaunch
    );
    tray_menu.append(&check_updates_on_launch).unwrap();

    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    // Troubleshooting section
    tray_menu
        .append(&MenuItem::new("Troubleshooting", false, None))
        .unwrap();

    let reinitialize = MenuItem::new("Reinitialize", true, None);
    insert_action!(reinitialize, "Reinitialize", MenuAction::Reinitialize);
    tray_menu.append(&reinitialize).unwrap();

    let open_sync_folder = MenuItem::new("Open sync folder...", state.folder.is_some(), None);
    insert_action!(
        open_sync_folder,
        "Open Sync Folder",
        MenuAction::OpenSyncFolder
    );
    tray_menu.append(&open_sync_folder).unwrap();

    let open_app_folder = MenuItem::new("Open app folder...", true, None);
    insert_action!(open_app_folder, "Open App Folder", MenuAction::OpenAppFolder);
    tray_menu.append(&open_app_folder).unwrap();

    #[cfg(target_os = "windows")]
    {
        let restart_od = MenuItem::new("Restart OneDrive...", true, None);
        insert_action!(
            restart_od,
            "Restart OneDrive",
            MenuAction::RestartOneDrive
        );
        tray_menu.append(&restart_od).unwrap();
    }

    // Update section
    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    let github_item = MenuItem::new("GitHub...", true, None);
    insert_action!(github_item, "GitHub", MenuAction::OpenGitHub);
    tray_menu.append(&github_item).unwrap();

    let (update_label, update_action) = match update_info {
        Some(info) => (
            format!("Update to {}...", info.latest_version),
            MenuAction::PerformUpdate,
        ),
        None => ("Check for updates".to_string(), MenuAction::CheckForUpdates),
    };
    let update_item = MenuItem::new(&update_label, true, None);
    insert_action!(update_item, &update_label, update_action);
    tray_menu.append(&update_item).unwrap();

    // Quit section
    tray_menu.append(&PredefinedMenuItem::separator()).unwrap();

    let quit_item = MenuItem::new("Quit", true, None);
    insert_action!(quit_item, "Quit", MenuAction::Quit);
    tray_menu.append(&quit_item).unwrap();

    actions
}