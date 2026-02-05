//! CLI subcommand infrastructure for lazytail.
//!
//! Provides subcommand definitions for config initialization and management.

pub mod init;

use clap::{Args, Subcommand};

/// Available subcommands for lazytail.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Initialize a new lazytail.yaml config file
    Init(InitArgs),

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
