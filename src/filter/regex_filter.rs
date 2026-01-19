use super::Filter;
use regex::Regex;

/// Regex-based filter
#[allow(dead_code)]
pub struct RegexFilter {
    regex: Regex,
}

#[allow(dead_code)]
impl RegexFilter {
    pub fn new(pattern: &str) -> Result<Self, regex::Error> {
        let regex = Regex::new(pattern)?;
        Ok(Self { regex })
    }
}

impl Filter for RegexFilter {
    fn matches(&self, line: &str) -> bool {
        self.regex.is_match(line)
    }
}
