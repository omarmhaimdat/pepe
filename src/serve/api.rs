//! The routes. Everything under `/api/` but `/api/health` takes a token.
//!
//! A run is asked for in the arguments `pepe mcp`'s tools take (`url`,
//! `requests`, `concurrency`, `rate`, `headers`, …), or as a saved test by
//! its name, and becomes a child `pepe` the same way: a value is never
//! read as a flag. What would reach past the guardrails or the machine
//! (webhooks, `--otlp`'s listener, files outside the project) isn't taken.

use std::path::{Component, Path};
use std::sync::Arc;

use hyper::body::Incoming;
use hyper::{Method, Request, StatusCode};
use serde_json::{json, Map, Value};
use tokio::sync::mpsc;

use super::http::{self, Reply};
use super::runs::{self, Share};
use super::Server;

/// What a request may not set: the server's own flags and its
/// `pepe.toml` do
const NOT_HERE: [&str; 6] = ["webhook", "webhook_on", "slack", "otlp", "svg", "save"];
/// What names a file: it has to be in the project
const PATHS: [&str; 5] = ["spec", "file", "log", "vars", "files"];
const MODES: [&str; 6] = ["run", "ping", "ramp", "api", "flow", "replay"];

/// One start at a time: the count of runs going and the start are one
/// step, so two requests can't both take the last place
static STARTING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

