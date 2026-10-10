//! The ramp screen: one view, no tabs. Each step is a row as it finishes,
//! with everything measured about the selected one beside it, the run
//! second by second and the curves under them, and the conclusion last.

use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Padding, Paragraph},
    Frame, Terminal,
};
use tokio::time::MissedTickBehavior;

use super::kit::{about_line, caption, chips_fit, help, marker, panel, FAINT, SELECTED};
use super::view::{label, status_color, truncate, value, ACCENT, BAD, GOOD, LABEL, RULE, WARN};
use super::{format, mascot, theme, Outcome};
use crate::insights::Level;
use crate::load::LoadHandle;
use crate::ramp::{self, End, Ramp, RampPlan, Second, Step, Tick};
use crate::Cli;

/// How often results are taken off the channel and the step clock checked
const PUMP: Duration = Duration::from_millis(50);
/// Redraw every this many pumps while the ramp runs
const PUMPS_PER_FRAME: u64 = 2;
/// The step's details sit beside the table from this width
const BESIDE: u16 = 130;
/// Every other step of the timeline, to tell them apart
const SECOND_SHADE: Color = Color::Indexed(67);
const BLOCKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];

/// What a key asks for
#[derive(Debug, PartialEq)]
enum Effect {
    None,
    Leave(Outcome),
    /// The ramp moved: change level, or stop sending
    Ramp(Tick),
    Paused(bool),
}

pub struct RampScreen {
    cli: Cli,
    ramp: Ramp,
    /// The step whose details show; none follows the step in progress
    selected: Option<usize>,
    /// The keys overlay is open
    show_help: bool,
}

fn level_color(level: Level) -> Color {
    match level {
        Level::Healthy => GOOD,
        Level::Degraded => WARN,
        Level::Failing => BAD,
    }
}

/// The colour a step is drawn in: by what its note says of it
fn step_color(step: &Step) -> Color {
    step.note
        .as_ref()
        .map_or(GOOD, |(level, _)| level_color(*level))
}

fn errors_color(percent: f64) -> Color {
    match percent {
        e if e >= 5.0 => BAD,
        e if e >= 1.0 => WARN,
        _ => Color::Reset,
    }
}

/// "+12%", or "×3.4" once it's more than double
fn change(was: f64, now: f64) -> String {
    if was <= 0.0 || now <= 0.0 {
        return "n/a".into();
    }
    let ratio = now / was;
    if ratio >= 2.0 {
        format!("×{ratio:.1}")
    } else {
        format!("{:+.0}%", (ratio - 1.0) * 100.0)
    }
}

impl RampScreen {
    pub fn new(cli: Cli, plan: RampPlan) -> Self {
        RampScreen {
            cli,
            ramp: Ramp::new(plan, Instant::now()),
            selected: None,
            show_help: false,
        }
    }

    /// The ramp as text, once at least one step has been measured
    /// Everything the ramp measured, for `--fail-if`
    pub fn metrics(&self) -> &crate::metrics::Metrics {
        &self.ramp.total
    }

    pub fn report(&self) -> Option<String> {
        (!self.ramp.steps.is_empty()).then(|| ramp::report(&self.cli, &self.ramp))
    }

    /// The row the details are of: the selected one, else the step in
    /// progress, else the last measured
    fn cursor(&self) -> usize {
        let follow = if self.ramp.end.is_some() {
            self.ramp.steps.len().saturating_sub(1)
        } else {
            self.ramp.steps.len()
        };
        self.selected
            .unwrap_or(follow)
            .min(self.ramp.plan.levels.len() - 1)
    }

