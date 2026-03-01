use crate::theme::Palette;
use ratatui::style::{Color, Modifier, Style};

/// The IR unit — text content + style metadata.
pub struct StyledSegment {
    pub text: String,
    pub style: SegmentStyle,
}

/// Style for a rendered segment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentStyle {
    Default,
    Dim,
    Bold,
    Italic,
    Fg(SegmentColor),
    Compound {
        dim: bool,
        bold: bool,
        italic: bool,
        fg: Option<SegmentColor>,
    },
}

/// Named colors for segment styling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SegmentColor {
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    White,
    Gray,
    Palette(String),
}

/// Maps severity level strings to appropriate styles. Case-insensitive.
pub fn resolve_severity_style(value: &str) -> SegmentStyle {
    match value.to_ascii_lowercase().as_str() {
        "error" | "err" | "fatal" => SegmentStyle::Fg(SegmentColor::Red),
        "warn" | "warning" => SegmentStyle::Fg(SegmentColor::Yellow),
        "info" => SegmentStyle::Fg(SegmentColor::Green),
        "debug" => SegmentStyle::Fg(SegmentColor::Cyan),
        "trace" => SegmentStyle::Fg(SegmentColor::Gray),
        _ => SegmentStyle::Default,
    }
}

/// Maps HTTP status code strings to colored styles.
pub fn resolve_status_code_style(value: &str) -> SegmentStyle {
    match value.chars().next() {
        Some('2') => SegmentStyle::Fg(SegmentColor::Green),
        Some('3') => SegmentStyle::Fg(SegmentColor::Cyan),
        Some('4') => SegmentStyle::Fg(SegmentColor::Yellow),
        Some('5') => SegmentStyle::Fg(SegmentColor::Red),
        _ => SegmentStyle::Default,
    }
}

/// Converts `SegmentStyle` → ratatui `Style`.
///
/// When a palette is provided, fixed color variants (Red, Green, etc.) resolve
/// through the palette. `Palette(name)` resolves via `palette.get_color(name)`.
pub fn to_ratatui_style(style: &SegmentStyle, palette: Option<&Palette>) -> Style {
    match style {
        SegmentStyle::Default => Style::default(),
        SegmentStyle::Dim => Style::default().add_modifier(Modifier::DIM),
        SegmentStyle::Bold => Style::default().add_modifier(Modifier::BOLD),
        SegmentStyle::Italic => Style::default().add_modifier(Modifier::ITALIC),
        SegmentStyle::Fg(color) => Style::default().fg(segment_color_to_ratatui(color, palette)),
        SegmentStyle::Compound {
            dim,
            bold,
            italic,
            fg,
        } => {
            let mut style = Style::default();
            if *dim {
                style = style.add_modifier(Modifier::DIM);
            }
            if *bold {
                style = style.add_modifier(Modifier::BOLD);
            }
            if *italic {
                style = style.add_modifier(Modifier::ITALIC);
            }
            if let Some(color) = fg {
                style = style.fg(segment_color_to_ratatui(color, palette));
            }
            style
        }
    }
}

/// Concatenate segment text values into a plain text string.
pub fn segments_to_plain_text(segments: &[StyledSegment]) -> String {
    let mut out = String::new();
    for seg in segments {
        out.push_str(&seg.text);
    }
    out
}

fn segment_color_to_ratatui(color: &SegmentColor, palette: Option<&Palette>) -> Color {
    match color {
        SegmentColor::Red => palette
            .and_then(|p| p.get_color("red"))
            .unwrap_or(Color::Red),
        SegmentColor::Green => palette
            .and_then(|p| p.get_color("green"))
            .unwrap_or(Color::Green),
        SegmentColor::Yellow => palette
            .and_then(|p| p.get_color("yellow"))
            .unwrap_or(Color::Yellow),
        SegmentColor::Blue => palette
            .and_then(|p| p.get_color("blue"))
            .unwrap_or(Color::Blue),
        SegmentColor::Magenta => palette
            .and_then(|p| p.get_color("magenta"))
            .unwrap_or(Color::Magenta),
        SegmentColor::Cyan => palette
            .and_then(|p| p.get_color("cyan"))
            .unwrap_or(Color::Cyan),
        SegmentColor::White => palette
            .and_then(|p| p.get_color("white"))
            .unwrap_or(Color::White),
        SegmentColor::Gray => palette
            .and_then(|p| p.get_color("bright_black"))
            .unwrap_or(Color::Gray),
        SegmentColor::Palette(name) => palette
            .and_then(|p| p.get_color(name))
            .unwrap_or(Color::Reset),
    }
}

