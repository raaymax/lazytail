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

mod context;
mod lines;
pub(super) mod response;
mod search;
mod stats;

use super::types::*;
use crate::config::{self, DiscoveryResult};
use crate::renderer::PresetRegistry;
use crate::source;
use response::error_response;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_box, ServerHandler};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

/// LazyTail MCP server providing log file analysis tools.
#[derive(Clone)]
pub struct LazyTailMcp {
    /// Config discovery result for project-aware source resolution.
    discovery: DiscoveryResult,
    /// Compiled rendering presets.
    preset_registry: Arc<PresetRegistry>,
    /// Source name → renderer preset names mapping.
    source_renderer_map: HashMap<String, Vec<String>>,
}

impl LazyTailMcp {
    pub fn new() -> Self {
        let discovery = config::discover();
        let (registry, source_renderer_map) = match config::load(&discovery) {
            Ok(cfg) => {
                // Compilation errors are intentionally discarded — the MCP server has no
                // stderr channel. Invalid renderers are simply omitted from the registry.
                let (registry, _errors) = PresetRegistry::compile_from_config(
                    &cfg.renderers,
                    discovery.project_root.as_deref(),
                );
                let map: HashMap<String, Vec<String>> = cfg
                    .project_sources
                    .iter()
                    .chain(cfg.global_sources.iter())
                    .filter(|s| !s.renderer_names.is_empty())
                    .map(|s| (s.name.clone(), s.renderer_names.clone()))
                    .collect();
                (registry, map)
            }
            Err(_) => (PresetRegistry::new(Vec::new()), HashMap::new()),
        };
        Self {
            discovery,
            preset_registry: Arc::new(registry),
            source_renderer_map,
        }
    }

    /// Resolve renderer names for a source by matching the file stem against the source_renderer_map.
    pub(super) fn renderer_names_for_path(&self, path: &Path) -> Vec<String> {
        let source_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        self.source_renderer_map
            .get(source_name)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for LazyTailMcp {
    fn default() -> Self {
        Self::new()
    }
}

#[tool(tool_box)]
impl LazyTailMcp {
    /// Fetch lines from a lazytail source starting from a specific position.
    #[tool(
        description = "Fetch lines from a lazytail-captured log source. Pass a source name from list_sources. Returns up to 1000 lines starting from a given position. For recent lines, prefer get_tail instead."
    )]
    fn get_lines(&self, #[tool(aggr)] req: GetLinesRequest) -> String {
        let path = match source::resolve_source_for_context(&req.source, &self.discovery) {
            Ok(p) => p,
            Err(e) => return error_response(e),
        };
        self.get_lines_impl(
            &path,
            req.start,
            req.count,
            req.raw,
            req.output,
            req.full_content,
        )
    }

