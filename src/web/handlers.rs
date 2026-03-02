//! HTTP request routing and response helpers for the web server.

use crate::ansi::strip_ansi;
use crate::app::TabState;
use crate::filter::orchestrator::FilterOrchestrator;
use crate::filter::query;
use crate::filter::regex_filter::RegexFilter;
use crate::source::SourceStatus;

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tiny_http::{Header, Method, Response, StatusCode};

use super::state::{lock_state, PendingEventRequest, WebState};
use super::{
    BasicResponse, BodyReadError, CloseSourceRequest, FilterRequest, FollowRequest, LineRow,
    LinesResponse, SourceRequest, INDEX_HTML, MAX_LINES_PER_REQUEST, MAX_PENDING_EVENT_REQUESTS,
    MAX_REQUEST_BODY_SIZE,
};

pub(super) fn handle_request(request: tiny_http::Request, shared: &Arc<Mutex<WebState>>) {
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
                            .and_then(|s| s.label()),
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
            if let Err(e) =
                FilterOrchestrator::trigger(&mut tab.source, trimmed_pattern, mode, None)
            {
                respond_json_error(request, 400, e);
                return;
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

// --- HTTP response helpers ---

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

pub(super) fn respond_events(request: tiny_http::Request, next_revision: Option<u64>) {
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

fn to_json_string<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string())
}

// --- Request parsing helpers ---

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

fn read_body(request: &mut tiny_http::Request) -> Result<String, BodyReadError> {
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

// --- Business logic helpers ---

fn delete_ended_source(tab: &TabState, state: &WebState) -> anyhow::Result<()> {
    use anyhow::Context;

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
