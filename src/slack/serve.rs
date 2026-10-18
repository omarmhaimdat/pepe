//! `pepe slack serve`: the one place Slack's buttons and `/pepe` reach.
//! Slack hands every click of an app to one of its Socket Mode
//! connections, not to the process that posted the message, so a single
//! listener answers them all: left running on a machine, it runs the saved
//! tests of the directory it was started in, as `pepe --load NAME` with the
//! channel's `--slack`, under that directory's guardrails, for the people
//! allowed. It steers the runs it started (pause, resume, concurrency,
//! stop) through their stdin (see `crate::control`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use serde_json::{json, Value};
use tokio::sync::mpsc;

use super::wizard::{bad, bold, ember, faint, good};
use super::{Api, Connection};
use crate::store::Store;

/// A run this listener started, until it ends
struct Running {
    test: String,
    by: String,
    /// Lines for the run's stdin: `pause U1`, `concurrency 63 U1`, `stop U1`
    lines: mpsc::UnboundedSender<String>,
    started: Instant,
    paused: bool,
    /// The run's live message, once it's posted: questions in its thread
    /// come to this run
    message: Option<String>,
}

/// What a click, a form or a command asks for
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    Run {
        test: String,
        args: Vec<String>,
        keep: bool,
    },
    Edit {
        test: String,
        trigger: String,
    },
    Stop {
        run: Option<String>,
        test: Option<String>,
    },
    Pause {
        run: String,
    },
    Resume {
        run: String,
    },
    Size {
        run: String,
        concurrency: usize,
    },
    History {
        test: String,
    },
    /// A test picked in the dropdown: its card, in the same place
    Pick {
        test: String,
    },
    /// `/pepe`: the tests to pick from, and what runs
    Home,
    Status,
    Help,
}

/// Who asked, and where an answer only they see goes
#[derive(Debug, Clone, Default)]
pub struct From {
    pub user: String,
    pub name: String,
    pub channel: String,
    pub response_url: Option<String>,
}

/// An answer for the one who asked: text, and blocks when it's a card;
/// `replace` puts it in place of the card it came from
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Reply {
    pub text: String,
    pub blocks: Option<Value>,
    pub replace: bool,
}

impl Reply {
    fn text(text: impl Into<String>) -> Reply {
        Reply {
            text: text.into(),
            ..Reply::default()
        }
    }

    fn card(text: impl Into<String>, blocks: Value) -> Reply {
        Reply {
            text: text.into(),
            blocks: Some(blocks),
            replace: false,
        }
    }

    fn payload(&self) -> Value {
        let mut out = json!({"response_type": "ephemeral", "text": self.text});
        if let Some(blocks) = &self.blocks {
            out["blocks"] = blocks.clone();
        }
        if self.replace {
            out["replace_original"] = json!(true);
        } else {
            out["replace_original"] = json!(false);
        }
        out
    }
}

/// May `user` press the buttons? `*` lets anyone in the channel
pub fn allowed(allow: &[String], user: &str) -> bool {
    allow.iter().any(|a| a == "*" || a == user)
}

/// A button's click, or a pick in a dropdown
pub fn ask_of_action(action: &Value, trigger: &str) -> Option<Ask> {
    let value = action["value"].as_str().unwrap_or_default().to_string();
    match action["action_id"].as_str()? {
        "pepe:run" => Some(Ask::Run {
            test: value,
            args: Vec::new(),
            keep: false,
        }),
        "pepe:edit" => Some(Ask::Edit {
            test: value,
            trigger: trigger.to_string(),
        }),
        "pepe:stop" => Some(Ask::Stop {
            run: Some(value),
            test: None,
        }),
        "pepe:pause" => Some(Ask::Pause { run: value }),
        "pepe:resume" => Some(Ask::Resume { run: value }),
        // − 1 and + 1: two ids, since Slack wants each once in a block
        "pepe:less" | "pepe:more" | "pepe:size" => {
            let (run, n) = value.rsplit_once(':')?;
            Some(Ask::Size {
                run: run.to_string(),
                concurrency: n.parse().ok().filter(|n| *n > 0)?,
            })
        }
        "pepe:history" => Some(Ask::History { test: value }),
        "pepe:pick" => Some(Ask::Pick {
            test: action["selected_option"]["value"].as_str()?.to_string(),
        }),
        _ => None,
    }
}

/// `/pepe`, `/pepe run checkout`, `/pepe stop`, `/pepe status`, …
pub fn ask_of_command(text: &str) -> Ask {
    let mut words = text.split_whitespace();
    let verb = words.next().unwrap_or("").to_lowercase();
    let name = words.next().map(str::to_string);
    match (verb.as_str(), name) {
        ("run", Some(test)) => Ask::Run {
            test,
            args: Vec::new(),
            keep: false,
        },
        ("stop", test) => Ask::Stop { run: None, test },
        ("history", Some(test)) => Ask::History { test },
        ("status" | "running", _) => Ask::Status,
        ("help", _) => Ask::Help,
        // Nothing, a run with no name, a list, a test's name alone: the home
        ("" | "run" | "list" | "tests" | "history", None) => Ask::Home,
        (_, _) => Ask::Pick { test: verb },
    }
}

fn ago(when: SystemTime) -> String {
    let secs = when.elapsed().unwrap_or_default().as_secs();
    match secs {
        0..=59 => "just now".into(),
        60..=86_399 => format!(
            "{} ago",
            crate::ui::format::span(Duration::from_secs(secs / 60 * 60))
        ),
        86_400..=172_799 => "yesterday".into(),
        _ => format!("{} days ago", secs / 86_400),
    }
}

fn span(d: Duration) -> String {
    crate::ui::format::span(Duration::from_secs(d.as_secs()))
}

/// A recorded run in a few numbers: its face, req/s, p99, errors
struct Recorded {
    when: SystemTime,
    icon: &'static str,
    rps: f64,
    p99: f64,
    errors: f64,
}

