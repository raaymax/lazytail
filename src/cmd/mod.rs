//! CLI subcommand infrastructure for lazytail.
//!
//! Provides subcommand definitions for config initialization and management.

pub mod config;
pub mod init;

use clap::{Args, Subcommand};
use std::path::PathBuf;

/// Available subcommands for lazytail.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new lazytail.yaml config file
    Init(InitArgs),

    /// Start browser-based web UI
    Web(WebArgs),

    /// Config file commands
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

/// Arguments for the init subcommand.
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Overwrite existing config file
    #[arg(long)]
    pub force: bool,
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

    /// Do not open browser automatically
    #[arg(long)]
    pub no_open: bool,

    /// Disable file watching (sources won't auto-reload on changes)
    #[arg(long = "no-watch")]
    pub no_watch: bool,

    /// Verbose startup output
    #[arg(short = 'v', long = "verbose")]
    pub verbose: bool,
}
