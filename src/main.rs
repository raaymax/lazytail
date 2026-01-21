mod app;
mod event;
mod filter;
mod handlers;
mod reader;
mod tab;
mod ui;
mod watcher;

use anyhow::{Context, Result};
use app::{App, FilterState, ViewMode};
use clap::Parser;
use crossterm::{
    event::{self as crossterm_event, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use filter::{engine::FilterEngine, string_filter::StringFilter, Filter};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

// Constants
const FILTER_PROGRESS_INTERVAL: usize = 1000;
const INPUT_POLL_DURATION_MS: u64 = 100;
const PAGE_SIZE_OFFSET: usize = 5;

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
    start_line: Option<usize>,
    end_line: Option<usize>,
) {
    let filter: Arc<dyn Filter> = Arc::new(StringFilter::new(&pattern, false));

    let receiver = if let (Some(start), Some(end)) = (start_line, end_line) {
        // Incremental filtering (range)
        tab.filter_state = FilterState::Processing { progress: start };
        tab.is_incremental_filter = true;
        FilterEngine::run_filter_range(
            tab.reader.clone(),
            filter,
            FILTER_PROGRESS_INTERVAL,
            start,
            end,
        )
    } else {
        // Full filtering
        tab.filter_state = FilterState::Processing { progress: 0 };
        tab.is_incremental_filter = false;
        FilterEngine::run_filter(tab.reader.clone(), filter, FILTER_PROGRESS_INTERVAL)
    };

    tab.filter_receiver = Some(receiver);
}

/// Data about a file modification collected during tab iteration
struct FileModificationData {
    new_total: usize,
    old_total: usize,
}

/// Data about filter progress collected during tab iteration
struct FilterProgressData {
    tab_idx: usize,
    progress: filter::engine::FilterProgress,
    is_incremental: bool,
}

fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    use event::AppEvent;

    loop {
        // Render
        terminal.draw(|f| {
            if let Err(e) = ui::render(f, app) {
                eprintln!("Render error: {}", e);
            }
        })?;

        // Show/hide cursor based on input mode
        if app.is_entering_filter() || app.is_entering_line_jump() {
            terminal.show_cursor()?;
        } else {
            terminal.hide_cursor()?;
        }

        // Collect events from different sources
        let mut events = Vec::new();

        // === Phase 1: Collect file modification data from all tabs ===
        let mut file_modifications: Vec<FileModificationData> = Vec::new();

        for (tab_idx, tab) in app.tabs.iter_mut().enumerate() {
            if let Some(ref watcher) = tab.watcher {
                if let Some(file_event) = watcher.try_recv() {
                    match file_event {
                        watcher::FileEvent::Modified => {
                            // Reload the file reader
                            let mut reader_guard = tab.reader.lock().unwrap();
                            if let Err(e) = reader_guard.reload() {
                                eprintln!("Failed to reload file for tab {}: {}", tab_idx, e);
                            } else {
                                let new_total = reader_guard.total_lines();
                                let old_total = tab.total_lines;
                                drop(reader_guard);

                                if tab_idx == app.active_tab {
                                    // Collect for later processing
                                    file_modifications.push(FileModificationData {
                                        new_total,
                                        old_total,
                                    });
                                } else {
                                    // For inactive tabs, update state directly
                                    tab.total_lines = new_total;
                                    if tab.mode == ViewMode::Normal {
                                        tab.line_indices = (0..new_total).collect();
                                    }
                                    // If tab has an active filter, reapply it
                                    if let Some(pattern) = tab.filter_pattern.clone() {
                                        if new_total > tab.last_filtered_line {
                                            trigger_filter(
                                                tab,
                                                pattern,
                                                Some(tab.last_filtered_line),
                                                Some(new_total),
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        watcher::FileEvent::Error(err) => {
                            eprintln!("File watcher error for tab {}: {}", tab_idx, err);
                        }
                    }
                }
            }
        }

        // === Phase 2: Process active tab file modifications ===
        for mod_data in file_modifications {
            events.extend(handlers::file_events::process_file_modification(
                mod_data.new_total,
                mod_data.old_total,
                app,
            ));
        }

        // === Phase 3: Collect filter progress from all tabs ===
        let mut filter_progress_data: Vec<FilterProgressData> = Vec::new();

        for (tab_idx, tab) in app.tabs.iter_mut().enumerate() {
            if let Some(ref rx) = tab.filter_receiver {
                if let Ok(progress) = rx.try_recv() {
                    if tab_idx == app.active_tab {
                        // Collect for later processing
                        filter_progress_data.push(FilterProgressData {
                            tab_idx,
                            progress,
                            is_incremental: tab.is_incremental_filter,
                        });
                    } else {
                        // For inactive tabs, handle filter results directly
                        let filter_events = handlers::filter::handle_filter_progress(
                            progress,
                            tab.is_incremental_filter,
                        );

                        for ev in filter_events {
                            match ev {
                                AppEvent::FilterProgress(lines_processed) => {
                                    tab.filter_state = FilterState::Processing {
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
                                        let pattern =
                                            tab.filter_pattern.clone().unwrap_or_default();
                                        tab.apply_filter(indices, pattern);
                                    }
                                    if tab.follow_mode {
                                        tab.jump_to_end();
                                    }
                                    tab.filter_receiver = None;
                                }
                                AppEvent::FilterError(err) => {
                                    eprintln!("Filter error for tab {}: {}", tab_idx, err);
                                    tab.filter_state = FilterState::Inactive;
                                    tab.filter_receiver = None;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }
        }

        // === Phase 4: Process active tab filter progress ===
        for progress_data in filter_progress_data {
            let filter_events = handlers::filter::handle_filter_progress(
                progress_data.progress,
                progress_data.is_incremental,
            );

            // Check if filter completed (to clear receiver)
            let completed = filter_events.iter().any(|e| {
                matches!(
                    e,
                    AppEvent::FilterComplete { .. } | AppEvent::FilterError(_)
                )
            });
            events.extend(filter_events);
            if completed {
                app.tabs[progress_data.tab_idx].filter_receiver = None;
            }
        }

        // Handle input
        if crossterm_event::poll(Duration::from_millis(INPUT_POLL_DURATION_MS))? {
            match crossterm_event::read()? {
                Event::Key(key) => {
                    // Handle keyboard input and convert to events
                    let mut input_events = handlers::input::handle_input_event(key, app);

                    // Handle PageDown/PageUp - need terminal size
                    if matches!(key.code, KeyCode::PageDown) {
                        let page_size = terminal.size()?.height as usize - PAGE_SIZE_OFFSET;
                        input_events.push(AppEvent::PageDown(page_size));
                    } else if matches!(key.code, KeyCode::PageUp) {
                        let page_size = terminal.size()?.height as usize - PAGE_SIZE_OFFSET;
                        input_events.push(AppEvent::PageUp(page_size));
                    }

                    events.extend(input_events);
                }
                Event::Mouse(mouse_event) => {
                    use crossterm_event::MouseEventKind;

                    // Handle mouse scroll events
                    match mouse_event.kind {
                        MouseEventKind::ScrollDown => {
                            events.push(AppEvent::MouseScrollDown(3));
                            events.push(AppEvent::DisableFollowMode);
                        }
                        MouseEventKind::ScrollUp => {
                            events.push(AppEvent::MouseScrollUp(3));
                            events.push(AppEvent::DisableFollowMode);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }

        // Process all collected events
        for event in events.clone() {
            // Handle special events that need side effects (like starting filters)
            match &event {
                AppEvent::StartFilter {
                    pattern,
                    incremental: _,
                    range,
                } => {
                    // Set filter pattern before triggering
                    let tab = app.active_tab_mut();
                    tab.filter_pattern = Some(pattern.clone());

                    trigger_filter(
                        tab,
                        pattern.clone(),
                        range.map(|(start, _)| start),
                        range.map(|(_, end)| end),
                    );
                }
                AppEvent::FilterInputChar(_)
                | AppEvent::FilterInputBackspace
                | AppEvent::HistoryUp
                | AppEvent::HistoryDown => {
                    // Apply the event first
                    app.apply_event(event.clone());

                    // Then trigger live filter preview
                    let pattern = app.get_input().to_string();
                    if !pattern.is_empty() {
                        let tab = app.active_tab_mut();
                        tab.filter_pattern = Some(pattern.clone());
                        trigger_filter(tab, pattern, None, None);
                    } else {
                        // Empty input - clear filter
                        app.clear_filter();
                        app.active_tab_mut().filter_receiver = None;
                    }
                    continue; // Event already applied above
                }
                AppEvent::MouseScrollDown(lines) => {
                    // Calculate visible height from terminal size
                    let visible_height = terminal.size()?.height as usize - PAGE_SIZE_OFFSET - 1;
                    app.mouse_scroll_down(*lines, visible_height);
                    continue; // Event already applied
                }
                AppEvent::MouseScrollUp(lines) => {
                    // Calculate visible height from terminal size
                    let visible_height = terminal.size()?.height as usize - PAGE_SIZE_OFFSET - 1;
                    app.mouse_scroll_up(*lines, visible_height);
                    continue; // Event already applied
                }
                AppEvent::ClearFilter => {
                    app.active_tab_mut().filter_receiver = None;
                }
                AppEvent::FileTruncated { .. } => {
                    // Cancel any in-progress filter on truncation
                    let tab = app.active_tab_mut();
                    tab.filter_receiver = None;
                    tab.is_incremental_filter = false;
                }
                AppEvent::FilterComplete { .. } => {
                    // Apply the event first
                    app.apply_event(event.clone());

                    // Then handle follow mode jump
                    if app.active_tab().follow_mode {
                        app.jump_to_end();
                    }
                    continue; // Event already applied above
                }
                AppEvent::FileModified { .. } | AppEvent::FileError(_) => {
                    // For file events, check if we need to jump to end in follow mode
                    let should_jump_follow = app.active_tab().follow_mode
                        && app.active_tab().mode == ViewMode::Normal
                        && !events
                            .iter()
                            .any(|e| matches!(e, AppEvent::StartFilter { .. }));

                    app.apply_event(event.clone());

                    if should_jump_follow {
                        app.jump_to_end();
                    }
                    continue; // Event already applied above
                }
                _ => {}
            }

            // Apply the event to app state
            app.apply_event(event);
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
