//! The runs a server starts: a child `pepe` on this machine, or shares
//! of one on its workers. Each has a directory under `.pepe/serve/runs/`:
//! `meta.json` (what it is and how it stands), `events.jsonl` (the run as
//! it went, which `GET /api/runs/:id/events` follows) and `report.json`.

use std::collections::HashMap;
use std::io::{BufRead, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use hyper::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use super::Server;

/// How often a follower looks for new lines in a run's events
const FOLLOW_EVERY: Duration = Duration::from_millis(200);

/// What a run is and how it stands, as `meta.json` keeps it
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Meta {
    pub id: String,
    /// run, ping, ramp, api, flow or replay
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub test: Option<String>,
    /// The token's name, or `schedule:ID`
    pub by: String,
    pub started: String,
    pub started_unix: u64,
    /// running, done, failed, stopped, or lost (the server stopped first)
    pub state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exit: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The command it is, secrets masked
    pub command: String,
    /// The workers it was shared out to; none for a run on this machine
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workers: Vec<String>,
}

/// How a run that is going is steered
enum Steer {
    /// A child here: a line on its stdin, or an interrupt for a ping
    Local {
        stdin: mpsc::UnboundedSender<String>,
        pid: Option<u32>,
    },
    /// Shares on workers: the same line to each
    Remote(Vec<mpsc::UnboundedSender<String>>),
}

pub struct Run {
    pub id: String,
    pub dir: PathBuf,
    meta: Mutex<Meta>,
    steer: Mutex<Option<Steer>>,
    /// The events a distributed run's server writes itself, numbered
    log: Mutex<Option<(std::fs::File, u64)>>,
    /// A distributed run's workers: their latest progress and, once done,
    /// their reports
    shares: Mutex<Shares>,
}

#[derive(Default)]
struct Shares {
    progress: HashMap<String, Value>,
    reports: HashMap<String, Value>,
    pending: Vec<String>,
    failed: Vec<String>,
    last_merged: Option<std::time::Instant>,
}

impl Run {
    pub fn meta(&self) -> Meta {
        self.meta
            .lock()
            .map(|m| m.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    pub fn going(&self) -> bool {
        self.meta().state == "running"
    }

    fn set(&self, change: impl FnOnce(&mut Meta)) {
        if let Ok(mut meta) = self.meta.lock() {
            change(&mut meta);
            let _ = write_json(&self.dir.join("meta.json"), &json!(*meta));
        }
    }

    pub fn events_path(&self) -> PathBuf {
        self.dir.join("events.jsonl")
    }

    pub fn report(&self) -> Option<Value> {
        let text = std::fs::read_to_string(self.dir.join("report.json")).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// How it stands, for the API: the meta, and the report once it has one
    pub fn view(&self, with_report: bool) -> Value {
        let mut view = json!(self.meta());
        if with_report {
            if let Some(report) = self.report() {
                view["report"] = report;
            }
        }
        view
    }

    /// Send a line to the run: `pause`, `resume`, `concurrency 40`,
    /// `stop`. False when it isn't going.
    pub fn steer(&self, line: &str) -> bool {
        let Ok(steer) = self.steer.lock() else {
            return false;
        };
        match steer.as_ref() {
            Some(Steer::Local { stdin, pid }) => {
                // A ping isn't steered over stdin; an interrupt ends it
                // with its report, as Ctrl-C would
                if line == "stop" && self.meta().mode == "ping" {
                    return interrupt(*pid);
                }
                stdin.send(format!("{line}\n")).is_ok()
            }
            Some(Steer::Remote(workers)) => {
                workers.iter().all(|w| w.send(line.to_string()).is_ok())
            }
            None => false,
        }
    }

    /// An event of a distributed run, numbered by the server
    fn log(&self, kind: &str, body: Value) {
        let Ok(mut log) = self.log.lock() else { return };
        let Some((file, seq)) = log.as_mut() else {
            return;
        };
        let mut line = serde_json::Map::new();
        line.insert("type".into(), json!(kind));
        line.insert("seq".into(), json!(*seq));
        if let Value::Object(body) = body {
            line.extend(body);
        }
        *seq += 1;
        let _ = writeln!(file, "{}", Value::Object(line));
        let _ = file.flush();
    }
}

#[cfg(unix)]
fn interrupt(pid: Option<u32>) -> bool {
    match pid {
        // SAFETY: a signal to our own child, by the id the OS gave it
        Some(pid) => unsafe { libc::kill(pid as libc::pid_t, libc::SIGINT) == 0 },
        None => false,
    }
}

#[cfg(not(unix))]
fn interrupt(_pid: Option<u32>) -> bool {
    false
}

fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    let text = serde_json::to_string_pretty(value).unwrap_or_default();
    // Whole or not at all: a reader never meets half a file
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, text)?;
    std::fs::rename(tmp, path)
}

/// The runs a server before this one left; one it left going is lost
pub fn load_past(dir: &Path) -> HashMap<String, Arc<Run>> {
    let mut out = HashMap::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(text) = std::fs::read_to_string(path.join("meta.json")) else {
            continue;
        };
        let Ok(mut meta) = serde_json::from_str::<Meta>(&text) else {
            continue;
        };
        if meta.state == "running" {
            meta.state = "lost".into();
            meta.error = Some("the server stopped while it was going".into());
            let _ = write_json(&path.join("meta.json"), &json!(meta));
        }
        out.insert(
            meta.id.clone(),
            Arc::new(Run {
                id: meta.id.clone(),
                dir: path,
                meta: Mutex::new(meta),
                steer: Mutex::new(None),
                log: Mutex::new(None),
                shares: Mutex::new(Shares::default()),
            }),
        );
    }
    out
}

/// A run's id: when it started, and its number since the server did
pub fn new_id(server: &Server) -> String {
    let n = server
        .started
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        + 1;
    format!("{}-{n}", crate::store::stamp(SystemTime::now()))
}

/// How many runs are going, here and on the workers
pub fn going(server: &Server) -> usize {
    server
        .runs
        .lock()
        .map(|runs| runs.values().filter(|r| r.going()).count())
        .unwrap_or(0)
}

/// The command a run is, with what's secret masked
pub fn shown(argv: &[String]) -> String {
    const SECRET: [&str; 6] = [
        "authorization",
        "proxy-authorization",
        "cookie",
        "x-api-key",
        "x-auth-token",
        "api-key",
    ];
    let words: Vec<String> = argv
        .iter()
        .map(|a| {
            let header = a.strip_prefix("-H").unwrap_or(a);
            match header.split_once(':') {
                Some((name, _))
                    if a.starts_with("-H")
                        && SECRET.contains(&name.trim().to_ascii_lowercase().as_str()) =>
                {
                    format!("-H{}: ••••", name.trim())
                }
                _ => a.clone(),
            }
        })
        .collect();
    format!("pepe {}", words.join(" "))
}

/// Run pepe with the server's guardrails first, and say what it printed:
/// the exit code, stdout and stderr
pub async fn pepe(server: &Server, argv: &[String]) -> Result<(i32, String, String), String> {
    let output = tokio::process::Command::new(&server.exe)
        .args(&server.guard)
        .args(argv)
        .current_dir(&server.root)
        .env("PEPE_NO_UPDATE_CHECK", "1")
        .stdin(std::process::Stdio::null())
        .output()
        .await
        .map_err(|e| format!("couldn't run pepe: {e}"))?;
    Ok((
        output.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).trim().to_string(),
    ))
}

