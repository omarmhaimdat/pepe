//! Webhooks: what a run says, sent where someone will see it. `--webhook
//! URL` alone posts the verdict when the run ends, laid out for the place
//! the URL belongs to (Slack, Discord, Teams, Google Chat) or as the JSON
//! report anywhere else. A `[[webhook]]` table says more: which events
//! (start, progress, anomaly, end, fail), the format, headers, a body
//! template with `{{holes}}`, how often progress goes out, and who is
//! mentioned when the run fails. `${NAME}` in a URL, a header or a body is
//! taken from the environment, so a secret stays out of the file.
//!
//! Sending never holds the run up: each webhook has its own queue, sent in
//! order with retries, and the run waits for the queues only at its end.
//! The hooks can be changed while a run goes (the screens' webhook page).

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tokio::sync::mpsc;

use crate::flow::Template;

mod layout;
pub use layout::face;

/// `cut`, for the Slack listener's messages
pub fn layout_cut(text: &str, most: usize) -> String {
    cut(text, most)
}

/// How often progress goes out when a webhook doesn't say
pub const PROGRESS_EVERY: Duration = Duration::from_secs(60);
/// Shortest progress interval: a chat channel is not a dashboard
pub const PROGRESS_FLOOR: Duration = Duration::from_secs(5);
/// How often a Slack message is edited as the run goes: an edit adds no
/// line to the channel, so it can be often
pub const SLACK_EVERY: Duration = Duration::from_secs(10);
/// How long the end of a run waits for the last messages to go out
const FLUSH: Duration = Duration::from_secs(15);
const ATTEMPTS: u32 = 3;

/// When a webhook is sent
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Event {
    /// The run started: what it sends, to where
    Start,
    /// The numbers so far, every `every`
    Progress,
    /// Something changed during the run: "p99 jumped 4.5× at 26s"
    Anomaly,
    /// The run ended: the verdict
    End,
    /// The run ended and a `--fail-if` (or a ping's `--slo`) was crossed,
    /// or nothing answered
    Fail,
}

impl Event {
    pub const ALL: [Event; 5] = [
        Event::Start,
        Event::Progress,
        Event::Anomaly,
        Event::End,
        Event::Fail,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Event::Start => "start",
            Event::Progress => "progress",
            Event::Anomaly => "anomaly",
            Event::End => "end",
            Event::Fail => "fail",
        }
    }

    pub fn parse(text: &str) -> Result<Event, String> {
        Event::ALL
            .into_iter()
            .find(|e| e.name() == text.trim().to_lowercase())
            .ok_or_else(|| {
                format!("{text:?} is not a webhook event; start, progress, anomaly, end or fail")
            })
    }

    /// `start,end`: a list, as `--webhook-on` takes it
    pub fn parse_list(text: &str) -> Result<Vec<Event>, String> {
        let mut out = Vec::new();
        for part in text.split(',').filter(|p| !p.trim().is_empty()) {
            let event = Event::parse(part)?;
            if !out.contains(&event) {
                out.push(event);
            }
        }
        if out.is_empty() {
            return Err("name at least one event: start, progress, anomaly, end or fail".into());
        }
        Ok(out)
    }
}

/// How a message is laid out
#[derive(Serialize, Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
#[serde(rename_all = "kebab-case")]
pub enum Format {
    /// Told by the URL: Slack, Discord, Teams, Google Chat, or JSON
    #[default]
    Auto,
    Json,
    Slack,
    Discord,
    Teams,
    GoogleChat,
    /// One line of text, as `text/plain`
    Text,
}

impl Format {
    pub const ALL: [Format; 7] = [
        Format::Auto,
        Format::Json,
        Format::Slack,
        Format::Discord,
        Format::Teams,
        Format::GoogleChat,
        Format::Text,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Format::Auto => "auto",
            Format::Json => "json",
            Format::Slack => "slack",
            Format::Discord => "discord",
            Format::Teams => "teams",
            Format::GoogleChat => "google-chat",
            Format::Text => "text",
        }
    }

    /// What `Auto` means for this URL
    pub fn of(self, url: &str) -> Format {
        if self != Format::Auto {
            return self;
        }
        if url.trim_start().starts_with("slack:") {
            return Format::Slack;
        }
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_lowercase))
            .unwrap_or_default();
        let path = reqwest::Url::parse(url)
            .map(|u| u.path().to_string())
            .unwrap_or_default();
        if host == "hooks.slack.com" {
            Format::Slack
        } else if (host == "discord.com"
            || host == "discordapp.com"
            || host.ends_with(".discord.com"))
            && path.starts_with("/api/webhooks")
        {
            Format::Discord
        } else if host.ends_with(".webhook.office.com")
            || host.ends_with(".logic.azure.com")
            || host.ends_with(".powerplatform.com")
        {
            Format::Teams
        } else if host == "chat.googleapis.com" {
            Format::GoogleChat
        } else {
            Format::Json
        }
    }
}

/// One webhook, as `pepe.toml` has it
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Webhook {
    pub url: String,
    /// When it is sent; the end of the run when empty
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub on: Vec<Event>,
    #[serde(default, skip_serializing_if = "is_auto")]
    pub format: Format,
    /// `Name: value`, as `-H` takes them
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub headers: Vec<String>,
    /// The body to send instead, with `{{holes}}` (see `HOLES`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// How often progress goes out: "30s", "2m"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    /// Put at the start of a message about a failed run: "<!here>",
    /// "<@U012ABC>", "@oncall"
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mention: Option<String>,
}

fn is_auto(format: &Format) -> bool {
    *format == Format::Auto
}

impl Webhook {
    pub fn to(url: &str) -> Webhook {
        Webhook {
            url: url.to_string(),
            ..Default::default()
        }
    }

    /// The events it is sent on
    pub fn events(&self) -> Vec<Event> {
        match (self.on.is_empty(), self.is_slack()) {
            (false, _) => self.on.clone(),
            // A Slack message is kept up to date as the run goes
            (true, true) => vec![Event::Start, Event::Progress, Event::Anomaly, Event::End],
            (true, false) => vec![Event::End],
        }
    }

    /// `slack:#channel`: a message of pepe's Slack app (`pepe slack connect`)
    pub fn is_slack(&self) -> bool {
        self.url.trim_start().starts_with("slack:")
    }

    /// Only a URL: written as one string in a file
    fn is_bare(&self) -> bool {
        *self == Webhook::to(&self.url)
    }

    /// The URL with what follows the host masked, for a report or a screen:
    /// a webhook's path is its secret
    pub fn masked_url(&self) -> String {
        mask(&self.url)
    }

