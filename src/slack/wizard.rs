//! `pepe slack connect` at a terminal: a checklist redrawn in place, Pepe
//! beside the title, a spinner while Slack answers, the token typed as
//! dots, and the channel picked with the arrow keys from a list that
//! filters as you type.

use std::io::{stderr, Write};
use std::time::Duration;

use crossterm::cursor::{Hide, MoveToColumn, MoveUp, Show};
use crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, Event, EventStream, KeyCode, KeyEventKind,
    KeyModifiers,
};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, Clear, ClearType};
use crossterm::{execute, queue};
use futures::StreamExt;

use super::{Api, Channel};
use crate::ui::mascot::{self, Mood};

const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
/// Channels shown at once in the picker
const ROWS: usize = 7;

/// Colour, unless NO_COLOR or the theme says none
fn paint(code: &str, text: &str) -> String {
    if crate::ui::theme::no_color() {
        text.to_string()
    } else {
        format!("\x1b[{code}m{text}\x1b[0m")
    }
}
pub(super) fn good(t: &str) -> String {
    paint("38;2;156;207;106", t)
}
pub(super) fn ember(t: &str) -> String {
    paint("38;2;255;138;61", t)
}
pub(super) fn faint(t: &str) -> String {
    paint("38;2;156;138;126", t)
}
pub(super) fn bad(t: &str) -> String {
    paint("38;2;255;90;110", t)
}
pub(super) fn bold(t: &str) -> String {
    paint("1", t)
}

#[derive(Clone)]
enum State {
    Waiting,
    Active,
    Busy(String),
    Done(String),
}

struct Step {
    title: &'static str,
    state: State,
}

/// The terminal as the wizard leaves it, whatever happens
struct Raw;

impl Raw {
    fn enter() -> Result<Raw, String> {
        enable_raw_mode().map_err(|e| e.to_string())?;
        let _ = execute!(stderr(), EnableBracketedPaste, Hide);
        Ok(Raw)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        let _ = execute!(stderr(), DisableBracketedPaste, Show);
        let _ = disable_raw_mode();
    }
}

struct Wizard {
    steps: Vec<Step>,
    /// Lines under the active step: an input, the picker, a hint
    below: Vec<String>,
    /// What went wrong last, said under the steps
    error: Option<String>,
    hint: String,
    drawn: u16,
    tick: usize,
    events: EventStream,
}

impl Wizard {
    fn new() -> Wizard {
        let titles = [
            "Create the app",
            "Bot token",
            "Channel",
            "Test message",
            "Buttons",
            "Who may press them",
        ];
        Wizard {
            steps: titles
                .iter()
                .map(|title| Step {
                    title,
                    state: State::Waiting,
                })
                .collect(),
            below: Vec::new(),
            error: None,
            hint: String::new(),
            drawn: 0,
            tick: 0,
            events: EventStream::new(),
        }
    }

    fn set(&mut self, step: usize, state: State) {
        self.steps[step].state = state;
    }

    /// The checklist, drawn over what it drew last
    fn draw(&mut self) {
        let mut out = stderr();
        if self.drawn > 0 {
            let _ = queue!(out, MoveUp(self.drawn), MoveToColumn(0));
        }
        let _ = queue!(out, Clear(ClearType::FromCursorDown));
        let mut lines = Vec::new();
        for (i, step) in self.steps.iter().enumerate() {
            let n = i + 1;
            let (mark, title, detail) = match &step.state {
                State::Waiting => (faint("○"), faint(step.title), String::new()),
                State::Active => (ember("›"), bold(step.title), String::new()),
                State::Busy(what) => (
                    ember(SPINNER[self.tick % SPINNER.len()]),
                    bold(step.title),
                    faint(what),
                ),
                State::Done(what) => (good("✓"), step.title.to_string(), faint(what)),
            };
            let pad = " ".repeat(21usize.saturating_sub(step.title.chars().count()));
            lines.push(format!("  {mark} {n}  {title}{pad}{detail}"));
            if matches!(step.state, State::Active) {
                lines.extend(self.below.iter().map(|l| format!("        {l}")));
            }
        }
        lines.push(String::new());
        match &self.error {
            Some(e) => lines.push(format!("  {}", bad(e))),
            None => lines.push(format!("  {}", faint(&self.hint))),
        }
        for line in &lines {
            let _ = write!(out, "{line}\r\n");
        }
        let _ = out.flush();
        self.drawn = lines.len() as u16;
    }

    /// Wait for `work`, the spinner turning on `step` meanwhile
    async fn busy<T>(
        &mut self,
        step: usize,
        what: &str,
        work: impl std::future::Future<Output = T>,
    ) -> T {
        self.set(step, State::Busy(what.to_string()));
        let mut work = Box::pin(work);
        let mut clock = tokio::time::interval(Duration::from_millis(80));
        loop {
            tokio::select! {
                done = &mut work => return done,
                _ = clock.tick() => {
                    self.tick += 1;
                    self.draw();
                }
            }
        }
    }

