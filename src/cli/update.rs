//! Update subcommand for lazytail.
//!
//! Checks for updates and optionally installs them. Detects package manager
//! installations and advises the user accordingly.

use crate::update;
use colored::Colorize;

/// Run the update subcommand.
///
/// - `check_only`: If true, only check and report (exit code 0 = up-to-date, 1 = available).
/// - `nightly`: If true, look for the nightly pre-release instead of latest stable.
/// - Returns `Ok(())` on success or `Err(code)` with an exit code.
pub fn run(check_only: bool, nightly: bool) -> Result<(), i32> {
    let install_method = update::detection::detect_install_method();

    if nightly {
        eprintln!("Checking for nightly build...");
    } else {
        eprintln!("Checking for updates...");
    }

    let info = if nightly {
        match update::checker::check_nightly_version() {
            Ok(info) => info,
            Err(e) => {
                eprintln!("{} {}", "error:".red().bold(), e);
                return Err(1);
            }
        }
    } else {
        match update::checker::check_latest_version() {
            Ok(info) => info,
            Err(e) => {
                eprintln!("{} {}", "error:".red().bold(), e);
                return Err(1);
            }
        }
    };

    if !info.is_update_available() {
        println!(
            "{} lazytail {} is up to date.",
            "✓".green().bold(),
            info.current_version
        );
        return Ok(());
    }

    println!(
        "{} {} available: {} → {}",
        "●".yellow().bold(),
        if nightly { "Nightly build" } else { "Update" },
        info.current_version.dimmed(),
        info.latest_version.green().bold()
    );

    if check_only {
        // Exit code 1 signals "update available" for scripting
        return Err(1);
    }

    // Check if we should defer to a package manager
    match install_method {
        update::InstallMethod::PackageManager { name, upgrade_cmd } => {
            if nightly {
                println!(
                    "\nlazytail was installed via {}. Nightly builds are only available for self-managed installs.",
                    name.bold()
                );
            } else {
                println!(
                    "\nlazytail was installed via {}. Update with:\n  {}",
                    name.bold(),
                    upgrade_cmd.cyan()
                );
            }
            Ok(())
        }
        update::InstallMethod::SelfManaged => {
            println!();
            let result = if nightly {
                update::installer::install_nightly()
            } else {
                update::installer::install_latest()
            };
            match result {
                Ok(version) => {
                    println!(
                        "\n{} Successfully updated to {}!",
                        "✓".green().bold(),
                        version.green().bold()
                    );
                    Ok(())
                }
                Err(e) => {
                    eprintln!("{} {}", "error:".red().bold(), e);
                    Err(1)
                }
            }
        }
    }
}
