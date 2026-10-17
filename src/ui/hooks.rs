//! The webhook page: where the run is said, and when. F3 opens it over
//! any screen. On the setup form it edits the run about to start; while a
//! run goes, a change is sent to from then on. What it leaves is the run's
//! from then on: a restart, ctrl-s and the command to run it again keep it.

use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
    Frame,
};

use super::kit::{chips, fitting, marker, FAINT, FIELD, SELECTED};
use super::view::{label, truncate, ACCENT, BAD, GOOD, LABEL};
use crate::webhook::{self, Event, Format, Webhook};

/// The line every screen's `?` help has for this page
pub(super) const HELP: (&str, &str) = ("F3", "webhooks: Slack, Discord, Teams, any URL");

const WIDTH: u16 = 80;
const LABELS: usize = 10;

/// A line of the page
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Url(usize),
    Events(usize),
    Format(usize),
    Every(usize),
    Mention(usize),
    Add,
}

impl Row {
    fn hook(self) -> Option<usize> {
        match self {
            Row::Url(i) | Row::Events(i) | Row::Format(i) | Row::Every(i) | Row::Mention(i) => {
                Some(i)
            }
            Row::Add => None,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Row::Url(_) => "URL",
            Row::Events(_) => "When",
            Row::Format(_) => "Format",
            Row::Every(_) => "Every",
            Row::Mention(_) => "Mention",
            Row::Add => "",
        }
    }

    fn about(self) -> &'static str {
        match self {
            Row::Url(_) => "where it goes; ${NAME} is read from the environment · del removes it",
            Row::Events(_) => "1-5 or ← → and space: what is sent; the end when none is ticked",
            Row::Format(_) => "how it's laid out; auto goes by the URL",
            Row::Every(_) => "how often progress goes out: 30s, 2m (5s at least)",
            Row::Mention(_) => "put before a failed run's message: <!here>, <@U012ABC>, @oncall",
            Row::Add => "a Slack, Discord, Teams or Google Chat webhook, or any URL for the JSON",
        }
    }

    /// Typed rather than stepped through
    fn typed(self) -> bool {
        matches!(
            self,
            Row::Url(_) | Row::Every(_) | Row::Mention(_) | Row::Add
        )
    }
}

/// What a key did to the page
#[derive(Debug, Default, PartialEq)]
struct Did {
    changed: bool,
    close: bool,
}

struct Page {
    hooks: Vec<Webhook>,
    run: webhook::Run,
    at: usize,
    /// Which event the cursor is on, on a When line
    event: usize,
    editing: Option<String>,
    message: Option<(String, bool)>,
    /// What a test send said, when it answers
    tested: Arc<Mutex<Option<(String, bool)>>>,
}

impl Page {
    fn open(hooks: Vec<Webhook>, run: webhook::Run) -> Self {
        Self {
            hooks,
            run,
            at: 0,
            event: 0,
            editing: None,
            message: None,
            tested: Arc::default(),
        }
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for i in 0..self.hooks.len() {
            rows.extend([
                Row::Url(i),
                Row::Events(i),
                Row::Format(i),
                Row::Every(i),
                Row::Mention(i),
            ]);
        }
        rows.push(Row::Add);
        rows
    }

    fn row(&self) -> Row {
        self.rows().get(self.at).copied().unwrap_or(Row::Add)
    }

    fn move_by(&mut self, by: i64) {
        let last = self.rows().len() as i64 - 1;
        self.at = (self.at as i64 + by).clamp(0, last) as usize;
    }

    fn value(&self, row: Row) -> Option<String> {
        let hook = &self.hooks[row.hook()?];
        match row {
            Row::Url(_) => Some(hook.url.clone()),
            Row::Every(_) => hook.every.clone(),
            Row::Mention(_) => hook.mention.clone(),
            _ => None,
        }
    }

    /// Put what was typed on `row`
    fn set(&mut self, row: Row, text: &str) -> Result<(), String> {
        let text = text.trim();
        match row {
            Row::Add | Row::Url(_) if text.is_empty() => {
                return Err("a webhook needs its URL".into())
            }
            Row::Add | Row::Url(_) if !text.starts_with("${") => {
                let url = reqwest::Url::parse(text).map_err(|e| format!("not a URL: {e}"))?;
                if !matches!(url.scheme(), "http" | "https") {
                    return Err("http:// or https:// only".into());
                }
            }
            Row::Every(_) if !text.is_empty() => {
                crate::cli::Cli::parse_duration_for("every", text).map_err(|e| e.to_string())?;
            }
            _ => {}
        }
        let optional = (!text.is_empty()).then(|| text.to_string());
        match row {
            Row::Add => {
                self.hooks.push(Webhook::to(text));
                self.at = (self.hooks.len() - 1) * 5;
            }
            Row::Url(i) => self.hooks[i].url = text.to_string(),
            Row::Every(i) => self.hooks[i].every = optional,
            Row::Mention(i) => self.hooks[i].mention = optional,
            _ => {}
        }
        Ok(())
    }