    /// The next key or paste; Err when the person stops
    async fn input(&mut self) -> Result<Input, String> {
        loop {
            match self.events.next().await {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
                    return Ok(match key.code {
                        KeyCode::Char('c') if ctrl => return Err("stopped".into()),
                        KeyCode::Esc => return Err("stopped".into()),
                        KeyCode::Enter => Input::Enter,
                        KeyCode::Backspace => Input::Back,
                        KeyCode::Up => Input::Up,
                        KeyCode::Down | KeyCode::Tab => Input::Down,
                        KeyCode::Char('u') if ctrl => Input::Clear,
                        KeyCode::Char(c) => Input::Text(c.to_string()),
                        _ => continue,
                    });
                }
                Some(Ok(Event::Paste(text))) => return Ok(Input::Text(text.trim().to_string())),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(e.to_string()),
                None => return Err("stopped".into()),
            }
        }
    }

    /// The token, typed or pasted, shown as dots
    async fn token(&mut self) -> Result<String, String> {
        let hint = [faint(
            "the Bot User OAuth Token (xoxb-…), under OAuth & Permissions after Install",
        )];
        self.typed(true, &hint, false).await
    }

    /// A line typed or pasted: as dots when `secret`; `hint` under it; an
    /// empty one only when `empty` lets it be
    async fn typed(
        &mut self,
        secret: bool,
        hint: &[String],
        empty: bool,
    ) -> Result<String, String> {
        let mut text = String::new();
        loop {
            let shown = if secret {
                "•".repeat(text.chars().count().min(40))
            } else {
                text.clone()
            };
            self.below = vec![format!("{}{}", shown, ember("▏"))];
            self.below.extend(hint.iter().cloned());
            self.draw();
            match self.input().await? {
                Input::Enter if empty || !text.trim().is_empty() => {
                    return Ok(text.trim().to_string())
                }
                Input::Text(t) => {
                    self.error = None;
                    text.push_str(&t);
                }
                Input::Back => {
                    text.pop();
                }
                Input::Clear => text.clear(),
                _ => {}
            }
        }
    }

    /// The channel, picked from `all` with the arrow keys as typing filters it
    async fn channel(&mut self, all: &[Channel]) -> Result<Channel, String> {
        let mut query = String::new();
        let mut at = 0usize;
        loop {
            let found = super::matching(all, &query);
            at = at.min(found.len().saturating_sub(1));
            let top = at.saturating_sub(ROWS - 1);
            let mut below = vec![format!("#{query}{}", ember("▏"))];
            for (i, c) in found.iter().enumerate().skip(top).take(ROWS) {
                let name = format!("#{}", c.name);
                let about = format!(
                    "{} member{}{}{}",
                    crate::ui::format::count(c.members),
                    if c.members == 1 { "" } else { "s" },
                    if c.private { " · private" } else { "" },
                    if c.joined { " · pepe's in it" } else { "" },
                );
                let pad = " ".repeat(24usize.saturating_sub(name.chars().count()));
                below.push(if i == at {
                    format!("{} {}{pad}{}", ember("›"), bold(&name), faint(&about))
                } else {
                    format!("  {name}{pad}{}", faint(&about))
                });
            }
            if found.is_empty() {
                below.push(faint(
                    "no channel by that name: a private one needs /invite @pepe",
                ));
            } else if found.len() > ROWS {
                below.push(faint(&format!(
                    "{} more · type to narrow",
                    found.len() - ROWS
                )));
            }
            self.below = below;
            self.draw();
            match self.input().await? {
                Input::Enter => {
                    if let Some(c) = found.get(at) {
                        return Ok((*c).clone());
                    }
                }
                Input::Up => at = at.saturating_sub(1),
                Input::Down => at += 1,
                Input::Text(t) => {
                    query.push_str(t.trim_start_matches('#'));
                    at = 0;
                }
                Input::Back => {
                    query.pop();
                    at = 0;
                }
                Input::Clear => query.clear(),
            }
        }
    }
}

/// `text`, a link where the terminal makes them (OSC 8)
fn hyperlink(url: &str, text: &str) -> String {
    format!("\x1b]8;;{url}\x1b\\{}\x1b]8;;\x1b\\", bold(text))
}

/// Put `text` on the clipboard, when there's a program to do it
pub fn copy(text: &str) -> bool {
    use std::process::{Command, Stdio};
    let programs: &[(&str, &[&str])] = if cfg!(target_os = "macos") {
        &[("pbcopy", &[])]
    } else if cfg!(windows) {
        &[("clip", &[])]
    } else {
        &[
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ]
    };
    programs.iter().any(|(program, args)| {
        let Ok(mut child) = Command::new(program)
            .args(*args)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        else {
            return false;
        };
        let written = child
            .stdin
            .take()
            .is_some_and(|mut stdin| stdin.write_all(text.as_bytes()).is_ok());
        written && child.wait().is_ok_and(|s| s.success())
    })
}

enum Input {
    Enter,
    Back,
    Up,
    Down,
    Clear,
    Text(String),
}