fn recorded(run: &crate::store::Run) -> Option<Recorded> {
    let text = std::fs::read_to_string(&run.path).ok()?;
    let doc: Value = serde_json::from_str(&text).ok()?;
    let side = crate::compare::Side::of_report(String::new(), &doc).ok()?;
    let level = doc["verdict"]["level"].as_str().unwrap_or("healthy");
    Some(Recorded {
        when: run.when,
        icon: match level {
            "failing" => "❌",
            "degraded" => "⚠️",
            _ => "✅",
        },
        rps: side.requests_per_second.unwrap_or(0.0),
        p99: side.p99_ms.unwrap_or(0.0),
        errors: side.failed_percent,
    })
}

fn ms(value: f64) -> String {
    crate::ui::format::latency(Duration::from_secs_f64(value.max(0.0) / 1e3))
}

fn button(text: &str, action: &str, value: &str, style: Option<&str>) -> Value {
    let mut b = json!({"type": "button", "text": {"type": "plain_text", "text": text, "emoji": true}, "action_id": action, "value": value});
    if let Some(style) = style {
        b["style"] = json!(style);
    }
    b
}

/// The dropdown of saved tests, with `chosen` shown as picked
fn picker(tests: &[(String, String)], chosen: Option<&str>) -> Value {
    let option = |(name, about): &(String, String)| {
        json!({
            "text": {"type": "plain_text", "text": crate::webhook::layout_cut(&format!("{name} · {about}"), 75)},
            "value": name,
        })
    };
    let options: Vec<Value> = tests.iter().take(100).map(option).collect();
    let mut select = json!({
        "type": "static_select",
        "action_id": "pepe:pick",
        "placeholder": {"type": "plain_text", "text": "Pick a test"},
        "options": options,
    });
    if let Some(chosen) = chosen.and_then(|c| tests.iter().find(|(n, _)| n == c)) {
        select["initial_option"] = option(chosen);
    }
    select
}

fn controls(id: &str, paused: bool) -> Vec<Value> {
    vec![
        if paused {
            button("▶ Resume", "pepe:resume", id, Some("primary"))
        } else {
            button("⏸ Pause", "pepe:pause", id, None)
        },
        button("■ Stop", "pepe:stop", id, Some("danger")),
    ]
}

/// The help `/pepe help` gives
fn help(tests: &[String]) -> String {
    format!(
        "*pepe* runs the saved tests here: {}\n`/pepe` picks one · `/pepe run NAME` · `/pepe status` · `/pepe stop` · `/pepe history NAME`",
        if tests.is_empty() {
            "none yet (`pepe --keep NAME` saves one)".to_string()
        } else {
            tests.iter().map(|t| format!("`{t}`")).collect::<Vec<_>>().join(" ")
        }
    )
}

/// Say `reply` to the one who asked, and no one else
async fn whisper(client: &reqwest::Client, from: &From, reply: &Reply) {
    if let Some(url) = &from.response_url {
        let _ = client.post(url).json(&reply.payload()).send().await;
    }
}

fn log(who: &str, what: &str, test: &str, how: &str) {
    let now = clock();
    eprintln!(
        "  {}  {:<12} {:<16} {:<18} {how}",
        faint(&now),
        who,
        what,
        bold(test)
    );
}

