//! The library: the project's saved tests, in front of the setup form.
//! `pepe` alone opens here when `.pepe/tests` has anything in it; enter
//! puts a test into the form, where it can still be changed before the
//! start, and `n` starts from an empty form. The mouse works here too: a
//! click selects a row, a second click on it opens it, the wheel scrolls.

use std::time::{Instant, SystemTime};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Padding, Paragraph, Wrap},
    Frame,
};

use super::history::{HistoryScreen, Recorded};
use super::kit::{
    self, about_line, caption, help, marker, panel, Clicks, Hit, SaveAs, SaveKey, FAINT, SELECTED,
};
use super::view::{label, truncate, value, ACCENT, GOOD, LABEL, RULE, WARN};
use super::{mascot, settings, theme};
use crate::cli::Command;
use crate::compare::Side;
use crate::store::{Entry, LastRun, Store};
use crate::Cli;

/// How the library was left
pub enum LibraryOutcome {
    /// Put this saved test into the setup form
    Open(String),
    /// Start from an empty form
    New,
    Quit,
}

enum Action {
    Open,
    New,
    /// Enter in the copy-as prompt
    Copy,
    /// `h`: the selected test's recorded runs
    History,
    Quit,
}

/// Names wider than this are cut in the list
const NAME_MAX: usize = 24;

pub struct LibraryScreen {
    store: Store,
    entries: Vec<Entry>,
    selected: usize,
    /// The keys overlay is open
    show_help: bool,
    /// Tells a second click on a row from the first
    clicks: Clicks,
    /// The copy-as prompt, while it's open
    naming: Option<SaveAs>,
    /// Shown on the status line until the next key: (text, is an error)
    message: Option<(String, bool)>,
}

impl LibraryScreen {
    pub fn new(store: Store, entries: Vec<Entry>) -> Self {
        // Start on a test that runs, not on a file that doesn't read
        let selected = entries.iter().position(|e| e.cli.is_ok()).unwrap_or(0);
        LibraryScreen {
            store,
            entries,
            selected,
            show_help: false,
            clicks: Clicks::default(),
            naming: None,
            message: None,
        }
    }

    /// `c`: ask for the name a copy of the selected test goes under
    fn open_copy(&mut self) {
        let Some(entry) = self.selected() else {
            return;
        };
        if entry.cli.is_err() {
            self.message = Some(("this file can't be read, so can't be copied".into(), true));
            return;
        }
        let offered = format!("{}-copy", entry.name);
        self.naming = Some(SaveAs::new("copy as", &offered));
    }

    /// Enter in the copy-as prompt: the selected test's settings under the
    /// new name; the list then shows it, selected
    fn copy(&mut self) {
        let Some(mut naming) = self.naming.take() else {
            return;
        };
        let Some(Ok(cli)) = self.selected().map(|e| &e.cli) else {
            return;
        };
        let cli = cli.clone();
        match naming.save(&self.store, None, &cli) {
            Some(name) => {
                self.entries = self.store.entries();
                self.selected = self
                    .entries
                    .iter()
                    .position(|e| e.name == name)
                    .unwrap_or(self.selected);
                self.message = naming.message.take();
            }
            None => self.naming = Some(naming),
        }
    }

    /// A click selects the row under it, a second click on the same row
    /// opens it; a key chip is its key; the wheel moves the selection
    fn mouse(&mut self, event: MouseEvent, now: Instant) -> Option<Action> {
        if settings::is_open() {
            return None;
        }
        if self.show_help {
            if kit::pressed(&event) {
                self.show_help = false;
            }
            return None;
        }
        match kit::click(&event)? {
            Hit::Key(key) => self.key(key),
            hit @ Hit::Row { index, .. } if index < self.entries.len() && self.naming.is_none() => {
                self.selected = index;
                self.clicks.second(hit, now).then_some(Action::Open)
            }
            _ => None,
        }
    }

    fn selected(&self) -> Option<&Entry> {
        self.entries.get(self.selected)
    }

    fn move_by(&mut self, by: i64) {
        let last = self.entries.len().saturating_sub(1) as i64;
        self.selected = (self.selected as i64 + by).clamp(0, last) as usize;
    }

