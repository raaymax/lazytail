//! Plain text formatters for MCP tool responses.
//!
//! These produce a compact, human/AI-readable format that avoids the JSON escaping
//! explosion that occurs when log lines contain JSON content. Format:
//! - `--- key: value` header lines
//! - Blank line separator
//! - `{line_number}|{content}` for content lines
//! - `> ` prefix for match/target lines, `  ` for context lines

use std::fmt::Write;

use super::types::{GetContextResponse, GetLinesResponse, GetStatsResponse, SearchResponse};

/// Format a GetLinesResponse (used by get_lines and get_tail) as plain text.
pub fn format_lines_text(resp: &GetLinesResponse) -> String {
    let mut out = String::with_capacity(resp.lines.len() * 80 + 64);
    writeln!(out, "--- total_lines: {}", resp.total_lines).unwrap();
    writeln!(out, "--- has_more: {}", resp.has_more).unwrap();
    out.push('\n');

    for line in &resp.lines {
        writeln!(out, "{}|{}", line.line_number, line.content).unwrap();
    }

    out
}

/// Maximum total output size for search results in bytes. Once exceeded, remaining
/// matches are omitted and metadata is appended so the caller knows to narrow the search.
const SEARCH_MAX_OUTPUT_BYTES: usize = 80_000;

/// Format a SearchResponse as plain text.
pub fn format_search_text(resp: &SearchResponse) -> String {
    let mut out = String::with_capacity(resp.matches.len() * 160 + 128);
    writeln!(out, "--- total_matches: {}", resp.total_matches).unwrap();
    writeln!(out, "--- truncated: {}", resp.truncated).unwrap();
    writeln!(out, "--- lines_searched: {}", resp.lines_searched).unwrap();
    out.push('\n');

    for (i, m) in resp.matches.iter().enumerate() {
        if out.len() > SEARCH_MAX_OUTPUT_BYTES {
            writeln!(out, "--- output_truncated_at_bytes: {}", out.len()).unwrap();
            writeln!(out, "--- matches_shown: {}", i).unwrap();
            break;
        }

        if i > 0 {
            out.push('\n');
        }
        writeln!(out, "=== match").unwrap();

        let context_start = m.line_number.saturating_sub(m.before.len());
        for (j, line) in m.before.iter().enumerate() {
            writeln!(out, "  {}|{}", context_start + j, line).unwrap();
        }

        writeln!(out, "> {}|{}", m.line_number, m.content).unwrap();

        for (j, line) in m.after.iter().enumerate() {
            writeln!(out, "  {}|{}", m.line_number + 1 + j, line).unwrap();
        }
    }

    out
}

/// Format a GetContextResponse as plain text.
pub fn format_context_text(resp: &GetContextResponse) -> String {
    let lines_count = resp.before_lines.len() + 1 + resp.after_lines.len();
    let mut out = String::with_capacity(lines_count * 80 + 64);
    writeln!(out, "--- total_lines: {}", resp.total_lines).unwrap();
    out.push('\n');

    for line in &resp.before_lines {
        writeln!(out, "  {}|{}", line.line_number, line.content).unwrap();
    }

    writeln!(
        out,
        "> {}|{}",
        resp.target_line.line_number, resp.target_line.content
    )
    .unwrap();

    for line in &resp.after_lines {
        writeln!(out, "  {}|{}", line.line_number, line.content).unwrap();
    }

    out
}

