mod format;
mod view;

use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc::error::TryRecvError;
use tokio::time::MissedTickBehavior;

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
/// Requests kept for the request log
const LOG_CAPACITY: usize = 500;
/// Failed requests kept separately, so a burst of successes can't push them
/// out of the errors-only view
const ERROR_LOG_CAPACITY: usize = 200;
/// How long a notice (e.g. "concurrency 64 → 70") stays in the footer
const NOTICE_TTL: Duration = Duration::from_secs(2);

/// What the user asked for when leaving the dashboard
pub enum Outcome {
    Quit,
    Restart,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Overview,
    Latency,
    Requests,
}

impl Tab {
    const ALL: [Tab; 3] = [Tab::Overview, Tab::Latency, Tab::Requests];

    fn title(self) -> &'static str {
        match self {
            Tab::Overview => "Overview",
            Tab::Latency => "Latency",
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
    sent: u64,
    concurrency: usize,
    paused: bool,
    started: Instant,
    /// Set once every request has finished (or the run was stopped)
    finished: Option<Duration>,
    interrupted: bool,

    tab: Tab,
    show_help: bool,
    /// Request log shows failures only
    errors_only: bool,
    /// Rows scrolled back from the newest entry; 0 follows live
    scroll: usize,
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
            sent: 0,
            paused: false,
            started: Instant::now(),
            finished: None,
            interrupted: false,
            tab: Tab::Overview,
            show_help: false,
            errors_only: false,
            scroll: 0,
            notice: None,
        }
    }

    fn elapsed(&self) -> Duration {
        self.finished.unwrap_or_else(|| self.started.elapsed())
    }

    fn record(&mut self, stat: ResponseStats) {
        self.metrics.record(&stat);
        self.timeline.record(&stat);
        let entry = LogEntry {
            seq: self.metrics.total,
            at: self.started.elapsed(),
            stat,
        };

        // Keep a scrolled-back view anchored on the same rows
        if self.scroll > 0 && (!self.errors_only || entry.is_error()) {
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
        push_bounded(&mut self.log, entry, LOG_CAPACITY);
        self.scroll = self.scroll.min(self.visible_log().len().saturating_sub(1));
    }

    /// Pull everything the load generator produced since the last frame
    fn drain(&mut self, load: &mut LoadHandle) {
        loop {
            match load.rx.try_recv() {
                Ok(stat) => self.record(stat),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.finished.is_none() {
                        let now = self.started.elapsed();
                        self.finished = Some(now);
                        self.timeline.finish(now);
                    }
                    break;
                }
            }
        }
        if self.finished.is_none() {
            self.timeline.advance(self.started.elapsed());
        }
        self.sent = load.sent();
        self.concurrency = load.concurrency();
        self.paused = load.is_paused();
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

    fn visible_log(&self) -> &VecDeque<LogEntry> {
        if self.errors_only {
            &self.error_log
        } else {
            &self.log
        }
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

        let mut dirty = true;
        loop {
            if dirty {
                terminal.draw(|f| view::render(self, f))?;
                dirty = false;
            }
            tokio::select! {
                _ = frames.tick(), if self.animating() => {
                    self.drain(load);
                    dirty = true;
                }
                _ = pump.tick(), if self.finished.is_none() => {
                    self.drain(load);
                    // Show the end of the run right away
                    dirty |= self.finished.is_some();
                }
                event = events.next() => match event {
                    Some(Ok(event)) => {
                        if let Some(outcome) = self.handle(event, load) {
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
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Some(Outcome::Quit)
            }
            KeyCode::Esc | KeyCode::Char('?') if self.show_help => self.show_help = false,
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('q') | KeyCode::Esc => return Some(Outcome::Quit),
            KeyCode::Char('r') => return Some(Outcome::Restart),
            KeyCode::Char('s') | KeyCode::Char('i') if running => {
                // Stop sending; results so far stay on screen
                load.stop();
                self.interrupted = true;
                self.notify("run stopped".into());
            }
            KeyCode::Char(' ') | KeyCode::Char('p') if running => {
                let paused = !load.is_paused();
                load.set_paused(paused);
                self.paused = paused;
                self.notify(if paused { "paused" } else { "resumed" }.into());
            }
            KeyCode::Char('+') | KeyCode::Char('=') if running => self.adjust_concurrency(load, 1),
            KeyCode::Char('-') | KeyCode::Char('_') if running => self.adjust_concurrency(load, -1),

            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => self.tab = self.tab.cycle(1),
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => self.tab = self.tab.cycle(-1),
            KeyCode::Char(c @ '1'..='3') => self.tab = Tab::ALL[c as usize - '1' as usize],

            KeyCode::Char('e') => {
                self.errors_only = !self.errors_only;
                self.scroll = 0;
                self.tab = Tab::Requests;
            }
            KeyCode::Up | KeyCode::Char('k') => self.scroll_by(1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_by(-1),
            KeyCode::PageUp => self.scroll_by(10),
            KeyCode::PageDown => self.scroll_by(-10),
            KeyCode::Home | KeyCode::Char('g') => self.scroll = 0,
            KeyCode::End | KeyCode::Char('G') => self.scroll_by(isize::MAX / 2),
            _ => {}
        }
        None
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
        d.scroll_by(isize::MAX / 2);
        assert_eq!(d.scroll, 20);
    }

    #[test]
    fn tabs_cycle_both_ways() {
        assert_eq!(Tab::Overview.cycle(1), Tab::Latency);
        assert_eq!(Tab::Overview.cycle(-1), Tab::Requests);
        assert_eq!(Tab::Requests.cycle(1), Tab::Overview);
    }
}
