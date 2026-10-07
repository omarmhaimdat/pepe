//! The plan screen of API mode: the spec's endpoints grouped by tag on the
//! left, what the selected one sends on the right. Nothing is on until it's
//! picked, and nothing is sent until the run starts, except "try once" and
//! the single request that checks new credentials.

use std::cell::Cell;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Clear, Padding, Paragraph},
    Frame, Terminal,
};

use super::kit::{
    about_line, caption, chips, chips_fit, help, marker, pane_title, panel, wrap, FAINT, FIELD,
    SELECTED,
};
use super::view::{label, status_color, truncate, value, ACCENT, BAD, GOOD, LABEL, RULE, WARN};
use super::{body, format, mascot, theme};
use crate::api::ApiRun;
use crate::load::Plan;
use crate::openapi::{split_values, AuthKind, Credentials, Endpoint, Field, In, MASK};
use crate::response::ResponseStats;
use crate::Cli;

/// Two panes side by side from this width; one at a time below it
/// A method is an identity, not a judgment: the same magenta the dashboard
/// draws it in, never the colors that say healthy or failing
const METHOD: Color = Color::Magenta;
const WIDE: u16 = 100;
/// Specs with more endpoints than this open with their tags folded
const FOLD_ABOVE: usize = 40;

/// How the plan screen was left
pub enum PlanOutcome {
    Start,
    Quit,
}

/// One line of text being edited
#[derive(Debug, Default, Clone, PartialEq)]
struct TextInput {
    chars: Vec<char>,
    cursor: usize,
}

impl TextInput {
    fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        TextInput {
            cursor: chars.len(),
            chars,
        }
    }

    fn text(&self) -> String {
        self.chars.iter().collect()
    }

    fn insert(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            self.chars.insert(self.cursor, c);
            self.cursor += 1;
        }
    }

    /// Edit or move; false when the key isn't one for the text
    fn key(&mut self, key: KeyEvent) -> bool {
        // AltGr arrives as ctrl+alt on Windows, and types a character
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('u') if ctrl => {
                self.chars.drain(..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Char('w') if ctrl => {
                let mut start = self.cursor;
                while start > 0 && self.chars[start - 1] == ' ' {
                    start -= 1;
                }
                while start > 0 && self.chars[start - 1] != ' ' {
                    start -= 1;
                }
                self.chars.drain(start..self.cursor);
                self.cursor = start;
            }
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.chars.len(),
            KeyCode::Char(c) if !ctrl => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            KeyCode::Backspace | KeyCode::Delete => {}
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.chars.len(),
            _ => return false,
        }
        true
    }

    /// The text on lines of `width`, the `max_lines` of them around the
    /// cursor
    fn wrapped(&self, width: usize, max_lines: usize) -> Vec<Vec<Span<'static>>> {
        let width = width.max(2);
        // The cursor can sit one past the end
        let count = self.chars.len() / width + 1;
        let first = (self.cursor / width + 1).saturating_sub(max_lines.max(1));
        (first..count.min(first + max_lines.max(1)))
            .map(|line| {
                let start = line * width;
                let end = self.chars.len().min(start + width);
                let text = |range: std::ops::Range<usize>| -> String {
                    self.chars[range].iter().collect()
                };
                if (start..start + width).contains(&self.cursor) {
                    let under = if self.cursor < end {
                        text(self.cursor..self.cursor + 1)
                    } else {
                        " ".to_string()
                    };
                    vec![
                        Span::raw(text(start..self.cursor)),
                        Span::styled(under, Style::new().add_modifier(Modifier::REVERSED)),
                        Span::raw(text((self.cursor + 1).min(end)..end)),
                    ]
                } else {
                    vec![Span::raw(text(start..end))]
                }
            })
            .collect()
    }

    /// The part that fits `width` with the cursor in view, as spans
    fn spans(&self, width: usize, hidden: bool) -> Vec<Span<'static>> {
        let width = width.max(2);
        let start = (self.cursor + 1).saturating_sub(width);
        let end = self.chars.len().min(start + width);
        let show = |range: std::ops::Range<usize>| -> String {
            if hidden {
                "•".repeat(range.len())
            } else {
                self.chars[range].iter().collect()
            }
        };
        let under = if self.cursor < end {
            show(self.cursor..self.cursor + 1)
        } else {
            " ".to_string()
        };
        vec![
            Span::raw(show(start..self.cursor)),
            Span::styled(under, Style::new().add_modifier(Modifier::REVERSED)),
            Span::raw(show((self.cursor + 1).min(end)..end)),
        ]
    }
}

/// What the prompt at the bottom of the screen is editing
#[derive(Debug, Clone, Copy, PartialEq)]
enum Editing {
    /// Narrows the list as it's typed
    Filter,
    /// A credential (shown masked), and which form to send it in
    Auth {
        form: usize,
    },
    /// A parameter of an endpoint
    Field {
        endpoint: usize,
        field: usize,
    },
    Body(usize),
    Weight(usize),
    Concurrency,
    Requests,
    Duration,
    Server,
}

#[derive(Debug, Clone, PartialEq)]
struct Prompt {
    what: Editing,
    input: TextInput,
}

#[derive(Debug, PartialEq)]
enum Action {
    Start,
    /// Send this endpoint's request once
    Try(usize),
    /// Check and apply the credential just entered
    Auth {
        secret: String,
        form: usize,
    },
    Quit,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Focus {
    /// The endpoints, by tag
    List,
    /// What the selected endpoint sends
    Detail,
}

/// A line of the list
#[derive(Debug, Clone, Copy, PartialEq)]
enum Row {
    Tag(usize),
    Endpoint(usize),
}

/// A line of the detail pane that can be changed
#[derive(Debug, Clone, Copy, PartialEq)]
enum DetailRow {
    Field(usize),
    Body,
    Weight,
}

/// One way of sending a credential
#[derive(Debug, Clone, PartialEq)]
struct AuthForm {
    /// Shown in the prompt: "Bearer token"
    name: &'static str,
    /// The `--auth` value for a secret, minus the secret
    prefix: String,
}

/// What's known about the credentials in use
#[derive(Debug, Clone, PartialEq)]
enum AuthState {
    /// The spec asks for none, or none were given
    None,
    /// Given, not checked against the API
    Provided,
    /// The API took them: what answered, and how they're sent
    Accepted(String),
    /// The API refused them
    Rejected(String),
}

pub struct PlanScreen<'a> {
    run: &'a mut ApiRun,
    cli: &'a mut Cli,
    /// Per tag: its endpoints are hidden
    folded: Vec<bool>,
    /// Shows only the endpoints that contain this
    filter: String,
    /// Selected line of the list
    cursor: usize,
    focus: Focus,
    /// Selected line of the detail pane
    detail_cursor: usize,
    prompt: Option<Prompt>,
    /// (text, is a warning)
    message: Option<(String, bool)>,
    /// The last "try once" result per endpoint
    tried: Vec<Option<ResponseStats>>,
    auth: AuthState,
    /// The prompt for missing credentials was dismissed; don't ask again
    auth_declined: bool,
    // Set while drawing, for scrolling and paging
    list_top: Cell<usize>,
    list_height: Cell<usize>,
    detail_top: Cell<usize>,
    /// The keys overlay is open
    show_help: bool,
}

impl<'a> PlanScreen<'a> {
    pub fn new(run: &'a mut ApiRun, cli: &'a mut Cli) -> Self {
        let tried = vec![None; run.endpoints.len()];
        let auth = if run.credentials.is_empty() {
            AuthState::None
        } else {
            AuthState::Provided
        };
        let folded = vec![run.endpoints.len() > FOLD_ABOVE; run.spec.tags.len()];
        let mut screen = PlanScreen {
            run,
            cli,
            folded,
            filter: String::new(),
            cursor: 0,
            focus: Focus::List,
            detail_cursor: 0,
            prompt: None,
            message: None,
            tried,
            auth,
            auth_declined: false,
            list_top: Cell::new(0),
            list_height: Cell::new(10),
            detail_top: Cell::new(0),
            show_help: false,
        };
        // The API needs credentials and has none: ask straight away
        if screen.needs_auth() {
            screen.ask_for_auth();
        }
        screen
    }

    // ─── The list ────────────────────────────────────────────────────────────

    fn shown(&self, endpoint: &Endpoint) -> bool {
        let filter = self.filter.to_lowercase();
        filter.is_empty()
            || [&endpoint.label, &endpoint.summary, &endpoint.tag]
                .iter()
                .any(|text| text.to_lowercase().contains(&filter))
    }

    /// The endpoints of a tag that the filter leaves in
    fn members(&self, tag: usize) -> Vec<usize> {
        let name = &self.run.spec.tags[tag].0;
        (0..self.run.endpoints.len())
            .filter(|&i| self.run.endpoints[i].tag == *name && self.shown(&self.run.endpoints[i]))
            .collect()
    }

