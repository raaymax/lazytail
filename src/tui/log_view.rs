use crate::app::{App, FilterState, InputMode, TabState, ViewMode};
use crate::index::flags::Severity;
use crate::index::reader::IndexReader;
use crate::reader::combined_reader::CombinedReader;
use crate::reader::LogReader;
use crate::renderer::segment::{to_ratatui_style, StyledSegment};
use crate::renderer::PresetRegistry;
use crate::theme::UiColors;
use anyhow::Result;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};
use crate::text_wrap::{expand_tabs, wrap_content, wrap_plain, wrap_spans};

#[cfg(test)]
use unicode_width::UnicodeWidthStr;

// Line rendering constants
const LINE_PREFIX_WIDTH: usize = 9; // "{:6} | " = 9 characters
/// Extra prefix width for combined view: "[tag] " before the line number
const MAX_SOURCE_TAG_WIDTH: usize = 8; // e.g. "[api-sv] "

/// Map severity to a subtle background color for line highlighting.
fn severity_bg(severity: Severity, ui: &UiColors) -> Option<Color> {
    match severity {
        Severity::Warn => Some(ui.severity_warn_bg),
        Severity::Error => Some(ui.severity_error_bg),
        Severity::Fatal => Some(ui.severity_fatal_bg),
        _ => None,
    }
}

/// Apply selection styling to a span (dark bg, bold, adjust dark foreground colors)
fn apply_selection_style(style: Style, ui: &UiColors) -> Style {
    // Adjust foreground if it's too dark to see against selection background
    let adjusted = match style.fg {
        Some(Color::Gray) | Some(Color::DarkGray) | Some(Color::Black) => style.fg(ui.selection_fg),
        _ => style,
    };
    adjusted.bg(ui.selection_bg).add_modifier(Modifier::BOLD)
}

