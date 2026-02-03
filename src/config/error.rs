//! Config error types for lazytail.
//!
//! Provides rich error messages with file locations and typo suggestions.

use std::fmt;
use std::path::PathBuf;

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
