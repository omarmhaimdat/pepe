//! `pepe slack serve`: the one place Slack's buttons and `/pepe` reach.
//! Slack hands every click of an app to one of its Socket Mode
//! connections, not to the process that posted the message, so a single
//! listener answers them all: left running on a machine, it runs the saved
//! tests of the directory it was started in, as `pepe --load NAME` with the
//! channel's `--slack`, under that directory's guardrails, for the people
//! allowed, and stops those it started.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{json, Value};
use tokio::sync::oneshot;

use super::wizard::{bad, bold, ember, faint, good};
use super::{Api, Connection};
use crate::store::Store;

/// A run this listener started, until it ends
struct Running {
    test: String,
    by: String,
    stop: Option<oneshot::Sender<()>>,
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
    History {
        test: String,
    },
    List,
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

/// May `user` press the buttons? `*` lets anyone in the channel
pub fn allowed(allow: &[String], user: &str) -> bool {
    allow.iter().any(|a| a == "*" || a == user)
}

/// A button's click: its action and value
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
        "pepe:history" => Some(Ask::History { test: value }),
        _ => None,
    }
}

/// `/pepe run checkout`, `/pepe stop`, `/pepe history checkout`, `/pepe list`
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
        ("list" | "tests", _) => Ask::List,
        _ => Ask::Help,
    }
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

/// The help `/pepe` gives
fn help(tests: &[String]) -> String {
    format!(
        "*pepe* runs the saved tests here: {}\n`/pepe run NAME` · `/pepe stop` · `/pepe history NAME` · `/pepe list`",
        if tests.is_empty() {
            "none yet (`pepe --keep NAME` saves one)".to_string()
        } else {
            tests.iter().map(|t| format!("`{t}`")).collect::<Vec<_>>().join(" ")
        }
    )
}

/// Say `text` to the one who asked, and no one else
async fn whisper(client: &reqwest::Client, from: &From, text: &str) {
    if let Some(url) = &from.response_url {
        let _ = client
            .post(url)
            .json(&json!({"response_type": "ephemeral", "replace_original": false, "text": text}))
            .send()
            .await;
    }
}

fn log(who: &str, what: &str, test: &str, how: &str) {
    let now = chrono_like();
    eprintln!(
        "  {}  {:<12} {:<16} {:<18} {how}",
        faint(&now),
        who,
        what,
        bold(test)
    );
}

/// HH:MM:SS, local time without a time crate: from the system's `date`
fn chrono_like() -> String {
    std::process::Command::new("date")
        .arg("+%H:%M:%S")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_default()
}

struct Listener {
    connection: Connection,
    bot: Api,
    client: reqwest::Client,
    store: Store,
    runs: Arc<Mutex<HashMap<String, Running>>>,
    count: u64,
}

impl Listener {
    fn tests(&self) -> Vec<String> {
        self.store.names()
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
        if let Ok(runs) = self.runs.lock() {
            if runs.values().any(|r| r.test == test) {
                return Err(format!("`{test}` is running already: stop it first"));
            }
        }
        self.count += 1;
        let id = format!("{}-{}", std::process::id(), self.count);
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut command = tokio::process::Command::new(exe);
        command
            .args(["--load", test, "--json"])
            .arg(format!("--slack={}", from.channel))
            .args(&args)
            .env(super::RUN_ENV, &id)
            .env(super::BY_ENV, &from.user)
            .env("PEPE_NO_UPDATE_CHECK", "1")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        if keep {
            command.args(["--keep", test]);
        }
        let mut child = command
            .spawn()
            .map_err(|e| format!("couldn't start pepe: {e}"))?;
        let (stop_tx, stop_rx) = oneshot::channel::<()>();
        if let Ok(mut runs) = self.runs.lock() {
            runs.insert(
                id.clone(),
                Running {
                    test: test.to_string(),
                    by: from.name.clone(),
                    stop: Some(stop_tx),
                },
            );
        }
        let runs = self.runs.clone();
        let bot = self.bot.clone();
        let channel = from.channel.clone();
        let test = test.to_string();
        let started = Instant::now();
        tokio::spawn(async move {
            let mut stderr = child.stderr.take();
            let mut stopped = false;
            let status = tokio::select! {
                status = child.wait() => status,
                _ = stop_rx => {
                    // As ctrl-c would: the run ends, says what it measured
                    stopped = true;
                    interrupt(&mut child);
                    child.wait().await
                }
            };
            let mut said = String::new();
            if let Some(err) = stderr.as_mut() {
                use tokio::io::AsyncReadExt;
                let _ = err.read_to_string(&mut said).await;
            }
            if let Ok(mut runs) = runs.lock() {
                runs.remove(&id);
            }
            let code = status.ok().and_then(|s| s.code());
            match code {
                _ if stopped => log(
                    &test,
                    &faint("■ stopped"),
                    "",
                    &faint(&crate::ui::format::span(started.elapsed())),
                ),
                Some(0) => log(
                    &test,
                    &good("✓ passed"),
                    "",
                    &faint(&crate::ui::format::span(started.elapsed())),
                ),
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
                _ => log(&test, &faint("■ ended"), "", ""),
            }
        });
        Ok(())
    }

