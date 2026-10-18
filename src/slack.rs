//! Slack, as a place a run is said: one message posted when the run
//! starts, edited in place as it goes, anomalies in its thread, the verdict
//! when it ends. A webhook whose URL is `slack:#channel` (or `--slack`).
//!
//! It needs a bot token, which `pepe slack connect` gets without a form to
//! fill: it opens Slack's "create an app" page with everything already
//! said (a manifest in the link), and after Create and Install it takes
//! the token pasted in, checks it, picks the channel, joins it, posts a
//! test, and keeps it in `slack.toml` beside the settings, readable by
//! you alone. `PEPE_SLACK_TOKEN` stands in for the file, as in CI.

use std::io::{IsTerminal, Write};
use std::path::PathBuf;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

mod wizard;

/// The environment variable that holds a token, over the file
pub const TOKEN_ENV: &str = "PEPE_SLACK_TOKEN";
/// Where Slack's Web API is; tests point it at a server of their own
const API_ENV: &str = "PEPE_SLACK_API";
const API: &str = "https://slack.com/api";

/// What `pepe slack connect` keeps
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct Connection {
    pub token: String,
    /// The workspace's name and address
    #[serde(default)]
    pub team: String,
    #[serde(default)]
    pub url: String,
    /// The channel `slack:` alone and `--slack` send to: its name and id
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
}

/// `slack.toml`, beside the settings
pub fn path() -> Option<PathBuf> {
    crate::settings::path().map(|p| p.with_file_name("slack.toml"))
}

/// What `pepe slack connect` left, if it was run
pub fn load() -> Option<Connection> {
    let text = std::fs::read_to_string(path()?).ok()?;
    toml::from_str(&text).ok()
}

/// Keep the connection, readable by you alone
pub fn save(connection: &Connection) -> Result<PathBuf, String> {
    let path = path().ok_or("no home directory to keep the Slack token in")?;
    let failed = |e: std::io::Error| format!("couldn't write {}: {e}", path.display());
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(failed)?;
    }
    let text = format!(
        "# pepe's Slack connection · `pepe slack connect` writes it, `pepe slack forget` removes it\n\
         # the token is a secret: this file is readable by you alone\n\n{}",
        toml::to_string(connection).unwrap_or_default()
    );
    let partial = path.with_extension("toml.partial");
    {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&partial).map_err(failed)?;
        file.write_all(text.as_bytes()).map_err(failed)?;
    }
    std::fs::rename(&partial, &path).map_err(failed)?;
    Ok(path)
}

/// The token to send with: `PEPE_SLACK_TOKEN`, else the file's
pub fn token() -> Option<String> {
    std::env::var(TOKEN_ENV)
        .ok()
        .filter(|t| !t.trim().is_empty())
        .or_else(|| load().map(|c| c.token))
}

/// Where `slack:` with no channel sends: the file's channel
pub fn default_channel() -> Option<String> {
    let c = load()?;
    c.channel_id.or(c.channel)
}

/// The app pepe asks Slack to make: its name, and what it may do
pub fn manifest() -> Value {
    json!({
        "display_information": {
            "name": "pepe",
            "description": "Load tests and pings, said where your team sees them",
            "long_description": "pepe posts a message when a load test starts, keeps it up to date as the run goes, puts what changed in its thread, and ends it with the verdict: requests, latency, errors and what to look at. Made by `pepe slack connect`, from the terminal.",
            "background_color": "#121010"
        },
        "features": {
            "bot_user": {"display_name": "pepe", "always_online": false}
        },
        "oauth_config": {
            "scopes": {
                "bot": ["chat:write", "chat:write.public", "chat:write.customize", "channels:read", "channels:join", "groups:read"]
            }
        },
        "settings": {
            "org_deploy_enabled": false,
            "socket_mode_enabled": false,
            "token_rotation_enabled": false
        }
    })
}

/// Slack's "create an app" page, with the manifest already in it
pub fn manifest_link() -> String {
    let mut url = reqwest::Url::parse("https://api.slack.com/apps").expect("a URL");
    url.query_pairs_mut()
        .append_pair("new_app", "1")
        .append_pair("manifest_json", &manifest().to_string());
    url.to_string()
}

/// Slack's Web API, with a token
#[derive(Clone)]
pub struct Api {
    client: reqwest::Client,
    token: String,
    base: String,
}

