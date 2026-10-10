//! The ping screen: each target's latency as a line over time, where the
//! time of its last and typical ping went, and the pings themselves

use std::io::Write as _;
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

use super::kit::{about_line, chips_fit, help, marker, tabs, wrap, FAINT, SELECTED};
use super::view::{label, status_color, truncate, value, ACCENT, BAD, GOOD, LABEL, RULE, WARN};
use super::{format, theme};
use crate::ping::{glyph, percent, Kind, Phases, Sample, Shared, State, TargetStats};

/// How often the screen is drawn again while nothing is pressed
const FRAME: Duration = Duration::from_millis(200);
const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 14;
/// The graph's axis labels
const AXIS: u16 = 8;
/// The least and the most of the run the graph shows, in seconds
const WINDOW_MIN: f32 = 5.0;
const WINDOW_MAX: f32 = 7.0 * 24.0 * 3600.0;
/// Targets get these colours in turn when `--color` doesn't say
const PALETTE: [Color; 8] = [
    ACCENT,
    Color::Green,
    Color::Yellow,
    Color::Magenta,
    Color::Blue,
    Color::LightRed,
    Color::Cyan,
    Color::White,
];
/// The phases' colours in the breakdown, in their order
const PHASE_COLORS: [Color; 5] = [
    Color::Indexed(117),
    Color::Indexed(81),
    Color::Indexed(186),
    Color::Indexed(215),
    Color::Indexed(150),
];
const PHASE_NAMES: [&str; 5] = ["DNS", "connect", "TLS", "first byte", "download"];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Graph,
    Phases,
    Pings,
}

const TABS: [Tab; 3] = [Tab::Graph, Tab::Phases, Tab::Pings];

/// How the screen is set up from the command line
#[derive(Debug, Clone)]
pub struct Look {
    /// Seconds of the run the graph shows
    pub window: f32,
    /// The graph's floor and ceiling, in microseconds
    pub ymin: Option<u64>,
    pub ymax: Option<u64>,
    pub simple: bool,
    pub colors: Vec<Color>,
    pub bell: bool,
}

pub struct PingScreen {
    shared: Arc<Shared>,
    tab: Tab,
    /// The target the keys are on
    picked: usize,
    window: f32,
    /// The graph shows the whole run
    whole: bool,
    ymin: Option<u64>,
    ymax: Option<u64>,
    /// The floor is zero
    zero: bool,
    log_scale: bool,
    simple: bool,
    fail_marks: bool,
    /// The table's numbers are over the whole run rather than the window
    table_whole: bool,
    colors: Vec<Color>,
    bell: bool,
    /// Pings view: every target's pings together
    all: bool,
    /// Pings view: the line picked, by its place in the list; none
    /// follows the end
    line: Option<usize>,
    failures_only: bool,
    inspecting: bool,
    show_help: bool,
}

/// A colour named on the command line
pub fn parse_color(text: &str) -> Option<Color> {
    let text = text.trim().to_ascii_lowercase();
    if let Some(hex) = text.strip_prefix('#') {
        if hex.len() == 6 {
            let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            return Some(Color::Rgb(channel(0)?, channel(2)?, channel(4)?));
        }
        return None;
    }
    Some(match text.as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" | "grey" => Color::Gray,
        "dark-gray" | "dark-grey" | "darkgray" => Color::DarkGray,
        "light-red" | "lightred" => Color::LightRed,
        "light-green" | "lightgreen" => Color::LightGreen,
        "light-yellow" | "lightyellow" => Color::LightYellow,
        "light-blue" | "lightblue" => Color::LightBlue,
        "light-magenta" | "lightmagenta" => Color::LightMagenta,
        "light-cyan" | "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        _ => return None,
    })
}

/// A latency in microseconds, short
fn us(v: u64) -> String {
    format::latency_short(v)
}

/// The samples the pings view lists: a target's, or every target's in
/// time order
fn listed(state: &State, picked: usize, all: bool, failures_only: bool) -> Vec<&Sample> {
    let mut samples: Vec<&Sample> = if all {
        let mut v: Vec<&Sample> = state.targets.iter().flat_map(|t| t.recent.iter()).collect();
        v.sort_by(|a, b| a.at.cmp(&b.at).then(a.target.cmp(&b.target)));
        v
    } else {
        state
            .targets
            .get(picked)
            .map(|t| t.recent.iter().collect())
            .unwrap_or_default()
    };
    if failures_only {
        samples.retain(|s| !s.ok() || !s.violations.is_empty());
    }
    samples
}

impl PingScreen {
    pub fn new(shared: Arc<Shared>, look: Look) -> Self {
        PingScreen {
            shared,
            tab: Tab::Graph,
            picked: 0,
            window: look.window.clamp(WINDOW_MIN, WINDOW_MAX),
            whole: false,
            ymin: look.ymin,
            ymax: look.ymax,
            zero: look.ymin == Some(0),
            log_scale: false,
            simple: look.simple,
            fail_marks: true,
            table_whole: false,
            colors: look.colors,
            bell: look.bell,
            all: true,
            line: None,
            failures_only: false,
            inspecting: false,
            show_help: false,
        }
    }

    fn color(&self, target: usize) -> Color {
        self.colors
            .get(target)
            .copied()
            .unwrap_or(PALETTE[target % PALETTE.len()])
    }

    fn move_line(&mut self, by: i64) {
        let state = self.shared.lock();
        let len = listed(&state, self.picked, self.all, self.failures_only).len();
        drop(state);
        let Some(last) = len.checked_sub(1) else {
            self.line = None;
            return;
        };
        let at = self.line.map_or(last as i64 + 1, |l| l.min(last) as i64);
        let to = at + by;
        self.line = (to <= last as i64).then(|| to.max(0) as usize);
    }

    fn move_by(&mut self, by: i64) {
        match self.tab {
            Tab::Pings => self.move_line(by),
            Tab::Graph | Tab::Phases => {
                let count = self.shared.lock().targets.len().max(1);
                self.picked = (self.picked as i64 + by).rem_euclid(count as i64) as usize;
            }
        }
    }

