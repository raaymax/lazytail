use std::path::PathBuf;

use serde::Deserialize;
use strsim::jaro_winkler;

use crate::config::error::ConfigError;
use crate::theme::{Palette, RawPalette, RawThemeConfig, RawUiColors, Theme, UiColors};

const BUILTIN_THEMES: &[&str] = &["dark", "light"];
const SIMILARITY_THRESHOLD: f64 = 0.8;

/// Raw theme file structure for external `.yaml` theme files.
#[derive(Debug, Deserialize)]
pub struct RawThemeFile {
    #[allow(dead_code)]
    pub name: Option<String>,
    pub base: Option<String>,
    #[serde(default)]
    pub palette: Option<RawPalette>,
    #[serde(default)]
    pub ui: Option<RawUiColors>,
}

/// Resolve a raw theme config into a fully constructed Theme.
///
/// - `None` → dark theme (default)
/// - `Named("dark")` / `Named("light")` → built-in theme
/// - `Named(unknown)` → search themes_dirs, then error with suggestions
/// - `Custom { base, palette, ui }` → base theme with overrides applied
pub fn resolve_theme(
    raw: &Option<RawThemeConfig>,
    themes_dirs: &[PathBuf],
) -> Result<Theme, ConfigError> {
    let raw = match raw {
        None => return Ok(Theme::dark()),
        Some(r) => r,
    };

    match raw {
        RawThemeConfig::Named(name) => resolve_named(name, themes_dirs, &mut Vec::new()),
        RawThemeConfig::Custom { base, palette, ui } => {
            let mut theme = match base {
                Some(name) => resolve_named(name, themes_dirs, &mut Vec::new())?,
                None => Theme::dark(),
            };

            // Apply palette overrides and re-derive UI colors
            if let Some(raw_palette) = palette {
                apply_palette_overrides(&mut theme.palette, raw_palette);
                theme.ui = theme.palette.derive_ui_colors();
            }

            // Apply explicit UI overrides on top of derived colors
            if let Some(raw_ui) = ui {
                apply_ui_overrides(&mut theme.ui, raw_ui);
            }

            Ok(theme)
        }
    }
}

