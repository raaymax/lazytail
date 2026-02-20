//! Self-update functionality for lazytail.
//!
//! Provides update checking, caching, binary installation, and package manager detection.
//! Gated behind the `self-update` cargo feature flag.

pub mod checker;
pub mod detection;
pub mod installer;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// How long a cached update check remains valid (24 hours).
const CACHE_TTL_SECS: u64 = 24 * 60 * 60;

/// Information about an available update.
#[derive(Debug, Clone)]
pub struct UpdateInfo {
    pub current_version: String,
    pub latest_version: String,
    pub release_url: String,
}

impl UpdateInfo {
    /// Returns true if the latest version is newer than the current version.
    pub fn is_update_available(&self) -> bool {
        self.current_version != self.latest_version
            && version_is_newer(&self.latest_version, &self.current_version)
    }
}

/// How lazytail was installed, affecting how updates should be performed.
#[derive(Debug, Clone)]
pub enum InstallMethod {
    /// Installed via direct download (GitHub release) — can self-replace.
    SelfManaged,
    /// Installed via a package manager — user should update through it.
    PackageManager { name: String, upgrade_cmd: String },
}

/// Cached result of a GitHub release check.
#[derive(Debug, Serialize, Deserialize)]
pub struct UpdateCheckCache {
    pub checked_at: u64,
    pub latest_version: String,
    pub release_url: String,
}

/// Returns the path to the update check cache file.
pub fn cache_path() -> Option<PathBuf> {
    crate::source::lazytail_dir().map(|p| p.join("update_check.json"))
}

/// Load cached update check if it exists and is fresh (< 24h).
pub fn load_cache() -> Option<UpdateCheckCache> {
    let path = cache_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let cache: UpdateCheckCache = serde_json::from_str(&content).ok()?;

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();

    if now.saturating_sub(cache.checked_at) < CACHE_TTL_SECS {
        Some(cache)
    } else {
        None
    }
}

/// Save update check result to cache. Silently ignores errors.
pub fn save_cache(cache: &UpdateCheckCache) {
    let Some(path) = cache_path() else {
        return;
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let Ok(content) = serde_json::to_string(cache) else {
        return;
    };
    let _ = std::fs::write(&path, content);
}

/// Compare two semver-like version strings ("X.Y.Z").
/// Returns true if `latest` is strictly newer than `current`.
fn version_is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim_start_matches('v')
            .split('.')
            .filter_map(|s| s.parse().ok())
            .collect()
    };
    let l = parse(latest);
    let c = parse(current);
    l > c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_available() {
        let info = UpdateInfo {
            current_version: "0.6.0".to_string(),
            latest_version: "0.7.0".to_string(),
            release_url: "https://example.com".to_string(),
        };
        assert!(info.is_update_available());
    }

    #[test]
    fn test_no_update_when_same_version() {
        let info = UpdateInfo {
            current_version: "0.6.0".to_string(),
            latest_version: "0.6.0".to_string(),
            release_url: "https://example.com".to_string(),
        };
        assert!(!info.is_update_available());
    }

    #[test]
    fn test_no_update_when_current_is_newer() {
        let info = UpdateInfo {
            current_version: "0.8.0".to_string(),
            latest_version: "0.7.0".to_string(),
            release_url: "https://example.com".to_string(),
        };
        assert!(!info.is_update_available());
    }

    #[test]
    fn test_version_is_newer() {
        assert!(version_is_newer("0.7.0", "0.6.0"));
        assert!(version_is_newer("1.0.0", "0.9.9"));
        assert!(version_is_newer("0.6.1", "0.6.0"));
        assert!(!version_is_newer("0.6.0", "0.6.0"));
        assert!(!version_is_newer("0.5.0", "0.6.0"));
    }

    #[test]
    fn test_version_is_newer_with_v_prefix() {
        assert!(version_is_newer("v0.7.0", "v0.6.0"));
        assert!(version_is_newer("v0.7.0", "0.6.0"));
    }

    #[test]
    fn test_cache_serialization_roundtrip() {
        let cache = UpdateCheckCache {
            checked_at: 1700000000,
            latest_version: "0.7.0".to_string(),
            release_url: "https://github.com/raaymax/lazytail/releases/tag/v0.7.0".to_string(),
        };
        let json = serde_json::to_string(&cache).unwrap();
        let parsed: UpdateCheckCache = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.checked_at, cache.checked_at);
        assert_eq!(parsed.latest_version, cache.latest_version);
        assert_eq!(parsed.release_url, cache.release_url);
    }

    #[test]
    fn test_cache_expiry() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        // Fresh cache (1 hour old)
        let fresh = UpdateCheckCache {
            checked_at: now - 3600,
            latest_version: "0.7.0".to_string(),
            release_url: "https://example.com".to_string(),
        };
        assert!(now.saturating_sub(fresh.checked_at) < CACHE_TTL_SECS);

        // Stale cache (25 hours old)
        let stale = UpdateCheckCache {
            checked_at: now - 90000,
            latest_version: "0.7.0".to_string(),
            release_url: "https://example.com".to_string(),
        };
        assert!(now.saturating_sub(stale.checked_at) >= CACHE_TTL_SECS);
    }
}