    /// True when the key asks to leave
    fn key(&mut self, key: KeyEvent) -> bool {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return true;
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
            KeyCode::Char(c @ '1'..='3') => self.tab = TABS[c as usize - '1' as usize],
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_by(1),
            KeyCode::PageUp => self.move_by(-10),
            KeyCode::PageDown => self.move_by(10),
            KeyCode::Home => self.move_by(-i64::from(u32::MAX)),
            KeyCode::End => self.move_by(i64::from(u32::MAX)),
            KeyCode::Char(' ') => {
                let paused = !self.shared.lock().paused;
                self.shared.set_paused(paused);
            }
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.whole = false;
                self.window = (self.window / 2.0).max(WINDOW_MIN);
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.whole = false;
                self.window = (self.window * 2.0).min(WINDOW_MAX);
            }
            KeyCode::Char('w') => self.whole = !self.whole,
            KeyCode::Char('0') => {
                self.zero = !self.zero;
                if self.zero {
                    self.ymin = None;
                }
            }
            KeyCode::Char('l') => self.log_scale = !self.log_scale,
            KeyCode::Char('s') => self.simple = !self.simple,
            KeyCode::Char('f') => self.fail_marks = !self.fail_marks,
            KeyCode::Char('t') => self.table_whole = !self.table_whole,
            KeyCode::Char('a') if self.tab == Tab::Pings => {
                self.all = !self.all;
                self.line = None;
            }
            KeyCode::Char('x') if self.tab == Tab::Pings => {
                self.failures_only = !self.failures_only;
                self.line = None;
            }
            KeyCode::Enter if self.tab == Tab::Pings => {
                // Nothing picked: the newest, which is one back from the end
                if self.line.is_none() {
                    self.move_line(-1);
                }
                self.inspecting = self.line.is_some();
            }
            KeyCode::Enter => self.tab = Tab::Phases,
            KeyCode::Esc => {
                if self.tab == Tab::Pings && self.line.is_some() {
                    self.line = None;
                } else if self.tab == Tab::Pings && self.failures_only {
                    self.failures_only = false;
                } else if self.whole {
                    self.whole = false;
                } else {
                    return true;
                }
            }
            _ => {}
        }
        false
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
            let bells = {
                let mut state = self.shared.lock();
                std::mem::take(&mut state.bells)
            };
            if self.bell && bells > 0 {
                let mut out = std::io::stdout();
                let _ = out.write_all(b"\x07");
                let _ = out.flush();
            }
            terminal.draw(|f| theme::draw(f, |f| self.render(f)))?;
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

    fn render(&mut self, f: &mut Frame) {
        let area = f.area();
        if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
            f.render_widget(
                Paragraph::new(format!(
                    "pepe ping needs {MIN_WIDTH}×{MIN_HEIGHT}; this is {}×{}",
                    area.width, area.height
                )),
                area,
            );
            return;
        }
        let shared = self.shared.clone();
        let state = shared.lock();
        self.picked = self.picked.min(state.targets.len().saturating_sub(1));
        let now = state.elapsed().as_secs_f32();
        let rows = state.targets.len() as u16;
        let table_height = (rows + 1).min(area.height / 3);

        let [title, _, table, _, tab_bar, _, body, footer] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(table_height),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .areas(area);

        self.render_title(f, title, &state, now);
        self.render_table(f, table, &state, now);
        let titles: Vec<String> = ["1 Graph", "2 Phases", "3 Pings"]
            .iter()
            .map(|t| t.to_string())
            .collect();
        let at = TABS.iter().position(|t| *t == self.tab).unwrap_or(0);
        f.render_widget(Paragraph::new(tabs(&titles, at)), tab_bar);
        match self.tab {
            Tab::Graph => self.render_graph(f, body, &state, now),
            Tab::Phases => self.render_phases(f, body, &state),
            Tab::Pings => self.render_pings(f, body, &state),
        }

        let paused = state.paused;
        let keys: Vec<(&str, &str)> = match self.tab {
            Tab::Graph => vec![
                ("↑↓", "pick a target"),
                ("+ -", "zoom"),
                ("w", "whole run"),
                ("space", if paused { "resume" } else { "pause" }),
                ("tab", "view"),
                ("?", "keys"),
                ("q", "quit"),
            ],
            Tab::Phases => vec![
                ("↑↓", "pick a target"),
                ("t", "table: run or window"),
                ("tab", "view"),
                ("?", "keys"),
                ("q", "quit"),
            ],
            Tab::Pings => vec![
                ("↑↓", "pick a ping"),
                ("enter", "open it"),
                ("x", "failures only"),
                ("a", "one target or all"),
                ("tab", "view"),
                ("?", "keys"),
                ("q", "quit"),
            ],
        };
        f.render_widget(Paragraph::new(chips_fit(&keys, footer.width)), footer);

        if self.inspecting {
            self.render_inspector(f, area, &state);
        }
        if self.show_help {
            help(
                f,
                area,
                &[
                    ("tab ← →", "next or previous view; 1-3 pick one"),
                    ("↑ ↓ / j k", "pick a target, or a ping in the list"),
                    ("space", "pause and resume the pings"),
                    ("+ -", "graph: half or twice the time shown"),
                    ("w", "graph: the whole run; esc goes back"),
                    ("0", "graph: start at zero"),
                    ("l", "graph: a log scale"),
                    ("s", "graph: dots instead of braille"),
                    ("f", "graph: hide or show the failures"),
                    ("t", "table: the whole run, or what the graph shows"),
                    ("a x", "pings: one target or all; failures only"),
                    ("enter", "pings: everything about the picked one"),
                    ("esc", "let go of the pick, then quit"),
                    ("?", "close this help"),
                    ("q", "quit; the summary is left in the shell"),
                ],
                &[
                    "Each ping opens a connection, so every phase is measured;".into(),
                    "--keep-alive keeps it instead, as a browser would.".into(),
                    about_line(),
                ],
            );
        }
    }

    fn render_title(&self, f: &mut Frame, area: Rect, state: &State, now: f32) {
        let settings = &self.shared.settings;
        let status = if state.paused {
            value("paused", WARN)
        } else if state.done {
            label("ended")
        } else {
            value("● live", GOOD)
        };
        let right = Line::from(vec![
            status,
            label(format!(
                " · {} · every {}",
                format::clock(Duration::from_secs_f32(now.max(0.0))),
                crate::ping::every_text(settings.every)
            )),
        ]);
        let names: Vec<&str> = state
            .targets
            .iter()
            .map(|t| t.target.shown.as_str())
            .collect();
        let room = (area.width as usize).saturating_sub(right.width() + 14);
        let left = Line::from(vec![
            Span::styled("pepe ping", Style::new().fg(ACCENT).bold()),
            label(" · "),
            Span::raw(truncate(&names.join(", "), room)).bold(),
        ]);
        f.render_widget(Paragraph::new(left), area);
        f.render_widget(Paragraph::new(right.right_aligned()), area);
    }

    /// Each target's numbers over the window or the run
    fn render_table(&self, f: &mut Frame, area: Rect, state: &State, now: f32) {
        if area.height == 0 {
            return;
        }
        let window = self.shown_window(now);
        let title = if self.table_whole {
            "whole run".to_string()
        } else {
            format!("last {}", format::span(Duration::from_secs_f32(window)))
        };
        let wide = area.width >= 100;
        let name_width = (area.width as usize / 4).clamp(12, 32);
        let mut columns: Vec<(&str, usize)> = vec![
            ("last", 7),
            ("min", 7),
            ("avg", 7),
            ("max", 7),
            ("jitter", 7),
            ("p95", 7),
            ("p99", 7),
            ("loss", 6),
            ("t/o", 5),
            ("sent", 7),
        ];
        if !wide {
            columns.retain(|(name, _)| !matches!(*name, "p95" | "jitter" | "t/o"));
        }
        let mut header = vec![
            Span::raw(" "),
            Span::styled(
                format!("{:<name_width$}", title.to_uppercase()),
                Style::new().fg(LABEL).bold(),
            ),
        ];
        for (name, width) in &columns {
            header.push(Span::styled(
                format!("{:>width$}", name),
                Style::new().fg(LABEL).bold(),
            ));
        }
        f.render_widget(Paragraph::new(Line::from(header)), area);
        let rows = area.height.saturating_sub(1) as usize;
        // The picked target is always among the rows shown
        let first = self.picked.saturating_sub(rows.saturating_sub(1));
        for (row, (index, t)) in state
            .targets
            .iter()
            .enumerate()
            .skip(first)
            .take(rows)
            .enumerate()
        {
            let summary = if self.table_whole {
                t.whole()
            } else {
                t.window(now, window)
            };
            let picked = index == self.picked;
            let y = area.y + 1 + row as u16;
            let line_area = Rect::new(area.x, y, area.width, 1);
            if picked {
                f.render_widget(
                    Paragraph::new("").style(Style::new().bg(SELECTED)),
                    line_area,
                );
            }
            let mut spans = vec![
                marker(picked, true),
                Span::styled("■ ", Style::new().fg(self.color(index))),
                Span::raw(format!(
                    "{:<width$}",
                    truncate(&t.target.name, name_width - 2),
                    width = name_width - 2
                )),
            ];
            let lat = |v: u64, width: usize| -> Span<'static> {
                if summary.answered == 0 {
                    label(format!("{:>width$}", "–"))
                } else {
                    Span::raw(format!("{:>width$}", us(v)))
                }
            };
            for (name, width) in &columns {
                spans.push(match *name {
                    "last" => match (summary.last, t.recent.back()) {
                        (Some(v), Some(last)) if last.answered => {
                            let color = if last.violations.is_empty() {
                                Color::Reset
                            } else {
                                WARN
                            };
                            Span::styled(format!("{:>width$}", us(v)), Style::new().fg(color))
                        }
                        (_, Some(last)) if !last.answered => {
                            Span::styled(format!("{:>width$}", "lost"), Style::new().fg(BAD))
                        }
                        _ => label(format!("{:>width$}", "–")),
                    },
                    "min" => lat(summary.min, *width),
                    "avg" => lat(summary.avg, *width),
                    "max" => lat(summary.max, *width),
                    "jitter" => lat(summary.jitter, *width),
                    "p95" => lat(summary.p95, *width),
                    "p99" => lat(summary.p99, *width),
                    "loss" => {
                        let loss = summary.loss();
                        let color = match loss {
                            l if l >= 0.05 => BAD,
                            l if l > 0.0 => WARN,
                            _ => Color::Reset,
                        };
                        Span::styled(format!("{:>width$}", percent(loss)), Style::new().fg(color))
                    }
                    "t/o" => Span::raw(format!("{:>width$}", summary.timeouts)),
                    "sent" => {
                        Span::raw(format!("{:>width$}", format::compact(summary.sent as f64)))
                    }
                    _ => Span::raw(""),
                });
            }
            f.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
    }

    /// Seconds the graph shows: the window, or the run so far
    fn shown_window(&self, now: f32) -> f32 {
        if self.whole {
            now.max(WINDOW_MIN)
        } else {
            self.window
        }
    }

    fn render_graph(&self, f: &mut Frame, area: Rect, state: &State, now: f32) {
        let height = area.height.saturating_sub(1);
        let width = area.width.saturating_sub(AXIS + 1);
        if height < 2 || width < 4 {
            return;
        }
        let window = self.shown_window(now);
        let from = now - window;
        // The range: what is on screen, or what was asked for
        let mut lo = u64::MAX;
        let mut hi = 0u64;
        let mut any = false;
        for t in &state.targets {
            let start = t.points.partition_point(|p| p.at < from);
            for p in &t.points[start..] {
                if p.answered {
                    lo = lo.min(u64::from(p.us));
                    hi = hi.max(u64::from(p.us));
                    any = true;
                }
            }
        }
        if !any {
            lo = 0;
            hi = 100_000;
        }
        let lo = match (self.ymin, self.zero) {
            (Some(min), _) => min,
            (None, true) => 0,
            (None, false) => (lo as f64 * 0.9) as u64,
        };
        let hi = match self.ymax {
            Some(max) => max.max(lo + 1),
            None => ((hi as f64 * 1.1) as u64).max(lo + 1_000),
        };
        let buf_width = width as usize * 2;
        let buf_height = height as usize * 4;
        let (cols, rows) = (width as usize, height as usize);
        let row_of = |v: u64| -> usize {
            let t = if self.log_scale {
                let (lo, hi, v) = (lo.max(1) as f64, hi.max(2) as f64, v.max(1) as f64);
                (v.ln() - lo.ln()) / (hi.ln() - lo.ln())
            } else {
                (v as f64 - lo as f64) / (hi as f64 - lo as f64)
            };
            let t = t.clamp(0.0, 1.0);
            buf_height - 1 - ((t * (buf_height - 1) as f64).round() as usize).min(buf_height - 1)
        };
        let col_of = |at: f32| -> usize {
            let t = ((at - from) / window).clamp(0.0, 1.0);
            ((t * (buf_width - 1) as f32).round() as usize).min(buf_width - 1)
        };

        let mut bits = vec![0u32; cols * rows];
        let mut owner = vec![None::<usize>; cols * rows];
        // Failures, and answers that broke the SLO or were 5xx, on the top row
        let mut marks: Vec<(usize, usize, char)> = Vec::new();
        for (index, t) in state.targets.iter().enumerate() {
            let start = t.points.partition_point(|p| p.at < from);
            let mut prev: Option<(usize, usize)> = None;
            for p in &t.points[start..] {
                let x = col_of(p.at);
                if !p.answered {
                    marks.push((x / 2, index, '✖'));
                    prev = None;
                    continue;
                }
                if p.slow || p.status >= 500 {
                    marks.push((x / 2, index, if p.slow { '▲' } else { '!' }));
                }
                let y = row_of(u64::from(p.us));
                // Join with the last point: across the columns between,
                // then up or down to this one
                let (x0, ya) = match prev {
                    Some((px, py)) if px < x => (px + 1, py),
                    _ => (x, y),
                };
                for xx in x0..=x {
                    let (a, b) = if xx == x {
                        (ya.min(y), ya.max(y))
                    } else {
                        (ya, ya)
                    };
                    for yy in a..=b {
                        let i = (yy / 4) * cols + xx / 2;
                        if self.simple {
                            bits[i] = 1;
                        } else {
                            bits[i] |= braille_bit(xx % 2, yy % 4);
                        }
                        // The picked target's line is on top
                        owner[i] = Some(match owner[i] {
                            Some(o) if o == self.picked => o,
                            _ => index,
                        });
                    }
                }
                prev = Some((x, y));
            }
        }
        let buf = f.buffer_mut();
        for row in 0..rows {
            for col in 0..cols {
                let i = row * cols + col;
                if let (Some(line), true) = (owner[i], bits[i] != 0) {
                    if let Some(cell) =
                        buf.cell_mut((area.x + AXIS + col as u16, area.y + row as u16))
                    {
                        let symbol = if self.simple {
                            '•'
                        } else {
                            char::from_u32(0x2800 + bits[i]).unwrap_or(' ')
                        };
                        let color = if line == self.picked || state.targets.len() == 1 {
                            self.color(line)
                        } else {
                            dim(self.color(line))
                        };
                        cell.set_char(symbol).set_fg(color);
                    }
                }
            }
        }
        if self.fail_marks {
            for (col, index, mark) in marks {
                if let Some(cell) = buf.cell_mut((area.x + AXIS + col as u16, area.y)) {
                    cell.set_char(mark)
                        .set_fg(match (mark, state.targets.len()) {
                            ('✖', 1) => BAD,
                            ('✖', _) => self.color(index),
                            _ => WARN,
                        });
                }
            }
        }
        // The axis: the ceiling, the floor, and a value between
        let axis = AXIS as usize - 1;
        let mid = if self.log_scale {
            ((lo.max(1) as f64).ln().midpoint((hi as f64).ln())).exp() as u64
        } else {
            lo.midpoint(hi)
        };
        buf.set_string(
            area.x,
            area.y,
            format!("{:>axis$}", us(hi)),
            Style::new().fg(LABEL),
        );
        if height > 2 {
            buf.set_string(
                area.x,
                area.y + height / 2,
                format!("{:>axis$}", us(mid)),
                Style::new().fg(LABEL),
            );
        }
        buf.set_string(
            area.x,
            area.y + height - 1,
            format!("{:>axis$}", us(lo)),
            Style::new().fg(LABEL),
        );
        for row in 0..height {
            buf.set_string(area.x + AXIS - 1, area.y + row, "│", Style::new().fg(RULE));
        }
        // Under the plot: when it starts, when it ends
        let y = area.y + height;
        let left = format!("-{}", format::span(Duration::from_secs_f32(window)));
        let right = if state.paused { "paused" } else { "now" };
        buf.set_string(area.x + AXIS, y, &left, Style::new().fg(LABEL));
        let scale = match (self.log_scale, self.whole) {
            (true, true) => "log · whole run",
            (true, false) => "log",
            (false, true) => "whole run",
            (false, false) => "",
        };
        if !scale.is_empty() {
            let x = area.x + AXIS + (width / 2).saturating_sub(scale.len() as u16 / 2);
            buf.set_string(x, y, scale, Style::new().fg(FAINT));
        }
        let x = (area.x + AXIS + width).saturating_sub(right.len() as u16);
        buf.set_string(
            x,
            y,
            right,
            Style::new().fg(if state.paused { WARN } else { LABEL }),
        );
    }

    /// Where the picked target's time goes
    fn render_phases(&self, f: &mut Frame, area: Rect, state: &State) {
        let Some(t) = state.targets.get(self.picked) else {
            return;
        };
        let mut lines: Vec<Line<'static>> = Vec::new();
        let legend: Vec<Span> = PHASE_NAMES
            .iter()
            .zip(PHASE_COLORS)
            .flat_map(|(name, color)| {
                [
                    Span::styled("■ ", Style::new().fg(color)),
                    label(format!("{name}  ")),
                ]
            })
            .collect();
        lines.push(Line::from(
            [
                vec![Span::styled(
                    format!("{} ", t.target.name),
                    Style::new().fg(self.color(self.picked)).bold(),
                )],
                legend,
            ]
            .concat(),
        ));
        lines.push(Line::raw(""));
        let bar_width = (area.width as usize).saturating_sub(12).max(10);
        let last = t.recent.iter().rev().find(|s| s.answered);
        let median = Phases {
            dns: median_of(&t.dns),
            connect: median_of(&t.connect),
            tls: median_of(&t.tls_full).or(median_of(&t.tls_resumed)),
            ttfb: median_of(&t.ttfb),
            download: median_of(&t.download),
        };
        match &t.target.kind {
            Kind::Cmd(_) => {
                lines.push(Line::from(label(
                    "A command has one phase: how long it runs.",
                )));
            }
            _ => {
                if let Some(last) = last {
                    lines.extend(phase_bar("last", &last.phases, last.total, bar_width));
                    lines.push(Line::raw(""));
                }
                if t.answered > 0 {
                    let total: Duration = median.each().iter().filter_map(|(_, d)| *d).sum();
                    lines.extend(phase_bar("median", &median, total, bar_width));
                    lines.push(Line::raw(""));
                }
            }
        }
        // httpstat's running totals: where each phase ends
        if let Some(last) = last.filter(|l| l.phases.connect.is_some() || l.phases.ttfb.is_some()) {
            let mut acc = Duration::ZERO;
            let mut spans = vec![label("running  ")];
            for ((name, took), stop) in last.phases.each().iter().zip([
                "namelookup",
                "connect",
                "pretransfer",
                "starttransfer",
                "total",
            ]) {
                let Some(took) = took else {
                    continue;
                };
                acc += *took;
                let _ = name;
                spans.push(label(format!("{stop} ")));
                spans.push(value(format::latency(acc), Color::Reset));
                spans.push(label("  "));
            }
            lines.push(Line::from(spans));
            lines.push(Line::raw(""));
        }
        if t.tls_full.count() + t.tls_resumed.count() > 0 {
            let mut spans = vec![label("tls      ")];
            if t.tls_full.count() > 0 {
                spans.push(value(
                    format::latency(Duration::from_micros(t.tls_full.percentile(50.0))),
                    Color::Reset,
                ));
                spans.push(label(format!(" full ×{}", t.tls_full.count())));
            }
            if t.tls_resumed.count() > 0 {
                if t.tls_full.count() > 0 {
                    spans.push(label(" · "));
                }
                spans.push(value(
                    format::latency(Duration::from_micros(t.tls_resumed.percentile(50.0))),
                    GOOD,
                ));
                spans.push(label(format!(" resumed ×{}", t.tls_resumed.count())));
            }
            lines.push(Line::from(spans));
        }
        for line in about_target(t, state) {
            lines.push(line);
        }
        if t.reused > 0 {
            lines.push(Line::from(label(format!(
                "{} of {} pings went on a kept connection, with no DNS, connect or TLS of their own",
                t.reused, t.answered
            ))));
        }
        let findings = crate::diagnose::findings(t, &self.shared.settings, crate::logs::wall());
        if !findings.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::from(label("LOOK AT")));
            let width = (area.width as usize).saturating_sub(4).max(20);
            for finding in findings {
                let color = match finding.level {
                    crate::diagnose::Level::Good => GOOD,
                    crate::diagnose::Level::Note => LABEL,
                    crate::diagnose::Level::Warn => WARN,
                    crate::diagnose::Level::Bad => BAD,
                };
                for (i, piece) in wrap(&finding.text, width, 3).into_iter().enumerate() {
                    lines.push(Line::from(vec![
                        Span::styled(
                            if i == 0 {
                                format!("{} ", finding.level.glyph())
                            } else {
                                "  ".into()
                            },
                            Style::new().fg(color),
                        ),
                        Span::raw(piece),
                    ]));
                }
            }
        }
        f.render_widget(Paragraph::new(lines), area);
    }

    fn render_pings(&self, f: &mut Frame, area: Rect, state: &State) {
        let samples = listed(state, self.picked, self.all, self.failures_only);
        let height = area.height.saturating_sub(1) as usize;
        let wide = area.width >= 110;
        let name_width = if self.all { 18 } else { 0 };
        let mut header = vec![Span::raw(" "), label(format!("{:<9}", "time"))];
        if self.all {
            header.push(label(format!("{:<name_width$}", "target")));
        }
        header.push(label(format!("{:<6}", "seq")));
        header.push(label(format!("{:<8}", "status")));
        for name in ["dns", "tcp", "tls", "ttfb", "dl", "total"] {
            if !wide && matches!(name, "dns" | "dl") {
                continue;
            }
            header.push(label(format!("{:>8}", name)));
        }
        header.push(label(format!("{:>9}", "bytes")));
        header.push(label("  note"));
        let mut lines = vec![Line::from(header)];
        if samples.is_empty() {
            lines.push(Line::from(label(if self.failures_only {
                "  no failures"
            } else {
                "  nothing yet"
            })));
            f.render_widget(Paragraph::new(lines), area);
            return;
        }
        f.render_widget(
            Paragraph::new(lines),
            Rect::new(area.x, area.y, area.width, 1),
        );
        let last = samples.len() - 1;
        let picked = self.line.map(|l| l.min(last));
        let end = picked.map_or(last, |p| p.max(height.saturating_sub(1)).min(last));
        let start = (end + 1).saturating_sub(height);
        let offset = i64::from(crate::logs::local_offset());
        for (i, sample) in samples.iter().enumerate().take(end + 1).skip(start) {
            let is_picked = picked == Some(i);
            let mut spans = vec![
                marker(is_picked, true),
                Span::raw(format!(
                    "{:<9}",
                    crate::logs::time_of_day(sample.wall + offset)
                )),
            ];
            if self.all {
                let name = &state.targets[sample.target].target.name;
                spans.push(Span::styled(
                    format!("{:<name_width$}", truncate(name, name_width - 1)),
                    Style::new().fg(self.color(sample.target)),
                ));
            }
            spans.push(Span::raw(format!("{:<6}", sample.seq)));
            spans.push(match (sample.answered, sample.status, &sample.error) {
                (_, _, Some(_)) => value(
                    format!("{:<8}", if sample.timeout { "timeout" } else { "failed" }),
                    BAD,
                ),
                (true, Some(status), None) => match sample.target_kind_is_cmd(state) {
                    true => value(format!("{:<8}", format!("exit {status}")), GOOD),
                    false => value(format!("{:<8}", status), status_color(status)),
                },
                (true, None, None) => value(format!("{:<8}", "open"), GOOD),
                (false, _, None) => value(format!("{:<8}", "lost"), BAD),
            });
            let slow = |key: &str| sample.violations.iter().any(|v| v.key == key);
            for (name, key, took) in [
                ("dns", "dns", sample.phases.dns),
                ("tcp", "connect", sample.phases.connect),
                ("tls", "tls", sample.phases.tls),
                ("ttfb", "ttfb", sample.phases.ttfb),
                ("dl", "download", sample.phases.download),
            ] {
                if !wide && matches!(name, "dns" | "dl") {
                    continue;
                }
                spans.push(match took {
                    Some(d) => Span::styled(
                        format!("{:>8}", format::latency_short(d.as_micros() as u64)),
                        Style::new().fg(if slow(key) { WARN } else { Color::Reset }),
                    ),
                    None => label(format!("{:>8}", if sample.reused { "kept" } else { "–" })),
                });
            }
            spans.push(Span::styled(
                format!(
                    "{:>8}",
                    format::latency_short(sample.total.as_micros() as u64)
                ),
                Style::new()
                    .fg(if slow("total") {
                        WARN
                    } else if !sample.answered {
                        BAD
                    } else {
                        Color::Reset
                    })
                    .bold(),
            ));
            spans.push(Span::raw(format!(
                "{:>9}",
                if sample.bytes > 0 {
                    format::bytes(sample.bytes as f64)
                } else {
                    String::new()
                }
            )));
            let mut note = String::new();
            if let Some(error) = &sample.error {
                note = error.clone();
            } else if !sample.hops.is_empty() {
                note = format!(
                    "{} redirect{}",
                    sample.hops.len(),
                    if sample.hops.len() > 1 { "s" } else { "" }
                );
            } else if sample.tls.as_ref().is_some_and(|t| t.resumed) {
                note = "tls resumed".into();
            }
            if !sample.violations.is_empty() {
                let broke: Vec<&str> = sample.violations.iter().map(|v| v.key).collect();
                if !note.is_empty() {
                    note.push_str(" · ");
                }
                note.push_str(&format!("slo {}", broke.join(", ")));
            }
            spans.push(label(format!("  {note}")));
            let y = area.y + 1 + (i - start) as u16;
            let line_area = Rect::new(area.x, y, area.width, 1);
            if is_picked {
                f.render_widget(
                    Paragraph::new("").style(Style::new().bg(SELECTED)),
                    line_area,
                );
            }
            f.render_widget(Paragraph::new(Line::from(spans)), line_area);
        }
    }

    /// Everything about the picked ping
    fn render_inspector(&self, f: &mut Frame, area: Rect, state: &State) {
        let samples = listed(state, self.picked, self.all, self.failures_only);
        let Some(sample) = self
            .line
            .and_then(|l| samples.get(l.min(samples.len().saturating_sub(1))))
        else {
            return;
        };
        let t = &state.targets[sample.target];
        let width = (area.width - 4).min(100);
        let inner = width as usize - 4;
        let kv = |name: &str, text: String| -> Line<'static> {
            Line::from(vec![label(format!("{name:<14}")), Span::raw(text)])
        };
        let mut lines: Vec<Line<'static>> = Vec::new();
        lines.push(kv(
            "target",
            format!("{} · {}", t.target.name, t.target.shown),
        ));
        lines.push(kv(
            "when",
            format!(
                "{} · ping {} · {} into the run",
                crate::logs::day_and_time(sample.wall + i64::from(crate::logs::local_offset())),
                sample.seq,
                format::clock(sample.at)
            ),
        ));
        lines.push(Line::from(vec![
            label(format!("{:<14}", "outcome")),
            value(
                sample.outcome(),
                if sample.ok() && sample.violations.is_empty() {
                    GOOD
                } else if sample.ok() {
                    WARN
                } else {
                    BAD
                },
            ),
            label(format!(
                " in {}{}",
                format::latency(sample.total),
                if sample.reused {
                    " on a kept connection"
                } else {
                    ""
                }
            )),
        ]));
        let mut phases = Vec::new();
        for (name, took) in sample.phases.each() {
            if let Some(took) = took {
                phases.push(format!("{name} {}", format::latency(took)));
            }
        }
        if !phases.is_empty() {
            lines.push(kv("phases", phases.join(" · ")));
        }
        for v in &sample.violations {
            lines.push(Line::from(vec![
                label(format!("{:<14}", "slo")),
                value(
                    format!(
                        "{} {} is over {}",
                        v.key,
                        format::latency(v.actual),
                        format::latency(v.limit)
                    ),
                    WARN,
                ),
            ]));
        }
        if let (Some(remote), Some(local)) = (sample.remote, sample.local) {
            lines.push(kv("connection", format!("{local} → {remote}")));
        } else if let Some(remote) = sample.remote {
            lines.push(kv("connection", format!("→ {remote}")));
        }
        if let Some(tls) = &sample.tls {
            let mut said = tls.version.clone();
            if !tls.cipher.is_empty() {
                said.push_str(&format!(" · {}", tls.cipher));
            }
            if let Some(alpn) = &tls.alpn {
                said.push_str(&format!(" · {alpn}"));
            }
            said.push_str(if tls.resumed {
                " · session resumed"
            } else {
                " · full handshake"
            });
            lines.push(kv("tls", said));
            if let Some(cert) = &tls.cert {
                lines.push(kv(
                    "certificate",
                    format!(
                        "{} by {} · {} ({} → {})",
                        cert.subject,
                        cert.issuer,
                        cert.expiry(sample.wall),
                        crate::logs::day_and_time(cert.not_before),
                        crate::logs::day_and_time(cert.not_after)
                    ),
                ));
                if !cert.names.is_empty() {
                    lines.push(kv("names", truncate(&cert.names.join(", "), inner - 14)));
                }
            }
        }
        for (i, hop) in sample.hops.iter().enumerate() {
            lines.push(kv(
                if i == 0 { "redirects" } else { "" },
                format!(
                    "{} → {} in {}",
                    hop.status,
                    truncate(&hop.url, inner - 30),
                    format::latency(hop.took)
                ),
            ));
        }
        if !sample.headers.is_empty() {
            lines.push(Line::raw(""));
            lines.push(Line::from(label(
                sample.version.clone().unwrap_or_else(|| "response".into()),
            )));
            for (name, value) in &sample.headers {
                lines.push(Line::from(vec![
                    Span::styled(format!("{name}: "), Style::new().fg(ACCENT)),
                    Span::raw(truncate(value, inner.saturating_sub(name.len() + 2))),
                ]));
            }
        }
        if !sample.body.is_empty() {
            lines.push(Line::raw(""));
            let text = String::from_utf8_lossy(&sample.body)
                .chars()
                .map(|c| if c.is_control() && c != '\n' { ' ' } else { c })
                .collect::<String>();
            for raw in text.lines().take(12) {
                for piece in wrap(raw, inner, 2) {
                    lines.push(Line::from(Span::styled(piece, Style::new().fg(LABEL))));
                }
            }
            if sample.bytes as usize > sample.body.len() {
                lines.push(Line::from(label(format!(
                    "… {} of {}",
                    format::bytes(sample.body.len() as f64),
                    format::bytes(sample.bytes as f64)
                ))));
            }
        }
        let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(2));
        let [popup] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(popup);
        f.render_widget(Clear, popup);
        f.render_widget(
            Paragraph::new(lines).block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(ACCENT))
                    .title(Span::styled(
                        format!(" ping {} ", sample.seq),
                        Style::new().fg(ACCENT).bold(),
                    )),
            ),
            popup,
        );
    }
}

