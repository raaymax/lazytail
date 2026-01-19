use super::Filter;

/// Simple string matching filter (case-insensitive)
pub struct StringFilter {
    pattern: String,
    case_sensitive: bool,
}

impl StringFilter {
    pub fn new(pattern: &str, case_sensitive: bool) -> Self {
        Self {
            pattern: if case_sensitive {
                pattern.to_string()
            } else {
                pattern.to_lowercase()
            },
            case_sensitive,
        }
    }
}

impl Filter for StringFilter {
    fn matches(&self, line: &str) -> bool {
        if self.case_sensitive {
            line.contains(&self.pattern)
        } else {
            line.to_lowercase().contains(&self.pattern)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_case_insensitive_matching() {
        let filter = StringFilter::new("error", false);

        assert!(filter.matches("ERROR: Something went wrong"));
        assert!(filter.matches("error: Something went wrong"));
        assert!(filter.matches("Error: Something went wrong"));
        assert!(filter.matches("This is an ERROR message"));
        assert!(!filter.matches("INFO: Everything is fine"));
    }

    #[test]
    fn test_case_sensitive_matching() {
        let filter = StringFilter::new("ERROR", true);

        assert!(filter.matches("ERROR: Something went wrong"));
        assert!(filter.matches("This is an ERROR message"));
        assert!(!filter.matches("error: Something went wrong"));
        assert!(!filter.matches("Error: Something went wrong"));
        assert!(!filter.matches("INFO: Everything is fine"));
    }

    #[test]
    fn test_partial_matching() {
        let filter = StringFilter::new("warn", false);

        assert!(filter.matches("WARNING: Be careful"));
        assert!(filter.matches("warn: minor issue"));
        assert!(filter.matches("I warned you"));
        assert!(!filter.matches("INFO: All good"));
    }

    #[test]
    fn test_empty_pattern() {
        let filter = StringFilter::new("", false);

        // Empty pattern should match everything
        assert!(filter.matches("Any line"));
        assert!(filter.matches(""));
        assert!(filter.matches("12345"));
    }

    #[test]
    fn test_special_characters() {
        let filter = StringFilter::new("[ERROR]", false);

        assert!(filter.matches("[ERROR] Something failed"));
        assert!(filter.matches("Log: [error] Problem detected"));
        assert!(!filter.matches("INFO: All clear"));
    }

    #[test]
    fn test_unicode_characters() {
        let filter = StringFilter::new("日本", false);

        assert!(filter.matches("日本語のログメッセージ"));
        assert!(filter.matches("Message from 日本"));
        assert!(!filter.matches("English only message"));
    }

    #[test]
    fn test_whitespace_handling() {
        let filter = StringFilter::new("  error  ", false);

        // Pattern includes whitespace
        assert!(filter.matches("This is an  error  message"));
        assert!(!filter.matches("This is an error message"));
    }
}