    fn key(&mut self, key: KeyEvent) -> Option<Action> {
        // AltGr arrives as ctrl+alt on Windows, and types a character
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        if settings::key(key) {
            return None;
        }
        if self.show_help {
            // Any key closes it; ctrl-c still quits
            self.show_help = false;
            if key.code == KeyCode::Char('c') && ctrl {
                return Some(Action::Quit);
            }
            return None;
        }
        self.message = None;
        if let Some(naming) = &mut self.naming {
            match naming.key(key) {
                SaveKey::Quit => return Some(Action::Quit),
                SaveKey::Cancel => self.naming = None,
                SaveKey::Save => return Some(Action::Copy),
                SaveKey::Typed => {}
            }
            return None;
        }
        match key.code {
            KeyCode::Char('c') if ctrl => return Some(Action::Quit),
            KeyCode::Esc | KeyCode::Char('q') => return Some(Action::Quit),
            KeyCode::Enter if self.selected().is_some() => return Some(Action::Open),
            KeyCode::Char('n') => return Some(Action::New),
            KeyCode::Char('c') => self.open_copy(),
            KeyCode::Char('h') if self.selected().is_some() => return Some(Action::History),
            KeyCode::F(1) | KeyCode::Char('?') => self.show_help = true,
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_by(-i64::from(u32::MAX)),
            KeyCode::End => self.move_by(i64::from(u32::MAX)),
            _ => {}
        }
        None
    }

