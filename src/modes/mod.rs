//! The modes: one file per subcommand, each running its mode to the end
//! (with the screens, headless, or as `--json`) and leaving what it leaves
//! in the shell. What they share is here: starting the load, the report,
//! the verdict, `--dry-run`, `--fail-if`, and the update notice.

pub mod api;
pub mod compare;
pub mod flow;
pub mod history;
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
use crate::store::{self, Store};
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
    let (load, connects) = start_requests(
        args,
        args.concurrency as usize,
        args.concurrency as usize,
        plan(args),
        previews,
    )?;
    if let Some(warmup) = args.warmup() {
        load.set_warmup(warmup);
    }
    load.set_rate(args.rate);
    Ok((load, connects))
}

/// The load of a run or a ramp: its request, or with `--vars` one for
/// each row, sent in turn. `peak` is the most concurrency it will reach.
pub(crate) fn start_requests(
    args: &Cli,
    peak: usize,
    concurrency: usize,
    plan: Plan,
    previews: bool,
) -> Result<(LoadHandle, Arc<request::ConnectTimes>), PepeError> {
    let Some((vars, fill)) = args.rows()? else {
        let request = args.request()?;
        let (clients, connects) = clients_for(&request, args, peak)?;
        let load = load::start(clients, request, concurrency, plan, previews);
        return Ok((load, connects));
    };
    // The clients' shared headers are the first row's; each row keeps
    // only the ones with holes, which win over those name by name. A row
    // is a request kept ready, so it holds no more than it must.
    let (clients, connects) = clients_for(&args.row_request(&vars, &fill, 0)?, args, peak)?;
    let filled = fill.filled_headers();
    // Sized once: grown by doubling, a million rows would be there twice
    let mut targets = Vec::with_capacity(vars.rows.len());
    for index in 0..vars.rows.len() {
        let mut request = args.row_request(&vars, &fill, index)?;
        let mut headers = reqwest::header::HeaderMap::new();
        for (name, value) in std::mem::take(&mut request.headers).iter() {
            if filled.iter().any(|n| name.as_str().eq_ignore_ascii_case(n)) {
                headers.append(name, value.clone());
            }
        }
        targets.push(load::Target {
            headers,
            request,
            endpoint: 0,
            weight: 1,
        });
    }
    drop(vars);
    let load = load::start_targets(clients, targets, concurrency, plan, previews);
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

/// `--otlp`, after the text report: the breakdown and what it says
pub(crate) fn print_spans(spans: &crate::otlp::Breakdown, metrics: &Metrics) {
    let mut text = spans.text();
    for note in spans.notes(metrics.percentile(50.0)) {
        text.push_str(&format!("  · {}\n", note.text));
    }
    print_report(&text);
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
    /// The end-of-run report as JSON, for `--record`, unless the screen
    /// recorded it already
    result: Option<String>,
    /// `--otlp`: what to ask of the spans
    spans: Option<crate::otlp::Query>,
}

/// `--record` asks for a saved test to record under, and a run or a ramp
pub(crate) fn check_record(args: &Cli) -> Result<(), String> {
    match &args.command {
        None | Some(cli::Command::Ramp(_)) | Some(cli::Command::Ping(_)) => {}
        Some(_) => {
            return Err(
                "--record keeps a run's, a ramp's or a ping's report, not this mode's".into(),
            )
        }
    }
    if store::test_of(args).is_none() {
        return Err(
            "--record needs a saved test to record under: run it with --load NAME, or add --keep NAME"
                .into(),
        );
    }
    Ok(())
}

/// `--record`: the run's report into the store under its test, said on
/// stderr. With `pepe rerun`, what moved since the run before, when there
/// was one.
pub(crate) fn record_result(
    args: &Cli,
    json: &str,
) -> Result<Option<crate::compare::Comparison>, Box<dyn std::error::Error>> {
    let Some(name) = store::test_of(args) else {
        return Ok(None);
    };
    let store = Store::local();
    let previous = store.last_run(&name);
    let path = store.record(&name, json)?;
    eprintln!("recorded {} · pepe history {name}", path.display());
    if args.rerun.is_none() {
        return Ok(None);
    }
    let Some(previous) = previous else {
        eprintln!("the first recorded run of `{name}`: nothing to compare with yet");
        return Ok(None);
    };
    let before = crate::compare::Side::read(&previous.path)?;
    let after = crate::compare::Side::read(&path)?;
    Ok(Some(crate::compare::compare(&before, &after)))
}

/// After a `pepe rerun`: the comparison as text (JSON took the report's
/// place already), and exit 1 under `--gate` on a regression
pub(crate) fn finish_rerun(args: &Cli, comparison: Option<&crate::compare::Comparison>) {
    let Some(comparison) = comparison else { return };
    if !args.json {
        print_report(&comparison.report());
    }
    if args.rerun.as_ref().is_some_and(|r| r.gate) && comparison.regression {
        std::process::exit(1);
    }
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
    let spans = match dashboard.spans() {
        Some(query) => query.answer().await,
        None => None,
    };
    if let Some(report) = dashboard.report() {
        print_report(&report);
    }
    if let Some(spans) = &spans {
        print_spans(spans, dashboard.metrics());
    }
    let comparison = if args.record {
        let mut json = dashboard.report_json(&load);
        if let Some(spans) = &spans {
            json = json.with_spans(spans);
        }
        record_result(args, &json.to_json()?)?
    } else {
        None
    };
    say_if_newer(check).await;
    exit_if_failed(args, dashboard.metrics());
    finish_rerun(args, comparison.as_ref());
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
        let comparison = if args.record {
            let mut report = crate::ramp::json(&ramp);
            store::stamp_test(&mut report, args);
            record_result(args, &serde_json::to_string_pretty(&report)?)?
        } else {
            None
        };
        say_if_newer(check).await;
        exit_if_failed(args, &ramp.total);
        finish_rerun(args, comparison.as_ref());
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A server that answers every request on a kept connection and
    /// keeps each one's head
    async fn heads() -> (String, Arc<std::sync::Mutex<Vec<String>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let kept = seen.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let kept = kept.clone();
                tokio::spawn(async move {
                    let mut raw = Vec::new();
                    let mut buf = [0u8; 4096];
                    loop {
                        let Ok(n) = sock.read(&mut buf).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        raw.extend_from_slice(&buf[..n]);
                        while let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
                            let head = String::from_utf8_lossy(&raw[..end]).into_owned();
                            let length = head
                                .to_lowercase()
                                .lines()
                                .find_map(|l| {
                                    l.strip_prefix("content-length: ").map(str::to_string)
                                })
                                .map_or(0, |v| v.trim().parse().unwrap());
                            if raw.len() < end + 4 + length {
                                break;
                            }
                            let body = String::from_utf8_lossy(&raw[end + 4..end + 4 + length]);
                            kept.lock().unwrap().push(format!("{head}\r\n\r\n{body}"));
                            raw.drain(..end + 4 + length);
                            let _ = sock
                                .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n")
                                .await;
                        }
                    }
                });
            }
        });
        (url, seen)
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn each_request_takes_the_next_row() {
        let (url, seen) = heads().await;
        let dir = std::env::temp_dir().join(format!("pepe-vars-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("users.csv");
        std::fs::write(&file, "id,token\n1,aaa\n2,bbb\n3,ccc\n").unwrap();
        let mut args = Cli::parse_from([
            "pepe",
            "-n",
            "4",
            "-c",
            "1",
            "-m",
            "POST",
            "-H",
            "Authorization: Bearer {{token}}",
            "-H",
            "X-Static: kept",
            "-d",
            "id={{id}}",
            "--vars",
            file.to_str().unwrap(),
            &format!("{url}/users/{{{{id}}}}"),
        ]);
        args.validate().unwrap();
        let (mut load, _) = start_requests(&args, 1, 1, plan(&args), false).unwrap();
        while load.recv().await.is_some() {}
        let seen = seen.lock().unwrap().clone();
        assert_eq!(seen.len(), 4, "{seen:?}");
        for (head, (id, token)) in
            seen.iter()
                .zip([("1", "aaa"), ("2", "bbb"), ("3", "ccc"), ("1", "aaa")])
        {
            let lower = head.to_lowercase();
            assert!(
                head.starts_with(&format!("POST /users/{id} HTTP/1.1")),
                "{head}"
            );
            assert!(
                lower.contains(&format!("authorization: bearer {token}")),
                "{head}"
            );
            assert!(lower.contains("x-static: kept"), "{head}");
            assert!(head.ends_with(&format!("id={id}")), "{head}");
        }
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn vars_are_checked_before_anything_is_sent() {
        let dir = std::env::temp_dir().join(format!("pepe-vars-check-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("ids.csv");
        std::fs::write(&file, "id\n1\n").unwrap();
        let file = file.to_str().unwrap();
        let check = |argv: &[&str]| {
            let mut args = Cli::parse_from(argv);
            if let Some(cli::Command::Ramp(ramp)) = &args.command {
                args.url = ramp.url.clone();
            }
            args.validate().map_err(|e| e.to_string())
        };
        let n = ["pepe", "-n", "2", "-c", "1", "--vars", file];
        assert!(check(&[&n[..], &["https://x.io/{{id}}"]].concat()).is_ok());
        let missing = check(&[&n[..], &["https://x.io/{{uid}}"]].concat()).unwrap_err();
        assert!(missing.contains("{{uid}} isn't a column"), "{missing}");
        let none = check(&[&n[..], &["https://x.io/"]].concat()).unwrap_err();
        assert!(none.contains("no {{holes}}"), "{none}");
        // The host itself may be a value
        assert!(check(&[&n[..], &["https://{{id}}.x.io/"]].concat()).is_ok());
        assert!(check(&["pepe", "ramp", "--vars", file, "https://x.io/{{id}}"]).is_ok());
        let api = check(&["pepe", "api", "spec.yaml", "--vars", file]).unwrap_err();
        assert!(api.contains("a run, a ramp or a flow"), "{api}");
        let _ = std::fs::remove_dir_all(dir);
    }
}
