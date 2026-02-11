//! MCP tool implementations for log file analysis.
//!
//! Note on blocking: Tool handlers are synchronous as required by rmcp. The `search` tool
//! spawns a filter thread and blocks waiting for results. This is acceptable because:
//! 1. MCP stdio transport processes requests sequentially (one at a time)
//! 2. The actual filtering work runs on a dedicated thread, not blocking the tokio runtime
//! 3. Only the channel recv() blocks, which is waiting on real work
//!
//! If concurrent request handling is needed in the future, consider wrapping heavy
//! operations in `tokio::task::spawn_blocking`.

use super::ansi::strip_ansi;
use super::format;
use super::types::*;
use crate::filter::{cancel::CancelToken, engine::FilterProgress, streaming_filter};
use crate::filter::{regex_filter::RegexFilter, string_filter::StringFilter, Filter};
use crate::reader::{file_reader::FileReader, LogReader};
use crate::source;
use memchr::memchr_iter;
use memmap2::Mmap;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_box, ServerHandler};
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

/// Create a JSON error response string.
fn error_response(message: impl std::fmt::Display) -> String {
    serde_json::to_string(&serde_json::json!({ "error": message.to_string() }))
        .unwrap_or_else(|_| r#"{"error": "Failed to serialize error"}"#.to_string())
}

/// Strip ANSI escape codes from all line content in a GetLinesResponse.
fn strip_lines_response(resp: &mut GetLinesResponse) {
    for line in &mut resp.lines {
        line.content = strip_ansi(&line.content);
    }
}

/// Strip ANSI escape codes from all content in a SearchResponse.
fn strip_search_response(resp: &mut SearchResponse) {
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
fn strip_context_response(resp: &mut GetContextResponse) {
    for line in &mut resp.before_lines {
        line.content = strip_ansi(&line.content);
    }
    resp.target_line.content = strip_ansi(&resp.target_line.content);
    for line in &mut resp.after_lines {
        line.content = strip_ansi(&line.content);
    }
}

/// Format a GetLinesResponse according to the requested output format.
fn format_lines(resp: &GetLinesResponse, output: OutputFormat) -> String {
    match output {
        OutputFormat::Text => format::format_lines_text(resp),
        OutputFormat::Json => serde_json::to_string_pretty(resp)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e)),
    }
}

/// Format a SearchResponse according to the requested output format.
fn format_search(resp: &SearchResponse, output: OutputFormat) -> String {
    match output {
        OutputFormat::Text => format::format_search_text(resp),
        OutputFormat::Json => serde_json::to_string_pretty(resp)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e)),
    }
}

/// Format a GetContextResponse according to the requested output format.
fn format_context(resp: &GetContextResponse, output: OutputFormat) -> String {
    match output {
        OutputFormat::Text => format::format_context_text(resp),
        OutputFormat::Json => serde_json::to_string_pretty(resp)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e)),
    }
}

/// LazyTail MCP server providing log file analysis tools.
#[derive(Clone)]
pub struct LazyTailMcp;

impl LazyTailMcp {
    pub fn new() -> Self {
        Self
    }
}

impl Default for LazyTailMcp {
    fn default() -> Self {
        Self::new()
    }
}

