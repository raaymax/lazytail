// Library interface for LazyTail
// Exposes internal modules for examples, benchmarks, and external tools

pub mod config;
pub mod filter;
pub mod index;
pub mod parsing;
pub mod reader;
pub mod renderer;
pub mod source;
pub mod text_wrap;
pub mod theme;

#[cfg(test)]
pub mod test_utils;
