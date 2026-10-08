//! The logs screen: how busy nginx is now against each minute, hour and
//! day its logs go back, what is asked for and by whom, what fails, and
//! the lines themselves

use std::sync::Arc;
use std::time::Duration;

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
    Frame, Terminal,
};
use tokio::time::MissedTickBehavior;

use super::kit::{about_line, chips_fit, help, marker, tabs, wrap, FAINT, FIELD, SELECTED};
use super::view::{
    bar, label, section, status_color, truncate, value, ACCENT, BAD, GOOD, LABEL, RULE, WARN,
};
use super::{format, theme};
use crate::logs::{
    self, ago, busiest, day_and_time, echo, offset_label, percent, rate, time_of_day, typical,
    versus, Clock, Grain, Now, Parser, Recent, Row, Shared, Stats,
};

/// How often the screen is drawn again while nothing is pressed
const FRAME: Duration = Duration::from_millis(200);
const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 16;
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
/// Width of a number card
const CARD: u16 = 22;
/// The chart's axis labels
const AXIS: u16 = 7;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Traffic,
    Paths,
    Errors,
    Log,
}

const TABS: [Tab; 4] = [Tab::Traffic, Tab::Paths, Tab::Errors, Tab::Log];

/// What the paths are listed by
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sort {
    Requests,
    Failed,
    Refused,
    Time,
}

impl Sort {
    fn next(self) -> Sort {
        match self {
            Sort::Requests => Sort::Failed,
            Sort::Failed => Sort::Refused,
            Sort::Refused => Sort::Time,
            Sort::Time => Sort::Requests,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Sort::Requests => "most requested",
            Sort::Failed => "most 5xx",
            Sort::Refused => "most 4xx",
            Sort::Time => "slowest",
        }
    }
}

pub struct LogsScreen {
    shared: Arc<Shared>,
    parser: Parser,
    name: String,
    /// Seconds "now" is measured over
    window: i64,
    tab: Tab,
    grain: Grain,
    /// The slot picked, counted back from the newest
    slot: usize,
    sort: Sort,
    /// The error log cause picked
    cause: usize,
    /// The line picked, by its number in the log; none follows the end
    line: Option<u64>,
    errors_only: bool,
    search: String,
    /// The search is being typed
    searching: bool,
    /// The picked line's fields are open
    inspecting: bool,
    show_help: bool,
}

/// A list beside the paths: each name, its requests, and its colour
type Counts = Vec<(String, u64, Color)>;

/// 5xx share that is worth a colour
fn failing_color(share: f64) -> Color {
    match share {
        s if s >= 0.05 => BAD,
        s if s >= 0.01 => WARN,
        _ => Color::Reset,
    }
}

fn severity_color(severity: u8) -> Color {
    match severity {
        2 => BAD,
        1 => WARN,
        _ => LABEL,
    }
}

fn spark(values: &[u32]) -> String {
    let most = values.iter().copied().max().unwrap_or(0).max(1);
    values
        .iter()
        .map(|&v| match v {
            0 => ' ',
            v => BLOCKS[((v as usize * 8).div_ceil(most as usize) - 1).min(7)],
        })
        .collect()
}

/// First row shown so that row `selected` of `len` is among `height`
fn scroll(selected: usize, len: usize, height: usize) -> usize {
    selected
        .saturating_sub(height.saturating_sub(1))
        .min(len.saturating_sub(height))
}

fn right(text: String, width: usize) -> String {
    format!("{text:>width$}")
}

impl LogsScreen {
    pub fn new(shared: Arc<Shared>, parser: Parser, name: String, window: i64) -> Self {
        LogsScreen {
            shared,
            parser,
            name,
            window,
            tab: Tab::Traffic,
            grain: Grain::Minute,
            slot: 0,
            sort: Sort::Requests,
            cause: 0,
            line: None,
            errors_only: false,
            search: String::new(),
            searching: false,
            inspecting: false,
            show_help: false,
        }
    }