    fn toggle(&mut self, i: usize, event: Event) {
        let hook = &mut self.hooks[i];
        let mut on = hook.events();
        match on.iter().position(|e| *e == event) {
            Some(at) => {
                on.remove(at);
            }
            None => on.push(event),
        }
        on.sort_by_key(|e| Event::ALL.iter().position(|a| a == e));
        // Only the end is what saying nothing means
        hook.on = if on == [Event::End] { Vec::new() } else { on };
    }

    fn step_format(&mut self, i: usize, by: i64) {
        let all = Format::ALL;
        let at = all
            .iter()
            .position(|f| *f == self.hooks[i].format)
            .unwrap_or(0) as i64;
        self.hooks[i].format = all[(at + by).rem_euclid(all.len() as i64) as usize];
    }

    /// A test message to the selected hook, answered on the page
    fn test(&mut self) {
        let Some(i) = self.row().hook() else { return };
        let hook = self.hooks[i].clone();
        let run = self.run.clone();
        let tested = self.tested.clone();
        self.message = Some((format!("sending a test to {}…", hook.masked_url()), false));
        if let Ok(mut t) = tested.lock() {
            *t = None;
        }
        tokio::spawn(async move {
            let said = match webhook::try_hook(&hook, &run).await {
                Ok(()) => (format!("sent: {} answered", hook.masked_url()), false),
                Err(e) => (e, true),
            };
            if let Ok(mut t) = tested.lock() {
                *t = Some(said);
            }
        });
    }

    fn key(&mut self, key: KeyEvent) -> Did {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        let row = self.row();
        let changed = Did {
            changed: true,
            close: false,
        };
        if let Some(text) = &mut self.editing {
            match key.code {
                KeyCode::Esc => self.editing = None,
                KeyCode::Enter => {
                    let text = text.clone();
                    let before = self.hooks.clone();
                    match self.set(row, &text) {
                        Ok(()) => {
                            self.editing = None;
                            self.message = None;
                            if self.hooks != before {
                                return changed;
                            }
                        }
                        Err(e) => self.message = Some((e, true)),
                    }
                }
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char('u') if ctrl => text.clear(),
                KeyCode::Char(c) if !ctrl => text.push(c),
                _ => {}
            }
            return Did::default();
        }
        self.message = None;
        let before = self.hooks.clone();
        match (key.code, row) {
            (KeyCode::Esc | KeyCode::F(3) | KeyCode::Char('q'), _) => {
                return Did {
                    changed: false,
                    close: true,
                }
            }
            (KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab, _) => self.move_by(-1),
            (KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab, _) => self.move_by(1),
            (KeyCode::PageUp, _) => self.move_by(-5),
            (KeyCode::PageDown, _) => self.move_by(5),
            (KeyCode::Home, _) => self.at = 0,
            (KeyCode::End, _) => self.move_by(i64::MAX / 2),
            (
                KeyCode::Char('t'),
                Row::Url(_) | Row::Events(_) | Row::Format(_) | Row::Every(_) | Row::Mention(_),
            ) => self.test(),
            (KeyCode::Char(c @ '1'..='5'), Row::Events(i)) => {
                let at = c as usize - '1' as usize;
                self.event = at;
                self.toggle(i, Event::ALL[at]);
            }
            (KeyCode::Left | KeyCode::Char('h'), Row::Events(_)) => {
                self.event = self.event.saturating_sub(1)
            }
            (KeyCode::Right | KeyCode::Char('l'), Row::Events(_)) => {
                self.event = (self.event + 1).min(Event::ALL.len() - 1)
            }
            (KeyCode::Char(' ') | KeyCode::Enter, Row::Events(i)) => {
                self.toggle(i, Event::ALL[self.event])
            }
            (KeyCode::Left | KeyCode::Char('h'), Row::Format(i)) => self.step_format(i, -1),
            (
                KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') | KeyCode::Enter,
                Row::Format(i),
            ) => self.step_format(i, 1),
            (KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('d'), Row::Url(i)) => {
                self.hooks.remove(i);
                self.move_by(0);
                self.at = self.at.min(i * 5);
            }
            (KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('d'), Row::Events(i)) => {
                self.hooks[i].on.clear()
            }
            (KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('d'), Row::Format(i)) => {
                self.hooks[i].format = Format::Auto
            }
            (KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('d'), Row::Every(i)) => {
                self.hooks[i].every = None
            }
            (KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('d'), Row::Mention(i)) => {
                self.hooks[i].mention = None
            }
            (KeyCode::Enter | KeyCode::Right | KeyCode::Char('l'), r) if r.typed() => {
                self.editing = Some(self.value(r).unwrap_or_default())
            }
            // On the last line, typing starts a URL
            (KeyCode::Char(c), Row::Add) if !ctrl => self.editing = Some(c.to_string()),
            _ => {}
        }
        if self.hooks != before {
            return changed;
        }
        Did::default()
    }

