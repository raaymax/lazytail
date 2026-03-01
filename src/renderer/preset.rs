use regex::Regex;
use serde::Deserialize;
use std::collections::HashSet;

use super::field::{extract_fields, get_field, get_rest_fields};
use super::segment::{
    resolve_severity_style, resolve_status_code_style, SegmentStyle, StyledSegment,
};

/// Serde-deserializable YAML preset.
#[derive(Debug, Deserialize)]
pub struct RawPreset {
    pub name: String,
    pub detect: Option<RawDetect>,
    pub regex: Option<String>,
    pub layout: Vec<RawLayoutEntry>,
}

/// Detection rules for auto-matching.
#[derive(Debug, Deserialize)]
pub struct RawDetect {
    pub parser: Option<String>,
    pub filename: Option<String>,
}

/// A single layout entry from YAML.
#[derive(Debug, Deserialize)]
pub struct RawLayoutEntry {
    pub field: Option<String>,
    pub literal: Option<String>,
    pub style: Option<String>,
    pub width: Option<usize>,
    pub format: Option<String>,
}

/// Parser type for a preset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetParser {
    Json,
    Logfmt,
    Regex,
    Auto,
}

/// Compiled layout entry — either a field reference or a literal string.
pub enum CompiledLayoutEntry {
    Field {
        name: String,
        style_fn: StyleFn,
        width: Option<usize>,
        is_rest: bool,
        rest_format: RestFormat,
    },
    Literal {
        text: String,
        style: SegmentStyle,
    },
}

/// Determines how to style a field's value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StyleFn {
    None,
    Static(SegmentStyle),
    Severity,
    StatusCode,
}

/// Format for the `_rest` pseudo-field.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestFormat {
    KeyValue,
    Json,
}

/// A fully compiled, ready-to-render preset.
pub struct CompiledPreset {
    pub name: String,
    pub parser: PresetParser,
    pub detect: Option<super::detect::CompiledDetect>,
    pub regex: Option<Regex>,
    pub layout: Vec<CompiledLayoutEntry>,
    pub consumed_fields: HashSet<String>,
}

/// Validates and compiles a RawPreset into a CompiledPreset.
pub fn compile(raw: RawPreset) -> Result<CompiledPreset, String> {
    // Determine parser type
    let has_regex = raw.regex.is_some();
    let parser = match raw.detect.as_ref().and_then(|d| d.parser.as_deref()) {
        Some("json") => PresetParser::Json,
        Some("logfmt") => PresetParser::Logfmt,
        Some("regex") => PresetParser::Regex,
        Some("auto") => PresetParser::Auto,
        Some(other) => return Err(format!("unknown parser: {}", other)),
        None => {
            if has_regex {
                PresetParser::Regex
            } else {
                PresetParser::Auto
            }
        }
    };

    // Compile regex if present
    let compiled_regex = match raw.regex {
        Some(ref pattern) => {
            Some(Regex::new(pattern).map_err(|e| format!("invalid regex: {}", e))?)
        }
        None => None,
    };

    // Compile detect
    let detect = raw.detect.map(|d| super::detect::CompiledDetect {
        filename_pattern: d.filename,
        parser: d.parser.as_deref().and_then(|p| match p {
            "json" => Some(PresetParser::Json),
            "logfmt" => Some(PresetParser::Logfmt),
            "regex" => Some(PresetParser::Regex),
            "auto" => Some(PresetParser::Auto),
            _ => None,
        }),
    });

    // Compile layout entries and track consumed fields
    let mut consumed_fields = HashSet::new();
    let mut layout = Vec::new();

    for entry in raw.layout {
        if let Some(literal) = entry.literal {
            let style = resolve_style_string(entry.style.as_deref());
            layout.push(CompiledLayoutEntry::Literal {
                text: literal,
                style,
            });
        } else if let Some(field) = entry.field {
            let is_rest = field == "_rest";
            let style_fn = match entry.style.as_deref() {
                Some("severity") => StyleFn::Severity,
                Some("status_code") | Some("statuscode") => StyleFn::StatusCode,
                Some(s) => StyleFn::Static(resolve_style_string(Some(s))),
                None => StyleFn::None,
            };
            let rest_format = match entry.format.as_deref() {
                Some("json") => RestFormat::Json,
                _ => RestFormat::KeyValue,
            };
            if !is_rest {
                consumed_fields.insert(field.clone());
            }
            layout.push(CompiledLayoutEntry::Field {
                name: field,
                style_fn,
                width: entry.width,
                is_rest,
                rest_format,
            });
        }
    }

    Ok(CompiledPreset {
        name: raw.name,
        parser,
        detect,
        regex: compiled_regex,
        layout,
        consumed_fields,
    })
}

