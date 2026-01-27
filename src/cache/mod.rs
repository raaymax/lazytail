// Cache modules for performance optimization
// These are infrastructure components ready for integration
mod ansi_cache;
mod line_cache;

// Re-export for future use
#[allow(unused_imports)]
pub use ansi_cache::AnsiCache;
#[allow(unused_imports)]
pub use line_cache::LineCache;
