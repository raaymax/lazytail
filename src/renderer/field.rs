use crate::filter::query::{extract_json_field, parse_logfmt};
use regex::Regex;
use std::collections::{HashMap, HashSet};

use super::preset::PresetParser;

/// Holds parsed fields from a single line.
pub enum FieldSource {
    Json(serde_json::Value),
    Logfmt(HashMap<String, String>),
    Regex(HashMap<String, String>),
}

/// Unified field extraction entry point.
pub fn extract_fields(
    line: &str,
    parser: &PresetParser,
    regex: Option<&Regex>,
    flags: Option<u32>,
) -> Option<FieldSource> {
    match parser {
        PresetParser::Json => {
            let val: serde_json::Value = serde_json::from_str(line).ok()?;
            if val.is_object() {
                Some(FieldSource::Json(val))
            } else {
                None
            }
        }
        PresetParser::Logfmt => {
            let fields = parse_logfmt(line);
            if fields.is_empty() {
                None
            } else {
                Some(FieldSource::Logfmt(fields))
            }
        }
        PresetParser::Regex => {
            let re = regex?;
            let caps = re.captures(line)?;
            let mut map = HashMap::new();
            for name in re.capture_names().flatten() {
                if let Some(m) = caps.name(name) {
                    map.insert(name.to_string(), m.as_str().to_string());
                }
            }
            Some(FieldSource::Regex(map))
        }
        PresetParser::Auto => {
            if let Some(f) = flags {
                use crate::index::flags::{FLAG_FORMAT_JSON, FLAG_FORMAT_LOGFMT};
                if f & FLAG_FORMAT_JSON != 0 {
                    return extract_fields(line, &PresetParser::Json, regex, flags);
                }
                if f & FLAG_FORMAT_LOGFMT != 0 {
                    return extract_fields(line, &PresetParser::Logfmt, regex, flags);
                }
                None
            } else {
                // No flags — try JSON first, then logfmt
                if let Some(source) = extract_fields(line, &PresetParser::Json, regex, flags) {
                    return Some(source);
                }
                extract_fields(line, &PresetParser::Logfmt, regex, flags)
            }
        }
    }
}

/// Extracts a single field value from a FieldSource.
pub fn get_field(source: &FieldSource, field_name: &str) -> Option<String> {
    match source {
        FieldSource::Json(val) => extract_json_field(val, field_name),
        FieldSource::Logfmt(map) => map.get(field_name).cloned(),
        FieldSource::Regex(map) => map.get(field_name).cloned(),
    }
}

/// Returns all fields NOT in the consumed set, sorted by key.
pub fn get_rest_fields(source: &FieldSource, consumed: &HashSet<String>) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = match source {
        FieldSource::Json(val) => {
            if let Some(obj) = val.as_object() {
                obj.iter()
                    .filter(|(k, _)| !consumed.contains(*k))
                    .map(|(k, v)| {
                        let value = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        (k.clone(), value)
                    })
                    .collect()
            } else {
                Vec::new()
            }
        }
        FieldSource::Logfmt(map) => map
            .iter()
            .filter(|(k, _)| !consumed.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        FieldSource::Regex(map) => map
            .iter()
            .filter(|(k, _)| !consumed.contains(*k))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
    };
    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    pairs
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_json_fields() {
        let source = extract_fields(
            r#"{"level":"error","message":"fail"}"#,
            &PresetParser::Json,
            None,
            None,
        )
        .unwrap();
        assert_eq!(get_field(&source, "level"), Some("error".to_string()));
        assert_eq!(get_field(&source, "message"), Some("fail".to_string()));
    }

    #[test]
    fn test_extract_json_nested_field() {
        let source = extract_fields(
            r#"{"user":{"id":"123"},"level":"info"}"#,
            &PresetParser::Json,
            None,
            None,
        )
        .unwrap();
        assert_eq!(get_field(&source, "user.id"), Some("123".to_string()));
    }

    #[test]
    fn test_extract_logfmt_fields() {
        let source = extract_fields(
            "level=error msg=fail service=api",
            &PresetParser::Logfmt,
            None,
            None,
        )
        .unwrap();
        assert_eq!(get_field(&source, "level"), Some("error".to_string()));
        assert_eq!(get_field(&source, "msg"), Some("fail".to_string()));
    }

    #[test]
    fn test_extract_regex_fields() {
        let re = Regex::new(r"(?P<ip>\S+) - - \[(?P<date>[^\]]+)\]").unwrap();
        let source = extract_fields(
            "192.168.1.1 - - [01/Jan/2024] GET /",
            &PresetParser::Regex,
            Some(&re),
            None,
        )
        .unwrap();
        assert_eq!(get_field(&source, "ip"), Some("192.168.1.1".to_string()));
        assert_eq!(get_field(&source, "date"), Some("01/Jan/2024".to_string()));
    }

    #[test]
    fn test_extract_auto_json() {
        use crate::index::flags::FLAG_FORMAT_JSON;
        let source = extract_fields(
            r#"{"level":"info"}"#,
            &PresetParser::Auto,
            None,
            Some(FLAG_FORMAT_JSON),
        )
        .unwrap();
        assert_eq!(get_field(&source, "level"), Some("info".to_string()));
    }

    #[test]
    fn test_extract_auto_logfmt() {
        use crate::index::flags::FLAG_FORMAT_LOGFMT;
        let source = extract_fields(
            "level=info msg=hello",
            &PresetParser::Auto,
            None,
            Some(FLAG_FORMAT_LOGFMT),
        )
        .unwrap();
        assert_eq!(get_field(&source, "level"), Some("info".to_string()));
    }

    #[test]
    fn test_extract_auto_neither_flag() {
        let source = extract_fields("plain text line", &PresetParser::Auto, None, Some(0));
        assert!(source.is_none());
    }

    #[test]
    fn test_get_rest_fields() {
        let source = extract_fields(
            r#"{"level":"error","message":"fail","service":"api"}"#,
            &PresetParser::Json,
            None,
            None,
        )
        .unwrap();
        let consumed: HashSet<String> = ["level".to_string(), "message".to_string()]
            .into_iter()
            .collect();
        let rest = get_rest_fields(&source, &consumed);
        assert_eq!(rest, vec![("service".to_string(), "api".to_string())]);
    }

    #[test]
    fn test_get_rest_empty() {
        let source = extract_fields(
            r#"{"level":"error","message":"fail"}"#,
            &PresetParser::Json,
            None,
            None,
        )
        .unwrap();
        let consumed: HashSet<String> = ["level".to_string(), "message".to_string()]
            .into_iter()
            .collect();
        let rest = get_rest_fields(&source, &consumed);
        assert!(rest.is_empty());
    }
}
