use memchr::{memchr, memchr2};

// Severity — bits 0-2
pub const SEVERITY_MASK: u32 = 0b111;
pub const SEVERITY_UNKNOWN: u32 = 0;
pub const SEVERITY_TRACE: u32 = 1;
pub const SEVERITY_DEBUG: u32 = 2;
pub const SEVERITY_INFO: u32 = 3;
pub const SEVERITY_WARN: u32 = 4;
pub const SEVERITY_ERROR: u32 = 5;
pub const SEVERITY_FATAL: u32 = 6;

// Format flags — bits 3-9
pub const FLAG_FORMAT_JSON: u32 = 1 << 3;
pub const FLAG_FORMAT_LOGFMT: u32 = 1 << 4;
pub const FLAG_HAS_ANSI: u32 = 1 << 5;
pub const FLAG_HAS_TIMESTAMP: u32 = 1 << 6;
pub const FLAG_HAS_TRACE_ID: u32 = 1 << 7;
pub const FLAG_IS_EMPTY: u32 = 1 << 8;
pub const FLAG_IS_MULTILINE_CONT: u32 = 1 << 9;

// Template ID — bits 16-31
const TEMPLATE_SHIFT: u32 = 16;
const TEMPLATE_MASK: u32 = 0xFFFF_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Unknown,
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl Severity {
    pub fn from_flags(flags: u32) -> Self {
        match flags & SEVERITY_MASK {
            SEVERITY_TRACE => Severity::Trace,
            SEVERITY_DEBUG => Severity::Debug,
            SEVERITY_INFO => Severity::Info,
            SEVERITY_WARN => Severity::Warn,
            SEVERITY_ERROR => Severity::Error,
            SEVERITY_FATAL => Severity::Fatal,
            _ => Severity::Unknown,
        }
    }

    pub fn to_bits(self) -> u32 {
        match self {
            Severity::Unknown => SEVERITY_UNKNOWN,
            Severity::Trace => SEVERITY_TRACE,
            Severity::Debug => SEVERITY_DEBUG,
            Severity::Info => SEVERITY_INFO,
            Severity::Warn => SEVERITY_WARN,
            Severity::Error => SEVERITY_ERROR,
            Severity::Fatal => SEVERITY_FATAL,
        }
    }
}

/// Extract the template ID (bits 16-31) from a flags value.
pub fn template_id(flags: u32) -> u16 {
    ((flags & TEMPLATE_MASK) >> TEMPLATE_SHIFT) as u16
}

/// Set the template ID (bits 16-31) on a flags value, preserving other bits.
pub fn with_template_id(flags: u32, id: u16) -> u32 {
    (flags & !TEMPLATE_MASK) | ((id as u32) << TEMPLATE_SHIFT)
}

/// Detect per-line metadata flags from raw bytes. No UTF-8 validation needed.
///
/// Uses memchr-assisted detection for ANSI/logfmt/timestamp and first-match-in-text
/// severity scanning with ANSI skip-in-place. No allocations, no regex.
pub fn detect_flags_bytes(bytes: &[u8]) -> u32 {
    // Find first non-whitespace byte
    let trimmed_start = match bytes.iter().position(|&b| !b.is_ascii_whitespace()) {
        Some(pos) => pos,
        None => return FLAG_IS_EMPTY,
    };

    let mut flags = 0u32;

    // JSON: trimmed line starts with '{'
    if bytes[trimmed_start] == b'{' {
        flags |= FLAG_FORMAT_JSON;
    }

    // ANSI: contains ESC byte (0x1B) — single SIMD scan over full line
    if memchr(0x1B, bytes).is_some() {
        flags |= FLAG_HAS_ANSI;
    }

    // Severity: first-match-in-text over first ~80 bytes, ANSI skip-in-place
    let scan_len = bytes.len().min(80);
    flags |= detect_severity_bytes(&bytes[..scan_len]);

    // Logfmt: memchr(b'=') + key/value verification (skip if already JSON)
    if flags & FLAG_FORMAT_JSON == 0 && detect_logfmt_bytes(&bytes[trimmed_start..]) {
        flags |= FLAG_FORMAT_LOGFMT;
    }

    // Timestamp: memchr2(b'-', b':') in first 30 trimmed bytes
    let ts_limit = bytes.len().min(trimmed_start + 30);
    if detect_timestamp_bytes(&bytes[trimmed_start..ts_limit]) {
        flags |= FLAG_HAS_TIMESTAMP;
    }

    flags
}

