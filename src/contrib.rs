//! The files under `contrib/`: shell completions and man pages, generated
//! from the command definition so they can't drift from it.
//!
//! `cargo test` checks the committed files are current;
//! `UPDATE_CONTRIB=1 cargo test` rewrites them. They ship in every release
//! archive (see `include` in dist-workspace.toml).

#![cfg(test)]

use std::path::Path;

use clap::{Command, CommandFactory};
use clap_complete::Shell;
use roff::{bold, roman, Roff};

use crate::cli::Cli;

/// Shells with a completion file, and its name
const SHELLS: [(Shell, &str); 4] = [
    (Shell::Bash, "pepe.bash"),
    (Shell::Zsh, "_pepe"),
    (Shell::Fish, "pepe.fish"),
    (Shell::PowerShell, "_pepe.ps1"),
];

/// The dashboard's keys, as in the README
const DASHBOARD_KEYS: [(&str, &str); 15] = [
    (
        "space, p",
        "Pause or resume sending; a timed run's clock stops while paused",
    ),
    ("+, -", "Raise or lower concurrency by about 10%, live"),
    ("s, i", "Stop sending and keep the results on screen"),
    (
        "r",
        "Restart with the same settings (and the current concurrency)",
    ),
    ("e", "Back to the setup screen (or, in API mode, the plan)"),
    ("tab, left, right, 1-3", "Switch view"),
    (
        "up, down, j, k, PgUp, PgDn, home, end",
        "Select a request in the log",
    ),
    (
        "f",
        "Filter requests by status: 2xx, 3xx, 4xx, 5xx, no response, failed",
    ),
    (
        "l",
        "Filter requests by latency: at or above p50, p90 or p99",
    ),
    ("/", "Search the status and response text"),
    ("x", "Show only failed requests"),
    ("c", "Clear all filters"),
    ("enter", "Inspect the selected request; esc goes back"),
    ("?", "Show all keys"),
    ("q, esc, Ctrl-C", "Quit"),
];

const SETUP_KEYS: [(&str, &str); 7] = [
    ("tab", "Switch mode: Single URL, Ramp or API"),
    (
        "up, down",
        "Move between fields; left, right change a choice",
    ),
    ("enter", "Start the run (or load the spec, in API mode)"),
    ("ctrl-t", "Send the request once and show the response"),
    ("ctrl-s", "Save the form as pepe.toml"),
    ("F1", "Show all keys (? too, outside a text field)"),
    ("esc", "Quit"),
];

const RAMP_KEYS: [(&str, &str); 6] = [
    (
        "up, down",
        "Pick a step and see everything measured about it; esc follows the run again",
    ),
    (
        "space",
        "Pause or resume; a step's clock stops while paused",
    ),
    ("n", "End this step now and go on to the next"),
    ("s", "Stop the ramp here and keep the results"),
    ("r, e", "Run again; back to the setup screen"),
    ("?", "Show all keys"),
];

const ENVIRONMENT: [(&str, &str); 4] = [
    (
        "PEPE_NO_UPDATE_CHECK",
        "Set to anything to skip the look for a newer release",
    ),
    (
        "PEPE_GITHUB_TOKEN",
        "A GitHub token for pepe self-update, for forks or rate-limited CI",
    ),
    (
        "NO_COLOR",
        "Set to anything to draw without colour: reverse video and shades instead",
    ),
    (
        "PEPE_THEME",
        "light or dark, for the terminal's background; otherwise COLORFGBG decides",
    ),
];

const EXAMPLES: [(&str, &str); 7] = [
    (
        "pepe replay access.log --base-url https://staging.example.com -c 50 -z 2m",
        "The log's URLs, in their real proportions",
    ),
    (
        "pepe flow checkout.toml -c 20 -z 1m",
        "A sequence of requests, each step fed by the one before",
    ),
    (
        "pepe https://example.com",
        "100 requests, one per core at a time, with the dashboard",
    ),
    (
        "pepe -z 30s -c 50 https://example.com",
        "Thirty seconds at concurrency 50",
    ),
    (
        "pepe --curl -- curl -X POST https://httpbin.org/post -d '{\"a\":1}'",
        "The request a curl command would send",
    ),
    (
        "pepe ramp https://example.com --to 200 --until 'p99 > 500ms'",
        "Raise the load until p99 passes 500 ms",
    ),
    (
        "pepe --json -n 1000 https://example.com > results.json",
        "No dashboard; a JSON report",
    ),
];

/// The command without anything that changes between machines or releases,
/// so the generated files are the same everywhere: the concurrency default
/// is the machine's core count and the user agent's carries the version
fn command() -> Command {
    Cli::command()
        .mut_arg("concurrency", |a| a.default_value("one per core"))
        .mut_arg("user_agent", |a| a.hide_default_value(true))
}

fn completion(shell: Shell) -> String {
    let mut out = Vec::new();
    clap_complete::generate(shell, &mut command(), "pepe", &mut out);
    String::from_utf8(out).expect("completions are text")
}