pub async fn handle(server: Arc<Server>, request: Request<Incoming>) -> Reply {
    let path = request.uri().path().to_string();
    // The page: no token to load it; it asks for one before it calls the API
    if request.method() == Method::GET && !path.starts_with("/api") {
        return super::ui::serve(&path);
    }
    if path == "/api/health" || path == "/api" || path == "/api/" {
        return http::ok(&json!({
            "pepe": env!("CARGO_PKG_VERSION"),
            "schema_version": 1,
            "docs": "https://pepe.mhaimdat.com/docs/serve.html",
        }));
    }
    let Some(rest) = path.strip_prefix("/api/") else {
        return http::not_found();
    };
    // A report opened by its ticket, as a plain link from the page
    if request.method() == Method::GET {
        if let Some(reply) = by_ticket(&server, &request, rest) {
            return reply;
        }
    }
    let Some(who) = http::who(&request, &server.tokens).map(str::to_string) else {
        let mut reply = http::error(
            StatusCode::UNAUTHORIZED,
            "a token is needed: Authorization: Bearer TOKEN",
        );
        reply
            .headers_mut()
            .insert("www-authenticate", "Bearer".parse().expect("a header"));
        return reply;
    };
    let parts: Vec<String> = rest
        .trim_end_matches('/')
        .split('/')
        .map(|p| p.to_string())
        .collect();
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    let method = request.method().clone();
    match (&method, parts.as_slice()) {
        (&Method::GET, ["runs"]) => list_runs(&server, &request),
        (&Method::POST, ["runs"]) => match http::body(request).await {
            Ok(body) => match start(&server, &who, body).await {
                Ok(view) => http::json(StatusCode::ACCEPTED, &view),
                Err((status, why)) => http::error(status, why),
            },
            Err(reply) => *reply,
        },
        (&Method::GET, ["runs", id]) => match find(&server, id) {
            Some(run) => http::ok(&run.view(true)),
            None => http::not_found(),
        },
        (&Method::GET, ["runs", id, "events"]) => match find(&server, id) {
            Some(run) => {
                let after = request
                    .headers()
                    .get("last-event-id")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse().ok())
                    .or_else(|| {
                        http::query(&request)
                            .into_iter()
                            .find(|(k, _)| k == "after")
                            .and_then(|(_, v)| v.parse().ok())
                    })
                    .unwrap_or(0);
                let (tx, rx) = mpsc::channel(64);
                tokio::spawn(runs::follow(run, after, tx));
                http::events(rx)
            }
            None => http::not_found(),
        },
        (&Method::GET, ["runs", id, "report"]) => {
            let format = http::query(&request)
                .into_iter()
                .find(|(k, _)| k == "format")
                .map(|(_, v)| v)
                .unwrap_or_else(|| "json".into());
            report(&server, id, &format)
        }
        (&Method::GET, ["runs", id, "report.html"]) => report(&server, id, "html"),
        (&Method::POST, ["runs", id, "ticket"]) => match find(&server, id) {
            Some(_) => http::ok(&json!({
                "ticket": server.ticket(id),
                "expires_in_s": super::TICKET_LIFE.as_secs(),
            })),
            None => http::not_found(),
        },
        (&Method::POST, ["runs", id, action @ ("pause" | "resume" | "stop")]) => {
            steer(&server, &who, id, action)
        }
        (&Method::POST, ["runs", id, "concurrency"]) => match http::body(request).await {
            Ok(body) => match body["n"].as_u64().filter(|n| *n > 0) {
                Some(n) => steer(&server, &who, id, &format!("concurrency {n}")),
                None => http::error(StatusCode::BAD_REQUEST, "{\"n\": 40}: how many at once"),
            },
            Err(reply) => *reply,
        },
        (&Method::GET, ["tests"]) => list_tests(&server),
        (&Method::GET, ["tests", name]) => get_test(&server, name),
        (&Method::PUT, ["tests", name]) => {
            let name = name.to_string();
            match http::body(request).await {
                Ok(body) => put_test(&server, &who, &name, &body),
                Err(reply) => *reply,
            }
        }
        (&Method::DELETE, ["tests", name]) => delete_test(&server, &who, name),
        (&Method::GET, ["tests", name, "runs"]) => test_runs(&server, name),
        (&Method::GET, ["tests", name, "runs", stem]) => match recorded(&server, name, stem) {
            Ok(path) => match std::fs::read_to_string(path)
                .ok()
                .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            {
                Some(report) => http::ok(&report),
                None => http::not_found(),
            },
            Err(why) => http::error(StatusCode::NOT_FOUND, why),
        },
        (&Method::GET, ["compare"]) => compare(&server, &request).await,
        (&Method::GET, ["schema", kind]) => {
            match crate::schema::ALL.iter().find(|(n, _)| n == kind) {
                Some((_, text)) => {
                    http::text(StatusCode::OK, "application/schema+json", text.to_string())
                }
                None => http::error(
                    StatusCode::NOT_FOUND,
                    "the schemas are run, ramp, ping and compare",
                ),
            }
        }
        (&Method::GET, ["files"]) => list_files(&server),
        (&Method::PUT, ["files", name]) => {
            let name = name.to_string();
            put_file(&server, &who, &name, request).await
        }
        (&Method::DELETE, ["files", name]) => delete_file(&server, &who, name),
        (&Method::POST, ["curl"]) => match http::body(request).await {
            Ok(body) => from_curl(&body),
            Err(reply) => *reply,
        },
        (&Method::GET, ["spec"]) => spec(&server, &request).await,
        (&Method::GET, ["workers"]) => http::ok(&json!({ "workers": server.workers.list() })),
        (&Method::GET, ["workers", "connect"]) => {
            super::workers::connect(server.clone(), request, &who)
        }
        (&Method::GET, ["schedules"]) => http::ok(&json!({ "schedules": server.schedules.list() })),
        (&Method::POST, ["schedules"]) => match http::body(request).await {
            Ok(body) => add_schedule(&server, &who, &body),
            Err(reply) => *reply,
        },
        (&Method::DELETE, ["schedules", id]) => {
            if server.schedules.remove(id) {
                server.audit(&who, "schedule_removed", json!({ "id": id }));
                http::ok(&json!({ "removed": id }))
            } else {
                http::not_found()
            }
        }
        _ => http::not_found(),
    }
}

/// `GET /api/runs/ID/report?ticket=…`: a run's report without the token,
/// for a ticket made for that run; None when the request is anything else
fn by_ticket(server: &Server, request: &Request<Incoming>, rest: &str) -> Option<Reply> {
    let query = http::query(request);
    let ticket = query.iter().find(|(k, _)| k == "ticket").map(|(_, v)| v)?;
    let parts: Vec<&str> = rest.trim_end_matches('/').split('/').collect();
    let (id, html) = match parts.as_slice() {
        ["runs", id, "report"] => (*id, false),
        ["runs", id, "report.html"] => (*id, true),
        _ => {
            return Some(http::error(
                StatusCode::UNAUTHORIZED,
                "a ticket opens a run's report, nothing else",
            ))
        }
    };
    if !server.ticket_opens(ticket, id) {
        return Some(http::error(
            StatusCode::UNAUTHORIZED,
            "the ticket is for another run, or it ran out: open the report from the page again",
        ));
    }
    let format = match html {
        true => "html".to_string(),
        false => query
            .iter()
            .find(|(k, _)| k == "format")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "json".into()),
    };
    Some(report(server, id, &format))
}

