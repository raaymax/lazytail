mod ansi;
mod app;
mod cache;
mod capture;
mod cli;
mod config;
mod filter;
mod handlers;
mod history;
mod log_source;
#[cfg(feature = "mcp")]
mod mcp;
mod reader;
mod renderer;
mod session;
mod signal;
mod source;
mod theme;
mod tui;
#[cfg(feature = "self-update")]
mod update;
mod watcher;
mod web;

use anyhow::{Context, Result};
use app::{App, AppEvent, FilterState, SourceType, StreamMessage, TabState, ViewMode};
use clap::Parser;
use crossterm::{
    event::{self as crossterm_event, DisableMouseCapture, EnableMouseCapture, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use filter::orchestrator::FilterOrchestrator;
use lazytail::index;
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
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

    /// Output raw lines without rendering (only meaningful with -n)
    #[arg(long = "raw")]
    raw: bool,

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

    /// Disable background update check on startup
    #[cfg(feature = "self-update")]
    #[arg(long = "no-update-check")]
    no_update_check: bool,

    /// Subcommand to run
    #[command(subcommand)]
    command: Option<cli::Commands>,
}

fn main() -> Result<()> {
    use std::io::IsTerminal;

    let startup = Instant::now();
    let mut phase = Instant::now();
    let cli = Cli::parse();
    let verbose = cli.verbose;
    if verbose {
        eprintln!("[startup]   cli parse: {:.1?}", phase.elapsed());
    }

    // Handle subcommands first (before mode detection)
    if let Some(command) = cli.command {
        return match command {
            cli::Commands::Init(args) => cli::init::run(args.force)
                .map_err(|code| anyhow::anyhow!("init failed with exit code {}", code)),
            cli::Commands::Web(args) => {
                web::run(args).map_err(|code| anyhow::anyhow!("web failed with exit code {}", code))
            }
            cli::Commands::Bench(args) => cli::bench::run(args)
                .map_err(|code| anyhow::anyhow!("bench failed with exit code {}", code)),
            cli::Commands::Config { action } => match action {
                cli::ConfigAction::Validate => cli::config::validate().map_err(|code| {
                    anyhow::anyhow!("config validate failed with exit code {}", code)
                }),
                cli::ConfigAction::Show => cli::config::show()
                    .map_err(|code| anyhow::anyhow!("config show failed with exit code {}", code)),
            },
            cli::Commands::Theme { action } => match action {
                cli::ThemeAction::Import(args) => cli::theme::run_import(args)
                    .map_err(|code| anyhow::anyhow!("theme import failed with exit code {}", code)),
                cli::ThemeAction::List => cli::theme::run_list()
                    .map_err(|code| anyhow::anyhow!("theme list failed with exit code {}", code)),
            },
            #[cfg(feature = "self-update")]
            cli::Commands::Update(args) => cli::update::run(args.check)
                .map_err(|code| anyhow::anyhow!("update failed with exit code {}", code)),
        };
    }

    // Cleanup stale markers from previous SIGKILL scenarios
    // This runs before any mode to ensure collision checks work correctly
    phase = Instant::now();
    source::cleanup_stale_markers();
    if verbose {
        eprintln!("[startup]   stale marker cleanup: {:.1?}", phase.elapsed());
    }

    // Config discovery - run before mode dispatch
    phase = Instant::now();
    let (discovery, searched_paths) = config::discovery::discover_verbose();
    if verbose {
        eprintln!("[startup]   config discovery: {:.1?}", phase.elapsed());
    }
    if verbose {
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
    phase = Instant::now();
    let config_result = config::load(&discovery);
    let (cfg, mut config_errors) = match config_result {
        Ok(c) => (c, Vec::new()),
        Err(err) => {
            let err_msg = err.to_string();
            (config::Config::default(), vec![err_msg])
        }
    };
    if verbose {
        eprintln!("[startup]   config load: {:.1?}", phase.elapsed());
    }

    if verbose {
        if let Some(name) = &cfg.name {
            eprintln!("[config] Project name: {}", name);
        }
        eprintln!("[config] Project sources: {}", cfg.project_sources.len());
        eprintln!("[config] Global sources: {}", cfg.global_sources.len());
        for err in &config_errors {
            eprintln!("[config] Error: {}", err);
        }
    }

    // Spawn background update check (if self-update feature is enabled)
    #[cfg(feature = "self-update")]
    let update_handle = spawn_update_check(&cli, &cfg);

    // Mode 0: MCP server mode (--mcp flag)
    #[cfg(feature = "mcp")]
    if cli.mcp {
        return mcp::run_mcp_server();
    }

    // Compile rendering presets from config (before capture dispatch, needed for R21 capture rendering)
    let (registry, compile_errors) = renderer::PresetRegistry::compile_from_config(
        &cfg.renderers,
        discovery.project_root.as_deref(),
    );
    config_errors.extend(compile_errors);
    let preset_registry = Arc::new(registry);

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
        let renderer_names: Vec<String> = cfg
            .project_sources
            .iter()
            .chain(cfg.global_sources.iter())
            .find(|s| s.name == name)
            .map(|s| s.renderer_names.clone())
            .unwrap_or_default();
        return capture::run_capture_mode(
            name,
            &discovery,
            preset_registry,
            renderer_names,
            &cfg.theme.palette,
            cli.raw,
        );
    }

    // Mode 2: Discovery mode (no files, no stdin)
    if cli.files.is_empty() && !has_piped_input {
        let result = run_discovery_mode(
            cli.no_watch,
            cfg,
            config_errors,
            &discovery,
            startup,
            verbose,
        );
        #[cfg(feature = "self-update")]
        print_update_notice(update_handle);
        return result;
    }

    // Create app state BEFORE terminal setup (important for process substitution and stdin)
    // These sources may become invalid after terminal operations
    let watch = !cli.no_watch;

    // Build tabs from config sources first
    phase = Instant::now();
    let mut tabs = Vec::new();

    // Add project sources
    for source in &cfg.project_sources {
        match TabState::from_config_source(source, SourceType::ProjectSource, watch) {
            Ok(Some(t)) => tabs.push(t),
            Ok(None) => {} // Metadata-only source, skip
            Err(e) => config_errors.push(format!("Failed to open {}: {}", source.name, e)),
        }
    }

    // Add global sources
    for source in &cfg.global_sources {
        match TabState::from_config_source(source, SourceType::GlobalSource, watch) {
            Ok(Some(t)) => tabs.push(t),
            Ok(None) => {} // Metadata-only source, skip
            Err(e) => config_errors.push(format!("Failed to open {}: {}", source.name, e)),
        }
    }

    // Build tabs from CLI args, treating "-" as stdin
    let mut stdin_used = false;

    // If stdin has piped data, always include it as the first tab
    if has_piped_input {
        tabs.push(TabState::from_stdin().context("Failed to read from stdin")?);
        stdin_used = true;
    }

    for file in cli.files {
        if file.as_os_str() == "-" {
            if stdin_used {
                // Already read stdin, skip duplicate
                continue;
            }
            stdin_used = true;
            tabs.push(TabState::from_stdin().context("Failed to read from stdin")?);
        } else {
            tabs.push(TabState::new(file, watch).context("Failed to open log file")?);
        }
    }
    if verbose {
        eprintln!("[startup]   tab creation: {:.1?}", phase.elapsed());
    }

    // Build columnar indexes for file tabs that don't have one yet
    phase = Instant::now();
    for tab in &tabs {
        if let Some(path) = tab.file_path() {
            let idx_dir = source::index_dir_for_log(path);
            if !idx_dir.join("meta").exists() {
                let file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
                let name = path.file_name().unwrap_or_default().to_string_lossy();
                eprintln!("Building index for {} ({} bytes)...", name, file_size);
                let start = std::time::Instant::now();
                match index::builder::IndexBuilder::new().build(path, &idx_dir) {
                    Ok(meta) => {
                        eprintln!(
                            "  Done: {} lines indexed in {:.1?}",
                            meta.entry_count,
                            start.elapsed()
                        );
                    }
                    Err(e) => {
                        eprintln!("  Warning: failed to build index: {}", e);
                    }
                }
            }
        }
    }
    if verbose {
        eprintln!("[startup]   index build: {:.1?}", phase.elapsed());
    }

    // Log config errors to stderr (debug source is a future enhancement)
    for err in &config_errors {
        eprintln!("[config error] {}", err);
    }

    phase = Instant::now();
    let mut app = App::with_tabs(tabs, preset_registry);
    app.startup_time = Some(startup);
    app.verbose = verbose;
    app.theme = cfg.theme;
    app.ensure_combined_tabs();

    // Restore last active source from session
    let project_root = discovery.project_root.as_deref();
    restore_last_source(&mut app, project_root);

    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    if verbose {
        eprintln!("[startup]   terminal setup: {:.1?}", phase.elapsed());
    }

    // Main loop
    let res = run_app(&mut terminal, &mut app);

    // Save active source to session
    save_active_source(&app, project_root);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if app.verbose {
        if let Some(elapsed) = app.first_render_elapsed {
            eprintln!("[startup] First render in {:.1?}", elapsed);
        }
    }

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    #[cfg(feature = "self-update")]
    print_update_notice(update_handle);

    Ok(())
}

/// Run in discovery mode: auto-discover sources from project and global data directories
fn run_discovery_mode(
    no_watch: bool,
    cfg: config::Config,
    mut config_errors: Vec<String>,
    discovery: &config::DiscoveryResult,
    startup: Instant,
    verbose: bool,
) -> Result<()> {
    use source::{discover_sources_for_context, ensure_directories_for_context};

    // Ensure config directories exist (project or global based on context)
    ensure_directories_for_context(discovery)?;

    // Discover existing sources from both project and global directories
    let mut phase = Instant::now();
    let sources = discover_sources_for_context(discovery)?;
    if verbose {
        eprintln!("[startup]   source discovery: {:.1?}", phase.elapsed());
    }

    // Build columnar indexes for sources that don't have one yet
    phase = Instant::now();
    source::build_missing_indexes(&sources);
    if verbose {
        eprintln!("[startup]   index build: {:.1?}", phase.elapsed());
    }

    let watch = !no_watch;

    // Compile rendering presets from config
    let (registry, compile_errors) = renderer::PresetRegistry::compile_from_config(
        &cfg.renderers,
        discovery.project_root.as_deref(),
    );
    config_errors.extend(compile_errors);
    let preset_registry = Arc::new(registry);

    // Build source name → renderer_names map from config sources
    let source_renderer_map: std::collections::HashMap<String, Vec<String>> = cfg
        .project_sources
        .iter()
        .chain(cfg.global_sources.iter())
        .filter(|s| !s.renderer_names.is_empty())
        .map(|s| (s.name.clone(), s.renderer_names.clone()))
        .collect();

    // Build tabs from config sources first
    phase = Instant::now();
    let mut tabs = Vec::new();

    // Add project sources
    for source in &cfg.project_sources {
        match TabState::from_config_source(source, SourceType::ProjectSource, watch) {
            Ok(Some(t)) => tabs.push(t),
            Ok(None) => {} // Metadata-only source, skip
            Err(e) => config_errors.push(format!("Failed to open {}: {}", source.name, e)),
        }
    }

    // Add global sources
    for source in &cfg.global_sources {
        match TabState::from_config_source(source, SourceType::GlobalSource, watch) {
            Ok(Some(t)) => tabs.push(t),
            Ok(None) => {} // Metadata-only source, skip
            Err(e) => config_errors.push(format!("Failed to open {}: {}", source.name, e)),
        }
    }

    // Add discovered sources (with renderer_names from config if available)
    let discovery_tabs: Vec<TabState> = sources
        .into_iter()
        .filter_map(|s| {
            let renderers = source_renderer_map
                .get(&s.name)
                .cloned()
                .unwrap_or_default();
            TabState::from_discovered_source(s, watch, renderers).ok()
        })
        .collect();
    tabs.extend(discovery_tabs);
    if verbose {
        eprintln!("[startup]   tab creation: {:.1?}", phase.elapsed());
    }

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

    phase = Instant::now();
    let mut app = App::with_tabs(tabs, preset_registry);
    app.startup_time = Some(startup);
    app.verbose = verbose;
    app.theme = cfg.theme;
    app.source_renderer_map = source_renderer_map;
    app.ensure_combined_tabs();

    // Restore last active source from session
    let project_root = discovery.project_root.as_deref();
    restore_last_source(&mut app, project_root);

    // Optionally set up directory watcher for new sources
    // Watch project data dir if in project, otherwise global
    let dir_watcher = if watch {
        let watch_dir = if discovery.project_root.is_some() {
            source::resolve_data_dir(discovery)
        } else {
            source::data_dir()
        };
        watch_dir.and_then(|p| watcher::DirectoryWatcher::new(p).ok())
    } else {
        None
    };

    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    if verbose {
        eprintln!("[startup]   terminal setup: {:.1?}", phase.elapsed());
    }

    // Determine watched location for newly discovered sources
    let watched_location = if discovery.project_root.is_some() {
        Some(source::SourceLocation::Project)
    } else {
        Some(source::SourceLocation::Global)
    };

    // Main loop with directory watcher
    let res = run_app_with_discovery(&mut terminal, &mut app, dir_watcher, watched_location);

    // Save active source to session
    save_active_source(&app, project_root);

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if app.verbose {
        if let Some(elapsed) = app.first_render_elapsed {
            eprintln!("[startup] First render in {:.1?}", elapsed);
        }
    }

    if let Err(err) = res {
        eprintln!("Error: {:?}", err);
    }

    Ok(())
}

/// Restore the last active source from session, selecting the matching tab.
fn restore_last_source(app: &mut App, project_root: Option<&std::path::Path>) {
    if let Some(last_name) = session::load_last_source(project_root) {
        let categories = app.tabs_by_category();
        for (_, tab_indices) in &categories {
            for &tab_idx in tab_indices {
                if app.tabs[tab_idx].source.name == last_name {
                    app.select_tab(tab_idx);
                    return;
                }
            }
        }
    }
}

/// Save the active source name to session.
fn save_active_source(app: &App, project_root: Option<&std::path::Path>) {
    let name = &app.tabs[app.active_tab].source.name;
    session::save_last_source(project_root, name);
}

fn run_app<B: ratatui::backend::Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    run_app_with_discovery(terminal, app, None, None)
}

