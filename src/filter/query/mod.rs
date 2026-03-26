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

mod ast;
mod filter;
mod parser;
pub(crate) mod time;

// Re-export public types used outside this module
pub use ast::{Aggregation, FilterQuery, Parser};
pub use filter::QueryFilter;
pub use parser::parse_query;
pub use time::TsBounds;

// Re-export types only used in tests
#[cfg(test)]
pub use ast::{AggregationType, ExcludePattern, FieldFilter, Operator};

// Re-export from shared parsing module
pub use crate::parsing::{extract_json_field, parse_logfmt};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::Filter;

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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
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

    #[test]
    fn test_logfmt_exclude() {
        let query = FilterQuery {
            parser: Parser::Logfmt,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "error".to_string(),
            }],
            ts_filters: vec![],
            exclude: vec![ExcludePattern {
                field: "msg".to_string(),
                pattern: "ignore".to_string(),
            }],
            aggregate: None,
        };

        let filter = QueryFilter::new(query).unwrap();

        // Matches: level=error without excluded pattern
        assert!(filter.matches("level=error msg=\"real error\""));
        // Excluded: msg contains "ignore"
        assert!(!filter.matches("level=error msg=\"please ignore this\""));
        // Not matching: wrong level
        assert!(!filter.matches("level=info msg=\"real error\""));
    }

    // ========================================================================
    // index_mask() Tests
    // ========================================================================

    #[test]
    fn test_index_mask_json_level_error() {
        use crate::index::flags::*;

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
        use crate::index::flags::*;

        let query = parse_query("logfmt | level == warn").unwrap();
        let (mask, want) = query.index_mask().unwrap();

        assert_ne!(mask & FLAG_FORMAT_LOGFMT, 0);
        assert_ne!(want & FLAG_FORMAT_LOGFMT, 0);
        assert_eq!(want & SEVERITY_MASK, SEVERITY_WARN);
    }

    #[test]
    fn test_index_mask_json_no_level_filter() {
        use crate::index::flags::*;

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
        use crate::index::flags::*;

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
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        assert!(query.index_mask().is_none());
    }

    #[test]
    fn test_index_mask_ne_no_severity() {
        use crate::index::flags::*;

        // level != "error" cannot be expressed as a simple mask
        let query = parse_query("json | level != \"error\"").unwrap();
        let (mask, _want) = query.index_mask().unwrap();

        // Has format flag but no severity constraint
        assert_ne!(mask & FLAG_FORMAT_JSON, 0);
        assert_eq!(mask & SEVERITY_MASK, 0);
    }

    #[test]
    fn test_index_mask_severity_aliases() {
        use crate::index::flags::*;

        // "err" is an alias for error
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "err".to_string(),
            }],
            ts_filters: vec![],
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
            ts_filters: vec![],
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
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        let (_, want) = query.index_mask().unwrap();
        assert_eq!(want & SEVERITY_MASK, SEVERITY_FATAL);
    }

    #[test]
    fn test_index_mask_unknown_level_no_severity() {
        use crate::index::flags::*;

        // "notice" is not a known severity
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Eq,
                value: "notice".to_string(),
            }],
            ts_filters: vec![],
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
    // Time-Based Query Tests
    // ========================================================================

    #[test]
    fn test_time_query_relative_gte() {
        // Create a log line with a timestamp from 1 minute ago
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let one_min_ago = now - 60;

        let line = format!(
            r#"{{"timestamp": "{}", "level": "error", "msg": "test"}}"#,
            one_min_ago
        );

        // Should match: timestamp >= now-5m (1 min ago is within last 5 min)
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "timestamp".to_string(),
                op: Operator::Gte,
                value: "now-5m".to_string(),
            }],
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        let filter = QueryFilter::new(query).unwrap();
        assert!(filter.matches(&line));

        // Should NOT match: timestamp >= now-30s (1 min ago is NOT within last 30s)
        let query2 = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "timestamp".to_string(),
                op: Operator::Gte,
                value: "now-30s".to_string(),
            }],
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        let filter2 = QueryFilter::new(query2).unwrap();
        assert!(!filter2.matches(&line));
    }

    #[test]
    fn test_time_query_iso8601_field() {
        // Log line with ISO 8601 timestamp
        let line = r#"{"timestamp": "2024-01-15T10:30:00Z", "level": "error"}"#;

        // Should match: timestamp >= 2024-01-15T10:00:00Z
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "timestamp".to_string(),
                op: Operator::Gte,
                value: "2024-01-15T10:00:00Z".to_string(),
            }],
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        let filter = QueryFilter::new(query).unwrap();
        assert!(filter.matches(line));
    }

    #[test]
    fn test_time_query_absolute_timestamp_value() {
        // Filter VALUE is an absolute timestamp (not "now-..." relative)
        // Field value: 2024-01-15T10:30:00Z (epoch 1705314600000)
        // Filter value: 2024-01-15T10:00:00Z (epoch 1705312800000)
        let line = r#"{"timestamp": "2024-01-15T10:30:00Z", "level": "error"}"#;

        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "timestamp".to_string(),
                op: Operator::Gte,
                value: "2024-01-15T10:00:00Z".to_string(),
            }],
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        let filter = QueryFilter::new(query).unwrap();
        assert!(filter.matches(line));

        // Should NOT match when field timestamp is before the filter value
        let old_line = r#"{"timestamp": "2024-01-15T09:00:00Z", "level": "error"}"#;
        assert!(!filter.matches(old_line));

        // Cross-format: field has epoch seconds, filter has ISO 8601
        let epoch_line = r#"{"timestamp": "1705314600", "level": "info"}"#;
        assert!(filter.matches(epoch_line));
    }

    #[test]
    fn test_time_query_combined_with_level_filter() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let recent = now - 120; // 2 min ago

        let error_line = format!(
            r#"{{"timestamp": "{}", "level": "error", "msg": "fail"}}"#,
            recent
        );
        let info_line = format!(
            r#"{{"timestamp": "{}", "level": "info", "msg": "ok"}}"#,
            recent
        );

        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![
                FieldFilter {
                    field: "timestamp".to_string(),
                    op: Operator::Gte,
                    value: "now-5m".to_string(),
                },
                FieldFilter {
                    field: "level".to_string(),
                    op: Operator::Eq,
                    value: "error".to_string(),
                },
            ],
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        let filter = QueryFilter::new(query).unwrap();

        assert!(filter.matches(&error_line));
        assert!(!filter.matches(&info_line));
    }

    #[test]
    fn test_time_query_logfmt() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let recent = now - 60;

        let line = format!("ts={} level=error msg=\"something failed\"", recent);

        let query = FilterQuery {
            parser: Parser::Logfmt,
            filters: vec![FieldFilter {
                field: "ts".to_string(),
                op: Operator::Gte,
                value: "now-5m".to_string(),
            }],
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        let filter = QueryFilter::new(query).unwrap();
        assert!(filter.matches(&line));
    }

    #[test]
    fn test_time_query_lt_operator() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let old = now - 7200; // 2 hours ago

        let line = format!(r#"{{"timestamp": "{}", "msg": "old event"}}"#, old);

        // Should match: timestamp < now-1h (2 hours ago is before 1 hour ago)
        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "timestamp".to_string(),
                op: Operator::Lt,
                value: "now-1h".to_string(),
            }],
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        let filter = QueryFilter::new(query).unwrap();
        assert!(filter.matches(&line));
    }

    #[test]
    fn test_time_query_json_deserialize() {
        let json = r#"{
            "parser": "json",
            "filters": [
                {"field": "timestamp", "op": "gte", "value": "now-5m"},
                {"field": "level", "op": "eq", "value": "error"}
            ]
        }"#;

        let query: FilterQuery = serde_json::from_str(json).unwrap();
        assert_eq!(query.filters.len(), 2);
        assert_eq!(query.filters[0].field, "timestamp");
        assert_eq!(query.filters[0].value, "now-5m");

        // Should construct without error
        let filter = QueryFilter::new(query).unwrap();

        // Verify it works with a recent timestamp
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let line = format!(r#"{{"timestamp": "{}", "level": "error"}}"#, now - 60);
        assert!(filter.matches(&line));
    }

    #[test]
    fn test_time_query_unparseable_field_returns_no_match() {
        // When filter has a resolved time (now-5m) but field value is not a timestamp,
        // parse_timestamp returns None → comparison is None → no match
        let line = r#"{"level": "error", "msg": "something"}"#;

        let query = FilterQuery {
            parser: Parser::Json,
            filters: vec![FieldFilter {
                field: "level".to_string(),
                op: Operator::Gte,
                value: "now-5m".to_string(),
            }],
            ts_filters: vec![],
            exclude: vec![],
            aggregate: None,
        };
        let filter = QueryFilter::new(query).unwrap();
        assert!(!filter.matches(line));
    }

    // ========================================================================
    // @ts Virtual Field Tests
    // ========================================================================

    #[test]
    fn test_parse_ts_field_routed_to_ts_filters() {
        let query = parser::parse_query(r#"json | @ts >= "now-5m""#).unwrap();
        assert!(query.filters.is_empty());
        assert_eq!(query.ts_filters.len(), 1);
        assert_eq!(query.ts_filters[0].field, "@ts");
        assert_eq!(query.ts_filters[0].op, Operator::Gte);
        assert_eq!(query.ts_filters[0].value, "now-5m");
    }

    #[test]
    fn test_parse_ts_mixed_with_content_filters() {
        let query = parser::parse_query(r#"json | @ts >= "now-1h" | level == "error""#).unwrap();
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, "level");
        assert_eq!(query.ts_filters.len(), 1);
        assert_eq!(query.ts_filters[0].field, "@ts");
    }

    #[test]
    fn test_parse_ts_range() {
        let query =
            parser::parse_query(r#"json | @ts >= "now-1h" | @ts < "now-5m" | level == "error""#)
                .unwrap();
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.ts_filters.len(), 2);
        assert_eq!(query.ts_filters[0].op, Operator::Gte);
        assert_eq!(query.ts_filters[1].op, Operator::Lt);
    }

    #[test]
    fn test_partition_ts_filters_from_json_deserialization() {
        let json = r#"{
            "parser": "json",
            "filters": [
                {"field": "@ts", "op": "gte", "value": "now-5m"},
                {"field": "level", "op": "eq", "value": "error"}
            ]
        }"#;
        let mut query: FilterQuery = serde_json::from_str(json).unwrap();
        // Before partition: both in filters
        assert_eq!(query.filters.len(), 2);
        assert!(query.ts_filters.is_empty());

        query.partition_ts_filters();

        // After partition: separated
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, "level");
        assert_eq!(query.ts_filters.len(), 1);
        assert_eq!(query.ts_filters[0].field, "@ts");
    }

    #[test]
    fn test_parse_ts_standalone_no_parser() {
        let query = parser::parse_query(r#"@ts >= "now-5m""#).unwrap();
        assert_eq!(query.parser, Parser::Raw);
        assert!(query.filters.is_empty());
        assert_eq!(query.ts_filters.len(), 1);
        assert_eq!(query.ts_filters[0].value, "now-5m");
    }

    #[test]
    fn test_parse_ts_then_parser_then_filter() {
        let query = parser::parse_query(r#"@ts >= "now-1h" | json | level == "error""#).unwrap();
        assert_eq!(query.parser, Parser::Json);
        assert_eq!(query.ts_filters.len(), 1);
        assert_eq!(query.filters.len(), 1);
        assert_eq!(query.filters[0].field, "level");
    }

    #[test]
    fn test_parse_ts_range_then_logfmt() {
        let query =
            parser::parse_query(r#"@ts >= "now-1h" | @ts < "now-5m" | logfmt | level == error"#)
                .unwrap();
        assert_eq!(query.parser, Parser::Logfmt);
        assert_eq!(query.ts_filters.len(), 2);
        assert_eq!(query.filters.len(), 1);
    }

    #[test]
    fn test_parse_ts_only_no_content_filters() {
        let query = parser::parse_query(r#"@ts >= "now-30m""#).unwrap();
        assert_eq!(query.parser, Parser::Raw);
        assert!(query.filters.is_empty());
        assert_eq!(query.ts_filters.len(), 1);
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
