use crate::app::TabState;
use crate::app::{FilterState, SourceType, ViewMode};
use crate::cli::WebArgs;
use crate::config;
use crate::filter::engine::FilterProgress;
use crate::filter::orchestrator::FilterOrchestrator;
use crate::filter::query;
use crate::filter::regex_filter::RegexFilter;
use crate::filter::FilterMode;
use crate::index::flags::Severity;
use crate::signal::setup_shutdown_handlers;
use crate::source::{self, SourceLocation, SourceStatus};
use crate::watcher::FileEvent;
use crate::watcher::{DirEvent, DirectoryWatcher};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tiny_http::{Header, Method, Response, Server, StatusCode};

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

use crate::ansi::strip_ansi;

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

struct PendingEventRequest {
    request: tiny_http::Request,
    since: u64,
    started_at: Instant,
}

struct WebState {
    tabs: Vec<TabState>,
    dir_watcher: Option<DirectoryWatcher>,
    watched_location: Option<SourceLocation>,
    project_data_dir: Option<PathBuf>,
    global_data_dir: Option<PathBuf>,
    watch_enabled: bool,
    revision: u64,
    pending_event_requests: Vec<PendingEventRequest>,
}

impl WebState {
    fn new(
        tabs: Vec<TabState>,
        dir_watcher: Option<DirectoryWatcher>,
        watched_location: Option<SourceLocation>,
        project_data_dir: Option<PathBuf>,
        global_data_dir: Option<PathBuf>,
        watch_enabled: bool,
    ) -> Self {
        Self {
            tabs,
            dir_watcher,
            watched_location,
            project_data_dir,
            global_data_dir,
            watch_enabled,
            revision: 1,
            pending_event_requests: Vec::new(),
        }
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    fn tick(&mut self) {
        let mut changed = false;

        changed |= self.process_directory_events();
        changed |= self.process_file_events();
        changed |= self.process_filter_progress();
        changed |= self.refresh_source_statuses();
        self.process_pending_event_requests();

        if changed {
            self.bump_revision();
            self.process_pending_event_requests();
        }
    }

    fn process_pending_event_requests(&mut self) {
        if self.pending_event_requests.is_empty() {
            return;
        }

        let now = Instant::now();
        let mut remaining = Vec::with_capacity(self.pending_event_requests.len());

        for pending in self.pending_event_requests.drain(..) {
            if self.revision > pending.since {
                respond_events(pending.request, Some(self.revision));
                continue;
            }

            if now.duration_since(pending.started_at) >= EVENTS_WAIT_TIMEOUT {
                respond_events(pending.request, None);
            } else {
                remaining.push(pending);
            }
        }

        self.pending_event_requests = remaining;
    }

    fn process_directory_events(&mut self) -> bool {
        let Some(ref watcher) = self.dir_watcher else {
            return false;
        };

        let mut changed = false;

        while let Some(event) = watcher.try_recv() {
            match event {
                DirEvent::NewFile(path) => {
                    let already_open = self
                        .tabs
                        .iter()
                        .any(|t| t.source.source_path.as_ref() == Some(&path));
                    if already_open {
                        continue;
                    }

                    if let Some(stem) = path.file_stem() {
                        let name = stem.to_string_lossy().to_string();
                        let status = path
                            .parent()
                            .and_then(|d| d.parent())
                            .map(|base| base.join("sources"))
                            .filter(|s| s.exists())
                            .map(|s| source::check_source_status_in_dir(&name, &s))
                            .unwrap_or_else(|| source::check_source_status(&name));

                        let discovered = source::DiscoveredSource {
                            name,
                            log_path: path,
                            status,
                            location: self.watched_location.unwrap_or(SourceLocation::Global),
                        };

                        if let Ok(tab) =
                            TabState::from_discovered_source(discovered, self.watch_enabled)
                        {
                            self.tabs.push(tab);
                            changed = true;
                        }
                    }
                }
                DirEvent::FileRemoved(_) => {
                    // Keep tabs open for historical navigation.
                }
            }
        }

        changed
    }

    fn process_file_events(&mut self) -> bool {
        let mut changed = false;

        for tab in &mut self.tabs {
            // Drain all pending events — only reload once per cycle.
            let mut has_modified = false;
            if let Some(ref watcher) = tab.watcher {
                while let Some(file_event) = watcher.try_recv() {
                    match file_event {
                        FileEvent::Modified => has_modified = true,
                        FileEvent::Error(err) => {
                            eprintln!("[web] Watcher error for '{}': {}", tab.source.name, err);
                        }
                    }
                }
            }

            // Fallback: check file size directly if watcher didn't fire.
            // Catches cases where the OS file watcher misses events (macOS FSEvents).
            if !has_modified {
                if let Some(ref path) = tab.source.source_path {
                    if let Ok(meta) = std::fs::metadata(path) {
                        let current_size = meta.len();
                        if tab.source.file_size.is_some_and(|s| current_size != s) {
                            has_modified = true;
                        }
                    }
                }
            }

            if has_modified {
                let old_total = tab.source.total_lines;
                let mut reader = match tab.source.reader.lock() {
                    Ok(guard) => guard,
                    Err(poisoned) => poisoned.into_inner(),
                };

                if let Err(err) = reader.reload() {
                    eprintln!("[web] Failed to reload '{}': {}", tab.source.name, err);
                    continue;
                }

                let new_total = reader.total_lines();
                drop(reader);

                // Update file size so the poll fallback doesn't re-trigger
                if let Some(ref path) = tab.source.source_path {
                    tab.source.file_size = std::fs::metadata(path).map(|m| m.len()).ok();
                }

                if new_total < old_total {
                    reset_tab_after_truncation(tab, new_total);
                    changed = true;
                    continue;
                }

                tab.source.total_lines = new_total;
                if tab.source.mode == ViewMode::Normal {
                    let old = tab.source.line_indices.len();
                    if new_total > old {
                        tab.source.line_indices.extend(old..new_total);
                    }
                }

                if let Some(pattern) = tab.source.filter.pattern.clone() {
                    if new_total > tab.source.filter.last_filtered_line {
                        let mode = tab.source.filter.mode;
                        let range = Some((tab.source.filter.last_filtered_line, new_total));
                        FilterOrchestrator::trigger(&mut tab.source, pattern, mode, range);
                    }
                }

                if tab.source.follow_mode {
                    tab.jump_to_end();
                }

                changed = true;
            }
        }

        changed
    }

    fn process_filter_progress(&mut self) -> bool {
        let mut changed = false;

        for tab in &mut self.tabs {
            loop {
                let recv_result = {
                    let Some(rx) = tab.source.filter.receiver.as_ref() else {
                        break;
                    };
                    rx.try_recv()
                };

                match recv_result {
                    Ok(FilterProgress::Processing(lines_processed)) => {
                        tab.source.filter.state = FilterState::Processing { lines_processed };
                        changed = true;
                    }
                    Ok(FilterProgress::PartialResults {
                        matches,
                        lines_processed,
                    }) => {
                        merge_partial_filter_results(tab, matches, lines_processed);
                        changed = true;
                    }
                    Ok(FilterProgress::Complete {
                        matches,
                        lines_processed: _,
                    }) => {
                        if tab.source.filter.is_incremental {
                            tab.append_filter_results(matches);
                        } else {
                            let pattern = tab.source.filter.pattern.clone().unwrap_or_default();
                            tab.apply_filter(matches, pattern);
                        }

                        if tab.source.follow_mode {
                            tab.jump_to_end();
                        }

                        tab.source.filter.receiver = None;
                        changed = true;
                    }
                    Ok(FilterProgress::Error(err)) => {
                        eprintln!("[web] Filter error for '{}': {}", tab.source.name, err);
                        tab.source.filter.state = FilterState::Inactive;
                        tab.source.filter.receiver = None;
                        changed = true;
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        tab.source.filter.receiver = None;
                        if matches!(tab.source.filter.state, FilterState::Processing { .. }) {
                            tab.source.filter.state = FilterState::Inactive;
                            changed = true;
                        }
                        break;
                    }
                }
            }
        }

        changed
    }

    fn refresh_source_statuses(&mut self) -> bool {
        let mut changed = false;
        for tab in &mut self.tabs {
            let before = tab.source.source_status;
            tab.refresh_source_status();
            if tab.source.source_status != before {
                changed = true;
            }
        }
        changed
    }

    fn as_sources_response(&self) -> SourcesResponse {
        let sources =
            self.tabs
                .iter()
                .enumerate()
                .map(|(id, tab)| SourceView {
                    id,
                    name: tab.source.name.clone(),
                    category: source_type_label(tab.source_type()),
                    disabled: tab.source.disabled,
                    follow_mode: tab.source.follow_mode,
                    source_status: tab.source.source_status.map(source_status_label),
                    total_lines: tab.source.total_lines,
                    visible_lines: tab.source.line_indices.len(),
                    filter_pattern: tab.source.filter.pattern.clone(),
                    filter_mode: match tab.source.filter.mode {
                        FilterMode::Plain { .. } => "plain",
                        FilterMode::Regex { .. } => "regex",
                        FilterMode::Query {} => "query",
                    },
                    case_sensitive: tab.source.filter.mode.is_case_sensitive(),
                    filter_state: filter_state_view(tab.source.filter.state),
                    can_delete_ended: tab.source.source_status == Some(SourceStatus::Ended)
                        && tab.source.source_path.as_ref().is_some_and(|path| {
                            self.is_under_data_roots(path) && !tab.source.disabled
                        }),
                    severity_counts: tab.source.index_reader.as_ref().and_then(|ir| {
                        ir.checkpoints().last().map(|cp| SeverityCountsView {
                            trace: cp.severity_counts.trace,
                            debug: cp.severity_counts.debug,
                            info: cp.severity_counts.info,
                            warn: cp.severity_counts.warn,
                            error: cp.severity_counts.error,
                            fatal: cp.severity_counts.fatal,
                        })
                    }),
                })
                .collect();

        SourcesResponse {
            revision: self.revision,
            sources,
        }
    }

    fn is_under_data_roots(&self, path: &std::path::Path) -> bool {
        self.project_data_dir
            .as_ref()
            .is_some_and(|dir| path.starts_with(dir))
            || self
                .global_data_dir
                .as_ref()
                .is_some_and(|dir| path.starts_with(dir))
    }
}

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
    let server = match Server::http(&bind_addr) {
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
            Ok(Some(request)) => handle_request(request, &shared),
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
            Ok(tab) => tabs.push(tab),
            Err(err) => config_errors.push(format!("Failed to open {}: {}", source.name, err)),
        }
    }

    for source in &cfg.global_sources {
        match TabState::from_config_source(source, SourceType::GlobalSource, watch) {
            Ok(tab) => tabs.push(tab),
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
            if let Ok(tab) = TabState::from_discovered_source(src, watch) {
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

fn handle_request(request: tiny_http::Request, shared: &Arc<Mutex<WebState>>) {
    let mut request = request;
    let url = request.url().to_string();
    let (path, query) = split_url_and_query(&url);

    match (request.method(), path) {
        (&Method::Get, "/") => {
            respond_html(request, INDEX_HTML);
            return;
        }
        (&Method::Get, "/favicon.ico") => {
            respond_plain(request, 204, "");
            return;
        }
        (&Method::Get, "/api/sources") => {
            let mut state = lock_state(shared);
            state.tick();
            let body = to_json_string(&state.as_sources_response());
            respond_json(request, 200, body);
            return;
        }
        (&Method::Get, "/api/events") => {
            let since =
                parse_u64_query(&query, "since").unwrap_or_else(|| read_last_event_id(&request));

            let mut state = lock_state(shared);
            state.tick();
            let revision = state.revision;

            if revision > since {
                drop(state);
                respond_events(request, Some(revision));
            } else if state.pending_event_requests.len() >= MAX_PENDING_EVENT_REQUESTS {
                drop(state);
                respond_events_busy(request);
            } else {
                state.pending_event_requests.push(PendingEventRequest {
                    request,
                    since,
                    started_at: Instant::now(),
                });
            }
            return;
        }
        (&Method::Get, "/api/lines") => {
            let source = parse_usize_query(&query, "source");
            let offset = parse_usize_query(&query, "offset").unwrap_or(0);
            let limit = parse_usize_query(&query, "limit")
                .unwrap_or(200)
                .min(MAX_LINES_PER_REQUEST);

            let Some(source) = source else {
                respond_json_error(request, 400, "Missing 'source' query parameter");
                return;
            };

            let mut state = lock_state(shared);
            state.tick();
            let revision = state.revision;

            let Some(tab) = state.tabs.get_mut(source) else {
                respond_json_error(request, 404, "Source not found");
                return;
            };

            let total_visible = tab.source.line_indices.len();
            let start = offset.min(total_visible);
            let end = (start + limit).min(total_visible);

            let mut reader = match tab.source.reader.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };

            let index_reader = tab.source.index_reader.as_ref();

            let mut rows = Vec::with_capacity(end.saturating_sub(start));
            for visible_index in start..end {
                if let Some(&file_line) = tab.source.line_indices.get(visible_index) {
                    let content = reader
                        .get_line(file_line)
                        .ok()
                        .flatten()
                        .map(|line| strip_ansi(&line))
                        .unwrap_or_default();

                    rows.push(LineRow {
                        visible_index,
                        line_number: file_line + 1,
                        content,
                        severity: index_reader
                            .map(|ir| ir.severity(file_line))
                            .and_then(severity_label),
                    });
                }
            }

            let body = to_json_string(&LinesResponse {
                revision,
                total_visible,
                total_lines: tab.source.total_lines,
                offset: start,
                limit,
                rows,
            });
            respond_json(request, 200, body);
            return;
        }
        (&Method::Post, "/api/filter") => {
            let body = match read_body(&mut request) {
                Ok(body) => body,
                Err(BodyReadError::TooLarge) => {
                    respond_json_error(request, 413, "Request body too large");
                    return;
                }
                Err(BodyReadError::Invalid(err)) => {
                    respond_json_error(request, 400, format!("Invalid request body: {}", err));
                    return;
                }
            };

            let payload: FilterRequest = match serde_json::from_str(&body) {
                Ok(payload) => payload,
                Err(err) => {
                    respond_json_error(request, 400, format!("Invalid JSON payload: {}", err));
                    return;
                }
            };

            let mut state = lock_state(shared);
            state.tick();

            let Some(tab) = state.tabs.get_mut(payload.source) else {
                respond_json_error(request, 404, "Source not found");
                return;
            };

            let mode = payload.mode.into_filter_mode(payload.case_sensitive);
            let trimmed_pattern = payload.pattern;

            if trimmed_pattern.is_empty() {
                if let Some(ref cancel) = tab.source.filter.cancel_token {
                    cancel.cancel();
                }
                tab.source.filter.receiver = None;
                tab.clear_filter();
                state.bump_revision();
                respond_json(
                    request,
                    200,
                    to_json_string(&BasicResponse {
                        ok: true,
                        message: None,
                    }),
                );
                return;
            }

            // Pre-validate pattern before passing to orchestrator
            if mode.is_query() {
                if let Err(err) = query::parse_query(&trimmed_pattern) {
                    respond_json_error(request, 400, format!("Invalid query: {}", err));
                    return;
                }
            } else if mode.is_regex() {
                if let Err(err) = RegexFilter::new(&trimmed_pattern, mode.is_case_sensitive()) {
                    respond_json_error(request, 400, format!("Invalid regex pattern: {}", err));
                    return;
                }
            }

            tab.source.filter.pattern = Some(trimmed_pattern.clone());
            tab.source.filter.mode = mode;
            FilterOrchestrator::trigger(&mut tab.source, trimmed_pattern, mode, None);
            state.bump_revision();
            respond_json(
                request,
                200,
                to_json_string(&BasicResponse {
                    ok: true,
                    message: None,
                }),
            );

            return;
        }
        (&Method::Post, "/api/filter/clear") => {
            let body = match read_body(&mut request) {
                Ok(body) => body,
                Err(BodyReadError::TooLarge) => {
                    respond_json_error(request, 413, "Request body too large");
                    return;
                }
                Err(BodyReadError::Invalid(err)) => {
                    respond_json_error(request, 400, format!("Invalid request body: {}", err));
                    return;
                }
            };

            let payload: SourceRequest = match serde_json::from_str(&body) {
                Ok(payload) => payload,
                Err(err) => {
                    respond_json_error(request, 400, format!("Invalid JSON payload: {}", err));
                    return;
                }
            };

            let mut state = lock_state(shared);
            state.tick();

            let Some(tab) = state.tabs.get_mut(payload.source) else {
                respond_json_error(request, 404, "Source not found");
                return;
            };

            if let Some(ref cancel) = tab.source.filter.cancel_token {
                cancel.cancel();
            }
            tab.source.filter.receiver = None;
            tab.clear_filter();
            state.bump_revision();

            respond_json(
                request,
                200,
                to_json_string(&BasicResponse {
                    ok: true,
                    message: None,
                }),
            );
            return;
        }
        (&Method::Post, "/api/follow") => {
            let body = match read_body(&mut request) {
                Ok(body) => body,
                Err(BodyReadError::TooLarge) => {
                    respond_json_error(request, 413, "Request body too large");
                    return;
                }
                Err(BodyReadError::Invalid(err)) => {
                    respond_json_error(request, 400, format!("Invalid request body: {}", err));
                    return;
                }
            };

            let payload: FollowRequest = match serde_json::from_str(&body) {
                Ok(payload) => payload,
                Err(err) => {
                    respond_json_error(request, 400, format!("Invalid JSON payload: {}", err));
                    return;
                }
            };

            let mut state = lock_state(shared);
            state.tick();

            let Some(tab) = state.tabs.get_mut(payload.source) else {
                respond_json_error(request, 404, "Source not found");
                return;
            };

            tab.source.follow_mode = payload.enabled;
            if tab.source.follow_mode {
                tab.jump_to_end();
            }
            state.bump_revision();

            respond_json(
                request,
                200,
                to_json_string(&BasicResponse {
                    ok: true,
                    message: None,
                }),
            );
            return;
        }
        (&Method::Post, "/api/source/close") => {
            let body = match read_body(&mut request) {
                Ok(body) => body,
                Err(BodyReadError::TooLarge) => {
                    respond_json_error(request, 413, "Request body too large");
                    return;
                }
                Err(BodyReadError::Invalid(err)) => {
                    respond_json_error(request, 400, format!("Invalid request body: {}", err));
                    return;
                }
            };

            let payload: CloseSourceRequest = match serde_json::from_str(&body) {
                Ok(payload) => payload,
                Err(err) => {
                    respond_json_error(request, 400, format!("Invalid JSON payload: {}", err));
                    return;
                }
            };

            let mut state = lock_state(shared);
            state.tick();

            if payload.source >= state.tabs.len() {
                respond_json_error(request, 404, "Source not found");
                return;
            }

            if payload.delete_ended {
                let tab_ref = &state.tabs[payload.source];
                if let Err(err) = delete_ended_source(tab_ref, &state) {
                    respond_json(
                        request,
                        400,
                        to_json_string(&BasicResponse {
                            ok: false,
                            message: Some(err.to_string()),
                        }),
                    );
                    return;
                }
            }

            let mut tab = state.tabs.remove(payload.source);
            if let Some(ref cancel) = tab.source.filter.cancel_token {
                cancel.cancel();
            }
            tab.source.filter.receiver = None;

            state.bump_revision();

            respond_json(
                request,
                200,
                to_json_string(&BasicResponse {
                    ok: true,
                    message: None,
                }),
            );
            return;
        }
        _ => {}
    }

    respond_json_error(request, 404, "Not found");
}

fn lock_state<'a>(shared: &'a Arc<Mutex<WebState>>) -> std::sync::MutexGuard<'a, WebState> {
    match shared.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn split_url_and_query(url: &str) -> (&str, HashMap<String, String>) {
    if let Some(idx) = url.find('?') {
        (&url[..idx], parse_query_params(&url[idx + 1..]))
    } else {
        (url, HashMap::new())
    }
}

fn parse_query_params(query: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        if let Some((k, v)) = pair.split_once('=') {
            out.insert(k.to_string(), v.to_string());
        } else {
            out.insert(pair.to_string(), String::new());
        }
    }
    out
}

