mod app;
mod event;
mod filter;
mod handlers;
mod history;
mod reader;
mod tab;
mod ui;
mod viewport;
mod watcher;

use anyhow::{Context, Result};
use app::{App, FilterState, ViewMode};
use clap::Parser;
use crossterm::{
    event::{self as crossterm_event, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use filter::{
    engine::FilterEngine, regex_filter::RegexFilter, string_filter::StringFilter, Filter,
    FilterMode,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

// Constants
const FILTER_PROGRESS_INTERVAL: usize = 1000;
const INPUT_POLL_DURATION_MS: u64 = 100;
const PAGE_SIZE_OFFSET: usize = 5;
const MOUSE_SCROLL_LINES: usize = 3;

#[derive(Parser, Debug)]
#[command(name = "lazytail")]
#[command(about = "A universal terminal-based log viewer with filtering support", long_about = None)]
struct Args {
    /// Log files to view (multiple files will open in tabs, use - for stdin)
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Disable file watching
    #[arg(long = "no-watch")]
    no_watch: bool,
}

fn main() -> Result<()> {
    use std::io::IsTerminal;

    let args = Args::parse();

    // Auto-detect stdin: if nothing is piped and no files given, show usage
    let stdin_is_tty = std::io::stdin().is_terminal();
    let has_piped_input = !stdin_is_tty;

    if args.files.is_empty() && !has_piped_input {
        eprintln!("Usage: lazytail <FILE>...");
        eprintln!("       command | lazytail");
        eprintln!("       lazytail -  (explicit stdin)");
        std::process::exit(1);
    }

    // Create app state BEFORE terminal setup (important for process substitution and stdin)
    // These sources may become invalid after terminal operations
    let watch = !args.no_watch;

    // Build tabs, treating "-" as stdin
    let mut tabs = Vec::new();
    let mut stdin_used = false;

    // If stdin has piped data, always include it as the first tab
    if has_piped_input {
        tabs.push(tab::TabState::from_stdin().context("Failed to read from stdin")?);
        stdin_used = true;
    }

    for file in args.files {
        if file.as_os_str() == "-" {
            if stdin_used {
                // Already read stdin, skip duplicate
                continue;
            }
            stdin_used = true;
            tabs.push(tab::TabState::from_stdin().context("Failed to read from stdin")?);
        } else {
            tabs.push(tab::TabState::new(file, watch).context("Failed to open log file")?);
        }
    }

    let mut app = App::with_tabs(tabs);

    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main loop
    let res = run_app(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

/// Helper function to trigger a filter operation for a specific tab
fn trigger_filter(
    tab: &mut tab::TabState,
    pattern: String,
    mode: FilterMode,
    start_line: Option<usize>,
    end_line: Option<usize>,
) {
    let case_sensitive = mode.is_case_sensitive();
    let filter: Arc<dyn Filter> = if mode.is_regex() {
        match RegexFilter::new(&pattern, case_sensitive) {
            Ok(f) => Arc::new(f),
            Err(_) => {
                // Invalid regex - don't apply filter
                return;
            }
        }
    } else {
        Arc::new(StringFilter::new(&pattern, case_sensitive))
    };

    let receiver = if let (Some(start), Some(end)) = (start_line, end_line) {
        // Incremental filtering (range)
        tab.filter.state = FilterState::Processing { progress: start };
        tab.filter.is_incremental = true;
        FilterEngine::run_filter_range(
            tab.reader.clone(),
            filter,
            FILTER_PROGRESS_INTERVAL,
            start,
            end,
        )
    } else {
        // Full filtering
        tab.filter.state = FilterState::Processing { progress: 0 };
        tab.filter.is_incremental = false;
        FilterEngine::run_filter(tab.reader.clone(), filter, FILTER_PROGRESS_INTERVAL)
    };

    tab.filter.receiver = Some(receiver);
}

fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    use event::AppEvent;

    loop {
        // Phase 1: Render
        render(terminal, app)?;

        // Phase 2: Collect events from all sources
        let mut events = Vec::new();
        events.extend(collect_file_events(app));
        events.extend(collect_filter_progress(app));
        events.extend(collect_input_events(terminal, app)?);

        // Phase 3: Process all events
        let has_start_filter = events
            .iter()
            .any(|e| matches!(e, AppEvent::StartFilter { .. }));

        for event in events {
            process_event(app, event, has_start_filter);
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Render the UI and manage cursor visibility
fn render<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    terminal.draw(|f| {
        if let Err(e) = ui::render(f, app) {
            eprintln!("Render error: {}", e);
        }
    })?;

    if app.is_entering_filter() || app.is_entering_line_jump() {
        terminal.show_cursor()?;
    } else {
        terminal.hide_cursor()?;
    }

    Ok(())
}

/// Data about a file modification for the active tab
struct ActiveTabFileModification {
    new_total: usize,
    old_total: usize,
}

/// Collect file watcher events from all tabs
fn collect_file_events(app: &mut App) -> Vec<event::AppEvent> {
    let active_tab = app.active_tab;

    // First pass: reload files and handle inactive tabs
    let mut active_tab_modification: Option<ActiveTabFileModification> = None;

    for (tab_idx, tab) in app.tabs.iter_mut().enumerate() {
        if let Some(ref watcher) = tab.watcher {
            if let Some(file_event) = watcher.try_recv() {
                match file_event {
                    watcher::FileEvent::Modified => {
                        let mut reader_guard = tab
                            .reader
                            .lock()
                            .expect("Reader lock poisoned - filter thread panicked");

                        if let Err(e) = reader_guard.reload() {
                            eprintln!("Failed to reload file for tab {}: {}", tab_idx, e);
                            continue;
                        }

                        let new_total = reader_guard.total_lines();
                        let old_total = tab.total_lines;
                        drop(reader_guard);

                        if tab_idx == active_tab {
                            // Collect for processing after the loop
                            active_tab_modification = Some(ActiveTabFileModification {
                                new_total,
                                old_total,
                            });
                        } else {
                            // Inactive tab: update state directly
                            handle_inactive_tab_file_modification(tab, new_total);
                        }
                    }
                    watcher::FileEvent::Error(err) => {
                        eprintln!("File watcher error for tab {}: {}", tab_idx, err);
                    }
                }
            }
        }
    }

    // Second pass: process active tab modification (needs immutable app access)
    if let Some(mod_data) = active_tab_modification {
        handlers::file_events::process_file_modification(
            mod_data.new_total,
            mod_data.old_total,
            app,
        )
    } else {
        Vec::new()
    }
}

/// Handle file modification for an inactive tab
fn handle_inactive_tab_file_modification(tab: &mut tab::TabState, new_total: usize) {
    tab.total_lines = new_total;

    if tab.mode == ViewMode::Normal {
        tab.line_indices = (0..new_total).collect();
    }

    // If tab has an active filter, trigger incremental filtering
    if let Some(pattern) = tab.filter.pattern.clone() {
        if new_total > tab.filter.last_filtered_line {
            let mode = tab.filter.mode;
            trigger_filter(
                tab,
                pattern,
                mode,
                Some(tab.filter.last_filtered_line),
                Some(new_total),
            );
        }
    }
}

/// Collect filter progress from all tabs
fn collect_filter_progress(app: &mut App) -> Vec<event::AppEvent> {
    use event::AppEvent;

    let mut events = Vec::new();
    let active_tab = app.active_tab;

    for (tab_idx, tab) in app.tabs.iter_mut().enumerate() {
        if let Some(ref rx) = tab.filter.receiver {
            if let Ok(progress) = rx.try_recv() {
                let is_incremental = tab.filter.is_incremental;
                let filter_events =
                    handlers::filter::handle_filter_progress(progress, is_incremental);

                if tab_idx == active_tab {
                    // Active tab: check for completion and collect events
                    let completed = filter_events.iter().any(|e| {
                        matches!(
                            e,
                            AppEvent::FilterComplete { .. } | AppEvent::FilterError(_)
                        )
                    });
                    events.extend(filter_events);
                    if completed {
                        tab.filter.receiver = None;
                    }
                } else {
                    // Inactive tab: apply filter events directly
                    apply_filter_events_to_tab(tab, tab_idx, filter_events);
                }
            }
        }
    }

    events
}

/// Apply filter events directly to a tab (used for inactive tabs)
fn apply_filter_events_to_tab(
    tab: &mut tab::TabState,
    tab_idx: usize,
    filter_events: Vec<event::AppEvent>,
) {
    use event::AppEvent;

    for ev in filter_events {
        match ev {
            AppEvent::FilterProgress(lines_processed) => {
                tab.filter.state = FilterState::Processing {
                    progress: lines_processed,
                };
            }
            AppEvent::FilterComplete {
                indices,
                incremental,
            } => {
                if incremental {
                    tab.append_filter_results(indices);
                } else {
                    let pattern = tab.filter.pattern.clone().unwrap_or_default();
                    tab.apply_filter(indices, pattern);
                }
                if tab.follow_mode {
                    tab.jump_to_end();
                }
                tab.filter.receiver = None;
            }
            AppEvent::FilterError(err) => {
                eprintln!("Filter error for tab {}: {}", tab_idx, err);
                tab.filter.state = FilterState::Inactive;
                tab.filter.receiver = None;
            }
            _ => {}
        }
    }
}

/// Collect input events from keyboard and mouse
fn collect_input_events<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &App,
) -> Result<Vec<event::AppEvent>> {
    use crossterm_event::MouseEventKind;
    use event::AppEvent;

    let mut events = Vec::new();

    if !crossterm_event::poll(Duration::from_millis(INPUT_POLL_DURATION_MS))? {
        return Ok(events);
    }

    match crossterm_event::read()? {
        Event::Key(key) => {
            events.extend(handlers::input::handle_input_event(key, app));

            // Add page size for PageDown/PageUp
            if matches!(key.code, KeyCode::PageDown) {
                let page_size = terminal.size()?.height as usize - PAGE_SIZE_OFFSET;
                events.push(AppEvent::PageDown(page_size));
            } else if matches!(key.code, KeyCode::PageUp) {
                let page_size = terminal.size()?.height as usize - PAGE_SIZE_OFFSET;
                events.push(AppEvent::PageUp(page_size));
            }
        }
        Event::Mouse(mouse_event) => match mouse_event.kind {
            MouseEventKind::ScrollDown => {
                events.push(AppEvent::MouseScrollDown(MOUSE_SCROLL_LINES));
                events.push(AppEvent::DisableFollowMode);
            }
            MouseEventKind::ScrollUp => {
                events.push(AppEvent::MouseScrollUp(MOUSE_SCROLL_LINES));
                events.push(AppEvent::DisableFollowMode);
            }
            _ => {}
        },
        _ => {}
    }

    Ok(events)
}

/// Process a single event, handling side effects
fn process_event(app: &mut App, event: event::AppEvent, has_start_filter: bool) {
    use event::AppEvent;

    match &event {
        // Filter start - trigger background filter
        AppEvent::StartFilter { pattern, range, .. } => {
            let mode = app.current_filter_mode;
            let tab = app.active_tab_mut();
            tab.filter.pattern = Some(pattern.clone());
            tab.filter.mode = mode;
            trigger_filter(
                tab,
                pattern.clone(),
                mode,
                range.map(|(start, _)| start),
                range.map(|(_, end)| end),
            );
        }

        // Live filter preview events
        AppEvent::FilterInputChar(_)
        | AppEvent::FilterInputBackspace
        | AppEvent::HistoryUp
        | AppEvent::HistoryDown
        | AppEvent::ToggleFilterMode
        | AppEvent::ToggleCaseSensitivity => {
            app.apply_event(event);
            trigger_live_filter_preview(app);
        }

        // Mouse scroll - handle directly
        AppEvent::MouseScrollDown(lines) => {
            app.mouse_scroll_down(*lines);
        }
        AppEvent::MouseScrollUp(lines) => {
            app.mouse_scroll_up(*lines);
        }

        // Clear filter - also clear receiver
        AppEvent::ClearFilter => {
            app.active_tab_mut().filter.receiver = None;
            app.apply_event(event);
        }

        // File truncated - cancel in-progress filter
        AppEvent::FileTruncated { .. } => {
            let tab = app.active_tab_mut();
            tab.filter.receiver = None;
            tab.filter.is_incremental = false;
            app.apply_event(event);
        }

        // Filter complete - handle follow mode
        AppEvent::FilterComplete { .. } => {
            app.apply_event(event);
            if app.active_tab().follow_mode {
                app.jump_to_end();
            }
        }

        // File modified - handle follow mode
        AppEvent::FileModified { .. } => {
            let should_jump = app.active_tab().follow_mode
                && app.active_tab().mode == ViewMode::Normal
                && !has_start_filter;
            app.apply_event(event);
            if should_jump {
                app.jump_to_end();
            }
        }

        // All other events - apply directly
        _ => {
            app.apply_event(event);
        }
    }
}

/// Trigger live filter preview based on current input
fn trigger_live_filter_preview(app: &mut App) {
    let pattern = app.get_input().to_string();
    let mode = app.current_filter_mode;

    if !pattern.is_empty() && app.is_regex_valid() {
        let tab = app.active_tab_mut();
        tab.filter.pattern = Some(pattern.clone());
        tab.filter.mode = mode;
        trigger_filter(tab, pattern, mode, None, None);
    } else {
        app.clear_filter();
        app.active_tab_mut().filter.receiver = None;
    }
}
