//! Structured query language for log filtering.
//!
//! Provides both a JSON-based query interface for MCP and a text-based query language
//! for the UI. Supports field-based filtering on structured logs (JSON, logfmt).
//!
//! ## JSON Query (MCP interface)
//! ```json
//! {
//!   "parser": "json",
//!   "filters": [{"field": "level", "op": "eq", "value": "error"}]
//! }
//! ```
//!
//! ## Text Query (UI)
//! ```text
//! json | level == "error"
//! json | level == "error" | service =~ "api.*"
//! json | status >= 400
//! json | user.id == "123"
//! logfmt | level == error
//! ```

use crate::filter::Filter;
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::HashMap;

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

// ============================================================================
// Text Query Parser
// ============================================================================

/// Parse error with position info for helpful error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryParseError {
    pub message: String,
    pub position: usize,
}

impl std::fmt::Display for QueryParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at position {}", self.message, self.position)
    }
}

/// Parse text query into FilterQuery AST.
///
/// # Examples
/// ```ignore
/// parse_query("json | level == \"error\"")
/// parse_query("json | status >= 400 | service =~ \"api.*\"")
/// parse_query("logfmt | level == error")
/// ```
pub fn parse_query(input: &str) -> Result<FilterQuery, QueryParseError> {
    QueryTextParser::new(input).parse()
}

/// Text parser for LogQL-like query syntax.
struct QueryTextParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> QueryTextParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn parse(&mut self) -> Result<FilterQuery, QueryParseError> {
        self.skip_whitespace();

        // Parse parser type (json or logfmt)
        let parser = self.parse_parser()?;

        // Parse filter expressions separated by |
        let mut filters = Vec::new();
        let mut aggregate = None;

        self.skip_whitespace();
        while self.pos < self.input.len() {
            // Expect | separator
            if !self.consume_char('|') {
                if self.pos < self.input.len() {
                    return Err(QueryParseError {
                        message: format!(
                            "Expected '|', found '{}'",
                            self.peek_char().unwrap_or('?')
                        ),
                        position: self.pos,
                    });
                }
                break;
            }

            self.skip_whitespace();

            // Check for aggregation clause before filter
            if self.peek_word("count") {
                let fields = self.parse_count_by()?;
                self.skip_whitespace();

                // Optionally consume `| top N`
                let limit = if self.peek_char() == Some('|') {
                    let saved_pos = self.pos;
                    self.consume_char('|');
                    self.skip_whitespace();
                    match self.parse_top_clause() {
                        Some(n) => Some(n),
                        None => {
                            // Not a top clause, rewind
                            self.pos = saved_pos;
                            None
                        }
                    }
                } else {
                    None
                };

                aggregate = Some(Aggregation {
                    agg_type: AggregationType::CountBy,
                    fields,
                    limit,
                });
                break;
            }

            // Parse filter expression
            if self.pos < self.input.len() {
                let filter = self.parse_filter()?;
                filters.push(filter);
            }

            self.skip_whitespace();
        }