fn parse_usize_query(query: &HashMap<String, String>, key: &str) -> Option<usize> {
    query.get(key).and_then(|s| s.parse::<usize>().ok())
}

fn parse_u64_query(query: &HashMap<String, String>, key: &str) -> Option<u64> {
    query.get(key).and_then(|s| s.parse::<u64>().ok())
}

fn read_last_event_id(request: &tiny_http::Request) -> u64 {
    request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Last-Event-ID"))
        .and_then(|h| h.value.as_str().parse::<u64>().ok())
        .unwrap_or(0)
}

fn read_body(request: &mut tiny_http::Request) -> std::result::Result<String, BodyReadError> {
    if let Some(content_length) = request
        .headers()
        .iter()
        .find(|h| h.field.equiv("Content-Length"))
        .and_then(|h| h.value.as_str().parse::<u64>().ok())
    {
        if content_length > MAX_REQUEST_BODY_SIZE as u64 {
            return Err(BodyReadError::TooLarge);
        }
    }

    let mut body = String::new();
    let mut reader = request.as_reader().take((MAX_REQUEST_BODY_SIZE as u64) + 1);
    reader
        .read_to_string(&mut body)
        .map_err(|err| BodyReadError::Invalid(err.to_string()))?;

    if body.len() > MAX_REQUEST_BODY_SIZE {
        return Err(BodyReadError::TooLarge);
    }

    Ok(body)
}