    /// The lines the log view shows, with their numbers, oldest first
    fn lines<'a>(&self, stats: &'a Stats) -> Vec<(u64, &'a Recent)> {
        let needle = self.search.to_lowercase();
        stats
            .recent
            .iter()
            .enumerate()
            .filter(|(_, line)| !self.errors_only || line.kind.is_error())
            .filter(|(_, line)| needle.is_empty() || line.text.to_lowercase().contains(&needle))
            .map(|(i, line)| (stats.recent_base + i as u64, line))
            .collect()
    }

    /// Move the pick in the log view by `by` lines; past the end follows
    fn move_line(&mut self, by: i64) {
        let stats = self.shared.lock();
        let numbers: Vec<u64> = self.lines(&stats).iter().map(|(n, _)| *n).collect();
        drop(stats);
        let Some(last) = numbers.len().checked_sub(1) else {
            self.line = None;
            return;
        };
        let at = match self.line {
            Some(number) => numbers.partition_point(|n| *n < number).min(last),
            None => last + 1,
        };
        let to = at as i64 + by;
        self.line = (to <= last as i64).then(|| numbers[to.max(0) as usize]);
    }

    fn move_by(&mut self, by: i64) {
        let step = |at: usize| (at as i64 + by).max(0) as usize;
        match self.tab {
            // The table has the newest slot on top
            Tab::Traffic => self.slot = step(self.slot),
            Tab::Errors => self.cause = step(self.cause),
            Tab::Log => self.move_line(by),
            Tab::Paths => {}
        }
    }

    /// True when the key asks to leave
    fn key(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
        }
        if self.searching {
            match key.code {
                KeyCode::Enter => self.searching = false,
                KeyCode::Esc => {
                    self.searching = false;
                    self.search.clear();
                }
                KeyCode::Backspace => {
                    self.search.pop();
                }
                KeyCode::Char(c) => self.search.push(c),
                _ => {}
            }
            return false;
        }
        if self.show_help {
            self.show_help = !matches!(key.code, KeyCode::Char('?') | KeyCode::Esc | KeyCode::F(1));
            return false;
        }
        if self.inspecting && matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            self.inspecting = false;
            return false;
        }
        let at = TABS.iter().position(|t| *t == self.tab).unwrap_or(0);
        match key.code {
            KeyCode::Char('q') | KeyCode::Char('Q') => return true,
            KeyCode::Char('?') | KeyCode::F(1) => self.show_help = true,
            KeyCode::Tab | KeyCode::Right => self.tab = TABS[(at + 1) % TABS.len()],
            KeyCode::BackTab | KeyCode::Left => self.tab = TABS[(at + TABS.len() - 1) % TABS.len()],
            KeyCode::Char(c @ '1'..='4') => self.tab = TABS[c as usize - '1' as usize],
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_by(-i64::from(u32::MAX)),
            KeyCode::End => self.move_by(i64::from(u32::MAX)),
            KeyCode::Char('m') if self.tab == Tab::Traffic => self.set_grain(Grain::Minute),
            KeyCode::Char('h') if self.tab == Tab::Traffic => self.set_grain(Grain::Hour),
            KeyCode::Char('d') if self.tab == Tab::Traffic => self.set_grain(Grain::Day),
            KeyCode::Char('g') if self.tab == Tab::Traffic => {
                self.set_grain(Grain::ALL[(self.grain as usize + 1) % 3])
            }
            KeyCode::Char('s') if self.tab == Tab::Paths => self.sort = self.sort.next(),
            KeyCode::Char('x') if self.tab == Tab::Log => {
                self.errors_only = !self.errors_only;
                self.line = None;
            }
            KeyCode::Char('/') if self.tab == Tab::Log => {
                self.searching = true;
                self.line = None;
            }
            KeyCode::Char('c') if self.tab == Tab::Log => {
                self.errors_only = false;
                self.search.clear();
            }
            KeyCode::Enter if self.tab == Tab::Log => self.inspecting = true,
            // Esc lets go of what is held, one thing at a time, then leaves
            KeyCode::Esc => {
                if self.tab == Tab::Log && self.line.is_some() {
                    self.line = None;
                } else if self.tab == Tab::Log && (!self.search.is_empty() || self.errors_only) {
                    self.search.clear();
                    self.errors_only = false;
                } else if self.tab == Tab::Traffic && self.slot > 0 {
                    self.slot = 0;
                } else {
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn set_grain(&mut self, grain: Grain) {
        self.grain = grain;
        self.slot = 0;
    }

    pub async fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;
        let mut events = EventStream::new();
        let mut frame = tokio::time::interval(FRAME);
        frame.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);
        loop {
            terminal.draw(|f| theme::draw(f, |f| self.render(f, logs::wall())))?;
            tokio::select! {
                _ = &mut ctrl_c => return Ok(()),
                _ = frame.tick() => {}
                event = events.next() => match event {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        if self.key(key) {
                            return Ok(());
                        }
                    }
                    Some(Ok(_)) => {}
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(()),
                },
            }
        }
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    fn render(&mut self, f: &mut Frame, wall: i64) {
        let area = f.area();
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            f.render_widget(
                Paragraph::new(format!(
                    "pepe logs needs {MIN_WIDTH}×{MIN_HEIGHT}; this is {}×{}",
                    area.width, area.height
                )),
                area,
            );
            return;
        }
        let shared = self.shared.clone();
        let stats = shared.lock();
        let clock = stats.clock(wall);
        let now = stats.now(clock, self.window);
        let rows = stats.rows(self.grain, clock);
        let notes = self.notes(&stats);

        let [title, _, cards, _, notes_area, tab_bar, _, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(1),
            Constraint::Length(notes.len() as u16),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .areas(area);

        self.render_title(f, title, &stats, clock, wall);
        self.render_cards(f, cards, &stats, clock, &now, &rows);
        f.render_widget(Paragraph::new(notes), notes_area);
        let titles: Vec<String> = [
            "1 Traffic".to_string(),
            "2 Paths".to_string(),
            match stats.faults {
                0 => "3 Errors".to_string(),
                n => format!("3 Errors {}", format::compact(n as f64)),
            },
            "4 Log".to_string(),
        ]
        .into();
        let at = TABS.iter().position(|t| *t == self.tab).unwrap_or(0);
        f.render_widget(Paragraph::new(tabs(&titles, at)), tab_bar);

        match self.tab {
            Tab::Traffic => self.render_traffic(f, body, &stats, &now, &rows),
            Tab::Paths => self.render_paths(f, body, &stats),
            Tab::Errors => self.render_errors(f, body, &stats, clock),
            Tab::Log => self.render_log(f, body, &stats, clock),
        }

        let keys: Vec<(&str, &str)> = match self.tab {
            _ if self.searching => vec![("enter", "keep"), ("esc", "clear")],
            Tab::Traffic => vec![
                ("m h d", "per minute, hour, day"),
                ("↑↓", "pick a slot"),
                ("tab", "view"),
                ("?", "keys"),
                ("q", "quit"),
            ],
            Tab::Paths => vec![("s", "sort"), ("tab", "view"), ("?", "keys"), ("q", "quit")],
            Tab::Errors => vec![
                ("↑↓", "pick a message"),
                ("tab", "view"),
                ("?", "keys"),
                ("q", "quit"),
            ],
            Tab::Log => vec![
                ("↑↓", "pick a line"),
                ("enter", "its fields"),
                ("x", "errors only"),
                ("/", "search"),
                ("tab", "view"),
                ("?", "keys"),
                ("q", "quit"),
            ],
        };
        f.render_widget(Paragraph::new(chips_fit(&keys, footer.width)), footer);

        if self.inspecting {
            self.render_inspector(f, area, &stats);
        }
        if self.show_help {
            help(
                f,
                area,
                &[
                    ("tab ← →", "next or previous view; 1-4 pick one"),
                    ("m h d", "traffic per minute, hour or day; g goes round"),
                    ("↑ ↓ / j k", "pick a slot, a message or a line"),
                    ("PgUp PgDn", "ten at a time; home and end, the ends"),
                    ("s", "paths: most requested, most 5xx, most 4xx, slowest"),
                    ("x", "log: only failed requests and error log lines"),
                    ("/", "log: search the lines; c clears the filters"),
                    ("enter", "log: everything read from the picked line"),
                    ("esc", "let go of the pick, then quit"),
                    ("?", "close this help"),
                    ("q", "quit"),
                ],
                &[
                    format!(
                        "Now is the last {} of the log's own times.",
                        format::span(Duration::from_secs(self.window as u64))
                    ),
                    "A log not being written is held at its last line.".into(),
                    about_line(),
                ],
            );
        }
    }

    /// What the reader couldn't do, said above the tabs
    fn notes(&self, stats: &Stats) -> Vec<Line<'static>> {
        let mut notes = Vec::new();
        let warn = |text: String| Line::from(vec![value("▲ ", WARN), Span::raw(text)]);
        if let Some(trouble) = &stats.trouble {
            notes.push(Line::from(vec![
                value("✖ ", BAD),
                Span::raw(trouble.clone()),
            ]));
        }
        if stats.unread > 0 {
            notes.push(warn(format!(
                "{} of {} couldn't be read; a log with a format of its own needs --format",
                format::count(stats.unread),
                logs::counted(stats.lines, "line")
            )));
        }
        if stats.undated > 0 {
            notes.push(warn(format!(
                "{} had no time, and so are in no slot",
                logs::counted(stats.undated, "request")
            )));
        }
        notes
    }

    fn render_title(&self, f: &mut Frame, area: Rect, stats: &Stats, clock: Clock, wall: i64) {
        let state = if !stats.caught_up {
            let percent = stats.read_bytes as f64 / stats.total_bytes.max(1) as f64 * 100.0;
            match stats.total_bytes {
                0 => value("reading", ACCENT),
                _ => value(format!("reading {:.0}%", percent.min(99.0)), ACCENT),
            }
        } else if clock.live {
            value("● live", GOOD)
        } else if stats.last.is_some() {
            label(format!("ended {} ago", ago(wall - clock.now)))
        } else {
            label("nothing dated yet")
        };
        let right = Line::from(vec![
            state,
            label(format!(
                " · {} · {}",
                time_of_day(clock.now + i64::from(clock.offset)),
                offset_label(clock.offset)
            )),
        ]);
        let room = (area.width as usize).saturating_sub(right.width() + 14);
        let left = Line::from(vec![
            Span::styled("pepe logs", Style::new().fg(ACCENT).bold()),
            label(" · "),
            Span::raw(truncate(&self.name, room)).bold(),
        ]);
        f.render_widget(Paragraph::new(left), area);
        f.render_widget(Paragraph::new(right.right_aligned()), area);
    }

    fn render_cards(
        &self,
        f: &mut Frame,
        area: Rect,
        stats: &Stats,
        clock: Clock,
        now: &Now,
        rows: &[Row],
    ) {
        let grain = self.grain.name().to_uppercase();
        let per_second = |r: f64| value(format!("{} req/s", rate(r)), Color::Reset);
        let against = |then: f64| label(format!("now {}", versus(now.rate, then)));
        let mut cards: Vec<(String, Span, Line)> = Vec::new();
        let seconds = stats.last_seconds(
            clock,
            (CARD as usize).saturating_sub(4).min(self.window as usize),
        );
        cards.push((
            if clock.live {
                "NOW".into()
            } else {
                "AT THE END".into()
            },
            per_second(now.rate),
            Line::from(Span::styled(spark(&seconds), Style::new().fg(LABEL))),
        ));
        if let Some(usual) = typical(rows) {
            cards.push((
                format!("A USUAL {grain}"),
                per_second(usual),
                against(usual).into(),
            ));
        }
        if let Some(most) = busiest(rows) {
            cards.push((
                format!("BUSIEST {grain}"),
                per_second(most.rate),
                label(format!(
                    "{} · {}",
                    self.grain.label(most.start),
                    versus(now.rate, most.rate)
                ))
                .into(),
            ));
        }
        if let Some(then) = echo(rows, self.grain).filter(|r| !r.partial) {
            cards.push((
                self.grain.echo().1.to_uppercase(),
                per_second(then.rate),
                against(then.rate).into(),
            ));
        }
        cards.push((
            if clock.live {
                "FAILING NOW".into()
            } else {
                "FAILING THEN".into()
            },
            value(
                format!("5xx {}", percent(now.share_5xx())),
                failing_color(now.share_5xx()),
            ),
            label(format!("4xx {}", percent(now.share_4xx()))).into(),
        ));
        if stats.time.count() > 0 {
            let at = |q: f64| format::latency(Duration::from_micros(stats.time.percentile(q)));
            cards.push((
                "REQUEST TIME".into(),
                value(format!("p50 {}", at(50.0)), Color::Reset),
                label(format!("p99 {}", at(99.0))).into(),
            ));
        }
        cards.push((
            "IN ALL".into(),
            value(format::count(stats.requests), Color::Reset),
            label(format!("requests · {}", format::bytes(stats.bytes as f64))).into(),
        ));

        let fit = ((area.width / CARD) as usize).clamp(1, cards.len());
        // The total gives way first, then the comparisons from the right
        while cards.len() > fit {
            let last = cards.len() - 1;
            cards.remove(if cards.len() == fit + 1 {
                last
            } else {
                last - 1
            });
        }
        let columns = Layout::horizontal(vec![Constraint::Ratio(1, fit as u32); fit]).split(area);
        for ((title, number, note), column) in cards.into_iter().zip(columns.iter()) {
            let width = column.width.saturating_sub(1) as usize;
            let lines = vec![
                Line::from(Span::styled(
                    truncate(&title, width),
                    Style::new().fg(LABEL).bold(),
                )),
                Line::from(number),
                note,
            ];
            f.render_widget(Paragraph::new(lines), *column);
        }
    }

    fn render_traffic(
        &mut self,
        f: &mut Frame,
        area: Rect,
        stats: &Stats,
        now: &Now,
        rows: &[Row],
    ) {
        if rows.is_empty() {
            let waiting = if stats.caught_up {
                "No request with a time in the log yet"
            } else {
                "Reading…"
            };
            f.render_widget(Paragraph::new(label(waiting)), area);
            return;
        }
        self.slot = self.slot.min(rows.len() - 1);
        let picked = rows.len() - 1 - self.slot;
        let chart_height = match area.height {
            h if h >= 24 => 10,
            h if h >= 14 => 6,
            _ => 0,
        };
        let [chart, _, heading, table] = Layout::vertical([
            Constraint::Length(chart_height),
            Constraint::Length(u16::from(chart_height > 0)),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .areas(area);
        if chart_height > 0 {
            self.render_chart(f, chart, rows, now.rate, picked);
        }

        let timed = rows.iter().any(|r| r.slot.timed > 0);
        let logged = rows.iter().any(|r| r.slot.faults > 0);
        let mut head = format!(
            "  {:<17}{:>12}{:>9}{:>8}{:>8}{:>8}",
            format!("per {}", self.grain.name()),
            "requests",
            "req/s",
            "peak/s",
            "4xx",
            "5xx"
        );
        if timed {
            head.push_str(&right("mean time".into(), 11));
        }
        if logged {
            head.push_str(&right("errors".into(), 8));
        }
        head.push_str(&right("now vs".into(), 9));
        f.render_widget(
            Paragraph::new(Span::styled(head.clone(), Style::new().fg(LABEL).bold())),
            heading,
        );

        let most = rows.iter().map(|r| r.rate).fold(0.0, f64::max).max(1e-9);
        let height = table.height as usize;
        let from = scroll(self.slot, rows.len(), height);
        let room = (table.width as usize).saturating_sub(head.chars().count() + 2);
        let lines: Vec<Line> = rows
            .iter()
            .rev()
            .enumerate()
            .skip(from)
            .take(height)
            .map(|(i, row)| {
                let slot = &row.slot;
                let of = |n: u64| n as f64 / slot.requests.max(1) as f64;
                let share = |n: u64, color: Color| {
                    let color = if n == 0 { FAINT } else { color };
                    Span::styled(right(percent(of(n)), 8), Style::new().fg(color))
                };
                let mut spans = vec![
                    marker(i == self.slot, true),
                    Span::raw(format!(" {:<17}", self.grain.label(row.start))),
                    Span::raw(right(format::count(slot.requests), 12)),
                    Span::raw(right(rate(row.rate), 9)).bold(),
                    Span::raw(right(format::count(u64::from(slot.peak)), 8)),
                    share(slot.c4xx, Color::Reset),
                    share(
                        slot.c5xx,
                        failing_color(of(slot.c5xx)).max_or(WARN, slot.c5xx),
                    ),
                ];
                if timed {
                    let mean = (slot.timed > 0)
                        .then(|| format::latency(Duration::from_micros(slot.time_us / slot.timed)));
                    spans.push(Span::raw(right(mean.unwrap_or_default(), 11)));
                }
                if logged {
                    let color = if slot.faults > 0 { BAD } else { FAINT };
                    spans.push(Span::styled(
                        right(format::count(slot.faults), 8),
                        Style::new().fg(color),
                    ));
                }
                spans.push(label(right(versus(now.rate, row.rate), 9)));
                spans.push(Span::styled(
                    format!("  {}", bar(row.rate / most, room)),
                    Style::new().fg(LABEL),
                ));
                if row.partial {
                    spans.push(Span::styled(" partial", Style::new().fg(FAINT)));
                }
                let line = Line::from(spans);
                if i == self.slot {
                    line.style(Style::new().bg(SELECTED))
                } else {
                    line
                }
            })
            .collect();
        f.render_widget(Paragraph::new(lines), table);
    }

    /// A bar for each slot, the newest at the right, and the rate now as a
    /// line across them
    fn render_chart(&self, f: &mut Frame, area: Rect, rows: &[Row], now: f64, picked: usize) {
        let height = area.height.saturating_sub(1);
        let width = area.width.saturating_sub(AXIS);
        if height == 0 || width == 0 {
            return;
        }
        let shown = &rows[rows.len().saturating_sub(width as usize)..];
        let first = rows.len() - shown.len();
        // A few slots get bars with some width to them
        let per = (width / shown.len() as u16).clamp(1, 8);
        let bar_width = if per >= 3 { per - 1 } else { per } as usize;
        let most = shown.iter().map(|r| r.rate).fold(now, f64::max).max(1e-9);
        let eighths = |r: f64| ((r / most) * f64::from(height) * 8.0).round() as u16;
        let now_row = (eighths(now) / 8).min(height - 1);
        let left = area.x + AXIS + (width - shown.len() as u16 * per);
        let axis = AXIS as usize - 1;
        let buf = f.buffer_mut();
        for row in 0..height {
            // Rows count up from the bottom
            let y = area.y + height - 1 - row;
            if row == now_row {
                let line = "┄".repeat(width as usize);
                buf.set_string(area.x + AXIS, y, line, Style::new().fg(ACCENT));
                buf.set_string(
                    area.x,
                    y,
                    format!("{:>axis$}", "now"),
                    Style::new().fg(ACCENT),
                );
            }
            for (i, slot) in shown.iter().enumerate() {
                let filled = match eighths(slot.rate) {
                    0 if slot.slot.requests > 0 => 1,
                    e => e,
                };
                let symbol = match filled.saturating_sub(row * 8) {
                    0 => continue,
                    e @ 1..=7 => BLOCKS[e as usize - 1],
                    _ => '█',
                };
                let failing = slot.slot.c5xx as f64 / slot.slot.requests.max(1) as f64;
                let color = match failing_color(failing) {
                    Color::Reset if first + i == picked => Color::Reset,
                    Color::Reset => LABEL,
                    color => color,
                };
                buf.set_string(
                    left + i as u16 * per,
                    y,
                    symbol.to_string().repeat(bar_width),
                    Style::new().fg(color),
                );
            }
        }
        if now_row != height - 1 {
            buf.set_string(
                area.x,
                area.y,
                format!("{:>axis$}", rate(most)),
                Style::new().fg(LABEL),
            );
        }
        // Under the bars: where they start, which one is picked, where they end
        let y = area.y + height;
        let from = self.grain.label(shown[0].start);
        let to = self.grain.label(shown[shown.len() - 1].start);
        let end = area.x + AXIS + width;
        let to_x = end.saturating_sub(to.chars().count() as u16);
        if to_x > left + from.chars().count() as u16 + 1 {
            buf.set_string(left, y, &from, Style::new().fg(LABEL));
        }
        buf.set_string(to_x, y, &to, Style::new().fg(LABEL));
        if picked >= first {
            let x = left + (picked - first) as u16 * per + bar_width as u16 / 2;
            buf.set_string(x, y, "▲", Style::new().fg(ACCENT));
        }
    }

    fn render_paths(&self, f: &mut Frame, area: Rect, stats: &Stats) {
        let wide = area.width >= 110;
        let [paths, _, side] = if wide {
            Layout::horizontal([
                Constraint::Percentage(58),
                Constraint::Length(3),
                Constraint::Min(0),
            ])
            .areas(area)
        } else {
            Layout::vertical([
                Constraint::Percentage(50),
                Constraint::Length(1),
                Constraint::Min(0),
            ])
            .areas(area)
        };

        // Paths
        let [title, heading, list] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(paths);
        let count = match stats.paths.other {
            0 => format!("{} paths", format::count(stats.paths.len() as u64)),
            other => format!(
                "{} paths, and {} requests of others",
                format::count(stats.paths.len() as u64),
                format::count(other)
            ),
        };
        section(
            f,
            title,
            &format!("Paths · {}", self.sort.name()),
            Some(Line::from(label(count))),
        );
        let timed = stats.time.count() > 0;
        let numbers = 12 + 7 + 8 + 8 + if timed { 11 } else { 0 };
        let name_width = (list.width as usize).saturating_sub(numbers + 1).max(8);
        let mut head = format!(
            "{:<name_width$} {:>11}{:>7}{:>8}{:>8}",
            "", "requests", "share", "4xx", "5xx"
        );
        if timed {
            head.push_str(&right("mean time".into(), 11));
        }
        f.render_widget(
            Paragraph::new(Span::styled(head, Style::new().fg(LABEL).bold())),
            heading,
        );
        let top = stats.paths.top(list.height as usize, |p| match self.sort {
            Sort::Requests => p.requests,
            Sort::Failed => p.c5xx,
            Sort::Refused => p.c4xx,
            Sort::Time => p.mean_time().map_or(0, |t| t.as_micros() as u64),
        });
        let total = stats.requests.max(1) as f64;
        let lines: Vec<Line> = top
            .iter()
            .map(|(path, stat)| {
                let of = |n: u64| n as f64 / stat.requests.max(1) as f64;
                let share = |n: u64, color: Color| {
                    let color = if n == 0 { FAINT } else { color };
                    Span::styled(right(percent(of(n)), 8), Style::new().fg(color))
                };
                let mut spans = vec![
                    Span::raw(format!("{:<name_width$} ", truncate(path, name_width))),
                    Span::raw(right(format::count(stat.requests), 11)).bold(),
                    label(right(percent(stat.requests as f64 / total), 7)),
                    share(stat.c4xx, Color::Reset),
                    share(
                        stat.c5xx,
                        failing_color(of(stat.c5xx)).max_or(WARN, stat.c5xx),
                    ),
                ];
                if timed {
                    spans.push(Span::raw(right(
                        stat.mean_time().map(format::latency).unwrap_or_default(),
                        11,
                    )));
                }
                Line::from(spans)
            })
            .collect();
        f.render_widget(Paragraph::new(lines), list);

        // Beside them: who asks, and for what
        let mut statuses: Counts = stats
            .statuses
            .iter()
            .map(|(code, n)| match code {
                0 => ("no status".to_string(), *n, FAINT),
                code => (code.to_string(), *n, status_color(*code)),
            })
            .collect();
        statuses.sort_by(|a, b| b.1.cmp(&a.1));
        let counted = |top: &logs::Top<u64>, n: usize| -> Counts {
            top.top(n, |n| *n)
                .into_iter()
                .map(|(name, n)| (name.to_string(), *n, Color::Reset))
                .collect()
        };
        let each = (side.height / if wide { 3 } else { 2 }).max(3) as usize - 2;
        let lists: Vec<(&str, Counts)> = vec![
            ("Status", statuses),
            ("Clients", counted(&stats.clients, each)),
            ("User agents", counted(&stats.agents, each)),
            ("Query parameters", counted(&stats.params, each)),
            ("Methods", counted(&stats.methods, each)),
        ];
        let lists: Vec<_> = lists.into_iter().filter(|(_, l)| !l.is_empty()).collect();
        let across = if wide { 2 } else { 3 }.min(lists.len().max(1));
        let down = lists.len().div_ceil(across).max(1);
        let grid_rows = Layout::vertical(vec![Constraint::Ratio(1, down as u32); down]).split(side);
        for (i, (title, items)) in lists.iter().enumerate() {
            let cells = Layout::horizontal(vec![Constraint::Ratio(1, across as u32); across])
                .spacing(2)
                .split(grid_rows[i / across]);
            self.render_counts(f, cells[i % across], title, items, total);
        }
    }

    fn render_counts(
        &self,
        f: &mut Frame,
        area: Rect,
        title: &str,
        items: &[(String, u64, Color)],
        total: f64,
    ) {
        if area.height < 2 {
            return;
        }
        let [head, list] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
        section(f, head, title, None);
        let name_width = (area.width as usize).saturating_sub(17).max(6);
        let lines: Vec<Line> = items
            .iter()
            .take(list.height.saturating_sub(1) as usize)
            .map(|(name, n, color)| {
                Line::from(vec![
                    Span::styled(
                        format!("{:<name_width$} ", truncate(name, name_width)),
                        Style::new().fg(*color),
                    ),
                    Span::raw(right(format::compact(*n as f64), 8)).bold(),
                    label(right(percent(*n as f64 / total), 7)),
                ])
            })
            .collect();
        f.render_widget(Paragraph::new(lines), list);
    }

    fn render_errors(&mut self, f: &mut Frame, area: Rect, stats: &Stats, clock: Clock) {
        let failing = stats.paths.top(8, |p| p.c5xx);
        let refused = stats.paths.top(8, |p| p.c4xx);
        let below = (failing.len().max(refused.len()) as u16 + 2).min(area.height / 3);
        let [log, _, paths] = Layout::vertical([
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(below),
        ])
        .areas(area);

        let [title, list, _, example] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(if stats.faults > 0 { 4 } else { 0 }),
        ])
        .areas(log);
        let levels: Vec<String> = stats
            .levels
            .iter()
            .map(|(level, n)| format!("{level} {}", format::count(*n)))
            .collect();
        section(
            f,
            title,
            "Error log",
            Some(Line::from(label(levels.join(" · ")))),
        );
        if stats.faults == 0 {
            f.render_widget(
                Paragraph::new(vec![
                    Line::from(label("No error log line read.")),
                    Line::from(label(
                        "Name the error log beside the access log: pepe logs access.log error.log",
                    )),
                ]),
                list,
            );
        } else {
            let causes = stats.top_causes(usize::MAX);
            self.cause = self.cause.min(causes.len().saturating_sub(1));
            let height = list.height as usize;
            let from = scroll(self.cause, causes.len(), height);
            let room = (list.width as usize).saturating_sub(40);
            let lines: Vec<Line> = causes
                .iter()
                .enumerate()
                .skip(from)
                .take(height)
                .map(|(i, (what, cause))| {
                    let message = what.split_once(' ').map_or(*what, |(_, m)| m);
                    let line = Line::from(vec![
                        marker(i == self.cause, true),
                        Span::raw(right(format::count(cause.count), 9)).bold(),
                        Span::raw("  "),
                        Span::styled(
                            format!("{:<7}", cause.level),
                            Style::new().fg(severity_color(logs::severity(&cause.level))),
                        ),
                        label(format!(
                            "{:<17}",
                            cause
                                .last
                                .map(|at| day_and_time(at.at + i64::from(clock.offset)))
                                .unwrap_or_default()
                        )),
                        Span::raw(truncate(message, room)),
                    ]);
                    if i == self.cause {
                        line.style(Style::new().bg(SELECTED))
                    } else {
                        line
                    }
                })
                .collect();
            f.render_widget(Paragraph::new(lines), list);
            if let Some((_, cause)) = causes.get(self.cause) {
                let lines: Vec<Line> = wrap(&cause.example, example.width as usize, 4)
                    .into_iter()
                    .map(|line| Line::from(label(line)))
                    .collect();
                f.render_widget(Paragraph::new(lines), example);
            }
        }

        let [left, _, right_side] = Layout::horizontal([
            Constraint::Percentage(50),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .areas(paths);
        for (area, title, top, color) in [
            (left, "Paths answering 5xx", &failing, BAD),
            (right_side, "Paths answering 4xx", &refused, WARN),
        ] {
            if area.height < 2 {
                continue;
            }
            let [head, list] =
                Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
            section(f, head, title, None);
            let width = (area.width as usize).saturating_sub(21).max(8);
            let lines: Vec<Line> = top
                .iter()
                .take(list.height as usize)
                .map(|(path, stat)| {
                    let n = if color == BAD { stat.c5xx } else { stat.c4xx };
                    Line::from(vec![
                        Span::raw(format!("{:<width$} ", truncate(path, width))),
                        Span::styled(right(format::count(n), 10), Style::new().fg(color).bold()),
                        label(format!("  of {}", format::compact(stat.requests as f64))),
                    ])
                })
                .collect();
            if lines.is_empty() {
                f.render_widget(Paragraph::new(label("none")), list);
            } else {
                f.render_widget(Paragraph::new(lines), list);
            }
        }
    }

    /// One line of the log, in columns
    fn log_line(&self, line: &Recent, clock: Clock, width: usize) -> Vec<Span<'static>> {
        let time = |at: Option<logs::Stamp>| {
            label(format!(
                "{:<9}",
                at.map(|at| time_of_day(at.at + i64::from(clock.offset)))
                    .unwrap_or_default()
            ))
        };
        match self.parser.read(&line.text) {
            logs::Line::Request(r) => {
                let status = match r.status {
                    0 => Span::styled("  - ", Style::new().fg(FAINT)),
                    code => value(format!("{code} "), status_color(code)),
                };
                let tail = format!(
                    " {:>9} {:>9}",
                    format::bytes(r.bytes as f64),
                    r.time
                        .map(|t| format::latency(Duration::from_secs_f64(t)))
                        .unwrap_or_default()
                );
                let room = width.saturating_sub(9 + 4 + 8 + 17 + tail.len());
                vec![
                    time(r.at),
                    status,
                    Span::raw(format!("{:<7} ", truncate(&r.method, 7))),
                    label(format!("{:<16} ", truncate(&r.client, 16))),
                    Span::raw(format!("{:<room$}", truncate(&r.target, room))),
                    label(tail),
                ]
            }
            logs::Line::Fault(fault) => vec![
                time(fault.at),
                value(
                    format!("{:<11} ", fault.level),
                    severity_color(logs::severity(fault.level)),
                ),
                label(format!("{:<16} ", truncate(fault.client, 16))),
                Span::raw(truncate(fault.message, width.saturating_sub(38))),
            ],
            logs::Line::Unread => vec![Span::styled(
                truncate(&line.text, width),
                Style::new().fg(FAINT),
            )],
        }
    }

    fn render_log(&self, f: &mut Frame, area: Rect, stats: &Stats, clock: Clock) {
        let [title, list] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
        let lines = self.lines(stats);
        let mut filters: Vec<Span> = Vec::new();
        if self.errors_only {
            filters.push(value("errors only", WARN));
        }
        if self.searching || !self.search.is_empty() {
            if !filters.is_empty() {
                filters.push(label(" · "));
            }
            filters.push(label("/"));
            filters.push(Span::styled(
                format!("{}{}", self.search, if self.searching { "▏" } else { "" }),
                Style::new().bg(FIELD),
            ));
        }
        if !filters.is_empty() {
            filters.push(label(format!(
                " · {} of ",
                format::count(lines.len() as u64)
            )));
        }
        filters.push(label(format!(
            "the last {} lines",
            format::count(stats.recent.len() as u64)
        )));
        let heading = if self.line.is_some() {
            "Log"
        } else {
            "Log · following"
        };
        section(f, title, heading, Some(Line::from(filters)));

        let height = list.height as usize;
        let picked = self.line.map(|number| {
            lines
                .partition_point(|(n, _)| *n < number)
                .min(lines.len().saturating_sub(1))
        });
        let from = match picked {
            Some(at) => at
                .saturating_sub(height / 2)
                .min(lines.len().saturating_sub(height)),
            None => lines.len().saturating_sub(height),
        };
        let width = (list.width as usize).saturating_sub(1);
        let shown: Vec<Line> = lines
            .iter()
            .enumerate()
            .skip(from)
            .take(height)
            .map(|(i, (_, line))| {
                let mut spans = vec![marker(picked == Some(i), true)];
                spans.extend(self.log_line(line, clock, width));
                let line = Line::from(spans);
                if picked == Some(i) {
                    line.style(Style::new().bg(SELECTED))
                } else {
                    line
                }
            })
            .collect();
        if shown.is_empty() {
            let why = if stats.recent.is_empty() {
                "No line read yet"
            } else {
                "No line passes the filters; c clears them"
            };
            f.render_widget(Paragraph::new(label(why)), list);
        } else {
            f.render_widget(Paragraph::new(shown), list);
        }
    }

    /// Everything read from the picked line, and the line as written
    fn render_inspector(&self, f: &mut Frame, area: Rect, stats: &Stats) {
        let lines = self.lines(stats);
        let picked = match self.line {
            Some(number) => lines.iter().find(|(n, _)| *n >= number),
            None => lines.last(),
        };
        let Some((_, line)) = picked else {
            return;
        };
        let width = area.width.saturating_sub(8).min(100);
        let inner = width.saturating_sub(4) as usize;
        let fields = self.parser.inspect(&line.text);
        let names = fields
            .iter()
            .map(|(n, _)| n.chars().count())
            .max()
            .unwrap_or(0)
            .min(24);
        let mut rows: Vec<Line> = Vec::new();
        for (name, field) in &fields {
            let color = if name.starts_with('?') || name.starts_with('$') {
                LABEL
            } else {
                ACCENT
            };
            let wrapped = wrap(field, inner.saturating_sub(names + 2), 3);
            for (i, part) in wrapped.into_iter().enumerate() {
                let name = if i == 0 {
                    truncate(name, names)
                } else {
                    String::new()
                };
                rows.push(Line::from(vec![
                    Span::styled(format!(" {name:<names$}  "), Style::new().fg(color)),
                    Span::raw(part),
                ]));
            }
        }
        rows.push(Line::from(Span::styled(
            format!(" {}", "─".repeat(inner)),
            Style::new().fg(RULE),
        )));
        // Unlike a sentence, a log line is cut where the width says
        let raw: Vec<char> = line.text.chars().collect();
        for chunk in raw.chunks(inner.max(1)).take(6) {
            rows.push(Line::from(label(format!(
                " {}",
                chunk.iter().collect::<String>()
            ))));
        }
        let height = (rows.len() as u16 + 2).min(area.height.saturating_sub(2));
        let [popup] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(popup);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new(rows).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(ACCENT))
                    .title(Span::styled(
                        " read from the line ",
                        Style::new().fg(ACCENT).bold(),
                    ))
                    .title_bottom(Line::from(label(" esc closes ")).right_aligned()),
            ),
            popup,
        );
    }
}

