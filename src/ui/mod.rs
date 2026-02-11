use crate::app::{App, FilterState, InputMode, SourceType, TreeSelection, ViewMode};
use crate::source::SourceStatus;
use crate::tab::TabState;
use anyhow::Result;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};
use unicode_width::UnicodeWidthStr;

// UI Style Constants
const SELECTED_BG: Color = Color::DarkGray;
const EXPANDED_BG: Color = Color::Rgb(30, 30, 40);
const LINE_PREFIX_WIDTH: usize = 9; // "{:6} | " = 9 characters
const TAB_SIZE: usize = 4;

// Help overlay dimensions (as percentage of screen)
const HELP_POPUP_WIDTH_PERCENT: f32 = 0.6;
const HELP_POPUP_HEIGHT_PERCENT: f32 = 0.8;

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

pub fn render(f: &mut Frame, app: &mut App) -> Result<()> {
    // Main horizontal layout: side panel + content area
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(app.side_panel_width), Constraint::Min(1)])
        .split(f.area());

    // Render side panel with tabs
    render_side_panel(f, main_chunks[0], app);

    // Content area layout
    let content_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),    // Main content
            Constraint::Length(4), // Status bar (2 lines + borders)
            Constraint::Length(if app.is_entering_filter() || app.is_entering_line_jump() {
                3
            } else {
                0
            }), // Input prompt
        ])
        .split(main_chunks[1]);

    render_log_view(f, content_chunks[0], app)?;
    render_status_bar(f, content_chunks[1], app);

    if app.is_entering_filter() {
        render_filter_input_prompt(f, content_chunks[2], app);
    } else if app.is_entering_line_jump() {
        render_line_jump_prompt(f, content_chunks[2], app);
    }

    // Render help overlay on top of everything if active
    if app.show_help {
        render_help_overlay(f, f.area());
    }

    // Render close confirmation dialog on top of everything if active
    if app.input_mode == InputMode::ConfirmClose {
        render_confirm_close_dialog(f, f.area(), app);
    }

    Ok(())
}

fn render_side_panel(f: &mut Frame, area: Rect, app: &App) {
    // Split side panel into sources list and stats
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(5)])
        .split(area);

    // Render sources list
    render_sources_list(f, chunks[0], app);

    // Render stats panel
    render_stats_panel(f, chunks[1], app);
}

