//! MCP tool implementations for log file analysis.

use super::types::*;
use crate::filter::{regex_filter::RegexFilter, string_filter::StringFilter, Filter};
use crate::reader::{file_reader::FileReader, LogReader};
use rmcp::handler::server::tool::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerInfo};
use rmcp::{tool, tool_box, ServerHandler};
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
    fn get_lines(&self, Parameters(req): Parameters<GetLinesRequest>) -> String {
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

    /// Search for patterns in a log file using plain text or regex.
    #[tool(
        description = "Search for patterns in a log file using plain text or regex. Returns matching lines with optional context."
    )]
    fn search(&self, Parameters(req): Parameters<SearchRequest>) -> String {
        let max_results = req.max_results.min(1000);
        let context_lines = req.context_lines.min(50);

        // Build filter based on mode
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
        let mut matching_indices = Vec::new();

        // Find all matching lines
        for i in 0..total {
            if let Ok(Some(content)) = reader.get_line(i) {
                if filter.matches(&content) {
                    matching_indices.push(i);
                }
            }
        }

        let total_matches = matching_indices.len();
        let truncated = total_matches > max_results;
        matching_indices.truncate(max_results);

        // Build matches with context
        let mut matches = Vec::new();
        for &line_num in &matching_indices {
            let content = reader.get_line(line_num).ok().flatten().unwrap_or_default();

            // Get before context
            let mut before = Vec::new();
            if context_lines > 0 {
                let start = line_num.saturating_sub(context_lines);
                for i in start..line_num {
                    if let Ok(Some(c)) = reader.get_line(i) {
                        before.push(c);
                    }
                }
            }

            // Get after context
            let mut after = Vec::new();
            if context_lines > 0 {
                let end = (line_num + 1 + context_lines).min(total);
                for i in (line_num + 1)..end {
                    if let Ok(Some(c)) = reader.get_line(i) {
                        after.push(c);
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

        let response = SearchResponse {
            matches,
            total_matches,
            truncated,
            lines_searched: total,
        };

        serde_json::to_string_pretty(&response)
            .unwrap_or_else(|e| format!("Error serializing response: {}", e))
    }

    /// Get context lines around a specific line number in a log file.
    #[tool(description = "Get context lines around a specific line number in a log file.")]
    fn get_context(&self, Parameters(req): Parameters<GetContextRequest>) -> String {
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
}

// Generate the tool_box function
tool_box!(LazyTailMcp {
    get_lines,
    search,
    get_context
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
                 Use get_lines to read file contents, search to find patterns, \
                 and get_context to explore surrounding lines."
                    .into(),
            ),
            ..Default::default()
        }
    }

    // Derive list_tools and call_tool from the tool_box
    tool_box!(@derive);
}