/// Ask pepe what `argv` would do (`--dry-run`), which is the guardrails'
/// say: the plan, or why not
pub async fn plan(server: &Server, argv: &[String]) -> Result<Value, (StatusCode, String)> {
    let mut dry = argv.to_vec();
    // Before the `--` of what isn't a flag
    let at = dry.iter().position(|a| a == "--").unwrap_or(dry.len());
    dry.insert(at, "--dry-run".into());
    let (code, out, err) = pepe(server, &dry)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    match code {
        0 => serde_json::from_str(&out).map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("pepe's plan wasn't JSON: {}", out.trim()),
            )
        }),
        2 => Err((StatusCode::UNPROCESSABLE_ENTITY, refusal(&err))),
        _ => Err((StatusCode::BAD_REQUEST, refusal(&err))),
    }
}

/// What pepe said on stderr, without its `error: ` and usage lines
fn refusal(stderr: &str) -> String {
    let text = stderr
        .lines()
        .filter(|l| {
            !l.trim().is_empty()
                && !l.starts_with("Usage:")
                && !l.starts_with("For more information")
        })
        .map(|l| l.strip_prefix("error: ").unwrap_or(l))
        .collect::<Vec<_>>()
        .join("\n");
    match text.is_empty() {
        true => "pepe refused it".into(),
        false => text,
    }
}

