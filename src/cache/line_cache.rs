use lru::LruCache;
use std::num::NonZeroUsize;

/// Default cache capacity (number of lines)
const DEFAULT_CAPACITY: usize = 10_000;

/// LRU cache for line content to avoid redundant file reads
///
/// Caches recently accessed lines to eliminate disk I/O for frequently
/// viewed content. Particularly useful for:
/// - Scrolling back and forth in a log file
/// - Re-rendering the same viewport
/// - Accessing lines during filtering
pub struct LineCache {
    /// LRU cache mapping line number to content
    cache: LruCache<usize, String>,
}

impl LineCache {
    /// Create a new line cache with the specified capacity
    pub fn new(capacity: usize) -> Self {
        let capacity = capacity.max(1); // Minimum capacity of 1
        Self {
            cache: LruCache::new(NonZeroUsize::new(capacity).unwrap()),
        }
    }

    /// Create a line cache with default capacity (10,000 lines)
    pub fn default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    /// Get a line from cache, or load it using the provided closure
    ///
    /// If the line is in cache, returns a reference to it.
    /// Otherwise, calls the loader function and caches the result.
    pub fn get_or_load<F>(&mut self, line_num: usize, loader: F) -> Option<&str>
    where
        F: FnOnce() -> Option<String>,
    {
        // Check if already in cache
        if self.cache.contains(&line_num) {
            return self.cache.get(&line_num).map(|s| s.as_str());
        }

        // Load and cache
        if let Some(line) = loader() {
            self.cache.put(line_num, line);
            return self.cache.get(&line_num).map(|s| s.as_str());
        }

        None
    }

    /// Check if a line is in the cache
    pub fn contains(&self, line_num: usize) -> bool {
        self.cache.contains(&line_num)
    }

    /// Get a line from cache without loading
    pub fn get(&mut self, line_num: usize) -> Option<&str> {
        self.cache.get(&line_num).map(|s| s.as_str())
    }

    /// Peek at a line without updating LRU order
    pub fn peek(&self, line_num: usize) -> Option<&str> {
        self.cache.peek(&line_num).map(|s| s.as_str())
    }

    /// Put a line directly into the cache
    pub fn put(&mut self, line_num: usize, content: String) {
        self.cache.put(line_num, content);
    }

    /// Invalidate a specific line
    pub fn invalidate(&mut self, line_num: usize) {
        self.cache.pop(&line_num);
    }

    /// Invalidate all lines from a given line number onwards
    ///
    /// Useful when the file is modified (lines after the modification point
    /// may have changed).
    pub fn invalidate_from(&mut self, from_line: usize) {
        // Collect keys to remove (can't modify during iteration)
        let keys_to_remove: Vec<usize> = self
            .cache
            .iter()
            .filter(|(k, _)| **k >= from_line)
            .map(|(k, _)| *k)
            .collect();

        for key in keys_to_remove {
            self.cache.pop(&key);
        }
    }

    /// Clear all cached lines
    pub fn clear(&mut self) {
        self.cache.clear();
    }

    /// Get the number of cached lines
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

    /// Get cache statistics
    pub fn stats(&self) -> CacheStats {
        CacheStats {
            entries: self.cache.len(),
            capacity: self.cache.cap().get(),
        }
    }
}

/// Cache statistics
#[derive(Debug, Clone)]
pub struct CacheStats {
    /// Number of entries currently in cache
    pub entries: usize,
    /// Maximum cache capacity
    pub capacity: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_cache() {
        let cache = LineCache::new(100);
        assert_eq!(cache.capacity(), 100);
        assert!(cache.is_empty());
    }

    #[test]
    fn test_default_capacity() {
        let cache = LineCache::default_capacity();
        assert_eq!(cache.capacity(), 10_000);
    }

    #[test]
    fn test_minimum_capacity() {
        let cache = LineCache::new(0);
        assert_eq!(cache.capacity(), 1);
    }

