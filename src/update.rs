use axoupdater::AxoUpdater;
use semver::Version;
use serde::Deserialize;

use crate::utils::{default_user_agent, version};

const BBLUE: &str = "\x1b[1;34m"; // Bold Blue
const BGREEN: &str = "\x1b[1;32m"; // Bold Green
const BYELLOW: &str = "\x1b[1;33m"; // Bold Yellow
const BRED: &str = "\x1b[1;31m"; // Bold Red
const NC: &str = "\x1b[0m"; // No Color

const APP_NAME: &str = "pepe";
const LATEST_RELEASE_URL: &str = "https://api.github.com/repos/omarmhaimdat/pepe/releases/latest";
/// Set to any value to skip the update check that runs when pepe exits
const NO_UPDATE_CHECK_ENV: &str = "PEPE_NO_UPDATE_CHECK";

#[derive(Debug, Deserialize)]
struct Release {
    tag_name: String,
}

/// How this copy of pepe was installed, which decides how it gets updated
#[derive(Debug, PartialEq, Eq)]
enum InstallMethod {
    /// Installed by the shell/PowerShell installer, which leaves a receipt
    Installer,
    Homebrew,
    Nix,
    Cargo,
    Unknown,
}

impl InstallMethod {
    fn detect() -> Self {
        let exe = std::env::current_exe()
            .and_then(|p| p.canonicalize())
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        if exe.starts_with("/nix/store/") {
            return Self::Nix;
        }
        if exe.contains("/Cellar/") || exe.contains("/homebrew/") || exe.contains("/linuxbrew/") {
            return Self::Homebrew;
        }
        if installer_updater().is_some() {
            return Self::Installer;
        }
        if exe.contains("/.cargo/bin/") || exe.contains("\\.cargo\\bin\\") {
            return Self::Cargo;
        }
        Self::Unknown
    }

    fn update_command(&self) -> &'static str {
        match self {
            Self::Installer => "pepe self-update",
            Self::Homebrew => "brew upgrade pepe",
            Self::Nix => "nix profile upgrade pepe",
            Self::Cargo => {
                "cargo install --locked --force --git https://github.com/omarmhaimdat/pepe"
            }
            Self::Unknown if cfg!(windows) => "irm https://pepe.mhaimdat.com/install.ps1 | iex",
            Self::Unknown => "curl -LsSf https://pepe.mhaimdat.com/install.sh | sh",
        }
    }
}

/// An updater for installer-managed copies, if this executable is the one the
/// install receipt points at (a stale receipt must not update the wrong binary)
fn installer_updater() -> Option<AxoUpdater> {
    let mut updater = AxoUpdater::new_for(APP_NAME);
    updater.load_receipt().ok()?;
    match updater.check_receipt_is_for_this_executable() {
        Ok(true) => Some(updater),
        _ => None,
    }
}

/// `pepe self-update`: update in place when installed by the pepe installer,
/// otherwise explain how to update through the package manager that owns it
pub async fn self_update() -> Result<(), Box<dyn std::error::Error>> {
    let method = InstallMethod::detect();
    let Some(mut updater) = (method == InstallMethod::Installer)
        .then(installer_updater)
        .flatten()
    else {
        println!(
            "pepe {} was not installed by the pepe installer, so it can't update itself.",
            version()
        );
        println!("Update it with:\n  {}{}{}", BBLUE, method.update_command(), NC);
        return Ok(());
    };

    // Lets private forks / rate-limited CI pass a token, as axoupdater expects
    if let Ok(token) = std::env::var("PEPE_GITHUB_TOKEN") {
        updater.set_github_token(&token);
    }

    let outcome = match updater.run().await {
        Ok(outcome) => outcome,
        Err(e) => {
            eprintln!("{}Update failed:{} {}", BRED, NC, e);
            std::process::exit(1);
        }
    };
    match outcome {
        Some(result) => println!(
            "{}Updated pepe {} -> {}{}",
            BGREEN,
            result
                .old_version
                .map(|v| v.to_string())
                .unwrap_or_else(|| version().to_string()),
            result.new_version,
            NC
        ),
        None => println!("pepe {} is already the latest version.", version()),
    }
    Ok(())
}

async fn latest_version() -> Option<Version> {
    let release: Release = reqwest::Client::new()
        .get(LATEST_RELEASE_URL)
        .header("User-Agent", default_user_agent())
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    Version::parse(release.tag_name.trim_start_matches('v')).ok()
}

/// Print a notice when a newer release exists. Never fails and never nags
/// about older releases (dev builds are often ahead of the latest tag).
pub async fn check_for_updates() {
    if std::env::var_os(NO_UPDATE_CHECK_ENV).is_some() {
        return;
    }
    let Ok(current) = Version::parse(version()) else {
        return;
    };
    let Some(latest) = latest_version().await else {
        return;
    };
    if latest <= current {
        return;
    }

    println!("\n{}┌─────────────────────────────────────┐{}", BBLUE, NC);
    println!("{}│         Update available            │{}", BBLUE, NC);
    println!("{}└─────────────────────────────────────┘{}", BBLUE, NC);
    println!("{}→ Current version:{} {}{}{}", BYELLOW, NC, BRED, current, NC);
    println!("{}→ Latest version:{} {}{}{}\n", BYELLOW, NC, BGREEN, latest, NC);
    println!("{}To update, run:{}", BGREEN, NC);
    println!(
        "  {}{}{}\n",
        BBLUE,
        InstallMethod::detect().update_command(),
        NC
    );
    println!("(Set {}=1 to disable this check.)", NO_UPDATE_CHECK_ENV);
}