    fn key(&mut self, key: KeyEvent, now: Instant) -> Effect {
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Effect::Leave(Outcome::Quit);
        }
        let running = self.ramp.end.is_none();
        let last = self.ramp.plan.levels.len() - 1;
        if self.show_help && matches!(key.code, KeyCode::Char('?') | KeyCode::Esc | KeyCode::F(1)) {
            self.show_help = false;
            return Effect::None;
        }
        match key.code {
            KeyCode::Char('?') | KeyCode::F(1) => {
                self.show_help = true;
                Effect::None
            }
            KeyCode::Char('q') | KeyCode::Char('Q') => Effect::Leave(Outcome::Quit),
            KeyCode::Char('r') | KeyCode::Char('R') => Effect::Leave(Outcome::Restart),
            KeyCode::Char('e') | KeyCode::Char('E') => Effect::Leave(Outcome::Edit),
            KeyCode::Char(' ') if running => {
                let paused = !self.ramp.is_paused();
                self.ramp.set_paused(paused, now);
                Effect::Paused(paused)
            }
            KeyCode::Char('n') | KeyCode::Char('N') if running => Effect::Ramp(self.ramp.skip(now)),
            KeyCode::Char('s') | KeyCode::Char('S') if running => Effect::Ramp(self.ramp.stop(now)),
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = Some(self.cursor().saturating_sub(1));
                Effect::None
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.selected = Some((self.cursor() + 1).min(last));
                Effect::None
            }
            KeyCode::Home => {
                self.selected = Some(0);
                Effect::None
            }
            // Back to following the step in progress
            KeyCode::Esc | KeyCode::End => {
                self.selected = None;
                Effect::None
            }
            _ => Effect::None,
        }
    }

    /// Carry out what the ramp decided
    fn apply(&self, tick: Tick, load: &LoadHandle) {
        match tick {
            Tick::Hold => {}
            Tick::Level(level) => {
                load.set_concurrency(level as usize);
            }
            Tick::Done => load.stop(),
        }
    }

    /// Take in the results that arrived, and move to the next step when
    /// this one has been held long enough
    fn pump(&mut self, load: &mut LoadHandle) {
        let now = Instant::now();
        while let Ok(stat) = load.try_recv() {
            self.ramp.record(&stat, now);
        }
        let tick = self.ramp.tick(now);
        self.apply(tick, load);
    }

    pub async fn run(
        &mut self,
        load: &mut LoadHandle,
    ) -> Result<Outcome, Box<dyn std::error::Error>> {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;
        let mut events = EventStream::new();
        let mut pump = tokio::time::interval(PUMP);
        pump.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        let mut pumps = 0u64;
        let mut dirty = true;
        loop {
            if dirty {
                let now = Instant::now();
                terminal.draw(|f| theme::draw(f, |f| self.render(f, now)))?;
                dirty = false;
            }
            tokio::select! {
                _ = &mut ctrl_c => return Ok(Outcome::Quit),
                _ = pump.tick(), if self.ramp.end.is_none() => {
                    self.pump(load);
                    pumps += 1;
                    dirty |= pumps % PUMPS_PER_FRAME == 0 || self.ramp.end.is_some();
                }
                event = events.next() => match event {
                    Some(Ok(Event::Key(key))) if key.kind == KeyEventKind::Press => {
                        match self.key(key, Instant::now()) {
                            Effect::Leave(outcome) => return Ok(outcome),
                            Effect::Ramp(tick) => self.apply(tick, load),
                            Effect::Paused(paused) => load.set_paused(paused),
                            Effect::None => {}
                        }
                        dirty = true;
                    }
                    Some(Ok(_)) => dirty = true,
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(Outcome::Quit),
                },
            }
        }
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    fn mood(&self, current: Option<&Step>) -> (mascot::Mood, &'static str) {
        let steps = &self.ramp.steps;
        match &self.ramp.end {
            Some(End::Interrupted) => (mascot::Mood::Dizzy, "stopped early"),
            Some(End::Stopped(_)) => (mascot::Mood::Worried, "found the limit"),
            Some(End::Completed) => {
                let failing = steps
                    .iter()
                    .any(|s| matches!(s.note, Some((Level::Failing, _))));
                if failing {
                    (mascot::Mood::Worried, "found the limit")
                } else if steps.iter().any(|s| s.saturated) {
                    (mascot::Mood::Proud, "found the plateau")
                } else {
                    (mascot::Mood::Proud, "made it to the top")
                }
            }
            None if self.ramp.is_paused() => (mascot::Mood::Sleeping, "paused"),
            None => {
                let errors = current.map_or(0.0, |s| s.metrics.error_rate());
                let strained = steps.last().is_some_and(|s| s.note.is_some());
                if errors >= 5.0 {
                    (mascot::Mood::OnFire, "too hot!")
                } else if errors >= 1.0 || strained {
                    (mascot::Mood::Sweating, "getting heavy")
                } else {
                    (mascot::Mood::Happy, "climbing")
                }
            }
        }
    }

    /// Every step that has numbers: the measured ones, then the one in
    /// progress
    fn measured<'a>(&'a self, current: Option<&'a Step>) -> Vec<&'a Step> {
        self.ramp.steps.iter().chain(current).collect()
    }

    fn render(&self, f: &mut Frame, now: Instant) {
        let area = f.area();
        let current = self.ramp.current(now);
        let ended = self.ramp.end.is_some();
        let found = ramp::findings(&self.ramp.steps, self.ramp.completed());
        let tall = area.height >= 36 && area.width >= 110;
        let cards = area.height >= 22 && area.width >= 70;
        let beside = area.width >= BESIDE;

        let command = if ended {
            ramp::sustained(&self.cli, &found)
        } else {
            None
        };
        let result = (found.list.len().max(1) + if command.is_some() { 2 } else { 0 }) as u16 + 2;
        let result = result.min(area.height / 3).max(3);
        let header_height = if tall { mascot::HEIGHT + 1 } else { 3 };
        let numbers_height = if cards && !tall { 3 } else { 0 };

        // The table and the details get what they need; the timeline and
        // the curves are added while there's height for them, and what's
        // left over makes those taller rather than the table emptier
        let rest = area
            .height
            .saturating_sub(header_height + numbers_height + result + 1);
        let rows = self.ramp.plan.levels.len() as u16 + 3;
        let need = if beside { rows.max(19) } else { rows }.max(6);
        let mut spare = rest.saturating_sub(need);
        let mut timeline = if spare >= 6 && area.width >= 60 { 6 } else { 0 };
        spare -= timeline;
        let curves = if spare >= 8 && area.width >= 70 {
            spare.min(13)
        } else {
            0
        };
        spare -= curves;
        if timeline > 0 {
            timeline += spare.min(3);
        }

        let [header, numbers, middle, timeline_area, curves_area, result_area, footer] =
            Layout::vertical([
                Constraint::Length(header_height),
                Constraint::Length(numbers_height),
                Constraint::Min(4),
                Constraint::Length(timeline),
                Constraint::Length(curves),
                Constraint::Length(result),
                Constraint::Length(1),
            ])
            .areas(area);

        if tall {
            let [pet, _, main] = Layout::horizontal([
                Constraint::Length(mascot::WIDTH),
                Constraint::Length(2),
                Constraint::Min(0),
            ])
            .areas(header);
            let (mood, says) = self.mood(current.as_ref());
            let mut lines = mascot::lines(mood, 0);
            lines.push(Line::styled(says, Style::new().fg(ACCENT).italic()));
            f.render_widget(Paragraph::new(lines), pet);
            mascot::keep(pet);
            let [title, _, numbers] = Layout::vertical([
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Length(3),
            ])
            .areas(main);
            self.render_title(f, title, now);
            self.render_numbers(f, numbers, current.as_ref(), &found);
        } else {
            self.render_title(f, header, now);
            if cards {
                self.render_numbers(f, numbers, current.as_ref(), &found);
            }
        }

        if beside {
            let [table, detail] =
                Layout::horizontal([Constraint::Percentage(58), Constraint::Min(0)]).areas(middle);
            self.render_steps(f, table, current.as_ref(), true);
            self.render_detail(f, detail, current.as_ref(), &found);
        } else {
            self.render_steps(f, middle, current.as_ref(), self.selected.is_some());
        }
        // No room beside the table: a picked step's details take the place
        // of the charts until esc lets go of it
        let under = Rect {
            height: timeline + curves,
            ..timeline_area
        };
        if !beside && self.selected.is_some() && under.height >= 12 {
            self.render_detail(f, under, current.as_ref(), &found);
        } else {
            if timeline > 0 {
                self.render_timeline(f, timeline_area);
            }
            if curves > 0 {
                self.render_curves(f, curves_area, current.as_ref());
            }
        }
        self.render_result(f, result_area, &found, command);

        let mut keys: Vec<(&str, &str)> = Vec::new();
        if !ended {
            keys.extend([
                (
                    "space",
                    if self.ramp.is_paused() {
                        "resume"
                    } else {
                        "pause"
                    },
                ),
                ("n", "next step now"),
                ("s", "stop here"),
            ]);
        }
        keys.push(("↑↓", "step details"));
        if self.selected.is_some() {
            keys.push(("esc", if ended { "last step" } else { "follow" }));
        }
        keys.extend([
            ("r", if ended { "run again" } else { "restart" }),
            ("e", "edit"),
            ("?", "keys"),
            ("q", "quit"),
        ]);
        f.render_widget(Paragraph::new(chips_fit(&keys, footer.width)), footer);
        if self.show_help {
            help(
                f,
                area,
                &[
                    ("space", "pause or resume the ramp"),
                    ("n", "go to the next step now"),
                    ("s", "stop here and keep the results"),
                    ("↑ ↓ / j k", "pick a step to see its details"),
                    ("home", "the first step"),
                    ("esc / end", "follow the step in progress again"),
                    ("r", "restart (once ended: run again)"),
                    ("e", "edit the settings, then run again"),
                    ("?", "close this help"),
                    ("q", "quit"),
                ],
                &[
                    "Each step holds one concurrency for a while. A step".into(),
                    "holds when throughput keeps following the load and".into(),
                    "latency and errors stay in bounds.".into(),
                    String::new(),
                    about_line(),
                ],
            );
        }
    }

    /// What's being ramped, how far along it is, and what it climbs
    fn render_title(&self, f: &mut Frame, area: Rect, now: Instant) {
        let ramp = &self.ramp;
        let plan = &ramp.plan;
        let steps = plan.levels.len();
        let [brand, progress, about] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(area);

        let state = match &ramp.end {
            Some(End::Completed) => value("✔ done ", GOOD),
            Some(End::Stopped(condition)) => value(format!("■ stopped: {condition} "), BAD),
            Some(End::Interrupted) => value("■ stopped by hand ", WARN),
            None if ramp.is_paused() => value("⏸ paused ", WARN),
            None => Span::raw(""),
        };
        let place = if ramp.end.is_some() {
            format!(
                "{} of {steps} steps · {} ",
                ramp.steps.len(),
                format::clock(ramp.elapsed(now))
            )
        } else {
            format!(
                "step {} of {steps} · {} of {} ",
                ramp.steps.len() + 1,
                format::clock(ramp.elapsed(now)),
                format::clock(plan.total())
            )
        };
        let right = Line::from(vec![state, label(place)]);
        let room = (brand.width as usize).saturating_sub(right.width() + 16);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" pepe ", Style::new().bg(ACCENT).fg(Color::Black).bold()),
                Span::styled(" ramp ", Style::new().bg(SELECTED).fg(Color::White)),
                Span::raw("  "),
                Span::styled(
                    truncate(&format!("{} {}", self.cli.method, self.cli.url), room),
                    Style::new().bold(),
                ),
            ])),
            brand,
        );
        f.render_widget(Paragraph::new(right).alignment(Alignment::Right), brand);

        // Finished steps, and how far into this one
        let done = match &ramp.end {
            Some(End::Completed) => 1.0,
            Some(_) => ramp.steps.len() as f64 / steps as f64,
            None => {
                let within = (ramp.held(now).as_secs_f64() / plan.every.as_secs_f64()).min(1.0);
                (ramp.steps.len() as f64 + within) / steps as f64
            }
        };
        let width = progress.width as usize;
        let filled = ((done * width as f64).round() as usize).min(width);
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("━".repeat(filled), Style::new().fg(ACCENT)),
                Span::styled("─".repeat(width - filled), Style::new().fg(RULE)),
            ])),
            progress,
        );

        let mut spans = vec![label(format!(
            "{} → {} concurrent in {} steps, {} each",
            plan.levels[0],
            plan.levels[steps - 1],
            steps,
            format::span(plan.every)
        ))];
        if plan.until.is_empty() {
            spans.push(label(" · no stop condition"));
        } else {
            let conditions: Vec<&str> = plan.until.iter().map(|c| c.text.as_str()).collect();
            spans.push(label(" · stops when "));
            spans.push(Span::raw(conditions.join(" or ")));
        }
        f.render_widget(Paragraph::new(Line::from(spans)), about);
    }

    /// The step in progress as numbers; after the end, the ramp summed up
    fn render_numbers(
        &self,
        f: &mut Frame,
        area: Rect,
        current: Option<&Step>,
        found: &ramp::Findings,
    ) {
        let steps = &self.ramp.steps;
        let bold = |text: String| Span::styled(text, Style::new().bold());
        let faint = |text: String| Span::styled(text, Style::new().fg(FAINT));
        let mut numbers: Vec<(String, Vec<Span>)> = Vec::new();
        if let Some(step) = current {
            let before = steps.last();
            let delta = |was: Option<f64>, now: f64, more_is_good: bool| -> Span<'static> {
                match was.filter(|was| *was > 0.0 && now > 0.0) {
                    Some(was) => {
                        let worse = (now >= was) != more_is_good && (now / was - 1.0).abs() >= 0.1;
                        Span::styled(
                            format!("  {}", change(was, now)),
                            Style::new().fg(if worse { WARN } else { FAINT }),
                        )
                    }
                    None => Span::raw(""),
                }
            };
            let errors = step.metrics.error_rate();
            numbers.push(("concurrency".into(), vec![bold(step.level.to_string())]));
            numbers.push((
                "ok req/s".into(),
                vec![
                    bold(format::compact(step.rps())),
                    delta(before.map(Step::rps), step.rps(), true),
                ],
            ));
            numbers.push((
                "p50".into(),
                vec![bold(format::latency(step.metrics.percentile(50.0)))],
            ));
            numbers.push((
                "p99".into(),
                vec![
                    bold(format::latency(step.p99())),
                    delta(
                        before.map(|b| b.p99().as_micros() as f64),
                        step.p99().as_micros() as f64,
                        false,
                    ),
                ],
            ));
            numbers.push((
                "errors".into(),
                vec![value(format!("{errors:.1}%"), errors_color(errors))],
            ));
            numbers.push((
                "requests".into(),
                vec![
                    bold(format::count(step.metrics.total)),
                    faint(format!(
                        "  of {} in all",
                        format::compact(self.ramp.total.total as f64)
                    )),
                ],
            ));
        } else {
            let Some(top) = steps.last() else { return };
            let held = found
                .holds
                .and_then(|level| steps.iter().find(|s| s.level == level));
            let peak = steps
                .iter()
                .max_by(|a, b| a.rps().total_cmp(&b.rps()))
                .unwrap_or(top);
            let total = &self.ramp.total;
            numbers.push((
                "holds at".into(),
                vec![match held {
                    Some(step) => value(step.level.to_string(), GOOD),
                    None => value("nothing", BAD),
                }],
            ));
            numbers.push((
                "peak ok req/s".into(),
                vec![
                    bold(format::compact(peak.rps())),
                    faint(format!("  at {}", peak.level)),
                ],
            ));
            let shown = held.unwrap_or(&steps[0]);
            numbers.push((
                format!("p99 at {}", shown.level),
                vec![bold(format::latency(shown.p99()))],
            ));
            numbers.push((
                format!("p99 at {}", top.level),
                vec![
                    bold(format::latency(top.p99())),
                    Span::styled(
                        format!(
                            "  {}",
                            change(shown.p99().as_micros() as f64, top.p99().as_micros() as f64)
                        ),
                        Style::new().fg(if top.p99() >= shown.p99() * 2 {
                            WARN
                        } else {
                            FAINT
                        }),
                    ),
                ],
            ));
            numbers.push((
                "failed".into(),
                vec![
                    value(
                        format!("{:.1}%", total.error_rate()),
                        errors_color(total.error_rate()),
                    ),
                    faint(format!(
                        "  {} requests",
                        format::count(total.total - total.success)
                    )),
                ],
            ));
            numbers.push((
                "requests".into(),
                vec![
                    bold(format::count(total.total)),
                    faint(format!(
                        "  in {}",
                        format::span(steps.iter().map(|s| s.held).sum())
                    )),
                ],
            ));
        }
        // As many as fit without squeezing them
        let count = numbers.len().min((area.width as usize / 22).max(3));
        let areas = Layout::horizontal(vec![Constraint::Ratio(1, count as u32); count])
            .spacing(1)
            .split(area);
        for ((title, spans), area) in numbers.into_iter().zip(areas.iter()) {
            let block = panel(caption(&title, false), None, RULE).padding(Padding::horizontal(1));
            f.render_widget(Paragraph::new(Line::from(spans)).block(block), *area);
        }
    }

    /// Every step as a row: measured, in progress, or still to come
    fn render_steps(&self, f: &mut Frame, area: Rect, current: Option<&Step>, selectable: bool) {
        let ramp = &self.ramp;
        let levels = &ramp.plan.levels;
        let right = Line::from(label(format!(
            " {} of {} measured ",
            ramp.steps.len(),
            levels.len()
        )));
        let block = panel(caption("steps", false), Some(right), RULE);
        let inner = block.inner(area);
        f.render_widget(block, area);
        let [titles, body] =
            Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(inner);
        let width = inner.width as usize;
        // With room: how many requests, p90 and the slowest too, and p99
        // drawn beside throughput, to see one flatten while the other climbs
        let full = width >= 112;
        let bars = if full { 12 } else { 10 };

        let faint = Style::new().fg(FAINT);
        let heading = if full {
            format!(
                "   {:>5} {:>8} {:>7} {:<bars$} {:>8} {:>8} {:>8} {:<bars$} {:>8} {:>7}",
                "CONC", "REQUESTS", "OK/S", "", "P50", "P90", "P99", "", "MAX", "ERRORS"
            )
        } else {
            format!(
                "   {:>5} {:>7} {:<bars$} {:>8} {:>8} {:>7}",
                "CONC", "OK/S", "", "P50", "P99", "ERRORS"
            )
        };
        f.render_widget(Paragraph::new(Span::styled(heading, faint)), titles);

        let measured = self.measured(current);
        let peak = measured.iter().map(|s| s.rps()).fold(0.0, f64::max);
        let slowest = measured
            .iter()
            .map(|s| s.p99().as_micros() as f64)
            .fold(0.0, f64::max);
        // A thin bar, so rows stay rows instead of merging into a slab
        let bar = |share: f64, color: Color| -> Span<'static> {
            let cells = if share > 0.0 {
                ((share * bars as f64).round() as usize).clamp(1, bars)
            } else {
                0
            };
            Span::styled(
                format!("{:<bars$}", "━".repeat(cells)),
                Style::new().fg(color),
            )
        };
        let share = |amount: f64, of: f64| if of > 0.0 { amount / of } else { 0.0 };
        let row = |step: &Step, mark: Span<'static>, color: Color, note: Vec<Span<'static>>| {
            let errors = step.metrics.error_rate();
            let mut spans = vec![
                mark,
                Span::styled(format!("{:>5} ", step.level), Style::new().bold()),
            ];
            if full {
                spans.push(label(format!(
                    "{:>8} ",
                    format::compact(step.metrics.total as f64)
                )));
            }
            spans.push(Span::raw(format!("{:>7} ", format::compact(step.rps()))));
            spans.push(bar(share(step.rps(), peak), color));
            spans.push(Span::raw(format!(
                " {:>8} ",
                format::latency(step.metrics.percentile(50.0))
            )));
            if full {
                spans.push(Span::raw(format!(
                    "{:>8} ",
                    format::latency(step.metrics.percentile(90.0))
                )));
            }
            spans.push(Span::raw(format!("{:>8} ", format::latency(step.p99()))));
            if full {
                spans.push(bar(share(step.p99().as_micros() as f64, slowest), color));
                spans.push(label(format!(
                    " {:>8} ",
                    format::latency(step.metrics.max())
                )));
            }
            spans.push(Span::styled(
                format!("{errors:>6.1}%  "),
                Style::new().fg(errors_color(errors)),
            ));
            spans.extend(note);
            spans
        };

        let cursor = self.cursor();
        let mut lines: Vec<Line> = Vec::new();
        for (i, level) in levels.iter().enumerate() {
            let active = selectable && i == cursor;
            let mut spans = vec![marker(active, true)];
            if let Some(step) = ramp.steps.get(i) {
                let color = step_color(step);
                let (mark, note) = match &step.note {
                    Some((level, text)) => (
                        value(format!("{} ", level.symbol()), color),
                        vec![Span::styled(text.clone(), Style::new().fg(color))],
                    ),
                    None => (Span::styled("● ", Style::new().fg(GOOD)), Vec::new()),
                };
                spans.extend(row(step, mark, color, note));
            } else if let (Some(step), true) = (current, i == ramp.steps.len()) {
                let note = vec![label(format!(
                    "{} of {}",
                    format::span(Duration::from_secs(step.held.as_secs())),
                    format::span(ramp.plan.every)
                ))];
                spans.extend(row(step, value("▸ ", ACCENT), ACCENT, note));
            } else {
                spans.push(Span::styled(format!("○ {level:>5}"), faint));
            }
            let used: usize = spans.iter().map(Span::width).sum();
            let room = width.saturating_sub(used);
            spans.push(Span::raw(" ".repeat(room)));
            let line = Line::from(spans);
            lines.push(if active {
                line.style(Style::new().bg(SELECTED))
            } else {
                line
            });
        }
        // Keep the selected step in view
        let height = body.height as usize;
        let scroll = (cursor + 2).min(lines.len()).saturating_sub(height) as u16;
        f.render_widget(Paragraph::new(lines).scroll((scroll, 0)), body);
    }

    /// Everything measured about the selected step
    fn render_detail(
        &self,
        f: &mut Frame,
        area: Rect,
        current: Option<&Step>,
        found: &ramp::Findings,
    ) {
        let ramp = &self.ramp;
        let cursor = self.cursor();
        let level = ramp.plan.levels[cursor];
        let measured = self.measured(current);
        let step = measured.get(cursor).copied();
        let in_progress = current.is_some() && cursor == ramp.steps.len();

        let title = Line::from(vec![
            Span::styled(" STEP ", Style::new().fg(LABEL).bold()),
            Span::styled(
                format!("{} · {level} concurrent ", cursor + 1),
                Style::new().bold(),
            ),
        ]);
        let right = step.map(|step| {
            Line::from(label(format!(
                " {} {} ",
                if in_progress { "running for" } else { "held" },
                format::clock(step.held)
            )))
        });
        let block = panel(title, right, RULE).padding(Padding::horizontal(1));
        let inner = block.inner(area);
        f.render_widget(block, area);
        let Some(step) = step else {
            f.render_widget(
                Paragraph::new(Line::from(Span::styled(
                    "not measured yet",
                    Style::new().fg(FAINT).italic(),
                ))),
                inner,
            );
            return;
        };

        let width = inner.width as usize;
        let m = &step.metrics;
        let heading =
            |text: &str| Line::from(Span::styled(text.to_string(), Style::new().fg(FAINT)));
        let pair = |name: &str, shown: String| -> Vec<Span<'static>> {
            vec![
                label(format!("{name} ")),
                Span::styled(format!("{shown}   "), Style::new().bold()),
            ]
        };
        let mut lines: Vec<Line> = Vec::new();

        let failed = m.total - m.success;
        let mut requests = pair("requests", format::count(m.total));
        requests.extend(pair("ok", format::count(m.success)));
        requests.push(label("failed "));
        requests.push(value(
            format!("{} ({:.1}%)", format::count(failed), m.error_rate()),
            errors_color(m.error_rate()),
        ));
        lines.push(Line::from(requests));
        let mut throughput = pair("ok req/s", format::compact(step.rps()));
        throughput.extend(pair("all req/s", format::compact(m.rps(step.held))));
        throughput.extend(pair(
            "data",
            format!("{}/s", format::bytes(m.throughput(step.held))),
        ));
        lines.push(Line::from(throughput));

        lines.push(Line::raw(""));
        lines.push(heading("LATENCY"));
        let mut spread = pair("min", format::latency(m.min()));
        spread.extend(pair("mean", format::latency(m.mean())));
        spread.extend(pair("max", format::latency(m.max())));
        spread.extend(pair("±", format::latency(m.std_dev())));
        lines.push(Line::from(spread));
        let mut percentiles = Vec::new();
        for (name, q) in [("p50", 50.0), ("p75", 75.0), ("p90", 90.0), ("p95", 95.0)] {
            percentiles.extend(pair(name, format::latency(m.percentile(q))));
        }
        lines.push(Line::from(percentiles));
        let mut tail = Vec::new();
        for (name, q) in [("p99", 99.0), ("p99.9", 99.9)] {
            tail.extend(pair(name, format::latency(m.percentile(q))));
        }
        lines.push(Line::from(tail));
        // Where its requests fell, on the same axis for every step, so the
        // shape can be seen moving as the selection moves
        let low = measured
            .iter()
            .filter(|s| s.metrics.total > 0)
            .map(|s| s.metrics.min().as_micros() as f64)
            .fold(f64::MAX, f64::min)
            .max(1.0);
        let high = measured
            .iter()
            .map(|s| s.metrics.max().as_micros() as f64)
            .fold(1.0, f64::max);
        if high > low {
            let bins = width.clamp(8, 64);
            let mut counts = vec![0u64; bins];
            for (micros, count) in m.latency().buckets() {
                let at = ((micros.max(1) as f64 / low).ln() / (high / low).ln()).clamp(0.0, 1.0);
                counts[((at * (bins - 1) as f64).round() as usize).min(bins - 1)] += count;
            }
            let most = counts.iter().copied().max().unwrap_or(0).max(1) as f64;
            let shape: String = counts
                .iter()
                .map(|&count| match count {
                    0 => ' ',
                    // Square root, so the tail shows next to the bulk
                    n => BLOCKS[(((n as f64 / most).sqrt() * 8.0).ceil() as usize).clamp(1, 8) - 1],
                })
                .collect();
            lines.push(Line::from(Span::styled(
                shape,
                Style::new().fg(step_color(step)),
            )));
            let (left, right) = (
                format::latency_short(low as u64),
                format::latency_short(high as u64),
            );
            let gap = bins.saturating_sub(left.chars().count() + right.chars().count());
            lines.push(Line::from(Span::styled(
                format!("{left}{}{right}", " ".repeat(gap)),
                Style::new().fg(FAINT),
            )));
        }

        lines.push(Line::raw(""));
        lines.push(heading("RESPONSES"));
        let mut codes: Vec<(u16, u64)> = m.status_codes.iter().map(|(c, n)| (*c, *n)).collect();
        codes.sort_unstable();
        let mut responses = Vec::new();
        for (code, count) in codes {
            responses.push(value(code.to_string(), status_color(code)));
            responses.push(label(format!(" ×{}   ", format::count(count))));
        }
        for (name, count) in [("timed out", m.timeouts), ("no response", m.errors)] {
            if count > 0 {
                responses.push(value(name, BAD));
                responses.push(label(format!(" ×{}   ", format::count(count))));
            }
        }
        if responses.is_empty() {
            responses.push(label("none yet"));
        }
        lines.push(Line::from(responses));

        // Next to the step before it, and to the level that held
        let before = cursor.checked_sub(1).and_then(|i| measured.get(i)).copied();
        let held = found
            .holds
            .and_then(|level| ramp.steps.iter().find(|s| s.level == level))
            .filter(|held| held.level != step.level && Some(held.level) != before.map(|b| b.level));
        let compared: Vec<(&str, &Step)> = [
            before.map(|b| ("the step before", b)),
            held.map(|h| ("the level that held", h)),
        ]
        .into_iter()
        .flatten()
        .collect();
        if !compared.is_empty() {
            lines.push(Line::raw(""));
            lines.push(heading("COMPARED WITH"));
            for (name, other) in compared {
                let p99 = (
                    other.p99().as_micros() as f64,
                    step.p99().as_micros() as f64,
                );
                lines.push(Line::from(vec![
                    label(format!("{} at {name}   ", other.level)),
                    label("load "),
                    Span::raw(format!(
                        "{}   ",
                        change(other.level as f64, step.level as f64)
                    )),
                    label("ok req/s "),
                    Span::styled(
                        format!("{}   ", change(other.rps(), step.rps())),
                        Style::new().fg(if step.rps() < other.rps() * 0.95 {
                            WARN
                        } else {
                            Color::Reset
                        }),
                    ),
                    label("p99 "),
                    Span::styled(
                        change(p99.0, p99.1),
                        Style::new().fg(if p99.1 >= p99.0 * 2.0 {
                            WARN
                        } else {
                            Color::Reset
                        }),
                    ),
                ]));
            }
        }
        if let Some((level, text)) = &step.note {
            lines.push(Line::raw(""));
            lines.push(Line::from(value(
                format!("{} {text}", level.symbol()),
                level_color(*level),
            )));
        }
        f.render_widget(Paragraph::new(lines), inner);
    }

    /// The run second by second: what was answered well, with the steps
    /// told apart and the seconds that had failures marked
    fn render_timeline(&self, f: &mut Frame, area: Rect) {
        let ramp = &self.ramp;
        let right = Line::from(Span::styled(
            " ok req/s, each second of the run ",
            Style::new().fg(FAINT),
        ));
        let block =
            panel(caption("timeline", false), Some(right), RULE).padding(Padding::horizontal(1));
        let inner = block.inner(area);
        f.render_widget(block, area);
        // The second in progress is only part of one
        let seconds: &[Second] = match (&ramp.end, ramp.seconds.len()) {
            (None, n) if n > 0 => &ramp.seconds[..n - 1],
            _ => &ramp.seconds,
        };
        const AXIS: usize = 7;
        let room = (inner.width as usize).saturating_sub(AXIS);
        if seconds.is_empty() || inner.height < 2 || room == 0 {
            return;
        }
        // Laid out for the whole ramp, so it fills from the left as the run
        // goes: a column a second while they fit, widened to fill the card,
        // and several seconds to a column once they don't
        let planned = (ramp.plan.total().as_secs() as usize).max(seconds.len());
        let per_column = planned.div_ceil(room);
        let columns: Vec<(f64, f64, u16)> = seconds
            .chunks(per_column)
            .map(|chunk| {
                let ok: u32 = chunk.iter().map(|s| s.ok).sum();
                let failed: u32 = chunk.iter().map(|s| s.failed).sum();
                let all = (ok + failed).max(1) as f64;
                (
                    ok as f64 / chunk.len() as f64,
                    failed as f64 / all * 100.0,
                    chunk[0].step,
                )
            })
            .collect();
        let wide = (room / planned.div_ceil(per_column)).max(1);
        let top = columns.iter().map(|c| c.0).fold(0.0, f64::max);
        let rows = inner.height as usize - 1;

        let mut lines: Vec<Line> = Vec::with_capacity(rows + 1);
        for row in 0..rows {
            let axis = match row {
                0 => format!("{:>6} ", format::compact(top)),
                r if r == rows - 1 => format!("{:>6} ", "0"),
                _ => " ".repeat(AXIS),
            };
            let mut spans = vec![Span::styled(axis, Style::new().fg(FAINT))];
            let below = (rows - 1 - row) * 8;
            for (ok, failed, step) in &columns {
                let eighths = if top > 0.0 {
                    ((ok / top * (rows * 8) as f64).round() as usize).max(usize::from(*ok > 0.0))
                } else {
                    0
                };
                let cell = match eighths.saturating_sub(below) {
                    0 => ' ',
                    n => BLOCKS[n.min(8) - 1],
                };
                let color = match failed {
                    e if *e >= 5.0 => BAD,
                    e if *e >= 1.0 => WARN,
                    _ if step % 2 == 0 => ACCENT,
                    _ => SECOND_SHADE,
                };
                spans.push(Span::styled(
                    cell.to_string().repeat(wide),
                    Style::new().fg(color),
                ));
            }
            lines.push(Line::from(spans));
        }
        // Under each step's first second, its concurrency
        let mut labels = " ".repeat(AXIS);
        let mut last = None;
        for (i, (_, _, step)) in columns.iter().enumerate() {
            if last == Some(*step) {
                continue;
            }
            last = Some(*step);
            let at = AXIS + i * wide;
            if labels.chars().count() <= at {
                let level = ramp.plan.levels[(*step as usize).min(ramp.plan.levels.len() - 1)];
                labels.push_str(&" ".repeat(at - labels.chars().count()));
                labels.push_str(&format!("{level} "));
            }
        }
        lines.push(Line::from(Span::styled(labels, Style::new().fg(FAINT))));
        f.render_widget(Paragraph::new(lines), inner);
    }

    /// The curve the steps draw: throughput and p99 at each concurrency
    fn render_curves(&self, f: &mut Frame, area: Rect, current: Option<&Step>) {
        let steps: Vec<(&Step, Color)> = self
            .ramp
            .steps
            .iter()
            .map(|step| (step, step_color(step)))
            .chain(current.map(|step| (step, ACCENT)))
            .collect();
        let [left, right] =
            Layout::horizontal([Constraint::Percentage(50), Constraint::Min(0)]).areas(area);
        let throughput: Vec<(u32, f64, Color)> = steps
            .iter()
            .map(|(step, color)| (step.level, step.rps(), *color))
            .collect();
        column_chart(
            f,
            left,
            "throughput",
            "ok req/s at each concurrency",
            &throughput,
            self.ramp.plan.levels.len(),
            format::compact,
        );
        let latency: Vec<(u32, f64, Color)> = steps
            .iter()
            .map(|(step, color)| (step.level, step.p99().as_micros() as f64, *color))
            .collect();
        column_chart(
            f,
            right,
            "p99 latency",
            "at each concurrency",
            &latency,
            self.ramp.plan.levels.len(),
            |v| format::latency_short(v as u64),
        );
    }

    /// What the steps add up to
    fn render_result(
        &self,
        f: &mut Frame,
        area: Rect,
        found: &ramp::Findings,
        command: Option<String>,
    ) {
        let ended = self.ramp.end.is_some();
        let total = &self.ramp.total;
        let right = (total.total > 0).then(|| {
            Line::from(label(format!(
                " {} requests · {:.1}% failed · {} ",
                format::count(total.total),
                total.error_rate(),
                format::span(self.ramp.steps.iter().map(|s| s.held).sum())
            )))
        });
        let block = panel(
            caption(if ended { "result" } else { "so far" }, ended),
            right,
            if ended { ACCENT } else { RULE },
        )
        .padding(Padding::horizontal(1));
        // Titles in a column, so the details line up
        let titles = found
            .list
            .iter()
            .map(|f| f.title.chars().count())
            .max()
            .unwrap_or(0);
        let mut lines: Vec<Line> = found
            .list
            .iter()
            .map(|finding| {
                Line::from(vec![
                    value(
                        format!("{} {:<titles$}", finding.level.symbol(), finding.title),
                        level_color(finding.level),
                    ),
                    label(format!("   {}", finding.detail)),
                ])
            })
            .collect();
        if lines.is_empty() {
            lines.push(Line::from(label(if ended {
                "nothing was measured"
            } else {
                "measuring the first step…"
            })));
        }
        if let Some(command) = command {
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                label("run at the level that held:  "),
                Span::styled(command, Style::new().fg(ACCENT)),
            ]));
        }
        f.render_widget(Paragraph::new(lines).block(block), area);
    }
}

