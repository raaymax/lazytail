//! get_stats tool implementation.

use super::response::*;
use super::LazyTailMcp;
use crate::index::reader::IndexReader;
use crate::mcp::types::*;
use std::path::Path;

impl LazyTailMcp {
    pub(crate) fn get_stats_impl(path: &Path, source_name: &str, output: OutputFormat) -> String {
        let stats = IndexReader::stats(path);

        let (indexed_lines, log_file_size, has_index, severity_counts, lines_per_second, columns) =
            if let Some(stats) = stats {
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
                )
            } else {
                (0, 0, false, None, None, Vec::new())
            };

        let response = GetStatsResponse {
            source: source_name.to_string(),
            indexed_lines,
            log_file_size,
            has_index,
            severity_counts,
            lines_per_second,
            columns,
        };

        format_stats(&response, output)
    }
}
