use crate::filter::FilterHistoryEntry;
use std::fs;
use std::path::PathBuf;

/// Get the history file path
fn history_file_path() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("lazytail").join("history.json"))
}

/// Load filter history from disk
pub fn load_history() -> Vec<FilterHistoryEntry> {
    let Some(path) = history_file_path() else {
        return Vec::new();
    };

    if !path.exists() {
        return Vec::new();
    }

    match fs::read_to_string(&path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

/// Save filter history to disk
pub fn save_history(history: &[FilterHistoryEntry]) {
    let Some(path) = history_file_path() else {
        return;
    };

    // Create parent directory if it doesn't exist
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if let Ok(content) = serde_json::to_string_pretty(history) {
        let _ = fs::write(&path, content);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::FilterMode;

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
        ];

        let json = serde_json::to_string(&entries).unwrap();
        let loaded: Vec<FilterHistoryEntry> = serde_json::from_str(&json).unwrap();

        assert_eq!(entries, loaded);
    }
}
