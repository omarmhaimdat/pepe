//! The export page: the run as it stands, in the shape someone needs it.
//! F4 opens it over the dashboard, the ramp and the ping screens, during a
//! run or after: a page, the report, a summary, the text, the numbers;
//! the summary copied to paste; the verdict sent to the webhooks now.

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
use crate::export::{Format, Snapshot};

/// The line every run screen's `?` help has for this page
pub(super) const HELP: (&str, &str) = ("F4", "export: page, report, summary, CSV, share");

const WIDTH: u16 = 84;

/// Where "send to the webhooks" sends, and as which run
#[derive(Clone, Default)]
pub struct Share {
    pub hooks: Vec<crate::webhook::Webhook>,
    pub run: crate::webhook::Run,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Row {
    Save(Format),
    All,
    Copy,
    Send,
    Where,
    Open,
}

struct Page {
    snap: Snapshot,
    /// What the page is of, along its top
    title: String,
    share: Share,
    at: usize,
    /// The file name without its extension, `exports/checkout-2026-…`
    stem: String,
    editing: Option<String>,
    open_after: bool,
    message: Option<(String, bool)>,
    /// What sending answered, for the next draw
    sent: Arc<Mutex<Option<(String, bool)>>>,
}

impl Page {
    fn ping(&self) -> bool {
        self.snap.report["mode"] == "ping"
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = Format::ALL
            .into_iter()
            .filter(|f| !(*f == Format::Html && self.ping()))
            .map(Row::Save)
            .collect();
        rows.push(Row::All);
        rows.push(Row::Copy);
        if !self.share.hooks.is_empty() {
            rows.push(Row::Send);
        }
        rows.extend([Row::Where, Row::Open]);
        rows
    }

    fn row(&self) -> Row {
        self.rows().get(self.at).copied().unwrap_or(Row::Where)
    }

    fn path(&self, format: Format) -> std::path::PathBuf {
        std::path::PathBuf::from(format!("{}.{}", self.stem, format.ext()))
    }

    fn save(&mut self, formats: &[Format]) {
        let mut saved = Vec::new();
        for format in formats {
            match crate::export::save(*format, &self.snap, &self.path(*format)) {
                Ok(path) => saved.push((*format, path)),
                Err(e) => {
                    self.message = Some((e, true));
                    return;
                }
            }
        }
        let opened = self.open_after
            && saved
                .iter()
                .find(|(f, _)| *f == Format::Html)
                .is_some_and(|(_, p)| crate::slack::open_browser(&p.display().to_string()));
        let shown = match saved.as_slice() {
            [(_, path)] => path.display().to_string(),
            _ => format!(
                "{}.{{{}}}",
                self.stem,
                saved
                    .iter()
                    .map(|(f, _)| f.ext())
                    .collect::<Vec<_>>()
                    .join(",")
            ),
        };
        self.message = Some((
            format!("✓ saved {shown}{}", if opened { " · opened" } else { "" }),
            false,
        ));
    }

    fn copy(&mut self) {
        let summary = crate::export::markdown(&self.snap);
        self.message = Some(if crate::slack::copy(&summary) {
            (
                "✓ the summary is copied: paste it in a PR, an issue, a chat".into(),
                false,
            )
        } else {
            (
                "no clipboard program here: save the .md instead".into(),
                true,
            )
        });
    }

    fn send(&mut self) {
        let hooks = self.share.hooks.clone();
        let run = self.share.run.clone();
        let report = self.snap.report.clone();
        let x = &self.snap.extras;
        let ending = crate::webhook::Ending {
            checks: x.checks.clone(),
            failed: x.failed.clone(),
            compared: x
                .compared
                .as_ref()
                .map(|(c, when)| crate::webhook::Compared {
                    verdict: c.verdict.headline().to_string(),
                    regression: c.regression,
                    summary: String::new(),
                    when: format!("against the run {when}"),
                }),
        };
        let sent = self.sent.clone();
        self.message = Some(("sending…".into(), false));
        tokio::spawn(async move {
            let said = match crate::webhook::share(hooks, run, report, ending).await {
                Ok(n) => (
                    format!("✓ sent to {n} webhook{}", if n == 1 { "" } else { "s" }),
                    false,
                ),
                Err(e) => (e, true),
            };
            if let Ok(mut s) = sent.lock() {
                *s = Some(said);
            }
        });
    }

