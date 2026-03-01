use ratatui::style::{Modifier, Style};

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
pub fn to_ratatui_style(style: &SegmentStyle) -> Style {
    match style {
        SegmentStyle::Default => Style::default(),
        SegmentStyle::Dim => Style::default().add_modifier(Modifier::DIM),
        SegmentStyle::Bold => Style::default().add_modifier(Modifier::BOLD),
        SegmentStyle::Italic => Style::default().add_modifier(Modifier::ITALIC),
        SegmentStyle::Fg(color) => Style::default().fg(segment_color_to_ratatui(color)),
    }
}

fn segment_color_to_ratatui(color: &SegmentColor) -> ratatui::style::Color {
    match color {
        SegmentColor::Red => ratatui::style::Color::Red,
        SegmentColor::Green => ratatui::style::Color::Green,
        SegmentColor::Yellow => ratatui::style::Color::Yellow,
        SegmentColor::Blue => ratatui::style::Color::Blue,
        SegmentColor::Magenta => ratatui::style::Color::Magenta,
        SegmentColor::Cyan => ratatui::style::Color::Cyan,
        SegmentColor::White => ratatui::style::Color::White,
        SegmentColor::Gray => ratatui::style::Color::Gray,
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
        let style = to_ratatui_style(&SegmentStyle::Dim);
        assert!(style.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn test_to_ratatui_style_bold() {
        let style = to_ratatui_style(&SegmentStyle::Bold);
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }
}