/// Thin wrapper: detect flags from a `&str` by delegating to the bytes version.
pub fn detect_flags(line: &str) -> u32 {
    detect_flags_bytes(line.as_bytes())
}

/// First-match-in-text severity detection with ANSI skip-in-place.
///
/// Scans left-to-right. At each word boundary, dispatches on the first byte
/// (case-folded via `| 0x20`). Returns the first severity keyword found.
fn detect_severity_bytes(bytes: &[u8]) -> u32 {
    let len = bytes.len();
    let mut i = 0;
    let mut after_ansi = false;

    while i < len {
        let b = bytes[i];

        // ANSI skip-in-place: advance past CSI sequence without temp buffer
        if b == 0x1B {
            i += 1;
            if i < len && bytes[i] == b'[' {
                i += 1;
                while i < len && !(0x40..=0x7E).contains(&bytes[i]) {
                    i += 1;
                }
                if i < len {
                    i += 1; // skip final byte (e.g. 'm')
                }
            }
            after_ansi = true;
            continue;
        }

        // Word boundary: start of buffer, after non-alpha, or after ANSI sequence
        let at_boundary = after_ansi || i == 0 || !bytes[i - 1].is_ascii_alphabetic();
        after_ansi = false;

        if !at_boundary {
            i += 1;
            continue;
        }

        let remaining = len - i;

        match b | 0x20 {
            b'f' if remaining >= 5 => {
                if eq_ci_word(bytes, i, b"fatal") {
                    return SEVERITY_FATAL;
                }
            }
            b'e' if remaining >= 5 => {
                if eq_ci_word(bytes, i, b"error") {
                    return SEVERITY_ERROR;
                }
            }
            b'w' if remaining >= 4 => {
                if remaining >= 7 && eq_ci_word(bytes, i, b"warning") {
                    return SEVERITY_WARN;
                }
                if eq_ci_word(bytes, i, b"warn") {
                    return SEVERITY_WARN;
                }
            }
            b'i' if remaining >= 4 => {
                if eq_ci_word(bytes, i, b"info") {
                    return SEVERITY_INFO;
                }
            }
            b'd' if remaining >= 5 => {
                if eq_ci_word(bytes, i, b"debug") {
                    return SEVERITY_DEBUG;
                }
            }
            b't' if remaining >= 5 => {
                if eq_ci_word(bytes, i, b"trace") {
                    return SEVERITY_TRACE;
                }
            }
            _ => {}
        }

        i += 1;
    }

    SEVERITY_UNKNOWN
}

/// Case-insensitive keyword match at `pos` with word-boundary check after.
/// Needle must be lowercase ASCII.
#[inline]
fn eq_ci_word(bytes: &[u8], pos: usize, needle: &[u8]) -> bool {
    let end = pos + needle.len();
    if end > bytes.len() {
        return false;
    }
    for j in 0..needle.len() {
        if (bytes[pos + j] | 0x20) != needle[j] {
            return false;
        }
    }
    // Word boundary after: end of buffer or next byte is non-alphabetic
    end >= bytes.len() || !bytes[end].is_ascii_alphabetic()
}