    #[test]
    fn test_put_and_get() {
        let mut cache = LineCache::new(10);

        cache.put(5, "line five".to_string());
        cache.put(10, "line ten".to_string());

        assert_eq!(cache.get(5), Some("line five"));
        assert_eq!(cache.get(10), Some("line ten"));
        assert_eq!(cache.get(15), None);
    }

    #[test]
    fn test_get_or_load_cache_hit() {
        let mut cache = LineCache::new(10);
        cache.put(5, "cached".to_string());

        let mut called = false;
        let result = cache.get_or_load(5, || {
            called = true;
            Some("loaded".to_string())
        });

        assert_eq!(result, Some("cached"));
        assert!(!called, "Loader should not be called on cache hit");
    }

    #[test]
    fn test_get_or_load_cache_miss() {
        let mut cache = LineCache::new(10);

        let mut called = false;
        let result = cache.get_or_load(5, || {
            called = true;
            Some("loaded".to_string())
        });

        assert_eq!(result, Some("loaded"));
        assert!(called, "Loader should be called on cache miss");

        // Now it should be cached
        let mut called_again = false;
        let result = cache.get_or_load(5, || {
            called_again = true;
            Some("loaded again".to_string())
        });

        assert_eq!(result, Some("loaded"));
        assert!(
            !called_again,
            "Loader should not be called on second access"
        );
    }

    #[test]
    fn test_get_or_load_returns_none() {
        let mut cache = LineCache::new(10);

        let result = cache.get_or_load(5, || None);

        assert!(result.is_none());
        assert!(!cache.contains(5));
    }

    #[test]
    fn test_contains() {
        let mut cache = LineCache::new(10);

        assert!(!cache.contains(5));
        cache.put(5, "line".to_string());
        assert!(cache.contains(5));
    }

    #[test]
    fn test_peek() {
        let mut cache = LineCache::new(10);
        cache.put(5, "line".to_string());

        // Peek doesn't update LRU order
        assert_eq!(cache.peek(5), Some("line"));
        assert_eq!(cache.peek(10), None);
    }

    #[test]
    fn test_invalidate() {
        let mut cache = LineCache::new(10);
        cache.put(5, "line".to_string());

        assert!(cache.contains(5));
        cache.invalidate(5);
        assert!(!cache.contains(5));
    }

    #[test]
    fn test_invalidate_from() {
        let mut cache = LineCache::new(10);

        for i in 0..10 {
            cache.put(i, format!("line {}", i));
        }

        assert_eq!(cache.len(), 10);

        cache.invalidate_from(5);

        // Lines 0-4 should remain
        assert_eq!(cache.len(), 5);
        for i in 0..5 {
            assert!(cache.contains(i));
        }
        for i in 5..10 {
            assert!(!cache.contains(i));
        }
    }

    #[test]
    fn test_clear() {
        let mut cache = LineCache::new(10);

        for i in 0..5 {
            cache.put(i, format!("line {}", i));
        }

        assert_eq!(cache.len(), 5);
        cache.clear();
        assert!(cache.is_empty());
    }

    #[test]
    fn test_lru_eviction() {
        let mut cache = LineCache::new(3);

        cache.put(1, "one".to_string());
        cache.put(2, "two".to_string());
        cache.put(3, "three".to_string());

        assert_eq!(cache.len(), 3);

        // Access line 1 to make it recently used
        cache.get(1);

        // Add line 4, should evict line 2 (least recently used)
        cache.put(4, "four".to_string());

        assert_eq!(cache.len(), 3);
        assert!(cache.contains(1)); // Recently accessed
        assert!(!cache.contains(2)); // Evicted
        assert!(cache.contains(3));
        assert!(cache.contains(4)); // Newly added
    }

    #[test]
    fn test_stats() {
        let mut cache = LineCache::new(100);

        cache.put(1, "one".to_string());
        cache.put(2, "two".to_string());

        let stats = cache.stats();
        assert_eq!(stats.entries, 2);
        assert_eq!(stats.capacity, 100);
    }
}
