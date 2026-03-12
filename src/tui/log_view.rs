use crate::app::{App, InputMode, TabState, ViewMode};
use crate::index::flags::Severity;
use crate::index::reader::IndexReader;
use crate::reader::combined_reader::CombinedReader;
use crate::reader::LogReader;
use crate::renderer::segment::{to_ratatui_style, StyledSegment};
use crate::renderer::PresetRegistry;
use crate::text_wrap::{expand_tabs, wrap_content, wrap_plain, wrap_spans};
use crate::theme::UiColors;
use anyhow::Result;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem},
    Frame,
};

#[cfg(test)]
use unicode_width::UnicodeWidthStr;

// Line rendering constants
const LINE_PREFIX_WIDTH: usize = 9; // "{:6} | " = 9 characters
/// Extra prefix width for combined view: "[tag] " before the line number
const MAX_SOURCE_TAG_WIDTH: usize = 8; // e.g. "[api-sv] "

/// Shared rendering state for all lines in a frame.
struct RenderContext<'a> {
    ui: &'a UiColors,
    palette: &'a crate::theme::Palette,
    preset_registry: &'a PresetRegistry,
    tab_renderer_names: Vec<String>,
    tab_filename: Option<String>,
    index_reader: Option<&'a IndexReader>,
    is_combined: bool,
    raw_mode: bool,
    line_wrap: bool,
    prefix_width: usize,
    content_width: usize,
}

/// Per-line metadata resolved before rendering.
struct LineInfo {
    line_number: usize,
    source_tag: Option<(String, Color)>,
    severity: Severity,
    is_selected: bool,
    is_expanded: bool,
}

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
    let adjusted = match style.fg {
        Some(Color::Gray) | Some(Color::DarkGray) | Some(Color::Black) => style.fg(ui.selection_fg),
        _ => style,
    };
    adjusted.bg(ui.selection_bg).add_modifier(Modifier::BOLD)
}

pub(super) fn render_log_view(f: &mut Frame, area: Rect, app: &mut App) -> Result<()> {
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
    let visible_height = area.height.saturating_sub(2) as usize;

    // Layout
    let available_width = area.width.saturating_sub(2) as usize;
    let is_combined = tab.is_combined;
    let prefix_width = if is_combined {
        LINE_PREFIX_WIDTH + MAX_SOURCE_TAG_WIDTH
    } else {
        LINE_PREFIX_WIDTH
    };
    let content_width = available_width.saturating_sub(prefix_width);

    let ctx = RenderContext {
        ui,
        palette,
        preset_registry: &preset_registry,
        tab_renderer_names: tab.source.renderer_names.clone(),
        tab_filename: tab
            .source
            .source_path
            .as_ref()
            .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string())),
        index_reader: tab.source.index_reader.as_ref(),
        is_combined,
        raw_mode: tab.source.raw_mode,
        line_wrap: tab.source.line_wrap,
        prefix_width,
        content_width,
    };

    let mut reader_guard = match tab.source.reader.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    let expanded_lines = tab.expansion.expanded_lines.clone();
    let total_lines = tab.source.line_indices.len();

    // Resolve viewport with visual line heights.
    // For non-wrap mode every line is 1 row; for wrap mode lines may span
    // multiple rows. Viewport::ensure_visible uses these heights so
    // scrolling works correctly in both modes — single code path.
    let mut line_height = |idx: usize| -> usize {
        if let Some(&ln) = tab.source.line_indices.get(idx) {
            let needs_wrap =
                (ctx.line_wrap || expanded_lines.contains(&ln)) && ctx.content_width > 0;
            if needs_wrap {
                let raw = reader_guard.get_line(ln).ok().flatten().unwrap_or_default();
                let text = expand_tabs(&raw);
                if ctx.raw_mode {
                    wrap_plain(&text, ctx.content_width).len()
                } else {
                    wrap_content(&text, ctx.content_width).len()
                }
            } else {
                1
            }
        } else {
            1
        }
    };

    let view = tab.viewport.resolve_with_heights(
        &tab.source.line_indices,
        visible_height,
        &mut line_height,
    );

    tab.scroll_position = view.scroll_position;
    tab.selected_line = view.selected_index;

    let start_idx = view.scroll_position;
    let selected_idx = view.selected_index;

    // Build visible items
    let mut items = Vec::new();
    let mut visual_rows_used = 0usize;

    for i in start_idx..total_lines {
        if let Some(&line_number) = tab.source.line_indices.get(i) {
            let raw_line = reader_guard.get_line(line_number)?.unwrap_or_default();
            let line_text = expand_tabs(&raw_line);
            let is_expanded = expanded_lines.contains(&line_number);

            let info = LineInfo {
                line_number,
                source_tag: resolve_source_tag(
                    line_number,
                    ctx.is_combined,
                    &*reader_guard,
                    ctx.ui,
                ),
                severity: resolve_severity(
                    line_number,
                    ctx.is_combined,
                    ctx.index_reader,
                    &*reader_guard,
                ),
                is_selected: i == selected_idx,
                is_expanded,
            };

            // Content spans — single path for all modes
            let content_spans =
                format_line_spans(&raw_line, &line_text, &info, &ctx, &*reader_guard);

            // Wrap if needed (expanded, line_wrap, or neither → single line)
            let should_wrap = (is_expanded || ctx.line_wrap) && ctx.content_width > 0;
            let wrapped = if should_wrap {
                if is_expanded && ctx.raw_mode {
                    // Expanded + raw: wrap the raw text directly
                    Some(wrap_plain(&line_text, ctx.content_width))
                } else if is_expanded {
                    // Expanded: wrap raw ANSI content
                    Some(wrap_content(&line_text, ctx.content_width))
                } else {
                    // Line-wrap mode: wrap the already-styled spans
                    Some(wrap_spans(content_spans.clone(), ctx.content_width))
                }
            } else {
                None
            };

            let item_height = wrapped.as_ref().map_or(1, |w| w.len());
            if visual_rows_used > 0 && visual_rows_used + item_height > visible_height {
                break;
            }

            let item = build_item(
                wrapped.unwrap_or_else(|| vec![Line::from(content_spans)]),
                &info,
                &ctx,
            );
            items.push(item);
            visual_rows_used += item_height;
        }
    }

    drop(reader_guard);

    // Render widget
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

