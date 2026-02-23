use crate::app::{App, FilterState, InputMode, TabState, ViewMode};
use crate::index::flags::Severity;
use anyhow::Result;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use unicode_width::UnicodeWidthStr;

// Line rendering constants
const SELECTED_BG: Color = Color::DarkGray;
const EXPANDED_BG: Color = Color::Rgb(30, 30, 40);
const SEVERITY_WARN_BG: Color = Color::Rgb(50, 40, 0);
const SEVERITY_ERROR_BG: Color = Color::Rgb(55, 10, 10);
const SEVERITY_FATAL_BG: Color = Color::Rgb(75, 0, 15);
const LINE_PREFIX_WIDTH: usize = 9; // "{:6} | " = 9 characters
const TAB_SIZE: usize = 4;

/// Map severity to a subtle background color for line highlighting.
fn severity_bg(severity: Severity) -> Option<Color> {
    match severity {
        Severity::Warn => Some(SEVERITY_WARN_BG),
        Severity::Error => Some(SEVERITY_ERROR_BG),
        Severity::Fatal => Some(SEVERITY_FATAL_BG),
        _ => None,
    }
}

/// Apply selection styling to a span (dark bg, bold, adjust dark foreground colors)
fn apply_selection_style(style: Style) -> Style {
    // Adjust foreground if it's too dark to see against DarkGray background
    let adjusted = match style.fg {
        Some(Color::Gray) | Some(Color::DarkGray) | Some(Color::Black) => style.fg(Color::White),
        _ => style,
    };
    adjusted.bg(SELECTED_BG).add_modifier(Modifier::BOLD)
}

