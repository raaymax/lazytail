// Cache modules for performance optimization
// These are infrastructure components ready for integration
#[allow(dead_code)]
mod ansi_cache;
#[allow(dead_code)]
mod line_cache;

// Re-export for future use
#[allow(unused_imports)]
pub use ansi_cache::AnsiCache;
#[allow(unused_imports)]
pub use line_cache::LineCache;
