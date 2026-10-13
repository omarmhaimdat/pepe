//! A test's recorded runs, from the library: each run's numbers, and what
//! moved since the run before it.

use std::time::SystemTime;

use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent,
};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Padding, Paragraph, Wrap},
    Frame, Terminal,
};

use super::kit::{self, about_line, caption, help, marker, panel, Hit, FAINT, SELECTED};
use super::view::{label, truncate, value, ACCENT, BAD, GOOD, LABEL, RULE, WARN};
use super::{format, settings, theme};
use crate::compare::{compare, Comparison, Side};
use crate::store::{Run, Store};

/// One recorded run, read
pub struct Recorded {
    pub run: Run,
    pub side: Side,
}

pub struct HistoryScreen {
    store: Store,
    name: String,
    /// The newest first
    runs: Vec<Recorded>,
    selected: usize,
    show_help: bool,
}

impl HistoryScreen {
    pub fn new(store: Store, name: &str, mut runs: Vec<Recorded>) -> Self {
        runs.sort_by_key(|r| std::cmp::Reverse(r.run.when));
        HistoryScreen {
            store,
            name: name.to_string(),
            runs,
            selected: 0,
            show_help: false,
        }
    }

    /// The selected run against the one before it, when there is one
    fn since_previous(&self, index: usize) -> Option<Comparison> {
        let older = self.runs.get(index + 1)?;
        Some(compare(&older.side, &self.runs[index].side))
    }

    fn move_by(&mut self, by: i64) {
        let last = self.runs.len().saturating_sub(1) as i64;
        self.selected = (self.selected as i64 + by).clamp(0, last) as usize;
    }