    /// Ready to send: the environment's values in, the headers parsed, the
    /// body's holes checked
    pub fn resolve(&self) -> Result<Resolved, String> {
        if self.is_slack() {
            return self.resolve_slack();
        }
        let url = env(&self.url)?;
        let parsed = reqwest::Url::parse(&url)
            .map_err(|e| format!("webhook {}: not a URL ({e})", mask(&url)))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(format!("webhook {}: http:// or https:// only", mask(&url)));
        }
        let mut headers = reqwest::header::HeaderMap::new();
        for header in &self.headers {
            let (name, value) = crate::request::parse_header(&env(header)?)
                .map_err(|e| format!("webhook {}: {e}", mask(&url)))?;
            headers.append(name, value);
        }
        let body = match &self.body {
            Some(body) => {
                let template = Template::parse(&env(body)?);
                if let Some(hole) = template.vars().find(|v| !HOLES.contains(v)) {
                    return Err(format!(
                        "webhook {}: the body's {{{{{hole}}}}} is not one pepe fills; it has {}",
                        mask(&url),
                        HOLES.join(", ")
                    ));
                }
                Some(template)
            }
            None => None,
        };
        let every = match &self.every {
            Some(text) => {
                let ms = crate::cli::Cli::parse_duration_for("every", text)
                    .map_err(|e| format!("webhook {}: {e}", mask(&url)))?;
                Duration::from_millis(ms).max(PROGRESS_FLOOR)
            }
            None => PROGRESS_EVERY,
        };
        Ok(Resolved {
            slack: None,
            format: self.format.of(&url),
            events: self.events(),
            mention: self.mention.as_deref().map(env).transpose()?,
            url,
            headers,
            body,
            every,
            webhook: self.clone(),
        })
    }
}

impl Webhook {
    fn resolve_slack(&self) -> Result<Resolved, String> {
        let given = env(self.url.trim_start().trim_start_matches("slack:"))?;
        let given = given.trim();
        if !self.headers.is_empty() || self.body.is_some() {
            return Err(format!(
                "{}: a Slack message is laid out by pepe; headers and a body are for a URL",
                self.url
            ));
        }
        let token = crate::slack::token().ok_or_else(|| {
            format!(
                "{}: Slack isn't connected; run pepe slack connect (or set {})",
                self.url,
                crate::slack::TOKEN_ENV
            )
        })?;
        let channel = if given.is_empty() {
            crate::slack::default_channel()
                .ok_or_else(|| format!("{}: no channel; say one, as slack:#load-tests", self.url))?
        } else if given.starts_with('#') || given.chars().all(|c| c.is_ascii_alphanumeric()) {
            given.to_string()
        } else {
            format!("#{given}")
        };
        let every = match &self.every {
            Some(text) => {
                let ms = crate::cli::Cli::parse_duration_for("every", text)
                    .map_err(|e| format!("{}: {e}", self.url))?;
                Duration::from_millis(ms).max(Duration::from_secs(3))
            }
            None => SLACK_EVERY,
        };
        Ok(Resolved {
            webhook: self.clone(),
            url: self.url.clone(),
            format: Format::Slack,
            events: self.events(),
            headers: Default::default(),
            body: None,
            every,
            mention: self.mention.as_deref().map(env).transpose()?,
            slack: Some((token, channel)),
        })
    }
}

