mod bigtext;
mod body;
mod filter;
pub mod format;
mod mascot;
mod setup;
mod view;

pub use setup::{Setup, SetupOutcome};

use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::{FutureExt, StreamExt};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::time::MissedTickBehavior;

use crate::insights::{self, Level, Verdict};
use crate::load::{LoadHandle, Plan};
use crate::metrics::Metrics;
use crate::response::ResponseStats;
use crate::timeline::Timeline;
use crate::Cli;

/// Redraw interval while a run is live. Input redraws immediately, and a
/// finished dashboard only redraws on input, so it costs no CPU while idle.
const FRAME: Duration = Duration::from_millis(100);
/// How often results are pulled off the channel between frames. Draining
/// more often than drawing keeps the channel backlog (and memory) small at
/// high request rates, and costs almost nothing.
const PUMP: Duration = Duration::from_millis(25);
/// After a resize, wait this long for the window to stop changing before a
/// full redraw, so dragging the edge doesn't queue up dozens of them...
const RESIZE_SETTLE: Duration = Duration::from_millis(50);
/// ...but never hold the screen back longer than this while it keeps changing
const RESIZE_MAX_WAIT: Duration = Duration::from_millis(250);
/// While idle, how often to check the window size, in case a resize event
/// never arrives
const SIZE_CHECK: Duration = Duration::from_millis(250);
/// Requests kept for the request log
const LOG_CAPACITY: usize = 2_000;
/// Failed requests kept separately, so a burst of successes can't push them
/// out of a failures view
const ERROR_LOG_CAPACITY: usize = 500;
/// Memory the inspector's full responses may use; the oldest are dropped first
const DETAIL_BUDGET: usize = 32 * 1024 * 1024;
/// Distinct failure causes counted; any more are counted as "other"
const MAX_FAILURE_CAUSES: usize = 32;
/// How long a notice (e.g. "concurrency 64 → 70") stays in the footer
const NOTICE_TTL: Duration = Duration::from_secs(2);

/// What the user asked for when leaving the dashboard
pub enum Outcome {
    Quit,
    Restart,
    /// Back to the setup screen, to change the settings and run again
    Edit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Live,
    Stats,
    Requests,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Live, Tab::Stats, Tab::Requests];

    fn title(self) -> &'static str {
        match self {
            Tab::Live => "Live",
            Tab::Stats => "Stats",
            Tab::Requests => "Requests",
        }
    }

    fn index(self) -> usize {
        Self::ALL.iter().position(|&t| t == self).unwrap_or(0)
    }

    fn cycle(self, step: isize) -> Tab {
        let n = Self::ALL.len() as isize;
        Self::ALL[(self.index() as isize + step).rem_euclid(n) as usize]
    }
}

/// A finished request as shown in the log
struct LogEntry {
    /// 1-based completion order
    seq: u64,
    /// When it completed, relative to the run start
    at: Duration,
    stat: ResponseStats,
}

impl LogEntry {
    fn is_error(&self) -> bool {
        !self.stat.status_code.is_some_and(|code| code.is_success())
    }
}

pub struct Dashboard {
    args: Cli,
    plan: Plan,
    metrics: Metrics,
    timeline: Timeline,
    log: VecDeque<LogEntry>,
    error_log: VecDeque<LogEntry>,
    /// Failed requests by cause ("HTTP 503", "Connection refused ...")
    failure_causes: HashMap<Box<str>, u64>,
    sent: u64,
    concurrency: usize,
    paused: bool,
    /// Time spent paused, which the run's clock leaves out
    paused_total: Duration,
    paused_since: Option<Instant>,
    started: Instant,
    /// Set once every request has finished (or the run was stopped)
    finished: Option<Duration>,
    interrupted: bool,
    /// Findings, worked out once the run is over
    verdict: Option<Verdict>,
    /// Frames drawn while live, for the mascot's animation
    frame: u64,

    tab: Tab,
    show_help: bool,
    /// What the request log shows
    filter: filter::Filter,
    /// Latency the slow filter requires, refreshed as results arrive
    slow_threshold_us: u64,
    /// Selected row in the request log, counted from the newest; 0 follows
    /// live
    scroll: usize,
    /// The selected request is open in the inspector
    inspecting: bool,
    /// Lines scrolled down in the inspector's response
    detail_scroll: u16,
    /// Furthest the response can scroll, and its visible height; set when
    /// drawn, since only drawing knows how the text wraps
    detail_view: std::cell::Cell<(u16, u16)>,
    /// Show response bodies as received instead of formatted
    raw_body: bool,
    /// Requests holding a full response, oldest first, with their size
    detailed: VecDeque<(u64, usize)>,
    detail_bytes: usize,
    notice: Option<(String, Instant)>,
}