/// HH:MM:SS, local, from the system's `date`
fn clock() -> String {
    std::process::Command::new("date")
        .arg("+%H:%M:%S")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

/// The edit form for a saved test, filled with what it holds
pub fn edit_form(test: &str, channel: &str, saved: &crate::config::Config) -> Value {
    let input = |block: &str, label: &str, initial: Option<String>, hint: &str, optional: bool| {
        let mut element = json!({"type": "plain_text_input", "action_id": "v", "placeholder": {"type": "plain_text", "text": hint}});
        if let Some(initial) = initial.filter(|i| !i.is_empty()) {
            element["initial_value"] = json!(initial);
        }
        json!({"type": "input", "block_id": block, "optional": optional, "label": {"type": "plain_text", "text": label}, "element": element})
    };
    let length = saved
        .duration
        .clone()
        .or_else(|| saved.requests.map(|n| n.to_string()));
    let target = format!(
        "{} {}",
        saved.method.clone().unwrap_or_else(|| "GET".into()),
        saved.url.clone().unwrap_or_default()
    );
    json!({
        "type": "modal",
        "callback_id": "pepe:edit",
        "private_metadata": json!({"test": test, "channel": channel}).to_string(),
        "title": {"type": "plain_text", "text": super::super::webhook::layout_cut(&format!("Run {test}"), 24)},
        "submit": {"type": "plain_text", "text": "Run"},
        "close": {"type": "plain_text", "text": "Cancel"},
        "blocks": [
            {"type": "context", "elements": [{"type": "mrkdwn", "text": format!("`{}` · .pepe/tests/{test}.toml", target.trim())}]},
            input("c", "Concurrency", saved.concurrency.map(|c| c.to_string()), "50", false),
            input("z", "Duration, or a number of requests", length, "5m, or 10000", false),
            input("rate", "Rate (requests a second)", saved.rate.map(|r| r.to_string()), "as fast as it answers", true),
            input("fail", "Fail if", None, "p99 > 300ms, errors > 1%", true),
            {"type": "input", "block_id": "keep", "optional": true, "label": {"type": "plain_text", "text": "Keep"},
             "element": {"type": "checkboxes", "action_id": "v", "options": [
                {"text": {"type": "plain_text", "text": "Keep these settings in the test"}, "value": "keep"}]}}
        ]
    })
}

/// What the form was sent with: the flags to run with, or the fields that
/// are wrong and why
pub fn args_of_form(values: &Value) -> Result<(Vec<String>, bool), Value> {
    let text = |block: &str| {
        values[block]["v"]["value"]
            .as_str()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(str::to_string)
    };
    let mut args = Vec::new();
    let mut errors = serde_json::Map::new();
    match text("c").map(|c| c.parse::<u32>()) {
        Some(Ok(c)) if c > 0 => args.extend(["-c".to_string(), c.to_string()]),
        _ => {
            errors.insert("c".into(), json!("a number above 0, as 50"));
        }
    }
    match text("z") {
        Some(n) if n.chars().all(|c| c.is_ascii_digit()) => args.extend(["-n".into(), n]),
        Some(z) if crate::cli::Cli::parse_duration_for("-z", &z).is_ok() => {
            args.extend(["-z".into(), z])
        }
        _ => {
            errors.insert(
                "z".into(),
                json!("a time, as 30s or 5m, or a number of requests"),
            );
        }
    }
    if let Some(rate) = text("rate") {
        match rate.parse::<f64>() {
            Ok(r) if r > 0.0 => args.extend(["--rate".into(), rate]),
            _ => {
                errors.insert(
                    "rate".into(),
                    json!("requests a second, above 0, or nothing"),
                );
            }
        }
    }
    if let Some(fail) = text("fail") {
        for condition in fail.split(',').map(str::trim).filter(|c| !c.is_empty()) {
            match crate::ramp::Condition::parse(condition) {
                Ok(_) => args.extend(["--fail-if".into(), condition.to_string()]),
                Err(e) => {
                    errors.insert("fail".into(), json!(format!("{condition}: {e}")));
                }
            }
        }
    }
    let keep = values["keep"]["v"]["selected_options"]
        .as_array()
        .is_some_and(|o| !o.is_empty());
    if errors.is_empty() {
        Ok((args, keep))
    } else {
        Err(Value::Object(errors))
    }
}

struct Listener {
    connection: Connection,
    bot: Api,
    client: reqwest::Client,
    store: Store,
    runs: Arc<Mutex<HashMap<String, Running>>>,
    /// The messages of runs that ended, by their ts: the test they were of
    ended: Arc<Mutex<HashMap<String, String>>>,
    count: u64,
}

impl Listener {
    fn tests(&self) -> Vec<String> {
        self.store.names()
    }

    /// Each saved test and what it sends: "POST /checkout"
    fn described(&self) -> Vec<(String, String)> {
        self.tests()
            .into_iter()
            .map(|name| {
                let about = self
                    .store
                    .load(&name)
                    .ok()
                    .map(|(_, c)| {
                        let url = c.url.unwrap_or_default();
                        let path = reqwest::Url::parse(&url)
                            .map(|u| format!("{}{}", u.host_str().unwrap_or(""), u.path()))
                            .unwrap_or(url);
                        format!("{} {}", c.method.unwrap_or_else(|| "GET".into()), path)
                    })
                    .unwrap_or_default();
                (name, about)
            })
            .collect()
    }

    /// The runs going now: id, test, who, how long, paused
    fn going(&self) -> Vec<(String, String, String, Duration, bool)> {
        let Ok(runs) = self.runs.lock() else {
            return Vec::new();
        };
        let mut out: Vec<_> = runs
            .iter()
            .map(|(id, r)| {
                (
                    id.clone(),
                    r.test.clone(),
                    r.by.clone(),
                    r.started.elapsed(),
                    r.paused,
                )
            })
            .collect();
        out.sort_by_key(|r| std::cmp::Reverse(r.3));
        out
    }

    fn host() -> String {
        gethostname::gethostname().to_string_lossy().into_owned()
    }

    /// `/pepe`: the tests in a dropdown, and what's running with its controls
    fn home(&self) -> Reply {
        let tests = self.described();
        let going = self.going();
        let mut blocks = Vec::new();
        let summary = format!(
            "*pepe* · {} saved test{} on {}{}",
            tests.len(),
            if tests.len() == 1 { "" } else { "s" },
            Self::host(),
            match going.len() {
                0 => String::new(),
                n => format!(" · {n} running"),
            }
        );
        if tests.is_empty() {
            blocks.push(
                json!({"type": "section", "text": {"type": "mrkdwn", "text": format!(
                    "{summary}\nNo saved tests in `{}` yet: `pepe --keep NAME …` saves one.",
                    self.store.tests_dir().display()
                )}}),
            );
        } else {
            blocks.push(json!({"type": "section", "text": {"type": "mrkdwn", "text": summary}, "accessory": picker(&tests, None)}));
        }
        for (id, test, by, took, paused) in &going {
            blocks.push(json!({"type": "divider"}));
            blocks.push(
                json!({"type": "section", "text": {"type": "mrkdwn", "text": format!(
                    "{} *{test}* · {} · started by <@{by}>",
                    if *paused { "⏸" } else { "⏳" },
                    if *paused { "paused".to_string() } else { format!("{} in", span(*took)) },
                )}}),
            );
            blocks.push(json!({"type": "actions", "elements": controls(id, *paused)}));
        }
        blocks.push(
            json!({"type": "context", "elements": [{"type": "mrkdwn", "text":
            "`/pepe run NAME` · `/pepe status` · `/pepe stop` · `/pepe history NAME`"}]}),
        );
        Reply::card(summary, json!(blocks))
    }

    /// A test's card: what it sends, how its last runs went, and its buttons
    fn card(&self, test: &str) -> Reply {
        self.card_with(test, None)
    }

    /// The card, with `note` above it: what just happened
    fn card_with(&self, test: &str, note: Option<String>) -> Reply {
        let tests = self.described();
        let Ok((_, saved)) = self.store.load(test) else {
            return Reply::text(format!(
                "❌ no saved test `{test}` here: {}",
                help(&self.tests())
            ));
        };
        let load = {
            let mut parts = vec![format!(
                "{} {}",
                saved.method.clone().unwrap_or_else(|| "GET".into()),
                saved.url.clone().unwrap_or_default()
            )];
            if let Some(c) = saved.concurrency {
                parts.push(format!("{c} at a time"));
            }
            match (&saved.duration, saved.requests) {
                (Some(d), _) => parts.push(d.clone()),
                (None, Some(n)) => parts.push(format!(
                    "{} requests",
                    crate::ui::format::count(u64::from(n))
                )),
                _ => {}
            }
            if let Some(rate) = saved.rate {
                parts.push(format!("{rate} a second"));
            }
            parts.join(" · ")
        };
        let runs: Vec<Recorded> = self.store.runs(test).iter().filter_map(recorded).collect();
        let mut blocks = vec![
            json!({"type": "section", "text": {"type": "mrkdwn", "text": format!("*{test}*")}, "accessory": picker(&tests, Some(test))}),
            json!({"type": "context", "elements": [{"type": "mrkdwn", "text": format!("`{load}`")}]}),
        ];
        if let Some(note) = note {
            blocks.insert(
                0,
                json!({"type": "context", "elements": [{"type": "mrkdwn", "text": note}]}),
            );
        }
        match runs.last() {
            Some(last) => {
                let tail: Vec<&Recorded> = runs.iter().rev().take(8).rev().collect();
                let p99: Vec<f64> = tail.iter().map(|r| r.p99).collect();
                let rps: Vec<f64> = tail.iter().map(|r| r.rps).collect();
                let mut fields = vec![
                    json!({"type": "mrkdwn", "text": format!("*Last run · {}*\n{} {} req/s · p99 {}", ago(last.when), last.icon, crate::ui::format::count(last.rps.round() as u64), ms(last.p99))}),
                    json!({"type": "mrkdwn", "text": format!("*Errors*\n{:.2}%", last.errors)}),
                ];
                // A trend needs a few runs to be one
                if tail.len() >= 3 {
                    fields.push(json!({"type": "mrkdwn", "text": format!("*Trend, last {}*\np99 `{}` · req/s `{}`", tail.len(), crate::webhook::spark(&p99), crate::webhook::spark(&rps))}));
                }
                fields.push(json!({"type": "mrkdwn", "text": match runs.len() {
                    1 => format!("*Recorded*\n1 run, {}", ago(runs[0].when)),
                    n => format!("*Recorded*\n{n} runs since {}", ago(runs[0].when)),
                }}));
                blocks.push(json!({"type": "section", "fields": fields}));
            }
            None => blocks.push(json!({"type": "section", "text": {"type": "mrkdwn", "text":
                "_No recorded runs yet: runs from Slack, or with `--record`, are kept._"}})),
        }
        let running = self.going().into_iter().find(|r| r.1 == test);
        let mut elements = match &running {
            Some((id, _, _, _, paused)) => controls(id, *paused),
            None => vec![button("▶ Run", "pepe:run", test, Some("primary"))],
        };
        elements.push(button("✎ Edit and run", "pepe:edit", test, None));
        elements.push(button("History", "pepe:history", test, None));
        blocks.push(json!({"type": "actions", "elements": elements}));
        Reply {
            text: format!("{test} · {load}"),
            blocks: Some(json!(blocks)),
            replace: true,
        }
    }

    /// A test's recorded runs, a line each
    fn history(&self, test: &str) -> Reply {
        let runs: Vec<Recorded> = self.store.runs(test).iter().filter_map(recorded).collect();
        if runs.is_empty() {
            return Reply::text(format!("`{test}` has no recorded runs yet."));
        }
        let lines: Vec<String> = runs
            .iter()
            .rev()
            .take(12)
            .map(|r| {
                format!(
                    "{} {:<11} {:>9} req/s  p99 {:>8}  {:>6.2}%",
                    r.icon,
                    ago(r.when),
                    crate::ui::format::count(r.rps.round() as u64),
                    ms(r.p99),
                    r.errors
                )
            })
            .collect();
        let blocks = json!([
            {"type": "section", "text": {"type": "mrkdwn", "text": format!("*{test}* · the last {} of {} runs", lines.len(), runs.len())}},
            {"type": "section", "text": {"type": "mrkdwn", "text": format!("```{}```", lines.join("\n"))}},
            {"type": "actions", "elements": [button("▶ Run", "pepe:run", test, Some("primary")), button("✎ Edit and run", "pepe:edit", test, None)]},
        ]);
        Reply::card(format!("{test}: {} runs", runs.len()), blocks)
    }

    /// Start `pepe --load test` in this directory, its message in `channel`
    fn start(
        &mut self,
        test: &str,
        args: Vec<String>,
        keep: bool,
        from: &From,
    ) -> Result<(), String> {
        if !self.tests().iter().any(|t| t == test) {
            return Err(format!(
                "no saved test `{test}` in {}: {}",
                self.store.tests_dir().display(),
                help(&self.tests())
            ));
        }
        if self.going().iter().any(|r| r.1 == test) {
            return Err(format!("`{test}` is running already: stop it first"));
        }
        self.count += 1;
        let id = format!("{}-{}", std::process::id(), self.count);
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut command = tokio::process::Command::new(exe);
        command
            // Recorded, so the test's card and History have it
            .args(["--load", test, "--json", "--record"])
            .arg(format!("--slack={}", from.channel))
            .args(&args)
            .env(super::RUN_ENV, &id)
            .env(super::BY_ENV, &from.user)
            .env("PEPE_NO_UPDATE_CHECK", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if keep {
            command.args(["--keep", test]);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("couldn't start pepe: {e}"))?;
        let (lines, mut to_run) = mpsc::unbounded_channel::<String>();
        if let Ok(mut runs) = self.runs.lock() {
            runs.insert(
                id.clone(),
                Running {
                    test: test.to_string(),
                    by: from.user.clone(),
                    lines,
                    started: Instant::now(),
                    paused: false,
                    message: None,
                },
            );
        }
        let runs = self.runs.clone();
        let ended = self.ended.clone();
        let bot = self.bot.clone();
        let channel = from.channel.clone();
        let test = test.to_string();
        let started = Instant::now();
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt;
            let mut stdin = child.stdin.take();
            // What the run says on stderr: its warnings shown as they come
            // (a message Slack refused, a hook that failed), all of it kept
            // for when it stops at once
            let said = Arc::new(Mutex::new(String::new()));
            if let Some(err) = child.stderr.take() {
                let said = said.clone();
                let test = test.clone();
                let runs = runs.clone();
                let id = id.clone();
                tokio::spawn(async move {
                    use tokio::io::AsyncBufReadExt;
                    let mut lines = tokio::io::BufReader::new(err).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        // Where its message is: questions in that thread are its
                        if let Some(at) = line.strip_prefix("pepe-slack: message ") {
                            if let (Some(ts), Ok(mut runs)) =
                                (at.split_whitespace().nth(1), runs.lock())
                            {
                                if let Some(r) = runs.get_mut(&id) {
                                    r.message = Some(ts.to_string());
                                }
                            }
                            continue;
                        }
                        if let Some(warning) = line.strip_prefix("warning: ") {
                            log(&test, &bad("⚠ warning"), "", warning);
                        }
                        if let Ok(mut said) = said.lock() {
                            said.push_str(&line);
                            said.push('\n');
                        }
                    }
                });
            }
            let mut stopped = false;
            let status = loop {
                tokio::select! {
                    status = child.wait() => break status,
                    line = to_run.recv() => {
                        let Some(line) = line else { continue };
                        stopped |= line.starts_with("stop");
                        let sent = match stdin.as_mut() {
                            Some(stdin) => stdin.write_all(format!("{line}\n").as_bytes()).await.is_ok(),
                            None => false,
                        };
                        // A run that can't hear it still stops as ctrl-c stops it
                        if !sent && line.starts_with("stop") {
                            interrupt(&mut child);
                        }
                    }
                }
            };
            // The last lines a moment to arrive
            tokio::time::sleep(Duration::from_millis(100)).await;
            let said = said.lock().map(|s| s.clone()).unwrap_or_default();
            if let Ok(mut runs) = runs.lock() {
                // Its thread can still be asked: from the recorded report
                if let Some(ts) = runs.remove(&id).and_then(|r| r.message) {
                    if let Ok(mut ended) = ended.lock() {
                        ended.insert(ts, test.clone());
                    }
                }
            }
            let took = faint(&span(started.elapsed()));
            match status.ok().and_then(|s| s.code()) {
                _ if stopped => log(&test, &faint("■ stopped"), "", &took),
                Some(0) => log(&test, &good("✓ passed"), "", &took),
                Some(4) => log(&test, &bad("✗ failed"), "", "a threshold was crossed"),
                // It couldn't start, or was refused: the channel hears why
                Some(1 | 2) => {
                    let why = said
                        .lines()
                        .rev()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("it stopped at once")
                        .trim_start_matches("error: ")
                        .to_string();
                    log(&test, &bad("✗ didn't run"), "", &why);
                    let _ = bot
                        .call(
                            "chat.postMessage",
                            &[
                                ("channel", channel),
                                ("text", format!("❌ `{test}` didn't run: {why}")),
                                ("username", "pepe".into()),
                                ("icon_url", crate::webhook::face("worried")),
                            ],
                        )
                        .await;
                }
                _ => log(&test, &faint("■ ended"), "", &took),
            }
        });
        Ok(())
    }

    /// A line to the runs it's meant for: one by id, those of a test, or
    /// all; the tests it reached
    fn tell(&self, run: Option<&str>, test: Option<&str>, line: &str) -> Vec<String> {
        let Ok(mut runs) = self.runs.lock() else {
            return Vec::new();
        };
        let mut told = Vec::new();
        for (id, r) in runs.iter_mut() {
            let this = match (run, test) {
                (Some(run), _) => id == run,
                (None, Some(test)) => r.test == test,
                (None, None) => true,
            };
            if this && r.lines.send(line.to_string()).is_ok() {
                if line.starts_with("pause") {
                    r.paused = true;
                } else if line.starts_with("resume") {
                    r.paused = false;
                }
                told.push(r.test.clone());
            }
        }
        told
    }

    /// Stop the runs this listener started, as ctrl-c would
    fn stop(&self, run: Option<&str>, test: Option<&str>) -> Vec<String> {
        self.tell(run, test, "stop")
    }

    /// One thing asked; the answer for the one who asked, if any
    async fn answer(&mut self, ask: Ask, from: &From) -> Option<Reply> {
        let who = format!(
            "@{}",
            if from.name.is_empty() {
                &from.user
            } else {
                &from.name
            }
        );
        let looks = matches!(
            ask,
            Ask::Help | Ask::Home | Ask::Status | Ask::Pick { .. } | Ask::History { .. }
        );
        if !looks && !allowed(&self.connection.allow, &from.user) {
            log(&who, "asked", "", &bad("✗ refused: not allowed"));
            let names = if self.connection.allow_names.is_empty() {
                "no one yet".to_string()
            } else {
                self.connection.allow_names.join(", ")
            };
            return Some(Reply::text(format!(
                "🔒 Only {names} can run tests from Slack here. Ask one of them, or `pepe slack allow @you` on the machine that runs `pepe slack serve`."
            )));
        }
        let gone = || Some(Reply::text("That run has ended."));
        match ask {
            Ask::Run { test, args, keep } => {
                // Already going: its controls, rather than a no
                if self.going().iter().any(|r| r.1 == test) {
                    return Some(self.card_with(
                        &test,
                        Some(format!("⏳ `{test}` is running: pause or stop it here")),
                    ));
                }
                let shown = args.join(" ");
                match self.start(&test, args, keep, from) {
                    Ok(()) => {
                        log(
                            &who,
                            "run",
                            &test,
                            &format!("{} {}", ember("running"), faint(&shown)),
                        );
                        // The card it was pressed on turns into the run's
                        Some(self.card_with(
                            &test,
                            Some("▶ Started: its message is in the channel, its controls are here".into()),
                        ))
                    }
                    Err(e) => Some(Reply::text(format!("❌ {e}"))),
                }
            }
            Ask::Edit { test, trigger } => {
                let saved = match self.store.load(&test) {
                    Ok((_, saved)) => saved,
                    Err(e) => return Some(Reply::text(format!("❌ {e}"))),
                };
                let form = edit_form(&test, &from.channel, &saved);
                match self
                    .bot
                    .call(
                        "views.open",
                        &[("trigger_id", trigger), ("view", form.to_string())],
                    )
                    .await
                {
                    Ok(_) => None,
                    Err(e) => Some(Reply::text(format!("❌ couldn't open the form: {e}"))),
                }
            }
            Ask::Stop { run, test } => {
                let line = format!("stop {}", from.user);
                let told = self.tell(run.as_deref(), test.as_deref(), &line);
                log(&who, "stop", &told.join(" "), "");
                Some(Reply::text(if told.is_empty() {
                    "Nothing of mine is running.".to_string()
                } else {
                    format!("■ Stopping {}: the verdict follows.", told.join(", "))
                }))
            }
            Ask::Pause { run } => {
                let told = self.tell(Some(&run), None, &format!("pause {}", from.user));
                log(&who, "pause", &told.join(" "), "");
                if told.is_empty() {
                    gone()
                } else {
                    None
                }
            }
            Ask::Resume { run } => {
                let told = self.tell(Some(&run), None, &format!("resume {}", from.user));
                log(&who, "resume", &told.join(" "), "");
                if told.is_empty() {
                    gone()
                } else {
                    None
                }
            }
            Ask::Size { run, concurrency } => {
                let told = self.tell(
                    Some(&run),
                    None,
                    &format!("concurrency {concurrency} {}", from.user),
                );
                log(&who, &format!("-c {concurrency}"), &told.join(" "), "");
                if told.is_empty() {
                    gone()
                } else {
                    None
                }
            }
            Ask::History { test } => Some(self.history(&test)),
            Ask::Pick { test } => Some(self.card(&test)),
            Ask::Home => Some(self.home()),
            Ask::Status => {
                let going = self.going();
                if going.is_empty() {
                    return Some(Reply::text(
                        "Nothing of mine is running. `/pepe` picks a test.",
                    ));
                }
                Some(self.home())
            }
            Ask::Help => Some(Reply::text(help(&self.tests()))),
        }
    }

    /// `@pepe …` in a thread: the run going there answers it from its live
    /// numbers; a run that ended, from its recorded report; anything else
    /// is told what can be asked where
    async fn mention(&mut self, event: &Value) {
        let user = event["user"].as_str().unwrap_or_default().to_string();
        let channel = event["channel"].as_str().unwrap_or_default().to_string();
        let thread = event["thread_ts"]
            .as_str()
            .or(event["ts"].as_str())
            .unwrap_or_default()
            .to_string();
        // The question without the mentions in it
        let question = event["text"]
            .as_str()
            .unwrap_or_default()
            .split_whitespace()
            .filter(|w| !(w.starts_with("<@") && w.ends_with('>')))
            .collect::<Vec<_>>()
            .join(" ");
        let going = self.runs.lock().ok().and_then(|runs| {
            runs.iter()
                .find(|(_, r)| r.message.as_deref() == Some(thread.as_str()))
                .map(|(id, r)| (id.clone(), r.test.clone()))
        });
        if let Some((id, test)) = going {
            log(&format!("@{user}"), "asked", &test, &faint(&question));
            self.tell(Some(&id), None, &format!("ask {user} {question}"));
            return;
        }
        let ended = self.ended.lock().ok().and_then(|e| e.get(&thread).cloned());
        let text = match ended {
            Some(test) => {
                log(&format!("@{user}"), "asked", &test, &faint(&question));
                let runs = self.store.runs(&test);
                let report = runs
                    .last()
                    .and_then(|r| std::fs::read_to_string(&r.path).ok())
                    .and_then(|t| serde_json::from_str::<Value>(&t).ok());
                match report {
                    Some(report) => {
                        // Against the run before it, when there's one
                        let compared = runs.len().checked_sub(2).and_then(|i| {
                            let before = crate::compare::Side::read(&runs[i].path).ok()?;
                            let after = crate::compare::Side::of_report("this run".into(), &report).ok()?;
                            let ago = runs[i].when.elapsed().unwrap_or_default();
                            Some((
                                crate::compare::compare(&before, &after),
                                format!("{} ago", crate::ui::format::span(ago)),
                            ))
                        });
                        super::answer::answer(&question, &report, compared.as_ref())
                    }
                    None => "That run's report isn't here any more.".into(),
                }
            }
            None => "I answer in the threads of runs started from Slack here (`/pepe run NAME`, or ↻ Run again): ask in one of those.".into(),
        };
        let _ = self
            .bot
            .call(
                "chat.postMessage",
                &[
                    ("channel", channel),
                    ("thread_ts", thread),
                    ("text", format!("<@{user}> {text}")),
                    ("username", "pepe".into()),
                    ("icon_url", crate::webhook::face("happy")),
                ],
            )
            .await;
    }

    /// One envelope from Slack: what to acknowledge it with, at once
    async fn envelope(&mut self, envelope: &Value) -> Option<Value> {
        let payload = &envelope["payload"];
        match envelope["type"].as_str()? {
            "interactive" => match payload["type"].as_str()? {
                "block_actions" => {
                    let from = From {
                        user: payload["user"]["id"].as_str().unwrap_or_default().into(),
                        name: payload["user"]["username"]
                            .as_str()
                            .or(payload["user"]["name"].as_str())
                            .unwrap_or_default()
                            .into(),
                        channel: payload["channel"]["id"].as_str().unwrap_or_default().into(),
                        response_url: payload["response_url"].as_str().map(str::to_string),
                    };
                    let trigger = payload["trigger_id"].as_str().unwrap_or_default();
                    for action in payload["actions"].as_array().into_iter().flatten() {
                        if let Some(ask) = ask_of_action(action, trigger) {
                            if let Some(reply) = self.answer(ask, &from).await {
                                whisper(&self.client, &from, &reply).await;
                            }
                        }
                    }
                    None
                }
                "view_submission" if payload["view"]["callback_id"] == "pepe:edit" => {
                    let meta: Value = payload["view"]["private_metadata"]
                        .as_str()
                        .and_then(|m| serde_json::from_str(m).ok())
                        .unwrap_or(Value::Null);
                    let (args, keep) = match args_of_form(&payload["view"]["state"]["values"]) {
                        Ok(ok) => ok,
                        Err(errors) => {
                            return Some(json!({"response_action": "errors", "errors": errors}))
                        }
                    };
                    let from = From {
                        user: payload["user"]["id"].as_str().unwrap_or_default().into(),
                        name: payload["user"]["username"]
                            .as_str()
                            .unwrap_or_default()
                            .into(),
                        channel: meta["channel"].as_str().unwrap_or_default().into(),
                        response_url: None,
                    };
                    let test = meta["test"].as_str().unwrap_or_default().to_string();
                    if let Some(reply) = self.answer(Ask::Run { test, args, keep }, &from).await {
                        // The card with the run's controls, or why it didn't start
                        let mut args = vec![
                            ("channel", from.channel.clone()),
                            ("user", from.user.clone()),
                            ("text", reply.text.clone()),
                        ];
                        if let Some(blocks) = &reply.blocks {
                            args.push(("blocks", blocks.to_string()));
                        }
                        let _ = self.bot.call("chat.postEphemeral", &args).await;
                    }
                    None
                }
                _ => None,
            },
            "events_api" if payload["event"]["type"] == "app_mention" => {
                self.mention(&payload["event"]).await;
                None
            }
            "slash_commands" => {
                let from = From {
                    user: payload["user_id"].as_str().unwrap_or_default().into(),
                    name: payload["user_name"].as_str().unwrap_or_default().into(),
                    channel: payload["channel_id"].as_str().unwrap_or_default().into(),
                    response_url: payload["response_url"].as_str().map(str::to_string),
                };
                let ask = ask_of_command(payload["text"].as_str().unwrap_or_default());
                let reply = self.answer(ask.clone(), &from).await;
                let mut reply = reply.unwrap_or_else(|| {
                    Reply::text(match ask {
                        Ask::Run { test, .. } => {
                            format!("▶ Starting `{test}`: its message follows here.")
                        }
                        _ => "Done.".into(),
                    })
                });
                // A command's answer is new, not in place of anything
                reply.replace = false;
                let mut payload = reply.payload();
                if let Some(o) = payload.as_object_mut() {
                    o.remove("replace_original");
                }
                Some(payload)
            }
            _ => None,
        }
    }
}