    fn paste(&mut self, pasted: &str) {
        let clean: String = pasted.trim().chars().filter(|c| !c.is_control()).collect();
        if let Some(text) = &mut self.editing {
            text.push_str(&clean);
            return;
        }
        let row = self.row();
        if !row.typed() {
            return;
        }
        // A URL pasted replaces the one there; the rest add to theirs
        let mut text = match row {
            Row::Add | Row::Url(_) => String::new(),
            _ => self.value(row).unwrap_or_default(),
        };
        text.push_str(&clean);
        self.editing = Some(text);
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    fn line(&self, row: Row, selected: bool, width: usize) -> Line<'static> {
        let room = width.saturating_sub(LABELS + 3);
        let mut spans = vec![
            marker(selected, true),
            Span::styled(
                format!(" {:<LABELS$}", row.label()),
                Style::new().fg(if selected { Color::Reset } else { LABEL }),
            ),
        ];
        match (&self.editing, selected) {
            (Some(text), true) => {
                let chars: Vec<char> = text.chars().collect();
                let from = chars.len().saturating_sub(room.saturating_sub(2));
                let shown: String = chars[from..].iter().collect();
                let pad = room.saturating_sub(shown.chars().count() + 1);
                spans.push(Span::styled(shown, Style::new().bg(FIELD)));
                spans.push(Span::styled(" ", Style::new().bg(ACCENT)));
                spans.push(Span::styled(" ".repeat(pad), Style::new().bg(FIELD)));
            }
            _ => match row {
                Row::Add => spans.push(Span::styled(
                    "+ add a webhook: type or paste its URL",
                    Style::new().fg(if selected { ACCENT } else { FAINT }),
                )),
                Row::Url(i) => {
                    let hook = &self.hooks[i];
                    let wrong = hook.resolve().err();
                    spans.push(Span::styled(
                        truncate(&hook.masked_url(), room),
                        Style::new()
                            .bold()
                            .fg(if wrong.is_some() { BAD } else { Color::Reset }),
                    ));
                }
                Row::Events(i) => {
                    let on = self.hooks[i].events();
                    for (n, event) in Event::ALL.iter().enumerate() {
                        let ticked = on.contains(event);
                        let style = if selected && n == self.event {
                            Style::new().bg(ACCENT).fg(Color::Black)
                        } else if ticked {
                            Style::new().bold()
                        } else {
                            Style::new().fg(FAINT)
                        };
                        spans.push(Span::styled(
                            format!(
                                "{}{} {}",
                                if ticked { "●" } else { "○" },
                                n + 1,
                                event.name()
                            ),
                            style,
                        ));
                        spans.push(Span::raw(" "));
                    }
                }
                Row::Format(i) => {
                    let hook = &self.hooks[i];
                    let text = match hook.format {
                        Format::Auto => {
                            let url = hook.resolve().map(|_| hook.url.clone()).unwrap_or_default();
                            format!("auto · {}", Format::Auto.of(&url).name())
                        }
                        f => f.name().to_string(),
                    };
                    if selected {
                        spans.push(Span::styled("‹ ", Style::new().fg(ACCENT)));
                        spans.push(Span::styled(text, Style::new().bold()));
                        spans.push(Span::styled(" ›", Style::new().fg(ACCENT)));
                    } else {
                        spans.push(Span::styled(text, Style::new().bold()));
                    }
                }
                Row::Every(i) => {
                    let hook = &self.hooks[i];
                    let progress = hook.events().contains(&Event::Progress);
                    match &hook.every {
                        Some(every) => spans.push(Span::styled(every.clone(), Style::new().bold())),
                        None => spans.push(Span::styled(
                            if progress {
                                "1m"
                            } else {
                                "1m, once progress is ticked"
                            },
                            Style::new().fg(FAINT),
                        )),
                    }
                }
                Row::Mention(i) => match &self.hooks[i].mention {
                    Some(m) => spans.push(Span::styled(m.clone(), Style::new().bold())),
                    None => spans.push(Span::styled("nobody", Style::new().fg(FAINT))),
                },
            },
        }
        let mut line = Line::from(spans);
        if selected {
            let used = line.width();
            line.spans
                .push(Span::raw(" ".repeat(width.saturating_sub(used))));
            line = line.style(Style::new().bg(SELECTED));
        }
        line
    }