/// `${NAME}` taken from the environment; an unset one is an error
fn env(text: &str) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find("${") {
        let Some(close) = rest[open..].find('}') else {
            break;
        };
        out.push_str(&rest[..open]);
        let name = &rest[open + 2..open + close];
        match std::env::var(name) {
            Ok(value) => out.push_str(&value),
            Err(_) => {
                return Err(format!(
                    "webhook: ${{{name}}} is not set in the environment"
                ))
            }
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// `https://hooks.slack.com/••••`: scheme and host, the rest hidden. A URL
/// still written as `${NAME}` is shown as it is, since it holds no secret.
pub fn mask(url: &str) -> String {
    if url.starts_with("${") || url.trim_start().starts_with("slack:") {
        return url.to_string();
    }
    match reqwest::Url::parse(url) {
        Ok(u) => {
            let host = match (u.host_str(), u.port()) {
                (Some(host), Some(port)) => format!("{host}:{port}"),
                (Some(host), None) => host.to_string(),
                (None, _) => return "••••".into(),
            };
            if u.path().len() > 1 || u.query().is_some() {
                format!("{}://{host}/••••", u.scheme())
            } else {
                format!("{}://{host}/", u.scheme())
            }
        }
        Err(_) => "••••".into(),
    }
}

/// `webhook` in a file: a URL, a table, or a list of either
pub fn deserialize<'de, D>(de: D) -> Result<Option<Vec<Webhook>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error;
    let value = toml::Value::deserialize(de)?;
    let one = |value: toml::Value| -> Result<Webhook, D::Error> {
        match value {
            toml::Value::String(url) => Ok(Webhook::to(&url)),
            table @ toml::Value::Table(_) => table.try_into().map_err(D::Error::custom),
            other => Err(D::Error::custom(format!(
                "a webhook is a URL or a table with url = \"...\", not {}",
                other.type_str()
            ))),
        }
    };
    let hooks = match value {
        toml::Value::Array(items) => items.into_iter().map(one).collect::<Result<_, _>>()?,
        value => vec![one(value)?],
    };
    Ok(Some(hooks))
}

/// As `deserialize` reads it: one bare URL as a string
pub fn serialize<S>(hooks: &Option<Vec<Webhook>>, ser: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    match hooks.as_deref() {
        Some([one]) if one.is_bare() => ser.serialize_str(&one.url),
        Some(hooks) => hooks.serialize(ser),
        None => ser.serialize_none(),
    }
}

/// A webhook ready to send
#[derive(Debug, Clone)]
pub struct Resolved {
    pub webhook: Webhook,
    url: String,
    format: Format,
    events: Vec<Event>,
    headers: reqwest::header::HeaderMap,
    body: Option<Template>,
    every: Duration,
    mention: Option<String>,
    /// A `slack:` hook's token and channel
    slack: Option<(String, String)>,
}

/// The holes a body template can have
pub const HOLES: &[&str] = &[
    "event", "status", "level", "title", "text", "mode", "target", "test", "command", "requests",
    "failed", "rps", "p50", "p95", "p99", "errors", "duration", "progress", "verdict", "findings",
    "report",
];

/// What a run is, for every message about it
#[derive(Debug, Clone, Default)]
pub struct Run {
    /// run, ramp, ping, api, flow, replay
    pub mode: String,
    /// `GET https://…`, or the spec, the flow, the log, the ping's targets
    pub target: String,
    /// The saved test, when the run is one
    pub test: Option<String>,
    /// The command that reproduces it, secrets masked
    pub command: String,
    /// What the run sends: "for 30s, 50 at a time"
    pub load: String,
    /// The machine it runs on, and who started it
    pub host: String,
    pub who: String,
    /// How long it is planned to go, when a time says so
    pub planned: Option<Duration>,
    /// Concurrency, for the line under the title
    pub concurrency: usize,
    /// Started by `pepe slack serve`: the run's id, for its Stop button
    pub served: Option<String>,
    /// Who asked for it in Slack (a user id)
    pub by: Option<String>,
    /// `pepe slack serve` is set up: a saved test's verdict gets its buttons
    pub buttons: bool,
}

/// The numbers so far, for progress
#[derive(Debug, Clone, Default)]
pub struct Progress {
    pub elapsed: Duration,
    /// How far through the run, 0 to 1, when it has a known end
    pub done: Option<f64>,
    pub requests: u64,
    pub failed: u64,
    pub rps: f64,
    pub p50: Duration,
    pub p99: Duration,
    /// The last seconds, oldest first: requests a second and p99 in ms
    pub series: Vec<(f64, f64)>,
    /// What failed, most first: "503" or "timeout", and how many
    pub failures: Vec<(String, u64)>,
    /// Things that changed so far
    pub anomalies: usize,
}

/// How a run ended, beyond its report
#[derive(Debug, Clone, Default)]
pub struct Ending {
    /// Each `--fail-if`: whether it held, and what was measured
    pub checks: Vec<Check>,
    /// Why it failed besides those: "nothing answered", "the SLO was broken"
    pub failed: Vec<String>,
    /// Held against the saved test's last recorded run
    pub compared: Option<Compared>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Check {
    pub text: String,
    pub held: bool,
    pub measured: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Compared {
    /// Faster, Same, Slower, Better, Worse
    pub verdict: String,
    pub regression: bool,
    /// "p99 +38% · req/s −6%"
    pub summary: String,
    /// "2 h ago", "yesterday"
    pub when: String,
}

impl Ending {
    /// What made the run fail, a line each
    pub fn crossed(&self) -> Vec<String> {
        self.checks
            .iter()
            .filter(|c| !c.held)
            .map(|c| format!("{}: {}", c.text, c.measured))
            .chain(self.failed.iter().cloned())
            .collect()
    }
}

/// What happened, said once and laid out per format
#[derive(Debug, Clone)]
struct Message {
    event: Event,
    /// ✅ ⚠️ ❌ and the colour that goes with them
    tone: Tone,
    title: String,
    /// One line: "Passed · healthy", "Failed: p99 > 300ms (p99 was 412ms)"
    status: String,
    fields: Vec<(String, String)>,
    findings: Vec<String>,
    footer: String,
    /// The JSON format's body
    data: Value,
    /// The template's values
    values: HashMap<String, String>,
    /// What the richer layouts (Slack's) are drawn from
    detail: Detail,
}

/// The run as it stands, for a layout that says more than a few lines
#[derive(Debug, Clone, Default)]
struct Detail {
    run: Run,
    progress: Option<Progress>,
    report: Option<Value>,
    ending: Option<Ending>,
    /// The last progress's seconds, for a short run's sparklines at its end
    series: Vec<(f64, f64)>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Tone {
    Info,
    Good,
    Warn,
    Bad,
}

impl Tone {
    fn icon(self) -> &'static str {
        match self {
            Tone::Info => "⏳",
            Tone::Good => "✅",
            Tone::Warn => "⚠️",
            Tone::Bad => "❌",
        }
    }

    /// Discord's embed colour
    fn rgb(self) -> u32 {
        match self {
            Tone::Info => 0x5865f2,
            Tone::Good => 0x2eb67d,
            Tone::Warn => 0xecb22e,
            Tone::Bad => 0xe01e5a,
        }
    }

    /// A Teams TextBlock's colour
    fn teams(self) -> &'static str {
        match self {
            Tone::Info => "accent",
            Tone::Good => "good",
            Tone::Warn => "warning",
            Tone::Bad => "attention",
        }
    }
}

fn ms(d: Duration) -> String {
    crate::ui::format::latency(d)
}

fn count(n: u64) -> String {
    crate::ui::format::count(n)
}

impl Run {
    fn title(&self) -> String {
        match &self.test {
            Some(test) => format!("pepe {} · {test} · {}", self.mode, self.target),
            None => format!("pepe {} · {}", self.mode, self.target),
        }
    }

    fn footer(&self) -> String {
        match &self.test {
            Some(test) => format!("{} · test {test}", self.command),
            None => self.command.clone(),
        }
    }

    fn values(&self, event: Event) -> HashMap<String, String> {
        let mut values: HashMap<String, String> = HOLES
            .iter()
            .map(|h| (h.to_string(), String::new()))
            .collect();
        for (k, v) in [
            ("event", event.name().to_string()),
            ("mode", self.mode.clone()),
            ("target", self.target.clone()),
            ("test", self.test.clone().unwrap_or_default()),
            ("command", self.command.clone()),
            ("title", self.title()),
        ] {
            values.insert(k.into(), v);
        }
        values
    }

    fn base(&self, event: Event) -> Value {
        json!({
            "event": event.name(),
            "pepe": crate::utils::version(),
            "mode": self.mode,
            "target": self.target,
            "test": self.test,
            "command": self.command,
        })
    }

    fn start(&self) -> Message {
        let mut values = self.values(Event::Start);
        let text = format!("Started: {}", self.load);
        values.insert("status".into(), "running".into());
        values.insert("text".into(), format!("🚀 {} · {text}", self.title()));
        let mut data = self.base(Event::Start);
        data["load"] = json!(self.load);
        Message {
            event: Event::Start,
            tone: Tone::Info,
            title: self.title(),
            status: format!("🚀 {text}"),
            fields: Vec::new(),
            findings: Vec::new(),
            footer: self.footer(),
            data,
            values,
            detail: Detail::default(),
        }
    }

    fn progress(&self, p: &Progress) -> Message {
        let errors = if p.requests == 0 {
            0.0
        } else {
            p.failed as f64 * 100.0 / p.requests as f64
        };
        let bar = match p.done {
            Some(done) => {
                let filled = (done.clamp(0.0, 1.0) * 10.0).round() as usize;
                format!(
                    "{}{} {:.0}% · ",
                    "▰".repeat(filled),
                    "▱".repeat(10 - filled),
                    done * 100.0
                )
            }
            None => String::new(),
        };
        let line = format!(
            "{bar}{} in · {} requests · {:.0}/s · p50 {} · p99 {} · {errors:.1}% errors",
            crate::ui::format::span(p.elapsed),
            count(p.requests),
            p.rps,
            ms(p.p50),
            ms(p.p99),
        );
        let mut values = self.values(Event::Progress);
        for (k, v) in [
            ("status", "running".to_string()),
            ("text", format!("⏳ {} · {line}", self.title())),
            ("requests", p.requests.to_string()),
            ("failed", p.failed.to_string()),
            ("rps", format!("{:.1}", p.rps)),
            ("p50", ms(p.p50)),
            ("p99", ms(p.p99)),
            ("errors", format!("{errors:.2}%")),
            ("duration", crate::ui::format::span(p.elapsed)),
            (
                "progress",
                p.done
                    .map(|d| format!("{:.0}%", d * 100.0))
                    .unwrap_or_default(),
            ),
        ] {
            values.insert(k.into(), v);
        }
        let mut data = self.base(Event::Progress);
        data["elapsed_s"] = json!(p.elapsed.as_secs_f64());
        data["done"] = json!(p.done);
        data["summary"] = json!({
            "total_requests": p.requests,
            "failed_requests": p.failed,
            "requests_per_second": p.rps,
            "latency": {"median_ms": p.p50.as_secs_f64() * 1e3, "p99_ms": p.p99.as_secs_f64() * 1e3},
        });
        Message {
            event: Event::Progress,
            tone: Tone::Info,
            title: self.title(),
            status: format!("⏳ {line}"),
            fields: Vec::new(),
            findings: Vec::new(),
            footer: self.footer(),
            data,
            values,
            detail: Detail::default(),
        }
    }

    fn anomaly(&self, text: &str) -> Message {
        let mut values = self.values(Event::Anomaly);
        values.insert("status".into(), "running".into());
        values.insert("text".into(), format!("⚠️ {} · {text}", self.title()));
        values.insert("findings".into(), text.to_string());
        let mut data = self.base(Event::Anomaly);
        data["text"] = json!(text);
        Message {
            event: Event::Anomaly,
            tone: Tone::Warn,
            title: self.title(),
            status: format!("⚠️ {text}"),
            fields: Vec::new(),
            findings: Vec::new(),
            footer: self.footer(),
            data,
            values,
            detail: Detail::default(),
        }
    }

    /// The end: `report` is the run's JSON report, `crossed` what made it
    /// fail (a `--fail-if`, an SLO), if anything did
    fn end(&self, event: Event, report: &Value, ending: &Ending) -> Message {
        let crossed = &ending.crossed()[..];
        let outcome = Outcome::of(report);
        let failed = !crossed.is_empty();
        let tone = match (failed, outcome.level.as_str()) {
            (true, _) | (_, "failing") => Tone::Bad,
            (_, "degraded") => Tone::Warn,
            _ => Tone::Good,
        };
        let status = if failed {
            format!("{} Failed: {}", tone.icon(), crossed.join("; "))
        } else {
            format!(
                "{} {} · {}",
                tone.icon(),
                if outcome.level == "failing" {
                    "Done"
                } else {
                    "Passed"
                },
                outcome.level
            )
        };
        let mut values = self.values(event);
        let get = |k: &str| {
            outcome
                .fields
                .iter()
                .find(|(n, _)| n == k)
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        for (k, v) in [
            (
                "status",
                if failed { "failed" } else { "passed" }.to_string(),
            ),
            ("level", outcome.level.clone()),
            ("text", format!("{status} · {}", self.title())),
            ("requests", outcome.requests.to_string()),
            ("failed", outcome.failed.to_string()),
            ("rps", get("Req/s")),
            ("p50", get("p50")),
            ("p95", get("p95")),
            ("p99", get("p99")),
            ("errors", get("Errors")),
            ("duration", get("Duration")),
            (
                "verdict",
                outcome.findings.first().cloned().unwrap_or_default(),
            ),
            ("findings", outcome.findings.join("\n")),
            ("report", report.to_string()),
        ] {
            values.insert(k.into(), v);
        }
        let mut data = self.base(event);
        data["status"] = json!(if failed { "failed" } else { "passed" });
        data["crossed"] = json!(crossed);
        data["report"] = report.clone();
        Message {
            event,
            tone,
            title: self.title(),
            status,
            fields: outcome.fields,
            findings: outcome.findings,
            footer: self.footer(),
            data,
            values,
            detail: Detail::default(),
        }
    }
}

/// The numbers a report's end message shows, whichever mode wrote it
struct Outcome {
    level: String,
    requests: u64,
    failed: u64,
    fields: Vec<(String, String)>,
    findings: Vec<String>,
}

impl Outcome {
    fn of(report: &Value) -> Outcome {
        let num = |v: &Value| v.as_f64().unwrap_or(0.0);
        let latency = |v: &Value| ms(Duration::from_secs_f64(num(v).max(0.0) / 1e3));
        let texts = |v: &Value| -> Vec<String> {
            v.as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|f| f["text"].as_str().map(str::to_string))
                        .collect()
                })
                .unwrap_or_default()
        };
        if let Some(summary) = report.get("summary") {
            let requests = summary["total_requests"].as_u64().unwrap_or(0);
            let failed = summary["failed_requests"].as_u64().unwrap_or(0);
            let errors = if requests == 0 {
                0.0
            } else {
                failed as f64 * 100.0 / requests as f64
            };
            let lat = &summary["latency"];
            return Outcome {
                level: report["verdict"]["level"]
                    .as_str()
                    .unwrap_or("healthy")
                    .to_string(),
                requests,
                failed,
                fields: vec![
                    ("Requests".into(), count(requests)),
                    (
                        "Req/s".into(),
                        count(num(&summary["requests_per_second"]).round() as u64),
                    ),
                    ("p50".into(), latency(&lat["median_ms"])),
                    ("p95".into(), latency(&lat["p95_ms"])),
                    ("p99".into(), latency(&lat["p99_ms"])),
                    ("Errors".into(), format!("{errors:.2}%")),
                    (
                        "Duration".into(),
                        crate::ui::format::span(Duration::from_secs_f64(
                            summary["duration_ms"].as_f64().unwrap_or(0.0).max(0.0) / 1e3,
                        )),
                    ),
                ],
                findings: texts(&report["verdict"]["findings"]),
            };
        }
        if report.get("holds_concurrency").is_some() {
            let requests = report["requests"].as_u64().unwrap_or(0);
            let failed = report["failed_requests"].as_u64().unwrap_or(0);
            let mut fields = vec![(
                "Held".into(),
                match report["holds_concurrency"].as_u64() {
                    Some(c) => format!("{c} at once"),
                    None => "no step".into(),
                },
            )];
            if let Some(rps) = report["capacity"]["requests_per_second"].as_f64() {
                fields.push(("Capacity".into(), format!("about {rps:.0} req/s")));
            }
            fields.push(("Requests".into(), count(requests)));
            fields.push((
                "Errors".into(),
                format!(
                    "{:.2}%",
                    if requests == 0 {
                        0.0
                    } else {
                        failed as f64 * 100.0 / requests as f64
                    }
                ),
            ));
            fields.push((
                "Duration".into(),
                crate::ui::format::span(Duration::from_secs_f64(num(&report["seconds"]))),
            ));
            let findings = texts(&report["findings"]);
            let level = match report["findings"].as_array() {
                Some(f) if f.iter().any(|f| f["level"] == "bad") => "failing",
                Some(f) if f.iter().any(|f| f["level"] == "warn") => "degraded",
                _ => "healthy",
            };
            return Outcome {
                level: level.into(),
                requests,
                failed,
                fields,
                findings,
            };
        }
        // A ping: a line per target
        let targets = report["targets"].as_array().cloned().unwrap_or_default();
        let mut fields = Vec::new();
        let mut findings = Vec::new();
        let (mut requests, mut failed) = (0, 0);
        let mut level = "healthy";
        for t in &targets {
            let name = t["name"].as_str().or(t["target"].as_str()).unwrap_or("?");
            let lat = &t["latency_ms"];
            fields.push((
                name.to_string(),
                format!(
                    "p50 {} · p99 {} · loss {:.1}%",
                    latency(&lat["p50"]),
                    latency(&lat["p99"]),
                    num(&t["loss"]) * 100.0
                ),
            ));
            requests += t["sent"].as_u64().unwrap_or(0);
            failed += t["lost"].as_u64().unwrap_or(0);
            for f in t["findings"].as_array().into_iter().flatten() {
                if f["level"] == "bad" {
                    level = "failing";
                } else if f["level"] == "warn" && level == "healthy" {
                    level = "degraded";
                }
                if let Some(text) = f["text"].as_str() {
                    findings.push(if targets.len() > 1 {
                        format!("{name}: {text}")
                    } else {
                        text.to_string()
                    });
                }
            }
        }
        Outcome {
            level: level.into(),
            requests,
            failed,
            fields,
            findings,
        }
    }
}