fn meta_for(
    server: &Server,
    id: &str,
    mode: &str,
    test: Option<&str>,
    by: &str,
    argv: &[String],
) -> Meta {
    let now = SystemTime::now();
    let _ = server;
    Meta {
        id: id.to_string(),
        mode: mode.to_string(),
        test: test.map(str::to_string),
        by: by.to_string(),
        started: crate::store::stamp(now),
        started_unix: now.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()),
        state: "running".into(),
        exit: None,
        error: None,
        command: shown(argv),
        workers: Vec::new(),
    }
}

fn register(server: &Server, meta: Meta) -> Result<Arc<Run>, String> {
    let dir = server.dir.join("runs").join(&meta.id);
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't create {}: {e}", dir.display()))?;
    write_json(&dir.join("meta.json"), &json!(meta)).map_err(|e| e.to_string())?;
    let run = Arc::new(Run {
        id: meta.id.clone(),
        dir,
        meta: Mutex::new(meta),
        steer: Mutex::new(None),
        log: Mutex::new(None),
        shares: Mutex::new(Shares::default()),
    });
    if let Ok(mut runs) = server.runs.lock() {
        runs.insert(run.id.clone(), run.clone());
    }
    Ok(run)
}

/// Start `argv` as a child here. It writes its own events; its report is
/// the events' last line.
pub fn start_local(
    server: &Arc<Server>,
    mode: &str,
    test: Option<&str>,
    by: &str,
    argv: Vec<String>,
) -> Result<Arc<Run>, String> {
    let id = new_id(server);
    let run = register(server, meta_for(server, &id, mode, test, by, &argv))?;
    let mut command = tokio::process::Command::new(&server.exe);
    command
        .args(&server.guard)
        .arg("--events")
        .arg(run.events_path())
        .args(&argv)
        .current_dir(&server.root)
        .env("PEPE_NO_UPDATE_CHECK", "1")
        .env(crate::control::STEER_ENV, &id)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true);
    let mut child = command
        .spawn()
        .map_err(|e| format!("couldn't start pepe: {e}"))?;
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    if let Some(mut stdin) = child.stdin.take() {
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                if stdin.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                let _ = stdin.flush().await;
            }
        });
    }
    if let Ok(mut steer) = run.steer.lock() {
        *steer = Some(Steer::Local {
            stdin: tx,
            pid: child.id(),
        });
    }
    let done = run.clone();
    tokio::spawn(async move {
        let output = child.wait_with_output().await;
        if let Ok(mut steer) = done.steer.lock() {
            *steer = None;
        }
        let (code, stdout, stderr) = match output {
            Ok(o) => (
                o.status.code().unwrap_or(-1),
                String::from_utf8_lossy(&o.stdout).into_owned(),
                String::from_utf8_lossy(&o.stderr).trim().to_string(),
            ),
            Err(e) => (-1, String::new(), e.to_string()),
        };
        let report = last_report(&done.events_path())
            .or_else(|| serde_json::from_str::<Value>(stdout.trim()).ok());
        if let Some(report) = &report {
            let _ = write_json(&done.dir.join("report.json"), report);
        }
        let stopped = report.as_ref().is_some_and(|r| {
            r.pointer("/summary/interrupted") == Some(&json!(true))
                || r["interrupted"] == json!(true)
        });
        done.set(|meta| {
            meta.exit = Some(code);
            meta.state = match (code, &report) {
                (_, None) => "failed",
                _ if stopped => "stopped",
                // 4 is a limit crossed: the run happened, and failed its test
                (0 | 4, Some(_)) => "done",
                _ => "failed",
            }
            .into();
            if meta.state == "failed" && !stderr.is_empty() {
                meta.error = Some(refusal(&stderr));
            }
        });
    });
    Ok(run)
}

