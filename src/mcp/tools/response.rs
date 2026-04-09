//! Response processing helpers: ANSI stripping, truncation, and formatting.

use super::super::ansi::strip_ansi;
use super::super::format;
use super::super::types::*;
use crate::filter::engine::FilterProgress;
use crate::renderer::segment::segments_to_plain_text;
use crate::renderer::PresetRegistry;
use std::borrow::Cow;
use std::sync::mpsc::Receiver;

/// Maximum characters per line in MCP output. Lines exceeding this are truncated
/// with a suffix showing the number of hidden characters. Full content is available
/// via narrower `get_context` or `get_lines` calls.
pub(super) const MAX_LINE_LEN: usize = 500;

/// Collect filter results from a progress channel into matching line indices.
pub(super) fn collect_filter_results(
    rx: Receiver<FilterProgress>,
) -> Result<(Vec<usize>, usize), String> {
    let mut matching_indices = Vec::new();
    let mut lines_searched = 0;

    for progress in rx {
        match progress {
            FilterProgress::PartialResults {
                matches,
                lines_processed,
            } => {
                matching_indices.extend(matches);
                lines_searched = lines_processed;
            }
            FilterProgress::Complete {
                matches,
                lines_processed,
            } => {
                matching_indices.extend(matches);
                lines_searched = lines_processed;
            }
            FilterProgress::Processing(n) => {
                lines_searched = n;
            }
            FilterProgress::Error(e) => return Err(e),
        }
    }

    Ok((matching_indices, lines_searched))
}

/// Create a JSON error response string.
/// Errors are always returned as JSON regardless of the requested output format.
pub(super) fn error_response(message: impl std::fmt::Display) -> String {
    serde_json::to_string(&serde_json::json!({ "error": message.to_string() }))
        .unwrap_or_else(|_| r#"{"error": "Failed to serialize error"}"#.to_string())
}

/// Strip ANSI escape codes from a LineInfo's content.
fn strip_line_info(line: &mut LineInfo) {
    line.content = strip_ansi(&line.content);
}

/// Strip ANSI escape codes from all line content in a GetLinesResponse.
pub(super) fn strip_lines_response(resp: &mut GetLinesResponse) {
    for line in &mut resp.lines {
        strip_line_info(line);
    }
}

/// Strip ANSI escape codes from all content in a SearchResponse.
pub(super) fn strip_search_response(resp: &mut SearchResponse) {
    for m in &mut resp.matches {
        m.content = strip_ansi(&m.content);
        for line in &mut m.before {
            *line = strip_ansi(line);
        }
        for line in &mut m.after {
            *line = strip_ansi(line);
        }
    }
}

/// Strip ANSI escape codes from all content in a GetContextResponse.
pub(super) fn strip_context_response(resp: &mut GetContextResponse) {
    for line in &mut resp.before_lines {
        strip_line_info(line);
    }
    strip_line_info(&mut resp.target_line);
    for line in &mut resp.after_lines {
        strip_line_info(line);
    }
}

/// Truncate a single line to `max_len` characters, appending a suffix if truncated.
pub(super) fn truncate_line(line: &str, max_len: usize) -> Cow<'_, str> {
    if line.len() <= max_len {
        Cow::Borrowed(line)
    } else {
        let end = line.floor_char_boundary(max_len);
        let excess = line.len() - end;
        Cow::Owned(format!("{}…[+{} chars]", &line[..end], excess))
    }
}

/// Truncate content fields in a LineInfo.
fn truncate_line_info(line: &mut LineInfo) {
    if line.content.len() > MAX_LINE_LEN {
        line.content = truncate_line(&line.content, MAX_LINE_LEN).into_owned();
    }
    if let Some(ref rendered) = line.rendered {
        if rendered.len() > MAX_LINE_LEN {
            line.rendered = Some(truncate_line(rendered, MAX_LINE_LEN).into_owned());
        }
    }
}

/// Truncate all line content in a GetLinesResponse.
pub(super) fn truncate_lines_response(resp: &mut GetLinesResponse) {
    for line in &mut resp.lines {
        truncate_line_info(line);
    }
}

/// Truncate all content in a SearchResponse.
pub(super) fn truncate_search_response(resp: &mut SearchResponse) {
    for m in &mut resp.matches {
        if m.content.len() > MAX_LINE_LEN {
            m.content = truncate_line(&m.content, MAX_LINE_LEN).into_owned();
        }
        for line in &mut m.before {
            if line.len() > MAX_LINE_LEN {
                *line = truncate_line(line, MAX_LINE_LEN).into_owned();
            }
        }
        for line in &mut m.after {
            if line.len() > MAX_LINE_LEN {
                *line = truncate_line(line, MAX_LINE_LEN).into_owned();
            }
        }
    }
}

