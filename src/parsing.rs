use std::collections::HashMap;

/// Parse a logfmt line into key-value pairs.
pub fn parse_logfmt(line: &str) -> HashMap<String, String> {
    let mut result = HashMap::new();
    let mut chars = line.char_indices().peekable();

    while let Some((_, ch)) = chars.peek().copied() {
        // Skip whitespace
        if ch.is_whitespace() {
            chars.next();
            continue;
        }

        // Parse key
        let key_start = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
        while let Some(&(_, ch)) = chars.peek() {
            if ch == '=' || ch.is_whitespace() {
                break;
            }
            chars.next();
        }
        let key_end = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
        let key = &line[key_start..key_end];

        if key.is_empty() {
            break;
        }

        // Expect =
        if chars.peek().map(|(_, ch)| *ch) != Some('=') {
            // No value, skip this key
            continue;
        }
        chars.next(); // consume '='

        // Parse value
        let value = if chars.peek().map(|(_, ch)| *ch) == Some('"') {
            // Quoted value
            chars.next(); // consume opening quote
            let value_start = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
            let mut value_end = value_start;
            let mut escaped = false;

            for (i, ch) in chars.by_ref() {
                if escaped {
                    escaped = false;
                    value_end = i + ch.len_utf8();
                } else if ch == '\\' {
                    escaped = true;
                    value_end = i + ch.len_utf8();
                } else if ch == '"' {
                    break;
                } else {
                    value_end = i + ch.len_utf8();
                }
            }

            // Handle escape sequences in the value
            let raw_value = &line[value_start..value_end];
            raw_value
                .replace("\\\"", "\"")
                .replace("\\\\", "\\")
                .replace("\\n", "\n")
                .replace("\\t", "\t")
        } else {
            // Unquoted value
            let value_start = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
            while let Some(&(_, ch)) = chars.peek() {
                if ch.is_whitespace() {
                    break;
                }
                chars.next();
            }
            let value_end = chars.peek().map(|(i, _)| *i).unwrap_or(line.len());
            line[value_start..value_end].to_string()
        };

        result.insert(key.to_string(), value);
    }

    result
}

/// Extract a field from a JSON value, supporting dot-notation for nested fields
/// and numeric indices for arrays.
pub fn extract_json_field(json: &serde_json::Value, field: &str) -> Option<String> {
    let mut current = json;

    for part in field.split('.') {
        if current.is_array() {
            if let Ok(index) = part.parse::<usize>() {
                current = current.get(index)?;
                continue;
            }
        }
        current = current.get(part)?;
    }

    Some(match current {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        _ => current.to_string(),
    })
}
