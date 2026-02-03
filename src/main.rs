mod app;
mod cache;
mod capture;
mod config;
mod dir_watcher;
mod event;
mod filter;
mod handlers;
mod history;
#[cfg(feature = "mcp")]
mod mcp;
mod reader;
mod signal;
mod source;
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
    cancel::CancelToken, engine::FilterEngine, query, regex_filter::RegexFilter, streaming_filter,
    string_filter::StringFilter, Filter, FilterMode,
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

// Constants
const FILTER_PROGRESS_INTERVAL: usize = 1000;
const INPUT_POLL_DURATION_MS: u64 = 100;
const PAGE_SIZE_OFFSET: usize = 5;
const MOUSE_SCROLL_LINES: usize = 3;
/// Debounce delay for live filter preview (milliseconds)
const FILTER_DEBOUNCE_MS: u64 = 500;

#[derive(Parser, Debug)]
#[command(name = "lazytail")]
#[command(version)]
#[command(about = "A fast terminal-based log viewer with live filtering")]
#[command(
    long_about = "A fast terminal-based log viewer with live filtering, regex support, \
and multi-tab interface. Supports file watching, stdin piping, and source capture."
)]
#[command(after_help = "\
EXAMPLES:
    lazytail app.log                    View a single log file
    lazytail app.log error.log          View multiple files in tabs
    kubectl logs pod | lazytail         Pipe logs from any command
    lazytail                            Discover sources from ~/.config/lazytail/data/

CAPTURE MODE:
    cmd | lazytail -n \"API\"             Capture stdin to ~/.config/lazytail/data/API.log
                                        (tee-like: writes to file AND echoes to stdout)

    Then in another terminal:
    lazytail                            View all captured sources with live updates

IN-APP HELP:
    Press '?' inside the app to see all keyboard shortcuts.
    Press '/' to start filtering, 'f' to toggle follow mode.
")]
struct Args {
    /// Log files to view (omit for source discovery mode)
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    /// Disable file watching (files won't auto-reload on changes)
    #[arg(long = "no-watch")]
    no_watch: bool,

    /// Capture stdin to a named source file (tee-like behavior)
    ///
    /// Writes stdin to ~/.config/lazytail/data/<NAME>.log while echoing to stdout.
    /// The source can then be viewed with 'lazytail' (discovery mode).
    #[arg(short = 'n', long = "name", value_name = "NAME")]
    name: Option<String>,

    /// Run as MCP (Model Context Protocol) server
    ///
    /// Starts an MCP server using stdio transport for AI assistant integration.
    /// Provides tools for reading and searching log files.
    #[cfg(feature = "mcp")]
    #[arg(long = "mcp")]
    mcp: bool,

    /// Verbose output (show config discovery paths)
    #[arg(short = 'v', long = "verbose")]
    verbose: bool,
}

