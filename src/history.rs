use crate::filter::FilterHistoryEntry;
use std::fs;
use std::path::Path;
#[cfg(not(test))]
use std::path::PathBuf;

/// Get the history file path
#[cfg(not(test))]
fn history_file_path() -> Option<PathBuf> {
    crate::source::lazytail_dir().map(|p| p.join("history.json"))
}

/// Load filter history from disk.
///
/// In test builds, returns empty to avoid reading the user's real history file.
/// The core logic in `load_from` is tested directly.
pub fn load_history() -> Vec<FilterHistoryEntry> {
    #[cfg(test)]
    {
        return Vec::new();
    }

    #[cfg(not(test))]
    {
        let Some(path) = history_file_path() else {
            return Vec::new();
        };
        load_from(&path)
    }
}

/// Save filter history to disk.
///
/// In test builds, this is a no-op to avoid corrupting the user's real history file.
/// The core logic in `save_to` is tested directly.
pub fn save_history(history: &[FilterHistoryEntry]) {
    #[cfg(test)]
    {
        let _ = history;
        return;
    }

    #[cfg(not(test))]
    {
        let Some(path) = history_file_path() else {
            return;
        };
        save_to(&path, history);
    }
}

fn load_from(path: &Path) -> Vec<FilterHistoryEntry> {
    if !path.exists() {
        return Vec::new();
    }

    match fs::read_to_string(path) {
        Ok(content) => match serde_json::from_str(&content) {
            Ok(entries) => entries,
            Err(e) => {
                eprintln!("Warning: Failed to parse filter history: {}", e);
                Vec::new()
            }
        },
        Err(e) => {
            // Only log if file exists but can't be read (permission issues, etc.)
            // Don't log for missing files - that's expected on first run
            if path.exists() {
                eprintln!("Warning: Failed to read filter history: {}", e);
            }
            Vec::new()
        }
    }
}

fn save_to(path: &Path, history: &[FilterHistoryEntry]) {
    // Create parent directory if it doesn't exist
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("Failed to create config directory: {}", e);
            return;
        }
    }

    match serde_json::to_string_pretty(history) {
        Ok(content) => {
            if let Err(e) = fs::write(path, content) {
                eprintln!("Failed to save filter history: {}", e);
            }
        }
        Err(e) => {
            eprintln!("Failed to serialize filter history: {}", e);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::FilterMode;
    use tempfile::tempdir;

    #[test]
    fn test_history_serialization() {
        let entries = vec![
            FilterHistoryEntry::new("error".to_string(), FilterMode::default()),
            FilterHistoryEntry::new(
                "warn.*".to_string(),
                FilterMode::Regex {
                    case_sensitive: true,
                },
            ),
            FilterHistoryEntry::new(
                "json | level == \"error\"".to_string(),
                FilterMode::Query {},
            ),
        ];

        let json = serde_json::to_string(&entries).unwrap();
        let loaded: Vec<FilterHistoryEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(entries, loaded);
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("history.json");

        let entries = vec![
            FilterHistoryEntry::new("error".to_string(), FilterMode::default()),
            FilterHistoryEntry::new("warn.*".to_string(), FilterMode::regex()),
        ];

        save_to(&path, &entries);
        let loaded = load_from(&path);
        assert_eq!(entries, loaded);
    }

    #[test]
    fn test_load_missing_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nonexistent.json");

        let loaded = load_from(&path);
        assert!(loaded.is_empty());
    }
}
