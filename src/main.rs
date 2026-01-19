mod app;
mod event;
mod filter;
mod handlers;
mod reader;
mod ui;
mod watcher;

use anyhow::{Context, Result};
use app::App;
use clap::Parser;
use crossterm::{
    event::{self as crossterm_event, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use filter::{engine::FilterEngine, string_filter::StringFilter, Filter};
use ratatui::{backend::CrosstermBackend, Terminal};
use reader::{file_reader::FileReader, LogReader};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use watcher::FileWatcher;

// Constants
const FILTER_PROGRESS_INTERVAL: usize = 1000;
const INPUT_POLL_DURATION_MS: u64 = 100;
const PAGE_SIZE_OFFSET: usize = 5;

#[derive(Parser, Debug)]
#[command(name = "lazytail")]
#[command(about = "A universal terminal-based log viewer with filtering support", long_about = None)]
struct Args {
    /// Log file to view
    #[arg(value_name = "FILE")]
    file: Option<PathBuf>,

    /// Read from stdin instead of a file
    #[arg(short, long)]
    stdin: bool,

    /// Enable file watching (auto-reload on changes)
    #[arg(short, long, default_value_t = true)]
    watch: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    if args.stdin {
        eprintln!("STDIN support not yet implemented");
        std::process::exit(1);
    }

    let file_path = args.file.context("File path required (or use --stdin)")?;

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create file reader
    let reader = FileReader::new(&file_path)?;
    let total_lines = reader.total_lines();
    let reader = Arc::new(Mutex::new(reader));

    // Create app state
    let mut app = App::new(total_lines);

    // Track filter receiver
    let mut filter_receiver: Option<std::sync::mpsc::Receiver<filter::engine::FilterProgress>> =
        None;

    // Track whether the current filter operation is incremental (only new logs) vs full (entire file)
    let mut is_incremental_filter = false;

    // Create file watcher if enabled
    let watcher = if args.watch {
        Some(FileWatcher::new(&file_path)?)
    } else {
        None
    };

    // Main loop
    let res = run_app(
        &mut terminal,
        &mut app,
        reader.clone(),
        &mut filter_receiver,
        &mut is_incremental_filter,
        watcher,
    );

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

/// Helper function to trigger a filter operation
/// Reduces code duplication across multiple filter trigger points
fn trigger_filter(
    app: &mut App,
    reader: Arc<Mutex<dyn LogReader + Send>>,
    pattern: String,
    is_incremental: &mut bool,
    start_line: Option<usize>,
    end_line: Option<usize>,
) -> std::sync::mpsc::Receiver<filter::engine::FilterProgress> {
    let filter: Arc<dyn Filter> = Arc::new(StringFilter::new(&pattern, false));

    if let (Some(start), Some(end)) = (start_line, end_line) {
        // Incremental filtering (range)
        app.filter_state = app::FilterState::Processing { progress: start };
        *is_incremental = true;
        FilterEngine::run_filter_range(reader, filter, FILTER_PROGRESS_INTERVAL, start, end)
    } else {
        // Full filtering
        app.filter_state = app::FilterState::Processing { progress: 0 };
        *is_incremental = false;
        FilterEngine::run_filter(reader, filter, FILTER_PROGRESS_INTERVAL)
    }
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    reader: Arc<Mutex<dyn LogReader + Send>>,
    filter_receiver: &mut Option<std::sync::mpsc::Receiver<filter::engine::FilterProgress>>,
    is_incremental_filter: &mut bool,
    watcher: Option<FileWatcher>,
) -> Result<()> {
    use event::AppEvent;

    loop {
        // Render
        terminal.draw(|f| {
            let mut reader_guard = reader.lock().unwrap();
            if let Err(e) = ui::render(f, app, &mut *reader_guard) {
                eprintln!("Render error: {}", e);
            }
        })?;

        // Show/hide cursor based on input mode
        if app.is_entering_filter() {
            terminal.show_cursor()?;
        } else {
            terminal.hide_cursor()?;
        }

        // Collect events from different sources
        let mut events = Vec::new();

        // Check for file changes
        if let Some(ref watcher) = watcher {
            if let Some(file_event) = watcher.try_recv() {
                match file_event {
                    watcher::FileEvent::Modified => {
                        // Reload the file reader
                        let mut reader_guard = reader.lock().unwrap();
                        if let Err(e) = reader_guard.reload() {
                            events
                                .push(AppEvent::FileError(format!("Failed to reload file: {}", e)));
                        } else {
                            let new_total = reader_guard.total_lines();
                            let old_total = app.total_lines;
                            drop(reader_guard);

                            // Generate events based on file modification
                            events.extend(handlers::file_events::process_file_modification(
                                new_total, old_total, app,
                            ));
                        }
                    }
                    watcher::FileEvent::Error(err) => {
                        events.push(AppEvent::FileError(err));
                    }
                }
            }
        }

        // Check for filter progress
        if let Some(rx) = filter_receiver {
            if let Ok(progress) = rx.try_recv() {
                // Handle filter progress and convert to events
                let filter_events =
                    handlers::filter::handle_filter_progress(progress, *is_incremental_filter);
                events.extend(filter_events);

                // Check if filter completed (to clear receiver)
                if events.iter().any(|e| {
                    matches!(
                        e,
                        AppEvent::FilterComplete { .. } | AppEvent::FilterError(_)
                    )
                }) {
                    *filter_receiver = None;
                }
            }
        }

        // Handle input
        if crossterm_event::poll(Duration::from_millis(INPUT_POLL_DURATION_MS))? {
            if let Event::Key(key) = crossterm_event::read()? {
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
        }

        // Process all collected events
        for event in events.clone() {
            // Handle special events that need side effects (like starting filters)
            match &event {
                AppEvent::StartFilter {
                    pattern,
                    incremental,
                    range,
                } => {
                    // Set filter pattern before triggering
                    app.filter_pattern = Some(pattern.clone());

                    *filter_receiver = Some(trigger_filter(
                        app,
                        reader.clone(),
                        pattern.clone(),
                        is_incremental_filter,
                        range.map(|(start, _)| start),
                        range.map(|(_, end)| end),
                    ));
                    *is_incremental_filter = *incremental;
                }
                AppEvent::FilterInputChar(_) | AppEvent::FilterInputBackspace => {
                    // Apply the event first
                    app.apply_event(event.clone());

                    // Then trigger live filter preview
                    let pattern = app.get_input().to_string();
                    if !pattern.is_empty() {
                        app.filter_pattern = Some(pattern.clone());
                        *filter_receiver = Some(trigger_filter(
                            app,
                            reader.clone(),
                            pattern,
                            is_incremental_filter,
                            None,
                            None,
                        ));
                    } else {
                        // Empty input - clear filter
                        app.clear_filter();
                        *filter_receiver = None;
                    }
                    continue; // Event already applied above
                }
                AppEvent::ClearFilter => {
                    *filter_receiver = None;
                }
                AppEvent::FileTruncated { .. } => {
                    // Cancel any in-progress filter on truncation
                    *filter_receiver = None;
                    *is_incremental_filter = false;
                }
                AppEvent::FilterComplete { .. } => {
                    // Apply the event first
                    app.apply_event(event.clone());

                    // Then handle follow mode jump
                    if app.follow_mode {
                        app.jump_to_end();
                    }
                    continue; // Event already applied above
                }
                AppEvent::FileModified { .. } | AppEvent::FileError(_) => {
                    // For file events, check if we need to jump to end in follow mode
                    let should_jump_follow = app.follow_mode
                        && app.mode == app::ViewMode::Normal
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
