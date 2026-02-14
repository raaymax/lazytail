mod app;
mod cache;
mod capture;
mod cmd;
mod config;
mod dir_watcher;
mod event;
mod filter;
mod handlers;
mod history;
#[allow(dead_code)]
mod index;
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
use app::{App, SourceType};
use clap::Parser;
use crossterm::{
    event::{self as crossterm_event, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use filter::orchestrator::FilterOrchestrator;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

// Constants
const INPUT_POLL_DURATION_MS: u64 = 100;
const PAGE_SIZE_OFFSET: usize = 5;
const MOUSE_SCROLL_LINES: usize = 3;

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
struct Cli {
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

    /// Subcommand to run
    #[command(subcommand)]
    command: Option<cmd::Commands>,
}

fn main() -> Result<()> {
    use std::io::IsTerminal;

    let cli = Cli::parse();

    // Handle subcommands first (before mode detection)
    if let Some(command) = cli.command {
        return match command {
            cmd::Commands::Init(args) => cmd::init::run(args.force)
                .map_err(|code| anyhow::anyhow!("init failed with exit code {}", code)),
            cmd::Commands::Config { action } => match action {
                cmd::ConfigAction::Validate => cmd::config::validate().map_err(|code| {
                    anyhow::anyhow!("config validate failed with exit code {}", code)
                }),
                cmd::ConfigAction::Show => cmd::config::show()
                    .map_err(|code| anyhow::anyhow!("config show failed with exit code {}", code)),
            },
        };
    }

    // Cleanup stale markers from previous SIGKILL scenarios
    // This runs before any mode to ensure collision checks work correctly
    source::cleanup_stale_markers();

    // Config discovery - run before mode dispatch
    let (discovery, searched_paths) = config::discovery::discover_verbose();
    if cli.verbose {
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

    // Load config from discovered files
    let config_result = config::load(&discovery);
    let (cfg, mut config_errors) = match config_result {
        Ok(c) => (c, Vec::new()),
        Err(err) => {
            let err_msg = err.to_string();
            (config::Config::default(), vec![err_msg])
        }
    };

    if cli.verbose {
        if let Some(name) = &cfg.name {
            eprintln!("[config] Project name: {}", name);
        }
        eprintln!("[config] Project sources: {}", cfg.project_sources.len());
        eprintln!("[config] Global sources: {}", cfg.global_sources.len());
        for err in &config_errors {
            eprintln!("[config] Error: {}", err);
        }
    }

    // Mode 0: MCP server mode (--mcp flag)
    #[cfg(feature = "mcp")]
    if cli.mcp {
        return mcp::run_mcp_server();
    }

    // Auto-detect stdin: if nothing is piped and no files given, check for other modes
    let stdin_is_tty = std::io::stdin().is_terminal();
    let has_piped_input = !stdin_is_tty;

    // Mode 1: Capture mode (-n flag with stdin)
    if let Some(name) = cli.name {
        if stdin_is_tty {
            eprintln!("Error: Capture mode (-n) requires stdin input");
            eprintln!("Usage: command | lazytail -n <NAME>");
            std::process::exit(1);
        }
        return capture::run_capture_mode(name, &discovery);
    }

    // Mode 2: Discovery mode (no files, no stdin)
    if cli.files.is_empty() && !has_piped_input {
        return run_discovery_mode(cli.no_watch, cfg, config_errors, &discovery);
    }

    // Create app state BEFORE terminal setup (important for process substitution and stdin)
    // These sources may become invalid after terminal operations
    let watch = !cli.no_watch;

    // Build tabs from config sources first
    let mut tabs = Vec::new();

    // Add project sources
    for source in &cfg.project_sources {
        match tab::TabState::from_config_source(source, SourceType::ProjectSource, watch) {
            Ok(t) => tabs.push(t),
            Err(e) => config_errors.push(format!("Failed to open {}: {}", source.name, e)),
        }
    }

    // Add global sources
    for source in &cfg.global_sources {
        match tab::TabState::from_config_source(source, SourceType::GlobalSource, watch) {
            Ok(t) => tabs.push(t),
            Err(e) => config_errors.push(format!("Failed to open {}: {}", source.name, e)),
        }
    }

    // Build tabs from CLI args, treating "-" as stdin
    let mut stdin_used = false;

    // If stdin has piped data, always include it as the first tab
    if has_piped_input {
        tabs.push(tab::TabState::from_stdin().context("Failed to read from stdin")?);
        stdin_used = true;
    }

    for file in cli.files {
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

    // Log config errors to stderr (debug source is a future enhancement)
    for err in &config_errors {
        eprintln!("[config error] {}", err);
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

/// Run in discovery mode: auto-discover sources from project and global data directories
fn run_discovery_mode(
    no_watch: bool,
    cfg: config::Config,
    mut config_errors: Vec<String>,
    discovery: &config::DiscoveryResult,
) -> Result<()> {
    use source::{discover_sources_for_context, ensure_directories_for_context};

    // Ensure config directories exist (project or global based on context)
    ensure_directories_for_context(discovery)?;

    // Discover existing sources from both project and global directories
    let sources = discover_sources_for_context(discovery)?;

    let watch = !no_watch;

    // Build tabs from config sources first
    let mut tabs = Vec::new();

    // Add project sources
    for source in &cfg.project_sources {
        match tab::TabState::from_config_source(source, SourceType::ProjectSource, watch) {
            Ok(t) => tabs.push(t),
            Err(e) => config_errors.push(format!("Failed to open {}: {}", source.name, e)),
        }
    }

    // Add global sources
    for source in &cfg.global_sources {
        match tab::TabState::from_config_source(source, SourceType::GlobalSource, watch) {
            Ok(t) => tabs.push(t),
            Err(e) => config_errors.push(format!("Failed to open {}: {}", source.name, e)),
        }
    }

    // Add discovered sources
    let discovery_tabs: Vec<tab::TabState> = sources
        .into_iter()
        .filter_map(|s| tab::TabState::from_discovered_source(s, watch).ok())
        .collect();
    tabs.extend(discovery_tabs);

    // Log config errors to stderr (debug source is a future enhancement)
    for err in &config_errors {
        eprintln!("[config error] {}", err);
    }

    if tabs.is_empty() {
        eprintln!("No log sources found.");
        eprintln!();
        eprintln!("Options:");
        eprintln!("  1. Create a lazytail.yaml config file in your project");
        eprintln!("  2. Use capture mode: command | lazytail -n <NAME>");
        eprintln!("  3. Specify files directly: lazytail <FILE>...");
        std::process::exit(0);
    }

    let mut app = App::with_tabs(tabs);

    // Optionally set up directory watcher for new sources
    // Watch project data dir if in project, otherwise global
    let dir_watcher = if watch {
        let watch_dir = if discovery.project_root.is_some() {
            source::resolve_data_dir(discovery)
        } else {
            source::data_dir()
        };
        watch_dir.and_then(|p| dir_watcher::DirectoryWatcher::new(p).ok())
    } else {
        None
    };

    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Determine watched location for newly discovered sources
    let watched_location = if discovery.project_root.is_some() {
        Some(source::SourceLocation::Project)
    } else {
        Some(source::SourceLocation::Global)
    };

    // Main loop with directory watcher
    let res = run_app_with_discovery(&mut terminal, &mut app, dir_watcher, watched_location);

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

fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    run_app_with_discovery(terminal, app, None, None)
}

/// Run the app with optional directory watcher for source discovery mode
fn run_app_with_discovery<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    dir_watcher: Option<dir_watcher::DirectoryWatcher>,
    watched_location: Option<source::SourceLocation>,
) -> Result<()> {
    use event::AppEvent;

    loop {
        // Phase 1: Render
        render(terminal, app)?;

        // Phase 2: Check for pending debounced filter
        if let Some(trigger_at) = app.pending_filter_at {
            if Instant::now() >= trigger_at {
                app.pending_filter_at = None;
                FilterOrchestrator::trigger_preview(app);
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
                                let status = path
                                    .parent()
                                    .and_then(|d| d.parent())
                                    .map(|base| base.join("sources"))
                                    .filter(|s| s.exists())
                                    .map(|s| source::check_source_status_in_dir(&name, &s))
                                    .unwrap_or_else(|| source::check_source_status(&name));
                                let source = source::DiscoveredSource {
                                    name,
                                    log_path: path,
                                    status,
                                    // Use the location of the watched directory
                                    location: watched_location
                                        .unwrap_or(source::SourceLocation::Global),
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
        app.has_start_filter_in_batch = events
            .iter()
            .any(|e| matches!(e, AppEvent::StartFilter { .. }));

        for event in events {
            process_event(app, event);
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

                        // Update file size
                        if let Some(ref path) = tab.source_path {
                            tab.file_size = std::fs::metadata(path).map(|m| m.len()).ok();
                        }

                        if tab_idx == active_tab {
                            // Collect for processing after the loop
                            active_tab_modification = Some(ActiveTabFileModification {
                                new_total,
                                old_total,
                            });
                        } else {
                            // Inactive tab: update state directly
                            tab.apply_file_modification(new_total);
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
                    for ev in &filter_events {
                        if tab.apply_filter_event(ev) {
                            tab.filter.receiver = None;
                        }
                    }
                }
            }
        }
    }

    events
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

/// Process a single event — delegates to `app.apply_event()`.
///
/// All filter side-effects (debounce, cancellation, follow-mode jumps) are now
/// handled inside `App::apply_event()`, so this function is a thin passthrough.
fn process_event(app: &mut App, event: event::AppEvent) {
    app.apply_event(event);
}
