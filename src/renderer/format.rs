use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// DateTime formatting mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateTimeMode {
    Relative,
    Absolute,
}

/// Duration input unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DurationUnit {
    Milliseconds,
    Nanoseconds,
}

/// Field format specification parsed at compile time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldFormat {
    DateTime(DateTimeMode),
    Duration(DurationUnit),
    Bytes,
}

impl FieldFormat {
    /// Parse a `format:` string into a FieldFormat.
    /// Returns None for unrecognized format strings (including "json" and "key_value"
    /// which are handled separately as RestFormat).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "datetime" | "datetime:relative" => Some(FieldFormat::DateTime(DateTimeMode::Relative)),
            "datetime:absolute" => Some(FieldFormat::DateTime(DateTimeMode::Absolute)),
            "duration" | "duration:ms" => Some(FieldFormat::Duration(DurationUnit::Milliseconds)),
            "duration:ns" => Some(FieldFormat::Duration(DurationUnit::Nanoseconds)),
            "bytes" => Some(FieldFormat::Bytes),
            _ => None,
        }
    }

    /// Apply the format to a raw field value. Returns None if the value can't be parsed.
    pub fn apply(&self, value: &str) -> Option<String> {
        match self {
            FieldFormat::DateTime(mode) => format_datetime(value, mode),
            FieldFormat::Duration(unit) => format_duration(value, unit),
            FieldFormat::Bytes => format_bytes(value),
        }
    }
}

/// Parse an ISO 8601 timestamp and format it.
pub fn format_datetime(value: &str, mode: &DateTimeMode) -> Option<String> {
    let epoch_secs = parse_iso8601(value)?;
    match mode {
        DateTimeMode::Relative => {
            let now_secs = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_secs() as i64;
            let diff = now_secs - epoch_secs;
            if diff < 0 {
                Some(format!("in {}s", -diff))
            } else if diff < 60 {
                Some(format!("{}s ago", diff))
            } else if diff < 3600 {
                Some(format!("{}m ago", diff / 60))
            } else if diff < 86400 {
                Some(format!("{}h ago", diff / 3600))
            } else {
                Some(format!("{}d ago", diff / 86400))
            }
        }
        DateTimeMode::Absolute => {
            // Convert epoch seconds back to date components
            let (year, month, day, hour, minute, second) = epoch_secs_to_components(epoch_secs);
            Some(format!(
                "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
                year, month, day, hour, minute, second
            ))
        }
    }
}

/// Parse a numeric string as duration and humanize it.
pub fn format_duration(value: &str, unit: &DurationUnit) -> Option<String> {
    let num: f64 = value.parse().ok()?;
    let ms = match unit {
        DurationUnit::Milliseconds => num,
        DurationUnit::Nanoseconds => num / 1_000_000.0,
    };
    Some(humanize_duration_ms(ms))
}

/// Parse a numeric string as bytes and humanize it.
pub fn format_bytes(value: &str) -> Option<String> {
    let num: f64 = value.parse().ok()?;
    Some(humanize_bytes(num))
}

/// Minimal ISO 8601 parser — extracts seconds since epoch.
/// Handles: "2024-01-15T10:30:00Z", "2024-01-15T10:30:00+00:00",
/// "2024-01-15T10:30:00.123Z", "2024-01-15 10:30:00"
fn parse_iso8601(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 19 {
        return None;
    }

    let year: i64 = s.get(0..4)?.parse().ok()?;
    if s.as_bytes().get(4)? != &b'-' {
        return None;
    }
    let month: i64 = s.get(5..7)?.parse().ok()?;
    if s.as_bytes().get(7)? != &b'-' {
        return None;
    }
    let day: i64 = s.get(8..10)?.parse().ok()?;

    let sep = *s.as_bytes().get(10)?;
    if sep != b'T' && sep != b't' && sep != b' ' {
        return None;
    }

    let hour: i64 = s.get(11..13)?.parse().ok()?;
    if s.as_bytes().get(13)? != &b':' {
        return None;
    }
    let minute: i64 = s.get(14..16)?.parse().ok()?;
    if s.as_bytes().get(16)? != &b':' {
        return None;
    }
    let second: i64 = s.get(17..19)?.parse().ok()?;

    // Validate ranges
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) {
        return None;
    }
    if !(0..=23).contains(&hour) || !(0..=59).contains(&minute) || !(0..=60).contains(&second) {
        return None;
    }

    // Convert to epoch seconds (simplified — assumes UTC, uses basic day counting)
    let epoch = date_to_epoch(year, month, day, hour, minute, second);

    // Parse timezone offset if present (after seconds + optional fractional)
    let rest = &s[19..];
    let rest = if let Some(dot_rest) = rest.strip_prefix('.') {
        // Skip fractional seconds
        let end = dot_rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(dot_rest.len());
        &dot_rest[end..]
    } else {
        rest
    };

    let tz_offset_secs = if rest.is_empty() || rest == "Z" || rest == "z" {
        0i64
    } else if let Some(offset) = rest.strip_prefix('+') {
        parse_tz_offset(offset)?
    } else if let Some(offset) = rest.strip_prefix('-') {
        -parse_tz_offset(offset)?
    } else {
        return None;
    };

    Some(epoch - tz_offset_secs)
}

