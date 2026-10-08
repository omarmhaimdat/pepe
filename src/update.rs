//! Keeping pepe fresh: a once-a-day look for a newer release, the notice
//! Pepe gives when there is one, and `pepe self-update`.

use std::io::{stderr, IsTerminal, Write};
use std::path::PathBuf;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axoupdater::AxoUpdater;
use ratatui::style::Color;
use ratatui::text::Line;
use semver::Version;
use serde::{Deserialize, Serialize};

use crate::ui::mascot::{self, Mood};
use crate::utils::{default_user_agent, version};

const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const BLUE: &str = "\x1b[1;34m";
const GREEN: &str = "\x1b[1;32m";
const RED: &str = "\x1b[1;31m";
const NC: &str = "\x1b[0m";

const APP_NAME: &str = "pepe";
const REPO: &str = "omarmhaimdat/pepe";
/// Set to any value to skip the look for a newer release
const NO_UPDATE_CHECK_ENV: &str = "PEPE_NO_UPDATE_CHECK";
/// Where the look's result is kept between runs; set for tests
const CACHE_DIR_ENV: &str = "PEPE_CACHE_DIR";
/// How long one look is good for
const CHECK_INTERVAL: Duration = Duration::from_secs(24 * 60 * 60);
/// A look that takes longer than this is given up on
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);
/// How much longer than the run itself a look may take before pepe quits
/// without waiting for it
const EXIT_GRACE: Duration = Duration::from_millis(300);
/// Changelog entries shown in a notice
const MAX_NOTES: usize = 5;
/// Width the notice is laid out for
const NOTICE_WIDTH: usize = 78;

fn latest_release_url() -> String {
    format!("https://api.github.com/repos/{REPO}/releases/latest")
}

fn changelog_url(v: &Version) -> String {
    format!("https://raw.githubusercontent.com/{REPO}/v{v}/CHANGELOG.md")
}

fn release_page(v: &Version) -> String {
    format!("https://github.com/{REPO}/releases/tag/v{v}")
}

// ─── What's new ──────────────────────────────────────────────────────────────

/// One line of the changelog: a group ("Added", "Fixed", ...) and the entry
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Note {
    pub group: String,
    pub text: String,
}

/// A release newer than this pepe, and what changed in between
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Latest {
    pub version: Version,
    /// Newest release first; may be empty if the changelog wasn't reachable
    pub notes: Vec<Note>,
}

/// The entries of every changelog section newer than `current`. Sections
/// are `## [x.y.z]` headings, groups `### Group` headings, entries lines
/// starting with `- `; an entry's indented explanation is left out.
fn notes_since(changelog: &str, current: &Version) -> Vec<Note> {
    let mut notes = Vec::new();
    let mut wanted = false;
    let mut group = String::from("Other");
    for line in changelog.lines() {
        if let Some(rest) = line.strip_prefix("## [") {
            let name = rest.split(']').next().unwrap_or_default();
            wanted = Version::parse(name).is_ok_and(|v| v > *current);
            group = "Other".into();
        } else if let Some(name) = line.strip_prefix("### ") {
            group = name.trim().to_string();
        } else if let Some(text) = line.strip_prefix("- ") {
            // release-plz's own version-bump commits say nothing to a user
            if wanted && !text.starts_with("release v") {
                notes.push(Note {
                    group: group.clone(),
                    text: text.trim().to_string(),
                });
            }
        }
    }
    notes
}

/// The notes worth a notice: at most `MAX_NOTES`, dropping "Other" first
fn headline_notes(notes: &[Note]) -> (Vec<&Note>, usize) {
    let mut picked: Vec<&Note> = notes.iter().filter(|n| n.group != "Other").collect();
    if picked.is_empty() {
        picked = notes.iter().collect();
    }
    let hidden = notes.len() - picked.len().min(MAX_NOTES);
    picked.truncate(MAX_NOTES);
    (picked, hidden)
}

// ─── The look for a newer release ────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
struct Cache {
    /// Unix seconds
    checked_at: u64,
    /// The pepe that looked; a newer one looks again
    current: String,
    latest: String,
    notes: Vec<Note>,
}

