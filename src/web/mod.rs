mod handlers;
mod state;

use crate::app::TabState;
use crate::app::{FilterState, SourceType};
use crate::cli::WebArgs;
use crate::config;
use crate::filter::FilterMode;
use crate::signal::setup_shutdown_handlers;
use crate::source::{self, SourceLocation, SourceStatus};
use crate::watcher::DirectoryWatcher;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use state::{lock_state, WebState};

const INDEX_HTML: &str = include_str!("index.html");
const MAX_LINES_PER_REQUEST: usize = 5_000;
const MAX_REQUEST_BODY_SIZE: usize = 1024 * 1024;
const MAX_PENDING_EVENT_REQUESTS: usize = 256;
const TICK_INTERVAL_MS: u64 = 150;
const EVENTS_WAIT_TIMEOUT: Duration = Duration::from_secs(25);

type InitialTabsBuild = (
    Vec<TabState>,
    Option<DirectoryWatcher>,
    Option<SourceLocation>,
    Option<PathBuf>,
    Option<PathBuf>,
);

// --- Serde types for API responses ---

#[derive(Serialize)]
struct SourcesResponse {
    revision: u64,
    sources: Vec<SourceView>,
}

#[derive(Serialize)]
struct SourceView {
    id: usize,
    name: String,
    category: &'static str,
    disabled: bool,
    follow_mode: bool,
    source_status: Option<&'static str>,
    total_lines: usize,
    visible_lines: usize,
    filter_pattern: Option<String>,
    filter_mode: &'static str,
    case_sensitive: bool,
    filter_state: FilterStateView,
    can_delete_ended: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    severity_counts: Option<SeverityCountsView>,
}

#[derive(Serialize)]
#[serde(tag = "kind")]
enum FilterStateView {
    #[serde(rename = "inactive")]
    Inactive,
    #[serde(rename = "processing")]
    Processing { lines_processed: usize },
    #[serde(rename = "complete")]
    Complete { matches: usize },
}

#[derive(Serialize)]
struct SeverityCountsView {
    trace: u32,
    debug: u32,
    info: u32,
    warn: u32,
    error: u32,
    fatal: u32,
}

#[derive(Serialize)]
struct LinesResponse {
    revision: u64,
    total_visible: usize,
    total_lines: usize,
    offset: usize,
    limit: usize,
    rows: Vec<LineRow>,
}

#[derive(Serialize)]
struct LineRow {
    visible_index: usize,
    line_number: usize,
    content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    severity: Option<&'static str>,
}

#[derive(Serialize)]
struct BasicResponse {
    ok: bool,
    message: Option<String>,
}

// --- Serde types for API requests ---

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum WebFilterMode {
    Plain,
    Regex,
    Query,
}

impl WebFilterMode {
    fn into_filter_mode(self, case_sensitive: bool) -> FilterMode {
        match self {
            WebFilterMode::Plain => FilterMode::Plain { case_sensitive },
            WebFilterMode::Regex => FilterMode::Regex { case_sensitive },
            WebFilterMode::Query => FilterMode::Query {},
        }
    }
}

#[derive(Deserialize)]
struct FilterRequest {
    source: usize,
    pattern: String,
    mode: WebFilterMode,
    case_sensitive: bool,
}

#[derive(Deserialize)]
struct SourceRequest {
    source: usize,
}

#[derive(Deserialize)]
struct FollowRequest {
    source: usize,
    enabled: bool,
}

#[derive(Deserialize)]
struct CloseSourceRequest {
    source: usize,
    delete_ended: bool,
}

#[derive(Debug)]
enum BodyReadError {
    TooLarge,
    Invalid(String),
}

impl std::fmt::Display for BodyReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BodyReadError::TooLarge => write!(f, "request body too large"),
            BodyReadError::Invalid(msg) => f.write_str(msg),
        }
    }
}

// --- Label / view helpers ---

fn source_type_label(source_type: SourceType) -> &'static str {
    match source_type {
        SourceType::ProjectSource => "project",
        SourceType::GlobalSource => "global-config",
        SourceType::Global => "captured",
        SourceType::File => "file",
        SourceType::Pipe => "pipe",
    }
}

fn source_status_label(status: SourceStatus) -> &'static str {
    match status {
        SourceStatus::Active => "active",
        SourceStatus::Ended => "ended",
    }
}

fn filter_state_view(state: FilterState) -> FilterStateView {
    match state {
        FilterState::Inactive => FilterStateView::Inactive,
        FilterState::Processing { lines_processed } => {
            FilterStateView::Processing { lines_processed }
        }
        FilterState::Complete { matches } => FilterStateView::Complete { matches },
    }
}

// --- Public entry point ---