fn resolve_style_string(style: Option<&str>) -> SegmentStyle {
    match style {
        Some("dim") => SegmentStyle::Dim,
        Some("bold") => SegmentStyle::Bold,
        Some("italic") => SegmentStyle::Italic,
        Some("red") => SegmentStyle::Fg(super::segment::SegmentColor::Red),
        Some("green") => SegmentStyle::Fg(super::segment::SegmentColor::Green),
        Some("yellow") => SegmentStyle::Fg(super::segment::SegmentColor::Yellow),
        Some("blue") => SegmentStyle::Fg(super::segment::SegmentColor::Blue),
        Some("magenta") => SegmentStyle::Fg(super::segment::SegmentColor::Magenta),
        Some("cyan") => SegmentStyle::Fg(super::segment::SegmentColor::Cyan),
        Some("white") => SegmentStyle::Fg(super::segment::SegmentColor::White),
        Some("gray") => SegmentStyle::Fg(super::segment::SegmentColor::Gray),
        _ => SegmentStyle::Default,
    }
}

impl CompiledPreset {
    /// Returns `(mask, want)` for early reject via index flags.
    pub fn index_filter(&self) -> Option<(u32, u32)> {
        use crate::index::flags::{FLAG_FORMAT_JSON, FLAG_FORMAT_LOGFMT};
        match self.parser {
            PresetParser::Json => Some((FLAG_FORMAT_JSON, FLAG_FORMAT_JSON)),
            PresetParser::Logfmt => Some((FLAG_FORMAT_LOGFMT, FLAG_FORMAT_LOGFMT)),
            PresetParser::Regex | PresetParser::Auto => None,
        }
    }

    /// Main render function. Returns `None` if the line doesn't match the preset.
    pub fn render(&self, line: &str, flags: Option<u32>) -> Option<Vec<StyledSegment>> {
        // Early reject via index flags
        if let (Some(f), Some((mask, want))) = (flags, self.index_filter()) {
            if f & mask != want {
                return None;
            }
        }

        // Extract fields
        let source = extract_fields(line, &self.parser, self.regex.as_ref(), flags)?;

        // Walk layout entries and produce segments
        let mut segments = Vec::new();
        for entry in &self.layout {
            match entry {
                CompiledLayoutEntry::Literal { text, style } => {
                    segments.push(StyledSegment {
                        text: text.clone(),
                        style: style.clone(),
                    });
                }
                CompiledLayoutEntry::Field {
                    name,
                    style_fn,
                    width,
                    is_rest,
                    rest_format,
                } => {
                    if *is_rest {
                        let rest = get_rest_fields(&source, &self.consumed_fields);
                        if !rest.is_empty() {
                            let text = match rest_format {
                                RestFormat::KeyValue => rest
                                    .iter()
                                    .map(|(k, v)| format!("{}={}", k, v))
                                    .collect::<Vec<_>>()
                                    .join(" "),
                                RestFormat::Json => {
                                    let map: serde_json::Map<String, serde_json::Value> = rest
                                        .into_iter()
                                        .map(|(k, v)| (k, serde_json::Value::String(v)))
                                        .collect();
                                    serde_json::Value::Object(map).to_string()
                                }
                            };
                            let style = resolve_style_fn(style_fn, &text);
                            segments.push(StyledSegment { text, style });
                        }
                    } else if let Some(value) = get_field(&source, name) {
                        let formatted = apply_width(&value, *width);
                        let style = resolve_style_fn(style_fn, &value);
                        segments.push(StyledSegment {
                            text: formatted,
                            style,
                        });
                    }
                    // Missing fields are silently skipped
                }
            }
        }

        Some(segments)
    }
}

