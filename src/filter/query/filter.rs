//! QueryFilter implementation for structured log filtering.
//!
//! Implements the `Filter` trait for `FilterQuery` AST nodes, supporting
//! JSON and logfmt log line parsing with field-based matching.

use crate::filter::Filter;
use crate::parsing::{extract_json_field, parse_logfmt};
use regex::Regex;
use std::collections::HashMap;

use super::ast::*;
use super::time::{self, EpochMillis};

/// Filter implementation for structured queries.
#[derive(Debug)]
pub struct QueryFilter {
    query: FilterQuery,
    /// Pre-compiled regexes for Regex operator filters.
    filter_regexes: Vec<Option<Regex>>,
    /// Pre-compiled regexes for NotRegex operator filters.
    not_regex_patterns: Vec<Option<Regex>>,
    /// Resolved epoch millis for filters with relative time values.
    resolved_times: Vec<Option<EpochMillis>>,
}

impl QueryFilter {
    /// Create a new QueryFilter from a FilterQuery.
    ///
    /// Returns an error if any regex pattern is invalid.
    pub fn new(query: FilterQuery) -> Result<Self, String> {
        // Pre-compile regex patterns and resolve time values for filters
        let mut filter_regexes = Vec::with_capacity(query.filters.len());
        let mut not_regex_patterns = Vec::with_capacity(query.filters.len());
        let mut resolved_times = Vec::with_capacity(query.filters.len());

        for filter in &query.filters {
            match filter.op {
                Operator::Regex => {
                    let regex = Regex::new(&filter.value)
                        .map_err(|e| format!("Invalid regex '{}': {}", filter.value, e))?;
                    filter_regexes.push(Some(regex));
                    not_regex_patterns.push(None);
                    resolved_times.push(None);
                }
                Operator::NotRegex => {
                    let regex = Regex::new(&filter.value)
                        .map_err(|e| format!("Invalid regex '{}': {}", filter.value, e))?;
                    filter_regexes.push(None);
                    not_regex_patterns.push(Some(regex));
                    resolved_times.push(None);
                }
                _ => {
                    filter_regexes.push(None);
                    not_regex_patterns.push(None);
                    // Resolve time expressions: relative (e.g., "now-5m") or absolute timestamps
                    resolved_times.push(
                        time::resolve_relative_time(&filter.value)
                            .or_else(|| time::parse_timestamp(&filter.value)),
                    );
                }
            }
        }

        Ok(Self {
            query,
            filter_regexes,
            not_regex_patterns,
            resolved_times,
        })
    }

    /// Check if a field value matches a filter condition.
    fn matches_filter(
        &self,
        field_value: &str,
        filter: &FieldFilter,
        filter_regex: Option<&Regex>,
        not_regex: Option<&Regex>,
        resolved_time: Option<EpochMillis>,
    ) -> bool {
        match filter.op {
            Operator::Eq => field_value == filter.value,
            Operator::Ne => field_value != filter.value,
            Operator::Contains => field_value.contains(&filter.value),
            Operator::Regex => filter_regex.is_some_and(|r| r.is_match(field_value)),
            Operator::NotRegex => not_regex.is_none_or(|r| !r.is_match(field_value)),
            Operator::Gt | Operator::Lt | Operator::Gte | Operator::Lte => {
                let ordering = if let Some(threshold) = resolved_time {
                    // Time-aware comparison: parse field value as timestamp
                    time::parse_timestamp(field_value)
                        .and_then(|field_ts| field_ts.partial_cmp(&threshold))
                } else {
                    Self::compare_values(field_value, &filter.value)
                };
                match filter.op {
                    Operator::Gt => ordering == Some(std::cmp::Ordering::Greater),
                    Operator::Lt => ordering == Some(std::cmp::Ordering::Less),
                    Operator::Gte => ordering.is_some_and(|ord| ord != std::cmp::Ordering::Less),
                    Operator::Lte => ordering.is_some_and(|ord| ord != std::cmp::Ordering::Greater),
                    _ => unreachable!(),
                }
            }
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

    /// Check if a line matches any exclusion pattern (JSON).
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

    /// Check if a line matches any exclusion pattern (logfmt).
    fn matches_exclude_logfmt(&self, fields: &HashMap<String, String>) -> bool {
        for exclude in &self.query.exclude {
            if let Some(field_value) = fields.get(&exclude.field) {
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
                    let resolved_time = self.resolved_times.get(i).and_then(|t| *t);

                    if !self.matches_filter(
                        &field_value,
                        filter,
                        filter_regex,
                        not_regex,
                        resolved_time,
                    ) {
                        return false;
                    }
                }

                true
            }
            Parser::Logfmt => {
                // Parse line as logfmt
                let fields = parse_logfmt(line);

                // Check exclusion patterns first
                if self.matches_exclude_logfmt(&fields) {
                    return false;
                }

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
                    let resolved_time = self.resolved_times.get(i).and_then(|t| *t);

                    if !self.matches_filter(
                        &field_value,
                        filter,
                        filter_regex,
                        not_regex,
                        resolved_time,
                    ) {
                        return false;
                    }
                }

                true
            }
        }
    }
}