    /// Fetch the last N lines from a lazytail source.
    #[tool(
        description = "Fetch the last N lines from a lazytail-captured log source. Useful for checking recent activity. Pass a source name from list_sources. Returns up to 1000 lines from the end of the file. Supports incremental polling via since_line — pass the last line_number you received to get only new lines added after that point."
    )]
    fn get_tail(&self, #[tool(aggr)] req: GetTailRequest) -> String {
        let path = match source::resolve_source_for_context(&req.source, &self.discovery) {
            Ok(p) => p,
            Err(e) => return error_response(e),
        };
        self.get_tail_impl(
            &path,
            req.count,
            req.since_line,
            req.raw,
            req.output,
            req.full_content,
        )
    }

    /// Search for patterns in a lazytail source using plain text, regex, or structured query.
    #[tool(
        description = "Search for patterns in a lazytail-captured log source. Supports plain text (default), regex, or structured query modes. Pass a source name from list_sources. Returns up to max_results matches (default 100, max 1000) with optional context_lines. Structured queries use the `query` parameter (LogQL-style, ignores pattern/mode/case_sensitive when set). Query format: {\"parser\": \"json\"|\"logfmt\", \"filters\": [{\"field\": \"name\", \"op\": \"eq\"|\"ne\"|\"contains\"|\"regex\"|\"not_regex\"|\"gt\"|\"lt\"|\"gte\"|\"lte\", \"value\": \"...\"}]}. Supports dot notation for nested fields (\"user.id\"), exclusion patterns, and time-based filtering with relative values (\"now-5m\", \"now-1h30m\") or absolute timestamps on comparison operators. Use the virtual field \"@ts\" to filter by ingestion timestamp (when the line was captured) instead of a log field — e.g. {\"field\": \"@ts\", \"op\": \"gte\", \"value\": \"now-5m\"}. Aggregation: add {\"aggregate\": {\"type\": \"count_by\", \"fields\": [\"level\"], \"limit\": 10}} to group results."
    )]
    fn search(&self, #[tool(aggr)] req: SearchRequest) -> String {
        let path = match source::resolve_source_for_context(&req.source, &self.discovery) {
            Ok(p) => p,
            Err(e) => return error_response(e),
        };
        if let Some(query) = req.query {
            Self::query_impl(
                &path,
                query,
                req.max_results,
                req.context_lines,
                req.raw,
                req.output,
                req.full_content,
            )
        } else {
            Self::search_impl(
                &path,
                &req.pattern,
                req.mode,
                req.case_sensitive,
                req.max_results,
                req.context_lines,
                req.raw,
                req.output,
                req.full_content,
            )
        }
    }

    /// Get context lines around a specific line number in a lazytail source.
    #[tool(
        description = "Get context lines around a specific line number in a lazytail-captured log source. Useful for exploring what happened before and after a specific log entry. Pass a source name from list_sources. Returns the target line plus configurable lines before (default 5) and after (default 5)."
    )]
    fn get_context(&self, #[tool(aggr)] req: GetContextRequest) -> String {
        let path = match source::resolve_source_for_context(&req.source, &self.discovery) {
            Ok(p) => p,
            Err(e) => return error_response(e),
        };
        self.get_context_impl(
            &path,
            req.line_number,
            req.before,
            req.after,
            req.raw,
            req.output,
            req.full_content,
        )
    }

    /// List available log sources from project and global data directories.
    #[tool(
        description = "List available log sources. Shows captured sources with their status (active if being written to, ended otherwise), file paths, sizes, and location (project or global). Scans both project-local .lazytail/data/ (if in a project with lazytail.yaml) and global ~/.config/lazytail/data/. Use this first to discover what logs are available before searching."
    )]
    fn list_sources(&self, #[tool(aggr)] _req: ListSourcesRequest) -> String {
        let data_dir = source::data_dir().unwrap_or_default();

        // Get discovered sources from both project and global directories
        let discovered = match source::discover_sources_for_context(&self.discovery) {
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

            let location = match ds.location {
                source::SourceLocation::Project => SourceLocation::Project,
                source::SourceLocation::Global => SourceLocation::Global,
            };

            // Get file size
            let size_bytes = std::fs::metadata(&ds.log_path)
                .map(|m| m.len())
                .unwrap_or(0);

            let renderer_names = self
                .source_renderer_map
                .get(&ds.name)
                .cloned()
                .unwrap_or_default();

            sources.push(SourceInfo {
                name: ds.name,
                path: ds.log_path,
                status,
                size_bytes,
                location,
                renderer_names,
            });
        }

        let response = ListSourcesResponse {
            sources,
            data_directory: data_dir,
        };

        serde_json::to_string_pretty(&response)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e))
    }

    /// Get index statistics for a lazytail source.
    #[tool(
        description = "Get columnar index statistics for a lazytail source. Returns indexed line count, log file size, available columns, severity breakdown (from flags column), and recent ingestion rate (lines/sec from checkpoint timestamps). Useful for understanding log composition before searching. Pass a source name from list_sources."
    )]
    fn get_stats(&self, #[tool(aggr)] req: GetStatsRequest) -> String {
        let path = match source::resolve_source_for_context(&req.source, &self.discovery) {
            Ok(p) => p,
            Err(e) => return error_response(e),
        };
        Self::get_stats_impl(&path, &req.source, req.output)
    }
}

