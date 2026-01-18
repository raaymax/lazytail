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

    fn description(&self) -> String {
        format!(
            "String: {} ({})",
            self.pattern,
            if self.case_sensitive { "case-sensitive" } else { "case-insensitive" }
        )
    }
}
