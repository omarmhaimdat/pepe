use std::io::{stderr, stdin, stdout, IsTerminal};
use std::sync::Arc;
use std::time::Instant;

use clap::{CommandFactory, FromArgMatches};
use crossterm::{
    cursor::{Hide, Show},
    event::{DisableBracketedPaste, EnableBracketedPaste},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::cli::Cli;
use crate::load::{LoadHandle, Plan};
use crate::metrics::Metrics;
use crate::ramp::{Ramp, RampPlan, Tick};

mod api;
mod cache;
mod cert;
mod cli;
mod compare;
mod completions;
mod config;
mod contrib;
mod curl;
mod diagnose;
mod direct;
mod exporter;
mod flow;
mod insights;
mod json_report;
mod load;
mod logs;
mod metrics;
mod openapi;
mod ping;
mod ramp;
mod replay;
mod request;
mod response;
mod timeline;
mod trace;
mod ui;
mod update;
mod utils;
mod wire;

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
enum PepeError {
    HeaderParseError(String),
    IoError(std::io::Error),
    RequestError(reqwest::Error),
    InvalidUrl(String),
}

impl std::fmt::Display for PepeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderParseError(msg) => write!(f, "Header parse error: {}", msg),
            Self::RequestError(e) => write!(f, "Request error: {}", e),
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::InvalidUrl(e) => write!(f, "Invalid URL {}", e),
        }
    }
}

impl std::error::Error for PepeError {}

fn plan(args: &Cli) -> Plan {
    match args.run_duration() {
        Some(duration) => Plan::Duration(duration),
        None => Plan::Count(args.number as u64),
    }
}

/// Shard threads for a run: what `--threads` asked for, or one
fn shards(args: &Cli, concurrency: usize) -> load::Threads {
    load::shards(concurrency, args.threads)
}

/// One client per load shard, and where their connection times go
fn clients_for(
    request: &request::Request,
    args: &Cli,
    concurrency: usize,
) -> Result<(load::Senders, Arc<request::ConnectTimes>), PepeError> {
    request.build_clients(shards(args, concurrency))
}

/// `previews`: keep the start of each body, which only the dashboard shows
fn start_load(
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

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen, Show);
}

/// Puts the terminal into dashboard mode and restores it when dropped, so
/// every exit path (including `?` errors) leaves a usable shell behind
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> std::io::Result<Self> {
        enable_raw_mode()?;
        #[cfg(unix)]
        keep_ctrl_c_a_signal();
        // Bracketed paste: a pasted curl command arrives whole, not as keys
        execute!(stdout(), EnterAlternateScreen, Hide, EnableBracketedPaste)?;
        Ok(Self)
    }
}

/// Raw mode turns Ctrl-C into an ordinary key, which only gets through while
/// the dashboard is reading keys. Keep it a signal instead, so it works even
/// when the dashboard is stuck (see `CtrlCWatchdog`). Ctrl-Z and Ctrl-\ stay
/// plain keys: suspending mid-dashboard would leave the terminal in raw mode.
#[cfg(unix)]
fn keep_ctrl_c_a_signal() {
    use std::os::fd::AsRawFd;
    let fd = std::io::stdin().as_raw_fd();
    // SAFETY: termios is plain data, filled in by tcgetattr before use
    unsafe {
        if libc::isatty(fd) != 1 {
            return;
        }
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return;
        }
        let disabled = libc::fpathconf(fd, libc::_PC_VDISABLE);
        let disabled = if disabled < 0 {
            0
        } else {
            disabled as libc::cc_t
        };
        t.c_lflag |= libc::ISIG;
        t.c_cc[libc::VSUSP] = disabled;
        t.c_cc[libc::VQUIT] = disabled;
        libc::tcsetattr(fd, libc::TCSANOW, &t);
    }
}

/// If the dashboard hasn't quit a second after Ctrl-C (for example because
/// it's blocked writing to a terminal that stopped reading), restore the
/// terminal and exit from here instead. Disarmed when dropped.
struct CtrlCWatchdog(tokio::task::JoinHandle<()>);

impl CtrlCWatchdog {
    fn arm() -> Self {
        Self(tokio::spawn(async {
            if tokio::signal::ctrl_c().await.is_err() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            force_exit();
        }))
    }
}

/// Exit now, even if the dashboard is blocked writing to a terminal that
/// stopped reading
fn force_exit() -> ! {
    use std::io::Write;
    use std::time::Duration;

    let _ = disable_raw_mode();
    // Drop the frames the terminal hasn't read, so the reset below is next
    #[cfg(unix)]
    discard_pending_output();
    // Leave the alternate screen; from a thread, since the write may block
    std::thread::spawn(|| {
        let _ = std::io::stderr().write_all(b"\x1b[?1049l\x1b[?25h");
    });
    std::thread::sleep(Duration::from_millis(200));
    std::process::exit(130);
}

/// Throw away output queued for the terminal but not yet read by it
#[cfg(unix)]
fn discard_pending_output() {
    // SAFETY: tcflush only takes a file descriptor and a flag
    unsafe {
        libc::tcflush(libc::STDIN_FILENO, libc::TCOFLUSH);
    }
}

