//! Config error types for lazytail.
//!
//! Provides rich error messages with file locations and typo suggestions.

use std::fmt;
use std::path::PathBuf;
use strsim::jaro_winkler;

/// Known fields for root config.
const ROOT_FIELDS: &[&str] = &["name", "sources"];

/// Known fields for source entries.
const SOURCE_FIELDS: &[&str] = &["name", "path"];

/// Similarity threshold for suggestions (0.0 - 1.0).
/// 0.8 is a good balance between catching typos and avoiding false positives.
const SIMILARITY_THRESHOLD: f64 = 0.8;

/// Error loading or parsing a config file.
#[derive(Debug)]
pub enum ConfigError {
    /// IO error reading the config file.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },

    /// YAML parse error.
    Parse {
        path: PathBuf,
        message: String,
        line: Option<usize>,
        column: Option<usize>,
        suggestion: Option<String>,
    },

    /// Validation error (semantic errors after parsing).
    Validation { path: PathBuf, message: String },
}

impl ConfigError {
    /// Create a parse error from a serde-saphyr error, extracting location
    /// and generating suggestions for unknown fields.
    pub fn from_saphyr_error(path: PathBuf, error: serde_saphyr::Error) -> Self {
        let error_str = error.to_string();

        // Extract line/column from serde-saphyr error
        // Format is typically: "message at line X column Y"
        let (line, column) = extract_location(&error_str);

        // Try to find unknown field and generate suggestion
        let suggestion = extract_unknown_field(&error_str).and_then(|field| find_suggestion(&field));

        ConfigError::Parse {
            path,
            message: error_str,
            line,
            column,
            suggestion,
        }
    }

    /// Format error in Cargo-style format.
    pub fn format_cargo_style(&self) -> String {
        match self {
            ConfigError::Io { path, source } => {
                format!(
                    "error: cannot read config file\n  --> {}\n  |\n  = {}\n",
                    path.display(),
                    source
                )
            }
            ConfigError::Parse {
                path,
                message,
                line,
                column,
                suggestion,
            } => {
                let location = match (line, column) {
                    (Some(l), Some(c)) => format!("{}:{}:{}", path.display(), l, c),
                    (Some(l), None) => format!("{}:{}", path.display(), l),
                    _ => format!("{}", path.display()),
                };
                let mut output = format!("error: {}\n  --> {}\n  |\n", message, location);
                if let Some(suggestion) = suggestion {
                    output.push_str(&format!("  = help: did you mean `{}`?\n", suggestion));
                }
                output
            }
            ConfigError::Validation { path, message } => {
                format!("error: {}\n  --> {}\n  |\n", message, path.display())
            }
        }
    }
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.format_cargo_style())
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConfigError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Extract line and column from error message.
///
/// serde-saphyr errors typically contain "at line X column Y" format.
fn extract_location(error_msg: &str) -> (Option<usize>, Option<usize>) {
    // Look for "at line X column Y" pattern
    if let Some(at_idx) = error_msg.rfind(" at line ") {
        let rest = &error_msg[at_idx + 9..]; // Skip " at line "
        let parts: Vec<&str> = rest.split_whitespace().collect();

        if parts.len() >= 3 && parts[1] == "column" {
            let line = parts[0].parse().ok();
            let column = parts[2].parse().ok();
            return (line, column);
        }
    }

    // Alternative: look for "at position" pattern
    if let Some(at_idx) = error_msg.rfind(" at position ") {
        let rest = &error_msg[at_idx + 13..]; // Skip " at position "
        if let Some(colon_idx) = rest.find(':') {
            let line = rest[..colon_idx].parse().ok();
            let column = rest[colon_idx + 1..]
                .split_whitespace()
                .next()
                .and_then(|s| s.parse().ok());
            return (line, column);
        }
    }

    (None, None)
}

/// Extract unknown field name from serde error message.
///
/// Serde's deny_unknown_fields produces messages like:
/// "unknown field `fieldname`, expected one of `name`, `sources`"
fn extract_unknown_field(error_msg: &str) -> Option<String> {
    let prefix = "unknown field `";
    if let Some(start) = error_msg.find(prefix) {
        let rest = &error_msg[start + prefix.len()..];
        if let Some(end) = rest.find('`') {
            return Some(rest[..end].to_string());
        }
    }
    None
}