impl Dashboard {
    pub fn new(args: Cli, plan: Plan) -> Self {
        Self {
            concurrency: args.concurrency as usize,
            args,
            plan,
            metrics: Metrics::default(),
            timeline: Timeline::default(),
            log: VecDeque::with_capacity(LOG_CAPACITY),
            error_log: VecDeque::with_capacity(ERROR_LOG_CAPACITY),
            failure_causes: HashMap::new(),
            sent: 0,
            paused: false,
            paused_total: Duration::ZERO,
            paused_since: None,
            started: Instant::now(),
            finished: None,
            interrupted: false,
            verdict: None,
            frame: 0,
            tab: Tab::Live,
            show_help: false,
            filter: filter::Filter::default(),
            slow_threshold_us: 0,
            scroll: 0,
            inspecting: false,
            detail_scroll: 0,
            detail_view: std::cell::Cell::new((0, 0)),
            raw_body: false,
            detailed: VecDeque::new(),
            detail_bytes: 0,
            notice: None,
        }
    }

    /// How long the run has been going, not counting pauses
    fn elapsed(&self) -> Duration {
        self.finished.unwrap_or_else(|| self.active())
    }

    fn active(&self) -> Duration {
        let pausing = self.paused_since.map_or(Duration::ZERO, |t| t.elapsed());
        self.started
            .elapsed()
            .saturating_sub(self.paused_total + pausing)
    }

    fn set_paused(&mut self, paused: bool) {
        match (paused, self.paused_since) {
            (true, None) => self.paused_since = Some(Instant::now()),
            (false, Some(since)) => {
                self.paused_total += since.elapsed();
                self.paused_since = None;
            }
            _ => {}
        }
        self.paused = paused;
    }

    fn record(&mut self, stat: ResponseStats) {
        self.metrics.record(&stat);
        self.timeline.record(&stat);
        let entry = LogEntry {
            seq: self.metrics.total,
            at: self.active(),
            stat,
        };
        if entry.is_error() {
            self.count_failure(&entry.stat);
        }
        // The list holds still while a request is open in the inspector: at
        // high rates new rows would push the one being read out within
        // milliseconds. Everything else keeps counting.
        if self.inspecting {
            return;
        }

        // Keep a scrolled-back view anchored on the same rows
        if self.scroll > 0 && self.filter.matches(&entry.stat, self.slow_threshold_us) {
            self.scroll += 1;
        }
        if entry.is_error() {
            push_bounded(
                &mut self.error_log,
                LogEntry {
                    seq: entry.seq,
                    at: entry.at,
                    stat: entry.stat.clone(),
                },
                ERROR_LOG_CAPACITY,
            );
        }
        let detail = entry.stat.detail.as_ref().map(|d| (entry.seq, d.size()));
        push_bounded(&mut self.log, entry, LOG_CAPACITY);
        if let Some((seq, size)) = detail {
            self.detailed.push_back((seq, size));
            self.detail_bytes += size;
        }
        self.trim_details();
    }

    /// Forget full responses that left both logs, then the oldest ones until
    /// they fit the budget
    fn trim_details(&mut self) {
        let oldest_kept = match (self.log.front(), self.error_log.front()) {
            (Some(a), Some(b)) => a.seq.min(b.seq),
            (Some(e), None) | (None, Some(e)) => e.seq,
            (None, None) => u64::MAX,
        };
        while let Some(&(seq, size)) = self.detailed.front() {
            if seq >= oldest_kept && self.detail_bytes <= DETAIL_BUDGET {
                break;
            }
            self.detailed.pop_front();
            self.detail_bytes -= size;
            for log in [&mut self.log, &mut self.error_log] {
                if let Ok(i) = log.binary_search_by_key(&seq, |e| e.seq) {
                    log[i].stat.detail = None;
                }
            }
        }
    }

