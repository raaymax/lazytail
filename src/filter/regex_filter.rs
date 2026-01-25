use super::Filter;
use regex::{Regex, RegexBuilder};

/// Regex-based filter
pub struct RegexFilter {
    regex: Regex,
}

impl RegexFilter {
    /// Create a new regex filter with case sensitivity option
    pub fn new(pattern: &str, case_sensitive: bool) -> Result<Self, regex::Error> {
        let regex = RegexBuilder::new(pattern)
            .case_insensitive(!case_sensitive)
            .build()?;
        Ok(Self { regex })
    }
}

impl Filter for RegexFilter {
    fn matches(&self, line: &str) -> bool {
        self.regex.is_match(line)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_regex_matching() {
        let filter = RegexFilter::new(r"ERROR", true).unwrap();

        assert!(filter.matches("ERROR: Something went wrong"));
        assert!(filter.matches("This is an ERROR"));
        assert!(!filter.matches("error: lowercase"));
        assert!(!filter.matches("INFO: All good"));
    }

    #[test]
    fn test_case_insensitive_regex() {
        let filter = RegexFilter::new(r"error", false).unwrap();

        assert!(filter.matches("ERROR: Something went wrong"));
        assert!(filter.matches("error: Something went wrong"));
        assert!(filter.matches("Error: Something went wrong"));
        assert!(!filter.matches("INFO: All good"));
    }

    #[test]
    fn test_case_sensitive_regex() {
        let filter = RegexFilter::new(r"error", true).unwrap();

        assert!(!filter.matches("ERROR: Something went wrong"));
        assert!(filter.matches("error: Something went wrong"));
        assert!(!filter.matches("Error: Something went wrong"));
    }

    #[test]
    fn test_pattern_anchors() {
        let filter = RegexFilter::new(r"^ERROR", true).unwrap();

        assert!(filter.matches("ERROR: at start"));
        assert!(!filter.matches("Prefix ERROR: not at start"));

        let filter_end = RegexFilter::new(r"ERROR$", true).unwrap();
        assert!(filter_end.matches("Line ends with ERROR"));
        assert!(!filter_end.matches("ERROR: has suffix"));
    }

    #[test]
    fn test_character_classes() {
        let filter = RegexFilter::new(r"\d{4}-\d{2}-\d{2}", false).unwrap();

        assert!(filter.matches("Date: 2026-01-19"));
        assert!(filter.matches("2026-01-19 12:00:00"));
        assert!(!filter.matches("Date: 01-19-2026"));
        assert!(!filter.matches("No date here"));
    }

    #[test]
    fn test_alternation() {
        let filter = RegexFilter::new(r"ERROR|WARN|FATAL", true).unwrap();

        assert!(filter.matches("ERROR: Something failed"));
        assert!(filter.matches("WARN: Be careful"));
        assert!(filter.matches("FATAL: Critical issue"));
        assert!(!filter.matches("INFO: All good"));
        assert!(!filter.matches("DEBUG: Details"));
    }

    #[test]
    fn test_word_boundaries() {
        let filter = RegexFilter::new(r"\berror\b", false).unwrap();

        assert!(filter.matches("error: standalone word"));
        assert!(filter.matches("An error occurred"));
        assert!(!filter.matches("errors: plural"));
        assert!(!filter.matches("errorcode: compound"));
    }

    #[test]
    fn test_quantifiers() {
        let filter = RegexFilter::new(r"E+ROR", true).unwrap();

        assert!(filter.matches("EROR"));
        assert!(filter.matches("EEROR"));
        assert!(filter.matches("EEEEROR"));
        assert!(!filter.matches("ROR")); // No E at all
    }

    #[test]
    fn test_capturing_groups() {
        let filter = RegexFilter::new(r"(\w+):\s*(.+)", false).unwrap();

        assert!(filter.matches("ERROR: Something failed"));
        assert!(filter.matches("INFO: All good"));
        assert!(filter.matches("timestamp: 2026-01-19"));
        assert!(!filter.matches("No colon separator"));
    }

    #[test]
    fn test_special_characters_escaped() {
        let filter = RegexFilter::new(r"\[ERROR\]", true).unwrap();

        assert!(filter.matches("[ERROR] Bracketed"));
        assert!(!filter.matches("ERROR without brackets"));
    }

    #[test]
    fn test_invalid_regex() {
        let result = RegexFilter::new(r"[invalid(", false);

        assert!(result.is_err());
    }

    #[test]
    fn test_complex_log_pattern() {
        // Match timestamp + level + message pattern
        let filter = RegexFilter::new(
            r"\d{4}-\d{2}-\d{2}\s+\d{2}:\d{2}:\d{2}\s+(ERROR|WARN)",
            true,
        )
        .unwrap();

        assert!(filter.matches("2026-01-19 14:30:00 ERROR: Failed"));
        assert!(filter.matches("2026-01-19 14:30:00 WARN: Warning"));
        assert!(!filter.matches("2026-01-19 14:30:00 INFO: Info"));
        assert!(!filter.matches("Invalid format ERROR"));
    }

    #[test]
    fn test_unicode_regex() {
        let filter = RegexFilter::new(r"エラー", false).unwrap();

        assert!(filter.matches("エラーが発生しました"));
        assert!(filter.matches("ログ: エラー"));
        assert!(!filter.matches("正常に動作中"));
    }
}
