use regex::Regex;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};

use super::field::{
    extract_fields, get_field, get_rest_fields, json_value_type, FieldSource, JsonValueType,
};
use super::format::FieldFormat;
use super::segment::{
    resolve_severity_style, resolve_status_code_style, SegmentColor, SegmentStyle, StyledSegment,
};

pub use crate::config::types::StyleValue;

/// Serde-deserializable YAML preset.
#[derive(Debug, Deserialize)]
pub struct RawPreset {
    pub name: String,
    /// Top-level parser: sets the parser without enabling auto-detection.
    pub parser: Option<String>,
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
    pub style: Option<StyleValue>,
    pub width: Option<usize>,
    pub format: Option<String>,
    pub style_map: Option<HashMap<String, String>>,
    pub max_width: Option<usize>,
    pub style_when: Option<Vec<RawStyleCondition>>,
    /// Only render this field when the JSON value at the path matches this type.
    /// Supported values: `string`, `number`, `bool`, `array`, `object`.
    pub value_type: Option<String>,
}

/// A single conditional style rule (used by preset compilation).
#[derive(Debug, Deserialize)]
pub struct RawStyleCondition {
    pub field: Option<String>,
    pub op: String,
    pub value: String,
    pub style: StyleValue,
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
        max_width: Option<usize>,
        is_rest: bool,
        rest_format: RestFormat,
        field_format: Option<FieldFormat>,
        value_type: Option<JsonValueType>,
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
    Map(HashMap<String, SegmentStyle>),
    Conditional(Vec<CompiledCondition>),
}

/// Comparison operator for conditional styling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompareOp {
    Eq,
    Ne,
    Gt,
    Lt,
    Gte,
    Lte,
    Contains,
    Regex,
}

/// A compiled conditional style rule.
#[derive(Debug, Clone)]
pub struct CompiledCondition {
    pub field: Option<String>,
    pub op: CompareOp,
    pub value: String,
    pub compiled_regex: Option<Regex>,
    pub style: SegmentStyle,
}

/// `compiled_regex` is excluded — it's deterministically derived from `value`
/// when `op == Regex`, so equal `(op, value)` implies equivalent regex behavior.
impl PartialEq for CompiledCondition {
    fn eq(&self, other: &Self) -> bool {
        self.field == other.field
            && self.op == other.op
            && self.value == other.value
            && self.style == other.style
    }
}