/// Convert styled segments to a string with ANSI escape codes.
pub fn segments_to_ansi(segments: &[StyledSegment], palette: Option<&Palette>) -> String {
    let mut out = String::new();
    for seg in segments {
        if matches!(seg.style, SegmentStyle::Default) {
            out.push_str(&seg.text);
        } else {
            out.push_str("\x1b[");
            out.push_str(&style_to_ansi(&seg.style, palette));
            out.push('m');
            out.push_str(&seg.text);
            out.push_str("\x1b[0m");
        }
    }
    out
}

fn style_to_ansi(style: &SegmentStyle, palette: Option<&Palette>) -> String {
    match style {
        SegmentStyle::Default => String::new(),
        SegmentStyle::Dim => "2".to_string(),
        SegmentStyle::Bold => "1".to_string(),
        SegmentStyle::Italic => "3".to_string(),
        SegmentStyle::Fg(color) => segment_color_to_ansi(color, palette),
        SegmentStyle::Compound {
            dim,
            bold,
            italic,
            fg,
        } => {
            let mut codes = Vec::new();
            if *dim {
                codes.push("2".to_string());
            }
            if *bold {
                codes.push("1".to_string());
            }
            if *italic {
                codes.push("3".to_string());
            }
            if let Some(color) = fg {
                codes.push(segment_color_to_ansi(color, palette));
            }
            codes.join(";")
        }
    }
}

fn segment_color_to_ansi(color: &SegmentColor, palette: Option<&Palette>) -> String {
    match color {
        SegmentColor::Red => "31".to_string(),
        SegmentColor::Green => "32".to_string(),
        SegmentColor::Yellow => "33".to_string(),
        SegmentColor::Blue => "34".to_string(),
        SegmentColor::Magenta => "35".to_string(),
        SegmentColor::Cyan => "36".to_string(),
        SegmentColor::White => "37".to_string(),
        SegmentColor::Gray => "90".to_string(),
        SegmentColor::Palette(name) => {
            if let Some(color) = palette.and_then(|p| p.get_color(name)) {
                ratatui_color_to_ansi(color)
            } else {
                String::new()
            }
        }
    }
}