/// Detect logfmt via memchr(b'=') + backward key verification.
fn detect_logfmt_bytes(bytes: &[u8]) -> bool {
    let mut search_start = 0;
    while let Some(eq_offset) = memchr(b'=', &bytes[search_start..]) {
        let abs_eq = search_start + eq_offset;

        // Need at least 1 key char before and 1 value char after
        if abs_eq == 0 || abs_eq >= bytes.len() - 1 {
            search_start = abs_eq + 1;
            continue;
        }

        // Value char must exist and not be whitespace
        if bytes[abs_eq + 1].is_ascii_whitespace() {
            search_start = abs_eq + 1;
            continue;
        }

        // Scan backward from '=' to find key start (alphanumeric, '_', '.')
        let mut key_start = abs_eq;
        while key_start > 0 {
            let kb = bytes[key_start - 1];
            if kb.is_ascii_alphanumeric() || kb == b'_' || kb == b'.' {
                key_start -= 1;
            } else {
                break;
            }
        }

        // Key must be non-empty and at a word boundary (start of buffer or after whitespace)
        if key_start < abs_eq && (key_start == 0 || bytes[key_start - 1].is_ascii_whitespace()) {
            return true;
        }

        search_start = abs_eq + 1;
    }
    false
}

/// Detect timestamps via memchr2(b'-', b':') in the first 30 bytes.
/// Matches YYYY- (date) or HH:MM:SS (time) patterns.
fn detect_timestamp_bytes(bytes: &[u8]) -> bool {
    let limit = bytes.len().min(30);
    let scan = &bytes[..limit];

    let mut pos = 0;
    while let Some(offset) = memchr2(b'-', b':', &scan[pos..]) {
        let abs = pos + offset;

        if scan[abs] == b'-' {
            // YYYY- pattern: 4 digits immediately before '-'
            if abs >= 4
                && scan[abs - 4].is_ascii_digit()
                && scan[abs - 3].is_ascii_digit()
                && scan[abs - 2].is_ascii_digit()
                && scan[abs - 1].is_ascii_digit()
            {
                return true;
            }
        } else {
            // HH:MM:SS pattern: DD:DD:DD
            // Need 2 digits before ':', 2 digits + ':' + 2 digits after
            if abs >= 2
                && abs + 6 <= scan.len()
                && scan[abs - 2].is_ascii_digit()
                && scan[abs - 1].is_ascii_digit()
                && scan[abs + 1].is_ascii_digit()
                && scan[abs + 2].is_ascii_digit()
                && scan[abs + 3] == b':'
                && scan[abs + 4].is_ascii_digit()
                && scan[abs + 5].is_ascii_digit()
            {
                return true;
            }
        }

        pos = abs + 1;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- Severity enum ---

    #[test]
    fn severity_from_flags_all_values() {
        assert_eq!(Severity::from_flags(SEVERITY_UNKNOWN), Severity::Unknown);
        assert_eq!(Severity::from_flags(SEVERITY_TRACE), Severity::Trace);
        assert_eq!(Severity::from_flags(SEVERITY_DEBUG), Severity::Debug);
        assert_eq!(Severity::from_flags(SEVERITY_INFO), Severity::Info);
        assert_eq!(Severity::from_flags(SEVERITY_WARN), Severity::Warn);
        assert_eq!(Severity::from_flags(SEVERITY_ERROR), Severity::Error);
        assert_eq!(Severity::from_flags(SEVERITY_FATAL), Severity::Fatal);
    }

    #[test]
    fn severity_from_flags_ignores_other_bits() {
        // ERROR with JSON flag set
        let flags = SEVERITY_ERROR | FLAG_FORMAT_JSON;
        assert_eq!(Severity::from_flags(flags), Severity::Error);
    }

    #[test]
    fn severity_roundtrip() {
        for sev in [
            Severity::Unknown,
            Severity::Trace,
            Severity::Debug,
            Severity::Info,
            Severity::Warn,
            Severity::Error,
            Severity::Fatal,
        ] {
            assert_eq!(Severity::from_flags(sev.to_bits()), sev);
        }
    }

    #[test]
    fn severity_out_of_range_is_unknown() {
        assert_eq!(Severity::from_flags(7), Severity::Unknown);
    }

    // --- Template ID ---

    #[test]
    fn template_id_extract() {
        let flags = with_template_id(0, 42);
        assert_eq!(template_id(flags), 42);
    }

    #[test]
    fn template_id_max() {
        let flags = with_template_id(0, u16::MAX);
        assert_eq!(template_id(flags), u16::MAX);
    }

    #[test]
    fn template_id_preserves_lower_bits() {
        let flags = SEVERITY_ERROR | FLAG_FORMAT_JSON;
        let flags = with_template_id(flags, 123);
        assert_eq!(template_id(flags), 123);
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
        assert_ne!(flags & FLAG_FORMAT_JSON, 0);
    }

    #[test]
    fn template_id_zero_default() {
        assert_eq!(template_id(0), 0);
        assert_eq!(template_id(SEVERITY_ERROR), 0);
    }

    #[test]
    fn template_id_roundtrip() {
        for id in [0u16, 1, 100, 1000, u16::MAX] {
            let flags = with_template_id(SEVERITY_WARN | FLAG_HAS_ANSI, id);
            assert_eq!(template_id(flags), id);
        }
    }

    // --- detect_flags: empty ---

    #[test]
    fn detect_empty_line() {
        assert_eq!(detect_flags(""), FLAG_IS_EMPTY);
    }

    #[test]
    fn detect_whitespace_only() {
        assert_eq!(detect_flags("   \t  "), FLAG_IS_EMPTY);
    }

    // --- detect_flags: JSON ---

    #[test]
    fn detect_json_object() {
        let flags = detect_flags(r#"{"level":"error","msg":"fail"}"#);
        assert_ne!(flags & FLAG_FORMAT_JSON, 0);
    }

    #[test]
    fn detect_json_with_leading_whitespace() {
        let flags = detect_flags(r#"  {"key":"value"}"#);
        assert_ne!(flags & FLAG_FORMAT_JSON, 0);
    }

    #[test]
    fn detect_non_json_array() {
        let flags = detect_flags(r#"["not","json","object"]"#);
        assert_eq!(flags & FLAG_FORMAT_JSON, 0);
    }

    #[test]
    fn detect_non_json_plain_text() {
        let flags = detect_flags("just a plain log line");
        assert_eq!(flags & FLAG_FORMAT_JSON, 0);
    }

    // --- detect_flags: ANSI ---

    #[test]
    fn detect_ansi_escape() {
        let flags = detect_flags("\x1b[31mERROR\x1b[0m something failed");
        assert_ne!(flags & FLAG_HAS_ANSI, 0);
    }

    #[test]
    fn detect_no_ansi() {
        let flags = detect_flags("ERROR something failed");
        assert_eq!(flags & FLAG_HAS_ANSI, 0);
    }

    // --- detect_flags: severity (bare words) ---

    #[test]
    fn detect_severity_error_uppercase() {
        let flags = detect_flags("2024-01-01 ERROR something broke");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
    }

    #[test]
    fn detect_severity_error_lowercase() {
        let flags = detect_flags("2024-01-01 error something broke");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
    }

    #[test]
    fn detect_severity_warn_uppercase() {
        let flags = detect_flags("2024-01-01 WARN disk usage high");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_WARN);
    }

    #[test]
    fn detect_severity_warning() {
        let flags = detect_flags("2024-01-01 WARNING disk usage high");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_WARN);
    }

    #[test]
    fn detect_severity_info() {
        let flags = detect_flags("2024-01-01 INFO server started");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_INFO);
    }

    #[test]
    fn detect_severity_debug() {
        let flags = detect_flags("2024-01-01 DEBUG loading config");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_DEBUG);
    }

    #[test]
    fn detect_severity_trace() {
        let flags = detect_flags("2024-01-01 TRACE entering function");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_TRACE);
    }

    #[test]
    fn detect_severity_fatal() {
        let flags = detect_flags("2024-01-01 FATAL out of memory");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_FATAL);
    }

    // --- detect_flags: severity (bracketed) ---

    #[test]
    fn detect_severity_bracketed_error() {
        let flags = detect_flags("[ERROR] connection refused");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
    }

    #[test]
    fn detect_severity_bracketed_warn() {
        let flags = detect_flags("[WARN] retry attempt 3");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_WARN);
    }

    #[test]
    fn detect_severity_bracketed_info() {
        let flags = detect_flags("[INFO] startup complete");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_INFO);
    }

    // --- detect_flags: severity (logfmt) ---

    #[test]
    fn detect_severity_logfmt_error() {
        let flags = detect_flags("ts=2024-01-01 level=error msg=failed");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
    }

    #[test]
    fn detect_severity_logfmt_warn() {
        let flags = detect_flags("ts=2024-01-01 level=warn msg=slow");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_WARN);
    }

    // --- detect_flags: severity (JSON) ---

    #[test]
    fn detect_severity_json_level_error() {
        let flags = detect_flags(r#"{"level":"error","msg":"timeout"}"#);
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
    }

    #[test]
    fn detect_severity_json_level_info() {
        let flags = detect_flags(r#"{"level":"info","msg":"started"}"#);
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_INFO);
    }

    // --- detect_flags: severity (no match) ---

    #[test]
    fn detect_severity_unknown() {
        let flags = detect_flags("just a plain line with no severity");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_UNKNOWN);
    }

    #[test]
    fn detect_severity_not_in_word() {
        // "information" should NOT match "info"
        let flags = detect_flags("information about the system");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_UNKNOWN);
    }

    #[test]
    fn detect_severity_not_in_stacktrace() {
        // "stacktrace" should NOT match "trace"
        let flags = detect_flags("stacktrace: NullPointerException");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_UNKNOWN);
    }

    // --- detect_flags: logfmt ---

    #[test]
    fn detect_logfmt_line() {
        let flags = detect_flags("ts=2024-01-01 level=info msg=hello");
        assert_ne!(flags & FLAG_FORMAT_LOGFMT, 0);
    }

    #[test]
    fn detect_logfmt_with_dots() {
        let flags = detect_flags("http.method=GET http.status=200");
        assert_ne!(flags & FLAG_FORMAT_LOGFMT, 0);
    }

    #[test]
    fn detect_not_logfmt_plain() {
        let flags = detect_flags("this is a plain log message");
        assert_eq!(flags & FLAG_FORMAT_LOGFMT, 0);
    }

    #[test]
    fn detect_not_logfmt_json() {
        // JSON lines should not also flag logfmt
        let flags = detect_flags(r#"{"key":"value"}"#);
        assert_eq!(flags & FLAG_FORMAT_LOGFMT, 0);
    }

    #[test]
    fn detect_not_logfmt_url() {
        // URL query params should not match (non-alnum chars in "key")
        let flags = detect_flags("http://example.com?foo=bar");
        assert_eq!(flags & FLAG_FORMAT_LOGFMT, 0);
    }

    // --- detect_flags: timestamp ---

    #[test]
    fn detect_timestamp_iso() {
        let flags = detect_flags("2024-01-15T14:30:05Z ERROR something");
        assert_ne!(flags & FLAG_HAS_TIMESTAMP, 0);
    }

    #[test]
    fn detect_timestamp_date_only() {
        let flags = detect_flags("2024-01-15 ERROR something");
        assert_ne!(flags & FLAG_HAS_TIMESTAMP, 0);
    }

    #[test]
    fn detect_timestamp_time_only() {
        let flags = detect_flags("14:30:05 ERROR something");
        assert_ne!(flags & FLAG_HAS_TIMESTAMP, 0);
    }

    #[test]
    fn detect_no_timestamp() {
        let flags = detect_flags("ERROR something happened");
        assert_eq!(flags & FLAG_HAS_TIMESTAMP, 0);
    }

    // --- detect_flags: combined ---

    #[test]
    fn detect_combined_json_error() {
        let flags = detect_flags(r#"{"level":"error","msg":"fail"}"#);
        assert_ne!(flags & FLAG_FORMAT_JSON, 0);
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
    }

    #[test]
    fn detect_combined_ansi_warn() {
        let flags = detect_flags("\x1b[33mWARN\x1b[0m disk space low");
        assert_ne!(flags & FLAG_HAS_ANSI, 0);
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_WARN);
    }

    #[test]
    fn detect_combined_logfmt_timestamp_info() {
        let flags = detect_flags("2024-01-01T10:00:00Z level=info msg=started");
        assert_ne!(flags & FLAG_FORMAT_LOGFMT, 0);
        assert_ne!(flags & FLAG_HAS_TIMESTAMP, 0);
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_INFO);
    }

    // --- Flag mask filtering ---

    #[test]
    fn flags_mask_filtering() {
        // Simulate "all JSON ERROR lines"
        let mask = SEVERITY_MASK | FLAG_FORMAT_JSON;
        let want = SEVERITY_ERROR | FLAG_FORMAT_JSON;

        let flags = detect_flags(r#"{"level":"error","msg":"fail"}"#);
        assert_eq!(flags & mask, want);

        // Non-JSON error should not match
        let flags2 = detect_flags("2024-01-01 ERROR something");
        assert_ne!(flags2 & mask, want);

        // JSON info should not match
        let flags3 = detect_flags(r#"{"level":"info","msg":"ok"}"#);
        assert_ne!(flags3 & mask, want);
    }

    // --- detect_flags_bytes: new tests ---

    #[test]
    fn bytes_basic_roundtrip() {
        let lines = [
            r#"{"level":"error","msg":"fail"}"#,
            "2024-01-01 INFO server started",
            "ts=2024-01-01 level=warn msg=slow",
            "\x1b[31mERROR\x1b[0m something failed",
            "",
            "   ",
        ];
        for line in lines {
            assert_eq!(
                detect_flags_bytes(line.as_bytes()),
                detect_flags(line),
                "mismatch for line: {line:?}"
            );
        }
    }

    #[test]
    fn bytes_non_utf8() {
        // Raw bytes with high bytes (0x80+) should not panic
        let data = b"ERROR \x80\x81\x82 something failed";
        let flags = detect_flags_bytes(data);
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
    }

    #[test]
    fn bytes_empty() {
        assert_eq!(detect_flags_bytes(b""), FLAG_IS_EMPTY);
    }

    #[test]
    fn bytes_first_match_info_over_error() {
        // First-match: "INFO" appears before "error" → returns INFO
        let flags = detect_flags_bytes(b"INFO processing error count");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_INFO);
    }

    #[test]
    fn bytes_first_match_warn_over_fatal() {
        // First-match: "WARN" appears before "fatal" → returns WARN
        let flags = detect_flags_bytes(b"WARN fatal error in log");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_WARN);
    }

    #[test]
    fn bytes_ansi_skip_in_place() {
        // Severity detected through inline ANSI sequences (no temp buffer)
        let data = b"\x1b[1;31mERROR\x1b[0m connection lost";
        let flags = detect_flags_bytes(data);
        assert_ne!(flags & FLAG_HAS_ANSI, 0);
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
    }

    #[test]
    fn bytes_logfmt_memchr() {
        let flags = detect_flags_bytes(b"ts=2024-01-01 level=info msg=started");
        assert_ne!(flags & FLAG_FORMAT_LOGFMT, 0);
    }

    #[test]
    fn bytes_timestamp_memchr2() {
        let flags = detect_flags_bytes(b"2024-01-15T14:30:05Z INFO request");
        assert_ne!(flags & FLAG_HAS_TIMESTAMP, 0);
    }

    #[test]
    fn bytes_long_line_severity_in_prefix() {
        // Severity keyword within first 80 bytes is detected
        let mut line = b"2024-01-01 ERROR ".to_vec();
        line.extend(vec![b'x'; 200]);
        let flags = detect_flags_bytes(&line);
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);

        // Severity keyword only after 80 bytes is NOT detected
        let mut line2 = vec![b'x'; 81];
        line2.extend(b" ERROR something");
        let flags2 = detect_flags_bytes(&line2);
        assert_eq!(flags2 & SEVERITY_MASK, SEVERITY_UNKNOWN);
    }

    #[test]
    fn bytes_crlf_handling() {
        let flags = detect_flags_bytes(b"2024-01-01 ERROR something broke\r\n");
        assert_eq!(flags & SEVERITY_MASK, SEVERITY_ERROR);
        assert_ne!(flags & FLAG_HAS_TIMESTAMP, 0);
    }
}
