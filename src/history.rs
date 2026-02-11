use crate::filter::FilterHistoryEntry;
use std::fs;
use std::path::PathBuf;

/// Get the history file path
fn history_file_path() -> Option<PathBuf> {
    crate::source::lazytail_dir().map(|p| p.join("history.json"))
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

/// Save filter history to disk
pub fn save_history(history: &[FilterHistoryEntry]) {
    let Some(path) = history_file_path() else {
        return;
    };

    // Create parent directory if it doesn't exist
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            eprintln!("Failed to create config directory: {}", e);
            return;
        }
    }

    match serde_json::to_string_pretty(history) {
        Ok(content) => {
            if let Err(e) = fs::write(&path, content) {
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
