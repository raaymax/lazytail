//! Binary installation for lazytail self-update.
//!
//! Downloads and replaces the current binary using GitHub releases.

const REPO_OWNER: &str = "raaymax";
const REPO_NAME: &str = "lazytail";

/// Download and install the latest release, replacing the current binary.
pub fn install_latest() -> Result<String, String> {
    let target = get_target();

    let status = self_update::backends::github::Update::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .bin_name("lazytail")
        .target(&target)
        .show_download_progress(true)
        .no_confirm(true)
        .current_version(self_update::cargo_crate_version!())
        .build()
        .map_err(|e| format!("Failed to configure updater: {}", e))?
        .update()
        .map_err(|e| format!("Failed to install update: {}", e))?;

    Ok(status.version().to_string())
}

/// Map the current platform to the asset naming convention used in releases.
fn get_target() -> String {
    let os = if cfg!(target_os = "linux") {
        "linux"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "unknown"
    };

    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "unknown"
    };

    format!("{}-{}", os, arch)
}
