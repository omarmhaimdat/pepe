//! The setup screen: every setting as a field, for each mode. It edits the
//! same `Cli` the flags fill in, so anything set here can be given as a flag
//! and back.

use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Padding, Paragraph, Wrap},
    Frame, Terminal,
};

use super::kit::{caption, chips, marker, panel, FAINT, SELECTED};
use super::view::{label, status_color, truncate, value, ACCENT, BAD, GOOD, LABEL, RULE, WARN};
use super::{body, format, mascot};
use crate::cli::{ApiArgs, Command, RampArgs};
use crate::curl;
use crate::load::Plan;
use crate::ramp::RampPlan;
use crate::response::ResponseStats;
use crate::Cli;

const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
/// Width of the label column
const LABELS: usize = 14;
/// Cards side by side from this width; one column below it
const WIDE: u16 = 100;

/// How the setup screen was left
pub enum SetupOutcome {
    /// Run a load test with these settings
    Start(Box<Cli>),
    Quit,
}

/// What kind of load test is being set up
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// One request, a steady load
    Single,
    /// One request, the load raised step by step
    Ramp,
    /// Every endpoint of an OpenAPI spec
    Api,
}

impl Mode {
    const ALL: [Mode; 3] = [Mode::Single, Mode::Ramp, Mode::Api];

    fn name(self) -> &'static str {
        match self {
            Mode::Single => "Single URL",
            Mode::Ramp => "Ramp",
            Mode::Api => "API",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Mode::Single => "A steady load on one request: so many at a time, for a count or a duration.",
            Mode::Ramp => "Raise the load step by step, and find where the target stops keeping up.",
            Mode::Api => "Every endpoint of an OpenAPI spec. You pick them, and set their parameters, on the next screen.",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Url,
    Method,
    /// API mode: the OpenAPI spec, a file or a URL
    Spec,
    /// API mode: where requests go instead of the spec's server
    Server,
    Header(usize),
    /// Typing here starts a new header
    AddHeader,
    Body,
    Concurrency,
    /// Threads sending requests; empty for the default
    Threads,
    /// Requests started per second; empty for as many as -c allows
    Rate,
    /// A file the report is written to every minute; empty for none
    Snapshot,
    RunMode,
    /// The duration or the request count, depending on the run mode
    RunValue,
    /// Ramp mode: concurrency of the first step, the last, and between
    From,
    To,
    Step,
    /// Ramp mode: how long each step is held
    Every,
    /// Ramp mode: a stop condition
    Until(usize),
    /// Typing here starts a new stop condition
    AddUntil,
    Timeout,
    Redirects,
    KeepAlive,
    VerifyTls,
    Proxy,
    UserAgent,
}

enum Action {
    Start,
    Try,
    Quit,
}

/// A card of the form: its rows, and which one has the caret
struct Section {
    title: &'static str,
    lines: Vec<Line<'static>>,
    focus: Option<usize>,
}

pub struct Setup {
    /// Settings with no field here pass through untouched
    base: Cli,
    mode: Mode,
    url: String,
    method: String,
    headers: Vec<String>,
    body: String,
    /// The body field was typed in, so it replaces any raw bytes from curl
    body_edited: bool,
    concurrency: String,
    threads: String,
    rate: String,
    snapshot: String,
    by_duration: bool,
    duration: String,
    requests: String,
    from: String,
    to: String,
    step: String,
    every: String,
    until: Vec<String>,
    spec: String,
    server: String,
    /// API flags with no field here pass through untouched
    api: ApiArgs,
    timeout: String,
    follow_redirects: bool,
    keep_alive: bool,
    verify_tls: bool,
    proxy: String,
    user_agent: String,

    /// Index into `fields()`
    focus: usize,
    /// Caret position in the focused text field, in characters
    cursor: usize,
    /// Shown under the form: (text, is an error)
    message: Option<(String, bool)>,
    /// A newer release the last update check found, to mention
    fresher: Option<String>,
    /// The last "try once" response
    tried: Option<ResponseStats>,
}

impl Setup {
    pub fn new(cli: &Cli) -> Self {
        let ramp = match &cli.command {
            Some(Command::Ramp(ramp)) => ramp.clone(),
            _ => RampArgs::default(),
        };
        let api = match &cli.command {
            Some(Command::Api(api)) => api.clone(),
            _ => ApiArgs::default(),
        };
        let mode = match &cli.command {
            Some(Command::Ramp(_)) => Mode::Ramp,
            Some(Command::Api(_)) => Mode::Api,
            _ => Mode::Single,
        };
        let mut setup = Setup {
            base: cli.clone(),
            mode,
            url: if cli.url.is_empty() {
                ramp.url.clone()
            } else {
                cli.url.clone()
            },
            method: cli.method.clone(),
            headers: cli.headers.clone(),
            body: cli
                .body()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default(),
            body_edited: false,
            concurrency: cli.concurrency.to_string(),
            threads: cli.threads.map(|t| t.to_string()).unwrap_or_default(),
            rate: cli.rate.map(|r| r.to_string()).unwrap_or_default(),
            snapshot: cli
                .snapshot
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default(),
            by_duration: cli.duration.is_some(),
            duration: cli.duration.clone().unwrap_or_else(|| "30s".into()),
            requests: cli.number.to_string(),
            from: ramp.from.to_string(),
            to: ramp.to.to_string(),
            step: ramp.step.to_string(),
            every: ramp.every.clone(),
            until: ramp.until.clone(),
            spec: api.spec.clone(),
            server: api.server.clone().unwrap_or_default(),
            api,
            timeout: cli.timeout.to_string(),
            follow_redirects: !cli.disable_redirects,
            keep_alive: !cli.disable_keepalive,
            verify_tls: !cli.insecure,
            proxy: cli.proxy.clone().unwrap_or_default(),
            user_agent: cli.user_agent.clone(),
            focus: 0,
            cursor: 0,
            message: None,
            fresher: crate::update::known().map(|v| v.to_string()),
            tried: None,
        };
        let first = setup.focused();
        setup.cursor = setup.text(first).map_or(0, |t| t.chars().count());
        setup
    }

    /// Open with this said under the form, e.g. why the last start failed
    pub fn with_error(mut self, error: Option<String>) -> Self {
        // Said in terms of the form's fields, not of the flags
        self.message = error.map(|text| (text.replace("pass --server", "set Server to"), true));
        self
    }