impl Api {
    pub fn new(token: &str) -> Api {
        Api {
            client: reqwest::Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent(crate::utils::default_user_agent())
                .build()
                .unwrap_or_default(),
            token: token.trim().to_string(),
            base: std::env::var(API_ENV).unwrap_or_else(|_| API.to_string()),
        }
    }

    /// A method of the API with its arguments; Err is what to do about
    /// what Slack said. A 429 waits as long as Slack asks, twice at most.
    pub async fn call(&self, method: &str, args: &[(&str, String)]) -> Result<Value, String> {
        let mut form = reqwest::Url::parse("http://x/").expect("a URL");
        form.query_pairs_mut()
            .extend_pairs(args.iter().map(|(k, v)| (*k, v.as_str())));
        let body = form.query().unwrap_or_default().to_string();
        let mut last = String::new();
        for _ in 0..3 {
            let answer = self
                .client
                .post(format!("{}/{method}", self.base))
                .bearer_auth(&self.token)
                .header(
                    reqwest::header::CONTENT_TYPE,
                    "application/x-www-form-urlencoded",
                )
                .body(body.clone())
                .send()
                .await
                .map_err(|e| {
                    format!(
                        "couldn't reach Slack: {}",
                        crate::response::Failure::from(&e).message
                    )
                })?;
            if answer.status().as_u16() == 429 {
                let wait = answer
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1)
                    .min(30);
                last = "Slack said to slow down".into();
                tokio::time::sleep(Duration::from_secs(wait)).await;
                continue;
            }
            let value: Value = answer
                .json()
                .await
                .map_err(|e| format!("Slack's answer didn't read: {e}"))?;
            if value["ok"] == true {
                return Ok(value);
            }
            let error = value["error"].as_str().unwrap_or("an unknown error");
            let needed = value["needed"].as_str().unwrap_or("");
            return Err(explain(error, needed));
        }
        Err(last)
    }
}

/// What to do about an error Slack named
fn explain(error: &str, needed: &str) -> String {
    match error {
        "invalid_auth" | "not_authed" | "token_revoked" | "account_inactive" | "token_expired" => {
            "the Slack token isn't valid (any more): run pepe slack connect".into()
        }
        "channel_not_found" => {
            "pepe can't see that channel: a private one needs /invite @pepe in it first".into()
        }
        "is_archived" => "that channel is archived".into(),
        "not_in_channel" => "pepe isn't in that channel: /invite @pepe in it".into(),
        "missing_scope" => format!(
            "the Slack app can't do that (it lacks {needed}): pepe slack connect makes one that can"
        ),
        "ratelimited" => "Slack said to slow down".into(),
        other => format!("Slack said {other}"),
    }
}

/// One run's message, as it goes
pub struct Live {
    api: Api,
    /// As given (a name or an id), then the id Slack answered with
    channel: String,
    /// The message being kept up to date, once posted
    ts: Option<String>,
    ended: bool,
}

impl Live {
    pub fn new(token: &str, channel: &str) -> Live {
        Live {
            api: Api::new(token),
            channel: channel.to_string(),
            ts: None,
            ended: false,
        }
    }

    /// The message, once posted
    pub fn ts(&self) -> Option<&str> {
        self.ts.as_deref()
    }

    /// The channel, by the id Slack answered with once it did
    pub fn channel(&self) -> &str {
        &self.channel
    }

    /// Post, joining a public channel first when pepe isn't in it yet
    async fn post(&mut self, args: &[(&str, String)]) -> Result<Value, String> {
        let mut all = vec![("channel", self.channel.clone())];
        all.extend(args.iter().cloned());
        match self.api.call("chat.postMessage", &all).await {
            Err(e) if e.starts_with("pepe isn't in that channel") => {
                self.api
                    .call("conversations.join", &[("channel", self.channel.clone())])
                    .await
                    .map_err(|_| e.clone())?;
                self.api.call("chat.postMessage", &all).await
            }
            answer => answer,
        }
    }

