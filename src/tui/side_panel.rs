use crate::app::{App, InputMode, SourceType, TabState, TreeSelection};
use crate::source::SourceStatus;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

pub(super) fn render_side_panel(
    f: &mut Frame,
    area: Rect,
    app: &App,
) -> Option<(Line<'static>, Rect)> {
    // Stats panel height: 2 (borders) + 1 (line count) + 1 if filtered + 1 if index + severity rows
    let tab = app.active_tab();
    let is_filtered = tab.source.filter.pattern.is_some();
    let has_index = tab.source.index_size.is_some();
    let severity_rows = tab
        .source
        .index_reader
        .as_ref()
        .and_then(|ir| ir.checkpoints().last())
        .map(|cp| {
            let c = &cp.severity_counts;
            [c.fatal, c.error, c.warn, c.info, c.debug, c.trace]
                .iter()
                .filter(|&&v| v > 0)
                .count() as u16
        })
        .unwrap_or(0);
    let stats_height =
        3 + if is_filtered { 1 } else { 0 } + if has_index { 1 } else { 0 } + severity_rows;

    // Split side panel into sources list and stats
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(stats_height)])
        .split(area);

    // Render sources list
    let overflow = render_sources_list(f, chunks[0], app);

    // Render stats panel
    render_stats_panel(f, chunks[1], app);

    overflow
}

/// Build a source line with indicators (loading, filter, follow, status)
fn build_source_line(
    tab: &TabState,
    number: &str,
    indicator: &str,
    name: &str,
    style: Style,
) -> Line<'static> {
    let mut line = Line::from(vec![Span::styled(
        format!("  {}{} {}", number, indicator, name),
        style,
    )]);

    if tab.stream_receiver.is_some() {
        line.spans
            .push(Span::styled(" ⟳", Style::default().fg(Color::Magenta)));
    }
    if tab.source.filter.pattern.is_some() {
        line.spans
            .push(Span::styled(" *", Style::default().fg(Color::Cyan)));
    }
    if tab.source.follow_mode {
        line.spans
            .push(Span::styled(" F", Style::default().fg(Color::Green)));
    }
    if let Some(status) = tab.source.source_status {
        let (status_ind, color) = match status {
            SourceStatus::Active => ("●", Color::Green),
            SourceStatus::Ended => ("○", Color::DarkGray),
        };
        line.spans.push(Span::styled(
            format!(" {}", status_ind),
            Style::default().fg(color),
        ));
    }

    line
}

/// Format metadata string for a source (line count and optional file size)
fn format_source_meta(tab: &TabState) -> String {
    if let Some(size) = tab.source.file_size {
        format!(
            " {} \u{00b7} {}",
            format_count(tab.source.total_lines),
            format_file_size(size)
        )
    } else {
        format!(" {}", format_count(tab.source.total_lines))
    }
}