    pub async fn run(mut self) -> Result<LibraryOutcome, Box<dyn std::error::Error>> {
        let mut terminal = crate::terminal::Screen::new()?;
        terminal.clear()?;
        let mut events = crate::terminal::input();
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        loop {
            terminal.draw(|f| theme::draw(f, |f| self.render(f)))?;
            let event = tokio::select! {
                _ = &mut ctrl_c => return Ok(LibraryOutcome::Quit),
                // A frame skipped while the terminal was behind
                _ = crate::terminal::caught_up(), if terminal.owes() => continue,
                event = events.next() => event,
            };
            let action = match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => self.key(key),
                Some(Ok(Event::Mouse(mouse))) => self.mouse(mouse, Instant::now()),
                Some(Ok(_)) => None,
                Some(Err(e)) => return Err(e.into()),
                None => return Ok(LibraryOutcome::Quit),
            };
            match action {
                Some(Action::Quit) => return Ok(LibraryOutcome::Quit),
                Some(Action::New) => return Ok(LibraryOutcome::New),
                Some(Action::Copy) => self.copy(),
                Some(Action::History) => {
                    if let Some(entry) = self.selected() {
                        let name = entry.name.clone();
                        let runs: Vec<Recorded> = self
                            .store
                            .runs(&name)
                            .into_iter()
                            .filter_map(|run| {
                                Side::read(&run.path)
                                    .ok()
                                    .map(|side| Recorded { run, side })
                            })
                            .collect();
                        if runs.is_empty() {
                            self.message = Some((
                                format!("no recorded run of `{name}` yet · pepe --load {name} --record, or w on the dashboard"),
                                true,
                            ));
                        } else {
                            HistoryScreen::new(self.store.clone(), &name, runs)
                                .run()
                                .await?;
                            terminal.clear()?;
                        }
                    }
                }
                Some(Action::Open) => {
                    if let Some(entry) = self.selected() {
                        return Ok(LibraryOutcome::Open(entry.name.clone()));
                    }
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
        self.render_list(f, main);
        self.render_command(f, command);

        let line = match self.selected().map(|e| &e.cli) {
            _ if self.naming.is_some() => self
                .naming
                .as_ref()
                .map(|naming| naming.line(&self.store, status.width))
                .unwrap_or_default(),
            _ if self.message.is_some() => {
                let (text, error) = self.message.clone().unwrap_or_default();
                let color = if error { WARN } else { GOOD };
                Line::from(vec![
                    value(if error { " ! " } else { " ✔ " }, color),
                    Span::styled(
                        truncate(&text, (status.width as usize).saturating_sub(4)),
                        Style::new().fg(color),
                    ),
                ])
            }
            Some(Err(e)) => Line::from(vec![
                value(" ! ", WARN),
                Span::styled(
                    truncate(e, (status.width as usize).saturating_sub(4)),
                    Style::new().fg(WARN),
                ),
            ]),
            Some(Ok(_)) => Line::from(label(
                " enter or a double click puts it in the form, to change before the start · n starts from nothing",
            )),
            None => Line::from(label(" nothing saved yet · n starts a new test")),
        };
        f.render_widget(Paragraph::new(line), status);
        let chips: &[(&str, &str)] = if self.naming.is_some() {
            &[("enter", "copy"), ("esc", "cancel"), ("ctrl-c", "quit")]
        } else {
            &[
                ("enter", "open"),
                ("n", "new test"),
                ("c", "copy"),
                ("h", "history"),
                ("↑↓", "move"),
                ("F1", "keys"),
                ("esc", "quit"),
            ]
        };
        kit::footer(f, footer, chips);
        if self.show_help {
            help(
                f,
                area,
                &[
                    ("enter", "put the test in the setup form"),
                    ("n", "start from an empty form"),
                    ("c", "copy the test under another name"),
                    ("h", "its recorded runs, and what moved between them"),
                    ("↑ ↓ / j k", "move"),
                    ("F1 / ?", "this help"),
                    ("esc / q", "quit"),
                    ("click", "select a row; a second click opens it"),
                    ("wheel", "move"),
                ],
                &[
                    format!(
                        "Saved tests are the files in {}:",
                        self.store.tests_dir().display()
                    ),
                    "each a pepe.toml under a name, kept with the code.".into(),
                    "ctrl-s on the setup form saves one; pepe --load NAME".into(),
                    "runs it without a screen.".into(),
                    String::new(),
                    about_line(),
                ],
            );
        }
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
            lines.push(Line::styled("pick one", Style::new().fg(ACCENT).italic()));
            f.render_widget(Paragraph::new(lines), pet);
            mascot::keep(pet);
            Rect {
                y: main.y + 2,
                height: 4,
                ..main
            }
        } else {
            area
        };
        let count = match self.entries.len() {
            1 => "1 test".to_string(),
            n => format!("{n} tests"),
        };
        let lines = vec![
            Line::from(vec![
                Span::styled(" pepe ", Style::new().bg(ACCENT).fg(Color::Black).bold()),
                Span::styled(" library ", Style::new().bg(SELECTED).fg(Color::White)),
                Span::raw("  "),
                Span::styled("saved tests", Style::new().bold()),
                Span::styled(
                    format!("  {} · {count}", self.store.tests_dir().display()),
                    Style::new().fg(FAINT),
                ),
            ]),
            Line::from(label(truncate(
                "A test worked out once, kept with the code; enter opens it in the form.",
                area.width as usize,
            ))),
        ];
        let lines = if tall {
            let mut spaced = lines;
            spaced.insert(1, Line::raw(""));
            spaced
        } else {
            lines
        };
        f.render_widget(Paragraph::new(lines), area);
    }

    /// The list: name, mode, target, load and age, the columns the width
    /// allows, the selected row lit
    fn render_list(&self, f: &mut Frame, area: Rect) {
        let block = panel(caption("saved tests", true), None, RULE);
        let inner = block.inner(area);
        f.render_widget(block, area);
        let width = inner.width as usize;
        let visible = inner.height as usize;
        if width < 8 || visible == 0 {
            return;
        }
        let first = if self.selected >= visible {
            self.selected + 1 - visible
        } else {
            0
        };
        kit::hit_rows(inner, 0, first, self.entries.len().saturating_sub(first));
        let name_w = self
            .entries
            .iter()
            .map(|e| e.name.chars().count())
            .max()
            .unwrap_or(4)
            .clamp(4, NAME_MAX);
        let age_w = if width >= 50 { 12 } else { 0 };
        let mode_w = if width >= 60 { 4 } else { 0 };
        let load_w = if width >= 90 { 24 } else { 0 };
        let last_w = if width >= 120 { 30 } else { 0 };
        // The marker and a space, two between columns, one before the age
        let fixed = 2
            + name_w
            + 2
            + mode_w
            + (mode_w > 0) as usize * 2
            + load_w
            + (load_w > 0) as usize * 2
            + last_w
            + (last_w > 0) as usize * 2
            + age_w
            + (age_w > 0) as usize;
        let target_w = width.saturating_sub(fixed).max(6);
        let now = SystemTime::now();
        let pad = |text: &str, w: usize| {
            let text = truncate(text, w);
            format!("{text:<w$}", w = w)
        };

        let lines: Vec<Line> = self
            .entries
            .iter()
            .enumerate()
            .skip(first)
            .take(visible)
            .map(|(i, entry)| {
                let active = i == self.selected;
                let mut spans = vec![marker(active, true), Span::raw(" ")];
                spans.push(Span::styled(
                    pad(&entry.name, name_w),
                    if active {
                        Style::new().fg(ACCENT).bold()
                    } else {
                        Style::new().bold()
                    },
                ));
                spans.push(Span::raw("  "));
                if mode_w > 0 {
                    let mode = entry.cli.as_ref().map_or("", |cli| mode_of(cli));
                    spans.push(value(pad(mode, mode_w), FAINT));
                    spans.push(Span::raw("  "));
                }
                match &entry.cli {
                    Ok(cli) => spans.push(Span::raw(pad(&target_of(cli), target_w))),
                    Err(_) => spans.push(value(pad("can't be read", target_w), WARN)),
                }
                if load_w > 0 {
                    spans.push(Span::raw("  "));
                    let load = entry.cli.as_ref().map_or(String::new(), |cli| load_of(cli));
                    spans.push(value(pad(&load, load_w), LABEL));
                }
                if last_w > 0 {
                    spans.push(Span::raw("  "));
                    let last = entry
                        .last
                        .as_ref()
                        .map_or_else(|| "no run recorded".to_string(), |l| last_run_text(l, now));
                    spans.push(value(pad(&last, last_w), LABEL));
                }
                if age_w > 0 {
                    let age = entry.modified.map_or(String::new(), |t| ago(t, now));
                    spans.push(value(format!(" {age:>w$}", w = age_w), FAINT));
                }
                let line = Line::from(spans);
                if active {
                    line.style(Style::new().bg(SELECTED))
                } else {
                    line
                }
            })
            .collect();
        f.render_widget(Paragraph::new(lines), inner);
    }

    /// The command the selected test runs as, or why it can't
    fn render_command(&self, f: &mut Frame, area: Rect) {
        let block = panel(
            caption("command", false),
            Some(Line::from(Span::styled(
                " the same, as flags ",
                Style::new().fg(FAINT),
            ))),
            RULE,
        )
        .padding(Padding::horizontal(1));
        let text = match self.selected().map(|e| &e.cli) {
            Some(Ok(cli)) => Line::from(Span::styled(cli.command_line(), Style::new().fg(ACCENT))),
            Some(Err(e)) => Line::from(value(format!("not runnable: {e}"), WARN)),
            None => Line::from(label("nothing selected")),
        };
        f.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: false }).block(block),
            area,
        );
    }
}