    fn count_failure(&mut self, stat: &ResponseStats) {
        let cause = match (stat.status_code, &stat.error_message, stat.error) {
            (Some(code), _, _) => format!("HTTP {}", code.as_u16()),
            (None, Some(message), _) => message.to_string(),
            (None, None, Some(kind)) => kind.label().to_lowercase(),
            (None, None, None) => "error".to_string(),
        };
        if let Some(n) = self.failure_causes.get_mut(cause.as_str()) {
            *n += 1;
        } else if self.failure_causes.len() < MAX_FAILURE_CAUSES {
            self.failure_causes.insert(cause.into(), 1);
        } else {
            *self.failure_causes.entry("other".into()).or_insert(0) += 1;
        }
    }

    /// Failure causes, most frequent first
    fn top_failure_causes(&self) -> Vec<(&str, u64)> {
        let mut causes: Vec<(&str, u64)> = self
            .failure_causes
            .iter()
            .map(|(cause, &n)| (cause.as_ref(), n))
            .collect();
        causes.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
        causes
    }

    /// Pull everything the load generator produced since the last frame
    fn drain(&mut self, load: &mut LoadHandle) {
        loop {
            match load.rx.try_recv() {
                Ok(stat) => self.record(stat),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.finished.is_none() {
                        let now = self.active();
                        self.finished = Some(now);
                        self.timeline.finish(now);
                        let samples: Vec<_> = self.timeline.samples().iter().copied().collect();
                        self.verdict =
                            Some(insights::verdict(&self.metrics, &samples, self.interrupted));
                    }
                    break;
                }
            }
        }
        if self.finished.is_none() {
            self.timeline.advance(self.active());
        }
        self.refresh_slow_threshold();
        if self.scroll > 0 {
            self.scroll = self.scroll.min(self.visible_log().len().saturating_sub(1));
        }
        self.sent = load.sent();
        self.concurrency = load.concurrency();
        self.set_paused(load.is_paused());
        if self
            .notice
            .as_ref()
            .is_some_and(|(_, at)| at.elapsed() >= NOTICE_TTL)
        {
            self.notice = None;
        }
    }

    /// Whether anything on screen changes without input
    fn animating(&self) -> bool {
        self.finished.is_none() || self.notice.is_some()
    }

    fn refresh_slow_threshold(&mut self) {
        self.slow_threshold_us = self
            .filter
            .slow
            .quantile()
            .map_or(0, |q| self.metrics.percentile(q).as_micros() as u64);
    }

    /// Log entries that pass the filter, newest first. Failures that already
    /// left the main log are still found in the error log.
    fn visible_log(&self) -> Vec<&LogEntry> {
        let oldest = self.log.front().map_or(u64::MAX, |e| e.seq);
        let older_failures = self.error_log.iter().rev().filter(|e| e.seq < oldest);
        self.log
            .iter()
            .rev()
            .chain(older_failures)
            .filter(|e| self.filter.matches(&e.stat, self.slow_threshold_us))
            .collect()
    }

    /// Entries kept in total, whatever the filter
    fn kept(&self) -> usize {
        let oldest = self.log.front().map_or(u64::MAX, |e| e.seq);
        self.log.len() + self.error_log.iter().filter(|e| e.seq < oldest).count()
    }

    fn refilter(&mut self) {
        self.scroll = 0;
        self.tab = Tab::Requests;
        self.refresh_slow_threshold();
    }

    /// Typing into the search box. Returns false for keys it doesn't handle.
    fn edit_search(&mut self, key: KeyEvent) -> bool {
        match key.code {
            KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.filter.query.push(c)
            }
            KeyCode::Backspace => {
                self.filter.query.pop();
            }
            KeyCode::Enter => self.filter.editing = false,
            KeyCode::Esc => {
                self.filter.query.clear();
                self.filter.editing = false;
            }
            _ => return false,
        }
        self.scroll = 0;
        true
    }

    fn notify(&mut self, message: String) {
        self.notice = Some((message, Instant::now()));
    }

    pub async fn run(
        &mut self,
        load: &mut LoadHandle,
    ) -> Result<Outcome, Box<dyn std::error::Error>> {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;
        let mut events = EventStream::new();
        let mut frames = tokio::time::interval(FRAME);
        frames.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut pump = tokio::time::interval(PUMP);
        pump.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut size_check = tokio::time::interval(SIZE_CHECK);
        size_check.set_missed_tick_behavior(MissedTickBehavior::Skip);
        // Ctrl-C arrives as a signal (see `keep_ctrl_c_a_signal`)
        let ctrl_c = tokio::signal::ctrl_c();
        tokio::pin!(ctrl_c);

        let mut dirty = true;
        let mut drawn_size = None;
        loop {
            if dirty {
                terminal.draw(|f| view::render(self, f))?;
                drawn_size = crossterm::terminal::size().ok();
                dirty = false;
            }
            tokio::select! {
                _ = &mut ctrl_c => return Ok(Outcome::Quit),
                _ = frames.tick(), if self.animating() => {
                    self.frame += 1;
                    self.drain(load);
                    dirty = true;
                }
                _ = pump.tick(), if self.finished.is_none() => {
                    self.drain(load);
                    // Show the end of the run right away
                    dirty |= self.finished.is_some();
                }
                // Frames redraw at the current size anyway while animating
                _ = size_check.tick(), if !self.animating() => {
                    dirty = crossterm::terminal::size().ok() != drawn_size;
                }
                event = events.next() => match event {
                    Some(Ok(event)) => {
                        if let Some(outcome) = self.handle_burst(event, &mut events, load).await {
                            return Ok(outcome);
                        }
                        dirty = true;
                    }
                    Some(Err(e)) => return Err(e.into()),
                    None => return Ok(Outcome::Quit),
                },
            }
        }
    }

    /// Handle `first` and every event queued behind it, so a burst (fast
    /// typing, a window being dragged) costs one redraw instead of one each.
    /// After a resize, also wait briefly for the size to settle.
    async fn handle_burst(
        &mut self,
        first: Event,
        events: &mut EventStream,
        load: &LoadHandle,
    ) -> Option<Outcome> {
        let mut resized = matches!(first, Event::Resize(..));
        if let Some(outcome) = self.handle(first, load) {
            return Some(outcome);
        }
        let deadline = tokio::time::Instant::now() + RESIZE_MAX_WAIT;
        loop {
            let next = if resized {
                let settle = (tokio::time::Instant::now() + RESIZE_SETTLE).min(deadline);
                tokio::time::timeout_at(settle, events.next()).await.ok()
            } else {
                events.next().now_or_never()
            };
            match next {
                Some(Some(Ok(event))) => {
                    resized |= matches!(event, Event::Resize(..));
                    if let Some(outcome) = self.handle(event, load) {
                        return Some(outcome);
                    }
                }
                Some(None) => return Some(Outcome::Quit),
                _ => return None,
            }
        }
    }

    fn handle(&mut self, event: Event, load: &LoadHandle) -> Option<Outcome> {
        // Anything else (resize, focus, ...) just triggers the redraw
        let Event::Key(key) = event else { return None };
        // Windows reports key releases too; act on presses only
        if key.kind != KeyEventKind::Press {
            return None;
        }
        self.handle_key(key, load)
    }

    fn handle_key(&mut self, key: KeyEvent, load: &LoadHandle) -> Option<Outcome> {
        let running = self.finished.is_none();
        if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
            return Some(Outcome::Quit);
        }
        if self.filter.editing && self.edit_search(key) {
            return None;
        }
        if self.inspecting && self.tab == Tab::Requests && !self.show_help && self.inspect_key(key)
        {
            return None;
        }
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') if self.show_help => self.show_help = false,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Esc if self.filter.is_active() => self.filter.clear(),
            KeyCode::Char('q') | KeyCode::Esc => return Some(Outcome::Quit),
            KeyCode::Char('r') => return Some(Outcome::Restart),
            KeyCode::Char('E') => return Some(Outcome::Edit),
            KeyCode::Char('s') | KeyCode::Char('i') if running => {
                // Stop sending; results so far stay on screen
                load.stop();
                self.interrupted = true;
                self.notify("run stopped".into());
            }
            KeyCode::Char(' ') | KeyCode::Char('p') if running => {
                let paused = !load.is_paused();
                load.set_paused(paused);
                self.set_paused(paused);
                self.notify(if paused { "paused" } else { "resumed" }.into());
            }
            KeyCode::Char('+') | KeyCode::Char('=') if running => self.adjust_concurrency(load, 1),
            KeyCode::Char('-') | KeyCode::Char('_') if running => self.adjust_concurrency(load, -1),

            KeyCode::Tab | KeyCode::Right => self.tab = self.tab.cycle(1),
            KeyCode::BackTab | KeyCode::Left => self.tab = self.tab.cycle(-1),
            KeyCode::Char(c @ '1'..='3') => self.tab = Tab::ALL[c as usize - '1' as usize],

            KeyCode::Char('f') => {
                self.filter.status = self.filter.status.next();
                self.refilter();
            }
            KeyCode::Char('e') => {
                self.filter.status = match self.filter.status {
                    filter::Status::Failed => filter::Status::All,
                    _ => filter::Status::Failed,
                };
                self.refilter();
            }
            KeyCode::Char('l') => {
                self.filter.slow = self.filter.slow.next();
                self.refilter();
            }
            KeyCode::Char('/') => {
                self.filter.editing = true;
                self.refilter();
            }
            KeyCode::Char('c') => {
                self.filter.clear();
                self.refilter();
            }
            KeyCode::Enter if self.tab == Tab::Requests && !self.visible_log().is_empty() => {
                self.inspecting = true;
                self.detail_scroll = 0;
            }
            // The log lists the newest first: up is newer, down is older
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(1),
            KeyCode::PageUp => self.scroll_by(-10),
            KeyCode::PageDown => self.scroll_by(10),
            KeyCode::Home | KeyCode::Char('g') => self.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.scroll_by(isize::MAX / 2),
            _ => {}
        }
        None
    }

    /// How the mascot feels about the run so far
    fn mood(&self) -> mascot::Mood {
        use mascot::Mood;
        if self.interrupted && self.finished.is_some() {
            return Mood::Dizzy;
        }
        if let Some(verdict) = &self.verdict {
            return match verdict.level {
                Level::Healthy => Mood::Proud,
                _ => Mood::Worried,
            };
        }
        if self.paused {
            return Mood::Sleeping;
        }
        let Some(now) = self.timeline.last() else {
            return Mood::Waiting;
        };
        let error_share = if now.rps > 0.0 {
            now.errors / now.rps
        } else {
            0.0
        };
        let median_ms = self.metrics.percentile(50.0).as_secs_f64() * 1000.0;
        if error_share >= 0.05 {
            Mood::OnFire
        } else if error_share >= 0.005 || (median_ms > 0.0 && now.p99_ms > 10.0 * median_ms) {
            Mood::Sweating
        } else {
            Mood::Happy
        }
    }

    /// Headline numbers, most important first
    fn summary_parts(&self) -> Vec<String> {
        let m = &self.metrics;
        let elapsed = self.elapsed();
        vec![
            format!(
                "{} requests in {}",
                format::count(m.total),
                format::span(elapsed)
            ),
            format!("{} req/s", format::compact(m.rps(elapsed))),
            format!("p99 {}", format::latency(m.percentile(99.0))),
            format!("p50 {}", format::latency(m.percentile(50.0))),
            format!("{:.2}% ok", 100.0 - m.error_rate()),
        ]
    }

    /// Plain-text verdict to leave in the shell after quitting
    pub fn report(&self) -> Option<String> {
        let verdict = self.verdict.as_ref()?;
        let mut out = format!(
            "pepe · {} {} · ×{}\n{} {} · {}\n",
            self.args.method,
            self.args.url,
            self.concurrency,
            verdict.level.symbol(),
            verdict.level.headline(),
            self.summary_parts().join(" · ")
        );
        for note in &verdict.notes {
            out += &format!("  {} {}\n", note.level.symbol(), note.text);
        }
        Some(out)
    }

    /// Keys while a request is open in the inspector. Returns false for keys
    /// it leaves to the normal handling (quit, pause, switching tabs, ...).
    fn inspect_key(&mut self, key: KeyEvent) -> bool {
        let page = self.detail_view.get().1.max(2) as i32 - 1;
        match key.code {
            KeyCode::Esc | KeyCode::Enter | KeyCode::Backspace => self.inspecting = false,
            // Scroll the response like a pager. Terminals turn the trackpad
            // and mouse wheel into up/down here, so those scroll too.
            KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
            KeyCode::PageUp | KeyCode::Char('u') => self.scroll_detail(-page),
            KeyCode::PageDown | KeyCode::Char('d') => self.scroll_detail(page),
            KeyCode::Home | KeyCode::Char('g') => self.scroll_detail(i32::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.scroll_detail(i32::MAX / 2),
            // Walk through the requests: the list is newest first
            KeyCode::Left | KeyCode::Char('h') => self.step_inspected(-1),
            KeyCode::Right | KeyCode::Char('l') => self.step_inspected(1),
            // Jump to the nearest request whose full response was kept
            KeyCode::Char('[') => self.step_to_full(-1),
            KeyCode::Char(']') => self.step_to_full(1),
            KeyCode::Char('v') => {
                self.raw_body = !self.raw_body;
                self.detail_scroll = 0;
            }
            _ => return false,
        }
        true
    }

    /// Scroll the inspector's response, stopping at its last page
    fn scroll_detail(&mut self, lines: i32) {
        let (max, _) = self.detail_view.get();
        self.detail_scroll = (self.detail_scroll as i32 + lines).clamp(0, max as i32) as u16;
    }

    /// Move to the nearest newer (-1) or older (1) request with a full
    /// response; stay put if there's none that way
    fn step_to_full(&mut self, direction: isize) {
        let log = self.visible_log();
        let mut i = self.scroll as isize + direction;
        while i >= 0 && (i as usize) < log.len() {
            if log[i as usize].stat.detail.is_some() {
                let target = i as usize;
                drop(log);
                self.scroll = target;
                self.detail_scroll = 0;
                return;
            }
            i += direction;
        }
    }

    fn step_inspected(&mut self, rows: isize) {
        self.scroll_by(rows);
        self.detail_scroll = 0;
    }

    fn scroll_by(&mut self, rows: isize) {
        let max = self.visible_log().len().saturating_sub(1);
        self.scroll = self.scroll.saturating_add_signed(rows).min(max);
        if self.tab != Tab::Requests {
            self.tab = Tab::Requests;
        }
    }

    /// Step concurrency by about 10% (at least one) in `direction`
    fn adjust_concurrency(&mut self, load: &LoadHandle, direction: isize) {
        let current = load.concurrency();
        let step = (current / 10).max(1);
        let target = if direction > 0 {
            current.saturating_add(step)
        } else {
            current.saturating_sub(step)
        };
        let now = load.set_concurrency(target);
        self.concurrency = now;
        self.notify(format!("concurrency {current} → {now}"));
    }
}