fn render_sources_list(f: &mut Frame, area: Rect, app: &App) -> Option<(Line<'static>, Rect)> {
    let mut items: Vec<ListItem> = Vec::new();
    let categories = app.tabs_by_category();
    let is_panel_focused = app.input_mode == InputMode::SourcePanel;

    // Track global tab index for numbering and selected item for overflow overlay
    let mut global_idx = 0usize;
    let mut row_idx = 0usize;
    let mut selected_row: Option<usize> = None;
    let mut selected_line_content: Option<Line> = None;

    for (cat, tab_indices) in &categories {
        if tab_indices.is_empty() {
            continue; // Skip empty categories
        }

        // Category header
        let cat_name = match cat {
            SourceType::ProjectSource => "Project Sources",
            SourceType::GlobalSource => "Global Sources",
            SourceType::Global => "Captured",
            SourceType::File => "Files",
            SourceType::Pipe => "Pipes",
        };
        let cat_idx = *cat as usize;
        let expanded = app.source_panel.expanded[cat_idx];
        let arrow = if expanded { "▼" } else { "▶" };

        let is_cat_selected =
            is_panel_focused && app.source_panel.selection == Some(TreeSelection::Category(*cat));

        let cat_style = if is_cat_selected {
            Style::default()
                .bg(Color::DarkGray)
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        };

        items.push(ListItem::new(Line::from(vec![Span::styled(
            format!("{} {}", arrow, cat_name),
            cat_style,
        )])));
        row_idx += 1;

        // Category items (if expanded)
        if expanded {
            for (in_cat_idx, &tab_idx) in tab_indices.iter().enumerate() {
                let tab = &app.tabs[tab_idx];
                let is_active = tab_idx == app.active_tab;
                let is_tree_selected = is_panel_focused
                    && app.source_panel.selection == Some(TreeSelection::Item(*cat, in_cat_idx));

                if is_tree_selected {
                    selected_row = Some(row_idx);
                }

                // Build item display (indented)
                let number = if global_idx < 9 {
                    format!("{}", global_idx + 1)
                } else {
                    " ".to_string()
                };

                let indicator = if is_active { ">" } else { " " };

                // Truncate name to fit in panel width (accounting for indent)
                let max_len = (area.width as usize).saturating_sub(8); // "  N> " + indicators
                let name = if tab.source.name.len() > max_len {
                    let truncate_at = max_len.saturating_sub(3);
                    let boundary = tab.source.name.floor_char_boundary(truncate_at);
                    format!("{}...", &tab.source.name[..boundary])
                } else {
                    tab.source.name.clone()
                };

                let item_style = if tab.source.disabled {
                    // Disabled sources (file doesn't exist) shown grayed out
                    Style::default().fg(Color::DarkGray)
                } else if is_tree_selected {
                    Style::default()
                        .bg(Color::DarkGray)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else if is_active {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                // Build line with indicators and metadata
                let mut line = build_source_line(tab, &number, indicator, &name, item_style);

                // Inline metadata (line count · file size) - show whatever fits
                let meta = format_source_meta(tab);
                let used_width: usize = line.spans.iter().map(|s| s.content.width()).sum();
                let panel_inner = (area.width as usize).saturating_sub(2); // borders
                let remaining = panel_inner.saturating_sub(used_width);
                if remaining > 0 {
                    let truncated: String = meta.chars().take(remaining).collect();
                    line.spans.push(Span::styled(
                        truncated,
                        Style::default().fg(Color::DarkGray),
                    ));
                }

                if is_tree_selected {
                    // Build full untruncated line for overflow overlay
                    let mut full_line =
                        build_source_line(tab, &number, indicator, &tab.source.name, item_style);
                    full_line
                        .spans
                        .push(Span::styled(meta, Style::default().fg(Color::DarkGray)));
                    selected_line_content = Some(full_line);
                }

                items.push(ListItem::new(line));
                row_idx += 1;
                global_idx += 1;
            }
        } else {
            // When collapsed, still count tabs for numbering
            global_idx += tab_indices.len();
        }
    }

    // Title and border styling
    let title = if is_panel_focused {
        " Sources "
    } else {
        "Sources"
    };
    let border_style = if is_panel_focused {
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

    // Return overflow overlay data if the selected line was clipped
    if let (Some(row), Some(mut line_content)) = (selected_row, selected_line_content) {
        let line_width: usize = line_content.spans.iter().map(|s| s.content.width()).sum();
        let panel_inner = (area.width as usize).saturating_sub(2);

        if line_width > panel_inner {
            let row_u16 = u16::try_from(row).unwrap_or(u16::MAX);
            let overlay_y = area.y.saturating_add(1).saturating_add(row_u16);
            let needed_width = u16::try_from(line_width.saturating_add(1)).unwrap_or(u16::MAX);
            let max_width = f.area().width.saturating_sub(area.x + 1);
            let overlay_width = needed_width.min(max_width);

            if overlay_y < area.y + area.height.saturating_sub(1) {
                for span in &mut line_content.spans {
                    span.style = span.style.bg(Color::Black);
                }
                return Some((
                    line_content,
                    Rect {
                        x: area.x + 1,
                        y: overlay_y,
                        width: overlay_width,
                        height: 1,
                    },
                ));
            }
        }
    }

    None
}

fn render_stats_panel(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.active_tab();

    let total_lines = tab.source.total_lines;
    let filtered_lines = tab.source.line_indices.len();
    let is_filtered = tab.source.filter.pattern.is_some();
    let is_loading = tab.stream_receiver.is_some();

    let mut stats_text = Vec::new();

    // Show loading indicator
    if is_loading {
        stats_text.push(Line::from(vec![Span::styled(
            " Loading... ",
            Style::default().fg(Color::Magenta),
        )]));
    }

    // Show line counts
    if is_filtered {
        stats_text.push(Line::from(vec![
            Span::raw(" Lines:    "),
            Span::styled(
                format!("{}", total_lines),
                Style::default().fg(Color::White),
            ),
        ]));
        stats_text.push(Line::from(vec![
            Span::raw(" Filtered: "),
            Span::styled(
                format!("{}", filtered_lines),
                Style::default().fg(Color::Cyan),
            ),
        ]));
    } else {
        stats_text.push(Line::from(vec![
            Span::raw(" Lines: "),
            Span::styled(
                format!("{}", total_lines),
                Style::default().fg(Color::White),
            ),
        ]));
    }

    // Show index size if available
    if let Some(index_size) = tab.source.index_size {
        stats_text.push(Line::from(vec![
            Span::raw(" Index: "),
            Span::styled(
                format_file_size(index_size),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    // Show line ingestion rate
    if let Some(rate) = tab.source.rate_tracker.lines_per_second() {
        if rate > 0.1 {
            let (value, unit) = format_rate(rate);
            stats_text.push(Line::from(vec![
                Span::raw(" Speed: "),
                Span::styled(
                    format!("{:.1} {}", value, unit),
                    Style::default().fg(Color::Green),
                ),
            ]));
        }
    }

    // Show severity bar chart from live flags data
    if let Some(counts) = tab
        .source
        .index_reader
        .as_ref()
        .map(|ir| ir.severity_counts())
    {
        let entries: &[(u32, &str, Color)] = &[
            (counts.fatal, "Fatal", Color::Magenta),
            (counts.error, "Error", Color::Red),
            (counts.warn, "Warn", Color::Yellow),
            (counts.info, "Info", Color::Green),
            (counts.debug, "Debug", Color::Cyan),
            (counts.trace, "Trace", Color::DarkGray),
        ];

        let max_count = entries.iter().map(|&(c, _, _)| c).max().unwrap_or(1).max(1);
        // Available width: panel_width - 2 (borders) - 1 (left pad) - 6 (label) - 1 (space) - 1 (space) - count width
        // Use a fixed bar width that fits comfortably
        let bar_max = 10u32;

        for &(count, label, color) in entries {
            if count > 0 {
                let filled = ((count as u64 * bar_max as u64) / max_count as u64) as usize;
                let filled = filled.max(1); // at least 1 block for non-zero
                let empty = bar_max as usize - filled;
                let bar_filled: String = "\u{2588}".repeat(filled);
                let bar_empty: String = "\u{2591}".repeat(empty);
                let count_str = format_count(count as usize);

                stats_text.push(Line::from(vec![
                    Span::raw(format!(" {:<5} ", label)),
                    Span::styled(bar_filled, Style::default().fg(color)),
                    Span::styled(bar_empty, Style::default().fg(Color::DarkGray)),
                    Span::raw(" "),
                    Span::styled(count_str, Style::default().fg(Color::DarkGray)),
                ]));
            }
        }
    }

    let stats =
        Paragraph::new(stats_text).block(Block::default().borders(Borders::ALL).title("Stats"));

    f.render_widget(stats, area);
}

/// Format a file size in compact human-readable form.
/// Examples: `0B`, `512B`, `45KB`, `2.3MB`, `12MB`, `6GB`
fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;

    if bytes >= GB {
        let val = bytes as f64 / GB as f64;
        if val >= 10.0 {
            format!("{}GB", val as u64)
        } else {
            format!("{:.1}GB", val)
        }
    } else if bytes >= MB {
        let val = bytes as f64 / MB as f64;
        if val >= 10.0 {
            format!("{}MB", val as u64)
        } else {
            format!("{:.1}MB", val)
        }
    } else if bytes >= KB {
        let val = bytes as f64 / KB as f64;
        if val >= 10.0 {
            format!("{}KB", val as u64)
        } else {
            format!("{:.1}KB", val)
        }
    } else {
        format!("{}B", bytes)
    }
}

/// Format a line rate with adaptive units.
/// Returns (value, unit_str) picking the best unit so the value stays readable.
fn format_rate(lines_per_sec: f64) -> (f64, &'static str) {
    if lines_per_sec >= 1.0 {
        (lines_per_sec, "lines/s")
    } else if lines_per_sec * 60.0 >= 1.0 {
        (lines_per_sec * 60.0, "lines/min")
    } else {
        (lines_per_sec * 3600.0, "lines/h")
    }
}

/// Format a line count in compact human-readable form.
/// Examples: `0`, `999`, `1.2K`, `60M`, `1.3B`
pub(super) fn format_count(count: usize) -> String {
    if count >= 1_000_000_000 {
        let val = count as f64 / 1_000_000_000.0;
        if val >= 10.0 {
            format!("{}Bn", val as u64)
        } else {
            format!("{:.1}Bn", val)
        }
    } else if count >= 1_000_000 {
        let val = count as f64 / 1_000_000.0;
        if val >= 10.0 {
            format!("{}M", val as u64)
        } else {
            format!("{:.1}M", val)
        }
    } else if count >= 1_000 {
        let val = count as f64 / 1_000.0;
        if val >= 10.0 {
            format!("{}K", val as u64)
        } else {
            format!("{:.1}K", val)
        }
    } else {
        format!("{}", count)
    }
}
