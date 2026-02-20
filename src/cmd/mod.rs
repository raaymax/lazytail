//! CLI subcommand infrastructure for lazytail.
//!
//! Provides subcommand definitions for config initialization and management.

pub mod config;
pub mod init;
#[cfg(feature = "self-update")]
pub mod update;

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

/// Config subcommand actions.
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Validate the config file
    Validate,
    /// Show effective configuration
    Show,
}
