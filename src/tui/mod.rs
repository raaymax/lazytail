mod help;
mod log_view;
mod side_panel;
mod status_bar;

use crate::app::{App, InputMode};
use anyhow::Result;
use ratatui::{
    layout::{Constraint, Direction, Layout},
    widgets::Clear,
    Frame,
};

pub fn render(f: &mut Frame, app: &mut App) -> Result<()> {
    // Main horizontal layout: side panel + content area
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(app.side_panel_width), Constraint::Min(1)])
        .split(f.area());

    // Render side panel with tabs
    let source_overflow = side_panel::render_side_panel(f, main_chunks[0], app);

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

    log_view::render_log_view(f, content_chunks[0], app)?;
    status_bar::render_status_bar(f, content_chunks[1], app);

    if app.is_entering_filter() {
        status_bar::render_filter_input_prompt(f, content_chunks[2], app);
    } else if app.is_entering_line_jump() {
        status_bar::render_line_jump_prompt(f, content_chunks[2], app);
    }

    // Render source overflow overlay on top of log view
    if let Some((line_content, overlay_area)) = source_overflow {
        f.render_widget(Clear, overlay_area);
        f.render_widget(ratatui::widgets::Paragraph::new(line_content), overlay_area);
    }

    // Render help overlay on top of everything if active
    if app.show_help {
        help::render_help_overlay(f, f.area());
    }

    // Render close confirmation dialog on top of everything if active
    if app.input_mode == InputMode::ConfirmClose {
        help::render_confirm_close_dialog(f, f.area(), app);
    }

    Ok(())
}