/// Format a GetStatsResponse as plain text.
pub fn format_stats_text(resp: &GetStatsResponse) -> String {
    let mut out = String::with_capacity(256);
    writeln!(out, "--- source: {}", resp.source).unwrap();
    writeln!(out, "--- has_index: {}", resp.has_index).unwrap();
    writeln!(out, "--- indexed_lines: {}", resp.indexed_lines).unwrap();
    writeln!(out, "--- log_file_size: {}", resp.log_file_size).unwrap();
    writeln!(out, "--- columns: {}", resp.columns.join(", ")).unwrap();

    if let Some(ref counts) = resp.severity_counts {
        out.push('\n');
        writeln!(out, "severity_counts:").unwrap();
        writeln!(out, "  fatal: {}", counts.fatal).unwrap();
        writeln!(out, "  error: {}", counts.error).unwrap();
        writeln!(out, "  warn: {}", counts.warn).unwrap();
        writeln!(out, "  info: {}", counts.info).unwrap();
        writeln!(out, "  debug: {}", counts.debug).unwrap();
        writeln!(out, "  trace: {}", counts.trace).unwrap();
        writeln!(out, "  unknown: {}", counts.unknown).unwrap();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::types::{LineInfo, SearchMatch};

    #[test]
    fn lines_text_basic() {
        let resp = GetLinesResponse {
            lines: vec![
                LineInfo {
                    line_number: 0,
                    content: "first line".into(),
                },
                LineInfo {
                    line_number: 1,
                    content: "second line".into(),
                },
            ],
            total_lines: 100,
            has_more: true,
        };
        let text = format_lines_text(&resp);
        assert!(text.starts_with("--- total_lines: 100\n"));
        assert!(text.contains("--- has_more: true\n"));
        assert!(text.contains("0|first line\n"));
        assert!(text.contains("1|second line\n"));
    }

    #[test]
    fn lines_text_empty() {
        let resp = GetLinesResponse {
            lines: vec![],
            total_lines: 0,
            has_more: false,
        };
        let text = format_lines_text(&resp);
        assert!(text.contains("--- total_lines: 0\n"));
        assert!(text.contains("--- has_more: false\n"));
        // After the blank line separator, no content lines
        let after_blank = text.split("\n\n").nth(1).unwrap();
        assert!(after_blank.is_empty());
    }

    #[test]
    fn lines_text_json_content_verbatim() {
        let json_line = r#"{"level":"error","msg":"connection timeout"}"#;
        let resp = GetLinesResponse {
            lines: vec![LineInfo {
                line_number: 42,
                content: json_line.into(),
            }],
            total_lines: 100,
            has_more: false,
        };
        let text = format_lines_text(&resp);
        // JSON should appear verbatim, no escaping
        assert!(text.contains(&format!("42|{json_line}\n")));
        assert!(!text.contains('\\'));
    }

    #[test]
    fn lines_text_pipe_in_content() {
        let resp = GetLinesResponse {
            lines: vec![LineInfo {
                line_number: 5,
                content: "data | more | pipes".into(),
            }],
            total_lines: 10,
            has_more: false,
        };
        let text = format_lines_text(&resp);
        assert!(text.contains("5|data | more | pipes\n"));
    }

    #[test]
    fn search_text_basic() {
        let resp = SearchResponse {
            matches: vec![SearchMatch {
                line_number: 42,
                content: "error: something failed".into(),
                before: vec![],
                after: vec![],
            }],
            total_matches: 1,
            truncated: false,
            lines_searched: 1000,
        };
        let text = format_search_text(&resp);
        assert!(text.contains("--- total_matches: 1\n"));
        assert!(text.contains("--- truncated: false\n"));
        assert!(text.contains("--- lines_searched: 1000\n"));
        assert!(text.contains("=== match\n"));
        assert!(text.contains("> 42|error: something failed\n"));
    }

    #[test]
    fn search_text_with_context() {
        let resp = SearchResponse {
            matches: vec![SearchMatch {
                line_number: 10,
                content: "match line".into(),
                before: vec!["before 1".into(), "before 2".into()],
                after: vec!["after 1".into()],
            }],
            total_matches: 1,
            truncated: false,
            lines_searched: 100,
        };
        let text = format_search_text(&resp);
        assert!(text.contains("  8|before 1\n"));
        assert!(text.contains("  9|before 2\n"));
        assert!(text.contains("> 10|match line\n"));
        assert!(text.contains("  11|after 1\n"));
    }

    #[test]
    fn search_text_truncated() {
        let resp = SearchResponse {
            matches: vec![],
            total_matches: 500,
            truncated: true,
            lines_searched: 10000,
        };
        let text = format_search_text(&resp);
        assert!(text.contains("--- total_matches: 500\n"));
        assert!(text.contains("--- truncated: true\n"));
    }

    #[test]
    fn search_text_empty() {
        let resp = SearchResponse {
            matches: vec![],
            total_matches: 0,
            truncated: false,
            lines_searched: 100,
        };
        let text = format_search_text(&resp);
        assert!(text.contains("--- total_matches: 0\n"));
        assert!(!text.contains("=== match"));
    }

    #[test]
    fn search_text_json_content_verbatim() {
        let json_content = r#"{"level":"error","msg":"timeout"}"#;
        let resp = SearchResponse {
            matches: vec![SearchMatch {
                line_number: 42,
                content: json_content.into(),
                before: vec![],
                after: vec![],
            }],
            total_matches: 1,
            truncated: false,
            lines_searched: 100,
        };
        let text = format_search_text(&resp);
        assert!(text.contains(&format!("> 42|{json_content}\n")));
        assert!(!text.contains('\\'));
    }

    #[test]
    fn context_text_basic() {
        let resp = GetContextResponse {
            before_lines: vec![
                LineInfo {
                    line_number: 40,
                    content: "before line 1".into(),
                },
                LineInfo {
                    line_number: 41,
                    content: "before line 2".into(),
                },
            ],
            target_line: LineInfo {
                line_number: 42,
                content: "target line content".into(),
            },
            after_lines: vec![
                LineInfo {
                    line_number: 43,
                    content: "after line 1".into(),
                },
                LineInfo {
                    line_number: 44,
                    content: "after line 2".into(),
                },
            ],
            total_lines: 1000,
        };
        let text = format_context_text(&resp);
        assert!(text.contains("--- total_lines: 1000\n"));
        assert!(text.contains("  40|before line 1\n"));
        assert!(text.contains("  41|before line 2\n"));
        assert!(text.contains("> 42|target line content\n"));
        assert!(text.contains("  43|after line 1\n"));
        assert!(text.contains("  44|after line 2\n"));
    }

    #[test]
    fn context_text_no_surrounding_lines() {
        let resp = GetContextResponse {
            before_lines: vec![],
            target_line: LineInfo {
                line_number: 0,
                content: "only line".into(),
            },
            after_lines: vec![],
            total_lines: 1,
        };
        let text = format_context_text(&resp);
        assert!(text.contains("--- total_lines: 1\n"));
        assert!(text.contains("> 0|only line\n"));
    }

    #[test]
    fn search_text_output_budget_caps_output() {
        // Build a response with many matches whose lines are ~500 chars each.
        // 200 matches * ~500 chars = ~100KB, which exceeds the 80KB budget.
        let matches: Vec<SearchMatch> = (0..200)
            .map(|i| SearchMatch {
                line_number: i,
                content: "x".repeat(490),
                before: vec![],
                after: vec![],
            })
            .collect();
        let resp = SearchResponse {
            total_matches: 200,
            truncated: false,
            lines_searched: 10000,
            matches,
        };
        let text = format_search_text(&resp);

        // Output should be capped near 80KB
        assert!(
            text.len() < 100_000,
            "output should be capped, got {} bytes",
            text.len()
        );
        assert!(text.contains("--- output_truncated_at_bytes:"));
        assert!(text.contains("--- matches_shown:"));

        // Fewer than all 200 matches should be shown
        let shown: usize = text.matches("=== match").count();
        assert!(
            shown < 200,
            "should show fewer than 200 matches, got {shown}"
        );
        assert!(shown > 0, "should show at least some matches");
    }

    #[test]
    fn search_text_small_response_not_truncated() {
        let resp = SearchResponse {
            matches: vec![SearchMatch {
                line_number: 0,
                content: "short".into(),
                before: vec![],
                after: vec![],
            }],
            total_matches: 1,
            truncated: false,
            lines_searched: 100,
        };
        let text = format_search_text(&resp);
        assert!(!text.contains("output_truncated_at_bytes"));
        assert!(text.contains("=== match"));
    }
}
