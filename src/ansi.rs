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
