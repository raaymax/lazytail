use crate::app::{App, FilterState, ViewMode};
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

/// Expand tabs to spaces for proper rendering
fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }

    let mut result = String::with_capacity(line.len());
    let mut column = 0;

    for ch in line.chars() {
        if ch == '\t' {
            // Expand to next 4-column tab stop
            let spaces = 4 - (column % 4);
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

    for (idx, tab) in app.tabs.iter().enumerate() {
        let is_active = idx == app.active_tab;

        // Build the display string
        let number_prefix = if idx < 9 {
            format!("{} ", idx + 1)
        } else {
            "  ".to_string()
        };

        // Truncate name to fit in panel width
        let max_name_len = (area.width as usize).saturating_sub(4); // "N > " prefix
        let display_name = if tab.name.len() > max_name_len {
            format!("{}...", &tab.name[..max_name_len.saturating_sub(3)])
        } else {
            tab.name.clone()
        };

        let indicator = if is_active { "> " } else { "  " };

        let line_text = format!("{}{}{}", number_prefix, indicator, display_name);

        let style = if is_active {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        };

        let mut line = Line::from(vec![Span::styled(line_text, style)]);

        // Add filter indicator if tab has active filter
        if tab.filter_pattern.is_some() {
            line.spans
                .push(Span::styled(" *", Style::default().fg(Color::Cyan)));
        }

        // Add follow indicator if tab is in follow mode
        if tab.follow_mode {
            line.spans
                .push(Span::styled(" F", Style::default().fg(Color::Green)));
        }

        items.push(ListItem::new(line));
    }

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Sources"));

    f.render_widget(list, area);
}

fn render_stats_panel(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.active_tab();

    let total_lines = tab.total_lines;
    let filtered_lines = tab.line_indices.len();
    let is_filtered = tab.filter_pattern.is_some();

    let stats_text = if is_filtered {
        vec![
            Line::from(vec![
                Span::raw(" Lines:    "),
                Span::styled(
                    format!("{}", total_lines),
                    Style::default().fg(Color::White),
                ),
            ]),
            Line::from(vec![
                Span::raw(" Filtered: "),
                Span::styled(
                    format!("{}", filtered_lines),
                    Style::default().fg(Color::Cyan),
                ),
            ]),
        ]
    } else {
        vec![Line::from(vec![
            Span::raw(" Lines: "),
            Span::styled(
                format!("{}", total_lines),
                Style::default().fg(Color::White),
            ),
        ])]
    };

    let stats =
        Paragraph::new(stats_text).block(Block::default().borders(Borders::ALL).title("Stats"));

    f.render_widget(stats, area);
}

fn render_log_view(f: &mut Frame, area: Rect, app: &mut App) -> Result<()> {
    let tab = app.active_tab_mut();
    let visible_height = area.height.saturating_sub(2) as usize; // Account for borders

    // Use viewport to compute scroll position and selected index
    let view = tab.viewport.resolve(&tab.line_indices, visible_height);

    // Sync old fields from viewport (for backward compatibility during migration)
    tab.scroll_position = view.scroll_position;
    tab.selected_line = view.selected_index;

    // Use the viewport-computed values for rendering
    let start_idx = view.scroll_position;
    let selected_idx = view.selected_index;
    let count = visible_height.min(tab.visible_line_count().saturating_sub(start_idx));

    // Calculate available width for content (accounting for borders and line prefix)
    let available_width = area.width.saturating_sub(2) as usize; // Account for borders
    let prefix_width = 9; // "{:6} | " = 9 characters

    // Background color for expanded (non-selected) entries
    let expanded_bg = Color::Rgb(30, 30, 40);

    // Get reader access and collect expanded_lines snapshot
    let mut reader_guard = tab.reader.lock().unwrap();
    let expanded_lines = tab.expanded_lines.clone();

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
                let wrapped_lines = wrap_content(&line_text, content_width, prefix_width);

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
                            // Selected: dark gray background + bold
                            let new_style = match span.style.fg {
                                Some(Color::Gray) | Some(Color::DarkGray) | Some(Color::Black) => {
                                    span.style.fg(Color::White)
                                }
                                _ => span.style,
                            };
                            span.style = new_style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
                        } else {
                            // Expanded but not selected: subtle dark background
                            span.style = span.style.bg(expanded_bg);
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
                        let new_style = match span.style.fg {
                            Some(Color::Gray) | Some(Color::DarkGray) | Some(Color::Black) => {
                                span.style.fg(Color::White)
                            }
                            _ => span.style,
                        };
                        span.style = new_style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
                    }
                }

                items.push(ListItem::new(final_line));
            }
        }
    }

    drop(reader_guard);

    let title = build_title(tab);

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

    f.render_widget(list, area);

    Ok(())
}

fn build_title(tab: &TabState) -> String {
    match (&tab.mode, &tab.filter_pattern) {
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
        match &tab.filter_state {
            FilterState::Inactive => String::new(),
            FilterState::Processing { progress } =>
                format!("| Filtering: {}/{}", progress, tab.total_lines),
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
    let label = app.current_filter_mode.prompt_label();
    let input_text = format!("{}: {}", label, app.get_input());

    // Determine border color based on mode and validation state
    let border_color = if app.current_filter_mode.is_regex() {
        if app.regex_error.is_some() {
            Color::Red // Invalid regex
        } else {
            Color::Cyan // Valid regex mode
        }
    } else {
        Color::White // Plain text mode
    };

    // Build help text with mode hint
    let mode_hint = if app.current_filter_mode.is_regex() {
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
/// * `prefix_width` - The width of the line number prefix (for continuation indent)
fn wrap_content(content: &str, available_width: usize, prefix_width: usize) -> Vec<Line<'static>> {
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
    let continuation_indent = " ".repeat(prefix_width);

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
                    // Add continuation indent for wrapped lines
                    current_line_spans = vec![Span::raw(continuation_indent.clone())];
                    current_line_width = prefix_width;
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
    // Calculate centered popup area (60% width, 80% height)
    let popup_width = (area.width as f32 * 0.6) as u16;
    let popup_height = (area.height as f32 * 0.8) as u16;
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
            "LazyTail - Keyboard Shortcuts",
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
        Line::from("  j / Down      Scroll down one line"),
        Line::from("  k / Up        Scroll up one line"),
        Line::from("  PageDown      Scroll down one page"),
        Line::from("  PageUp        Scroll up one page"),
        Line::from("  g             Jump to start"),
        Line::from("  G             Jump to end"),
        Line::from("  :123          Jump to line 123"),
        Line::from("  zz            Center selection on screen"),
        Line::from("  zt            Move selection to top"),
        Line::from("  zb            Move selection to bottom"),
        Line::from("  Space         Toggle line expansion"),
        Line::from("  c             Collapse all expanded lines"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Tabs",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  Tab           Next tab"),
        Line::from("  Shift+Tab     Previous tab"),
        Line::from("  1-9           Select tab directly"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Filtering",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  /             Start live filter"),
        Line::from("  Tab           Toggle Plain/Regex mode"),
        Line::from("  Alt+C         Toggle case sensitivity"),
        Line::from("  Enter         Close filter input (keep filter)"),
        Line::from("  Esc           Clear filter / Cancel input"),
        Line::from("  Up/Down       Navigate filter history"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Modes",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  f             Toggle follow mode"),
        Line::from("                (auto-scroll to new content)"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Other",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("  ?             Show this help"),
        Line::from("  q / Ctrl+C    Quit"),
        Line::from(""),
        Line::from(vec![Span::styled(
            "Press any key to close this help",
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
