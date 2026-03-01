pub mod builtin;
pub mod detect;
pub mod field;
pub mod format;
pub mod preset;
pub mod segment;

use preset::CompiledPreset;
use segment::StyledSegment;

/// Holds all compiled presets (user + builtins).
pub struct PresetRegistry {
    presets: Vec<CompiledPreset>,
}

impl PresetRegistry {
    /// Compile renderer definitions from config into a registry.
    /// Returns the registry and a list of compilation error messages.
    pub fn compile_from_config(
        renderers: &[crate::config::types::RawRendererDef],
    ) -> (Self, Vec<String>) {
        let mut compiled = Vec::new();
        let mut errors = Vec::new();
        for raw in renderers {
            let raw_preset = preset::RawPreset {
                name: raw.name.clone(),
                detect: raw.detect.as_ref().map(|d| preset::RawDetect {
                    parser: d.parser.clone(),
                    filename: d.filename.clone(),
                }),
                regex: raw.regex.clone(),
                layout: raw
                    .layout
                    .iter()
                    .map(|e| preset::RawLayoutEntry {
                        field: e.field.clone(),
                        literal: e.literal.clone(),
                        style: e.style.clone(),
                        width: e.width,
                        format: e.format.clone(),
                        style_map: e.style_map.clone(),
                        max_width: e.max_width,
                        style_when: e.style_when.as_ref().map(|conditions| {
                            conditions
                                .iter()
                                .map(|c| preset::RawStyleCondition {
                                    field: c.field.clone(),
                                    op: c.op.clone(),
                                    value: c.value.clone(),
                                    style: c.style.clone(),
                                })
                                .collect()
                        }),
                    })
                    .collect(),
            };
            match preset::compile(raw_preset) {
                Ok(preset) => compiled.push(preset),
                Err(e) => errors.push(format!("Renderer '{}': {}", raw.name, e)),
            }
        }
        (Self::new(compiled), errors)
    }

    /// Merges user presets with builtins. User presets come first (can shadow builtins by name).
    pub fn new(user_presets: Vec<CompiledPreset>) -> Self {
        let user_names: std::collections::HashSet<String> =
            user_presets.iter().map(|p| p.name.clone()).collect();
        let mut presets = user_presets;
        for builtin in builtin::builtin_presets() {
            if !user_names.contains(&builtin.name) {
                presets.push(builtin);
            }
        }
        Self { presets }
    }

    /// Returns names of all registered presets (test-only).
    #[cfg(test)]
    pub fn all_preset_names(&self) -> Vec<&str> {
        self.presets.iter().map(|p| p.name.as_str()).collect()
    }

    /// Lookup preset by name.
    pub fn get_by_name(&self, name: &str) -> Option<&CompiledPreset> {
        self.presets.iter().find(|p| p.name == name)
    }

    /// Try each named preset in order. Returns first `Some` result.
    pub fn render_line(
        &self,
        line: &str,
        renderer_names: &[String],
        flags: Option<u32>,
    ) -> Option<Vec<StyledSegment>> {
        if renderer_names.is_empty() {
            return None;
        }
        for name in renderer_names {
            if let Some(preset) = self.get_by_name(name) {
                if let Some(segments) = preset.render(line, flags) {
                    return Some(segments);
                }
            }
        }
        None
    }

