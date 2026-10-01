//! Ramp mode: climb through concurrency levels a step at a time, measure
//! each one on its own, and say where the target stops keeping up.

use std::time::{Duration, Instant};

use crate::cli::{Cli, RampArgs};
use crate::insights::Level;
use crate::load::MAX_CONCURRENCY;
use crate::metrics::Metrics;
use crate::response::ResponseStats;
use crate::ui::format;

/// More steps than this is a typo, not a plan
const MAX_STEPS: usize = 500;
/// Errors below this share are noise; from here a step isn't clean
const ERRORS_START: f64 = 1.0;
/// From this share of errors the target is failing
const ERRORS_BREAK: f64 = 5.0;
/// Throughput that grows by less than this share of the added load has
/// stopped scaling
const FLAT_GAIN: f64 = 0.5;

/// What a stop condition looks at
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Measure {
    /// A latency percentile, e.g. 99.0
    Latency(f64),
    /// The share of requests that failed, in percent
    Errors,
}

/// `p99 > 500ms`, `errors > 1%`: ends the ramp once a step crosses it
#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub measure: Measure,
    /// Microseconds for a latency, percent for errors
    pub limit: f64,
    /// As it was written
    pub text: String,
}

impl Condition {
    pub fn parse(text: &str) -> Result<Condition, String> {
        let help = "expected e.g. 'p99 > 500ms' or 'errors > 1%'";
        let (name, limit) = text
            .split_once('>')
            .ok_or_else(|| format!("stop condition {text:?}: {help}"))?;
        let name = name.trim().to_lowercase();
        let limit = limit.trim_start_matches('=').trim().to_lowercase();
        let number = |unit: &str| -> Option<f64> {
            limit
                .strip_suffix(unit)?
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| *n >= 0.0)
        };
        let condition = if matches!(name.as_str(), "errors" | "error" | "error rate") {
            let percent = number("%").or_else(|| number("")).ok_or_else(|| {
                format!("stop condition {text:?}: errors take a percentage, e.g. 1%")
            })?;
            (Measure::Errors, percent)
        } else if let Some(percentile) = name
            .strip_prefix('p')
            .and_then(|p| p.parse::<f64>().ok())
            .filter(|p| (0.0..=100.0).contains(p))
        {
            // Longest unit first: "ms" also ends in "s"
            let micros = number("ms")
                .map(|n| n * 1e3)
                .or_else(|| number("us").or_else(|| number("µs")))
                .or_else(|| number("s").map(|n| n * 1e6))
                .ok_or_else(|| {
                    format!("stop condition {text:?}: latency takes a unit, e.g. 500ms or 2s")
                })?;
            (Measure::Latency(percentile), micros)
        } else {
            return Err(format!(
                "stop condition {text:?}: unknown measure {name:?}; {help}"
            ));
        };
        Ok(Condition {
            measure: condition.0,
            limit: condition.1,
            text: text.split_whitespace().collect::<Vec<_>>().join(" "),
        })
    }

    pub fn crossed(&self, metrics: &Metrics) -> bool {
        if metrics.total == 0 {
            return false;
        }
        match self.measure {
            Measure::Latency(percentile) => {
                metrics.percentile(percentile).as_micros() as f64 > self.limit
            }
            Measure::Errors => metrics.error_rate() > self.limit,
        }
    }
}

/// The steps to climb and when to give up
#[derive(Debug, Clone, PartialEq)]
pub struct RampPlan {
    /// Concurrency of each step, in order
    pub levels: Vec<u32>,
    /// How long each step is held
    pub every: Duration,
    pub until: Vec<Condition>,
}

impl RampPlan {
    pub fn from_args(args: &RampArgs) -> Result<RampPlan, String> {
        let every = Cli::parse_duration(&args.every)
            .map_err(|_| format!("--every {:?}: expected e.g. 10s, 1m", args.every))?;
        if every == 0 {
            return Err("--every must be longer than zero".into());
        }
        Ok(RampPlan {
            levels: levels(args.from, args.to, args.step)?,
            every: Duration::from_millis(every),
            until: args
                .until
                .iter()
                .filter(|text| !text.trim().is_empty())
                .map(|text| Condition::parse(text))
                .collect::<Result<_, _>>()?,
        })
    }

    pub fn total(&self) -> Duration {
        self.every * self.levels.len() as u32
    }
}

