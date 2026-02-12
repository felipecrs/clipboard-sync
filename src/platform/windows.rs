use crate::consts::{APP_AUMID, APP_NAME, PNG_ICON_BYTES, PNG_ICON_FILE_NAME};
use std::path::Path;
use windows::Win32::System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx};
use windows::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;
use windows::core::{HSTRING, Result};
use windows_registry::CURRENT_USER;

pub fn init_platform(executable_directory: &Path) {
    unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED).unwrap() };
    let _ = setup_app_aumid(executable_directory);
}

fn setup_app_aumid(executable_directory: &Path) -> Result<()> {
    let registry_path = format!(r"SOFTWARE\Classes\AppUserModelId\{APP_AUMID}");
    let _ = CURRENT_USER.remove_tree(registry_path.clone());
    let key = CURRENT_USER.create(registry_path.clone()).unwrap();
    let _ = key.set_string("DisplayName", APP_NAME);

    let png_path = executable_directory.join(PNG_ICON_FILE_NAME);
    if let Err(e) = std::fs::write(&png_path, PNG_ICON_BYTES) {
        log::warn!("Failed to write {PNG_ICON_FILE_NAME} icon: {e}");
        let _ = key.remove_value("IconUri");
    } else {
        let _ = key.set_hstring("IconUri", &png_path.as_path().into());
    }

    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(&HSTRING::from(APP_AUMID));
    }

    Ok(())
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
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", script])
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
                log::error!("OneDrive restart failed with exit code: {:?}", output.status.code());
            }
        }
        Err(e) => {
            log::error!("Failed to run OneDrive restart script: {e}");
        }
    }
}
