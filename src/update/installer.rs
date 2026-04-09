//! Binary installation for lazytail self-update.
//!
//! Downloads and replaces the current binary using GitHub releases.

use super::{get_target, REPO_NAME, REPO_OWNER};

/// Download and install the latest stable release, replacing the current binary.
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

/// Download and install the nightly pre-release build, replacing the current binary.
pub fn install_nightly() -> Result<String, String> {
    let target = get_target();

    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .map_err(|e| format!("Failed to configure release list: {}", e))?
        .fetch()
        .map_err(|e| format!("Failed to fetch releases: {}", e))?;

    let nightly = releases
        .iter()
        .find(|r| r.name == "nightly" && r.has_target_asset(&target))
        .ok_or_else(|| {
            format!(
                "No nightly release found with assets for target '{}'",
                target
            )
        })?;

    let asset = nightly
        .asset_for(&target, None)
        .ok_or_else(|| format!("No nightly asset found for target '{}'", target))?;

    let tmp_dir =
        self_update::TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;

    let tmp_tarball_path = tmp_dir.path().join(&asset.name);
    let tmp_tarball = std::fs::File::create(&tmp_tarball_path)
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    self_update::Download::from_url(&asset.download_url)
        .show_progress(true)
        .download_to(&tmp_tarball)
        .map_err(|e| format!("Failed to download nightly: {}", e))?;

    let bin_name = "lazytail";
    self_update::Extract::from_source(&tmp_tarball_path)
        .archive(self_update::ArchiveKind::Tar(Some(
            self_update::Compression::Gz,
        )))
        .extract_file(tmp_dir.path(), bin_name)
        .map_err(|e| format!("Failed to extract nightly: {}", e))?;

    let new_exe = tmp_dir.path().join(bin_name);

    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to determine current executable path: {}", e))?;

    self_update::Move::from_source(&new_exe)
        .replace_using_temp(&current_exe)
        .to_dest(&current_exe)
        .map_err(|e| format!("Failed to replace binary: {}", e))?;

    Ok(nightly.version.clone())
}
