use crate::filter::query;
use crate::filter::{FilterHistoryEntry, FilterMode};
use crate::history;
use std::time::{Duration, Instant};

/// Debounce delay for live filter preview (milliseconds)
const FILTER_DEBOUNCE_MS: u64 = 500;

/// Maximum number of filter history entries to keep
const MAX_HISTORY_ENTRIES: usize = 50;

/// Manages filter validation, debouncing, and history navigation.
#[derive(Debug)]
pub struct FilterController {
    /// Current filter mode for input (Plain, Regex, or Query, with case sensitivity)
    pub current_mode: FilterMode,

    /// Regex validation error (None = valid or plain mode)
    pub regex_error: Option<String>,

    /// Query syntax validation error (None = valid or not query syntax)
    pub query_error: Option<String>,

    /// Time when pending filter should be triggered (for debouncing)
    pub pending_at: Option<Instant>,

    /// Filter history (up to MAX_HISTORY_ENTRIES)
    history: Vec<FilterHistoryEntry>,

    /// Current position in filter history (None = not navigating)
    history_index: Option<usize>,
}

impl FilterController {
    pub fn new() -> Self {
        Self {
            current_mode: FilterMode::default(),
            regex_error: None,
            query_error: None,
            pending_at: None,
            history: history::load_history(),
            history_index: None,
        }
    }

    /// Validate the current input as a regex (if in regex mode)
    pub fn validate_regex(&mut self, buffer: &str) {
        self.validate_query(buffer);

        if !self.current_mode.is_regex() || buffer.is_empty() {
            self.regex_error = None;
            return;
        }

        match regex::Regex::new(buffer) {
            Ok(_) => self.regex_error = None,
            Err(e) => self.regex_error = Some(e.to_string()),
        }
    }

    /// Validate the current input as a query (if in query mode)
    pub fn validate_query(&mut self, buffer: &str) {
        if !self.current_mode.is_query() || buffer.is_empty() {
            self.query_error = None;
            return;
        }

        match query::parse_query(buffer) {
            Ok(filter_query) => match query::QueryFilter::new(filter_query) {
                Ok(_) => self.query_error = None,
                Err(e) => self.query_error = Some(e),
            },
            Err(e) => self.query_error = Some(e.message),
        }
    }

    /// Check if the current filter input is valid (regex and query)
    pub fn is_valid(&self) -> bool {
        self.regex_error.is_none() && self.query_error.is_none()
    }

    /// Schedule a debounced filter trigger
    pub fn schedule_debounce(&mut self) {
        self.pending_at = Some(Instant::now() + Duration::from_millis(FILTER_DEBOUNCE_MS));
    }

    /// Add filter pattern to history (called on filter submit)
    pub fn add_to_history(&mut self, pattern: String, mode: FilterMode) {
        if pattern.is_empty() {
            return;
        }

        let entry = FilterHistoryEntry::new(pattern, mode);

        // Don't add if it's the same as the last entry (same pattern AND mode)
        if let Some(last) = self.history.last() {
            if last.matches(&entry) {
                return;
            }
        }

        self.history.push(entry);

        if self.history.len() > MAX_HISTORY_ENTRIES {
            self.history.remove(0);
        }

        self.history_index = None;
        history::save_history(&self.history);
    }

    /// Navigate up in filter history (older entries).
    /// Returns Some((pattern, mode)) if a history entry was selected.
    pub fn history_up(&mut self) -> Option<(String, FilterMode)> {
        if self.history.is_empty() {
            return None;
        }

        let new_index = match self.history_index {
            None => Some(self.history.len() - 1),
            Some(idx) => {
                if idx > 0 {
                    Some(idx - 1)
                } else {
                    Some(idx)
                }
            }
        };

        self.history_index = new_index;
        new_index.map(|idx| {
            let entry = &self.history[idx];
            self.current_mode = entry.mode;
            (entry.pattern.clone(), entry.mode)
        })
    }

    /// Navigate down in filter history (newer entries).
    /// Returns Some((pattern, mode)) if a history entry was selected,
    /// or None if returning to empty input.
    pub fn history_down(&mut self) -> Option<(String, FilterMode)> {
        if self.history.is_empty() {
            return None;
        }

        let new_index = match self.history_index {
            None => return None,
            Some(idx) => {
                if idx < self.history.len() - 1 {
                    Some(idx + 1)
                } else {
                    None
                }
            }
        };

        self.history_index = new_index;
        new_index.map(|idx| {
            let entry = &self.history[idx];
            self.current_mode = entry.mode;
            (entry.pattern.clone(), entry.mode)
        })
    }

    /// Reset history navigation index
    pub fn reset_history_index(&mut self) {
        self.history_index = None;
    }
}
