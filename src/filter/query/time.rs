//! Time value resolution for structured queries.
//!
//! Supports relative time expressions like `now-5m`, `now-1h30m` in filter values,
//! and timestamp-aware comparison for log line fields.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// Resolved epoch milliseconds for a time value.
pub type EpochMillis = i64;

/// Try to resolve a filter value as a relative time expression.
///
/// Supported formats:
/// - `now` — current UTC time
/// - `now-5s`, `now-30m`, `now-2h`, `now-1d` — relative offsets
/// - `now-1h30m`, `now-2d12h` — compound offsets
///
/// Returns epoch milliseconds if the value is a relative time expression.
pub fn resolve_relative_time(value: &str) -> Option<EpochMillis> {
    let value = value.trim();
    if !value.starts_with("now") {
        return None;
    }

    let now_millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;

    let rest = &value[3..];
    if rest.is_empty() {
        return Some(now_millis);
    }

    if let Some(suffix) = rest.strip_prefix('-') {
        let offset = parse_duration(suffix)?;
        Some(now_millis - offset.as_millis() as i64)
    } else if let Some(suffix) = rest.strip_prefix('+') {
        let offset = parse_duration(suffix)?;
        Some(now_millis + offset.as_millis() as i64)
    } else {
        None
    }
}

/// Parse a duration string like `5s`, `30m`, `2h`, `1d`, `1h30m`.
fn parse_duration(s: &str) -> Option<Duration> {
    if s.is_empty() {
        return None;
    }

    let mut total_secs: u64 = 0;
    let mut num_start = 0;
    let bytes = s.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }

        // We hit a unit character
        let num_str = &s[num_start..i];
        let num: u64 = num_str.parse().ok()?;

        let multiplier = match bytes[i] {
            b's' => 1,
            b'm' => 60,
            b'h' => 3600,
            b'd' => 86400,
            b'w' => 604800,
            _ => return None,
        };

        total_secs += num * multiplier;
        i += 1;
        num_start = i;
    }

    // Trailing digits without unit — not valid
    if num_start < bytes.len() {
        return None;
    }

    if total_secs == 0 {
        return None;
    }

    Some(Duration::from_secs(total_secs))
}

/// Try to parse a timestamp string from a log field into epoch milliseconds.
///
/// Supports:
/// - RFC 3339 / ISO 8601: `2024-01-15T10:55:00Z`, `2024-01-15T10:55:00.123Z`,
///   `2024-01-15T10:55:00+00:00`, `2024-01-15T10:55:00.123+05:30`
/// - Space-separated datetime: `2024-01-15 10:55:00`, `2024-01-15 10:55:00.123`
/// - Epoch seconds (10-digit number): `1705312500`
/// - Epoch milliseconds (13-digit number): `1705312500123`
pub fn parse_timestamp(value: &str) -> Option<EpochMillis> {
    let value = value.trim();

    // Try epoch numeric formats first (fastest check)
    if let Ok(num) = value.parse::<i64>() {
        if value.len() == 13 {
            // Epoch milliseconds
            return Some(num);
        } else if value.len() == 10 {
            // Epoch seconds
            return Some(num * 1000);
        }
        // Other numeric lengths — could be seconds or millis, try heuristic
        if num > 1_000_000_000_000 {
            return Some(num); // millis
        } else if num > 1_000_000_000 {
            return Some(num * 1000); // seconds
        }
    }

    // Try datetime formats
    parse_datetime(value)
}