/// Cut to `most` characters, with an ellipsis when cut
fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_string();
    }
    let mut out: String = text.chars().take(most.saturating_sub(1)).collect();
    out.push('…');
    out
}

impl Message {
    /// The body and its content type, laid out for `format`
    fn render(&self, hook: &Resolved) -> (String, &'static str) {
        let mention = match (&hook.mention, self.tone) {
            (Some(m), Tone::Bad) => format!("{m} "),
            _ => String::new(),
        };
        if let Some(template) = &hook.body {
            let mut values = self.values.clone();
            values.insert("text".into(), format!("{mention}{}", values["text"]));
            let body = template.render(&values).unwrap_or_default();
            let kind = if serde_json::from_str::<Value>(&body).is_ok() {
                "application/json"
            } else {
                "text/plain; charset=utf-8"
            };
            return (body, kind);
        }
        let findings: Vec<String> = self
            .findings
            .iter()
            .take(6)
            .map(|f| format!("• {f}"))
            .collect();
        let body = match hook.format {
            Format::Auto | Format::Json => {
                let mut data = self.data.clone();
                if !mention.is_empty() {
                    data["mention"] = json!(hook.mention);
                }
                data
            }
            Format::Slack => self.slack_payload(
                &mention,
                matches!(self.event, Event::Progress | Event::Anomaly),
            ),
            Format::Discord => {
                let fields: Vec<Value> = self
                    .fields
                    .iter()
                    .take(25)
                    .map(|(n, v)| json!({"name": cut(n, 256), "value": cut(v, 1024), "inline": true}))
                    .collect();
                json!({
                    "content": cut(&format!("{mention}{}", self.status), 2000),
                    "embeds": [{
                        "title": cut(&self.title, 256),
                        "description": cut(&findings.join("\n"), 4096),
                        "color": self.tone.rgb(),
                        "fields": fields,
                        "footer": {"text": cut(&self.footer, 2048)},
                    }],
                    "allowed_mentions": {"parse": ["everyone", "roles", "users"]},
                })
            }
            Format::Teams => {
                let mut body = vec![
                    json!({"type": "TextBlock", "text": self.title, "weight": "Bolder", "size": "Medium", "wrap": true}),
                    json!({"type": "TextBlock", "text": format!("{mention}{}", self.status), "color": self.tone.teams(), "wrap": true}),
                ];
                if !self.fields.is_empty() {
                    let facts: Vec<Value> = self
                        .fields
                        .iter()
                        .map(|(n, v)| json!({"title": n, "value": v}))
                        .collect();
                    body.push(json!({"type": "FactSet", "facts": facts}));
                }
                if !findings.is_empty() {
                    body.push(
                        json!({"type": "TextBlock", "text": findings.join("\n\n"), "wrap": true}),
                    );
                }
                body.push(json!({"type": "TextBlock", "text": self.footer, "isSubtle": true, "wrap": true, "fontType": "Monospace", "size": "Small"}));
                json!({
                    "type": "message",
                    "attachments": [{
                        "contentType": "application/vnd.microsoft.card.adaptive",
                        "content": {
                            "$schema": "http://adaptivecards.io/schemas/adaptive-card.json",
                            "type": "AdaptiveCard",
                            "version": "1.4",
                            "body": body,
                        },
                    }],
                })
            }
            Format::GoogleChat => json!({"text": self.plain(&mention)}),
            Format::Text => return (self.plain(&mention), "text/plain; charset=utf-8"),
        };
        (body.to_string(), "application/json")
    }