impl Cache {
    fn path() -> Option<PathBuf> {
        if let Some(dir) = std::env::var_os(CACHE_DIR_ENV) {
            return Some(PathBuf::from(dir).join("update-check.json"));
        }
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))?;
        let dir = if cfg!(target_os = "macos") {
            PathBuf::from(home).join("Library/Caches")
        } else if cfg!(windows) {
            std::env::var_os("LOCALAPPDATA").map_or_else(|| PathBuf::from(&home), PathBuf::from)
        } else {
            std::env::var_os("XDG_CACHE_HOME")
                .map_or_else(|| PathBuf::from(&home).join(".cache"), PathBuf::from)
        };
        Some(dir.join(APP_NAME).join("update-check.json"))
    }

    fn read() -> Option<Self> {
        serde_json::from_str(&std::fs::read_to_string(Self::path()?).ok()?).ok()
    }

    /// Best effort: a cache that can't be written just means another look
    /// next time
    fn write(&self) {
        let Some(path) = Self::path() else { return };
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string(self) {
            let _ = std::fs::write(path, json);
        }
    }

    fn fresh(&self, now: u64, current: &Version) -> bool {
        self.current == current.to_string()
            && now.saturating_sub(self.checked_at) < CHECK_INTERVAL.as_secs()
    }

    fn newer_than(&self, current: &Version) -> Option<Latest> {
        let version = Version::parse(&self.latest).ok()?;
        (version > *current).then(|| Latest {
            version,
            notes: self.notes.clone(),
        })
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Nobody is there to read a notice in CI, and some people don't want the
/// request made at all
fn disabled() -> bool {
    std::env::var_os(NO_UPDATE_CHECK_ENV).is_some() || std::env::var_os("CI").is_some()
}

async fn fetch(url: &str) -> Option<String> {
    let mut request = reqwest::Client::new()
        .get(url)
        .header("User-Agent", default_user_agent())
        .timeout(FETCH_TIMEOUT);
    // Anonymous calls to GitHub's API are rate-limited per address, which
    // shared CI runners exhaust; a token, where one is set, lifts that
    if let Ok(token) = std::env::var("PEPE_GITHUB_TOKEN") {
        if url.starts_with("https://api.github.com/") && !token.is_empty() {
            request = request.bearer_auth(token);
        }
    }
    request
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .text()
        .await
        .ok()
}

async fn latest_version() -> Option<Version> {
    #[derive(Deserialize)]
    struct Release {
        tag_name: String,
    }
    let release: Release = serde_json::from_str(&fetch(&latest_release_url()).await?).ok()?;
    Version::parse(release.tag_name.trim_start_matches('v')).ok()
}

/// Ask GitHub, and read the changelog if there is something newer
async fn look_up(current: &Version) -> Option<Latest> {
    let latest = latest_version().await?;
    let notes = if latest > *current {
        notes_since(
            &fetch(&changelog_url(&latest)).await.unwrap_or_default(),
            current,
        )
    } else {
        Vec::new()
    };
    Cache {
        checked_at: unix_now(),
        current: current.to_string(),
        latest: latest.to_string(),
        notes: notes.clone(),
    }
    .write();
    (latest > *current).then_some(Latest {
        version: latest,
        notes,
    })
}

/// The look for a newer release, started when pepe starts and read when it
/// quits, so it costs the run nothing and quitting almost nothing. Once a
/// day the look asks GitHub; otherwise it's the cached answer.
pub struct Check(Option<tokio::task::JoinHandle<Option<Latest>>>);

impl Check {
    pub fn start() -> Self {
        if disabled() {
            return Self(None);
        }
        Self(Some(tokio::spawn(async {
            let current = Version::parse(version()).ok()?;
            match Cache::read() {
                Some(cache) if cache.fresh(unix_now(), &current) => cache.newer_than(&current),
                _ => look_up(&current).await,
            }
        })))
    }

    /// What the look found, if it's done or nearly so
    pub async fn finish(self) -> Option<Latest> {
        let handle = self.0?;
        tokio::time::timeout(EXIT_GRACE, handle)
            .await
            .ok()?
            .ok()
            .flatten()
    }
}

/// A newer release the last look found, for screens to mention; no request
/// is made
pub fn known() -> Option<Version> {
    let current = Version::parse(version()).ok()?;
    Cache::read()?.newer_than(&current).map(|l| l.version)
}

// ─── How this copy was installed ─────────────────────────────────────────────

/// How this copy of pepe was installed, which decides how it gets updated
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
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

    fn update_command(self) -> &'static str {
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

    /// "installed with Homebrew", for a sentence
    fn described(self) -> &'static str {
        match self {
            Self::Installer => "installed by the pepe installer",
            Self::Homebrew => "installed with Homebrew",
            Self::Nix => "installed with Nix",
            Self::Cargo => "installed with cargo",
            Self::Unknown => "not installed by the pepe installer",
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

// ─── Saying it ───────────────────────────────────────────────────────────────

/// A ratatui line as ANSI text: the mascot is drawn with 256-colour cells,
/// or true colour in pepe's own theme
fn ansi(line: &Line) -> String {
    let code = |color: Option<Color>, layer: u8| match color {
        Some(Color::Indexed(n)) => Some(format!("{layer}8;5;{n}")),
        Some(Color::Rgb(r, g, b)) => Some(format!("{layer}8;2;{r};{g};{b}")),
        _ => None,
    };
    let mut out = String::new();
    for span in &line.spans {
        let codes: Vec<String> = [code(span.style.fg, 3), code(span.style.bg, 4)]
            .into_iter()
            .flatten()
            .collect();
        if codes.is_empty() {
            out.push_str(&span.content);
        } else {
            out.push_str(&format!("\x1b[{}m{}{NC}", codes.join(";"), span.content));
        }
    }
    out
}

/// Pepe, each row padded to the mascot's width, with a word under it
fn pepe_says(mood: Mood, words: &str) -> Vec<String> {
    let width = mascot::WIDTH as usize;
    let mut rows: Vec<String> = mascot::lines(mood, 0)
        .iter()
        .map(|line| format!("{}{}", ansi(line), " ".repeat(width - line.width())))
        .collect();
    rows.push(format!("{DIM}{words:^width$}{NC}"));
    rows
}

/// `left` beside `right`, row by row; whichever is shorter is padded
fn side_by_side(left: &[String], left_width: usize, right: &[String]) -> String {
    let blank = " ".repeat(left_width);
    let mut out = String::new();
    for i in 0..left.len().max(right.len()) {
        let l = left.get(i).map_or(blank.as_str(), String::as_str);
        let r = right.get(i).map_or("", String::as_str);
        out.push_str(l);
        out.push_str("  ");
        out.push_str(r);
        out.push('\n');
    }
    out
}

/// A shell command on lines of at most `width`, indented, broken at spaces
/// with `\` so it still pastes
fn wrap_command(command: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut line = String::from("  ");
    for word in command.split(' ') {
        // Room for the word and a trailing " \"
        if line.len() > 2 && line.len() + 1 + word.len() + 2 > width {
            line.push_str(" \\");
            lines.push(std::mem::replace(&mut line, String::from("    ")));
        }
        if line.trim().is_empty() {
            line.push_str(word);
        } else {
            line.push(' ');
            line.push_str(word);
        }
    }
    lines.push(line);
    lines
}

/// A path cut from the left to fit `max`, keeping its end
fn fit_path(path: &str, max: usize) -> String {
    let n = path.chars().count();
    if n <= max {
        return path.to_string();
    }
    let tail: String = path.chars().skip(n - max.saturating_sub(1)).collect();
    format!("…{tail}")
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// "what's new" lines, group then entry, fitted to `NOTICE_WIDTH`
fn whats_new(notes: &[Note]) -> String {
    let (picked, hidden) = headline_notes(notes);
    if picked.is_empty() {
        return String::new();
    }
    let group_width = picked
        .iter()
        .map(|n| n.group.chars().count())
        .max()
        .unwrap_or(0);
    let mut out = format!("{BOLD}what's new{NC}\n");
    for note in picked {
        let text = truncate(&note.text, NOTICE_WIDTH.saturating_sub(group_width + 4));
        out.push_str(&format!(
            "  {DIM}{:<group_width$}{NC}  {text}\n",
            note.group
        ));
    }
    if hidden > 0 {
        out.push_str(&format!("  {DIM}… and {hidden} more{NC}\n"));
    }
    out
}

/// Pepe saying `words` with `lines` beside it, or just the lines when the
/// terminal isn't one that shows colours
fn pepe_block(mood: Mood, words: &str, lines: &[String], color: bool) -> String {
    if color {
        side_by_side(&pepe_says(mood, words), mascot::WIDTH as usize, lines)
    } else {
        let mut out = String::new();
        for line in lines {
            out.push_str(line);
            out.push('\n');
        }
        out
    }
}

/// Columns left for text beside the mascot
const BESIDE_WIDTH: usize = NOTICE_WIDTH - mascot::WIDTH as usize - 2;

/// "update with" and the command, laid out beside the mascot
fn how_to_update(method: InstallMethod) -> Vec<String> {
    let mut lines = vec![format!("{DIM}update with{NC}")];
    lines.extend(
        wrap_command(method.update_command(), BESIDE_WIDTH)
            .into_iter()
            .map(|l| format!("{BLUE}{l}{NC}")),
    );
    lines
}

/// The notice that a newer release exists, as pepe prints it when it quits
pub fn notice(latest: &Latest, color: bool) -> String {
    notice_for(latest, InstallMethod::detect(), color)
}

/// `notice` for a copy installed a known way. Finding out how this copy
/// was installed reads the install receipt, which the tests leave alone.
fn notice_for(latest: &Latest, method: InstallMethod, color: bool) -> String {
    let mut beside = vec![
        format!(
            "{BOLD}pepe {} is out{NC} {DIM}· you have {}{NC}",
            latest.version,
            version()
        ),
        String::new(),
    ];
    beside.extend(how_to_update(method));
    format!(
        "\n{}{}{DIM}notes{NC}  {}\n{DIM}({NO_UPDATE_CHECK_ENV}=1 turns this off){NC}\n",
        pepe_block(Mood::Proud, "a fresher me is out", &beside, color),
        whats_new(&latest.notes),
        release_page(&latest.version)
    )
}

/// Text without the escape codes, for terminals that don't want them
fn plain(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// A line on stderr, its colour dropped where colour is off
macro_rules! sayln {
    ($($arg:tt)*) => {
        say(&format!("{}\n", format!($($arg)*)))
    };
}

/// Colour on stderr: a terminal, and `NO_COLOR` not set
fn colorful() -> bool {
    stderr().is_terminal() && !crate::ui::theme::no_color()
}

fn say(text: &str) {
    if colorful() {
        eprint!("{text}");
    } else {
        eprint!("{}", plain(text));
    }
}

// ─── pepe self-update ────────────────────────────────────────────────────────

/// Runs `work` behind a one-line spinner on stderr, when someone's watching
async fn with_spinner<T>(label: &str, work: impl std::future::Future<Output = T>) -> T {
    if !stderr().is_terminal() {
        eprintln!("{label}…");
        return work.await;
    }
    let label = label.to_string();
    let spinner = tokio::spawn(async move {
        const FRAMES: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
        for frame in FRAMES.iter().cycle() {
            if colorful() {
                eprint!("\r{BLUE}{frame}{NC} {label}…");
            } else {
                eprint!("\r{frame} {label}…");
            }
            let _ = stderr().flush();
            tokio::time::sleep(Duration::from_millis(80)).await;
        }
    });
    let out = work.await;
    spinner.abort();
    eprint!("\r\x1b[2K");
    out
}

/// `pepe self-update`: say what's new, then update in place when installed
/// by the pepe installer, or say how to update through whatever installed
/// it. `check_only` stops after saying; `verbose` shows the installer's own
/// output.
pub async fn self_update(
    check_only: bool,
    verbose: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let current = Version::parse(version())?;
    let method = InstallMethod::detect();
    let color = colorful();

    let latest = with_spinner("looking for a newer pepe", look_up(&current)).await;
    let Some(latest) = latest else {
        // Either up to date or unreachable; a cache written by this pepe
        // means the look got through
        let reached = Cache::read().is_some_and(|c| c.current == current.to_string());
        if !reached {
            sayln!(
                "{RED}couldn't reach GitHub{NC} to look for a newer pepe; try again in a moment"
            );
            std::process::exit(2);
        }
        say(&pepe_block(
            Mood::Happy,
            "fresh as can be",
            &[
                format!("{BOLD}pepe {current} is the latest release{NC}"),
                format!("{DIM}this copy was {}{NC}", method.described()),
            ],
            color,
        ));
        return Ok(());
    };

    let updater = (method == InstallMethod::Installer)
        .then(installer_updater)
        .flatten();
    let mut beside = vec![
        format!(
            "{BOLD}pepe {} is out{NC} {DIM}· you have {current}{NC}",
            latest.version
        ),
        String::new(),
    ];
    if check_only || updater.is_none() {
        if updater.is_none() {
            beside.push(format!("{DIM}this copy was {}{NC}", method.described()));
        }
        beside.extend(how_to_update(method));
    }
    say(&pepe_block(
        Mood::Waiting,
        "a fresher me is out",
        &beside,
        color,
    ));
    say(&format!(
        "{}{DIM}notes{NC}  {}\n",
        whats_new(&latest.notes),
        release_page(&latest.version)
    ));
    if check_only {
        std::process::exit(1);
    }
    if updater.is_none() {
        return Ok(());
    }
    let mut updater = updater.expect("checked above");

    // Lets private forks / rate-limited CI pass a token, as axoupdater expects
    if let Ok(token) = std::env::var("PEPE_GITHUB_TOKEN") {
        updater.set_github_token(&token);
    }
    if !verbose {
        updater.disable_installer_output();
    }
    say("\n");
    // The installer runs as a child process, which would stall a spinner
    // on this thread, so the update runs on another, with a runtime of its
    // own for the updater's downloads
    let run = tokio::task::spawn_blocking(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build a tokio runtime")
            .block_on(updater.run())
            // The updater's error is a large enum; boxed, the result stays small
            .map_err(Box::new)
    });
    let outcome = if verbose {
        run.await?
    } else {
        with_spinner(
            &format!("downloading and installing pepe {}", latest.version),
            run,
        )
        .await?
    };

    match outcome {
        Ok(Some(result)) => {
            let old = result
                .old_version
                .map(|v| v.to_string())
                .unwrap_or_else(|| current.to_string());
            say(&pepe_block(
                Mood::Proud,
                "fresh out of the oven",
                &[
                    format!(
                        "{GREEN}✔ pepe {} is in{NC} {DIM}· was {old}{NC}",
                        result.new_version
                    ),
                    format!(
                        "{DIM}installed to{NC} {}",
                        fit_path(result.install_prefix.as_str(), BESIDE_WIDTH - 13)
                    ),
                    format!("{DIM}run{NC} pepe {DIM}to try it{NC}"),
                ],
                color,
            ));
            // A fresh pepe shouldn't be told about itself
            if let Some(mut cache) = Cache::read() {
                cache.current = result.new_version.to_string();
                cache.write();
            }
            refresh_completions(std::path::Path::new(result.install_prefix.as_str()));
        }
        Ok(None) => sayln!("{GREEN}pepe {current} is already the latest release.{NC}"),
        Err(e) => {
            sayln!("{RED}✖ the update didn't finish:{NC} {e}");
            sayln!(
                "\nrun it again with {BLUE}pepe self-update --verbose{NC} to see the installer, \
                 or install the release directly:\n  {BLUE}{}{NC}",
                InstallMethod::Unknown.update_command()
            );
            std::process::exit(1);
        }
    }
    Ok(())
}

/// Completions set up with `pepe completions --install` come out of the
/// binary, so the new pepe writes them again for the shells they were set
/// up for; shells never set up are left alone
fn refresh_completions(install_prefix: &std::path::Path) {
    let shells = crate::completions::installed_shells();
    if shells.is_empty() {
        return;
    }
    let pepe = install_prefix.join(if cfg!(windows) { "pepe.exe" } else { "pepe" });
    for shell in shells {
        let ok = std::process::Command::new(&pepe)
            .args(["completions", "--install", shell.arg()])
            .output()
            .is_ok_and(|out| out.status.success());
        if ok {
            sayln!("{GREEN}✔{NC} tab completion for {} refreshed", shell.arg());
        } else {
            sayln!(
                "{DIM}tab completion for {} wasn't refreshed; run{NC} pepe completions --install",
                shell.arg()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANGELOG: &str = "\
# Changelog

## [Unreleased]

## [0.7.0](https://x/compare/v0.6.1...v0.7.0) - 2026-10-09

### Added

- a thing people wanted
- *(api)* another thing

### Other

- release v0.7.0
- tidy the workflow

## [0.6.1](https://x/compare/v0.6.0...v0.6.1) - 2026-10-02

### Performance

- share-nothing load engine, 4× less CPU for the same requests

  pepe spent most of its CPU coordinating threads.

  - Requests go out from shard threads.

## [0.6.0] - 2026-10-01

### Fixed

- something old
";

    fn v(s: &str) -> Version {
        Version::parse(s).unwrap()
    }

    #[test]
    fn notes_cover_every_release_newer_than_this_one() {
        let notes = notes_since(CHANGELOG, &v("0.6.0"));
        let texts: Vec<&str> = notes.iter().map(|n| n.text.as_str()).collect();
        assert_eq!(
            texts,
            [
                "a thing people wanted",
                "*(api)* another thing",
                "tidy the workflow",
                "share-nothing load engine, 4× less CPU for the same requests",
            ],
            "indented explanations and release bumps are left out"
        );
        assert_eq!(notes[0].group, "Added");
        assert_eq!(notes[2].group, "Other");
        assert_eq!(notes[3].group, "Performance");
        assert!(notes_since(CHANGELOG, &v("0.7.0")).is_empty());
        assert_eq!(notes_since(CHANGELOG, &v("0.6.1")).len(), 3);
    }

    #[test]
    fn headlines_drop_other_first_and_count_the_rest() {
        let notes: Vec<Note> = (0..8)
            .map(|i| Note {
                group: if i % 2 == 0 { "Added" } else { "Other" }.into(),
                text: format!("note {i}"),
            })
            .collect();
        let (picked, hidden) = headline_notes(&notes);
        assert_eq!(picked.len(), 4);
        assert!(picked.iter().all(|n| n.group == "Added"));
        assert_eq!(hidden, 4);

        let only_other = vec![Note {
            group: "Other".into(),
            text: "x".into(),
        }];
        assert_eq!(headline_notes(&only_other).0.len(), 1);
    }

    #[test]
    fn cache_is_good_for_a_day_for_the_same_pepe() {
        let cache = Cache {
            checked_at: 1_000_000,
            current: "0.6.1".into(),
            latest: "0.7.0".into(),
            notes: Vec::new(),
        };
        assert!(cache.fresh(1_000_000 + 3_600, &v("0.6.1")));
        assert!(!cache.fresh(1_000_000 + 90_000, &v("0.6.1")));
        assert!(
            !cache.fresh(1_000_000 + 60, &v("0.7.0")),
            "a new pepe looks again"
        );
        assert_eq!(cache.newer_than(&v("0.6.1")).unwrap().version, v("0.7.0"));
        assert!(cache.newer_than(&v("0.7.0")).is_none());
    }

    #[test]
    fn notice_fits_eighty_columns_and_says_how() {
        let latest = Latest {
            version: v("0.7.0"),
            notes: notes_since(CHANGELOG, &v("0.6.0")),
        };
        for color in [true, false] {
            let text = plain(&notice_for(&latest, InstallMethod::Homebrew, color));
            assert!(text.contains("pepe 0.7.0 is out"));
            assert!(text.contains("what's new"));
            assert!(text.contains("a thing people wanted"));
            assert!(text.contains("releases/tag/v0.7.0"));
            for line in text.lines() {
                assert!(line.chars().count() <= 80, "{line:?}");
            }
        }
        let text = plain(&notice_for(&latest, InstallMethod::Cargo, true));
        assert!(
            text.contains("cargo install --locked --force --git \\"),
            "long commands wrap"
        );
        // Eight sprite rows and the words under them; "what's new", three
        // entries and the count of the rest; the notes link; the hint
        assert_eq!(text.lines().filter(|l| !l.is_empty()).count(), 16);
        assert!(
            text.contains("4× less CPU for the same requests"),
            "entries are not cut short"
        );
    }

    #[test]
    fn commands_wrap_at_spaces_and_still_paste() {
        let short = wrap_command("pepe self-update", 56);
        assert_eq!(short, ["  pepe self-update"]);
        let long = wrap_command(InstallMethod::Cargo.update_command(), 56);
        assert_eq!(
            long,
            [
                "  cargo install --locked --force --git \\",
                "    https://github.com/omarmhaimdat/pepe",
            ]
        );
        for method in [
            InstallMethod::Unknown,
            InstallMethod::Homebrew,
            InstallMethod::Nix,
        ] {
            for line in wrap_command(method.update_command(), 56) {
                assert!(line.chars().count() <= 56, "{line:?}");
            }
        }
        assert_eq!(fit_path("/a/b/c", 10), "/a/b/c");
        assert_eq!(fit_path("/very/long/path/to/bin", 10), "…th/to/bin");
    }

    #[test]
    fn plain_strips_escape_codes() {
        assert_eq!(plain(&format!("{BOLD}a{NC} b\x1b[38;5;196mc{NC}")), "a bc");
    }
}