fn find(server: &Server, id: &str) -> Option<Arc<runs::Run>> {
    server.runs.lock().ok()?.get(id).cloned()
}

fn list_runs(server: &Server, request: &Request<Incoming>) -> Reply {
    let limit = http::query(request)
        .into_iter()
        .find(|(k, _)| k == "limit")
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(50);
    let mut metas: Vec<runs::Meta> = server
        .runs
        .lock()
        .map(|runs| runs.values().map(|r| r.meta()).collect())
        .unwrap_or_default();
    metas.sort_by(|a, b| b.started_unix.cmp(&a.started_unix).then(b.id.cmp(&a.id)));
    metas.truncate(limit);
    http::ok(&json!({ "runs": metas, "going": runs::going(server), "max_runs": server.max_runs }))
}

/// A path a request names: relative, inside the project, there
fn in_project(server: &Server, path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(format!(
            "{path:?}: a file is named from the project's directory, inside it"
        ));
    }
    if !server.root.join(p).is_file() {
        return Err(format!("{path:?}: no such file in the project"));
    }
    Ok(())
}

/// Split `total` into `parts` as even as can be, the larger first
fn split(total: u64, parts: usize) -> Vec<u64> {
    let parts = parts.max(1) as u64;
    (0..parts)
        .map(|i| total / parts + u64::from(i < total % parts))
        .collect()
}

/// Start a run as `body` asks: its id and how it stands, or why not
pub async fn start(
    server: &Arc<Server>,
    who: &str,
    body: Value,
) -> Result<Value, (StatusCode, String)> {
    let bad = |why: String| (StatusCode::BAD_REQUEST, why);
    let Value::Object(mut body) = body else {
        return Err(bad("the body is a JSON object".into()));
    };
    if let Some(key) = NOT_HERE.iter().find(|k| body.contains_key(**k)) {
        return Err(bad(format!(
            "{key} isn't taken over the API: the server's own flags and its pepe.toml set it"
        )));
    }
    for key in PATHS {
        match body.get(key) {
            Some(Value::String(path)) => in_project(server, path).map_err(bad)?,
            Some(Value::Array(paths)) => {
                for path in paths.iter().filter_map(Value::as_str) {
                    in_project(server, path).map_err(bad)?;
                }
            }
            _ => {}
        }
    }
    let dry = body.remove("dry_run") == Some(json!(true));
    let workers = body.remove("workers");
    let test = body
        .remove("test")
        .and_then(|t| t.as_str().map(str::to_string));
    let value = Value::Object(body.clone());

    let store = server.store();
    let (mode, argv, config) = match &test {
        Some(name) => {
            crate::store::validate_name(name).map_err(bad)?;
            let (path, config) = store.load(name).map_err(|e| (StatusCode::NOT_FOUND, e))?;
            let mode = if config.ramp.is_some() {
                "ramp"
            } else if config.api.is_some() {
                "api"
            } else if config.ping.is_some() {
                "ping"
            } else {
                "run"
            };
            let mut argv: Vec<String> = vec!["--load".into(), name.clone(), "--json".into()];
            if workers.is_none() {
                // Kept with the test's other runs, for history and compare
                argv.push("--record".into());
            }
            crate::mcp::load_flags(&mut argv, &value);
            let text = std::fs::read_to_string(&path).ok();
            (mode.to_string(), argv, text)
        }
        None => {
            let mode = body
                .remove("mode")
                .and_then(|m| m.as_str().map(str::to_string))
                .unwrap_or_else(|| "run".into());
            if !MODES.contains(&mode.as_str()) {
                return Err(bad(format!("mode is one of {}", MODES.join(", "))));
            }
            let value = Value::Object(body.clone());
            let argv = crate::mcp::command_for(&mode, &value).map_err(bad)?;
            (mode, argv, None)
        }
    };

    // The plan, which is the guardrails' say over the whole of it
    let plan = runs::plan(server, &argv).await?;
    if dry {
        return Ok(json!({ "plan": plan }));
    }

    let _starting = STARTING.lock().await;
    if runs::going(server) >= server.max_runs {
        return Err((
            StatusCode::TOO_MANY_REQUESTS,
            format!(
                "{} run{} going already, the most this server takes (--max-runs)",
                server.max_runs,
                if server.max_runs == 1 { "" } else { "s" }
            ),
        ));
    }
    let run = match workers {
        None => runs::start_local(server, &mode, test.as_deref(), who, argv.clone())
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?,
        Some(asked) => {
            if mode.as_str() != "run" {
                return Err(bad(
                    "only a plain run is shared out to workers, for now".into()
                ));
            }
            let id = runs::new_id(server);
            let taken = server
                .workers
                .take(&asked, &id)
                .map_err(|e| (StatusCode::CONFLICT, e))?;
            let shares = shares(&value, config.as_deref(), &taken).inspect_err(|_| {
                for (name, _) in &taken {
                    server.workers.free(name, &id);
                }
            })?;
            runs::start_remote(server, id, &mode, test.as_deref(), who, &argv, shares)
                .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
        }
    };
    server.audit(
        who,
        "run",
        json!({ "id": run.id, "command": run.meta().command, "workers": run.meta().workers }),
    );
    Ok(run.view(false))
}

