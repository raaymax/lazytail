pub mod loader;

use ratatui::style::Color;
use serde::Deserialize;

/// A named color suitable for YAML config deserialization.
/// Wraps `ratatui::style::Color` with support for named colors, hex, and "default".
#[derive(Debug, Clone, Copy)]
pub struct ThemeColor(pub Color);

impl<'de> Deserialize<'de> for ThemeColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        parse_color(&s)
            .map(ThemeColor)
            .map_err(serde::de::Error::custom)
    }
}

/// Parse a color string into a ratatui `Color`.
///
/// Supports:
/// - Named colors: `red`, `dark_gray`, `light_cyan`, etc.
/// - Hex: `#rrggbb` or `#rgb`
/// - `"default"` → `Color::Reset`
pub fn parse_color(s: &str) -> Result<Color, String> {
    let s = s.trim();
    if s.eq_ignore_ascii_case("default") {
        return Ok(Color::Reset);
    }
    if let Some(hex) = s.strip_prefix('#') {
        return parse_hex_color(hex);
    }
    parse_named_color(s)
}

fn parse_hex_color(hex: &str) -> Result<Color, String> {
    match hex.len() {
        6 => {
            let r = u8::from_str_radix(&hex[0..2], 16)
                .map_err(|_| format!("invalid hex color: #{}", hex))?;
            let g = u8::from_str_radix(&hex[2..4], 16)
                .map_err(|_| format!("invalid hex color: #{}", hex))?;
            let b = u8::from_str_radix(&hex[4..6], 16)
                .map_err(|_| format!("invalid hex color: #{}", hex))?;
            Ok(Color::Rgb(r, g, b))
        }
        3 => {
            let r = u8::from_str_radix(&hex[0..1], 16)
                .map_err(|_| format!("invalid hex color: #{}", hex))?;
            let g = u8::from_str_radix(&hex[1..2], 16)
                .map_err(|_| format!("invalid hex color: #{}", hex))?;
            let b = u8::from_str_radix(&hex[2..3], 16)
                .map_err(|_| format!("invalid hex color: #{}", hex))?;
            Ok(Color::Rgb(r * 17, g * 17, b * 17))
        }
        _ => Err(format!("invalid hex color: #{}", hex)),
    }
}

fn parse_named_color(s: &str) -> Result<Color, String> {
    match s.to_lowercase().as_str() {
        "black" => Ok(Color::Black),
        "red" => Ok(Color::Red),
        "green" => Ok(Color::Green),
        "yellow" => Ok(Color::Yellow),
        "blue" => Ok(Color::Blue),
        "magenta" => Ok(Color::Magenta),
        "cyan" => Ok(Color::Cyan),
        "gray" | "grey" => Ok(Color::Gray),
        "dark_gray" | "dark_grey" | "darkgray" | "darkgrey" => Ok(Color::DarkGray),
        "light_red" | "lightred" => Ok(Color::LightRed),
        "light_green" | "lightgreen" => Ok(Color::LightGreen),
        "light_yellow" | "lightyellow" => Ok(Color::LightYellow),
        "light_blue" | "lightblue" => Ok(Color::LightBlue),
        "light_magenta" | "lightmagenta" => Ok(Color::LightMagenta),
        "light_cyan" | "lightcyan" => Ok(Color::LightCyan),
        "white" => Ok(Color::White),
        "reset" => Ok(Color::Reset),
        _ => Err(format!("unknown color: '{}'. Valid names: black, red, green, yellow, blue, magenta, cyan, gray, dark_gray, light_red, light_green, light_yellow, light_blue, light_magenta, light_cyan, white, reset, default, or hex (#rrggbb / #rgb)", s)),
    }
}