/// The report in a run's events: its last `report` line
fn last_report(path: &Path) -> Option<Value> {
    let file = std::fs::File::open(path).ok()?;
    std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .filter_map(|line| serde_json::from_str::<Value>(&line).ok())
        .filter(|event| event["type"] == "report")
        .last()
        .map(|mut event| event["report"].take())
}

/// What each worker of a distributed run is sent
pub struct Share {
    pub worker: String,
    pub to: mpsc::UnboundedSender<String>,
    pub argv: Vec<String>,
    pub config: Option<String>,
}

/// Start a run's shares on its workers; the server keeps its events and
/// adds their reports up when the last is in
pub fn start_remote(
    server: &Arc<Server>,
    id: String,
    mode: &str,
    test: Option<&str>,
    by: &str,
    whole: &[String],
    shares: Vec<Share>,
) -> Result<Arc<Run>, String> {
    let mut meta = meta_for(server, &id, mode, test, by, whole);
    meta.workers = shares.iter().map(|s| s.worker.clone()).collect();
    let run = register(server, meta)?;
    let file = std::fs::File::create(run.events_path()).map_err(|e| e.to_string())?;
    if let Ok(mut log) = run.log.lock() {
        *log = Some((file, 1));
    }
    if let Ok(mut s) = run.shares.lock() {
        s.pending = shares.iter().map(|s| s.worker.clone()).collect();
    }
    run.log(
        "start",
        json!({ "mode": mode, "test": test, "workers": run.meta().workers, "command": shown(whole) }),
    );
    let mut steer = Vec::new();
    for share in shares {
        let message = json!({
            "type": "run",
            "id": id,
            "argv": share.argv,
            "config": share.config,
        });
        if share.to.send(message.to_string()).is_err() {
            run.worker_done(&share.worker, -1, None, Some("the worker went away".into()));
            continue;
        }
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let to = share.to.clone();
        let run_id = id.clone();
        tokio::spawn(async move {
            while let Some(line) = rx.recv().await {
                let message = json!({ "type": "steer", "id": run_id, "line": line });
                if to.send(message.to_string()).is_err() {
                    break;
                }
            }
        });
        steer.push(tx);
    }
    if let Ok(mut s) = run.steer.lock() {
        *s = Some(Steer::Remote(steer));
    }
    Ok(run)
}

impl Run {
    /// An event a worker's share sent: kept as it came, under the worker's
    /// name, and the shares' progress added up a second at a time
    pub fn worker_event(&self, worker: &str, event: Value) {
        if event["type"] == "progress" {
            let merged = {
                let Ok(mut shares) = self.shares.lock() else {
                    return;
                };
                shares.progress.insert(worker.to_string(), event.clone());
                let due = shares
                    .last_merged
                    .is_none_or(|at| at.elapsed() >= crate::events::EVERY);
                if due {
                    shares.last_merged = Some(std::time::Instant::now());
                    Some(super::merge::progress(&shares.progress))
                } else {
                    None
                }
            };
            if let Some(merged) = merged {
                self.log("progress", merged);
            }
            return;
        }
        if event["type"] == "report" {
            // It comes with `done`
            return;
        }
        let mut event = event;
        if let Some(object) = event.as_object_mut() {
            let kind = object.remove("type").unwrap_or(json!("event"));
            object.remove("seq");
            object.insert("worker".into(), json!(worker));
            self.log(
                kind.as_str().unwrap_or("event"),
                Value::Object(object.clone()),
            );
        }
    }