    /// Slack's blocks: the title, the status, the numbers, the findings and
    /// the command; `compact` is one line under the title, for progress
    /// and anomalies sent as messages of their own
    fn slack_payload(&self, mention: &str, compact: bool) -> Value {
        let findings: Vec<String> = self
            .findings
            .iter()
            .take(6)
            .map(|f| format!("• {f}"))
            .collect();
        let mut blocks = vec![
            json!({"type": "header", "text": {"type": "plain_text", "text": cut(&self.title, 150), "emoji": true}}),
            json!({"type": "section", "text": {"type": "mrkdwn", "text": cut(&format!("{mention}{}", self.status), 3000)}}),
        ];
        if !self.fields.is_empty() {
            let fields: Vec<Value> = self
                .fields
                .iter()
                .take(10)
                .map(|(n, v)| json!({"type": "mrkdwn", "text": cut(&format!("*{n}*\n{v}"), 2000)}))
                .collect();
            blocks.push(json!({"type": "section", "fields": fields}));
        }
        if !findings.is_empty() {
            blocks.push(json!({"type": "section", "text": {"type": "mrkdwn", "text": cut(&findings.join("\n"), 3000)}}));
        }
        blocks.push(json!({"type": "context", "elements": [{"type": "mrkdwn", "text": cut(&format!("`{}`", self.footer), 3000)}]}));
        // Progress and anomalies are short: one line, no header
        if compact {
            blocks = vec![
                json!({"type": "section", "text": {"type": "mrkdwn", "text": cut(&format!("*{}*\n{}", self.title, self.status), 3000)}}),
            ];
        }
        json!({"text": cut(&format!("{mention}{} · {}", self.status, self.title), 3000), "blocks": blocks})
    }

    /// The message as text: a few lines
    fn plain(&self, mention: &str) -> String {
        let mut out = format!("{mention}{}\n{}", self.status, self.title);
        if !self.fields.is_empty() {
            let fields: Vec<String> = self
                .fields
                .iter()
                .map(|(n, v)| format!("{n} {v}"))
                .collect();
            out.push_str(&format!("\n{}", fields.join(" · ")));
        }
        for f in self.findings.iter().take(6) {
            out.push_str(&format!("\n• {f}"));
        }
        out.push_str(&format!("\n{}", self.footer));
        out
    }
}

/// One webhook's queue, sent in order by its own task
struct Line {
    hook: Resolved,
    tx: Option<mpsc::UnboundedSender<Message>>,
    task: Option<tokio::task::JoinHandle<()>>,
    /// When progress last went out
    last: Instant,
}

impl Line {
    fn open(hook: Resolved, client: reqwest::Client) -> Line {
        let (tx, mut rx) = mpsc::unbounded_channel::<Message>();
        let task = tokio::spawn({
            let hook = hook.clone();
            async move {
                // Slack: one message for the run, kept up to date
                let mut live = hook
                    .slack
                    .as_ref()
                    .map(|(token, channel)| crate::slack::Live::new(token, channel));
                while let Some(message) = rx.recv().await {
                    let sent = match &mut live {
                        Some(live) => to_slack(live, &hook, &message).await,
                        None => {
                            let (body, kind) = message.render(&hook);
                            deliver(&client, &hook, body, kind).await
                        }
                    };
                    delivered(&hook.webhook.url, sent.as_ref().err().cloned());
                    match sent {
                        Err(e) if hook.slack.is_some() => {
                            problem(format!("{}: {e}", hook.webhook.masked_url()))
                        }
                        Err(e) => problem(e),
                        Ok(()) => {}
                    }
                }
            }
        });
        Line {
            hook,
            tx: Some(tx),
            task: Some(task),
            last: Instant::now(),
        }
    }

    fn send(&self, message: &Message) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(message.clone());
        }
    }
}

/// One event, to the run's Slack message: the start posts it, progress
/// edits it, an anomaly goes in its thread, the end makes it the verdict
/// (and a failed run's mention goes in the thread, shown in the channel
/// too, since an edit notifies no one)
async fn to_slack(
    live: &mut crate::slack::Live,
    hook: &Resolved,
    message: &Message,
) -> Result<(), String> {
    match message.event {
        Event::Start | Event::Progress => live.show(&layout::live(message)).await,
        Event::Anomaly => {
            let (text, icon) = layout::anomaly(message);
            live.reply(&text, false, Some(&icon)).await
        }
        Event::End | Event::Fail => {
            let (line, icon) = layout::result_line(message, hook.mention.as_deref());
            live.end(&layout::verdict(message), &line, &icon).await
        }
    }
}