    /// The message as `payload` says ({text, and blocks or attachments,
    /// and an icon_url for Pepe's face}): posted the first time, edited in
    /// place after. Slack keeps a message's face from when it was posted.
    pub async fn show(&mut self, payload: &Value) -> Result<(), String> {
        let mut args = vec![
            (
                "text",
                payload["text"].as_str().unwrap_or_default().to_string(),
            ),
            ("unfurl_links", "false".to_string()),
        ];
        for key in ["blocks", "attachments"] {
            if payload[key].is_array() {
                args.push((key, payload[key].to_string()));
            }
        }
        match &self.ts {
            Some(ts) => {
                let mut all = vec![("channel", self.channel.clone()), ("ts", ts.clone())];
                all.extend(args);
                self.api.call("chat.update", &all).await?;
            }
            None => {
                args.push(("username", "pepe".into()));
                if let Some(icon) = payload["icon_url"].as_str() {
                    args.push(("icon_url", icon.to_string()));
                }
                let answer = self.post(&args).await?;
                self.ts = answer["ts"].as_str().map(str::to_string);
                if let Some(id) = answer["channel"].as_str() {
                    self.channel = id.to_string();
                }
            }
        }
        Ok(())
    }

    /// A line in the message's thread, with Pepe's `icon`; `broadcast`
    /// shows it in the channel too, which is what makes a mention notify
    pub async fn reply(
        &mut self,
        text: &str,
        broadcast: bool,
        icon: Option<&str>,
    ) -> Result<(), String> {
        let mut args = vec![("text", text.to_string()), ("username", "pepe".into())];
        if let Some(icon) = icon {
            args.push(("icon_url", icon.to_string()));
        }
        if let Some(ts) = &self.ts {
            args.push(("thread_ts", ts.clone()));
            if broadcast {
                args.push(("reply_broadcast", "true".into()));
            }
        }
        let answer = self.post(&args).await?;
        if let Some(id) = answer["channel"].as_str() {
            self.channel = id.to_string();
        }
        Ok(())
    }

    /// The run's end, once: the message becomes the verdict, and `line`
    /// goes under it, shown in the channel, with the face of the outcome
    pub async fn end(&mut self, payload: &Value, line: &str, icon: &str) -> Result<(), String> {
        if self.ended {
            return Ok(());
        }
        self.ended = true;
        self.show(payload).await?;
        self.reply(line, true, Some(icon)).await
    }
}

// ─── pepe slack ──────────────────────────────────────────────────────────────

fn say(text: &str) {
    eprintln!("{text}");
}

fn ask(prompt: &str) -> Result<String, String> {
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    let mut line = String::new();
    std::io::stdin()
        .read_line(&mut line)
        .map_err(|e| format!("couldn't read the answer: {e}"))?;
    Ok(line.trim().to_string())
}

/// A secret typed or pasted, shown as dots
fn ask_secret(prompt: &str) -> Result<String, String> {
    use crossterm::event::{read, Event, KeyCode, KeyEventKind, KeyModifiers};
    if !std::io::stdin().is_terminal() {
        return ask(prompt);
    }
    eprint!("{prompt}");
    let _ = std::io::stderr().flush();
    crossterm::terminal::enable_raw_mode().map_err(|e| e.to_string())?;
    let mut secret = String::new();
    let outcome = loop {
        match read() {
            Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                KeyCode::Enter => break Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    break Err("stopped".to_string())
                }
                KeyCode::Esc => break Err("stopped".to_string()),
                KeyCode::Backspace => {
                    if secret.pop().is_some() {
                        eprint!("\x08 \x08");
                    }
                }
                KeyCode::Char(c) => {
                    secret.push(c);
                    eprint!("•");
                }
                _ => {}
            },
            Ok(Event::Paste(text)) => {
                for c in text.trim().chars() {
                    secret.push(c);
                    eprint!("•");
                }
            }
            Ok(_) => {}
            Err(e) => break Err(e.to_string()),
        }
        let _ = std::io::stderr().flush();
    };
    let _ = crossterm::terminal::disable_raw_mode();
    eprintln!();
    outcome.map(|()| secret.trim().to_string())
}

/// Open `url` in the browser, when there is one to open it in. `BROWSER`
/// names the program, as elsewhere; `BROWSER=none` opens nothing
pub fn open_browser(url: &str) -> bool {
    if let Ok(browser) = std::env::var("BROWSER") {
        if browser.is_empty() || browser == "none" {
            return false;
        }
        return std::process::Command::new(browser)
            .arg(url)
            .status()
            .is_ok_and(|s| s.success());
    }
    let command = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).status()
    } else if cfg!(windows) {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
    } else {
        std::process::Command::new("xdg-open").arg(url).status()
    };
    command.is_ok_and(|s| s.success())
}

