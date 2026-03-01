use super::detect::CompiledDetect;
use super::preset::{CompiledLayoutEntry, CompiledPreset, PresetParser, RestFormat, StyleFn};
use super::segment::SegmentStyle;
use std::collections::HashSet;

/// Returns the two built-in presets: `json` and `logfmt`.
pub fn builtin_presets() -> Vec<CompiledPreset> {
    vec![builtin_json(), builtin_logfmt()]
}

fn builtin_json() -> CompiledPreset {
    let consumed_fields: HashSet<String> = ["timestamp", "level", "message", "msg"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    CompiledPreset {
        name: "json".to_string(),
        parser: PresetParser::Json,
        detect: Some(CompiledDetect {
            filename_pattern: None,
            parser: Some(PresetParser::Json),
        }),
        regex: None,
        layout: vec![
            CompiledLayoutEntry::Field {
                name: "timestamp".to_string(),
                style_fn: StyleFn::Static(SegmentStyle::Dim),
                width: None,
                is_rest: false,
                rest_format: RestFormat::KeyValue,
            },
            CompiledLayoutEntry::Literal {
                text: " ".to_string(),
                style: SegmentStyle::Default,
            },
            CompiledLayoutEntry::Field {
                name: "level".to_string(),
                style_fn: StyleFn::Severity,
                width: Some(5),
                is_rest: false,
                rest_format: RestFormat::KeyValue,
            },
            CompiledLayoutEntry::Literal {
                text: " | ".to_string(),
                style: SegmentStyle::Dim,
            },
            CompiledLayoutEntry::Field {
                name: "message".to_string(),
                style_fn: StyleFn::None,
                width: None,
                is_rest: false,
                rest_format: RestFormat::KeyValue,
            },
            CompiledLayoutEntry::Field {
                name: "msg".to_string(),
                style_fn: StyleFn::None,
                width: None,
                is_rest: false,
                rest_format: RestFormat::KeyValue,
            },
            CompiledLayoutEntry::Literal {
                text: " ".to_string(),
                style: SegmentStyle::Default,
            },
            CompiledLayoutEntry::Field {
                name: "_rest".to_string(),
                style_fn: StyleFn::Static(SegmentStyle::Dim),
                width: None,
                is_rest: true,
                rest_format: RestFormat::KeyValue,
            },
        ],
        consumed_fields,
    }
}

fn builtin_logfmt() -> CompiledPreset {
    let consumed_fields: HashSet<String> = ["ts", "level", "msg"]
        .iter()
        .map(|s| s.to_string())
        .collect();

    CompiledPreset {
        name: "logfmt".to_string(),
        parser: PresetParser::Logfmt,
        detect: Some(CompiledDetect {
            filename_pattern: None,
            parser: Some(PresetParser::Logfmt),
        }),
        regex: None,
        layout: vec![
            CompiledLayoutEntry::Field {
                name: "ts".to_string(),
                style_fn: StyleFn::Static(SegmentStyle::Dim),
                width: None,
                is_rest: false,
                rest_format: RestFormat::KeyValue,
            },
            CompiledLayoutEntry::Literal {
                text: " ".to_string(),
                style: SegmentStyle::Default,
            },
            CompiledLayoutEntry::Field {
                name: "level".to_string(),
                style_fn: StyleFn::Severity,
                width: Some(5),
                is_rest: false,
                rest_format: RestFormat::KeyValue,
            },
            CompiledLayoutEntry::Literal {
                text: " ".to_string(),
                style: SegmentStyle::Default,
            },
            CompiledLayoutEntry::Field {
                name: "msg".to_string(),
                style_fn: StyleFn::None,
                width: None,
                is_rest: false,
                rest_format: RestFormat::KeyValue,
            },
            CompiledLayoutEntry::Literal {
                text: " ".to_string(),
                style: SegmentStyle::Default,
            },
            CompiledLayoutEntry::Field {
                name: "_rest".to_string(),
                style_fn: StyleFn::Static(SegmentStyle::Dim),
                width: None,
                is_rest: true,
                rest_format: RestFormat::KeyValue,
            },
        ],
        consumed_fields,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_builtin_json_preset_renders() {
        let presets = builtin_presets();
        let json_preset = presets.iter().find(|p| p.name == "json").unwrap();
        let segments = json_preset
            .render(
                r#"{"timestamp":"2024-01-01T00:00:00Z","level":"error","message":"fail","service":"api"}"#,
                None,
            )
            .unwrap();
        assert!(!segments.is_empty());
        // Should contain "error" as severity-styled
        let level_seg = segments
            .iter()
            .find(|s| s.text.starts_with("error"))
            .unwrap();
        assert_eq!(
            level_seg.style,
            crate::renderer::segment::SegmentStyle::Fg(crate::renderer::segment::SegmentColor::Red)
        );
    }

    #[test]
    fn test_builtin_logfmt_preset_renders() {
        let presets = builtin_presets();
        let logfmt_preset = presets.iter().find(|p| p.name == "logfmt").unwrap();
        let segments = logfmt_preset
            .render("ts=2024-01-01 level=warn msg=slow service=db", None)
            .unwrap();
        assert!(!segments.is_empty());
        let level_seg = segments
            .iter()
            .find(|s| s.text.starts_with("warn"))
            .unwrap();
        assert_eq!(
            level_seg.style,
            crate::renderer::segment::SegmentStyle::Fg(
                crate::renderer::segment::SegmentColor::Yellow
            )
        );
    }

    #[test]
    fn test_builtin_json_rejects_plain_text() {
        let presets = builtin_presets();
        let json_preset = presets.iter().find(|p| p.name == "json").unwrap();
        let result = json_preset.render("just a plain text line", None);
        assert!(result.is_none());
    }
}
