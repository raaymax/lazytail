use crate::app::{App, FilterState, ViewMode};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub(super) fn render_status_bar(f: &mut Frame, area: Rect, app: &App) {
    let tab = app.active_tab();

    let status_text = format!(
        " Line {}/{} | Total: {} | Mode: {} {}{}",
        tab.selected_line + 1,
        tab.visible_line_count(),
        tab.source.total_lines,
        match tab.source.mode {
            ViewMode::Normal => "Normal",
            ViewMode::Filtered => "Filtered",
            ViewMode::Aggregation => "Aggregation",
        },
        match &tab.source.filter.state {
            FilterState::Inactive => String::new(),
            FilterState::Processing { lines_processed } => {
                let percent = if tab.source.total_lines > 0 {
                    (lines_processed * 100) / tab.source.total_lines
                } else {
                    0
                };
                format!("| Filtering: {}%", percent)
            }
            FilterState::Complete { matches } => format!("| Matches: {}", matches),
        },
        if tab.source.follow_mode {
            " | FOLLOW"
        } else {
            ""
        }
    );

    let show_status_msg = app
        .status_message
        .as_ref()
        .is_some_and(|(_, t)| t.elapsed().as_secs() < 3);

    let bottom_line = if tab.source.mode == ViewMode::Aggregation {
        if let Some(ref result) = tab.source.aggregation_result {
            Line::from(vec![Span::styled(
                format!(
                    " Row {}/{} | Enter: drill down | Esc: back | / - re-filter",
                    tab.aggregation_view.selected_row + 1,
                    result.groups.len()
                ),
                Style::default().fg(Color::Magenta),
            )])
        } else {
            Line::from(vec![Span::styled(
                " Computing aggregation...",
                Style::default().fg(Color::DarkGray),
            )])
        }
    } else if show_status_msg {
        let msg = &app.status_message.as_ref().unwrap().0;
        Line::from(vec![Span::styled(
            format!(" {}", msg),
            Style::default().fg(Color::Green),
        )])
    } else {
        let help_text = if app.tab_count() > 1 {
            " Tab/Shift+Tab - Switch | 1-9 - Select | ? - Help"
        } else {
            " q - Quit | j/k - Navigate | g/G - Start/End | / - Filter | ? - Help"
        };
        Line::from(vec![Span::styled(
            help_text,
            Style::default().fg(Color::DarkGray),
        )])
    };

    let status_lines = vec![
        Line::from(vec![Span::styled(
            status_text,
            Style::default().add_modifier(Modifier::BOLD),
        )]),
        bottom_line,
    ];

    let paragraph =
        Paragraph::new(status_lines).block(Block::default().borders(Borders::ALL).title("Status"));

    f.render_widget(paragraph, area);
}

pub(super) fn render_filter_input_prompt(f: &mut Frame, area: Rect, app: &App) {
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

pub(super) fn render_line_jump_prompt(f: &mut Frame, area: Rect, app: &App) {
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