fn push_bounded<T>(queue: &mut VecDeque<T>, item: T, capacity: usize) {
    if queue.len() == capacity {
        queue.pop_front();
    }
    queue.push_back(item);
}

/// Progress in percent (0..=100)
fn progress_percent(plan: Plan, completed: u64, elapsed: Duration, finished: bool) -> u16 {
    if finished {
        return 100;
    }
    let ratio = match plan {
        Plan::Count(0) => 1.0,
        Plan::Count(n) => completed as f64 / n as f64,
        Plan::Duration(d) if d.is_zero() => 1.0,
        Plan::Duration(d) => elapsed.as_secs_f64() / d.as_secs_f64(),
    };
    // In-flight requests at the end of a duration run finish after the
    // deadline: hold at 99% until they do
    (ratio * 100.0).clamp(0.0, 99.0) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use reqwest::StatusCode;

    fn dashboard() -> Dashboard {
        let args = Cli::parse_from(["pepe", "-c", "4", "http://x"]);
        Dashboard::new(args, Plan::Count(100))
    }

    /// A run with nothing to send, for key handling that needs a handle
    fn dummy_load() -> LoadHandle {
        let request = Cli::parse_from(["pepe", "-c", "1", "http://x"])
            .request()
            .unwrap();
        let client = request.build_client().unwrap();
        crate::load::start(client, request, 1, Plan::Count(0), false)
    }

    fn stat(status: u16) -> ResponseStats {
        ResponseStats {
            status_code: Some(StatusCode::from_u16(status).unwrap()),
            ..Default::default()
        }
    }

    #[test]
    fn progress_is_bounded_in_both_modes() {
        let secs = Duration::from_secs;
        // Count mode never exceeds 99% before the run reports finished
        assert_eq!(progress_percent(Plan::Count(10), 5, secs(1), false), 50);
        assert_eq!(progress_percent(Plan::Count(10), 50, secs(1), false), 99);
        assert_eq!(progress_percent(Plan::Count(10), 10, secs(1), true), 100);
        // Duration mode is time based, whatever the request count
        let d = Plan::Duration(secs(10));
        assert_eq!(progress_percent(d, 1_000_000, secs(5), false), 50);
        assert_eq!(progress_percent(d, 1_000_000, secs(60), false), 99);
        assert_eq!(progress_percent(Plan::Count(0), 0, secs(0), false), 99);
    }

    #[test]
    fn logs_are_bounded_and_errors_kept_apart() {
        let mut d = dashboard();
        d.record(stat(500));
        for _ in 0..LOG_CAPACITY + 10 {
            d.record(stat(200));
        }
        assert_eq!(d.log.len(), LOG_CAPACITY);
        assert_eq!(d.log.back().unwrap().seq, LOG_CAPACITY as u64 + 11);
        // The early failure fell out of the main log but not the error log
        assert_eq!(d.error_log.len(), 1);
        assert_eq!(d.error_log[0].seq, 1);
    }

    #[test]
    fn scrolled_log_stays_anchored() {
        let mut d = dashboard();
        for _ in 0..20 {
            d.record(stat(200));
        }
        d.scroll_by(5);
        assert_eq!((d.scroll, d.tab), (5, Tab::Requests));
        d.record(stat(200));
        assert_eq!(d.scroll, 6, "view keeps pointing at the same rows");
        d.scroll_by(-100);
        assert_eq!(d.scroll, 0);
        // While inspecting, the list holds still and the counts go on
        d.inspecting = true;
        let (kept, total) = (d.log.len(), d.metrics.total);
        d.record(stat(200));
        assert_eq!((d.scroll, d.log.len()), (0, kept));
        assert_eq!(d.metrics.total, total + 1);
        d.inspecting = false;
        d.record(stat(200));
        d.scroll_by(isize::MAX / 2);
        assert_eq!(d.scroll, 21, "the oldest of 22");
    }

    #[test]
    fn filters_reach_failures_older_than_the_main_log() {
        let mut d = dashboard();
        d.record(stat(503));
        for _ in 0..LOG_CAPACITY {
            d.record(stat(200));
        }
        d.record(stat(404));
        assert_eq!(d.kept(), LOG_CAPACITY + 1);

        d.filter.status = filter::Status::Failed;
        let seqs: Vec<u64> = d.visible_log().iter().map(|e| e.seq).collect();
        assert_eq!(seqs, vec![LOG_CAPACITY as u64 + 2, 1], "newest first");

        d.filter.status = filter::Status::ClientError;
        assert_eq!(d.visible_log().len(), 1);
        d.filter.clear();
        d.filter.query = "503".into();
        assert_eq!(d.visible_log().len(), 1);
    }

    #[test]
    fn typing_in_search_does_not_trigger_shortcuts() {
        let mut d = dashboard();
        d.filter.editing = true;
        for c in "quit".chars() {
            assert!(d.edit_search(KeyEvent::from(KeyCode::Char(c))));
        }
        assert_eq!(d.filter.query, "quit");
        d.edit_search(KeyEvent::from(KeyCode::Backspace));
        d.edit_search(KeyEvent::from(KeyCode::Enter));
        assert_eq!((d.filter.query.as_str(), d.filter.editing), ("qui", false));
    }

    #[test]
    fn failures_are_grouped_by_cause() {
        let mut d = dashboard();
        let refused = || ResponseStats {
            error: Some(crate::response::ErrorKind::Connect),
            error_message: Some("Connection refused".into()),
            ..Default::default()
        };
        d.record(refused());
        d.record(refused());
        d.record(stat(503));
        d.record(stat(200));
        assert_eq!(
            d.top_failure_causes(),
            vec![("Connection refused", 2), ("HTTP 503", 1)]
        );
        for code in 400..450 {
            d.record(stat(code));
        }
        assert_eq!(
            d.failure_causes.len(),
            MAX_FAILURE_CAUSES + 1,
            "capped, plus other"
        );
    }

    #[test]
    fn full_responses_fit_the_budget() {
        use crate::response::Detail;
        let mut d = dashboard();
        let big = || ResponseStats {
            status_code: Some(StatusCode::OK),
            detail: Some(std::sync::Arc::new(Detail {
                version: reqwest::Version::HTTP_11,
                headers: Default::default(),
                body: bytes::Bytes::from(vec![b'x'; 1024 * 1024]),
                truncated: false,
                remote_addr: None,
                final_url: String::new(),
            })),
            ..Default::default()
        };
        for _ in 0..40 {
            d.record(big());
        }
        assert!(d.detail_bytes <= DETAIL_BUDGET);
        let kept = d.log.iter().filter(|e| e.stat.detail.is_some()).count();
        assert!((20..40).contains(&kept), "kept={kept}");
        // The newest keep theirs, the oldest lost them
        assert!(d.log.back().unwrap().stat.detail.is_some());
        assert!(d.log.front().unwrap().stat.detail.is_none());
    }

    #[tokio::test]
    async fn inspector_walks_the_log() {
        let mut d = dashboard();
        for _ in 0..5 {
            d.record(stat(200));
        }
        d.tab = Tab::Requests;
        d.handle_key(KeyEvent::from(KeyCode::Enter), &dummy_load());
        assert!(d.inspecting);
        d.handle_key(KeyEvent::from(KeyCode::Right), &dummy_load());
        d.handle_key(KeyEvent::from(KeyCode::Right), &dummy_load());
        assert_eq!(
            d.visible_log()[d.scroll].seq,
            3,
            "two older than the newest"
        );
        // As drawn: 50 lines past the first screen, 20 lines tall
        d.detail_view.set((50, 20));
        d.handle_key(KeyEvent::from(KeyCode::Down), &dummy_load());
        d.handle_key(KeyEvent::from(KeyCode::PageDown), &dummy_load());
        assert_eq!(d.detail_scroll, 20, "a line, then a page");
        d.handle_key(KeyEvent::from(KeyCode::Char('G')), &dummy_load());
        assert_eq!(d.detail_scroll, 50, "stops at the last page");
        d.handle_key(KeyEvent::from(KeyCode::Char('d')), &dummy_load());
        assert_eq!(d.detail_scroll, 50);
        d.handle_key(KeyEvent::from(KeyCode::Char('g')), &dummy_load());
        assert_eq!(d.detail_scroll, 0);
        d.handle_key(KeyEvent::from(KeyCode::Up), &dummy_load());
        assert_eq!(d.detail_scroll, 0, "not past the top");
        d.handle_key(KeyEvent::from(KeyCode::Char('G')), &dummy_load());
        d.handle_key(KeyEvent::from(KeyCode::Left), &dummy_load());
        assert_eq!((d.visible_log()[d.scroll].seq, d.detail_scroll), (4, 0));
        d.handle_key(KeyEvent::from(KeyCode::Esc), &dummy_load());
        assert!(!d.inspecting);
    }

    #[tokio::test]
    async fn brackets_jump_to_kept_responses() {
        use crate::response::Detail;
        let mut d = dashboard();
        for i in 0..10 {
            let mut s = stat(200);
            if i == 2 || i == 7 {
                s.detail = Some(std::sync::Arc::new(Detail {
                    version: reqwest::Version::HTTP_11,
                    headers: Default::default(),
                    body: Default::default(),
                    truncated: false,
                    remote_addr: None,
                    final_url: String::new(),
                }));
            }
            d.record(s);
        }
        d.tab = Tab::Requests;
        let load = dummy_load();
        d.handle_key(KeyEvent::from(KeyCode::Enter), &load);
        let seq = |d: &Dashboard| d.visible_log()[d.scroll].seq;
        assert_eq!(seq(&d), 10);
        d.handle_key(KeyEvent::from(KeyCode::Char(']')), &load);
        assert_eq!(seq(&d), 8, "the newer kept one");
        d.handle_key(KeyEvent::from(KeyCode::Char(']')), &load);
        assert_eq!(seq(&d), 3);
        d.handle_key(KeyEvent::from(KeyCode::Char(']')), &load);
        assert_eq!(seq(&d), 3, "none older: stays put");
        d.handle_key(KeyEvent::from(KeyCode::Char('[')), &load);
        assert_eq!(seq(&d), 8);
    }

    #[test]
    fn the_clock_stops_while_paused() {
        let mut d = dashboard();
        d.started = Instant::now() - Duration::from_secs(10);
        d.set_paused(true);
        d.paused_since = Some(Instant::now() - Duration::from_secs(4));
        let during = d.elapsed();
        assert!(
            (5_900..=6_100).contains(&(during.as_millis() as u64)),
            "{during:?}"
        );
        d.set_paused(false);
        let after = d.elapsed();
        // Resuming doesn't make the clock jump
        assert!(
            after.abs_diff(during) < Duration::from_millis(100),
            "{during:?} {after:?}"
        );
    }

    #[test]
    fn tabs_cycle_both_ways() {
        assert_eq!(Tab::Live.cycle(1), Tab::Stats);
        assert_eq!(Tab::Live.cycle(-1), Tab::Requests);
        assert_eq!(Tab::Requests.cycle(1), Tab::Live);
    }
}
