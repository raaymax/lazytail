pub mod engine;
pub mod regex_filter;
pub mod string_filter;

/// Trait for extensible filtering
pub trait Filter: Send + Sync {
    fn matches(&self, line: &str) -> bool;
}

use serde::{Deserialize, Serialize};

/// Filter mode for switching between plain text and regex filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterMode {
    Plain { case_sensitive: bool },
    Regex { case_sensitive: bool },
}

impl Default for FilterMode {
    fn default() -> Self {
        FilterMode::Plain {
            case_sensitive: false,
        }
    }
}

impl FilterMode {
    /// Create a new plain text filter mode (case-insensitive by default)
    #[cfg(test)]
    pub fn plain() -> Self {
        FilterMode::Plain {
            case_sensitive: false,
        }
    }

    /// Create a new regex filter mode (case-insensitive by default)
    #[cfg(test)]
    pub fn regex() -> Self {
        FilterMode::Regex {
            case_sensitive: false,
        }
    }

    /// Toggle between Plain and Regex modes, preserving case sensitivity
    pub fn toggle_mode(&mut self) {
        *self = match *self {
            FilterMode::Plain { case_sensitive } => FilterMode::Regex { case_sensitive },
            FilterMode::Regex { case_sensitive } => FilterMode::Plain { case_sensitive },
        };
    }

    /// Toggle case sensitivity within the current mode
    pub fn toggle_case_sensitivity(&mut self) {
        match self {
            FilterMode::Plain { case_sensitive } => *case_sensitive = !*case_sensitive,
            FilterMode::Regex { case_sensitive } => *case_sensitive = !*case_sensitive,
        }
    }

    /// Check if current mode is regex
    pub fn is_regex(&self) -> bool {
        matches!(self, FilterMode::Regex { .. })
    }

    /// Check if current mode is case sensitive
    pub fn is_case_sensitive(&self) -> bool {
        match self {
            FilterMode::Plain { case_sensitive } | FilterMode::Regex { case_sensitive } => {
                *case_sensitive
            }
        }
    }

    /// Get display label for the filter prompt
    pub fn prompt_label(&self) -> &'static str {
        match self {
            FilterMode::Plain {
                case_sensitive: false,
            } => "Filter",
            FilterMode::Plain {
                case_sensitive: true,
            } => "Filter [Aa]",
            FilterMode::Regex {
                case_sensitive: false,
            } => "Regex",
            FilterMode::Regex {
                case_sensitive: true,
            } => "Regex [Aa]",
        }
    }
}

/// A filter history entry that stores both the pattern and the mode
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterHistoryEntry {
    pub pattern: String,
    pub mode: FilterMode,
}

impl FilterHistoryEntry {
    /// Create a new history entry
    pub fn new(pattern: String, mode: FilterMode) -> Self {
        Self { pattern, mode }
    }

    /// Check if this entry matches another (same pattern and mode)
    pub fn matches(&self, other: &FilterHistoryEntry) -> bool {
        self.pattern == other.pattern && self.mode == other.mode
    }
}

#[cfg(test)]
mod filter_history_entry_tests {
    use super::*;

    #[test]
    fn test_new_entry() {
        let entry = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        assert_eq!(entry.pattern, "error");
        assert!(!entry.mode.is_regex());
    }

    #[test]
    fn test_entry_with_regex_mode() {
        let entry = FilterHistoryEntry::new("err.*".to_string(), FilterMode::regex());
        assert_eq!(entry.pattern, "err.*");
        assert!(entry.mode.is_regex());
    }

    #[test]
    fn test_entry_preserves_case_sensitivity() {
        let mode = FilterMode::Regex {
            case_sensitive: true,
        };
        let entry = FilterHistoryEntry::new("Error".to_string(), mode);
        assert!(entry.mode.is_case_sensitive());
    }

    #[test]
    fn test_matches_same_pattern_and_mode() {
        let entry1 = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        let entry2 = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        assert!(entry1.matches(&entry2));
    }

    #[test]
    fn test_matches_different_pattern() {
        let entry1 = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        let entry2 = FilterHistoryEntry::new("warn".to_string(), FilterMode::plain());
        assert!(!entry1.matches(&entry2));
    }

    #[test]
    fn test_matches_different_mode() {
        let entry1 = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        let entry2 = FilterHistoryEntry::new("error".to_string(), FilterMode::regex());
        assert!(!entry1.matches(&entry2));
    }

    #[test]
    fn test_matches_different_case_sensitivity() {
        let entry1 = FilterHistoryEntry::new(
            "error".to_string(),
            FilterMode::Plain {
                case_sensitive: false,
            },
        );
        let entry2 = FilterHistoryEntry::new(
            "error".to_string(),
            FilterMode::Plain {
                case_sensitive: true,
            },
        );
        assert!(!entry1.matches(&entry2));
    }
}

#[cfg(test)]
mod filter_mode_tests {
    use super::*;

    #[test]
    fn test_default_is_plain_case_insensitive() {
        let mode = FilterMode::default();
        assert!(!mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_plain_constructor() {
        let mode = FilterMode::plain();
        assert!(!mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_regex_constructor() {
        let mode = FilterMode::regex();
        assert!(mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_mode_plain_to_regex() {
        let mut mode = FilterMode::plain();
        mode.toggle_mode();
        assert!(mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_mode_regex_to_plain() {
        let mut mode = FilterMode::regex();
        mode.toggle_mode();
        assert!(!mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_mode_preserves_case_sensitivity() {
        let mut mode = FilterMode::Plain {
            case_sensitive: true,
        };
        mode.toggle_mode();
        assert!(mode.is_regex());
        assert!(mode.is_case_sensitive());

        mode.toggle_mode();
        assert!(!mode.is_regex());
        assert!(mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_case_sensitivity_plain() {
        let mut mode = FilterMode::plain();
        assert!(!mode.is_case_sensitive());

        mode.toggle_case_sensitivity();
        assert!(mode.is_case_sensitive());

        mode.toggle_case_sensitivity();
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_case_sensitivity_regex() {
        let mut mode = FilterMode::regex();
        assert!(!mode.is_case_sensitive());

        mode.toggle_case_sensitivity();
        assert!(mode.is_case_sensitive());

        mode.toggle_case_sensitivity();
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_prompt_label_plain() {
        let mode = FilterMode::Plain {
            case_sensitive: false,
        };
        assert_eq!(mode.prompt_label(), "Filter");

        let mode = FilterMode::Plain {
            case_sensitive: true,
        };
        assert_eq!(mode.prompt_label(), "Filter [Aa]");
    }

    #[test]
    fn test_prompt_label_regex() {
        let mode = FilterMode::Regex {
            case_sensitive: false,
        };
        assert_eq!(mode.prompt_label(), "Regex");

        let mode = FilterMode::Regex {
            case_sensitive: true,
        };
        assert_eq!(mode.prompt_label(), "Regex [Aa]");
    }

    #[test]
    fn test_filter_mode_clone() {
        let mode1 = FilterMode::Regex {
            case_sensitive: true,
        };
        let mode2 = mode1;
        assert_eq!(mode1, mode2);
    }
}
