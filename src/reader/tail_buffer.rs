use std::collections::VecDeque;

/// Default tail buffer capacity (number of lines to keep in RAM)
const DEFAULT_CAPACITY: usize = 10_000;

/// A circular buffer that keeps the most recent N lines in memory
///
/// Used for follow mode to provide instant access to recent lines
/// without disk I/O. When the buffer is full, old lines are evicted
/// and can be handed off for indexing.
#[derive(Debug)]
pub struct TailBuffer {
    /// Lines stored in the buffer (newest at back)
    lines: VecDeque<String>,

    /// Line number of the first line in the buffer (0-indexed)
    /// This tracks the "global" line number in the file
    start_line: usize,

    /// Maximum number of lines to keep
    capacity: usize,
}

/// Information about an evicted line, for index updates
#[derive(Debug, Clone)]
pub struct EvictedLine {
    /// The global line number of the evicted line
    pub line_number: usize,
    /// The content of the evicted line
    pub content: String,
    /// Byte length of the line (including newline)
    pub byte_len: usize,
}

impl TailBuffer {
    /// Create a new tail buffer with the given capacity
    pub fn new(capacity: usize) -> Self {
        Self {
            lines: VecDeque::with_capacity(capacity.min(1000)), // Start smaller, grow as needed
            start_line: 0,
            capacity: capacity.max(1), // Minimum capacity of 1
        }
    }