#[tool(tool_box)]
impl LazyTailMcp {
    /// Fetch lines from a log file starting from a specific position.
    #[tool(
        description = "Fetch lines from a log file. Returns up to 1000 lines starting from a given position. Use this to read specific sections of a log file. For recent lines, prefer get_tail instead."
    )]
    fn get_lines(&self, #[tool(aggr)] req: GetLinesRequest) -> String {
        let count = req.count.min(1000);

        let mut reader = match FileReader::new(&req.file) {
            Ok(r) => r,
            Err(e) => {
                return error_response(format!(
                    "Failed to open file '{}': {}",
                    req.file.display(),
                    e
                ))
            }
        };

        let total = reader.total_lines();
        let mut lines = Vec::new();
        for i in req.start..(req.start + count).min(total) {
            if let Ok(Some(content)) = reader.get_line(i) {
                lines.push(LineInfo {
                    line_number: i,
                    content,
                });
            }
        }

        let mut response = GetLinesResponse {
            lines,
            total_lines: total,
            has_more: req.start + count < total,
        };

        if !req.raw {
            strip_lines_response(&mut response);
        }

        format_lines(&response, req.output)
    }

    /// Fetch the last N lines from a log file.
    #[tool(
        description = "Fetch the last N lines from a log file. Useful for checking recent activity. Returns up to 1000 lines from the end of the file."
    )]
    fn get_tail(&self, #[tool(aggr)] req: GetTailRequest) -> String {
        let count = req.count.min(1000);

        let mut reader = match FileReader::new(&req.file) {
            Ok(r) => r,
            Err(e) => {
                return error_response(format!(
                    "Failed to open file '{}': {}",
                    req.file.display(),
                    e
                ))
            }
        };

        let total = reader.total_lines();
        let start = total.saturating_sub(count);

        let mut lines = Vec::new();
        for i in start..total {
            if let Ok(Some(content)) = reader.get_line(i) {
                lines.push(LineInfo {
                    line_number: i,
                    content,
                });
            }
        }

        let mut response = GetLinesResponse {
            lines,
            total_lines: total,
            has_more: start > 0,
        };

        if !req.raw {
            strip_lines_response(&mut response);
        }

        format_lines(&response, req.output)
    }

    /// Search for patterns in a log file using plain text or regex.
    #[tool(
        description = "Search for patterns in a log file. Supports plain text (default) or regex mode. Returns matching lines with optional context lines before/after each match. Use context_lines parameter to see surrounding log entries. Returns up to max_results matches (default 100, max 1000)."
    )]
    fn search(&self, #[tool(aggr)] req: SearchRequest) -> String {
        let max_results = req.max_results.min(1000);
        let context_lines = req.context_lines.min(50);

        // Use streaming filter for fast search (same as UI)
        let filter: Arc<dyn Filter> = match req.mode {
            SearchMode::Plain => Arc::new(StringFilter::new(&req.pattern, req.case_sensitive)),
            SearchMode::Regex => match RegexFilter::new(&req.pattern, req.case_sensitive) {
                Ok(f) => Arc::new(f),
                Err(e) => return error_response(format!("Invalid regex pattern: {}", e)),
            },
        };

        // Run streaming filter (grep-like performance).
        // The filter runs on a dedicated thread; we block here waiting for results.
        // See module doc for why this is acceptable in the current MCP design.
        let rx = match streaming_filter::run_streaming_filter(
            req.file.clone(),
            filter,
            CancelToken::new(),
        ) {
            Ok(rx) => rx,
            Err(e) => {
                return error_response(format!(
                    "Failed to search file '{}': {}",
                    req.file.display(),
                    e
                ))
            }
        };

        // Collect matching line indices from channel
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
                FilterProgress::Error(e) => return error_response(format!("Search error: {}", e)),
            }
        }

        let total_matches = matching_indices.len();
        let truncated = total_matches > max_results;
        matching_indices.truncate(max_results);

        // If we need line content or context, use mmap for fast random access
        let matches = if matching_indices.is_empty() {
            Vec::new()
        } else {
            match Self::get_lines_content(&req.file, &matching_indices, context_lines) {
                Ok(m) => m,
                Err(e) => return error_response(format!("Failed to read line content: {}", e)),
            }
        };

        let mut response = SearchResponse {
            matches,
            total_matches,
            truncated,
            lines_searched,
        };

        if !req.raw {
            strip_search_response(&mut response);
        }

        format_search(&response, req.output)
    }

    /// Fetch line content and context for search matches using a single-pass mmap scan.
    ///
    /// This is a specialized batch operation that differs from `FileReader`:
    /// - `FileReader`: Builds a full line index, optimized for random access to any line
    /// - This function: Single sequential pass, only extracts specific lines + context
    ///
    /// For search results with context, this approach is more efficient because:
    /// 1. We know exactly which lines we need upfront (matches + context)
    /// 2. Single pass through file up to the last needed line, then early exit
    /// 3. No index structure overhead - just a BTreeSet of needed line numbers
    /// 4. Handles overlapping context ranges efficiently via deduplication
    fn get_lines_content(
        path: &Path,
        line_indices: &[usize],
        context_lines: usize,
    ) -> anyhow::Result<Vec<SearchMatch>> {
        if line_indices.is_empty() {
            return Ok(Vec::new());
        }

        let file = File::open(path)?;
        // SAFETY: The file handle is kept open for the lifetime of the mmap.
        // We only perform read operations on the mapped memory.
        // The file is opened read-only and we don't modify it.
        let mmap = unsafe { Mmap::map(&file)? };
        let data = &mmap[..];

        // Build a set of all line numbers we need (matches + context)
        let mut needed_lines: std::collections::BTreeSet<usize> = std::collections::BTreeSet::new();
        for &line_num in line_indices {
            let start = line_num.saturating_sub(context_lines);
            let end = line_num + context_lines + 1;
            for i in start..end {
                needed_lines.insert(i);
            }
        }

        // Single pass through file to collect all needed lines
        let max_needed = *needed_lines.iter().next_back().unwrap_or(&0);
        let mut line_contents: std::collections::HashMap<usize, String> =
            std::collections::HashMap::new();

        let mut line_num = 0;
        let mut line_start = 0;

        for pos in memchr_iter(b'\n', data) {
            if needed_lines.contains(&line_num) {
                let line_bytes = &data[line_start..pos];
                let content = String::from_utf8_lossy(line_bytes).into_owned();
                line_contents.insert(line_num, content);
            }

            line_num += 1;
            line_start = pos + 1;

            // Early termination once we have all needed lines
            if line_num > max_needed {
                break;
            }
        }

        // Handle last line (no trailing newline)
        if line_start < data.len() && needed_lines.contains(&line_num) {
            let line_bytes = &data[line_start..];
            let content = String::from_utf8_lossy(line_bytes).into_owned();
            line_contents.insert(line_num, content);
        }

        // Build SearchMatch results
        let mut matches = Vec::with_capacity(line_indices.len());
        for &line_num in line_indices {
            let content = line_contents.get(&line_num).cloned().unwrap_or_default();

            let mut before = Vec::new();
            if context_lines > 0 {
                let start = line_num.saturating_sub(context_lines);
                for i in start..line_num {
                    if let Some(c) = line_contents.get(&i) {
                        before.push(c.clone());
                    }
                }
            }

            let mut after = Vec::new();
            if context_lines > 0 {
                for i in (line_num + 1)..=(line_num + context_lines) {
                    if let Some(c) = line_contents.get(&i) {
                        after.push(c.clone());
                    }
                }
            }

            matches.push(SearchMatch {
                line_number: line_num,
                content,
                before,
                after,
            });
        }

        Ok(matches)
    }

    /// Get context lines around a specific line number in a log file.
    #[tool(
        description = "Get context lines around a specific line number in a log file. Useful for exploring what happened before and after a specific log entry. Returns the target line plus configurable lines before (default 5) and after (default 5)."
    )]
    fn get_context(&self, #[tool(aggr)] req: GetContextRequest) -> String {
        let before_count = req.before.min(50);
        let after_count = req.after.min(50);

        let mut reader = match FileReader::new(&req.file) {
            Ok(r) => r,
            Err(e) => {
                return error_response(format!(
                    "Failed to open file '{}': {}",
                    req.file.display(),
                    e
                ))
            }
        };

        let total = reader.total_lines();

        if req.line_number >= total {
            return error_response(format!(
                "Line {} does not exist (file has {} lines)",
                req.line_number, total
            ));
        }

        // Get before lines
        let start_before = req.line_number.saturating_sub(before_count);
        let mut before_lines = Vec::new();
        for i in start_before..req.line_number {
            if let Ok(Some(content)) = reader.get_line(i) {
                before_lines.push(LineInfo {
                    line_number: i,
                    content,
                });
            }
        }

        // Get target line
        let target_content = match reader.get_line(req.line_number) {
            Ok(Some(c)) => c,
            _ => return error_response("Failed to read target line"),
        };
        let target_line = LineInfo {
            line_number: req.line_number,
            content: target_content,
        };

        // Get after lines
        let end_after = (req.line_number + 1 + after_count).min(total);
        let mut after_lines = Vec::new();
        for i in (req.line_number + 1)..end_after {
            if let Ok(Some(content)) = reader.get_line(i) {
                after_lines.push(LineInfo {
                    line_number: i,
                    content,
                });
            }
        }

        let mut response = GetContextResponse {
            before_lines,
            target_line,
            after_lines,
            total_lines: total,
        };

        if !req.raw {
            strip_context_response(&mut response);
        }

        format_context(&response, req.output)
    }

    /// List available log sources from the LazyTail data directory.
    #[tool(
        description = "List available log sources from ~/.config/lazytail/data/. Shows captured sources with their status (active if being written to, ended otherwise), file paths, and sizes. Use this first to discover what logs are available before searching."
    )]
    fn list_sources(&self, #[tool(aggr)] _req: ListSourcesRequest) -> String {
        let data_dir = match source::data_dir() {
            Some(dir) => dir,
            None => return error_response("Could not determine data directory"),
        };

        // Get discovered sources
        let discovered = match source::discover_sources() {
            Ok(sources) => sources,
            Err(e) => return error_response(format!("Failed to discover sources: {}", e)),
        };

        let mut sources = Vec::new();

        for ds in discovered {
            // Map source status
            let status = match ds.status {
                source::SourceStatus::Active => SourceStatus::Active,
                source::SourceStatus::Ended => SourceStatus::Ended,
            };

            // Get file size
            let size_bytes = std::fs::metadata(&ds.log_path)
                .map(|m| m.len())
                .unwrap_or(0);

            sources.push(SourceInfo {
                name: ds.name,
                path: ds.log_path,
                status,
                size_bytes,
            });
        }

        let response = ListSourcesResponse {
            sources,
            data_directory: data_dir,
        };

        serde_json::to_string_pretty(&response)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e))
    }
}

