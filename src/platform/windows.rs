use crate::consts::{APP_AUMID, APP_ICON_PNG_BYTES, APP_ICON_PNG_FILE_NAME, APP_NAME};
use std::path::Path;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::core::{HSTRING, Result};
use windows_registry::CURRENT_USER;

pub fn init_platform(executable_directory: &Path) -> anyhow::Result<()> {
    // SAFETY: CoInitializeEx is safe to call; first call on this thread.
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()? };
    if let Err(e) = setup_app_aumid(executable_directory) {
        log::warn!("Failed to set up app AUMID: {e:#}");
    }
    Ok(())
}

fn setup_app_aumid(executable_directory: &Path) -> Result<()> {
    let registry_path = format!(r"SOFTWARE\Classes\AppUserModelId\{APP_AUMID}");
    let _ = CURRENT_USER.remove_tree(registry_path.clone());
    let key = CURRENT_USER.create(&registry_path)?;
    if let Err(e) = key.set_string("DisplayName", APP_NAME) {
        log::warn!("Failed to set AUMID DisplayName: {e:#}");
    }

    // We need an icon file for the AUMID to work properly
    let png_path = executable_directory.join(APP_ICON_PNG_FILE_NAME);
    if let Err(e) = std::fs::write(&png_path, APP_ICON_PNG_BYTES) {
        log::warn!("Failed to write {APP_ICON_PNG_FILE_NAME} icon: {e:#}");
        let _ = key.remove_value("IconUri");
    } else if let Err(e) = key.set_hstring("IconUri", &png_path.as_path().into()) {
        log::warn!("Failed to set AUMID IconUri: {e:#}");
    }

    // SAFETY: APP_AUMID is a valid static string; setting the AUMID is a standard shell API call.
    unsafe {
        if let Err(e) = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_AUMID)) {
            log::warn!("Failed to set explicit AppUserModelID: {e:#}");
        }
    }

    Ok(())
}

/// Checks if a directory is writable by attempting to create and delete a temp file.
pub fn is_directory_writable(dir: &Path) -> bool {
    let test_path = dir.join(".clipboard_sync_write_test");
    match std::fs::write(&test_path, b"") {
        Ok(()) => {
            let _ = std::fs::remove_file(&test_path);
            true
        }
        Err(_) => false,
    }
}

/// Restart OneDrive (Windows specific).
pub fn restart_onedrive() {
    log::info!("Restarting OneDrive...");

    let script = r#"
        $oneDriveProcesses = Get-Process -Name OneDrive -ErrorAction SilentlyContinue
        if ($oneDriveProcesses) {
            $oneDrivePath = $oneDriveProcesses[0].Path
            Stop-Process -Name OneDrive -Force
            Start-Sleep -Seconds 2
            Start-Process -FilePath $oneDrivePath
            Write-Output "OneDrive restarted successfully."
        } else {
            Write-Output "OneDrive is not running."
        }
    "#;

    let result = std::process::Command::new("PowerShell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .output();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            if !stdout.is_empty() {
                log::info!("[restart-onedrive] {}", stdout.trim());
            }
            if !stderr.is_empty() {
                log::warn!("[restart-onedrive] {}", stderr.trim());
            }
            if !output.status.success() {
                log::error!(
                    "OneDrive restart failed with exit code: {:?}",
                    output.status.code()
                );
            }
        }
        Err(e) => {
            log::error!("Failed to run OneDrive restart script: {e:#}");
        }
    }
}