    fn lines(&self, width: usize) -> (Vec<Line<'static>>, usize) {
        let mut lines = Vec::new();
        let mut selected = 0;
        for (index, row) in self.rows().into_iter().enumerate() {
            if let Row::Url(i) = row {
                if i > 0 {
                    lines.push(Line::raw(""));
                }
                lines.push(Line::from(Span::styled(
                    format!("  WEBHOOK {}", i + 1),
                    Style::new().fg(LABEL).bold(),
                )));
            }
            if row == Row::Add && !self.hooks.is_empty() {
                lines.push(Line::raw(""));
            }
            if index == self.at {
                selected = lines.len();
            }
            lines.push(self.line(row, index == self.at, width));
        }
        (lines, selected)
    }

    fn render(&mut self, f: &mut Frame) -> Rect {
        if let Some(said) = self.tested.lock().ok().and_then(|mut t| t.take()) {
            self.message = Some(said);
        }
        let area = f.area();
        let width = WIDTH.min(area.width);
        let inner_width = width.saturating_sub(2) as usize;
        let (lines, selected) = self.lines(inner_width);
        let height = (lines.len() as u16 + 6).min(area.height);
        let [popup] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(popup);
        f.render_widget(Clear, popup);
        let place = if webhook::live() {
            " this run, from now on "
        } else {
            " the next run "
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ACCENT))
            .title(Span::styled(" webhooks ", Style::new().fg(ACCENT).bold()))
            .title_top(Line::from(label(place)).right_aligned());
        let inner = block.inner(popup);
        f.render_widget(block, popup);
        let [list, _, about, message, keys] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        let room = list.height as usize;
        let top = (selected + 2)
            .saturating_sub(room)
            .min(lines.len().saturating_sub(room));
        let shown: Vec<Line> = lines.into_iter().skip(top).take(room).collect();
        f.render_widget(Paragraph::new(shown), list);
        f.render_widget(
            Paragraph::new(Line::from(label(format!("  {}", self.row().about())))),
            about,
        );
        let wrong = self
            .row()
            .hook()
            .and_then(|i| self.hooks.get(i))
            .and_then(|h| h.resolve().err());
        let note = match (&self.message, wrong) {
            (Some((text, bad)), _) => Line::from(Span::styled(
                format!("  {}", truncate(text, inner_width.saturating_sub(2))),
                Style::new().fg(if *bad { BAD } else { GOOD }),
            )),
            (None, Some(wrong)) => Line::from(Span::styled(
                format!("  {}", truncate(&wrong, inner_width.saturating_sub(2))),
                Style::new().fg(BAD),
            )),
            (None, None) => Line::from(Span::styled(
                "  ctrl-s on a screen keeps them with the test · [[webhook]] in pepe.toml",
                Style::new().fg(FAINT),
            )),
        };
        f.render_widget(Paragraph::new(note), message);
        let pairs: &[(&str, &str)] = if self.editing.is_some() {
            &[("enter", "keep"), ("esc", "leave as it was")]
        } else {
            match self.row() {
                Row::Add => &[("↑↓", "move"), ("type", "a URL"), ("esc", "close")],
                Row::Events(_) => &[
                    ("1-5", "tick"),
                    ("← →", "pick"),
                    ("space", "tick"),
                    ("t", "test"),
                    ("esc", "close"),
                ],
                Row::Format(_) => &[("← →", "change"), ("t", "test"), ("esc", "close")],
                Row::Url(_) => &[
                    ("↑↓", "move"),
                    ("enter", "edit"),
                    ("t", "test"),
                    ("del", "remove"),
                    ("esc", "close"),
                ],
                _ => &[
                    ("↑↓", "move"),
                    ("enter", "edit"),
                    ("t", "test"),
                    ("del", "usual"),
                    ("esc", "close"),
                ],
            }
        };
        f.render_widget(Paragraph::new(chips(&fitting(pairs, inner.width))), keys);
        popup
    }
}