    /// For sources without explicit renderers — auto-detect presets, try each.
    pub fn render_line_auto(
        &self,
        line: &str,
        filename: Option<&str>,
        flags: Option<u32>,
    ) -> Option<Vec<StyledSegment>> {
        let detected = detect::detect_presets(&self.presets, filename, flags);
        for preset in detected {
            if let Some(segments) = preset.render(line, flags) {
                return Some(segments);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_get_by_name() {
        let registry = PresetRegistry::new(Vec::new());
        let json = registry.get_by_name("json");
        assert!(json.is_some());
        assert_eq!(json.unwrap().name, "json");
    }

    #[test]
    fn test_registry_render_line_json() {
        let registry = PresetRegistry::new(Vec::new());
        let result = registry.render_line(
            r#"{"level":"error","message":"fail"}"#,
            &["json".to_string()],
            None,
        );
        assert!(result.is_some());
    }

    #[test]
    fn test_registry_render_line_chain_fallthrough() {
        use crate::renderer::preset::{compile, RawDetect, RawLayoutEntry, RawPreset};

        // A custom preset that only matches logfmt
        let custom = compile(RawPreset {
            name: "custom-logfmt".to_string(),
            detect: Some(RawDetect {
                parser: Some("logfmt".to_string()),
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
                style_when: None,
            }],
        })
        .unwrap();

        let registry = PresetRegistry::new(vec![custom]);

        // JSON line — custom-logfmt returns None, "json" should match
        let result = registry.render_line(
            r#"{"level":"error","message":"fail"}"#,
            &["custom-logfmt".to_string(), "json".to_string()],
            None,
        );
        assert!(result.is_some());
    }

    #[test]
    fn test_registry_render_line_no_match() {
        let registry = PresetRegistry::new(Vec::new());
        let result = registry.render_line("plain text line", &["json".to_string()], None);
        assert!(result.is_none());
    }

    #[test]
    fn test_registry_auto_detect_json() {
        use crate::index::flags::FLAG_FORMAT_JSON;
        let registry = PresetRegistry::new(Vec::new());
        let result = registry.render_line_auto(
            r#"{"level":"error","message":"fail"}"#,
            None,
            Some(FLAG_FORMAT_JSON),
        );
        assert!(result.is_some());
    }

    #[test]
    fn test_user_preset_shadows_builtin() {
        use crate::renderer::preset::{compile, RawDetect, RawLayoutEntry, RawPreset};

        // User preset with same name "json" but different layout
        let user_json = compile(RawPreset {
            name: "json".to_string(),
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
                style_when: None,
            }],
        })
        .unwrap();

        let registry = PresetRegistry::new(vec![user_json]);

        // Should use user's "json" (1 layout entry) not builtin (many entries)
        let result = registry.render_line(
            r#"{"level":"error","message":"fail"}"#,
            &["json".to_string()],
            None,
        );
        let segments = result.unwrap();
        // User preset only has level field → exactly 1 segment
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "error");
    }

    #[test]
    fn test_compile_from_config_basic() {
        use crate::config::types::{RawDetectDef, RawLayoutEntryDef, RawRendererDef, StyleValue};

        let renderers = vec![RawRendererDef {
            name: "my-json".to_string(),
            detect: Some(RawDetectDef {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntryDef {
                field: Some("level".to_string()),
                literal: None,
                style: Some(StyleValue::Single("severity".to_string())),
                width: Some(5),
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
            }],
        }];

        let (registry, errors) = PresetRegistry::compile_from_config(&renderers);
        assert!(errors.is_empty());
        // User preset exists
        assert!(registry.get_by_name("my-json").is_some());
        // Builtins also present
        assert!(registry.get_by_name("json").is_some());
    }

    #[test]
    fn test_compile_from_config_errors() {
        use crate::config::types::{RawDetectDef, RawLayoutEntryDef, RawRendererDef};

        let renderers = vec![RawRendererDef {
            name: "bad-regex".to_string(),
            detect: Some(RawDetectDef {
                parser: Some("regex".to_string()),
                filename: None,
            }),
            regex: Some("[invalid(".to_string()),
            layout: vec![RawLayoutEntryDef {
                field: Some("msg".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
            }],
        }];

        let (_, errors) = PresetRegistry::compile_from_config(&renderers);
        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("bad-regex"));
        assert!(errors[0].contains("invalid regex"));
    }

    #[test]
    fn test_all_preset_names() {
        let registry = PresetRegistry::new(Vec::new());
        let names = registry.all_preset_names();
        // Should have builtins
        assert!(names.contains(&"json"));
        assert!(names.contains(&"logfmt"));
    }
}