    /// True when the page closes
    fn key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        if let Some(text) = &mut self.editing {
            match key.code {
                KeyCode::Esc => self.editing = None,
                KeyCode::Enter => {
                    let text = text.trim().trim_end_matches(".html").to_string();
                    if !text.is_empty() {
                        self.stem = text;
                    }
                    self.editing = None;
                }
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char('u') if ctrl => text.clear(),
                KeyCode::Char(c) if !ctrl => text.push(c),
                _ => {}
            }
            return false;
        }
        self.message = None;
        let last = self.rows().len() - 1;
        match key.code {
            KeyCode::Esc | KeyCode::F(4) | KeyCode::Char('q') => return true,
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                self.at = self.at.saturating_sub(1)
            }
            KeyCode::Down | KeyCode::Char('j') => self.at = (self.at + 1).min(last),
            KeyCode::Tab => {
                self.at = self
                    .rows()
                    .iter()
                    .position(|r| *r == Row::Where)
                    .unwrap_or(0);
                self.editing = Some(self.stem.clone());
            }
            KeyCode::Enter | KeyCode::Char(' ') => match self.row() {
                Row::Save(format) => self.save(&[format]),
                Row::All => {
                    let all: Vec<Format> = self
                        .rows()
                        .into_iter()
                        .filter_map(|r| match r {
                            Row::Save(f) => Some(f),
                            _ => None,
                        })
                        .collect();
                    self.save(&all)
                }
                Row::Copy => self.copy(),
                Row::Send => self.send(),
                Row::Where => self.editing = Some(self.stem.clone()),
                Row::Open => self.open_after = !self.open_after,
            },
            KeyCode::Left | KeyCode::Right if self.row() == Row::Open => {
                self.open_after = !self.open_after
            }
            _ => {}
        }
        false
    }

    fn line(&self, row: Row, selected: bool, width: usize) -> Line<'static> {
        let mut spans = vec![marker(selected, true)];
        let cell = |text: String, w: usize| format!(" {text:<w$}");
        match row {
            Row::Save(format) => {
                spans.push(Span::styled(
                    cell(format.label().into(), 12),
                    Style::new().bold(),
                ));
                spans.push(Span::styled(
                    cell(format!(".{}", format.ext()), 6),
                    Style::new().fg(ACCENT),
                ));
                spans.push(Span::styled(
                    format!(" {}", format.about()),
                    Style::new().fg(LABEL),
                ));
            }
            Row::All => {
                spans.push(Span::styled(
                    cell("Everything".into(), 12),
                    Style::new().bold(),
                ));
                spans.push(Span::styled(
                    format!(
                        " every one of them, side by side{}",
                        if self.ping() { "" } else { ", the page opened" }
                    ),
                    Style::new().fg(LABEL),
                ));
            }
            Row::Copy => {
                spans.push(Span::styled(
                    cell("Copy the summary".into(), 26),
                    Style::new().bold(),
                ));
                spans.push(Span::styled(
                    " Markdown, ready to paste",
                    Style::new().fg(LABEL),
                ));
            }
            Row::Send => {
                let hooks: Vec<String> = self.share.hooks.iter().map(|h| h.masked_url()).collect();
                spans.push(Span::styled(
                    cell("Send to the webhooks".into(), 26),
                    Style::new().bold(),
                ));
                spans.push(Span::styled(
                    format!(" {}", truncate(&hooks.join(", "), width.saturating_sub(32))),
                    Style::new().fg(LABEL),
                ));
            }
            Row::Where => {
                spans.push(Span::styled(
                    cell("Where".into(), 8),
                    Style::new().fg(LABEL).bold(),
                ));
                match (&self.editing, selected) {
                    (Some(text), true) => {
                        spans.push(Span::styled(format!(" {text}"), Style::new().bg(FIELD)));
                        spans.push(Span::styled(" ", Style::new().bg(ACCENT)));
                    }
                    _ => spans.push(Span::styled(format!(" {}", self.stem), Style::new().bold())),
                }
                spans.push(Span::styled("  .ext added", Style::new().fg(FAINT)));
            }
            Row::Open => {
                spans.push(Span::styled(
                    cell("Open".into(), 8),
                    Style::new().fg(LABEL).bold(),
                ));
                let word = if self.open_after { "yes" } else { "no" };
                if selected {
                    spans.push(Span::styled(" ‹ ", Style::new().fg(ACCENT)));
                    spans.push(Span::styled(word, Style::new().bold()));
                    spans.push(Span::styled(" ›", Style::new().fg(ACCENT)));
                } else {
                    spans.push(Span::styled(format!(" {word}"), Style::new().bold()));
                }
                spans.push(Span::styled(
                    "   the page opens in your browser once saved",
                    Style::new().fg(FAINT),
                ));
            }
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
        let heading = |t: &str| {
            Line::from(Span::styled(
                format!("  {t}"),
                Style::new().fg(LABEL).bold(),
            ))
        };
        for (i, row) in self.rows().into_iter().enumerate() {
            match row {
                Row::Save(_) if i == 0 => lines.push(heading("SAVE")),
                Row::Copy => {
                    lines.push(Line::raw(""));
                    lines.push(heading("SHARE"));
                }
                Row::Where => lines.push(Line::raw("")),
                _ => {}
            }
            if i == self.at {
                selected = lines.len();
            }
            lines.push(self.line(row, i == self.at, width));
        }
        (lines, selected)
    }

    fn render(&mut self, f: &mut Frame) -> Rect {
        if let Some(said) = self.sent.lock().ok().and_then(|mut s| s.take()) {
            self.message = Some(said);
        }
        let area = f.area();
        let width = WIDTH.min(area.width);
        let inner_width = width.saturating_sub(2) as usize;
        let (lines, selected) = self.lines(inner_width);
        let height = (lines.len() as u16 + 5).min(area.height);
        let [popup] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(popup);
        f.render_widget(Clear, popup);
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ACCENT))
            .title(Span::styled(" export ", Style::new().fg(ACCENT).bold()))
            .title_top(
                Line::from(label(format!(
                    " {} ",
                    truncate(&self.title, inner_width.saturating_sub(14))
                )))
                .right_aligned(),
            );
        let inner = block.inner(popup);
        f.render_widget(block, popup);
        let [list, _, message, keys] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);
        let room = list.height as usize;
        let top = (selected + 2)
            .saturating_sub(room)
            .min(lines.len().saturating_sub(room));
        f.render_widget(
            Paragraph::new(lines.into_iter().skip(top).take(room).collect::<Vec<_>>()),
            list,
        );
        let note = match &self.message {
            Some((text, bad)) => Line::from(Span::styled(
                format!("  {}", truncate(text, inner_width.saturating_sub(2))),
                Style::new().fg(if *bad { BAD } else { GOOD }),
            )),
            None => Line::from(Span::styled(
                "  the run as it stands now: a later export has more",
                Style::new().fg(FAINT),
            )),
        };
        f.render_widget(Paragraph::new(note), message);
        let pairs: &[(&str, &str)] = if self.editing.is_some() {
            &[("enter", "keep"), ("esc", "leave as it was")]
        } else {
            &[
                ("↑↓", "pick"),
                ("enter", "do it"),
                ("tab", "where"),
                ("esc", "close"),
            ]
        };
        f.render_widget(Paragraph::new(chips(&fitting(pairs, inner.width))), keys);
        let _ = Color::Reset;
        popup
    }
}