// ─── The page over whichever screen is up ────────────────────────────────────

thread_local! {
    static PAGE: RefCell<Option<Page>> = const { RefCell::new(None) };
}

/// Keys come here before the screen's (see `settings::key`). True when the
/// key was the page's: F3 opens it, and while it's open every key but
/// ctrl-c is its own.
pub(super) fn key(key: KeyEvent) -> bool {
    let ctrl =
        key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    if ctrl && key.code == KeyCode::Char('c') {
        return false;
    }
    PAGE.with(|page| {
        let mut page = page.borrow_mut();
        let Some(open) = page.as_mut() else {
            let opens = key.code == KeyCode::F(3);
            if opens {
                let (hooks, run) = webhook::editable();
                *page = Some(Page::open(hooks, run));
            }
            return opens;
        };
        let did = open.key(key);
        if did.changed {
            if let Err(e) = webhook::edit(open.hooks.clone()) {
                open.message = Some((e, true));
            } else if webhook::live() {
                open.message = Some(("sent to from now on".into(), false));
            }
        }
        if did.close {
            *page = None;
        }
        true
    })
}

pub(super) fn paste(text: &str) -> bool {
    PAGE.with(|page| match page.borrow_mut().as_mut() {
        Some(open) => {
            open.paste(text);
            true
        }
        None => false,
    })
}

pub(super) fn is_open() -> bool {
    PAGE.with(|page| page.borrow().is_some())
}

pub(super) fn render(f: &mut Frame) -> Option<Rect> {
    PAGE.with(|page| page.borrow_mut().as_mut().map(|open| open.render(f)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn press(page: &mut Page, code: KeyCode) -> Did {
        page.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn a_hook_is_added_ticked_laid_out_and_removed() {
        let mut page = Page::open(Vec::new(), webhook::Run::default());
        assert_eq!(page.row(), Row::Add);
        for c in "https://hooks.slack.com/services/T/B/x".chars() {
            press(&mut page, KeyCode::Char(c));
        }
        assert!(press(&mut page, KeyCode::Enter).changed);
        assert_eq!(page.hooks[0].url, "https://hooks.slack.com/services/T/B/x");
        assert_eq!(page.row(), Row::Url(0));
        // When: 1 is start, 5 is fail; ticking end alone is saying nothing
        press(&mut page, KeyCode::Down);
        press(&mut page, KeyCode::Char('1'));
        press(&mut page, KeyCode::Char('5'));
        assert_eq!(page.hooks[0].on, [Event::Start, Event::End, Event::Fail]);
        press(&mut page, KeyCode::Char('1'));
        press(&mut page, KeyCode::Char('5'));
        assert!(page.hooks[0].on.is_empty());
        // Format steps; every is checked
        press(&mut page, KeyCode::Down);
        press(&mut page, KeyCode::Right);
        assert_eq!(page.hooks[0].format, Format::Json);
        press(&mut page, KeyCode::Down);
        press(&mut page, KeyCode::Enter);
        for c in "soon".chars() {
            press(&mut page, KeyCode::Char(c));
        }
        assert!(!press(&mut page, KeyCode::Enter).changed);
        assert!(page.message.as_ref().is_some_and(|(_, bad)| *bad));
        page.editing = Some("30s".into());
        assert!(press(&mut page, KeyCode::Enter).changed);
        assert_eq!(page.hooks[0].every.as_deref(), Some("30s"));
        // A URL that isn't one is refused
        page.at = page.rows().len() - 1;
        page.editing = Some("not a url".into());
        press(&mut page, KeyCode::Enter);
        assert_eq!(page.hooks.len(), 1);
        page.editing = None;
        // del on its URL removes it
        page.at = 0;
        assert!(press(&mut page, KeyCode::Char('d')).changed);
        assert!(page.hooks.is_empty());
        assert!(press(&mut page, KeyCode::Esc).close);
    }

    #[test]
    fn it_draws_the_hooks_masked() {
        let mut page = Page::open(
            vec![Webhook {
                on: vec![Event::Start, Event::End],
                ..Webhook::to("https://hooks.slack.com/services/T/B/secret")
            }],
            webhook::Run::default(),
        );
        let mut terminal = Terminal::new(TestBackend::new(80, 20)).unwrap();
        terminal
            .draw(|f| {
                page.render(f);
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("https://hooks.slack.com/••••"), "{text}");
        assert!(!text.contains("secret"));
        assert!(
            text.contains("●1 start") && text.contains("○2 progress"),
            "{text}"
        );
        assert!(text.contains("auto · slack"), "{text}");
    }
}