    /// Create a tail buffer with default capacity (10,000 lines)
    pub fn default_capacity() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }

    /// Push a new line to the buffer
    ///
    /// Returns `Some(EvictedLine)` if a line was evicted to make room,
    /// or `None` if no eviction was needed.
    pub fn push(&mut self, line: String) -> Option<EvictedLine> {
        let byte_len = line.len() + 1; // +1 for newline
        self.lines.push_back(line);

        // Check if we need to evict
        if self.lines.len() > self.capacity {
            let evicted_content = self.lines.pop_front()?;
            let evicted_line_num = self.start_line;
            self.start_line += 1;

            return Some(EvictedLine {
                line_number: evicted_line_num,
                content: evicted_content,
                byte_len,
            });
        }

        None
    }

    /// Get a line by its global line number
    ///
    /// Returns `None` if the line is not in the buffer (either evicted or not yet added)
    pub fn get(&self, line_num: usize) -> Option<&str> {
        if line_num < self.start_line {
            return None; // Line was evicted
        }

        let idx = line_num - self.start_line;
        self.lines.get(idx).map(|s| s.as_str())
    }

    /// Check if a line number is in the buffer
    pub fn contains(&self, line_num: usize) -> bool {
        line_num >= self.start_line && line_num < self.start_line + self.lines.len()
    }

    /// Get the range of line numbers in the buffer (inclusive start, exclusive end)
    pub fn line_range(&self) -> (usize, usize) {
        (self.start_line, self.start_line + self.lines.len())
    }

    /// Get the number of lines currently in the buffer
    pub fn len(&self) -> usize {
        self.lines.len()
    }

    /// Check if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    /// Get the line number of the first line in the buffer
    pub fn start_line(&self) -> usize {
        self.start_line
    }

    /// Get the line number of the last line in the buffer (if any)
    pub fn end_line(&self) -> Option<usize> {
        if self.lines.is_empty() {
            None
        } else {
            Some(self.start_line + self.lines.len() - 1)
        }
    }

    /// Clear the buffer and reset to a new starting line
    pub fn clear(&mut self) {
        self.lines.clear();
        self.start_line = 0;
    }

    /// Clear and set a new starting line number (for appending after existing content)
    pub fn reset_from(&mut self, start_line: usize) {
        self.lines.clear();
        self.start_line = start_line;
    }

    /// Get the buffer's capacity
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Iterate over all lines with their global line numbers
    pub fn iter(&self) -> impl Iterator<Item = (usize, &str)> {
        let start = self.start_line;
        self.lines
            .iter()
            .enumerate()
            .map(move |(idx, line)| (start + idx, line.as_str()))
    }

    /// Get approximate memory usage in bytes
    pub fn memory_usage(&self) -> usize {
        let struct_size = std::mem::size_of::<Self>();
        let content_size: usize = self.lines.iter().map(|s| s.capacity()).sum();
        let vec_overhead = self.lines.capacity() * std::mem::size_of::<String>();
        struct_size + content_size + vec_overhead
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_buffer() {
        let buffer = TailBuffer::new(100);
        assert_eq!(buffer.capacity(), 100);
        assert_eq!(buffer.len(), 0);
        assert!(buffer.is_empty());
    }

    #[test]
    fn test_default_capacity() {
        let buffer = TailBuffer::default_capacity();
        assert_eq!(buffer.capacity(), 10_000);
    }

    #[test]
    fn test_minimum_capacity() {
        let buffer = TailBuffer::new(0);
        assert_eq!(buffer.capacity(), 1);
    }

    #[test]
    fn test_push_without_eviction() {
        let mut buffer = TailBuffer::new(5);

        assert!(buffer.push("line 0".to_string()).is_none());
        assert!(buffer.push("line 1".to_string()).is_none());
        assert!(buffer.push("line 2".to_string()).is_none());

        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.get(0), Some("line 0"));
        assert_eq!(buffer.get(1), Some("line 1"));
        assert_eq!(buffer.get(2), Some("line 2"));
    }

    #[test]
    fn test_push_with_eviction() {
        let mut buffer = TailBuffer::new(3);

        buffer.push("line 0".to_string());
        buffer.push("line 1".to_string());
        buffer.push("line 2".to_string());
        assert_eq!(buffer.len(), 3);

        // This push should evict line 0
        let evicted = buffer.push("line 3".to_string());
        assert!(evicted.is_some());
        let evicted = evicted.unwrap();
        assert_eq!(evicted.line_number, 0);
        assert_eq!(evicted.content, "line 0");

        assert_eq!(buffer.len(), 3);
        assert_eq!(buffer.get(0), None); // Evicted
        assert_eq!(buffer.get(1), Some("line 1"));
        assert_eq!(buffer.get(2), Some("line 2"));
        assert_eq!(buffer.get(3), Some("line 3"));
    }

    #[test]
    fn test_continuous_eviction() {
        let mut buffer = TailBuffer::new(2);

        buffer.push("line 0".to_string());
        buffer.push("line 1".to_string());

        // Push and evict multiple times
        for i in 2..10 {
            let evicted = buffer.push(format!("line {}", i));
            assert!(evicted.is_some());
            let evicted = evicted.unwrap();
            assert_eq!(evicted.line_number, i - 2);
        }

        // Buffer should contain lines 8 and 9
        assert_eq!(buffer.len(), 2);
        assert_eq!(buffer.get(8), Some("line 8"));
        assert_eq!(buffer.get(9), Some("line 9"));
        assert_eq!(buffer.start_line(), 8);
    }

    #[test]
    fn test_contains() {
        let mut buffer = TailBuffer::new(3);

        buffer.push("line 0".to_string());
        buffer.push("line 1".to_string());
        buffer.push("line 2".to_string());
        buffer.push("line 3".to_string()); // Evicts line 0

        assert!(!buffer.contains(0)); // Evicted
        assert!(buffer.contains(1));
        assert!(buffer.contains(2));
        assert!(buffer.contains(3));
        assert!(!buffer.contains(4)); // Not yet added
    }

    #[test]
    fn test_line_range() {
        let mut buffer = TailBuffer::new(5);

        assert_eq!(buffer.line_range(), (0, 0));

        buffer.push("line 0".to_string());
        buffer.push("line 1".to_string());
        assert_eq!(buffer.line_range(), (0, 2));

        // Fill and evict
        buffer.push("line 2".to_string());
        buffer.push("line 3".to_string());
        buffer.push("line 4".to_string());
        buffer.push("line 5".to_string()); // Evicts 0

        assert_eq!(buffer.line_range(), (1, 6));
    }

    #[test]
    fn test_end_line() {
        let mut buffer = TailBuffer::new(5);

        assert_eq!(buffer.end_line(), None);

        buffer.push("line 0".to_string());
        assert_eq!(buffer.end_line(), Some(0));

        buffer.push("line 1".to_string());
        assert_eq!(buffer.end_line(), Some(1));
    }

    #[test]
    fn test_clear() {
        let mut buffer = TailBuffer::new(5);

        buffer.push("line 0".to_string());
        buffer.push("line 1".to_string());

        buffer.clear();

        assert!(buffer.is_empty());
        assert_eq!(buffer.start_line(), 0);
        assert_eq!(buffer.line_range(), (0, 0));
    }

    #[test]
    fn test_reset_from() {
        let mut buffer = TailBuffer::new(5);

        buffer.push("line 0".to_string());
        buffer.push("line 1".to_string());

        buffer.reset_from(100);

        assert!(buffer.is_empty());
        assert_eq!(buffer.start_line(), 100);

        buffer.push("line 100".to_string());
        assert_eq!(buffer.get(100), Some("line 100"));
        assert_eq!(buffer.line_range(), (100, 101));
    }

    #[test]
    fn test_iter() {
        let mut buffer = TailBuffer::new(5);

        buffer.push("a".to_string());
        buffer.push("b".to_string());
        buffer.push("c".to_string());

        let items: Vec<_> = buffer.iter().collect();
        assert_eq!(items, vec![(0, "a"), (1, "b"), (2, "c")]);
    }

    #[test]
    fn test_iter_after_eviction() {
        let mut buffer = TailBuffer::new(2);

        buffer.push("a".to_string());
        buffer.push("b".to_string());
        buffer.push("c".to_string()); // Evicts a

        let items: Vec<_> = buffer.iter().collect();
        assert_eq!(items, vec![(1, "b"), (2, "c")]);
    }

    #[test]
    fn test_memory_usage() {
        let mut buffer = TailBuffer::new(100);
        let empty_usage = buffer.memory_usage();

        for i in 0..50 {
            buffer.push(format!("line {} with some content", i));
        }

        let with_content = buffer.memory_usage();
        assert!(
            with_content > empty_usage,
            "Memory should increase with content"
        );
    }

    #[test]
    fn test_evicted_line_byte_len() {
        let mut buffer = TailBuffer::new(1);

        buffer.push("hello".to_string());
        let evicted = buffer.push("world".to_string());

        let evicted = evicted.unwrap();
        assert_eq!(evicted.byte_len, 6); // "hello" + newline
    }

    #[test]
    fn test_get_out_of_range() {
        let mut buffer = TailBuffer::new(5);

        buffer.push("line 0".to_string());
        buffer.push("line 1".to_string());

        assert_eq!(buffer.get(0), Some("line 0"));
        assert_eq!(buffer.get(1), Some("line 1"));
        assert_eq!(buffer.get(2), None); // Not added yet
        assert_eq!(buffer.get(100), None); // Way out of range
    }
}