// Generate the tool_box function
tool_box!(LazyTailMcp {
    get_lines,
    get_tail,
    search,
    get_context,
    list_sources
});

impl ServerHandler for LazyTailMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            server_info: Implementation {
                name: "lazytail".into(),
                version: env!("CARGO_PKG_VERSION").into(),
            },
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            instructions: Some(
                "LazyTail MCP server for log file analysis. \
                 Start with list_sources to discover available logs and their paths. \
                 Use search to find patterns (supports regex), get_tail for recent activity, \
                 get_lines to read specific sections, and get_context to explore around a line number. \
                 Log sources are captured via 'cmd | lazytail -n NAME' and stored in ~/.config/lazytail/data/."
                    .into(),
            ),
            ..Default::default()
        }
    }

    // Derive list_tools and call_tool from the tool_box
    tool_box!(@derive);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Lines with ANSI escape codes for testing.
    const ANSI_LINES: &str = "\
\x1b[1;32m[INFO]\x1b[0m Server started\n\
\x1b[1;31m[ERROR]\x1b[0m Connection failed\n\
\x1b[36m[DEBUG]\x1b[0m Processing request\n\
plain line with no escapes\n\
\x1b]8;;https://example.com\x07hyperlink\x1b]8;;\x07 text\n";

    fn write_ansi_tempfile() -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(ANSI_LINES.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }

    fn mcp() -> LazyTailMcp {
        LazyTailMcp::new()
    }

    // -- get_lines tests --

    #[test]
    fn get_lines_strips_ansi_by_default() {
        let f = write_ansi_tempfile();
        let result = mcp().get_lines(GetLinesRequest {
            file: f.path().into(),
            start: 0,
            count: 100,
            raw: false,
            output: OutputFormat::Json,
        });
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.lines[0].content, "[INFO] Server started");
        assert_eq!(resp.lines[1].content, "[ERROR] Connection failed");
        assert_eq!(resp.lines[3].content, "plain line with no escapes");
        assert_eq!(resp.lines[4].content, "hyperlink text");
    }

    #[test]
    fn get_lines_preserves_ansi_when_raw() {
        let f = write_ansi_tempfile();
        let result = mcp().get_lines(GetLinesRequest {
            file: f.path().into(),
            start: 0,
            count: 100,
            raw: true,
            output: OutputFormat::Json,
        });
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert!(resp.lines[0].content.contains("\x1b[1;32m"));
        assert!(resp.lines[1].content.contains("\x1b[1;31m"));
    }

    // -- get_tail tests --

    #[test]
    fn get_tail_strips_ansi_by_default() {
        let f = write_ansi_tempfile();
        let result = mcp().get_tail(GetTailRequest {
            file: f.path().into(),
            count: 2,
            raw: false,
            output: OutputFormat::Json,
        });
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.lines.len(), 2);
        assert_eq!(resp.lines[0].content, "plain line with no escapes");
        assert_eq!(resp.lines[1].content, "hyperlink text");
    }

    #[test]
    fn get_tail_preserves_ansi_when_raw() {
        let f = write_ansi_tempfile();
        let result = mcp().get_tail(GetTailRequest {
            file: f.path().into(),
            count: 5,
            raw: true,
            output: OutputFormat::Json,
        });
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert!(resp.lines[0].content.contains("\x1b["));
    }

    // -- search tests --

    #[test]
    fn search_strips_ansi_by_default() {
        let f = write_ansi_tempfile();
        let result = mcp().search(SearchRequest {
            file: f.path().into(),
            pattern: "ERROR".into(),
            mode: SearchMode::Plain,
            case_sensitive: false,
            max_results: 100,
            context_lines: 1,
            raw: false,
            output: OutputFormat::Json,
        });
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.total_matches, 1);
        let m = &resp.matches[0];
        assert_eq!(m.content, "[ERROR] Connection failed");
        // Context lines should also be stripped
        assert!(!m.before.is_empty());
        assert!(!m.before[0].contains("\x1b["));
        assert!(!m.after.is_empty());
        assert!(!m.after[0].contains("\x1b["));
    }

    #[test]
    fn search_preserves_ansi_when_raw() {
        let f = write_ansi_tempfile();
        let result = mcp().search(SearchRequest {
            file: f.path().into(),
            pattern: "ERROR".into(),
            mode: SearchMode::Plain,
            case_sensitive: false,
            max_results: 100,
            context_lines: 0,
            raw: true,
            output: OutputFormat::Json,
        });
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();
        assert!(resp.matches[0].content.contains("\x1b[1;31m"));
    }

    // -- get_context tests --

    #[test]
    fn get_context_strips_ansi_by_default() {
        let f = write_ansi_tempfile();
        let result = mcp().get_context(GetContextRequest {
            file: f.path().into(),
            line_number: 2,
            before: 1,
            after: 1,
            raw: false,
            output: OutputFormat::Json,
        });
        let resp: GetContextResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.target_line.content, "[DEBUG] Processing request");
        assert_eq!(resp.before_lines[0].content, "[ERROR] Connection failed");
        assert_eq!(resp.after_lines[0].content, "plain line with no escapes");
    }

    #[test]
    fn get_context_preserves_ansi_when_raw() {
        let f = write_ansi_tempfile();
        let result = mcp().get_context(GetContextRequest {
            file: f.path().into(),
            line_number: 0,
            before: 0,
            after: 0,
            raw: true,
            output: OutputFormat::Json,
        });
        let resp: GetContextResponse = serde_json::from_str(&result).unwrap();
        assert!(resp.target_line.content.contains("\x1b[1;32m"));
    }

    // -- plain text passthrough --

    #[test]
    fn json_content_in_lines_survives_stripping() {
        let line0 =
            serde_json::json!({"level":"info","msg":"started","nested":{"port":8080}}).to_string();
        let line1_json = r#"{"data": "{}", "list": [1,2,3]}"#;
        // ANSI injected around a value inside raw JSON
        let line2_raw = r#"{"outer": {"inner": "value", "num": 42}}"#;
        let line2_ansi = line2_raw.replacen("\"value\"", "\x1b[36m\"value\"\x1b[0m", 1);

        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{}", ansi_wrap("32", &line0)).unwrap();
        writeln!(f, "{}", ansi_wrap("1;33", line1_json)).unwrap();
        writeln!(f, "{line2_ansi}").unwrap();
        f.flush().unwrap();

        let result = mcp().get_lines(GetLinesRequest {
            file: f.path().into(),
            start: 0,
            count: 100,
            raw: false,
            output: OutputFormat::Json,
        });
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.lines[0].content, line0);
        assert_eq!(resp.lines[1].content, line1_json);
        assert_eq!(resp.lines[2].content, line2_raw);

        // Verify the outer JSON response itself is valid by re-serializing
        let reserialized = serde_json::to_string(&resp).unwrap();
        let _: GetLinesResponse = serde_json::from_str(&reserialized).unwrap();
    }

    /// Build a JSON log line by serializing from inside out, then wrap with ANSI.
    /// This avoids unreadable multi-level escape sequences in test source.
    fn ansi_wrap(code: &str, content: &str) -> String {
        format!("\x1b[{code}m{content}\x1b[0m")
    }

    #[test]
    fn json_with_escaped_nested_json_strings() {
        // Build nested JSON strings programmatically (inside-out) to avoid escape hell
        let inner_json = serde_json::json!({"nested": "text"}).to_string();
        let line0 = serde_json::json!({"data": inner_json}).to_string();

        let err_json = serde_json::json!({"err": "timeout"}).to_string();
        // ANSI codes interleaved inside the serialized JSON value
        let line1_raw = serde_json::json!({"msg": err_json}).to_string();
        // Inject ANSI around the inner JSON value within the serialized string
        let line1 = line1_raw.replacen(&err_json, &ansi_wrap("31", &err_json), 1);

        let deep_json = serde_json::json!({"deep": "val"}).to_string();
        let mid_json = serde_json::json!({"inner": deep_json}).to_string();
        let line2 = serde_json::json!({"payload": mid_json}).to_string();

        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{}", ansi_wrap("32", &line0)).unwrap();
        writeln!(f, "{line1}").unwrap();
        writeln!(f, "{}", ansi_wrap("33", &line2)).unwrap();
        f.flush().unwrap();

        // Raw mode should preserve ESC bytes
        let raw_result = mcp().get_lines(GetLinesRequest {
            file: f.path().into(),
            start: 0,
            count: 100,
            raw: true,
            output: OutputFormat::Json,
        });
        let raw_resp: GetLinesResponse = serde_json::from_str(&raw_result).unwrap();
        assert!(raw_resp.lines[0].content.contains('\x1b'));

        // Stripped mode (default)
        let result = mcp().get_lines(GetLinesRequest {
            file: f.path().into(),
            start: 0,
            count: 100,
            raw: false,
            output: OutputFormat::Json,
        });
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();

        // Line 0: ANSI stripped, nested JSON intact
        assert_eq!(resp.lines[0].content, line0);
        let parsed: serde_json::Value = serde_json::from_str(&resp.lines[0].content).unwrap();
        assert_eq!(parsed["data"].as_str().unwrap(), inner_json);

        // Line 1: ANSI inside value stripped, JSON structure preserved
        let expected_line1 = serde_json::json!({"msg": err_json}).to_string();
        assert_eq!(resp.lines[1].content, expected_line1);
        let parsed: serde_json::Value = serde_json::from_str(&resp.lines[1].content).unwrap();
        assert_eq!(parsed["msg"].as_str().unwrap(), err_json);

        // Line 2: triple-nested JSON round-trips cleanly
        assert_eq!(resp.lines[2].content, line2);
        let parsed: serde_json::Value = serde_json::from_str(&resp.lines[2].content).unwrap();
        let inner: serde_json::Value =
            serde_json::from_str(parsed["payload"].as_str().unwrap()).unwrap();
        assert_eq!(inner["inner"].as_str().unwrap(), deep_json);

        // MCP response round-trip
        let round_trip = serde_json::to_string(&resp).unwrap();
        let resp2: GetLinesResponse = serde_json::from_str(&round_trip).unwrap();
        for i in 0..3 {
            assert_eq!(resp.lines[i].content, resp2.lines[i].content);
        }
    }

    #[test]
    fn json_content_preserved_when_raw() {
        let line = r#"{"data": "{}", "key": "val"}"#;
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{}", ansi_wrap("33", line)).unwrap();
        f.flush().unwrap();

        let result = mcp().get_lines(GetLinesRequest {
            file: f.path().into(),
            start: 0,
            count: 100,
            raw: true,
            output: OutputFormat::Json,
        });
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        // Raw should keep ANSI and JSON intact
        assert!(resp.lines[0].content.contains("\x1b[33m"));
        assert!(resp.lines[0].content.contains(r#""data": "{}""#));
    }

    #[test]
    fn search_with_json_content_strips_correctly() {
        let line_ok = serde_json::json!({"level":"info","msg":"ok"}).to_string();
        let line_err = serde_json::json!({"level":"error","msg":"fail","ctx":{}}).to_string();
        let line_done = serde_json::json!({"level":"info","msg":"done"}).to_string();

        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{}", ansi_wrap("32", &line_ok)).unwrap();
        writeln!(f, "{}", ansi_wrap("31", &line_err)).unwrap();
        writeln!(f, "{}", ansi_wrap("32", &line_done)).unwrap();
        f.flush().unwrap();

        let result = mcp().search(SearchRequest {
            file: f.path().into(),
            pattern: "error".into(),
            mode: SearchMode::Plain,
            case_sensitive: false,
            max_results: 100,
            context_lines: 1,
            raw: false,
            output: OutputFormat::Json,
        });
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.total_matches, 1);
        assert_eq!(resp.matches[0].content, line_err);
        // Context lines should also have clean JSON
        assert_eq!(resp.matches[0].before[0], line_ok);
        assert_eq!(resp.matches[0].after[0], line_done);
    }

    #[test]
    fn plain_text_unmodified_by_stripping() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "no ansi here").unwrap();
        writeln!(f, "just plain text").unwrap();
        f.flush().unwrap();

        let result = mcp().get_lines(GetLinesRequest {
            file: f.path().into(),
            start: 0,
            count: 100,
            raw: false,
            output: OutputFormat::Json,
        });
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.lines[0].content, "no ansi here");
        assert_eq!(resp.lines[1].content, "just plain text");
    }

    // -- text output format tests --

    #[test]
    fn get_lines_text_format() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, r#"{{"level":"info","msg":"started"}}"#).unwrap();
        writeln!(f, "plain log line").unwrap();
        f.flush().unwrap();

        let result = mcp().get_lines(GetLinesRequest {
            file: f.path().into(),
            start: 0,
            count: 100,
            raw: false,
            output: OutputFormat::Text,
        });
        assert!(result.starts_with("--- "));
        assert!(result.contains("--- total_lines: 2\n"));
        assert!(result.contains("--- has_more: false\n"));
        // JSON content should appear verbatim without escaping
        assert!(result.contains(r#"0|{"level":"info","msg":"started"}"#));
        assert!(result.contains("1|plain log line\n"));
        // No backslash escaping
        assert!(!result.contains("\\\""));
    }

    #[test]
    fn get_tail_text_format() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "line 0").unwrap();
        writeln!(f, "line 1").unwrap();
        writeln!(f, "line 2").unwrap();
        f.flush().unwrap();

        let result = mcp().get_tail(GetTailRequest {
            file: f.path().into(),
            count: 2,
            raw: false,
            output: OutputFormat::Text,
        });
        assert!(result.starts_with("--- "));
        assert!(result.contains("--- has_more: true\n"));
        assert!(result.contains("1|line 1\n"));
        assert!(result.contains("2|line 2\n"));
        assert!(!result.contains("0|line 0"));
    }

    #[test]
    fn search_text_format() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "before line").unwrap();
        writeln!(f, r#"{{"level":"error","msg":"timeout"}}"#).unwrap();
        writeln!(f, "after line").unwrap();
        f.flush().unwrap();

        let result = mcp().search(SearchRequest {
            file: f.path().into(),
            pattern: "error".into(),
            mode: SearchMode::Plain,
            case_sensitive: false,
            max_results: 100,
            context_lines: 1,
            raw: false,
            output: OutputFormat::Text,
        });
        assert!(result.starts_with("--- "));
        assert!(result.contains("--- total_matches: 1\n"));
        assert!(result.contains("--- truncated: false\n"));
        assert!(result.contains("=== match\n"));
        // Match line has > prefix
        assert!(result.contains(r#"> 1|{"level":"error","msg":"timeout"}"#));
        // Context lines have space prefix
        assert!(result.contains("  0|before line\n"));
        assert!(result.contains("  2|after line\n"));
    }

    #[test]
    fn get_context_text_format() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "line 0").unwrap();
        writeln!(f, "line 1").unwrap();
        writeln!(f, r#"{{"target":"line"}}"#).unwrap();
        writeln!(f, "line 3").unwrap();
        writeln!(f, "line 4").unwrap();
        f.flush().unwrap();

        let result = mcp().get_context(GetContextRequest {
            file: f.path().into(),
            line_number: 2,
            before: 2,
            after: 2,
            raw: false,
            output: OutputFormat::Text,
        });
        assert!(result.starts_with("--- "));
        assert!(result.contains("--- total_lines: 5\n"));
        assert!(result.contains("  0|line 0\n"));
        assert!(result.contains("  1|line 1\n"));
        assert!(result.contains(r#"> 2|{"target":"line"}"#));
        assert!(result.contains("  3|line 3\n"));
        assert!(result.contains("  4|line 4\n"));
    }

    #[test]
    fn default_output_is_text() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "test line").unwrap();
        f.flush().unwrap();

        // Simulate what MCP does: deserialize without output field
        let json = format!(
            r#"{{"file": "{}","start": 0,"count": 10}}"#,
            f.path().display()
        );
        let req: GetLinesRequest = serde_json::from_str(&json).unwrap();
        let result = mcp().get_lines(req);
        // Default should be text format (starts with ---)
        assert!(result.starts_with("--- "));
    }
}