fn respond_html(request: tiny_http::Request, body: &str) {
    let response = make_response(200, "text/html; charset=utf-8", body.to_string());
    let _ = request.respond(response);
}

fn respond_json(request: tiny_http::Request, status: u16, body: String) {
    let response = make_response(status, "application/json; charset=utf-8", body);
    let _ = request.respond(response);
}

fn respond_json_error(request: tiny_http::Request, status: u16, message: impl Into<String>) {
    let body = to_json_string(&BasicResponse {
        ok: false,
        message: Some(message.into()),
    });
    respond_json(request, status, body);
}

fn respond_plain(request: tiny_http::Request, status: u16, body: &str) {
    let response = make_response(status, "text/plain; charset=utf-8", body.to_string());
    let _ = request.respond(response);
}

fn make_response(
    status: u16,
    content_type: &str,
    body: String,
) -> Response<std::io::Cursor<Vec<u8>>> {
    let response = Response::from_string(body).with_status_code(StatusCode(status));
    match Header::from_bytes("Content-Type", content_type) {
        Ok(header) => response.with_header(header),
        Err(_) => response,
    }
}

fn respond_events(request: tiny_http::Request, next_revision: Option<u64>) {
    let body = match next_revision {
        Some(next) => format!(
            "retry: 250\nid: {}\nevent: revision\ndata: {}\n\n",
            next, next
        ),
        None => "retry: 250\n: keepalive\n\n".to_string(),
    };

    let mut response = Response::from_string(body).with_status_code(StatusCode(200));
    let mut headers = Vec::new();
    if let Ok(header) = Header::from_bytes("Content-Type", "text/event-stream; charset=utf-8") {
        headers.push(header);
    }
    if let Ok(header) = Header::from_bytes("Cache-Control", "no-cache") {
        headers.push(header);
    }
    if let Ok(header) = Header::from_bytes("X-Accel-Buffering", "no") {
        headers.push(header);
    }
    for header in headers {
        response = response.with_header(header);
    }

    let _ = request.respond(response);
}

