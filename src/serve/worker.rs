//! `pepe worker --join http://server:7878`: this machine takes shares of
//! a server's distributed runs. It dials out (so it works from behind
//! NAT), and runs each share as a child `pepe` under its own guardrails:
//! the flags it was started with, the `pepe.toml` where it runs and the
//! machine's settings, whatever the server asks. The share's events go
//! back as they come; its report, with its latency histogram whole, when
//! it ends.

use std::collections::HashMap;
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::cli::{Cli, WorkerArgs};

/// How long to wait before joining again, at first and at most
const BACK_OFF: Duration = Duration::from_secs(1);
const BACK_OFF_MOST: Duration = Duration::from_secs(30);
/// How often a share's events are looked at
const FOLLOW_EVERY: Duration = Duration::from_millis(200);

/// The token from the file (`name token` or the token alone, the first
/// line that has one), else `$PEPE_TOKEN`
fn token(args: &WorkerArgs) -> Result<String, String> {
    if let Some(path) = &args.token_file {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
        let tokens = super::read_tokens(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        return Ok(tokens[0].1.clone());
    }
    std::env::var("PEPE_TOKEN")
        .ok()
        .filter(|t| !t.trim().is_empty())
        .map(|t| t.trim().to_string())
        .ok_or_else(|| "a worker needs the server's token: --token-file, or $PEPE_TOKEN".into())
}

/// `http://host:7878` → `ws://host:7878/api/workers/connect?name=NAME`
pub fn connect_url(join: &str, name: &str) -> Result<String, String> {
    let mut url = reqwest::Url::parse(join).map_err(|e| format!("--join {join}: {e}"))?;
    let scheme = match url.scheme() {
        "http" | "ws" => "ws",
        "https" | "wss" => "wss",
        other => {
            return Err(format!(
                "--join is an http:// or https:// URL, not {other}://"
            ))
        }
    };
    url.set_scheme(scheme)
        .map_err(|_| "--join: can't speak WebSocket to it")?;
    url.set_path("/api/workers/connect");
    url.query_pairs_mut().clear().append_pair("name", name);
    Ok(url.to_string())
}

pub async fn run(cli: &Cli, args: &WorkerArgs) -> Result<(), Box<dyn std::error::Error>> {
    let token = token(args)?;
    let name = args
        .name
        .clone()
        .unwrap_or_else(|| gethostname::gethostname().to_string_lossy().into_owned());
    crate::store::validate_name(&name).map_err(|e| format!("--name {name:?}: {e}"))?;
    let url = connect_url(&args.join, &name)?;
    let guard = crate::mcp::guard_args(cli);
    let described = crate::guard::Guard::of(cli).describe();
    let exe = std::env::current_exe()?;
    let dir = std::env::temp_dir().join(format!("pepe-worker-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    eprintln!(
        "pepe worker {name} · joining {} · {}",
        args.join,
        described.as_deref().unwrap_or("no guardrails of its own")
    );
    let mut wait = BACK_OFF;
    loop {
        let auth = format!("Bearer {token}");
        match crate::ws::connect_with(&url, &[("Authorization", &auth)]).await {
            Ok(socket) => {
                eprintln!("joined");
                wait = BACK_OFF;
                let worker = Session {
                    name: name.clone(),
                    exe: exe.clone(),
                    guard: guard.clone(),
                    described: described.clone(),
                    dir: dir.clone(),
                };
                if let Err(e) = worker.serve(socket).await {
                    eprintln!("left the server: {e}");
                }
            }
            Err(e) => eprintln!("couldn't join: {e}; again in {}s", wait.as_secs()),
        }
        tokio::time::sleep(wait).await;
        wait = (wait * 2).min(BACK_OFF_MOST);
    }
}

struct Session {
    name: String,
    exe: PathBuf,
    guard: Vec<String>,
    described: Option<String>,
    dir: PathBuf,
}

/// A share that is going: where its stdin lines go, and its process
struct Going {
    stdin: mpsc::UnboundedSender<String>,
    pid: Option<u32>,
    ping: bool,
}

impl Session {
    async fn serve(&self, mut socket: crate::ws::Socket) -> Result<(), String> {
        let hello = json!({
            "type": "hello",
            "name": self.name,
            "pepe": env!("CARGO_PKG_VERSION"),
            "guard": self.described,
        });
        socket.send(&hello.to_string()).await?;
        let (out, mut outgoing) = mpsc::unbounded_channel::<String>();
        let mut going: HashMap<String, Going> = HashMap::new();
        let (ended, mut finished) = mpsc::unbounded_channel::<String>();
        loop {
            tokio::select! {
                message = socket.recv() => {
                    let Some(text) = message? else { return Err("the server closed".into()) };
                    let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
                    let Some(id) = message["id"].as_str().map(str::to_string) else { continue };
                    match message["type"].as_str() {
                        Some("run") => match self.start(&id, &message, out.clone(), ended.clone()) {
                            Ok(share) => { going.insert(id, share); }
                            Err(e) => {
                                let done = json!({"type": "done", "id": id, "code": -1, "report": null, "error": e});
                                socket.send(&done.to_string()).await?;
                            }
                        },
                        Some("steer") => {
                            let line = message["line"].as_str().unwrap_or("");
                            if let Some(share) = going.get(&id) {
                                if line == "stop" && share.ping {
                                    interrupt(share.pid);
                                } else {
                                    let _ = share.stdin.send(format!("{line}\n"));
                                }
                            }
                        }
                        _ => {}
                    }
                }
                text = outgoing.recv() => {
                    if let Some(text) = text {
                        socket.send(&text).await?;
                    }
                }
                id = finished.recv() => {
                    if let Some(id) = id {
                        going.remove(&id);
                    }
                }
            }
        }
    }

    /// Start a share: `pepe` with this worker's guardrails first, then the
    /// share's arguments, its events followed into messages
    fn start(
        &self,
        id: &str,
        message: &Value,
        out: mpsc::UnboundedSender<String>,
        ended: mpsc::UnboundedSender<String>,
    ) -> Result<Going, String> {
        crate::store::validate_name(id).map_err(|_| "a run's id is a name".to_string())?;
        let mut argv: Vec<String> = message["argv"]
            .as_array()
            .ok_or("a share has its arguments")?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect();
        let dir = self.dir.join(id);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // A saved test: its file, as the server has it
        if let Some(config) = message["config"].as_str() {
            crate::config::parse(config).map_err(|e| format!("the test's file: {e}"))?;
            let path = dir.join("test.toml");
            std::fs::write(&path, config).map_err(|e| e.to_string())?;
            argv.splice(0..0, ["--config".to_string(), path.display().to_string()]);
        }
        let events = dir.join("events.jsonl");
        let ping = argv.first().is_some_and(|a| a == "ping");
        let mut child = tokio::process::Command::new(&self.exe)
            .args(&self.guard)
            .arg("--events")
            .arg(&events)
            .arg("--histogram")
            .args(&argv)
            .env("PEPE_NO_UPDATE_CHECK", "1")
            .env(crate::control::STEER_ENV, id)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("couldn't start pepe: {e}"))?;
        let (stdin_tx, mut stdin_rx) = mpsc::unbounded_channel::<String>();
        if let Some(mut stdin) = child.stdin.take() {
            tokio::spawn(async move {
                while let Some(line) = stdin_rx.recv().await {
                    if stdin.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                    let _ = stdin.flush().await;
                }
            });
        }
        let pid = child.id();
        let id = id.to_string();
        tokio::spawn(async move {
            let mut follower = Follower::new(events.clone());
            let wait = child.wait_with_output();
            tokio::pin!(wait);
            let output = loop {
                tokio::select! {
                    output = &mut wait => break output,
                    _ = tokio::time::sleep(FOLLOW_EVERY) => {
                        for event in follower.next() {
                            let _ = out.send(json!({"type": "event", "id": id, "event": event}).to_string());
                        }
                    }
                }
            };
            let mut report = None;
            for event in follower.next() {
                if event["type"] == "report" {
                    report = Some(event["report"].clone());
                }
                let _ = out.send(json!({"type": "event", "id": id, "event": event}).to_string());
            }
            let report = report.or(follower.report);
            let (code, error) = match &output {
                Ok(o) => (
                    o.status.code().unwrap_or(-1),
                    String::from_utf8_lossy(&o.stderr).trim().to_string(),
                ),
                Err(e) => (-1, e.to_string()),
            };
            let done = json!({
                "type": "done",
                "id": id,
                "code": code,
                "report": report,
                "error": (!error.is_empty()).then_some(error),
            });
            let _ = out.send(done.to_string());
            let _ = ended.send(id.clone());
            let _ = std::fs::remove_dir_all(events.parent().unwrap_or(Path::new(".")));
        });
        Ok(Going {
            stdin: stdin_tx,
            pid,
            ping,
        })
    }
}

/// The new whole lines of a share's events, each time it is asked
struct Follower {
    path: PathBuf,
    offset: u64,
    partial: String,
    /// The report, once a `report` line went by
    report: Option<Value>,
}

impl Follower {
    fn new(path: PathBuf) -> Self {
        Follower {
            path,
            offset: 0,
            partial: String::new(),
            report: None,
        }
    }

    fn next(&mut self) -> Vec<Value> {
        let mut out = Vec::new();
        let Ok(mut file) = std::fs::File::open(&self.path) else {
            return out;
        };
        if file.seek(std::io::SeekFrom::Start(self.offset)).is_err() {
            return out;
        }
        let mut text = String::new();
        if file.read_to_string(&mut text).is_err() {
            return out;
        }
        self.offset += text.len() as u64;
        self.partial.push_str(&text);
        while let Some(end) = self.partial.find('\n') {
            let line: String = self.partial.drain(..=end).collect();
            if let Ok(event) = serde_json::from_str::<Value>(line.trim_end()) {
                if event["type"] == "report" {
                    self.report = Some(event["report"].clone());
                }
                out.push(event);
            }
        }
        out
    }
}

#[cfg(unix)]
fn interrupt(pid: Option<u32>) {
    if let Some(pid) = pid {
        // SAFETY: a signal to our own child, by the id the OS gave it
        unsafe {
            libc::kill(pid as libc::pid_t, libc::SIGINT);
        }
    }
}

#[cfg(not(unix))]
fn interrupt(_pid: Option<u32>) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_join_url_becomes_the_websocket_one() {
        assert_eq!(
            connect_url("http://10.0.0.5:7878", "eu-1").unwrap(),
            "ws://10.0.0.5:7878/api/workers/connect?name=eu-1"
        );
        assert_eq!(
            connect_url("https://pepe.internal/", "us").unwrap(),
            "wss://pepe.internal/api/workers/connect?name=us"
        );
        assert!(connect_url("ftp://x", "a").is_err());
    }
}