fn main() -> Result<()> {
    use std::io::IsTerminal;

    let args = Args::parse();

    // Cleanup stale markers from previous SIGKILL scenarios
    // This runs before any mode to ensure collision checks work correctly
    source::cleanup_stale_markers();

    // Config discovery - run before mode dispatch
    // Phase 3 will use this for config loading; for now we just report in verbose mode
    let (discovery, searched_paths) = config::discovery::discover_verbose();
    if args.verbose {
        for path in &searched_paths {
            eprintln!("[discovery] Searched: {}", path.display());
        }
        eprintln!(
            "[discovery] Project root: {}",
            discovery
                .project_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not found".to_string())
        );
        eprintln!(
            "[discovery] Project config: {}",
            discovery
                .project_config
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not found".to_string())
        );
        eprintln!(
            "[discovery] Global config: {}",
            discovery
                .global_config
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not found".to_string())
        );
    }
    // Store discovery result for future use (Phase 3)
    let _discovery = discovery;

    // Mode 0: MCP server mode (--mcp flag)
    #[cfg(feature = "mcp")]
    if args.mcp {
        return mcp::run_mcp_server();
    }

    // Auto-detect stdin: if nothing is piped and no files given, check for other modes
    let stdin_is_tty = std::io::stdin().is_terminal();
    let has_piped_input = !stdin_is_tty;

    // Mode 1: Capture mode (-n flag with stdin)
    if let Some(name) = args.name {
        if stdin_is_tty {
            eprintln!("Error: Capture mode (-n) requires stdin input");
            eprintln!("Usage: command | lazytail -n <NAME>");
            std::process::exit(1);
        }
        return capture::run_capture_mode(name);
    }

    // Mode 2: Discovery mode (no files, no stdin)
    if args.files.is_empty() && !has_piped_input {
        return run_discovery_mode(args.no_watch);
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

/// Run in discovery mode: auto-discover sources from ~/.config/lazytail/data/
fn run_discovery_mode(no_watch: bool) -> Result<()> {
    use source::{discover_sources, ensure_directories};

    // Ensure config directories exist
    ensure_directories()?;

    // Discover existing sources
    let sources = discover_sources()?;

    if sources.is_empty() {
        eprintln!("No log sources found in ~/.config/lazytail/data/");
        eprintln!();
        eprintln!("To create sources, use capture mode:");
        eprintln!("  command | lazytail -n <NAME>");
        eprintln!();
        eprintln!("Or specify files directly:");
        eprintln!("  lazytail <FILE>...");
        std::process::exit(0);
    }

    // Create tabs from discovered sources
    let watch = !no_watch;
    let tabs: Vec<tab::TabState> = sources
        .into_iter()
        .filter_map(|s| tab::TabState::from_discovered_source(s, watch).ok())
        .collect();

    if tabs.is_empty() {
        eprintln!("Failed to open any log sources");
        std::process::exit(1);
    }

    let mut app = App::with_tabs(tabs);

    // Optionally set up directory watcher for new sources
    let dir_watcher = if watch {
        source::data_dir().and_then(|p| dir_watcher::DirectoryWatcher::new(p).ok())
    } else {
        None
    };

    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Main loop with directory watcher
    let res = run_app_with_discovery(&mut terminal, &mut app, dir_watcher);

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
    // Cancel any previous filter operation
    if let Some(ref cancel) = tab.filter.cancel_token {
        cancel.cancel();
    }

    // Check for query syntax (json | ... or logfmt | ...)
    if query::is_query_syntax(&pattern) {
        trigger_query_filter(tab, pattern, start_line, end_line);
        return;
    }

    let case_sensitive = mode.is_case_sensitive();
    let is_regex = mode.is_regex();

    // Create new cancel token for this operation
    let cancel = CancelToken::new();
    tab.filter.cancel_token = Some(cancel.clone());

    // For incremental filtering, we need the generic filter
    let receiver = if let (Some(start), Some(end)) = (start_line, end_line) {
        tab.filter.state = FilterState::Processing { lines_processed: 0 };
        tab.filter.is_incremental = true;

        let filter: Arc<dyn Filter> = if is_regex {
            match RegexFilter::new(&pattern, case_sensitive) {
                Ok(f) => Arc::new(f),
                Err(_) => return,
            }
        } else {
            Arc::new(StringFilter::new(&pattern, case_sensitive))
        };

        if let Some(path) = &tab.source_path {
            match streaming_filter::run_streaming_filter_range(
                path.clone(),
                filter,
                start,
                end,
                cancel,
            ) {
                Ok(rx) => rx,
                Err(_) => return,
            }
        } else {
            FilterEngine::run_filter_range(
                tab.reader.clone(),
                filter,
                FILTER_PROGRESS_INTERVAL,
                start,
                end,
                cancel,
            )
        }
    } else {
        // Full filtering
        tab.filter.needs_clear = true;
        tab.filter.state = FilterState::Processing { lines_processed: 0 };
        tab.filter.is_incremental = false;

        if let Some(path) = &tab.source_path {
            if is_regex {
                // Regex: use generic filter
                let filter: Arc<dyn Filter> = match RegexFilter::new(&pattern, case_sensitive) {
                    Ok(f) => Arc::new(f),
                    Err(_) => return,
                };
                match streaming_filter::run_streaming_filter(path.clone(), filter, cancel) {
                    Ok(rx) => rx,
                    Err(_) => return,
                }
            } else {
                // Plain text: use FAST byte-level filter with SIMD
                match streaming_filter::run_streaming_filter_fast(
                    path.clone(),
                    pattern.as_bytes(),
                    case_sensitive,
                    cancel,
                ) {
                    Ok(rx) => rx,
                    Err(_) => return,
                }
            }
        } else {
            // Stdin: use generic filter
            let filter: Arc<dyn Filter> = if is_regex {
                match RegexFilter::new(&pattern, case_sensitive) {
                    Ok(f) => Arc::new(f),
                    Err(_) => return,
                }
            } else {
                Arc::new(StringFilter::new(&pattern, case_sensitive))
            };
            FilterEngine::run_filter(tab.reader.clone(), filter, FILTER_PROGRESS_INTERVAL, cancel)
        }
    };

    tab.filter.receiver = Some(receiver);
}

/// Trigger a query-based filter (json | ... or logfmt | ...)
fn trigger_query_filter(
    tab: &mut tab::TabState,
    pattern: String,
    start_line: Option<usize>,
    end_line: Option<usize>,
) {
    // Parse the query
    let filter_query = match query::parse_query(&pattern) {
        Ok(q) => q,
        Err(_) => return, // Invalid query, don't filter
    };

    // Create QueryFilter
    let query_filter = match query::QueryFilter::new(filter_query) {
        Ok(f) => f,
        Err(_) => return, // Invalid filter (e.g., bad regex)
    };

    let filter: Arc<dyn Filter> = Arc::new(query_filter);

    // Create new cancel token for this operation
    let cancel = CancelToken::new();
    tab.filter.cancel_token = Some(cancel.clone());

    // For incremental filtering
    let receiver = if let (Some(start), Some(end)) = (start_line, end_line) {
        tab.filter.state = FilterState::Processing { lines_processed: 0 };
        tab.filter.is_incremental = true;

        if let Some(path) = &tab.source_path {
            match streaming_filter::run_streaming_filter_range(
                path.clone(),
                filter,
                start,
                end,
                cancel,
            ) {
                Ok(rx) => rx,
                Err(_) => return,
            }
        } else {
            FilterEngine::run_filter_range(
                tab.reader.clone(),
                filter,
                FILTER_PROGRESS_INTERVAL,
                start,
                end,
                cancel,
            )
        }
    } else {
        // Full filtering
        tab.filter.needs_clear = true;
        tab.filter.state = FilterState::Processing { lines_processed: 0 };
        tab.filter.is_incremental = false;

        if let Some(path) = &tab.source_path {
            match streaming_filter::run_streaming_filter(path.clone(), filter, cancel) {
                Ok(rx) => rx,
                Err(_) => return,
            }
        } else {
            FilterEngine::run_filter(tab.reader.clone(), filter, FILTER_PROGRESS_INTERVAL, cancel)
        }
    };

    tab.filter.receiver = Some(receiver);
}

fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    run_app_with_discovery(terminal, app, None)
}

/// Run the app with optional directory watcher for source discovery mode
fn run_app_with_discovery<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    dir_watcher: Option<dir_watcher::DirectoryWatcher>,
) -> Result<()> {
    use event::AppEvent;

    loop {
        // Phase 1: Render
        render(terminal, app)?;

        // Phase 2: Check for pending debounced filter
        if let Some(trigger_at) = app.pending_filter_at {
            if Instant::now() >= trigger_at {
                app.pending_filter_at = None;
                trigger_live_filter_preview(app);
            }
        }

        // Phase 2.5: Refresh source status for discovered sources
        for tab in &mut app.tabs {
            tab.refresh_source_status();
        }

        // Phase 2.6: Check for new sources from directory watcher
        if let Some(ref watcher) = dir_watcher {
            while let Some(dir_event) = watcher.try_recv() {
                match dir_event {
                    dir_watcher::DirEvent::NewFile(path) => {
                        // Check if we already have this file open
                        let already_open = app
                            .tabs
                            .iter()
                            .any(|t| t.source_path.as_ref() == Some(&path));
                        if !already_open {
                            // Extract name from path
                            if let Some(stem) = path.file_stem() {
                                let name = stem.to_string_lossy().to_string();
                                let status = source::check_source_status(&name);
                                let source = source::DiscoveredSource {
                                    name,
                                    log_path: path,
                                    status,
                                };
                                if let Ok(tab) = tab::TabState::from_discovered_source(source, true)
                                {
                                    app.add_tab(tab);
                                }
                            }
                        }
                    }
                    dir_watcher::DirEvent::FileRemoved(_path) => {
                        // Optionally handle file removal (don't close tab, just mark as unavailable)
                    }
                }
            }
        }

        // Phase 3: Collect events from all sources
        let mut events = Vec::new();
        events.extend(collect_file_events(app));
        events.extend(collect_filter_progress(app));
        collect_stream_events(app); // Handle stream events directly (modifies tabs)
        events.extend(collect_input_events(terminal, app)?);

        // Phase 4: Process all events
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
                tab.filter.state = FilterState::Processing { lines_processed };
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

/// Collect and process stream events from background readers (pipes/stdin)
/// This modifies tabs directly rather than returning events
fn collect_stream_events(app: &mut App) {
    use std::sync::mpsc::TryRecvError;
    use tab::StreamMessage;

    for tab in app.tabs.iter_mut() {
        if tab.stream_receiver.is_none() {
            continue;
        }

        // Drain all available messages
        loop {
            let msg = {
                let receiver = tab.stream_receiver.as_ref().unwrap();
                match receiver.try_recv() {
                    Ok(msg) => msg,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        // Channel closed - mark complete and stop
                        tab.mark_stream_complete();
                        break;
                    }
                }
            };

            match msg {
                StreamMessage::Lines(lines) => {
                    tab.append_stream_lines(lines);
                }
                StreamMessage::Complete => {
                    tab.mark_stream_complete();
                    break;
                }
                StreamMessage::Error(err) => {
                    eprintln!("Stream read error: {}", err);
                    tab.mark_stream_complete();
                    break;
                }
            }
        }
    }
}

/// Collect input events from keyboard and mouse
/// Coalesces mouse scroll events to prevent input lag while keeping key events responsive
fn collect_input_events<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &App,
) -> Result<Vec<event::AppEvent>> {
    use crossterm_event::MouseEventKind;
    use event::AppEvent;

    let mut events = Vec::new();

    // Wait for at least one event (with timeout)
    if !crossterm_event::poll(Duration::from_millis(INPUT_POLL_DURATION_MS))? {
        return Ok(events);
    }

    // Read the first event - this is guaranteed to be available after poll() returns true
    let first_event = crossterm_event::read()?;

    // Process the first event
    match first_event {
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

            // For key events, return immediately to allow state changes
            // (e.g., 'z' changes mode, second 'z' needs to see new mode)
            return Ok(events);
        }
        Event::Mouse(mouse_event) => {
            // For mouse events, we'll coalesce scroll events below
            match mouse_event.kind {
                MouseEventKind::ScrollDown => {
                    let mut scroll_count: usize = 1;

                    // Drain and coalesce consecutive scroll-down events
                    while crossterm_event::poll(Duration::ZERO)? {
                        match crossterm_event::read()? {
                            Event::Mouse(me) if me.kind == MouseEventKind::ScrollDown => {
                                scroll_count += 1;
                            }
                            Event::Key(key) => {
                                // Got a key event - emit scroll first, then key
                                events.push(AppEvent::MouseScrollDown(
                                    scroll_count * MOUSE_SCROLL_LINES,
                                ));
                                events.push(AppEvent::DisableFollowMode);
                                events.extend(handlers::input::handle_input_event(key, app));
                                return Ok(events);
                            }
                            _ => break, // Different event type, stop coalescing
                        }
                    }

                    events.push(AppEvent::MouseScrollDown(scroll_count * MOUSE_SCROLL_LINES));
                    events.push(AppEvent::DisableFollowMode);
                }
                MouseEventKind::ScrollUp => {
                    let mut scroll_count: usize = 1;

                    // Drain and coalesce consecutive scroll-up events
                    while crossterm_event::poll(Duration::ZERO)? {
                        match crossterm_event::read()? {
                            Event::Mouse(me) if me.kind == MouseEventKind::ScrollUp => {
                                scroll_count += 1;
                            }
                            Event::Key(key) => {
                                // Got a key event - emit scroll first, then key
                                events.push(AppEvent::MouseScrollUp(
                                    scroll_count * MOUSE_SCROLL_LINES,
                                ));
                                events.push(AppEvent::DisableFollowMode);
                                events.extend(handlers::input::handle_input_event(key, app));
                                return Ok(events);
                            }
                            _ => break, // Different event type, stop coalescing
                        }
                    }

                    events.push(AppEvent::MouseScrollUp(scroll_count * MOUSE_SCROLL_LINES));
                    events.push(AppEvent::DisableFollowMode);
                }
                _ => {}
            }
        }
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

        // Live filter preview events - debounce to avoid lag while typing
        AppEvent::FilterInputChar(_)
        | AppEvent::FilterInputBackspace
        | AppEvent::HistoryUp
        | AppEvent::HistoryDown
        | AppEvent::ToggleFilterMode
        | AppEvent::ToggleCaseSensitivity => {
            app.apply_event(event);
            // Cancel any in-progress filter immediately to free the reader lock
            if let Some(ref cancel) = app.active_tab().filter.cancel_token {
                cancel.cancel();
            }
            // Schedule filter to run after debounce delay
            app.pending_filter_at =
                Some(Instant::now() + Duration::from_millis(FILTER_DEBOUNCE_MS));
        }

        // Mouse scroll - handle directly
        AppEvent::MouseScrollDown(lines) => {
            app.mouse_scroll_down(*lines);
        }
        AppEvent::MouseScrollUp(lines) => {
            app.mouse_scroll_up(*lines);
        }

        // Clear filter - also clear receiver and pending filter
        AppEvent::ClearFilter => {
            app.pending_filter_at = None;
            if let Some(ref cancel) = app.active_tab().filter.cancel_token {
                cancel.cancel();
            }
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

        // Filter input cancelled - clear pending filter and cancel any in-progress
        AppEvent::FilterInputCancel => {
            app.pending_filter_at = None;
            if let Some(ref cancel) = app.active_tab().filter.cancel_token {
                cancel.cancel();
            }
            app.apply_event(event);
        }

        // Filter input submitted - trigger filter immediately (bypass debounce)
        AppEvent::FilterInputSubmit => {
            app.pending_filter_at = None;
            // Trigger filter with current input BEFORE apply_event clears it
            let pattern = app.get_input().to_string();
            let mode = app.current_filter_mode;
            if !pattern.is_empty() && app.is_regex_valid() {
                let tab = app.active_tab_mut();
                tab.filter.pattern = Some(pattern.clone());
                tab.filter.mode = mode;
                trigger_filter(tab, pattern, mode, None, None);
            }
            app.apply_event(event);
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