pub(super) fn render_log_view(f: &mut Frame, area: Rect, app: &mut App) -> Result<()> {
    // Clone preset_registry before mutable borrow of app
    let preset_registry = app.preset_registry.clone();

    let ui = &app.theme.ui;
    let palette = &app.theme.palette;
    let tab = if let Some(cat) = app.active_combined {
        app.combined_tabs[cat as usize]
            .as_mut()
            .expect("active_combined set but no combined tab for category")
    } else {
        &mut app.tabs[app.active_tab]
    };
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
    let mut start_idx = view.scroll_position;
    let selected_idx = view.selected_index;

    // Calculate available width for content (accounting for borders and line prefix)
    let available_width = area.width.saturating_sub(2) as usize; // Account for borders
    let is_combined = tab.is_combined;
    let prefix_width = if is_combined {
        LINE_PREFIX_WIDTH + MAX_SOURCE_TAG_WIDTH
    } else {
        LINE_PREFIX_WIDTH
    };
    let content_width = available_width.saturating_sub(prefix_width);

    // Raw mode, line wrap, and preset rendering setup
    let raw_mode = tab.source.raw_mode;
    let line_wrap = tab.source.line_wrap;
    let tab_renderer_names = tab.source.renderer_names.clone();
    let tab_filename = tab
        .source
        .source_path
        .as_ref()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()));

    // Get reader access and collect snapshots for rendering
    let mut reader_guard = match tab.source.reader.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let expanded_lines = tab.expansion.expanded_lines.clone();
    let index_reader = tab.source.index_reader.as_ref();
    let total_lines = tab.source.line_indices.len();

    // When lines can span multiple visual rows (expanded or line_wrap),
    // adjust start_idx so the selected line fits on screen.
    let has_multi_row = line_wrap || !expanded_lines.is_empty();
    if has_multi_row {
        let wrap_fn = if raw_mode { wrap_plain } else { wrap_content };
        let end = selected_idx.min(total_lines.saturating_sub(1));
        // Bound scan: each line takes at least 1 row, so start no earlier than
        // visible_height lines before selected_idx to avoid O(N) scan
        let bounded_start = if end >= visible_height {
            start_idx.max(end - visible_height)
        } else {
            start_idx
        };
        let mut visual_rows = 0usize;
        for i in bounded_start..=end {
            if let Some(&ln) = tab.source.line_indices.get(i) {
                let needs_wrap = (line_wrap || expanded_lines.contains(&ln)) && content_width > 0;
                let h = if needs_wrap {
                    let raw = reader_guard.get_line(ln).ok().flatten().unwrap_or_default();
                    wrap_fn(&expand_tabs(&raw), content_width).len()
                } else {
                    1
                };
                visual_rows += h;
            }
        }
        start_idx = bounded_start;
        while visual_rows > visible_height && start_idx < selected_idx {
            if let Some(&ln) = tab.source.line_indices.get(start_idx) {
                let needs_wrap = (line_wrap || expanded_lines.contains(&ln)) && content_width > 0;
                let h = if needs_wrap {
                    let raw = reader_guard.get_line(ln).ok().flatten().unwrap_or_default();
                    wrap_fn(&expand_tabs(&raw), content_width).len()
                } else {
                    1
                };
                visual_rows -= h;
            }
            start_idx += 1;
        }
    }

    // Fetch the lines to display with visual row budget
    let mut items = Vec::new();
    let mut visual_rows_used = 0usize;
    for i in start_idx..total_lines {
        if let Some(&line_number) = tab.source.line_indices.get(i) {
            let raw_line = reader_guard.get_line(line_number)?.unwrap_or_default();
            let line_text = expand_tabs(&raw_line);
            let is_selected = i == selected_idx;
            let is_expanded = expanded_lines.contains(&line_number);

            // Determine wrapping: expanded lines and line_wrap lines get wrapped
            let should_wrap = (is_expanded || line_wrap) && content_width > 0;

            // Pre-compute wrapped lines and item height
            let (wrapped, use_expanded_bg) = if should_wrap {
                let wrapped_lines = if is_expanded {
                    // Expanded lines: wrap raw/ANSI content (existing behavior)
                    if raw_mode {
                        wrap_plain(&line_text, content_width)
                    } else {
                        wrap_content(&line_text, content_width)
                    }
                } else {
                    // Line-wrap mode: format through presets, then wrap styled spans
                    let spans = format_line_spans(
                        &raw_line,
                        &line_text,
                        line_number,
                        is_combined,
                        raw_mode,
                        &tab_renderer_names,
                        tab_filename.as_deref(),
                        index_reader,
                        &*reader_guard,
                        &preset_registry,
                        palette,
                    );
                    wrap_spans(spans, content_width)
                };
                (Some(wrapped_lines), is_expanded)
            } else {
                (None, false)
            };
            let item_height = wrapped.as_ref().map_or(1, |w| w.len());

            // Break if this item would exceed the visible height (always render first item)
            if visual_rows_used > 0 && visual_rows_used + item_height > visible_height {
                break;
            }

            // Get source tag and severity from CombinedReader or regular index
            let (source_tag, severity) = if is_combined {
                let combined = reader_guard.as_any().downcast_ref::<CombinedReader>();
                let tag = combined.and_then(|c| {
                    c.source_info(line_number, &ui.source_colors)
                        .map(|(name, color)| (name.to_string(), color))
                });
                let sev = combined
                    .map(|c| c.severity(line_number))
                    .unwrap_or(Severity::Unknown);
                (tag, sev)
            } else {
                let sev = index_reader
                    .map(|ir| ir.severity(line_number))
                    .unwrap_or(Severity::Unknown);
                (None, sev)
            };

            // Add line number prefix (split so severity bg stops before separator)
            let line_num_part = format!("{:6} |", line_number + 1);
            let line_sep_part = " ";

            if let Some(wrapped_lines) = wrapped {
                items.push(build_expanded_item(
                    wrapped_lines,
                    &line_num_part,
                    line_sep_part,
                    &source_tag,
                    severity,
                    is_selected,
                    use_expanded_bg,
                    prefix_width,
                    ui,
                ));
            } else {
                items.push(build_single_line_item(
                    &raw_line,
                    &line_text,
                    line_number,
                    &source_tag,
                    severity,
                    is_selected,
                    is_combined,
                    raw_mode,
                    &tab_renderer_names,
                    tab_filename.as_deref(),
                    index_reader,
                    &*reader_guard,
                    &preset_registry,
                    palette,
                    ui,
                ));
            }

            visual_rows_used += item_height;
        }
    }

    drop(reader_guard);

    let title = build_title(tab);
    let is_log_focused = app.input_mode != InputMode::SourcePanel;
    let border_style = if is_log_focused {
        Style::default().fg(ui.primary)
    } else {
        Style::default()
    };

    let list = List::new(items).style(ui.bg_style()).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title)
            .style(ui.bg_style()),
    );

    f.render_widget(list, area);

    Ok(())
}

