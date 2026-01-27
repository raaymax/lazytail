use lru::LruCache;
use ratatui::text::Text;
use std::hash::{Hash, Hasher};
use std::num::NonZeroUsize;

/// Default cache capacity (number of parsed lines)
const DEFAULT_CAPACITY: usize = 1_000;

/// LRU cache for parsed ANSI text to avoid redundant parsing
///
/// ANSI parsing is expensive and the same line may be rendered multiple
/// times (every frame). This cache stores parsed `Text` objects keyed by
/// a hash of the raw line content.
pub struct AnsiCache {
    /// LRU cache mapping content hash to parsed Text
    cache: LruCache<u64, Text<'static>>,
}

impl AnsiCache {
    /// Create a new ANSI cache with the specified capacity
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1);
        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
        }
    }

    /// Create an ANSI cache with default capacity (1,000 lines)
    pub fn default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    /// Get parsed text from cache, or parse and cache it
    ///
    /// Uses content hash as the key to handle dynamic line numbers.
    pub fn get_or_parse(&mut self, raw: &str) -> Text<'static> {
        let hash = hash_string(raw);

        // Check cache first
        if let Some(parsed) = self.cache.get(&hash) {
            return parsed.clone();
        }

        // Parse ANSI codes
        let parsed = parse_ansi(raw);

        // Cache and return
        self.cache.put(hash, parsed.clone());
        parsed
    }

    /// Get parsed text from cache without parsing
    pub fn get(&mut self, raw: &str) -> Option<Text<'static>> {
        let hash = hash_string(raw);
        self.cache.get(&hash).cloned()
    }

    /// Check if parsed text for content is in cache
    pub fn contains(&self, raw: &str) -> bool {
        let hash = hash_string(raw);
        self.cache.contains(&hash)
    }

    /// Clear the cache
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Get the number of cached entries
    pub fn len(&self) -> usize {
        self.cache.len()
    }

    /// Check if the cache is empty
    pub fn is_empty(&self) -> bool {
        self.cache.is_empty()
    }

    /// Get the cache capacity
    pub fn capacity(&self) -> usize {
        self.cache.cap().get()
    }
}

/// Hash a string for cache key
fn hash_string(s: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// Parse ANSI codes into ratatui Text
fn parse_ansi(raw: &str) -> Text<'static> {
    use ansi_to_tui::IntoText;

    raw.into_text()
        .unwrap_or_else(|_| Text::raw(raw.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_cache() {
        let cache = AnsiCache::new(100);
        assert_eq!(cache.capacity(), 100);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_default_capacity() {
        let cache = AnsiCache::default_capacity();
        assert_eq!(cache.capacity(), 1_000);
    }

    #[test]
    fn test_get_or_parse_plain_text() {
        let mut cache = AnsiCache::new(10);

        let text = cache.get_or_parse("Hello, World!");

        // Should produce a Text with one line
        assert_eq!(text.lines.len(), 1);
    }

    #[test]
    fn test_get_or_parse_ansi_text() {
        let mut cache = AnsiCache::new(10);

        let text = cache.get_or_parse("\x1b[31mRed text\x1b[0m");

        // Should produce styled text
        assert_eq!(text.lines.len(), 1);
    }

    #[test]
    fn test_cache_hit() {
        let mut cache = AnsiCache::new(10);

        let raw = "\x1b[32mGreen\x1b[0m";

        // First parse
        let text1 = cache.get_or_parse(raw);
        assert!(cache.contains(raw));

        // Second access should be cached
        let text2 = cache.get_or_parse(raw);

        // Should get same content
        assert_eq!(text1.lines.len(), text2.lines.len());
    }

    #[test]
    fn test_get_without_parse() {
        let mut cache = AnsiCache::new(10);

        let raw = "test line";

        assert!(cache.get(raw).is_none());

        cache.get_or_parse(raw);

        assert!(cache.get(raw).is_some());
    }

    #[test]
    fn test_contains() {
        let mut cache = AnsiCache::new(10);

        let raw = "test line";

        assert!(!cache.contains(raw));
        cache.get_or_parse(raw);
        assert!(cache.contains(raw));
    }

    #[test]
    fn test_clear() {
        let mut cache = AnsiCache::new(10);

        cache.get_or_parse("line 1");
        cache.get_or_parse("line 2");

        assert_eq!(cache.len(), 2);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = AnsiCache::new(2);

        cache.get_or_parse("line 1");
        cache.get_or_parse("line 2");
        assert_eq!(cache.len(), 2);

        // Access line 1 to make it recently used
        cache.get_or_parse("line 1");

        // Add line 3, should evict line 2
        cache.get_or_parse("line 3");

        assert_eq!(cache.len(), 2);
        assert!(cache.contains("line 1"));
        assert!(!cache.contains("line 2"));
        assert!(cache.contains("line 3"));
    }

    #[test]
    fn test_unicode_content() {
        let mut cache = AnsiCache::new(10);

        let text = cache.get_or_parse("Hello 世界 🌍");

        assert_eq!(text.lines.len(), 1);
    }

    #[test]
    fn test_multiline_content() {
        let mut cache = AnsiCache::new(10);

        // Note: ANSI parsing handles embedded newlines
        let text = cache.get_or_parse("line1\nline2");

        // Depending on ansi_to_tui behavior, may produce multiple lines
        assert!(!text.lines.is_empty());
    }

    #[test]
    fn test_complex_ansi() {
        let mut cache = AnsiCache::new(10);

        // Complex ANSI with 256-color and multiple attributes
        let raw = "\x1b[38;5;214m\x1b[1mBold Orange\x1b[0m \x1b[4mUnderline\x1b[0m";
        let text = cache.get_or_parse(raw);

        assert_eq!(text.lines.len(), 1);
    }

    #[test]
    fn test_hash_stability() {
        // Same content should always hash to same value
        let hash1 = hash_string("test content");
        let hash2 = hash_string("test content");
        assert_eq!(hash1, hash2);

        // Different content should hash differently
        let hash3 = hash_string("different content");
        assert_ne!(hash1, hash3);
    }
}