    fn fields(&self) -> Vec<Field> {
        let mut fields = match self.mode {
            Mode::Api => vec![Field::Spec, Field::Server],
            _ => vec![Field::Url, Field::Method],
        };
        fields.extend((0..self.headers.len()).map(Field::Header));
        fields.push(Field::AddHeader);
        if self.mode != Mode::Api {
            fields.push(Field::Body);
        }
        if self.mode == Mode::Ramp {
            fields.extend([Field::From, Field::To, Field::Step, Field::Every]);
            fields.extend((0..self.until.len()).map(Field::Until));
            fields.push(Field::AddUntil);
        } else {
            fields.extend([Field::Concurrency, Field::RunMode, Field::RunValue]);
        }
        fields.extend([
            Field::Timeout,
            Field::Threads,
            Field::Rate,
            Field::Snapshot,
            Field::Redirects,
            Field::KeepAlive,
            Field::VerifyTls,
            Field::Proxy,
            Field::UserAgent,
        ]);
        fields
    }

    fn focused(&self) -> Field {
        let fields = self.fields();
        fields[self.focus.min(fields.len() - 1)]
    }

    /// The text behind a field, for those edited by typing
    fn text(&mut self, field: Field) -> Option<&mut String> {
        Some(match field {
            Field::Url => &mut self.url,
            Field::Spec => &mut self.spec,
            Field::Server => &mut self.server,
            Field::Header(i) => self.headers.get_mut(i)?,
            Field::Body => &mut self.body,
            Field::Concurrency => &mut self.concurrency,
            Field::Threads => &mut self.threads,
            Field::Rate => &mut self.rate,
            Field::Snapshot => &mut self.snapshot,
            Field::RunValue if self.by_duration => &mut self.duration,
            Field::RunValue => &mut self.requests,
            Field::From => &mut self.from,
            Field::To => &mut self.to,
            Field::Step => &mut self.step,
            Field::Every => &mut self.every,
            Field::Until(i) => self.until.get_mut(i)?,
            Field::Timeout => &mut self.timeout,
            Field::Proxy => &mut self.proxy,
            Field::UserAgent => &mut self.user_agent,
            _ => return None,
        })
    }

    /// Fields that only take digits
    fn numeric(&self, field: Field) -> bool {
        match field {
            Field::Concurrency
            | Field::Threads
            | Field::Rate
            | Field::Timeout
            | Field::From
            | Field::To
            | Field::Step => true,
            Field::RunValue => !self.by_duration,
            _ => false,
        }
    }

    fn move_focus(&mut self, step: isize) {
        // Leaving a header or a stop condition empty removes it
        let emptied = match self.focused() {
            Field::Header(i) if self.headers[i].trim().is_empty() => {
                self.headers.remove(i);
                true
            }
            Field::Until(i) if self.until[i].trim().is_empty() => {
                self.until.remove(i);
                true
            }
            _ => false,
        };
        if emptied && step > 0 {
            self.focus = self.focus.saturating_sub(1);
        }
        let count = self.fields().len() as isize;
        self.focus = (self.focus as isize + step).rem_euclid(count) as usize;
        let field = self.focused();
        self.cursor = self.text(field).map_or(0, |t| t.chars().count());
    }

    /// Tab: the next mode, keeping everything the modes share
    fn switch_mode(&mut self, step: isize) {
        let at = Mode::ALL.iter().position(|m| *m == self.mode).unwrap_or(0);
        let count = Mode::ALL.len() as isize;
        self.mode = Mode::ALL[(at as isize + step).rem_euclid(count) as usize];
        self.focus = 0;
        let field = self.focused();
        self.cursor = self.text(field).map_or(0, |t| t.chars().count());
        self.tried = None;
    }

    fn insert(&mut self, text: &str) {
        let field = self.focused();
        let numeric = self.numeric(field);
        // The new header or condition takes this row; "add" moves down one
        if field == Field::AddHeader {
            self.headers.push(String::new());
            self.cursor = 0;
        }
        if field == Field::AddUntil {
            self.until.push(String::new());
            self.cursor = 0;
        }
        let field = self.focused();
        let cursor = self.cursor;
        let Some(target) = self.text(field) else {
            return;
        };
        let clean: String = text
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .filter(|c| !numeric || c.is_ascii_digit())
            .collect();
        let at = target
            .char_indices()
            .nth(cursor)
            .map_or(target.len(), |(i, _)| i);
        target.insert_str(at, &clean);
        self.cursor += clean.chars().count();
        self.body_edited |= field == Field::Body;
    }

    /// Delete the character before the caret (`back`) or under it
    fn delete(&mut self, back: bool) {
        let field = self.focused();
        let cursor = self.cursor;
        let Some(target) = self.text(field) else {
            return;
        };
        let index = if back {
            cursor.checked_sub(1)
        } else {
            Some(cursor)
        };
        let Some((at, _)) = index.and_then(|i| target.char_indices().nth(i)) else {
            return;
        };
        target.remove(at);
        if back {
            self.cursor -= 1;
        }
        self.body_edited |= field == Field::Body;
    }

    /// Left/right (or space) on a choice or toggle
    fn change(&mut self, step: isize) {
        match self.focused() {
            Field::Method => {
                let current = METHODS
                    .iter()
                    .position(|m| m.eq_ignore_ascii_case(&self.method));
                let next = match current {
                    Some(i) => (i as isize + step).rem_euclid(METHODS.len() as isize) as usize,
                    None => 0,
                };
                self.method = METHODS[next].to_string();
            }
            Field::RunMode => self.by_duration = !self.by_duration,
            Field::Redirects => self.follow_redirects = !self.follow_redirects,
            Field::KeepAlive => self.keep_alive = !self.keep_alive,
            Field::VerifyTls => self.verify_tls = !self.verify_tls,
            _ => {}
        }
    }

    fn key(&mut self, key: KeyEvent) -> Option<Action> {
        // AltGr arrives as ctrl+alt on Windows, and types a character
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        let field = self.focused();
        let editable =
            self.text(field).is_some() || matches!(field, Field::AddHeader | Field::AddUntil);
        match key.code {
            KeyCode::Char('c') if ctrl => return Some(Action::Quit),
            KeyCode::Esc => return Some(Action::Quit),
            KeyCode::Enter => return Some(Action::Start),
            KeyCode::Char('t') if ctrl => return Some(Action::Try),
            KeyCode::Tab => self.switch_mode(1),
            KeyCode::BackTab => self.switch_mode(-1),
            KeyCode::Up => self.move_focus(-1),
            KeyCode::Down => self.move_focus(1),
            KeyCode::Left if editable => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right if editable => {
                let len = self.text(field).map_or(0, |t| t.chars().count());
                self.cursor = (self.cursor + 1).min(len);
            }
            KeyCode::Home if editable => self.cursor = 0,
            KeyCode::End if editable => {
                self.cursor = self.text(field).map_or(0, |t| t.chars().count())
            }
            KeyCode::Left => self.change(-1),
            KeyCode::Right | KeyCode::Char(' ') if !editable => self.change(1),
            KeyCode::Backspace => self.delete(true),
            KeyCode::Delete => self.delete(false),
            KeyCode::Char(c) if !ctrl && editable => self.insert(&c.to_string()),
            _ => return None,
        }
        self.message = None;
        None
    }