/// Run the app with optional directory watcher for source discovery mode
fn run_app_with_discovery<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    dir_watcher: Option<watcher::DirectoryWatcher>,
    watched_location: Option<source::SourceLocation>,
) -> Result<()> {
    let mut last_status_refresh = Instant::now();
    let mut last_file_poll = Instant::now();
    loop {
        // Phase 1: Render
        render(terminal, app)?;

        if let Some(start) = app.startup_time.take() {
            app.first_render_elapsed = Some(start.elapsed());
        }

        // Phase 2: Check for pending debounced filter
        if let Some(trigger_at) = app.pending_filter_at {
            if Instant::now() >= trigger_at {
                app.pending_filter_at = None;
                FilterOrchestrator::trigger_preview(app);
            }
        }

        // Phase 2.5: Refresh source status for discovered sources (throttled to every 2s)
        if last_status_refresh.elapsed() >= Duration::from_secs(2) {
            last_status_refresh = Instant::now();
            for tab in &mut app.tabs {
                tab.refresh_source_status();
            }
        }

        // Phase 2.6: Check for new sources from directory watcher
        if let Some(ref watcher) = dir_watcher {
            while let Some(dir_event) = watcher.try_recv() {
                match dir_event {
                    watcher::DirEvent::NewFile(path) => {
                        // Check if we already have this file open
                        let already_open = app
                            .tabs
                            .iter()
                            .any(|t| t.source.source_path.as_ref() == Some(&path));
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
                                let renderers = app
                                    .source_renderer_map
                                    .get(&source.name)
                                    .cloned()
                                    .unwrap_or_default();
                                if let Ok(tab) =
                                    TabState::from_discovered_source(source, true, renderers)
                                {
                                    app.add_tab(tab);
                                    app.ensure_combined_tabs();
                                }
                            }
                        }
                    }
                    watcher::DirEvent::FileRemoved(_path) => {
                        // Optionally handle file removal (don't close tab, just mark as unavailable)
                    }
                }
            }
        }

        // Phase 2.7: Periodic file size poll — safety net for platforms where
        // the file watcher may not deliver events reliably (e.g. macOS FSEvents).
        let force_poll = if last_file_poll.elapsed() >= Duration::from_secs(1) {
            last_file_poll = Instant::now();
            true
        } else {
            false
        };

        // Phase 3: Collect events from all sources
        let mut events = Vec::new();
        events.extend(collect_file_events(app, force_poll));
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
        if let Err(e) = tui::render(f, app) {
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

/// Collect file watcher events from all tabs.
/// When `force_poll` is true, also checks file sizes directly as a safety net
/// for platforms where the file watcher may miss events (e.g. macOS FSEvents).
fn collect_file_events(app: &mut App, force_poll: bool) -> Vec<AppEvent> {
    let active_tab = app.active_tab;

    // First pass: reload files and handle inactive tabs
    let mut active_tab_modification: Option<ActiveTabFileModification> = None;
    let mut any_file_modified = false;

    for (tab_idx, tab) in app.tabs.iter_mut().enumerate() {
        // Drain watcher events
        let mut has_modified = false;
        let mut last_error = None;
        if let Some(ref watcher) = tab.watcher {
            // Drain all pending events — only the last one matters.
            // The unbounded channel can accumulate thousands of events
            // (one per flush in capture mode), but we only need one reload.
            while let Some(file_event) = watcher.try_recv() {
                match file_event {
                    watcher::FileEvent::Modified => has_modified = true,
                    watcher::FileEvent::Error(err) => last_error = Some(err),
                }
            }
        }

        // Periodic file size poll: if the watcher didn't fire but the file grew,
        // treat it as a modification. This catches cases where the OS file watcher
        // fails to deliver events (common on macOS with FSEvents).
        if !has_modified && force_poll {
            if let Some(ref path) = tab.source.source_path {
                if let Ok(meta) = std::fs::metadata(path) {
                    let current_size = meta.len();
                    if tab.source.file_size.is_some_and(|s| current_size != s) {
                        has_modified = true;
                    }
                }
            }
        }

        if let Some(err) = last_error {
            eprintln!("File watcher error for tab {}: {}", tab_idx, err);
        }

        if has_modified {
            any_file_modified = true;
            let mut reader_guard = tab
                .source
                .reader
                .lock()
                .expect("Reader lock poisoned - filter thread panicked");

            if let Err(e) = reader_guard.reload() {
                eprintln!("Failed to reload file for tab {}: {}", tab_idx, e);
                continue;
            }

            let new_total = reader_guard.total_lines();
            let old_total = tab.source.total_lines;
            drop(reader_guard);

            // Update file size
            if let Some(ref path) = tab.source.source_path {
                tab.source.file_size = std::fs::metadata(path).map(|m| m.len()).ok();
            }

            if tab_idx == active_tab && app.active_combined.is_none() {
                // Collect for processing after the loop (only when a regular tab is active)
                active_tab_modification = Some(ActiveTabFileModification {
                    new_total,
                    old_total,
                });
            } else {
                // Inactive tab: update state directly
                tab.apply_file_modification(new_total);
            }
        }
    }

    // Propagate file changes to combined tabs
    if any_file_modified {
        for cat_idx in 0..5 {
            if let Some(ref mut combined) = app.combined_tabs[cat_idx] {
                let old_total = combined.source.total_lines;
                {
                    let mut reader = combined
                        .source
                        .reader
                        .lock()
                        .expect("Combined reader lock poisoned");
                    if let Err(e) = reader.reload() {
                        eprintln!(
                            "Failed to reload combined reader for cat {}: {}",
                            cat_idx, e
                        );
                        continue;
                    }
                    let new_total = reader.total_lines();
                    drop(reader);

                    if new_total != old_total {
                        combined.source.total_lines = new_total;
                        if combined.source.mode == ViewMode::Normal {
                            combined.source.line_indices = (0..new_total).collect();
                        }

                        // Follow mode jump for active combined tab
                        let is_active_combined =
                            app.active_combined == Some(SourceType::from_index(cat_idx));
                        if is_active_combined
                            && combined.source.follow_mode
                            && combined.source.mode == ViewMode::Normal
                        {
                            let len = combined.source.line_indices.len();
                            combined.viewport.jump_to_end(&combined.source.line_indices);
                            if len > 0 {
                                combined.selected_line = len - 1;
                            }
                        }
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

/// Collect filter progress from all tabs (regular + combined)
fn collect_filter_progress(app: &mut App) -> Vec<AppEvent> {
    let mut events = Vec::new();
    let active_tab = app.active_tab;
    let active_combined = app.active_combined;

    // Regular tabs
    for (tab_idx, tab) in app.tabs.iter_mut().enumerate() {
        if let Some(ref rx) = tab.source.filter.receiver {
            match rx.try_recv() {
                Ok(progress) => {
                    let is_incremental = tab.source.filter.is_incremental;
                    let filter_events =
                        handlers::filter::handle_filter_progress(progress, is_incremental);

                    if tab_idx == active_tab && active_combined.is_none() {
                        // Active tab: check for completion and collect events
                        let completed = filter_events.iter().any(|e| {
                            matches!(
                                e,
                                AppEvent::FilterComplete { .. } | AppEvent::FilterError(_)
                            )
                        });
                        events.extend(filter_events);
                        if completed {
                            tab.source.filter.receiver = None;
                        }
                    } else {
                        // Inactive tab: apply filter events directly
                        for ev in &filter_events {
                            if tab.apply_filter_event(ev) {
                                tab.source.filter.receiver = None;
                            }
                        }
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    tab.source.filter.receiver = None;
                    if matches!(tab.source.filter.state, FilterState::Processing { .. }) {
                        tab.source.filter.state = FilterState::Inactive;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
    }

    // Combined tabs
    for cat_idx in 0..5 {
        if let Some(ref mut combined) = app.combined_tabs[cat_idx] {
            if let Some(ref rx) = combined.source.filter.receiver {
                match rx.try_recv() {
                    Ok(progress) => {
                        let is_incremental = combined.source.filter.is_incremental;
                        let filter_events =
                            handlers::filter::handle_filter_progress(progress, is_incremental);

                        let is_active = active_combined == Some(SourceType::from_index(cat_idx));
                        if is_active {
                            let completed = filter_events.iter().any(|e| {
                                matches!(
                                    e,
                                    AppEvent::FilterComplete { .. } | AppEvent::FilterError(_)
                                )
                            });
                            events.extend(filter_events);
                            if completed {
                                combined.source.filter.receiver = None;
                            }
                        } else {
                            for ev in &filter_events {
                                if combined.apply_filter_event(ev) {
                                    combined.source.filter.receiver = None;
                                }
                            }
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        combined.source.filter.receiver = None;
                        if matches!(combined.source.filter.state, FilterState::Processing { .. }) {
                            combined.source.filter.state = FilterState::Inactive;
                        }
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
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
) -> Result<Vec<AppEvent>> {
    use crossterm_event::MouseEventKind;

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
                MouseEventKind::Down(crossterm_event::MouseButton::Left) => {
                    events.push(AppEvent::MouseClick {
                        column: mouse_event.column,
                        row: mouse_event.row,
                    });
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
fn process_event(app: &mut App, event: AppEvent) {
    app.apply_event(event);
}

/// Spawn a background thread to check for updates (if enabled).
#[cfg(feature = "self-update")]
fn spawn_update_check(
    cli: &Cli,
    cfg: &config::Config,
) -> Option<std::thread::JoinHandle<Result<update::UpdateInfo, String>>> {
    // Respect --no-update-check flag
    if cli.no_update_check {
        return None;
    }
    // Respect config: update_check: false
    if cfg.update_check == Some(false) {
        return None;
    }
    Some(std::thread::spawn(update::checker::check_with_cache))
}

/// Print a subtle update notice after TUI exits (if an update is available).
#[cfg(feature = "self-update")]
fn print_update_notice(
    handle: Option<std::thread::JoinHandle<Result<update::UpdateInfo, String>>>,
) {
    let Some(handle) = handle else { return };
    let Ok(Ok(info)) = handle.join() else { return };
    if !info.is_update_available() {
        return;
    }

    let install_method = update::detection::detect_install_method();
    match install_method {
        update::InstallMethod::PackageManager { upgrade_cmd, .. } => {
            eprintln!(
                "Update available: {} → {}. Update with: {}",
                info.current_version, info.latest_version, upgrade_cmd
            );
        }
        update::InstallMethod::SelfManaged => {
            eprintln!(
                "Update available: {} → {}. Run 'lazytail update' to install.",
                info.current_version, info.latest_version
            );
        }
    }
}