    /// The lines of the list: each tag, then its endpoints unless folded
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for tag in 0..self.run.spec.tags.len() {
            let members = self.members(tag);
            if members.is_empty() {
                continue;
            }
            rows.push(Row::Tag(tag));
            // A filter looks inside folded tags too
            if !self.folded[tag] || !self.filter.is_empty() {
                rows.extend(members.into_iter().map(Row::Endpoint));
            }
        }
        rows
    }

    fn current(&self) -> Option<Row> {
        self.rows().get(self.cursor).copied()
    }

    /// The endpoint the cursor is on
    fn selected(&self) -> Option<usize> {
        match self.current() {
            Some(Row::Endpoint(index)) => Some(index),
            _ => None,
        }
    }

    fn move_to(&mut self, cursor: usize) {
        let last = self.rows().len().saturating_sub(1);
        let cursor = cursor.min(last);
        if cursor != self.cursor {
            self.cursor = cursor;
            self.detail_cursor = 0;
            self.detail_top.set(0);
        }
    }

    /// The lines of the detail pane that can be changed
    fn detail_rows(&self, index: usize) -> Vec<DetailRow> {
        let endpoint = &self.run.endpoints[index];
        let mut rows: Vec<DetailRow> = (0..endpoint.fields.len()).map(DetailRow::Field).collect();
        if endpoint.content_type.is_some() {
            rows.push(DetailRow::Body);
        }
        rows.push(DetailRow::Weight);
        rows
    }

    /// Switch these on, leaving out what shouldn't run unasked: endpoints
    /// that need a value, and writes (unless --include-writes)
    fn switch_on(&mut self, indexes: &[usize]) {
        let (mut on, mut need, mut writes) = (0, 0, 0);
        let include_writes = self.run.options.include_writes;
        for &index in indexes {
            let endpoint = &mut self.run.endpoints[index];
            if !endpoint.missing().is_empty() {
                need += 1;
            } else if endpoint.is_write() && !include_writes {
                writes += 1;
            } else {
                endpoint.enabled = true;
                on += 1;
            }
        }
        let mut parts = vec![format!("{on} on")];
        if need > 0 {
            parts.push(format!("{need} left off until their parameters are set"));
        }
        if writes > 0 {
            parts.push(format!(
                "{writes} writes left off (switch those on one by one)"
            ));
        }
        self.message = Some((parts.join(" · "), on == 0));
    }

    fn toggle(&mut self) {
        match self.current() {
            Some(Row::Tag(tag)) => {
                let members = self.members(tag);
                if members.iter().any(|&i| self.run.endpoints[i].enabled) {
                    for index in members {
                        self.run.endpoints[index].enabled = false;
                    }
                } else {
                    self.switch_on(&members);
                }
            }
            Some(Row::Endpoint(index)) => {
                let endpoint = &mut self.run.endpoints[index];
                endpoint.enabled = !endpoint.enabled;
                if !endpoint.enabled {
                    return;
                }
                let missing = endpoint.missing();
                if !missing.is_empty() {
                    self.message = Some((
                        format!(
                            "on, but {} has no real value: enter sets the parameters",
                            missing.join(", ")
                        ),
                        true,
                    ));
                } else if endpoint.is_write() {
                    self.message = Some((
                        format!(
                            "on: this sends {} requests that may change data",
                            endpoint.method
                        ),
                        true,
                    ));
                }
            }
            None => {}
        }
    }

    // ─── Credentials ─────────────────────────────────────────────────────────

    /// The spec declares auth and no credentials are set
    fn needs_auth(&self) -> bool {
        !self.run.spec.auth.is_empty() && self.run.credentials.is_empty()
    }

    fn ask_for_auth(&mut self) {
        self.open(Editing::Auth { form: 0 }, "");
    }

    /// The ways a credential can be sent, the spec's own first. APIs that
    /// declare bearer auth often want the bare key instead, so that's
    /// offered (and tried) too.
    fn auth_forms(&self) -> Vec<AuthForm> {
        let bearer = AuthForm {
            name: "Bearer token",
            prefix: "bearer:".into(),
        };
        let raw = AuthForm {
            name: "plain Authorization header",
            prefix: "header:Authorization=".into(),
        };
        match self.run.spec.auth.first().map(|s| &s.kind) {
            Some(AuthKind::Basic) => vec![AuthForm {
                name: "user:password",
                prefix: "basic:".into(),
            }],
            Some(AuthKind::ApiKey { .. }) => vec![AuthForm {
                name: "API key",
                prefix: "apikey:".into(),
            }],
            _ => vec![bearer, raw],
        }
    }

    /// Use `secret` in the given form
    fn set_auth(&mut self, secret: &str, form: &AuthForm) -> Result<(), String> {
        self.run.credentials =
            Credentials::parse(&[format!("{}{secret}", form.prefix)], &self.run.spec)?;
        self.tried = vec![None; self.run.endpoints.len()];
        Ok(())
    }

    /// A read with all its values, to check credentials against: one
    /// that's on if there is one, then the one with the fewest parameters
    fn probe(&self) -> Option<usize> {
        let endpoints = &self.run.endpoints;
        (0..endpoints.len())
            .filter(|&i| !endpoints[i].is_write() && endpoints[i].missing().is_empty())
            .min_by_key(|&i| (!endpoints[i].enabled, endpoints[i].fields.len()))
    }

    /// Apply a credential and check it with one request. If the API refuses
    /// the chosen form, the other forms are tried and the one it accepts is
    /// kept.
    async fn apply_auth(&mut self, secret: String, chosen: usize) {
        let forms = self.auth_forms();
        let chosen = chosen.min(forms.len() - 1);
        let mut order = vec![chosen];
        order.extend((0..forms.len()).filter(|&i| i != chosen));

        let mut refused = None;
        for index in order {
            let form = &forms[index];
            if let Err(e) = self.set_auth(&secret, form) {
                self.message = Some((e, true));
                return;
            }
            let Some(probe) = self.probe() else {
                self.auth = AuthState::Provided;
                self.message = Some((
                    "credentials set (no endpoint has all its values yet, to check them against)"
                        .into(),
                    false,
                ));
                return;
            };
            let label = self.run.endpoints[probe].label.clone();
            let Some(stat) = self.send_once(probe).await else {
                self.auth = AuthState::Provided;
                self.message = Some((format!("credentials set; no answer from {label}"), true));
                return;
            };
            let code = stat.status_code.map(|c| c.as_u16());
            self.tried[probe] = Some(stat);
            if !matches!(code, Some(401 | 403)) {
                let answer = code.map_or("no response".to_string(), |c| c.to_string());
                let how = if index == chosen {
                    format!("sent as {}", form.name)
                } else {
                    format!("sent as {} (refused as {})", form.name, forms[chosen].name)
                };
                self.auth = AuthState::Accepted(format!("{answer} on {label}, {how}"));
                self.message = Some(("credentials accepted".into(), false));
                return;
            }
            refused.get_or_insert(format!("{} on {label}", code.unwrap_or(401)));
        }
        // Nothing worked: leave the form the user chose in place
        let _ = self.set_auth(&secret, &forms[chosen]);
        self.auth = AuthState::Rejected(refused.unwrap_or_default());
        self.message = Some((
            "the API refused these credentials: press a to enter them again".into(),
            true,
        ));
    }

    // ─── Keys ────────────────────────────────────────────────────────────────

    fn open(&mut self, what: Editing, text: &str) {
        self.prompt = Some(Prompt {
            what,
            input: TextInput::new(text),
        });
    }

    fn key(&mut self, key: KeyEvent) -> Option<Action> {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(Action::Quit);
        }
        if self.prompt.is_some() {
            return self.prompt_key(key);
        }
        if self.show_help && matches!(key.code, KeyCode::Char('?') | KeyCode::Esc | KeyCode::F(1)) {
            self.show_help = false;
            return None;
        }
        self.message = None;
        match key.code {
            KeyCode::Char('?') | KeyCode::F(1) => self.show_help = true,
            KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Char('g') => return self.start(),
            KeyCode::Char('t') => match self.selected() {
                Some(index) => return Some(Action::Try(index)),
                None => self.message = Some(("move to an endpoint to try it".into(), true)),
            },
            KeyCode::Char('a') => self.ask_for_auth(),
            KeyCode::Char('c') => {
                self.open(Editing::Concurrency, &self.cli.concurrency.to_string())
            }
            KeyCode::Char('n') => self.open(Editing::Requests, &self.cli.number.to_string()),
            KeyCode::Char('z') => self.open(
                Editing::Duration,
                &self.cli.duration.clone().unwrap_or_default(),
            ),
            KeyCode::Char('u') => self.open(Editing::Server, &self.run.spec.base_url.clone()),
            KeyCode::Char('/') => {
                self.focus = Focus::List;
                self.open(Editing::Filter, &self.filter.clone());
            }
            _ => match self.focus {
                Focus::List => return self.list_key(key),
                Focus::Detail => self.detail_key(key),
            },
        }
        None
    }

    fn start(&mut self) -> Option<Action> {
        if self.run.enabled().is_empty() {
            self.message = Some(("nothing to start".into(), true));
            return None;
        }
        // Starting without credentials the API asks for: check first
        if self.needs_auth() && !self.auth_declined {
            self.ask_for_auth();
            return None;
        }
        Some(Action::Start)
    }

    fn list_key(&mut self, key: KeyEvent) -> Option<Action> {
        let page = self.list_height.get().max(2) - 1;
        match key.code {
            KeyCode::Esc if !self.filter.is_empty() => {
                self.filter.clear();
                self.move_to(0);
            }
            KeyCode::Esc => return Some(Action::Quit),
            KeyCode::Up | KeyCode::Char('k') => self.move_to(self.cursor.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.move_to(self.cursor + 1),
            KeyCode::PageUp => self.move_to(self.cursor.saturating_sub(page)),
            KeyCode::PageDown => self.move_to(self.cursor + page),
            KeyCode::Home => self.move_to(0),
            KeyCode::End => self.move_to(usize::MAX),
            KeyCode::Char(' ') => self.toggle(),
            // Everything shown on (what can run unasked), or everything off
            KeyCode::Char('x') => {
                let shown: Vec<usize> = (0..self.run.endpoints.len())
                    .filter(|&i| self.shown(&self.run.endpoints[i]))
                    .collect();
                if shown.iter().any(|&i| self.run.endpoints[i].enabled) {
                    for index in shown {
                        self.run.endpoints[index].enabled = false;
                    }
                } else {
                    self.switch_on(&shown);
                }
            }
            KeyCode::Left | KeyCode::Char('h') => match self.current() {
                Some(Row::Tag(tag)) => self.folded[tag] = true,
                // Back up to the endpoint's tag
                Some(Row::Endpoint(_)) => {
                    let rows = self.rows();
                    let tag = (0..self.cursor)
                        .rev()
                        .find(|&i| matches!(rows[i], Row::Tag(_)));
                    self.move_to(tag.unwrap_or(0));
                }
                None => {}
            },
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Enter | KeyCode::Tab => {
                match self.current() {
                    Some(Row::Tag(tag)) if key.code == KeyCode::Enter => {
                        self.folded[tag] = !self.folded[tag]
                    }
                    Some(Row::Tag(tag)) if self.folded[tag] => self.folded[tag] = false,
                    Some(Row::Tag(_)) => self.move_to(self.cursor + 1),
                    Some(Row::Endpoint(_)) => self.focus = Focus::Detail,
                    None => {}
                }
            }
            _ => {}
        }
        None
    }

    fn detail_key(&mut self, key: KeyEvent) {
        let Some(index) = self.selected() else {
            self.focus = Focus::List;
            return;
        };
        let rows = self.detail_rows(index);
        let row = rows[self.detail_cursor.min(rows.len() - 1)];
        match key.code {
            KeyCode::Esc | KeyCode::Left | KeyCode::Char('h') | KeyCode::Tab | KeyCode::BackTab => {
                self.focus = Focus::List
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.detail_cursor = self.detail_cursor.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.detail_cursor = (self.detail_cursor + 1).min(rows.len() - 1)
            }
            KeyCode::Home => self.detail_cursor = 0,
            KeyCode::End => self.detail_cursor = rows.len() - 1,
            KeyCode::Enter => self.edit(index, row),
            KeyCode::Char(' ') => match row {
                DetailRow::Field(field) => {
                    if !next_value(&mut self.run.endpoints[index].fields[field]) {
                        self.edit(index, row);
                    }
                    self.tried[index] = None;
                }
                _ => self.edit(index, row),
            },
            KeyCode::Backspace | KeyCode::Delete => {
                let endpoint = &mut self.run.endpoints[index];
                match row {
                    DetailRow::Field(field) => {
                        endpoint.fields[field].set(Vec::new());
                        if endpoint.fields[field].required {
                            self.message = Some((
                                format!(
                                    "{} is required: enter sets it",
                                    endpoint.fields[field].name
                                ),
                                true,
                            ));
                        }
                    }
                    DetailRow::Body => endpoint.body = None,
                    DetailRow::Weight => endpoint.weight = 1,
                }
                self.tried[index] = None;
            }
            _ => {}
        }
    }

    /// Open the prompt for a line of the detail pane
    fn edit(&mut self, index: usize, row: DetailRow) {
        let endpoint = &self.run.endpoints[index];
        match row {
            DetailRow::Field(field) => {
                let current = &endpoint.fields[field];
                // A guess isn't worth keeping: start from nothing
                let text = if current.guessed {
                    String::new()
                } else {
                    current.values.join(", ")
                };
                self.open(
                    Editing::Field {
                        endpoint: index,
                        field,
                    },
                    &text,
                );
            }
            DetailRow::Body => {
                let text = endpoint
                    .body
                    .as_deref()
                    .map(|b| String::from_utf8_lossy(b).into_owned())
                    .unwrap_or_default();
                self.open(Editing::Body(index), &text);
            }
            DetailRow::Weight => self.open(Editing::Weight(index), &endpoint.weight.to_string()),
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) -> Option<Action> {
        let forms = self.auth_forms().len();
        let prompt = self.prompt.as_mut()?;
        match (key.code, prompt.what) {
            (KeyCode::Esc, what) => {
                self.prompt = None;
                self.message = None;
                match what {
                    Editing::Filter => {
                        self.filter.clear();
                        self.move_to(0);
                    }
                    Editing::Auth { .. } if self.needs_auth() => {
                        self.auth_declined = true;
                        self.message = Some((
                            "no credentials: requests will most likely be refused (a adds them)"
                                .into(),
                            true,
                        ));
                    }
                    _ => {}
                }
            }
            (KeyCode::Enter, _) => return self.confirm(),
            (KeyCode::Tab | KeyCode::BackTab, Editing::Auth { form }) => {
                prompt.what = Editing::Auth {
                    form: (form + 1) % forms,
                }
            }
            // Through the values the spec offers
            (KeyCode::Tab, Editing::Field { endpoint, field }) => {
                let field = &self.run.endpoints[endpoint].fields[field];
                let offered = offered(field);
                if !offered.is_empty() {
                    let text = prompt.input.text();
                    let next = offered
                        .iter()
                        .position(|o| *o == text.trim())
                        .map_or(0, |i| (i + 1) % offered.len());
                    prompt.input = TextInput::new(&offered[next]);
                }
            }
            _ => {
                if prompt.input.key(key) && prompt.what == Editing::Filter {
                    self.filter = prompt.input.text();
                    self.cursor = 0;
                    self.list_top.set(0);
                }
            }
        }
        None
    }

    fn paste(&mut self, pasted: &str) {
        if let Some(prompt) = &mut self.prompt {
            prompt.input.insert(pasted.trim());
            if prompt.what == Editing::Filter {
                self.filter = prompt.input.text();
                self.cursor = 0;
            }
        }
    }

    /// Enter in the prompt: apply what was typed, or say why not and stay
    fn confirm(&mut self) -> Option<Action> {
        let prompt = self.prompt.take()?;
        let text = prompt.input.text().trim().to_string();
        self.message = None;
        let number = |name: &str| -> Result<u32, String> {
            text.replace([',', '_'], "")
                .parse::<u32>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or_else(|| format!("{name} must be a number above zero"))
        };
        let result: Result<(), String> = match prompt.what {
            Editing::Filter => Ok(()),
            Editing::Auth { form } => {
                if text.is_empty() {
                    self.prompt = Some(prompt);
                    return None;
                }
                return Some(Action::Auth { secret: text, form });
            }
            Editing::Field { endpoint, field } => {
                self.run.endpoints[endpoint].fields[field].set(split_values(&text));
                self.tried[endpoint] = None;
                Ok(())
            }
            Editing::Body(index) => {
                let endpoint = &mut self.run.endpoints[index];
                let json = endpoint
                    .content_type
                    .as_deref()
                    .is_some_and(|t| t.contains("json"));
                if json && !text.is_empty() {
                    if let Err(e) = serde_json::from_str::<serde_json::Value>(&text) {
                        self.message =
                            Some((format!("kept, though it isn't valid JSON: {e}"), true));
                    }
                }
                endpoint.body = (!text.is_empty()).then(|| text.clone().into_bytes());
                self.tried[index] = None;
                Ok(())
            }
            Editing::Weight(index) => text
                .parse::<u32>()
                .ok()
                .filter(|w| (1..=100).contains(w))
                .map(|w| self.run.endpoints[index].weight = w)
                .ok_or_else(|| "the share must be a number from 1 to 100".to_string()),
            Editing::Concurrency => {
                number("concurrency").and_then(|n| self.change_cli(|cli| cli.concurrency = n))
            }
            Editing::Requests => number("requests").and_then(|n| {
                self.change_cli(|cli| {
                    cli.number = n;
                    cli.duration = None;
                })
            }),
            Editing::Duration => {
                let duration = (!text.is_empty()).then(|| text.clone());
                self.change_cli(|cli| cli.duration = duration)
            }
            Editing::Server => match reqwest::Url::parse(&text) {
                Ok(url) if matches!(url.scheme(), "http" | "https") => {
                    self.run.spec.base_url = text.trim_end_matches('/').to_string();
                    self.tried = vec![None; self.run.endpoints.len()];
                    // Whatever was accepted was accepted by the other server
                    if matches!(self.auth, AuthState::Accepted(_) | AuthState::Rejected(_)) {
                        self.auth = AuthState::Provided;
                    }
                    Ok(())
                }
                Ok(_) => Err("the server must be an http:// or https:// URL".into()),
                Err(e) => Err(format!("server: {e}")),
            },
        };
        if let Err(e) = result {
            self.message = Some((e, true));
            self.prompt = Some(prompt);
        }
        None
    }

    /// Change the load settings, if they still make sense together
    fn change_cli(&mut self, change: impl FnOnce(&mut Cli)) -> Result<(), String> {
        let mut changed = self.cli.clone();
        change(&mut changed);
        changed.validate().map_err(|e| {
            let text = e.to_string();
            let first = text.lines().next().unwrap_or_default();
            first.trim_start_matches("error: ").trim().to_string()
        })?;
        *self.cli = changed;
        Ok(())
    }

    // ─── Sending ─────────────────────────────────────────────────────────────

    /// Send one endpoint's request once
    async fn send_once(&mut self, index: usize) -> Option<ResponseStats> {
        let built = self
            .run
            .client(self.cli)
            .and_then(|client| Ok((client, self.run.targets(self.cli, &[index])?)));
        let (client, mut targets) = match built {
            Ok(built) => built,
            Err(e) => {
                self.message = Some((e.to_string(), true));
                return None;
            }
        };
        targets.truncate(1);
        let mut load = crate::load::start_targets(client, targets, 1, Plan::Count(1), true);
        let wait = Duration::from_secs(self.cli.timeout as u64 + 2);
        tokio::time::timeout(wait, load.recv()).await.ok().flatten()
    }

    async fn try_once(&mut self, index: usize) {
        match self.send_once(index).await {
            Some(stat) => {
                self.tried[index] = Some(stat);
                self.message = None;
            }
            None if self.message.is_none() => {
                self.message = Some(("no response to the test request".into(), true))
            }
            None => {}
        }
    }

    pub async fn run(mut self) -> Result<PlanOutcome, Box<dyn std::error::Error>> {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;
        let mut events = EventStream::new();
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        loop {
            terminal.draw(|f| theme::draw(f, |f| self.render(f)))?;
            let event = tokio::select! {
                _ = &mut ctrl_c => return Ok(PlanOutcome::Quit),
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
                None => return Ok(PlanOutcome::Quit),
            };
            match action {
                Some(Action::Quit) => return Ok(PlanOutcome::Quit),
                Some(Action::Start) => return Ok(PlanOutcome::Start),
                Some(Action::Try(index)) => {
                    self.message = Some(("sending one request…".into(), false));
                    terminal.draw(|f| theme::draw(f, |f| self.render(f)))?;
                    self.try_once(index).await;
                }
                Some(Action::Auth { secret, form }) => {
                    self.message = Some(("checking the credentials…".into(), false));
                    terminal.draw(|f| theme::draw(f, |f| self.render(f)))?;
                    self.apply_auth(secret, form).await;
                }
                None => {}
            }
        }
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    fn render(&self, f: &mut Frame) {
        let area = f.area();
        // The mascot when there's height to spare, cards when there's width
        let tall = area.height >= 40 && area.width >= 110;
        let cards = area.height >= 24 && area.width >= 90;
        let [header, body, status, footer] = Layout::vertical([
            Constraint::Length(if tall { mascot::HEIGHT + 1 } else { 4 }),
            Constraint::Min(3),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        self.render_header(f, header, tall, cards);
        self.render_body(f, body);
        self.render_status(f, status);
        f.render_widget(
            Paragraph::new(chips_fit(&self.hints(), footer.width)),
            footer,
        );
        if self.show_help {
            help(
                f,
                area,
                &[
                    ("↑ ↓ / j k", "move through the endpoints"),
                    ("space", "turn the endpoint (or tag) on or off"),
                    ("enter / →", "its parameters; on a tag, fold it"),
                    ("x", "everything shown on, or all off"),
                    ("/", "filter by path, method or tag"),
                    ("t", "send the endpoint once and show the response"),
                    ("u a", "the server, the credentials"),
                    ("c n z", "concurrency, requests, duration"),
                    ("g", "start the run"),
                    ("?", "close this help"),
                    ("esc / q", "back, then quit"),
                ],
                &[
                    "Only the endpoints that are on (●) are sent: pick".into(),
                    "them, set their parameters, try one, then g starts.".into(),
                    String::new(),
                    about_line(),
                ],
            );
        }
        if let Some(prompt) = &self.prompt {
            if prompt.what != Editing::Filter {
                self.render_prompt(f, area, prompt);
            }
        }
    }

    fn hints(&self) -> Vec<(&'static str, &'static str)> {
        match (self.prompt.as_ref().map(|p| p.what), self.focus) {
            (Some(Editing::Filter), _) => vec![("enter", "keep"), ("esc", "clear")],
            (Some(Editing::Auth { .. }), _) => vec![
                ("enter", "check and use"),
                ("tab", "how it's sent"),
                ("esc", "skip"),
            ],
            (Some(Editing::Field { .. }), _) => vec![
                ("enter", "set"),
                ("tab", "the spec's values"),
                ("esc", "cancel"),
            ],
            (Some(_), _) => vec![("enter", "set"), ("esc", "cancel")],
            (None, Focus::List) => vec![
                ("↑↓", "move"),
                ("space", "on/off"),
                (
                    "enter",
                    if self.selected().is_some() {
                        "parameters"
                    } else {
                        "fold"
                    },
                ),
                ("x", "all/none"),
                ("/", "filter"),
                ("t", "try once"),
                ("g", "start"),
                ("?", "keys"),
                ("q", "quit"),
            ],
            (None, Focus::Detail) => vec![
                ("↑↓", "move"),
                ("enter", "edit"),
                ("space", "next value"),
                ("del", "leave out"),
                ("t", "try once"),
                ("g", "start"),
                ("?", "keys"),
                ("esc", "back"),
            ],
        }
    }

    fn load_text(&self) -> String {
        match self.cli.run_duration() {
            Some(d) => format!("for {}", format::span(d)),
            None => format!("{} requests", format::count(self.cli.number as u64)),
        }
    }

    /// How the mascot feels about the plan, and what it says
    fn mood(&self) -> (mascot::Mood, &'static str) {
        if matches!(self.auth, AuthState::Rejected(_)) {
            (mascot::Mood::Worried, "that key didn't work")
        } else if self.needs_auth() {
            (mascot::Mood::Waiting, "who goes there?")
        } else if self.run.enabled().is_empty() {
            (mascot::Mood::Waiting, "pick your targets")
        } else {
            (mascot::Mood::Happy, "ready when you are")
        }
    }

    /// Server, auth and load: title, the keys that change it, what it is
    /// now, and the colour of its card
    fn settings(&self) -> [(&'static str, &'static str, Line<'static>, Color); 3] {
        let spec = &self.run.spec;
        let wanted = spec
            .auth
            .first()
            .map_or("credentials".to_string(), |s| s.describe());
        let (auth, color) = match &self.auth {
            AuthState::None if spec.auth.is_empty() => (vec![label("none needed")], RULE),
            AuthState::None => (
                vec![value("✖ missing", BAD), label(format!("  {wanted}"))],
                BAD,
            ),
            AuthState::Provided => (
                vec![
                    value("● set", GOOD),
                    label(format!("  {wanted}, not checked yet")),
                ],
                RULE,
            ),
            AuthState::Accepted(how) => (
                vec![value("✔ accepted", GOOD), label(format!("  {how}"))],
                RULE,
            ),
            AuthState::Rejected(why) => (
                vec![value("✖ refused", BAD), label(format!("  {why}"))],
                BAD,
            ),
        };
        [
            (
                "server",
                "u",
                Line::from(Span::raw(spec.base_url.clone())),
                RULE,
            ),
            ("auth", "a", Line::from(auth), color),
            (
                "load",
                "c n z",
                Line::from(vec![
                    Span::raw(format!(
                        "{} concurrent · {}",
                        self.cli.concurrency,
                        self.load_text()
                    )),
                    label(format!(" · {}s timeout", self.cli.timeout)),
                ]),
                RULE,
            ),
        ]
    }

    fn render_header(&self, f: &mut Frame, area: Rect, tall: bool, cards: bool) {
        let area = if tall {
            let [pet, _, main] = Layout::horizontal([
                Constraint::Length(mascot::WIDTH),
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .areas(area);
            let (mood, says) = self.mood();
            let mut lines = mascot::lines(mood, 0);
            lines.push(Line::styled(says, Style::new().fg(ACCENT).italic()));
            f.render_widget(Paragraph::new(lines), pet);
            mascot::keep(pet);
            // Beside the mascot, in the middle of its height
            Rect {
                y: main.y + 1,
                height: 5,
                ..main
            }
        } else {
            area
        };
        let [brand, _, rest] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(u16::from(tall)),
            Constraint::Min(0),
        ])
        .areas(area);

        let spec = &self.run.spec;
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" pepe ", Style::new().bg(ACCENT).fg(Color::Black).bold()),
                Span::styled(" api ", Style::new().bg(SELECTED).fg(Color::White)),
                Span::raw("  "),
                Span::styled(spec.title.clone(), Style::new().bold()),
                label(format!("  {}", spec.version)),
            ])),
            brand,
        );
        let count = format!(
            "{} endpoints · {} tags ",
            self.run.endpoints.len(),
            spec.tags.len()
        );
        if brand.width as usize > count.len() + spec.title.len() + 30 {
            f.render_widget(
                Paragraph::new(Line::from(label(count))).alignment(Alignment::Right),
                brand,
            );
        }

        let settings = self.settings();
        if cards {
            let areas: [Rect; 3] = Layout::horizontal([Constraint::Ratio(1, 3); 3])
                .spacing(1)
                .areas(rest);
            for ((title, keys, line, color), area) in settings.into_iter().zip(areas) {
                let keys = Line::from(Span::styled(
                    format!(" {keys} "),
                    Style::new().fg(ACCENT).bold(),
                ));
                let block =
                    panel(caption(title, false), Some(keys), color).padding(Padding::horizontal(1));
                f.render_widget(Paragraph::new(line).block(block), area);
            }
        } else {
            let lines: Vec<Line> = settings
                .into_iter()
                .map(|(title, keys, line, _)| {
                    let mut spans = vec![label(format!("{title:<8}"))];
                    spans.extend(line.spans);
                    spans.push(Span::styled(
                        format!("  {keys}"),
                        Style::new().fg(ACCENT).bold(),
                    ));
                    Line::from(spans)
                })
                .collect();
            f.render_widget(Paragraph::new(lines), rest);
        }
    }

    /// The list, and beside it what the selected line is and sends. Panes
    /// are dropped, then stacked, as the width goes.
    fn render_body(&self, f: &mut Frame, area: Rect) {
        if area.width < WIDE {
            match (self.focus, self.selected()) {
                (Focus::Detail, Some(index)) => self.render_endpoint(f, area, index),
                _ => self.render_list(f, area),
            }
            return;
        }
        let left = (area.width * 30 / 100).clamp(46, 72);
        let [list, right] =
            Layout::horizontal([Constraint::Length(left), Constraint::Min(0)]).areas(area);
        self.render_list(f, list);
        match self.current() {
            Some(Row::Endpoint(index)) => self.render_endpoint(f, right, index),
            Some(Row::Tag(tag)) => self.render_tag(f, right, tag),
            None => f.render_widget(panel(caption("nothing selected", false), None, RULE), right),
        }
    }

    /// An endpoint's parameters, and the request they make
    fn render_endpoint(&self, f: &mut Frame, area: Rect, index: usize) {
        if area.width >= 110 {
            let [params, preview] =
                Layout::horizontal([Constraint::Percentage(56), Constraint::Min(0)]).areas(area);
            self.render_params(f, params, index);
            self.render_preview(f, preview, index);
        } else if area.height >= 26 {
            let [params, preview] = Layout::vertical([
                Constraint::Min(12),
                Constraint::Length((area.height * 2 / 5).max(10)),
            ])
            .areas(area);
            self.render_params(f, params, index);
            self.render_preview(f, preview, index);
        } else {
            self.render_params(f, area, index);
        }
    }

    fn render_list(&self, f: &mut Frame, area: Rect) {
        let focused = self.focus == Focus::List;
        let endpoints = &self.run.endpoints;
        let on = endpoints.iter().filter(|e| e.enabled).count();
        let right = Line::from(vec![
            Span::raw(" "),
            value(format!("{on}"), if on > 0 { GOOD } else { LABEL }),
            label(format!(" of {} on ", endpoints.len())),
        ]);
        let mut block = panel(
            caption("endpoints", focused),
            Some(right),
            if focused { ACCENT } else { RULE },
        );
        // The filter sits in the bottom border, where it's typed too
        let accent = Style::new().fg(ACCENT).bold();
        match &self.prompt {
            Some(Prompt {
                what: Editing::Filter,
                input,
            }) => {
                let mut spans = vec![Span::styled(" / ", accent)];
                spans.extend(input.spans(30, false));
                spans.push(Span::raw(" "));
                block = block.title_bottom(Line::from(spans));
            }
            _ if !self.filter.is_empty() => {
                block = block.title_bottom(Span::styled(format!(" / {} ", self.filter), accent));
            }
            _ => {}
        }
        let body = block.inner(area);
        f.render_widget(block, area);

        let rows = self.rows();
        if rows.is_empty() {
            f.render_widget(
                Paragraph::new(Line::from(label(format!(
                    " nothing contains \"{}\"",
                    self.filter
                )))),
                body,
            );
            return;
        }
        // Keep the selected row on screen
        let height = (body.height as usize).max(1);
        self.list_height.set(height);
        let cursor = self.cursor.min(rows.len() - 1);
        let mut top = self.list_top.get().min(cursor);
        if cursor >= top + height {
            top = cursor + 1 - height;
        }
        self.list_top.set(top);

        let shared = shared_prefix(endpoints);
        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .skip(top)
            .take(height)
            .map(|(i, row)| self.row_line(*row, body.width as usize, i == cursor, shared))
            .collect();
        f.render_widget(Paragraph::new(lines), body);
    }

    fn row_line(&self, row: Row, width: usize, active: bool, shared: usize) -> Line<'static> {
        let mut spans = vec![marker(active, self.focus == Focus::List)];
        let right = match row {
            Row::Tag(tag) => {
                let members = self.members(tag);
                let on = members
                    .iter()
                    .filter(|&&i| self.run.endpoints[i].enabled)
                    .count();
                let folded = self.folded[tag] && self.filter.is_empty();
                spans.push(label(if folded { "▸ " } else { "▾ " }));
                spans.push(match on {
                    0 => Span::styled("○ ", Style::new().fg(FAINT)),
                    n if n == members.len() => Span::styled("● ", Style::new().fg(GOOD)),
                    _ => Span::styled("◐ ", Style::new().fg(GOOD)),
                });
                spans.push(Span::styled(
                    truncate(&self.run.spec.tags[tag].0, width.saturating_sub(14)),
                    Style::new().bold(),
                ));
                spans.push(Span::raw(" "));
                Span::styled(
                    format!(" {on}/{} ", members.len()),
                    Style::new().fg(if on > 0 { GOOD } else { LABEL }),
                )
            }
            Row::Endpoint(index) => {
                let endpoint = &self.run.endpoints[index];
                let (status, color) = self.status(index);
                let room = width.saturating_sub(1 + 4 + 2 + 5 + status.chars().count() + 2);
                spans.push(Span::raw("    "));
                spans.push(if endpoint.enabled {
                    Span::styled("● ", Style::new().fg(GOOD))
                } else {
                    Span::styled("○ ", Style::new().fg(FAINT))
                });
                spans.push(Span::styled(
                    format!("{:<5}", short_method(&endpoint.method)),
                    Style::new().fg(METHOD),
                ));
                spans.extend(path_spans(&endpoint.path, shared, endpoint.enabled, room));
                Span::styled(format!("{status} "), Style::new().fg(color))
            }
        };
        let used: usize = spans.iter().map(Span::width).sum::<usize>() + right.width();
        let gap = width.saturating_sub(used);
        // A tag's name runs into a rule, like a section title
        spans.push(match row {
            Row::Tag(_) => Span::styled("─".repeat(gap), Style::new().fg(RULE)),
            Row::Endpoint(_) => Span::raw(" ".repeat(gap)),
        });
        spans.push(right);
        let line = Line::from(spans);
        if active {
            line.style(Style::new().bg(SELECTED))
        } else {
            line
        }
    }

    /// What the list says next to an endpoint
    fn status(&self, index: usize) -> (String, Color) {
        let endpoint = &self.run.endpoints[index];
        if let Some(stat) = &self.tried[index] {
            return tried_text(stat, false);
        }
        let missing = endpoint.missing();
        match missing.len() {
            0 if endpoint.is_write() => ("write".into(), LABEL),
            0 => match self.run.urls(index).len() {
                1 => (String::new(), LABEL),
                n => (format!("{n} URLs"), LABEL),
            },
            1 => (format!("needs {}", truncate(missing[0], 16)), WARN),
            n => (format!("needs {n} values"), WARN),
        }
    }

    fn render_tag(&self, f: &mut Frame, area: Rect, tag: usize) {
        let (name, description) = &self.run.spec.tags[tag];
        let members = self.members(tag);
        let endpoints = &self.run.endpoints;
        let count = |test: &dyn Fn(&Endpoint) -> bool| {
            members.iter().filter(|&&i| test(&endpoints[i])).count()
        };
        let on = count(&|e| e.enabled);
        let right = Line::from(vec![
            Span::raw(" "),
            value(format!("{on}"), if on > 0 { GOOD } else { LABEL }),
            label(format!(" of {} on ", members.len())),
        ]);
        let title = Line::from(vec![
            Span::styled(" TAG ", Style::new().fg(LABEL).bold()),
            Span::styled(format!("{name} "), Style::new().bold()),
        ]);
        let block = panel(title, Some(right), RULE).padding(Padding::new(2, 2, 1, 0));
        let body = block.inner(area);
        f.render_widget(block, area);

        let width = body.width as usize;
        let mut lines: Vec<Line> = Vec::new();
        for line in wrap(description, width, 3) {
            lines.push(Line::raw(line));
        }
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }
        // What's in it, and what each is for
        let shared = shared_prefix(endpoints);
        let paths = members
            .iter()
            .map(|&i| endpoints[i].path.chars().count())
            .max()
            .unwrap_or(0)
            .min(width / 2);
        for &index in &members {
            let endpoint = &endpoints[index];
            let mut spans = vec![
                if endpoint.enabled {
                    Span::styled("● ", Style::new().fg(GOOD))
                } else {
                    Span::styled("○ ", Style::new().fg(FAINT))
                },
                Span::styled(
                    format!("{:<5}", short_method(&endpoint.method)),
                    Style::new().fg(METHOD),
                ),
            ];
            let path = path_spans(&endpoint.path, shared, endpoint.enabled, paths);
            let used: usize = path.iter().map(Span::width).sum();
            spans.extend(path);
            spans.push(Span::raw(" ".repeat(paths.saturating_sub(used) + 3)));
            spans.push(label(truncate(
                &endpoint.summary,
                width.saturating_sub(7 + paths + 3),
            )));
            lines.push(Line::from(spans));
        }
        lines.push(Line::raw(""));
        let mut notes = Vec::new();
        let need = count(&|e| !e.missing().is_empty());
        if need > 0 {
            notes.push(format!(
                "{need} still need{} a value",
                if need == 1 { "s" } else { "" }
            ));
        }
        let writes = count(&|e| e.is_write());
        if writes > 0 {
            notes.push(format!(
                "{writes} write{}",
                if writes == 1 { "s" } else { "" }
            ));
        }
        if !notes.is_empty() {
            lines.push(Line::from(value(notes.join(" · "), WARN)));
        }
        for line in wrap(
            "space switches the whole tag on or off. Endpoints that still need a value, \
             and writes, stay off until they're switched on themselves.",
            width,
            3,
        ) {
            lines.push(Line::from(label(line)));
        }
        f.render_widget(Paragraph::new(lines), body);
    }

    /// The selected endpoint: what it is, and every line that can be changed
    fn render_params(&self, f: &mut Frame, area: Rect, index: usize) {
        let endpoint = &self.run.endpoints[index];
        let focused = self.focus == Focus::Detail;
        let title = Line::from(vec![
            Span::styled(
                format!(" {} ", endpoint.method),
                Style::new().fg(METHOD).bold(),
            ),
            Span::styled(
                format!(
                    "{} ",
                    truncate(&endpoint.path, (area.width as usize).saturating_sub(20))
                ),
                Style::new().bold(),
            ),
        ]);
        let state = if endpoint.enabled {
            Line::from(value(" ● on ", GOOD))
        } else {
            Line::from(Span::styled(" ○ off ", Style::new().fg(LABEL)))
        };
        let block = panel(title, Some(state), if focused { ACCENT } else { RULE });
        let inner = block.inner(area);
        f.render_widget(block, area);
        let width = inner.width as usize;

        // What it is
        let mut head: Vec<Line> = Vec::new();
        if !endpoint.summary.is_empty() {
            head.push(Line::styled(
                format!(" {}", truncate(&endpoint.summary, width.saturating_sub(2))),
                Style::new().bold(),
            ));
        }
        if inner.height >= 18 && endpoint.description != endpoint.summary {
            for line in wrap(&endpoint.description, width.saturating_sub(2), 2) {
                head.push(Line::from(vec![Span::raw(" "), label(line)]));
            }
        }
        if !head.is_empty() {
            head.push(Line::raw(""));
        }
        let rows = self.detail_rows(index);
        // The table is as tall as its lines, up to what's left; the note
        // about the selected line sits right under it
        let fit = inner.height.saturating_sub(head.len() as u16 + 4).max(1);
        let [head_area, columns, table, rule, about] = Layout::vertical([
            Constraint::Length(head.len() as u16),
            Constraint::Length(1),
            Constraint::Length(fit.min(rows.len() as u16)),
            Constraint::Length(1),
            Constraint::Length(2),
        ])
        .areas(inner);
        f.render_widget(Paragraph::new(head), head_area);

        // The lines that can be changed, scrolled to keep the cursor in view
        let cursor = self.detail_cursor.min(rows.len() - 1);
        let height = (table.height as usize).max(1);
        let mut top = self.detail_top.get().min(cursor);
        if cursor >= top + height {
            top = cursor + 1 - height;
        }
        self.detail_top.set(top);
        let name_width = endpoint
            .fields
            .iter()
            .map(|f| f.name.chars().count() + 2)
            .max()
            .unwrap_or(0)
            .clamp(11, 28);

        let faint = Style::new().fg(FAINT);
        let mut titles = vec![Span::styled(
            format!(
                "   {:<name_width$}{:<7}{:<11}VALUE",
                "PARAMETER", "IN", "TYPE"
            ),
            faint,
        )];
        if rows.len() > height {
            let shown = format!(
                "{}–{} of {} ",
                top + 1,
                (top + height).min(rows.len()),
                rows.len()
            );
            let used: usize = titles[0].width() + shown.chars().count();
            titles.push(Span::raw(" ".repeat(width.saturating_sub(used))));
            titles.push(Span::styled(shown, faint));
        }
        f.render_widget(Paragraph::new(Line::from(titles)), columns);

        let lines: Vec<Line> = rows
            .iter()
            .enumerate()
            .skip(top)
            .take(height)
            .map(|(i, row)| {
                let active = focused && i == cursor;
                let mut spans = vec![marker(active, true)];
                spans.extend(self.detail_spans(endpoint, *row, name_width, width));
                let used: usize = spans.iter().map(Span::width).sum();
                spans.push(Span::raw(" ".repeat(width.saturating_sub(used))));
                let line = Line::from(spans);
                if active {
                    line.style(Style::new().bg(SELECTED))
                } else {
                    line
                }
            })
            .collect();
        f.render_widget(Paragraph::new(lines), table);

        // About the selected line
        f.render_widget(
            Paragraph::new("─".repeat(width)).style(Style::new().fg(RULE)),
            rule,
        );
        let help: Vec<Line> = if focused {
            about_row(endpoint, rows[cursor], width.saturating_sub(2))
                .into_iter()
                .map(|line| Line::from(vec![Span::raw(" "), label(line)]))
                .collect()
        } else {
            vec![Line::from(label(
                " enter or → comes here, to set what this endpoint sends",
            ))]
        };
        f.render_widget(Paragraph::new(help), about);
    }

    /// One line of the parameters table, after its marker
    fn detail_spans(
        &self,
        endpoint: &Endpoint,
        row: DetailRow,
        name_width: usize,
        width: usize,
    ) -> Vec<Span<'static>> {
        let room = width.saturating_sub(3 + name_width + 7 + 11 + 1);
        let columns = |dot: Span<'static>,
                       name: Vec<Span<'static>>,
                       place: &str,
                       kind: &str|
         -> Vec<Span<'static>> {
            let used: usize = name.iter().map(Span::width).sum();
            let mut spans = vec![dot];
            spans.extend(name);
            spans.push(Span::raw(" ".repeat(name_width.saturating_sub(used))));
            spans.push(label(format!("{place:<7}")));
            spans.push(label(format!("{:<11}", truncate(kind, 10))));
            spans
        };
        let sent = Span::styled("● ", Style::new().fg(GOOD));
        let unsent = Span::styled("○ ", Style::new().fg(FAINT));
        match row {
            DetailRow::Field(index) => {
                let field = &endpoint.fields[index];
                let set = !field.values.is_empty() && !field.guessed;
                let mut name = vec![Span::styled(
                    truncate(&field.name, name_width - 2),
                    if set {
                        Style::new().bold()
                    } else {
                        Style::new()
                    },
                )];
                if field.required {
                    name.push(value("*", WARN));
                }
                let place = match field.location {
                    In::Path => "path",
                    In::Query => "query",
                    In::Header => "header",
                    In::Cookie => "cookie",
                };
                let dot = if field.is_missing() {
                    value("! ", WARN)
                } else if set {
                    sent
                } else {
                    unsent
                };
                let mut spans = columns(dot, name, place, &field.kind);
                if field.is_missing() {
                    spans.push(value("needs a value", WARN));
                } else if !set {
                    let offered = if field.options.is_empty() {
                        field
                            .suggestion
                            .as_ref()
                            .map_or(String::new(), |example| format!("e.g. {example}"))
                    } else {
                        field.options.join(" | ")
                    };
                    spans.push(Span::styled(
                        truncate(&offered, room),
                        Style::new().fg(FAINT).italic(),
                    ));
                } else {
                    let turn = if field.values.len() > 1 && !field.is_list() {
                        "  in turn"
                    } else {
                        ""
                    };
                    spans.push(Span::styled(
                        truncate(&field.values.join(", "), room.saturating_sub(turn.len())),
                        Style::new().bold(),
                    ));
                    spans.push(label(turn));
                }
                spans
            }
            DetailRow::Body => {
                let kind = endpoint.content_type.as_deref().unwrap_or_default();
                let kind = kind.rsplit(['/', '+']).next().unwrap_or(kind);
                let dot = if endpoint.body.is_some() {
                    sent
                } else {
                    unsent
                };
                let mut spans = columns(dot, vec![Span::raw("body")], "body", kind);
                spans.push(match &endpoint.body {
                    Some(body) => Span::styled(
                        truncate(&String::from_utf8_lossy(body), room),
                        Style::new().bold(),
                    ),
                    None => Span::styled("none sent", Style::new().fg(FAINT).italic()),
                });
                spans
            }
            DetailRow::Weight => {
                let mut spans = columns(Span::raw("  "), vec![Span::raw("share")], "", "traffic");
                spans.push(Span::styled(
                    format!("×{}", endpoint.weight),
                    Style::new().bold(),
                ));
                spans
            }
        }
    }

    /// The request as it goes out, and the answer the last time it was tried
    fn render_preview(&self, f: &mut Frame, area: Rect, index: usize) {
        let urls = self.run.urls(index).len();
        let right = (urls > 1).then(|| Line::from(label(format!(" {urls} URLs, in turn "))));
        let block = panel(caption("request", false), right, RULE).padding(Padding::horizontal(1));
        let inner = block.inner(area);
        f.render_widget(block, area);

        let mut request = self.request_lines(index);
        let stat = self.tried[index].as_ref();
        let mut answer: Vec<Line> = Vec::new();
        if let Some(body) = stat.and_then(|stat| stat.preview.as_ref().map(|b| (stat, b))) {
            let (stat, bytes) = body;
            let text = String::from_utf8_lossy(bytes);
            let kind = if text.trim_start().starts_with(['{', '[']) {
                body::Format::Json
            } else {
                body::Format::Text
            };
            answer = body::lines(&text, kind, bytes.len() as u64 >= stat.body_bytes, false);
        }
        // The answer gets a third of the height at least, once there is one
        let floor = if stat.is_some() {
            (inner.height / 3).max(2)
        } else {
            1
        };
        let most = inner.height.saturating_sub(1 + floor) as usize;
        if request.len() > most {
            request.truncate(most.saturating_sub(1));
            request.push(Line::from(label("…")));
        }
        let [request_area, title, answer_area] = Layout::vertical([
            Constraint::Length(request.len() as u16 + 1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(inner);
        f.render_widget(Paragraph::new(request), request_area);
        let summary = match stat {
            Some(stat) => {
                let (text, color) = tried_text(stat, true);
                Line::from(value(text, color))
            }
            None => Line::from(label("t sends it once")),
        };
        pane_title(f, title, "response", Some(summary), false);
        f.render_widget(Paragraph::new(answer), answer_area);
    }

    /// The first of the endpoint's requests, spelled out. Credentials are
    /// kept off the screen.
    fn request_lines(&self, index: usize) -> Vec<Line<'static>> {
        let endpoint = &self.run.endpoints[index];
        let credentials = &self.run.credentials;
        // Built with the credentials masked, so none can reach the screen
        let url = endpoint
            .urls(&self.run.spec.base_url, &credentials.masked())
            .into_iter()
            .next()
            .unwrap_or_default();
        let (address, query) = url.split_once('?').unwrap_or((&url, ""));
        let parsed = reqwest::Url::parse(address).ok();
        let host = parsed.as_ref().map(|u| {
            let host = u.host_str().unwrap_or_default();
            u.port().map_or(host.to_string(), |p| format!("{host}:{p}"))
        });
        let path = parsed
            .as_ref()
            .map_or(address.to_string(), |u| u.path().to_string());

        let mut lines = vec![Line::from(vec![
            Span::styled(
                format!("{} ", endpoint.method),
                Style::new().fg(METHOD).bold(),
            ),
            Span::styled(path, Style::new().bold()),
        ])];
        for (i, pair) in query.split('&').filter(|p| !p.is_empty()).enumerate() {
            let (name, shown) = pair.split_once('=').unwrap_or((pair, ""));
            let secret = shown == MASK;
            lines.push(Line::from(vec![
                label(if i == 0 { "  ? " } else { "  & " }),
                Span::styled(name.to_string(), Style::new().fg(ACCENT)),
                label(" = "),
                if secret {
                    label(HIDDEN)
                } else {
                    Span::raw(shown.to_string())
                },
            ]));
        }

        // Host, credentials, -H flags, then the endpoint's own
        let mut headers: Vec<(String, Span)> = Vec::new();
        if let Some(host) = host {
            headers.push(("Host".into(), Span::raw(host)));
        }
        for (name, _) in &credentials.headers {
            headers.push((name.clone(), label(HIDDEN)));
        }
        for header in &self.cli.headers {
            if let Some((name, shown)) = header.split_once(':') {
                headers.push((name.trim().to_string(), Span::raw(shown.trim().to_string())));
            }
        }
        for (name, shown) in endpoint.headers() {
            headers.push((name, Span::raw(shown)));
        }
        let names = headers
            .iter()
            .map(|(name, _)| name.chars().count())
            .max()
            .unwrap_or(0);
        lines.push(Line::raw(""));
        for (name, shown) in headers {
            lines.push(Line::from(vec![label(format!("{name:<names$}  ")), shown]));
        }

        if let Some(bytes) = &endpoint.body {
            let json = endpoint
                .content_type
                .as_deref()
                .is_some_and(|t| t.contains("json"));
            let kind = if json {
                body::Format::Json
            } else {
                body::Format::Text
            };
            lines.push(Line::raw(""));
            lines.extend(body::lines(
                &String::from_utf8_lossy(bytes),
                kind,
                true,
                false,
            ));
        }
        lines
    }

    /// A message when there's one, else what a run would be
    fn render_status(&self, f: &mut Frame, area: Rect) {
        let width = area.width as usize;
        let line = if let Some((text, warning)) = &self.message {
            Line::from(vec![
                if *warning {
                    value(" ! ", WARN)
                } else {
                    value(" ✔ ", GOOD)
                },
                Span::styled(
                    truncate(text, width.saturating_sub(4)),
                    Style::new().fg(if *warning { WARN } else { GOOD }),
                ),
            ])
        } else {
            let on = self.run.enabled();
            if on.is_empty() {
                Line::from(vec![
                    Span::styled(" nothing on ", Style::new().bg(SELECTED).fg(LABEL)),
                    label("  space switches on an endpoint, or a whole tag"),
                ])
            } else {
                let urls: usize = on.iter().map(|&i| self.run.urls(i).len()).sum();
                Line::from(vec![
                    Span::styled(
                        " ▶ g starts ",
                        Style::new().bg(GOOD).fg(Color::Black).bold(),
                    ),
                    Span::raw(format!(
                        "  {} endpoint{} · {urls} URL{} · {} concurrent · {}",
                        on.len(),
                        if on.len() == 1 { "" } else { "s" },
                        if urls == 1 { "" } else { "s" },
                        self.cli.concurrency,
                        self.load_text()
                    )),
                ])
            }
        };
        f.render_widget(Paragraph::new(line), area);
    }

    /// The prompt, as a dialog over the middle of the screen
    fn render_prompt(&self, f: &mut Frame, area: Rect, prompt: &Prompt) {
        let body = matches!(prompt.what, Editing::Body(_));
        let width = area
            .width
            .saturating_sub(4)
            .min(if body { 110 } else { 76 })
            .max(20);
        let inside = width.saturating_sub(4) as usize;
        let (title, hint) = self.prompt_text(prompt.what);

        // What's being asked for, above the field
        let mut lines: Vec<Line> = Vec::new();
        let mut kind = String::new();
        match prompt.what {
            Editing::Auth { .. } => {
                let api = Some(self.run.spec.title.as_str())
                    .filter(|t| !t.is_empty())
                    .unwrap_or("This API");
                let text = format!("{api} asks for credentials. They're hidden as you type.");
                lines.extend(wrap(&text, inside, 3).into_iter().map(Line::raw));
            }
            Editing::Field { endpoint, field } => {
                let field = &self.run.endpoints[endpoint].fields[field];
                kind = format!(
                    " {} · {}{} ",
                    match field.location {
                        In::Path => "path",
                        In::Query => "query",
                        In::Header => "header",
                        In::Cookie => "cookie",
                    },
                    field.kind,
                    if field.required { " · required" } else { "" }
                );
                lines.extend(
                    wrap(&field.description, inside, 3)
                        .into_iter()
                        .map(Line::raw),
                );
            }
            _ => {}
        }
        if !lines.is_empty() {
            lines.push(Line::raw(""));
        }

        // The field itself
        let hidden = matches!(prompt.what, Editing::Auth { .. });
        let field_style = Style::new().bg(FIELD);
        let rows = if body {
            prompt.input.wrapped(inside.saturating_sub(2), 8)
        } else {
            vec![prompt.input.spans(inside.saturating_sub(3), hidden)]
        };
        for (i, row) in rows.into_iter().enumerate() {
            let mut spans = vec![if i == 0 {
                value("› ", ACCENT)
            } else {
                Span::raw("  ")
            }];
            spans.extend(row);
            let used: usize = spans.iter().map(Span::width).sum();
            spans.push(Span::raw(" ".repeat(inside.saturating_sub(used))));
            lines.push(Line::from(spans).style(field_style));
        }
        if let Some((text, true)) = &self.message {
            lines.push(Line::from(value(truncate(text, inside), WARN)));
        }
        lines.push(Line::raw(""));

        // What can go in it
        match prompt.what {
            Editing::Auth { form } => {
                let forms = self.auth_forms();
                let mut spans = vec![label("send as  ")];
                for (i, option) in forms.iter().enumerate() {
                    spans.push(if i == form.min(forms.len() - 1) {
                        Span::styled(
                            format!(" {} ", option.name),
                            Style::new().bg(ACCENT).fg(Color::Black).bold(),
                        )
                    } else {
                        label(format!(" {} ", option.name))
                    });
                    spans.push(Span::raw(" "));
                }
                lines.push(Line::from(spans));
                if forms.len() > 1 {
                    lines.push(Line::from(label(
                        "if the API refuses that form, the other is tried",
                    )));
                }
            }
            Editing::Field { endpoint, field } => {
                let field = &self.run.endpoints[endpoint].fields[field];
                if !field.options.is_empty() {
                    let allowed = format!("allowed: {}", field.options.join(" · "));
                    lines.extend(
                        wrap(&allowed, inside, 4)
                            .into_iter()
                            .map(|line| Line::from(label(line))),
                    );
                } else if let Some(example) = &field.suggestion {
                    lines.push(Line::from(label(truncate(
                        &format!("the spec's example: {example}"),
                        inside,
                    ))));
                }
                lines.push(Line::from(label(hint)));
            }
            _ => lines.extend(
                wrap(&hint, inside, 3)
                    .into_iter()
                    .map(|line| Line::from(label(line))),
            ),
        }
        lines.push(Line::raw(""));
        lines.push(chips(&self.hints()));

        let height = (lines.len() as u16 + 2).min(area.height);
        let popup = Rect {
            x: area.x + (area.width - width.min(area.width)) / 2,
            y: area.y + (area.height - height) / 3,
            width: width.min(area.width),
            height,
        };
        let title = Line::from(Span::styled(
            format!(" {title} "),
            Style::new().fg(ACCENT).bold(),
        ));
        let right = (!kind.is_empty()).then(|| Line::from(label(kind)));
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new(lines)
                .block(panel(title, right, ACCENT).padding(Padding::horizontal(1))),
            popup,
        );
    }

    /// The prompt's title, and what to know while typing
    fn prompt_text(&self, what: Editing) -> (String, String) {
        match what {
            Editing::Filter => (
                "filter".into(),
                "shows the endpoints whose path, summary or tag contains this".into(),
            ),
            Editing::Auth { .. } => (
                self.run
                    .spec
                    .auth
                    .first()
                    .map_or("credentials".to_string(), |s| s.describe()),
                String::new(),
            ),
            Editing::Field { endpoint, field } => {
                let field = &self.run.endpoints[endpoint].fields[field];
                let several = if field.is_list() {
                    "a, b sends both in each request"
                } else {
                    "a, b sends them in turn, one per request"
                };
                let empty = if field.required {
                    ""
                } else {
                    " · empty leaves it out"
                };
                (field.name.clone(), format!("{several}{empty}"))
            }
            Editing::Body(index) => (
                "body".into(),
                format!(
                    "{} · empty sends none",
                    self.run.endpoints[index]
                        .content_type
                        .as_deref()
                        .unwrap_or_default()
                ),
            ),
            Editing::Weight(_) => (
                "share of the traffic".into(),
                "2 gets twice the requests of an endpoint with 1".into(),
            ),
            Editing::Concurrency => (
                "concurrency".into(),
                "Requests in flight at once (-c).".into(),
            ),
            Editing::Requests => (
                "requests".into(),
                "How many to send in all, across the endpoints that are on (-n).".into(),
            ),
            Editing::Duration => (
                "duration".into(),
                "Run for this long instead of a number of requests, e.g. 30s or 5m (-z). \
                 Empty goes back to a count."
                    .into(),
            ),
            Editing::Server => (
                "server".into(),
                "Where requests go, instead of the spec's server (--server).".into(),
            ),
        }
    }
}

