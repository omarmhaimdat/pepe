//! The setup screen: every setting as a field. It edits the same `Cli` the
//! flags fill in, so anything set here can be given as a flag and back.

use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame, Terminal,
};

use super::view::{
    heading, key_hints, label, section, status_color, truncate, value, ACCENT, BAD, GOOD, LABEL,
};
use super::{body, format, mascot};
use crate::curl;
use crate::load::Plan;
use crate::response::ResponseStats;
use crate::Cli;

const METHODS: [&str; 7] = ["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];
/// Width of the label column
const LABELS: usize = 14;

/// How the setup screen was left
pub enum SetupOutcome {
    /// Run a load test with these settings
    Start(Box<Cli>),
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Url,
    Method,
    Header(usize),
    /// Typing here starts a new header
    AddHeader,
    Body,
    Concurrency,
    RunMode,
    /// The duration or the request count, depending on the run mode
    RunValue,
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

pub struct Setup {
    /// Settings with no field here pass through untouched
    base: Cli,
    url: String,
    method: String,
    headers: Vec<String>,
    body: String,
    /// The body field was typed in, so it replaces any raw bytes from curl
    body_edited: bool,
    concurrency: String,
    by_duration: bool,
    duration: String,
    requests: String,
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
    /// The last "try once" response
    tried: Option<ResponseStats>,
}

impl Setup {
    pub fn new(cli: &Cli) -> Self {
        let mut setup = Setup {
            base: cli.clone(),
            url: cli.url.clone(),
            method: cli.method.clone(),
            headers: cli.headers.clone(),
            body: cli
                .body()
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default(),
            body_edited: false,
            concurrency: cli.concurrency.to_string(),
            by_duration: cli.duration.is_some(),
            duration: cli.duration.clone().unwrap_or_else(|| "30s".into()),
            requests: cli.number.to_string(),
            timeout: cli.timeout.to_string(),
            follow_redirects: !cli.disable_redirects,
            keep_alive: !cli.disable_keepalive,
            verify_tls: !cli.insecure,
            proxy: cli.proxy.clone().unwrap_or_default(),
            user_agent: cli.user_agent.clone(),
            focus: 0,
            cursor: 0,
            message: None,
            tried: None,
        };
        setup.cursor = setup.url.chars().count();
        setup
    }

    fn fields(&self) -> Vec<Field> {
        let mut fields = vec![Field::Url, Field::Method];
        fields.extend((0..self.headers.len()).map(Field::Header));
        fields.extend([
            Field::AddHeader,
            Field::Body,
            Field::Concurrency,
            Field::RunMode,
            Field::RunValue,
            Field::Timeout,
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
            Field::Header(i) => self.headers.get_mut(i)?,
            Field::Body => &mut self.body,
            Field::Concurrency => &mut self.concurrency,
            Field::RunValue if self.by_duration => &mut self.duration,
            Field::RunValue => &mut self.requests,
            Field::Timeout => &mut self.timeout,
            Field::Proxy => &mut self.proxy,
            Field::UserAgent => &mut self.user_agent,
            _ => return None,
        })
    }

    /// Fields that only take digits
    fn numeric(&self, field: Field) -> bool {
        match field {
            Field::Concurrency | Field::Timeout => true,
            Field::RunValue => !self.by_duration,
            _ => false,
        }
    }

    fn move_focus(&mut self, step: isize) {
        // Leaving a header empty removes it
        if let Field::Header(i) = self.focused() {
            if self.headers[i].trim().is_empty() {
                self.headers.remove(i);
                if step > 0 {
                    self.focus = self.focus.saturating_sub(1);
                }
            }
        }
        let count = self.fields().len() as isize;
        self.focus = (self.focus as isize + step).rem_euclid(count) as usize;
        let field = self.focused();
        self.cursor = self.text(field).map_or(0, |t| t.chars().count());
    }

    fn insert(&mut self, text: &str) {
        let field = self.focused();
        let numeric = self.numeric(field);
        if field == Field::AddHeader {
            // The new header takes this row; "add" moves down one
            self.headers.push(String::new());
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
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let field = self.focused();
        let editable = self.text(field).is_some() || field == Field::AddHeader;
        match key.code {
            KeyCode::Char('c') if ctrl => return Some(Action::Quit),
            KeyCode::Esc => return Some(Action::Quit),
            KeyCode::Enter => return Some(Action::Start),
            KeyCode::Char('t') if ctrl => return Some(Action::Try),
            KeyCode::Up | KeyCode::BackTab => self.move_focus(-1),
            KeyCode::Down | KeyCode::Tab => self.move_focus(1),
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
        if looks_like_curl(text) {
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
        cli.headers = self
            .headers
            .iter()
            .map(|h| h.trim().to_string())
            .filter(|h| !h.is_empty())
            .collect();
        if self.body_edited {
            cli.body = (!self.body.is_empty()).then(|| self.body.clone());
            cli.body_bytes = None;
        }
        cli.concurrency = number("concurrency", &self.concurrency)?;
        if self.by_duration {
            cli.duration = Some(self.duration.trim().to_string());
        } else {
            cli.duration = None;
            cli.number = number("requests", &self.requests)?;
        }
        cli.timeout = number("timeout", &self.timeout)?;
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
        let mut load = crate::load::start(client, request, 1, Plan::Count(1), true);
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
                    if looks_like_curl(&self.url) {
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
        let [header, _, main, _, command, message, footer] = Layout::vertical([
            Constraint::Length(mascot::HEIGHT + 1),
            Constraint::Length(1),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        self.render_header(f, header);

        // The try-once response sits beside the form when there's room
        let beside = self.tried.is_some() && main.width >= 110;
        let [form, _, tried] = Layout::horizontal([
            Constraint::Min(0),
            Constraint::Length(if beside { 3 } else { 0 }),
            Constraint::Percentage(if beside { 45 } else { 0 }),
        ])
        .areas(main);
        self.render_form(f, form);
        if beside {
            self.render_tried(f, tried);
        }

        self.render_command(f, command);
        if let Some((text, error)) = &self.message {
            let color = if *error { BAD } else { GOOD };
            f.render_widget(
                Paragraph::new(Span::styled(
                    truncate(text, message.width as usize),
                    Style::new().fg(color),
                )),
                message,
            );
        } else if let (Some(stat), false) = (&self.tried, beside) {
            f.render_widget(Paragraph::new(tried_summary(stat)), message);
        }
        f.render_widget(
            Paragraph::new(Line::from(key_hints(&[
                ("↑↓", "move"),
                ("←→", "change"),
                ("enter", "start"),
                ("ctrl-t", "try once"),
                ("esc", "quit"),
            ]))),
            footer,
        );
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let show_mascot = area.width >= 80;
        let [pet, _, text] = Layout::horizontal([
            Constraint::Length(if show_mascot { mascot::WIDTH } else { 0 }),
            Constraint::Length(if show_mascot { 2 } else { 0 }),
            Constraint::Min(0),
        ])
        .areas(area);
        if show_mascot {
            let mut lines = mascot::lines(mascot::Mood::Waiting, 0);
            lines.push(Line::styled("set me up", Style::new().fg(ACCENT).italic()));
            f.render_widget(Paragraph::new(lines), pet);
        }
        let lines = vec![
            Line::from(vec![
                value("pepe", ACCENT),
                Span::raw("  "),
                Span::styled("new load test", Style::new().bold()),
            ]),
            Line::raw(""),
            Line::from(label(
                "Every field is also a flag: the command at the bottom reproduces this setup.",
            )),
            Line::from(label(
                "Paste a curl command anywhere to fill everything in from it.",
            )),
        ];
        f.render_widget(Paragraph::new(lines), text);
    }

    fn render_form(&self, f: &mut Frame, area: Rect) {
        let focused = self.focused();
        let width = (area.width as usize).saturating_sub(LABELS + 4);
        let mut lines: Vec<Line> = Vec::new();
        let mut focus_line = 0;
        let mut row =
            |lines: &mut Vec<Line>, field: Field, name: &str, shown: Vec<Span<'static>>| {
                let active = field == focused;
                if active {
                    focus_line = lines.len();
                }
                let mut spans = vec![
                    Span::styled(
                        if active { "▸ " } else { "  " },
                        Style::new().fg(ACCENT).bold(),
                    ),
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
                lines.push(Line::from(spans));
            };
        let text = |field: Field, text: &str, hint: &str| -> Vec<Span<'static>> {
            let active = field == focused;
            if text.is_empty() && !active {
                return vec![label(hint.to_string())];
            }
            if !active {
                return vec![Span::raw(truncate(text, width))];
            }
            if text.is_empty() {
                return vec![
                    Span::styled(" ", Style::new().add_modifier(Modifier::REVERSED)),
                    label(format!(" {hint}")),
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
                Span::raw(before),
                Span::styled(
                    under.to_string(),
                    Style::new().add_modifier(Modifier::REVERSED),
                ),
                Span::raw(after),
            ]
        };
        let choice = |field: Field, shown: &str| -> Vec<Span<'static>> {
            // The value stays in the same column as text fields; the arrows
            // after it show it can be changed
            if field == focused {
                vec![
                    Span::styled(shown.to_string(), Style::new().bold()),
                    Span::styled("  ‹ ›", Style::new().fg(ACCENT)),
                ]
            } else {
                vec![Span::raw(shown.to_string())]
            }
        };
        fn on_off<'a>(on: bool, yes: &'a str, no: &'a str) -> &'a str {
            if on {
                yes
            } else {
                no
            }
        }

        lines.push(heading("target"));
        row(
            &mut lines,
            Field::Url,
            "URL",
            text(Field::Url, &self.url, "https://… or a curl command"),
        );
        row(
            &mut lines,
            Field::Method,
            "Method",
            choice(Field::Method, &self.method),
        );
        for (i, header) in self.headers.iter().enumerate() {
            row(
                &mut lines,
                Field::Header(i),
                "Header",
                text(Field::Header(i), header, "Name: value"),
            );
        }
        row(
            &mut lines,
            Field::AddHeader,
            "Header",
            if focused == Field::AddHeader {
                vec![
                    Span::styled(" ", Style::new().add_modifier(Modifier::REVERSED)),
                    label(" type Name: value to add one"),
                ]
            } else {
                vec![label("+ add")]
            },
        );
        row(
            &mut lines,
            Field::Body,
            "Body",
            text(Field::Body, &self.body, "none"),
        );

        lines.push(Line::raw(""));
        lines.push(heading("load"));
        row(
            &mut lines,
            Field::Concurrency,
            "Concurrency",
            text(Field::Concurrency, &self.concurrency, ""),
        );
        row(
            &mut lines,
            Field::RunMode,
            "Run",
            choice(
                Field::RunMode,
                on_off(self.by_duration, "for a duration", "a number of requests"),
            ),
        );
        row(
            &mut lines,
            Field::RunValue,
            on_off(self.by_duration, "Duration", "Requests"),
            text(
                Field::RunValue,
                on_off(self.by_duration, &self.duration, &self.requests),
                on_off(self.by_duration, "10s, 3m, 2h", ""),
            ),
        );

        lines.push(Line::raw(""));
        lines.push(heading("options"));
        row(
            &mut lines,
            Field::Timeout,
            "Timeout (s)",
            text(Field::Timeout, &self.timeout, ""),
        );
        row(
            &mut lines,
            Field::Redirects,
            "Redirects",
            choice(
                Field::Redirects,
                on_off(self.follow_redirects, "follow", "don't follow"),
            ),
        );
        row(
            &mut lines,
            Field::KeepAlive,
            "Keep-alive",
            choice(Field::KeepAlive, on_off(self.keep_alive, "on", "off")),
        );
        row(
            &mut lines,
            Field::VerifyTls,
            "TLS",
            choice(
                Field::VerifyTls,
                on_off(
                    self.verify_tls,
                    "verify certificates",
                    "accept invalid certificates",
                ),
            ),
        );
        row(
            &mut lines,
            Field::Proxy,
            "Proxy",
            text(Field::Proxy, &self.proxy, "none"),
        );
        row(
            &mut lines,
            Field::UserAgent,
            "User agent",
            text(Field::UserAgent, &self.user_agent, ""),
        );

        // Scroll so the focused row stays on screen
        let height = area.height as usize;
        let scroll = (focus_line + 2).saturating_sub(height) as u16;
        f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), area);
    }

    /// The equivalent command, or why the settings aren't valid yet
    fn render_command(&self, f: &mut Frame, area: Rect) {
        let [title, text] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        section(f, title, "command", None);
        let line = match self.to_cli() {
            Ok(cli) => Span::raw(cli.command_line()),
            Err(e) => Span::styled(format!("not ready: {e}"), Style::new().fg(LABEL).italic()),
        };
        f.render_widget(
            Paragraph::new(Line::from(line)).wrap(Wrap { trim: false }),
            text,
        );
    }

    fn render_tried(&self, f: &mut Frame, area: Rect) {
        let Some(stat) = &self.tried else { return };
        let [title, text] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        section(f, title, "try once", Some(tried_summary(stat)));
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
        f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), text);
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
        label("try once: "),
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
    fn renders_at_many_sizes_with_every_field_focused() {
        let mut s = setup(&["pepe", "-H", "A: 1", "http://example.com/"]);
        s.tried = Some(ResponseStats {
            status_code: Some(reqwest::StatusCode::OK),
            ..Default::default()
        });
        for (w, h) in [(40, 12), (80, 24), (120, 40), (220, 60)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            for focus in 0..s.fields().len() {
                s.focus = focus;
                s.cursor = 0;
                terminal.draw(|f| s.render(f)).unwrap();
            }
        }
    }
}
