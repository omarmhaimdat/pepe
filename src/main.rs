use std::io::stdout;
use std::time::Instant;

use clap::Parser;
use crossterm::{
    cursor::{Hide, Show},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::cli::Cli;
use crate::load::{LoadHandle, Plan};
use crate::metrics::Metrics;

mod cache;
mod cli;
mod json_report;
mod load;
mod metrics;
mod request;
mod response;
mod ui;
mod update;
mod utils;

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
enum PepeError {
    HeaderParseError(String),
    IoError(std::io::Error),
    RequestError(reqwest::Error),
    UrlParseError(hyper::http::uri::InvalidUri),
    HostParseError,
}

impl std::fmt::Display for PepeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderParseError(msg) => write!(f, "Header parse error: {}", msg),
            Self::RequestError(e) => write!(f, "Request error: {}", e),
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::UrlParseError(e) => write!(f, "URL parse error: {}", e),
            Self::HostParseError => write!(f, "Host parse error"),
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

fn start_load(args: &Cli) -> Result<LoadHandle, PepeError> {
    let request = args.request()?;
    let client = request.build_client()?;
    Ok(load::start(
        client,
        request,
        args.concurrency as usize,
        plan(args),
    ))
}

fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(stdout(), LeaveAlternateScreen, Show);
}

/// Puts the terminal into dashboard mode and restores it when dropped, so
/// every exit path (including `?` errors) leaves a usable shell behind
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> std::io::Result<Self> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, Hide)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore_terminal();
    }
}

/// `--json`: no dashboard; run to completion (or Ctrl-C), print the report
async fn run_json(args: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut load = start_load(args)?;
    let started = Instant::now();
    let mut metrics = Metrics::default();
    let mut interrupted = false;

    loop {
        tokio::select! {
            stat = load.rx.recv() => match stat {
                Some(stat) => metrics.record(&stat),
                None => break,
            },
            _ = tokio::signal::ctrl_c(), if !interrupted => {
                interrupted = true;
                load.stop();
            }
        }
    }

    let report = json_report::JsonReport::generate(&metrics, started.elapsed(), interrupted);
    println!("{}", report.to_json()?);
    Ok(())
}

async fn run_dashboard(args: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let _terminal = TerminalGuard::enter()?;
    loop {
        let mut load = start_load(args)?;
        let mut dashboard = ui::Dashboard::new(args.clone(), plan(args));
        match dashboard.run(&mut load)? {
            // Dropping `load` stops the previous run before the next starts
            ui::Outcome::Restart => continue,
            ui::Outcome::Quit => return Ok(()),
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = Cli::parse();

    if let Some(cli::Command::SelfUpdate) = args.command {
        return update::self_update().await;
    }

    if let Err(e) = args.validate() {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    if args.json {
        return run_json(&args).await;
    }

    // Release builds abort on panic; restore the terminal first so a crash
    // never leaves the shell in raw mode
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        default_hook(info);
    }));

    run_dashboard(&args).await?;
    update::check_for_updates().await;
    Ok(())
}