impl Sample {
    fn target_kind_is_cmd(&self, state: &State) -> bool {
        matches!(state.targets[self.target].target.kind, Kind::Cmd(_))
    }
}

fn median_of(h: &crate::metrics::Histogram) -> Option<Duration> {
    (h.count() > 0).then(|| Duration::from_micros(h.percentile(50.0)))
}

/// A stacked bar of the phases, with each one's share and time under it
fn phase_bar(title: &str, phases: &Phases, total: Duration, width: usize) -> Vec<Line<'static>> {
    let known: Vec<(usize, Duration)> = phases
        .each()
        .iter()
        .enumerate()
        .filter_map(|(i, (_, d))| d.map(|d| (i, d)))
        .collect();
    let sum: Duration = known.iter().map(|(_, d)| *d).sum();
    let sum = if sum.is_zero() { total } else { sum };
    let mut bar: Vec<Span<'static>> = vec![label(format!("{title:<9}"))];
    let mut legend: Vec<Span<'static>> = vec![Span::raw(" ".repeat(9))];
    let mut used = 0usize;
    for (n, (i, took)) in known.iter().enumerate() {
        let share = took.as_secs_f64() / sum.as_secs_f64().max(1e-9);
        let mut cells = (share * width as f64).round() as usize;
        if n == known.len() - 1 {
            cells = width.saturating_sub(used);
        }
        cells = cells.max(1);
        used += cells;
        bar.push(Span::styled(
            "█".repeat(cells),
            Style::new().fg(PHASE_COLORS[*i]),
        ));
        let text = format::latency_short(took.as_micros() as u64);
        let text = if cells > text.len() {
            format!("{text:<cells$}")
        } else {
            " ".repeat(cells)
        };
        legend.push(Span::styled(text, Style::new().fg(PHASE_COLORS[*i])));
    }
    bar.push(label(format!(" {}", format::latency(sum))));
    vec![Line::from(bar), Line::from(legend)]
}

