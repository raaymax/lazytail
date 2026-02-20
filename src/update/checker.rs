//! GitHub release checking for lazytail updates.
//!
//! Fetches the latest release from GitHub and compares with the current version.
//! Results are cached for 24 hours to avoid repeated API calls.

use super::{get_target, save_cache, UpdateCheckCache, UpdateInfo, REPO_NAME, REPO_OWNER};
use std::time::{SystemTime, UNIX_EPOCH};

/// Check the latest version from GitHub releases.
///
/// Iterates through releases to find the first one that has downloadable
/// assets for the current platform. This skips drafts, prereleases, and
/// releases that lack binaries for this OS/arch.
pub fn check_latest_version() -> Result<UpdateInfo, String> {
    let releases = self_update::backends::github::ReleaseList::configure()
        .repo_owner(REPO_OWNER)
        .repo_name(REPO_NAME)
        .build()
        .map_err(|e| format!("Failed to configure release list: {}", e))?
        .fetch()
        .map_err(|e| format!("Failed to fetch releases: {}", e))?;

    let target = get_target();
    let latest = releases
        .iter()
        .find(|r| r.has_target_asset(&target))
        .ok_or_else(|| format!("No releases found with assets for target '{}'", target))?;

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