/// `from`, then up by `step`, ending on `to` exactly
pub fn levels(from: u32, to: u32, step: u32) -> Result<Vec<u32>, String> {
    if from == 0 || step == 0 {
        return Err("--from and --step must be above zero".into());
    }
    if to < from {
        return Err(format!("--to {to} is below --from {from}"));
    }
    if to as usize > MAX_CONCURRENCY {
        return Err(format!("--to can't be above {MAX_CONCURRENCY}"));
    }
    if ((to - from) / step) as usize >= MAX_STEPS {
        return Err(format!(
            "that's more than {MAX_STEPS} steps; use a larger --step"
        ));
    }
    let mut levels: Vec<u32> = (from..=to).step_by(step as usize).collect();
    if levels.last() != Some(&to) {
        levels.push(to);
    }
    Ok(levels)
}

/// One level, measured on its own
#[derive(Debug, Clone, Default)]
pub struct Step {
    pub level: u32,
    pub metrics: Metrics,
    /// How long it ran, pauses left out
    pub held: Duration,
    /// What stands out next to the step before it
    pub note: Option<(Level, String)>,
    /// The stop condition it crossed
    pub crossed: Option<String>,
    /// By this step, throughput had stopped following the load
    pub saturated: bool,
}

impl Step {
    /// Requests answered well per second. Failures don't count: a target
    /// that sheds load answers fast, and that isn't throughput.
    pub fn rps(&self) -> f64 {
        let seconds = self.held.as_secs_f64();
        if seconds <= 0.0 {
            return 0.0;
        }
        self.metrics.success as f64 / seconds
    }

    pub fn p99(&self) -> Duration {
        self.metrics.percentile(99.0)
    }

    /// No errors to speak of, and no stop condition crossed
    fn clean(&self) -> bool {
        self.crossed.is_none() && self.metrics.error_rate() < ERRORS_START
    }

    fn broken(&self) -> bool {
        self.crossed.is_some() || self.metrics.error_rate() >= ERRORS_BREAK
    }

    /// What most of its failed requests were: "HTTP 503", "no response"
    fn failure(&self) -> String {
        let m = &self.metrics;
        let status = m
            .status_codes
            .iter()
            .filter(|(code, _)| !(200..300).contains(*code))
            .max_by_key(|(_, count)| **count);
        match status {
            Some((code, count)) if *count >= m.timeouts + m.errors => format!("HTTP {code}"),
            _ if m.timeouts >= m.errors => "timeouts".into(),
            _ => "no response".into(),
        }
    }
}

/// How much of the added load came back as throughput, between two steps:
/// 1 is linear scaling, 0 is none
fn gain(before: &Step, after: &Step) -> Option<f64> {
    let load = after.level as f64 / before.level as f64 - 1.0;
    let before_rps = before.rps();
    if load <= 0.0 || before_rps <= 0.0 {
        return None;
    }
    Some((after.rps() / before_rps - 1.0) / load)
}

fn percent_change(before: f64, after: f64) -> String {
    if before <= 0.0 {
        return "n/a".into();
    }
    format!("{:+.0}%", (after / before - 1.0) * 100.0)
}

/// The clean step with the most throughput
fn best(steps: &[Step]) -> Option<&Step> {
    steps
        .iter()
        .filter(|s| s.clean())
        .max_by(|a, b| a.rps().total_cmp(&b.rps()))
}

/// Throughput has stopped following the load by `step`: it had already,
/// or this step added load and got less than half of it back
fn saturated(previous: &[Step], step: &Step) -> bool {
    let Some(before) = previous.last() else {
        return false;
    };
    before.saturated || gain(before, step).is_some_and(|g| g < FLAT_GAIN)
}