    /// A worker's share ended: its report, or why there is none. With the
    /// last, the shares' reports are added up into the run's.
    pub fn worker_done(
        &self,
        worker: &str,
        code: i32,
        report: Option<Value>,
        error: Option<String>,
    ) {
        let finished = {
            let Ok(mut shares) = self.shares.lock() else {
                return;
            };
            if !shares.pending.iter().any(|w| w == worker) {
                return;
            }
            shares.pending.retain(|w| w != worker);
            match report {
                Some(report) => {
                    shares.reports.insert(worker.to_string(), report);
                }
                None => shares.failed.push(format!(
                    "{worker}: {}",
                    error.clone().unwrap_or_else(|| format!("exit {code}"))
                )),
            }
            shares.pending.is_empty().then(|| {
                let mut reports: Vec<(String, Value)> = shares.reports.drain().collect();
                reports.sort_by(|a, b| a.0.cmp(&b.0));
                (reports, shares.failed.clone())
            })
        };
        self.log(
            "worker_done",
            json!({ "worker": worker, "exit": code, "error": error }),
        );
        let Some((reports, failed)) = finished else {
            return;
        };
        if let Ok(mut steer) = self.steer.lock() {
            *steer = None;
        }
        let merged = (!reports.is_empty()).then(|| super::merge::reports(&reports, &failed));
        if let Some(report) = &merged {
            let _ = write_json(&self.dir.join("report.json"), report);
            self.log("report", json!({ "report": report }));
        }
        let interrupted = merged
            .as_ref()
            .is_some_and(|r| r.pointer("/summary/interrupted") == Some(&json!(true)));
        self.set(|meta| {
            meta.exit = Some(if failed.is_empty() { 0 } else { 1 });
            meta.state = match (&merged, interrupted) {
                (None, _) => "failed",
                (Some(_), true) => "stopped",
                (Some(_), false) => "done",
            }
            .into();
            if !failed.is_empty() {
                meta.error = Some(failed.join("; "));
            }
        });
        if let Ok(mut log) = self.log.lock() {
            *log = None;
        }
    }
}

/// Follow a run's events from after `after`, sending each line as a
/// Server-Sent Event, until the run is over and its last line sent, or
/// the reader goes
pub async fn follow(run: Arc<Run>, after: u64, tx: mpsc::Sender<String>) {
    let path = run.events_path();
    let mut offset = 0u64;
    let mut partial = String::new();
    let mut last = after;
    loop {
        // Read before asking whether the run is over, so its last lines
        // are never left behind
        let over = !run.going();
        if let Ok(mut file) = std::fs::File::open(&path) {
            if file.seek(std::io::SeekFrom::Start(offset)).is_ok() {
                let mut text = String::new();
                if std::io::Read::read_to_string(&mut file, &mut text).is_ok() {
                    offset += text.len() as u64;
                    partial.push_str(&text);
                }
            }
        }
        while let Some(end) = partial.find('\n') {
            let line: String = partial.drain(..=end).collect();
            let line = line.trim_end();
            let Ok(event) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let seq = event["seq"].as_u64().unwrap_or(0);
            if seq <= after {
                continue;
            }
            let kind = event["type"].as_str().unwrap_or("event");
            last = last.max(seq);
            if tx.send(super::http::event(seq, kind, line)).await.is_err() {
                return;
            }
        }
        if over {
            let meta = run.meta();
            let _ = tx
                .send(super::http::event(
                    last + 1,
                    "end",
                    &json!({ "state": meta.state, "exit": meta.exit, "error": meta.error })
                        .to_string(),
                ))
                .await;
            return;
        }
        tokio::time::sleep(FOLLOW_EVERY).await;
        if tx.is_closed() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_shown_has_its_secrets_masked() {
        let argv: Vec<String> = [
            "--json",
            "-HAuthorization: Bearer s3cret",
            "-HAccept: */*",
            "--",
            "http://x/",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        let text = shown(&argv);
        assert!(!text.contains("s3cret"), "{text}");
        assert!(text.contains("-HAuthorization: ••••"), "{text}");
        assert!(text.contains("-HAccept: */*"), "{text}");
    }

    #[test]
    fn a_refusal_is_pepes_words_without_its_noise() {
        let err = "error: 127.0.0.1 isn't an allowed host (--allow-host localhost)\n\nUsage: pepe [OPTIONS]\n";
        assert_eq!(
            refusal(err),
            "127.0.0.1 isn't an allowed host (--allow-host localhost)"
        );
    }
}