        Ok(FilterQuery {
            parser,
            filters,
            exclude: vec![],
            aggregate,
        })
    }

    fn parse_parser(&mut self) -> Result<Parser, QueryParseError> {
        if self.consume_word("json") {
            Ok(Parser::Json)
        } else if self.consume_word("logfmt") {
            Ok(Parser::Logfmt)
        } else {
            Err(QueryParseError {
                message: "Expected 'json' or 'logfmt'".to_string(),
                position: self.pos,
            })
        }
    }

    fn parse_filter(&mut self) -> Result<FieldFilter, QueryParseError> {
        // Parse field name (may contain dots for nested access)
        let field = self.parse_field()?;

        self.skip_whitespace();

        // Parse operator
        let op = self.parse_operator()?;

        self.skip_whitespace();

        // Parse value
        let value = self.parse_value()?;

        Ok(FieldFilter { field, op, value })
    }

    fn parse_field(&mut self) -> Result<String, QueryParseError> {
        let start = self.pos;

        // Field can contain alphanumeric, underscore, and dots (for nested access)
        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }

        if self.pos == start {
            return Err(QueryParseError {
                message: "Expected field name".to_string(),
                position: self.pos,
            });
        }

        Ok(self.input[start..self.pos].to_string())
    }

    fn parse_operator(&mut self) -> Result<Operator, QueryParseError> {
        // Try two-character operators first
        if self.consume_str("==") {
            Ok(Operator::Eq)
        } else if self.consume_str("!=") {
            Ok(Operator::Ne)
        } else if self.consume_str("=~") {
            Ok(Operator::Regex)
        } else if self.consume_str("!~") {
            Ok(Operator::NotRegex)
        } else if self.consume_str(">=") {
            Ok(Operator::Gte)
        } else if self.consume_str("<=") {
            Ok(Operator::Lte)
        } else if self.consume_str(">") {
            Ok(Operator::Gt)
        } else if self.consume_str("<") {
            Ok(Operator::Lt)
        } else {
            Err(QueryParseError {
                message: "Expected operator (==, !=, =~, !~, >, <, >=, <=)".to_string(),
                position: self.pos,
            })
        }
    }

    fn parse_value(&mut self) -> Result<String, QueryParseError> {
        self.skip_whitespace();

        match self.peek_char() {
            Some('"') => self.parse_quoted_string('"'),
            Some('\'') => self.parse_quoted_string('\''),
            _ => self.parse_unquoted_word(),
        }
    }

    fn parse_quoted_string(&mut self, quote_char: char) -> Result<String, QueryParseError> {
        let start_pos = self.pos;

        // Consume opening quote
        if !self.consume_char(quote_char) {
            return Err(QueryParseError {
                message: format!("Expected opening {}", quote_char),
                position: self.pos,
            });
        }

        let mut result = String::new();

        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            self.pos += ch.len_utf8();

            if ch == quote_char {
                return Ok(result);
            } else if ch == '\\' {
                // Handle escape sequences
                if let Some(escaped) = self.input[self.pos..].chars().next() {
                    self.pos += escaped.len_utf8();
                    match escaped {
                        'n' => result.push('\n'),
                        't' => result.push('\t'),
                        'r' => result.push('\r'),
                        '"' => result.push('"'),
                        '\'' => result.push('\''),
                        '\\' => result.push('\\'),
                        _ => {
                            result.push('\\');
                            result.push(escaped);
                        }
                    }
                }
            } else {
                result.push(ch);
            }
        }

        Err(QueryParseError {
            message: "Unterminated string".to_string(),
            position: start_pos,
        })
    }

    fn parse_unquoted_word(&mut self) -> Result<String, QueryParseError> {
        let start = self.pos;

        // Unquoted word: until whitespace or | or end
        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            if ch.is_whitespace() || ch == '|' {
                break;
            }
            self.pos += ch.len_utf8();
        }

        if self.pos == start {
            return Err(QueryParseError {
                message: "Expected value".to_string(),
                position: self.pos,
            });
        }

        Ok(self.input[start..self.pos].to_string())
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn consume_str(&mut self, expected: &str) -> bool {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            true
        } else {
            false
        }
    }

    fn consume_word(&mut self, word: &str) -> bool {
        if self.input[self.pos..].starts_with(word) {
            // Make sure it's a complete word (followed by whitespace, |, or end)
            let after_pos = self.pos + word.len();
            if after_pos >= self.input.len() {
                self.pos = after_pos;
                return true;
            }
            let next_char = self.input[after_pos..].chars().next().unwrap();
            if next_char.is_whitespace() || next_char == '|' {
                self.pos = after_pos;
                return true;
            }
        }
        false
    }

    /// Peek whether the next word matches without consuming.
    fn peek_word(&self, word: &str) -> bool {
        let rest = &self.input[self.pos..];
        if rest.starts_with(word) {
            let after_pos = word.len();
            if after_pos >= rest.len() {
                return true;
            }
            let next_char = rest[after_pos..].chars().next().unwrap();
            return next_char.is_whitespace() || next_char == '|' || next_char == '(';
        }
        false
    }

    /// Parse `count by (field1, field2, ...)`.
    fn parse_count_by(&mut self) -> Result<Vec<String>, QueryParseError> {
        if !self.consume_word("count") {
            return Err(QueryParseError {
                message: "Expected 'count'".to_string(),
                position: self.pos,
            });
        }
        self.skip_whitespace();

        if !self.consume_word("by") {
            return Err(QueryParseError {
                message: "Expected 'by' after 'count'".to_string(),
                position: self.pos,
            });
        }
        self.skip_whitespace();

        self.parse_field_list()
    }

    /// Parse a parenthesized, comma-separated list of field names.
    fn parse_field_list(&mut self) -> Result<Vec<String>, QueryParseError> {
        if !self.consume_char('(') {
            return Err(QueryParseError {
                message: "Expected '(' after 'by'".to_string(),
                position: self.pos,
            });
        }

        let mut fields = Vec::new();
        loop {
            self.skip_whitespace();
            let field = self.parse_field()?;
            fields.push(field);
            self.skip_whitespace();

            if self.consume_char(')') {
                break;
            }
            if !self.consume_char(',') {
                return Err(QueryParseError {
                    message: "Expected ',' or ')' in field list".to_string(),
                    position: self.pos,
                });
            }
        }

        if fields.is_empty() {
            return Err(QueryParseError {
                message: "Expected at least one field in 'count by'".to_string(),
                position: self.pos,
            });
        }

        Ok(fields)
    }

    /// Try to parse `top N`, returning Some(N) on success.
    fn parse_top_clause(&mut self) -> Option<usize> {
        if !self.peek_word("top") {
            return None;
        }
        self.consume_word("top");
        self.skip_whitespace();

        let start = self.pos;
        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            if ch.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }

        if self.pos == start {
            return None;
        }

        self.input[start..self.pos].parse::<usize>().ok()
    }
}

// ============================================================================
// Logfmt Parser
// ============================================================================