/// What stands out about a step, next to the ones before it
fn note(previous: &[Step], step: &Step) -> Option<(Level, String)> {
    if let Some(condition) = &step.crossed {
        return Some((Level::Failing, format!("crossed {condition}")));
    }
    let before = previous.last();
    let errors = step.metrics.error_rate();
    if errors >= ERRORS_BREAK {
        return Some((Level::Failing, format!("failing: {}", step.failure())));
    }
    if errors >= ERRORS_START && before.is_none_or(|b| b.metrics.error_rate() < ERRORS_START) {
        return Some((
            Level::Degraded,
            format!("errors started: {}", step.failure()),
        ));
    }
    let before = before?;
    // Past the point where it stopped scaling, what matters is what the
    // extra load bought since then, not how this step compares with its
    // neighbour
    if before.saturated {
        let held = previous
            .iter()
            .rev()
            .find(|s| !s.saturated)
            .or_else(|| best(previous))?;
        let slower = step.p99().as_micros() as f64 / held.p99().as_micros().max(1) as f64;
        let latency = if slower >= 2.0 {
            format!("×{slower:.1}")
        } else {
            percent_change(1.0, slower)
        };
        return Some((
            Level::Degraded,
            format!(
                "{} over {} · p99 {latency}",
                percent_change(held.rps(), step.rps()),
                held.level
            ),
        ));
    }
    if step.rps() < before.rps() * 0.95 {
        return Some((Level::Degraded, "throughput fell".into()));
    }
    if gain(before, step)? < FLAT_GAIN {
        return Some((Level::Degraded, "stops scaling here".into()));
    }
    let (was, now) = (before.p99().as_micros(), step.p99().as_micros());
    if was > 0 && now >= was * 2 {
        return Some((Level::Degraded, "p99 doubled".into()));
    }
    None
}

/// One second of the ramp
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Second {
    /// Requests answered well
    pub ok: u32,
    pub failed: u32,
    /// Which step it belongs to
    pub step: u16,
}

/// Why the ramp ended
#[derive(Debug, Clone, PartialEq)]
pub enum End {
    /// Every step ran
    Completed,
    /// A stop condition was crossed
    Stopped(String),
    /// Ended by hand before the last step
    Interrupted,
}

/// What to do after a tick
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tick {
    /// Keep going as is
    Hold,
    /// The next step began: change to this concurrency
    Level(u32),
    /// The ramp just ended: stop sending
    Done,
}

/// The step being measured
#[derive(Debug, Clone)]
struct Running {
    metrics: Metrics,
    started: Instant,
    /// Time spent paused since it started
    paused: Duration,
}

/// A ramp as it runs: feed it results and the time, and it says when to
/// change level
#[derive(Debug, Clone)]
pub struct Ramp {
    pub plan: RampPlan,
    /// Finished steps
    pub steps: Vec<Step>,
    running: Running,
    paused_since: Option<Instant>,
    pub end: Option<End>,
    /// Every request of the run
    pub total: Metrics,
    /// What each second of the run answered, pauses left out
    pub seconds: Vec<Second>,
}

impl Ramp {
    pub fn new(plan: RampPlan, now: Instant) -> Ramp {
        Ramp {
            plan,
            steps: Vec::new(),
            running: Running {
                metrics: Metrics::default(),
                started: now,
                paused: Duration::ZERO,
            },
            paused_since: None,
            end: None,
            total: Metrics::default(),
            seconds: Vec::new(),
        }
    }

    /// Every step ran, so what wasn't reached can be said too
    pub fn completed(&self) -> bool {
        self.end == Some(End::Completed)
    }

    /// Concurrency of the step in progress (the last one, once ended)
    pub fn level(&self) -> u32 {
        let index = self.steps.len().min(self.plan.levels.len() - 1);
        self.plan.levels[index]
    }

    pub fn record(&mut self, stat: &ResponseStats, now: Instant) {
        if self.end.is_some() {
            return;
        }
        self.running.metrics.record(stat);
        self.total.record(stat);
        let second = self.elapsed(now).as_secs() as usize;
        if second >= self.seconds.len() {
            self.seconds.resize(second + 1, Second::default());
        }
        let slot = &mut self.seconds[second];
        slot.step = self.steps.len() as u16;
        if stat.status_code.is_some_and(|code| code.is_success()) {
            slot.ok += 1;
        } else {
            slot.failed += 1;
        }
    }

    pub fn is_paused(&self) -> bool {
        self.paused_since.is_some()
    }

    pub fn set_paused(&mut self, paused: bool, now: Instant) {
        match (paused, self.paused_since) {
            (true, None) => self.paused_since = Some(now),
            (false, Some(since)) => {
                self.running.paused += now.saturating_duration_since(since);
                self.paused_since = None;
            }
            _ => {}
        }
    }

    /// How long the step in progress has been held, pauses left out
    pub fn held(&self, now: Instant) -> Duration {
        let until = self.paused_since.unwrap_or(now);
        until
            .saturating_duration_since(self.running.started)
            .saturating_sub(self.running.paused)
    }