/// In place of a credential
const HIDDEN: &str = "••••••••";

/// How many leading characters every endpoint's path has in common, up to
/// a `/`: "/api" of "/api/ads" and "/api/boards"
fn shared_prefix(endpoints: &[Endpoint]) -> usize {
    let Some(first) = endpoints.first() else {
        return 0;
    };
    if endpoints.len() < 2 {
        return 0;
    }
    let mut shared = 0;
    for (i, c) in first.path.char_indices().skip(1) {
        if c == '/'
            && endpoints
                .iter()
                .all(|e| e.path.starts_with(&first.path[..=i]))
        {
            shared = first.path[..i].chars().count();
        }
    }
    shared
}

/// A path with what every path shares dimmed, and its `{parameters}` lit
fn path_spans(path: &str, shared: usize, bright: bool, max: usize) -> Vec<Span<'static>> {
    let shown = truncate(path, max);
    let plain = if bright {
        Style::new().bold()
    } else {
        Style::new()
    };
    let mut spans = vec![Span::styled(
        shown.chars().take(shared).collect::<String>(),
        Style::new().fg(FAINT),
    )];
    let mut text = String::new();
    for c in shown.chars().skip(shared) {
        if c == '{' && !text.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut text), plain));
        }
        text.push(c);
        if c == '}' {
            spans.push(Span::styled(
                std::mem::take(&mut text),
                Style::new().fg(ACCENT),
            ));
        }
    }
    // A parameter cut short by the width is still one
    let rest = if text.starts_with('{') {
        Style::new().fg(ACCENT)
    } else {
        plain
    };
    spans.push(Span::styled(text, rest));
    spans
}

