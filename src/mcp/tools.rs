//! MCP tool implementations for log file analysis.

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
use std::sync::Arc;

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
    /// Fetch lines from a log file. Returns up to 1000 lines starting from a given position.
    #[tool(
        description = "Fetch lines from a log file. Returns up to 1000 lines starting from a given position."
    )]
    fn get_lines(&self, #[tool(aggr)] req: GetLinesRequest) -> String {
        let count = req.count.min(1000);

        let reader_result = FileReader::new(&req.file);
        let mut reader = match reader_result {
            Ok(r) => r,
            Err(e) => {
                return serde_json::to_string(&serde_json::json!({
                    "error": format!("Failed to open file '{}': {}", req.file.display(), e)
                }))
                .unwrap_or_else(|_| "Error serializing response".to_string());
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

        let response = GetLinesResponse {
            lines,
            total_lines: total,
            has_more: req.start + count < total,
        };

        serde_json::to_string_pretty(&response)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e))
    }

    /// Fetch the last N lines from a log file.
    #[tool(
        description = "Fetch the last N lines from a log file. Useful for checking recent activity."
    )]
    fn get_tail(&self, #[tool(aggr)] req: GetTailRequest) -> String {
        let count = req.count.min(1000);

        let reader_result = FileReader::new(&req.file);
        let mut reader = match reader_result {
            Ok(r) => r,
            Err(e) => {
                return serde_json::to_string(&serde_json::json!({
                    "error": format!("Failed to open file '{}': {}", req.file.display(), e)
                }))
                .unwrap_or_else(|_| "Error serializing response".to_string());
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

        let response = GetLinesResponse {
            lines,
            total_lines: total,
            has_more: start > 0,
        };

        serde_json::to_string_pretty(&response)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e))
    }

    /// Search for patterns in a log file using plain text or regex.
    #[tool(
        description = "Search for patterns in a log file using plain text or regex. Returns matching lines with optional context."
    )]
    fn search(&self, #[tool(aggr)] req: SearchRequest) -> String {
        let max_results = req.max_results.min(1000);
        let context_lines = req.context_lines.min(50);

        // Use streaming filter for fast search (same as UI)
        let filter: Arc<dyn Filter> = match req.mode {
            SearchMode::Plain => Arc::new(StringFilter::new(&req.pattern, req.case_sensitive)),
            SearchMode::Regex => match RegexFilter::new(&req.pattern, req.case_sensitive) {
                Ok(f) => Arc::new(f),
                Err(e) => {
                    return serde_json::to_string(&serde_json::json!({
                        "error": format!("Invalid regex pattern: {}", e)
                    }))
                    .unwrap_or_else(|_| "Error serializing response".to_string());
                }
            },
        };

        // Run streaming filter (grep-like performance)
        let rx = match streaming_filter::run_streaming_filter(
            req.file.clone(),
            filter,
            CancelToken::new(),
        ) {
            Ok(rx) => rx,
            Err(e) => {
                return serde_json::to_string(&serde_json::json!({
                    "error": format!("Failed to search file '{}': {}", req.file.display(), e)
                }))
                .unwrap_or_else(|_| "Error serializing response".to_string());
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
                FilterProgress::Error(e) => {
                    return serde_json::to_string(&serde_json::json!({
                        "error": format!("Search error: {}", e)
                    }))
                    .unwrap_or_else(|_| "Error serializing response".to_string());
                }
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
                Err(e) => {
                    return serde_json::to_string(&serde_json::json!({
                        "error": format!("Failed to read line content: {}", e)
                    }))
                    .unwrap_or_else(|_| "Error serializing response".to_string());
                }
            }
        };

        let response = SearchResponse {
            matches,
            total_matches,
            truncated,
            lines_searched,
        };

        serde_json::to_string_pretty(&response)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e))
    }

    /// Get line content and context using mmap (no index building required)
    fn get_lines_content(
        path: &std::path::Path,
        line_indices: &[usize],
        context_lines: usize,
    ) -> anyhow::Result<Vec<SearchMatch>> {
        if line_indices.is_empty() {
            return Ok(Vec::new());
        }

        let file = File::open(path)?;
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
    #[tool(description = "Get context lines around a specific line number in a log file.")]
    fn get_context(&self, #[tool(aggr)] req: GetContextRequest) -> String {
        let before_count = req.before.min(50);
        let after_count = req.after.min(50);

        let reader_result = FileReader::new(&req.file);
        let mut reader = match reader_result {
            Ok(r) => r,
            Err(e) => {
                return serde_json::to_string(&serde_json::json!({
                    "error": format!("Failed to open file '{}': {}", req.file.display(), e)
                }))
                .unwrap_or_else(|_| "Error serializing response".to_string());
            }
        };

        let total = reader.total_lines();

        if req.line_number >= total {
            return serde_json::to_string(&serde_json::json!({
                "error": format!("Line {} does not exist (file has {} lines)", req.line_number, total)
            }))
            .unwrap_or_else(|_| "Error serializing response".to_string());
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
            _ => {
                return serde_json::to_string(&serde_json::json!({
                    "error": "Failed to read target line"
                }))
                .unwrap_or_else(|_| "Error serializing response".to_string());
            }
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

        let response = GetContextResponse {
            before_lines,
            target_line,
            after_lines,
            total_lines: total,
        };

        serde_json::to_string_pretty(&response)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e))
    }

    /// List available log sources from the LazyTail data directory.
    #[tool(
        description = "List available log sources from ~/.config/lazytail/data/. Shows captured sources with their status (active/ended)."
    )]
    fn list_sources(&self, #[tool(aggr)] _req: ListSourcesRequest) -> String {
        let data_dir = match source::data_dir() {
            Some(dir) => dir,
            None => {
                return serde_json::to_string(&serde_json::json!({
                    "error": "Could not determine data directory"
                }))
                .unwrap_or_else(|_| "Error serializing response".to_string());
            }
        };

        // Get discovered sources
        let discovered = match source::discover_sources() {
            Ok(sources) => sources,
            Err(e) => {
                return serde_json::to_string(&serde_json::json!({
                    "error": format!("Failed to discover sources: {}", e),
                    "data_directory": data_dir.display().to_string()
                }))
                .unwrap_or_else(|_| "Error serializing response".to_string());
            }
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
                 Use list_sources to discover available logs, \
                 get_lines to read file contents, get_tail to read recent lines, \
                 search to find patterns, and get_context to explore surrounding lines."
                    .into(),
            ),
            ..Default::default()
        }
    }

    // Derive list_tools and call_tool from the tool_box
    tool_box!(@derive);
}