fn ratatui_color_to_ansi(color: Color) -> String {
    match color {
        Color::Black => "30".to_string(),
        Color::Red => "31".to_string(),
        Color::Green => "32".to_string(),
        Color::Yellow => "33".to_string(),
        Color::Blue => "34".to_string(),
        Color::Magenta => "35".to_string(),
        Color::Cyan => "36".to_string(),
        Color::White => "37".to_string(),
        Color::Gray => "90".to_string(),
        Color::DarkGray => "90".to_string(),
        Color::LightRed => "91".to_string(),
        Color::LightGreen => "92".to_string(),
        Color::LightYellow => "93".to_string(),
        Color::LightBlue => "94".to_string(),
        Color::LightMagenta => "95".to_string(),
        Color::LightCyan => "96".to_string(),
        Color::Rgb(r, g, b) => format!("38;2;{};{};{}", r, g, b),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_severity_error() {
        assert_eq!(
            resolve_severity_style("error"),
            SegmentStyle::Fg(SegmentColor::Red)
        );
    }

    #[test]
    fn test_resolve_severity_case_insensitive() {
        assert_eq!(
            resolve_severity_style("ERROR"),
            resolve_severity_style("error")
        );
    }

    #[test]
    fn test_resolve_severity_unknown() {
        assert_eq!(
            resolve_severity_style("unknown_value"),
            SegmentStyle::Default
        );
    }

    #[test]
    fn test_resolve_status_code_2xx() {
        assert_eq!(
            resolve_status_code_style("200"),
            SegmentStyle::Fg(SegmentColor::Green)
        );
    }

    #[test]
    fn test_resolve_status_code_5xx() {
        assert_eq!(
            resolve_status_code_style("503"),
            SegmentStyle::Fg(SegmentColor::Red)
        );
    }

    #[test]
    fn test_to_ratatui_style_dim() {
        let style = to_ratatui_style(&SegmentStyle::Dim, None);
        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn test_to_ratatui_style_bold() {
        let style = to_ratatui_style(&SegmentStyle::Bold, None);
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }

    #[test]
    fn test_to_ratatui_style_compound() {
        let style = to_ratatui_style(
            &SegmentStyle::Compound {
                dim: true,
                bold: true,
                italic: false,
                fg: Some(SegmentColor::Cyan),
            },
            None,
        );
        assert!(style.add_modifier.contains(Modifier::DIM));
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(!style.add_modifier.contains(Modifier::ITALIC));
        assert_eq!(style.fg, Some(ratatui::style::Color::Cyan));
    }

    #[test]
    fn test_to_ratatui_style_compound_no_fg() {
        let style = to_ratatui_style(
            &SegmentStyle::Compound {
                dim: false,
                bold: true,
                italic: false,
                fg: None,
            },
            None,
        );
        assert!(style.add_modifier.contains(Modifier::BOLD));
        assert!(style.fg.is_none());
    }

    #[test]
    fn test_segments_to_plain_text() {
        let segments = vec![
            StyledSegment {
                text: "ERROR".to_string(),
                style: SegmentStyle::Fg(SegmentColor::Red),
            },
            StyledSegment {
                text: " ".to_string(),
                style: SegmentStyle::Default,
            },
            StyledSegment {
                text: "something failed".to_string(),
                style: SegmentStyle::Dim,
            },
        ];
        assert_eq!(segments_to_plain_text(&segments), "ERROR something failed");
    }

    #[test]
    fn test_segments_to_plain_text_empty() {
        assert_eq!(segments_to_plain_text(&[]), "");
    }

    #[test]
    fn test_segments_to_ansi_basic() {
        let segments = vec![
            StyledSegment {
                text: "ERROR".to_string(),
                style: SegmentStyle::Fg(SegmentColor::Red),
            },
            StyledSegment {
                text: " msg".to_string(),
                style: SegmentStyle::Default,
            },
        ];
        let ansi = segments_to_ansi(&segments, None);
        assert_eq!(ansi, "\x1b[31mERROR\x1b[0m msg");
    }

    #[test]
    fn test_segments_to_ansi_dim() {
        let segments = vec![StyledSegment {
            text: "dim text".to_string(),
            style: SegmentStyle::Dim,
        }];
        let ansi = segments_to_ansi(&segments, None);
        assert!(ansi.contains("\x1b[2m"));
    }

    #[test]
    fn test_segments_to_ansi_compound() {
        let segments = vec![StyledSegment {
            text: "bold cyan".to_string(),
            style: SegmentStyle::Compound {
                dim: false,
                bold: true,
                italic: false,
                fg: Some(SegmentColor::Cyan),
            },
        }];
        let ansi = segments_to_ansi(&segments, None);
        assert!(ansi.contains("\x1b[1;36m"));
    }

    #[test]
    fn test_segments_to_ansi_empty() {
        assert_eq!(segments_to_ansi(&[], None), "");
    }

    #[test]
    fn test_segments_to_ansi_palette_with_palette() {
        let palette = Palette::dark();
        let segments = vec![StyledSegment {
            text: "colored".to_string(),
            style: SegmentStyle::Fg(SegmentColor::Palette("red".to_string())),
        }];
        let ansi = segments_to_ansi(&segments, Some(&palette));
        // dark palette red is Color::Red → ANSI code 31
        assert!(ansi.contains("\x1b[31m"));
    }

    #[test]
    fn test_segments_to_ansi_palette_without_palette() {
        let segments = vec![StyledSegment {
            text: "colored".to_string(),
            style: SegmentStyle::Fg(SegmentColor::Palette("red".to_string())),
        }];
        let ansi = segments_to_ansi(&segments, None);
        // No palette → empty color code, but still has escape sequence wrapper
        assert!(ansi.contains("\x1b[m"));
    }

    #[test]
    fn test_segment_color_to_ratatui_with_palette() {
        let mut palette = Palette::dark();
        palette.red = Color::Rgb(255, 0, 0);
        let color = segment_color_to_ratatui(&SegmentColor::Red, Some(&palette));
        assert_eq!(color, Color::Rgb(255, 0, 0));
    }

    #[test]
    fn test_segment_color_to_ratatui_without_palette() {
        let color = segment_color_to_ratatui(&SegmentColor::Red, None);
        assert_eq!(color, Color::Red);
    }

    #[test]
    fn test_segment_color_palette_variant_ratatui() {
        let palette = Palette::dark();
        let color = segment_color_to_ratatui(
            &SegmentColor::Palette("foreground".to_string()),
            Some(&palette),
        );
        assert_eq!(color, palette.foreground);
    }

    #[test]
    fn test_segment_color_palette_unknown_ratatui() {
        let palette = Palette::dark();
        let color = segment_color_to_ratatui(
            &SegmentColor::Palette("nonexistent".to_string()),
            Some(&palette),
        );
        assert_eq!(color, Color::Reset);
    }
}