fn render_sources_list(f: &mut Frame, area: Rect, app: &App) {
    let mut items: Vec<ListItem> = Vec::new();
    let categories = app.tabs_by_category();
    let is_panel_focused = app.input_mode == InputMode::SourcePanel;

    // Track global tab index for numbering
    let mut global_idx = 0usize;

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

        // Category items (if expanded)
        if expanded {
            for (in_cat_idx, &tab_idx) in tab_indices.iter().enumerate() {
                let tab = &app.tabs[tab_idx];
                let is_active = tab_idx == app.active_tab;
                let is_tree_selected = is_panel_focused
                    && app.source_panel.selection == Some(TreeSelection::Item(*cat, in_cat_idx));

                // Build item display (indented)
                let number = if global_idx < 9 {
                    format!("{}", global_idx + 1)
                } else {
                    " ".to_string()
                };
                let indicator = if is_active { ">" } else { " " };

                // Truncate name to fit in panel width (accounting for indent)
                let max_len = (area.width as usize).saturating_sub(8); // "  N> " + indicators
                let name = if tab.name.len() > max_len {
                    let truncate_at = max_len.saturating_sub(3);
                    let boundary = tab.name.floor_char_boundary(truncate_at);
                    format!("{}...", &tab.name[..boundary])
                } else {
                    tab.name.clone()
                };

                let item_style = if tab.disabled {
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

                let mut line = Line::from(vec![Span::styled(
                    format!("  {}{} {}", number, indicator, name),
                    item_style,
                )]);

                // Add loading indicator if stream is still loading
                if tab.stream_receiver.is_some() {
                    line.spans
                        .push(Span::styled(" ⟳", Style::default().fg(Color::Magenta)));
                }

                // Add filter indicator if tab has active filter
                if tab.filter.pattern.is_some() {
                    line.spans
                        .push(Span::styled(" *", Style::default().fg(Color::Cyan)));
                }

                // Add follow indicator if tab is in follow mode
                if tab.follow_mode {
                    line.spans
                        .push(Span::styled(" F", Style::default().fg(Color::Green)));
                }

                // Add source status indicator for discovered sources
                if let Some(status) = tab.source_status {
                    let (status_ind, color) = match status {
                        SourceStatus::Active => ("●", Color::Green),
                        SourceStatus::Ended => ("○", Color::DarkGray),
                    };
                    line.spans.push(Span::styled(
                        format!(" {}", status_ind),
                        Style::default().fg(color),
                    ));
                }

                items.push(ListItem::new(line));
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
}

fn render_stats_panel(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.active_tab();

    let total_lines = tab.total_lines;
    let filtered_lines = tab.line_indices.len();
    let is_filtered = tab.filter.pattern.is_some();
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

    let stats =
        Paragraph::new(stats_text).block(Block::default().borders(Borders::ALL).title("Stats"));

    f.render_widget(stats, area);
}

fn render_log_view(f: &mut Frame, area: Rect, app: &mut App) -> Result<()> {
    let tab = app.active_tab_mut();
    let visible_height = area.height.saturating_sub(2) as usize; // Account for borders

    // During filtering, preserve anchor so selection doesn't jump when partial results arrive
    let is_filtering = matches!(tab.filter.state, FilterState::Processing { .. });
    let view = tab
        .viewport
        .resolve_with_options(&tab.line_indices, visible_height, is_filtering);

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

    // Get reader access and collect expanded_lines snapshot
    let mut reader_guard = tab.reader.lock().unwrap();
    let expanded_lines = tab.expansion.expanded_lines.clone();

    // Fetch the lines to display
    let mut items = Vec::new();
    for i in start_idx..start_idx + count {
        if let Some(&line_number) = tab.line_indices.get(i) {
            let raw_line = reader_guard.get_line(line_number)?.unwrap_or_default();
            let line_text = expand_tabs(&raw_line);
            let is_selected = i == selected_idx;
            let is_expanded = expanded_lines.contains(&line_number);

            // Add line number prefix
            let line_prefix = format!("{:6} | ", line_number + 1);

            if is_expanded && available_width > prefix_width {
                // Expanded: wrap content across multiple lines
                let content_width = available_width.saturating_sub(prefix_width);
                let wrapped_lines = wrap_content(&line_text, content_width);

                let mut item_lines: Vec<Line<'static>> = Vec::new();

                for (wrap_idx, mut wrapped_line) in wrapped_lines.into_iter().enumerate() {
                    // First line gets the line number prefix, others get indent
                    let prefix = if wrap_idx == 0 {
                        line_prefix.clone()
                    } else {
                        " ".repeat(prefix_width)
                    };

                    // Insert prefix at the beginning
                    wrapped_line.spans.insert(0, Span::raw(prefix));

                    // Apply styling based on selection/expansion state
                    for span in &mut wrapped_line.spans {
                        if is_selected {
                            span.style = apply_selection_style(span.style);
                        } else {
                            // Expanded but not selected: subtle dark background
                            span.style = span.style.bg(EXPANDED_BG);
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
                final_line.spans.push(Span::raw(line_prefix));

                if let Some(first_line) = parsed_text.lines.first() {
                    for span in &first_line.spans {
                        final_line
                            .spans
                            .push(Span::styled(span.content.to_string(), span.style));
                    }
                }

                // Apply selection background if this is the selected line
                if is_selected {
                    for span in &mut final_line.spans {
                        span.style = apply_selection_style(span.style);
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
    match (&tab.mode, &tab.filter.pattern) {
        (ViewMode::Normal, None) => tab.name.clone(),
        (ViewMode::Filtered, Some(pattern)) => format!("{} (Filter: \"{}\")", tab.name, pattern),
        (ViewMode::Filtered, None) => format!("{} (Filtered)", tab.name),
        (ViewMode::Normal, Some(_)) => tab.name.clone(),
    }
}

fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.active_tab();

    let status_text = format!(
        " Line {}/{} | Total: {} | Mode: {} {}{}",
        tab.selected_line + 1,
        tab.visible_line_count(),
        tab.total_lines,
        match tab.mode {
            ViewMode::Normal => "Normal",
            ViewMode::Filtered => "Filtered",
        },
        match &tab.filter.state {
            FilterState::Inactive => String::new(),
            FilterState::Processing { lines_processed } => {
                let percent = if tab.total_lines > 0 {
                    (lines_processed * 100) / tab.total_lines
                } else {
                    0
                };
                format!("| Filtering: {}%", percent)
            }
            FilterState::Complete { matches } => format!("| Matches: {}", matches),
        },
        if tab.follow_mode { " | FOLLOW" } else { "" }
    );

    let help_text = if app.tab_count() > 1 {
        " Tab/Shift+Tab - Switch | 1-9 - Select | ? - Help"
    } else {
        " q - Quit | j/k - Navigate | g/G - Start/End | / - Filter | ? - Help"
    };

    let status_lines = vec![
        Line::from(vec![Span::styled(
            status_text,
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            help_text,
            Style::default().fg(Color::DarkGray),
        )]),
    ];

    let paragraph =
        Paragraph::new(status_lines).block(Block::default().borders(Borders::ALL).title("Status"));

    f.render_widget(paragraph, area);
}

fn render_filter_input_prompt(f: &mut Frame, area: Rect, app: &App) {
    let input = app.get_input();

    // Detect if this is query syntax for display purposes
    let is_query = crate::filter::query::is_query_syntax(input);

    let label = if is_query {
        "Query"
    } else {
        app.current_filter_mode.prompt_label()
    };
    let input_text = format!("{}: {}", label, input);

    // Determine border color based on mode and validation state
    let border_color = if app.query_error.is_some() {
        Color::Red // Invalid query
    } else if is_query {
        Color::Magenta // Valid query mode
    } else if app.current_filter_mode.is_regex() {
        if app.regex_error.is_some() {
            Color::Red // Invalid regex
        } else {
            Color::Cyan // Valid regex mode
        }
    } else {
        Color::White // Plain text mode
    };

    // Build help text with mode hint
    let mode_hint = if is_query {
        "Query"
    } else if app.current_filter_mode.is_regex() {
        "Tab: Plain"
    } else {
        "Tab: Regex"
    };
    let title = format!("Live Filter ({}, Enter to close, Esc to clear)", mode_hint);

    let input = Paragraph::new(input_text)
        .style(Style::default().fg(Color::Yellow))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .title(title),
        );

    f.render_widget(input, area);

    // Show cursor at the cursor position (label + ": " + chars before cursor)
    // Count characters before cursor, not bytes (for proper Unicode support)
    let cursor_offset = label.len() as u16 + 2; // +2 for ": "
    let chars_before_cursor = app.get_input()[..app.get_cursor_position()].chars().count() as u16;
    f.set_cursor_position((area.x + 1 + cursor_offset + chars_before_cursor, area.y + 1));
}

fn render_line_jump_prompt(f: &mut Frame, area: Rect, app: &App) {
    let input_text = format!(":{}", app.get_input());

    let input = Paragraph::new(input_text)
        .style(Style::default().fg(Color::Cyan))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Jump to Line (Enter to jump, Esc to cancel)"),
        );

    f.render_widget(input, area);

    // Show cursor at the cursor position (: + chars before cursor)
    let chars_before_cursor = app.get_input()[..app.get_cursor_position()].chars().count() as u16;
    f.set_cursor_position((area.x + 2 + chars_before_cursor, area.y + 1));
}

/// Wrap content to fit within the available width, preserving ANSI styles
/// Returns a vector of Lines, where each Line contains styled spans
///
/// # Arguments
/// * `content` - The raw content string (may contain ANSI codes)
/// * `available_width` - The width in columns to wrap to
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
    // Note: We don't add continuation indent here - the caller (render_log_view) handles prefixes
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

fn render_help_overlay(f: &mut Frame, area: Rect) {
    // Calculate centered popup area
    let popup_width = (area.width as f32 * HELP_POPUP_WIDTH_PERCENT) as u16;
    let popup_height = (area.height as f32 * HELP_POPUP_HEIGHT_PERCENT) as u16;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: area.x + popup_x,
        y: area.y + popup_y,
        width: popup_width,
        height: popup_height,
    };

    // Help content
    let help_lines = vec![
        Line::from(vec![Span::styled(
            "LazyTail - Quick Reference",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Navigation",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  j/k, ↑/↓      Move selection up/down"),
        Line::from("  g / G         Jump to start / end"),
        Line::from("  PageUp/Down   Scroll by page"),
        Line::from("  Ctrl+E/Y      Scroll viewport (vim-style)"),
        Line::from("  :123          Jump to line number"),
        Line::from("  zz/zt/zb      Center/top/bottom view"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Filtering",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  /             Start filter (live preview)"),
        Line::from("  Tab           Toggle Plain ↔ Regex mode"),
        Line::from("  Alt+C         Toggle case sensitivity"),
        Line::from("  ↑/↓           Browse filter history"),
        Line::from("  Enter         Apply filter"),
        Line::from("  Esc           Clear filter"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Tabs",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  1-9           Jump to tab"),
        Line::from("  x, Ctrl+W     Close tab"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Source Panel",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Tab           Toggle panel focus"),
        Line::from("  j/k, ↑/↓      Navigate tree"),
        Line::from("  Space         Expand/collapse category"),
        Line::from("  Enter         Select source"),
        Line::from("  x             Close selected source"),
        Line::from("  Esc           Return to log view"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "View",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Space         Expand/collapse line"),
        Line::from("  c             Collapse all"),
        Line::from("  f             Toggle follow mode"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Side Panel Indicators",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("F", Style::default().fg(Color::Green)),
            Span::raw("  Follow mode    "),
            Span::styled("*", Style::default().fg(Color::Cyan)),
            Span::raw("  Filter active"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("●", Style::default().fg(Color::Green)),
            Span::raw("  Source active  "),
            Span::styled("○", Style::default().fg(Color::DarkGray)),
            Span::raw("  Source ended"),
        ]),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("⟳", Style::default().fg(Color::Magenta)),
            Span::raw("  Loading"),
        ]),
        Line::from(""),
        Line::from("  q / Ctrl+C    Quit"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press any key to close",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::ITALIC),
        )]),
    ];

    let help_paragraph = Paragraph::new(help_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Help ")
                .style(Style::default().bg(Color::Black)),
        )
        .style(Style::default().bg(Color::Black).fg(Color::White));

    // Clear the area first to remove background content
    f.render_widget(Clear, popup_area);
    f.render_widget(help_paragraph, popup_area);
}

fn render_confirm_close_dialog(f: &mut Frame, area: Rect, app: &App) {
    let tab_index = match &app.pending_close_tab {
        Some((idx, name)) if *idx < app.tabs.len() && app.tabs[*idx].name == *name => *idx,
        _ => return,
    };

    let tab = &app.tabs[tab_index];
    let tab_name = &tab.name;
    let is_last = app.tabs.len() <= 1;
    let will_delete = tab.source_status == Some(SourceStatus::Ended) && tab.source_path.is_some();

    // Truncate name to fit in popup
    let max_name_len = 30;
    let display_name = if tab_name.len() > max_name_len {
        let truncate_at = max_name_len.saturating_sub(3);
        let boundary = tab_name.floor_char_boundary(truncate_at);
        format!("{}...", &tab_name[..boundary])
    } else {
        tab_name.clone()
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Close "),
            Span::styled(
                &display_name,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw("?"),
        ]),
    ];

    // Add context note
    if will_delete {
        lines.push(Line::from(vec![Span::styled(
            "  Source file will be deleted",
            Style::default().fg(Color::Red),
        )]));
    } else if is_last {
        lines.push(Line::from(vec![Span::styled(
            "  This will quit the application",
            Style::default().fg(Color::DarkGray),
        )]));
    } else {
        lines.push(Line::from(""));
    }

    lines.push(Line::from(vec![
        Span::raw("  "),
        Span::styled("y/Enter", Style::default().fg(Color::Green)),
        Span::raw(" confirm  "),
        Span::styled("n/Esc", Style::default().fg(Color::Red)),
        Span::raw(" cancel"),
    ]));

    let popup_width = 44.min(area.width.saturating_sub(4));
    let popup_height = 6;
    let popup_x = (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect {
        x: area.x + popup_x,
        y: area.y + popup_y,
        width: popup_width,
        height: popup_height,
    };

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Close Source ")
                .style(Style::default().bg(Color::Black)),
        )
        .style(Style::default().bg(Color::Black).fg(Color::White));

    f.render_widget(Clear, popup_area);
    f.render_widget(paragraph, popup_area);
}