/// Build a ListItem from wrapped lines with prefix and styling.
/// When `use_expanded_bg` is true, applies the expanded background color
/// to non-selected content spans (used for expanded lines).
/// When false, no special background is applied (used for line-wrap mode).
#[allow(clippy::too_many_arguments)]
fn build_expanded_item(
    wrapped_lines: Vec<Line<'static>>,
    line_num_part: &str,
    line_sep_part: &'static str,
    source_tag: &Option<(String, Color)>,
    severity: Severity,
    is_selected: bool,
    use_expanded_bg: bool,
    prefix_width: usize,
    ui: &UiColors,
) -> ListItem<'static> {
    let mut item_lines: Vec<Line<'static>> = Vec::new();
    let severity_color = severity_bg(severity, ui);

    for (wrap_idx, mut wrapped_line) in wrapped_lines.into_iter().enumerate() {
        if wrap_idx == 0 {
            let num_style = severity_color
                .map(|bg| Style::default().bg(bg))
                .unwrap_or_default();
            wrapped_line
                .spans
                .insert(0, Span::styled(line_sep_part, Style::default()));
            wrapped_line
                .spans
                .insert(0, Span::styled(line_num_part.to_string(), num_style));
            if let Some((ref name, color)) = source_tag {
                let tag = format_source_tag(name, MAX_SOURCE_TAG_WIDTH);
                wrapped_line
                    .spans
                    .insert(0, Span::styled(tag, Style::default().fg(*color)));
            }
        } else {
            wrapped_line
                .spans
                .insert(0, Span::raw(" ".repeat(prefix_width)));
        }

        if is_selected {
            for span in &mut wrapped_line.spans {
                span.style = apply_selection_style(span.style, ui);
            }
        } else if use_expanded_bg {
            let skip = if wrap_idx == 0 {
                if source_tag.is_some() {
                    3
                } else {
                    2
                }
            } else {
                1
            };
            for span in wrapped_line.spans.iter_mut().skip(skip) {
                span.style = span.style.bg(ui.expanded_bg);
            }
            if wrap_idx == 0 {
                let sep_idx = if source_tag.is_some() { 2 } else { 1 };
                if let Some(sep_span) = wrapped_line.spans.get_mut(sep_idx) {
                    sep_span.style = sep_span.style.bg(ui.expanded_bg);
                }
                let num_idx = if source_tag.is_some() { 1 } else { 0 };
                if severity_color.is_none() {
                    if let Some(num_span) = wrapped_line.spans.get_mut(num_idx) {
                        num_span.style = num_span.style.bg(ui.expanded_bg);
                    }
                }
            } else if let Some(indent_span) = wrapped_line.spans.first_mut() {
                indent_span.style = indent_span.style.bg(ui.expanded_bg);
            }
        } else if wrap_idx == 0 {
            // Line-wrap mode (no expanded bg): apply severity bg to line number only
            if let Some(bg) = severity_color {
                let num_idx = if source_tag.is_some() { 1 } else { 0 };
                if let Some(num_span) = wrapped_line.spans.get_mut(num_idx) {
                    num_span.style = num_span.style.bg(bg);
                }
            }
        }

        item_lines.push(wrapped_line);
    }

    ListItem::new(item_lines)
}

