//! Request and response types for MCP tools.

use crate::filter::query::FilterQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Core parsing logic for flexible usize conversion.
/// Accepts u64, i64, f64, or string representations of non-negative integers.
mod flexible_usize {
    use serde::de;

    pub(super) fn from_u64<E: de::Error>(v: u64) -> Result<usize, E> {
        usize::try_from(v).map_err(|_| {
            E::custom(format!(
                "value {} is too large (maximum is {})",
                v,
                usize::MAX
            ))
        })
    }

    pub(super) fn from_i64<E: de::Error>(v: i64) -> Result<usize, E> {
        if v < 0 {
            Err(E::custom(format!(
                "expected a non-negative integer, got {}",
                v
            )))
        } else {
            from_u64(v as u64)
        }
    }

    pub(super) fn from_f64<E: de::Error>(v: f64) -> Result<usize, E> {
        if v.fract() != 0.0 {
            Err(E::custom(format!(
                "expected an integer, got floating-point value {}",
                v
            )))
        } else if v < 0.0 {
            Err(E::custom(format!(
                "expected a non-negative integer, got {}",
                v
            )))
        } else {
            from_u64(v as u64)
        }
    }

    pub(super) fn from_str<E: de::Error>(v: &str) -> Result<usize, E> {
        v.parse::<usize>().map_err(|_| {
            E::custom(format!(
                "invalid value \"{}\": expected a non-negative integer",
                v
            ))
        })
    }
}

/// Deserialize a `usize` that accepts both numeric values and string-encoded numbers.
///
/// MCP clients sometimes send numeric parameters as strings (e.g., `"100"` instead of `100`).
/// This deserializer accepts both forms and provides descriptive parse errors.
pub(crate) fn deserialize_flexible_usize<'de, D>(deserializer: D) -> Result<usize, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct V;

    impl<'de> Visitor<'de> for V {
        type Value = usize;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str("a non-negative integer or a string containing a non-negative integer")
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            flexible_usize::from_u64(v)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            flexible_usize::from_i64(v)
        }

        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
            flexible_usize::from_f64(v)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            flexible_usize::from_str(v)
        }
    }

    deserializer.deserialize_any(V)
}

/// Same as [`deserialize_flexible_usize`] but wrapped in `Option` for optional fields.
pub(crate) fn deserialize_flexible_usize_option<'de, D>(
    deserializer: D,
) -> Result<Option<usize>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::{self, Visitor};
    use std::fmt;

    struct V;

    impl<'de> Visitor<'de> for V {
        type Value = Option<usize>;

        fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
            f.write_str(
                "null, a non-negative integer, or a string containing a non-negative integer",
            )
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D2>(self, deserializer: D2) -> Result<Self::Value, D2::Error>
        where
            D2: serde::Deserializer<'de>,
        {
            deserialize_flexible_usize(deserializer).map(Some)
        }

        fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
            flexible_usize::from_u64(v).map(Some)
        }

        fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
            flexible_usize::from_i64(v).map(Some)
        }

        fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
            flexible_usize::from_f64(v).map(Some)
        }

        fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
            flexible_usize::from_str(v).map(Some)
        }
    }

    deserializer.deserialize_any(V)
}

/// Output format for tool responses.
#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum OutputFormat {
    /// Plain text format optimized for AI consumption (less escaping overhead)
    #[default]
    Text,
    /// JSON format for programmatic consumption
    Json,
}

fn default_count() -> usize {
    100
}

fn default_max_results() -> usize {
    100
}

fn default_context() -> usize {
    5
}

/// Request to fetch lines from a lazytail source.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetLinesRequest {
    /// Source name (from list_sources)
    pub source: String,
    /// Starting line number (0-indexed)
    #[serde(default, deserialize_with = "deserialize_flexible_usize")]
    pub start: usize,
    /// Number of lines to fetch (default 100, max 1000)
    #[serde(
        default = "default_count",
        deserialize_with = "deserialize_flexible_usize"
    )]
    pub count: usize,
    /// Return raw content with ANSI escape codes intact (default: false, strips ANSI)
    #[serde(default)]
    pub raw: bool,
    /// Output format: "text" (default, plain text) or "json"
    #[serde(default)]
    pub output: OutputFormat,
    /// Return full line content without truncation (default: false, lines over 500 chars are truncated)
    #[serde(default)]
    pub full_content: bool,
}