fn resolve_named(
    name: &str,
    themes_dirs: &[PathBuf],
    visited: &mut Vec<String>,
) -> Result<Theme, ConfigError> {
    match name {
        "dark" => Ok(Theme::dark()),
        "light" => Ok(Theme::light()),
        _ => {
            // Search theme directories for {name}.yaml
            for dir in themes_dirs {
                let path = dir.join(format!("{}.yaml", name));
                if path.is_file() {
                    return load_theme_file(&path, themes_dirs, visited);
                }
            }

            let external = discover_themes(themes_dirs);
            let all_names: Vec<&str> = BUILTIN_THEMES
                .iter()
                .copied()
                .chain(external.iter().map(|s| s.as_str()))
                .collect();

            let suggestion = all_names
                .iter()
                .filter(|&&known| jaro_winkler(name, known) >= SIMILARITY_THRESHOLD)
                .max_by(|a, b| {
                    jaro_winkler(name, a)
                        .partial_cmp(&jaro_winkler(name, b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|&s| s.to_string());

            let available = all_names.join(", ");
            let mut message = format!("unknown theme '{}'. Available themes: {}", name, available);
            if let Some(ref s) = suggestion {
                message.push_str(&format!(". Did you mean '{}'?", s));
            }

            Err(ConfigError::Validation {
                path: PathBuf::new(),
                message,
            })
        }
    }
}

/// Load a theme from an external YAML file with recursive base resolution.
fn load_theme_file(
    path: &std::path::Path,
    themes_dirs: &[PathBuf],
    visited: &mut Vec<String>,
) -> Result<Theme, ConfigError> {
    let name = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();

    // Cycle detection
    if visited.contains(&name) {
        return Err(ConfigError::Validation {
            path: path.to_path_buf(),
            message: format!(
                "circular theme reference detected: {} -> {}",
                visited.join(" -> "),
                name
            ),
        });
    }
    visited.push(name);

    let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    let raw: RawThemeFile = serde_saphyr::from_str(&content).map_err(|e| ConfigError::Parse {
        path: path.to_path_buf(),
        message: e.to_string(),
        line: None,
        column: None,
        suggestion: None,
    })?;

    // Resolve base theme
    let mut theme = match &raw.base {
        Some(base_name) => resolve_named(base_name, themes_dirs, visited)?,
        None => Theme::dark(),
    };

    // Apply palette overrides and re-derive UI colors
    if let Some(ref raw_palette) = raw.palette {
        apply_palette_overrides(&mut theme.palette, raw_palette);
        theme.ui = theme.palette.derive_ui_colors();
    }

    // Apply explicit UI overrides on top of derived colors
    if let Some(ref raw_ui) = raw.ui {
        apply_ui_overrides(&mut theme.ui, raw_ui);
    }

    Ok(theme)
}

/// Discover available theme names from theme directories.
///
/// Scans each directory for `.yaml` files and returns the filename stem as the theme name.
pub fn discover_themes(themes_dirs: &[PathBuf]) -> Vec<String> {
    let mut names = Vec::new();
    for dir in themes_dirs {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("yaml") {
                    if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                        names.push(stem.to_string());
                    }
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

/// Build the list of theme directories from project root and global config dir.
///
/// Returns only directories that exist on disk.
pub fn collect_themes_dirs(project_root: Option<&std::path::Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    // Project themes dir: {project_root}/.lazytail/themes/
    if let Some(root) = project_root {
        let project_themes = root.join(".lazytail").join("themes");
        if project_themes.is_dir() {
            dirs.push(project_themes);
        }

        // Repo-bundled themes: {project_root}/themes/
        let repo_themes = root.join("themes");
        if repo_themes.is_dir() {
            dirs.push(repo_themes);
        }
    }

    // Global themes dir: ~/.config/lazytail/themes/
    if let Some(lazytail_dir) = crate::source::lazytail_dir() {
        let global_themes = lazytail_dir.join("themes");
        if global_themes.is_dir() {
            dirs.push(global_themes);
        }
    }

    dirs
}

fn apply_palette_overrides(palette: &mut Palette, raw: &crate::theme::RawPalette) {
    macro_rules! override_field {
        ($field:ident) => {
            if let Some(c) = raw.$field {
                palette.$field = c.0;
            }
        };
    }
    override_field!(black);
    override_field!(red);
    override_field!(green);
    override_field!(yellow);
    override_field!(blue);
    override_field!(magenta);
    override_field!(cyan);
    override_field!(white);
    override_field!(bright_black);
    override_field!(bright_red);
    override_field!(bright_green);
    override_field!(bright_yellow);
    override_field!(bright_blue);
    override_field!(bright_magenta);
    override_field!(bright_cyan);
    override_field!(bright_white);
    override_field!(foreground);
    override_field!(background);
    override_field!(selection);
}

fn apply_ui_overrides(ui: &mut UiColors, raw: &RawUiColors) {
    macro_rules! override_field {
        ($field:ident) => {
            if let Some(c) = raw.$field {
                ui.$field = c.0;
            }
        };
    }
    override_field!(fg);
    override_field!(muted);
    override_field!(accent);
    override_field!(highlight);
    override_field!(primary);
    override_field!(positive);
    override_field!(negative);
    override_field!(selection_bg);
    override_field!(selection_fg);
    override_field!(expanded_bg);
    override_field!(severity_warn_bg);
    override_field!(severity_error_bg);
    override_field!(severity_fatal_bg);
    override_field!(severity_fatal);
    override_field!(severity_error);
    override_field!(severity_warn);
    override_field!(severity_info);
    override_field!(severity_debug);
    override_field!(severity_trace);
    override_field!(filter_plain);
    override_field!(filter_regex);
    override_field!(filter_query);
    override_field!(filter_error);
    override_field!(popup_bg);
    override_field!(bg);

    if let Some(ref colors) = raw.source_colors {
        let resolved: Vec<ratatui::style::Color> = colors.iter().map(|c| c.0).collect();
        if !resolved.is_empty() {
            ui.source_colors = resolved;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{RawPalette, ThemeColor};
    use ratatui::style::Color;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_resolve_none_returns_dark() {
        let theme = resolve_theme(&None, &[]).unwrap();
        assert_eq!(theme, Theme::dark());
    }

    #[test]
    fn test_resolve_dark_string() {
        let raw = Some(RawThemeConfig::Named("dark".into()));
        let theme = resolve_theme(&raw, &[]).unwrap();
        assert_eq!(theme, Theme::dark());
    }

    #[test]
    fn test_resolve_light_string() {
        let raw = Some(RawThemeConfig::Named("light".into()));
        let theme = resolve_theme(&raw, &[]).unwrap();
        assert_eq!(theme, Theme::light());
    }

    #[test]
    fn test_resolve_unknown_name_error() {
        let raw = Some(RawThemeConfig::Named("draclua".into()));
        let result = resolve_theme(&raw, &[]);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unknown theme"));
        assert!(err.contains("dark"));
        assert!(err.contains("light"));
    }

    #[test]
    fn test_resolve_custom_palette_override() {
        let raw = Some(RawThemeConfig::Custom {
            base: Some("dark".into()),
            palette: Some(RawPalette {
                red: Some(ThemeColor(Color::Rgb(255, 0, 0))),
                ..Default::default()
            }),
            ui: None,
        });
        let theme = resolve_theme(&raw, &[]).unwrap();
        assert_eq!(theme.palette.red, Color::Rgb(255, 0, 0));
        // Re-derived: severity_error should use the overridden red
        assert_eq!(theme.ui.severity_error, Color::Rgb(255, 0, 0));
    }

    #[test]
    fn test_resolve_custom_ui_override() {
        let raw = Some(RawThemeConfig::Custom {
            base: Some("dark".into()),
            palette: None,
            ui: Some(RawUiColors {
                primary: Some(ThemeColor(Color::Cyan)),
                ..Default::default()
            }),
        });
        let theme = resolve_theme(&raw, &[]).unwrap();
        assert_eq!(theme.ui.primary, Color::Cyan);
        // Rest should be dark defaults
        assert_eq!(theme.ui.accent, Theme::dark().ui.accent);
    }

    #[test]
    fn test_resolve_custom_ui_bg_override() {
        let raw = Some(RawThemeConfig::Custom {
            base: Some("dark".into()),
            palette: None,
            ui: Some(RawUiColors {
                bg: Some(ThemeColor(Color::Rgb(0x2e, 0x34, 0x40))),
                ..Default::default()
            }),
        });
        let theme = resolve_theme(&raw, &[]).unwrap();
        assert_eq!(theme.ui.bg, Color::Rgb(0x2e, 0x34, 0x40));
    }

    #[test]
    fn test_resolve_palette_override_then_ui_override() {
        let raw = Some(RawThemeConfig::Custom {
            base: Some("dark".into()),
            palette: Some(RawPalette {
                red: Some(ThemeColor(Color::Rgb(200, 50, 50))),
                ..Default::default()
            }),
            ui: Some(RawUiColors {
                severity_error: Some(ThemeColor(Color::Rgb(255, 0, 0))),
                ..Default::default()
            }),
        });
        let theme = resolve_theme(&raw, &[]).unwrap();
        // Palette red was overridden
        assert_eq!(theme.palette.red, Color::Rgb(200, 50, 50));
        // severity_error: the explicit UI override takes precedence over the derived one
        assert_eq!(theme.ui.severity_error, Color::Rgb(255, 0, 0));
        // negative was derived from palette red (no explicit override)
        assert_eq!(theme.ui.negative, Color::Rgb(200, 50, 50));
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_resolve_external_theme_file() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("custom.yaml"),
            "base: dark\npalette:\n  red: \"#ff0000\"\n  foreground: \"#aabbcc\"\n",
        )
        .unwrap();

        let raw = Some(RawThemeConfig::Named("custom".into()));
        let dirs = vec![temp.path().to_path_buf()];
        let theme = resolve_theme(&raw, &dirs).unwrap();

        assert_eq!(theme.palette.red, Color::Rgb(0xff, 0x00, 0x00));
        assert_eq!(theme.palette.foreground, Color::Rgb(0xaa, 0xbb, 0xcc));
        // Other colors should be dark defaults
        assert_eq!(theme.palette.green, Theme::dark().palette.green);
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_resolve_external_with_base_dark() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("warm.yaml"),
            "base: dark\npalette:\n  red: \"#ff5555\"\n",
        )
        .unwrap();

        let raw = Some(RawThemeConfig::Named("warm".into()));
        let dirs = vec![temp.path().to_path_buf()];
        let theme = resolve_theme(&raw, &dirs).unwrap();

        assert_eq!(theme.palette.red, Color::Rgb(0xff, 0x55, 0x55));
        // Base dark colors preserved
        assert_eq!(theme.palette.blue, Theme::dark().palette.blue);
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_resolve_external_with_base_external() {
        let temp = TempDir::new().unwrap();
        // parent.yaml defines red
        fs::write(
            temp.path().join("parent.yaml"),
            "base: dark\npalette:\n  red: \"#aa0000\"\n",
        )
        .unwrap();
        // child.yaml references parent and overrides green
        fs::write(
            temp.path().join("child.yaml"),
            "base: parent\npalette:\n  green: \"#00ff00\"\n",
        )
        .unwrap();

        let raw = Some(RawThemeConfig::Named("child".into()));
        let dirs = vec![temp.path().to_path_buf()];
        let theme = resolve_theme(&raw, &dirs).unwrap();

        // Red from parent
        assert_eq!(theme.palette.red, Color::Rgb(0xaa, 0x00, 0x00));
        // Green from child
        assert_eq!(theme.palette.green, Color::Rgb(0x00, 0xff, 0x00));
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_resolve_external_cycle_detection() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("a.yaml"), "base: b\n").unwrap();
        fs::write(temp.path().join("b.yaml"), "base: a\n").unwrap();

        let raw = Some(RawThemeConfig::Named("a".into()));
        let dirs = vec![temp.path().to_path_buf()];
        let result = resolve_theme(&raw, &dirs);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("circular"));
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_resolve_unknown_name_includes_external_themes() {
        let temp = TempDir::new().unwrap();
        fs::write(
            temp.path().join("monokai.yaml"),
            "base: dark\npalette:\n  red: \"#ff0000\"\n",
        )
        .unwrap();

        let raw = Some(RawThemeConfig::Named("monoka".into()));
        let dirs = vec![temp.path().to_path_buf()];
        let result = resolve_theme(&raw, &dirs);

        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("monokai"));
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_discover_themes_finds_yaml_files() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("monokai.yaml"), "base: dark\n").unwrap();
        fs::write(temp.path().join("dracula.yaml"), "base: dark\n").unwrap();
        fs::write(temp.path().join("readme.txt"), "not a theme").unwrap();

        let names = discover_themes(&[temp.path().to_path_buf()]);
        assert_eq!(names, vec!["dracula", "monokai"]);
    }

    #[test]
    fn test_discover_themes_empty_dir() {
        let temp = TempDir::new().unwrap();
        let names = discover_themes(&[temp.path().to_path_buf()]);
        assert!(names.is_empty());
    }

    #[test]
    #[ignore] // Slow: creates temp directory
    fn test_collect_themes_dirs() {
        let temp = TempDir::new().unwrap();
        let themes_dir = temp.path().join(".lazytail").join("themes");
        fs::create_dir_all(&themes_dir).unwrap();

        let dirs = collect_themes_dirs(Some(temp.path()));
        assert!(dirs.contains(&themes_dir));

        // Also verify repo-bundled themes/ dir is included
        let repo_themes = temp.path().join("themes");
        fs::create_dir_all(&repo_themes).unwrap();
        let dirs = collect_themes_dirs(Some(temp.path()));
        assert!(dirs.contains(&themes_dir));
        assert!(dirs.contains(&repo_themes));
        // Verify priority: .lazytail/themes/ comes before themes/
        let pos_project = dirs.iter().position(|d| d == &themes_dir).unwrap();
        let pos_repo = dirs.iter().position(|d| d == &repo_themes).unwrap();
        assert!(pos_project < pos_repo);
    }

    #[test]
    #[ignore] // Slow: creates temp directory
    fn test_collect_themes_dirs_repo_themes_only() {
        let temp = TempDir::new().unwrap();
        let repo_themes = temp.path().join("themes");
        fs::create_dir_all(&repo_themes).unwrap();

        let dirs = collect_themes_dirs(Some(temp.path()));
        assert!(dirs.contains(&repo_themes));
    }
}