    /// Time the whole ramp has run, pauses left out
    pub fn elapsed(&self, now: Instant) -> Duration {
        let done: Duration = self.steps.iter().map(|s| s.held).sum();
        if self.end.is_some() {
            done
        } else {
            done + self.held(now)
        }
    }

    /// The step in progress, as measured so far
    pub fn current(&self, now: Instant) -> Option<Step> {
        self.end.is_none().then(|| Step {
            level: self.level(),
            metrics: self.running.metrics.clone(),
            held: self.held(now),
            ..Default::default()
        })
    }

    /// Close the step in progress and decide what comes next
    fn finish_step(&mut self, now: Instant) -> Tick {
        let mut step = Step {
            level: self.level(),
            metrics: std::mem::take(&mut self.running.metrics),
            held: self.held(now),
            ..Default::default()
        };
        step.crossed = self
            .plan
            .until
            .iter()
            .find(|condition| condition.crossed(&step.metrics))
            .map(|condition| condition.text.clone());
        step.saturated = saturated(&self.steps, &step);
        step.note = note(&self.steps, &step);
        let crossed = step.crossed.clone();
        self.steps.push(step);
        self.running = Running {
            metrics: Metrics::default(),
            started: now,
            paused: Duration::ZERO,
        };
        if self.paused_since.is_some() {
            self.paused_since = Some(now);
        }

        if let Some(condition) = crossed {
            self.end = Some(End::Stopped(condition));
            Tick::Done
        } else if self.steps.len() == self.plan.levels.len() {
            self.end = Some(End::Completed);
            Tick::Done
        } else {
            Tick::Level(self.level())
        }
    }

    /// Call often: moves to the next step once this one has been held
    pub fn tick(&mut self, now: Instant) -> Tick {
        if self.end.is_some() || self.is_paused() || self.held(now) < self.plan.every {
            return Tick::Hold;
        }
        self.finish_step(now)
    }

    /// End the step in progress now and go on to the next
    pub fn skip(&mut self, now: Instant) -> Tick {
        // A step with nothing measured would read as throughput falling
        // to zero; wait for its first answer instead
        if self.end.is_some() || self.running.metrics.total == 0 {
            return Tick::Hold;
        }
        self.finish_step(now)
    }

    /// End the ramp now, keeping what the step in progress measured
    pub fn stop(&mut self, now: Instant) -> Tick {
        if self.end.is_some() {
            return Tick::Hold;
        }
        if self.running.metrics.total > 0 {
            self.finish_step(now);
        }
        if !matches!(self.end, Some(End::Stopped(_))) {
            self.end = Some(if self.steps.len() == self.plan.levels.len() {
                End::Completed
            } else {
                End::Interrupted
            });
        }
        Tick::Done
    }
}

/// A conclusion about the whole ramp
#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub level: Level,
    /// "Holds 50 concurrent"
    pub title: String,
    /// "3.9k req/s · p99 48ms · 0% errors"
    pub detail: String,
}

/// Where it holds, where it stops scaling, where it breaks
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Findings {
    pub list: Vec<Finding>,
    /// The highest concurrency that ran clean and still scaled
    pub holds: Option<u32>,
}