/// Response containing lines from a log file.
#[derive(Debug, Serialize, JsonSchema)]
#[cfg_attr(test, derive(Deserialize))]
pub struct GetLinesResponse {
    /// The requested lines
    pub lines: Vec<LineInfo>,
    /// Total lines in the file
    pub total_lines: usize,
    /// Whether more lines exist after the requested range
    pub has_more: bool,
}

/// Information about a single line.
#[derive(Debug, Serialize, JsonSchema)]
#[cfg_attr(test, derive(Deserialize))]
pub struct LineInfo {
    /// Line number (0-indexed)
    pub line_number: usize,
    /// Line content
    pub content: String,
    /// Severity level (from columnar index, if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity: Option<String>,
    /// Rendered line content using preset formatting (if a preset matched)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendered: Option<String>,
}

/// Search mode for pattern matching.
#[derive(Debug, Default, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SearchMode {
    /// Plain text search (fast, literal matching)
    #[default]
    Plain,
    /// Regular expression search
    Regex,
}

/// Request to search for patterns in a lazytail source.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchRequest {
    /// Source name (from list_sources)
    pub source: String,
    /// Search pattern (plain text or regex). Not required when using `query`.
    #[serde(default)]
    pub pattern: String,
    /// Search mode: "plain" or "regex" (default: plain)
    #[serde(default)]
    pub mode: SearchMode,
    /// Case sensitive search (default: false)
    #[serde(default)]
    pub case_sensitive: bool,
    /// Maximum number of results to return (default 100, max 1000)
    #[serde(
        default = "default_max_results",
        deserialize_with = "deserialize_flexible_usize"
    )]
    pub max_results: usize,
    /// Number of context lines before and after each match (default 0, max 50)
    #[serde(default, deserialize_with = "deserialize_flexible_usize")]
    pub context_lines: usize,
    /// Return raw content with ANSI escape codes intact (default: false, strips ANSI)
    #[serde(default)]
    pub raw: bool,
    /// Output format: "text" (default, plain text) or "json"
    #[serde(default)]
    pub output: OutputFormat,
    /// Structured query for field-based filtering (LogQL-style).
    /// When provided, pattern/mode/case_sensitive are ignored.
    /// Example: {"parser": "json", "filters": [{"field": "level", "op": "eq", "value": "error"}]}
    #[serde(default)]
    pub query: Option<FilterQuery>,
    /// Return full line content without truncation (default: false, lines over 500 chars are truncated)
    #[serde(default)]
    pub full_content: bool,
}

/// Response containing search results.
#[derive(Debug, Serialize, JsonSchema)]
#[cfg_attr(test, derive(Deserialize))]
pub struct SearchResponse {
    /// Matching lines with optional context
    pub matches: Vec<SearchMatch>,
    /// Total number of matches found (may be more than returned if truncated)
    pub total_matches: usize,
    /// Whether results were truncated due to max_results limit
    pub truncated: bool,
    /// Total lines searched in the file
    pub lines_searched: usize,
}

/// A single search match with optional context.
#[derive(Debug, Serialize, JsonSchema)]
#[cfg_attr(test, derive(Deserialize))]
pub struct SearchMatch {
    /// Line number of the match (0-indexed)
    pub line_number: usize,
    /// The matching line content
    pub content: String,
    /// Context lines before the match (if requested)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    /// Context lines after the match (if requested)
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
}

/// Request to fetch the last N lines from a lazytail source.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTailRequest {
    /// Source name (from list_sources)
    pub source: String,
    /// Number of lines to fetch from the end (default 100, max 1000)
    #[serde(
        default = "default_count",
        deserialize_with = "deserialize_flexible_usize"
    )]
    pub count: usize,
    /// Only return lines after this line number (0-indexed, exclusive).
    /// Enables efficient incremental polling — pass the last line_number
    /// you received to get only new lines. When set, returns up to `count`
    /// lines starting from `since_line + 1`.
    #[serde(default, deserialize_with = "deserialize_flexible_usize_option")]
    pub since_line: Option<usize>,
    /// Return raw content with ANSI escape codes intact (default: false, strips ANSI)
    #[serde(default)]
    pub raw: bool,
    /// Output format: "text" (default, plain text) or "json"
    #[serde(default)]
    pub output: OutputFormat,
    /// Return full line content without truncation (default: false, lines over 500 chars are truncated)
    #[serde(default)]
    pub full_content: bool,
}