/// A channel pepe can see
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    pub name: String,
    pub id: String,
    pub private: bool,
    pub members: u64,
    /// pepe is in it already
    pub joined: bool,
}

/// The workspace's channels pepe can post in, the busiest first
pub async fn channels(api: &Api) -> Result<Vec<Channel>, String> {
    let mut out = Vec::new();
    let mut cursor = String::new();
    for _ in 0..10 {
        let mut args = vec![
            ("types", "public_channel,private_channel".to_string()),
            ("exclude_archived", "true".to_string()),
            ("limit", "200".to_string()),
        ];
        if !cursor.is_empty() {
            args.push(("cursor", cursor.clone()));
        }
        let page = api.call("conversations.list", &args).await?;
        for c in page["channels"].as_array().into_iter().flatten() {
            if let (Some(id), Some(name)) = (c["id"].as_str(), c["name"].as_str()) {
                out.push(Channel {
                    name: name.to_string(),
                    id: id.to_string(),
                    private: c["is_private"] == true,
                    members: c["num_members"].as_u64().unwrap_or(0),
                    joined: c["is_member"] == true,
                });
            }
        }
        cursor = page["response_metadata"]["next_cursor"]
            .as_str()
            .unwrap_or("")
            .to_string();
        if cursor.is_empty() {
            break;
        }
    }
    out.sort_by(|a, b| b.members.cmp(&a.members).then(a.name.cmp(&b.name)));
    Ok(out)
}

/// The channels `text` could mean: the one it names exactly, else those
/// with it in their name
pub fn matching<'a>(all: &'a [Channel], text: &str) -> Vec<&'a Channel> {
    let text = text.trim().trim_start_matches('#').to_lowercase();
    if text.is_empty() {
        return all.iter().collect();
    }
    if let Some(exact) = all
        .iter()
        .find(|c| c.name == text || c.id.to_lowercase() == text)
    {
        return vec![exact];
    }
    all.iter().filter(|c| c.name.contains(&text)).collect()
}

/// Ask which channel, by name or part of it; `given` answers without asking
async fn pick_channel(api: &Api, given: Option<&str>) -> Result<(String, String), String> {
    let all = channels(api).await?;
    let names = |many: &[&Channel]| {
        many.iter()
            .take(5)
            .map(|c| format!("#{}", c.name))
            .collect::<Vec<_>>()
            .join(", ")
    };
    if let Some(given) = given {
        return match matching(&all, given).as_slice() {
            [c] => Ok((c.name.clone(), c.id.clone())),
            [] => Err(format!(
                "no channel {given} that pepe can see: a private one needs /invite @pepe first"
            )),
            many => Err(format!("{given} could be {}", names(many))),
        };
    }
    if all.is_empty() {
        return Err("pepe can't see any channel yet".into());
    }
    say(&format!(
        "  Channels: {}",
        names(&all.iter().collect::<Vec<_>>())
    ));
    loop {
        let answer = ask("  Which channel? (a name, or part of one) › ")?;
        match matching(&all, &answer).as_slice() {
            [c] => return Ok((c.name.clone(), c.id.clone())),
            [] => say("  No channel by that name pepe can see. A private one needs /invite @pepe in it first."),
            many => say(&format!("  That could be {}: say more", names(many))),
        }
    }
}

/// `pepe slack connect`: from nothing to a test message in the channel
pub async fn connect(token: Option<String>, channel: Option<String>) -> Result<(), String> {
    let token = token.or_else(|| std::env::var(TOKEN_ENV).ok());
    let terminal = std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if terminal && token.is_none() && channel.is_none() {
        let finished = wizard::run(&manifest_link()).await?;
        done(&finished);
        return Ok(());
    }
    // Told what to use, or no one to ask: a line at a time
    let token = match token {
        Some(token) => token,
        None => {
            let link = manifest_link();
            say("1. Make the app: Slack's page, everything filled in");
            open_browser(&link);
            say(&format!("   {link}"));
            say("   Pick the workspace, Create, then Install to Workspace and Allow.");
            say("2. Copy the Bot User OAuth Token (xoxb-…), under OAuth & Permissions.");
            ask_secret("   Paste it here › ")?
        }
    };
    let (team, url) = check(&token).await?;
    say(&format!("✓ {team} · {}", url.trim_end_matches('/')));
    let api = Api::new(&token);
    let (name, id) = pick_channel(&api, channel.as_deref()).await?;
    let finished = finish(&token, &team, &url, &name, &id).await?;
    done(&finished);
    Ok(())
}