/// The base color palette (16 ANSI colors + foreground/background/selection).
#[derive(Debug, Clone, PartialEq)]
pub struct Palette {
    pub black: Color,
    pub red: Color,
    pub green: Color,
    pub yellow: Color,
    pub blue: Color,
    pub magenta: Color,
    pub cyan: Color,
    pub white: Color,
    pub bright_black: Color,
    pub bright_red: Color,
    pub bright_green: Color,
    pub bright_yellow: Color,
    pub bright_blue: Color,
    pub bright_magenta: Color,
    pub bright_cyan: Color,
    pub bright_white: Color,
    pub foreground: Color,
    pub background: Color,
    pub selection: Color,
}

impl Palette {
    pub fn dark() -> Self {
        Self {
            black: Color::Black,
            red: Color::Red,
            green: Color::Green,
            yellow: Color::Yellow,
            blue: Color::Blue,
            magenta: Color::Magenta,
            cyan: Color::Cyan,
            white: Color::White,
            bright_black: Color::DarkGray,
            bright_red: Color::LightRed,
            bright_green: Color::LightGreen,
            bright_yellow: Color::LightYellow,
            bright_blue: Color::LightBlue,
            bright_magenta: Color::LightMagenta,
            bright_cyan: Color::LightCyan,
            bright_white: Color::White,
            foreground: Color::White,
            background: Color::Black,
            selection: Color::DarkGray,
        }
    }

    pub fn light() -> Self {
        Self {
            black: Color::Black,
            red: Color::Red,
            green: Color::Rgb(0, 128, 0),
            yellow: Color::Rgb(128, 128, 0),
            blue: Color::Blue,
            magenta: Color::Magenta,
            cyan: Color::Rgb(0, 128, 128),
            white: Color::White,
            bright_black: Color::DarkGray,
            bright_red: Color::LightRed,
            bright_green: Color::LightGreen,
            bright_yellow: Color::LightYellow,
            bright_blue: Color::LightBlue,
            bright_magenta: Color::LightMagenta,
            bright_cyan: Color::LightCyan,
            bright_white: Color::White,
            foreground: Color::Black,
            background: Color::White,
            selection: Color::Rgb(200, 200, 200),
        }
    }

    /// Derive semantic UI colors from this palette.
    pub fn derive_ui_colors(&self) -> UiColors {
        let light_bg = self.is_background_light();

        let (expanded_bg, severity_warn_bg, severity_error_bg, severity_fatal_bg, popup_bg) =
            if light_bg {
                (
                    Color::Rgb(230, 230, 240),
                    Color::Rgb(255, 248, 220),
                    Color::Rgb(255, 230, 230),
                    Color::Rgb(255, 220, 235),
                    self.white,
                )
            } else {
                (
                    Color::Rgb(30, 30, 40),
                    Color::Rgb(50, 40, 0),
                    Color::Rgb(55, 10, 10),
                    Color::Rgb(75, 0, 15),
                    self.black,
                )
            };

        UiColors {
            fg: self.foreground,
            muted: self.bright_black,
            accent: self.cyan,
            highlight: self.magenta,
            primary: self.yellow,
            positive: self.green,
            negative: self.red,
            selection_bg: self.selection,
            selection_fg: self.foreground,
            expanded_bg,
            severity_warn_bg,
            severity_error_bg,
            severity_fatal_bg,
            severity_fatal: self.magenta,
            severity_error: self.red,
            severity_warn: self.yellow,
            severity_info: self.green,
            severity_debug: self.cyan,
            severity_trace: self.bright_black,
            filter_plain: self.white,
            filter_regex: self.cyan,
            filter_query: self.magenta,
            filter_error: self.red,
            popup_bg,
            source_colors: vec![
                self.cyan,
                self.green,
                self.yellow,
                self.magenta,
                self.blue,
                self.red,
                self.bright_cyan,
                self.bright_green,
            ],
        }
    }

    /// Check if the palette background is light (for deriving appropriate UI colors).
    fn is_background_light(&self) -> bool {
        match self.background {
            Color::White
            | Color::LightYellow
            | Color::LightGreen
            | Color::LightBlue
            | Color::LightCyan
            | Color::LightMagenta
            | Color::LightRed => true,
            Color::Rgb(r, g, b) => (r as u16 + g as u16 + b as u16) > 384,
            _ => false,
        }
    }
}