/// Parse a logfmt line into key-value pairs.
///
/// Logfmt format: `key=value key2="quoted value" key3=unquoted`
pub fn parse_logfmt(line: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut chars = line.char_indices().peekable();

    while let Some((_, ch)) = chars.peek().copied() {
        // Skip whitespace
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        // Parse key
        let key_start = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
        while let Some(&(_, ch)) = chars.peek() {
            if ch == '=' || ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        let key_end = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
        let key = &line[key_start..key_end];

        if key.is_empty() {
            break;
        }

        // Expect =
        if chars.peek().map(|(_, ch)| *ch) != Some('=') {
            // No value, skip this key
            continue;
        }
        chars.next(); // consume '='

        // Parse value
        let value = if chars.peek().map(|(_, ch)| *ch) == Some('"') {
            // Quoted value
            chars.next(); // consume opening quote
            let value_start = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
            let mut value_end = value_start;
            let mut escaped = false;

            for (i, ch) in chars.by_ref() {
                if escaped {
                    escaped = false;
                    value_end = i + ch.len_utf8();
                } else if ch == '\\' {
                    escaped = true;
                    value_end = i + ch.len_utf8();
                } else if ch == '"' {
                    break;
                } else {
                    value_end = i + ch.len_utf8();
                }
            }

            // Handle escape sequences in the value
            let raw_value = &line[value_start..value_end];
            raw_value
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
                .replace("\\n", "\n")
                .replace("\\t", "\t")
        } else {
            // Unquoted value
            let value_start = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
            while let Some(&(_, ch)) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                chars.next();
            }
            let value_end = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
            line[value_start..value_end].to_string()
        };

        result.insert(key.to_string(), value);
    }

    result
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

    /// Exclusion patterns (any match excludes the line).
    #[serde(default)]
    pub exclude: Vec<ExcludePattern>,

    /// Optional aggregation clause for grouped results.
    #[serde(default)]
    pub aggregate: Option<Aggregation>,
}

