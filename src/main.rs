mod app;
mod filter;
mod reader;
mod ui;
mod watcher;

use anyhow::{Context, Result};
use app::App;
use clap::Parser;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyModifiers},
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

#[derive(Parser, Debug)]
#[command(name = "logviewer")]
#[command(about = "A TUI log viewer with filtering support", long_about = None)]
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
    let mut filter_receiver: Option<std::sync::mpsc::Receiver<filter::engine::FilterProgress>> = None;

    // Track whether the current filter operation is incremental (only new logs) vs full (entire file)
    let mut is_incremental_filter = false;

    // Create file watcher if enabled
    let watcher = if args.watch {
        Some(FileWatcher::new(&file_path)?)
    } else {
        None
    };

    // Main loop
    let res = run_app(&mut terminal, &mut app, reader.clone(), &mut filter_receiver, &mut is_incremental_filter, watcher);

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

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    reader: Arc<Mutex<dyn LogReader + Send>>,
    filter_receiver: &mut Option<std::sync::mpsc::Receiver<filter::engine::FilterProgress>>,
    is_incremental_filter: &mut bool,
    watcher: Option<FileWatcher>,
) -> Result<()> {
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

        // Check for file changes
        if let Some(ref watcher) = watcher {
            if let Some(event) = watcher.try_recv() {
                match event {
                    watcher::FileEvent::Modified => {
                        // Reload the file reader
                        let mut reader_guard = reader.lock().unwrap();
                        if let Err(e) = reader_guard.reload() {
                            eprintln!("Failed to reload file: {}", e);
                        } else {
                            let new_total = reader_guard.total_lines();
                            let old_total = app.total_lines;
                            app.total_lines = new_total;

                            // If no filter is active, update line indices
                            let is_reapplying_filter = if app.mode == app::ViewMode::Normal {
                                app.line_indices = (0..new_total).collect();
                                false
                            } else {
                                // Apply filter on new content only (incremental filtering)
                                if let Some(pattern) = app.filter_pattern.clone() {
                                    // Only filter NEW lines that were added
                                    let start_line = app.last_filtered_line;
                                    if start_line < new_total {
                                        let filter: Arc<dyn Filter> = Arc::new(
                                            StringFilter::new(&pattern, false)
                                        );
                                        app.filter_state = app::FilterState::Processing { progress: start_line };
                                        *is_incremental_filter = true;
                                        *filter_receiver = Some(FilterEngine::run_filter_range(
                                            reader.clone(),
                                            filter,
                                            1000,
                                            start_line,
                                            new_total,
                                        ));
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            };

                            // If follow mode is enabled and we're not re-applying a filter, jump to end
                            // (if we're re-applying, the filter completion will handle the jump)
                            if app.follow_mode && !is_reapplying_filter {
                                app.jump_to_end();
                            }
                        }
                    }
                    watcher::FileEvent::Error(err) => {
                        eprintln!("File watcher error: {}", err);
                    }
                }
            }
        }

        // Check for filter progress
        if let Some(rx) = filter_receiver {
            if let Ok(progress) = rx.try_recv() {
                match progress {
                    filter::engine::FilterProgress::Processing(lines_processed) => {
                        app.filter_state = app::FilterState::Processing { progress: lines_processed };
                    }
                    filter::engine::FilterProgress::Complete(matching_indices) => {
                        if *is_incremental_filter {
                            // Incremental filtering - append new results to existing filtered lines
                            app.append_filter_results(matching_indices);
                        } else {
                            // Full filtering - replace all filtered lines
                            let pattern = app.filter_pattern.clone().unwrap_or_default();
                            app.apply_filter(matching_indices, pattern);
                        }
                        *filter_receiver = None;

                        // If follow mode is enabled, jump to end after filter completes
                        if app.follow_mode {
                            app.jump_to_end();
                        }
                    }
                    filter::engine::FilterProgress::Error(err) => {
                        eprintln!("Filter error: {}", err);
                        *filter_receiver = None;
                    }
                }
            }
        }

        // Handle input
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if app.is_entering_filter() {
                    // Handle filter input mode
                    match key.code {
                        KeyCode::Char(c) => {
                            app.input_char(c);
                            // Trigger live filter preview
                            let pattern = app.get_input().to_string();
                            if !pattern.is_empty() {
                                let filter: Arc<dyn Filter> = Arc::new(
                                    StringFilter::new(&pattern, false)
                                );
                                app.filter_state = app::FilterState::Processing { progress: 0 };
                                app.filter_pattern = Some(pattern);
                                *is_incremental_filter = false;
                                *filter_receiver = Some(FilterEngine::run_filter(
                                    reader.clone(),
                                    filter,
                                    1000,
                                ));
                            } else {
                                // Empty input - show all lines
                                app.clear_filter();
                                *filter_receiver = None;
                            }
                        }
                        KeyCode::Backspace => {
                            app.input_backspace();
                            // Trigger live filter preview
                            let pattern = app.get_input().to_string();
                            if !pattern.is_empty() {
                                let filter: Arc<dyn Filter> = Arc::new(
                                    StringFilter::new(&pattern, false)
                                );
                                app.filter_state = app::FilterState::Processing { progress: 0 };
                                app.filter_pattern = Some(pattern);
                                *is_incremental_filter = false;
                                *filter_receiver = Some(FilterEngine::run_filter(
                                    reader.clone(),
                                    filter,
                                    1000,
                                ));
                            } else {
                                // Empty input - show all lines
                                app.clear_filter();
                                *filter_receiver = None;
                            }
                        }
                        KeyCode::Enter => {
                            // Just exit input mode, keep the current filter active
                            app.cancel_filter_input();
                        }
                        KeyCode::Esc => {
                            // Cancel input and clear filter
                            app.cancel_filter_input();
                            app.clear_filter();
                            *filter_receiver = None;
                        }
                        _ => {}
                    }
                } else {
                    // Handle normal navigation mode
                    match key.code {
                        KeyCode::Char('q') => {
                            app.should_quit = true;
                        }
                        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            app.should_quit = true;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.scroll_down();
                            // Disable follow mode on manual scroll
                            app.follow_mode = false;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.scroll_up();
                            // Disable follow mode on manual scroll
                            app.follow_mode = false;
                        }
                        KeyCode::PageDown => {
                            let page_size = terminal.size()?.height as usize - 5;
                            app.page_down(page_size);
                            // Disable follow mode on manual scroll
                            app.follow_mode = false;
                        }
                        KeyCode::PageUp => {
                            let page_size = terminal.size()?.height as usize - 5;
                            app.page_up(page_size);
                            // Disable follow mode on manual scroll
                            app.follow_mode = false;
                        }
                        KeyCode::Char('g') => {
                            app.jump_to_start();
                            app.follow_mode = false;
                        }
                        KeyCode::Char('G') => {
                            app.jump_to_end();
                            app.follow_mode = false;
                        }
                        KeyCode::Char('f') => {
                            app.toggle_follow_mode();
                        }
                        KeyCode::Char('/') => {
                            app.start_filter_input();
                        }
                        KeyCode::Esc => {
                            app.clear_filter();
                            *filter_receiver = None;
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