    /// Pasted text: a curl command fills the whole form, anything else goes
    /// into the focused field
    fn paste(&mut self, text: &str) {
        if looks_like_curl(text) && self.mode != Mode::Api {
            self.fill_from_curl(text);
        } else {
            self.insert(text.trim());
        }
    }

    fn fill_from_curl(&mut self, command: &str) {
        let request = match curl::parse_command(command) {
            Ok(request) => request,
            Err(e) => {
                self.message = Some((format!("curl command: {e}"), true));
                return;
            }
        };
        self.url = request.url;
        self.method = request.method;
        self.headers = request.headers;
        self.follow_redirects = request.follow_redirects;
        self.verify_tls = !request.insecure;
        self.keep_alive = !request.no_keepalive;
        if let Some(user_agent) = request.user_agent {
            self.user_agent = user_agent;
        }
        if let Some(proxy) = request.proxy {
            self.proxy = proxy;
        }
        if let Some(timeout) = request.timeout_secs {
            self.timeout = timeout.to_string();
        }
        // Keep the exact bytes (a file upload may not be text); the field
        // shows them as text until it's edited
        self.body = request
            .body
            .as_deref()
            .map(|b| String::from_utf8_lossy(b).into_owned())
            .unwrap_or_default();
        self.base.body = None;
        self.base.body_bytes = request.body;
        self.body_edited = false;
        self.focus = 0;
        self.cursor = self.url.chars().count();
        self.tried = None;
        let notes = match request.notes.len() {
            0 => String::new(),
            n => format!(" · {n} note(s): {}", request.notes.join("; ")),
        };
        self.message = Some((format!("filled in from the curl command{notes}"), false));
    }

    /// The ramp the fields describe, as flags would give it
    fn ramp_args(&self, url: &str) -> Result<RampArgs, String> {
        let number = |name: &str, text: &str| -> Result<u32, String> {
            text.trim()
                .parse::<u32>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{name} must be a number above zero"))
        };
        let args = RampArgs {
            url: url.to_string(),
            from: number("from", &self.from)?,
            to: number("to", &self.to)?,
            step: number("step", &self.step)?,
            every: self.every.trim().to_string(),
            until: self
                .until
                .iter()
                .map(|c| c.trim().to_string())
                .filter(|c| !c.is_empty())
                .collect(),
        };
        // The same reading the run does, so what's wrong shows up here
        RampPlan::from_args(&args).map_err(|e| e.replace("--", ""))?;
        Ok(args)
    }

    /// The settings as flags would have given them, checked the same way
    fn to_cli(&self) -> Result<Cli, String> {
        let number = |name: &str, text: &str| -> Result<u32, String> {
            text.trim()
                .parse::<u32>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{name} must be a number above zero"))
        };
        let mut cli = self.base.clone();
        cli.command = None;
        if self.mode == Mode::Api {
            let spec = self.spec.trim();
            if spec.is_empty() {
                return Err("enter the OpenAPI spec: a file or a URL".into());
            }
            let server = self.server.trim();
            cli.url = String::new();
            cli.command = Some(Command::Api(ApiArgs {
                spec: spec.to_string(),
                server: (!server.is_empty()).then(|| server.to_string()),
                ..self.api.clone()
            }));
        } else {
            let url = self.url.trim();
            if url.is_empty() {
                return Err("enter a URL, or paste a curl command".into());
            }
            cli.url = if url.contains("://") {
                url.to_string()
            } else {
                format!("http://{url}")
            };
            reqwest::Url::parse(&cli.url).map_err(|e| format!("URL: {e}"))?;
            cli.method = self.method.clone();
            if self.body_edited {
                cli.body = (!self.body.is_empty()).then(|| self.body.clone());
                cli.body_bytes = None;
            }
        }
        cli.headers = self
            .headers
            .iter()
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect();
        if self.mode == Mode::Ramp {
            cli.command = Some(Command::Ramp(self.ramp_args(&cli.url)?));
        } else {
            cli.concurrency = number("concurrency", &self.concurrency)?;
            if self.by_duration {
                cli.duration = Some(self.duration.trim().to_string());
            } else {
                cli.duration = None;
                cli.number = number("requests", &self.requests)?;
            }
        }
        cli.timeout = number("timeout", &self.timeout)?;
        cli.threads = match self.threads.trim() {
            "" => None,
            text => Some(number("threads", text)?.max(1)),
        };
        cli.rate = match self.rate.trim() {
            "" => None,
            text => Some(f64::from(number("rate", text)?.max(1))),
        };
        cli.snapshot = match self.snapshot.trim() {
            "" => None,
            path => Some(path.into()),
        };
        cli.disable_redirects = !self.follow_redirects;
        cli.disable_keepalive = !self.keep_alive;
        cli.insecure = !self.verify_tls;
        cli.proxy = (!self.proxy.trim().is_empty()).then(|| self.proxy.trim().to_string());
        cli.user_agent = self.user_agent.clone();
        cli.curl = false;
        cli.setup = false;
        cli.args = vec![String::new()];
        cli.validate().map_err(|e| {
            let text = e.to_string();
            let first = text.lines().next().unwrap_or_default();
            first.trim_start_matches("error: ").trim().to_string()
        })?;
        Ok(cli)
    }

    /// Send the request once and keep the response to show
    async fn try_once(&mut self) {
        if self.mode == Mode::Api {
            self.message = Some((
                "endpoints are tried on the next screen, once the spec is loaded".into(),
                false,
            ));
            return;
        }
        let cli = match self.to_cli() {
            Ok(cli) => cli,
            Err(e) => return self.message = Some((e, true)),
        };
        let built = cli
            .request()
            .and_then(|request| Ok((request.build_client()?, request)));
        let (client, request) = match built {
            Ok(built) => built,
            Err(e) => return self.message = Some((e.to_string(), true)),
        };
        let mut load = crate::load::start(vec![client], request, 1, Plan::Count(1), true);
        let wait = Duration::from_secs(cli.timeout as u64 + 2);
        match tokio::time::timeout(wait, load.rx.recv()).await {
            Ok(Some(stat)) => {
                self.tried = Some(stat);
                self.message = None;
            }
            _ => self.message = Some(("no response to the test request".into(), true)),
        }
    }