/// Methods in a column of four
fn short_method(method: &str) -> &str {
    match method {
        "DELETE" => "DEL",
        "PATCH" => "PTCH",
        "OPTIONS" => "OPT",
        other => other,
    }
}

/// The values the spec offers for a parameter: the ones it allows, or its
/// example
fn offered(field: &Field) -> Vec<String> {
    if !field.options.is_empty() {
        field.options.clone()
    } else {
        field.suggestion.iter().cloned().collect()
    }
}

/// Step a parameter through the values the spec offers, then (when it's
/// optional) back to leaving it out. False when the spec offers nothing.
fn next_value(field: &mut Field) -> bool {
    let offered = offered(field);
    if offered.is_empty() {
        return false;
    }
    let current = match (field.guessed, field.values.as_slice()) {
        (false, [only]) => offered.iter().position(|o| o == only),
        _ => None,
    };
    let next = match current {
        None => Some(0),
        Some(i) if i + 1 < offered.len() => Some(i + 1),
        Some(_) if field.required => Some(0),
        Some(_) => None,
    };
    field.set(next.map(|i| offered[i].clone()).into_iter().collect());
    true
}

/// Two lines about the selected line of the detail pane
fn about_row(endpoint: &Endpoint, row: DetailRow, width: usize) -> Vec<String> {
    match row {
        DetailRow::Field(index) => {
            let field = &endpoint.fields[index];
            let mut lines = wrap(&field.description, width, 1);
            lines.push(truncate(
                &if !field.options.is_empty() {
                    format!("one of: {}", field.options.join(", "))
                } else if let Some(example) = &field.suggestion {
                    format!("the spec's example: {example}")
                } else if field.required {
                    "required, and the spec gives no example for it".to_string()
                } else {
                    "optional: left out unless it's set".to_string()
                },
                width,
            ));
            lines
        }
        DetailRow::Body => vec![
            "The request body, from the spec's example or its schema.".into(),
            "enter edits it, del sends none".into(),
        ],
        DetailRow::Weight => vec![
            "This endpoint's share of the traffic, next to the others that are on.".into(),
            "2 gets twice the requests of an endpoint with 1".into(),
        ],
    }
}