/// POST, again on a dropped connection, a 429 or a 5xx
async fn deliver(
    client: &reqwest::Client,
    hook: &Resolved,
    body: String,
    kind: &'static str,
) -> Result<(), String> {
    let mut wait = Duration::from_secs(1);
    let mut last = String::new();
    for attempt in 1..=ATTEMPTS {
        let sent = client
            .post(&hook.url)
            .headers(hook.headers.clone())
            .header(reqwest::header::CONTENT_TYPE, kind)
            .body(body.clone())
            .send()
            .await;
        match sent {
            Ok(r) if r.status().is_success() => return Ok(()),
            Ok(r) => {
                let status = r.status();
                let after = r
                    .headers()
                    .get(reqwest::header::RETRY_AFTER)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .map(|s| Duration::from_secs(s.min(10)));
                let text = r.text().await.unwrap_or_default();
                last = format!("HTTP {}: {}", status.as_u16(), cut(text.trim(), 200));
                if !(status.as_u16() == 429 || status.is_server_error()) {
                    break;
                }
                if let Some(after) = after {
                    wait = after;
                }
            }
            Err(e) => last = crate::response::Failure::from(&e).message,
        }
        if attempt < ATTEMPTS {
            tokio::time::sleep(wait).await;
            wait *= 2;
        }
    }
    Err(format!(
        "webhook {} wasn't sent: {last}",
        hook.webhook.masked_url()
    ))
}

/// What the webhooks of the run going now are, and where they stand
struct State {
    run: Run,
    /// The seconds the last progress had
    series: Vec<(f64, f64)>,
    client: reqwest::Client,
    lines: Vec<Line>,
    /// Queues closed by a change of hooks, still sending what they had
    closing: Vec<tokio::task::JoinHandle<()>>,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
/// Each hook's last delivery, by URL: when, and what went wrong if it did
static DELIVERED: Mutex<Vec<(String, Instant, Option<String>)>> = Mutex::new(Vec::new());

fn delivered(url: &str, error: Option<String>) {
    if let Ok(mut all) = DELIVERED.lock() {
        all.retain(|(u, _, _)| u != url);
        all.push((url.to_string(), Instant::now(), error));
    }
}

/// How the hook at `url` last went: how long ago, and its error if any
pub fn last_delivery(url: &str) -> Option<(Duration, Option<String>)> {
    DELIVERED.lock().ok().and_then(|all| {
        all.iter()
            .find(|(u, _, _)| u == url)
            .map(|(_, at, error)| (at.elapsed(), error.clone()))
    })
}
/// The hooks as the screens' webhook page left them: they win over flags
/// and files for the rest of this pepe
static EDITED: Mutex<Option<Vec<Webhook>>> = Mutex::new(None);
/// The hooks and the run last begun, for the page to open on
static LAST: Mutex<Option<(Vec<Webhook>, Run)>> = Mutex::new(None);
/// A run has begun and not ended, hooks or none: one added now is sent to
static GOING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The hooks the screens' page set, when it set any
pub fn edited() -> Option<Vec<Webhook>> {
    EDITED.lock().ok().and_then(|e| e.clone())
}

/// The screens' page changed the hooks: from now on these are the run's,
/// and a run going now sends to them at once. Err names the first that
/// doesn't resolve; it is kept all the same, to be fixed.
pub fn edit(hooks: Vec<Webhook>) -> Result<(), String> {
    if let Ok(mut edited) = EDITED.lock() {
        *edited = Some(hooks.clone());
    }
    let live: Vec<Webhook> = hooks
        .iter()
        .filter(|h| h.resolve().is_ok())
        .cloned()
        .collect();
    let run = LAST
        .lock()
        .ok()
        .and_then(|l| l.as_ref().map(|(_, run)| run.clone()))
        .unwrap_or_default();
    if self::live() {
        replace(&live, || run)?;
    }
    check(&hooks)
}

/// The hooks a screen starts from, before any run: the setup form's
pub fn seed(hooks: &[Webhook]) {
    if let Ok(mut last) = LAST.lock() {
        let run = last.take().map(|(_, run)| run).unwrap_or_default();
        *last = Some((hooks.to_vec(), run));
    }
}

/// What the page opens on: what it left, or the hooks last begun or seeded
pub fn editable() -> (Vec<Webhook>, Run) {
    let last = LAST.lock().ok().and_then(|l| l.clone()).unwrap_or_default();
    (edited().unwrap_or(last.0), last.1)
}

/// Whether a run is sending to webhooks now
pub fn live() -> bool {
    GOING.load(std::sync::atomic::Ordering::Relaxed)
}
static PROBLEMS: Mutex<Vec<String>> = Mutex::new(Vec::new());

fn problem(text: String) {
    if let Ok(mut problems) = PROBLEMS.lock() {
        problems.push(text);
    }
}

/// Messages that couldn't be sent, each said once
pub fn take_problems() -> Vec<String> {
    PROBLEMS
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default()
}

fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(crate::utils::default_user_agent())
        .build()
        .unwrap_or_default()
}

/// Check the hooks before a run: each resolves, or the first problem
pub fn check(hooks: &[Webhook]) -> Result<(), String> {
    hooks.iter().try_for_each(|h| h.resolve().map(|_| ()))
}

/// The run starts: its hooks are opened and those on `start` sent. With no
/// hooks nothing is kept, and every call below does nothing.
pub fn begin(run: Run, hooks: &[Webhook]) {
    let hooks = edited().unwrap_or_else(|| hooks.to_vec());
    if let Ok(mut last) = LAST.lock() {
        *last = Some((hooks.clone(), run.clone()));
    }
    GOING.store(true, std::sync::atomic::Ordering::Relaxed);
    let client = client();
    let lines: Vec<Line> = hooks
        .iter()
        .filter_map(|h| match h.resolve() {
            Ok(hook) => Some(Line::open(hook, client.clone())),
            Err(e) => {
                problem(e);
                None
            }
        })
        .collect();
    let Ok(mut state) = STATE.lock() else { return };
    if let Some(old) = state.take() {
        close(old);
    }
    if lines.is_empty() {
        return;
    }
    let mut message = run.start();
    message.detail.run = run.clone();
    for line in lines
        .iter()
        .filter(|l| l.hook.events.contains(&Event::Start))
    {
        line.send(&message);
    }
    *state = Some(State {
        series: Vec::new(),
        run,
        client,
        lines,
        closing: Vec::new(),
    });
}

fn close(mut state: State) {
    for line in &mut state.lines {
        line.tx = None;
    }
}

/// Whether any hook wants progress now; cheap, so it can be asked often
pub fn progress_due() -> bool {
    let Ok(state) = STATE.lock() else {
        return false;
    };
    state.as_ref().is_some_and(|s| {
        s.lines
            .iter()
            .any(|l| l.hook.events.contains(&Event::Progress) && l.last.elapsed() >= l.hook.every)
    })
}

/// The numbers so far, to the hooks whose interval has passed
pub fn progress(numbers: impl FnOnce() -> Progress) {
    if !progress_due() {
        return;
    }
    let numbers = numbers();
    let Ok(mut state) = STATE.lock() else { return };
    let Some(state) = state.as_mut() else { return };
    let mut message = state.run.progress(&numbers);
    if !numbers.series.is_empty() {
        state.series = numbers.series.clone();
    }
    message.detail.run = state.run.clone();
    message.detail.progress = Some(numbers);
    for line in state.lines.iter_mut() {
        if line.hook.events.contains(&Event::Progress) && line.last.elapsed() >= line.hook.every {
            line.last = Instant::now();
            line.send(&message);
        }
    }
}

