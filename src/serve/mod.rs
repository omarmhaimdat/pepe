//! `pepe serve`: pepe over HTTP. A script, an agent or a browser starts,
//! watches, steers and compares runs; workers on other machines take a
//! share of a distributed run. The design is `docs/design/serve.md`.
//!
//! The server sends no load itself: every run is a child `pepe` with
//! `--events`, under the server's guardrails, the directory's
//! `pepe.toml` and the machine's settings, so the guardrails and the code
//! paths are the CLI's own. Runs are kept under `.pepe/serve/runs/`, saved
//! tests' recorded runs under `.pepe/runs/` as `--record` keeps them.

mod api;
mod http;
mod merge;
mod runs;
mod schedule;
mod ui;
pub mod worker;
mod workers;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use tokio::net::TcpListener;

use crate::cli::{Cli, ServeArgs};

/// Where the server listens unless told
pub const DEFAULT_LISTEN: &str = "127.0.0.1:7878";

/// What every request shares
pub struct Server {
    /// The directory it was started in: the project's `.pepe` is here
    root: PathBuf,
    /// `.pepe/serve`
    dir: PathBuf,
    /// Who may call it: a name and a token each
    tokens: Vec<(String, String)>,
    /// The guardrail flags it was started with, first on every child
    guard: Vec<String>,
    max_runs: usize,
    exe: PathBuf,
    runs: Mutex<HashMap<String, Arc<runs::Run>>>,
    /// Runs started since the server did, to tell ids apart
    started: AtomicU64,
    workers: workers::Workers,
    schedules: schedule::Schedules,
    tickets: Tickets,
    audit: Mutex<Option<std::fs::File>>,
}

impl Server {
    /// The project's saved tests and their recorded runs
    fn store(&self) -> crate::store::Store {
        crate::store::Store::at(self.root.join(crate::store::DIR))
    }

    /// A line in `.pepe/serve/audit.jsonl`: who did what
    fn audit(&self, who: &str, action: &str, detail: Value) {
        use std::io::Write;
        let line = json!({
            "at": crate::store::stamp(std::time::SystemTime::now()),
            "who": who,
            "action": action,
            "detail": detail,
        });
        if let Ok(mut file) = self.audit.lock() {
            if let Some(file) = file.as_mut() {
                let _ = writeln!(file, "{line}");
            }
        }
    }
}

/// How long a report's ticket opens it
pub const TICKET_LIFE: std::time::Duration = std::time::Duration::from_secs(600);

/// Short-lived tickets that open one run's reports as a plain link, which
/// can't carry the token in a header: the ticket, the run, and until when
#[derive(Default)]
struct Tickets(Mutex<Vec<(String, String, std::time::Instant)>>);

impl Tickets {
    /// A ticket that opens `run`'s reports for `life`, and nothing else
    fn issue(&self, run: &str, life: std::time::Duration) -> String {
        let ticket = new_token();
        if let Ok(mut tickets) = self.0.lock() {
            let now = std::time::Instant::now();
            tickets.retain(|(_, _, until)| *until > now);
            tickets.push((ticket.clone(), run.to_string(), now + life));
        }
        ticket
    }

    /// Whether `ticket` opens `run`'s reports now
    fn opens(&self, ticket: &str, run: &str) -> bool {
        let now = std::time::Instant::now();
        self.0.lock().is_ok_and(|tickets| {
            tickets.iter().any(|(t, r, until)| {
                *until > now && r == run && http::same(t.as_bytes(), ticket.as_bytes())
            })
        })
    }
}

impl Server {
    fn ticket(&self, run: &str) -> String {
        self.tickets.issue(run, TICKET_LIFE)
    }

    fn ticket_opens(&self, ticket: &str, run: &str) -> bool {
        self.tickets.opens(ticket, run)
    }
}

/// Make a token: 24 random bytes, hex
fn new_token() -> String {
    let mut bytes = [0u8; 24];
    let provider = tokio_rustls::rustls::crypto::ring::default_provider();
    let _ = provider.secure_random.fill(&mut bytes);
    use std::fmt::Write;
    bytes.iter().fold(String::new(), |mut out, b| {
        let _ = write!(out, "{b:02x}");
        out
    })
}

/// `name token` a line, or the token alone (named `default`); `#` starts
/// a comment
pub fn read_tokens(text: &str) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut words = line.split_whitespace();
        let (name, token) = match (words.next(), words.next(), words.next()) {
            (Some(token), None, _) => ("default", token),
            (Some(name), Some(token), None) => (name, token),
            _ => return Err(format!("line {}: `name token`, or the token alone", n + 1)),
        };
        if token.len() < 16 {
            return Err(format!("line {}: a token is 16 characters or more", n + 1));
        }
        out.push((name.to_string(), token.to_string()));
    }
    if out.is_empty() {
        return Err("no token in it".into());
    }
    Ok(out)
}

/// Whether `addr` takes connections from this machine only
fn loopback(addr: &SocketAddr) -> bool {
    addr.ip().is_loopback()
}

