use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

/// Wrap plain text without ANSI interpretation (for raw mode).
pub fn wrap_plain(content: &str, available_width: usize) -> Vec<Line<'static>> {
    if available_width == 0 {
        return vec![Line::default()];
    }

    let total_width: usize = content.width();
    if total_width <= available_width {
        return vec![Line::from(Span::raw(content.to_string()))];
    }

    let mut result_lines: Vec<Line<'static>> = Vec::new();
    let mut remaining = content;

    while !remaining.is_empty() {
        let mut break_pos = 0;
        let mut break_width = 0;

        for (idx, ch) in remaining.char_indices() {
            let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
            if break_width + ch_width > available_width {
                break;
            }
            break_width += ch_width;
            break_pos = idx + ch.len_utf8();
        }

        if break_pos == 0 {
            if let Some(ch) = remaining.chars().next() {
                break_pos = ch.len_utf8();
            } else {
                break;
            }
        }

        let (part, rest) = remaining.split_at(break_pos);
        result_lines.push(Line::from(Span::raw(part.to_string())));
        remaining = rest;
    }

    if result_lines.is_empty() {
        vec![Line::default()]
    } else {
        result_lines
    }
}

/// Wrap content to fit within the available width, preserving ANSI styles.
pub fn wrap_content(content: &str, available_width: usize) -> Vec<Line<'static>> {
    if available_width == 0 {
        return vec![Line::default()];
    }

    let parsed_text = ansi_to_tui::IntoText::into_text(&content)
        .unwrap_or_else(|_| ratatui::text::Text::raw(content.to_string()));

    let spans: Vec<Span<'static>> = parsed_text
        .lines
        .first()
        .map(|line| {
            line.spans
                .iter()
                .map(|s| Span::styled(s.content.to_string(), s.style))
                .collect()
        })
        .unwrap_or_default();

    wrap_spans(spans, available_width)
}

/// Wrap pre-styled spans to fit within the available width.
pub fn wrap_spans(spans: Vec<Span<'static>>, available_width: usize) -> Vec<Line<'static>> {
    if available_width == 0 {
        return vec![Line::default()];
    }

    let total_width: usize = spans.iter().map(|s| s.content.width()).sum();
    if total_width <= available_width {
        return vec![Line::from(spans)];
    }

    let mut result_lines: Vec<Line<'static>> = Vec::new();
    let mut current_line_spans: Vec<Span<'static>> = Vec::new();
    let mut current_line_width = 0;

    for span in spans {
        let span_text = span.content.to_string();
        let span_style = span.style;

        let mut remaining = span_text.as_str();

        while !remaining.is_empty() {
            let remaining_width = remaining.width();
            let line_width_limit = if current_line_spans.is_empty() {
                available_width
            } else {
                available_width.saturating_sub(current_line_width)
            };

            if remaining_width <= line_width_limit {
                current_line_spans.push(Span::styled(remaining.to_string(), span_style));
                current_line_width += remaining_width;
                break;
            } else {
                let mut break_pos = 0;
                let mut break_width = 0;

                for (idx, ch) in remaining.char_indices() {
                    let ch_width = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if break_width + ch_width > line_width_limit {
                        break;
                    }
                    break_width += ch_width;
                    break_pos = idx + ch.len_utf8();
                }

                if break_pos == 0 && current_line_spans.is_empty() {
                    if let Some(ch) = remaining.chars().next() {
                        break_pos = ch.len_utf8();
                    } else {
                        break;
                    }
                }

                if break_pos > 0 {
                    let (part, rest) = remaining.split_at(break_pos);
                    if !part.is_empty() {
                        current_line_spans.push(Span::styled(part.to_string(), span_style));
                    }
                    remaining = rest;
                }

                if !current_line_spans.is_empty() {
                    result_lines.push(Line::from(current_line_spans));
                    current_line_spans = Vec::new();
                    current_line_width = 0;
                }
            }
        }
    }

    if !current_line_spans.is_empty() {
        result_lines.push(Line::from(current_line_spans));
    }

    if result_lines.is_empty() {
        vec![Line::default()]
    } else {
        result_lines
    }
}

/// Expand tabs to spaces for proper rendering.
pub fn expand_tabs(line: &str) -> String {
    const TAB_SIZE: usize = 4;

    if !line.contains('\t') {
        return line.to_string();
    }

    let mut result = String::with_capacity(line.len());
    let mut column = 0;

    for ch in line.chars() {
        if ch == '\t' {
            let spaces = TAB_SIZE - (column % TAB_SIZE);
            for _ in 0..spaces {
                result.push(' ');
            }
            column += spaces;
        } else {
            result.push(ch);
            column += 1;
        }
    }

    result
}
