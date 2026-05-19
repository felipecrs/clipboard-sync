use super::MenuAction;
use crate::config::{PersistentState, WatchMode};
use crate::consts::GITHUB_REPO_URL;
use crate::update::UpdateInfo;
use crate::utils::{get_executable_directory, open_path, open_url};
use std::path::PathBuf;

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

/// Pick a folder using a cross-platform dialog.
fn pick_folder() -> Option<String> {
    tinyfiledialogs::select_folder_dialog("Select folder to save and read clipboard files", "")
}

/// Handle a menu event and return the result for the main event loop to act on.
pub fn handle_menu_event(
    menu_id: &tray_icon::menu::MenuId,
    menu_actions: &super::MenuIdMap,
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

    let info = match menu_actions.get(menu_id) {
        Some(a) => a,
        None => return result,
    };

    log::info!("Menu action: {}", info.name);

    match &info.action {
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
                state.folder = Some(std::path::PathBuf::from(folder));
                result.save_and_reload = true;
            }
        }
        MenuAction::OpenSyncFolder => {
            if let Some(folder) = sync_folder {
                if let Err(e) = open_path(folder) {
                    log::error!("Failed to open sync folder: {e:#}");
                }
            }
        }
        MenuAction::OpenAppFolder => {
            match get_executable_directory() {
                Ok(dir) => {
                    if let Err(e) = open_path(&dir) {
                        log::error!("Failed to open app directory: {e:#}");
                    }
                }
                Err(e) => log::error!("Failed to get executable directory: {e:#}"),
            }
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
            if let Err(e) = open_url(GITHUB_REPO_URL) {
                log::error!("Failed to open GitHub repo: {e:#}");
            }
        }
        MenuAction::Quit => {
            result.quit = true;
        }
    }

    result
}