/// A test's last recorded run in a few words: `319 rps · p99 645ms · 2 h ago`
fn last_run_text(last: &LastRun, now: SystemTime) -> String {
    let mut parts = Vec::new();
    if let Some(rps) = last.side.requests_per_second {
        parts.push(format!("{} rps", super::format::compact(rps)));
    } else if let Some(capacity) = last.side.capacity_per_second {
        parts.push(format!("holds {} rps", super::format::compact(capacity)));
    }
    if let Some(p99) = last.side.p99_ms {
        parts.push(format!("p99 {}", super::format::ms(p99)));
    }
    if last.side.failed_percent > 0.0 {
        parts.push(format!("{:.1}% failed", last.side.failed_percent));
    }
    parts.push(ago(last.when, now));
    parts.join(" · ")
}

/// What a test does, in a word
fn mode_of(cli: &Cli) -> &'static str {
    match &cli.command {
        Some(Command::Ramp(_)) => "ramp",
        Some(Command::Api(_)) => "api",
        Some(Command::Ping(_)) => "ping",
        _ => "run",
    }
}

/// Where it sends: the URL, or the spec
fn target_of(cli: &Cli) -> String {
    match &cli.command {
        Some(Command::Ramp(ramp)) if cli.url.is_empty() => ramp.url.clone(),
        Some(Command::Api(api)) => api.spec.clone(),
        Some(Command::Ping(ping)) => ping.targets.join(", "),
        _ => cli.url.clone(),
    }
}

/// Its load, in a few words
fn load_of(cli: &Cli) -> String {
    match &cli.command {
        Some(Command::Ramp(ramp)) => format!("{} → {} · {} a step", ramp.from, ramp.to, ramp.every),
        Some(Command::Ping(ping)) => {
            let count = if cli.number_from_file {
                format!("{} pings", cli.number)
            } else {
                "until stopped".to_string()
            };
            match &ping.slo {
                Some(slo) => format!("every {} · {count} · slo {slo}", ping.every),
                None => format!("every {} · {count}", ping.every),
            }
        }
        _ => match &cli.duration {
            Some(duration) => format!("{} at a time · {duration}", cli.concurrency),
            None => format!("{} at a time · {} requests", cli.concurrency, cli.number),
        },
    }
}