impl Drop for CtrlCWatchdog {
    fn drop(&mut self) {
        self.0.abort();
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// How often the `--json` modes collect results (see `LoadHandle::drain`)
const PUMP: std::time::Duration = std::time::Duration::from_millis(25);
/// How often `--snapshot` writes the report so far
pub const SNAPSHOT_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

/// `--json`: no dashboard; run to completion (or Ctrl-C), print the report
async fn run_json(args: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let (mut load, connects) = start_load(args, false)?;
    // The clock starts when the warm-up, if any, is over
    let mut started = Instant::now();
    let mut warming = load.warming();
    let mut warmup_requests = 0;
    let mut metrics = Metrics::default();
    let mut timeline = timeline::Timeline::default();
    let mut slowest = metrics::Slowest::default();
    let mut watch = insights::Watch::default();
    let mut anomalies: Vec<insights::Anomaly> = Vec::new();
    let mut interrupted = false;
    let mut pump = tokio::time::interval(PUMP);
    let mut snapshots = tokio::time::interval(SNAPSHOT_EVERY);
    snapshots.tick().await; // the first tick is now; the first snapshot is in a minute
    let report = |metrics: &Metrics,
                  timeline: &timeline::Timeline,
                  slowest: &metrics::Slowest,
                  anomalies: &[insights::Anomaly],
                  load: &LoadHandle,
                  elapsed: std::time::Duration,
                  warmup_requests: u64,
                  interrupted| {
        json_report::JsonReport::generate(metrics, elapsed, interrupted)
            .with_generator(
                load.threads(),
                load.peak_busy(),
                load.rate().map(|r| (r, load.missed())),
            )
            .with_target("run", Some(&args.method), &args.url, args.concurrency)
            .with_warmup(args.warmup(), warmup_requests)
            .with_timeline(timeline)
            .with_slowest(slowest)
            .with_anomalies(anomalies)
            .with_connects(&connects)
    };

    loop {
        tokio::select! {
            _ = pump.tick() => {
                if warming && !load.warming() {
                    warming = false;
                    started = Instant::now();
                }
                let now = started.elapsed();
                let over = !load.drain(|stat| {
                    if stat.warmup {
                        warmup_requests += 1;
                        return;
                    }
                    metrics.record(&stat);
                    timeline.record(&stat);
                    slowest.record(&stat, now);
                });
                if !warming {
                    timeline.advance(started.elapsed());
                    anomalies.extend(watch.observe(timeline.samples(), false));
                }
                exporter::publish(|| {
                    exporter::run_page(
                        &args.target_label(),
                        &metrics,
                        &timeline,
                        started.elapsed(),
                        args.concurrency as usize,
                        &[],
                    )
                });
                if over {
                    break;
                }
            }
            _ = snapshots.tick(), if args.snapshot.is_some() => {
                let path = args.snapshot.as_ref().expect("checked");
                if let Err(e) = report(&metrics, &timeline, &slowest, &anomalies, &load, started.elapsed(), warmup_requests, interrupted).with_snapshot(true).write_to(path) {
                    eprintln!("couldn't write the snapshot to {}: {e}", path.display());
                }
            }
            _ = tokio::signal::ctrl_c(), if !interrupted => {
                interrupted = true;
                load.stop();
            }
        }
    }

    timeline.finish(started.elapsed());
    let report = report(
        &metrics,
        &timeline,
        &slowest,
        &anomalies,
        &load,
        started.elapsed(),
        warmup_requests,
        interrupted,
    );
    if let Some(path) = &args.snapshot {
        report.clone().with_snapshot(false).write_to(path)?;
    }
    println!("{}", report.to_json()?);
    Ok(())
}

/// The report left in the shell, with the verdict in its colours when the
/// shell shows them
fn print_report(report: &str) {
    let color = stdout().is_terminal() && !ui::theme::no_color();
    print!("{}", ui::theme::color_report(report, color));
}

/// After the report: Pepe mentions a newer release, if the look that
/// started with the run found one
async fn say_if_newer(check: update::Check) {
    if let Some(latest) = check.finish().await {
        eprint!(
            "{}",
            update::notice(&latest, stderr().is_terminal() && !ui::theme::no_color())
        );
    }
}

/// What to print once the terminal is back to normal
#[derive(Default)]
struct Farewell {
    /// The end-of-run report
    report: Option<String>,
    /// The command that reproduces settings chosen on the setup screen
    command: Option<String>,
}

/// A ramp's load has no end of its own: the ramp stops it
const UNTIL_STOPPED: std::time::Duration = std::time::Duration::from_secs(365 * 24 * 60 * 60);

/// Runs the setup screen (when asked for, or when there's nothing to run
/// yet) and then the screen of the mode it chose, until the user quits
async fn run_interactive(
    args: &Cli,
    mut setup: bool,
) -> Result<Farewell, Box<dyn std::error::Error>> {
    // Declared first so it's dropped last: restoring the terminal writes to
    // it, which can block too, and Ctrl-C must still get out then
    let _watchdog = CtrlCWatchdog::arm();
    let _terminal = TerminalGuard::enter()?;
    let mut args = args.clone();
    let mut farewell = Farewell::default();
    // Why the last start didn't happen, for the setup screen to say
    let mut error: Option<String> = None;
    loop {
        if setup {
            match ui::Setup::new(&args).with_error(error.take()).run().await? {
                ui::SetupOutcome::Start(chosen) => {
                    args = *chosen;
                    farewell.command = Some(args.command_line());
                }
                ui::SetupOutcome::Quit => return Ok(farewell),
            }
            setup = false;
        }
        match args.command.clone() {
            Some(cli::Command::Ramp(ramp)) => {
                let plan = match RampPlan::from_args(&ramp) {
                    Ok(plan) => plan,
                    Err(e) => {
                        error = Some(e);
                        setup = true;
                        continue;
                    }
                };
                let request = args.request()?;
                let mut load = load::start(
                    clients_for(&request, &args, plan.peak() as usize)?.0,
                    request,
                    plan.levels[0] as usize,
                    Plan::Duration(UNTIL_STOPPED),
                    false,
                );
                let mut screen = ui::RampScreen::new(args.clone(), plan);
                match screen.run(&mut load).await? {
                    ui::Outcome::Restart => continue,
                    ui::Outcome::Edit => setup = true,
                    ui::Outcome::Quit => {
                        farewell.report = screen.report();
                        return Ok(farewell);
                    }
                }
            }
            Some(cli::Command::Api(api)) => {
                let mut run = match api::ApiRun::load(&api).await {
                    Ok(run) => run,
                    Err(e) => {
                        error = Some(e);
                        setup = true;
                        continue;
                    }
                };
                farewell.report = api_session(&args, &mut run).await?;
                return Ok(farewell);
            }
            _ => {
                let (mut load, connects) = start_load(&args, true)?;
                let mut dashboard =
                    ui::Dashboard::new(args.clone(), plan(&args)).with_connects(connects);
                let outcome = dashboard.run(&mut load).await?;
                // Keep any concurrency the user dialed in during the run.
                // Dropping `load` stops the previous run before the next starts.
                args.concurrency = load.concurrency() as u32;
                match outcome {
                    ui::Outcome::Restart => continue,
                    ui::Outcome::Edit => setup = true,
                    ui::Outcome::Quit => {
                        farewell.report = dashboard.report();
                        return Ok(farewell);
                    }
                }
            }
        }
    }
}

/// Ramp mode with `--json`: climb the steps, print what each measured
async fn run_ramp_json(args: &Cli, plan: RampPlan) -> Result<(), Box<dyn std::error::Error>> {
    let request = args.request()?;
    let mut load = load::start(
        clients_for(&request, args, plan.peak() as usize)?.0,
        request,
        plan.levels[0] as usize,
        Plan::Duration(UNTIL_STOPPED),
        false,
    );
    let mut ramp = Ramp::new(plan, Instant::now());
    let mut clock = tokio::time::interval(std::time::Duration::from_millis(50));
    let mut pump = tokio::time::interval(PUMP);
    while ramp.end.is_none() {
        tokio::select! {
            _ = pump.tick() => {
                let now = Instant::now();
                if !load.drain(|stat| ramp.record(&stat, now)) {
                    break;
                }
            }
            _ = clock.tick() => {
                if let Tick::Level(level) = ramp.tick(Instant::now()) {
                    load.set_concurrency(level as usize);
                }
            }
            _ = tokio::signal::ctrl_c() => {
                ramp.stop(Instant::now());
            }
        }
    }
    load.stop();
    let mut report = ramp::json(&ramp);
    report["generator"] = serde_json::to_value(json_report::Generator {
        threads: load.threads(),
        peak_busy_percent: load.peak_busy(),
        warmup_s: None,
        warmup_requests: None,
        rate_per_second: None,
        rate_missed: None,
    })?;
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// API mode with `--json`: run the endpoints that are on, print one report
async fn run_api_json(args: &Cli, run: &api::ApiRun) -> Result<(), Box<dyn std::error::Error>> {
    // Nobody to prompt here: say what's missing instead
    if let (Some(scheme), true) = (run.spec.auth.first(), run.credentials.is_empty()) {
        eprintln!(
            "note: this API declares {} and no credentials were given; pass {}",
            scheme.describe(),
            scheme.hint()
        );
    }
    let which = run.enabled();
    if which.is_empty() {
        let tags: Vec<&str> = run
            .spec
            .tags
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        eprintln!(
            "error: no endpoint to run. Pick some with --all, --tag NAME or --only PATTERN \
             (tags: {}); --set gives parameters their values, --include-writes allows writes",
            tags.join(", ")
        );
        std::process::exit(1);
    }
    let targets = run.targets(args, &which)?;
    let row_labels: Vec<String> = which
        .iter()
        .map(|&index| run.endpoints[index].label.clone())
        .collect();
    let timeline = timeline::Timeline::default();
    let (clients, connects) = run.clients(args, shards(args, args.concurrency as usize))?;
    let mut load = load::start_targets(
        clients,
        targets,
        args.concurrency as usize,
        plan(args),
        false,
    );
    load.set_rate(args.rate);
    if let Some(warmup) = args.warmup() {
        load.set_warmup(warmup);
    }
    let mut started = Instant::now();
    let mut warming = load.warming();
    let mut warmup_requests = 0;
    let mut total = Metrics::default();
    let mut each = vec![Metrics::default(); which.len()];
    let mut interrupted = false;
    let mut pump = tokio::time::interval(PUMP);
    loop {
        tokio::select! {
            _ = pump.tick() => {
                if warming && !load.warming() {
                    warming = false;
                    started = Instant::now();
                }
                let over = !load.drain(|stat| {
                    if stat.warmup {
                        warmup_requests += 1;
                        return;
                    }
                    total.record(&stat);
                    if let Some(metrics) = each.get_mut(stat.endpoint as usize) {
                        metrics.record(&stat);
                    }
                });
                exporter::publish(|| {
                    let rows: Vec<(String, &Metrics)> =
                        row_labels.iter().cloned().zip(each.iter()).collect();
                    exporter::run_page(
                        &args.target_label(),
                        &total,
                        &timeline,
                        started.elapsed(),
                        args.concurrency as usize,
                        &rows,
                    )
                });
                if over {
                    break;
                }
            }
            _ = tokio::signal::ctrl_c(), if !interrupted => {
                interrupted = true;
                load.stop();
            }
        }
    }
    let elapsed = started.elapsed();
    let report = json_report::JsonReport::generate(&total, elapsed, interrupted)
        .with_generator(
            load.threads(),
            load.peak_busy(),
            load.rate().map(|r| (r, load.missed())),
        )
        .with_target("api", None, &run.spec.base_url, args.concurrency)
        .with_warmup(args.warmup(), warmup_requests)
        .with_connects(&connects);
    let mut report = serde_json::to_value(&report)?;
    let ms = |d: std::time::Duration| (d.as_secs_f64() * 1_000_000.0).round() / 1000.0;
    let endpoints: Vec<serde_json::Value> = which
        .iter()
        .zip(&each)
        .map(|(&index, m)| {
            serde_json::json!({
                "endpoint": run.endpoints[index].label,
                "requests": m.total,
                "failed_requests": m.total - m.success,
                "requests_per_second": m.rps(elapsed),
                "median_ms": ms(m.percentile(50.0)),
                "p99_ms": ms(m.percentile(99.0)),
                "status_codes": m.status_codes.iter().map(|(k, v)| (k.to_string(), *v)).collect::<std::collections::BTreeMap<_, _>>(),
            })
        })
        .collect();
    report["endpoints"] = serde_json::Value::Array(endpoints);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// The plan screen, then the dashboard with one row per endpoint, and
/// back, until the user quits. Returns the report of the last run. The
/// terminal is already the dashboard's.
async fn api_session(
    args: &Cli,
    run: &mut api::ApiRun,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    // The dashboard's title shows the API rather than one URL
    let mut shown = args.clone();
    shown.method = "API".into();
    shown.url = run.spec.base_url.clone();
    let mut planning = true;
    loop {
        if planning {
            match ui::PlanScreen::new(run, &mut shown).run().await? {
                ui::PlanOutcome::Start => planning = false,
                ui::PlanOutcome::Quit => return Ok(None),
            }
            shown.url = run.spec.base_url.clone();
        }
        let which = run.enabled();
        let targets = run.targets(&shown, &which)?;
        let (clients, connects) =
            run.clients(&shown, shards(&shown, shown.concurrency as usize))?;
        let mut load = load::start_targets(
            clients,
            targets,
            shown.concurrency as usize,
            plan(&shown),
            true,
        );
        if let Some(warmup) = shown.warmup() {
            load.set_warmup(warmup);
        }
        load.set_rate(shown.rate);
        let mut dashboard = ui::Dashboard::new(shown.clone(), plan(&shown))
            .with_endpoints(run.views(&shown, &which))
            .with_connects(connects);
        let outcome = dashboard.run(&mut load).await?;
        shown.concurrency = load.concurrency() as u32;
        match outcome {
            ui::Outcome::Restart => {}
            ui::Outcome::Edit => planning = true,
            ui::Outcome::Quit => return Ok(dashboard.report()),
        }
    }
}

/// API mode: read the spec, then the plan screen and the dashboard
/// `pepe flow FILE`: the steps in order, each worker one user
async fn run_flow(args: &Cli, what: &cli::FlowArgs) -> Result<(), Box<dyn std::error::Error>> {
    let flow = match flow::load(&what.file) {
        Ok(flow) => flow,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    // The dashboard's title shows the flow rather than one URL
    let mut shown = args.clone();
    shown.method = "FLOW".into();
    shown.url = flow.name.clone();
    if args.json {
        return run_flow_json(&shown, flow).await;
    }
    let check = update::Check::start();
    let report = {
        let _watchdog = CtrlCWatchdog::arm();
        let _terminal = TerminalGuard::enter()?;
        flow_session(&mut shown, flow).await?
    };
    if let Some(report) = report {
        print_report(&report);
    }
    say_if_newer(check).await;
    Ok(())
}

/// One client per shard, built around the flow's first URL: the shared
/// headers and settings are what matter
fn flow_clients(
    args: &Cli,
    flow: &flow::Flow,
    concurrency: usize,
) -> Result<(load::Senders, std::sync::Arc<request::ConnectTimes>), PepeError> {
    let base = request::Request::new(
        flow.base_url(),
        "GET".into(),
        None,
        &args.headers,
        args.settings(),
    )?;
    base.build_clients(shards(args, concurrency))
}

async fn flow_session(
    shown: &mut Cli,
    flow: flow::Flow,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    loop {
        let (clients, connects) = flow_clients(shown, &flow, shown.concurrency as usize)?;
        let mut load = load::start_flow(
            clients,
            flow.clone(),
            shown.concurrency as usize,
            plan(shown),
            true,
        );
        load.set_rate(shown.rate);
        if let Some(warmup) = shown.warmup() {
            load.set_warmup(warmup);
        }
        let mut dashboard = ui::Dashboard::new(shown.clone(), plan(shown))
            .with_steps(flow.views())
            .with_connects(connects);
        let outcome = dashboard.run(&mut load).await?;
        shown.concurrency = load.concurrency() as u32;
        match outcome {
            // There is no setup screen for a flow: edit the file, run again
            ui::Outcome::Restart | ui::Outcome::Edit => {}
            ui::Outcome::Quit => return Ok(dashboard.report()),
        }
    }
}

async fn run_flow_json(args: &Cli, flow: flow::Flow) -> Result<(), Box<dyn std::error::Error>> {
    let name = flow.name.clone();
    let views = flow.views();
    let row_labels: Vec<String> = views.iter().map(|v| v.label.clone()).collect();
    let timeline = timeline::Timeline::default();
    let (clients, connects) = flow_clients(args, &flow, args.concurrency as usize)?;
    let mut load = load::start_flow(clients, flow, args.concurrency as usize, plan(args), false);
    load.set_rate(args.rate);
    if let Some(warmup) = args.warmup() {
        load.set_warmup(warmup);
    }
    let mut started = Instant::now();
    let mut warming = load.warming();
    let mut warmup_requests = 0;
    let mut total = Metrics::default();
    let mut each = vec![Metrics::default(); views.len()];
    let mut interrupted = false;
    let mut pump = tokio::time::interval(PUMP);
    loop {
        tokio::select! {
            _ = pump.tick() => {
                if warming && !load.warming() {
                    warming = false;
                    started = Instant::now();
                }
                let over = !load.drain(|stat| {
                    if stat.warmup {
                        warmup_requests += 1;
                        return;
                    }
                    total.record(&stat);
                    if let Some(metrics) = each.get_mut(stat.endpoint as usize) {
                        metrics.record(&stat);
                    }
                });
                exporter::publish(|| {
                    let rows: Vec<(String, &Metrics)> =
                        row_labels.iter().cloned().zip(each.iter()).collect();
                    exporter::run_page(
                        &args.target_label(),
                        &total,
                        &timeline,
                        started.elapsed(),
                        args.concurrency as usize,
                        &rows,
                    )
                });
                if over {
                    break;
                }
            }
            _ = tokio::signal::ctrl_c(), if !interrupted => {
                interrupted = true;
                load.stop();
            }
        }
    }
    let elapsed = started.elapsed();
    let report = json_report::JsonReport::generate(&total, elapsed, interrupted)
        .with_generator(
            load.threads(),
            load.peak_busy(),
            load.rate().map(|r| (r, load.missed())),
        )
        .with_target("flow", None, &name, args.concurrency)
        .with_warmup(args.warmup(), warmup_requests)
        .with_connects(&connects);
    let mut report = serde_json::to_value(&report)?;
    let ms = |d: std::time::Duration| (d.as_secs_f64() * 1_000_000.0).round() / 1000.0;
    let steps: Vec<serde_json::Value> = views
        .iter()
        .zip(&each)
        .map(|(view, m)| {
            serde_json::json!({
                "step": view.label,
                "requests": m.total,
                "failed_requests": m.total - m.success,
                "requests_per_second": m.rps(elapsed),
                "median_ms": ms(m.percentile(50.0)),
                "p99_ms": ms(m.percentile(99.0)),
                "status_codes": m.status_codes.iter().map(|(k, v)| (k.to_string(), *v)).collect::<std::collections::BTreeMap<_, _>>(),
            })
        })
        .collect();
    // Chains that went all the way: the last step's successes
    report["flow"] = serde_json::json!({
        "name": args.url,
        "chains_started": each.first().map_or(0, |m| m.total),
        "chains_completed": each.last().map_or(0, |m| m.success),
        "steps": steps,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// `pepe replay LOG`: the log's URLs, weighted as they were seen
async fn run_replay(args: &Cli, what: &cli::ReplayArgs) -> Result<(), Box<dyn std::error::Error>> {
    let replay = match replay::load(&what.log, what.base_url.as_deref(), what.include_writes) {
        Ok(replay) => replay,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    // The dashboard's title shows the log rather than one URL
    let mut shown = args.clone();
    shown.method = "REPLAY".into();
    shown.url = what
        .log
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| what.log.display().to_string());
    if args.json {
        return run_replay_json(&shown, &replay, what).await;
    }
    let check = update::Check::start();
    let report = {
        let _watchdog = CtrlCWatchdog::arm();
        let _terminal = TerminalGuard::enter()?;
        replay_session(&mut shown, &replay, what).await?
    };
    if let Some(report) = report {
        print_report(&report);
    }
    say_if_newer(check).await;
    Ok(())
}

/// One client per shard, built around the most seen URL: the shared
/// headers and settings are what matter
fn replay_clients(
    args: &Cli,
    replay: &replay::Replay,
    concurrency: usize,
) -> Result<(load::Senders, std::sync::Arc<request::ConnectTimes>), PepeError> {
    let base = request::Request::new(
        replay.urls[0].url.clone(),
        "GET".into(),
        None,
        &args.headers,
        args.settings(),
    )?;
    base.build_clients(shards(args, concurrency))
}

async fn replay_session(
    shown: &mut Cli,
    replay: &replay::Replay,
    what: &cli::ReplayArgs,
) -> Result<Option<String>, Box<dyn std::error::Error>> {
    loop {
        let (clients, connects) = replay_clients(shown, replay, shown.concurrency as usize)?;
        let mut load = load::start_targets(
            clients,
            replay.targets(shown, what.rows)?,
            shown.concurrency as usize,
            plan(shown),
            true,
        );
        load.set_rate(shown.rate);
        if let Some(warmup) = shown.warmup() {
            load.set_warmup(warmup);
        }
        let mut dashboard = ui::Dashboard::new(shown.clone(), plan(shown))
            .with_rows(ui::Rows::Urls, replay.views(what.rows))
            .with_connects(connects);
        let outcome = dashboard.run(&mut load).await?;
        shown.concurrency = load.concurrency() as u32;
        match outcome {
            // There is no setup screen for a replay: the log is the setup
            ui::Outcome::Restart | ui::Outcome::Edit => {}
            ui::Outcome::Quit => return Ok(dashboard.report()),
        }
    }
}

async fn run_replay_json(
    args: &Cli,
    replay: &replay::Replay,
    what: &cli::ReplayArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let views = replay.views(what.rows);
    let row_labels: Vec<String> = views.iter().map(|v| v.label.clone()).collect();
    let timeline = timeline::Timeline::default();
    let (clients, connects) = replay_clients(args, replay, args.concurrency as usize)?;
    let mut load = load::start_targets(
        clients,
        replay.targets(args, what.rows)?,
        args.concurrency as usize,
        plan(args),
        false,
    );
    load.set_rate(args.rate);
    if let Some(warmup) = args.warmup() {
        load.set_warmup(warmup);
    }
    let mut started = Instant::now();
    let mut warming = load.warming();
    let mut warmup_requests = 0;
    let mut total = Metrics::default();
    let mut each = vec![Metrics::default(); views.len()];
    let mut interrupted = false;
    let mut pump = tokio::time::interval(PUMP);
    loop {
        tokio::select! {
            _ = pump.tick() => {
                if warming && !load.warming() {
                    warming = false;
                    started = Instant::now();
                }
                let over = !load.drain(|stat| {
                    if stat.warmup {
                        warmup_requests += 1;
                        return;
                    }
                    total.record(&stat);
                    if let Some(metrics) = each.get_mut(stat.endpoint as usize) {
                        metrics.record(&stat);
                    }
                });
                exporter::publish(|| {
                    let rows: Vec<(String, &Metrics)> =
                        row_labels.iter().cloned().zip(each.iter()).collect();
                    exporter::run_page(
                        &args.target_label(),
                        &total,
                        &timeline,
                        started.elapsed(),
                        args.concurrency as usize,
                        &rows,
                    )
                });
                if over {
                    break;
                }
            }
            _ = tokio::signal::ctrl_c(), if !interrupted => {
                interrupted = true;
                load.stop();
            }
        }
    }
    let elapsed = started.elapsed();
    let report = json_report::JsonReport::generate(&total, elapsed, interrupted)
        .with_generator(
            load.threads(),
            load.peak_busy(),
            load.rate().map(|r| (r, load.missed())),
        )
        .with_target(
            "replay",
            None,
            &what.log.display().to_string(),
            args.concurrency,
        )
        .with_warmup(args.warmup(), warmup_requests)
        .with_connects(&connects);
    let mut report = serde_json::to_value(&report)?;
    let ms = |d: std::time::Duration| (d.as_secs_f64() * 1_000_000.0).round() / 1000.0;
    let kept = replay.kept().max(1) as f64;
    let urls: Vec<serde_json::Value> = views
        .iter()
        .zip(&each)
        .enumerate()
        .map(|(rank, (view, m))| {
            let share = match replay.urls.get(rank).filter(|_| rank < what.rows) {
                Some(entry) => entry.count as f64 / kept,
                None => replay.urls[what.rows.min(replay.urls.len())..].iter().map(|e| e.count).sum::<u64>() as f64 / kept,
            };
            serde_json::json!({
                "method": view.method,
                "url": if view.variants > 1 { format!("{} other URLs", view.variants) } else { view.url.clone() },
                "share_in_log": (share * 10_000.0).round() / 10_000.0,
                "requests": m.total,
                "failed_requests": m.total - m.success,
                "requests_per_second": m.rps(elapsed),
                "median_ms": ms(m.percentile(50.0)),
                "p99_ms": ms(m.percentile(99.0)),
                "status_codes": m.status_codes.iter().map(|(k, v)| (k.to_string(), *v)).collect::<std::collections::BTreeMap<_, _>>(),
            })
        })
        .collect();
    report["replay"] = serde_json::json!({
        "log": what.log.display().to_string(),
        "requests_in_log": replay.requests,
        "replayed_from_log": replay.kept(),
        "distinct_urls": replay.urls.len(),
        "left_out": {
            "unparsed_lines": replay.unparsed,
            "writes": replay.writes,
            "no_host": replay.no_host,
            "rare_urls": replay.tail,
        },
        "urls": urls,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

/// `pepe logs`: what nginx's logs say of its traffic, now and before
async fn run_logs(args: &Cli, what: &cli::LogsArgs) -> Result<(), Box<dyn std::error::Error>> {
    let fail = |message: String| -> ! {
        eprintln!("error: {message}");
        std::process::exit(2);
    };
    let parser = logs::Parser::new(what.format.as_deref())
        .unwrap_or_else(|e| fail(format!("--format: {e}")));
    let window = logs::span(&what.window).unwrap_or_else(|e| fail(format!("--window: {e}")));
    let since = what
        .since
        .as_deref()
        .map(|since| logs::span(since).unwrap_or_else(|e| fail(format!("--since: {e}"))))
        .map(|since| logs::wall() - since);
    let piped = !stdin().is_terminal();
    let mut files = what.files.clone();
    let dash = files.iter().any(|f| f.as_os_str() == "-");
    if dash && files.len() > 1 {
        fail("- reads what is piped in, and can't be mixed with files".into());
    }
    if dash {
        files.clear();
    } else if files.is_empty() && !piped {
        files = logs::default_files();
        if files.is_empty() {
            fail(
                "no log to read: name one, as in `pepe logs /var/log/nginx/access.log`, \
                 or pipe one in"
                    .into(),
            );
        }
    }
    if let Some(missing) = files.iter().find(|f| !f.is_file()) {
        fail(format!("{} isn't a file", missing.display()));
    }
    // A log that is piped in has stdin; the screen needs it for its keys,
    // so the pipe is put aside and the terminal takes its place
    let mut screen = !args.json && stdout().is_terminal();
    let mut aside = None;
    if screen && files.is_empty() {
        aside = logs::piped_aside();
        screen = aside.is_some();
    }
    // At a terminal a log that is being written is watched, not read: the
    // screen starts a few minutes back, enough for "now" to be right at
    // once and for the chart to have something in it. A log nobody is
    // writing has no now, and is read whole.
    let mut since = since;
    if screen && since.is_none() && !what.all && !files.is_empty() {
        let wall = logs::wall();
        if logs::being_written(&files, &parser, wall) {
            since = Some(wall - window.max(logs::LOOKBACK));
        }
    }
    let mut job = logs::Job {
        files,
        piped: aside,
        parser,
        since,
        follow: screen,
        exact_paths: what.exact_paths,
    };
    let name = job.name();
    if !screen {
        job.follow = false;
        let shared = logs::Shared::default();
        logs::read(&job, &shared);
        let stats = shared.lock();
        if let Some(trouble) = &stats.trouble {
            eprintln!("error: {trouble}");
        }
        if args.json {
            let report = logs::json_report(&stats, &job, logs::wall(), window, what.rows);
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print!(
                "{}",
                logs::report(&stats, &name, logs::wall(), window, what.rows)
            );
        }
        return Ok(());
    }
    let check = update::Check::start();
    let shared = logs::start(job.clone());
    let outcome = {
        let _watchdog = CtrlCWatchdog::arm();
        let _terminal = TerminalGuard::enter()?;
        ui::LogsScreen::new(shared.clone(), job.parser.clone(), name.clone(), window)
            .run()
            .await
    };
    shared.stop();
    outcome?;
    print_report(&logs::report(
        &shared.lock(),
        &name,
        logs::wall(),
        window,
        what.rows,
    ));
    say_if_newer(check).await;
    Ok(())
}

/// `pepe ping`: a request every so often, each with its phases, on a
/// graph; or a line per ping when there's no screen to draw on
async fn run_ping(
    args: &Cli,
    what: &cli::PingArgs,
    matches: &clap::ArgMatches,
) -> Result<(), Box<dyn std::error::Error>> {
    let fail = |message: String| -> ! {
        eprintln!("error: {message}");
        std::process::exit(2);
    };
    let family = match (what.ipv4, what.ipv6) {
        (true, _) => Some(ping::Family::V4),
        (_, true) => Some(ping::Family::V6),
        _ => None,
    };
    let mut every = ping::interval(&what.every, std::time::Duration::from_secs(1))
        .unwrap_or_else(|e| fail(format!("--every: {e}")));
    let typed = |name: &str| {
        matches
            .subcommand_matches("ping")
            .and_then(|m| m.value_source(name))
            .or_else(|| matches.value_source(name))
            == Some(clap::parser::ValueSource::CommandLine)
    };
    // --once: three pings, quickly, unless told otherwise
    if what.once && !typed("every") {
        every = std::time::Duration::from_millis(200);
    }
    let window = ping::interval(&what.window, std::time::Duration::from_secs(1))
        .unwrap_or_else(|e| fail(format!("--window: {e}")));
    let slo = match &what.slo {
        Some(text) => ping::Slo::parse(text).unwrap_or_else(|e| fail(format!("--slo: {e}"))),
        None => ping::Slo::default(),
    };
    let bind = what.interface.as_deref().map(|name| {
        ping::local_address(name, family).unwrap_or_else(|e| fail(format!("--interface: {e}")))
    });
    let colors: Vec<ratatui::style::Color> = what
        .color
        .iter()
        .flat_map(|c| c.split(','))
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| {
            ui::parse_color(c).unwrap_or_else(|| fail(format!("--color: {c:?} isn't a colour")))
        })
        .collect();
    // Only a count that was typed ends the run: the default is forever
    let count = if typed("number") {
        Some(u64::from(args.number))
    } else if what.once {
        Some(3)
    } else {
        None
    };
    let headers = {
        let mut map = reqwest::header::HeaderMap::new();
        for header in &args.headers {
            let (name, value) = request::parse_header(header).unwrap_or_else(|e| fail(e));
            map.append(name, value);
        }
        map
    };
    let settings = ping::Settings {
        every,
        timeout: std::time::Duration::from_secs(u64::from(args.timeout)),
        family,
        bind,
        insecure: args.insecure,
        keep_alive: what.keep_alive,
        follow_redirects: !args.disable_redirects,
        refused_is_pong: what.tcp_rst == cli::Refused::Pong,
        slo,
        keep_body: if what.save_body.is_some() {
            ping::BODY_SAVED
        } else if what.show_body {
            ping::BODY_PREVIEW
        } else {
            0
        },
        proxy: args.proxy.clone(),
        user_agent: args.user_agent.clone(),
        headers,
        method: reqwest::Method::from_bytes(args.method.as_bytes()).unwrap_or(reqwest::Method::GET),
        body: args.body().map(bytes::Bytes::from).unwrap_or_default(),
        count,
        duration: args.run_duration(),
        compression: !args.disable_compression,
    };
    let targets = ping::targets(ping::Words {
        words: &what.targets,
        names: &what.name,
        tcp: what.tcp,
        port: what.port,
        cmd: what.cmd,
        all_ips: what.all_ips,
        family,
    })
    .await
    .unwrap_or_else(|e| fail(e));

    let screen = !args.json
        && !what.jsonl
        && !what.csv
        && !what.once
        && stdout().is_terminal()
        && stdin().is_terminal();
    let shared = ping::start(targets, settings.clone());
    if exporter::address().is_some() {
        let shared = shared.clone();
        tokio::spawn(async move {
            while !shared.stopped() {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                exporter::publish(|| exporter::ping_page(&shared.lock()));
            }
        });
    }
    let mut interrupted = false;
    if screen {
        let check = update::Check::start();
        let outcome = {
            let _watchdog = CtrlCWatchdog::arm();
            let _terminal = TerminalGuard::enter()?;
            ui::PingScreen::new(
                shared.clone(),
                ui::Look {
                    window: window.as_secs_f32(),
                    ymin: if what.ymin_zero {
                        Some(0)
                    } else {
                        what.ymin.map(|ms| ms * 1000)
                    },
                    ymax: what.ymax.map(|ms| ms * 1000),
                    simple: what.simple_graphics,
                    colors,
                    bell: what.bell,
                },
            )
            .run()
            .await
        };
        shared.stop();
        outcome?;
        {
            let state = shared.lock();
            interrupted = !state.done;
            print_report(&ping::report(&state, &settings));
        }
        say_if_newer(check).await;
    } else {
        // No screen: each ping as a line, as it happens, and the summary
        // at the end; JSON goes to stdout alone, the summary to stderr
        let (jsonl, csv, json) = (what.jsonl, what.csv, args.json);
        // --once says it all at the end, not a line per ping
        let quiet = what.once && !jsonl && !csv;
        if csv {
            println!("{}", ping::CSV_HEADER);
        }
        let mut printed: Vec<usize> = vec![0; shared.lock().targets.len()];
        let mut pump = tokio::time::interval(std::time::Duration::from_millis(50));
        let bell = what.bell;
        loop {
            tokio::select! {
                _ = pump.tick() => {}
                _ = tokio::signal::ctrl_c() => {
                    interrupted = true;
                    shared.stop();
                }
            }
            let mut state = shared.lock();
            let mut lines = Vec::new();
            for (index, t) in state.targets.iter().enumerate() {
                let seen = t.recent.len();
                let dropped = (t.sent as usize).saturating_sub(seen);
                let from = printed[index].saturating_sub(dropped).min(seen);
                for sample in t.recent.iter().skip(from) {
                    let line = if jsonl {
                        ping::sample_json(sample, &t.target).to_string()
                    } else if csv {
                        ping::sample_csv(sample, &t.target)
                    } else if json || quiet {
                        continue;
                    } else {
                        ping::sample_line(sample, &t.target)
                    };
                    lines.push((sample.at, line));
                }
                printed[index] = seen + dropped;
            }
            let rang = std::mem::take(&mut state.bells);
            let done = state.done || shared.stopped();
            drop(state);
            lines.sort_by_key(|l| l.0);
            for (_, line) in lines {
                println!("{line}");
            }
            if bell && rang > 0 {
                eprint!("\x07");
            }
            if done {
                break;
            }
        }
        let state = shared.lock();
        if json {
            println!(
                "{}",
                serde_json::to_string_pretty(&ping::json_report(&state, &settings, interrupted))?
            );
        } else if jsonl || csv {
            eprint!("{}", ping::report(&state, &settings));
        } else if what.once {
            print_report(&diagnose::once(&state, &settings));
        } else {
            print_report(&ping::report(&state, &settings));
        }
    }
    let state = shared.lock();
    if let Some(path) = &what.save {
        let report =
            serde_json::to_string_pretty(&ping::json_report(&state, &settings, interrupted))?;
        if let Err(e) = std::fs::write(path, report) {
            eprintln!("couldn't write {}: {e}", path.display());
        }
    }
    if let Some(path) = &what.save_body {
        match state.targets.iter().find_map(|t| t.last_body.as_ref()) {
            Some(body) => {
                if let Err(e) = std::fs::write(path, body) {
                    eprintln!("couldn't write {}: {e}", path.display());
                }
            }
            None => eprintln!("no body to write to {}", path.display()),
        }
    }
    // Like ping: 1 when nothing ever answered; 4 when the SLO was broken
    let code = if state.any_violation() {
        4
    } else if state.nothing_answered() {
        1
    } else {
        0
    };
    drop(state);
    if code != 0 {
        std::process::exit(code);
    }
    Ok(())
}

/// `pepe compare before.json after.json`: what moved, as a report or as
/// JSON; with --gate, exit 1 when it is a regression
fn run_compare(args: &Cli, what: &cli::CompareArgs) -> Result<(), Box<dyn std::error::Error>> {
    let sides = (
        compare::Side::read(&what.before),
        compare::Side::read(&what.after),
    );
    let (before, after) = match sides {
        (Ok(before), Ok(after)) => (before, after),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    };
    let comparison = compare::compare(&before, &after);
    if let Some(path) = &what.svg {
        std::fs::write(path, comparison.svg())
            .map_err(|e| format!("can't write {}: {e}", path.display()))?;
    }
    if args.json {
        println!("{}", comparison.to_json()?);
    } else {
        print_report(&comparison.report());
    }
    if what.gate && comparison.regression {
        std::process::exit(1);
    }
    Ok(())
}

async fn run_api(args: &Cli, api: &cli::ApiArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut run = match api::ApiRun::load(api).await {
        Ok(run) => run,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    if args.json {
        return run_api_json(args, &run).await;
    }

    let check = update::Check::start();
    let report = {
        let _watchdog = CtrlCWatchdog::arm();
        let _terminal = TerminalGuard::enter()?;
        api_session(args, &mut run).await?
    };
    if let Some(report) = report {
        print_report(&report);
    }
    say_if_newer(check).await;
    Ok(())
}

// The load engine has its own threads (see `load`); this runtime only runs
// the screens, the reports and the update check
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parsed by hand so the config file can tell typed flags from defaults
    let matches = Cli::command().get_matches();
    let mut args = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    match config::load(args.config.as_deref()) {
        Ok(Some((path, file))) => {
            file.apply(&mut args, &matches);
            args.config_loaded = Some(path);
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
    if let Some(path) = args.write_config.take() {
        if let Err(e) = config::write(&path, &args) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        println!(
            "wrote {} · run it with: pepe --config {}",
            path.display(),
            path.display()
        );
        return Ok(());
    }
    // Which response header carries the request id; `validate` rejects a
    // name that isn't one
    if let Some(name) = args
        .trace_header
        .as_deref()
        .and_then(|n| reqwest::header::HeaderName::from_bytes(n.as_bytes()).ok())
    {
        trace::use_id_header(name);
    }

    if let Some(cli::Command::SelfUpdate(what)) = &args.command {
        return update::self_update(what.check, what.verbose).await;
    }
    if let Some(cli::Command::Completions(what)) = &args.command {
        return completions::run(what);
    }

    // Release builds abort on panic; restore the terminal first so a crash
    // never leaves the shell in raw mode
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    // stderr, so `--json` output on stdout stays valid JSON
    if let Some(spec) = &args.metrics {
        match exporter::start(spec).await {
            Ok(addr) => eprintln!("metrics: http://{addr}/metrics"),
            Err(e) => {
                eprintln!("error: --metrics: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(cli::Command::Replay(what)) = args.command.clone() {
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return run_replay(&args, &what).await;
    }
    if let Some(cli::Command::Logs(what)) = args.command.clone() {
        return run_logs(&args, &what).await;
    }
    if let Some(cli::Command::Ping(what)) = args.command.clone() {
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return run_ping(&args, &what, &matches).await;
    }
    if let Some(cli::Command::Compare(what)) = args.command.clone() {
        return run_compare(&args, &what);
    }
    if let Some(cli::Command::Flow(what)) = args.command.clone() {
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return run_flow(&args, &what).await;
    }
    if let Some(cli::Command::Api(api)) = args.command.clone() {
        if api.spec.is_empty() {
            // With nothing to load yet, the setup screen asks for the spec:
            // a file, a URL, or the document pasted in
            let interactive = !args.json && stdin().is_terminal() && stdout().is_terminal();
            if !interactive {
                eprintln!("error: api needs a spec: pepe api openapi.yaml, or `spec` under [api] in pepe.toml");
                std::process::exit(2);
            }
            return run_screens(&args, true).await;
        }
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return run_api(&args, &api).await;
    }

    // A ramp names its URL after `ramp`; from here on it's the URL
    let ramp = match &args.command {
        Some(cli::Command::Ramp(ramp)) => {
            args.url = ramp.url.clone();
            Some(ramp.clone())
        }
        _ => None,
    };

    // The setup screen opens on request, or when there's nothing to run
    // yet and someone is at the terminal to fill it in
    let interactive = !args.json && stdin().is_terminal() && stdout().is_terminal();
    let nothing_to_run = args.url.is_empty() && !args.curl;
    if nothing_to_run && !interactive {
        Cli::command().print_help()?;
        std::process::exit(2);
    }
    let setup = interactive && (args.setup || nothing_to_run);

    // With nothing to run yet, the setup screen does the checking instead
    if !nothing_to_run {
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }

    if let (Some(ramp), false) = (&ramp, nothing_to_run) {
        match RampPlan::from_args(ramp) {
            Ok(plan) if args.json => return run_ramp_json(&args, plan).await,
            Ok(_) => {}
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
    if args.json {
        return run_json(&args).await;
    }
    run_screens(&args, setup).await
}

/// The screens (the setup form when `setup`, then the run), and what they
/// leave in the shell
async fn run_screens(args: &Cli, setup: bool) -> Result<(), Box<dyn std::error::Error>> {
    let check = update::Check::start();
    let farewell = run_interactive(args, setup).await?;
    if let Some(report) = farewell.report {
        print_report(&report);
    }
    if let Some(command) = farewell.command {
        println!("Run this again with:\n  {command}");
    }
    say_if_newer(check).await;
    Ok(())
}