/// One column per step, as tall as its value next to the largest, spread
/// over the width of the card
fn column_chart(
    f: &mut Frame,
    area: Rect,
    title: &str,
    about: &str,
    columns: &[(u32, f64, Color)],
    planned: usize,
    unit: impl Fn(f64) -> String,
) {
    /// The axis: the top value, right-aligned, and a space
    const AXIS: usize = 7;
    let right = Line::from(Span::styled(format!(" {about} "), Style::new().fg(FAINT)));
    let block = panel(caption(title, false), Some(right), RULE).padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);
    let room = (inner.width as usize).saturating_sub(AXIS);
    if columns.is_empty() || inner.height < 3 || room < 2 {
        return;
    }
    let rows = inner.height as usize - 1;
    // Each step gets an equal share of the width: a column, then air.
    // The latest steps when they don't all fit.
    // Laid out for every step of the plan, so a column doesn't move as
    // the ones after it arrive.
    let shown = &columns[columns.len().saturating_sub(room / 2)..];
    let pitch = (room / planned.max(shown.len())).max(2);
    let wide = (pitch * 3 / 5).clamp(1, 8);
    let top = shown.iter().map(|c| c.1).fold(0.0, f64::max);

    let mut lines: Vec<Line> = Vec::with_capacity(rows + 1);
    for row in 0..rows {
        let axis = match row {
            0 => format!("{:>6} ", unit(top)),
            r if r == rows - 1 => format!("{:>6} ", "0"),
            _ => " ".repeat(AXIS),
        };
        let mut spans = vec![Span::styled(axis, Style::new().fg(FAINT))];
        // Eighths of a cell still to draw at this row, from the bottom up
        let below = (rows - 1 - row) * 8;
        for (_, amount, color) in shown {
            let eighths = if top > 0.0 {
                // What was measured always shows, if only as a sliver
                ((amount / top * (rows * 8) as f64).round() as usize)
                    .max(usize::from(*amount > 0.0))
            } else {
                0
            };
            let cell = match eighths.saturating_sub(below) {
                0 => ' ',
                n => BLOCKS[n.min(8) - 1],
            };
            spans.push(Span::styled(
                cell.to_string().repeat(wide),
                Style::new().fg(*color),
            ));
            spans.push(Span::raw(" ".repeat(pitch - wide)));
        }
        lines.push(Line::from(spans));
    }
    // Concurrency under each column, skipping some when they'd run together
    let widest = shown
        .iter()
        .map(|c| c.0.to_string().len())
        .max()
        .unwrap_or(1);
    let every = (widest + 1).div_ceil(pitch);
    let mut labels = " ".repeat(AXIS);
    for (i, (level, _, _)) in shown.iter().enumerate() {
        let text = if i % every == 0 {
            level.to_string()
        } else {
            String::new()
        };
        labels.push_str(&format!("{text:<pitch$}"));
    }
    lines.push(Line::from(Span::styled(labels, Style::new().fg(FAINT))));
    f.render_widget(Paragraph::new(lines), inner);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ramp::Condition;
    use crate::response::ResponseStats;
    use clap::Parser;
    use ratatui::backend::TestBackend;

    fn screen(levels: &[u32]) -> RampScreen {
        let mut cli = Cli::parse_from(["pepe", "ramp", "http://x.io/a"]);
        cli.url = "http://x.io/a".into();
        RampScreen::new(
            cli,
            RampPlan {
                levels: levels.to_vec(),
                every: Duration::from_secs(10),
                until: vec![Condition::parse("p99 > 100ms").unwrap()],
            },
        )
    }

    fn stat(ms: u64, code: u16) -> ResponseStats {
        ResponseStats {
            duration: Duration::from_millis(ms),
            status_code: Some(reqwest::StatusCode::from_u16(code).unwrap()),
            ..Default::default()
        }
    }

    fn press(s: &mut RampScreen, c: char, now: Instant) -> Effect {
        s.key(KeyEvent::from(KeyCode::Char(c)), now)
    }

    #[test]
    fn keys_pause_skip_and_stop() {
        let mut s = screen(&[10, 20, 30]);
        let now = Instant::now();
        assert_eq!(press(&mut s, ' ', now), Effect::Paused(true));
        assert!(s.ramp.is_paused());
        assert_eq!(press(&mut s, ' ', now), Effect::Paused(false));

        s.ramp.record(&stat(5, 200), now);
        assert_eq!(press(&mut s, 'n', now), Effect::Ramp(Tick::Level(20)));
        s.ramp.record(&stat(5, 200), now);
        assert_eq!(press(&mut s, 's', now), Effect::Ramp(Tick::Done));
        assert_eq!(s.ramp.end, Some(End::Interrupted));
        assert_eq!(s.ramp.steps.len(), 2);

        // Ended: the run keys do nothing, the rest still work
        assert_eq!(press(&mut s, 'n', now), Effect::None);
        assert_eq!(press(&mut s, ' ', now), Effect::None);
        assert_eq!(press(&mut s, 'r', now), Effect::Leave(Outcome::Restart));
        assert_eq!(press(&mut s, 'e', now), Effect::Leave(Outcome::Edit));
        assert_eq!(press(&mut s, 'q', now), Effect::Leave(Outcome::Quit));
        let report = s.report().unwrap();
        assert!(report.contains("pepe ramp · GET http://x.io/a"), "{report}");
        assert!(report.contains("2 requests in"), "{report}");
    }

    #[test]
    fn the_details_follow_the_ramp_until_a_step_is_picked() {
        let mut s = screen(&[10, 20, 30]);
        let now = Instant::now();
        assert_eq!(s.cursor(), 0);
        s.ramp.record(&stat(5, 200), now);
        s.ramp.skip(now);
        assert_eq!(s.cursor(), 1, "on to the step in progress");

        s.key(KeyEvent::from(KeyCode::Up), now);
        assert_eq!(s.cursor(), 0);
        s.ramp.record(&stat(5, 200), now);
        s.ramp.skip(now);
        assert_eq!(s.cursor(), 0, "a picked step stays picked");
        for _ in 0..5 {
            s.key(KeyEvent::from(KeyCode::Down), now);
        }
        assert_eq!(s.cursor(), 2, "steps to come can be looked at too");
        s.key(KeyEvent::from(KeyCode::Esc), now);
        assert_eq!((s.selected, s.cursor()), (None, 2));

        // Ended: the last measured step
        s.ramp.record(&stat(5, 200), now);
        s.ramp.stop(now);
        assert_eq!(s.cursor(), 2);
        assert_eq!(change(100.0, 110.0), "+10%");
        assert_eq!(change(100.0, 340.0), "×3.4");
        assert_eq!(change(0.0, 5.0), "n/a");
    }

    #[test]
    fn renders_at_many_sizes_through_a_whole_ramp() {
        let mut s = screen(&[10, 20, 30, 40]);
        let start = Instant::now();
        let sizes = [(40, 12), (80, 24), (120, 40), (160, 44), (240, 57)];
        let draw = |s: &mut RampScreen, now: Instant| {
            for (w, h) in sizes {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                // Following, then with every step picked in turn
                for selected in [None, Some(0), Some(1), Some(2), Some(3)] {
                    s.selected = selected;
                    terminal.draw(|f| s.render(f, now)).unwrap();
                }
            }
            s.selected = None;
        };
        draw(&mut s, start);
        assert!(s.report().is_none(), "nothing measured yet");
        for (i, (ms, code)) in [(5, 200), (8, 200), (400, 503)].into_iter().enumerate() {
            let begun = start + Duration::from_secs(10 * i as u64);
            for n in 0..50 * (i as u64 + 1) {
                s.ramp
                    .record(&stat(ms, code), begun + Duration::from_millis(n * 20));
            }
            let now = begun + Duration::from_secs(10);
            draw(&mut s, now - Duration::from_secs(3));
            s.ramp.tick(now);
            draw(&mut s, now);
        }
        assert_eq!(s.ramp.end, Some(End::Stopped("p99 > 100ms".into())));
        assert!(s.ramp.seconds.len() >= 20, "{:?}", s.ramp.seconds.len());
        s.ramp.set_paused(true, start);
        draw(&mut s, start);
    }
}
