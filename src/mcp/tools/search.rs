//! search and query tool implementations.

use super::response::*;
use super::LazyTailMcp;
use crate::filter::query::QueryFilter;
use crate::filter::search_engine::SearchEngine;
use crate::filter::{cancel::CancelToken, regex_filter::RegexFilter, Filter};
use crate::index::reader::IndexReader;
use crate::mcp::types::*;
use crate::reader::file_reader::FileReader;
use memchr::memchr_iter;
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use std::sync::Arc;

impl LazyTailMcp {
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn search_impl(
        path: &Path,
        pattern: &str,
        mode: SearchMode,
        case_sensitive: bool,
        max_results: usize,
        context_lines: usize,
        raw: bool,
        output: OutputFormat,
        full_content: bool,
    ) -> String {
        let max_results = max_results.min(1000);
        let context_lines = context_lines.min(50);

        // Use SIMD fast path for plain text, generic path for regex
        let rx = match mode {
            SearchMode::Plain => SearchEngine::search_file_fast(
                path,
                pattern.as_bytes(),
                case_sensitive,
                CancelToken::new(),
            ),
            SearchMode::Regex => {
                let filter: Arc<dyn Filter> = match RegexFilter::new(pattern, case_sensitive) {
                    Ok(f) => Arc::new(f),
                    Err(e) => return error_response(format!("Invalid regex pattern: {}", e)),
                };
                SearchEngine::search_file(path, filter, None, None, None, CancelToken::new())
            }
        };
        let rx = match rx {
            Ok(rx) => rx,
            Err(e) => {
                return error_response(format!("Failed to search file '{}': {}", path.display(), e))
            }
        };

        let (matching_indices, lines_searched) = match collect_filter_results(rx) {
            Ok(r) => r,
            Err(e) => return error_response(format!("Search error: {}", e)),
        };

        Self::build_search_response(
            path,
            matching_indices,
            lines_searched,
            max_results,
            context_lines,
            raw,
            output,
            full_content,
        )
    }

    /// Assemble a SearchResponse from collected filter results — shared by search and query paths.
    #[allow(clippy::too_many_arguments)]
    fn build_search_response(
        path: &Path,
        mut matching_indices: Vec<usize>,
        lines_searched: usize,
        max_results: usize,
        context_lines: usize,
        raw: bool,
        output: OutputFormat,
        full_content: bool,
    ) -> String {
        let total_matches = matching_indices.len();
        let truncated = total_matches > max_results;
        matching_indices.truncate(max_results);

        let matches = if matching_indices.is_empty() {
            Vec::new()
        } else {
            match Self::get_lines_content(path, &matching_indices, context_lines) {
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

        if !raw {
            strip_search_response(&mut response);
        }
        if !full_content {
            truncate_search_response(&mut response);
        }

        format_search(&response, output)
    }

    /// Build an aggregation response from matching indices.
    fn build_aggregation_response(
        path: &Path,
        matching_indices: &[usize],
        lines_searched: usize,
        aggregation: &crate::filter::query::Aggregation,
        parser: &crate::filter::query::Parser,
        output: OutputFormat,
    ) -> String {
        let mut reader = match FileReader::new(path) {
            Ok(r) => r,
            Err(e) => return error_response(format!("Failed to open file: {}", e)),
        };

        let result = crate::filter::aggregation::AggregationResult::compute(
            &mut reader,
            matching_indices,
            aggregation,
            parser,
        );

        let response = AggregationResponse {
            groups: result
                .groups
                .iter()
                .map(|g| AggregationGroupInfo {
                    key: g.key.iter().cloned().collect(),
                    count: g.count,
                })
                .collect(),
            total_matches: result.total_matches,
            lines_searched,
        };

        crate::mcp::format::format_aggregation(&response, output)
    }

    pub(crate) fn query_impl(
        path: &Path,
        query: crate::filter::query::FilterQuery,
        max_results: usize,
        context_lines: usize,
        raw: bool,
        output: OutputFormat,
        full_content: bool,
    ) -> String {
        let max_results = max_results.min(1000);
        let context_lines = context_lines.min(50);

        // Extract aggregation before filtering (filter operates without aggregate clause)
        let aggregate = query.aggregate.clone();
        let parser = query.parser.clone();
        let mut filter_query = query;
        filter_query.aggregate = None;

        let index = IndexReader::open(path);

        let query_filter = match QueryFilter::new(filter_query.clone()) {
            Ok(f) => f,
            Err(e) => return error_response(format!("Invalid query: {}", e)),
        };

        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        let rx = match SearchEngine::search_file(
            path,
            filter,
            Some(&filter_query),
            index.as_ref(),
            None,
            CancelToken::new(),
        ) {
            Ok(rx) => rx,
            Err(e) => {
                return error_response(format!("Failed to search file '{}': {}", path.display(), e))
            }
        };

        let (matching_indices, lines_searched) = match collect_filter_results(rx) {
            Ok(r) => r,
            Err(e) => return error_response(format!("Query error: {}", e)),
        };

        // If aggregation is requested, compute and return aggregation response
        if let Some(agg) = aggregate {
            return Self::build_aggregation_response(
                path,
                &matching_indices,
                lines_searched,
                &agg,
                &parser,
                output,
            );
        }

        Self::build_search_response(
            path,
            matching_indices,
            lines_searched,
            max_results,
            context_lines,
            raw,
            output,
            full_content,
        )
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
}
