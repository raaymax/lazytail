use crate::app::{App, FilterState, ViewMode};
use crate::reader::LogReader;
use anyhow::Result;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

pub fn render<R: LogReader + ?Sized>(f: &mut Frame, app: &mut App, reader: &mut R) -> Result<()> {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(1),                                               // Main content
            Constraint::Length(3),                                            // Status bar
            Constraint::Length(if app.is_entering_filter() { 3 } else { 0 }), // Input prompt
        ])
        .split(f.area());

    render_log_view(f, chunks[0], app, reader)?;
    render_status_bar(f, chunks[1], app);

    if app.is_entering_filter() {
        render_input_prompt(f, chunks[2], app);
    }

    // Render help overlay on top of everything if active
    if app.show_help {
        render_help_overlay(f, f.area());
    }

    Ok(())
}

fn render_log_view<R: LogReader + ?Sized>(
    f: &mut Frame,
    area: Rect,
    app: &mut App,
    reader: &mut R,
) -> Result<()> {
    let visible_height = area.height.saturating_sub(2) as usize; // Account for borders

    // Adjust scroll position to keep selection in view
    app.adjust_scroll(visible_height);

    // Use the scroll position to determine which lines to display
    let start_idx = app.scroll_position;
    let count = visible_height.min(app.visible_line_count() - start_idx);

    // Fetch the lines to display
    let mut items = Vec::new();
    for i in start_idx..start_idx + count {
        if let Some(&line_number) = app.line_indices.get(i) {
            let line_text = reader.get_line(line_number)?.unwrap_or_default();

            // Add line number prefix
            let line_prefix = format!("{:6} │ ", line_number + 1);

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
            if i == app.selected_line {
                for span in &mut final_line.spans {
                    span.style = span.style.bg(Color::DarkGray).add_modifier(Modifier::BOLD);
                }
            }

            items.push(ListItem::new(final_line));
        }
    }

    let title = match (&app.mode, &app.filter_pattern) {
        (ViewMode::Normal, None) => "LazyTail".to_string(),
        (ViewMode::Filtered, Some(pattern)) => format!("LazyTail (Filter: \"{}\")", pattern),
        (ViewMode::Filtered, None) => "LazyTail (Filtered)".to_string(),
        (ViewMode::Normal, Some(_)) => "LazyTail".to_string(),
    };

    let list = List::new(items).block(Block::default().borders(Borders::ALL).title(title));

    f.render_widget(list, area);

    Ok(())
}

fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let status_text = format!(
        " Line {}/{} | Total: {} | Mode: {} {}{}",
        app.selected_line + 1,
        app.visible_line_count(),
        app.total_lines,
        match app.mode {
            ViewMode::Normal => "Normal",
            ViewMode::Filtered => "Filtered",
        },
        match &app.filter_state {
            FilterState::Inactive => String::new(),
            FilterState::Processing { progress } =>
                format!("| Filtering: {}/{}", progress, app.total_lines),
            FilterState::Complete { matches } => format!("| Matches: {}", matches),
        },
        if app.follow_mode { " | FOLLOW" } else { "" }
    );

    let help_text =
        " q: Quit | ↑↓: Navigate | g/G: Start/End | f: Follow | /: Filter | Esc: Clear | ?: Help";

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

fn render_input_prompt(f: &mut Frame, area: Rect, app: &App) {
    let input_text = format!("Filter: {}", app.get_input());

    let input = Paragraph::new(input_text)
        .style(Style::default().fg(Color::Yellow))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Live Filter (Enter to close, Esc to clear)"),
        );

    f.render_widget(input, area);

    // Show cursor at the end of input
    f.set_cursor_position((area.x + 9 + app.get_input().len() as u16, area.y + 1));
}

fn render_help_overlay(f: &mut Frame, area: Rect) {
    // Calculate centered popup area (60% width, 70% height)
    let popup_width = (area.width as f32 * 0.6) as u16;
    let popup_height = (area.height as f32 * 0.7) as u16;
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
        Line::from("  j / ↓         Scroll down one line"),
        Line::from("  k / ↑         Scroll up one line"),
        Line::from("  PageDown      Scroll down one page"),
        Line::from("  PageUp        Scroll up one page"),
        Line::from("  g             Jump to start"),
        Line::from("  G             Jump to end"),
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
