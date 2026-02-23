//! Request and response types for MCP tools.

use crate::filter::query::FilterQuery;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

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
    #[serde(default)]
    pub start: usize,
    /// Number of lines to fetch (default 100, max 1000)
    #[serde(default = "default_count")]
    pub count: usize,
    /// Return raw content with ANSI escape codes intact (default: false, strips ANSI)
    #[serde(default)]
    pub raw: bool,
    /// Output format: "text" (default, plain text) or "json"
    #[serde(default)]
    pub output: OutputFormat,
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
    #[serde(default = "default_max_results")]
    pub max_results: usize,
    /// Number of context lines before and after each match (default 0, max 50)
    #[serde(default)]
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
    #[serde(default = "default_count")]
    pub count: usize,
    /// Only return lines after this line number (0-indexed, exclusive).
    /// Enables efficient incremental polling — pass the last line_number
    /// you received to get only new lines. When set, returns up to `count`
    /// lines starting from `since_line + 1`.
    #[serde(default)]
    pub since_line: Option<usize>,
    /// Return raw content with ANSI escape codes intact (default: false, strips ANSI)
    #[serde(default)]
    pub raw: bool,
    /// Output format: "text" (default, plain text) or "json"
    #[serde(default)]
    pub output: OutputFormat,
}

/// Request to get context around a specific line in a lazytail source.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetContextRequest {
    /// Source name (from list_sources)
    pub source: String,
    /// The target line number (0-indexed)
    pub line_number: usize,
    /// Number of lines before the target (default 5, max 50)
    #[serde(default = "default_context")]
    pub before: usize,
    /// Number of lines after the target (default 5, max 50)
    #[serde(default = "default_context")]
    pub after: usize,
    /// Return raw content with ANSI escape codes intact (default: false, strips ANSI)
    #[serde(default)]
    pub raw: bool,
    /// Output format: "text" (default, plain text) or "json"
    #[serde(default)]
    pub output: OutputFormat,
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