/// `ended`: the ramp is over, so what wasn't reached can be said too
pub fn findings(steps: &[Step], ended: bool) -> Findings {
    let mut out = Findings::default();
    if steps.is_empty() {
        return out;
    }
    // The first step that isn't clean, the first where throughput stops
    // following the load, and the first that fails outright
    let dirty = steps.iter().position(|s| !s.clean());
    let knee = (1..steps.len()).find(|&i| {
        steps[i].clean() && gain(&steps[i - 1], &steps[i]).is_some_and(|g| g < FLAT_GAIN)
    });
    let broken = steps.iter().position(Step::broken);

    let limit = [dirty, knee].into_iter().flatten().min();
    let holds = match limit {
        Some(0) => None,
        Some(i) => Some(i - 1),
        None => Some(steps.len() - 1),
    };
    let summary = |s: &Step| {
        format!(
            "{} req/s · p99 {} · {:.1}% errors",
            format::compact(s.rps()),
            format::latency(s.p99()),
            s.metrics.error_rate()
        )
    };
    match holds {
        Some(i) => {
            out.holds = Some(steps[i].level);
            out.list.push(Finding {
                level: Level::Healthy,
                title: format!("Holds {} concurrent", steps[i].level),
                detail: summary(&steps[i]),
            });
        }
        None => out.list.push(Finding {
            level: Level::Failing,
            title: format!("Struggles already at {} concurrent", steps[0].level),
            detail: summary(&steps[0]),
        }),
    }
    if let Some(i) = knee.filter(|&i| dirty.is_none_or(|d| i <= d)) {
        let (before, after) = (&steps[i - 1], &steps[i]);
        out.list.push(Finding {
            level: Level::Degraded,
            title: format!("Stops scaling between {} and {}", before.level, after.level),
            detail: format!(
                "{} load gave {} throughput, p99 {}",
                percent_change(before.level as f64, after.level as f64),
                percent_change(before.rps(), after.rps()),
                percent_change(
                    before.p99().as_micros() as f64,
                    after.p99().as_micros() as f64
                ),
            ),
        });
    }
    // What the extra load cost in latency, from the level that held to
    // the last one measured
    if let (Some(i), Some(last)) = (holds, steps.last()) {
        let (was, now) = (steps[i].p99(), last.p99());
        let slower = now.as_micros() as f64 / was.as_micros().max(1) as f64;
        if i + 1 < steps.len() && slower >= 3.0 {
            out.list.push(Finding {
                level: Level::Degraded,
                title: format!("p99 ×{slower:.1} by {} concurrent", last.level),
                detail: format!(
                    "{} at {} → {} at {}",
                    format::latency(was),
                    steps[i].level,
                    format::latency(now),
                    last.level
                ),
            });
        }
    }
    // Failures too few to call a step failing are still worth knowing of
    if dirty.is_none() {
        let failing = |s: &&Step| s.metrics.total > s.metrics.success;
        if let Some(first) = steps.iter().find(failing) {
            let worst = steps
                .iter()
                .max_by(|a, b| a.metrics.error_rate().total_cmp(&b.metrics.error_rate()))
                .unwrap_or(first);
            out.list.push(Finding {
                level: Level::Degraded,
                title: format!("First failures at {} concurrent", first.level),
                detail: format!(
                    "{} · at most {:.1}% of requests, at {}",
                    first.failure(),
                    worst.metrics.error_rate(),
                    worst.level
                ),
            });
        }
    }
    match (broken, dirty) {
        (Some(i), _) => {
            let step = &steps[i];
            out.list.push(Finding {
                level: Level::Failing,
                title: format!("Breaks at {} concurrent", step.level),
                detail: match &step.crossed {
                    Some(condition) => format!("crossed {condition} · {}", summary(step)),
                    None => format!(
                        "{:.1}% errors (mostly {}) · p99 {}",
                        step.metrics.error_rate(),
                        step.failure(),
                        format::latency(step.p99())
                    ),
                },
            });
        }
        (None, Some(i)) => {
            let step = &steps[i];
            out.list.push(Finding {
                level: Level::Degraded,
                title: format!("Errors from {} concurrent", step.level),
                detail: format!(
                    "{:.1}% errors (mostly {})",
                    step.metrics.error_rate(),
                    step.failure()
                ),
            });
        }
        (None, None) if knee.is_none() && ended => out.list.push(Finding {
            level: Level::Healthy,
            title: format!(
                "No limit found up to {} concurrent",
                steps[steps.len() - 1].level
            ),
            detail: "throughput was still growing: try a higher --to".into(),
        }),
        (None, None) => {}
    }
    out
}

/// The ramp as text, for the shell once the screen is gone
pub fn report(cli: &Cli, ramp: &Ramp) -> String {
    let mut out = format!("pepe ramp · {} {}\n\n", cli.method, cli.url);
    out += "  conc  requests      ok/s       p50       p90       p99       max   errors\n";
    for step in &ramp.steps {
        out += &format!(
            "  {:>4}  {:>8}  {:>8}  {:>8}  {:>8}  {:>8}  {:>8}  {:>6.1}%{}\n",
            step.level,
            format::compact(step.metrics.total as f64),
            format::compact(step.rps()),
            format::latency(step.metrics.percentile(50.0)),
            format::latency(step.metrics.percentile(90.0)),
            format::latency(step.p99()),
            format::latency(step.metrics.max()),
            step.metrics.error_rate(),
            step.note
                .as_ref()
                .map_or(String::new(), |(_, text)| format!("   {text}")),
        );
    }
    out += &format!(
        "\n  {} requests in {} · {:.1}% failed\n\n",
        format::count(ramp.total.total),
        format::span(ramp.steps.iter().map(|s| s.held).sum()),
        ramp.total.error_rate()
    );
    let findings = findings(&ramp.steps, ramp.completed());
    for finding in &findings.list {
        out += &format!(
            "  {} {} · {}\n",
            finding.level.symbol(),
            finding.title,
            finding.detail
        );
    }
    if let Some(command) = sustained(cli, &findings) {
        out += &format!("\nRun at the level that held:\n  {command}\n");
    }
    out
}