    pub async fn run(mut self) -> Result<SetupOutcome, Box<dyn std::error::Error>> {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;
        let mut events = EventStream::new();
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        loop {
            terminal.draw(|f| self.render(f))?;
            let event = tokio::select! {
                _ = &mut ctrl_c => return Ok(SetupOutcome::Quit),
                event = events.next() => event,
            };
            let action = match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => self.key(key),
                Some(Ok(Event::Paste(text))) => {
                    self.paste(&text);
                    None
                }
                Some(Ok(_)) => None,
                Some(Err(e)) => return Err(e.into()),
                None => return Ok(SetupOutcome::Quit),
            };
            match action {
                Some(Action::Quit) => return Ok(SetupOutcome::Quit),
                Some(Action::Start) => {
                    // A curl command typed into the URL field
                    if self.mode != Mode::Api && looks_like_curl(&self.url) {
                        let command = self.url.clone();
                        self.fill_from_curl(&command);
                        continue;
                    }
                    match self.to_cli() {
                        Ok(cli) => return Ok(SetupOutcome::Start(Box::new(cli))),
                        Err(e) => self.message = Some((e, true)),
                    }
                }
                Some(Action::Try) => {
                    self.message = Some(("sending one request…".into(), false));
                    terminal.draw(|f| self.render(f))?;
                    self.try_once().await;
                }
                None => {}
            }
        }
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    fn render(&self, f: &mut Frame) {
        let area = f.area();
        let tall = area.height >= 40 && area.width >= 110;
        let [header, main, command, status, footer] = Layout::vertical([
            Constraint::Length(if tall { mascot::HEIGHT + 1 } else { 3 }),
            Constraint::Min(3),
            Constraint::Length(4),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        self.render_header(f, header, tall);
        // With cards side by side, the try-once response has one too
        let beside = main.width >= WIDE;
        self.render_form(f, main);
        self.render_command(f, command);

        let line = if let Some((text, error)) = &self.message {
            Line::from(vec![
                if *error {
                    value(" ! ", WARN)
                } else {
                    value(" ✔ ", GOOD)
                },
                Span::styled(
                    truncate(text, (status.width as usize).saturating_sub(4)),
                    Style::new().fg(if *error { WARN } else { GOOD }),
                ),
            ])
        } else if let (Some(stat), false) = (&self.tried, beside) {
            tried_summary(stat)
        } else if let Some(fresher) = &self.fresher {
            Line::from(vec![
                value(" ✦ ", GOOD),
                label(format!("pepe {fresher} is out · pepe self-update")),
            ])
        } else if self.mode == Mode::Api {
            Line::from(label(" enter loads the spec and shows its endpoints"))
        } else {
            Line::from(label(
                " paste a curl command anywhere to fill everything in from it",
            ))
        };
        f.render_widget(Paragraph::new(line), status);
        f.render_widget(
            Paragraph::new(chips(&[
                ("↑↓", "move"),
                ("←→", "change"),
                ("tab", "mode"),
                (
                    "enter",
                    if self.mode == Mode::Api {
                        "load"
                    } else {
                        "start"
                    },
                ),
                ("ctrl-t", "try once"),
                ("esc", "quit"),
            ])),
            footer,
        );
    }

    fn render_header(&self, f: &mut Frame, area: Rect, tall: bool) {
        let area = if tall {
            let [pet, _, main] = Layout::horizontal([
                Constraint::Length(mascot::WIDTH),
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .areas(area);
            let mut lines = mascot::lines(mascot::Mood::Waiting, 0);
            lines.push(Line::styled("set me up", Style::new().fg(ACCENT).italic()));
            f.render_widget(Paragraph::new(lines), pet);
            // Beside the mascot, in the middle of its height
            Rect {
                y: main.y + 2,
                height: 4,
                ..main
            }
        } else {
            area
        };
        let mut tabs = Vec::new();
        for mode in Mode::ALL {
            tabs.push(if mode == self.mode {
                Span::styled(
                    format!(" {} ", mode.name()),
                    Style::new().bg(ACCENT).fg(Color::Black).bold(),
                )
            } else {
                Span::styled(format!(" {} ", mode.name()), Style::new().fg(LABEL))
            });
            tabs.push(Span::raw(" "));
        }
        tabs.push(Span::styled("  tab switches", Style::new().fg(FAINT)));
        let lines = vec![
            Line::from(vec![
                Span::styled(" pepe ", Style::new().bg(ACCENT).fg(Color::Black).bold()),
                Span::styled(" setup ", Style::new().bg(SELECTED).fg(Color::White)),
                Span::raw("  "),
                Span::styled("new load test", Style::new().bold()),
            ]),
            Line::from(tabs),
            Line::from(label(truncate(self.mode.about(), area.width as usize))),
        ];
        // A line of air under the brand when there's the height for it
        let lines = if tall {
            let mut spaced = lines;
            spaced.insert(1, Line::raw(""));
            spaced
        } else {
            lines
        };
        f.render_widget(Paragraph::new(lines), area);
    }

    /// One row of the form: the bar, the label, then the value
    fn row(
        &self,
        section: &mut Section,
        field: Field,
        name: &str,
        shown: Vec<Span<'static>>,
        width: usize,
    ) {
        let active = field == self.focused();
        if active {
            section.focus = Some(section.lines.len());
        }
        let mut spans = vec![
            marker(active, true),
            Span::raw(" "),
            Span::styled(
                format!("{name:<LABELS$}"),
                if active {
                    Style::new().fg(ACCENT).bold()
                } else {
                    Style::new().fg(LABEL)
                },
            ),
        ];
        spans.extend(shown);
        let used: usize = spans.iter().map(Span::width).sum();
        spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
        let line = Line::from(spans);
        section.lines.push(if active {
            line.style(Style::new().bg(SELECTED))
        } else {
            line
        });
    }

    /// A field edited by typing: its text, with the caret when it's focused
    fn typed(&self, field: Field, text: &str, hint: &str, width: usize) -> Vec<Span<'static>> {
        let active = field == self.focused();
        let hint_style = Style::new().fg(FAINT).italic();
        if text.is_empty() && !active {
            return vec![Span::styled(hint.to_string(), hint_style)];
        }
        if !active {
            return vec![Span::raw(truncate(text, width))];
        }
        if text.is_empty() {
            return vec![
                Span::styled(" ", Style::new().add_modifier(Modifier::REVERSED)),
                Span::styled(format!(" {hint}"), hint_style),
            ];
        }
        // Show a window around the caret, with the caret cell reversed
        let chars: Vec<char> = text.chars().collect();
        let start = (self.cursor + 1).saturating_sub(width.max(1));
        let end = (start + width.max(1)).min(chars.len());
        let before: String = chars[start..self.cursor.min(end)].iter().collect();
        let under = chars.get(self.cursor).copied().unwrap_or(' ');
        let after: String = chars
            .get(self.cursor + 1..end)
            .unwrap_or_default()
            .iter()
            .collect();
        vec![
            Span::styled(before, Style::new().bold()),
            Span::styled(
                under.to_string(),
                Style::new().add_modifier(Modifier::REVERSED),
            ),
            Span::styled(after, Style::new().bold()),
        ]
    }

    /// A field changed with ←→: its value, with arrows when it's focused
    fn choice(&self, field: Field, shown: &str) -> Vec<Span<'static>> {
        if field == self.focused() {
            vec![
                Span::styled("‹ ", Style::new().fg(ACCENT)),
                Span::styled(shown.to_string(), Style::new().bold()),
                Span::styled(" ›", Style::new().fg(ACCENT)),
            ]
        } else {
            vec![Span::raw(shown.to_string())]
        }
    }

    /// The cards of the form: target, load and options. The target's rows
    /// are `target_width` wide, the others' `width`.
    fn sections(&self, target_width: usize, width: usize) -> [Section; 3] {
        fn on_off<'a>(on: bool, yes: &'a str, no: &'a str) -> &'a str {
            if on {
                yes
            } else {
                no
            }
        }
        let room = width.saturating_sub(LABELS + 3);
        let target_room = target_width.saturating_sub(LABELS + 3);
        let focused = self.focused();

        let mut target = Section {
            title: "target",
            lines: Vec::new(),
            focus: None,
        };
        if self.mode == Mode::Api {
            self.row(
                &mut target,
                Field::Spec,
                "Spec",
                self.typed(
                    Field::Spec,
                    &self.spec,
                    "openapi.yaml, or https://…/openapi.json",
                    target_room,
                ),
                target_width,
            );
            self.row(
                &mut target,
                Field::Server,
                "Server",
                self.typed(Field::Server, &self.server, "the spec's own", target_room),
                target_width,
            );
        } else {
            self.row(
                &mut target,
                Field::Url,
                "URL",
                self.typed(
                    Field::Url,
                    &self.url,
                    "https://… or a curl command",
                    target_room,
                ),
                target_width,
            );
            self.row(
                &mut target,
                Field::Method,
                "Method",
                self.choice(Field::Method, &self.method),
                target_width,
            );
        }
        for (i, header) in self.headers.iter().enumerate() {
            self.row(
                &mut target,
                Field::Header(i),
                "Header",
                self.typed(Field::Header(i), header, "Name: value", target_room),
                target_width,
            );
        }
        self.row(
            &mut target,
            Field::AddHeader,
            "Header",
            if focused == Field::AddHeader {
                vec![
                    Span::styled(" ", Style::new().add_modifier(Modifier::REVERSED)),
                    Span::styled(
                        " type Name: value to add one",
                        Style::new().fg(FAINT).italic(),
                    ),
                ]
            } else {
                vec![Span::styled("+ add", Style::new().fg(FAINT))]
            },
            target_width,
        );
        if self.mode != Mode::Api {
            self.row(
                &mut target,
                Field::Body,
                "Body",
                self.typed(Field::Body, &self.body, "none", target_room),
                target_width,
            );
        }

        let mut load = Section {
            title: if self.mode == Mode::Ramp {
                "ramp"
            } else {
                "load"
            },
            lines: Vec::new(),
            focus: None,
        };
        if self.mode == Mode::Ramp {
            for (field, name, text, hint) in [
                (
                    Field::From,
                    "From",
                    &self.from,
                    "concurrency of the first step",
                ),
                (Field::To, "To", &self.to, "concurrency of the last step"),
                (Field::Step, "Step", &self.step, "added at each step"),
                (Field::Every, "Hold each", &self.every, "10s, 1m"),
            ] {
                self.row(
                    &mut load,
                    field,
                    name,
                    self.typed(field, text, hint, room),
                    width,
                );
            }
            for (i, condition) in self.until.iter().enumerate() {
                self.row(
                    &mut load,
                    Field::Until(i),
                    "Stop when",
                    self.typed(Field::Until(i), condition, "p99 > 500ms", room),
                    width,
                );
            }
            self.row(
                &mut load,
                Field::AddUntil,
                "Stop when",
                if focused == Field::AddUntil {
                    vec![
                        Span::styled(" ", Style::new().add_modifier(Modifier::REVERSED)),
                        Span::styled(
                            " type p99 > 500ms, or errors > 1%",
                            Style::new().fg(FAINT).italic(),
                        ),
                    ]
                } else {
                    vec![Span::styled("+ add", Style::new().fg(FAINT))]
                },
                width,
            );
            load.lines.push(Line::raw(""));
            load.lines.push(self.staircase(width));
        } else {
            self.row(
                &mut load,
                Field::Concurrency,
                "Concurrency",
                self.typed(
                    Field::Concurrency,
                    &self.concurrency,
                    "requests at once",
                    room,
                ),
                width,
            );
            self.row(
                &mut load,
                Field::RunMode,
                "Run",
                self.choice(
                    Field::RunMode,
                    on_off(self.by_duration, "for a duration", "a number of requests"),
                ),
                width,
            );
            self.row(
                &mut load,
                Field::RunValue,
                on_off(self.by_duration, "Duration", "Requests"),
                self.typed(
                    Field::RunValue,
                    on_off(self.by_duration, &self.duration, &self.requests),
                    on_off(self.by_duration, "10s, 3m, 2h", "how many in all"),
                    room,
                ),
                width,
            );
        }

        let mut options = Section {
            title: "options",
            lines: Vec::new(),
            focus: None,
        };
        self.row(
            &mut options,
            Field::Timeout,
            "Timeout (s)",
            self.typed(Field::Timeout, &self.timeout, "seconds", room),
            width,
        );
        self.row(
            &mut options,
            Field::Threads,
            "Threads",
            self.typed(Field::Threads, &self.threads, "auto", room),
            width,
        );
        self.row(
            &mut options,
            Field::Rate,
            "Rate (req/s)",
            self.typed(Field::Rate, &self.rate, "as fast as -c allows", room),
            width,
        );
        self.row(
            &mut options,
            Field::Snapshot,
            "Snapshot",
            self.typed(
                Field::Snapshot,
                &self.snapshot,
                "a file for the report, every minute",
                room,
            ),
            width,
        );
        self.row(
            &mut options,
            Field::Redirects,
            "Redirects",
            self.choice(
                Field::Redirects,
                on_off(self.follow_redirects, "follow", "don't follow"),
            ),
            width,
        );
        self.row(
            &mut options,
            Field::KeepAlive,
            "Keep-alive",
            self.choice(Field::KeepAlive, on_off(self.keep_alive, "on", "off")),
            width,
        );
        self.row(
            &mut options,
            Field::VerifyTls,
            "TLS",
            self.choice(
                Field::VerifyTls,
                on_off(
                    self.verify_tls,
                    "verify certificates",
                    "accept invalid certificates",
                ),
            ),
            width,
        );
        self.row(
            &mut options,
            Field::Proxy,
            "Proxy",
            self.typed(Field::Proxy, &self.proxy, "none", room),
            width,
        );
        self.row(
            &mut options,
            Field::UserAgent,
            "User agent",
            self.typed(Field::UserAgent, &self.user_agent, "", room),
            width,
        );
        [target, load, options]
    }

    /// The ramp about to run, drawn small: how many steps, how long, and
    /// the climb. Or what's wrong with the fields.
    fn staircase(&self, width: usize) -> Line<'static> {
        const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
        let plan = self
            .ramp_args("")
            .and_then(|args| RampPlan::from_args(&args));
        match plan {
            Ok(plan) => {
                let text = format!(
                    "  {} step{} · {} in all   ",
                    plan.levels.len(),
                    if plan.levels.len() == 1 { "" } else { "s" },
                    format::span(plan.total())
                );
                // One block per step, thinned out when there are too many
                let room = width.saturating_sub(text.chars().count() + 1).clamp(1, 32);
                let count = plan.levels.len().min(room);
                let top = *plan.levels.last().unwrap_or(&1) as f64;
                let stairs: String = (0..count)
                    .map(|i| {
                        let level = plan.levels[i * plan.levels.len() / count] as f64;
                        BLOCKS[((level / top * 8.0).ceil() as usize).clamp(1, 8) - 1]
                    })
                    .collect();
                Line::from(vec![
                    label(text),
                    Span::styled(stairs, Style::new().fg(ACCENT)),
                ])
            }
            Err(e) => Line::from(Span::styled(
                truncate(&format!("  {e}"), width),
                Style::new().fg(WARN),
            )),
        }
    }

    fn render_form(&self, f: &mut Frame, area: Rect) {
        let card = |f: &mut Frame, section: &Section, area: Rect| {
            let lit = section.focus.is_some();
            let block = panel(
                caption(section.title, lit),
                None,
                if lit { ACCENT } else { RULE },
            );
            let inner = block.inner(area);
            // Scroll so the focused row stays inside the card
            let scroll = section
                .focus
                .map_or(0, |line| (line + 1).saturating_sub(inner.height as usize));
            f.render_widget(
                Paragraph::new(section.lines.clone())
                    .scroll((scroll as u16, 0))
                    .block(block),
                area,
            );
        };

        if area.width >= WIDE {
            let [left, right] =
                Layout::horizontal([Constraint::Percentage(52), Constraint::Min(0)]).areas(area);
            let [target, load, options] = self.sections(
                (left.width as usize).saturating_sub(2),
                (right.width as usize).saturating_sub(2),
            );
            let [load_area, options_area] = Layout::vertical([
                Constraint::Length(load.lines.len() as u16 + 2),
                Constraint::Min(3),
            ])
            .areas(right);
            // Under the target, what it answers
            let [target_area, tried_area] = Layout::vertical([
                Constraint::Length(target.lines.len() as u16 + 2),
                Constraint::Min(0),
            ])
            .areas(left);
            card(f, &target, target_area);
            if tried_area.height >= 4 {
                self.render_tried(f, tried_area);
            }
            card(f, &load, load_area);
            card(f, &options, options_area);
        } else {
            // One card with everything, scrolled to the focused row
            let width = (area.width as usize).saturating_sub(2);
            let mut all = Section {
                title: "setup",
                lines: Vec::new(),
                focus: None,
            };
            for section in self.sections(width, width) {
                if !all.lines.is_empty() {
                    all.lines.push(Line::raw(""));
                }
                all.lines.push(Line::from(Span::styled(
                    format!(" {}", section.title.to_uppercase()),
                    Style::new().fg(LABEL).bold(),
                )));
                if let Some(line) = section.focus {
                    all.focus = Some(all.lines.len() + line);
                }
                all.lines.extend(section.lines);
            }
            card(f, &all, area);
        }
    }

    /// The equivalent command, or why the settings aren't valid yet
    fn render_command(&self, f: &mut Frame, area: Rect) {
        let line = match self.to_cli() {
            Ok(cli) => Span::raw(cli.command_line()),
            Err(e) => Span::styled(format!("not ready: {e}"), Style::new().fg(LABEL).italic()),
        };
        let right = Line::from(Span::styled(" the same, as flags ", Style::new().fg(FAINT)));
        let block =
            panel(caption("command", false), Some(right), RULE).padding(Padding::horizontal(1));
        f.render_widget(
            Paragraph::new(Line::from(line))
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
    }

    fn render_tried(&self, f: &mut Frame, area: Rect) {
        let Some(stat) = &self.tried else {
            let hint = if self.mode == Mode::Api {
                "enter loads the spec; its endpoints are tried on the next screen"
            } else {
                "ctrl-t sends the request once, to check it before the run"
            };
            let block =
                panel(caption("response", false), None, RULE).padding(Padding::horizontal(1));
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    hint,
                    Style::new().fg(FAINT).italic(),
                )))
                .block(block),
                area,
            );
            return;
        };
        let mut summary = tried_summary(stat);
        summary.spans.insert(0, Span::raw(" "));
        summary.spans.push(Span::raw(" "));
        let block =
            panel(caption("response", false), Some(summary), RULE).padding(Padding::horizontal(1));
        let mut lines = Vec::new();
        match &stat.detail {
            Some(detail) => {
                for (name, v) in &detail.headers {
                    lines.push(Line::from(vec![
                        Span::styled(format!("{name}: "), Style::new().fg(ACCENT)),
                        Span::raw(String::from_utf8_lossy(v.as_bytes()).into_owned()),
                    ]));
                }
                lines.push(Line::raw(""));
                let text = String::from_utf8_lossy(&detail.body);
                if text.is_empty() {
                    lines.push(Line::from(label("(empty body)")));
                } else {
                    let format = body::Format::detect(detail, &text);
                    lines.extend(body::lines(&text, format, !detail.truncated, false));
                }
            }
            None => lines.push(Line::from(Span::styled(
                stat.error_message
                    .as_deref()
                    .unwrap_or("the request failed")
                    .to_string(),
                Style::new().fg(BAD),
            ))),
        }
        f.render_widget(
            Paragraph::new(lines)
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
    }
}

