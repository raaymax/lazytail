//! get_context tool implementation.

use super::response::*;
use super::LazyTailMcp;
use crate::index::reader::IndexReader;
use crate::mcp::types::*;
use crate::reader::{file_reader::FileReader, LogReader};
use std::path::Path;

impl LazyTailMcp {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn get_context_impl(
        &self,
        path: &Path,
        line_number: usize,
        before: usize,
        after: usize,
        raw: bool,
        output: OutputFormat,
        full_content: bool,
    ) -> String {
        let before_count = before.min(50);
        let after_count = after.min(50);

        let mut reader = match FileReader::new(path) {
            Ok(r) => r,
            Err(e) => {
                return error_response(format!("Failed to open file '{}': {}", path.display(), e))
            }
        };

        let index_reader = IndexReader::open(path);
        let renderer_names = self.renderer_names_for_path(path);
        let ctx = RenderContext {
            registry: &self.preset_registry,
            renderer_names: &renderer_names,
        };

        let total = reader.total_lines();

        if line_number >= total {
            return error_response(format!(
                "Line {} does not exist (file has {} lines)",
                line_number, total
            ));
        }

        // Get before lines
        let start_before = line_number.saturating_sub(before_count);
        let mut before_lines = Vec::new();
        for i in start_before..line_number {
            if let Ok(Some(content)) = reader.get_line(i) {
                let flags = index_reader.as_ref().and_then(|ir| ir.flags(i));
                let mut info = LineInfo {
                    line_number: i,
                    content,
                    severity: index_reader
                        .as_ref()
                        .map(|ir| ir.severity(i))
                        .and_then(|s| s.label().map(String::from)),
                    rendered: None,
                };
                let raw_content = info.content.clone();
                render_line_info(&mut info, &raw_content, flags, &ctx);
                before_lines.push(info);
            }
        }

        // Get target line
        let target_content = match reader.get_line(line_number) {
            Ok(Some(c)) => c,
            _ => return error_response("Failed to read target line"),
        };
        let target_flags = index_reader.as_ref().and_then(|ir| ir.flags(line_number));
        let mut target_line = LineInfo {
            line_number,
            content: target_content,
            severity: index_reader
                .as_ref()
                .map(|ir| ir.severity(line_number))
                .and_then(|s| s.label().map(String::from)),
            rendered: None,
        };
        let raw_target = target_line.content.clone();
        render_line_info(&mut target_line, &raw_target, target_flags, &ctx);

        // Get after lines
        let end_after = (line_number + 1 + after_count).min(total);
        let mut after_lines = Vec::new();
        for i in (line_number + 1)..end_after {
            if let Ok(Some(content)) = reader.get_line(i) {
                let flags = index_reader.as_ref().and_then(|ir| ir.flags(i));
                let mut info = LineInfo {
                    line_number: i,
                    content,
                    severity: index_reader
                        .as_ref()
                        .map(|ir| ir.severity(i))
                        .and_then(|s| s.label().map(String::from)),
                    rendered: None,
                };
                let raw_content = info.content.clone();
                render_line_info(&mut info, &raw_content, flags, &ctx);
                after_lines.push(info);
            }
        }

        let mut response = GetContextResponse {
            before_lines,
            target_line,
            after_lines,
            total_lines: total,
        };

        if !raw {
            strip_context_response(&mut response);
        }
        if !full_content {
            truncate_context_response(&mut response);
        }

        format_context(&response, output)
    }
}