impl FilterQuery {
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
        use lazytail::index::flags::{
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
fn severity_from_level_value(value: &str) -> Option<lazytail::index::flags::Severity> {
    use lazytail::index::flags::Severity;

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

/// Filter implementation for structured queries.
#[derive(Debug)]
pub struct QueryFilter {
    query: FilterQuery,
    /// Pre-compiled regexes for Regex operator filters.
    filter_regexes: Vec<Option<Regex>>,
    /// Pre-compiled regexes for NotRegex operator filters.
    not_regex_patterns: Vec<Option<Regex>>,
}

/// Extract a field value from a JSON object.
///
/// Supports dot notation for nested field access: "user.id" -> json["user"]["id"]
pub fn extract_json_field(json: &serde_json::Value, field: &str) -> Option<String> {
    let mut current = json;

    for part in field.split('.') {
        if current.is_array() {
            if let Ok(index) = part.parse::<usize>() {
                current = current.get(index)?;
                continue;
            }
        }
        current = current.get(part)?;
    }

    Some(match current {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        _ => current.to_string(),
    })
}

impl QueryFilter {
    /// Create a new QueryFilter from a FilterQuery.
    ///
    /// Returns an error if any regex pattern is invalid.
    pub fn new(query: FilterQuery) -> Result<Self, String> {
        // Pre-compile regex patterns for filters
        let mut filter_regexes = Vec::with_capacity(query.filters.len());
        let mut not_regex_patterns = Vec::with_capacity(query.filters.len());

        for filter in &query.filters {
            match filter.op {
                Operator::Regex => {
                    let regex = Regex::new(&filter.value)
                        .map_err(|e| format!("Invalid regex '{}': {}", filter.value, e))?;
                    filter_regexes.push(Some(regex));
                    not_regex_patterns.push(None);
                }
                Operator::NotRegex => {
                    let regex = Regex::new(&filter.value)
                        .map_err(|e| format!("Invalid regex '{}': {}", filter.value, e))?;
                    filter_regexes.push(None);
                    not_regex_patterns.push(Some(regex));
                }
                _ => {
                    filter_regexes.push(None);
                    not_regex_patterns.push(None);
                }
            }
        }

        Ok(Self {
            query,
            filter_regexes,
            not_regex_patterns,
        })
    }

    /// Check if a field value matches a filter condition.
    fn matches_filter(
        &self,
        field_value: &str,
        filter: &FieldFilter,
        filter_regex: Option<&Regex>,
        not_regex: Option<&Regex>,
    ) -> bool {
        match filter.op {
            Operator::Eq => field_value == filter.value,
            Operator::Ne => field_value != filter.value,
            Operator::Contains => field_value.contains(&filter.value),
            Operator::Regex => filter_regex.is_some_and(|r| r.is_match(field_value)),
            Operator::NotRegex => not_regex.is_none_or(|r| !r.is_match(field_value)),
            Operator::Gt => {
                Self::compare_values(field_value, &filter.value)
                    == Some(std::cmp::Ordering::Greater)
            }
            Operator::Lt => {
                Self::compare_values(field_value, &filter.value) == Some(std::cmp::Ordering::Less)
            }
            Operator::Gte => Self::compare_values(field_value, &filter.value)
                .is_some_and(|ord| ord != std::cmp::Ordering::Less),
            Operator::Lte => Self::compare_values(field_value, &filter.value)
                .is_some_and(|ord| ord != std::cmp::Ordering::Greater),
        }
    }

    /// Compare two values, trying numeric comparison first, then string comparison.
    fn compare_values(a: &str, b: &str) -> Option<std::cmp::Ordering> {
        // Try numeric comparison first
        if let (Ok(a_num), Ok(b_num)) = (a.parse::<f64>(), b.parse::<f64>()) {
            return a_num.partial_cmp(&b_num);
        }
        // Fall back to string comparison
        Some(a.cmp(b))
    }

    /// Check if a line matches all exclusion patterns.
    fn matches_exclude(&self, json: &serde_json::Value) -> bool {
        for exclude in &self.query.exclude {
            if let Some(field_value) = extract_json_field(json, &exclude.field) {
                if field_value.contains(&exclude.pattern) {
                    return true;
                }
            }
        }
        false
    }
}

impl Filter for QueryFilter {
    fn matches(&self, line: &str) -> bool {
        match self.query.parser {
            Parser::Raw => {
                // Raw parser: no field extraction, filters don't apply
                // Only return true if there are no filters and no exclusions
                self.query.filters.is_empty() && self.query.exclude.is_empty()
            }
            Parser::Json => {
                // Parse line as JSON
                let json: serde_json::Value = match serde_json::from_str(line) {
                    Ok(v) => v,
                    Err(_) => return false, // Non-JSON lines don't match
                };

                // Check exclusion patterns first
                if self.matches_exclude(&json) {
                    return false;
                }

                // All filters must match (AND logic)
                for (i, filter) in self.query.filters.iter().enumerate() {
                    let field_value = match extract_json_field(&json, &filter.field) {
                        Some(v) => v,
                        None => return false, // Missing field = no match
                    };

                    let filter_regex = self.filter_regexes.get(i).and_then(|r| r.as_ref());
                    let not_regex = self.not_regex_patterns.get(i).and_then(|r| r.as_ref());

                    if !self.matches_filter(&field_value, filter, filter_regex, not_regex) {
                        return false;
                    }
                }

                true
            }
            Parser::Logfmt => {
                // Parse line as logfmt
                let fields = parse_logfmt(line);

                // All filters must match (AND logic)
                for (i, filter) in self.query.filters.iter().enumerate() {
                    // For logfmt, nested fields use the full field name as key
                    // (logfmt doesn't have native nesting)
                    let field_value = match fields.get(&filter.field) {
                        Some(v) => v.clone(),
                        None => return false, // Missing field = no match
                    };

                    let filter_regex = self.filter_regexes.get(i).and_then(|r| r.as_ref());
                    let not_regex = self.not_regex_patterns.get(i).and_then(|r| r.as_ref());

                    if !self.matches_filter(&field_value, filter, filter_regex, not_regex) {
                        return false;
                    }
                }

                true
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parser_deserialize() {
        let raw: Parser = serde_json::from_str(r#""raw""#).unwrap();
        assert_eq!(raw, Parser::Raw);

        let json: Parser = serde_json::from_str(r#""json""#).unwrap();
        assert_eq!(json, Parser::Json);
    }

    #[test]
    fn test_operator_deserialize() {
        assert_eq!(
            serde_json::from_str::<Operator>(r#""eq""#).unwrap(),
            Operator::Eq
        );
        assert_eq!(
            serde_json::from_str::<Operator>(r#""ne""#).unwrap(),
            Operator::Ne
        );
        assert_eq!(
            serde_json::from_str::<Operator>(r#""regex""#).unwrap(),
            Operator::Regex
        );
        assert_eq!(
            serde_json::from_str::<Operator>(r#""not_regex""#).unwrap(),
            Operator::NotRegex
        );
        assert_eq!(
            serde_json::from_str::<Operator>(r#""contains""#).unwrap(),
            Operator::Contains
        );
        assert_eq!(
            serde_json::from_str::<Operator>(r#""gt""#).unwrap(),
            Operator::Gt
        );
        assert_eq!(
            serde_json::from_str::<Operator>(r#""lt""#).unwrap(),
            Operator::Lt
        );
        assert_eq!(
            serde_json::from_str::<Operator>(r#""gte""#).unwrap(),
            Operator::Gte
        );
        assert_eq!(
            serde_json::from_str::<Operator>(r#""lte""#).unwrap(),
            Operator::Lte
        );
    }

    #[test]
    fn test_filter_query_deserialize() {
        let json = r#"{
            "parser": "json",
            "filters": [
                {"field": "level", "op": "eq", "value": "error"}
            ]
        }"#;

        let query: FilterQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.parser, Parser::Json);
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, "level");
        assert_eq!(query.filters[0].op, Operator::Eq);
        assert_eq!(query.filters[0].value, "error");
    }

    #[test]
    fn test_filter_query_with_exclude() {
        let json = r#"{
            "parser": "json",
            "filters": [
                {"field": "level", "op": "eq", "value": "error"}
            ],
            "exclude": [
                {"field": "msg", "pattern": "ignore_this"}
            ]
        }"#;

        let query: FilterQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.exclude.len(), 1);
        assert_eq!(query.exclude[0].field, "msg");
        assert_eq!(query.exclude[0].pattern, "ignore_this");
    }

    #[test]
    fn test_filter_query_defaults() {
        let json = r#"{}"#;
        let query: FilterQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.parser, Parser::Raw);
        assert!(query.filters.is_empty());
        assert!(query.exclude.is_empty());
    }

    #[test]
    fn test_query_filter_eq() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "error".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"level": "error", "msg": "something"}"#));
        assert!(!filter.matches(r#"{"level": "info", "msg": "something"}"#));
        assert!(!filter.matches(r#"{"msg": "no level field"}"#));
    }

    #[test]
    fn test_query_filter_ne() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Ne,
                value: "debug".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"level": "error"}"#));
        assert!(filter.matches(r#"{"level": "info"}"#));
        assert!(!filter.matches(r#"{"level": "debug"}"#));
    }

    #[test]
    fn test_query_filter_contains() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "msg".to_string(),
                op: Operator::Contains,
                value: "fail".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"msg": "connection failed"}"#));
        assert!(filter.matches(r#"{"msg": "failure detected"}"#));
        assert!(!filter.matches(r#"{"msg": "success"}"#));
    }

    #[test]
    fn test_query_filter_regex() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "service".to_string(),
                op: Operator::Regex,
                value: "^api-.*".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"service": "api-users"}"#));
        assert!(filter.matches(r#"{"service": "api-orders"}"#));
        assert!(!filter.matches(r#"{"service": "web-frontend"}"#));
    }

    #[test]
    fn test_query_filter_not_regex() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "service".to_string(),
                op: Operator::NotRegex,
                value: "^test-.*".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"service": "api-users"}"#));
        assert!(!filter.matches(r#"{"service": "test-service"}"#));
    }

    #[test]
    fn test_query_filter_numeric_comparison() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "status".to_string(),
                op: Operator::Gte,
                value: "400".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"status": 400}"#));
        assert!(filter.matches(r#"{"status": 500}"#));
        assert!(!filter.matches(r#"{"status": 200}"#));
    }

    #[test]
    fn test_query_filter_lt() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "count".to_string(),
                op: Operator::Lt,
                value: "10".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"count": 5}"#));
        assert!(filter.matches(r#"{"count": 0}"#));
        assert!(!filter.matches(r#"{"count": 10}"#));
        assert!(!filter.matches(r#"{"count": 15}"#));
    }

    #[test]
    fn test_query_filter_gt() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "latency".to_string(),
                op: Operator::Gt,
                value: "1000".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"latency": 1500}"#));
        assert!(!filter.matches(r#"{"latency": 1000}"#));
        assert!(!filter.matches(r#"{"latency": 500}"#));
    }

    #[test]
    fn test_query_filter_lte() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "priority".to_string(),
                op: Operator::Lte,
                value: "5".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"priority": 5}"#));
        assert!(filter.matches(r#"{"priority": 3}"#));
        assert!(!filter.matches(r#"{"priority": 6}"#));
    }

    #[test]
    fn test_query_filter_multiple_conditions() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![
                FieldFilter {
                    field: "level".to_string(),
                    op: Operator::Eq,
                    value: "error".to_string(),
                },
                FieldFilter {
                    field: "service".to_string(),
                    op: Operator::Regex,
                    value: "api|worker".to_string(),
                },
            ],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"level": "error", "service": "api"}"#));
        assert!(filter.matches(r#"{"level": "error", "service": "worker"}"#));
        assert!(!filter.matches(r#"{"level": "info", "service": "api"}"#));
        assert!(!filter.matches(r#"{"level": "error", "service": "web"}"#));
    }

    #[test]
    fn test_query_filter_exclude() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "error".to_string(),
            }],
            exclude: vec![ExcludePattern {
                field: "msg".to_string(),
                pattern: "ignore".to_string(),
            }],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"level": "error", "msg": "real error"}"#));
        assert!(!filter.matches(r#"{"level": "error", "msg": "please ignore this"}"#));
    }

    #[test]
    fn test_query_filter_raw_parser() {
        let query = FilterQuery {
            parser: Parser::Raw,
            filters: vec![],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        // Raw parser with no filters matches everything
        assert!(filter.matches("any plain text line"));
        assert!(filter.matches(r#"{"even": "json"}"#));
    }

    #[test]
    fn test_query_filter_raw_parser_with_filters() {
        let query = FilterQuery {
            parser: Parser::Raw,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "error".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        // Raw parser with filters matches nothing (can't extract fields)
        assert!(!filter.matches("any plain text line"));
        assert!(!filter.matches(r#"{"level": "error"}"#));
    }

    #[test]
    fn test_query_filter_invalid_json() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "error".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        // Invalid JSON doesn't match
        assert!(!filter.matches("not json at all"));
        assert!(!filter.matches("{invalid json}"));
    }

    #[test]
    fn test_query_filter_invalid_regex() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "msg".to_string(),
                op: Operator::Regex,
                value: "[invalid".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let result = QueryFilter::new(query);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Invalid regex"));
    }

    #[test]
    fn test_query_filter_boolean_field() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "active".to_string(),
                op: Operator::Eq,
                value: "true".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"active": true}"#));
        assert!(!filter.matches(r#"{"active": false}"#));
    }

    #[test]
    fn test_query_filter_null_field() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "value".to_string(),
                op: Operator::Eq,
                value: "null".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"value": null}"#));
        assert!(!filter.matches(r#"{"value": "something"}"#));
    }

    #[test]
    fn test_query_filter_string_comparison() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "name".to_string(),
                op: Operator::Gt,
                value: "alice".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"name": "bob"}"#));
        assert!(!filter.matches(r#"{"name": "alice"}"#));
        assert!(!filter.matches(r#"{"name": "adam"}"#));
    }

    // ========================================================================
    // Text Parser Tests
    // ========================================================================

    #[test]
    fn test_parse_simple_json_query() {
        let query = parse_query("json | level == \"error\"").unwrap();
        assert_eq!(query.parser, Parser::Json);
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, "level");
        assert_eq!(query.filters[0].op, Operator::Eq);
        assert_eq!(query.filters[0].value, "error");
    }

    #[test]
    fn test_parse_json_only() {
        let query = parse_query("json").unwrap();
        assert_eq!(query.parser, Parser::Json);
        assert!(query.filters.is_empty());
    }

    #[test]
    fn test_parse_logfmt_query() {
        let query = parse_query("logfmt | level == error").unwrap();
        assert_eq!(query.parser, Parser::Logfmt);
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, "level");
        assert_eq!(query.filters[0].op, Operator::Eq);
        assert_eq!(query.filters[0].value, "error");
    }

    #[test]
    fn test_parse_multiple_filters() {
        let query = parse_query("json | level == \"error\" | service =~ \"api.*\"").unwrap();
        assert_eq!(query.filters.len(), 2);
        assert_eq!(query.filters[0].field, "level");
        assert_eq!(query.filters[0].op, Operator::Eq);
        assert_eq!(query.filters[0].value, "error");
        assert_eq!(query.filters[1].field, "service");
        assert_eq!(query.filters[1].op, Operator::Regex);
        assert_eq!(query.filters[1].value, "api.*");
    }

    #[test]
    fn test_parse_all_operators() {
        // ==
        let q = parse_query("json | f == \"v\"").unwrap();
        assert_eq!(q.filters[0].op, Operator::Eq);

        // !=
        let q = parse_query("json | f != \"v\"").unwrap();
        assert_eq!(q.filters[0].op, Operator::Ne);

        // =~
        let q = parse_query("json | f =~ \"v\"").unwrap();
        assert_eq!(q.filters[0].op, Operator::Regex);

        // !~
        let q = parse_query("json | f !~ \"v\"").unwrap();
        assert_eq!(q.filters[0].op, Operator::NotRegex);

        // >
        let q = parse_query("json | f > 10").unwrap();
        assert_eq!(q.filters[0].op, Operator::Gt);

        // <
        let q = parse_query("json | f < 10").unwrap();
        assert_eq!(q.filters[0].op, Operator::Lt);

        // >=
        let q = parse_query("json | f >= 10").unwrap();
        assert_eq!(q.filters[0].op, Operator::Gte);

        // <=
        let q = parse_query("json | f <= 10").unwrap();
        assert_eq!(q.filters[0].op, Operator::Lte);
    }

    #[test]
    fn test_parse_nested_field() {
        let query = parse_query("json | user.id == \"123\"").unwrap();
        assert_eq!(query.filters[0].field, "user.id");
        assert_eq!(query.filters[0].value, "123");
    }

    #[test]
    fn test_parse_deep_nested_field() {
        let query =
            parse_query("json | request.headers.content_type == \"application/json\"").unwrap();
        assert_eq!(query.filters[0].field, "request.headers.content_type");
    }

    #[test]
    fn test_parse_quoted_string_with_spaces() {
        let query = parse_query("json | msg == \"hello world\"").unwrap();
        assert_eq!(query.filters[0].value, "hello world");
    }

    #[test]
    fn test_parse_quoted_string_with_escapes() {
        let query = parse_query(r#"json | msg == "hello \"world\"""#).unwrap();
        assert_eq!(query.filters[0].value, "hello \"world\"");
    }

    #[test]
    fn test_parse_single_quoted_string() {
        let query = parse_query("json | level == 'error'").unwrap();
        assert_eq!(query.filters[0].value, "error");
    }

    #[test]
    fn test_parse_single_quoted_string_with_spaces() {
        let query = parse_query("json | msg == 'hello world'").unwrap();
        assert_eq!(query.filters[0].value, "hello world");
    }

    #[test]
    fn test_parse_single_quoted_string_with_escapes() {
        let query = parse_query(r#"json | msg == 'it\'s working'"#).unwrap();
        assert_eq!(query.filters[0].value, "it's working");
    }

    #[test]
    fn test_parse_single_quotes_containing_double_quotes() {
        let query = parse_query(r#"json | msg == 'say "hello"'"#).unwrap();
        assert_eq!(query.filters[0].value, "say \"hello\"");
    }

    #[test]
    fn test_parse_unquoted_value() {
        let query = parse_query("json | status >= 400").unwrap();
        assert_eq!(query.filters[0].value, "400");
    }

    #[test]
    fn test_parse_error_invalid_parser() {
        let result = parse_query("xml | level == error");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("json"));
    }

    #[test]
    fn test_parse_error_missing_operator() {
        let result = parse_query("json | level error");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_error_unterminated_string() {
        let result = parse_query("json | level == \"error");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.message.contains("Unterminated"));
    }

    #[test]
    fn test_text_parser_filter_execution() {
        let query = parse_query("json | level == \"error\"").unwrap();
        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"level": "error", "msg": "something"}"#));
        assert!(!filter.matches(r#"{"level": "info", "msg": "something"}"#));
    }

    #[test]
    fn test_text_parser_numeric_filter() {
        let query = parse_query("json | status >= 400").unwrap();
        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"status": 400}"#));
        assert!(filter.matches(r#"{"status": 500}"#));
        assert!(!filter.matches(r#"{"status": 200}"#));
    }

    #[test]
    fn test_text_parser_regex_filter() {
        let query = parse_query("json | service =~ \"api.*\"").unwrap();
        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"service": "api-users"}"#));
        assert!(!filter.matches(r#"{"service": "web-frontend"}"#));
    }

    #[test]
    fn test_text_parser_invalid_regex() {
        let query = parse_query("json | service =~ \"[invalid\"").unwrap();
        let result = QueryFilter::new(query);
        assert!(result.is_err());
    }

    // ========================================================================
    // Nested Field Access Tests
    // ========================================================================

    #[test]
    fn test_nested_field_json() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "user.id".to_string(),
                op: Operator::Eq,
                value: "123".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"user": {"id": "123", "name": "Alice"}}"#));
        assert!(!filter.matches(r#"{"user": {"id": "456", "name": "Bob"}}"#));
        assert!(!filter.matches(r#"{"user": {"name": "Charlie"}}"#)); // missing id
    }

    #[test]
    fn test_deeply_nested_field() {
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "request.headers.authorization".to_string(),
                op: Operator::Eq,
                value: "Bearer token".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(r#"{"request": {"headers": {"authorization": "Bearer token"}}}"#));
        assert!(!filter.matches(r#"{"request": {"headers": {"authorization": "Basic creds"}}}"#));
    }

    // ========================================================================
    // Logfmt Tests
    // ========================================================================

    #[test]
    fn test_parse_logfmt_basic() {
        let fields = parse_logfmt("level=error msg=something");
        assert_eq!(fields.get("level"), Some(&"error".to_string()));
        assert_eq!(fields.get("msg"), Some(&"something".to_string()));
    }

    #[test]
    fn test_parse_logfmt_quoted() {
        let fields = parse_logfmt("level=error msg=\"hello world\"");
        assert_eq!(fields.get("level"), Some(&"error".to_string()));
        assert_eq!(fields.get("msg"), Some(&"hello world".to_string()));
    }

    #[test]
    fn test_logfmt_filter() {
        let query = FilterQuery {
            parser: Parser::Logfmt,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "error".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches("level=error msg=\"something failed\""));
        assert!(!filter.matches("level=info msg=\"all good\""));
    }

    #[test]
    fn test_logfmt_text_parser() {
        let query = parse_query("logfmt | level == error").unwrap();
        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches("level=error msg=\"something failed\""));
        assert!(!filter.matches("level=info msg=\"all good\""));
    }

    #[test]
    fn test_logfmt_numeric_comparison() {
        let query = parse_query("logfmt | status >= 400").unwrap();
        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches("status=500 msg=error"));
        assert!(filter.matches("status=400 msg=error"));
        assert!(!filter.matches("status=200 msg=ok"));
    }

    // ========================================================================
    // index_mask() Tests
    // ========================================================================

    #[test]
    fn test_index_mask_json_level_error() {
        use lazytail::index::flags::*;

        let query = parse_query("json | level == \"error\"").unwrap();
        let (mask, want) = query.index_mask().unwrap();

        assert_ne!(mask & FLAG_FORMAT_JSON, 0);
        assert_ne!(want & FLAG_FORMAT_JSON, 0);
        assert_ne!(mask & SEVERITY_MASK, 0);
        assert_eq!(want & SEVERITY_MASK, SEVERITY_ERROR);
        // Empty lines excluded
        assert_ne!(mask & FLAG_IS_EMPTY, 0);
        assert_eq!(want & FLAG_IS_EMPTY, 0);
    }

    #[test]
    fn test_index_mask_logfmt_level_warn() {
        use lazytail::index::flags::*;

        let query = parse_query("logfmt | level == warn").unwrap();
        let (mask, want) = query.index_mask().unwrap();

        assert_ne!(mask & FLAG_FORMAT_LOGFMT, 0);
        assert_ne!(want & FLAG_FORMAT_LOGFMT, 0);
        assert_eq!(want & SEVERITY_MASK, SEVERITY_WARN);
    }

    #[test]
    fn test_index_mask_json_no_level_filter() {
        use lazytail::index::flags::*;

        let query = parse_query("json | service == \"api\"").unwrap();
        let (mask, want) = query.index_mask().unwrap();

        // Has format flag
        assert_ne!(mask & FLAG_FORMAT_JSON, 0);
        assert_ne!(want & FLAG_FORMAT_JSON, 0);
        // No severity constraint
        assert_eq!(mask & SEVERITY_MASK, 0);
    }

    #[test]
    fn test_index_mask_json_only() {
        use lazytail::index::flags::*;

        let query = parse_query("json").unwrap();
        let (mask, want) = query.index_mask().unwrap();

        assert_ne!(mask & FLAG_FORMAT_JSON, 0);
        assert_ne!(want & FLAG_FORMAT_JSON, 0);
        assert_eq!(mask & SEVERITY_MASK, 0);
    }

    #[test]
    fn test_index_mask_raw_returns_none() {
        let query = FilterQuery {
            parser: Parser::Raw,
            filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        assert!(query.index_mask().is_none());
    }

    #[test]
    fn test_index_mask_ne_no_severity() {
        use lazytail::index::flags::*;

        // level != "error" cannot be expressed as a simple mask
        let query = parse_query("json | level != \"error\"").unwrap();
        let (mask, _want) = query.index_mask().unwrap();

        // Has format flag but no severity constraint
        assert_ne!(mask & FLAG_FORMAT_JSON, 0);
        assert_eq!(mask & SEVERITY_MASK, 0);
    }

    #[test]
    fn test_index_mask_severity_aliases() {
        use lazytail::index::flags::*;

        // "err" is an alias for error
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "err".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };
        let (_, want) = query.index_mask().unwrap();
        assert_eq!(want & SEVERITY_MASK, SEVERITY_ERROR);

        // "WARNING" maps to warn
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "WARNING".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };
        let (_, want) = query.index_mask().unwrap();
        assert_eq!(want & SEVERITY_MASK, SEVERITY_WARN);

        // "critical" maps to fatal
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "critical".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };
        let (_, want) = query.index_mask().unwrap();
        assert_eq!(want & SEVERITY_MASK, SEVERITY_FATAL);
    }

    #[test]
    fn test_index_mask_unknown_level_no_severity() {
        use lazytail::index::flags::*;

        // "notice" is not a known severity
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "notice".to_string(),
            }],
            exclude: vec![],
            aggregate: None,
        };
        let (mask, _want) = query.index_mask().unwrap();
        // No severity constraint since we can't map "notice"
        assert_eq!(mask & SEVERITY_MASK, 0);
    }

    // ========================================================================
    // Aggregation Parser Tests
    // ========================================================================

    #[test]
    fn test_parse_count_by_single_field() {
        let query = parse_query("json | level == \"error\" | count by (service)").unwrap();
        assert_eq!(query.parser, Parser::Json);
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, "level");
        let agg = query.aggregate.unwrap();
        assert_eq!(agg.agg_type, AggregationType::CountBy);
        assert_eq!(agg.fields, vec!["service"]);
        assert!(agg.limit.is_none());
    }

    #[test]
    fn test_parse_count_by_multiple_fields() {
        let query = parse_query("json | count by (service, level)").unwrap();
        assert_eq!(query.parser, Parser::Json);
        assert!(query.filters.is_empty());
        let agg = query.aggregate.unwrap();
        assert_eq!(agg.fields, vec!["service", "level"]);
    }

    #[test]
    fn test_parse_count_by_with_top() {
        let query = parse_query("json | count by (level) | top 5").unwrap();
        let agg = query.aggregate.unwrap();
        assert_eq!(agg.fields, vec!["level"]);
        assert_eq!(agg.limit, Some(5));
    }

    #[test]
    fn test_parse_count_by_no_filters() {
        let query = parse_query("json | count by (service)").unwrap();
        assert!(query.filters.is_empty());
        assert!(query.aggregate.is_some());
    }

    #[test]
    fn test_parse_count_by_logfmt() {
        let query = parse_query("logfmt | level == error | count by (service)").unwrap();
        assert_eq!(query.parser, Parser::Logfmt);
        assert_eq!(query.filters.len(), 1);
        let agg = query.aggregate.unwrap();
        assert_eq!(agg.fields, vec!["service"]);
    }

    #[test]
    fn test_has_aggregation() {
        let query = parse_query("json | count by (service)").unwrap();
        assert!(query.aggregate.is_some());

        let query = parse_query("json | level == \"error\"").unwrap();
        assert!(query.aggregate.is_none());
    }

    #[test]
    fn test_aggregation_json_deserialize() {
        let json = r#"{
            "parser": "json",
            "filters": [],
            "aggregate": {
                "type": "count_by",
                "fields": ["service"],
                "limit": 10
            }
        }"#;
        let query: FilterQuery = serde_json::from_str(json).unwrap();
        let agg = query.aggregate.unwrap();
        assert_eq!(agg.agg_type, AggregationType::CountBy);
        assert_eq!(agg.fields, vec!["service"]);
        assert_eq!(agg.limit, Some(10));
    }

    #[test]
    fn test_aggregation_json_deserialize_no_limit() {
        let json = r#"{
            "parser": "json",
            "aggregate": {
                "type": "count_by",
                "fields": ["service", "level"]
            }
        }"#;
        let query: FilterQuery = serde_json::from_str(json).unwrap();
        let agg = query.aggregate.unwrap();
        assert_eq!(agg.fields, vec!["service", "level"]);
        assert!(agg.limit.is_none());
    }

    #[test]
    fn test_aggregation_json_deserialize_absent() {
        let json = r#"{"parser": "json"}"#;
        let query: FilterQuery = serde_json::from_str(json).unwrap();
        assert!(query.aggregate.is_none());
    }

    // ========================================================================
    // Array Index Field Access Tests (R16)
    // ========================================================================

    #[test]
    fn test_extract_json_field_array_index() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"content":[{"type":"text","text":"hello"}]}"#).unwrap();
        assert_eq!(
            extract_json_field(&json, "content.0.text"),
            Some("hello".to_string())
        );
    }

    #[test]
    fn test_extract_json_field_array_index_nested() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"message":{"content":[{"type":"tool_use","name":"grep"}]}}"#)
                .unwrap();
        assert_eq!(
            extract_json_field(&json, "message.content.0.type"),
            Some("tool_use".to_string())
        );
    }

    #[test]
    fn test_extract_json_field_array_index_out_of_bounds() {
        let json: serde_json::Value =
            serde_json::from_str(r#"{"content":[{"type":"text","text":"hello"}]}"#).unwrap();
        assert_eq!(extract_json_field(&json, "content.5.text"), None);
    }

    #[test]
    fn test_extract_json_field_numeric_key_on_object() {
        let json: serde_json::Value = serde_json::from_str(r#"{"items":{"0":"first"}}"#).unwrap();
        assert_eq!(
            extract_json_field(&json, "items.0"),
            Some("first".to_string())
        );
    }
}
