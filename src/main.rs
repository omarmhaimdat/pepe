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
mod curl;
mod insights;
mod json_report;
mod load;
mod metrics;
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

/// `previews`: keep the start of each body, which only the dashboard shows
fn start_load(args: &Cli, previews: bool) -> Result<LoadHandle, PepeError> {
    let request = args.request()?;
    let client = request.build_client()?;
    Ok(load::start(
        client,
        request,
        args.concurrency as usize,
        plan(args),
        previews,
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
        #[cfg(unix)]
        keep_ctrl_c_a_signal();
        execute!(stdout(), EnterAlternateScreen, Hide)?;
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

/// `--json`: no dashboard; run to completion (or Ctrl-C), print the report
async fn run_json(args: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let mut load = start_load(args, false)?;
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

/// Runs the dashboard until the user quits. Returns the end-of-run report, to
/// print once the terminal is back to normal.
async fn run_dashboard(args: &Cli) -> Result<Option<String>, Box<dyn std::error::Error>> {
    // Declared first so it's dropped last: restoring the terminal writes to
    // it, which can block too, and Ctrl-C must still get out then
    let _watchdog = CtrlCWatchdog::arm();
    let _terminal = TerminalGuard::enter()?;
    let mut args = args.clone();
    loop {
        let mut load = start_load(&args, true)?;
        let mut dashboard = ui::Dashboard::new(args.clone(), plan(&args));
        match dashboard.run(&mut load).await? {
            ui::Outcome::Restart => {
                // Keep any concurrency the user dialed in during the run.
                // Dropping `load` stops the previous run before the next starts.
                args.concurrency = load.concurrency() as u32;
                continue;
            }
            ui::Outcome::Quit => return Ok(dashboard.report()),
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

    if let Some(report) = run_dashboard(&args).await? {
        print!("{report}");
    }
    update::check_for_updates().await;
    Ok(())
}
