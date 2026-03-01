use std::path::PathBuf;

use strsim::jaro_winkler;

use crate::config::error::ConfigError;
use crate::theme::{Palette, RawThemeConfig, RawUiColors, Theme, UiColors};

const BUILTIN_THEMES: &[&str] = &["dark", "light"];
const SIMILARITY_THRESHOLD: f64 = 0.8;

/// Resolve a raw theme config into a fully constructed Theme.
///
/// - `None` → dark theme (default)
/// - `Named("dark")` / `Named("light")` → built-in theme
/// - `Named(unknown)` → error with suggestions
/// - `Custom { base, palette, ui }` → base theme with overrides applied
pub fn resolve_theme(
    raw: &Option<RawThemeConfig>,
    _themes_dirs: &[PathBuf],
) -> Result<Theme, ConfigError> {
    let raw = match raw {
        None => return Ok(Theme::dark()),
        Some(r) => r,
    };

    match raw {
        RawThemeConfig::Named(name) => resolve_named(name),
        RawThemeConfig::Custom { base, palette, ui } => {
            let mut theme = match base {
                Some(name) => resolve_named(name)?,
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

fn resolve_named(name: &str) -> Result<Theme, ConfigError> {
    match name {
        "dark" => Ok(Theme::dark()),
        "light" => Ok(Theme::light()),
        _ => {
            let suggestion = BUILTIN_THEMES
                .iter()
                .filter(|&&known| jaro_winkler(name, known) >= SIMILARITY_THRESHOLD)
                .max_by(|a, b| {
                    jaro_winkler(name, a)
                        .partial_cmp(&jaro_winkler(name, b))
                        .unwrap_or(std::cmp::Ordering::Equal)
                })
                .map(|&s| s.to_string());

            let mut message = format!(
                "unknown theme '{}'. Available themes: {}",
                name,
                BUILTIN_THEMES.join(", ")
            );
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
}