    /// True when the key asks to leave
    fn key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if settings::key(key) {
            return false;
        }
        if self.show_help {
            self.show_help = false;
            return key.code == KeyCode::Char('c') && ctrl;
        }
        match key.code {
            KeyCode::Char('c') if ctrl => return true,
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => return true,
            KeyCode::F(1) | KeyCode::Char('?') => self.show_help = true,
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_by(-i64::from(u32::MAX)),
            KeyCode::End => self.move_by(i64::from(u32::MAX)),
            _ => {}
        }
        false
    }

    fn mouse(&mut self, event: MouseEvent) -> bool {
        if settings::is_open() {
            return false;
        }
        if self.show_help {
            if kit::pressed(&event) {
                self.show_help = false;
            }
            return false;
        }
        match kit::click(&event) {
            Some(Hit::Key(key)) => self.key(key),
            Some(Hit::Row { index, .. }) if index < self.runs.len() => {
                self.selected = index;
                false
            }
            _ => false,
        }
    }

    pub async fn run(mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;
        let mut events = EventStream::new();
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        loop {
            terminal.draw(|f| theme::draw(f, |f| self.render(f)))?;
            let event = tokio::select! {
                _ = &mut ctrl_c => return Ok(()),
                event = events.next() => event,
            };
            let leave = match event {
                Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => self.key(key),
                Some(Ok(Event::Mouse(mouse))) => self.mouse(mouse),
                Some(Ok(_)) => false,
                Some(Err(e)) => return Err(e.into()),
                None => true,
            };
            if leave {
                return Ok(());
            }
        }
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    fn render(&self, f: &mut Frame) {
        let area = f.area();
        let [header, main, since, footer] = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(9),
            Constraint::Length(1),
        ])
        .areas(area);

        let count = match self.runs.len() {
            1 => "1 run".to_string(),
            n => format!("{n} runs"),
        };
        f.render_widget(
            Paragraph::new(vec![
                Line::from(vec![
                    Span::styled(" pepe ", Style::new().bg(ACCENT).fg(Color::Black).bold()),
                    Span::styled(" history ", Style::new().bg(SELECTED).fg(Color::White)),
                    Span::raw("  "),
                    Span::styled(self.name.clone(), Style::new().bold()),
                    Span::styled(
                        format!("  {} · {count}", self.store.runs_dir(&self.name).display()),
                        Style::new().fg(FAINT),
                    ),
                ]),
                Line::from(label(truncate(
                    "Each recorded run of the test, the latest first, and what moved since the run before it.",
                    area.width as usize,
                ))),
            ]),
            header,
        );
        self.render_runs(f, main);
        self.render_since(f, since);
        kit::footer(
            f,
            footer,
            &[("↑↓", "move"), ("F1", "keys"), ("esc", "back")],
        );
        if self.show_help {
            help(
                f,
                area,
                &[
                    ("↑ ↓ / j k", "move between runs"),
                    ("F1 / ?", "this help"),
                    ("esc / q", "back to the library"),
                ],
                &[
                    "A run is recorded with --record, pepe rerun, or w on".into(),
                    "the dashboard once it has ended. `pepe history NAME`".into(),
                    "prints this list; `pepe compare NAME` holds the last two".into(),
                    "runs against each other.".into(),
                    String::new(),
                    about_line(),
                ],
            );
        }
    }

    fn render_runs(&self, f: &mut Frame, area: Rect) {
        let block = panel(caption("runs", true), None, RULE);
        let inner = block.inner(area);
        f.render_widget(block, area);
        let width = inner.width as usize;
        let [titles, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
        let wide = width >= 100;
        let heading = if wide {
            format!(
                "  {:<17}{:>9}  {:>7}  {:>8}  {:>8}  {:>7}  SINCE PREVIOUS",
                "WHEN", "REQUESTS", "RPS", "P50", "P99", "FAILED"
            )
        } else {
            format!(
                "  {:<17}{:>7}  {:>8}  {:>7}",
                "WHEN", "RPS", "P99", "FAILED"
            )
        };
        f.render_widget(
            Paragraph::new(Span::styled(
                truncate(&heading, width),
                Style::new().fg(FAINT),
            )),
            titles,
        );
        let visible = body.height as usize;
        if visible == 0 {
            return;
        }
        let first = if self.selected >= visible {
            self.selected + 1 - visible
        } else {
            0
        };
        kit::hit_rows(body, 0, first, self.runs.len().saturating_sub(first));
        let now = SystemTime::now();
        let lines: Vec<Line> = self
            .runs
            .iter()
            .enumerate()
            .skip(first)
            .take(visible)
            .map(|(i, r)| {
                let active = i == self.selected;
                let dash = || "-".to_string();
                let when = if i == 0 {
                    format!("{} ", super::ago(r.run.when, now))
                } else {
                    format::local_stamp(r.run.when)
                };
                let rps = r
                    .side
                    .requests_per_second
                    .map_or_else(dash, format::compact);
                let p50 = r.side.median_ms.map_or_else(dash, format::ms);
                let p99 = r.side.p99_ms.map_or_else(dash, format::ms);
                let failed = format!("{:.1}%", r.side.failed_percent);
                let mut spans = vec![marker(active, true), Span::raw(" ")];
                if wide {
                    spans.push(Span::raw(format!(
                        "{:<17}{:>9}  {:>7}  {:>8}  {:>8}  ",
                        truncate(&when, 16),
                        format::count(r.side.requests),
                        rps,
                        p50,
                        p99
                    )));
                } else {
                    spans.push(Span::raw(format!(
                        "{:<17}{:>7}  {:>8}  ",
                        truncate(&when, 16),
                        rps,
                        p99
                    )));
                }
                spans.push(value(
                    format!("{failed:>7}"),
                    if r.side.failed_percent > 0.0 {
                        WARN
                    } else {
                        LABEL
                    },
                ));
                if wide {
                    spans.push(Span::raw("  "));
                    match self.since_previous(i) {
                        Some(c) => spans.push(value(
                            crate::modes::history::since_words(&c),
                            verdict_color(&c),
                        )),
                        None => spans.push(value("first run", LABEL)),
                    }
                }
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
        f.render_widget(Paragraph::new(lines), body);
    }

    /// The selected run against the one before it, as `pepe compare` says it
    fn render_since(&self, f: &mut Frame, area: Rect) {
        let block = panel(caption("since the run before", false), None, RULE)
            .padding(Padding::horizontal(1));
        let text = match self.since_previous(self.selected) {
            Some(c) => {
                let color = verdict_color(&c);
                let mut lines: Vec<Line> = vec![Line::from(Span::styled(
                    crate::modes::history::since_words(&c),
                    Style::new().fg(color).bold(),
                ))];
                lines.extend(
                    c.report()
                        .lines()
                        .skip(1)
                        .take((area.height as usize).saturating_sub(3))
                        .map(|l| Line::from(label(l.to_string()))),
                );
                lines
            }
            None => vec![Line::from(label(match self.runs.get(self.selected) {
                Some(r) => format!(
                    "the first recorded run · {} requests · {} · {}",
                    format::count(r.side.requests),
                    r.side.command_or_file(),
                    r.run.path.display()
                ),
                None => "no recorded run yet".to_string(),
            }))],
        };
        f.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: false }).block(block),
            area,
        );
    }
}

