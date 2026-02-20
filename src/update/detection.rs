//! Package manager detection for lazytail.
//!
//! Determines how lazytail was installed to advise users on the correct
//! update method (self-update vs package manager command).

use super::InstallMethod;
use std::process::Command;

/// Detect how lazytail was installed.
///
/// Checks common package managers in order:
/// 1. pacman (Arch/AUR)
/// 2. dpkg (Debian/Ubuntu)
/// 3. brew (macOS Homebrew)
/// 4. Path heuristic (/usr/bin = likely system-managed)
/// 5. Fallback to SelfManaged
pub fn detect_install_method() -> InstallMethod {
    let bin_path = match std::env::current_exe() {
        Ok(p) => p,
        Err(_) => return InstallMethod::SelfManaged,
    };

    let bin_str = bin_path.to_string_lossy().to_string();

    // Check pacman (Arch Linux / AUR)
    if let Ok(output) = Command::new("pacman").args(["-Qo", &bin_str]).output() {
        if output.status.success() {
            return InstallMethod::PackageManager {
                name: "pacman/AUR".to_string(),
                upgrade_cmd: "yay -S lazytail".to_string(),
            };
        }
    }

    // Check dpkg (Debian/Ubuntu)
    if let Ok(output) = Command::new("dpkg").args(["-S", &bin_str]).output() {
        if output.status.success() {
            return InstallMethod::PackageManager {
                name: "dpkg".to_string(),
                upgrade_cmd: "sudo apt update && sudo apt upgrade lazytail".to_string(),
            };
        }
    }

    // Check Homebrew (macOS)
    if let Ok(output) = Command::new("brew")
        .args(["list", "--formula", "lazytail"])
        .output()
    {
        if output.status.success() {
            return InstallMethod::PackageManager {
                name: "Homebrew".to_string(),
                upgrade_cmd: "brew upgrade lazytail".to_string(),
            };
        }
    }

    // Path-based heuristic
    detect_from_path(&bin_path)
}

/// Detect install method based on the binary path.
///
/// Binaries in system directories (/usr/bin, /usr/local/bin with package manager)
/// are likely system-managed.
pub fn detect_from_path(path: &std::path::Path) -> InstallMethod {
    let path_str = path.to_string_lossy();

    if path_str.starts_with("/usr/bin/") || path_str.starts_with("/usr/sbin/") {
        return InstallMethod::PackageManager {
            name: "system".to_string(),
            upgrade_cmd: "your system package manager".to_string(),
        };
    }

    InstallMethod::SelfManaged
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn test_detect_from_path_usr_bin() {
        let method = detect_from_path(Path::new("/usr/bin/lazytail"));
        match method {
            InstallMethod::PackageManager { name, .. } => {
                assert_eq!(name, "system");
            }
            InstallMethod::SelfManaged => panic!("Expected PackageManager"),
        }
    }

    #[test]
    fn test_detect_from_path_home() {
        let method = detect_from_path(Path::new("/home/user/.local/bin/lazytail"));
        assert!(matches!(method, InstallMethod::SelfManaged));
    }

    #[test]
    fn test_detect_from_path_cargo() {
        let method = detect_from_path(Path::new("/home/user/.cargo/bin/lazytail"));
        assert!(matches!(method, InstallMethod::SelfManaged));
    }

    #[test]
    fn test_detect_from_path_opt() {
        let method = detect_from_path(Path::new("/opt/lazytail/bin/lazytail"));
        assert!(matches!(method, InstallMethod::SelfManaged));
    }
}
