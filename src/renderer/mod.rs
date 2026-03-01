pub mod builtin;
pub mod detect;
pub mod field;
pub mod format;
pub mod preset;
pub mod segment;

use preset::CompiledPreset;
use segment::StyledSegment;
use std::path::{Path, PathBuf};

/// Holds all compiled presets (user + builtins).
pub struct PresetRegistry {
    presets: Vec<CompiledPreset>,
}

impl PresetRegistry {
    /// Compile renderer definitions from config into a registry.
    /// Returns the registry and a list of compilation error messages.
    ///
    /// Priority: inline config presets > external file presets > builtins.
    pub fn compile_from_config(
        renderers: &[crate::config::types::RawRendererDef],
        project_root: Option<&Path>,
    ) -> (Self, Vec<String>) {
        let mut compiled = Vec::new();
        let mut errors = Vec::new();

        // 1. Compile inline presets (highest priority)
        for raw in renderers {
            let raw_preset = preset::RawPreset {
                name: raw.name.clone(),
                parser: raw.parser.clone(),
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
                        value_type: e.value_type.clone(),
                    })
                    .collect(),
            };
            match preset::compile(raw_preset) {
                Ok(preset) => compiled.push(preset),
                Err(e) => errors.push(format!("Renderer '{}': {}", raw.name, e)),
            }
        }

        // 2. Load and compile external presets (skip names already defined inline)
        let inline_names: std::collections::HashSet<String> =
            compiled.iter().map(|p| p.name.clone()).collect();
        let dirs = collect_renderers_dirs(project_root);
        let (external_presets, external_errors) = load_external_presets(&dirs);
        errors.extend(external_errors);
        for raw_preset in external_presets {
            if inline_names.contains(&raw_preset.name) {
                continue;
            }
            match preset::compile(raw_preset) {
                Ok(preset) => compiled.push(preset),
                Err(e) => errors.push(format!("External renderer: {}", e)),
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

/// Build the list of renderer directories from project root and global config dir.
///
/// Returns only directories that exist on disk. Mirrors `collect_themes_dirs()`.
fn collect_renderers_dirs(project_root: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    if let Some(root) = project_root {
        let project_renderers = root.join(".lazytail").join("renderers");
        if project_renderers.is_dir() {
            dirs.push(project_renderers);
        }

        let repo_renderers = root.join("renderers");
        if repo_renderers.is_dir() {
            dirs.push(repo_renderers);
        }
    }

    if let Some(lazytail_dir) = crate::source::lazytail_dir() {
        let global_renderers = lazytail_dir.join("renderers");
        if global_renderers.is_dir() {
            dirs.push(global_renderers);
        }
    }

    dirs
}

/// Load external preset files from the given directories.
///
/// Each `.yaml` file is parsed as a `RawPreset`. Parse errors are collected,
/// not fatal — other presets still load.
fn load_external_presets(dirs: &[PathBuf]) -> (Vec<preset::RawPreset>, Vec<String>) {
    let mut presets = Vec::new();
    let mut errors = Vec::new();

    for dir in dirs {
        let entries = match std::fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
                continue;
            }
            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(e) => {
                    errors.push(format!(
                        "Failed to read renderer file {}: {}",
                        path.display(),
                        e
                    ));
                    continue;
                }
            };
            match serde_saphyr::from_str::<preset::RawPreset>(&content) {
                Ok(raw) => presets.push(raw),
                Err(e) => {
                    errors.push(format!(
                        "Failed to parse renderer file {}: {}",
                        path.display(),
                        e
                    ));
                }
            }
        }
    }

    (presets, errors)
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
            parser: None,
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
                value_type: None,
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
            parser: None,
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
                value_type: None,
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
            parser: None,
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
                value_type: None,
            }],
        }];

        let (registry, errors) = PresetRegistry::compile_from_config(&renderers, None);
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
            parser: None,
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
                value_type: None,
            }],
        }];

        let (_, errors) = PresetRegistry::compile_from_config(&renderers, None);
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

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_collect_renderers_dirs_project_scoped() {
        let temp = tempfile::TempDir::new().unwrap();
        let renderers_dir = temp.path().join(".lazytail").join("renderers");
        std::fs::create_dir_all(&renderers_dir).unwrap();

        let dirs = collect_renderers_dirs(Some(temp.path()));
        assert!(dirs.contains(&renderers_dir));
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_collect_renderers_dirs_repo_bundled() {
        let temp = tempfile::TempDir::new().unwrap();
        let renderers_dir = temp.path().join("renderers");
        std::fs::create_dir_all(&renderers_dir).unwrap();

        let dirs = collect_renderers_dirs(Some(temp.path()));
        assert!(dirs.contains(&renderers_dir));
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_collect_renderers_dirs_priority() {
        let temp = tempfile::TempDir::new().unwrap();
        let project_dir = temp.path().join(".lazytail").join("renderers");
        let repo_dir = temp.path().join("renderers");
        std::fs::create_dir_all(&project_dir).unwrap();
        std::fs::create_dir_all(&repo_dir).unwrap();

        let dirs = collect_renderers_dirs(Some(temp.path()));
        let pos_project = dirs.iter().position(|d| d == &project_dir).unwrap();
        let pos_repo = dirs.iter().position(|d| d == &repo_dir).unwrap();
        assert!(pos_project < pos_repo);
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_load_external_presets() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("renderers");
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("custom.yaml"),
            "name: custom\ndetect:\n  parser: json\nlayout:\n  - field: level\n",
        )
        .unwrap();
        std::fs::write(
            dir.join("custom2.yaml"),
            "name: custom2\ndetect:\n  parser: logfmt\nlayout:\n  - field: msg\n",
        )
        .unwrap();

        let (presets, errors) = load_external_presets(&[dir]);
        assert!(errors.is_empty(), "errors: {:?}", errors);
        assert_eq!(presets.len(), 2);
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"custom"));
        assert!(names.contains(&"custom2"));
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_inline_shadows_external() {
        use crate::config::types::{RawDetectDef, RawLayoutEntryDef, RawRendererDef};

        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join(".lazytail").join("renderers");
        std::fs::create_dir_all(&dir).unwrap();

        // External preset named "json"
        std::fs::write(
            dir.join("json.yaml"),
            "name: json\ndetect:\n  parser: json\nlayout:\n  - field: msg\n",
        )
        .unwrap();

        // Inline preset also named "json"
        let renderers = vec![RawRendererDef {
            parser: None,
            name: "json".to_string(),
            detect: Some(RawDetectDef {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntryDef {
                field: Some("level".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        }];

        let (registry, errors) = PresetRegistry::compile_from_config(&renderers, Some(temp.path()));
        assert!(errors.is_empty(), "errors: {:?}", errors);

        // Inline "json" should be used (has "level" field), not external (has "msg" field)
        let result = registry.render_line(
            r#"{"level":"error","msg":"fail"}"#,
            &["json".to_string()],
            None,
        );
        let segments = result.unwrap();
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "error");
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_compile_from_config_with_external() {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join(".lazytail").join("renderers");
        std::fs::create_dir_all(&dir).unwrap();

        std::fs::write(
            dir.join("custom.yaml"),
            "name: custom-ext\ndetect:\n  parser: json\nlayout:\n  - field: level\n",
        )
        .unwrap();

        let (registry, errors) = PresetRegistry::compile_from_config(&[], Some(temp.path()));
        assert!(errors.is_empty(), "errors: {:?}", errors);
        assert!(registry.get_by_name("custom-ext").is_some());
        // Builtins still present
        assert!(registry.get_by_name("json").is_some());
    }
}