    /// Stop the runs this listener started: one by id, those of a test, or
    /// all of them
    fn stop(&self, run: Option<&str>, test: Option<&str>) -> Vec<String> {
        let Ok(mut runs) = self.runs.lock() else {
            return Vec::new();
        };
        let mut stopped = Vec::new();
        for (id, r) in runs.iter_mut() {
            let this = match (run, test) {
                (Some(run), _) => id == run,
                (None, Some(test)) => r.test == test,
                (None, None) => true,
            };
            if this {
                if let Some(stop) = r.stop.take() {
                    let _ = stop.send(());
                    stopped.push(if r.by.is_empty() {
                        r.test.clone()
                    } else {
                        format!("{} (started by @{})", r.test, r.by)
                    });
                }
            }
        }
        stopped
    }

    /// The test's recorded runs, as `pepe history` says them
    async fn history(&self, test: &str) -> String {
        let Ok(exe) = std::env::current_exe() else {
            return "couldn't find pepe".into();
        };
        let out = tokio::process::Command::new(exe)
            .args(["history", test])
            .env("NO_COLOR", "1")
            .env("PEPE_NO_UPDATE_CHECK", "1")
            .output()
            .await;
        match out {
            Ok(o) => {
                let text = String::from_utf8_lossy(&o.stdout).to_string()
                    + &String::from_utf8_lossy(&o.stderr);
                format!(
                    "```{}```",
                    super::super::webhook::layout_cut(text.trim(), 2900)
                )
            }
            Err(e) => format!("couldn't read the history: {e}"),
        }
    }

    /// One thing asked; the answer for the one who asked, if any
    async fn answer(&mut self, ask: Ask, from: &From) -> Option<String> {
        let who = if from.name.is_empty() {
            &from.user
        } else {
            &from.name
        };
        let gate = |ask: &Ask| !matches!(ask, Ask::Help | Ask::List);
        if gate(&ask) && !allowed(&self.connection.allow, &from.user) {
            log(
                &format!("@{who}"),
                "asked",
                "",
                &bad("✗ refused: not allowed"),
            );
            let names = if self.connection.allow_names.is_empty() {
                "no one yet".to_string()
            } else {
                self.connection.allow_names.join(", ")
            };
            return Some(format!(
                "🔒 Only {names} can run tests from Slack here. Ask one of them, or `pepe slack allow @you` on the machine that runs `pepe slack serve`."
            ));
        }
        match ask {
            Ask::Run { test, args, keep } => {
                let shown = if args.is_empty() {
                    String::new()
                } else {
                    args.join(" ")
                };
                match self.start(&test, args, keep, from) {
                    Ok(()) => {
                        log(
                            &format!("@{who}"),
                            "run",
                            &test,
                            &format!("{} {}", ember("running"), faint(&shown)),
                        );
                        None
                    }
                    Err(e) => Some(format!("❌ {e}")),
                }
            }
            Ask::Edit { test, trigger } => {
                let saved = match self.store.load(&test) {
                    Ok((_, saved)) => saved,
                    Err(e) => return Some(format!("❌ {e}")),
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
                    Err(e) => Some(format!("❌ couldn't open the form: {e}")),
                }
            }
            Ask::Stop { run, test } => {
                let stopped = self.stop(run.as_deref(), test.as_deref());
                log(&format!("@{who}"), "stop", &stopped.join(" "), "");
                Some(if stopped.is_empty() {
                    "Nothing of mine is running.".into()
                } else {
                    format!("■ Stopping {}: the verdict follows.", stopped.join(", "))
                })
            }
            Ask::History { test } => Some(self.history(&test).await),
            Ask::List => Some(help(&self.tests())),
            Ask::Help => Some(help(&self.tests())),
        }
    }

    /// One envelope from Slack: what to acknowledge it with, at once, and
    /// what to do after
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
                            if let Some(said) = self.answer(ask, &from).await {
                                whisper(&self.client, &from, &said).await;
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
                    if let Some(said) = self.answer(Ask::Run { test, args, keep }, &from).await {
                        let _ = self
                            .bot
                            .call(
                                "chat.postEphemeral",
                                &[
                                    ("channel", from.channel.clone()),
                                    ("user", from.user.clone()),
                                    ("text", said),
                                ],
                            )
                            .await;
                    }
                    None
                }
                _ => None,
            },
            "slash_commands" => {
                let from = From {
                    user: payload["user_id"].as_str().unwrap_or_default().into(),
                    name: payload["user_name"].as_str().unwrap_or_default().into(),
                    channel: payload["channel_id"].as_str().unwrap_or_default().into(),
                    response_url: payload["response_url"].as_str().map(str::to_string),
                };
                let ask = ask_of_command(payload["text"].as_str().unwrap_or_default());
                let said = self.answer(ask.clone(), &from).await;
                Some(json!({
                    "response_type": "ephemeral",
                    "text": said.unwrap_or_else(|| match ask {
                        Ask::Run { test, .. } => format!("▶ Starting `{test}`: its message follows here."),
                        _ => "Done.".into(),
                    })
                }))
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
        assert_eq!(ask_of_command(""), Ask::Help);
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
