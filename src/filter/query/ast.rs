//! AST types for structured query filtering.
//!
//! Contains the core type definitions shared by the text query parser,
//! the JSON/MCP query interface, and the filter implementation.

use schemars::JsonSchema;
use serde::Deserialize;

/// Parser for extracting fields from log lines.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Parser {
    /// Plain text, no field extraction. Field filters will not match.
    #[default]
    Raw,
    /// Parse as JSON object. Fields are accessed by keys (supports dot notation).
    Json,
    /// Parse as logfmt (key=value pairs).
    Logfmt,
}

/// Comparison operators for field filtering.
#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Operator {
    /// Equality (==)
    Eq,
    /// Inequality (!=)
    Ne,
    /// Regex match (=~)
    Regex,
    /// Negated regex match (!~)
    NotRegex,
    /// Substring contains
    Contains,
    /// Greater than (>)
    Gt,
    /// Less than (<)
    Lt,
    /// Greater than or equal (>=)
    Gte,
    /// Less than or equal (<=)
    Lte,
}

/// A single field filter condition.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct FieldFilter {
    /// Field name to extract from the parsed log line.
    pub field: String,
    /// Comparison operator.
    pub op: Operator,
    /// Value to compare against.
    pub value: String,
}

/// Exclusion pattern (negative filter).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ExcludePattern {
    /// Field name to check for exclusion.
    pub field: String,
    /// Pattern to match (substring match).
    pub pattern: String,
}

/// Aggregation type for grouped results.
#[derive(Debug, Clone, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AggregationType {
    /// Count lines grouped by field values.
    CountBy,
}

/// Aggregation clause for grouped query results.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct Aggregation {
    /// Type of aggregation to perform (used by serde for deserialization dispatch).
    #[serde(rename = "type")]
    #[allow(dead_code)]
    pub agg_type: AggregationType,
    /// Fields to group by.
    pub fields: Vec<String>,
    /// Optional limit on number of groups returned (top N).
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Complete query definition for structured log filtering.
#[derive(Debug, Clone, Default, Deserialize, JsonSchema)]
pub struct FilterQuery {
    /// Parser to use for extracting fields from log lines.
    #[serde(default)]
    pub parser: Parser,

    /// Field filters to apply (all must match - AND logic).
    #[serde(default)]
    pub filters: Vec<FieldFilter>,

    /// Virtual `@ts` filters on the index ingestion timestamp.
    /// Separated from content filters — evaluated as a bitmap at the scan level.
    #[serde(default)]
    pub ts_filters: Vec<FieldFilter>,

    /// Exclusion patterns (any match excludes the line).
    #[serde(default)]
    pub exclude: Vec<ExcludePattern>,

    /// Optional aggregation clause for grouped results.
    #[serde(default)]
    pub aggregate: Option<Aggregation>,
}

impl FilterQuery {
    /// Whether this query has `@ts` (index timestamp) filters.
    pub fn has_ts_filters(&self) -> bool {
        !self.ts_filters.is_empty()
    }

    /// Move any `@ts` entries from `filters` into `ts_filters`.
    ///
    /// Called after JSON deserialization (MCP path) where all filters arrive
    /// in the single `filters` array.
    pub fn partition_ts_filters(&mut self) {
        let (ts, content): (Vec<_>, Vec<_>) = std::mem::take(&mut self.filters)
            .into_iter()
            .partition(|f| f.field == "@ts");
        self.filters = content;
        self.ts_filters.extend(ts);
    }

    /// Build a (mask, want) pair for index pre-filtering.
    ///
    /// Returns `None` if the query cannot benefit from index pre-filtering
    /// (e.g., Raw parser). When `Some((mask, want))` is returned, lines where
    /// `flags & mask != want` can be skipped without parsing content.
    ///
    /// The mask encodes:
    /// - Format flag (JSON or logfmt) from the parser type
    /// - Empty-line exclusion
    /// - Severity level from `level == "value"` filters (exact match only)
    pub fn index_mask(&self) -> Option<(u32, u32)> {
        use crate::index::flags::{
            FLAG_FORMAT_JSON, FLAG_FORMAT_LOGFMT, FLAG_IS_EMPTY, SEVERITY_MASK,
        };

        let mut mask = 0u32;
        let mut want = 0u32;

        match self.parser {
            Parser::Json => {
                mask |= FLAG_FORMAT_JSON;
                want |= FLAG_FORMAT_JSON;
            }
            Parser::Logfmt => {
                mask |= FLAG_FORMAT_LOGFMT;
                want |= FLAG_FORMAT_LOGFMT;
            }
            Parser::Raw => return None,
        }

        // Empty lines never match structured queries
        mask |= FLAG_IS_EMPTY;

        // Severity from level == "value" filter (exact match only)
        for filter in &self.filters {
            if filter.field == "level" && filter.op == Operator::Eq {
                if let Some(sev) = severity_from_level_value(&filter.value) {
                    mask |= SEVERITY_MASK;
                    want |= sev.to_bits();
                    break;
                }
            }
        }

        Some((mask, want))
    }
}

/// Map a level filter value to a Severity enum for index pre-filtering.
fn severity_from_level_value(value: &str) -> Option<crate::index::flags::Severity> {
    use crate::index::flags::Severity;

    match value.to_ascii_lowercase().as_str() {
        "trace" => Some(Severity::Trace),
        "debug" => Some(Severity::Debug),
        "info" => Some(Severity::Info),
        "warn" | "warning" => Some(Severity::Warn),
        "error" | "err" => Some(Severity::Error),
        "fatal" | "critical" | "crit" | "emerg" | "emergency" | "panic" => Some(Severity::Fatal),
        _ => None,
    }
}