/// "200 OK · 12.3ms · 1.1 KiB"
fn tried_summary(stat: &ResponseStats) -> Line<'static> {
    let (status, color) = match (stat.status_code, stat.error) {
        (Some(code), _) => (
            format!(
                "{} {}",
                code.as_u16(),
                code.canonical_reason().unwrap_or("")
            ),
            status_color(code.as_u16()),
        ),
        (None, error) => (error.map_or("ERROR", |e| e.label()).to_string(), BAD),
    };
    let mut spans = vec![
        value(status.trim_end().to_string(), color),
        label(format!(
            " · {} · {}",
            format::latency(stat.duration),
            format::bytes(stat.body_bytes as f64)
        )),
    ];
    if let (None, Some(message)) = (stat.status_code, &stat.error_message) {
        spans.push(Span::styled(
            format!(" · {message}"),
            Style::new().fg(Color::Red),
        ));
    }
    Line::from(spans)
}

fn looks_like_curl(text: &str) -> bool {
    let text = text.trim_start().trim_start_matches("$ ");
    text.starts_with("curl ") || text.starts_with("curl.exe ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use ratatui::backend::TestBackend;

    fn setup(argv: &[&str]) -> Setup {
        Setup::new(&Cli::parse_from(argv))
    }

    fn press(s: &mut Setup, code: KeyCode) -> Option<Action> {
        s.key(KeyEvent::from(code))
    }

    fn type_text(s: &mut Setup, text: &str) {
        for c in text.chars() {
            press(s, KeyCode::Char(c));
        }
    }

    #[test]
    fn flags_prefill_the_form_and_come_back_unchanged() {
        let argv = [
            "pepe",
            "-c",
            "50",
            "-z",
            "30s",
            "-m",
            "POST",
            "-H",
            "Accept: */*",
            "-d",
            "{\"a\":1}",
            "-t",
            "5",
            "-k",
            "--disable-redirects",
            "https://x.io/items",
        ];
        let s = setup(&argv);
        assert_eq!(
            (s.url.as_str(), s.method.as_str()),
            ("https://x.io/items", "POST")
        );
        assert!(s.by_duration && !s.verify_tls && !s.follow_redirects);
        let cli = s.to_cli().unwrap();
        assert_eq!(cli.command_line(), Cli::parse_from(argv).command_line());
    }

    #[test]
    fn typing_moving_and_toggling() {
        let mut s = setup(&["pepe"]);
        type_text(&mut s, "x.io/a");
        press(&mut s, KeyCode::Left);
        press(&mut s, KeyCode::Backspace);
        type_text(&mut s, "!");
        assert_eq!(s.url, "x.io!a", "edits happen at the caret");
        press(&mut s, KeyCode::Backspace);
        type_text(&mut s, "/");

        press(&mut s, KeyCode::Down); // method
        press(&mut s, KeyCode::Right);
        assert_eq!(s.method, "POST");
        press(&mut s, KeyCode::Left);
        press(&mut s, KeyCode::Left);
        assert_eq!(s.method, "OPTIONS", "wraps around");

        press(&mut s, KeyCode::Down); // add header
        type_text(&mut s, "X-Id: 7");
        assert_eq!(s.headers, ["X-Id: 7"]);
        assert_eq!(s.focused(), Field::Header(0));

        press(&mut s, KeyCode::Down); // add header again
        press(&mut s, KeyCode::Down); // body
        press(&mut s, KeyCode::Down); // concurrency
        assert_eq!(s.focused(), Field::Concurrency);
        for _ in 0..4 {
            press(&mut s, KeyCode::Backspace);
        }
        type_text(&mut s, "8x9");
        assert_eq!(s.concurrency, "89", "numbers take digits only");

        let cli = s.to_cli().unwrap();
        assert_eq!(
            cli.url, "http://x.io/a",
            "a missing scheme defaults to http"
        );
        assert_eq!((cli.method.as_str(), cli.concurrency), ("OPTIONS", 89));
        assert_eq!(cli.headers, ["X-Id: 7"]);
    }

    #[test]
    fn an_emptied_header_is_removed() {
        let mut s = setup(&["pepe", "-H", "A: 1", "-H", "B: 2", "http://x.io"]);
        s.focus = 2; // first header
        s.cursor = 4;
        for _ in 0..4 {
            press(&mut s, KeyCode::Backspace);
        }
        press(&mut s, KeyCode::Down);
        assert_eq!(s.headers, ["B: 2"]);
        assert_eq!(
            s.focused(),
            Field::Header(0),
            "focus lands on what was next"
        );
    }

    #[test]
    fn pasting_a_curl_command_fills_the_form() {
        let mut s = setup(&["pepe", "-c", "7"]);
        s.paste("curl -X PUT 'https://api.x.io/u/1' \\\n -H 'Content-Type: application/json' -k -L \\\n --data-raw '{\"n\":1}'");
        assert_eq!(
            (s.url.as_str(), s.method.as_str()),
            ("https://api.x.io/u/1", "PUT")
        );
        assert_eq!(s.headers, ["Content-Type: application/json"]);
        assert!(!s.verify_tls && s.follow_redirects);
        assert_eq!(s.concurrency, "7", "load settings are kept");
        let cli = s.to_cli().unwrap();
        assert_eq!(cli.body().as_deref(), Some(&b"{\"n\":1}"[..]));
        assert!(matches!(&s.message, Some((m, false)) if m.contains("curl")));

        // Other pasted text goes into the focused field, on one line
        let mut s = setup(&["pepe"]);
        s.paste("  https://x.io/a\n");
        assert_eq!(s.url, "https://x.io/a");
        s.paste("curl --nope https://x.io");
        assert!(matches!(&s.message, Some((m, true)) if m.contains("--nope")));
    }

    #[test]
    fn invalid_settings_say_why() {
        let mut s = setup(&["pepe"]);
        assert!(s.to_cli().unwrap_err().contains("URL"));
        type_text(&mut s, "http://x.io");
        s.concurrency = "0".into();
        assert!(s.to_cli().unwrap_err().contains("concurrency"));
        s.concurrency = "500".into();
        s.requests = "10".into();
        assert!(s
            .to_cli()
            .unwrap_err()
            .contains("Concurrency cannot be greater"));
        s.by_duration = true;
        s.duration = "soon".into();
        assert!(s.to_cli().is_err());
        s.duration = "10s".into();
        assert!(s.to_cli().is_ok());
    }

    #[test]
    fn enter_starts_and_escape_quits() {
        let mut s = setup(&["pepe", "http://x.io"]);
        assert!(matches!(press(&mut s, KeyCode::Enter), Some(Action::Start)));
        assert!(matches!(press(&mut s, KeyCode::Esc), Some(Action::Quit)));
        let try_key = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL);
        assert!(matches!(s.key(try_key), Some(Action::Try)));
        assert_eq!(s.url, "http://x.io", "ctrl-t isn't typed into the field");
    }

    #[test]
    fn ramp_flags_prefill_the_form_and_come_back_unchanged() {
        let argv = [
            "pepe",
            "ramp",
            "--from",
            "5",
            "--to",
            "60",
            "--step",
            "5",
            "--every",
            "20s",
            "--until",
            "p99 > 500ms",
            "--until",
            "errors > 1%",
            "-m",
            "POST",
            "-H",
            "A: 1",
            "https://x.io/items",
        ];
        let mut cli = Cli::parse_from(argv);
        cli.url = "https://x.io/items".into();
        let s = Setup::new(&cli);
        assert_eq!(s.mode, Mode::Ramp);
        assert_eq!(
            (
                s.from.as_str(),
                s.to.as_str(),
                s.step.as_str(),
                s.every.as_str()
            ),
            ("5", "60", "5", "20s")
        );
        assert_eq!(s.until, ["p99 > 500ms", "errors > 1%"]);
        let back = s.to_cli().unwrap();
        assert_eq!(
            back.command_line(),
            "pepe ramp --from 5 --to 60 --step 5 --every 20s --until 'p99 > 500ms' \
             --until 'errors > 1%' -m POST -H 'A: 1' https://x.io/items"
        );
        assert_eq!(back.command_line(), cli.command_line());
    }

    #[test]
    fn tab_switches_mode_and_the_fields_follow() {
        let mut s = setup(&["pepe", "-c", "500", "http://x.io"]);
        assert_eq!(s.mode, Mode::Single);
        assert!(s.fields().contains(&Field::Concurrency));
        // 500 at once against the default 100 requests: not a valid steady run
        assert!(s.to_cli().is_err());

        press(&mut s, KeyCode::Tab);
        assert_eq!((s.mode, s.focused()), (Mode::Ramp, Field::Url));
        assert_eq!(s.url, "http://x.io", "the target is kept");
        let fields = s.fields();
        assert!(fields.contains(&Field::From) && !fields.contains(&Field::Concurrency));
        // A ramp sets its own concurrency, so the steady one isn't checked
        let cli = s.to_cli().unwrap();
        assert_eq!(cli.command_line(), "pepe ramp http://x.io");
        assert!(matches!(&cli.command, Some(Command::Ramp(ramp)) if ramp.to == 100));

        // A stop condition is typed like a header, and removed when emptied
        s.focus = s
            .fields()
            .iter()
            .position(|f| *f == Field::AddUntil)
            .unwrap();
        type_text(&mut s, "p99 > 1s");
        assert_eq!(s.until, ["p99 > 1s"]);
        assert_eq!(s.focused(), Field::Until(0));
        assert!(s
            .to_cli()
            .unwrap()
            .command_line()
            .contains("--until 'p99 > 1s'"));
        for _ in 0..8 {
            press(&mut s, KeyCode::Backspace);
        }
        press(&mut s, KeyCode::Up);
        assert!(s.until.is_empty());
        assert_eq!(s.focused(), Field::Every);

        // What's wrong with a ramp is said in its own words, not as flags
        s.to = "5".into();
        assert_eq!(s.to_cli().unwrap_err(), "to 5 is below from 10");
        s.to = "100".into();
        s.until = vec!["fast".into()];
        assert!(s.to_cli().unwrap_err().contains("stop condition"));
        s.until.clear();
        s.every = "soon".into();
        assert!(s.to_cli().unwrap_err().contains("every"));

        press(&mut s, KeyCode::Tab);
        assert_eq!((s.mode, s.focused()), (Mode::Api, Field::Spec));
        assert!(s.to_cli().unwrap_err().contains("spec"));
        type_text(&mut s, "openapi.yaml");
        press(&mut s, KeyCode::Down);
        type_text(&mut s, "http://localhost:3000");
        // The steady load is back, and checked again
        assert!(s.to_cli().unwrap_err().contains("Concurrency"));
        // A concurrency that is the default on no machine, since the card
        // leaves out flags at their default
        let concurrency = crate::utils::num_of_cores() + 1;
        s.concurrency = concurrency.to_string();
        let cli = s.to_cli().unwrap();
        assert_eq!(
            cli.command_line(),
            format!("pepe api --server http://localhost:3000 -c {concurrency} openapi.yaml")
        );
        press(&mut s, KeyCode::BackTab);
        press(&mut s, KeyCode::BackTab);
        assert_eq!(s.mode, Mode::Single);
    }

    #[test]
    fn the_staircase_previews_the_ramp() {
        let mut s = setup(&["pepe", "ramp", "http://x.io"]);
        let text = |s: &Setup| -> String {
            s.staircase(80)
                .spans
                .iter()
                .map(|x| x.content.to_string())
                .collect()
        };
        assert_eq!(text(&s), "  10 steps · 1m40s in all   ▁▂▃▄▄▅▆▇██");
        s.step = "0".into();
        assert!(text(&s).contains("step must be a number above zero"));
    }

    #[test]
    fn renders_at_many_sizes_with_every_field_focused() {
        let mut s = setup(&["pepe", "-H", "A: 1", "http://example.com/"]);
        s.tried = Some(ResponseStats {
            status_code: Some(reqwest::StatusCode::OK),
            ..Default::default()
        });
        s.until = vec!["p99 > 500ms".into()];
        for (w, h) in [(40, 12), (80, 24), (120, 40), (220, 60)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            for mode in Mode::ALL {
                s.mode = mode;
                for focus in 0..s.fields().len() {
                    s.focus = focus;
                    s.cursor = 0;
                    terminal.draw(|f| s.render(f)).unwrap();
                }
            }
            s.message = Some(("something to say".into(), true));
            terminal.draw(|f| s.render(f)).unwrap();
            s.message = None;
        }
    }
}