/// Request to get context around a specific line in a lazytail source.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetContextRequest {
    /// Source name (from list_sources)
    pub source: String,
    /// The target line number (0-indexed)
    #[serde(deserialize_with = "deserialize_flexible_usize")]
    pub line_number: usize,
    /// Number of lines before the target (default 5, max 50)
    #[serde(
        default = "default_context",
        deserialize_with = "deserialize_flexible_usize"
    )]
    pub before: usize,
    /// Number of lines after the target (default 5, max 50)
    #[serde(
        default = "default_context",
        deserialize_with = "deserialize_flexible_usize"
    )]
    pub after: usize,
    /// Return raw content with ANSI escape codes intact (default: false, strips ANSI)
    #[serde(default)]
    pub raw: bool,
    /// Output format: "text" (default, plain text) or "json"
    #[serde(default)]
    pub output: OutputFormat,
    /// Return full line content without truncation (default: false, lines over 500 chars are truncated)
    #[serde(default)]
    pub full_content: bool,
}

/// Response containing context around a line.
#[derive(Debug, Serialize, JsonSchema)]
#[cfg_attr(test, derive(Deserialize))]
pub struct GetContextResponse {
    /// Lines before the target
    pub before_lines: Vec<LineInfo>,
    /// The target line
    pub target_line: LineInfo,
    /// Lines after the target
    pub after_lines: Vec<LineInfo>,
    /// Total lines in the file
    pub total_lines: usize,
}

/// Request to list available sources (no parameters needed).
#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListSourcesRequest {}

/// Response containing available log sources.
#[derive(Debug, Serialize, JsonSchema)]
pub struct ListSourcesResponse {
    /// Available log sources
    pub sources: Vec<SourceInfo>,
    /// Path to the data directory
    pub data_directory: PathBuf,
}

/// Information about a log source.
#[derive(Debug, Serialize, JsonSchema)]
pub struct SourceInfo {
    /// Source name (without .log extension)
    pub name: String,
    /// Full path to the log file
    pub path: PathBuf,
    /// Whether the source is actively being written to
    pub status: SourceStatus,
    /// File size in bytes
    pub size_bytes: u64,
    /// Where the source was found (project-local or global)
    pub location: SourceLocation,
    /// Renderer preset names assigned to this source
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub renderer_names: Vec<String>,
}

/// Status of a log source.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SourceStatus {
    /// Source is actively being written to (capture process running)
    Active,
    /// Source capture has ended (file still available)
    Ended,
}

/// Location where a source was discovered.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SourceLocation {
    /// Source is in project-local .lazytail/data/
    Project,
    /// Source is in global ~/.config/lazytail/data/
    Global,
}

/// Information about a single aggregation group.
#[derive(Debug, Serialize, JsonSchema)]
#[cfg_attr(test, derive(Deserialize))]
pub struct AggregationGroupInfo {
    /// Field name-value pairs forming the group key.
    pub key: std::collections::HashMap<String, String>,
    /// Number of matching lines in this group.
    pub count: usize,
}

/// Response containing aggregation results.
#[derive(Debug, Serialize, JsonSchema)]
#[cfg_attr(test, derive(Deserialize))]
pub struct AggregationResponse {
    /// Groups sorted by count descending.
    pub groups: Vec<AggregationGroupInfo>,
    /// Total number of matching lines across all groups.
    pub total_matches: usize,
    /// Total lines searched in the file.
    pub lines_searched: usize,
}

/// Request to get index stats for a lazytail source.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetStatsRequest {
    /// Source name (from list_sources)
    pub source: String,
    /// Output format: "text" (default, plain text) or "json"
    #[serde(default)]
    pub output: OutputFormat,
}