/// What is known of a target's far end
fn about_target(t: &TargetStats, state: &State) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut parts: Vec<String> = Vec::new();
    if let Some(remote) = t.last_remote {
        parts.push(match t.last_local {
            Some(local) => format!("{local} → {remote}"),
            None => format!("→ {remote}"),
        });
    }
    if let Some(version) = &t.last_version {
        parts.push(version.clone());
    }
    if let Some(tls) = &t.last_tls {
        let mut said = tls.version.clone();
        if !tls.cipher.is_empty() {
            said.push_str(&format!(" {}", tls.cipher));
        }
        if let Some(alpn) = &tls.alpn {
            said.push_str(&format!(" · {alpn}"));
        }
        parts.push(said);
    }
    if !parts.is_empty() {
        lines.push(Line::from(vec![
            label("where    "),
            Span::raw(parts.join(" · ")),
        ]));
    }
    if let Some(cert) = t.last_tls.as_ref().and_then(|t| t.cert.as_ref()) {
        let now = crate::logs::wall();
        let color = match cert.days_left(now) {
            d if d < 0 => BAD,
            d if d < 14 => WARN,
            _ => Color::Reset,
        };
        lines.push(Line::from(vec![
            label("cert     "),
            Span::raw(format!("{} by {} · ", cert.subject, cert.issuer)),
            value(cert.expiry(now), color),
        ]));
    }
    let codes: Vec<String> = t
        .statuses
        .iter()
        .map(|(code, n)| match t.target.kind {
            Kind::Cmd(_) => format!("exit {code} ×{n}"),
            _ => format!("{code} ×{n}"),
        })
        .collect();
    let mut answers = Vec::new();
    if !codes.is_empty() {
        answers.push(codes.join(" · "));
    }
    for (cause, n) in &t.causes {
        answers.push(format!("{cause} ×{n}"));
    }
    if !answers.is_empty() {
        let whole = t.whole();
        lines.push(Line::from(vec![
            label("answers  "),
            Span::raw(format!(
                "{} sent · {} answered · {} loss · ",
                whole.sent,
                whole.answered,
                percent(whole.loss())
            )),
            Span::raw(truncate(&answers.join(" · "), 120)),
        ]));
    }
    if t.violations > 0 {
        let worst: Vec<String> = t
            .worst
            .iter()
            .map(|(k, d)| format!("{k} {}", format::latency(*d)))
            .collect();
        lines.push(Line::from(vec![
            label("slo      "),
            value(
                format!("broken {} times · worst {}", t.violations, worst.join(", ")),
                WARN,
            ),
        ]));
    }
    let _ = state;
    let _ = glyph;
    lines
}

