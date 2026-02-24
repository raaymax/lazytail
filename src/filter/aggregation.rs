//! Aggregation computation for grouped query results.
//!
//! Computes grouped counts from matching log line indices, supporting
//! `count by (field1, field2, ...)` with optional `top N` limiting.

use crate::filter::query::{extract_json_field, parse_logfmt, Aggregation, Parser};
use crate::reader::LogReader;
use std::collections::HashMap;

/// A single aggregation group with its key, count, and source line indices.
#[derive(Debug, Clone)]
pub struct AggregationGroup {
    /// Field name-value pairs forming the group key.
    pub key: Vec<(String, String)>,
    /// Number of matching lines in this group.
    pub count: usize,
    /// Original line indices belonging to this group.
    pub line_indices: Vec<usize>,
}

/// Result of an aggregation computation.
#[derive(Debug, Clone)]
pub struct AggregationResult {
    /// Groups sorted by count descending.
    pub groups: Vec<AggregationGroup>,
    /// Total number of matching lines across all groups.
    pub total_matches: usize,
    /// The aggregation clause that produced this result.
    pub aggregation: Aggregation,
    /// The parser used for field extraction (retained for drill-down context).
    #[allow(dead_code)]
    pub parser: Parser,
}

impl AggregationResult {
    /// Compute aggregation from matching line indices.
    ///
    /// Reads each matching line, extracts group-by fields using the specified parser,
    /// accumulates counts, sorts by count descending, and applies the optional limit.
    pub fn compute(
        reader: &mut dyn LogReader,
        matching_indices: &[usize],
        aggregation: &Aggregation,
        parser: &Parser,
    ) -> Self {
        // HashMap: group key (field values) -> (count, line_indices)
        let mut groups: HashMap<Vec<String>, (usize, Vec<usize>)> = HashMap::new();

        for &line_idx in matching_indices {
            let line = match reader.get_line(line_idx) {
                Ok(Some(l)) => l,
                _ => continue,
            };

            let field_values = extract_fields(&line, &aggregation.fields, parser);
            let entry = groups
                .entry(field_values)
                .or_insert_with(|| (0, Vec::new()));
            entry.0 += 1;
            entry.1.push(line_idx);
        }

        // Convert to sorted Vec<AggregationGroup>
        let mut result_groups: Vec<AggregationGroup> = groups
            .into_iter()
            .map(|(key_values, (count, line_indices))| {
                let key = aggregation
                    .fields
                    .iter()
                    .zip(key_values.iter())
                    .map(|(name, value)| (name.clone(), value.clone()))
                    .collect();
                AggregationGroup {
                    key,
                    count,
                    line_indices,
                }
            })
            .collect();

        // Sort by count descending, then by key for stability
        result_groups.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.key.cmp(&b.key)));

        // Apply limit
        if let Some(limit) = aggregation.limit {
            result_groups.truncate(limit);
        }

        let total_matches = matching_indices.len();

        AggregationResult {
            groups: result_groups,
            total_matches,
            aggregation: aggregation.clone(),
            parser: parser.clone(),
        }
    }
}

