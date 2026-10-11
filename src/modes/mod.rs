//! The modes: one file per subcommand, each running its mode to the end
//! (with the screens, headless, or as `--json`) and leaving what it leaves
//! in the shell. What they share is here: starting the load, the report,
//! the verdict, `--dry-run`, `--fail-if`, and the update notice.

pub mod api;
pub mod compare;
pub mod flow;
pub mod logs;
pub mod ping;
pub mod ramp;
pub mod replay;
pub mod run;

use std::io::{stderr, stdin, stdout, IsTerminal};
use std::sync::Arc;
use std::time::Instant;

use crate::cli::{self, Cli};
use crate::load::{LoadHandle, Plan};
use crate::metrics::Metrics;
use crate::ramp::{Ramp, RampPlan, Tick};
use crate::terminal::{CtrlCWatchdog, TerminalGuard};
use crate::{
    diagnose, exporter, guard, insights, json_report, load, metrics, request, timeline, ui, update,
    PepeError, SNAPSHOT_EVERY,
};

pub(crate) fn plan(args: &Cli) -> Plan {
    match args.run_duration() {
        Some(duration) => Plan::Duration(duration),
        None => Plan::Count(args.number as u64),
    }
}

/// Shard threads for a run: what `--threads` asked for, or one
pub(crate) fn shards(args: &Cli, concurrency: usize) -> load::Threads {
    load::shards(concurrency, args.threads)
}

/// One client per load shard, and where their connection times go
pub(crate) fn clients_for(
    request: &request::Request,
    args: &Cli,
    concurrency: usize,
) -> Result<(load::Senders, Arc<request::ConnectTimes>), PepeError> {
    request.build_clients(shards(args, concurrency))
}

/// `previews`: keep the start of each body, which only the dashboard shows
pub(crate) fn start_load(
    args: &Cli,
    previews: bool,
) -> Result<(LoadHandle, Arc<request::ConnectTimes>), PepeError> {
    let request = args.request()?;
    let (clients, connects) = clients_for(&request, args, args.concurrency as usize)?;
    let load = load::start(
        clients,
        request,
        args.concurrency as usize,
        plan(args),
        previews,
    );
    if let Some(warmup) = args.warmup() {
        load.set_warmup(warmup);
    }
    load.set_rate(args.rate);
    Ok((load, connects))
}

/// How often the `--json` modes collect results (see `LoadHandle::drain`)
pub(crate) const PUMP: std::time::Duration = std::time::Duration::from_millis(25);

/// The report left in the shell, with the verdict in its colours when the
/// shell shows them
pub(crate) fn print_report(report: &str) {
    let color = stdout().is_terminal() && !ui::theme::no_color();
    print!("{}", ui::theme::color_report(report, color));
}

/// After the report: Pepe mentions a newer release, if the look that
/// started with the run found one
pub(crate) async fn say_if_newer(check: update::Check) {
    if let Some(latest) = check.finish().await {
        eprint!(
            "{}",
            update::notice(&latest, stderr().is_terminal() && !ui::theme::no_color())
        );
    }
}

/// What to print once the terminal is back to normal
#[derive(Default)]
pub(crate) struct Farewell {
    /// The end-of-run report
    report: Option<String>,
    /// What the run measured, for `--fail-if`
    metrics: Option<Metrics>,
    /// The command that reproduces settings chosen on the setup screen
    command: Option<String>,
}

/// A ramp's load has no end of its own: the ramp stops it
pub(crate) const UNTIL_STOPPED: std::time::Duration =
    std::time::Duration::from_secs(365 * 24 * 60 * 60);

/// What the dashboard would have said of a run, for its JSON report:
/// the findings, with where the time went
pub(crate) fn verdict_of(
    args: &Cli,
    metrics: &Metrics,
    samples: &[timeline::Sample],
    interrupted: bool,
    connects: &request::ConnectTimes,
    concurrency: usize,
) -> insights::Verdict {
    let mut verdict = insights::verdict(metrics, samples, interrupted);
    let hist = connects.histogram();
    for note in insights::phase_notes(metrics, Some(&hist), !args.disable_keepalive, concurrency) {
        verdict.level = verdict.level.max(note.level);
        verdict.notes.push(note);
    }
    verdict
}

/// `--dry-run`: the plan, as text or as JSON, and nothing sent
pub(crate) fn print_plan(args: &Cli, plan: &guard::Plan) -> Result<(), Box<dyn std::error::Error>> {
    if args.json {
        println!("{}", serde_json::to_string_pretty(&plan.json())?);
    } else {
        print!("{}", plan.text());
    }
    Ok(())
}

/// No terminal to draw on, and no `--json`: the run goes to its end and
/// the report the dashboard would have left is printed, so a forgotten
/// flag in a script costs nothing
pub(crate) fn headless(args: &Cli) -> bool {
    !(args.json || (stdin().is_terminal() && stdout().is_terminal()))
}

/// A dashboard mode without the dashboard: to the end, then the report
pub(crate) async fn headless_dashboard(
    args: &Cli,
    mut dashboard: ui::Dashboard,
    mut load: LoadHandle,
) -> Result<(), Box<dyn std::error::Error>> {
    let check = update::Check::start();
    dashboard.run_headless(&mut load).await?;
    if let Some(report) = dashboard.report() {
        print_report(&report);
    }
    say_if_newer(check).await;
    exit_if_failed(args, dashboard.metrics());
    Ok(())
}

/// A plain run or a ramp with no terminal
pub(crate) async fn run_headless(
    args: &Cli,
    ramp: Option<cli::RampArgs>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(ramp) = ramp {
        let plan = match RampPlan::from_args(&ramp) {
            Ok(plan) => plan,
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        };
        let check = update::Check::start();
        let (ramp, _load) = ramp::climb(args, plan).await?;
        print_report(&crate::ramp::report(args, &ramp));
        say_if_newer(check).await;
        exit_if_failed(args, &ramp.total);
        return Ok(());
    }
    let (load, connects) = start_load(args, false)?;
    let dashboard = ui::Dashboard::new(args.clone(), plan(args)).with_connects(connects);
    headless_dashboard(args, dashboard, load).await
}

/// `--fail-if`: say which conditions the run crossed and end with exit
/// code 4; nothing happens when none was, or none was given
pub(crate) fn exit_if_failed(args: &Cli, metrics: &Metrics) {
    let mut failed = false;
    for text in &args.fail_if {
        let Ok(condition) = crate::ramp::Condition::parse(text) else {
            continue;
        };
        if !condition.crossed(metrics) {
            continue;
        }
        failed = true;
        let was = match condition.measure {
            crate::ramp::Measure::Latency(p) => {
                format!("p{p} was {}", ui::format::latency(metrics.percentile(p)))
            }
            crate::ramp::Measure::Errors => format!("errors were {:.1}%", metrics.error_rate()),
        };
        eprintln!("✖ --fail-if {}: {was}", condition.text);
    }
    if failed {
        std::process::exit(4);
    }
}