fn resolve_style_fn(style_fn: &StyleFn, value: &str) -> SegmentStyle {
    match style_fn {
        StyleFn::None => SegmentStyle::Default,
        StyleFn::Static(s) => s.clone(),
        StyleFn::Severity => resolve_severity_style(value),
        StyleFn::StatusCode => resolve_status_code_style(value),
    }
}

fn apply_width(value: &str, width: Option<usize>) -> String {
    match width {
        Some(w) => {
            if value.len() > w {
                value[..w].to_string()
            } else {
                format!("{:<width$}", value, width = w)
            }
        }
        None => value.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::segment::SegmentColor;

    fn json_preset() -> CompiledPreset {
        compile(RawPreset {
            name: "test-json".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![
                RawLayoutEntry {
                    field: Some("level".to_string()),
                    literal: None,
                    style: Some("severity".to_string()),
                    width: Some(5),
                    format: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                },
                RawLayoutEntry {
                    field: Some("message".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                },
                RawLayoutEntry {
                    field: Some("_rest".to_string()),
                    literal: None,
                    style: Some("dim".to_string()),
                    width: None,
                    format: None,
                },
            ],
        })
        .unwrap()
    }

    #[test]
    fn test_compile_json_preset() {
        let preset = json_preset();
        assert_eq!(preset.name, "test-json");
        assert_eq!(preset.parser, PresetParser::Json);
        assert!(preset.regex.is_none());
        assert_eq!(preset.layout.len(), 5);
        assert!(preset.consumed_fields.contains("level"));
        assert!(preset.consumed_fields.contains("message"));
    }

    #[test]
    fn test_compile_regex_preset() {
        let preset = compile(RawPreset {
            name: "test-regex".to_string(),
            detect: None,
            regex: Some(r"(?P<ip>\S+) (?P<method>\S+)".to_string()),
            layout: vec![RawLayoutEntry {
                field: Some("ip".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
            }],
        })
        .unwrap();
        assert_eq!(preset.parser, PresetParser::Regex);
        assert!(preset.regex.is_some());
    }

    #[test]
    fn test_compile_invalid_regex() {
        let result = compile(RawPreset {
            name: "bad".to_string(),
            detect: None,
            regex: Some("[invalid".to_string()),
            layout: vec![],
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_render_json_line() {
        let preset = json_preset();
        let segments = preset
            .render(
                r#"{"level":"error","message":"connection failed","service":"api"}"#,
                None,
            )
            .unwrap();

        assert!(segments.len() >= 3);
        assert_eq!(segments[0].text, "error");
        assert_eq!(segments[0].style, SegmentStyle::Fg(SegmentColor::Red));
        assert_eq!(segments[2].text, "connection failed");
    }

    #[test]
    fn test_render_json_line_missing_field() {
        let preset = json_preset();
        // No "message" field — should still render the other fields
        let segments = preset
            .render(r#"{"level":"info","service":"api"}"#, None)
            .unwrap();

        // level + " " + " " + _rest (message is skipped)
        let has_level = segments.iter().any(|s| s.text == "info ");
        assert!(has_level);
    }

    #[test]
    fn test_render_logfmt_line() {
        let preset = compile(RawPreset {
            name: "logfmt".to_string(),
            detect: Some(RawDetect {
                parser: Some("logfmt".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![
                RawLayoutEntry {
                    field: Some("level".to_string()),
                    literal: None,
                    style: Some("severity".to_string()),
                    width: None,
                    format: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                },
                RawLayoutEntry {
                    field: Some("msg".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                },
            ],
        })
        .unwrap();

        let segments = preset
            .render("level=error msg=timeout service=api", None)
            .unwrap();
        assert_eq!(segments[0].text, "error");
        assert_eq!(segments[0].style, SegmentStyle::Fg(SegmentColor::Red));
        assert_eq!(segments[2].text, "timeout");
    }

    #[test]
    fn test_render_regex_line() {
        let preset = compile(RawPreset {
            name: "nginx".to_string(),
            detect: None,
            regex: Some(r"(?P<ip>\S+) - - \[(?P<date>[^\]]+)\] (?P<request>.+)".to_string()),
            layout: vec![
                RawLayoutEntry {
                    field: Some("ip".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                },
                RawLayoutEntry {
                    field: Some("request".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                },
            ],
        })
        .unwrap();

        let segments = preset
            .render("192.168.1.1 - - [01/Jan/2024] GET /api/v1", None)
            .unwrap();
        assert_eq!(segments[0].text, "192.168.1.1");
        assert_eq!(segments[2].text, "GET /api/v1");
    }

    #[test]
    fn test_render_non_matching_line() {
        let preset = json_preset();
        let result = preset.render("plain text line, not JSON", None);
        assert!(result.is_none());
    }

    #[test]
    fn test_render_rest_field() {
        let preset = json_preset();
        let segments = preset
            .render(
                r#"{"level":"info","message":"ok","service":"api","duration_ms":42}"#,
                None,
            )
            .unwrap();

        let rest_segment = segments.iter().find(|s| s.text.contains("=")).unwrap();
        // Rest should contain the unconsumed fields sorted by key
        assert!(rest_segment.text.contains("duration_ms="));
        assert!(rest_segment.text.contains("service="));
        assert_eq!(rest_segment.style, SegmentStyle::Dim);
    }

    #[test]
    fn test_render_rest_json_format() {
        let preset = compile(RawPreset {
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![
                RawLayoutEntry {
                    field: Some("level".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                },
                RawLayoutEntry {
                    field: Some("_rest".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: Some("json".to_string()),
                },
            ],
        })
        .unwrap();

        let segments = preset
            .render(r#"{"level":"info","service":"api"}"#, None)
            .unwrap();

        let rest = segments
            .iter()
            .find(|s| s.text.contains("service"))
            .unwrap();
        // Should be JSON format
        assert!(rest.text.contains('{'));
        assert!(rest.text.contains('}'));
    }

    #[test]
    fn test_render_width_truncation() {
        let preset = compile(RawPreset {
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("level".to_string()),
                literal: None,
                style: None,
                width: Some(5),
                format: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"level":"information"}"#, None).unwrap();
        assert_eq!(segments[0].text, "infor");
    }

    #[test]
    fn test_render_width_padding() {
        let preset = compile(RawPreset {
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("level".to_string()),
                literal: None,
                style: None,
                width: Some(5),
                format: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"level":"err"}"#, None).unwrap();
        assert_eq!(segments[0].text, "err  ");
    }

    #[test]
    fn test_index_filter_json() {
        use crate::index::flags::FLAG_FORMAT_JSON;
        let preset = json_preset();
        let (mask, want) = preset.index_filter().unwrap();
        assert_eq!(mask, FLAG_FORMAT_JSON);
        assert_eq!(want, FLAG_FORMAT_JSON);
    }

    #[test]
    fn test_index_filter_regex() {
        let preset = compile(RawPreset {
            name: "test".to_string(),
            detect: None,
            regex: Some(r"(?P<ip>\S+)".to_string()),
            layout: vec![],
        })
        .unwrap();
        assert!(preset.index_filter().is_none());
    }

    #[test]
    fn test_early_reject() {
        use crate::index::flags::FLAG_FORMAT_LOGFMT;
        let preset = json_preset();
        // Logfmt flag, no JSON flag → should be rejected without parsing
        let result = preset.render(
            r#"{"level":"error","message":"fail"}"#,
            Some(FLAG_FORMAT_LOGFMT),
        );
        assert!(result.is_none());
    }
}