/// The command for a steady run at the level that held
pub fn sustained(cli: &Cli, findings: &Findings) -> Option<String> {
    let mut steady = cli.clone();
    steady.command = None;
    steady.concurrency = findings.holds?;
    steady.duration = Some("1m".into());
    Some(steady.command_line())
}

/// The ramp as JSON, for `--json`
pub fn json(ramp: &Ramp) -> serde_json::Value {
    let ms = |d: Duration| (d.as_secs_f64() * 1_000_000.0).round() / 1000.0;
    let findings = findings(&ramp.steps, ramp.completed());
    serde_json::json!({
        "ended": match &ramp.end {
            Some(End::Stopped(condition)) => format!("stopped: {condition}"),
            Some(End::Interrupted) => "interrupted".to_string(),
            _ => "completed".to_string(),
        },
        "holds_concurrency": findings.holds,
        "requests": ramp.total.total,
        "failed_requests": ramp.total.total - ramp.total.success,
        "seconds": ramp.steps.iter().map(|s| s.held).sum::<Duration>().as_secs_f64(),
        "findings": findings.list.iter().map(|f| serde_json::json!({
            "level": f.level.headline().to_lowercase(),
            "title": f.title,
            "detail": f.detail,
        })).collect::<Vec<_>>(),
        "steps": ramp.steps.iter().map(|s| serde_json::json!({
            "concurrency": s.level,
            "seconds": (s.held.as_secs_f64() * 1000.0).round() / 1000.0,
            "requests": s.metrics.total,
            "failed_requests": s.metrics.total - s.metrics.success,
            "requests_per_second": s.metrics.rps(s.held),
            "successful_per_second": s.rps(),
            "error_rate_percent": s.metrics.error_rate(),
            "data_transfer_bytes": s.metrics.bytes,
            "min_ms": ms(s.metrics.min()),
            "avg_ms": ms(s.metrics.mean()),
            "median_ms": ms(s.metrics.percentile(50.0)),
            "p90_ms": ms(s.metrics.percentile(90.0)),
            "p95_ms": ms(s.metrics.percentile(95.0)),
            "p99_ms": ms(s.p99()),
            "max_ms": ms(s.metrics.max()),
            "status_codes": s.metrics.status_codes.iter()
                .map(|(code, count)| (code.to_string(), *count))
                .collect::<std::collections::BTreeMap<_, _>>(),
            "timeouts": s.metrics.timeouts,
            "connection_errors": s.metrics.errors,
            "note": s.note.as_ref().map(|(_, text)| text.clone()),
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    fn stat(ms: u64, code: u16) -> ResponseStats {
        ResponseStats {
            duration: Duration::from_millis(ms),
            status_code: Some(StatusCode::from_u16(code).unwrap()),
            ..Default::default()
        }
    }

    /// A step that ran for a second: `requests` of them, `failed` with a 503
    fn step(level: u32, requests: u64, ms: u64, failed: u64) -> Step {
        let mut metrics = Metrics::default();
        for i in 0..requests {
            metrics.record(&stat(ms, if i < failed { 503 } else { 200 }));
        }
        Step {
            level,
            metrics,
            held: Duration::from_secs(1),
            ..Default::default()
        }
    }

    #[test]
    fn levels_climb_and_end_on_the_target() {
        assert_eq!(levels(10, 50, 10).unwrap(), [10, 20, 30, 40, 50]);
        assert_eq!(levels(10, 35, 10).unwrap(), [10, 20, 30, 35]);
        assert_eq!(levels(5, 5, 1).unwrap(), [5]);
        assert!(levels(0, 5, 1).is_err());
        assert!(levels(5, 50, 0).is_err());
        assert!(levels(50, 5, 5).unwrap_err().contains("below"));
        assert!(levels(1, 5000, 1).unwrap_err().contains("steps"));
    }

    #[test]
    fn stop_conditions_are_parsed_and_checked() {
        let p99 = Condition::parse("p99 > 500ms").unwrap();
        assert_eq!(
            (p99.measure, p99.limit),
            (Measure::Latency(99.0), 500_000.0)
        );
        let p50 = Condition::parse("P50>=2s").unwrap();
        assert_eq!(
            (p50.measure, p50.limit),
            (Measure::Latency(50.0), 2_000_000.0)
        );
        assert_eq!(p50.text, "P50>=2s");
        let fine = Condition::parse("p99.9 > 250us").unwrap();
        assert_eq!(fine.limit, 250.0);
        let errors = Condition::parse("errors  >  1%").unwrap();
        assert_eq!((errors.measure, errors.limit), (Measure::Errors, 1.0));
        assert_eq!(errors.text, "errors > 1%");
        for bad in [
            "p99",
            "p99 > fast",
            "p99 > 500",
            "speed > 1ms",
            "errors > lots",
            "p300 > 1s",
        ] {
            assert!(Condition::parse(bad).is_err(), "{bad}");
        }

        let slow = step(10, 100, 800, 2);
        assert!(p99.crossed(&slow.metrics));
        assert!(errors.crossed(&slow.metrics), "2% is above 1%");
        let quick = step(10, 100, 20, 0);
        assert!(!p99.crossed(&quick.metrics) && !errors.crossed(&quick.metrics));
        assert!(!p99.crossed(&Metrics::default()), "nothing measured yet");
    }

    fn plan(levels: &[u32], until: &[&str]) -> RampPlan {
        RampPlan {
            levels: levels.to_vec(),
            every: Duration::from_secs(10),
            until: until.iter().map(|c| Condition::parse(c).unwrap()).collect(),
        }
    }

    #[test]
    fn a_ramp_steps_through_its_levels() {
        let start = Instant::now();
        let at = |secs: u64| start + Duration::from_secs(secs);
        let mut ramp = Ramp::new(plan(&[10, 20, 30], &[]), start);
        assert_eq!(ramp.level(), 10);
        ramp.record(&stat(5, 200), start);
        assert_eq!(ramp.tick(at(9)), Tick::Hold);
        assert_eq!(ramp.tick(at(10)), Tick::Level(20));
        assert_eq!((ramp.steps.len(), ramp.steps[0].metrics.total), (1, 1));
        assert_eq!(ramp.steps[0].held, Duration::from_secs(10));

        // A pause doesn't count towards the step
        ramp.set_paused(true, at(12));
        assert_eq!(ramp.tick(at(60)), Tick::Hold);
        ramp.set_paused(false, at(60));
        assert_eq!(ramp.held(at(61)), Duration::from_secs(3));
        assert_eq!(ramp.tick(at(67)), Tick::Hold);
        assert_eq!(ramp.tick(at(68)), Tick::Level(30));
        assert_eq!(ramp.elapsed(at(70)), Duration::from_secs(22));

        // n: on to the next step now; the last one ends the ramp
        ramp.record(&stat(5, 200), start);
        assert_eq!(ramp.skip(at(70)), Tick::Done);
        assert_eq!(ramp.end, Some(End::Completed));
        assert_eq!(ramp.steps[2].held, Duration::from_secs(2));
        assert_eq!(ramp.tick(at(500)), Tick::Hold);
        assert_eq!(ramp.total.total, 2);

        // n with nothing measured yet waits, instead of recording an empty step
        let mut ramp = Ramp::new(plan(&[10, 20], &[]), start);
        assert_eq!(ramp.skip(at(1)), Tick::Hold);
        assert!(ramp.steps.is_empty());
    }

    #[test]
    fn a_crossed_condition_ends_the_ramp() {
        let start = Instant::now();
        let mut ramp = Ramp::new(plan(&[10, 20, 30], &["p99 > 100ms"]), start);
        ramp.record(&stat(20, 200), start);
        assert_eq!(ramp.tick(start + Duration::from_secs(10)), Tick::Level(20));
        ramp.record(&stat(300, 200), start);
        assert_eq!(ramp.tick(start + Duration::from_secs(20)), Tick::Done);
        assert_eq!(ramp.end, Some(End::Stopped("p99 > 100ms".into())));
        assert_eq!(ramp.steps[1].crossed.as_deref(), Some("p99 > 100ms"));
        assert!(
            matches!(&ramp.steps[1].note, Some((Level::Failing, text)) if text.contains("p99 > 100ms"))
        );
    }

    #[test]
    fn stopping_by_hand_keeps_the_step_in_progress() {
        let start = Instant::now();
        let mut ramp = Ramp::new(plan(&[10, 20], &[]), start);
        ramp.record(&stat(5, 200), start);
        assert_eq!(ramp.stop(start + Duration::from_secs(3)), Tick::Done);
        assert_eq!(ramp.end, Some(End::Interrupted));
        assert_eq!(ramp.steps.len(), 1);
        // Nothing measured: nothing kept
        let mut ramp = Ramp::new(plan(&[10, 20], &[]), start);
        ramp.stop(start);
        assert!(ramp.steps.is_empty());
    }

    #[test]
    fn notes_say_what_changed() {
        let clean = step(10, 1000, 10, 0);
        assert_eq!(note(&[], &clean), None);
        // Twice the load, twice the throughput: nothing to say
        assert_eq!(
            note(std::slice::from_ref(&clean), &step(20, 2000, 10, 0)),
            None
        );
        let text =
            |before: &Step, after: Step| note(std::slice::from_ref(before), &after).unwrap().1;
        assert_eq!(text(&clean, step(20, 1200, 10, 0)), "stops scaling here");
        assert_eq!(text(&clean, step(20, 900, 10, 0)), "throughput fell");
        assert_eq!(text(&clean, step(20, 2000, 25, 0)), "p99 doubled");
        assert_eq!(
            text(&clean, step(20, 2000, 10, 40)),
            "errors started: HTTP 503"
        );
        assert_eq!(text(&clean, step(20, 2000, 10, 400)), "failing: HTTP 503");

        // Once it has stopped scaling, steps are held against the best one,
        // so a small rise over the step before isn't called scaling
        let mut flat = step(20, 1100, 12, 0);
        flat.saturated = saturated(std::slice::from_ref(&clean), &flat);
        assert!(flat.saturated);
        let previous = [clean.clone(), flat];
        let later = step(30, 1150, 45, 0);
        assert!(saturated(&previous, &later));
        assert_eq!(
            note(&previous, &later).unwrap().1,
            "+15% over 10 · p99 ×4.5"
        );
        assert_eq!(
            note(&previous, &step(30, 1500, 12, 0)).unwrap().1,
            "+50% over 10 · p99 +20%"
        );
    }

    #[test]
    fn findings_name_where_it_holds_bends_and_breaks() {
        let steps = [
            step(10, 1000, 10, 0),
            step(20, 2000, 11, 0),
            step(30, 2900, 12, 0),
            step(40, 3000, 30, 0),
            step(50, 3000, 80, 300),
        ];
        let found = findings(&steps, true);
        assert_eq!(found.holds, Some(30));
        let titles: Vec<&str> = found.list.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Holds 30 concurrent",
                "Stops scaling between 30 and 40",
                "p99 ×6.7 by 50 concurrent",
                "Breaks at 50 concurrent"
            ]
        );
        assert!(found.list[1]
            .detail
            .contains("+33% load gave +3% throughput"));
        assert_eq!(found.list[2].detail, "12.00ms at 30 → 80.00ms at 50");
        assert!(found.list[3]
            .detail
            .contains("10.0% errors (mostly HTTP 503)"));

        // A few failures, short of a failing step, are still reported
        let few = [
            step(10, 1000, 10, 0),
            step(20, 2000, 10, 4),
            step(30, 3000, 10, 15),
        ];
        let found = findings(&few, true);
        let titles: Vec<&str> = found.list.iter().map(|f| f.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Holds 30 concurrent",
                "First failures at 20 concurrent",
                "No limit found up to 30 concurrent"
            ]
        );
        assert_eq!(
            found.list[1].detail,
            "HTTP 503 · at most 0.5% of requests, at 30"
        );

        // Clean all the way: no limit found
        let found = findings(&steps[..3], true);
        assert_eq!(found.holds, Some(30));
        assert!(found.list[1].title.contains("No limit found up to 30"));
        assert_eq!(
            findings(&steps[..3], false).list.len(),
            1,
            "not said mid-run"
        );

        // Failing from the first step: nothing holds
        let found = findings(&[step(10, 1000, 10, 500)], true);
        assert_eq!(found.holds, None);
        assert!(found.list[0].title.contains("Struggles already at 10"));
        assert!(findings(&[], true).list.is_empty());
    }
}
