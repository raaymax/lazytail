//! Text query parser for LogQL-like query syntax.
//!
//! Parses human-readable query strings like `json | level == "error"` into
//! the `FilterQuery` AST for execution.

use super::ast::*;

/// Parse error with position info for helpful error messages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryParseError {
    pub message: String,
    pub position: usize,
}

impl std::fmt::Display for QueryParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} at position {}", self.message, self.position)
    }
}

/// Parse text query into FilterQuery AST.
///
/// # Examples
/// ```ignore
/// parse_query("json | level == \"error\"")
/// parse_query("json | status >= 400 | service =~ \"api.*\"")
/// parse_query("logfmt | level == error")
/// ```
pub fn parse_query(input: &str) -> Result<FilterQuery, QueryParseError> {
    QueryTextParser::new(input).parse()
}

/// Text parser for LogQL-like query syntax.
struct QueryTextParser<'a> {
    input: &'a str,
    pos: usize,
}

impl<'a> QueryTextParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, pos: 0 }
    }

    fn parse(&mut self) -> Result<FilterQuery, QueryParseError> {
        self.skip_whitespace();

        // Parse parser type (json or logfmt)
        let parser = self.parse_parser()?;

        // Parse filter expressions separated by |
        let mut filters = Vec::new();
        let mut ts_filters = Vec::new();
        let mut aggregate = None;

        self.skip_whitespace();
        while self.pos < self.input.len() {
            // Expect | separator
            if !self.consume_char('|') {
                if self.pos < self.input.len() {
                    return Err(QueryParseError {
                        message: format!(
                            "Expected '|', found '{}'",
                            self.peek_char().unwrap_or('?')
                        ),
                        position: self.pos,
                    });
                }
                break;
            }

            self.skip_whitespace();

            // Check for aggregation clause before filter
            if self.peek_word("count") {
                let fields = self.parse_count_by()?;
                self.skip_whitespace();

                // Optionally consume `| top N`
                let limit = if self.peek_char() == Some('|') {
                    let saved_pos = self.pos;
                    self.consume_char('|');
                    self.skip_whitespace();
                    match self.parse_top_clause() {
                        Some(n) => Some(n),
                        None => {
                            // Not a top clause, rewind
                            self.pos = saved_pos;
                            None
                        }
                    }
                } else {
                    None
                };

                aggregate = Some(Aggregation {
                    agg_type: AggregationType::CountBy,
                    fields,
                    limit,
                });
                break;
            }

            // Parse filter expression
            if self.pos < self.input.len() {
                let filter = self.parse_filter()?;
                if filter.field == "@ts" {
                    ts_filters.push(filter);
                } else {
                    filters.push(filter);
                }
            }

            self.skip_whitespace();
        }

        Ok(FilterQuery {
            parser,
            filters,
            ts_filters,
            exclude: vec![],
            aggregate,
        })
    }

    fn parse_parser(&mut self) -> Result<Parser, QueryParseError> {
        if self.consume_word("json") {
            Ok(Parser::Json)
        } else if self.consume_word("logfmt") {
            Ok(Parser::Logfmt)
        } else {
            Err(QueryParseError {
                message: "Expected 'json' or 'logfmt'".to_string(),
                position: self.pos,
            })
        }
    }

    fn parse_filter(&mut self) -> Result<FieldFilter, QueryParseError> {
        // Parse field name (may contain dots for nested access)
        let field = self.parse_field()?;

        self.skip_whitespace();

        // Parse operator
        let op = self.parse_operator()?;

        self.skip_whitespace();

        // Parse value
        let value = self.parse_value()?;

        Ok(FieldFilter { field, op, value })
    }

    fn parse_field(&mut self) -> Result<String, QueryParseError> {
        let start = self.pos;

        // Allow @ prefix for virtual fields (e.g., @ts)
        if self.pos < self.input.len() && self.input[self.pos..].starts_with('@') {
            self.pos += 1;
        }

        // Field can contain alphanumeric, underscore, and dots (for nested access)
        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            if ch.is_alphanumeric() || ch == '_' || ch == '.' {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }

        if self.pos == start {
            return Err(QueryParseError {
                message: "Expected field name".to_string(),
                position: self.pos,
            });
        }

        Ok(self.input[start..self.pos].to_string())
    }

    fn parse_operator(&mut self) -> Result<Operator, QueryParseError> {
        // Try two-character operators first
        if self.consume_str("==") {
            Ok(Operator::Eq)
        } else if self.consume_str("!=") {
            Ok(Operator::Ne)
        } else if self.consume_str("=~") {
            Ok(Operator::Regex)
        } else if self.consume_str("!~") {
            Ok(Operator::NotRegex)
        } else if self.consume_str(">=") {
            Ok(Operator::Gte)
        } else if self.consume_str("<=") {
            Ok(Operator::Lte)
        } else if self.consume_str(">") {
            Ok(Operator::Gt)
        } else if self.consume_str("<") {
            Ok(Operator::Lt)
        } else {
            Err(QueryParseError {
                message: "Expected operator (==, !=, =~, !~, >, <, >=, <=)".to_string(),
                position: self.pos,
            })
        }
    }

    fn parse_value(&mut self) -> Result<String, QueryParseError> {
        self.skip_whitespace();

        match self.peek_char() {
            Some('"') => self.parse_quoted_string('"'),
            Some('\'') => self.parse_quoted_string('\''),
            _ => self.parse_unquoted_word(),
        }
    }

    fn parse_quoted_string(&mut self, quote_char: char) -> Result<String, QueryParseError> {
        let start_pos = self.pos;

        // Consume opening quote
        if !self.consume_char(quote_char) {
            return Err(QueryParseError {
                message: format!("Expected opening {}", quote_char),
                position: self.pos,
            });
        }

        let mut result = String::new();

        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            self.pos += ch.len_utf8();

            if ch == quote_char {
                return Ok(result);
            } else if ch == '\\' {
                // Handle escape sequences
                if let Some(escaped) = self.input[self.pos..].chars().next() {
                    self.pos += escaped.len_utf8();
                    match escaped {
                        'n' => result.push('\n'),
                        't' => result.push('\t'),
                        'r' => result.push('\r'),
                        '"' => result.push('"'),
                        '\'' => result.push('\''),
                        '\\' => result.push('\\'),
                        _ => {
                            result.push('\\');
                            result.push(escaped);
                        }
                    }
                }
            } else {
                result.push(ch);
            }
        }

        Err(QueryParseError {
            message: "Unterminated string".to_string(),
            position: start_pos,
        })
    }

    fn parse_unquoted_word(&mut self) -> Result<String, QueryParseError> {
        let start = self.pos;

        // Unquoted word: until whitespace or | or end
        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            if ch.is_whitespace() || ch == '|' {
                break;
            }
            self.pos += ch.len_utf8();
        }

        if self.pos == start {
            return Err(QueryParseError {
                message: "Expected value".to_string(),
                position: self.pos,
            });
        }

        Ok(self.input[start..self.pos].to_string())
    }

    fn skip_whitespace(&mut self) {
        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            if ch.is_whitespace() {
                self.pos += ch.len_utf8();
            } else {
                break;
            }
        }
    }

    fn peek_char(&self) -> Option<char> {
        self.input[self.pos..].chars().next()
    }

    fn consume_char(&mut self, expected: char) -> bool {
        if self.peek_char() == Some(expected) {
            self.pos += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn consume_str(&mut self, expected: &str) -> bool {
        if self.input[self.pos..].starts_with(expected) {
            self.pos += expected.len();
            true
        } else {
            false
        }
    }

    fn consume_word(&mut self, word: &str) -> bool {
        if self.input[self.pos..].starts_with(word) {
            // Make sure it's a complete word (followed by whitespace, |, or end)
            let after_pos = self.pos + word.len();
            if after_pos >= self.input.len() {
                self.pos = after_pos;
                return true;
            }
            let next_char = self.input[after_pos..].chars().next().unwrap();
            if next_char.is_whitespace() || next_char == '|' {
                self.pos = after_pos;
                return true;
            }
        }
        false
    }

    /// Peek whether the next word matches without consuming.
    fn peek_word(&self, word: &str) -> bool {
        let rest = &self.input[self.pos..];
        if rest.starts_with(word) {
            let after_pos = word.len();
            if after_pos >= rest.len() {
                return true;
            }
            let next_char = rest[after_pos..].chars().next().unwrap();
            return next_char.is_whitespace() || next_char == '|' || next_char == '(';
        }
        false
    }

    /// Parse `count by (field1, field2, ...)`.
    fn parse_count_by(&mut self) -> Result<Vec<String>, QueryParseError> {
        if !self.consume_word("count") {
            return Err(QueryParseError {
                message: "Expected 'count'".to_string(),
                position: self.pos,
            });
        }
        self.skip_whitespace();

        if !self.consume_word("by") {
            return Err(QueryParseError {
                message: "Expected 'by' after 'count'".to_string(),
                position: self.pos,
            });
        }
        self.skip_whitespace();

        self.parse_field_list()
    }

    /// Parse a parenthesized, comma-separated list of field names.
    fn parse_field_list(&mut self) -> Result<Vec<String>, QueryParseError> {
        if !self.consume_char('(') {
            return Err(QueryParseError {
                message: "Expected '(' after 'by'".to_string(),
                position: self.pos,
            });
        }

        let mut fields = Vec::new();
        loop {
            self.skip_whitespace();
            let field = self.parse_field()?;
            fields.push(field);
            self.skip_whitespace();

            if self.consume_char(')') {
                break;
            }
            if !self.consume_char(',') {
                return Err(QueryParseError {
                    message: "Expected ',' or ')' in field list".to_string(),
                    position: self.pos,
                });
            }
        }

        if fields.is_empty() {
            return Err(QueryParseError {
                message: "Expected at least one field in 'count by'".to_string(),
                position: self.pos,
            });
        }

        Ok(fields)
    }

    /// Try to parse `top N`, returning Some(N) on success.
    fn parse_top_clause(&mut self) -> Option<usize> {
        if !self.peek_word("top") {
            return None;
        }
        self.consume_word("top");
        self.skip_whitespace();

        let start = self.pos;
        while self.pos < self.input.len() {
            let ch = self.input[self.pos..].chars().next().unwrap();
            if ch.is_ascii_digit() {
                self.pos += 1;
            } else {
                break;
            }
        }

        if self.pos == start {
            return None;
        }

        self.input[start..self.pos].parse::<usize>().ok()
    }
}