/// Something changed during the run
pub fn anomaly(text: &str) {
    let Ok(state) = STATE.lock() else { return };
    let Some(state) = state.as_ref() else { return };
    let mut message = state.run.anomaly(text);
    message.detail.run = state.run.clone();
    for line in state
        .lines
        .iter()
        .filter(|l| l.hook.events.contains(&Event::Anomaly))
    {
        line.send(&message);
    }
}

/// The run ended: the verdict to the hooks on `end`, and to those on
/// `fail` when `crossed` says why it failed. The messages go out in the
/// background; `flush` waits for them.
pub fn end(report: &Value, ending: &Ending) {
    GOING.store(false, std::sync::atomic::Ordering::Relaxed);
    let Ok(mut state) = STATE.lock() else { return };
    let Some(mut state) = state.take() else {
        return;
    };
    let failed = !ending.crossed().is_empty();
    for event in [Event::End, Event::Fail] {
        if event == Event::Fail && !failed {
            continue;
        }
        let mut message = state.run.end(event, report, ending);
        message.detail = Detail {
            run: state.run.clone(),
            report: Some(report.clone()),
            ending: Some(ending.clone()),
            series: state.series.clone(),
            ..Detail::default()
        };
        for line in state
            .lines
            .iter()
            .filter(|l| l.hook.events.contains(&event))
        {
            line.send(&message);
        }
    }
    let mut tasks = std::mem::take(&mut state.closing);
    for line in &mut state.lines {
        line.tx = None;
        tasks.extend(line.task.take());
    }
    if let Ok(mut pending) = PENDING.lock() {
        pending.extend(tasks);
    }
}

/// Before pepe exits: wait for what is still being sent, for at most
/// `FLUSH`, and say on stderr what couldn't be
pub async fn flush() {
    let tasks = PENDING
        .lock()
        .map(|mut p| std::mem::take(&mut *p))
        .unwrap_or_default();
    let open = STATE.lock().ok().and_then(|mut s| s.take());
    let mut tasks = tasks;
    if let Some(mut state) = open {
        tasks.append(&mut state.closing);
        for line in &mut state.lines {
            line.tx = None;
            tasks.extend(line.task.take());
        }
    }
    let _ = tokio::time::timeout(FLUSH, futures_join(tasks)).await;
    for text in take_problems() {
        eprintln!("warning: {text}");
    }
}

static PENDING: Mutex<Vec<tokio::task::JoinHandle<()>>> = Mutex::new(Vec::new());

async fn futures_join(tasks: Vec<tokio::task::JoinHandle<()>>) {
    for task in tasks {
        let _ = task.await;
    }
}

/// The hooks of the run going now, as they stand
#[cfg(test)]
pub fn current() -> Vec<Webhook> {
    STATE
        .lock()
        .ok()
        .and_then(|s| {
            s.as_ref()
                .map(|s| s.lines.iter().map(|l| l.hook.webhook.clone()).collect())
        })
        .unwrap_or_default()
}

/// Change the hooks while the run goes: from now on these are sent to.
/// Err names the first that doesn't resolve, and nothing changes then.
pub fn replace(hooks: &[Webhook], run: impl FnOnce() -> Run) -> Result<(), String> {
    let resolved = hooks
        .iter()
        .map(Webhook::resolve)
        .collect::<Result<Vec<_>, _>>()?;
    let Ok(mut state) = STATE.lock() else {
        return Ok(());
    };
    match state.as_mut() {
        Some(state) => {
            let client = state.client.clone();
            let old = std::mem::take(&mut state.lines);
            for mut line in old {
                line.tx = None;
                state.closing.extend(line.task.take());
            }
            state.lines = resolved
                .into_iter()
                .map(|h| Line::open(h, client.clone()))
                .collect();
        }
        None if !resolved.is_empty() => {
            let client = client();
            *state = Some(State {
                series: Vec::new(),
                run: run(),
                lines: resolved
                    .into_iter()
                    .map(|h| Line::open(h, client.clone()))
                    .collect(),
                client,
                closing: Vec::new(),
            });
        }
        None => {}
    }
    Ok(())
}

/// A test message to one hook, now, whatever its events; the answer is
/// what came back
pub async fn try_hook(hook: &Webhook, run: &Run) -> Result<(), String> {
    let sent = send_test(hook, run).await;
    delivered(&hook.url, sent.as_ref().err().cloned());
    sent
}