/// "200 OK · 12.3ms" for a tried endpoint; with its size when `full`
fn tried_text(stat: &ResponseStats, full: bool) -> (String, Color) {
    match (stat.status_code, stat.error) {
        (Some(code), _) if full => (
            format!(
                "{} {} · {} · {}",
                code.as_u16(),
                code.canonical_reason().unwrap_or(""),
                format::latency(stat.duration),
                format::bytes(stat.body_bytes as f64)
            ),
            status_color(code.as_u16()),
        ),
        (Some(code), _) => (
            format!("{} · {}", code.as_u16(), format::latency(stat.duration)),
            status_color(code.as_u16()),
        ),
        (None, error) => (
            stat.error_message
                .as_deref()
                .map(|m| truncate(m, if full { 60 } else { 18 }))
                .unwrap_or_else(|| error.map_or("error", |e| e.label()).to_lowercase()),
            BAD,
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::openapi::{self, PlanOptions, Spec};
    use clap::Parser;
    use ratatui::backend::TestBackend;

    fn run_for(doc: serde_json::Value) -> ApiRun {
        let spec = Spec::parse(&doc, None, None).unwrap();
        let options = PlanOptions::default();
        let endpoints = openapi::plan(&spec, &options);
        ApiRun {
            spec,
            options,
            credentials: Credentials::default(),
            endpoints,
        }
    }

    /// Two tags: a read, a write, a read needing a value with optional
    /// parameters, and a body
    fn demo() -> serde_json::Value {
        serde_json::json!({
            "openapi": "3.0.0",
            "info": {"title": "Demo", "version": "1"},
            "servers": [{"url": "https://api.demo.io"}],
            "tags": [{"name": "Pets", "description": "Everything about pets"}, {"name": "Orders"}],
            "paths": {
                "/pets": {
                    "get": {"tags": ["Pets"], "summary": "List pets", "parameters": [
                        {"name": "limit", "in": "query", "schema": {"type": "integer", "default": 20}},
                        {"name": "status", "in": "query", "description": "Which pets", "schema": {"type": "string", "enum": ["available", "sold"]}},
                        {"name": "q", "in": "query", "schema": {"type": "string"}}
                    ]},
                    "post": {"tags": ["Pets"], "requestBody": {"content": {"application/json": {"schema": {"type": "object", "properties": {"name": {"type": "string", "example": "Rex"}}}}}}}
                },
                "/pets/{id}": {"get": {"tags": ["Pets"], "parameters": [{"name": "id", "in": "path", "required": true, "schema": {"type": "integer"}}]}},
                "/orders": {"get": {"tags": ["Orders"]}}
            }
        })
    }

    fn with_key(mut doc: serde_json::Value) -> serde_json::Value {
        doc["components"] = serde_json::json!({"securitySchemes": {"key": {"type": "apiKey", "in": "query", "name": "api_key"}}});
        doc
    }

    fn cli() -> Cli {
        Cli::parse_from(["pepe", "api", "x"])
    }

    fn press(s: &mut PlanScreen, code: KeyCode) -> Option<Action> {
        s.key(KeyEvent::from(code))
    }

    fn type_text(s: &mut PlanScreen, text: &str) {
        for c in text.chars() {
            assert!(press(s, KeyCode::Char(c)).is_none());
        }
    }

    fn on(s: &PlanScreen) -> Vec<bool> {
        s.run.endpoints.iter().map(|e| e.enabled).collect()
    }

    #[test]
    fn endpoints_are_listed_under_their_tags_and_nothing_is_on() {
        let (mut run, mut cli) = (run_for(demo()), cli());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        assert_eq!(
            s.rows(),
            [
                Row::Tag(0),
                Row::Endpoint(0),
                Row::Endpoint(1),
                Row::Endpoint(2),
                Row::Tag(1),
                Row::Endpoint(3)
            ]
        );
        assert_eq!(on(&s), [false; 4]);
        // Starting with nothing on says so instead
        assert!(press(&mut s, KeyCode::Char('g')).is_none());
        assert!(matches!(&s.message, Some((m, true)) if m.contains("nothing to start")));

        // Folding hides a tag's endpoints; ← on an endpoint goes to its tag
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.rows(), [Row::Tag(0), Row::Tag(1), Row::Endpoint(3)]);
        press(&mut s, KeyCode::Right);
        assert_eq!(s.rows().len(), 6);
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Down);
        assert_eq!(s.selected(), Some(1));
        press(&mut s, KeyCode::Left);
        assert_eq!(s.current(), Some(Row::Tag(0)));
    }

    #[test]
    fn a_tag_switches_on_what_can_run_unasked() {
        let (mut run, mut cli) = (run_for(demo()), cli());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(
            on(&s),
            [true, false, false, false],
            "not the write, not the one needing a value"
        );
        assert!(
            matches!(&s.message, Some((m, false)) if m.contains("1 on") && m.contains("1 writes") && m.contains("1 left off"))
        );
        // Again: the tag goes off
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(on(&s), [false; 4]);

        // x: everything that can run unasked, then nothing
        press(&mut s, KeyCode::Char('x'));
        assert_eq!(on(&s), [true, false, false, true]);
        press(&mut s, KeyCode::Char('x'));
        assert_eq!(on(&s), [false; 4]);

        // One endpoint: a write warns, and starting then works
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char(' '));
        assert!(on(&s)[1]);
        assert!(matches!(&s.message, Some((m, true)) if m.contains("change data")));
        assert_eq!(press(&mut s, KeyCode::Char('g')), Some(Action::Start));
    }

    #[test]
    fn parameters_are_set_in_the_detail_pane() {
        let (mut run, mut cli) = (run_for(demo()), cli());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        // GET /pets: limit (default 20), status (enum), q
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.focus, Focus::Detail);
        assert_eq!(
            s.detail_rows(0),
            [
                DetailRow::Field(0),
                DetailRow::Field(1),
                DetailRow::Field(2),
                DetailRow::Weight
            ]
        );

        // space: the spec's example, then left out again
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(s.run.endpoints[0].fields[0].values, ["20"]);
        press(&mut s, KeyCode::Char(' '));
        assert!(s.run.endpoints[0].fields[0].values.is_empty());

        // space steps through the values the spec allows
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(s.run.endpoints[0].fields[1].values, ["available"]);
        press(&mut s, KeyCode::Char(' '));
        assert_eq!(s.run.endpoints[0].fields[1].values, ["sold"]);

        // The spec offers nothing for q: space opens the prompt. Several
        // values rotate.
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char(' '));
        assert!(matches!(
            s.prompt.as_ref().map(|p| p.what),
            Some(Editing::Field {
                endpoint: 0,
                field: 2
            })
        ));
        type_text(&mut s, "rex, tom cat");
        press(&mut s, KeyCode::Enter);
        assert!(s.prompt.is_none());
        assert_eq!(
            s.run.urls(0),
            [
                "https://api.demo.io/pets?status=sold&q=rex",
                "https://api.demo.io/pets?status=sold&q=tom%20cat"
            ]
        );

        // del leaves a parameter out; the share is a number from 1 to 100
        press(&mut s, KeyCode::Delete);
        assert_eq!(s.run.urls(0), ["https://api.demo.io/pets?status=sold"]);
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Enter);
        press(&mut s, KeyCode::Backspace);
        type_text(&mut s, "0");
        press(&mut s, KeyCode::Enter);
        assert!(s.prompt.is_some(), "refused, and the prompt stays");
        assert!(matches!(&s.message, Some((m, true)) if m.contains("1 to 100")));
        press(&mut s, KeyCode::Backspace);
        type_text(&mut s, "3");
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.run.endpoints[0].weight, 3);

        // esc goes back to the list, not out of the screen
        assert!(press(&mut s, KeyCode::Esc).is_none());
        assert_eq!(s.focus, Focus::List);
    }

    #[test]
    fn a_missing_value_and_a_body_are_edited() {
        let (mut run, mut cli) = (run_for(demo()), cli());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        // GET /pets/{id} needs id: its guess isn't offered as the text to edit
        s.move_to(3);
        assert_eq!(s.selected(), Some(2));
        assert_eq!(s.status(2).0, "needs id");
        press(&mut s, KeyCode::Enter);
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.prompt.as_ref().unwrap().input.text(), "");
        type_text(&mut s, "7");
        press(&mut s, KeyCode::Enter);
        assert!(s.run.endpoints[2].missing().is_empty());
        assert_eq!(s.run.urls(2), ["https://api.demo.io/pets/7"]);
        assert!(
            !s.run.endpoints[2].enabled,
            "setting a value doesn't switch it on"
        );

        // POST /pets: the body, with the cursor moved inside the text
        press(&mut s, KeyCode::Esc);
        s.move_to(2);
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.detail_rows(1), [DetailRow::Body, DetailRow::Weight]);
        press(&mut s, KeyCode::Enter);
        assert_eq!(s.prompt.as_ref().unwrap().input.text(), r#"{"name":"Rex"}"#);
        for _ in 0..2 {
            press(&mut s, KeyCode::Left);
        }
        press(&mut s, KeyCode::Backspace);
        type_text(&mut s, "x!");
        press(&mut s, KeyCode::Enter);
        assert_eq!(
            s.run.endpoints[1].body.as_deref(),
            Some(&br#"{"name":"Rex!"}"#[..])
        );
        // Not JSON: kept, with a warning; del sends no body
        press(&mut s, KeyCode::Enter);
        type_text(&mut s, "oops");
        press(&mut s, KeyCode::Enter);
        assert!(matches!(&s.message, Some((m, true)) if m.contains("valid JSON")));
        press(&mut s, KeyCode::Delete);
        assert!(s.run.endpoints[1].body.is_none());
        assert!(
            s.run.endpoints[1].headers().is_empty(),
            "no content type without a body"
        );
    }

    #[test]
    fn the_filter_narrows_the_list() {
        let (mut run, mut cli) = (run_for(demo()), cli());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        press(&mut s, KeyCode::Char('/'));
        type_text(&mut s, "order");
        assert_eq!(s.rows(), [Row::Tag(1), Row::Endpoint(3)]);
        press(&mut s, KeyCode::Enter);
        assert!(s.prompt.is_none());
        // x only touches what's shown
        press(&mut s, KeyCode::Char('x'));
        assert_eq!(on(&s), [false, false, false, true]);
        // esc clears the filter before it quits
        assert!(press(&mut s, KeyCode::Esc).is_none());
        assert_eq!(s.rows().len(), 6);
        assert_eq!(press(&mut s, KeyCode::Esc), Some(Action::Quit));
    }

    #[test]
    fn load_and_server_are_changed_from_the_screen() {
        let (mut run, mut cli) = (run_for(demo()), cli());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        let retype = |s: &mut PlanScreen, key: char, text: &str| {
            press(s, KeyCode::Char(key));
            s.prompt.as_mut().unwrap().input = TextInput::new(text);
            press(s, KeyCode::Enter);
        };
        retype(&mut s, 'n', "5,000");
        retype(&mut s, 'c', "40");
        assert_eq!((s.cli.number, s.cli.concurrency), (5000, 40));
        // More at once than there are requests: refused, with the reason
        retype(&mut s, 'c', "9000");
        assert!(matches!(&s.message, Some((m, true)) if m.contains("Concurrency")));
        assert_eq!(s.cli.concurrency, 40);
        press(&mut s, KeyCode::Esc);

        retype(&mut s, 'z', "30s");
        assert_eq!(s.cli.run_duration(), Some(Duration::from_secs(30)));
        retype(&mut s, 'z', "soon");
        assert!(s.prompt.is_some());
        press(&mut s, KeyCode::Esc);
        retype(&mut s, 'z', "");
        assert_eq!(s.cli.duration, None);

        retype(&mut s, 'u', "http://localhost:3000/");
        assert_eq!(s.run.urls(3), ["http://localhost:3000/orders"]);
        retype(&mut s, 'u', "localhost");
        assert!(s.prompt.is_some(), "not a URL");
    }

    #[test]
    fn missing_auth_is_asked_for_up_front() {
        let (mut run, mut cli) = (run_for(with_key(demo())), cli());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        assert!(
            matches!(
                s.prompt.as_ref().map(|p| p.what),
                Some(Editing::Auth { form: 0 })
            ),
            "prompt opens by itself"
        );
        assert!(s
            .prompt_text(Editing::Auth { form: 0 })
            .0
            .contains("API key"));

        // Typing goes to the prompt, shortcuts included; enter hands the
        // secret over to be checked
        type_text(&mut s, "qs3cret");
        let action = press(&mut s, KeyCode::Enter);
        assert_eq!(
            action,
            Some(Action::Auth {
                secret: "qs3cret".into(),
                form: 0
            })
        );
        assert!(s.prompt.is_none());
    }

    #[test]
    fn declining_auth_is_remembered_and_start_asks_once() {
        let (mut run, mut cli) = (run_for(with_key(demo())), cli());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        press(&mut s, KeyCode::Esc);
        assert!(s.prompt.is_none() && s.auth_declined);
        assert!(matches!(&s.message, Some((m, true)) if m.contains("refused")));
        press(&mut s, KeyCode::Char('x'));
        // Declined: starting goes ahead without asking again
        assert_eq!(press(&mut s, KeyCode::Char('g')), Some(Action::Start));

        // Not declined (credentials cleared some other way): start asks first
        s.auth_declined = false;
        assert!(press(&mut s, KeyCode::Char('g')).is_none());
        assert!(matches!(
            s.prompt.as_ref().map(|p| p.what),
            Some(Editing::Auth { .. })
        ));
    }

    #[test]
    fn no_prompt_when_auth_is_given_or_not_needed() {
        let mut given = run_for(with_key(demo()));
        given.credentials = Credentials::parse(&["apikey:k".into()], &given.spec).unwrap();
        let mut cli = cli();
        let s = PlanScreen::new(&mut given, &mut cli);
        assert!(s.prompt.is_none());
        assert_eq!(s.auth, AuthState::Provided);
        assert_eq!(s.run.urls(3), ["https://api.demo.io/orders?api_key=k"]);

        let mut open = run_for(demo());
        let s = PlanScreen::new(&mut open, &mut cli);
        assert!(s.prompt.is_none());
    }

    #[test]
    fn bearer_specs_also_offer_the_bare_key() {
        let mut run = run_for(serde_json::json!({
            "openapi": "3.0.0", "servers": [{"url": "https://api.demo.io"}],
            "components": {"securitySchemes": {"b": {"type": "http", "scheme": "bearer"}}},
            "paths": {"/a": {"get": {}}}
        }));
        let mut cli = cli();
        let mut s = PlanScreen::new(&mut run, &mut cli);
        let forms = s.auth_forms();
        assert_eq!(forms.len(), 2);
        // Tab switches the form in the prompt
        press(&mut s, KeyCode::Tab);
        assert_eq!(s.prompt.as_ref().unwrap().what, Editing::Auth { form: 1 });
        press(&mut s, KeyCode::Tab);
        assert_eq!(s.prompt.as_ref().unwrap().what, Editing::Auth { form: 0 });

        s.set_auth("k", &forms[0]).unwrap();
        assert_eq!(
            s.run.credentials.headers,
            [("Authorization".to_string(), "Bearer k".to_string())]
        );
        s.set_auth("k", &forms[1]).unwrap();
        assert_eq!(
            s.run.credentials.headers,
            [("Authorization".to_string(), "k".to_string())]
        );
    }

    #[tokio::test]
    async fn the_form_the_api_accepts_is_found() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        // Like Foreplay: the spec says bearer, the API wants the bare key
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let request = String::from_utf8_lossy(&buf[..n]).to_lowercase();
                    let ok = request.contains("authorization: k3y\r\n");
                    let status = if ok { "200 OK" } else { "401 Unauthorized" };
                    let _ = sock
                        .write_all(format!("HTTP/1.1 {status}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n").as_bytes())
                        .await;
                });
            }
        });
        // The check goes to the read with the fewest parameters, though
        // nothing is on
        let doc = serde_json::json!({
            "openapi": "3.0.0", "servers": [{"url": url}],
            "components": {"securitySchemes": {"b": {"type": "http", "scheme": "bearer"}}},
            "paths": {
                "/ads": {"get": {"parameters": [{"name": "limit", "in": "query", "schema": {"type": "integer"}}]}},
                "/ad/{id}": {"get": {"parameters": [{"name": "id", "in": "path", "required": true}]}},
                "/usage": {"get": {}}
            }
        });
        let mut cli = cli();

        let mut run = run_for(doc.clone());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        s.apply_auth("k3y".into(), 0).await;
        assert!(
            matches!(&s.auth, AuthState::Accepted(how) if how.contains("200 on GET /usage") && how.contains("plain Authorization header (refused as Bearer token)")),
            "{:?}",
            s.auth
        );
        assert_eq!(
            s.run.credentials.headers,
            [("Authorization".to_string(), "k3y".to_string())]
        );
        assert_eq!(s.status(2).0.split(' ').next(), Some("200"));

        // A key the API refuses in every form
        let mut run = run_for(doc);
        let mut s = PlanScreen::new(&mut run, &mut cli);
        s.apply_auth("wrong".into(), 0).await;
        assert!(
            matches!(&s.auth, AuthState::Rejected(why) if why.contains("401")),
            "{:?}",
            s.auth
        );
        assert_eq!(
            s.run.credentials.headers,
            [("Authorization".to_string(), "Bearer wrong".to_string())],
            "the chosen form stays"
        );
    }

    #[test]
    fn text_input_edits_in_the_middle() {
        let mut input = TextInput::new("hello world");
        for _ in 0..5 {
            input.key(KeyEvent::from(KeyCode::Left));
        }
        input.key(KeyEvent::from(KeyCode::Backspace));
        input.insert("_big_\n");
        assert_eq!(input.text(), "hello_big_world");
        input.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "world");
        input.key(KeyEvent::from(KeyCode::Delete));
        assert_eq!(input.text(), "orld");
        input.key(KeyEvent::from(KeyCode::End));
        input.key(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(input.text(), "");

        // A long text scrolls to keep the cursor in view, and can be hidden
        let input = TextInput::new("abcdefghij");
        let shown: String = input
            .spans(5, false)
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(shown, "ghij ");
        let hidden: String = input
            .spans(20, true)
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(hidden, "•••••••••• ");
    }

    #[test]
    fn wrapping() {
        assert_eq!(wrap("one two three", 8, 5), ["one two", "three"]);
        assert_eq!(
            wrap("https://api.demo.io/a/very/long/path", 12, 5),
            ["https://api.", "demo.io/a/ve", "ry/long/path"]
        );
        assert_eq!(wrap("one two three four", 8, 1), ["one two…"]);
        assert!(wrap("", 20, 3).is_empty());
    }

    #[test]
    fn renders_at_many_sizes() {
        let (mut run, mut cli) = (run_for(with_key(demo())), cli());
        let mut s = PlanScreen::new(&mut run, &mut cli);
        let states = [
            AuthState::None,
            AuthState::Provided,
            AuthState::Accepted("200 on GET /a, sent as API key".into()),
            AuthState::Rejected("401 on GET /a".into()),
        ];
        for (w, h) in [(40, 12), (80, 24), (100, 30), (120, 40), (220, 60)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            // The auth prompt, then every row with each pane focused
            terminal.draw(|f| s.render(f)).unwrap();
            s.prompt = None;
            for row in 0..s.rows().len() {
                s.cursor = row;
                s.auth = states[row % states.len()].clone();
                for focus in [Focus::List, Focus::Detail] {
                    s.focus = focus;
                    for detail in 0..4 {
                        s.detail_cursor = detail;
                        terminal.draw(|f| s.render(f)).unwrap();
                    }
                }
            }
            s.focus = Focus::List;
            s.filter = "nothing matches".into();
            terminal.draw(|f| s.render(f)).unwrap();
            s.filter.clear();
            s.open(Editing::Body(1), &"x".repeat(500));
            terminal.draw(|f| s.render(f)).unwrap();
        }
    }
}
