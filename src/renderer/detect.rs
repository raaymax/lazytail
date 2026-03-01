use super::preset::{CompiledPreset, PresetParser};

/// Compiled detection rules for auto-matching presets to sources.
pub struct CompiledDetect {
    pub filename_pattern: Option<String>,
    pub parser: Option<PresetParser>,
}

/// Simple glob matching — `*` matches any chars, `?` matches single char.
pub fn matches_filename(pattern: &str, filename: &str) -> bool {
    let mut p_chars = pattern.chars().peekable();
    let mut f_chars = filename.chars().peekable();

    while p_chars.peek().is_some() || f_chars.peek().is_some() {
        match p_chars.peek() {
            Some('*') => {
                p_chars.next();
                // Match rest of pattern against all suffixes of filename
                if p_chars.peek().is_none() {
                    return true; // trailing * matches everything
                }
                let remaining_pattern: String = p_chars.collect();
                let remaining_filename: String = f_chars.collect();
                for i in 0..=remaining_filename.len() {
                    if matches_filename(&remaining_pattern, &remaining_filename[i..]) {
                        return true;
                    }
                }
                return false;
            }
            Some('?') => {
                p_chars.next();
                if f_chars.next().is_none() {
                    return false;
                }
            }
            Some(&pc) => {
                if f_chars.next() != Some(pc) {
                    return false;
                }
                p_chars.next();
            }
            None => return false, // pattern exhausted but filename has more
        }
    }

    true
}

/// Returns presets whose detect rule matches the given filename and/or flags.
pub fn detect_presets<'a>(
    presets: &'a [CompiledPreset],
    filename: Option<&str>,
    flags: Option<u32>,
) -> Vec<&'a CompiledPreset> {
    use crate::index::flags::{FLAG_FORMAT_JSON, FLAG_FORMAT_LOGFMT};

    let mut results = Vec::new();

    // First pass: filename-based matching (higher priority)
    if let Some(fname) = filename {
        for preset in presets {
            if let Some(ref detect) = preset.detect {
                if let Some(ref pattern) = detect.filename_pattern {
                    if matches_filename(pattern, fname) {
                        results.push(preset);
                    }
                }
            }
        }
    }

    if !results.is_empty() {
        return results;
    }

    // Second pass: parser-based detection from index flags
    if let Some(f) = flags {
        for preset in presets {
            if let Some(ref detect) = preset.detect {
                if let Some(ref parser) = detect.parser {
                    let matches = match parser {
                        PresetParser::Json => f & FLAG_FORMAT_JSON != 0,
                        PresetParser::Logfmt => f & FLAG_FORMAT_LOGFMT != 0,
                        _ => false,
                    };
                    if matches {
                        results.push(preset);
                    }
                }
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::builtin::builtin_presets;

    #[test]
    fn test_matches_filename_glob_star() {
        assert!(matches_filename("access*.log", "access_2024.log"));
    }

    #[test]
    fn test_matches_filename_exact() {
        assert!(matches_filename("app.log", "app.log"));
    }

    #[test]
    fn test_matches_filename_no_match() {
        assert!(!matches_filename("access*.log", "error.log"));
    }

    #[test]
    fn test_detect_json_by_flags() {
        use crate::index::flags::FLAG_FORMAT_JSON;
        let presets = builtin_presets();
        let detected = detect_presets(&presets, None, Some(FLAG_FORMAT_JSON));
        assert!(!detected.is_empty());
        assert_eq!(detected[0].name, "json");
    }

    #[test]
    fn test_detect_logfmt_by_flags() {
        use crate::index::flags::FLAG_FORMAT_LOGFMT;
        let presets = builtin_presets();
        let detected = detect_presets(&presets, None, Some(FLAG_FORMAT_LOGFMT));
        assert!(!detected.is_empty());
        assert_eq!(detected[0].name, "logfmt");
    }

    #[test]
    fn test_detect_filename_priority() {
        use crate::index::flags::FLAG_FORMAT_JSON;
        use crate::renderer::preset::{compile, RawDetect, RawLayoutEntry, RawPreset};

        let custom = compile(RawPreset {
            name: "custom".to_string(),
            detect: Some(RawDetect {
                parser: None,
                filename: Some("app*.log".to_string()),
            }),
            regex: None,
            layout: vec![RawLayoutEntry {
                field: Some("level".to_string()),
                literal: None,
                style: None,
                width: None,
                format: None,
                style_map: None,
                max_width: None,
            }],
        })
        .unwrap();

        let mut presets = builtin_presets();
        presets.insert(0, custom);

        // Filename match should take priority over flag-based detection
        let detected = detect_presets(&presets, Some("app_prod.log"), Some(FLAG_FORMAT_JSON));
        assert_eq!(detected.len(), 1);
        assert_eq!(detected[0].name, "custom");
    }
}
