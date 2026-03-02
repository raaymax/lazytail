use crate::reader::LogReader;
use anyhow::Result;

/// Mock LogReader for testing — holds lines in memory.
pub struct MockLogReader {
    pub lines: Vec<String>,
}

impl MockLogReader {
    pub fn new(lines: Vec<String>) -> Self {
        Self { lines }
    }
}

impl LogReader for MockLogReader {
    fn total_lines(&self) -> usize {
        self.lines.len()
    }

    fn get_line(&mut self, index: usize) -> Result<Option<String>> {
        Ok(self.lines.get(index).cloned())
    }

    fn reload(&mut self) -> Result<()> {
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