// ---------------------------------------------------------------------------
// Per-line metadata resolution
// ---------------------------------------------------------------------------

fn resolve_source_tag(
    line_number: usize,
    is_combined: bool,
    reader: &dyn LogReader,
    ui: &UiColors,
) -> Option<(String, Color)> {
    if !is_combined {
        return None;
    }
    let combined = reader.as_any().downcast_ref::<CombinedReader>()?;
    combined
        .source_info(line_number, &ui.source_colors)
        .map(|(name, color)| (name.to_string(), color))
}

fn resolve_severity(
    line_number: usize,
    is_combined: bool,
    index_reader: Option<&IndexReader>,
    reader: &dyn LogReader,
) -> Severity {
    if is_combined {
        reader
            .as_any()
            .downcast_ref::<CombinedReader>()
            .map(|c| c.severity(line_number))
            .unwrap_or(Severity::Unknown)
    } else {
        index_reader
            .map(|ir| ir.severity(line_number))
            .unwrap_or(Severity::Unknown)
    }
}

// ---------------------------------------------------------------------------
// Content formatting — single path for all modes
// ---------------------------------------------------------------------------

/// Format a line's content into styled spans.
/// This is the single entry point for content rendering — used by both
/// single-line and wrapped/expanded paths.
fn format_line_spans(
    raw_line: &str,
    line_text: &str,
    info: &LineInfo,
    ctx: &RenderContext<'_>,
    reader: &dyn LogReader,
) -> Vec<Span<'static>> {
    if ctx.raw_mode {
        return vec![Span::raw(line_text.to_string())];
    }

    let line_flags: Option<u32> = if ctx.is_combined {
        None
    } else {
        ctx.index_reader.and_then(|ir| ir.flags(info.line_number))
    };

    let renderer_names = if ctx.is_combined {
        let combined = reader.as_any().downcast_ref::<CombinedReader>();
        combined
            .map(|c| c.renderer_names(info.line_number))
            .unwrap_or(&[])
    } else {
        &ctx.tab_renderer_names
    };

    let preset_segments: Option<Vec<StyledSegment>> = if !renderer_names.is_empty() {
        ctx.preset_registry
            .render_line(raw_line, renderer_names, line_flags)
    } else {
        ctx.preset_registry
            .render_line_auto(raw_line, ctx.tab_filename.as_deref(), line_flags)
    };

    if let Some(segments) = preset_segments {
        segments
            .iter()
            .map(|seg| {
                Span::styled(
                    seg.text.clone(),
                    to_ratatui_style(&seg.style, Some(ctx.palette)),
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

// ---------------------------------------------------------------------------
// Item building — single path for all lines
// ---------------------------------------------------------------------------

/// Build a ListItem from content lines (1 for single-line, N for wrapped/expanded).
/// Adds prefix (source tag + line number + separator), then applies styling
/// (selection, expanded bg, severity bg).
fn build_item(
    content_lines: Vec<Line<'static>>,
    info: &LineInfo,
    ctx: &RenderContext<'_>,
) -> ListItem<'static> {
    let severity_color = severity_bg(info.severity, ctx.ui);
    let line_num_part = format!("{:6} |", info.line_number + 1);
    let line_sep_part = " ";

    let mut item_lines: Vec<Line<'static>> = Vec::new();

    for (row_idx, mut line) in content_lines.into_iter().enumerate() {
        // Add prefix: first row gets line number, continuation rows get indent
        if row_idx == 0 {
            let num_style = severity_color
                .map(|bg| Style::default().bg(bg))
                .unwrap_or_default();
            line.spans
                .insert(0, Span::styled(line_sep_part, Style::default()));
            line.spans
                .insert(0, Span::styled(line_num_part.clone(), num_style));
            if let Some((ref name, color)) = info.source_tag {
                let tag = format_source_tag(name, MAX_SOURCE_TAG_WIDTH);
                line.spans
                    .insert(0, Span::styled(tag, Style::default().fg(color)));
            }
        } else {
            line.spans
                .insert(0, Span::raw(" ".repeat(ctx.prefix_width)));
        }

        // Apply styling
        if info.is_selected {
            for span in &mut line.spans {
                span.style = apply_selection_style(span.style, ctx.ui);
            }
        } else if info.is_expanded {
            apply_expanded_bg(
                &mut line,
                row_idx,
                info.source_tag.is_some(),
                severity_color,
                ctx.ui,
            );
        } else if row_idx == 0 {
            // Normal/wrap mode: severity bg on line number only
            if let Some(bg) = severity_color {
                let num_idx = if info.source_tag.is_some() { 1 } else { 0 };
                if let Some(num_span) = line.spans.get_mut(num_idx) {
                    num_span.style = num_span.style.bg(bg);
                }
            }
        }

        item_lines.push(line);
    }

    ListItem::new(item_lines)
}

/// Apply expanded background color to non-prefix spans.
fn apply_expanded_bg(
    line: &mut Line<'static>,
    row_idx: usize,
    has_source_tag: bool,
    severity_color: Option<Color>,
    ui: &UiColors,
) {
    // Content spans start after prefix spans
    let prefix_span_count = if row_idx == 0 {
        if has_source_tag {
            3
        } else {
            2
        } // [tag] + num + sep, or num + sep
    } else {
        1 // indent
    };

    for span in line.spans.iter_mut().skip(prefix_span_count) {
        span.style = span.style.bg(ui.expanded_bg);
    }

    if row_idx == 0 {
        // Separator gets expanded bg
        let sep_idx = if has_source_tag { 2 } else { 1 };
        if let Some(sep_span) = line.spans.get_mut(sep_idx) {
            sep_span.style = sep_span.style.bg(ui.expanded_bg);
        }
        // Line number gets expanded bg (unless severity color takes precedence)
        if severity_color.is_none() {
            let num_idx = if has_source_tag { 1 } else { 0 };
            if let Some(num_span) = line.spans.get_mut(num_idx) {
                num_span.style = num_span.style.bg(ui.expanded_bg);
            }
        }
    } else if let Some(indent_span) = line.spans.first_mut() {
        indent_span.style = indent_span.style.bg(ui.expanded_bg);
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
        let content = "\x1b[31mred\x1b[0m";
        let lines = wrap_plain(content, 100);
        let text: String = lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, content);
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
        assert_eq!(lines[0].spans[0].style.fg, Some(Color::Red));
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