/// A two-column section: term in bold, then its explanation
fn table(roff: &mut Roff, title: &str, rows: &[(&str, &str)]) {
    roff.control("SH", [title]);
    for (term, text) in rows {
        roff.control("TP", []);
        roff.text([bold(*term)]);
        roff.text([roman(*text)]);
    }
}

/// `pepe.1`: the command, then the keys of its screens, examples and
/// environment, which the README has and `--help` doesn't
fn main_page() -> String {
    // No version in the header: the page would otherwise change with every
    // release and fall out of date on the release PR
    let man = clap_mangen::Man::new(command())
        .source("pepe")
        .manual("User Commands");
    let mut out = Vec::new();
    man.render_title(&mut out).unwrap();
    man.render_name_section(&mut out).unwrap();
    man.render_synopsis_section(&mut out).unwrap();
    man.render_description_section(&mut out).unwrap();
    man.render_options_section(&mut out).unwrap();
    man.render_subcommands_section(&mut out).unwrap();
    let mut roff = Roff::new();
    table(&mut roff, "DASHBOARD KEYS", &DASHBOARD_KEYS);
    table(&mut roff, "SETUP SCREEN KEYS", &SETUP_KEYS);
    table(&mut roff, "RAMP KEYS", &RAMP_KEYS);
    table(&mut roff, "EXAMPLES", &EXAMPLES);
    table(&mut roff, "ENVIRONMENT", &ENVIRONMENT);
    roff.control("SH", ["SEE ALSO"]);
    roff.text([
        roman("pepe-ramp(1), pepe-api(1), pepe-self-update(1), and "),
        bold("https://github.com/omarmhaimdat/pepe"),
    ]);
    roff.to_writer(&mut out).unwrap();
    String::from_utf8(out).expect("roff is text")
}

/// Each subcommand's page: (subcommand, page name, how it's typed)
const SUBCOMMANDS: [(&str, &str, &str); 6] = [
    ("ramp", "pepe-ramp", "pepe ramp"),
    ("api", "pepe-api", "pepe api"),
    ("replay", "pepe-replay", "pepe replay"),
    ("flow", "pepe-flow", "pepe flow"),
    ("self-update", "pepe-self-update", "pepe self-update"),
    ("completions", "pepe-completions", "pepe completions"),
];

/// `pepe-<sub>.1` for each subcommand
fn subcommand_pages() -> Vec<(String, String)> {
    let command = command();
    let mut subcommands: Vec<&str> = command
        .get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .map(|sub| sub.get_name())
        .collect();
    subcommands.sort_unstable();
    let mut named: Vec<&str> = SUBCOMMANDS.iter().map(|s| s.0).collect();
    named.sort_unstable();
    assert_eq!(subcommands, named, "every subcommand has a man page name");
    SUBCOMMANDS
        .iter()
        .map(|&(sub, name, typed)| {
            let page = command.find_subcommand(sub).unwrap().clone();
            let man = clap_mangen::Man::new(page.name(name).bin_name(typed))
                .source("pepe")
                .manual("User Commands");
            let mut out = Vec::new();
            man.render(&mut out).unwrap();
            (format!("{name}.1"), String::from_utf8(out).unwrap())
        })
        .collect()
}

/// Every file under contrib/, with its content
fn files() -> Vec<(String, String)> {
    let mut files: Vec<(String, String)> = SHELLS
        .iter()
        .map(|&(shell, name)| (format!("completions/{name}"), completion(shell)))
        .collect();
    files.push(("man/pepe.1".into(), main_page()));
    files.extend(
        subcommand_pages()
            .into_iter()
            .map(|(name, page)| (format!("man/{name}"), page)),
    );
    files
}

#[test]
fn contrib_files_are_current() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("contrib");
    let update = std::env::var_os("UPDATE_CONTRIB").is_some();
    let mut stale = Vec::new();
    for (name, content) in files() {
        let path = root.join(&name);
        if update {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &content).unwrap();
        } else {
            // A Windows checkout may have turned the line endings into CRLF
            let on_disk = std::fs::read_to_string(&path)
                .unwrap_or_default()
                .replace("\r\n", "\n");
            if on_disk != content {
                stale.push(name);
            }
        }
    }
    assert!(
        stale.is_empty(),
        "out of date: {stale:?}; run `UPDATE_CONTRIB=1 cargo test contrib` and commit the result"
    );
    assert!(files().iter().any(|(n, _)| n == "man/pepe-ramp.1"));
    let page = main_page();
    for section in [
        "DASHBOARD KEYS",
        "SETUP SCREEN KEYS",
        "RAMP KEYS",
        "EXAMPLES",
        "ENVIRONMENT",
    ] {
        assert!(page.contains(section), "{section}");
    }
    assert!(
        !page.contains(crate::utils::version()),
        "no version in the page"
    );
}