/// Each worker's share of a run: the count, the concurrency and the rate
/// split as evenly as they go; a duration is each one's whole
fn shares(
    body: &Value,
    config: Option<&str>,
    workers: &[(String, mpsc::UnboundedSender<String>)],
) -> Result<Vec<Share>, (StatusCode, String)> {
    let bad = |why: String| (StatusCode::BAD_REQUEST, why);
    let file = config.map(crate::config::parse).transpose().map_err(bad)?;
    let number = |key: &str, from_file: Option<f64>| body[key].as_f64().or(from_file);
    let k = workers.len();
    let concurrency = number(
        "concurrency",
        file.as_ref().and_then(|f| f.concurrency).map(f64::from),
    )
    .ok_or_else(|| bad("a run shared out says its concurrency, to split".into()))?
        as u64;
    if concurrency < k as u64 {
        return Err(bad(format!(
            "-c {concurrency} can't be shared out to {k} workers: at least one each"
        )));
    }
    let requests = number(
        "requests",
        file.as_ref().and_then(|f| f.requests).map(|n| n as f64),
    )
    .map(|n| n as u64);
    let duration = body["duration"]
        .as_str()
        .map(str::to_string)
        .or_else(|| file.as_ref().and_then(|f| f.duration.clone()));
    if requests.is_none() && duration.is_none() {
        return Err(bad(
            "a run shared out says its requests or its duration".into()
        ));
    }
    if requests.is_some_and(|n| n < k as u64) {
        return Err(bad(format!(
            "{} requests can't be shared out to {k} workers",
            requests.unwrap_or(0)
        )));
    }
    let rate = number("rate", file.as_ref().and_then(|f| f.rate));
    let concurrencies = split(concurrency, k);
    let counts = requests.map(|n| split(n, k));
    Ok(workers
        .iter()
        .enumerate()
        .map(|(i, (name, to))| {
            let mut share: Map<String, Value> = body.as_object().cloned().unwrap_or_default();
            share.insert("concurrency".into(), json!(concurrencies[i]));
            if let Some(counts) = &counts {
                share.insert("requests".into(), json!(counts[i]));
                share.remove("duration");
            } else if let Some(d) = &duration {
                share.insert("duration".into(), json!(d));
            }
            if let Some(rate) = rate {
                share.insert("rate".into(), json!(rate / k as f64));
            }
            let share = Value::Object(share);
            let argv = match config {
                // The test's file goes along; these say the share
                Some(_) => {
                    let mut argv = vec!["--json".to_string()];
                    crate::mcp::load_flags(&mut argv, &share);
                    argv
                }
                None => crate::mcp::command_for("run", &share).unwrap_or_default(),
            };
            Share {
                worker: name.clone(),
                to: to.clone(),
                argv,
                config: config.map(str::to_string),
            }
        })
        .collect())
}