/// Semantic UI colors derived from a palette (individually overridable).
#[derive(Debug, Clone, PartialEq)]
pub struct UiColors {
    pub fg: Color,
    pub muted: Color,
    pub accent: Color,
    pub highlight: Color,
    pub primary: Color,
    pub positive: Color,
    pub negative: Color,
    pub selection_bg: Color,
    pub selection_fg: Color,
    pub expanded_bg: Color,
    pub severity_warn_bg: Color,
    pub severity_error_bg: Color,
    pub severity_fatal_bg: Color,
    pub severity_fatal: Color,
    pub severity_error: Color,
    pub severity_warn: Color,
    pub severity_info: Color,
    pub severity_debug: Color,
    pub severity_trace: Color,
    pub filter_plain: Color,
    pub filter_regex: Color,
    pub filter_query: Color,
    pub filter_error: Color,
    pub popup_bg: Color,
    pub source_colors: Vec<Color>,
}

/// A complete theme: palette + derived/overridden UI colors.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub palette: Palette,
    pub ui: UiColors,
}

impl Theme {
    pub fn dark() -> Self {
        let palette = Palette::dark();
        let ui = palette.derive_ui_colors();
        Self { palette, ui }
    }

    pub fn light() -> Self {
        let palette = Palette::light();
        let ui = palette.derive_ui_colors();
        Self { palette, ui }
    }
}

impl Default for Theme {
    fn default() -> Self {
        Self::dark()
    }
}

/// Raw theme config from YAML — either a named string or a custom struct.
#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub enum RawThemeConfig {
    Named(String),
    Custom {
        base: Option<String>,
        #[serde(default)]
        palette: Option<RawPalette>,
        #[serde(default)]
        ui: Option<RawUiColors>,
    },
}

/// Raw palette with all optional fields for partial overrides.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawPalette {
    pub black: Option<ThemeColor>,
    pub red: Option<ThemeColor>,
    pub green: Option<ThemeColor>,
    pub yellow: Option<ThemeColor>,
    pub blue: Option<ThemeColor>,
    pub magenta: Option<ThemeColor>,
    pub cyan: Option<ThemeColor>,
    pub white: Option<ThemeColor>,
    pub bright_black: Option<ThemeColor>,
    pub bright_red: Option<ThemeColor>,
    pub bright_green: Option<ThemeColor>,
    pub bright_yellow: Option<ThemeColor>,
    pub bright_blue: Option<ThemeColor>,
    pub bright_magenta: Option<ThemeColor>,
    pub bright_cyan: Option<ThemeColor>,
    pub bright_white: Option<ThemeColor>,
    pub foreground: Option<ThemeColor>,
    pub background: Option<ThemeColor>,
    pub selection: Option<ThemeColor>,
}

