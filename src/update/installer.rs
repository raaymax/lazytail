//! Binary installation for lazytail self-update.
//!
//! Downloads and replaces the current binary using GitHub releases.

use super::{expected_asset_name, get_target, REPO_NAME, REPO_OWNER};

/// Download and install the latest stable release, replacing the current binary.
pub fn install_latest() -> Result<String, String> {
    let target = get_target();
    let asset_name = expected_asset_name(&target);

    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .map_err(|e| format!("Failed to configure release list: {}", e))?
        .fetch()
        .map_err(|e| format!("Failed to fetch releases: {}", e))?;

    let latest = releases
        .iter()
        .find(|r| r.version != "nightly" && r.assets.iter().any(|a| a.name == asset_name))
        .ok_or_else(|| format!("No releases found with asset '{}'", asset_name))?;

    download_and_replace(latest, &asset_name)?;
    Ok(latest.version.clone())
}

/// Download and install the nightly pre-release build, replacing the current binary.
pub fn install_nightly() -> Result<String, String> {
    let target = get_target();
    let asset_name = expected_asset_name(&target);

    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .map_err(|e| format!("Failed to configure release list: {}", e))?
        .fetch()
        .map_err(|e| format!("Failed to fetch releases: {}", e))?;

    let nightly = releases
        .iter()
        .find(|r| r.version == "nightly" && r.assets.iter().any(|a| a.name == asset_name))
        .ok_or_else(|| format!("No nightly release found with asset '{}'", asset_name))?;

    download_and_replace(nightly, &asset_name)?;
    Ok(nightly.version.clone())
}

/// Download the named asset from a release and replace the running binary.
fn download_and_replace(
    release: &self_update::update::Release,
    asset_name: &str,
) -> Result<(), String> {
    let asset = release
        .assets
        .iter()
        .find(|a| a.name == asset_name)
        .ok_or_else(|| format!("Asset '{}' not found in release", asset_name))?;

    let tmp_dir =
        self_update::TempDir::new().map_err(|e| format!("Failed to create temp dir: {}", e))?;

    let tmp_tarball_path = tmp_dir.path().join(&asset.name);
    let tmp_tarball = std::fs::File::create(&tmp_tarball_path)
        .map_err(|e| format!("Failed to create temp file: {}", e))?;

    self_update::Download::from_url(&asset.download_url)
        .show_progress(true)
        .download_to(&tmp_tarball)
        .map_err(|e| format!("Failed to download update: {}", e))?;

    let bin_name = "lazytail";
    self_update::Extract::from_source(&tmp_tarball_path)
        .archive(self_update::ArchiveKind::Tar(Some(
            self_update::Compression::Gz,
        )))
        .extract_file(tmp_dir.path(), bin_name)
        .map_err(|e| format!("Failed to extract update: {}", e))?;

    let new_exe = tmp_dir.path().join(bin_name);

    let current_exe = std::env::current_exe()
        .map_err(|e| format!("Failed to determine current executable path: {}", e))?;

    self_update::Move::from_source(&new_exe)
        .replace_using_temp(&current_exe)
        .to_dest(&current_exe)
        .map_err(|e| format!("Failed to replace binary: {}", e))?;

    Ok(())
}