thread_local! {
    static PAGE: RefCell<Option<Page>> = const { RefCell::new(None) };
    /// A run screen is up: F4 is there to press, and the footer says so
    static HERE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// A run screen comes up (true) or goes (false)
pub(super) fn here(on: bool) {
    HERE.with(|h| h.set(on));
}

pub(super) fn is_here() -> bool {
    HERE.with(|h| h.get())
}

/// Held by a run screen while it's up: F4 is there, and the page goes
/// with the screen
pub(super) struct Here;

impl Here {
    pub(super) fn on() -> Here {
        here(true);
        Here
    }
}

impl Drop for Here {
    fn drop(&mut self) {
        here(false);
        PAGE.with(|page| *page.borrow_mut() = None);
    }
}

/// Open the page on `snap`; `title` says what it's of
pub(super) fn open(snap: Snapshot, title: String, share: Share) {
    let stem = format!("exports/{}", crate::export::stem(&snap.report));
    PAGE.with(|page| {
        *page.borrow_mut() = Some(Page {
            snap,
            title,
            share,
            at: 0,
            stem,
            editing: None,
            open_after: true,
            message: None,
            sent: Arc::default(),
        })
    });
}

/// Keys come here first while it's open (see `settings::key`)
pub(super) fn key(key: KeyEvent) -> bool {
    let ctrl =
        key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    if ctrl && key.code == KeyCode::Char('c') {
        return false;
    }
    PAGE.with(|page| {
        let mut page = page.borrow_mut();
        let Some(open) = page.as_mut() else {
            return false;
        };
        if open.key(key) {
            *page = None;
        }
        true
    })
}

pub(super) fn paste(text: &str) -> bool {
    PAGE.with(|page| match page.borrow_mut().as_mut() {
        Some(open) => {
            if let Some(editing) = &mut open.editing {
                editing.push_str(text.trim());
            }
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
    use serde_json::json;

    fn press(page: &mut Page, code: KeyCode) -> bool {
        page.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    #[test]
    fn it_saves_each_format_and_everything_where_it_is_told() {
        let dir = std::env::temp_dir().join(format!("pepe-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let snap = Snapshot {
            report: json!({"test": "t", "command": "pepe x",
                "summary": {"total_requests": 10, "failed_requests": 0, "requests_per_second": 5.0, "duration_ms": 2000.0,
                    "latency": {"median_ms": 1.0, "p99_ms": 2.0}},
                "verdict": {"level": "healthy", "findings": []}}),
            ..Snapshot::default()
        };
        open(snap, "t".into(), Share::default());
        PAGE.with(|p| {
            let mut p = p.borrow_mut();
            let page = p.as_mut().unwrap();
            page.open_after = false;
            page.stem = dir.join("run").display().to_string();
            // Json is the second row
            press(page, KeyCode::Down);
            press(page, KeyCode::Enter);
            assert!(dir.join("run.json").exists(), "{:?}", page.message);
            // Everything
            page.at = page.rows().iter().position(|r| *r == Row::All).unwrap();
            press(page, KeyCode::Enter);
            for ext in ["html", "json", "md", "txt", "csv"] {
                assert!(dir.join(format!("run.{ext}")).exists(), "{ext}");
            }
            assert!(page
                .message
                .as_ref()
                .is_some_and(|(m, bad)| m.starts_with("✓ saved") && !bad));
            // No webhooks: no Send row
            assert!(!page.rows().contains(&Row::Send));
            // Tab goes to where, and a new name is kept
            press(page, KeyCode::Tab);
            assert!(page.editing.is_some());
            assert!(!press(page, KeyCode::Esc) && page.editing.is_none());
            assert!(press(page, KeyCode::Esc), "esc closes");
        });
        let _ = std::fs::remove_dir_all(&dir);
    }
}