impl Eq for CompiledCondition {}

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
    // Determine parser type: top-level `parser` takes priority, then `detect.parser`
    let has_regex = raw.regex.is_some();
    let parser_str = raw
        .parser
        .as_deref()
        .or_else(|| raw.detect.as_ref().and_then(|d| d.parser.as_deref()));
    let parser = match parser_str {
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
            let style = resolve_style_value(entry.style.as_ref())?;
            layout.push(CompiledLayoutEntry::Literal {
                text: literal,
                style,
            });
        } else if let Some(field) = entry.field {
            // Validate mutual exclusivity of style, style_map, style_when
            let style_count = entry.style.is_some() as u8
                + entry.style_map.is_some() as u8
                + entry.style_when.is_some() as u8;
            if style_count > 1 {
                return Err(format!(
                    "field '{}': `style`, `style_map`, and `style_when` are mutually exclusive",
                    field
                ));
            }
            if entry.width.is_some() && entry.max_width.is_some() {
                return Err(format!(
                    "field '{}': `width` and `max_width` are mutually exclusive",
                    field
                ));
            }

            let is_rest = field == "_rest";
            let style_fn = if let Some(ref conditions) = entry.style_when {
                let mut compiled = Vec::new();
                for cond in conditions {
                    let op = match cond.op.as_str() {
                        "eq" => CompareOp::Eq,
                        "ne" => CompareOp::Ne,
                        "gt" => CompareOp::Gt,
                        "lt" => CompareOp::Lt,
                        "gte" => CompareOp::Gte,
                        "lte" => CompareOp::Lte,
                        "contains" => CompareOp::Contains,
                        "regex" => CompareOp::Regex,
                        other => {
                            return Err(format!(
                                "field '{}': unknown style_when operator: {}",
                                field, other
                            ))
                        }
                    };
                    let compiled_regex = if op == CompareOp::Regex {
                        Some(Regex::new(&cond.value).map_err(|e| {
                            format!("field '{}': invalid regex in style_when: {}", field, e)
                        })?)
                    } else {
                        None
                    };
                    let style = resolve_style_value(Some(&cond.style))?;
                    compiled.push(CompiledCondition {
                        field: cond.field.clone(),
                        op,
                        value: cond.value.clone(),
                        compiled_regex,
                        style,
                    });
                }
                StyleFn::Conditional(compiled)
            } else if let Some(ref map) = entry.style_map {
                let mut compiled_map = HashMap::new();
                for (k, v) in map {
                    compiled_map.insert(k.clone(), resolve_style_string(Some(v)));
                }
                StyleFn::Map(compiled_map)
            } else {
                match &entry.style {
                    Some(StyleValue::Single(s)) => match s.as_str() {
                        "severity" => StyleFn::Severity,
                        "status_code" | "statuscode" => StyleFn::StatusCode,
                        _ => StyleFn::Static(resolve_style_string(Some(s))),
                    },
                    Some(StyleValue::List(names)) => {
                        StyleFn::Static(resolve_compound_style(names)?)
                    }
                    None => StyleFn::None,
                }
            };
            let rest_format = match entry.format.as_deref() {
                Some("json") => RestFormat::Json,
                _ => RestFormat::KeyValue,
            };
            // Parse field format for non-rest fields
            let field_format = if !is_rest {
                entry.format.as_deref().and_then(FieldFormat::parse)
            } else {
                None
            };
            let value_type = match entry.value_type.as_deref() {
                Some("string") => Some(JsonValueType::String),
                Some("number") => Some(JsonValueType::Number),
                Some("bool") => Some(JsonValueType::Bool),
                Some("array") => Some(JsonValueType::Array),
                Some("object") => Some(JsonValueType::Object),
                Some(other) => {
                    return Err(format!(
                        "field '{}': unknown value_type: {} (expected string, number, bool, array, object)",
                        field, other
                    ))
                }
                None => None,
            };
            if !is_rest {
                consumed_fields.insert(field.clone());
            }
            layout.push(CompiledLayoutEntry::Field {
                name: field,
                style_fn,
                width: entry.width,
                max_width: entry.max_width,
                is_rest,
                rest_format,
                field_format,
                value_type,
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
        Some("red") => SegmentStyle::Fg(SegmentColor::Red),
        Some("green") => SegmentStyle::Fg(SegmentColor::Green),
        Some("yellow") => SegmentStyle::Fg(SegmentColor::Yellow),
        Some("blue") => SegmentStyle::Fg(SegmentColor::Blue),
        Some("magenta") => SegmentStyle::Fg(SegmentColor::Magenta),
        Some("cyan") => SegmentStyle::Fg(SegmentColor::Cyan),
        Some("white") => SegmentStyle::Fg(SegmentColor::White),
        Some("gray") => SegmentStyle::Fg(SegmentColor::Gray),
        Some(s) if s.starts_with("palette.") => {
            SegmentStyle::Fg(SegmentColor::Palette(s["palette.".len()..].to_string()))
        }
        _ => SegmentStyle::Default,
    }
}

fn resolve_style_value(style: Option<&StyleValue>) -> Result<SegmentStyle, String> {
    match style {
        Some(StyleValue::Single(s)) => Ok(resolve_style_string(Some(s))),
        Some(StyleValue::List(names)) => resolve_compound_style(names),
        None => Ok(SegmentStyle::Default),
    }
}

fn resolve_compound_style(names: &[String]) -> Result<SegmentStyle, String> {
    if names.is_empty() {
        return Ok(SegmentStyle::Default);
    }
    let mut dim = false;
    let mut bold = false;
    let mut italic = false;
    let mut fg: Option<SegmentColor> = None;

    for name in names {
        match name.as_str() {
            "dim" => dim = true,
            "bold" => bold = true,
            "italic" => italic = true,
            color_name => {
                let color = if let Some(suffix) = color_name.strip_prefix("palette.") {
                    SegmentColor::Palette(suffix.to_string())
                } else {
                    match color_name {
                        "red" => SegmentColor::Red,
                        "green" => SegmentColor::Green,
                        "yellow" => SegmentColor::Yellow,
                        "blue" => SegmentColor::Blue,
                        "magenta" => SegmentColor::Magenta,
                        "cyan" => SegmentColor::Cyan,
                        "white" => SegmentColor::White,
                        "gray" => SegmentColor::Gray,
                        _ => return Err(format!("unknown style name: {}", name)),
                    }
                };
                if fg.is_some() {
                    return Err(format!(
                        "compound style has two colors: cannot combine '{}' with existing color",
                        name
                    ));
                }
                fg = Some(color);
            }
        }
    }

    Ok(SegmentStyle::Compound {
        dim,
        bold,
        italic,
        fg,
    })
}

fn apply_max_width(value: &str, max_width: usize) -> String {
    if value.chars().count() > max_width {
        value.chars().take(max_width).collect()
    } else {
        value.to_string()
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

        // Walk layout entries and produce segments.
        // Track whether the last emitted segment came from a field so we can
        // auto-insert a space between consecutive fields that have no literal
        // separator between them.
        let mut segments = Vec::new();
        let mut has_field_content = false;
        let mut last_was_field = false;
        for entry in &self.layout {
            match entry {
                CompiledLayoutEntry::Literal { text, style } => {
                    segments.push(StyledSegment {
                        text: text.clone(),
                        style: style.clone(),
                    });
                    last_was_field = false;
                }
                CompiledLayoutEntry::Field {
                    name,
                    style_fn,
                    width,
                    max_width,
                    is_rest,
                    rest_format,
                    field_format,
                    value_type,
                } => {
                    // If value_type is set, skip this field when the JSON type doesn't match
                    if let Some(expected) = value_type {
                        if json_value_type(&source, name).as_ref() != Some(expected) {
                            continue;
                        }
                    }
                    if *is_rest {
                        let rest = get_rest_fields(&source, &self.consumed_fields);
                        if !rest.is_empty() {
                            if last_was_field {
                                segments.push(StyledSegment {
                                    text: " ".to_string(),
                                    style: SegmentStyle::Default,
                                });
                            }
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
                            let style = resolve_style_fn(style_fn, &text, Some(&source));
                            segments.push(StyledSegment { text, style });
                            has_field_content = true;
                            last_was_field = true;
                        }
                    } else if let Some(value) = get_field(&source, name) {
                        if last_was_field {
                            segments.push(StyledSegment {
                                text: " ".to_string(),
                                style: SegmentStyle::Default,
                            });
                        }
                        // Apply field format if present, falling back to raw value
                        let display_value = field_format
                            .as_ref()
                            .and_then(|ff| ff.apply(&value))
                            .unwrap_or_else(|| value.clone());
                        let formatted = match (*width, *max_width) {
                            (Some(w), _) => apply_width(&display_value, Some(w)),
                            (_, Some(mw)) => apply_max_width(&display_value, mw),
                            _ => display_value,
                        };
                        let style = resolve_style_fn(style_fn, &value, Some(&source));
                        segments.push(StyledSegment {
                            text: formatted,
                            style,
                        });
                        has_field_content = true;
                        last_was_field = true;
                    }
                    // Missing fields are silently skipped (last_was_field unchanged)
                }
            }
        }

        // If no field-based segments were produced (only literals), this preset
        // doesn't meaningfully match the line — return None to allow fallthrough
        // to other presets.
        if has_field_content {
            Some(segments)
        } else {
            None
        }
    }
}

fn resolve_style_fn(style_fn: &StyleFn, value: &str, source: Option<&FieldSource>) -> SegmentStyle {
    match style_fn {
        StyleFn::None => SegmentStyle::Default,
        StyleFn::Static(s) => s.clone(),
        StyleFn::Severity => resolve_severity_style(value),
        StyleFn::StatusCode => resolve_status_code_style(value),
        StyleFn::Map(map) => map
            .get(value)
            .or_else(|| map.get("_default"))
            .cloned()
            .unwrap_or(SegmentStyle::Default),
        StyleFn::Conditional(conditions) => {
            for cond in conditions {
                let check_value = if let Some(ref field_name) = cond.field {
                    source.and_then(|s| get_field(s, field_name))
                } else {
                    Some(value.to_string())
                };
                let check_value = match check_value {
                    Some(v) => v,
                    None => continue,
                };
                if eval_condition(
                    &cond.op,
                    &check_value,
                    &cond.value,
                    cond.compiled_regex.as_ref(),
                ) {
                    return cond.style.clone();
                }
            }
            SegmentStyle::Default
        }
    }
}

fn eval_condition(
    op: &CompareOp,
    field_value: &str,
    cond_value: &str,
    regex: Option<&Regex>,
) -> bool {
    match op {
        CompareOp::Eq => field_value == cond_value,
        CompareOp::Ne => field_value != cond_value,
        CompareOp::Contains => field_value.contains(cond_value),
        CompareOp::Regex => regex.is_some_and(|re| re.is_match(field_value)),
        CompareOp::Gt | CompareOp::Lt | CompareOp::Gte | CompareOp::Lte => {
            let fv: f64 = match field_value.parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            let cv: f64 = match cond_value.parse() {
                Ok(v) => v,
                Err(_) => return false,
            };
            match op {
                CompareOp::Gt => fv > cv,
                CompareOp::Lt => fv < cv,
                CompareOp::Gte => fv >= cv,
                CompareOp::Lte => fv <= cv,
                _ => unreachable!(),
            }
        }
    }
}

fn apply_width(value: &str, width: Option<usize>) -> String {
    match width {
        Some(w) => {
            let char_count = value.chars().count();
            if char_count > w {
                value.chars().take(w).collect()
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
            parser: None,
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
                    style: Some(StyleValue::Single("severity".to_string())),
                    width: Some(5),
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: Some("message".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: Some("_rest".to_string()),
                    literal: None,
                    style: Some(StyleValue::Single("dim".to_string())),
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
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
            parser: None,
            name: "test-regex".to_string(),
            detect: None,
            regex: Some(r"(?P<ip>\S+) (?P<method>\S+)".to_string()),
            layout: vec![RawLayoutEntry {
                field: Some("ip".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();
        assert_eq!(preset.parser, PresetParser::Regex);
        assert!(preset.regex.is_some());
    }

    #[test]
    fn test_compile_invalid_regex() {
        let result = compile(RawPreset {
            parser: None,
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
            parser: None,
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
                    style: Some(StyleValue::Single("severity".to_string())),
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: Some("msg".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
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
            parser: None,
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
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: Some("request".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
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
            parser: None,
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
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: Some("_rest".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: Some("json".to_string()),
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
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
            parser: None,
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
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"level":"information"}"#, None).unwrap();
        assert_eq!(segments[0].text, "infor");
    }

    #[test]
    fn test_render_width_padding() {
        let preset = compile(RawPreset {
            parser: None,
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
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
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
            parser: None,
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

    // ========================================================================
    // Style Map Tests (R17)
    // ========================================================================

    #[test]
    fn test_compile_style_map() {
        let mut style_map = HashMap::new();
        style_map.insert("error".to_string(), "red".to_string());
        style_map.insert("warn".to_string(), "yellow".to_string());

        let preset = compile(RawPreset {
            parser: None,
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
                width: None,
                format: None,
                style_map: Some(style_map),
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        match &preset.layout[0] {
            CompiledLayoutEntry::Field { style_fn, .. } => match style_fn {
                StyleFn::Map(map) => {
                    assert_eq!(map.get("error"), Some(&SegmentStyle::Fg(SegmentColor::Red)));
                    assert_eq!(
                        map.get("warn"),
                        Some(&SegmentStyle::Fg(SegmentColor::Yellow))
                    );
                }
                other => panic!("expected StyleFn::Map, got {:?}", other),
            },
            _ => panic!("expected Field"),
        }
    }

    #[test]
    fn test_compile_style_and_style_map_error() {
        let mut style_map = HashMap::new();
        style_map.insert("error".to_string(), "red".to_string());

        let result = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("level".to_string()),
                literal: None,
                style: Some(StyleValue::Single("bold".to_string())),
                width: None,
                format: None,
                style_map: Some(style_map),
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        });
        let err = result.err().expect("expected compile error");
        assert!(err.contains("mutually exclusive"));
    }

    #[test]
    fn test_compile_width_and_max_width_error() {
        let result = compile(RawPreset {
            parser: None,
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
                style_map: None,
                max_width: Some(10),
                style_when: None,
                value_type: None,
            }],
        });
        let err = result.err().expect("expected compile error");
        assert!(err.contains("mutually exclusive"));
    }

    #[test]
    fn test_render_style_map_match() {
        let mut style_map = HashMap::new();
        style_map.insert("error".to_string(), "red".to_string());

        let preset = compile(RawPreset {
            parser: None,
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
                width: None,
                format: None,
                style_map: Some(style_map),
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"level":"error"}"#, None).unwrap();
        assert_eq!(segments[0].style, SegmentStyle::Fg(SegmentColor::Red));
    }

    #[test]
    fn test_render_style_map_no_match() {
        let mut style_map = HashMap::new();
        style_map.insert("error".to_string(), "red".to_string());

        let preset = compile(RawPreset {
            parser: None,
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
                width: None,
                format: None,
                style_map: Some(style_map),
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"level":"unknown"}"#, None).unwrap();
        assert_eq!(segments[0].style, SegmentStyle::Default);
    }

    #[test]
    fn test_render_style_map_default_fallback() {
        let mut style_map = HashMap::new();
        style_map.insert("error".to_string(), "red".to_string());
        style_map.insert("_default".to_string(), "dim".to_string());

        let preset = compile(RawPreset {
            parser: None,
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
                width: None,
                format: None,
                style_map: Some(style_map),
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"level":"info"}"#, None).unwrap();
        assert_eq!(segments[0].style, SegmentStyle::Dim);
    }

    // ========================================================================
    // Max Width Tests (R18)
    // ========================================================================

    #[test]
    fn test_render_max_width_truncates() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("message".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: Some(5),
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"message":"hello world"}"#, None).unwrap();
        assert_eq!(segments[0].text, "hello");
    }

    #[test]
    fn test_render_max_width_no_pad() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("message".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: Some(20),
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"message":"short"}"#, None).unwrap();
        // Should NOT be padded — max_width only truncates
        assert_eq!(segments[0].text, "short");
    }

    // ========================================================================
    // Compound Style Tests (R19)
    // ========================================================================

    #[test]
    fn test_compile_compound_style() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("status".to_string()),
                literal: None,
                style: Some(StyleValue::List(vec![
                    "bold".to_string(),
                    "cyan".to_string(),
                ])),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        match &preset.layout[0] {
            CompiledLayoutEntry::Field { style_fn, .. } => match style_fn {
                StyleFn::Static(SegmentStyle::Compound {
                    dim,
                    bold,
                    italic,
                    fg,
                }) => {
                    assert!(!dim);
                    assert!(*bold);
                    assert!(!italic);
                    assert_eq!(fg, &Some(SegmentColor::Cyan));
                }
                other => panic!("expected Static(Compound), got {:?}", other),
            },
            _ => panic!("expected Field"),
        }
    }

    #[test]
    fn test_compile_compound_two_colors_error() {
        let result = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("status".to_string()),
                literal: None,
                style: Some(StyleValue::List(vec![
                    "red".to_string(),
                    "cyan".to_string(),
                ])),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        });
        let err = result.err().expect("expected compile error");
        assert!(err.contains("two colors"));
    }

    #[test]
    fn test_render_compound_style() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("status".to_string()),
                literal: None,
                style: Some(StyleValue::List(vec![
                    "bold".to_string(),
                    "cyan".to_string(),
                ])),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"status":"ok"}"#, None).unwrap();
        assert_eq!(
            segments[0].style,
            SegmentStyle::Compound {
                dim: false,
                bold: true,
                italic: false,
                fg: Some(SegmentColor::Cyan),
            }
        );
    }

    // ========================================================================
    // Array Index Field Path Test (R16, end-to-end)
    // ========================================================================

    #[test]
    fn test_render_array_index_field() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("content.0.text".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset
            .render(
                r#"{"content":[{"type":"text","text":"hello world"}]}"#,
                None,
            )
            .unwrap();
        assert_eq!(segments[0].text, "hello world");
    }

    // -- Field format tests --

    #[test]
    fn test_compile_field_format_datetime() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("ts".to_string()),
                literal: None,
                style: None,
                width: None,
                format: Some("datetime".to_string()),
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        match &preset.layout[0] {
            CompiledLayoutEntry::Field { field_format, .. } => {
                assert!(field_format.is_some());
            }
            _ => panic!("expected Field"),
        }
    }

    #[test]
    fn test_compile_field_format_duration_ns() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("elapsed".to_string()),
                literal: None,
                style: None,
                width: None,
                format: Some("duration:ns".to_string()),
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        match &preset.layout[0] {
            CompiledLayoutEntry::Field { field_format, .. } => {
                assert!(field_format.is_some());
            }
            _ => panic!("expected Field"),
        }
    }

    #[test]
    fn test_render_field_format_duration() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("duration_ms".to_string()),
                literal: None,
                style: None,
                width: None,
                format: Some("duration".to_string()),
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"duration_ms":"42"}"#, None).unwrap();
        assert_eq!(segments[0].text, "42ms");
    }

    // -- style_when tests --

    #[test]
    fn test_compile_style_when() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("latency".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: Some(vec![
                    RawStyleCondition {
                        field: None,
                        op: "gt".to_string(),
                        value: "1000".to_string(),
                        style: StyleValue::Single("red".to_string()),
                    },
                    RawStyleCondition {
                        field: None,
                        op: "lt".to_string(),
                        value: "100".to_string(),
                        style: StyleValue::Single("green".to_string()),
                    },
                ]),
                value_type: None,
            }],
        })
        .unwrap();

        match &preset.layout[0] {
            CompiledLayoutEntry::Field { style_fn, .. } => match style_fn {
                StyleFn::Conditional(conds) => {
                    assert_eq!(conds.len(), 2);
                    assert_eq!(conds[0].op, CompareOp::Gt);
                    assert_eq!(conds[1].op, CompareOp::Lt);
                }
                other => panic!("expected Conditional, got {:?}", other),
            },
            _ => panic!("expected Field"),
        }
    }

    #[test]
    fn test_compile_style_when_regex() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("path".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: Some(vec![RawStyleCondition {
                    field: None,
                    op: "regex".to_string(),
                    value: r"^/api/v\d+".to_string(),
                    style: StyleValue::Single("blue".to_string()),
                }]),
                value_type: None,
            }],
        })
        .unwrap();

        match &preset.layout[0] {
            CompiledLayoutEntry::Field { style_fn, .. } => match style_fn {
                StyleFn::Conditional(conds) => {
                    assert!(conds[0].compiled_regex.is_some());
                }
                other => panic!("expected Conditional, got {:?}", other),
            },
            _ => panic!("expected Field"),
        }
    }

    #[test]
    fn test_compile_style_and_style_when_error() {
        let result = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("level".to_string()),
                literal: None,
                style: Some(StyleValue::Single("red".to_string())),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: Some(vec![RawStyleCondition {
                    field: None,
                    op: "eq".to_string(),
                    value: "error".to_string(),
                    style: StyleValue::Single("red".to_string()),
                }]),
                value_type: None,
            }],
        });

        let err = result.err().expect("expected compile error");
        assert!(err.contains("mutually exclusive"));
    }

    #[test]
    fn test_compile_style_map_and_style_when_error() {
        let mut map = HashMap::new();
        map.insert("error".to_string(), "red".to_string());

        let result = compile(RawPreset {
            parser: None,
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
                width: None,
                format: None,
                style_map: Some(map),
                max_width: None,
                style_when: Some(vec![RawStyleCondition {
                    field: None,
                    op: "eq".to_string(),
                    value: "error".to_string(),
                    style: StyleValue::Single("red".to_string()),
                }]),
                value_type: None,
            }],
        });

        let err = result.err().expect("expected compile error");
        assert!(err.contains("mutually exclusive"));
    }

    #[test]
    fn test_render_style_when_gt_match() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("latency".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: Some(vec![RawStyleCondition {
                    field: None,
                    op: "gt".to_string(),
                    value: "500".to_string(),
                    style: StyleValue::Single("red".to_string()),
                }]),
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"latency":"1000"}"#, None).unwrap();
        assert_eq!(segments[0].style, SegmentStyle::Fg(SegmentColor::Red));
    }

    #[test]
    fn test_render_style_when_gt_no_match() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("latency".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: Some(vec![RawStyleCondition {
                    field: None,
                    op: "gt".to_string(),
                    value: "500".to_string(),
                    style: StyleValue::Single("red".to_string()),
                }]),
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"latency":"100"}"#, None).unwrap();
        assert_eq!(segments[0].style, SegmentStyle::Default);
    }

    #[test]
    fn test_render_style_when_first_match_wins() {
        let preset = compile(RawPreset {
            parser: None,
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
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: Some(vec![
                    RawStyleCondition {
                        field: None,
                        op: "eq".to_string(),
                        value: "error".to_string(),
                        style: StyleValue::Single("red".to_string()),
                    },
                    RawStyleCondition {
                        field: None,
                        op: "eq".to_string(),
                        value: "error".to_string(),
                        style: StyleValue::Single("yellow".to_string()),
                    },
                ]),
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset.render(r#"{"level":"error"}"#, None).unwrap();
        // First condition matches — gets red, not yellow
        assert_eq!(segments[0].style, SegmentStyle::Fg(SegmentColor::Red));
    }

    #[test]
    fn test_render_style_when_contains() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("message".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: Some(vec![RawStyleCondition {
                    field: None,
                    op: "contains".to_string(),
                    value: "timeout".to_string(),
                    style: StyleValue::Single("yellow".to_string()),
                }]),
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset
            .render(r#"{"message":"connection timeout after 30s"}"#, None)
            .unwrap();
        assert_eq!(segments[0].style, SegmentStyle::Fg(SegmentColor::Yellow));
    }

    #[test]
    fn test_render_style_when_cross_field() {
        let preset = compile(RawPreset {
            parser: None,
            name: "test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("message".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: Some(vec![RawStyleCondition {
                    field: Some("level".to_string()),
                    op: "eq".to_string(),
                    value: "error".to_string(),
                    style: StyleValue::Single("red".to_string()),
                }]),
                value_type: None,
            }],
        })
        .unwrap();

        let segments = preset
            .render(r#"{"level":"error","message":"something failed"}"#, None)
            .unwrap();
        // Message field styled red because level == "error"
        assert_eq!(segments[0].style, SegmentStyle::Fg(SegmentColor::Red));
        assert_eq!(segments[0].text, "something failed");
    }

    // ========================================================================
    // Empty field fallthrough test
    // ========================================================================

    #[test]
    fn test_consecutive_fields_get_auto_space() {
        // When two fields are adjacent (no literal separator between them),
        // the renderer should auto-insert a space so values don't concatenate.
        let preset = compile(RawPreset {
            parser: None,
            name: "no-sep".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![
                RawLayoutEntry {
                    field: Some("tool".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                // No literal separator here
                RawLayoutEntry {
                    field: Some("query".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
            ],
        })
        .unwrap();

        let result = preset
            .render(r#"{"tool":"WebSearch","query":"multitail"}"#, None)
            .unwrap();
        let text: String = result.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "WebSearch multitail");
    }

    #[test]
    fn test_consecutive_fields_with_missing_middle_field() {
        // When a middle field is missing, the surrounding fields should still
        // get an auto-space, not concatenate.
        let preset = compile(RawPreset {
            parser: None,
            name: "mid-gap".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![
                RawLayoutEntry {
                    field: Some("a".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: Some("b".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: Some("c".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
            ],
        })
        .unwrap();

        // "b" is missing — "a" and "c" should not concatenate
        let result = preset.render(r#"{"a":"hello","c":"world"}"#, None).unwrap();
        let text: String = result.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "hello world");
    }

    #[test]
    fn test_no_double_space_with_explicit_literal() {
        // When there's already an explicit literal separator, we should NOT
        // add an extra auto-space.
        let preset = compile(RawPreset {
            parser: None,
            name: "explicit-sep".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![
                RawLayoutEntry {
                    field: Some("a".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" | ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: Some("b".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
            ],
        })
        .unwrap();

        let result = preset.render(r#"{"a":"foo","b":"bar"}"#, None).unwrap();
        let text: String = result.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "foo | bar");
    }

    #[test]
    fn test_render_returns_none_when_no_fields_match() {
        // A preset expecting specific fields that don't exist in the JSON line
        // should return None (not Some with only literals), so other presets
        // can be tried.
        let preset = compile(RawPreset {
            parser: None,
            name: "specific-schema".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![
                RawLayoutEntry {
                    field: Some("agentName".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntry {
                    field: Some("subtype".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
            ],
        })
        .unwrap();

        // JSON is valid but has completely different fields
        let result = preset.render(
            r#"{"timestamp":"2026-02-22T21:22:40Z","level":"info","service":"api","msg":"ok"}"#,
            None,
        );
        assert!(
            result.is_none(),
            "Preset with no matching fields should return None for fallthrough"
        );
    }

    #[test]
    fn test_value_type_filters_field() {
        // A field with value_type: string should only render when the JSON
        // value is a string, not when it's an array or object.
        let preset = compile(RawPreset {
            parser: None,
            name: "vtype".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("content".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: Some("string".to_string()),
            }],
        })
        .unwrap();

        // String value → renders
        let result = preset.render(r#"{"content":"hello world"}"#, None);
        assert!(result.is_some());
        let text: String = result.unwrap().iter().map(|s| s.text.as_str()).collect();
        assert_eq!(text, "hello world");

        // Array value → skipped (no field content → returns None)
        let result = preset.render(r#"{"content":[{"type":"text"}]}"#, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_value_type_invalid() {
        let result = compile(RawPreset {
            parser: None,
            name: "bad-vtype".to_string(),
            detect: None,
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("x".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: Some("int".to_string()),
            }],
        });
        match result {
            Err(msg) => assert!(msg.contains("unknown value_type"), "got: {}", msg),
            Ok(_) => panic!("expected error for invalid value_type"),
        }
    }

    #[test]
    fn test_compile_palette_style() {
        let preset = compile(RawPreset {
            parser: None,
            name: "palette-test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("message".to_string()),
                literal: None,
                style: Some(StyleValue::Single("palette.foreground".to_string())),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        // Check the compiled layout has the expected style
        match &preset.layout[0] {
            CompiledLayoutEntry::Field { style_fn, .. } => match style_fn {
                StyleFn::Static(SegmentStyle::Fg(SegmentColor::Palette(name))) => {
                    assert_eq!(name, "foreground");
                }
                other => panic!("expected Static(Fg(Palette)), got: {:?}", other),
            },
            _ => panic!("expected Field layout entry"),
        }
    }

    #[test]
    fn test_compile_compound_palette_style() {
        let preset = compile(RawPreset {
            parser: None,
            name: "compound-palette-test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("message".to_string()),
                literal: None,
                style: Some(StyleValue::List(vec![
                    "bold".to_string(),
                    "palette.cyan".to_string(),
                ])),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        })
        .unwrap();

        match &preset.layout[0] {
            CompiledLayoutEntry::Field { style_fn, .. } => match style_fn {
                StyleFn::Static(SegmentStyle::Compound {
                    bold,
                    fg: Some(SegmentColor::Palette(name)),
                    ..
                }) => {
                    assert!(*bold);
                    assert_eq!(name, "cyan");
                }
                other => panic!("expected Compound with Palette, got: {:?}", other),
            },
            _ => panic!("expected Field layout entry"),
        }
    }

    #[test]
    fn test_compile_palette_two_colors_error() {
        let result = compile(RawPreset {
            parser: None,
            name: "two-palette-test".to_string(),
            detect: Some(RawDetect {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("message".to_string()),
                literal: None,
                style: Some(StyleValue::List(vec![
                    "palette.red".to_string(),
                    "palette.cyan".to_string(),
                ])),
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        });
        match result {
            Err(msg) => assert!(msg.contains("two colors"), "got: {}", msg),
            Ok(_) => panic!("expected error for two palette colors"),
        }
    }
}
