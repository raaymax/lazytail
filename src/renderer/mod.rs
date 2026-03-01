pub mod builtin;
pub mod detect;
pub mod field;
pub mod preset;
pub mod segment;

use preset::CompiledPreset;
use segment::StyledSegment;

/// Holds all compiled presets (user + builtins).
pub struct PresetRegistry {
    presets: Vec<CompiledPreset>,
}

impl PresetRegistry {
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
}
