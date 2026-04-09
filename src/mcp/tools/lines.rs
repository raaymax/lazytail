//! get_lines and get_tail tool implementations.

use super::response::*;
use super::LazyTailMcp;
use crate::index::reader::IndexReader;
use crate::mcp::types::*;
use crate::reader::{file_reader::FileReader, LogReader};
use std::path::Path;

impl LazyTailMcp {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn get_lines_impl(
        &self,
        path: &Path,
        start: usize,
        count: usize,
        raw: bool,
        output: OutputFormat,
        full_content: bool,
        include_ts: bool,
    ) -> String {
        let count = count.min(1000);

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
        let mut lines = Vec::new();
        for i in start..(start + count).min(total) {
            if let Ok(Some(content)) = reader.get_line(i) {
                let flags = index_reader.as_ref().and_then(|ir| ir.flags(i));
                let timestamp = if include_ts {
                    index_reader
                        .as_ref()
                        .and_then(|ir| ir.get_timestamp(i))
                        .map(millis_to_iso8601)
                } else {
                    None
                };
                let mut info = LineInfo {
                    line_number: i,
                    content,
                    severity: index_reader
                        .as_ref()
                        .map(|ir| ir.severity(i))
                        .and_then(|s| s.label().map(String::from)),
                    rendered: None,
                    timestamp,
                };
                let raw_content = info.content.clone();
                render_line_info(&mut info, &raw_content, flags, &ctx);
                lines.push(info);
            }
        }

        let mut response = GetLinesResponse {
            lines,
            total_lines: total,
            has_more: start + count < total,
        };

        if !raw {
            strip_lines_response(&mut response);
        }
        if !full_content {
            truncate_lines_response(&mut response);
        }

        format_lines(&response, output)
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn get_tail_impl(
        &self,
        path: &Path,
        count: usize,
        since_line: Option<usize>,
        raw: bool,
        output: OutputFormat,
        full_content: bool,
        include_ts: bool,
    ) -> String {
        let count = count.min(1000);

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

        let (start, end, has_more) = if let Some(since) = since_line {
            let start = since.saturating_add(1);
            let end = start.saturating_add(count).min(total);
            let has_more = end < total;
            (start, end, has_more)
        } else {
            let start = total.saturating_sub(count);
            (start, total, start > 0)
        };

        let mut lines = Vec::new();
        for i in start..end {
            if let Ok(Some(content)) = reader.get_line(i) {
                let flags = index_reader.as_ref().and_then(|ir| ir.flags(i));
                let timestamp = if include_ts {
                    index_reader
                        .as_ref()
                        .and_then(|ir| ir.get_timestamp(i))
                        .map(millis_to_iso8601)
                } else {
                    None
                };
                let mut info = LineInfo {
                    line_number: i,
                    content,
                    severity: index_reader
                        .as_ref()
                        .map(|ir| ir.severity(i))
                        .and_then(|s| s.label().map(String::from)),
                    rendered: None,
                    timestamp,
                };
                let raw_content = info.content.clone();
                render_line_info(&mut info, &raw_content, flags, &ctx);
                lines.push(info);
            }
        }

        let mut response = GetLinesResponse {
            lines,
            total_lines: total,
            has_more,
        };

        if !raw {
            strip_lines_response(&mut response);
        }
        if !full_content {
            truncate_lines_response(&mut response);
        }

        format_lines(&response, output)
    }
}