fn steer(server: &Server, who: &str, id: &str, action: &str) -> Reply {
    let Some(run) = find(server, id) else {
        return http::not_found();
    };
    if !run.going() {
        return http::error(
            StatusCode::CONFLICT,
            format!("{id} isn't going: it is {}", run.meta().state),
        );
    }
    if run.meta().mode == "ping" && action != "stop" {
        return http::error(StatusCode::CONFLICT, "a ping can be stopped, not steered");
    }
    if !run.steer(action) {
        return http::error(StatusCode::CONFLICT, format!("{id} couldn't be told"));
    }
    server.audit(who, "steer", json!({ "id": id, "said": action }));
    http::json(StatusCode::ACCEPTED, &json!({ "id": id, "said": action }))
}

fn report(server: &Server, id: &str, format: &str) -> Reply {
    use crate::export::{render, Format, Snapshot};
    let Some(report) = find(server, id).and_then(|run| run.report()) else {
        return http::error(
            StatusCode::NOT_FOUND,
            "no report yet: the run is going, or it ended without one",
        );
    };
    let (format, content_type) = match format {
        "json" => (Format::Json, "application/json"),
        "html" => (Format::Html, "text/html; charset=utf-8"),
        // Plain text, so a browser shows it rather than download it
        "md" | "markdown" => (Format::Markdown, "text/plain; charset=utf-8"),
        "txt" | "text" => (Format::Text, "text/plain; charset=utf-8"),
        "csv" => (Format::Csv, "text/csv; charset=utf-8"),
        _ => {
            return http::error(
                StatusCode::BAD_REQUEST,
                "format is json, html, md, txt or csv",
            )
        }
    };
    if format == Format::Html && report["mode"] == "ping" {
        return http::error(
            StatusCode::NOT_FOUND,
            "a ping has no page yet: json, md, txt or csv",
        );
    }
    let snap = Snapshot {
        report,
        ..Default::default()
    };
    let mut reply = http::text(StatusCode::OK, content_type, render(format, &snap));
    // A report is a page of its own: its styles and its face inline, never
    // a script
    reply.headers_mut().insert(
        "content-security-policy",
        "default-src 'none'; style-src 'unsafe-inline'; img-src data:; frame-ancestors 'none'"
            .parse()
            .expect("a header"),
    );
    reply.headers_mut().insert(
        "x-content-type-options",
        "nosniff".parse().expect("a header"),
    );
    reply
}

fn list_tests(server: &Server) -> Reply {
    let store = server.store();
    let tests: Vec<Value> = store
        .names()
        .into_iter()
        .map(|name| {
            let runs = store.runs(&name);
            let last = runs
                .last()
                .and_then(|r| r.path.file_stem())
                .map(|s| s.to_string_lossy().into_owned());
            match store.load(&name) {
                Ok((_, config)) => {
                    json!({ "name": name, "config": config, "runs": runs.len(), "last_run": last })
                }
                Err(e) => json!({ "name": name, "error": e, "runs": runs.len(), "last_run": last }),
            }
        })
        .collect();
    http::ok(&json!({ "tests": tests }))
}

fn get_test(server: &Server, name: &str) -> Reply {
    let store = server.store();
    match store.load(name) {
        Ok((path, config)) => http::ok(&json!({
            "name": name,
            "config": config,
            "toml": std::fs::read_to_string(path).unwrap_or_default(),
        })),
        Err(e) => http::error(StatusCode::NOT_FOUND, e),
    }
}

