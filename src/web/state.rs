//! Web server state management — event processing, file watching, filter progress.

use crate::app::TabState;
use crate::app::{FilterState, ViewMode};
use crate::filter::engine::FilterProgress;
use crate::filter::orchestrator::FilterOrchestrator;
use crate::filter::FilterMode;
use crate::source::{self, SourceLocation, SourceStatus};
use crate::watcher::{DirEvent, DirectoryWatcher, FileEvent};

use std::path::PathBuf;
use std::sync::mpsc::TryRecvError;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use super::handlers::respond_events;
use super::{
    filter_state_view, source_status_label, source_type_label, SeverityCountsView, SourceView,
    SourcesResponse, EVENTS_WAIT_TIMEOUT,
};

pub(super) struct PendingEventRequest {
    pub(super) request: tiny_http::Request,
    pub(super) since: u64,
    pub(super) started_at: Instant,
}

pub(super) struct WebState {
    pub(super) tabs: Vec<TabState>,
    pub(super) dir_watcher: Option<DirectoryWatcher>,
    pub(super) watched_location: Option<SourceLocation>,
    pub(super) project_data_dir: Option<PathBuf>,
    pub(super) global_data_dir: Option<PathBuf>,
    pub(super) watch_enabled: bool,
    pub(super) revision: u64,
    pub(super) pending_event_requests: Vec<PendingEventRequest>,
}

impl WebState {
    pub(super) fn new(
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

    pub(super) fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    pub(super) fn tick(&mut self) {
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

                        if let Ok(tab) = TabState::from_discovered_source(
                            discovered,
                            self.watch_enabled,
                            Vec::new(),
                        ) {
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
                    tab.reset_after_truncation(new_total);
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
                        tab.merge_partial_filter_results(matches, lines_processed);
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

    pub(super) fn as_sources_response(&self) -> SourcesResponse {
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

    pub(super) fn is_under_data_roots(&self, path: &std::path::Path) -> bool {
        self.project_data_dir
            .as_ref()
            .is_some_and(|dir| path.starts_with(dir))
            || self
                .global_data_dir
                .as_ref()
                .is_some_and(|dir| path.starts_with(dir))
    }
}

pub(super) fn lock_state(shared: &Arc<Mutex<WebState>>) -> std::sync::MutexGuard<'_, WebState> {
    match shared.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}
