//! GitHub release checking for lazytail updates.
//!
//! Fetches the latest release from GitHub and compares with the current version.
//! Results are cached for 24 hours to avoid repeated API calls.

use super::{save_cache, UpdateCheckCache, UpdateInfo};
use std::time::{SystemTime, UNIX_EPOCH};

const REPO_OWNER: &str = "raaymax";
const REPO_NAME: &str = "lazytail";

/// Check the latest version from GitHub releases.
pub fn check_latest_version() -> Result<UpdateInfo, String> {
    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .map_err(|e| format!("Failed to configure release list: {}", e))?
        .fetch()
        .map_err(|e| format!("Failed to fetch releases: {}", e))?;

    let latest = releases
        .first()
        .ok_or_else(|| "No releases found".to_string())?;

    let current_version = env!("CARGO_PKG_VERSION").to_string();
    let latest_version = latest.version.clone();
    let release_url = format!(
        "https://github.com/{}/{}/releases/tag/{}",
        REPO_OWNER, REPO_NAME, latest.name
    );

    Ok(UpdateInfo {
        current_version,
        latest_version,
        release_url,
    })
}

/// Check for updates, using cache if available and fresh.
pub fn check_with_cache() -> Result<UpdateInfo, String> {
    // Try cached result first
    if let Some(cache) = super::load_cache() {
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        return Ok(UpdateInfo {
            current_version,
            latest_version: cache.latest_version,
            release_url: cache.release_url,
        });
    }

    // Cache miss or stale — fetch from GitHub
    let info = check_latest_version()?;

    // Save to cache
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    save_cache(&UpdateCheckCache {
        checked_at: now,
        latest_version: info.latest_version.clone(),
        release_url: info.release_url.clone(),
    });

    Ok(info)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore] // Requires network access
    fn test_check_latest_version() {
        let result = check_latest_version();
        assert!(result.is_ok(), "Failed: {:?}", result.err());
        let info = result.unwrap();
        assert!(!info.latest_version.is_empty());
        assert!(!info.release_url.is_empty());
    }
}
