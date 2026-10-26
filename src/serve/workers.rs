//! The workers that joined a server (`pepe worker --join`): each holds a
//! WebSocket it opened itself, so a machine behind NAT can take a share of
//! a distributed run. Text messages, JSON, both ways:
//!
//! - worker → server: `hello` (its name, version, guardrails), `event` (a
//!   line of a share's `--events`), `done` (the share's exit code and
//!   report, or why there is none)
//! - server → worker: `run` (a share: its id, its arguments, and the saved
//!   test's file when it is one), `steer` (a line for the share's stdin)

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use hyper::body::Incoming;
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::http::{self, Reply};
use super::Server;

struct Worker {
    since: String,
    version: String,
    guard: Option<String>,
    to: mpsc::UnboundedSender<String>,
    /// The run it has a share of
    busy: Option<String>,
}

#[derive(Default)]
pub struct Workers {
    joined: Mutex<HashMap<String, Worker>>,
}

impl Workers {
    pub fn list(&self) -> Vec<Value> {
        let Ok(joined) = self.joined.lock() else {
            return Vec::new();
        };
        let mut out: Vec<Value> = joined
            .iter()
            .map(|(name, w)| {
                json!({
                    "name": name,
                    "since": w.since,
                    "pepe": w.version,
                    "guard": w.guard,
                    "busy": w.busy,
                })
            })
            .collect();
        out.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
        out
    }

    /// The idle workers asked for (`all`, or names), marked busy with
    /// `run`: a name and where its messages go each
    pub fn take(
        &self,
        asked: &Value,
        run: &str,
    ) -> Result<Vec<(String, mpsc::UnboundedSender<String>)>, String> {
        let Ok(mut joined) = self.joined.lock() else {
            return Err("the workers can't be read".into());
        };
        let names: Vec<String> = match asked {
            Value::String(all) if all == "all" => {
                let mut idle: Vec<String> = joined
                    .iter()
                    .filter(|(_, w)| w.busy.is_none())
                    .map(|(n, _)| n.clone())
                    .collect();
                idle.sort();
                idle
            }
            Value::Array(names) => names
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect(),
            _ => return Err("workers is \"all\" or a list of their names".into()),
        };
        if names.is_empty() {
            return Err("no worker is free: none joined, or all have a share already".into());
        }
        for name in &names {
            match joined.get(name) {
                None => return Err(format!("no worker named {name:?} joined")),
                Some(w) if w.busy.is_some() => {
                    return Err(format!(
                        "{name} has a share of {} already",
                        w.busy.as_deref().unwrap_or("?")
                    ))
                }
                Some(_) => {}
            }
        }
        Ok(names
            .into_iter()
            .map(|name| {
                let w = joined.get_mut(&name).expect("checked above");
                w.busy = Some(run.to_string());
                (name, w.to.clone())
            })
            .collect())
    }

    /// The share of `run` on `name` is over
    pub fn free(&self, name: &str, run: &str) {
        if let Ok(mut joined) = self.joined.lock() {
            if let Some(w) = joined.get_mut(name) {
                if w.busy.as_deref() == Some(run) {
                    w.busy = None;
                }
            }
        }
    }
}

/// `GET /api/workers/connect?name=NAME`, upgraded to a WebSocket
pub fn connect(server: Arc<Server>, request: Request<Incoming>, who: &str) -> Reply {
    let upgrade = request
        .headers()
        .get("upgrade")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"));
    let Some(key) = request
        .headers()
        .get("sec-websocket-key")
        .and_then(|v| v.to_str().ok())
        .map(str::to_string)
        .filter(|_| upgrade)
    else {
        return http::error(StatusCode::BAD_REQUEST, "a worker joins over a WebSocket");
    };
    let name = http::query(&request)
        .into_iter()
        .find(|(k, _)| k == "name")
        .map(|(_, v)| v)
        .unwrap_or_default();
    if crate::store::validate_name(&name).is_err() {
        return http::error(
            StatusCode::BAD_REQUEST,
            "a worker's name is letters, digits, . - and _",
        );
    }
    if server
        .workers
        .joined
        .lock()
        .is_ok_and(|joined| joined.contains_key(&name))
    {
        return http::error(
            StatusCode::CONFLICT,
            format!("a worker named {name} has joined already"),
        );
    }
    server.audit(who, "worker_join", json!({ "worker": name }));
    tokio::spawn(async move {
        let Ok(upgraded) = hyper::upgrade::on(request).await else {
            return;
        };
        session(
            server,
            name,
            crate::ws::Socket::server(TokioIo::new(upgraded)),
        )
        .await;
    });
    Response::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("upgrade", "websocket")
        .header("connection", "Upgrade")
        .header("sec-websocket-accept", crate::ws::accept_key(&key))
        .body(http::full(""))
        .expect("a valid response")
}

/// A worker's connection, until it goes. Its shares that were going end
/// as failed: the run's other shares go on.
async fn session(server: Arc<Server>, name: String, mut socket: crate::ws::Socket) {
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    if let Ok(mut joined) = server.workers.joined.lock() {
        joined.insert(
            name.clone(),
            Worker {
                since: crate::store::stamp(std::time::SystemTime::now()),
                version: String::new(),
                guard: None,
                to: tx,
                busy: None,
            },
        );
    }
    loop {
        tokio::select! {
            message = socket.recv() => {
                let Ok(Some(text)) = message else { break };
                let Ok(message) = serde_json::from_str::<Value>(&text) else { continue };
                heard(&server, &name, message);
            }
            out = rx.recv() => {
                let Some(text) = out else { break };
                if socket.send(&text).await.is_err() {
                    break;
                }
            }
        }
    }
    let busy = server
        .workers
        .joined
        .lock()
        .ok()
        .and_then(|mut joined| joined.remove(&name))
        .and_then(|w| w.busy);
    if let Some(run) = busy.and_then(|id| find(&server, &id)) {
        run.worker_done(&name, -1, None, Some("the worker went away".into()));
    }
    server.audit(&name, "worker_left", json!({}));
}

fn find(server: &Server, id: &str) -> Option<Arc<super::runs::Run>> {
    server.runs.lock().ok()?.get(id).cloned()
}

/// What a worker said
fn heard(server: &Arc<Server>, name: &str, message: Value) {
    match message["type"].as_str() {
        Some("hello") => {
            if let Ok(mut joined) = server.workers.joined.lock() {
                if let Some(w) = joined.get_mut(name) {
                    w.version = message["pepe"].as_str().unwrap_or("").to_string();
                    w.guard = message["guard"].as_str().map(str::to_string);
                }
            }
        }
        Some("event") => {
            let Some(run) = message["id"].as_str().and_then(|id| find(server, id)) else {
                return;
            };
            run.worker_event(name, message["event"].clone());
        }
        Some("done") => {
            let Some(id) = message["id"].as_str() else {
                return;
            };
            server.workers.free(name, id);
            let Some(run) = find(server, id) else { return };
            let report = Some(message["report"].clone()).filter(|r| r.is_object());
            run.worker_done(
                name,
                message["code"].as_i64().unwrap_or(-1) as i32,
                report,
                message["error"].as_str().map(str::to_string),
            );
        }
        _ => {}
    }
}