/// The token works: the workspace's name and address
pub async fn check(token: &str) -> Result<(String, String), String> {
    let token = token.trim();
    if !token.starts_with("xoxb-") {
        return Err(
            "that isn't a bot token: it starts with xoxb-, under OAuth & Permissions".into(),
        );
    }
    let me = Api::new(token).call("auth.test", &[]).await?;
    Ok((
        me["team"].as_str().unwrap_or("your workspace").to_string(),
        me["url"].as_str().unwrap_or("").to_string(),
    ))
}

/// The channel named in `text` (a name, part of one, or an id), as
/// `#name` and its id
pub async fn find_channel(token: &str, text: &str) -> Result<(String, String), String> {
    pick_channel(&Api::new(token), Some(text)).await
}

/// What connecting left: where it's kept, and the test message
pub struct Done {
    pub path: PathBuf,
    pub permalink: Option<String>,
    /// Pepe's face, for the app's icon
    pub icon: Option<PathBuf>,
    pub team: String,
    pub channel: String,
}

/// Pepe's face as Slack asks an app's icon to be: 512 pixels square
const ICON: &[u8] = include_bytes!("../site/img/pepe-icon.png");

/// Join the channel, post the test message there, and keep it all
pub async fn finish(
    token: &str,
    team: &str,
    url: &str,
    name: &str,
    id: &str,
) -> Result<Done, String> {
    let api = Api::new(token);
    // A public channel is joined; a private one must have invited pepe
    let _ = api
        .call("conversations.join", &[("channel", id.to_string())])
        .await;
    let mut live = Live::new(token, id);
    live.show(&json!({
        "text": "pepe is connected",
        "icon_url": crate::webhook::face("happy"),
        "attachments": [{
            "color": "#ff8a3d",
            "blocks": [
                {"type": "header", "text": {"type": "plain_text", "text": "👋 pepe is here", "emoji": true}},
                {"type": "section", "text": {"type": "mrkdwn", "text": "Load tests run with `--slack` post in this channel: *one message per run*, kept up to date as it goes (progress, throughput, latency, errors), what changes in its thread, and the verdict at the end, with the thresholds and how it compares with the last run."}},
                {"type": "context", "elements": [{"type": "mrkdwn", "text": "`pepe --slack -z 30s https://…` · `webhook = \"slack:\"` in a pepe.toml · `pepe slack status`"}]}
            ]
        }]
    }))
    .await?;
    let permalink = match live.ts() {
        Some(ts) => api
            .call(
                "chat.getPermalink",
                &[
                    ("channel", live.channel().to_string()),
                    ("message_ts", ts.to_string()),
                ],
            )
            .await
            .ok()
            .and_then(|v| v["permalink"].as_str().map(str::to_string)),
        None => None,
    };
    let path = save(&Connection {
        token: token.trim().to_string(),
        team: team.to_string(),
        url: url.to_string(),
        channel: Some(format!("#{name}")),
        channel_id: Some(id.to_string()),
    })?;
    let icon = path.with_file_name("pepe-icon.png");
    let icon = std::fs::write(&icon, ICON).ok().map(|()| icon);
    Ok(Done {
        path,
        permalink,
        icon,
        team: team.to_string(),
        channel: format!("#{name}"),
    })
}