/// How long ago, in a reader's words: "just now", "5 min ago", "3 h ago",
/// "yesterday", "4 days ago", "3 weeks ago", "2 months ago"
pub fn ago(then: SystemTime, now: SystemTime) -> String {
    let secs = now.duration_since(then).unwrap_or_default().as_secs();
    match secs {
        0..60 => "just now".into(),
        60..3600 => format!("{} min ago", secs / 60),
        3600..86_400 => format!("{} h ago", secs / 3600),
        86_400..172_800 => "yesterday".into(),
        172_800..1_209_600 => format!("{} days ago", secs / 86_400),
        1_209_600..5_184_000 => format!("{} weeks ago", secs / 604_800),
        _ => format!("{} months ago", secs / 2_592_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::time::Duration;

    fn entry(name: &str, argv: &[&str]) -> Entry {
        Entry {
            name: name.into(),
            modified: Some(SystemTime::now() - Duration::from_secs(3 * 86_400)),
            cli: Ok(Box::new(Cli::parse_from(argv))),
            last: None,
        }
    }

    fn library() -> LibraryScreen {
        LibraryScreen::new(
            Store::at("/nowhere/.pepe"),
            vec![
                entry(
                    "api-health",
                    &[
                        "pepe",
                        "-c",
                        "50",
                        "-z",
                        "30s",
                        "https://api.example.com/health",
                    ],
                ),
                entry(
                    "climb",
                    &["pepe", "ramp", "--to", "200", "https://api.example.com/"],
                ),
                Entry {
                    name: "broken".into(),
                    modified: None,
                    cli: Err(".pepe/tests/broken.toml: expected a value".into()),
                    last: None,
                },
            ],
        )
    }

    fn press(s: &mut LibraryScreen, code: KeyCode) -> Option<Action> {
        s.key(KeyEvent::from(code))
    }

    #[test]
    fn the_keys_move_open_start_fresh_and_quit() {
        let mut s = library();
        assert!(press(&mut s, KeyCode::Up).is_none());
        assert_eq!(s.selected, 0);
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::Char('j'));
        press(&mut s, KeyCode::Down);
        assert_eq!(s.selected, 2, "stops at the last");
        press(&mut s, KeyCode::Home);
        assert_eq!(s.selected, 0);
        press(&mut s, KeyCode::End);
        assert_eq!(s.selected, 2);
        press(&mut s, KeyCode::Char('k'));
        assert!(matches!(press(&mut s, KeyCode::Enter), Some(Action::Open)));
        assert_eq!(s.selected().unwrap().name, "climb");
        assert!(matches!(
            press(&mut s, KeyCode::Char('n')),
            Some(Action::New)
        ));
        assert!(matches!(press(&mut s, KeyCode::Esc), Some(Action::Quit)));
        assert!(matches!(
            press(&mut s, KeyCode::Char('q')),
            Some(Action::Quit)
        ));
        // The help closes on any key, and ctrl-c still quits from it
        press(&mut s, KeyCode::F(1));
        assert!(s.show_help);
        assert!(press(&mut s, KeyCode::Enter).is_none());
        assert!(!s.show_help);
        press(&mut s, KeyCode::Char('?'));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(s.key(ctrl_c), Some(Action::Quit)));
    }

    #[test]
    fn the_first_readable_test_is_selected_to_begin_with() {
        let mut entries = library().entries;
        entries.rotate_right(1);
        assert_eq!(entries[0].name, "broken");
        let s = LibraryScreen::new(Store::at("/nowhere/.pepe"), entries);
        assert_eq!(s.selected().unwrap().name, "api-health");
    }

    fn draw(s: &LibraryScreen, terminal: &mut Terminal<TestBackend>) {
        terminal.draw(|f| theme::draw(f, |f| s.render(f))).unwrap();
    }

    /// Where row `index` was drawn in the last frame
    fn row_at(index: usize) -> Rect {
        kit::hits()
            .into_iter()
            .find(|(_, hit)| *hit == Hit::Row { list: 0, index })
            .map(|(area, _)| area)
            .unwrap_or_else(|| panic!("row {index} isn't on screen"))
    }

    fn click_at(area: Rect) -> MouseEvent {
        MouseEvent {
            kind: crossterm::event::MouseEventKind::Down(crossterm::event::MouseButton::Left),
            column: area.x + 1,
            row: area.y,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn a_click_selects_a_row_a_second_one_opens_it_and_the_wheel_moves() {
        let mut s = library();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        draw(&s, &mut terminal);
        let t0 = Instant::now();
        // The second row, once: selected, not opened
        assert!(s.mouse(click_at(row_at(1)), t0).is_none());
        assert_eq!(s.selected().unwrap().name, "climb");
        // Again, soon after: opened
        assert!(matches!(
            s.mouse(click_at(row_at(1)), t0 + Duration::from_millis(200)),
            Some(Action::Open)
        ));
        // Too late for a double click: just selected again
        assert!(s
            .mouse(click_at(row_at(1)), t0 + Duration::from_secs(2))
            .is_none());
        // Another row within the time: a first click on that one
        assert!(s
            .mouse(click_at(row_at(0)), t0 + Duration::from_millis(2100))
            .is_none());
        assert_eq!(s.selected().unwrap().name, "api-health");
        // Under the rows there's nothing to click
        let below = Rect {
            y: row_at(2).y + 3,
            ..row_at(2)
        };
        assert!(s.mouse(click_at(below), t0).is_none());
        assert_eq!(s.selected().unwrap().name, "api-health");
        // A key chip presses its key
        let (new_chip, _) = kit::hits()
            .into_iter()
            .find(|(_, hit)| *hit == Hit::Key(KeyEvent::from(KeyCode::Char('n'))))
            .expect("the n chip");
        assert!(matches!(s.mouse(click_at(new_chip), t0), Some(Action::New)));
        // The wheel
        let wheel = |kind| MouseEvent {
            kind,
            column: 10,
            row: 10,
            modifiers: KeyModifiers::NONE,
        };
        s.mouse(wheel(crossterm::event::MouseEventKind::ScrollDown), t0);
        s.mouse(wheel(crossterm::event::MouseEventKind::ScrollDown), t0);
        assert_eq!(s.selected, 2);
        s.mouse(wheel(crossterm::event::MouseEventKind::ScrollUp), t0);
        assert_eq!(s.selected, 1);
        // A click closes the help without selecting anything
        s.show_help = true;
        assert!(s.mouse(click_at(row_at(0)), t0).is_none());
        assert!(!s.show_help);
        assert_eq!(s.selected, 1);
    }

    #[test]
    fn a_click_on_a_scrolled_list_finds_the_right_row() {
        let entries: Vec<Entry> = (0..40)
            .map(|i| entry(&format!("test-{i:02}"), &["pepe", "https://x.io/"]))
            .collect();
        let mut s = LibraryScreen::new(Store::at("/nowhere/.pepe"), entries);
        s.selected = 39;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        draw(&s, &mut terminal);
        let first = kit::hits()
            .into_iter()
            .filter_map(|(_, hit)| match hit {
                Hit::Row { index, .. } => Some(index),
                _ => None,
            })
            .min()
            .unwrap();
        assert!(first > 0, "the list scrolled to the end");
        s.mouse(click_at(row_at(first)), Instant::now());
        assert_eq!(s.selected, first);
    }

    #[test]
    fn c_copies_the_selected_test_under_another_name() {
        let root = std::env::temp_dir().join(format!("pepe-library-copy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = Store::at(&root);
        store
            .save(
                "api-health",
                &Cli::parse_from(["pepe", "-c", "5", "https://x.io/"]),
            )
            .unwrap();
        std::fs::write(store.path("broken"), "nope = ").unwrap();
        let mut s = LibraryScreen::new(store.clone(), store.entries());
        assert_eq!(s.selected().unwrap().name, "api-health");
        press(&mut s, KeyCode::Char('c'));
        assert_eq!(
            s.naming.as_ref().map(|n| n.name()).as_deref(),
            Some("api-health-copy")
        );
        // Letters go into the name now
        press(&mut s, KeyCode::Char('2'));
        assert!(matches!(press(&mut s, KeyCode::Enter), Some(Action::Copy)));
        s.copy();
        assert!(s.naming.is_none());
        assert_eq!(s.selected().unwrap().name, "api-health-copy2");
        assert_eq!(
            store.load("api-health-copy2").unwrap().1.concurrency,
            Some(5)
        );
        assert!(matches!(&s.message, Some((m, false)) if m.contains("saved")));
        // A copy under a taken name asks first
        press(&mut s, KeyCode::Char('c'));
        s.naming.as_mut().unwrap().input = kit::TextInput::new("api-health");
        s.copy();
        assert!(s.naming.is_some(), "still asking");
        s.copy();
        assert!(s.naming.is_none());
        // The broken file can't be copied
        s.selected = s.entries.iter().position(|e| e.name == "broken").unwrap();
        press(&mut s, KeyCode::Char('c'));
        assert!(s.naming.is_none());
        assert!(matches!(&s.message, Some((m, true)) if m.contains("can't be read")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_empty_library_opens_nothing() {
        let mut s = LibraryScreen::new(Store::at("/nowhere/.pepe"), Vec::new());
        assert!(press(&mut s, KeyCode::Enter).is_none());
        assert!(matches!(
            press(&mut s, KeyCode::Char('n')),
            Some(Action::New)
        ));
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| s.render(f)).unwrap();
        assert!(terminal.backend().to_string().contains("nothing saved yet"));
    }

    #[test]
    fn rows_say_what_each_test_does() {
        let s = library();
        let cli = s.entries[0].cli.as_ref().unwrap();
        assert_eq!(mode_of(cli), "run");
        assert_eq!(target_of(cli), "https://api.example.com/health");
        assert_eq!(load_of(cli), "50 at a time · 30s");
        let ramp = s.entries[1].cli.as_ref().unwrap();
        assert_eq!(mode_of(ramp), "ramp");
        assert_eq!(target_of(ramp), "https://api.example.com/");
        assert!(
            load_of(ramp).starts_with("10 → 200 · "),
            "{}",
            load_of(ramp)
        );
    }

    #[test]
    fn ages_read_like_a_person_says_them() {
        let now = SystemTime::now();
        let back = |secs: u64| ago(now - Duration::from_secs(secs), now);
        assert_eq!(back(5), "just now");
        assert_eq!(back(300), "5 min ago");
        assert_eq!(back(3 * 3600), "3 h ago");
        assert_eq!(back(30 * 3600), "yesterday");
        assert_eq!(back(4 * 86_400), "4 days ago");
        assert_eq!(back(20 * 86_400), "2 weeks ago");
        assert_eq!(back(100 * 86_400), "3 months ago");
        assert_eq!(ago(now + Duration::from_secs(60), now), "just now");
    }

    #[test]
    fn renders_at_many_sizes_with_each_row_selected() {
        let mut s = library();
        for (w, h) in [(40, 12), (80, 24), (120, 40), (220, 60)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            for selected in 0..s.entries.len() {
                s.selected = selected;
                terminal.draw(|f| s.render(f)).unwrap();
                // The selected row is always in view, however few rows fit
                let screen = terminal.backend().to_string();
                let name = s.entries[selected].name.as_str();
                assert!(screen.contains(name), "{w}x{h}, {name}:\n{screen}");
            }
            let screen = terminal.backend().to_string();
            assert!(screen.contains("can't be read"), "{w}x{h}:\n{screen}");
            s.show_help = true;
            terminal.draw(|f| s.render(f)).unwrap();
            s.show_help = false;
        }
        // The wide screen shows every column
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        s.selected = 0;
        terminal.draw(|f| s.render(f)).unwrap();
        let screen = terminal.backend().to_string();
        for text in ["run", "ramp", "50 at a time · 30s", "3 days ago", "-c 50"] {
            assert!(screen.contains(text), "missing {text}:\n{screen}");
        }
    }

    #[test]
    fn a_long_list_keeps_the_selected_row_in_view() {
        let entries: Vec<Entry> = (0..40)
            .map(|i| entry(&format!("test-{i:02}"), &["pepe", "https://x.io/"]))
            .collect();
        let mut s = LibraryScreen::new(Store::at("/nowhere/.pepe"), entries);
        s.selected = 39;
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| s.render(f)).unwrap();
        let screen = terminal.backend().to_string();
        assert!(
            screen.contains("test-39") && !screen.contains("test-00"),
            "{screen}"
        );
    }
}