/// Build a ListItem for a single (non-expanded) line with content rendering.
#[allow(clippy::too_many_arguments)]
fn build_single_line_item(
    raw_line: &str,
    line_text: &str,
    line_number: usize,
    source_tag: &Option<(String, Color)>,
    severity: Severity,
    is_selected: bool,
    is_combined: bool,
    raw_mode: bool,
    tab_renderer_names: &[String],
    tab_filename: Option<&str>,
    index_reader: Option<&IndexReader>,
    reader: &dyn LogReader,
    preset_registry: &PresetRegistry,
    palette: &crate::theme::Palette,
    ui: &UiColors,
) -> ListItem<'static> {
    let line_num_part = format!("{:6} |", line_number + 1);
    let line_sep_part = " ";
    let mut final_line = Line::default();

    if let Some((ref name, color)) = source_tag {
        let tag = format_source_tag(name, MAX_SOURCE_TAG_WIDTH);
        final_line
            .spans
            .push(Span::styled(tag, Style::default().fg(*color)));
    }

    final_line.spans.push(Span::raw(line_num_part));
    final_line.spans.push(Span::raw(line_sep_part));

    if raw_mode {
        final_line.spans.push(Span::raw(line_text.to_string()));
    } else {
        let line_flags: Option<u32> = if is_combined {
            None
        } else {
            index_reader.and_then(|ir| ir.flags(line_number))
        };

        let renderer_names = if is_combined {
            let combined = reader.as_any().downcast_ref::<CombinedReader>();
            combined
                .map(|c| c.renderer_names(line_number))
                .unwrap_or(&[])
        } else {
            tab_renderer_names
        };

        let preset_segments: Option<Vec<StyledSegment>> = if !renderer_names.is_empty() {
            preset_registry.render_line(raw_line, renderer_names, line_flags)
        } else {
            preset_registry.render_line_auto(raw_line, tab_filename, line_flags)
        };

        if let Some(segments) = preset_segments {
            for seg in &segments {
                final_line.spans.push(Span::styled(
                    seg.text.clone(),
                    to_ratatui_style(&seg.style, Some(palette)),
                ));
            }
        } else {
            let parsed_text = ansi_to_tui::IntoText::into_text(&line_text)
                .unwrap_or_else(|_| ratatui::text::Text::raw(line_text.to_string()));
            if let Some(first_line) = parsed_text.lines.first() {
                for span in &first_line.spans {
                    final_line
                        .spans
                        .push(Span::styled(span.content.to_string(), span.style));
                }
            }
        }
    }

    if is_selected {
        for span in &mut final_line.spans {
            span.style = apply_selection_style(span.style, ui);
        }
    } else if let Some(bg) = severity_bg(severity, ui) {
        let num_idx = if source_tag.is_some() { 1 } else { 0 };
        if let Some(num_span) = final_line.spans.get_mut(num_idx) {
            num_span.style = num_span.style.bg(bg);
        }
    }

    ListItem::new(final_line)
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
        (ViewMode::Aggregation, Some(pattern)) => {
            format!(
                "{}{} (Aggregation: \"{}\")",
                tab.source.name, path_suffix, pattern
            )
        }
        (ViewMode::Aggregation, None) => {
            format!("{}{} (Aggregation)", tab.source.name, path_suffix)
        }
        (ViewMode::Normal, Some(_)) => format!("{}{}", tab.source.name, path_suffix),
    }
}

/// Format a source name into a fixed-width tag like "[api] " or "[web-s..] ".
fn format_source_tag(name: &str, max_width: usize) -> String {
    // Reserve 3 chars for "[ ] " (brackets + space)
    let inner_max = max_width.saturating_sub(3);
    let truncated = if name.len() > inner_max {
        let cut = inner_max.saturating_sub(2);
        let boundary = name.floor_char_boundary(cut);
        format!("{}..", &name[..boundary])
    } else {
        name.to_string()
    };
    format!("[{:<width$}] ", truncated, width = inner_max)
}

/// Format a line through preset rendering and return styled spans.
/// Used by both single-line and line-wrap rendering paths.
#[allow(clippy::too_many_arguments)]
fn format_line_spans(
    raw_line: &str,
    line_text: &str,
    line_number: usize,
    is_combined: bool,
    raw_mode: bool,
    tab_renderer_names: &[String],
    tab_filename: Option<&str>,
    index_reader: Option<&IndexReader>,
    reader: &dyn LogReader,
    preset_registry: &PresetRegistry,
    palette: &crate::theme::Palette,
) -> Vec<Span<'static>> {
    if raw_mode {
        return vec![Span::raw(line_text.to_string())];
    }

    let line_flags: Option<u32> = if is_combined {
        None
    } else {
        index_reader.and_then(|ir| ir.flags(line_number))
    };

    let renderer_names = if is_combined {
        let combined = reader.as_any().downcast_ref::<CombinedReader>();
        combined
            .map(|c| c.renderer_names(line_number))
            .unwrap_or(&[])
    } else {
        tab_renderer_names
    };

    let preset_segments: Option<Vec<StyledSegment>> = if !renderer_names.is_empty() {
        preset_registry.render_line(raw_line, renderer_names, line_flags)
    } else {
        preset_registry.render_line_auto(raw_line, tab_filename, line_flags)
    };

    if let Some(segments) = preset_segments {
        segments
            .iter()
            .map(|seg| {
                Span::styled(
                    seg.text.clone(),
                    to_ratatui_style(&seg.style, Some(palette)),
                )
            })
            .collect()
    } else {
        let parsed_text = ansi_to_tui::IntoText::into_text(&line_text)
            .unwrap_or_else(|_| ratatui::text::Text::raw(line_text.to_string()));
        if let Some(first_line) = parsed_text.lines.first() {
            first_line
                .spans
                .iter()
                .map(|s| Span::styled(s.content.to_string(), s.style))
                .collect()
        } else {
            vec![Span::raw(line_text.to_string())]
        }
    }
}


#[cfg(test)]
mod wrap_content_tests {
    use super::*;

