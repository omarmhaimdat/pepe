//! `pepe history`: the saved tests and their recorded runs.

use std::time::SystemTime;

use super::*;
use crate::compare::{compare, Side};
use crate::store::{stamp, Run};
use crate::ui::format;

/// `pepe history`: every saved test with its last run; `pepe history
/// NAME`: that test's runs, the latest last, each against the one before
pub(crate) fn run_history(
    args: &Cli,
    what: &cli::HistoryArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let store = Store::local();
    match &what.name {
        Some(name) => one(args, &store, name),
        None => all(args, &store),
    }
}

/// A recorded run, read
struct Read {
    run: Run,
    side: Side,
}

fn read(runs: Vec<Run>) -> Vec<Read> {
    runs.into_iter()
        .filter_map(|run| Side::read(&run.path).ok().map(|side| Read { run, side }))
        .collect()
}

fn all(args: &Cli, store: &Store) -> Result<(), Box<dyn std::error::Error>> {
    let entries = store.entries();
    if entries.is_empty() {
        eprintln!(
            "nothing is saved in {} yet: ctrl-s on the setup screen, or --keep NAME on a run",
            store.tests_dir().display()
        );
        std::process::exit(1);
    }
    if args.json {
        let tests: Vec<serde_json::Value> = entries
            .iter()
            .map(|e| {
                serde_json::json!({
                    "name": e.name,
                    "runs": store.runs(&e.name).len(),
                    "last": e.last.as_ref().map(|l| serde_json::json!({
                        "when": stamp(l.when),
                        "report": l.side,
                    })),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "tests": tests }))?
        );
        return Ok(());
    }
    let name_w = entries
        .iter()
        .map(|e| e.name.len())
        .max()
        .unwrap_or(4)
        .max(4);
    let mut out = format!(
        "pepe history · {} saved test{} · {}\n{:<name_w$}  {:>4}  LAST RUN\n",
        entries.len(),
        if entries.len() == 1 { "" } else { "s" },
        store.tests_dir().display(),
        "NAME",
        "RUNS",
    );
    for e in &entries {
        let runs = store.runs(&e.name).len();
        let last = match &e.last {
            Some(last) => format!("{} · {}", ago(last.when), numbers(&last.side)),
            None if runs > 0 => "unreadable".to_string(),
            None => "none yet".to_string(),
        };
        out += &format!("{:<name_w$}  {:>4}  {last}\n", e.name, runs);
    }
    print!("{out}");
    Ok(())
}

fn one(args: &Cli, store: &Store, name: &str) -> Result<(), Box<dyn std::error::Error>> {
    if let Err(e) = store.load(name) {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
    let runs = read(store.runs(name));
    if runs.is_empty() {
        eprintln!("no recorded run of `{name}` yet: pepe --load {name} --record");
        std::process::exit(1);
    }
    if args.json {
        let list: Vec<serde_json::Value> = runs
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let against = (i > 0).then(|| compare(&runs[i - 1].side, &r.side));
                serde_json::json!({
                    "file": r.run.path.display().to_string(),
                    "when": stamp(r.run.when),
                    "report": r.side,
                    "since_previous": against.map(|c| serde_json::json!({
                        "verdict": c.verdict,
                        "regression": c.regression,
                        "message": c.badge.message,
                    })),
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({ "test": name, "runs": list }))?
        );
        return Ok(());
    }
    let mut out = format!(
        "pepe history · {name} · {} run{}\n{:<20}  {:>9}  {:>7}  {:>8}  {:>8}  {:>7}  SINCE PREVIOUS\n",
        runs.len(),
        if runs.len() == 1 { "" } else { "s" },
        "WHEN",
        "REQUESTS",
        "RPS",
        "P50",
        "P99",
        "FAILED",
    );
    for (i, r) in runs.iter().enumerate() {
        let since = if i == 0 {
            "first run".to_string()
        } else {
            let c = compare(&runs[i - 1].side, &r.side);
            since_words(&c)
        };
        out += &format!(
            "{:<20}  {:>9}  {:>7}  {:>8}  {:>8}  {:>6.1}%  {since}\n",
            when(r.run.when),
            format::count(r.side.requests),
            r.side
                .requests_per_second
                .map_or("-".to_string(), format::compact),
            r.side.median_ms.map_or("-".to_string(), ms),
            r.side.p99_ms.map_or("-".to_string(), ms),
            r.side.failed_percent,
        );
    }
    print!("{out}");
    Ok(())
}

/// A run's numbers in a few words: `319 rps · p99 645ms · 0.0% failed`
fn numbers(side: &Side) -> String {
    let mut parts = Vec::new();
    if let Some(rps) = side.requests_per_second {
        parts.push(format!("{} rps", format::compact(rps)));
    }
    if let Some(capacity) = side.capacity_per_second {
        parts.push(format!("capacity {} rps", format::compact(capacity)));
    }
    if let Some(p99) = side.p99_ms {
        parts.push(format!("p99 {}", ms(p99)));
    }
    parts.push(format!("{:.1}% failed", side.failed_percent));
    parts.join(" · ")
}

fn ms(v: f64) -> String {
    format::ms(v)
}

fn when(at: SystemTime) -> String {
    format::local_stamp(at)
}

/// How long ago, in a reader's words
fn ago(at: SystemTime) -> String {
    crate::ui::ago(at, SystemTime::now())
}

/// `slower · p99 +38%`, or just `about the same`
pub(crate) fn since_words(c: &crate::compare::Comparison) -> String {
    let headline = c.verdict.headline().to_lowercase();
    if c.badge.message == headline {
        headline
    } else {
        format!("{headline} · {}", c.badge.message)
    }
}