/// The checklist from the first step to the test message; what it kept
pub(super) async fn run(link: &str) -> Result<super::Done, String> {
    // Pepe and the title, once, above the checklist
    let face = mascot::ansi(Mood::Happy);
    let title = [
        String::new(),
        String::new(),
        bold("pepe × Slack"),
        faint("your app, in your workspace · nothing hosted"),
    ];
    eprintln!();
    for (i, row) in face.iter().enumerate() {
        eprintln!("  {row}   {}", title.get(i).cloned().unwrap_or_default());
    }
    if face.is_empty() {
        eprintln!(
            "  {}   {}",
            bold("pepe × Slack"),
            faint("your app, in your workspace")
        );
    }
    eprintln!();

    let _raw = Raw::enter()?;
    let mut w = Wizard::new();
    w.hint = "enter next · esc stop".into();
    let opened = super::open_browser(link);
    let copied = !opened && copy(link);
    w.set(
        0,
        State::Done(match (opened, copied) {
            (true, _) => "opened in your browser, everything filled in".into(),
            (false, true) => format!(
                "{} · the link is copied: paste it in a browser",
                hyperlink(link, "Slack's create-app page")
            ),
            (false, false) => format!("open {}", hyperlink(link, "Slack's create-app page")),
        }),
    );
    w.hint = "Slack: pick the workspace → Create → Install to Workspace → Allow · esc stops".into();

    // The token, until Slack takes it
    let (token, team, url) =
        loop {
            w.set(1, State::Active);
            let token = w.token().await?;
            w.below.clear();
            match w
                .busy(1, "checking with Slack…", super::check(&token))
                .await
            {
                Ok((team, url)) => {
                    w.set(
                        1,
                        State::Done(format!("{team} · {}", url.trim_end_matches('/'))),
                    );
                    w.error = None;
                    break (token, team, url);
                }
                // Here, "run pepe slack connect" would send them where they are
                Err(e) if e.contains("isn't valid") => w.error = Some(
                    "Slack didn't take that token: copy the Bot User OAuth Token again (xoxb-…)"
                        .into(),
                ),
                Err(e) => w.error = Some(e),
            }
        };

    // The channel
    let api = Api::new(&token);
    let all = w
        .busy(2, "listing the channels…", super::channels(&api))
        .await?;
    w.hint = "↑↓ pick · type to filter · enter · esc stop".into();
    w.set(2, State::Active);
    let channel = w.channel(&all).await?;
    w.below.clear();
    w.set(2, State::Done(format!("#{}", channel.name)));

    // In, a test message, and kept
    w.hint = String::new();
    let posted = w
        .busy(
            3,
            &format!("joining #{} and posting a test…", channel.name),
            super::finish(&token, &team, &url, &channel.name, &channel.id),
        )
        .await;
    let mut done = match posted {
        Ok(done) => {
            let shown = done
                .permalink
                .clone()
                .unwrap_or_else(|| format!("posted in #{}", channel.name));
            w.set(3, State::Done(shown));
            w.error = None;
            done
        }
        Err(e) => {
            w.error = Some(e.clone());
            w.draw();
            return Err(e);
        }
    };

    // Buttons: an app-level token for `pepe slack serve`, or skipped
    w.hint = "enter with nothing skips: pepe slack connect again adds them later".into();
    let app = loop {
        w.set(4, State::Active);
        let hint = [
            faint("for Stop, Run again, Edit and run, and /pepe: the app's Basic Information →"),
            faint("App-Level Tokens → Generate Token and Scopes, add connections:write, Generate (xapp-…)"),
        ];
        let app = w.typed(true, &hint, true).await?;
        w.below.clear();
        if app.is_empty() {
            w.set(4, State::Done("skipped".into()));
            w.set(5, State::Done("no buttons, no one to ask".into()));
            break None;
        }
        match w
            .busy(
                4,
                "opening a Socket Mode connection…",
                super::check_app(&app),
            )
            .await
        {
            Ok(()) => {
                w.set(
                    4,
                    State::Done("Socket Mode answers · pepe slack serve listens with it".into()),
                );
                w.error = None;
                break Some(app);
            }
            Err(e) => w.error = Some(e),
        }
    };

    // Who may press them
    if let Some(app) = app {
        w.hint = "@names, Slack ids, or * for anyone in the channel".into();
        let (ids, names) = loop {
            w.set(5, State::Active);
            let hint = [faint("you, at least: your Slack @name")];
            let who = w.typed(false, &hint, false).await?;
            w.below.clear();
            match w
                .busy(5, "looking them up…", super::people(&token, &who))
                .await
            {
                Ok(found) => {
                    w.error = None;
                    break found;
                }
                Err(e) => w.error = Some(e),
            }
        };
        w.set(5, State::Done(names.join(", ")));
        let kept = super::update(|c| {
            c.app_token = Some(app.clone());
            c.allow = ids;
            c.allow_names = names.clone();
        });
        if let Err(e) = kept {
            w.error = Some(e);
        }
        done.buttons = true;
        done.allowed = names;
    }
    w.hint = String::new();
    w.draw();
    Ok(done)
}