/// A colour for a count that is there: the one its share earned, or `floor`
/// when the share alone wouldn't have coloured it
trait MaxOr {
    fn max_or(self, floor: Color, count: u64) -> Color;
}

impl MaxOr for Color {
    fn max_or(self, floor: Color, count: u64) -> Color {
        match (self, count) {
            (Color::Reset, 1..) => floor,
            (color, _) => color,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    /// 08 Oct 2026 11:55:36 UTC
    const AT: i64 = 1_791_460_536;

    fn hit(seconds: i64, path: &str, status: u16) -> String {
        let c = logs::civil(AT + seconds);
        format!(
            "10.0.0.{} - - [{:02}/Oct/{}:{:02}:{:02}:{:02} +0000] \"GET {path} HTTP/1.1\" {status} 2048 \"-\" \"curl/8.4\" rt=0.0{}",
            seconds % 4,
            c.day,
            c.year,
            c.hour,
            c.minute,
            c.second,
            seconds % 9 + 1
        )
    }

    /// Ten minutes of traffic that doubles halfway, with an error log
    fn screen() -> LogsScreen {
        let shared = Arc::new(Shared::default());
        let parser = Parser::default();
        {
            let mut stats = shared.lock();
            let mut fold = |line: String| stats.fold(parser.read(&line), &line, AT + 86_400);
            for s in 24..624 {
                fold(hit(s, "/", 200));
                if s >= 324 {
                    fold(hit(
                        s,
                        "/search?q=tea&page=2",
                        if s % 10 == 0 { 502 } else { 200 },
                    ));
                }
                if s % 60 == 0 {
                    fold(hit(s, "/missing.png", 404));
                    fold(format!(
                        "2026/10/08 {} [error] 7#7: *{s} connect() failed (111: Connection refused) while connecting to upstream, client: 10.0.0.1, server: _, request: \"GET /search HTTP/1.1\", upstream: \"http://127.0.0.1:8080/search\"",
                        time_of_day(AT + s + i64::from(logs::local_offset()))
                    ));
                }
            }
            fold("what is this".into());
            stats.caught_up = true;
        }
        LogsScreen::new(shared, parser, "access.log +1".into(), 60)
    }

    fn draw(screen: &mut LogsScreen, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| screen.render(f, AT + 86_400)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn press(screen: &mut LogsScreen, code: KeyCode) -> bool {
        screen.key(KeyEvent::from(code))
    }

    #[test]
    fn traffic_holds_now_against_each_slot() {
        let mut s = screen();
        let text = draw(&mut s, 140, 40);
        assert!(text.contains("pepe logs · access.log +1"), "{text}");
        assert!(
            text.contains("ended 23h ago · 12:05:59 · UTC+00:00"),
            "{text}"
        );
        // The last minute: 60 of / and 60 of /search, and a 404
        assert!(text.contains("AT THE END"), "{text}");
        assert!(text.contains("2.0 req/s"), "{text}");
        assert!(text.contains("A USUAL MINUTE"), "{text}");
        assert!(text.contains("BUSIEST MINUTE"), "{text}");
        assert!(text.contains("5xx 5.0%"), "{text}");
        assert!(text.contains("REQUEST TIME"), "{text}");
        assert!(text.contains("1 of 921 lines couldn't be read"), "{text}");
        assert!(text.contains("3 Errors 10"), "{text}");
        assert!(text.contains("per minute"), "{text}");
        assert!(text.contains("now ┄┄┄"), "{text}");
        // The newest slot first; the first whole minute at half today's rate
        let newest = text
            .lines()
            .find(|l| l.contains(" 08 Oct 12:05  "))
            .unwrap();
        assert!(newest.starts_with("▌ 08 Oct 12:05"), "{newest}");
        let early = text.lines().find(|l| l.contains("08 Oct 11:57")).unwrap();
        assert!(early.contains("+98%"), "{early}");
        // By hour there is one slot, and no usual hour to speak of
        assert!(!press(&mut s, KeyCode::Char('h')));
        let text = draw(&mut s, 140, 40);
        assert!(
            text.contains("per hour") && text.contains("08 Oct 11:00"),
            "{text}"
        );
        assert!(!text.contains("A USUAL HOUR"), "{text}");
        press(&mut s, KeyCode::Char('d'));
        assert!(draw(&mut s, 140, 40).contains("Thu 08 Oct 2026"));

        // Picking walks back in time and stops at the oldest
        press(&mut s, KeyCode::Char('m'));
        press(&mut s, KeyCode::Down);
        press(&mut s, KeyCode::End);
        let text = draw(&mut s, 140, 40);
        assert_eq!(s.slot, 9);
        assert!(
            text.lines().any(|l| l.starts_with("▌ 08 Oct 11:56")),
            "{text}"
        );
        assert!(!press(&mut s, KeyCode::Esc), "esc lets go first");
        assert_eq!(s.slot, 0);
        assert!(press(&mut s, KeyCode::Esc));
    }

    #[test]
    fn paths_errors_and_the_log_have_their_views() {
        let mut s = screen();
        press(&mut s, KeyCode::Tab);
        let text = draw(&mut s, 140, 40);
        assert!(text.contains("PATHS · MOST REQUESTED"), "{text}");
        let root = text.lines().find(|l| l.starts_with("/  ")).unwrap();
        assert!(root.contains("600") && root.contains("66%"), "{root}");
        assert!(
            text.contains("STATUS") && text.contains("CLIENTS"),
            "{text}"
        );
        assert!(
            text.contains("QUERY PARAMETERS") && text.contains("page"),
            "{text}"
        );
        assert!(text.contains("curl/8.4"), "{text}");
        press(&mut s, KeyCode::Char('s'));
        let text = draw(&mut s, 140, 40);
        assert!(text.contains("PATHS · MOST 5XX"), "{text}");
        assert!(
            !text.lines().any(|l| l.starts_with("/  ")),
            "no 5xx there: {text}"
        );

        press(&mut s, KeyCode::Char('3'));
        let text = draw(&mut s, 140, 40);
        assert!(
            text.contains("ERROR LOG") && text.contains("error 10"),
            "{text}"
        );
        assert!(
            text.contains(
                "connect() failed (111: Connection refused) while connecting to upstream"
            ),
            "{text}"
        );
        assert!(
            text.contains("upstream: \"http://127.0.0.1:8080/search\""),
            "the example: {text}"
        );
        assert!(
            text.contains("PATHS ANSWERING 5XX") && text.contains("PATHS ANSWERING 4XX"),
            "{text}"
        );
        assert!(text.contains("/missing.png"), "{text}");

        press(&mut s, KeyCode::Char('4'));
        let text = draw(&mut s, 140, 40);
        assert!(text.contains("LOG · FOLLOWING"), "{text}");
        assert!(text.contains("what is this"), "{text}");
        assert!(
            text.lines()
                .any(|l| l.contains("12:05:59") && l.contains("200 GET") && l.contains("2.0 KiB")),
            "{text}"
        );

        // Errors only, then a search within them
        press(&mut s, KeyCode::Char('x'));
        let text = draw(&mut s, 140, 40);
        assert!(
            text.contains("errors only · 50 of the last 921 lines"),
            "{text}"
        );
        assert!(
            !text.contains("what is this") && text.contains("502 GET"),
            "{text}"
        );
        press(&mut s, KeyCode::Char('/'));
        for c in "MISSING".chars() {
            assert!(
                !press(&mut s, KeyCode::Char(c)),
                "typing, q included, isn't leaving"
            );
        }
        press(&mut s, KeyCode::Enter);
        let text = draw(&mut s, 140, 40);
        assert!(text.contains("/MISSING · 10 of the last"), "{text}");

        // Pick the line before the last and open it
        press(&mut s, KeyCode::Up);
        press(&mut s, KeyCode::Up);
        assert!(s.line.is_some());
        press(&mut s, KeyCode::Enter);
        let text = draw(&mut s, 140, 40);
        assert!(
            text.contains("path") && text.contains("/missing.png"),
            "{text}"
        );
        assert!(
            text.contains("request time") && text.contains("user agent"),
            "{text}"
        );
        assert!(text.contains("esc closes"), "{text}");
        press(&mut s, KeyCode::Esc);
        assert!(!s.inspecting);
        // Down past the end follows again
        press(&mut s, KeyCode::PageDown);
        assert_eq!(s.line, None);
        press(&mut s, KeyCode::Char('c'));
        assert!(!s.errors_only && s.search.is_empty());
    }

    #[test]
    fn small_and_empty_screens_still_draw() {
        let mut s = screen();
        for tab in TABS {
            s.tab = tab;
            for (width, height) in [(60, 16), (80, 24), (100, 30), (200, 60)] {
                let text = draw(&mut s, width, height);
                assert!(
                    text.contains("pepe logs"),
                    "{tab:?} {width}×{height}: {text}"
                );
            }
        }
        assert!(draw(&mut s, 40, 10).contains("needs 60×16"));
        let mut empty = LogsScreen::new(
            Arc::new(Shared::default()),
            Parser::default(),
            "stdin".into(),
            60,
        );
        for tab in TABS {
            empty.tab = tab;
            let text = draw(&mut empty, 100, 30);
            assert!(text.contains("reading"), "{text}");
        }
        press(&mut empty, KeyCode::Enter);
        press(&mut empty, KeyCode::Up);
        draw(&mut empty, 100, 30);
        press(&mut empty, KeyCode::Char('?'));
        assert!(draw(&mut empty, 100, 30).contains("keys"));
        assert_eq!(spark(&[0, 1, 4, 8]), " ▁▄█");
        assert_eq!(
            (scroll(0, 50, 10), scroll(12, 50, 10), scroll(49, 50, 10)),
            (0, 3, 40)
        );
    }
}