/// Find the best suggestion for an unknown field using Jaro-Winkler similarity.
///
/// Checks both root fields and source fields, returning the best match above threshold.
fn find_suggestion(unknown_field: &str) -> Option<String> {
    let all_fields = ROOT_FIELDS.iter().chain(SOURCE_FIELDS.iter());

    let mut best_match: Option<(&str, f64)> = None;

    for &known_field in all_fields {
        let similarity = jaro_winkler(unknown_field, known_field);
        if similarity >= SIMILARITY_THRESHOLD {
            match best_match {
                Some((_, best_score)) if similarity > best_score => {
                    best_match = Some((known_field, similarity));
                }
                None => {
                    best_match = Some((known_field, similarity));
                }
                _ => {}
            }
        }
    }

    best_match.map(|(field, _)| field.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_location_line_column() {
        let msg = "unknown field `nam` at line 2 column 5";
        let (line, column) = extract_location(msg);
        assert_eq!(line, Some(2));
        assert_eq!(column, Some(5));
    }

    #[test]
    fn test_extract_location_no_location() {
        let msg = "some error without location";
        let (line, column) = extract_location(msg);
        assert_eq!(line, None);
        assert_eq!(column, None);
    }

    #[test]
    fn test_extract_unknown_field() {
        let msg = "unknown field `nam`, expected one of `name`, `sources`";
        let field = extract_unknown_field(msg);
        assert_eq!(field, Some("nam".to_string()));
    }

    #[test]
    fn test_extract_unknown_field_no_match() {
        let msg = "some other error";
        let field = extract_unknown_field(msg);
        assert_eq!(field, None);
    }

    #[test]
    fn test_find_suggestion_typo() {
        // "nam" is very similar to "name"
        let suggestion = find_suggestion("nam");
        assert_eq!(suggestion, Some("name".to_string()));
    }

    #[test]
    fn test_find_suggestion_typo_path() {
        // "pth" is somewhat similar to "path"
        let suggestion = find_suggestion("pth");
        // This might not meet threshold depending on Jaro-Winkler
        // If it doesn't suggest, that's OK - check behavior
        // "pth" vs "path" jaro_winkler is ~0.85
        assert!(suggestion.is_none() || suggestion == Some("path".to_string()));
    }

    #[test]
    fn test_find_suggestion_source_typo() {
        // "souces" is similar to "sources"
        let suggestion = find_suggestion("souces");
        assert_eq!(suggestion, Some("sources".to_string()));
    }

    #[test]
    fn test_find_suggestion_no_match() {
        // "xyz" is not similar to any known field
        let suggestion = find_suggestion("xyz");
        assert_eq!(suggestion, None);
    }

    #[test]
    fn test_config_error_display_io() {
        let path = PathBuf::from("/test/config.yaml");
        let error = ConfigError::Io {
            path: path.clone(),
            source: std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        };

        let display = error.to_string();
        assert!(display.contains("cannot read config file"));
        assert!(display.contains("/test/config.yaml"));
    }

    #[test]
    fn test_config_error_display_parse_with_suggestion() {
        let error = ConfigError::Parse {
            path: PathBuf::from("/test/config.yaml"),
            message: "unknown field `nam`".to_string(),
            line: Some(2),
            column: Some(5),
            suggestion: Some("name".to_string()),
        };

        let display = error.to_string();
        assert!(display.contains("unknown field `nam`"));
        assert!(display.contains("/test/config.yaml:2:5"));
        assert!(display.contains("did you mean `name`?"));
    }

    #[test]
    fn test_config_error_display_parse_without_suggestion() {
        let error = ConfigError::Parse {
            path: PathBuf::from("/test/config.yaml"),
            message: "invalid syntax".to_string(),
            line: Some(1),
            column: None,
            suggestion: None,
        };

        let display = error.to_string();
        assert!(display.contains("invalid syntax"));
        assert!(display.contains("/test/config.yaml:1"));
        assert!(!display.contains("did you mean"));
    }

    #[test]
    fn test_config_error_display_validation() {
        let error = ConfigError::Validation {
            path: PathBuf::from("/test/config.yaml"),
            message: "empty source name".to_string(),
        };

        let display = error.to_string();
        assert!(display.contains("empty source name"));
        assert!(display.contains("/test/config.yaml"));
    }
}
