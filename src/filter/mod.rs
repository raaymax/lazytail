pub mod engine;
pub mod regex_filter;
pub mod string_filter;

/// Trait for extensible filtering
pub trait Filter: Send + Sync {
    fn matches(&self, line: &str) -> bool;
}