/// Extract field values from a log line using the specified parser.
fn extract_fields(line: &str, fields: &[String], parser: &Parser) -> Vec<String> {
    match parser {
        Parser::Json => {
            let json: serde_json::Value = match serde_json::from_str(line) {
                Ok(v) => v,
                Err(_) => return fields.iter().map(|_| "<parse error>".to_string()).collect(),
            };
            fields
                .iter()
                .map(|f| extract_json_field(&json, f).unwrap_or_else(|| "<missing>".to_string()))
                .collect()
        }
        Parser::Logfmt => {
            let kv = parse_logfmt(line);
            fields
                .iter()
                .map(|f| {
                    kv.get(f)
                        .cloned()
                        .unwrap_or_else(|| "<missing>".to_string())
                })
                .collect()
        }
        Parser::Raw => fields.iter().map(|_| "<raw>".to_string()).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::query::{Aggregation, AggregationType};

    /// Mock reader that returns lines from a Vec.
    struct MockReader {
        lines: Vec<String>,
    }

    impl LogReader for MockReader {
        fn total_lines(&self) -> usize {
            self.lines.len()
        }

        fn get_line(&mut self, index: usize) -> anyhow::Result<Option<String>> {
            Ok(self.lines.get(index).cloned())
        }

        fn reload(&mut self) -> anyhow::Result<()> {
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    fn make_aggregation(fields: Vec<&str>, limit: Option<usize>) -> Aggregation {
        Aggregation {
            agg_type: AggregationType::CountBy,
            fields: fields.into_iter().map(|s| s.to_string()).collect(),
            limit,
        }
    }

    #[test]
    fn test_compute_json_single_field() {
        let mut reader = MockReader {
            lines: vec![
                r#"{"service":"api","level":"error"}"#.into(),
                r#"{"service":"worker","level":"info"}"#.into(),
                r#"{"service":"api","level":"warn"}"#.into(),
                r#"{"service":"api","level":"error"}"#.into(),
                r#"{"service":"worker","level":"error"}"#.into(),
            ],
        };
        let indices: Vec<usize> = (0..5).collect();
        let agg = make_aggregation(vec!["service"], None);

        let result = AggregationResult::compute(&mut reader, &indices, &agg, &Parser::Json);

        assert_eq!(result.total_matches, 5);
        assert_eq!(result.groups.len(), 2);
        // api has 3, worker has 2
        assert_eq!(result.groups[0].key, vec![("service".into(), "api".into())]);
        assert_eq!(result.groups[0].count, 3);
        assert_eq!(
            result.groups[1].key,
            vec![("service".into(), "worker".into())]
        );
        assert_eq!(result.groups[1].count, 2);
    }

    #[test]
    fn test_compute_json_multiple_fields() {
        let mut reader = MockReader {
            lines: vec![
                r#"{"service":"api","level":"error"}"#.into(),
                r#"{"service":"api","level":"error"}"#.into(),
                r#"{"service":"api","level":"info"}"#.into(),
                r#"{"service":"worker","level":"error"}"#.into(),
            ],
        };
        let indices: Vec<usize> = (0..4).collect();
        let agg = make_aggregation(vec!["service", "level"], None);

        let result = AggregationResult::compute(&mut reader, &indices, &agg, &Parser::Json);

        assert_eq!(result.groups.len(), 3);
        // (api, error) = 2, (api, info) = 1, (worker, error) = 1
        assert_eq!(result.groups[0].count, 2);
        assert_eq!(
            result.groups[0].key,
            vec![
                ("service".into(), "api".into()),
                ("level".into(), "error".into())
            ]
        );
    }

    #[test]
    fn test_compute_with_limit() {
        let mut reader = MockReader {
            lines: vec![
                r#"{"level":"error"}"#.into(),
                r#"{"level":"error"}"#.into(),
                r#"{"level":"info"}"#.into(),
                r#"{"level":"warn"}"#.into(),
            ],
        };
        let indices: Vec<usize> = (0..4).collect();
        let agg = make_aggregation(vec!["level"], Some(2));

        let result = AggregationResult::compute(&mut reader, &indices, &agg, &Parser::Json);

        assert_eq!(result.groups.len(), 2);
        assert_eq!(result.groups[0].count, 2); // error
        assert_eq!(result.groups[1].count, 1); // info or warn (alphabetical tiebreak)
    }

    #[test]
    fn test_compute_logfmt() {
        let mut reader = MockReader {
            lines: vec![
                "level=error service=api".into(),
                "level=info service=worker".into(),
                "level=error service=api".into(),
            ],
        };
        let indices: Vec<usize> = (0..3).collect();
        let agg = make_aggregation(vec!["service"], None);

        let result = AggregationResult::compute(&mut reader, &indices, &agg, &Parser::Logfmt);

        assert_eq!(result.groups.len(), 2);
        assert_eq!(result.groups[0].count, 2); // api
        assert_eq!(result.groups[1].count, 1); // worker
    }

    #[test]
    fn test_compute_preserves_line_indices() {
        let mut reader = MockReader {
            lines: vec![
                r#"{"service":"api"}"#.into(),
                r#"{"service":"worker"}"#.into(),
                r#"{"service":"api"}"#.into(),
            ],
        };
        let indices: Vec<usize> = (0..3).collect();
        let agg = make_aggregation(vec!["service"], None);

        let result = AggregationResult::compute(&mut reader, &indices, &agg, &Parser::Json);

        let api_group = &result.groups[0];
        assert_eq!(api_group.line_indices, vec![0, 2]);
        let worker_group = &result.groups[1];
        assert_eq!(worker_group.line_indices, vec![1]);
    }

    #[test]
    fn test_compute_empty_indices() {
        let mut reader = MockReader { lines: vec![] };
        let agg = make_aggregation(vec!["service"], None);

        let result = AggregationResult::compute(&mut reader, &[], &agg, &Parser::Json);

        assert_eq!(result.total_matches, 0);
        assert!(result.groups.is_empty());
    }

    #[test]
    fn test_compute_missing_field() {
        let mut reader = MockReader {
            lines: vec![r#"{"service":"api"}"#.into(), r#"{"other":"value"}"#.into()],
        };
        let indices: Vec<usize> = (0..2).collect();
        let agg = make_aggregation(vec!["service"], None);

        let result = AggregationResult::compute(&mut reader, &indices, &agg, &Parser::Json);

        assert_eq!(result.groups.len(), 2);
        // One group for "api", one for "<missing>"
    }
}