fn respond_events_busy(request: tiny_http::Request) {
    let mut response = make_response(
        503,
        "text/plain; charset=utf-8",
        "Too many pending event requests".to_string(),
    );
    if let Ok(header) = Header::from_bytes("Retry-After", "1") {
        response = response.with_header(header);
    }
    let _ = request.respond(response);
}

fn to_json_string<T: Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

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

fn severity_label(severity: Severity) -> Option<&'static str> {
    match severity {
        Severity::Trace => Some("trace"),
        Severity::Debug => Some("debug"),
        Severity::Info => Some("info"),
        Severity::Warn => Some("warn"),
        Severity::Error => Some("error"),
        Severity::Fatal => Some("fatal"),
        Severity::Unknown => None,
    }
}

fn delete_ended_source(tab: &TabState, state: &WebState) -> Result<()> {
    if tab.source.source_status != Some(SourceStatus::Ended) {
        anyhow::bail!("Only ended captured sources can be deleted");
    }

    let path = tab
        .source
        .source_path
        .as_ref()
        .context("Source has no file path")?;

    if !state.is_under_data_roots(path) {
        anyhow::bail!("Cannot delete source outside lazytail data directories");
    }

    // Resolve canonical path and re-check to prevent TOCTOU symlink attacks
    if path.exists() {
        let canonical = path.canonicalize().context("Cannot resolve source path")?;
        if !state.is_under_data_roots(&canonical) {
            anyhow::bail!("Resolved path is outside lazytail data directories");
        }
        fs::remove_file(&canonical)
            .with_context(|| format!("Failed to delete source file: {}", canonical.display()))?;
    }

    if let Some(marker_path) = path
        .parent()
        .and_then(|data_dir| data_dir.parent())
        .map(|root| root.join("sources").join(&tab.source.name))
    {
        if marker_path.exists() {
            let _ = fs::remove_file(marker_path);
        }
    }

    Ok(())
}