fn parse_tz_offset(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 2 {
        return None;
    }
    let (hh, mm) = if s.len() >= 5 && s.as_bytes()[2] == b':' {
        (
            s.get(0..2)?.parse::<i64>().ok()?,
            s.get(3..5)?.parse::<i64>().ok()?,
        )
    } else if s.len() >= 4 {
        (
            s.get(0..2)?.parse::<i64>().ok()?,
            s.get(2..4)?.parse::<i64>().ok()?,
        )
    } else {
        (s.get(0..2)?.parse::<i64>().ok()?, 0)
    };
    Some(hh * 3600 + mm * 60)
}

/// Convert date components to epoch seconds (UTC).
fn date_to_epoch(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64 {
    // Days from year 1970 to start of given year
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 9 } else { month - 3 };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    days * 86400 + hour * 3600 + minute * 60 + second
}

/// Convert epoch seconds to date components (UTC).
fn epoch_secs_to_components(epoch: i64) -> (i64, i64, i64, i64, i64, i64) {
    let secs_in_day = epoch.rem_euclid(86400);
    let days = epoch.div_euclid(86400) + 719468;
    let era = days.div_euclid(146097);
    let doe = days.rem_euclid(146097);
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };
    let hour = secs_in_day / 3600;
    let minute = (secs_in_day % 3600) / 60;
    let second = secs_in_day % 60;
    (year, month, day, hour, minute, second)
}

fn humanize_duration_ms(ms: f64) -> String {
    if ms < 1.0 {
        format!("{:.0}us", ms * 1000.0)
    } else if ms < 1000.0 {
        if ms == ms.floor() {
            format!("{}ms", ms as i64)
        } else {
            format!("{:.1}ms", ms)
        }
    } else if ms < 60_000.0 {
        let s = ms / 1000.0;
        if s == s.floor() {
            format!("{}s", s as i64)
        } else {
            format!("{:.1}s", s)
        }
    } else if ms < 3_600_000.0 {
        let total_secs = (ms / 1000.0) as i64;
        let m = total_secs / 60;
        let s = total_secs % 60;
        if s == 0 {
            format!("{}m", m)
        } else {
            format!("{}m {}s", m, s)
        }
    } else {
        let total_secs = (ms / 1000.0) as i64;
        let h = total_secs / 3600;
        let m = (total_secs % 3600) / 60;
        if m == 0 {
            format!("{}h", h)
        } else {
            format!("{}h {}m", h, m)
        }
    }
}