// Generate the tool_box function
tool_box!(LazyTailMcp {
    get_lines,
    get_tail,
    search,
    get_context,
    list_sources,
    get_stats
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
                 Start with list_sources to discover available logs and their names. \
                 Use search to find patterns (supports regex), get_tail for recent activity, \
                 get_lines to read specific sections, and get_context to explore around a line number. \
                 All tools accept a source name (from list_sources), not a file path. \
                 Log sources are captured via 'cmd | lazytail -n NAME'. \
                 Sources are discovered from both project-local (.lazytail/data/ next to lazytail.yaml) \
                 and global (~/.config/lazytail/data/) directories. Project sources shadow global ones with the same name. \
                 The search tool also supports structured queries via the `query` parameter for \
                 field-based filtering on JSON/logfmt logs (LogQL-style). Example query: \
                 {\"parser\": \"json\", \"filters\": [{\"field\": \"level\", \"op\": \"eq\", \"value\": \"error\"}]}. \
                 Queries support aggregation via the `aggregate` field for grouping and counting results. \
                 Example: {\"parser\": \"json\", \"aggregate\": {\"type\": \"count_by\", \"fields\": [\"level\"]}}."
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
    use crate::filter::query::FilterQuery;
    use response::truncate_line;
    use std::io::Write;
    use tempfile::NamedTempFile;

    /// Create a test MCP instance with empty registry (no renderers).
    fn test_mcp() -> LazyTailMcp {
        LazyTailMcp {
            discovery: config::discover(),
            preset_registry: Arc::new(PresetRegistry::new(Vec::new())),
            source_renderer_map: HashMap::new(),
        }
    }

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

    // -- get_lines tests --

    #[test]
    fn get_lines_strips_ansi_by_default() {
        let f = write_ansi_tempfile();
        let result = test_mcp().get_lines_impl(f.path(), 0, 100, false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.lines[0].content, "[INFO] Server started");
        assert_eq!(resp.lines[1].content, "[ERROR] Connection failed");
        assert_eq!(resp.lines[3].content, "plain line with no escapes");
        assert_eq!(resp.lines[4].content, "hyperlink text");
    }

    #[test]
    fn get_lines_preserves_ansi_when_raw() {
        let f = write_ansi_tempfile();
        let result = test_mcp().get_lines_impl(f.path(), 0, 100, true, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert!(resp.lines[0].content.contains("\x1b[1;32m"));
        assert!(resp.lines[1].content.contains("\x1b[1;31m"));
    }

    // -- get_tail tests --

    #[test]
    fn get_tail_strips_ansi_by_default() {
        let f = write_ansi_tempfile();
        let result = test_mcp().get_tail_impl(f.path(), 2, None, false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.lines.len(), 2);
        assert_eq!(resp.lines[0].content, "plain line with no escapes");
        assert_eq!(resp.lines[1].content, "hyperlink text");
    }

    #[test]
    fn get_tail_preserves_ansi_when_raw() {
        let f = write_ansi_tempfile();
        let result = test_mcp().get_tail_impl(f.path(), 5, None, true, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert!(resp.lines[0].content.contains("\x1b["));
    }

    #[test]
    fn get_tail_since_line_returns_new_lines() {
        let mut f = NamedTempFile::new().unwrap();
        for i in 0..5 {
            writeln!(f, "line {i}").unwrap();
        }
        f.flush().unwrap();

        let result =
            test_mcp().get_tail_impl(f.path(), 100, Some(2), false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.lines.len(), 2);
        assert_eq!(resp.lines[0].line_number, 3);
        assert_eq!(resp.lines[0].content, "line 3");
        assert_eq!(resp.lines[1].line_number, 4);
        assert_eq!(resp.lines[1].content, "line 4");
        assert_eq!(resp.total_lines, 5);
        assert!(!resp.has_more);
    }

    #[test]
    fn get_tail_since_line_with_count() {
        let mut f = NamedTempFile::new().unwrap();
        for i in 0..10 {
            writeln!(f, "line {i}").unwrap();
        }
        f.flush().unwrap();

        let result =
            test_mcp().get_tail_impl(f.path(), 2, Some(5), false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.lines.len(), 2);
        assert_eq!(resp.lines[0].line_number, 6);
        assert_eq!(resp.lines[0].content, "line 6");
        assert_eq!(resp.lines[1].line_number, 7);
        assert_eq!(resp.lines[1].content, "line 7");
        assert!(resp.has_more);
    }

    #[test]
    fn get_tail_since_line_at_end() {
        let mut f = NamedTempFile::new().unwrap();
        for i in 0..5 {
            writeln!(f, "line {i}").unwrap();
        }
        f.flush().unwrap();

        let result =
            test_mcp().get_tail_impl(f.path(), 100, Some(4), false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();

        assert!(resp.lines.is_empty());
        assert!(!resp.has_more);
        assert_eq!(resp.total_lines, 5);
    }

    #[test]
    fn get_tail_since_line_beyond_end() {
        let mut f = NamedTempFile::new().unwrap();
        for i in 0..5 {
            writeln!(f, "line {i}").unwrap();
        }
        f.flush().unwrap();

        let result =
            test_mcp().get_tail_impl(f.path(), 100, Some(100), false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();

        assert!(resp.lines.is_empty());
        assert!(!resp.has_more);
        assert_eq!(resp.total_lines, 5);
    }

    // -- search tests --

    #[test]
    fn search_strips_ansi_by_default() {
        let f = write_ansi_tempfile();
        let result = LazyTailMcp::search_impl(
            f.path(),
            "ERROR",
            SearchMode::Plain,
            false,
            100,
            1,
            false,
            OutputFormat::Json,
            false,
        );
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
        let result = LazyTailMcp::search_impl(
            f.path(),
            "ERROR",
            SearchMode::Plain,
            false,
            100,
            0,
            true,
            OutputFormat::Json,
            false,
        );
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();
        assert!(resp.matches[0].content.contains("\x1b[1;31m"));
    }

    // -- get_context tests --

    #[test]
    fn get_context_strips_ansi_by_default() {
        let f = write_ansi_tempfile();
        let result =
            test_mcp().get_context_impl(f.path(), 2, 1, 1, false, OutputFormat::Json, false);
        let resp: GetContextResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.target_line.content, "[DEBUG] Processing request");
        assert_eq!(resp.before_lines[0].content, "[ERROR] Connection failed");
        assert_eq!(resp.after_lines[0].content, "plain line with no escapes");
    }

    #[test]
    fn get_context_preserves_ansi_when_raw() {
        let f = write_ansi_tempfile();
        let result =
            test_mcp().get_context_impl(f.path(), 0, 0, 0, true, OutputFormat::Json, false);
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

        let result = test_mcp().get_lines_impl(f.path(), 0, 100, false, OutputFormat::Json, false);
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
        let raw_result =
            test_mcp().get_lines_impl(f.path(), 0, 100, true, OutputFormat::Json, false);
        let raw_resp: GetLinesResponse = serde_json::from_str(&raw_result).unwrap();
        assert!(raw_resp.lines[0].content.contains('\x1b'));

        // Stripped mode (default)
        let result = test_mcp().get_lines_impl(f.path(), 0, 100, false, OutputFormat::Json, false);
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

        let result = test_mcp().get_lines_impl(f.path(), 0, 100, true, OutputFormat::Json, false);
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

        let result = LazyTailMcp::search_impl(
            f.path(),
            "error",
            SearchMode::Plain,
            false,
            100,
            1,
            false,
            OutputFormat::Json,
            false,
        );
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

        let result = test_mcp().get_lines_impl(f.path(), 0, 100, false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.lines[0].content, "no ansi here");
        assert_eq!(resp.lines[1].content, "just plain text");
    }

    // -- text output format tests --

    #[test]
    fn get_lines_text_format() {
        let json_line = r#"{"level":"info","msg":"started"}"#;
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{json_line}").unwrap();
        writeln!(f, "plain log line").unwrap();
        f.flush().unwrap();

        let result = test_mcp().get_lines_impl(f.path(), 0, 100, false, OutputFormat::Text, false);
        assert!(result.starts_with("--- "));
        assert!(result.contains("--- total_lines: 2\n"));
        assert!(result.contains("--- has_more: false\n"));
        // JSON content should appear verbatim without escaping
        assert!(result.contains(&format!("[L0] {json_line}\n")));
        assert!(result.contains("[L1] plain log line\n"));
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

        let result = test_mcp().get_tail_impl(f.path(), 2, None, false, OutputFormat::Text, false);
        assert!(result.starts_with("--- "));
        assert!(result.contains("--- has_more: true\n"));
        assert!(result.contains("[L1] line 1\n"));
        assert!(result.contains("[L2] line 2\n"));
        assert!(!result.contains("[L0] line 0"));
    }

    #[test]
    fn search_text_format() {
        let json_line = r#"{"level":"error","msg":"timeout"}"#;
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "before line").unwrap();
        writeln!(f, "{json_line}").unwrap();
        writeln!(f, "after line").unwrap();
        f.flush().unwrap();

        let result = LazyTailMcp::search_impl(
            f.path(),
            "error",
            SearchMode::Plain,
            false,
            100,
            1,
            false,
            OutputFormat::Text,
            false,
        );
        assert!(result.starts_with("--- "));
        assert!(result.contains("--- total_matches: 1\n"));
        assert!(result.contains("--- truncated: false\n"));
        assert!(result.contains("=== match\n"));
        // Match line has > prefix
        assert!(result.contains(&format!("> 1|{json_line}\n")));
        // Context lines have space prefix
        assert!(result.contains("  0|before line\n"));
        assert!(result.contains("  2|after line\n"));
    }

    #[test]
    fn get_context_text_format() {
        let json_line = r#"{"target":"line"}"#;
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "line 0").unwrap();
        writeln!(f, "line 1").unwrap();
        writeln!(f, "{json_line}").unwrap();
        writeln!(f, "line 3").unwrap();
        writeln!(f, "line 4").unwrap();
        f.flush().unwrap();

        let result =
            test_mcp().get_context_impl(f.path(), 2, 2, 2, false, OutputFormat::Text, false);
        assert!(result.starts_with("--- "));
        assert!(result.contains("--- total_lines: 5\n"));
        assert!(result.contains("  [L0] line 0\n"));
        assert!(result.contains("  [L1] line 1\n"));
        assert!(result.contains(&format!("> [L2] {json_line}\n")));
        assert!(result.contains("  [L3] line 3\n"));
        assert!(result.contains("  [L4] line 4\n"));
    }

    #[test]
    fn get_lines_text_format_strips_ansi() {
        let f = write_ansi_tempfile();
        let result = test_mcp().get_lines_impl(f.path(), 0, 100, false, OutputFormat::Text, false);
        assert!(result.starts_with("--- "));
        // ANSI should be stripped
        assert!(result.contains("[L0] [INFO] Server started\n"));
        assert!(result.contains("[L1] [ERROR] Connection failed\n"));
        assert!(!result.contains("\x1b["));
    }

    #[test]
    fn search_text_format_multiple_matches() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "error one").unwrap();
        writeln!(f, "ok line").unwrap();
        writeln!(f, "error two").unwrap();
        f.flush().unwrap();

        let result = LazyTailMcp::search_impl(
            f.path(),
            "error",
            SearchMode::Plain,
            false,
            100,
            0,
            false,
            OutputFormat::Text,
            false,
        );
        assert!(result.contains("--- total_matches: 2\n"));
        // Two match blocks separated by blank line
        let match_count = result.matches("=== match").count();
        assert_eq!(match_count, 2);
        assert!(result.contains("> 0|error one\n"));
        assert!(result.contains("> 2|error two\n"));
    }

    #[test]
    fn default_output_is_text() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "test line").unwrap();
        f.flush().unwrap();

        // Simulate what MCP does: deserialize without output field
        let json = r#"{"source": "myapp", "start": 0, "count": 10}"#;
        let req: GetLinesRequest = serde_json::from_str(json).unwrap();
        // Verify the source field deserializes correctly
        assert_eq!(req.source, "myapp");
        // Use _impl to verify default output format is text
        let result =
            test_mcp().get_lines_impl(f.path(), req.start, req.count, req.raw, req.output, false);
        // Default should be text format (starts with ---)
        assert!(result.starts_with("--- "));
    }

    // -- query tests --

    fn write_json_log_tempfile() -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"{{"level":"info","msg":"server started","service":"api-gateway"}}"#
        )
        .unwrap();
        writeln!(f, r#"{{"level":"error","msg":"connection timeout","service":"api-users","user":{{"id":"123","name":"Alice"}}}}"#).unwrap();
        writeln!(
            f,
            r#"{{"level":"info","msg":"request processed","service":"web-frontend","status":200}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"level":"error","msg":"database error","service":"api-orders","status":500}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"level":"debug","msg":"ignore this","service":"test-service"}}"#
        )
        .unwrap();
        f.flush().unwrap();
        f
    }

    fn write_logfmt_tempfile() -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "level=info msg=\"server started\" service=api-gateway").unwrap();
        writeln!(
            f,
            "level=error msg=\"connection timeout\" service=api-users"
        )
        .unwrap();
        writeln!(
            f,
            "level=info msg=\"request processed\" service=web-frontend status=200"
        )
        .unwrap();
        writeln!(
            f,
            "level=error msg=\"database error\" service=api-orders status=500"
        )
        .unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn query_json_eq_filter() {
        let f = write_json_log_tempfile();
        let query: FilterQuery = serde_json::from_str(
            r#"{
            "parser": "json",
            "filters": [{"field": "level", "op": "eq", "value": "error"}]
        }"#,
        )
        .unwrap();

        let result =
            LazyTailMcp::query_impl(f.path(), query, 100, 0, false, OutputFormat::Json, false);
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.total_matches, 2);
        assert!(resp.matches[0].content.contains("connection timeout"));
        assert!(resp.matches[1].content.contains("database error"));
    }

    #[test]
    fn query_logfmt_filter() {
        let f = write_logfmt_tempfile();
        let query: FilterQuery = serde_json::from_str(
            r#"{
            "parser": "logfmt",
            "filters": [{"field": "level", "op": "eq", "value": "error"}]
        }"#,
        )
        .unwrap();

        let result =
            LazyTailMcp::query_impl(f.path(), query, 100, 0, false, OutputFormat::Json, false);
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.total_matches, 2);
        assert!(resp.matches[0].content.contains("connection timeout"));
        assert!(resp.matches[1].content.contains("database error"));
    }

    #[test]
    fn query_exclusion_patterns() {
        let f = write_json_log_tempfile();
        let query: FilterQuery = serde_json::from_str(
            r#"{
            "parser": "json",
            "filters": [{"field": "level", "op": "eq", "value": "error"}],
            "exclude": [{"field": "msg", "pattern": "database"}]
        }"#,
        )
        .unwrap();

        let result =
            LazyTailMcp::query_impl(f.path(), query, 100, 0, false, OutputFormat::Json, false);
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.total_matches, 1);
        assert!(resp.matches[0].content.contains("connection timeout"));
    }

    #[test]
    fn query_nested_field_access() {
        let f = write_json_log_tempfile();
        let query: FilterQuery = serde_json::from_str(
            r#"{
            "parser": "json",
            "filters": [{"field": "user.id", "op": "eq", "value": "123"}]
        }"#,
        )
        .unwrap();

        let result =
            LazyTailMcp::query_impl(f.path(), query, 100, 0, false, OutputFormat::Json, false);
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.total_matches, 1);
        assert!(resp.matches[0].content.contains("Alice"));
    }

    #[test]
    fn query_regex_operator() {
        let f = write_json_log_tempfile();
        let query: FilterQuery = serde_json::from_str(
            r#"{
            "parser": "json",
            "filters": [{"field": "service", "op": "regex", "value": "^api-.*"}]
        }"#,
        )
        .unwrap();

        let result =
            LazyTailMcp::query_impl(f.path(), query, 100, 0, false, OutputFormat::Json, false);
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.total_matches, 3);
    }

    #[test]
    fn query_takes_precedence_over_pattern() {
        let f = write_json_log_tempfile();

        // Deserialize a request with both pattern and query
        let req: SearchRequest = serde_json::from_str(
            r#"{
            "source": "test",
            "pattern": "this_should_be_ignored",
            "query": {
                "parser": "json",
                "filters": [{"field": "level", "op": "eq", "value": "error"}]
            }
        }"#,
        )
        .unwrap();

        assert!(req.query.is_some());

        // Use query_impl directly (since search() needs source resolution)
        let result = LazyTailMcp::query_impl(
            f.path(),
            req.query.unwrap(),
            req.max_results,
            req.context_lines,
            req.raw,
            OutputFormat::Json,
            false,
        );
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.total_matches, 2);
    }

    #[test]
    fn query_invalid_regex_returns_error() {
        let f = write_json_log_tempfile();
        let query: FilterQuery = serde_json::from_str(
            r#"{
            "parser": "json",
            "filters": [{"field": "msg", "op": "regex", "value": "[invalid"}]
        }"#,
        )
        .unwrap();

        let result =
            LazyTailMcp::query_impl(f.path(), query, 100, 0, false, OutputFormat::Json, false);
        assert!(result.contains("error"));
        assert!(result.contains("Invalid"));
    }

    #[test]
    fn query_with_context_lines() {
        let f = write_json_log_tempfile();
        let query: FilterQuery = serde_json::from_str(
            r#"{
            "parser": "json",
            "filters": [{"field": "msg", "op": "eq", "value": "database error"}]
        }"#,
        )
        .unwrap();

        let result =
            LazyTailMcp::query_impl(f.path(), query, 100, 1, false, OutputFormat::Json, false);
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.total_matches, 1);
        assert_eq!(resp.matches[0].line_number, 3);
        assert!(!resp.matches[0].before.is_empty());
    }

    #[test]
    fn query_numeric_comparison() {
        let f = write_json_log_tempfile();
        let query: FilterQuery = serde_json::from_str(
            r#"{
            "parser": "json",
            "filters": [{"field": "status", "op": "gte", "value": "400"}]
        }"#,
        )
        .unwrap();

        let result =
            LazyTailMcp::query_impl(f.path(), query, 100, 0, false, OutputFormat::Json, false);
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.total_matches, 1);
        assert!(resp.matches[0].content.contains("\"status\":500"));
    }

    #[test]
    fn query_pattern_not_required() {
        // Verify pattern defaults to empty when only query is provided
        let req: SearchRequest = serde_json::from_str(
            r#"{
            "source": "test",
            "query": {
                "parser": "json",
                "filters": [{"field": "level", "op": "eq", "value": "info"}]
            }
        }"#,
        )
        .unwrap();

        assert_eq!(req.pattern, "");
        assert!(req.query.is_some());
    }

    // -- get_stats tests --

    #[test]
    fn get_stats_no_index() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "just a log line").unwrap();
        f.flush().unwrap();

        let result = LazyTailMcp::get_stats_impl(f.path(), "test", OutputFormat::Json);
        let resp: GetStatsResponse = serde_json::from_str(&result).unwrap();

        assert_eq!(resp.source, "test");
        assert!(!resp.has_index);
        assert_eq!(resp.indexed_lines, 0);
        assert!(resp.severity_counts.is_none());
        assert!(resp.columns.is_empty());
    }

    #[test]
    fn get_stats_with_index() {
        use crate::index::builder::IndexBuilder;

        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "2024-01-01 INFO server started").unwrap();
        writeln!(f, "2024-01-01 ERROR connection failed").unwrap();
        writeln!(f, "2024-01-01 WARN disk space low").unwrap();
        f.flush().unwrap();

        let idx_dir = tempfile::tempdir().unwrap();
        IndexBuilder::new().build(f.path(), idx_dir.path()).unwrap();

        // Create a symlink-like setup: the index dir must be at path.with_extension("idx")
        // Instead, use a temp file whose .idx/ sibling we control
        let dir = tempfile::tempdir().unwrap();
        let log_path = dir.path().join("test.log");
        std::fs::copy(f.path(), &log_path).unwrap();
        let real_idx_dir = dir.path().join("test.idx");
        std::fs::create_dir_all(&real_idx_dir).unwrap();

        // Copy index files
        for entry in std::fs::read_dir(idx_dir.path()).unwrap() {
            let entry = entry.unwrap();
            std::fs::copy(entry.path(), real_idx_dir.join(entry.file_name())).unwrap();
        }

        let result = LazyTailMcp::get_stats_impl(&log_path, "test", OutputFormat::Json);
        let resp: GetStatsResponse = serde_json::from_str(&result).unwrap();

        assert!(resp.has_index);
        assert_eq!(resp.indexed_lines, 3);
        assert!(!resp.columns.is_empty());
        assert!(resp.columns.contains(&"flags".to_string()));
    }

    #[test]
    fn get_stats_text_format() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "test").unwrap();
        f.flush().unwrap();

        let result = LazyTailMcp::get_stats_impl(f.path(), "myapp", OutputFormat::Text);
        assert!(result.contains("--- source: myapp"));
        assert!(result.contains("--- has_index: false"));
    }

    // -- truncation tests --

    #[test]
    fn truncate_line_short_unchanged() {
        let line = "short line";
        let result = truncate_line(line, 500);
        assert_eq!(&*result, "short line");
        assert!(matches!(result, std::borrow::Cow::Borrowed(_)));
    }

    #[test]
    fn truncate_line_long_truncated() {
        let line = "x".repeat(600);
        let result = truncate_line(&line, 500);
        assert!(result.len() < line.len());
        assert!(result.starts_with(&"x".repeat(500)));
        assert!(result.contains("…[+100 chars]"));
    }

    #[test]
    fn truncate_line_multibyte_boundary() {
        // 'é' is 2 bytes in UTF-8; build a string where byte 500 lands mid-char
        let mut line = "a".repeat(499);
        line.push('é'); // bytes 499..501
        line.push_str(&"b".repeat(100));
        let result = truncate_line(&line, 500);
        // Must not panic and must be valid UTF-8
        assert!(result.contains('…'));
        // The truncation point should be at or before byte 500
        let prefix_end = result.find('…').unwrap();
        assert!(prefix_end <= 500);
    }

    #[test]
    fn get_lines_truncates_long_lines() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "short").unwrap();
        writeln!(f, "{}", "x".repeat(1000)).unwrap();
        f.flush().unwrap();

        let result = test_mcp().get_lines_impl(f.path(), 0, 100, false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.lines[0].content, "short");
        assert!(resp.lines[1].content.len() < 1000);
        assert!(resp.lines[1].content.contains("…[+"));
    }

    #[test]
    fn get_lines_full_content_skips_truncation() {
        let mut f = NamedTempFile::new().unwrap();
        let long = "x".repeat(1000);
        writeln!(f, "{long}").unwrap();
        f.flush().unwrap();

        let result = test_mcp().get_lines_impl(f.path(), 0, 100, false, OutputFormat::Json, true);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.lines[0].content.len(), 1000);
        assert!(!resp.lines[0].content.contains("…[+"));
    }

    #[test]
    fn search_full_content_skips_truncation() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "error {}", "b".repeat(1000)).unwrap();
        f.flush().unwrap();

        let result = LazyTailMcp::search_impl(
            f.path(),
            "error",
            SearchMode::Plain,
            false,
            100,
            0,
            false,
            OutputFormat::Json,
            true,
        );
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();
        assert!(!resp.matches[0].content.contains("…[+"));
        assert!(resp.matches[0].content.len() > 1000);
    }

    #[test]
    fn get_context_full_content_skips_truncation() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{}", "a".repeat(1000)).unwrap();
        writeln!(f, "{}", "b".repeat(1000)).unwrap();
        f.flush().unwrap();

        let result =
            test_mcp().get_context_impl(f.path(), 0, 0, 1, false, OutputFormat::Json, true);
        let resp: GetContextResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.target_line.content.len(), 1000);
        assert!(!resp.after_lines[0].content.contains("…[+"));
    }

    #[test]
    fn search_truncates_long_match_and_context() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{}", "a".repeat(1000)).unwrap();
        writeln!(f, "error {}", "b".repeat(1000)).unwrap();
        writeln!(f, "{}", "c".repeat(1000)).unwrap();
        f.flush().unwrap();

        let result = LazyTailMcp::search_impl(
            f.path(),
            "error",
            SearchMode::Plain,
            false,
            100,
            1,
            false,
            OutputFormat::Json,
            false,
        );
        let resp: SearchResponse = serde_json::from_str(&result).unwrap();
        assert_eq!(resp.total_matches, 1);
        let m = &resp.matches[0];
        assert!(m.content.contains("…[+"));
        assert!(m.before[0].contains("…[+"));
        assert!(m.after[0].contains("…[+"));
    }

    #[test]
    fn get_context_truncates_long_lines() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "{}", "a".repeat(1000)).unwrap();
        writeln!(f, "{}", "b".repeat(1000)).unwrap();
        writeln!(f, "{}", "c".repeat(1000)).unwrap();
        f.flush().unwrap();

        let result =
            test_mcp().get_context_impl(f.path(), 1, 1, 1, false, OutputFormat::Json, false);
        let resp: GetContextResponse = serde_json::from_str(&result).unwrap();
        assert!(resp.before_lines[0].content.contains("…[+"));
        assert!(resp.target_line.content.contains("…[+"));
        assert!(resp.after_lines[0].content.contains("…[+"));
    }

    // -- Rendering integration tests --

    /// Create a test MCP with a JSON renderer that formats level + message,
    /// with the given file stem mapped to the "json" renderer.
    fn test_mcp_with_renderer_for(file_stem: &str) -> LazyTailMcp {
        use crate::config::types::{RawDetectDef, RawLayoutEntryDef, RawRendererDef, StyleValue};

        let renderers = vec![RawRendererDef {
            parser: None,
            name: "json".to_string(),
            detect: Some(RawDetectDef {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![
                RawLayoutEntryDef {
                    field: Some("level".to_string()),
                    literal: None,
                    style: Some(StyleValue::Single("severity".to_string())),
                    width: Some(5),
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntryDef {
                    field: None,
                    literal: Some(" ".to_string()),
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
                RawLayoutEntryDef {
                    field: Some("message".to_string()),
                    literal: None,
                    style: None,
                    width: None,
                    format: None,
                    style_map: None,
                    max_width: None,
                    style_when: None,
                    value_type: None,
                },
            ],
        }];

        let (registry, _) = crate::renderer::PresetRegistry::compile_from_config(&renderers, None);
        let mut source_renderer_map = HashMap::new();
        source_renderer_map.insert(file_stem.to_string(), vec!["json".to_string()]);
        LazyTailMcp {
            discovery: config::discover(),
            preset_registry: Arc::new(registry),
            source_renderer_map,
        }
    }

    fn write_json_tempfile() -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, r#"{{"level":"error","message":"something failed"}}"#).unwrap();
        writeln!(f, r#"{{"level":"info","message":"server started"}}"#).unwrap();
        writeln!(f, "plain text line").unwrap();
        f.flush().unwrap();
        f
    }

    fn file_stem(f: &NamedTempFile) -> String {
        f.path().file_stem().unwrap().to_str().unwrap().to_string()
    }

    #[test]
    fn get_lines_with_rendered() {
        let f = write_json_tempfile();
        let mcp = test_mcp_with_renderer_for(&file_stem(&f));
        let result = mcp.get_lines_impl(f.path(), 0, 100, false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();

        // JSON lines get rendered field
        assert!(resp.lines[0].rendered.is_some());
        let rendered = resp.lines[0].rendered.as_ref().unwrap();
        assert!(rendered.contains("error"));
        assert!(rendered.contains("something failed"));

        // Plain text line has no rendered field
        assert!(resp.lines[2].rendered.is_none());
    }

    #[test]
    fn get_lines_plain_text_no_rendered() {
        // No renderer for plain text → rendered stays None
        let mcp = test_mcp();
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "just a plain line").unwrap();
        f.flush().unwrap();

        let result = mcp.get_lines_impl(f.path(), 0, 100, false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        assert!(resp.lines[0].rendered.is_none());
    }

    #[test]
    fn get_tail_with_rendered() {
        let f = write_json_tempfile();
        let mcp = test_mcp_with_renderer_for(&file_stem(&f));
        let result = mcp.get_tail_impl(f.path(), 10, None, false, OutputFormat::Json, false);
        let resp: GetLinesResponse = serde_json::from_str(&result).unwrap();
        // At least one JSON line should have rendered
        assert!(resp.lines[0].rendered.is_some());
    }

    #[test]
    fn get_context_with_rendered() {
        let f = write_json_tempfile();
        let mcp = test_mcp_with_renderer_for(&file_stem(&f));
        let result = mcp.get_context_impl(f.path(), 0, 0, 1, false, OutputFormat::Json, false);
        let resp: GetContextResponse = serde_json::from_str(&result).unwrap();
        // Target line is JSON → rendered
        assert!(resp.target_line.rendered.is_some());
    }

    #[test]
    fn list_sources_renderer_names() {
        use crate::config::types::{RawDetectDef, RawLayoutEntryDef, RawRendererDef};

        let renderers = vec![RawRendererDef {
            parser: None,
            name: "my-preset".to_string(),
            detect: Some(RawDetectDef {
                parser: Some("json".to_string()),
                filename: None,
            }),
            regex: None,
            layout: vec![RawLayoutEntryDef {
                field: Some("msg".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
                style_when: None,
                value_type: None,
            }],
        }];

        let (registry, _) = crate::renderer::PresetRegistry::compile_from_config(&renderers, None);
        let mut source_renderer_map = HashMap::new();
        source_renderer_map.insert("test-source".to_string(), vec!["my-preset".to_string()]);

        let mcp = LazyTailMcp {
            discovery: config::discover(),
            preset_registry: Arc::new(registry),
            source_renderer_map,
        };

        // The renderer_names_for_path function should resolve names
        let names = mcp.renderer_names_for_path(std::path::Path::new("/some/test-source.log"));
        assert!(names.contains(&"my-preset".to_string()));
    }
}