fn verdict_color(c: &Comparison) -> Color {
    use crate::compare::Outcome;
    match c.verdict {
        Outcome::Better | Outcome::Faster => GOOD,
        Outcome::Same => LABEL,
        Outcome::Slower => WARN,
        Outcome::Worse => BAD,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use std::time::{Duration, UNIX_EPOCH};

    fn recorded(dir: &std::path::Path, stamp: &str, rps: u32, p99: u32) -> Recorded {
        let path = dir.join(format!("{stamp}.json"));
        std::fs::write(
            &path,
            format!(
                r#"{{"schema_version":1,"target":{{"mode":"run","url":"https://x.io/","concurrency":4}},"summary":{{"total_requests":1000,"failed_requests":0,"requests_per_second":{rps},"latency":{{"median_ms":10,"p99_ms":{p99},"std_dev_ms":2,"avg_ms":12}}}}}}"#
            ),
        )
        .unwrap();
        let side = Side::read(&path).unwrap();
        let when = UNIX_EPOCH + Duration::from_secs(1_791_900_000 + u64::from(p99));
        Recorded {
            run: Run { path, when },
            side,
        }
    }

    #[test]
    fn the_runs_are_listed_newest_first_each_against_the_one_before() {
        let dir = std::env::temp_dir().join(format!("pepe-history-screen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let runs = vec![
            recorded(&dir, "a", 300, 50),
            recorded(&dir, "b", 310, 120),
            recorded(&dir, "c", 305, 55),
        ];
        let mut s = HistoryScreen::new(Store::at(dir.join(".pepe")), "api", runs);
        assert_eq!(s.runs[0].side.p99_ms, Some(120.0), "the newest first");
        // The newest against the one before it: p99 50 → 120 is slower
        let since = s.since_previous(0).unwrap();
        assert!(since.verdict.is_regression(), "{since:?}");
        assert!(
            s.since_previous(2).is_none(),
            "the oldest has nothing before it"
        );
        // Keys move, esc leaves
        assert!(!s.key(KeyEvent::from(KeyCode::Down)));
        assert_eq!(s.selected, 1);
        assert!(s.key(KeyEvent::from(KeyCode::Esc)));
        for (w, h) in [(80, 24), (120, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            for selected in 0..3 {
                s.selected = selected;
                terminal.draw(|f| theme::draw(f, |f| s.render(f))).unwrap();
            }
            let screen = terminal.backend().to_string();
            assert!(screen.contains("api"), "{screen}");
            assert!(screen.contains("first recorded run"), "{screen}");
            if w >= 120 {
                assert!(screen.contains("3 runs"), "{screen}");
                assert!(screen.contains("first run"), "{screen}");
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}