    fn plain_text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn short_content_single_line() {
        let lines = wrap_content("hello", 20);
        assert_eq!(lines.len(), 1);
        assert_eq!(plain_text(&lines), vec!["hello"]);
    }

    #[test]
    fn long_content_wraps_to_multiple_lines() {
        let lines = wrap_content("abcdefghij", 4);
        assert_eq!(lines.len(), 3);
        assert_eq!(plain_text(&lines), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn content_exactly_at_width() {
        let lines = wrap_content("abcd", 4);
        assert_eq!(lines.len(), 1);
        assert_eq!(plain_text(&lines), vec!["abcd"]);
    }

    #[test]
    fn zero_width_returns_default() {
        let lines = wrap_content("hello", 0);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn empty_content_single_line() {
        let lines = wrap_content("", 10);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn wrap_height_matches_line_count() {
        let lines = wrap_content("a]b]c]d]e]f]g]h]i]j]k]l]m]n]o]p", 10);
        assert!(lines.len() > 1);
        for line in &lines {
            let width: usize = line.spans.iter().map(|s| s.content.width()).sum();
            assert!(width <= 10, "line width {} exceeds 10", width);
        }
    }
}

#[cfg(test)]
mod wrap_plain_tests {
    use super::*;

    fn plain_text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn short_content_single_line() {
        let lines = wrap_plain("hello", 20);
        assert_eq!(lines.len(), 1);
        assert_eq!(plain_text(&lines), vec!["hello"]);
    }

    #[test]
    fn long_content_wraps_to_multiple_lines() {
        let lines = wrap_plain("abcdefghij", 4);
        assert_eq!(lines.len(), 3);
        assert_eq!(plain_text(&lines), vec!["abcd", "efgh", "ij"]);
    }

    #[test]
    fn content_exactly_at_width() {
        let lines = wrap_plain("abcd", 4);
        assert_eq!(lines.len(), 1);
        assert_eq!(plain_text(&lines), vec!["abcd"]);
    }

    #[test]
    fn zero_width_returns_default() {
        let lines = wrap_plain("hello", 0);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn preserves_ansi_escape_sequences_literally() {
        // In raw mode, ANSI escape codes should appear as literal characters
        let content = "\x1b[31mred\x1b[0m";
        let lines = wrap_plain(content, 100);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, content);
        // Should be unstyled (no color interpretation)
        assert_eq!(lines[0].spans.len(), 1);
        assert_eq!(lines[0].spans[0].style, Style::default());
    }
}

#[cfg(test)]
mod wrap_spans_tests {
    use super::*;

    fn plain_text(lines: &[Line<'_>]) -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect()
    }

    #[test]
    fn short_spans_single_line() {
        let spans = vec![
            Span::styled("hello", Style::default().fg(Color::Red)),
            Span::raw(" world"),
        ];
        let lines = wrap_spans(spans, 20);
        assert_eq!(lines.len(), 1);
        assert_eq!(plain_text(&lines), vec!["hello world"]);
    }

    #[test]
    fn wraps_styled_spans_across_lines() {
        let spans = vec![
            Span::styled("abcd", Style::default().fg(Color::Red)),
            Span::styled("efgh", Style::default().fg(Color::Blue)),
            Span::raw("ij"),
        ];
        let lines = wrap_spans(spans, 4);
        assert_eq!(lines.len(), 3);
        assert_eq!(plain_text(&lines), vec!["abcd", "efgh", "ij"]);
        // First line should retain red style
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Red));
        // Second line should retain blue style
        assert_eq!(lines[1].spans[0].style.fg, Some(Color::Blue));
    }

    #[test]
    fn splits_single_span_across_lines() {
        let spans = vec![Span::styled(
            "abcdefghij",
            Style::default().fg(Color::Green),
        )];
        let lines = wrap_spans(spans, 4);
        assert_eq!(lines.len(), 3);
        assert_eq!(plain_text(&lines), vec!["abcd", "efgh", "ij"]);
        // All parts should retain the green style
        for line in &lines {
            for span in &line.spans {
                assert_eq!(span.style.fg, Some(Color::Green));
            }
        }
    }

    #[test]
    fn zero_width_returns_default() {
        let spans = vec![Span::raw("hello")];
        let lines = wrap_spans(spans, 0);
        assert_eq!(lines.len(), 1);
    }

    #[test]
    fn empty_spans_single_line() {
        let spans: Vec<Span<'static>> = vec![];
        let lines = wrap_spans(spans, 10);
        assert_eq!(lines.len(), 1);
    }
}