/// Raw UI colors with all optional fields for partial overrides.
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct RawUiColors {
    pub fg: Option<ThemeColor>,
    pub muted: Option<ThemeColor>,
    pub accent: Option<ThemeColor>,
    pub highlight: Option<ThemeColor>,
    pub primary: Option<ThemeColor>,
    pub positive: Option<ThemeColor>,
    pub negative: Option<ThemeColor>,
    pub selection_bg: Option<ThemeColor>,
    pub selection_fg: Option<ThemeColor>,
    pub expanded_bg: Option<ThemeColor>,
    pub severity_warn_bg: Option<ThemeColor>,
    pub severity_error_bg: Option<ThemeColor>,
    pub severity_fatal_bg: Option<ThemeColor>,
    pub severity_fatal: Option<ThemeColor>,
    pub severity_error: Option<ThemeColor>,
    pub severity_warn: Option<ThemeColor>,
    pub severity_info: Option<ThemeColor>,
    pub severity_debug: Option<ThemeColor>,
    pub severity_trace: Option<ThemeColor>,
    pub filter_plain: Option<ThemeColor>,
    pub filter_regex: Option<ThemeColor>,
    pub filter_query: Option<ThemeColor>,
    pub filter_error: Option<ThemeColor>,
    pub popup_bg: Option<ThemeColor>,
    pub source_colors: Option<Vec<ThemeColor>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_color_named() {
        assert_eq!(parse_color("red").unwrap(), Color::Red);
        assert_eq!(parse_color("dark_gray").unwrap(), Color::DarkGray);
        assert_eq!(parse_color("light_cyan").unwrap(), Color::LightCyan);
    }

    #[test]
    fn test_parse_color_hex_6digit() {
        assert_eq!(parse_color("#ff5500").unwrap(), Color::Rgb(255, 85, 0));
    }

    #[test]
    fn test_parse_color_hex_3digit() {
        assert_eq!(parse_color("#f50").unwrap(), Color::Rgb(255, 85, 0));
    }

    #[test]
    fn test_parse_color_default() {
        assert_eq!(parse_color("default").unwrap(), Color::Reset);
    }

    #[test]
    fn test_parse_color_invalid() {
        let result = parse_color("not_a_color");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown color"));
    }

    #[test]
    fn test_parse_color_hex_invalid() {
        let result = parse_color("#xyz");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("invalid hex color"));
    }

    #[test]
    fn test_dark_palette_derives_expected_ui_colors() {
        let ui = Palette::dark().derive_ui_colors();
        assert_eq!(ui.selection_bg, Color::DarkGray);
        assert_eq!(ui.severity_error, Color::Red);
        assert_eq!(ui.primary, Color::Yellow);
        assert_eq!(ui.accent, Color::Cyan);
        assert_eq!(ui.positive, Color::Green);
        assert_eq!(ui.negative, Color::Red);
        assert_eq!(ui.highlight, Color::Magenta);
        assert_eq!(ui.muted, Color::DarkGray);
    }

    #[test]
    fn test_light_palette_derives_different_from_dark() {
        let dark_ui = Palette::dark().derive_ui_colors();
        let light_ui = Palette::light().derive_ui_colors();
        assert_ne!(dark_ui.fg, light_ui.fg);
        assert_ne!(dark_ui.selection_bg, light_ui.selection_bg);
    }

    #[test]
    fn test_light_theme_uses_light_severity_backgrounds() {
        let theme = Theme::light();
        assert_eq!(theme.ui.expanded_bg, Color::Rgb(230, 230, 240));
        assert_eq!(theme.ui.severity_warn_bg, Color::Rgb(255, 248, 220));
        assert_eq!(theme.ui.severity_error_bg, Color::Rgb(255, 230, 230));
        assert_eq!(theme.ui.severity_fatal_bg, Color::Rgb(255, 220, 235));
        assert_eq!(theme.ui.popup_bg, Color::White);
    }

    #[test]
    fn test_dark_theme_matches_hardcoded_values() {
        let theme = Theme::dark();
        assert_eq!(theme.ui.expanded_bg, Color::Rgb(30, 30, 40));
        assert_eq!(theme.ui.severity_warn_bg, Color::Rgb(50, 40, 0));
        assert_eq!(theme.ui.severity_error_bg, Color::Rgb(55, 10, 10));
        assert_eq!(theme.ui.severity_fatal_bg, Color::Rgb(75, 0, 15));
        assert_eq!(theme.ui.selection_bg, Color::DarkGray);
    }

    #[test]
    fn test_source_colors_default() {
        let theme = Theme::dark();
        assert_eq!(
            theme.ui.source_colors,
            vec![
                Color::Cyan,
                Color::Green,
                Color::Yellow,
                Color::Magenta,
                Color::Blue,
                Color::Red,
                Color::LightCyan,
                Color::LightGreen,
            ]
        );
    }
}