/// What to do now, after connecting
fn done(d: &Done) {
    let home = |p: &std::path::Path| {
        let shown = p.display().to_string();
        match std::env::var("HOME") {
            Ok(home) if shown.starts_with(&home) => shown.replacen(&home, "~", 1),
            _ => shown,
        }
    };
    use wizard::{bold, ember, faint, good};
    say("");
    say(&format!(
        "  {} {}",
        good("✓"),
        bold(&format!("pepe posts in {} on {}", d.channel, d.team))
    ));
    if let Some(link) = &d.permalink {
        say(&format!("    {} {link}", faint("the test message")));
    }
    say(&format!(
        "    {} {} {}",
        faint("the token is kept in"),
        home(&d.path),
        faint("(only you can read it)")
    ));
    if let Some(icon) = &d.icon {
        say("");
        say(&format!(
            "  {} {}",
            ember("◆"),
            bold("Give the app Pepe's face")
        ));
        say(&format!(
            "    {} {} {}",
            faint("drag"),
            home(icon),
            faint("into Basic Information → App icon, at https://api.slack.com/apps")
        ));
    }
    say("");
    say(&format!("  {} {}", ember("◆"), bold("Try it")));
    let row = |cmd: String, what: &str| say(&format!("    {cmd:<52}{}", faint(what)));
    row(
        "pepe --slack -z 30s https://api.example.com/health".into(),
        "a run, live in the channel",
    );
    row("pepe --slack=#another …".into(), "somewhere else");
    row(
        "webhook = \"slack:\"".into(),
        "in a pepe.toml: every run there",
    );
    row(
        format!("{TOKEN_ENV}=xoxb-… pepe --slack={} …", d.channel),
        "in CI",
    );
    say("");
}

/// `pepe slack status`: what is connected, checked with Slack
pub async fn status() -> Result<(), String> {
    let from_env = std::env::var(TOKEN_ENV).is_ok_and(|t| !t.trim().is_empty());
    let Some(token) = token() else {
        say("Slack isn't connected: pepe slack connect");
        return Ok(());
    };
    let me = Api::new(&token).call("auth.test", &[]).await?;
    say(&format!(
        "✓ {} {} · token from {}",
        me["team"].as_str().unwrap_or("?"),
        me["url"].as_str().unwrap_or(""),
        if from_env {
            TOKEN_ENV.to_string()
        } else {
            path().map(|p| p.display().to_string()).unwrap_or_default()
        }
    ));
    match load().and_then(|c| c.channel) {
        Some(channel) => say(&format!("  --slack and slack: post in {channel}")),
        None => say("  no channel kept: --slack '#channel' says one"),
    }
    Ok(())
}

