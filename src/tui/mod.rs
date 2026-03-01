mod aggregation_view;
mod help;
mod log_view;
mod side_panel;
mod status_bar;

use crate::app::{App, InputMode, LayoutRect, ViewMode};
use anyhow::Result;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{Block, Clear},
    Frame,
};

pub fn render(f: &mut Frame, app: &mut App) -> Result<()> {
    let bg_block = Block::default().style(app.theme.ui.bg_style());
    f.render_widget(bg_block, f.area());

    // Main horizontal layout: side panel + content area
    let main_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(app.side_panel_width), Constraint::Min(1)])
        .split(f.area());

    // Render side panel with tabs
    let (sources_area, source_overflow) = side_panel::render_side_panel(f, main_chunks[0], app);

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

    // Store layout areas for mouse click hit testing
    app.layout.side_panel_sources = rect_to_layout(sources_area);
    app.layout.log_view = rect_to_layout(content_chunks[0]);

    if app.active_tab().source.mode == ViewMode::Aggregation {
        let ui = &app.theme.ui;
        let tab = if let Some(cat) = app.active_combined {
            app.combined_tabs[cat as usize]
                .as_mut()
                .expect("active_combined set but no combined tab for category")
        } else {
            &mut app.tabs[app.active_tab]
        };
        aggregation_view::render_aggregation_view(f, content_chunks[0], tab, ui);
    } else {
        log_view::render_log_view(f, content_chunks[0], app)?;
    }
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
    if let Some(scroll_offset) = app.help_scroll_offset {
        help::render_help_overlay(f, f.area(), scroll_offset, &app.theme.ui);
    }

    // Render close confirmation dialog on top of everything if active
    if app.input_mode == InputMode::ConfirmClose {
        help::render_confirm_close_dialog(f, f.area(), app);
    }

    Ok(())
}

fn rect_to_layout(r: Rect) -> LayoutRect {
    LayoutRect {
        x: r.x,
        y: r.y,
        width: r.width,
        height: r.height,
    }
}