/// Off loopback, the server is a traffic generator anyone who reaches it
/// with a token can point: it has to be held
fn check_exposed(addr: &SocketAddr, cli: &Cli) -> Result<(), String> {
    if loopback(addr) {
        return Ok(());
    }
    let guard = crate::guard::Guard::of(cli);
    let mut missing = Vec::new();
    if guard.hosts.is_empty() {
        missing.push("--allow-host");
    }
    if guard.max_requests.is_none() {
        missing.push("--max-requests");
    }
    if guard.max_rate.is_none() {
        missing.push("--max-rate");
    }
    if guard.max_concurrency.is_none() {
        missing.push("--max-concurrency");
    }
    match missing.is_empty() {
        true => Ok(()),
        false => Err(format!(
            "{addr} takes connections from other machines, so the server needs its guardrails: {} (in the flags, a pepe.toml here or the settings)",
            missing.join(", ")
        )),
    }
}

pub async fn serve(cli: &Cli, args: &ServeArgs) -> Result<(), Box<dyn std::error::Error>> {
    let spec = args.listen.as_deref().unwrap_or(DEFAULT_LISTEN);
    let addr = crate::exporter::parse(spec)?;
    if let Err(e) = check_exposed(&addr, cli) {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
    if args.max_runs == 0 {
        eprintln!("error: --max-runs is 1 or more");
        std::process::exit(2);
    }
    let (tokens, made) = match &args.token_file {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
            (
                read_tokens(&text).map_err(|e| format!("{}: {e}", path.display()))?,
                None,
            )
        }
        None => {
            let token = new_token();
            (vec![("default".to_string(), token.clone())], Some(token))
        }
    };
    let root = std::env::current_dir()?;
    let dir = root.join(crate::store::DIR).join("serve");
    std::fs::create_dir_all(dir.join("runs"))
        .map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    let audit = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("audit.jsonl"))
        .ok();
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("couldn't listen on {addr}: {e}"))?;
    let bound = listener.local_addr()?;
    let server = Arc::new(Server {
        runs: Mutex::new(runs::load_past(&dir.join("runs"))),
        schedules: schedule::Schedules::load(&dir),
        root,
        dir,
        tokens,
        guard: crate::mcp::guard_args(cli),
        max_runs: args.max_runs,
        exe: std::env::current_exe()?,
        started: AtomicU64::new(0),
        workers: workers::Workers::default(),
        tickets: Tickets::default(),
        audit: Mutex::new(audit),
    });
    eprintln!(
        "pepe serve · http://{bound}/api · {}",
        match &made {
            Some(_) => "a token made for this run, below".to_string(),
            None => format!("{} token(s) from the file", server.tokens.len()),
        }
    );
    if let Some(guard) = crate::guard::Guard::of(cli).describe() {
        eprintln!("  guard  {guard}");
    }
    if let Some(token) = &made {
        eprintln!("  page   http://{bound}/#token={token}");
        // stdout, alone, so a script can take it
        println!("{token}");
    } else {
        eprintln!("  page   http://{bound}/");
    }
    server.audit("server", "start", json!({ "listen": bound.to_string() }));
    tokio::spawn(schedule::tick(server.clone()));
    loop {
        let (stream, _) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(_) => continue,
        };
        let server = server.clone();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |request| {
                let server = server.clone();
                async move { Ok::<_, std::convert::Infallible>(api::handle(server, request).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(std::time::Duration::from_secs(10))
                .serve_connection(TokioIo::new(stream), service)
                .with_upgrades()
                .await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn tokens_are_read_with_their_names() {
        let tokens =
            read_tokens("# who may\nci 0123456789abcdef0123\n  fedcba9876543210fedc  \n").unwrap();
        assert_eq!(
            tokens,
            [
                ("ci".to_string(), "0123456789abcdef0123".to_string()),
                ("default".to_string(), "fedcba9876543210fedc".to_string())
            ]
        );
        assert!(read_tokens("short").unwrap_err().contains("16 characters"));
        assert!(read_tokens("a b c").is_err());
        assert!(read_tokens("\n# none\n").is_err());
        assert_eq!(new_token().len(), 48);
        assert_ne!(new_token(), new_token());
    }

    /// A ticket opens its run's reports, for a while, and no other run's
    #[test]
    fn a_ticket_opens_one_runs_reports_for_a_while() {
        let tickets = Tickets::default();
        let t = tickets.issue("run-1", std::time::Duration::from_secs(60));
        assert!(tickets.opens(&t, "run-1"));
        assert!(!tickets.opens(&t, "run-2"));
        assert!(!tickets.opens("nope", "run-1"));
        let gone = tickets.issue("run-1", std::time::Duration::ZERO);
        assert!(!tickets.opens(&gone, "run-1"), "run out");
    }

    /// On loopback the server needs nothing; anywhere else, every guardrail
    #[test]
    fn off_loopback_the_guardrails_are_required() {
        let cli = |args: &[&str]| Cli::parse_from(["pepe"].iter().chain(args));
        let local: SocketAddr = "127.0.0.1:7878".parse().unwrap();
        let open: SocketAddr = "0.0.0.0:7878".parse().unwrap();
        assert!(check_exposed(&local, &cli(&["serve"])).is_ok());
        let err = check_exposed(&open, &cli(&["serve"])).unwrap_err();
        assert!(
            err.contains("--allow-host, --max-requests, --max-rate, --max-concurrency"),
            "{err}"
        );
        let held = cli(&[
            "--allow-host",
            ".staging.test",
            "--max-requests",
            "1000",
            "--max-rate",
            "50",
            "--max-concurrency",
            "16",
            "serve",
        ]);
        assert!(check_exposed(&open, &held).is_ok());
    }
}