/// Truncate all content in a GetContextResponse.
pub(super) fn truncate_context_response(resp: &mut GetContextResponse) {
    for line in &mut resp.before_lines {
        truncate_line_info(line);
    }
    truncate_line_info(&mut resp.target_line);
    for line in &mut resp.after_lines {
        truncate_line_info(line);
    }
}

/// Format a GetLinesResponse according to the requested output format.
pub(super) fn format_lines(resp: &GetLinesResponse, output: OutputFormat) -> String {
    match output {
        OutputFormat::Text => format::format_lines_text(resp),
        OutputFormat::Json => serde_json::to_string_pretty(resp)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e)),
    }
}

/// Format a SearchResponse according to the requested output format.
pub(super) fn format_search(resp: &SearchResponse, output: OutputFormat) -> String {
    match output {
        OutputFormat::Text => format::format_search_text(resp),
        OutputFormat::Json => serde_json::to_string_pretty(resp)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e)),
    }
}

/// Format a GetContextResponse according to the requested output format.
pub(super) fn format_context(resp: &GetContextResponse, output: OutputFormat) -> String {
    match output {
        OutputFormat::Text => format::format_context_text(resp),
        OutputFormat::Json => serde_json::to_string_pretty(resp)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e)),
    }
}

/// Format a GetStatsResponse according to the requested output format.
pub(super) fn format_stats(resp: &GetStatsResponse, output: OutputFormat) -> String {
    match output {
        OutputFormat::Text => format::format_stats_text(resp),
        OutputFormat::Json => serde_json::to_string_pretty(resp)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e)),
    }
}

/// Convert epoch milliseconds to ISO 8601 UTC string (e.g. "2026-04-08T12:34:56Z").
pub(super) fn millis_to_iso8601(ms: u64) -> String {
    let epoch = (ms / 1000) as i64;
    let secs_in_day = epoch.rem_euclid(86400);
    let days = epoch.div_euclid(86400) + 719468;
    let era = days.div_euclid(146097);
    let doe = days.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, hour, minute, second
    )
}

/// Rendering context passed to tool implementations for preset rendering.
pub(super) struct RenderContext<'a> {
    pub registry: &'a PresetRegistry,
    pub renderer_names: &'a [String],
}

/// Attempt preset rendering on a LineInfo, setting `rendered` if a preset matches.
/// Must be called before ANSI stripping since the raw content is needed for field parsing.
pub(super) fn render_line_info(
    line_info: &mut LineInfo,
    raw_content: &str,
    flags: Option<u32>,
    ctx: &RenderContext<'_>,
) {
    if ctx.renderer_names.is_empty() {
        return;
    }
    if let Some(segments) = ctx
        .registry
        .render_line(raw_content, ctx.renderer_names, flags)
    {
        line_info.rendered = Some(segments_to_plain_text(&segments));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn millis_to_iso8601_epoch_zero() {
        assert_eq!(millis_to_iso8601(0), "1970-01-01T00:00:00Z");
    }

    #[test]
    fn millis_to_iso8601_known_date() {
        // 2026-04-08T12:34:56Z = 1775651696 seconds = 1775651696000 ms
        assert_eq!(millis_to_iso8601(1775651696000), "2026-04-08T12:34:56Z");
    }

    #[test]
    fn millis_to_iso8601_subsecond_truncation() {
        // 999ms after epoch should still show :00
        assert_eq!(millis_to_iso8601(999), "1970-01-01T00:00:00Z");
        assert_eq!(millis_to_iso8601(1000), "1970-01-01T00:00:01Z");
    }

    #[test]
    fn millis_to_iso8601_leap_year() {
        // 2024-02-29T00:00:00Z (leap day) = 1709164800 seconds
        assert_eq!(millis_to_iso8601(1709164800000), "2024-02-29T00:00:00Z");
    }

    #[test]
    fn millis_to_iso8601_year_boundary() {
        // 2024-12-31T23:59:59Z = 1735689599 seconds
        assert_eq!(millis_to_iso8601(1735689599000), "2024-12-31T23:59:59Z");
        // 2025-01-01T00:00:00Z = 1735689600 seconds
        assert_eq!(millis_to_iso8601(1735689600000), "2025-01-01T00:00:00Z");
    }
}
