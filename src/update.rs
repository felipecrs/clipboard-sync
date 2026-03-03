use crate::consts::{CURRENT_VERSION, GITHUB_REPO_URL};
use semver::Version;
use ureq::config::Config;
use ureq::tls::{RootCerts, TlsConfig, TlsProvider};
use ureq::{Agent, ResponseExt};

fn create_agent() -> Agent {
    let config = Config::builder()
        .tls_config(
            TlsConfig::builder()
                .provider(TlsProvider::NativeTls)
                .root_certs(RootCerts::PlatformVerifier)
                .build(),
        )
        .build();

    config.new_agent()
}

#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub latest_version: String,
    #[allow(dead_code)]
    pub release_url: String,
}

fn check_for_updates() -> Result<Option<UpdateInfo>, Box<dyn std::error::Error>> {
    log::info!("Checking for updates...");

    let agent = create_agent();
    let releases_url = format!("{GITHUB_REPO_URL}/releases/latest");
    let response = agent.head(&releases_url).call()?;
    let release_url = response.get_uri().to_string();

    let latest_tag = release_url
        .rsplit('/')
        .next()
        .ok_or("Could not extract version from redirect URL")?;

    let latest_version = latest_tag.trim_start_matches('v');

    log::info!("Current: {CURRENT_VERSION}, Latest: {latest_version}");

    // Development version should not attempt to update
    if CURRENT_VERSION == "0.0.0-development" {
        log::info!("Development version, skipping update check.");
        return Ok(None);
    }

    let current = Version::parse(CURRENT_VERSION)?;
    let latest = Version::parse(latest_version)?;

    if latest > current {
        Ok(Some(UpdateInfo {
            latest_version: latest_version.to_string(),
            release_url: release_url.to_string(),
        }))
    } else {
        Ok(None)
    }
}

/// Check for updates. If `silent` is true, don't log the "no update" case.
pub fn check(silent: bool) -> Option<UpdateInfo> {
    match check_for_updates() {
        Ok(Some(info)) => {
            log::info!("Update available: v{}", info.latest_version);
            Some(info)
        }
        Ok(None) => {
            if !silent {
                log::info!("No updates available.");
            }
            None
        }
        Err(e) => {
            log::error!("Failed to check for updates: {e}");
            None
        }
    }
}

/// Get the download URL for the current platform.
pub fn get_download_url(info: &UpdateInfo) -> String {
    let version = &info.latest_version;
    let base = format!("{GITHUB_REPO_URL}/releases/download/v{version}");

    #[cfg(target_os = "windows")]
    {
        format!("{base}/Clipboard.Sync-{version}.Setup.exe")
    }
    #[cfg(target_os = "macos")]
    {
        format!("{base}/Clipboard.Sync-{version}-x64.dmg")
    }
    #[cfg(target_os = "linux")]
    {
        format!("{GITHUB_REPO_URL}/releases")
    }
}