/// A line that isn't picked, a shade quieter
fn dim(color: Color) -> Color {
    match color {
        Color::Rgb(r, g, b) => Color::Rgb(r / 2 + 40, g / 2 + 40, b / 2 + 40),
        Color::Indexed(81) => Color::Indexed(67),
        Color::White => Color::Gray,
        _ => Color::DarkGray.max_bright(color),
    }
}

trait MaxBright {
    fn max_bright(self, color: Color) -> Color;
}

impl MaxBright for Color {
    /// A named colour's dimmer form, where the terminal has one
    fn max_bright(self, color: Color) -> Color {
        match color {
            Color::LightRed => Color::Red,
            Color::LightGreen => Color::Green,
            Color::LightYellow => Color::Yellow,
            Color::LightBlue => Color::Blue,
            Color::LightMagenta => Color::Magenta,
            Color::LightCyan => Color::Cyan,
            other => {
                let _ = self;
                other
            }
        }
    }
}

/// Braille bit for the dot at (column, row) within a cell
fn braille_bit(dx: usize, dy: usize) -> u32 {
    match (dx, dy) {
        (0, 3) => 0x40,
        (1, 3) => 0x80,
        (0, dy) => 1 << dy,
        (_, dy) => 1 << (dy + 3),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ping::{self, Settings, Slo, Target};
    use ratatui::backend::TestBackend;

    fn settings() -> Settings {
        Settings {
            every: Duration::from_secs(1),
            timeout: Duration::from_secs(5),
            family: None,
            bind: None,
            insecure: false,
            keep_alive: false,
            follow_redirects: true,
            refused_is_pong: true,
            slo: Slo::parse("total=50").unwrap(),
            keep_body: 0,
            proxy: None,
            user_agent: "pepe/test".into(),
            headers: Default::default(),
            method: reqwest::Method::GET,
            body: Default::default(),
            count: None,
            duration: None,
            compression: false,
        }
    }

    /// Two targets pinged for a minute, one of which loses a few and is
    /// slow once
    fn screen() -> PingScreen {
        let targets = vec![
            Target {
                name: "api".into(),
                shown: "https://api.test/".into(),
                kind: Kind::Http {
                    url: reqwest::Url::parse("https://api.test/").unwrap(),
                    connect_to: None,
                },
            },
            Target {
                name: "cdn".into(),
                shown: "https://cdn.test/".into(),
                kind: Kind::Http {
                    url: reqwest::Url::parse("https://cdn.test/").unwrap(),
                    connect_to: None,
                },
            },
        ];
        let cfg = settings();
        let shared = ping::shared(targets, cfg.clone());
        {
            let mut state = shared.lock();
            for seq in 1..=60u64 {
                for target in 0..2 {
                    let mut s = ping::Sample::new(target, seq, Duration::from_secs(seq));
                    s.wall = 1_791_460_536 + seq as i64;
                    let us = 20_000 + (seq * 700) % 9_000 + target as u64 * 5_000;
                    s.total = Duration::from_micros(us);
                    s.answered = true;
                    s.status = Some(200);
                    s.phases = Phases {
                        dns: Some(Duration::from_micros(1_500)),
                        connect: Some(Duration::from_micros(8_000)),
                        tls: Some(Duration::from_micros(us / 2)),
                        ttfb: Some(Duration::from_micros(4_000)),
                        download: Some(Duration::from_micros(500)),
                    };
                    s.bytes = 1_234;
                    s.remote = Some("93.184.216.34:443".parse().unwrap());
                    s.local = Some("10.0.0.2:50000".parse().unwrap());
                    s.version = Some("HTTP/1.1".into());
                    s.headers = vec![("content-type".into(), "text/plain".into())];
                    if target == 1 && seq % 20 == 0 {
                        s.answered = false;
                        s.status = None;
                        s.timeout = true;
                        s.error = Some("no answer in 5s".into());
                        s.total = Duration::from_secs(5);
                    }
                    if target == 1 && seq == 30 {
                        s.total = Duration::from_millis(80);
                        s.violations = cfg.slo_check_for_test(&s);
                    }
                    state.targets[target].sent += 1;
                    state.targets[target].in_flight += 1;
                    state.targets[target].record(s, None);
                }
            }
        }
        PingScreen::new(
            shared,
            Look {
                window: 60.0,
                ymin: None,
                ymax: None,
                simple: false,
                colors: Vec::new(),
                bell: false,
            },
        )
    }

    impl Settings {
        fn slo_check_for_test(&self, s: &ping::Sample) -> Vec<ping::Violation> {
            let mut out = Vec::new();
            if let Some(limit) = self.slo.total {
                if s.total > limit {
                    out.push(ping::Violation {
                        key: "total",
                        limit,
                        actual: s.total,
                    });
                }
            }
            out
        }
    }

    fn draw(screen: &mut PingScreen, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| screen.render(f)).unwrap();
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

    fn press(screen: &mut PingScreen, code: KeyCode) -> bool {
        screen.key(KeyEvent::from(code))
    }

    #[test]
    fn the_graph_has_both_targets_and_the_table_their_numbers() {
        let mut screen = screen();
        let shown = draw(&mut screen, 120, 30);
        assert!(
            shown.contains("pepe ping · https://api.test/, https://cdn.test/"),
            "{shown}"
        );
        assert!(
            shown.contains("1 Graph") && shown.contains("2 Phases") && shown.contains("3 Pings")
        );
        assert!(
            shown.contains("■ api") && shown.contains("■ cdn"),
            "{shown}"
        );
        // cdn lost 3 of 60: 5% loss, three timeouts
        assert!(
            shown
                .lines()
                .any(|l| l.contains("cdn") && l.contains("5.0%") && l.contains("3")),
            "{shown}"
        );
        // Braille somewhere in the plot, and the failure marks on top
        assert!(
            shown
                .chars()
                .any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)),
            "{shown}"
        );
        assert!(shown.contains('✖'), "{shown}");
        assert!(shown.contains("-1m") && shown.contains("now"), "{shown}");
        // Dots instead, zoomed in, on a log scale
        press(&mut screen, KeyCode::Char('s'));
        press(&mut screen, KeyCode::Char('+'));
        press(&mut screen, KeyCode::Char('l'));
        let shown = draw(&mut screen, 120, 30);
        assert!(
            shown.contains('•') && shown.contains("-30s") && shown.contains("log"),
            "{shown}"
        );
        assert!(!shown
            .chars()
            .any(|c| ('\u{2800}'..='\u{28ff}').contains(&c)));
        press(&mut screen, KeyCode::Char('w'));
        let shown = draw(&mut screen, 120, 30);
        assert!(shown.contains("whole run"), "{shown}");
        press(&mut screen, KeyCode::Char('f'));
        assert!(!draw(&mut screen, 120, 30).contains('✖'));
    }

    #[test]
    fn phases_and_pings_have_their_views() {
        let mut screen = screen();
        press(&mut screen, KeyCode::Char('2'));
        press(&mut screen, KeyCode::Down);
        let shown = draw(&mut screen, 120, 34);
        assert!(
            shown.contains("cdn") && shown.contains("DNS") && shown.contains("first byte"),
            "{shown}"
        );
        assert!(
            shown.contains("last") && shown.contains("median"),
            "{shown}"
        );
        assert!(
            shown.contains("namelookup") && shown.contains("starttransfer"),
            "{shown}"
        );
        assert!(
            shown.contains("10.0.0.2:50000 → 93.184.216.34:443"),
            "{shown}"
        );
        assert!(shown.contains("broken 1 times"), "{shown}");
        press(&mut screen, KeyCode::Char('3'));
        let shown = draw(&mut screen, 120, 34);
        assert!(
            shown.contains("time") && shown.contains("status") && shown.contains("ttfb"),
            "{shown}"
        );
        assert!(
            shown.contains("timeout") && shown.contains("no answer in 5s"),
            "{shown}"
        );
        press(&mut screen, KeyCode::Char('x'));
        let shown = draw(&mut screen, 120, 34);
        assert!(shown.contains("slo total"), "{shown}");
        let rows = shown
            .lines()
            .filter(|l| l.contains("timeout") || l.contains("slo total"))
            .count();
        assert_eq!(rows, 4, "{shown}");
        press(&mut screen, KeyCode::Enter);
        let shown = draw(&mut screen, 120, 34);
        assert!(
            shown.contains("ping 60") || shown.contains("ping 30"),
            "{shown}"
        );
        assert!(
            shown.contains("outcome") && shown.contains("content-type"),
            "{shown}"
        );
        assert!(!press(&mut screen, KeyCode::Esc));
        assert!(!screen.inspecting);
    }

    #[test]
    fn small_screens_say_so_and_q_quits() {
        let mut screen = screen();
        assert!(draw(&mut screen, 40, 10).contains("pepe ping needs"));
        assert!(!press(&mut screen, KeyCode::Char('?')));
        assert!(draw(&mut screen, 120, 30).contains("close this help"));
        assert!(!press(&mut screen, KeyCode::Esc));
        assert!(press(&mut screen, KeyCode::Char('q')));
        assert_eq!(parse_color("#ff8a3d"), Some(Color::Rgb(0xff, 0x8a, 0x3d)));
        assert_eq!(parse_color("light-red"), Some(Color::LightRed));
        assert_eq!(parse_color("plaid"), None);
    }
}