/// Expand tabs to spaces for proper rendering
fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }

    let mut result = String::with_capacity(line.len());
    let mut column = 0;

    for ch in line.chars() {
        if ch == '\t' {
            // Expand to next tab stop
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

pub(super) fn render_log_view(f: &mut Frame, area: Rect, app: &mut App) -> Result<()> {
    let tab = app.active_tab_mut();
    let visible_height = area.height.saturating_sub(2) as usize; // Account for borders

    // During filtering, preserve anchor so selection doesn't jump when partial results arrive
    let is_filtering = matches!(tab.source.filter.state, FilterState::Processing { .. });
    let view =
        tab.viewport
            .resolve_with_options(&tab.source.line_indices, visible_height, is_filtering);

    // Sync old fields from viewport (for backward compatibility during migration)
    tab.scroll_position = view.scroll_position;
    tab.selected_line = view.selected_index;

    // Use the viewport-computed values for rendering
    let start_idx = view.scroll_position;
    let selected_idx = view.selected_index;
    let count = visible_height.min(tab.visible_line_count().saturating_sub(start_idx));

    // Calculate available width for content (accounting for borders and line prefix)
    let available_width = area.width.saturating_sub(2) as usize; // Account for borders
    let prefix_width = LINE_PREFIX_WIDTH;

    // Get reader access and collect snapshots for rendering
    let mut reader_guard = tab.source.reader.lock().unwrap();
    let expanded_lines = tab.expansion.expanded_lines.clone();
    let index_reader = tab.source.index_reader.as_ref();

    // Fetch the lines to display
    let mut items = Vec::new();
    for i in start_idx..start_idx + count {
        if let Some(&line_number) = tab.source.line_indices.get(i) {
            let raw_line = reader_guard.get_line(line_number)?.unwrap_or_default();
            let line_text = expand_tabs(&raw_line);
            let is_selected = i == selected_idx;
            let is_expanded = expanded_lines.contains(&line_number);

            // Add line number prefix (split so severity bg stops before separator)
            let line_num_part = format!("{:6} |", line_number + 1);
            let line_sep_part = " ";

            if is_expanded && available_width > prefix_width {
                // Expanded: wrap content across multiple lines
                let content_width = available_width.saturating_sub(prefix_width);
                let wrapped_lines = wrap_content(&line_text, content_width);

                let mut item_lines: Vec<Line<'static>> = Vec::new();

                let severity_color = index_reader
                    .map(|ir| ir.severity(line_number))
                    .and_then(severity_bg);

                for (wrap_idx, mut wrapped_line) in wrapped_lines.into_iter().enumerate() {
                    if wrap_idx == 0 {
                        // First line: number part with severity bg, then separator
                        let num_style = severity_color
                            .map(|bg| Style::default().bg(bg))
                            .unwrap_or_default();
                        wrapped_line.spans.insert(0, Span::styled(line_sep_part, Style::default()));
                        wrapped_line.spans.insert(0, Span::styled(line_num_part.clone(), num_style));
                    } else {
                        wrapped_line.spans.insert(0, Span::raw(" ".repeat(prefix_width)));
                    }

                    // Apply styling based on selection/expansion state
                    if is_selected {
                        for span in &mut wrapped_line.spans {
                            span.style = apply_selection_style(span.style);
                        }
                    } else {
                        // Expanded but not selected: subtle dark background for content spans
                        // Skip the number span (index 0) and separator span (index 1) on first line
                        let skip = if wrap_idx == 0 { 2 } else { 1 };
                        for span in wrapped_line.spans.iter_mut().skip(skip) {
                            span.style = span.style.bg(EXPANDED_BG);
                        }
                        // Separator gets expanded bg on first line
                        if wrap_idx == 0 {
                            if let Some(sep_span) = wrapped_line.spans.get_mut(1) {
                                sep_span.style = sep_span.style.bg(EXPANDED_BG);
                            }
                            // Number part gets expanded bg only if no severity color
                            if severity_color.is_none() {
                                if let Some(num_span) = wrapped_line.spans.first_mut() {
                                    num_span.style = num_span.style.bg(EXPANDED_BG);
                                }
                            }
                        } else {
                            // Continuation indent gets expanded bg
                            if let Some(indent_span) = wrapped_line.spans.first_mut() {
                                indent_span.style = indent_span.style.bg(EXPANDED_BG);
                            }
                        }
                    }

                    item_lines.push(wrapped_line);
                }

                items.push(ListItem::new(item_lines));
            } else {
                // Not expanded: single line (truncated if too long)
                let parsed_text = ansi_to_tui::IntoText::into_text(&line_text)
                    .unwrap_or_else(|_| ratatui::text::Text::raw(line_text.clone()));

                let mut final_line = Line::default();
                final_line.spans.push(Span::raw(line_num_part.clone()));
                final_line.spans.push(Span::raw(line_sep_part));

                if let Some(first_line) = parsed_text.lines.first() {
                    for span in &first_line.spans {
                        final_line
                            .spans
                            .push(Span::styled(span.content.to_string(), span.style));
                    }
                }

                // Apply line styling: selection takes priority, then severity on number only
                if is_selected {
                    for span in &mut final_line.spans {
                        span.style = apply_selection_style(span.style);
                    }
                } else if let Some(bg) = index_reader
                    .map(|ir| ir.severity(line_number))
                    .and_then(severity_bg)
                {
                    // Color only the line number background, not the separator or content
                    if let Some(num_span) = final_line.spans.first_mut() {
                        num_span.style = num_span.style.bg(bg);
                    }
                }

                items.push(ListItem::new(final_line));
            }
        }
    }

    drop(reader_guard);

    let title = build_title(tab);
    let is_log_focused = app.input_mode != InputMode::SourcePanel;
    let border_style = if is_log_focused {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title),
    );

    f.render_widget(list, area);

    Ok(())
}

fn build_title(tab: &TabState) -> String {
    let path_suffix = tab
        .source
        .source_path
        .as_ref()
        .map(|p| format!(" — {}", p.display()))
        .unwrap_or_default();

    match (&tab.source.mode, &tab.source.filter.pattern) {
        (ViewMode::Normal, None) => format!("{}{}", tab.source.name, path_suffix),
        (ViewMode::Filtered, Some(pattern)) => {
            format!(
                "{}{} (Filter: \"{}\")",
                tab.source.name, path_suffix, pattern
            )
        }
        (ViewMode::Filtered, None) => format!("{}{} (Filtered)", tab.source.name, path_suffix),
        (ViewMode::Normal, Some(_)) => format!("{}{}", tab.source.name, path_suffix),
    }
}

/// Wrap content to fit within the available width, preserving ANSI styles.
/// Returns a vector of Lines, where each Line contains styled spans.
fn wrap_content(content: &str, available_width: usize) -> Vec<Line<'static>> {
    if available_width == 0 {
        return vec![Line::default()];
    }

    // Parse ANSI codes and convert to ratatui Text with styles
    let parsed_text = ansi_to_tui::IntoText::into_text(&content)
        .unwrap_or_else(|_| ratatui::text::Text::raw(content.to_string()));

    // Get the first line's spans (we only deal with single-line content here)
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

    // If content fits on one line, return as-is
    let total_width: usize = spans.iter().map(|s| s.content.width()).sum();
    if total_width <= available_width {
        return vec![Line::from(spans)];
    }

    // Word wrap the spans
    let mut result_lines: Vec<Line<'static>> = Vec::new();
    let mut current_line_spans: Vec<Span<'static>> = Vec::new();
    let mut current_line_width = 0;

    for span in spans {
        let span_text = span.content.to_string();
        let span_style = span.style;

        // For each span, we may need to split it across multiple lines
        let mut remaining = span_text.as_str();

        while !remaining.is_empty() {
            let remaining_width = remaining.width();
            let line_width_limit = if result_lines.is_empty() && current_line_spans.is_empty() {
                available_width
            } else if current_line_spans.is_empty() {
                // For continuation lines, account for indent
                available_width
            } else {
                available_width.saturating_sub(current_line_width)
            };

            if remaining_width <= line_width_limit {
                // The rest fits on this line
                current_line_spans.push(Span::styled(remaining.to_string(), span_style));
                current_line_width += remaining_width;
                break;
            } else {
                // Need to split - find where to break
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
                    // Can't fit even one character - force at least one
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

                // Commit current line and start new one
                if !current_line_spans.is_empty() {
                    result_lines.push(Line::from(current_line_spans));
                    current_line_spans = Vec::new();
                    current_line_width = 0;
                }
            }
        }
    }

    // Don't forget the last line
    if !current_line_spans.is_empty() {
        result_lines.push(Line::from(current_line_spans));
    }

    if result_lines.is_empty() {
        vec![Line::default()]
    } else {
        result_lines
    }
}
