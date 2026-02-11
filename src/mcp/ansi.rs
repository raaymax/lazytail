//! ANSI escape code stripping for MCP responses.
//!
//! MCP responses are consumed by AI models where ANSI escape sequences waste tokens.
//! The ESC byte (0x1b) serializes as `\u001b` in JSON — 6 chars per escape.
//! Lines with heavy ANSI styling (colored borders, hyperlinks) become massively inflated.

use regex::Regex;
use std::sync::LazyLock;

/// Regex matching ANSI escape sequences:
/// - CSI sequences: ESC [ ... (params) final_byte  (colors, cursor movement, etc.)
/// - OSC sequences: ESC ] ... ST  (hyperlinks, window titles, etc.)
/// - Character set designators: ESC ( B, ESC ) 0, etc.
/// - Simple two-byte escapes: ESC =, ESC >, ESC M, etc.
static ANSI_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"\x1b\[[0-9;?]*[ -/]*[@-~]",          // CSI sequences
        r"|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)", // OSC sequences (ST = BEL or ESC \)
        r"|\x1b[()][A-Z0-9]",                  // Character set designators
        r"|\x1b[^\[\]()0-9]",                  // Simple two-byte escapes
    ))
    .expect("ANSI regex must compile")
});

/// Strip all ANSI escape sequences from a string.
pub fn strip_ansi(s: &str) -> String {
    ANSI_RE.replace_all(s, "").into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_passthrough() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn empty_string() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn sgr_colors() {
        // Red text
        assert_eq!(strip_ansi("\x1b[31mhello\x1b[0m"), "hello");
        // Green background
        assert_eq!(strip_ansi("\x1b[42mworld\x1b[0m"), "world");
    }

    #[test]
    fn color_256() {
        assert_eq!(strip_ansi("\x1b[38;5;196mred\x1b[0m"), "red");
    }

    #[test]
    fn truecolor_rgb() {
        assert_eq!(strip_ansi("\x1b[38;2;255;0;0mred\x1b[0m"), "red");
    }

    #[test]
    fn bold_and_reset() {
        assert_eq!(strip_ansi("\x1b[1mbold\x1b[0m plain"), "bold plain");
    }

    #[test]
    fn cursor_movement() {
        // Move cursor up 2 lines
        assert_eq!(strip_ansi("\x1b[2Ahello"), "hello");
        // Move cursor to column 10
        assert_eq!(strip_ansi("\x1b[10Gworld"), "world");
        // Erase line
        assert_eq!(strip_ansi("\x1b[2Ktext"), "text");
    }

    #[test]
    fn osc_hyperlinks() {
        // OSC 8 hyperlink with BEL terminator
        assert_eq!(
            strip_ansi("\x1b]8;;https://example.com\x07link\x1b]8;;\x07"),
            "link"
        );
        // OSC 8 hyperlink with ST terminator (ESC \)
        assert_eq!(
            strip_ansi("\x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\"),
            "link"
        );
    }

    #[test]
    fn mixed_content() {
        let input = "\x1b[1;32m[INFO]\x1b[0m \x1b[36m2024-01-01\x1b[0m Server started on port \x1b[33m8080\x1b[0m";
        assert_eq!(
            strip_ansi(input),
            "[INFO] 2024-01-01 Server started on port 8080"
        );
    }

    #[test]
    fn character_set_designators() {
        assert_eq!(strip_ansi("\x1b(Bhello\x1b)0"), "hello");
    }

    #[test]
    fn multiple_sequences_adjacent() {
        assert_eq!(strip_ansi("\x1b[1m\x1b[31m\x1b[42mtext\x1b[0m"), "text");
    }
}