pub fn run(args: WebArgs) -> Result<(), i32> {
    source::cleanup_stale_markers();

    let watch = !args.no_watch;
    let (tabs, dir_watcher, watched_location, project_data_dir, global_data_dir) =
        match build_initial_tabs(&args.files, watch, args.verbose) {
            Ok(result) => result,
            Err(err) => {
                eprintln!("error: {}", err);
                return Err(1);
            }
        };

    if tabs.is_empty() {
        eprintln!("No log sources found.");
        eprintln!("Options:");
        eprintln!("  1. Create a lazytail.yaml config file in your project");
        eprintln!("  2. Use capture mode: command | lazytail -n <NAME>");
        eprintln!("  3. Specify files directly: lazytail web <FILE>...");
        return Err(1);
    }

    let shared = Arc::new(Mutex::new(WebState::new(
        tabs,
        dir_watcher,
        watched_location,
        project_data_dir,
        global_data_dir,
        watch,
    )));

    let bind_addr = format!("{}:{}", args.host, args.port);
    let server = match tiny_http::Server::http(&bind_addr) {
        Ok(server) => server,
        Err(err) => {
            eprintln!("error: Failed to bind web server on {}: {}", bind_addr, err);
            return Err(1);
        }
    };

    let open_host = if args.host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        &args.host
    };
    let open_url = format!("http://{}:{}/", open_host, args.port);

    println!("LazyTail Web UI started at {}", open_url);
    println!("Press Ctrl+C to stop.");

    let shutdown_flag = match setup_shutdown_handlers() {
        Ok(flag) => flag,
        Err(err) => {
            eprintln!("warning: Failed to set signal handlers: {}", err);
            return Err(1);
        }
    };

    while !shutdown_flag.load(Ordering::SeqCst) {
        match server.recv_timeout(Duration::from_millis(TICK_INTERVAL_MS)) {
            Ok(Some(request)) => handlers::handle_request(request, &shared),
            Ok(None) => {
                let mut state = lock_state(&shared);
                state.tick();
            }
            Err(err) => {
                eprintln!("error: Web server receive error: {}", err);
                return Err(1);
            }
        }
    }

    Ok(())
}

fn build_initial_tabs(files: &[PathBuf], watch: bool, verbose: bool) -> Result<InitialTabsBuild> {
    let (discovery, searched_paths) = config::discovery::discover_verbose();

    if verbose {
        for path in &searched_paths {
            eprintln!("[web][discovery] Searched: {}", path.display());
        }
        eprintln!(
            "[web][discovery] Project root: {}",
            discovery
                .project_root
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not found".to_string())
        );
        eprintln!(
            "[web][discovery] Project config: {}",
            discovery
                .project_config
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not found".to_string())
        );
        eprintln!(
            "[web][discovery] Global config: {}",
            discovery
                .global_config
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not found".to_string())
        );
    }

    let mut config_errors = Vec::new();
    let cfg = match config::load(&discovery) {
        Ok(c) => c,
        Err(err) => {
            config_errors.push(err.to_string());
            config::Config::default()
        }
    };

    let mut tabs = Vec::new();

    for source in &cfg.project_sources {
        match TabState::from_config_source(source, SourceType::ProjectSource, watch) {
            Ok(Some(tab)) => tabs.push(tab),
            Ok(None) => {} // Metadata-only source, skip
            Err(err) => config_errors.push(format!("Failed to open {}: {}", source.name, err)),
        }
    }

    for source in &cfg.global_sources {
        match TabState::from_config_source(source, SourceType::GlobalSource, watch) {
            Ok(Some(tab)) => tabs.push(tab),
            Ok(None) => {} // Metadata-only source, skip
            Err(err) => config_errors.push(format!("Failed to open {}: {}", source.name, err)),
        }
    }

    let mut dir_watcher = None;
    let mut watched_location = None;
    let mut project_data_dir = None;
    let global_data_dir = source::data_dir();

    if files.is_empty() {
        source::ensure_directories_for_context(&discovery)
            .context("Failed to prepare source directories")?;

        if discovery.project_root.is_some() {
            project_data_dir = source::resolve_data_dir(&discovery);
        }

        let discovered = source::discover_sources_for_context(&discovery)
            .context("Failed to discover sources")?;
        for src in discovered {
            if let Ok(tab) = TabState::from_discovered_source(src, watch, Vec::new()) {
                tabs.push(tab);
            }
        }

        if watch {
            let watch_dir = if discovery.project_root.is_some() {
                source::resolve_data_dir(&discovery)
            } else {
                source::data_dir()
            };
            dir_watcher = watch_dir.and_then(|path| DirectoryWatcher::new(path).ok());
            watched_location = if discovery.project_root.is_some() {
                Some(SourceLocation::Project)
            } else {
                Some(SourceLocation::Global)
            };
        }
    } else {
        for path in files {
            tabs.push(
                TabState::new(path.clone(), watch)
                    .with_context(|| format!("Failed to open {}", path.display()))?,
            );
        }
    }

    for err in &config_errors {
        eprintln!("[web][config error] {}", err);
    }

    Ok((
        tabs,
        dir_watcher,
        watched_location,
        project_data_dir,
        global_data_dir,
    ))
}