/// `pepe slack forget`: the file removed
pub fn forget() -> Result<(), String> {
    let Some(path) = path() else { return Ok(()) };
    match std::fs::remove_file(&path) {
        Ok(()) => say(&format!("✓ removed {}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => say("Slack wasn't connected"),
        Err(e) => return Err(format!("couldn't remove {}: {e}", path.display())),
    }
    say("  The app stays in Slack: remove it from your workspace's apps if you're done with it");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_link_carries_the_whole_manifest() {
        let link = reqwest::Url::parse(&manifest_link()).unwrap();
        assert_eq!(link.host_str(), Some("api.slack.com"));
        let manifest: Value = link
            .query_pairs()
            .find(|(k, _)| k == "manifest_json")
            .map(|(_, v)| serde_json::from_str(&v).unwrap())
            .unwrap();
        assert_eq!(manifest["display_information"]["name"], "pepe");
        let scopes = manifest["oauth_config"]["scopes"]["bot"]
            .as_array()
            .unwrap();
        assert!(scopes.iter().any(|s| s == "chat:write"));
        assert!(scopes.iter().any(|s| s == "channels:join"));
    }

    /// A Slack that answers the calls a run makes, and keeps them
    async fn fake_slack() -> (
        String,
        std::sync::Arc<std::sync::Mutex<Vec<(String, Value)>>>,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let calls = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let kept = calls.clone();
        tokio::spawn(async move {
            let mut posted = 0;
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 65536];
                let (head, body) = loop {
                    let n = sock.read(&mut buf).await.unwrap();
                    raw.extend_from_slice(&buf[..n]);
                    let text = String::from_utf8_lossy(&raw).to_string();
                    if let Some(end) = text.find("\r\n\r\n") {
                        let len: usize = text[..end]
                            .lines()
                            .find_map(|l| {
                                l.to_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|v| v.trim().parse().unwrap())
                            })
                            .unwrap_or(0);
                        if raw.len() >= end + 4 + len {
                            break (text[..end].to_string(), text[end + 4..].to_string());
                        }
                    }
                };
                let method = head
                    .split_whitespace()
                    .nth(1)
                    .unwrap()
                    .rsplit('/')
                    .next()
                    .unwrap()
                    .to_string();
                let args: serde_json::Map<String, Value> =
                    reqwest::Url::parse(&format!("http://x/?{body}"))
                        .unwrap()
                        .query_pairs()
                        .map(|(k, v)| (k.into_owned(), Value::String(v.into_owned())))
                        .collect();
                // Not yet in the channel the first time: joined, then sent again
                let answer = match method.as_str() {
                    "chat.postMessage" if posted == 0 => {
                        posted += 1;
                        json!({"ok": false, "error": "not_in_channel"})
                    }
                    "chat.postMessage" => {
                        posted += 1;
                        json!({"ok": true, "ts": format!("1.{posted}"), "channel": "C1"})
                    }
                    _ => json!({"ok": true}),
                };
                kept.lock().unwrap().push((method, Value::Object(args)));
                let body = answer.to_string();
                let _ = sock
                    .write_all(format!("HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}", body.len()).as_bytes())
                    .await;
            }
        });
        (base, calls)
    }

    #[tokio::test]
    async fn a_run_is_one_message_kept_up_to_date() {
        let (base, calls) = fake_slack().await;
        let mut live = Live::new("xoxb-test", "#load-tests");
        live.api.base = base;
        let payload = |status: &str| json!({"text": status, "blocks": [{"type": "header"}, {"type": "section", "text": {"type": "mrkdwn", "text": status}}]});
        live.show(&payload("🚀 Started")).await.unwrap();
        live.show(&payload("⏳ 40%")).await.unwrap();
        live.reply("⚠️ p99 jumped", false, Some("https://x/face.png"))
            .await
            .unwrap();
        live.end(
            &payload("❌ Failed: p99 > 300ms"),
            "<!here> ❌ Failed: p99 > 300ms",
            "https://x/worried.png",
        )
        .await
        .unwrap();
        // The end is said once, however many times it's asked
        live.end(
            &payload("❌ again"),
            "<!here> again",
            "https://x/worried.png",
        )
        .await
        .unwrap();
        let calls = calls.lock().unwrap().clone();
        let methods: Vec<&str> = calls.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(
            methods,
            [
                "chat.postMessage",
                "conversations.join",
                "chat.postMessage",
                "chat.update",
                "chat.postMessage",
                "chat.update",
                "chat.postMessage"
            ]
        );
        assert_eq!(calls[3].1["ts"], "1.2", "edited in place");
        assert_eq!(calls[3].1["channel"], "C1", "by the id Slack answered with");
        assert_eq!(calls[4].1["thread_ts"], "1.2");
        assert!(calls[4].1.get("reply_broadcast").is_none());
        assert_eq!(calls[6].1["text"], "<!here> ❌ Failed: p99 > 300ms");
        assert_eq!(
            calls[6].1["reply_broadcast"], "true",
            "a mention in the channel notifies"
        );
    }

    #[test]
    fn slacks_errors_say_what_to_do() {
        assert!(explain("invalid_auth", "").contains("pepe slack connect"));
        assert!(explain("channel_not_found", "").contains("/invite @pepe"));
        assert!(explain("missing_scope", "chat:write").contains("chat:write"));
        assert_eq!(explain("weird", ""), "Slack said weird");
    }
}

#[cfg(test)]
mod flag_tests {
    use crate::cli::Cli;
    use crate::webhook::{Event, Webhook};
    use clap::Parser;

    #[test]
    fn slack_alone_keeps_the_url_and_reads_back() {
        // --slack takes its channel after =, so the URL after it stays the URL
        let cli = Cli::parse_from(["pepe", "-n", "10", "-c", "1", "--slack", "https://a.io/"]);
        assert_eq!(
            (cli.slack.as_deref(), cli.url.as_str()),
            (Some(""), "https://a.io/")
        );
        assert!(
            cli.command_line().contains(" --slack "),
            "{}",
            cli.command_line()
        );
        let cli = Cli::parse_from(["pepe", "--slack=#perf", "https://a.io/"]);
        assert_eq!(cli.hooks()[0].url, "slack:#perf");
        assert!(
            cli.command_line().contains("--slack='#perf'"),
            "{}",
            cli.command_line()
        );
        // A Slack message is kept up to date unless said otherwise
        assert_eq!(
            Webhook::to("slack:").events(),
            [Event::Start, Event::Progress, Event::Anomaly, Event::End]
        );
        assert_eq!(crate::webhook::mask("slack:#perf"), "slack:#perf");
    }
}
