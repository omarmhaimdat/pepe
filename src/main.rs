use std::io::{stderr, stdin, stdout, IsTerminal};
use std::time::Instant;

use clap::{CommandFactory, Parser};
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
mod cli;
mod completions;
mod contrib;
mod curl;
mod insights;
mod json_report;
mod load;
mod metrics;
mod openapi;
mod ramp;
mod request;
mod response;
mod timeline;
mod ui;
mod update;
mod utils;

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
fn shards(args: &Cli, concurrency: usize) -> usize {
    load::shards(concurrency, args.threads.map(|t| t as usize))
}

/// One client per load shard
fn clients_for(
    request: &request::Request,
    args: &Cli,
    concurrency: usize,
) -> Result<Vec<reqwest::Client>, PepeError> {
    request.build_clients(shards(args, concurrency))
}

/// `previews`: keep the start of each body, which only the dashboard shows
fn start_load(args: &Cli, previews: bool) -> Result<LoadHandle, PepeError> {
    let request = args.request()?;
    let clients = clients_for(&request, args, args.concurrency as usize)?;
    Ok(load::start(
        clients,
        request,
        args.concurrency as usize,
        plan(args),
        previews,
    ))
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
    let mut load = start_load(args, false)?;
    let started = Instant::now();
    let mut metrics = Metrics::default();
    let mut timeline = timeline::Timeline::default();
    let mut interrupted = false;
    let mut pump = tokio::time::interval(PUMP);
    let mut snapshots = tokio::time::interval(SNAPSHOT_EVERY);
    snapshots.tick().await; // the first tick is now; the first snapshot is in a minute
    let mut peak_busy = None;
    let report = |metrics: &Metrics,
                  timeline: &timeline::Timeline,
                  load: &LoadHandle,
                  peak_busy,
                  interrupted| {
        json_report::JsonReport::generate(metrics, started.elapsed(), interrupted)
            .with_generator(load.threads(), peak_busy)
            .with_timeline(timeline)
    };

    loop {
        tokio::select! {
            _ = pump.tick() => {
                peak_busy = peak_busy.max(load.busy());
                let over = !load.drain(|stat| {
                    metrics.record(&stat);
                    timeline.record(&stat);
                });
                timeline.advance(started.elapsed());
                if over {
                    break;
                }
            }
            _ = snapshots.tick(), if args.snapshot.is_some() => {
                let path = args.snapshot.as_ref().expect("checked");
                if let Err(e) = report(&metrics, &timeline, &load, peak_busy, interrupted).with_snapshot(true).write_to(path) {
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
    let report = report(&metrics, &timeline, &load, peak_busy, interrupted);
    if let Some(path) = &args.snapshot {
        report.clone().with_snapshot(false).write_to(path)?;
    }
    println!("{}", report.to_json()?);
    Ok(())
}

/// After the report: Pepe mentions a newer release, if the look that
/// started with the run found one
async fn say_if_newer(check: update::Check) {
    if let Some(latest) = check.finish().await {
        eprint!("{}", update::notice(&latest, stderr().is_terminal()));
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
                    clients_for(&request, &args, plan.peak() as usize)?,
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
                let mut load = start_load(&args, true)?;
                let mut dashboard = ui::Dashboard::new(args.clone(), plan(&args));
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
        clients_for(&request, args, plan.peak() as usize)?,
        request,
        plan.levels[0] as usize,
        Plan::Duration(UNTIL_STOPPED),
        false,
    );
    let mut ramp = Ramp::new(plan, Instant::now());
    let mut clock = tokio::time::interval(std::time::Duration::from_millis(50));
    let mut pump = tokio::time::interval(PUMP);
    let mut peak_busy = None;
    while ramp.end.is_none() {
        tokio::select! {
            _ = pump.tick() => {
                peak_busy = peak_busy.max(load.busy());
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
        peak_busy_percent: peak_busy,
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
    let mut load = load::start_targets(
        run.clients(args, shards(args, args.concurrency as usize))?,
        targets,
        args.concurrency as usize,
        plan(args),
        false,
    );
    let started = Instant::now();
    let mut total = Metrics::default();
    let mut each = vec![Metrics::default(); which.len()];
    let mut interrupted = false;
    let mut pump = tokio::time::interval(PUMP);
    let mut peak_busy = None;
    loop {
        tokio::select! {
            _ = pump.tick() => {
                peak_busy = peak_busy.max(load.busy());
                let over = !load.drain(|stat| {
                    total.record(&stat);
                    if let Some(metrics) = each.get_mut(stat.endpoint as usize) {
                        metrics.record(&stat);
                    }
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
        .with_generator(load.threads(), peak_busy);
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
        let mut load = load::start_targets(
            run.clients(&shown, shards(&shown, shown.concurrency as usize))?,
            targets,
            shown.concurrency as usize,
            plan(&shown),
            true,
        );
        let mut dashboard = ui::Dashboard::new(shown.clone(), plan(&shown))
            .with_endpoints(run.views(&shown, &which));
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
        print!("{report}");
    }
    say_if_newer(check).await;
    Ok(())
}

// The load engine has its own threads (see `load`); this runtime only runs
// the screens, the reports and the update check
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Cli::parse();

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

    if let Some(cli::Command::Api(api)) = args.command.clone() {
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

    let check = update::Check::start();
    let farewell = run_interactive(&args, setup).await?;
    if let Some(report) = farewell.report {
        print!("{report}");
    }
    if let Some(command) = farewell.command {
        println!("Run this again with:\n  {command}");
    }
    say_if_newer(check).await;
    Ok(())
}