/// Response containing index statistics for a source.
#[derive(Debug, Serialize, JsonSchema)]
#[cfg_attr(test, derive(Deserialize))]
pub struct GetStatsResponse {
    /// Source name
    pub source: String,
    /// Total number of indexed lines
    pub indexed_lines: u64,
    /// Log file size in bytes (as recorded in index)
    pub log_file_size: u64,
    /// Whether a columnar index exists
    pub has_index: bool,
    /// Severity counts from flags column
    #[serde(skip_serializing_if = "Option::is_none")]
    pub severity_counts: Option<SeverityCountsInfo>,
    /// Approximate ingestion rate in lines per second (from checkpoint timestamps)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lines_per_second: Option<f64>,
    /// Which index columns are present
    pub columns: Vec<String>,
}

/// Severity count breakdown from checkpoint data.
#[derive(Debug, Serialize, JsonSchema)]
#[cfg_attr(test, derive(Deserialize))]
pub struct SeverityCountsInfo {
    pub unknown: u32,
    pub trace: u32,
    pub debug: u32,
    pub info: u32,
    pub warn: u32,
    pub error: u32,
    pub fatal: u32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn get_lines_accepts_numeric_params() {
        let req: GetLinesRequest =
            serde_json::from_value(json!({"source": "test", "start": 10, "count": 50})).unwrap();
        assert_eq!(req.start, 10);
        assert_eq!(req.count, 50);
    }

    #[test]
    fn get_lines_accepts_string_params() {
        let req: GetLinesRequest =
            serde_json::from_value(json!({"source": "test", "start": "10", "count": "50"}))
                .unwrap();
        assert_eq!(req.start, 10);
        assert_eq!(req.count, 50);
    }

    #[test]
    fn get_lines_defaults_when_omitted() {
        let req: GetLinesRequest = serde_json::from_value(json!({"source": "test"})).unwrap();
        assert_eq!(req.start, 0);
        assert_eq!(req.count, 100);
    }

    #[test]
    fn get_tail_accepts_string_since_line() {
        let req: GetTailRequest =
            serde_json::from_value(json!({"source": "test", "count": "200", "since_line": "42"}))
                .unwrap();
        assert_eq!(req.count, 200);
        assert_eq!(req.since_line, Some(42));
    }

    #[test]
    fn get_tail_since_line_null() {
        let req: GetTailRequest =
            serde_json::from_value(json!({"source": "test", "since_line": null})).unwrap();
        assert_eq!(req.since_line, None);
    }

    #[test]
    fn get_context_accepts_string_params() {
        let req: GetContextRequest = serde_json::from_value(
            json!({"source": "test", "line_number": "100", "before": "3", "after": "7"}),
        )
        .unwrap();
        assert_eq!(req.line_number, 100);
        assert_eq!(req.before, 3);
        assert_eq!(req.after, 7);
    }

    #[test]
    fn search_accepts_string_params() {
        let req: SearchRequest = serde_json::from_value(
            json!({"source": "test", "pattern": "err", "max_results": "50", "context_lines": "3"}),
        )
        .unwrap();
        assert_eq!(req.max_results, 50);
        assert_eq!(req.context_lines, 3);
    }

    #[test]
    fn rejects_negative_number() {
        let err = serde_json::from_value::<GetLinesRequest>(json!({"source": "test", "start": -1}))
            .unwrap_err();
        assert!(
            err.to_string().contains("non-negative"),
            "error should mention 'non-negative', got: {}",
            err
        );
    }

    #[test]
    fn rejects_non_numeric_string() {
        let err =
            serde_json::from_value::<GetLinesRequest>(json!({"source": "test", "start": "abc"}))
                .unwrap_err();
        assert!(
            err.to_string().contains("invalid value \"abc\""),
            "error should include the bad value, got: {}",
            err
        );
    }

    #[test]
    fn rejects_float_value() {
        let err =
            serde_json::from_value::<GetLinesRequest>(json!({"source": "test", "start": 1.5}))
                .unwrap_err();
        assert!(
            err.to_string().contains("floating-point"),
            "error should mention floating-point, got: {}",
            err
        );
    }
}