/// End a run the way ctrl-c does, so it reports what it measured
fn interrupt(child: &mut tokio::process::Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        let _ = std::process::Command::new("kill")
            .args(["-INT", &pid.to_string()])
            .status();
        return;
    }
    let _ = child.start_kill();
}

/// `pepe slack serve`: connect, answer, reconnect when Slack asks, until
/// ctrl-c
pub async fn run() -> Result<(), String> {
    let connection = super::load()
        .filter(|c| !c.token.is_empty())
        .ok_or("Slack isn't connected: pepe slack connect")?;
    let app = super::app_token().ok_or(
        "buttons aren't set up: pepe slack connect again, and give it an app token at step 5",
    )?;
    let store = Store::local();
    let mut listener = Listener {
        bot: Api::new(&connection.token),
        connection: connection.clone(),
        client: reqwest::Client::new(),
        store: store.clone(),
        runs: Arc::default(),
        ended: Arc::default(),
        count: 0,
    };
    let face = crate::ui::mascot::ansi(crate::ui::mascot::Mood::Happy);
    let tests = listener.tests();
    let title = [
        String::new(),
        String::new(),
        bold("pepe slack serve"),
        faint(&format!(
            "{} · {} · {} saved test{} in {}",
            connection.team,
            connection.channel.clone().unwrap_or_default(),
            tests.len(),
            if tests.len() == 1 { "" } else { "s" },
            store.tests_dir().display()
        )),
        faint(&format!(
            "allowed: {}",
            if connection.allow_names.is_empty() {
                "no one yet (pepe slack allow @name)".to_string()
            } else {
                connection.allow_names.join(", ")
            }
        )),
    ];
    eprintln!();
    for (i, row) in face.iter().enumerate() {
        eprintln!("  {row}   {}", title.get(i).cloned().unwrap_or_default());
    }
    // No colour, no Pepe: the title alone
    if face.is_empty() {
        for line in title.iter().filter(|l| !l.is_empty()) {
            eprintln!("  {line}");
        }
    }
    eprintln!();
    let mut wait = Duration::from_secs(1);
    loop {
        let url = match Api::new(&app).call("apps.connections.open", &[]).await {
            Ok(v) => v["url"].as_str().unwrap_or_default().to_string(),
            Err(e) if e.contains("isn't valid") => {
                return Err("Slack didn't take the app token: pepe slack connect, step 5".into())
            }
            Err(e) => {
                eprintln!("  {} {e}; again in {}s", bad("✗"), wait.as_secs());
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        let mut socket = match crate::ws::connect(&url).await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  {} {e}; again in {}s", bad("✗"), wait.as_secs());
                tokio::time::sleep(wait).await;
                wait = (wait * 2).min(Duration::from_secs(60));
                continue;
            }
        };
        loop {
            let message = tokio::select! {
                m = socket.recv() => m,
                _ = tokio::signal::ctrl_c() => {
                    socket.close().await;
                    let stopped = listener.stop(None, None);
                    if !stopped.is_empty() {
                        eprintln!("  stopping {}", stopped.join(", "));
                        tokio::time::sleep(Duration::from_secs(3)).await;
                    }
                    return Ok(());
                }
            };
            let Ok(Some(text)) = message else { break };
            let Ok(envelope) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            match envelope["type"].as_str() {
                Some("hello") => {
                    wait = Duration::from_secs(1);
                    eprintln!(
                        "  {} connected · buttons and /pepe answer here · ctrl-c to stop",
                        good("●")
                    );
                    continue;
                }
                Some("disconnect") => break,
                _ => {}
            }
            let Some(id) = envelope["envelope_id"].as_str().map(str::to_string) else {
                continue;
            };
            // Slack waits three seconds for the ack: commands and forms are
            // answered in it, everything slow happens after
            let reply = listener.envelope(&envelope).await;
            let mut ack = json!({"envelope_id": id});
            if let Some(reply) = reply {
                ack["payload"] = reply;
            }
            if socket.send(&ack.to_string()).await.is_err() {
                break;
            }
        }
        eprintln!("  {} reconnecting…", faint("○"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clicks_and_commands_say_what_they_ask() {
        let run = json!({"action_id": "pepe:run", "value": "checkout"});
        assert_eq!(
            ask_of_action(&run, "t"),
            Some(Ask::Run {
                test: "checkout".into(),
                args: vec![],
                keep: false
            })
        );
        let edit = json!({"action_id": "pepe:edit", "value": "checkout"});
        assert_eq!(
            ask_of_action(&edit, "trig"),
            Some(Ask::Edit {
                test: "checkout".into(),
                trigger: "trig".into()
            })
        );
        assert_eq!(ask_of_action(&json!({"action_id": "other"}), ""), None);
        assert_eq!(
            ask_of_command("run login"),
            Ask::Run {
                test: "login".into(),
                args: vec![],
                keep: false
            }
        );
        assert_eq!(
            ask_of_command("stop"),
            Ask::Stop {
                run: None,
                test: None
            }
        );
        assert_eq!(
            ask_of_command("history login"),
            Ask::History {
                test: "login".into()
            }
        );
        assert_eq!(ask_of_command(""), Ask::Home);
        assert_eq!(ask_of_command("list"), Ask::Home);
        assert_eq!(ask_of_command("run"), Ask::Home, "no name: pick one");
        assert_eq!(ask_of_command("status"), Ask::Status);
        assert_eq!(ask_of_command("help"), Ask::Help);
        assert_eq!(
            ask_of_command("checkout"),
            Ask::Pick {
                test: "checkout".into()
            }
        );
        let size = json!({"action_id": "pepe:size", "value": "41-2:63"});
        assert_eq!(
            ask_of_action(&size, ""),
            Some(Ask::Size {
                run: "41-2".into(),
                concurrency: 63
            })
        );
        assert_eq!(
            ask_of_action(&json!({"action_id": "pepe:size", "value": "41-2:0"}), ""),
            None
        );
        let pick = json!({"action_id": "pepe:pick", "selected_option": {"value": "login"}});
        assert_eq!(
            ask_of_action(&pick, ""),
            Some(Ask::Pick {
                test: "login".into()
            })
        );
        assert_eq!(
            ask_of_action(&json!({"action_id": "pepe:pause", "value": "41-2"}), ""),
            Some(Ask::Pause { run: "41-2".into() })
        );
        assert!(allowed(&["U1".into()], "U1") && !allowed(&["U1".into()], "U2"));
        assert!(allowed(&["*".into()], "anyone"));
        assert!(!allowed(&[], "U1"), "no one until someone is allowed");
    }

    #[test]
    fn the_form_becomes_flags_or_says_what_is_wrong() {
        let values = |c: &str, z: &str, rate: &str, fail: &str, keep: bool| {
            json!({
                "c": {"v": {"value": c}}, "z": {"v": {"value": z}},
                "rate": {"v": {"value": rate}}, "fail": {"v": {"value": fail}},
                "keep": {"v": {"selected_options": if keep { json!([{"value": "keep"}]) } else { json!([]) }}},
            })
        };
        let (args, keep) =
            args_of_form(&values("50", "5m", "", "p99 > 300ms, errors > 1%", true)).unwrap();
        assert_eq!(
            args,
            [
                "-c",
                "50",
                "-z",
                "5m",
                "--fail-if",
                "p99 > 300ms",
                "--fail-if",
                "errors > 1%"
            ]
        );
        assert!(keep);
        let (args, _) = args_of_form(&values("8", "10000", "200", "", false)).unwrap();
        assert_eq!(args, ["-c", "8", "-n", "10000", "--rate", "200"]);
        let wrong = args_of_form(&values("0", "soon", "fast", "p99 >", false)).unwrap_err();
        for field in ["c", "z", "rate", "fail"] {
            assert!(wrong.get(field).is_some(), "{field} is said wrong: {wrong}");
        }
    }

    #[test]
    fn the_form_is_filled_from_the_test() {
        let saved = crate::config::parse(
            "url = \"https://a.io/x\"\nmethod = \"POST\"\nconcurrency = 50\nduration = \"5m\"\n",
        )
        .unwrap();
        let form = edit_form("checkout", "C1", &saved);
        assert_eq!(form["callback_id"], "pepe:edit");
        assert_eq!(form["blocks"][1]["element"]["initial_value"], "50");
        assert_eq!(form["blocks"][2]["element"]["initial_value"], "5m");
        assert!(form["blocks"][0]["elements"][0]["text"]
            .as_str()
            .unwrap()
            .contains("POST https://a.io/x"));
        let meta: Value = serde_json::from_str(form["private_metadata"].as_str().unwrap()).unwrap();
        assert_eq!(
            (meta["test"].as_str(), meta["channel"].as_str()),
            (Some("checkout"), Some("C1"))
        );
    }
}
