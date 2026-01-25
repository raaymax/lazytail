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

    // Get reader access
    let mut reader_guard = tab.reader.lock().unwrap();

    // Fetch the lines to display
    let mut items = Vec::new();
    for i in start_idx..start_idx + count {
        if let Some(&line_number) = tab.line_indices.get(i) {
            let line_text = reader_guard.get_line(line_number)?.unwrap_or_default();

            // Add line number prefix
            let line_prefix = format!("{:6} | ", line_number + 1);

            // Parse ANSI codes and convert to ratatui Line with styles
            // Convert to owned text to avoid lifetime issues
            let parsed_text = ansi_to_tui::IntoText::into_text(&line_text)
                .unwrap_or_else(|_| ratatui::text::Text::raw(line_text.clone()));

            // Build the final line with prefix
            let mut final_line = Line::default();
            final_line.spans.push(Span::raw(line_prefix));

            // Add the parsed colored spans from the log line
            // Make sure all spans are owned (convert Cow to String)
            if let Some(first_line) = parsed_text.lines.first() {
                for span in &first_line.spans {
                    final_line
                        .spans
                        .push(Span::styled(span.content.to_string(), span.style));
                }
            }

            // Apply selection background if this is the selected line
            if i == selected_idx {
                for span in &mut final_line.spans {
                    // Remap foreground colors that would be invisible on dark gray background
                    let new_style = match span.style.fg {
                        Some(Color::Gray) | Some(Color::DarkGray) | Some(Color::Black) => {
                            // Remap to white for visibility
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