fn humanize_bytes(bytes: f64) -> String {
    if bytes < 1024.0 {
        format!("{} B", bytes as i64)
    } else if bytes < 1024.0 * 1024.0 {
        format!("{:.1} KB", bytes / 1024.0)
    } else if bytes < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", bytes / (1024.0 * 1024.0))
    } else if bytes < 1024.0 * 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} GB", bytes / (1024.0 * 1024.0 * 1024.0))
    } else {
        format!("{:.1} TB", bytes / (1024.0 * 1024.0 * 1024.0 * 1024.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_datetime_absolute() {
        let result = format_datetime("2024-01-15T10:30:00Z", &DateTimeMode::Absolute);
        assert_eq!(result, Some("2024-01-15 10:30:00".to_string()));
    }

    #[test]
    fn test_format_datetime_invalid() {
        assert_eq!(format_datetime("not a date", &DateTimeMode::Relative), None);
        assert_eq!(format_datetime("", &DateTimeMode::Relative), None);
        assert_eq!(format_datetime("2024", &DateTimeMode::Relative), None);
    }

    #[test]
    fn test_format_datetime_relative() {
        // Use a timestamp close to now to test relative formatting
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let five_min_ago = now - 300;
        let (y, mo, d, h, mi, s) = epoch_secs_to_components(five_min_ago as i64);
        let ts = format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", y, mo, d, h, mi, s);
        let result = format_datetime(&ts, &DateTimeMode::Relative).unwrap();
        assert!(
            result.contains("m ago"),
            "expected 'm ago', got: {}",
            result
        );
    }

    #[test]
    fn test_format_duration_ms() {
        assert_eq!(
            format_duration("42", &DurationUnit::Milliseconds),
            Some("42ms".to_string())
        );
        assert_eq!(
            format_duration("1500", &DurationUnit::Milliseconds),
            Some("1.5s".to_string())
        );
        assert_eq!(
            format_duration("65000", &DurationUnit::Milliseconds),
            Some("1m 5s".to_string())
        );
    }

    #[test]
    fn test_format_duration_ns() {
        assert_eq!(
            format_duration("1000000", &DurationUnit::Nanoseconds),
            Some("1ms".to_string())
        );
        assert_eq!(
            format_duration("1500000000", &DurationUnit::Nanoseconds),
            Some("1.5s".to_string())
        );
    }

    #[test]
    fn test_format_duration_invalid() {
        assert_eq!(format_duration("abc", &DurationUnit::Milliseconds), None);
        assert_eq!(format_duration("", &DurationUnit::Milliseconds), None);
    }

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes("500"), Some("500 B".to_string()));
        assert_eq!(format_bytes("1024"), Some("1.0 KB".to_string()));
        assert_eq!(format_bytes("1048576"), Some("1.0 MB".to_string()));
    }

    #[test]
    fn test_format_bytes_invalid() {
        assert_eq!(format_bytes("abc"), None);
        assert_eq!(format_bytes(""), None);
    }

    #[test]
    fn test_parse_iso8601_with_timezone() {
        let utc = parse_iso8601("2024-01-15T10:30:00Z").unwrap();
        let plus5 = parse_iso8601("2024-01-15T15:30:00+05:00").unwrap();
        assert_eq!(utc, plus5);
    }

    #[test]
    fn test_parse_iso8601_with_fractional() {
        let without = parse_iso8601("2024-01-15T10:30:00Z").unwrap();
        let with_frac = parse_iso8601("2024-01-15T10:30:00.123Z").unwrap();
        assert_eq!(without, with_frac);
    }

    #[test]
    fn test_parse_iso8601_space_separator() {
        let t_sep = parse_iso8601("2024-01-15T10:30:00Z").unwrap();
        let space = parse_iso8601("2024-01-15 10:30:00").unwrap();
        assert_eq!(t_sep, space);
    }

    #[test]
    fn test_field_format_parse() {
        assert_eq!(
            FieldFormat::parse("datetime"),
            Some(FieldFormat::DateTime(DateTimeMode::Relative))
        );
        assert_eq!(
            FieldFormat::parse("datetime:relative"),
            Some(FieldFormat::DateTime(DateTimeMode::Relative))
        );
        assert_eq!(
            FieldFormat::parse("datetime:absolute"),
            Some(FieldFormat::DateTime(DateTimeMode::Absolute))
        );
        assert_eq!(
            FieldFormat::parse("duration"),
            Some(FieldFormat::Duration(DurationUnit::Milliseconds))
        );
        assert_eq!(
            FieldFormat::parse("duration:ms"),
            Some(FieldFormat::Duration(DurationUnit::Milliseconds))
        );
        assert_eq!(
            FieldFormat::parse("duration:ns"),
            Some(FieldFormat::Duration(DurationUnit::Nanoseconds))
        );
        assert_eq!(FieldFormat::parse("bytes"), Some(FieldFormat::Bytes));
        assert_eq!(FieldFormat::parse("json"), None);
        assert_eq!(FieldFormat::parse("unknown"), None);
    }
}