fn put_test(server: &Server, who: &str, name: &str, body: &Value) -> Reply {
    if let Err(e) = crate::store::validate_name(name) {
        return http::error(StatusCode::BAD_REQUEST, e);
    }
    let header =
        format!("# pepe load test `{name}` · run it with `pepe --load {name}` in this directory");
    let text = match (body.get("toml").and_then(Value::as_str), body.get("config")) {
        (Some(toml), _) => match crate::config::parse(toml) {
            Ok(_) => toml.to_string(),
            Err(e) => return http::error(StatusCode::BAD_REQUEST, format!("the test: {e}")),
        },
        (None, Some(config)) => {
            match serde_json::from_value::<crate::config::Config>(config.clone()) {
                Ok(config) => config.to_toml_with(&header),
                Err(e) => return http::error(StatusCode::BAD_REQUEST, format!("the test: {e}")),
            }
        }
        (None, None) => {
            return http::error(
                StatusCode::BAD_REQUEST,
                "{\"config\": {…the keys of a pepe.toml…}} or {\"toml\": \"…\"}",
            )
        }
    };
    let store = server.store();
    let path = store.path(name);
    let existed = path.is_file();
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Err(e) = std::fs::write(&path, text) {
        return http::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    server.audit(who, "test_saved", json!({ "name": name }));
    http::json(
        if existed {
            StatusCode::OK
        } else {
            StatusCode::CREATED
        },
        &json!({ "name": name, "saved": true }),
    )
}

fn delete_test(server: &Server, who: &str, name: &str) -> Reply {
    let store = server.store();
    if crate::store::validate_name(name).is_err() || !store.has(name) {
        return http::not_found();
    }
    match std::fs::remove_file(store.path(name)) {
        Ok(()) => {
            server.audit(who, "test_deleted", json!({ "name": name }));
            http::ok(&json!({ "name": name, "deleted": true }))
        }
        Err(e) => http::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

fn test_runs(server: &Server, name: &str) -> Reply {
    if crate::store::validate_name(name).is_err() {
        return http::not_found();
    }
    let store = server.store();
    let runs: Vec<Value> = store
        .runs(name)
        .iter()
        .rev()
        .map(|run| {
            let stem = run
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let side = crate::compare::Side::read(&run.path).ok();
            json!({
                "id": format!("{name}/{stem}"),
                "when": stem,
                "summary": side,
            })
        })
        .collect();
    http::ok(&json!({ "test": name, "runs": runs }))
}

/// A recorded run of a test, by its stem
fn recorded(server: &Server, name: &str, stem: &str) -> Result<std::path::PathBuf, String> {
    crate::store::validate_name(name)?;
    crate::store::validate_name(stem)?;
    let path = server.store().runs_dir(name).join(format!("{stem}.json"));
    match path.is_file() {
        true => Ok(path),
        false => Err(format!("no recorded run {name}/{stem}")),
    }
}

/// A report a comparison names: a run of this server's by its id, or a
/// test's recorded run as `NAME/WHEN`
fn report_path(server: &Server, id: &str) -> Result<std::path::PathBuf, String> {
    if let Some(run) = find(server, id) {
        let path = run.dir.join("report.json");
        return match path.is_file() {
            true => Ok(path),
            false => Err(format!("{id} has no report")),
        };
    }
    match id.split_once('/') {
        Some((name, stem)) => recorded(server, name, stem),
        None => Err(format!(
            "{id:?} is no run of this server's, nor a test's NAME/WHEN"
        )),
    }
}

async fn compare(server: &Server, request: &Request<Incoming>) -> Reply {
    let query = http::query(request);
    let get = |key: &str| query.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone());
    let (Some(before), Some(after)) = (get("before"), get("after")) else {
        return http::error(
            StatusCode::BAD_REQUEST,
            "?before=…&after=…: run ids, or NAME/WHEN",
        );
    };
    let paths = report_path(server, &before).and_then(|b| Ok((b, report_path(server, &after)?)));
    let (before, after) = match paths {
        Ok(paths) => paths,
        Err(e) => return http::error(StatusCode::NOT_FOUND, e),
    };
    let mut argv = vec!["compare".to_string(), "--json".to_string()];
    if get("gate").is_some_and(|g| g == "1" || g == "true") {
        argv.push("--gate".into());
    }
    argv.push("--".into());
    argv.push(before.display().to_string());
    argv.push(after.display().to_string());
    match runs::pepe(server, &argv).await {
        // 1 is a regression under --gate: the comparison says so
        Ok((0 | 1, out, _)) => match serde_json::from_str::<Value>(&out) {
            Ok(comparison) => http::ok(&comparison),
            Err(_) => http::error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "pepe compare said nothing readable",
            ),
        },
        Ok((_, _, err)) => http::error(StatusCode::BAD_REQUEST, err),
        Err(e) => http::error(StatusCode::INTERNAL_SERVER_ERROR, e),
    }
}

/// Where the page's uploads go: in the project, so a run may name them
const FILES: &str = "files";
/// The most an upload may be: a day of an access log, a large spec
const FILE_MOST: usize = 256 * 1024 * 1024;

fn files_dir(server: &Server) -> std::path::PathBuf {
    server.dir.join(FILES)
}

/// A file's path as a run names it: from the project's directory
fn project_path(server: &Server, path: &Path) -> String {
    path.strip_prefix(&server.root)
        .unwrap_or(path)
        .display()
        .to_string()
}

fn list_files(server: &Server) -> Reply {
    let mut files: Vec<Value> = std::fs::read_dir(files_dir(server))
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().is_file())
                .map(|e| {
                    let path = e.path();
                    json!({
                        "name": e.file_name().to_string_lossy(),
                        "path": project_path(server, &path),
                        "bytes": e.metadata().map(|m| m.len()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    files.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    http::ok(&json!({ "files": files }))
}

/// `PUT /api/files/NAME`: the body, as it is, kept as a file a run can
/// name (a spec, a log, a flow, values)
async fn put_file(server: &Server, who: &str, name: &str, request: Request<Incoming>) -> Reply {
    if crate::store::validate_name(name).is_err() {
        return http::error(
            StatusCode::BAD_REQUEST,
            "a file's name is letters, digits, . - and _",
        );
    }
    let bytes = match http::read_up_to(request, FILE_MOST).await {
        Ok(bytes) => bytes,
        Err(reply) => return *reply,
    };
    let dir = files_dir(server);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return http::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    let path = dir.join(name);
    if let Err(e) = std::fs::write(&path, &bytes) {
        return http::error(StatusCode::INTERNAL_SERVER_ERROR, e.to_string());
    }
    server.audit(
        who,
        "file_saved",
        json!({ "name": name, "bytes": bytes.len() }),
    );
    http::json(
        StatusCode::CREATED,
        &json!({ "name": name, "path": project_path(server, &path), "bytes": bytes.len() }),
    )
}

fn delete_file(server: &Server, who: &str, name: &str) -> Reply {
    if crate::store::validate_name(name).is_err() {
        return http::not_found();
    }
    match std::fs::remove_file(files_dir(server).join(name)) {
        Ok(()) => {
            server.audit(who, "file_deleted", json!({ "name": name }));
            http::ok(&json!({ "name": name, "deleted": true }))
        }
        Err(_) => http::not_found(),
    }
}

/// `POST /api/curl {"command": "curl …"}`: the request a curl command
/// is, as a run's arguments, and what of it pepe can't honour
fn from_curl(body: &Value) -> Reply {
    let Some(command) = body["command"].as_str() else {
        return http::error(StatusCode::BAD_REQUEST, "{\"command\": \"curl …\"}");
    };
    match crate::curl::parse_command(command) {
        Ok(c) => {
            let mut args = json!({
                "url": c.url,
                "method": c.method,
                "headers": c.headers,
            });
            if let Some(body) = &c.body {
                args["body"] = json!(String::from_utf8_lossy(body));
            }
            if let Some(ua) = &c.user_agent {
                args["user_agent"] = json!(ua);
            }
            if let Some(t) = c.timeout_secs {
                args["timeout"] = json!(t);
            }
            if c.insecure {
                args["insecure"] = json!(true);
            }
            if c.no_keepalive {
                args["no_keepalive"] = json!(true);
            }
            if !c.follow_redirects {
                args["no_redirects"] = json!(true);
            }
            if let Some(proxy) = &c.proxy {
                args["proxy"] = json!(proxy);
            }
            http::ok(&json!({ "args": args, "notes": c.notes }))
        }
        Err(e) => http::error(StatusCode::BAD_REQUEST, e),
    }
}

/// `GET /api/spec?file=PATH`: an OpenAPI spec in the project, as the page
/// picks from it: its servers, tags, auth and every operation
async fn spec(server: &Server, request: &Request<Incoming>) -> Reply {
    let Some(file) = http::query(request)
        .into_iter()
        .find(|(k, _)| k == "file")
        .map(|(_, v)| v)
    else {
        return http::error(StatusCode::BAD_REQUEST, "?file=PATH: a spec in the project");
    };
    if let Err(e) = in_project(server, &file) {
        return http::error(StatusCode::BAD_REQUEST, e);
    }
    let path = server.root.join(&file);
    let doc = match crate::openapi::load(&path.display().to_string()).await {
        Ok(doc) => doc,
        Err(e) => return http::error(StatusCode::BAD_REQUEST, e),
    };
    match crate::openapi::Spec::parse(&doc, None, None) {
        Ok(spec) => http::ok(&json!({
            "title": spec.title,
            "version": spec.version,
            "base_url": spec.base_url,
            "auth": spec.auth.iter().map(|a| a.name.clone()).collect::<Vec<_>>(),
            "tags": spec.tags.iter().map(|(name, about)| json!({"name": name, "about": about})).collect::<Vec<_>>(),
            "operations": spec.operations.iter().map(|o| json!({
                "method": o.method,
                "path": o.path,
                "tag": o.tag,
                "summary": o.summary,
                "params": o.params.iter().map(|p| p.name.clone()).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
        })),
        Err(e) => http::error(StatusCode::BAD_REQUEST, e),
    }
}

fn add_schedule(server: &Server, who: &str, body: &Value) -> Reply {
    let Some(test) = body["test"].as_str() else {
        return http::error(
            StatusCode::BAD_REQUEST,
            "{\"test\": NAME, \"every\": \"1h\"}",
        );
    };
    if !server.store().has(test) {
        return http::error(StatusCode::NOT_FOUND, format!("no test named {test}"));
    }
    let every = match body["every"].as_str().map(crate::cli::Cli::parse_duration) {
        Some(Ok(ms)) => std::time::Duration::from_millis(ms),
        _ => return http::error(StatusCode::BAD_REQUEST, "every is a time: 15m, 1h, 1d"),
    };
    match server
        .schedules
        .add(test, every, body.get("workers").cloned(), who)
    {
        Ok(schedule) => {
            server.audit(who, "schedule_added", json!(schedule));
            http::json(StatusCode::CREATED, &json!(schedule))
        }
        Err(e) => http::error(StatusCode::BAD_REQUEST, e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A curl command becomes a run's arguments, and what curl does by
    /// default that pepe doesn't (not following redirects) is said
    #[test]
    fn a_curl_command_becomes_a_runs_arguments() {
        use http_body_util::BodyExt;
        let reply = from_curl(
            &json!({"command": "curl -X POST https://api.test/orders -H 'Authorization: Bearer t' -d '{\"id\": 1}' -k --max-time 5"}),
        );
        assert_eq!(reply.status(), StatusCode::OK);
        let body = futures::executor::block_on(reply.into_body().collect())
            .unwrap()
            .to_bytes();
        let v: Value = serde_json::from_slice(&body).unwrap();
        let args = &v["args"];
        assert_eq!(args["url"], "https://api.test/orders");
        assert_eq!(args["method"], "POST");
        assert_eq!(args["body"], "{\"id\": 1}");
        assert_eq!(args["timeout"], 5);
        assert_eq!(args["insecure"], true);
        assert_eq!(args["no_redirects"], true);
        assert!(args["headers"]
            .as_array()
            .unwrap()
            .iter()
            .any(|h| h == "Authorization: Bearer t"));
        // What becomes the run is checked as any run is: the MCP tool's
        let argv = crate::mcp::command_for("run", args).unwrap();
        assert!(
            argv.contains(&"--disable-redirects".to_string()) && argv.contains(&"-k".to_string()),
            "{argv:?}"
        );
        assert_eq!(
            from_curl(&json!({"command": "curl 'https://api.test/ unclosed"})).status(),
            StatusCode::BAD_REQUEST
        );
    }

    #[test]
    fn counts_split_evenly_the_larger_first() {
        assert_eq!(split(10, 3), [4, 3, 3]);
        assert_eq!(split(64, 2), [32, 32]);
        assert_eq!(split(5, 1), [5]);
        assert_eq!(split(7, 4).iter().sum::<u64>(), 7);
    }
}
