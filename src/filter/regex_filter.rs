use super::Filter;
use regex::Regex;

/// Regex-based filter
pub struct RegexFilter {
    regex: Regex,
    pattern: String,
}

impl RegexFilter {
    pub fn new(pattern: &str) -> Result<Self, regex::Error> {
        let regex = Regex::new(pattern)?;
        Ok(Self {
            regex,
            pattern: pattern.to_string(),
        })
    }
}

impl Filter for RegexFilter {
    fn matches(&self, line: &str) -> bool {
        self.regex.is_match(line)
    }

    fn description(&self) -> String {
        format!("Regex: {}", self.pattern)
    }
}
