//! CLI subcommand infrastructure for lazytail.
//!
//! Provides subcommand definitions for config initialization and management.

pub mod bench;
pub mod config;
pub mod init;
#[cfg(feature = "self-update")]
pub mod update;

use clap::{Args, Subcommand};
use std::path::PathBuf;

/// Available subcommands for lazytail.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new lazytail.yaml config file
    Init(InitArgs),

    /// Start browser-based web UI
    Web(WebArgs),

    /// Benchmark filter performance
    Bench(BenchArgs),

    /// Config file commands
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Check for and install updates
    #[cfg(feature = "self-update")]
    Update(UpdateArgs),
}

/// Arguments for the init subcommand.
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Overwrite existing config file
    #[arg(long)]
    pub force: bool,
}

/// Arguments for the update subcommand.
#[cfg(feature = "self-update")]
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Only check for updates, don't install (exit code 0 = up-to-date, 1 = available)
    #[arg(long)]
    pub check: bool,
}

/// Arguments for the bench subcommand.
#[derive(Args, Debug)]
pub struct BenchArgs {
    /// Filter pattern
    #[arg(value_name = "PATTERN")]
    pub pattern: String,

    /// Log files to benchmark
    #[arg(value_name = "FILE", required = true)]
    pub files: Vec<PathBuf>,

    /// Use regex mode
    #[arg(long)]
    pub regex: bool,

    /// Use query mode (structured query syntax)
    #[arg(long)]
    pub query: bool,

    /// Case-sensitive matching (default: case-insensitive)
    #[arg(long)]
    pub case_sensitive: bool,

    /// Number of benchmark trials
    #[arg(long, default_value_t = 5)]
    pub trials: usize,

    /// Output JSON instead of human-readable table
    #[arg(long)]
    pub json: bool,

    /// Run both indexed and non-indexed paths, report speedup
    #[arg(long)]
    pub compare: bool,
}

/// Config subcommand actions.
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Validate the config file
    Validate,
    /// Show effective configuration
    Show,
}

/// Arguments for the web subcommand.
#[derive(Args, Debug)]
pub struct WebArgs {
    /// Log files to open (optional). If omitted, discover sources from config/data dirs.
    #[arg(value_name = "FILE")]
    pub files: Vec<PathBuf>,

    /// Bind host
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Bind port
    #[arg(short = 'p', long, default_value_t = 8421)]
    pub port: u16,

    /// Disable file watching (sources won't auto-reload on changes)
    #[arg(long = "no-watch")]
    pub no_watch: bool,

    /// Verbose startup output
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}
