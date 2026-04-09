//! get_stats tool implementation.

use super::response::*;
use super::LazyTailMcp;
use crate::index::reader::IndexReader;
use crate::mcp::types::*;
use std::path::Path;

impl LazyTailMcp {
    pub(crate) fn get_stats_impl(path: &Path, source_name: &str, output: OutputFormat) -> String {
        let stats = IndexReader::stats(path);

        let (
            indexed_lines,
            log_file_size,
            has_index,
            severity_counts,
            lines_per_second,
            columns,
            time_range,
        ) = if let Some(stats) = stats {
            let sc = stats.severity_counts.map(|sc| SeverityCountsInfo {
                unknown: sc.unknown,
                trace: sc.trace,
                debug: sc.debug,
                info: sc.info,
                warn: sc.warn,
                error: sc.error,
                fatal: sc.fatal,
            });
            (
                stats.indexed_lines,
                stats.log_file_size,
                true,
                sc,
                stats.lines_per_second,
                stats.columns,
                stats.time_range,
            )
        } else {
            // Index unavailable or corrupt — fall back to file metadata for size
            let log_file_size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            (0, log_file_size, false, None, None, Vec::new(), None)
        };

        let response = GetStatsResponse {
            source: source_name.to_string(),
            indexed_lines,
            log_file_size,
            has_index,
            severity_counts,
            lines_per_second,
            columns,
            time_range_start: time_range.map(|(start, _)| millis_to_iso8601(start)),
            time_range_end: time_range.map(|(_, end)| millis_to_iso8601(end)),
        };

        format_stats(&response, output)
    }
}