/// Parse a datetime string into epoch milliseconds.
///
/// Handles ISO 8601 / RFC 3339 variants without external dependencies.
fn parse_datetime(s: &str) -> Option<EpochMillis> {
    // Need at least "YYYY-MM-DDThh:mm:ss" (19 chars)
    if s.len() < 19 {
        return None;
    }

    // Parse date portion: YYYY-MM-DD
    let year: i32 = s.get(0..4)?.parse().ok()?;
    if s.as_bytes().get(4)? != &b'-' {
        return None;
    }
    let month: u32 = s.get(5..7)?.parse().ok()?;
    if s.as_bytes().get(7)? != &b'-' {
        return None;
    }
    let day: u32 = s.get(8..10)?.parse().ok()?;

    // Separator: 'T' or ' '
    let sep = *s.as_bytes().get(10)?;
    if sep != b'T' && sep != b' ' {
        return None;
    }

    // Parse time portion: hh:mm:ss
    let hour: u32 = s.get(11..13)?.parse().ok()?;
    if s.as_bytes().get(13)? != &b':' {
        return None;
    }
    let minute: u32 = s.get(14..16)?.parse().ok()?;
    if s.as_bytes().get(16)? != &b':' {
        return None;
    }
    let second: u32 = s.get(17..19)?.parse().ok()?;

    // Parse optional fractional seconds
    let mut frac_millis: i64 = 0;
    let mut pos = 19;
    if s.as_bytes().get(pos) == Some(&b'.') {
        pos += 1;
        let frac_start = pos;
        while pos < s.len() && s.as_bytes()[pos].is_ascii_digit() {
            pos += 1;
        }
        let frac_str = &s[frac_start..pos];
        if !frac_str.is_empty() {
            // Normalize to milliseconds (3 digits)
            let padded = if frac_str.len() >= 3 {
                frac_str[..3].to_string()
            } else {
                format!("{:0<3}", frac_str)
            };
            frac_millis = padded.parse().unwrap_or(0);
        }
    }

    // Parse optional timezone offset
    let tz_offset_secs: i64 = if pos >= s.len() {
        // No timezone — assume UTC
        0
    } else {
        match s.as_bytes()[pos] {
            b'Z' | b'z' => 0,
            b'+' | b'-' => {
                let sign: i64 = if s.as_bytes()[pos] == b'+' { -1 } else { 1 };
                pos += 1;
                let tz_rest = &s[pos..];
                // Formats: HH:MM, HHMM, HH
                let (tz_h, tz_m) = if tz_rest.len() >= 5 && tz_rest.as_bytes()[2] == b':' {
                    (
                        tz_rest[0..2].parse::<i64>().ok()?,
                        tz_rest[3..5].parse::<i64>().ok()?,
                    )
                } else if tz_rest.len() >= 4 {
                    (
                        tz_rest[0..2].parse::<i64>().ok()?,
                        tz_rest[2..4].parse::<i64>().ok()?,
                    )
                } else if tz_rest.len() >= 2 {
                    (tz_rest[0..2].parse::<i64>().ok()?, 0)
                } else {
                    return None;
                };
                sign * (tz_h * 3600 + tz_m * 60)
            }
            _ => 0, // Unknown suffix, assume UTC
        }
    };

    // Validate ranges
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 60
    {
        return None;
    }

    // Convert to epoch millis using a simplified calculation
    let epoch_days = days_from_civil(year, month, day);
    let epoch_secs = epoch_days * 86400 + hour as i64 * 3600 + minute as i64 * 60 + second as i64;
    let epoch_millis = epoch_secs * 1000 + frac_millis + tz_offset_secs * 1000;

    Some(epoch_millis)
}