fn reset_tab_after_truncation(tab: &mut TabState, new_total: usize) {
    if let Some(ref cancel) = tab.source.filter.cancel_token {
        cancel.cancel();
    }

    tab.source.total_lines = new_total;
    tab.source.line_indices = (0..new_total).collect();
    tab.source.mode = ViewMode::Normal;

    tab.source.filter.pattern = None;
    tab.source.filter.state = FilterState::Inactive;
    tab.source.filter.last_filtered_line = 0;
    tab.source.filter.cancel_token = None;
    tab.source.filter.receiver = None;
    tab.source.filter.needs_clear = false;
    tab.source.filter.is_incremental = false;

    if new_total > 0 {
        tab.jump_to_end();
    } else {
        tab.jump_to_start();
    }
}

fn merge_partial_filter_results(
    tab: &mut TabState,
    new_indices: Vec<usize>,
    lines_processed: usize,
) {
    if tab.source.filter.needs_clear {
        tab.source.mode = ViewMode::Filtered;
        tab.source.line_indices.clear();
        tab.source.filter.needs_clear = false;
    } else if tab.source.mode == ViewMode::Normal {
        tab.source.mode = ViewMode::Filtered;
        tab.source.line_indices.clear();
    }

    if tab.source.line_indices.is_empty() {
        tab.source.line_indices = new_indices;
        tab.viewport.jump_to_end(&tab.source.line_indices);
    } else {
        let first_existing = tab.source.line_indices[0];
        let prepended_count = new_indices
            .iter()
            .filter(|&&idx| idx < first_existing)
            .count();

        let mut merged = Vec::with_capacity(tab.source.line_indices.len() + new_indices.len());
        let mut i = 0;
        let mut j = 0;

        while i < tab.source.line_indices.len() && j < new_indices.len() {
            if tab.source.line_indices[i] <= new_indices[j] {
                merged.push(tab.source.line_indices[i]);
                i += 1;
            } else {
                merged.push(new_indices[j]);
                j += 1;
            }
        }

        merged.extend_from_slice(&tab.source.line_indices[i..]);
        merged.extend_from_slice(&new_indices[j..]);

        tab.source.line_indices = merged;
        tab.viewport.adjust_scroll_for_prepend(prepended_count);
    }

    tab.source.filter.state = FilterState::Processing { lines_processed };
}
