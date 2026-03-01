use super::preset::CompiledPreset;

const BUILTIN_JSON: &str = r#"
name: json
detect:
  parser: json
layout:
  - field: timestamp
    style: dim
  - literal: " "
  - field: level
    style: severity
    width: 5
  - literal: " | "
    style: dim
  - field: message
  - field: msg
  - literal: " "
  - field: _rest
    style: dim
"#;

const BUILTIN_LOGFMT: &str = r#"
name: logfmt
detect:
  parser: logfmt
layout:
  - field: ts
    style: dim
  - literal: " "
  - field: level
    style: severity
    width: 5
  - literal: " "
  - field: msg
  - literal: " "
  - field: _rest
    style: dim
"#;

fn compile_builtin(yaml: &str) -> CompiledPreset {
    let raw: super::preset::RawPreset =
        serde_saphyr::from_str(yaml).expect("builtin preset YAML is malformed");
    super::preset::compile(raw).expect("builtin preset failed to compile")
}

/// Returns the two built-in presets: `json` and `logfmt`.
pub fn builtin_presets() -> Vec<CompiledPreset> {
    vec![
        compile_builtin(BUILTIN_JSON),
        compile_builtin(BUILTIN_LOGFMT),
    ]
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