/// Convert a civil date to days since Unix epoch (1970-01-01).
///
/// Algorithm from Howard Hinnant's date library.
fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y } as i64;
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as u64;
    let m = m as u64;
    let d = d as u64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe as i64 - 719468
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_duration_single_unit() {
        assert_eq!(parse_duration("5s"), Some(Duration::from_secs(5)));
        assert_eq!(parse_duration("30m"), Some(Duration::from_secs(1800)));
        assert_eq!(parse_duration("2h"), Some(Duration::from_secs(7200)));
        assert_eq!(parse_duration("1d"), Some(Duration::from_secs(86400)));
        assert_eq!(parse_duration("1w"), Some(Duration::from_secs(604800)));
    }

    #[test]
    fn test_parse_duration_compound() {
        assert_eq!(
            parse_duration("1h30m"),
            Some(Duration::from_secs(3600 + 1800))
        );
        assert_eq!(
            parse_duration("2d12h"),
            Some(Duration::from_secs(2 * 86400 + 12 * 3600))
        );
        assert_eq!(
            parse_duration("1h30m45s"),
            Some(Duration::from_secs(3600 + 1800 + 45))
        );
    }

    #[test]
    fn test_parse_duration_invalid() {
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("5"), None); // no unit
        assert_eq!(parse_duration("abc"), None);
        assert_eq!(parse_duration("5x"), None); // unknown unit
    }

    #[test]
    fn test_resolve_relative_time_now() {
        let result = resolve_relative_time("now").unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        // Should be within 100ms of actual now
        assert!((result - now).abs() < 100);
    }

    #[test]
    fn test_resolve_relative_time_offset() {
        let result = resolve_relative_time("now-5m").unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let expected = now - 5 * 60 * 1000;
        assert!((result - expected).abs() < 100);
    }

    #[test]
    fn test_resolve_relative_time_plus() {
        let result = resolve_relative_time("now+1h").unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let expected = now + 3600 * 1000;
        assert!((result - expected).abs() < 100);
    }

    #[test]
    fn test_resolve_not_relative() {
        assert!(resolve_relative_time("2024-01-15T10:00:00Z").is_none());
        assert!(resolve_relative_time("error").is_none());
        assert!(resolve_relative_time("").is_none());
    }

    #[test]
    fn test_parse_timestamp_rfc3339() {
        let ts = parse_timestamp("2024-01-15T10:30:00Z").unwrap();
        // 2024-01-15T10:30:00Z = 1705314600000 ms
        assert_eq!(ts, 1705314600000);
    }

    #[test]
    fn test_parse_timestamp_rfc3339_fractional() {
        let ts = parse_timestamp("2024-01-15T10:30:00.123Z").unwrap();
        assert_eq!(ts, 1705314600123);
    }

    #[test]
    fn test_parse_timestamp_space_separated() {
        let ts = parse_timestamp("2024-01-15 10:30:00").unwrap();
        assert_eq!(ts, 1705314600000);
    }

    #[test]
    fn test_parse_timestamp_with_tz_offset() {
        // +05:30 means the local time is 5h30m ahead of UTC
        // So 10:30:00+05:30 = 05:00:00 UTC
        let ts = parse_timestamp("2024-01-15T10:30:00+05:30").unwrap();
        let utc_ts = parse_timestamp("2024-01-15T05:00:00Z").unwrap();
        assert_eq!(ts, utc_ts);
    }

    #[test]
    fn test_parse_timestamp_negative_tz() {
        // -05:00 means 5h behind UTC
        // 10:30:00-05:00 = 15:30:00 UTC
        let ts = parse_timestamp("2024-01-15T10:30:00-05:00").unwrap();
        let utc_ts = parse_timestamp("2024-01-15T15:30:00Z").unwrap();
        assert_eq!(ts, utc_ts);
    }

    #[test]
    fn test_parse_timestamp_epoch_seconds() {
        let ts = parse_timestamp("1705314600").unwrap();
        assert_eq!(ts, 1705314600000);
    }

    #[test]
    fn test_parse_timestamp_epoch_millis() {
        let ts = parse_timestamp("1705314600123").unwrap();
        assert_eq!(ts, 1705314600123);
    }

    #[test]
    fn test_parse_timestamp_invalid() {
        assert!(parse_timestamp("not a timestamp").is_none());
        assert!(parse_timestamp("").is_none());
        assert!(parse_timestamp("2024").is_none());
    }

    #[test]
    fn test_days_from_civil_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn test_days_from_civil_known_date() {
        // 2024-01-15 is 19737 days after epoch
        assert_eq!(days_from_civil(2024, 1, 15), 19737);
    }
}