async fn send_test(hook: &Webhook, run: &Run) -> Result<(), String> {
    let resolved = hook.resolve()?;
    let mut message = run.start();
    message.status = format!("🔔 A test from pepe: this webhook works · {}", run.load);
    message.values.insert("status".into(), "test".into());
    message.values.insert("event".into(), "test".into());
    message.values.insert(
        "text".into(),
        format!("🔔 {} · a test from pepe", run.title()),
    );
    message.data["event"] = json!("test");
    if let Some((token, channel)) = &resolved.slack {
        return crate::slack::Live::new(token, channel)
            .show(&message.slack_payload("", false))
            .await;
    }
    let (body, kind) = message.render(&resolved);
    deliver(&client(), &resolved, body, kind).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run() -> Run {
        Run {
            mode: "run".into(),
            target: "GET https://api.example.com/health".into(),
            test: Some("health".into()),
            command: "pepe -z 30s https://api.example.com/health".into(),
            load: "for 30s, 50 at a time".into(),
            ..Run::default()
        }
    }

    fn report() -> Value {
        json!({
            "schema_version": 1,
            "summary": {
                "total_requests": 1000, "failed_requests": 10, "requests_per_second": 250.5,
                "duration_ms": 4000,
                "latency": {"median_ms": 12.0, "p95_ms": 40.0, "p99_ms": 80.0},
            },
            "verdict": {"level": "degraded", "findings": [{"level": "warn", "text": "1% of requests failed"}]},
        })
    }

    #[test]
    fn the_format_is_told_by_the_url() {
        let of = |u| Format::Auto.of(u);
        assert_eq!(of("https://hooks.slack.com/services/T/B/x"), Format::Slack);
        assert_eq!(of("https://discord.com/api/webhooks/1/x"), Format::Discord);
        assert_eq!(
            of("https://acme.webhook.office.com/webhookb2/x"),
            Format::Teams
        );
        assert_eq!(
            of("https://prod-1.westus.logic.azure.com/workflows/x"),
            Format::Teams
        );
        assert_eq!(
            of("https://chat.googleapis.com/v1/spaces/x/messages"),
            Format::GoogleChat
        );
        assert_eq!(of("https://example.com/hook"), Format::Json);
        assert_eq!(Format::Text.of("https://hooks.slack.com/x"), Format::Text);
    }

    #[test]
    fn a_file_says_a_url_a_table_or_a_list() {
        #[derive(Deserialize, Serialize)]
        struct F {
            #[serde(
                default,
                deserialize_with = "deserialize",
                serialize_with = "serialize"
            )]
            webhook: Option<Vec<Webhook>>,
        }
        let one: F = toml::from_str(r#"webhook = "https://hooks.slack.com/x""#).unwrap();
        assert_eq!(
            one.webhook.as_ref().unwrap()[0].url,
            "https://hooks.slack.com/x"
        );
        assert_eq!(
            toml::to_string(&one).unwrap().trim(),
            r#"webhook = "https://hooks.slack.com/x""#
        );
        let many: F = toml::from_str(
            r#"
            [[webhook]]
            url = "${HOOK}"
            on = ["start", "fail"]
            mention = "<!here>"
            [[webhook]]
            url = "https://example.com/a"
            "#,
        )
        .unwrap();
        let hooks = many.webhook.clone().unwrap();
        assert_eq!(hooks[0].on, [Event::Start, Event::Fail]);
        assert_eq!(hooks[1].events(), [Event::End]);
        let again: F = toml::from_str(&toml::to_string(&many).unwrap()).unwrap();
        assert_eq!(again.webhook, many.webhook);
        let typo = toml::from_str::<F>("[[webhook]]\nurl = \"x\"\nevery-time = 1\n");
        assert!(typo.err().unwrap().to_string().contains("every-time"));
        assert!(toml::from_str::<F>("webhook = 3").is_err());
    }

    #[test]
    fn secrets_come_from_the_environment_and_stay_masked() {
        std::env::set_var(
            "PEPE_TEST_HOOK",
            "https://hooks.slack.com/services/T/B/secret",
        );
        let hook = Webhook {
            url: "${PEPE_TEST_HOOK}".into(),
            body: Some(r#"{"text": "{{text}}"}"#.into()),
            ..Default::default()
        };
        let resolved = hook.resolve().unwrap();
        assert_eq!(resolved.url, "https://hooks.slack.com/services/T/B/secret");
        assert_eq!(resolved.format, Format::Slack);
        assert_eq!(hook.masked_url(), "${PEPE_TEST_HOOK}");
        assert_eq!(mask(&resolved.url), "https://hooks.slack.com/••••");
        let unset = Webhook::to("${PEPE_TEST_NOT_SET}").resolve().unwrap_err();
        assert!(unset.contains("PEPE_TEST_NOT_SET"), "{unset}");
        let hole = Webhook {
            body: Some("{{nope}}".into()),
            ..Webhook::to("https://x.io/h")
        };
        assert!(hole.resolve().unwrap_err().contains("{{nope}}"));
        assert!(Event::parse_list("start, END").is_ok());
        assert!(Event::parse_list("later").unwrap_err().contains("later"));
    }

    #[test]
    fn the_end_is_laid_out_for_each_place() {
        let failed = Ending {
            failed: vec!["p99 > 50ms (p99 was 80ms)".to_string()],
            ..Ending::default()
        };
        let msg = run().end(Event::End, &report(), &failed);
        let hook = |format, mention: Option<&str>| Resolved {
            format,
            mention: mention.map(str::to_string),
            ..Webhook::to("https://x.io/h").resolve().unwrap()
        };
        let (slack, kind) = msg.render(&hook(Format::Slack, Some("<!here>")));
        assert_eq!(kind, "application/json");
        let slack: Value = serde_json::from_str(&slack).unwrap();
        assert!(slack["text"]
            .as_str()
            .unwrap()
            .starts_with("<!here> ❌ Failed: p99 > 50ms"));
        assert_eq!(slack["blocks"][0]["type"], "header");
        assert_eq!(slack["blocks"][2]["fields"][0]["text"], "*Requests*\n1,000");
        let discord: Value =
            serde_json::from_str(&msg.render(&hook(Format::Discord, None)).0).unwrap();
        assert_eq!(discord["embeds"][0]["color"], Tone::Bad.rgb());
        let teams: Value = serde_json::from_str(&msg.render(&hook(Format::Teams, None)).0).unwrap();
        assert_eq!(teams["attachments"][0]["content"]["type"], "AdaptiveCard");
        let json: Value = serde_json::from_str(&msg.render(&hook(Format::Json, None)).0).unwrap();
        assert_eq!(
            (json["event"].clone(), json["status"].clone()),
            (json!("end"), json!("failed"))
        );
        assert_eq!(json["report"]["summary"]["total_requests"], 1000);
        let (text, kind) = msg.render(&hook(Format::Text, None));
        assert!(
            kind.starts_with("text/plain") && text.contains("1% of requests failed"),
            "{text}"
        );
        // A template has the holes
        let custom = Resolved {
            body: Some(Template::parse(
                r#"{"s": "{{status}} {{p99}} {{requests}} {{test}}"}"#,
            )),
            ..hook(Format::Json, None)
        };
        let (body, kind) = msg.render(&custom);
        assert_eq!(
            (body.as_str(), kind),
            (r#"{"s": "failed 80.00ms 1000 health"}"#, "application/json")
        );
        // Passed, and a ramp's report
        let passed = run().end(Event::End, &report(), &Ending::default());
        assert!(
            passed.status.contains("Passed · degraded"),
            "{}",
            passed.status
        );
        let ramp = json!({"holds_concurrency": 40, "capacity": {"requests_per_second": 3000.0},
            "requests": 9000, "failed_requests": 0, "seconds": 60.0, "findings": [{"level": "good", "text": "held to 40"}]});
        let ramp = run().end(Event::End, &ramp, &Ending::default());
        assert_eq!(
            ramp.fields[1],
            ("Capacity".to_string(), "about 3000 req/s".to_string())
        );
    }

    #[test]
    fn progress_is_one_line_with_a_bar() {
        let p = Progress {
            elapsed: Duration::from_secs(12),
            done: Some(0.4),
            requests: 12_000,
            failed: 12,
            rps: 1000.0,
            p50: Duration::from_millis(10),
            p99: Duration::from_millis(45),
            ..Progress::default()
        };
        let msg = run().progress(&p);
        assert!(
            msg.status.starts_with("⏳ ▰▰▰▰▱▱▱▱▱▱ 40% · 12s in"),
            "{}",
            msg.status
        );
        assert!(msg.status.contains("0.1% errors"), "{}", msg.status);
    }

    #[tokio::test]
    async fn messages_go_out_in_order_and_failures_are_retried() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/hook", listener.local_addr().unwrap());
        let got = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let seen = got.clone();
        tokio::spawn(async move {
            let mut first = true;
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let mut raw = Vec::new();
                let mut buf = [0u8; 65536];
                loop {
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
                            break;
                        }
                    }
                }
                let text = String::from_utf8_lossy(&raw).to_string();
                // The first attempt fails, to be sent again
                let status = if first { "503 Busy" } else { "200 OK" };
                if !first {
                    seen.lock()
                        .unwrap()
                        .push(text.split("\r\n\r\n").nth(1).unwrap().to_string());
                }
                first = false;
                let _ = sock
                    .write_all(
                        format!(
                            "HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n"
                        )
                        .as_bytes(),
                    )
                    .await;
            }
        });
        let hook = Webhook {
            on: vec![Event::Start, Event::Anomaly, Event::End, Event::Fail],
            format: Format::Text,
            ..Webhook::to(&url)
        };
        begin(run(), &[hook]);
        anomaly("p99 jumped 4.5× to 45ms at 26s");
        end(
            &report(),
            &Ending {
                failed: vec!["errors > 0.5% (errors were 1.0%)".into()],
                ..Ending::default()
            },
        );
        flush().await;
        let got = got.lock().unwrap().clone();
        assert_eq!(got.len(), 4, "{got:?}");
        assert!(got[0].starts_with("🚀 Started"), "{}", got[0]);
        assert!(got[1].starts_with("⚠️ p99 jumped"), "{}", got[1]);
        assert!(got[2].starts_with("❌ Failed: errors > 0.5%"), "{}", got[2]);
        assert!(got[3].starts_with("❌ Failed"), "{}", got[3]);
        assert!(current().is_empty(), "the run is over");
    }
}
