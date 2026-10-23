//! What nginx's logs say is wrong now, against what they usually say:
//! findings, most serious first, and the `--fail-if` limits a log can be
//! held to. Everything here reads the counts `logs::Stats` already keeps.

use std::time::Duration;

use crate::diagnose::Level;
use crate::logs::{percent, rate, time_of_day, typical, versus, Clock, Coarse, Grain, Row, Stats};
use crate::ui::format;
use crate::webhook::Check;

/// Requests the last window needs before a share of them says anything
const ENOUGH: u64 = 20;
/// A share this many times the usual one is out of the usual
const TIMES_USUAL: f64 = 3.0;
/// Paths looked at for what they do now: the busiest, the most failing
/// and the most refused, this many of each
const PATHS_LOOKED_AT: usize = 30;
/// Findings about paths, of each kind, at most
const PATH_FINDINGS: usize = 3;
/// Failed requests a path needs in the window before its share is said:
/// one failure in forty is no finding
const PATH_FAILED: u64 = 5;

/// What a finding is about, for scripts and for which view shows it
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The share answering 5xx, over every path
    Errors,
    /// One path answering 5xx
    PathErrors,
    /// One path answering 4xx
    PathRefused,
    /// The error log, at `error` and above
    ErrorLog,
    TrafficDown,
    TrafficUp,
    Latency,
    /// One client sends much of the traffic
    Client,
    /// Lines that couldn't be read
    Unread,
    /// Nothing out of the usual
    Steady,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Errors => "errors",
            Kind::PathErrors => "path_errors",
            Kind::PathRefused => "path_refused",
            Kind::ErrorLog => "error_log",
            Kind::TrafficDown => "traffic_down",
            Kind::TrafficUp => "traffic_up",
            Kind::Latency => "latency",
            Kind::Client => "client",
            Kind::Unread => "unread",
            Kind::Steady => "steady",
        }
    }

    /// What a webhook's anomaly says moved
    pub fn moved(self) -> &'static str {
        match self {
            Kind::TrafficDown | Kind::TrafficUp => "throughput",
            Kind::Latency => "latency",
            _ => "errors",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Finding {
    pub level: Level,
    pub kind: Kind,
    pub text: String,
    pub path: Option<String>,
    /// When it started, on the log's clock (local seconds)
    pub since: Option<i64>,
    /// The number it is about now, and usually: a share, a rate, ms
    pub now: Option<f64>,
    pub usual: Option<f64>,
    /// What `usual` is: "a usual minute", "this hour a day ago"
    pub against: Option<String>,
}

impl Finding {
    fn new(level: Level, kind: Kind, text: String) -> Finding {
        Finding {
            level,
            kind,
            text,
            path: None,
            since: None,
            now: None,
            usual: None,
            against: None,
        }
    }

    /// The same finding, whatever its numbers: what tells a finding that
    /// turns up from one that was already there
    pub fn key(&self) -> String {
        format!(
            "{} {}",
            self.kind.name(),
            self.path.as_deref().unwrap_or("")
        )
    }

    pub fn json(&self, offset: i32) -> serde_json::Value {
        let mut out = serde_json::json!({
            "level": self.level.name(),
            "kind": self.kind.name(),
            "text": self.text,
        });
        let round = |v: f64| (v * 10_000.0).round() / 10_000.0;
        if let Some(path) = &self.path {
            out["path"] = path.clone().into();
        }
        if let Some(since) = self.since {
            out["since"] = crate::logs::Stamp {
                at: since - i64::from(offset),
                offset,
            }
            .iso()
            .into();
        }
        if let Some(now) = self.now {
            out["now"] = round(now).into();
        }
        if let Some(usual) = self.usual {
            out["usual"] = round(usual).into();
        }
        if let Some(against) = &self.against {
            out["against"] = against.clone().into();
        }
        out
    }
}

/// The whole in a word: failing when anything is bad, degraded when
/// anything is worth a warning, healthy otherwise
pub fn level(findings: &[Finding]) -> &'static str {
    match findings.iter().map(|f| f.level).max() {
        Some(Level::Bad) => "failing",
        Some(Level::Warn) => "degraded",
        _ => "healthy",
    }
}

/// The minute rows the last `window` seconds fall in, each with the part
/// of it that is in the window
fn in_window(rows: &[Row], clock: Clock, window: i64) -> Vec<(f64, &Row)> {
    let end = clock.now + i64::from(clock.offset) + 1;
    let start = end - window;
    rows.iter()
        .filter(|row| row.start + 60 > start && row.start < end)
        .map(|row| {
            let to = (row.start + 60).min(end);
            let part = (to - row.start.max(start)) as f64 / (to - row.start).max(1) as f64;
            (part, row)
        })
        .collect()
}

/// The whole minutes before the window: what is usual
fn before_window(rows: &[Row], clock: Clock, window: i64) -> Vec<Row> {
    let start = clock.now + i64::from(clock.offset) + 1 - window;
    rows.iter()
        .filter(|row| !row.partial && row.start + 60 <= start)
        .copied()
        .collect()
}

/// (5xx, 4xx, requests) of rows, each counted for `part` of it
fn shares<'a>(rows: impl Iterator<Item = (f64, &'a Row)>) -> (f64, f64, f64) {
    rows.fold((0.0, 0.0, 0.0), |(c5, c4, n), (part, row)| {
        (
            c5 + row.slot.c5xx as f64 * part,
            c4 + row.slot.c4xx as f64 * part,
            n + row.slot.requests as f64 * part,
        )
    })
}

/// Where a run of minutes, newest first, each passing `failing`, began
fn since(rows: &[Row], failing: impl Fn(&Row) -> bool) -> Option<i64> {
    let mut start = None;
    for row in rows.iter().rev() {
        if row.slot.requests == 0 || !failing(row) {
            break;
        }
        start = Some(row.start);
    }
    start
}

fn hh_mm(local: i64) -> String {
    let time = time_of_day(local);
    time.rsplit_once(':')
        .map_or(time.clone(), |(hm, _)| hm.to_string())
}

fn since_text(since: Option<i64>) -> String {
    since
        .map(|at| format!(" since {}", hh_mm(at)))
        .unwrap_or_default()
}

/// The request times of the minutes in the window
fn window_times(rows: &[(f64, &Row)]) -> Coarse {
    let mut times = Coarse::default();
    for (_, row) in rows {
        times.add(&row.slot.times);
    }
    times
}

/// What is worth saying about the logs now, most serious first
pub fn findings(stats: &Stats, clock: Clock, window: i64) -> Vec<Finding> {
    let mut out = Vec::new();
    if stats.requests == 0 {
        return out;
    }
    let now = stats.now(clock, window);
    let minutes = stats.rows(Grain::Minute, clock);
    let recent = in_window(&minutes, clock, window);
    let usual_rows = before_window(&minutes, clock, window);
    let (u5, _, un) = shares(usual_rows.iter().map(|r| (1.0, r)));
    let usual_5xx = (un > 0.0).then(|| u5 / un);

    // Every path, failing
    let s5 = now.share_5xx();
    if now.requests >= ENOUGH {
        let usual = usual_5xx.unwrap_or(0.0);
        let out_of_usual = s5 >= usual * TIMES_USUAL;
        // Failing this much is bad however long it has gone on; less is
        // worth a warning when it is out of the usual
        let level = match s5 {
            s if s >= 0.05 => Some(Level::Bad),
            s if s >= 0.01 && out_of_usual => Some(Level::Warn),
            _ => None,
        };
        if let Some(level) = level {
            let threshold = match level {
                Level::Bad => 0.05,
                _ => (usual * TIMES_USUAL).max(0.01),
            };
            let started = since(&minutes, |row| {
                row.slot.c5xx as f64 / row.slot.requests.max(1) as f64 >= threshold
            });
            let mut text = format!(
                "5xx {} of requests {}",
                percent(s5),
                since_text(started).trim_start()
            );
            if started.is_none() {
                text = format!("5xx {} of requests now", percent(s5));
            }
            if let Some(usual) = usual_5xx {
                text.push_str(&format!(", usually {}", percent(usual)));
            }
            // Where they come from: the path's and the whole's counted
            // alike, from the minutes the window is in
            let failing = stats.paths.top(1, |p| p.c5xx);
            let (all5, _, _) = shares(recent.iter().copied());
            if let Some((path, _)) = failing.first() {
                let theirs = stats.path_now(clock, window, path);
                let share = theirs.c5xx as f64 / all5.max(1.0);
                if share >= 0.5 {
                    text.push_str(&format!("; {} of them on {path}", percent(share.min(1.0))));
                }
            }
            let mut finding = Finding::new(level, Kind::Errors, text);
            finding.since = started;
            finding.now = Some(s5);
            finding.usual = usual_5xx;
            finding.against = Some("the minutes before".into());
            out.push(finding);
        }
    }

    // Path by path: the busiest, the most failing, the most refused
    let mut looked: Vec<&str> = Vec::new();
    let orders: [fn(&crate::logs::PathStat) -> u64; 3] = [|p| p.requests, |p| p.c5xx, |p| p.c4xx];
    for by in orders {
        for (path, _) in stats.paths.top(PATHS_LOOKED_AT, by) {
            if !looked.contains(&path) {
                looked.push(path);
            }
        }
    }
    let mut failing_paths = Vec::new();
    let mut refused_paths = Vec::new();
    let mut slower: Option<(f64, f64, f64, &str)> = None;
    let mut grew: Option<(f64, &str)> = None;
    for path in &looked {
        let theirs = stats.path_now(clock, window, path);
        let rows = stats.path_rows(Grain::Minute, clock, path);
        let before = before_window(&rows, clock, window);
        let (b5, b4, bn) = shares(before.iter().map(|r| (1.0, r)));
        // What it grew by, against its usual minute
        if let Some(usual) = typical(&before) {
            let more = theirs.rate - usual;
            if more > 0.0 && grew.is_none_or(|(most, _)| more > most) {
                grew = Some((more, path));
            }
        }
        // Its mean time now against before
        let windowed = in_window(&rows, clock, window);
        let (tn, ts) = windowed.iter().fold((0.0, 0.0), |(n, t), (part, row)| {
            (
                n + row.slot.timed as f64 * part,
                t + row.slot.time_us as f64 * part,
            )
        });
        let (bt, bts) = before.iter().fold((0.0, 0.0), |(n, t), row| {
            (n + row.slot.timed as f64, t + row.slot.time_us as f64)
        });
        if tn >= ENOUGH as f64 && bt >= ENOUGH as f64 {
            let (mean_now, mean_before) = (ts / tn, bts / bt);
            let ratio = mean_now / mean_before.max(1.0);
            if ratio > 1.5 && slower.is_none_or(|(most, ..)| ratio > most) {
                slower = Some((ratio, mean_now, mean_before, path));
            }
        }
        if theirs.requests < ENOUGH {
            continue;
        }
        let n = theirs.requests as f64;
        let (s5, s4) = (theirs.c5xx as f64 / n, theirs.c4xx as f64 / n);
        let (usual5, usual4) = match bn > 0.0 {
            true => (Some(b5 / bn), Some(b4 / bn)),
            false => (None, None),
        };
        let level5 = match s5 {
            _ if theirs.c5xx < PATH_FAILED => None,
            s if s >= 0.10 => Some(Level::Bad),
            s if s >= 0.02 && s >= usual5.unwrap_or(0.0) * TIMES_USUAL => Some(Level::Warn),
            _ => None,
        };
        if let Some(level) = level5 {
            failing_paths.push((level, s5, usual5, path.to_string(), rows.clone()));
        }
        let level4 = match s4 {
            _ if theirs.c4xx < PATH_FAILED => None,
            s if s >= 0.50 && s >= usual4.unwrap_or(0.0) * TIMES_USUAL => Some(Level::Bad),
            s if s >= 0.30 && s >= usual4.unwrap_or(0.0) * TIMES_USUAL => Some(Level::Warn),
            _ => None,
        };
        if let Some(level) = level4 {
            refused_paths.push((level, s4, usual4, path.to_string(), rows));
        }
    }
    let by_share = |a: &(Level, f64, Option<f64>, String, Vec<Row>),
                    b: &(Level, f64, Option<f64>, String, Vec<Row>)| {
        b.0.cmp(&a.0).then(b.1.total_cmp(&a.1))
    };
    failing_paths.sort_by(by_share);
    refused_paths.sort_by(by_share);
    for (kind, list, what, class) in [
        (Kind::PathErrors, failing_paths, "5xx", 5u64),
        (Kind::PathRefused, refused_paths, "4xx", 4u64),
    ] {
        for (level, share, usual, path, rows) in list.into_iter().take(PATH_FINDINGS) {
            let threshold = match (class, level) {
                (5, Level::Bad) => 0.10,
                (_, Level::Bad) => 0.50,
                (5, _) => (usual.unwrap_or(0.0) * TIMES_USUAL).max(0.02),
                _ => (usual.unwrap_or(0.0) * TIMES_USUAL).max(0.30),
            };
            let started = since(&rows, |row| {
                let n = match class {
                    5 => row.slot.c5xx,
                    _ => row.slot.c4xx,
                };
                n as f64 / row.slot.requests.max(1) as f64 >= threshold
            });
            let codes: Vec<String> = stats
                .paths
                .count(&path)
                .map(|p| p.statuses())
                .unwrap_or_default()
                .into_iter()
                .filter(|(code, _)| u64::from(*code / 100) == class)
                .take(3)
                .map(|(code, _)| code.to_string())
                .collect();
            let mut text = format!(
                "{path} answers {what} to {}{}",
                percent(share),
                match started {
                    Some(_) => since_text(started),
                    None => " now".into(),
                }
            );
            if !codes.is_empty() {
                text.push_str(&format!(" ({})", codes.join(", ")));
            }
            if let Some(usual) = usual {
                text.push_str(&format!(", usually {}", percent(usual)));
            }
            let mut finding = Finding::new(level, kind, text);
            finding.path = Some(path);
            finding.since = started;
            finding.now = Some(share);
            finding.usual = usual;
            finding.against = Some("the minutes before".into());
            out.push(finding);
        }
    }

    // The error log
    let faults: f64 = recent
        .iter()
        .map(|(part, row)| row.slot.faults as f64 * part)
        .sum();
    let usual_faults = match usual_rows.len() {
        0 => None,
        n => Some(usual_rows.iter().map(|r| r.slot.faults as f64).sum::<f64>() / n as f64),
    };
    let per_window = usual_faults.map(|f| f * window as f64 / 60.0);
    let level = match faults {
        f if f >= 10.0 && f >= per_window.unwrap_or(0.0) * 10.0 => Some(Level::Bad),
        f if f >= 3.0 && f >= per_window.unwrap_or(0.0) * TIMES_USUAL => Some(Level::Warn),
        _ => None,
    };
    if let Some(level) = level {
        let start = clock.now - window + 1;
        let cause = stats
            .top_causes(usize::MAX)
            .into_iter()
            .filter(|(_, c)| crate::logs::severity(&c.level) >= 2)
            .filter(|(_, c)| c.last.is_some_and(|last| last.at >= start))
            .max_by_key(|(_, c)| c.count);
        let what = cause
            .map(|(what, _)| what.split_once(' ').map_or(what, |(_, m)| m).to_string())
            .unwrap_or_else(|| "errors".into());
        let mut text = format!(
            "the error log has {} {} in the last {}",
            format::count(faults.round() as u64),
            if faults.round() as u64 == 1 {
                "error"
            } else {
                "errors"
            },
            format::span(Duration::from_secs(window as u64))
        );
        if let Some(usual) = usual_faults {
            text.push_str(&format!(", usually {usual:.1} a minute"));
        }
        text.push_str(&format!(": {}", crate::webhook::layout_cut(&what, 90)));
        let mut finding = Finding::new(level, Kind::ErrorLog, text);
        finding.now = Some(faults);
        finding.usual = per_window;
        finding.against = Some("the minutes before".into());
        out.push(finding);
    }

    // Traffic: against the same hour a day before when there is one, a
    // usual minute otherwise
    let hours = stats.rows(Grain::Hour, clock);
    let echo = crate::logs::echo(&hours, Grain::Hour)
        .filter(|r| !r.partial)
        .map(|r| (r.rate, "this hour a day ago"));
    let usual_minute = typical(&usual_rows).map(|r| (r, "a usual minute"));
    if let Some((usual, against)) = echo.or(usual_minute).filter(|(u, _)| *u > 0.0) {
        let ratio = now.rate / usual;
        if clock.live && usual >= 1.0 && ratio < 0.3 {
            let level = if ratio < 0.1 { Level::Bad } else { Level::Warn };
            let mut finding = Finding::new(
                level,
                Kind::TrafficDown,
                format!(
                    "traffic {} req/s, {} {against} ({} req/s)",
                    rate(now.rate),
                    versus(now.rate, usual),
                    rate(usual)
                ),
            );
            finding.now = Some(now.rate);
            finding.usual = Some(usual);
            finding.against = Some(against.into());
            out.push(finding);
        } else if ratio >= 2.0 && now.requests >= ENOUGH {
            let mut text = format!(
                "traffic {} against {against}: {} req/s, then {}",
                versus(now.rate, usual),
                rate(now.rate),
                rate(usual)
            );
            if let Some((more, path)) = grew.filter(|(more, _)| *more >= (now.rate - usual) * 0.3) {
                text.push_str(&format!(", led by {path} (+{}/s)", rate(more)));
            }
            let mut finding = Finding::new(Level::Warn, Kind::TrafficUp, text);
            finding.now = Some(now.rate);
            finding.usual = Some(usual);
            finding.against = Some(against.into());
            out.push(finding);
        }
    }

    // Request time: the window's p99 against a usual minute's
    let times = window_times(&recent);
    let usual_p99: Vec<f64> = usual_rows
        .iter()
        .filter(|r| r.slot.times.count() >= ENOUGH)
        .filter_map(|r| r.slot.times.percentile(0.99))
        .map(|d| d.as_secs_f64())
        .collect();
    if let (Some(p99), Some(usual)) = (times.percentile(0.99), median(usual_p99)) {
        let p99 = p99.as_secs_f64();
        let ratio = p99 / usual.max(1e-6);
        let level = match ratio {
            r if r >= 5.0 && p99 >= 1.0 => Some(Level::Bad),
            r if r >= 2.0 && p99 >= 0.2 => Some(Level::Warn),
            _ => None,
        };
        if let Some(level) = level.filter(|_| times.count() >= ENOUGH) {
            let d = |s: f64| format::latency(Duration::from_secs_f64(s));
            let mut text = format!(
                "p99 {} now, {ratio:.1}× a usual minute's {}",
                d(p99),
                d(usual)
            );
            let mut path_named = None;
            if let Some((ratio, now_us, before_us, path)) = slower {
                text.push_str(&format!(
                    "; most slowed {path}, its mean {} from {} ({ratio:.1}×)",
                    d(now_us / 1e6),
                    d(before_us / 1e6)
                ));
                if let Some(share) = stats.paths.count(path).and_then(|p| p.upstream_share()) {
                    text.push_str(&match share {
                        s if s >= 0.7 => format!(", {} of it upstream: the app", percent(s)),
                        s if s <= 0.3 => format!(", {} of it in nginx", percent(1.0 - s)),
                        s => format!(", {} of it upstream", percent(s)),
                    });
                }
                path_named = Some(path.to_string());
            }
            let mut finding = Finding::new(level, Kind::Latency, text);
            finding.path = path_named;
            finding.now = Some(p99 * 1e3);
            finding.usual = Some(usual * 1e3);
            finding.against = Some("a usual minute".into());
            out.push(finding);
        }
    }

    // One client, much of everything
    if stats.requests >= 1_000 && stats.clients.len() >= 3 {
        let top = stats.clients.top(2, |n| *n);
        if let Some((client, n)) = top.first() {
            let share = **n as f64 / stats.requests as f64;
            let next = top.get(1).map_or(0, |(_, n)| **n);
            if share >= 0.2 && **n >= next * 2 {
                let mut finding = Finding::new(
                    Level::Note,
                    Kind::Client,
                    format!(
                        "{client} sends {} of all requests ({})",
                        percent(share),
                        format::count(**n)
                    ),
                );
                finding.now = Some(share);
                out.push(finding);
            }
        }
    }
    if stats.unread > 0 {
        out.push(Finding::new(
            Level::Note,
            Kind::Unread,
            format!(
                "{} of {} lines couldn't be read; a log with a format of its own needs --format",
                format::count(stats.unread),
                format::count(stats.lines)
            ),
        ));
    }
    if !out.iter().any(|f| f.level >= Level::Warn) && now.requests > 0 {
        let mut text = format!("5xx {} now", percent(s5));
        if let Some(usual) = usual_5xx {
            text.push_str(&format!(", {} usually", percent(usual)));
        }
        if let Some((usual, against)) = usual_minute {
            text.push_str(&format!(
                "; traffic {} {against}",
                match versus(now.rate, usual).as_str() {
                    "=" => "as in".to_string(),
                    v => format!("{v} on"),
                }
            ));
        }
        out.push(Finding::new(Level::Good, Kind::Steady, text));
    }
    out.sort_by_key(|f| std::cmp::Reverse(f.level));
    out
}

fn median(mut values: Vec<f64>) -> Option<f64> {
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    Some(values[values.len() / 2])
}

// ─── --fail-if ───────────────────────────────────────────────────────────────

/// What a log is held to
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Measure {
    /// The share answering 5xx, in percent
    Errors,
    /// The share answering 4xx, in percent
    Refused,
    /// A percentile of the request time, in microseconds
    Time(f64),
    /// Requests a second
    Rate,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Condition {
    pub measure: Measure,
    /// Crossed above the limit; below it with `<`
    pub above: bool,
    pub limit: f64,
    /// As it was written
    pub text: String,
}

/// Which requests a limit is held against
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// Everything read: a report, from cron with `--since`
    All,
    /// The last `--window`: while the log is followed
    Window,
}

impl Condition {
    pub fn parse(text: &str) -> Result<Condition, String> {
        let help = "expected e.g. '5xx > 2%', '4xx > 20%', 'p99 > 1s' or 'rate < 5/s'";
        let (at, above) = match (text.find('>'), text.find('<')) {
            (Some(at), None) => (at, true),
            (None, Some(at)) => (at, false),
            _ => return Err(format!("{text:?}: {help}")),
        };
        let name = text[..at].trim().to_lowercase();
        let limit = text[at + 1..].trim_start_matches('=').trim().to_lowercase();
        let number = |unit: &str| -> Option<f64> {
            limit
                .strip_suffix(unit)?
                .trim()
                .parse::<f64>()
                .ok()
                .filter(|n| *n >= 0.0 && n.is_finite())
        };
        let (measure, limit) = match name.as_str() {
            "5xx" | "errors" | "error" | "error rate" => (
                Measure::Errors,
                number("%")
                    .ok_or_else(|| format!("{text:?}: a share takes a percentage, e.g. 2%"))?,
            ),
            "4xx" => (
                Measure::Refused,
                number("%")
                    .ok_or_else(|| format!("{text:?}: a share takes a percentage, e.g. 20%"))?,
            ),
            "rate" | "req/s" | "rps" => (
                Measure::Rate,
                number("/s")
                    .or_else(|| number("req/s"))
                    .or_else(|| number(""))
                    .ok_or_else(|| format!("{text:?}: a rate is requests a second, e.g. 5/s"))?,
            ),
            name => {
                let percentile = name
                    .strip_prefix('p')
                    .and_then(|p| p.parse::<f64>().ok())
                    .filter(|p| (0.0..=100.0).contains(p))
                    .ok_or_else(|| format!("{text:?}: unknown measure {name:?}; {help}"))?;
                let micros = number("ms")
                    .map(|n| n * 1e3)
                    .or_else(|| number("us").or_else(|| number("µs")))
                    .or_else(|| number("s").map(|n| n * 1e6))
                    .ok_or_else(|| format!("{text:?}: a time takes a unit, e.g. 500ms or 2s"))?;
                (Measure::Time(percentile), micros)
            }
        };
        if !above && measure != Measure::Rate {
            return Err(format!(
                "{text:?}: only a rate is held from below, as in 'rate < 5/s'"
            ));
        }
        Ok(Condition {
            measure,
            above,
            limit,
            text: text.split_whitespace().collect::<Vec<_>>().join(" "),
        })
    }

    /// What the logs measure, and whether that crosses the limit
    pub fn check(&self, stats: &Stats, clock: Clock, window: i64, scope: Scope) -> Check {
        let now = stats.now(clock, window);
        let (requests, c5xx, c4xx) = match scope {
            Scope::Window => (now.requests, now.c5xx, now.c4xx),
            Scope::All => (
                stats.requests,
                stats.statuses.range(500..).map(|(_, n)| n).sum(),
                stats.statuses.range(400..500).map(|(_, n)| n).sum(),
            ),
        };
        let (value, measured) = match self.measure {
            Measure::Errors | Measure::Refused => {
                if requests == 0 {
                    return Check {
                        text: self.text.clone(),
                        held: true,
                        measured: "no requests".into(),
                    };
                }
                let (n, what) = match self.measure {
                    Measure::Errors => (c5xx, "5xx"),
                    _ => (c4xx, "4xx"),
                };
                let share = n as f64 / requests as f64;
                (share * 100.0, format!("{what} were {}", percent(share)))
            }
            Measure::Time(p) => {
                let us = match scope {
                    Scope::All => (stats.time.count() > 0).then(|| stats.time.percentile(p) as f64),
                    Scope::Window => {
                        let minutes = stats.rows(Grain::Minute, clock);
                        window_times(&in_window(&minutes, clock, window))
                            .percentile(p / 100.0)
                            .map(|d| d.as_micros() as f64)
                    }
                };
                let Some(us) = us else {
                    return Check {
                        text: self.text.clone(),
                        held: true,
                        measured: "no request time in the log".into(),
                    };
                };
                (
                    us,
                    format!(
                        "p{} was {}",
                        format_percentile(p),
                        format::latency(Duration::from_micros(us as u64))
                    ),
                )
            }
            Measure::Rate => {
                let rate_now = match scope {
                    Scope::Window => now.rate,
                    Scope::All => match (stats.first, stats.last) {
                        (Some(first), Some(last)) => {
                            stats.requests as f64 / (last.at - first.at + 1).max(1) as f64
                        }
                        _ => 0.0,
                    },
                };
                (rate_now, format!("{} req/s", rate(rate_now)))
            }
        };
        let crossed = match self.above {
            true => value > self.limit,
            false => value < self.limit,
        };
        Check {
            text: self.text.clone(),
            held: !crossed,
            measured,
        }
    }
}

fn format_percentile(p: f64) -> String {
    if p.fract() == 0.0 {
        format!("{p:.0}")
    } else {
        format!("{p}")
    }
}

// ─── Watching ────────────────────────────────────────────────────────────────

/// A limit while the log is followed: crossed at some point, and what was
/// measured when it was
#[derive(Debug, Clone, Default)]
pub struct Tally {
    pub now: Option<Check>,
    /// When it was first crossed (the clock's second), and what was measured
    pub crossed: Option<(i64, String)>,
    /// It is crossed now
    pub crossing: bool,
}

/// The findings and the limits of a log being followed, worked out once a
/// second, and what changed said to the webhooks
#[derive(Debug, Default)]
pub struct Watch {
    pub conditions: Vec<Condition>,
    pub window: i64,
    pub findings: Vec<Finding>,
    pub tallies: Vec<Tally>,
    /// The second they were worked out at
    at: Option<i64>,
    /// The findings that were bad and said so, by key, with what they
    /// said: one that turns up is said once, and so is its going
    bad: Vec<(String, String)>,
}

impl Watch {
    pub fn new(conditions: Vec<Condition>, window: i64) -> Watch {
        Watch {
            tallies: vec![Tally::default(); conditions.len()],
            conditions,
            window,
            ..Watch::default()
        }
    }

    /// Work everything out again when a second has passed. Once the log
    /// has been read to where it is being written, what turns bad, what
    /// is crossed and what comes back are each said to the webhooks.
    pub fn tick(&mut self, stats: &Stats, wall: i64) {
        if self.at == Some(wall) {
            return;
        }
        self.at = Some(wall);
        let clock = stats.clock(wall);
        self.findings = findings(stats, clock, self.window);
        let telling = stats.caught_up;
        let bad: Vec<&Finding> = self
            .findings
            .iter()
            .filter(|f| f.level == Level::Bad)
            .collect();
        // Nothing is said until the log is read to where it is being
        // written: what was wrong an hour ago isn't news
        if telling {
            for finding in &bad {
                if !self.bad.iter().any(|(key, _)| *key == finding.key()) {
                    crate::webhook::anomaly(finding.kind.moved(), &finding.text);
                }
            }
            for (key, text) in &self.bad {
                if !bad.iter().any(|f| f.key() == *key) {
                    crate::webhook::anomaly("errors", &format!("recovered: {text}"));
                }
            }
            self.bad = bad.iter().map(|f| (f.key(), f.text.clone())).collect();
        }
        for (condition, tally) in self.conditions.iter().zip(&mut self.tallies) {
            let check = condition.check(stats, clock, self.window, Scope::Window);
            let crossing = !check.held;
            if telling && crossing && !tally.crossing {
                crate::webhook::anomaly(
                    match condition.measure {
                        Measure::Time(_) => "latency",
                        Measure::Rate => "throughput",
                        _ => "errors",
                    },
                    &format!("crossed {}: {}", condition.text, check.measured),
                );
                if tally.crossed.is_none() {
                    tally.crossed = Some((wall, check.measured.clone()));
                }
            } else if telling && !crossing && tally.crossing {
                crate::webhook::anomaly(
                    "errors",
                    &format!("held again {}: {}", condition.text, check.measured),
                );
            }
            tally.crossing = crossing && telling;
            tally.now = Some(check);
        }
        if telling {
            let now = stats.now(clock, self.window);
            let seconds = stats.seconds_back(clock, 60);
            let minutes = stats.rows(Grain::Minute, clock);
            let times = window_times(&in_window(&minutes, clock, self.window));
            let at = |q: f64| times.percentile(q).unwrap_or_default();
            crate::webhook::progress(|| crate::webhook::Progress {
                requests: stats.requests,
                failed: stats.statuses.range(500..).map(|(_, n)| n).sum(),
                rps: now.rate,
                p50: at(0.5),
                p95: at(0.95),
                p99: at(0.99),
                bytes: stats.bytes,
                series: seconds
                    .iter()
                    .map(|[n, ..]| (f64::from(*n), at(0.99).as_secs_f64() * 1e3))
                    .collect(),
                failures: stats
                    .statuses
                    .iter()
                    .filter(|(code, _)| **code >= 500)
                    .map(|(code, n)| (code.to_string(), *n))
                    .collect(),
                ..Default::default()
            });
        }
    }

    /// Each limit as the session leaves it: crossed if it ever was
    pub fn checks(&self) -> Vec<Check> {
        self.conditions
            .iter()
            .zip(&self.tallies)
            .map(|(condition, tally)| match &tally.crossed {
                Some((at, measured)) => Check {
                    text: condition.text.clone(),
                    held: false,
                    measured: format!(
                        "{measured} at {}",
                        time_of_day(*at + i64::from(crate::logs::local_offset()))
                    ),
                },
                None => tally.now.clone().unwrap_or(Check {
                    text: condition.text.clone(),
                    held: true,
                    measured: "not measured".into(),
                }),
            })
            .collect()
    }
}

/// The findings and the limits, as the report printed in the shell says
/// them: a line each
pub fn text(findings: &[Finding], checks: &[Check]) -> String {
    let mut out = String::new();
    for finding in findings {
        out.push_str(&format!("  {} {}\n", finding.level.glyph(), finding.text));
    }
    if !checks.is_empty() {
        out.push('\n');
    }
    for check in checks {
        let glyph = if check.held { '✔' } else { '✖' };
        out.push_str(&format!("  {glyph} {}: {}\n", check.text, check.measured));
    }
    out
}

/// What the JSON report adds: the verdict, and each limit
pub fn json(findings: &[Finding], checks: &[Check], offset: i32) -> serde_json::Value {
    serde_json::json!({
        "verdict": {
            "level": level(findings),
            "findings": findings.iter().map(|f| f.json(offset)).collect::<Vec<_>>(),
        },
        "fail_if": checks.iter().map(|c| serde_json::json!({
            "condition": c.text,
            "held": c.held,
            "measured": c.measured,
        })).collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logs::{civil, Parser};

    /// 08 Oct 2026 11:55:36 UTC
    const AT: i64 = 1_791_460_536;
    /// Thirty minutes of the usual before the last one
    const USUAL: i64 = 30 * 60;

    fn hit(s: i64, path: &str, status: u16, seconds: f64) -> String {
        let c = civil(AT + s);
        format!(
            "10.0.0.{} - - [{:02}/Oct/{}:{:02}:{:02}:{:02} +0000] \"GET {path} HTTP/1.1\" {status} 512 \"-\" \"curl/8\" rt={seconds:.3} urt={:.3}",
            s % 4, c.day, c.year, c.hour, c.minute, c.second, seconds * 0.9
        )
    }

    fn fault(s: i64) -> String {
        let c = civil(AT + s);
        format!(
            "{}/{:02}/{:02} {:02}:{:02}:{:02} [error] 7#7: *1 upstream timed out (110: Connection timed out) while reading response header from upstream, client: 10.0.0.1, server: _, request: \"GET /a HTTP/1.1\"",
            c.year, c.month, c.day, c.hour, c.minute, c.second
        )
    }

    /// The usual half hour, then a last minute that `last` writes, read
    /// as a log ended a day ago, or being written when `live`
    fn watched(last: impl Fn(i64) -> Vec<String>, live: bool) -> (Stats, Clock) {
        let parser = Parser::default();
        let mut stats = Stats::default();
        let end = USUAL + 60;
        let wall = if live { AT + end } else { AT + 86_400 };
        let mut fold = |line: String| stats.fold(parser.read(&line), &line, wall);
        for s in 0..USUAL {
            fold(hit(s, "/a", 200, 0.020));
            fold(hit(s, "/b", if s % 200 == 0 { 404 } else { 200 }, 0.030));
            if s % 600 == 0 {
                fold(fault(s));
            }
        }
        for s in USUAL..end {
            for line in last(s) {
                fold(line);
            }
        }
        stats.caught_up = true;
        let clock = stats.clock(wall);
        (stats, clock)
    }

    fn usual(s: i64) -> Vec<String> {
        vec![hit(s, "/a", 200, 0.020), hit(s, "/b", 200, 0.030)]
    }

    #[test]
    fn a_usual_minute_is_steady() {
        let (stats, clock) = watched(usual, false);
        let found = findings(&stats, clock, 60);
        assert_eq!(found.len(), 1, "{found:#?}");
        assert_eq!((found[0].level, found[0].kind), (Level::Good, Kind::Steady));
        assert!(
            found[0].text.contains("traffic as in a usual minute"),
            "{}",
            found[0].text
        );
        assert_eq!(level(&found), "healthy");
    }

    #[test]
    fn a_path_that_starts_failing_is_named_with_when() {
        let (stats, clock) = watched(
            |s| {
                let mut lines = usual(s);
                lines.push(hit(
                    s,
                    "/orders/7",
                    if s % 2 == 0 { 502 } else { 200 },
                    0.05,
                ));
                lines
            },
            false,
        );
        let found = findings(&stats, clock, 60);
        let errors = found.iter().find(|f| f.kind == Kind::Errors).unwrap();
        assert_eq!(errors.level, Level::Bad, "{found:#?}");
        assert!(
            errors.text.starts_with("5xx 17% of requests since 12:25"),
            "{}",
            errors.text
        );
        assert!(
            errors.text.contains("100% of them on /orders/*"),
            "{}",
            errors.text
        );
        let path = found.iter().find(|f| f.kind == Kind::PathErrors).unwrap();
        assert_eq!(path.path.as_deref(), Some("/orders/*"));
        assert_eq!(path.level, Level::Bad);
        assert!(
            path.text
                .starts_with("/orders/* answers 5xx to 50% since 12:25 (502)"),
            "{}",
            path.text
        );
        // The minute it started in
        assert_eq!(path.since.map(|at| at % 60), Some(0));
        assert_eq!(level(&found), "failing");
        assert_eq!(found[0].level, Level::Bad, "most serious first");
        let json = path.json(0);
        assert_eq!(json["kind"], "path_errors");
        assert_eq!(json["now"], 0.5);
        assert!(json["since"]
            .as_str()
            .unwrap()
            .starts_with("2026-10-08T12:25"));
    }

    #[test]
    fn a_path_refused_in_bulk_is_bad() {
        let (stats, clock) = watched(
            |s| {
                let mut lines = usual(s);
                lines
                    .extend((0..3).map(|i| hit(s, "/login", if i == 0 { 200 } else { 401 }, 0.01)));
                lines
            },
            false,
        );
        let found = findings(&stats, clock, 60);
        let refused = found.iter().find(|f| f.kind == Kind::PathRefused).unwrap();
        assert_eq!(
            (refused.level, refused.path.as_deref()),
            (Level::Bad, Some("/login"))
        );
        assert!(refused.text.contains("(401)"), "{}", refused.text);
    }

    #[test]
    fn the_error_log_bursting_is_said_with_its_message() {
        let (stats, clock) = watched(
            |s| {
                let mut lines = usual(s);
                if s % 3 == 0 {
                    lines.push(fault(s));
                }
                lines
            },
            false,
        );
        let found = findings(&stats, clock, 60);
        let log = found.iter().find(|f| f.kind == Kind::ErrorLog).unwrap();
        assert_eq!(log.level, Level::Bad, "{found:#?}");
        // The last minute straddles two on the clock: each counts for the
        // part of it in the window
        assert!(
            log.text.contains("15 errors in the last 1m"),
            "{}",
            log.text
        );
        assert!(log.text.contains("upstream timed out"), "{}", log.text);
    }

    #[test]
    fn traffic_up_names_the_path_that_grew_and_down_is_bad_when_live() {
        let (stats, clock) = watched(
            |s| {
                let mut lines = usual(s);
                lines.extend((0..6).map(|_| hit(s, "/search", 200, 0.02)));
                lines
            },
            false,
        );
        let found = findings(&stats, clock, 60);
        let up = found.iter().find(|f| f.kind == Kind::TrafficUp).unwrap();
        assert_eq!(up.level, Level::Warn);
        assert!(
            up.text.starts_with("traffic ×4.0 against a usual minute"),
            "{}",
            up.text
        );
        assert!(up.text.contains("led by /search (+"), "{}", up.text);

        // Being written, and quiet
        let (stats, clock) = watched(
            |s| match s % 30 {
                0 => vec![hit(s, "/a", 200, 0.02)],
                _ => Vec::new(),
            },
            true,
        );
        assert!(clock.live);
        let found = findings(&stats, clock, 60);
        let down = found.iter().find(|f| f.kind == Kind::TrafficDown).unwrap();
        assert_eq!(down.level, Level::Bad, "{found:#?}");
        // The same, read later: a log that ended isn't quiet
        let (stats, clock) = watched(
            |s| match s % 30 {
                0 => vec![hit(s, "/a", 200, 0.02)],
                _ => Vec::new(),
            },
            false,
        );
        assert!(!findings(&stats, clock, 60)
            .iter()
            .any(|f| f.kind == Kind::TrafficDown));
    }

    #[test]
    fn slower_requests_name_the_path_and_where_the_time_went() {
        let (stats, clock) = watched(
            |s| vec![hit(s, "/a", 200, 0.020), hit(s, "/b", 200, 0.800)],
            false,
        );
        let found = findings(&stats, clock, 60);
        let slow = found.iter().find(|f| f.kind == Kind::Latency).unwrap();
        assert_eq!(slow.path.as_deref(), Some("/b"), "{found:#?}");
        assert!(slow.text.contains("most slowed /b"), "{}", slow.text);
        assert!(
            slow.text.contains("of it upstream: the app"),
            "{}",
            slow.text
        );
    }

    #[test]
    fn limits_hold_over_everything_read_or_the_last_window() {
        let (stats, clock) = watched(
            |s| {
                let mut lines = usual(s);
                lines.push(hit(
                    s,
                    "/orders/7",
                    if s % 2 == 0 { 502 } else { 200 },
                    0.05,
                ));
                lines
            },
            false,
        );
        let errors = Condition::parse("5xx > 5%").unwrap();
        let all = errors.check(&stats, clock, 60, Scope::All);
        assert!(all.held, "30 of 3,780: {}", all.measured);
        let window = errors.check(&stats, clock, 60, Scope::Window);
        assert_eq!(
            (window.held, window.measured.as_str()),
            (false, "5xx were 17%")
        );
        let quiet = Condition::parse("rate < 5/s").unwrap();
        let check = quiet.check(&stats, clock, 60, Scope::Window);
        assert_eq!((check.held, check.measured.as_str()), (false, "3 req/s"));
        let slow = Condition::parse("p95 > 40ms").unwrap();
        assert!(!slow.check(&stats, clock, 60, Scope::Window).held);
        assert!(slow.check(&stats, clock, 60, Scope::All).held);
        // With nothing read, a share holds and a rate from below doesn't
        let empty = Stats::default();
        let clock = empty.clock(AT);
        assert!(errors.check(&empty, clock, 60, Scope::All).held);
        assert!(!quiet.check(&empty, clock, 60, Scope::All).held);

        // While watching: a limit crossed once stays crossed, with when
        let (stats, _) = watched(
            |s| {
                let mut lines = usual(s);
                lines.push(hit(s, "/orders/7", 502, 0.05));
                lines
            },
            false,
        );
        let mut watch = Watch::new(vec![errors.clone()], 60);
        watch.tick(&stats, AT + 86_400);
        assert!(watch.tallies[0].crossing);
        let checks = watch.checks();
        assert!(
            !checks[0].held && checks[0].measured.contains(" at "),
            "{:?}",
            checks[0]
        );
        assert_eq!(
            text(&watch.findings, &checks)
                .lines()
                .last()
                .unwrap()
                .trim_start()
                .chars()
                .next(),
            Some('✖')
        );
        let json = json(&watch.findings, &checks, 0);
        assert_eq!(json["verdict"]["level"], "failing");
        assert_eq!(json["fail_if"][0]["held"], false);
    }

    #[test]
    fn limits_are_read_as_written() {
        let c = Condition::parse("5xx > 2%").unwrap();
        assert_eq!((c.measure, c.above, c.limit), (Measure::Errors, true, 2.0));
        let c = Condition::parse("errors >= 1.5%").unwrap();
        assert_eq!((c.measure, c.limit), (Measure::Errors, 1.5));
        let c = Condition::parse("4xx > 20%").unwrap();
        assert_eq!(c.measure, Measure::Refused);
        let c = Condition::parse("p99 > 1s").unwrap();
        assert_eq!((c.measure, c.limit), (Measure::Time(99.0), 1e6));
        let c = Condition::parse("p99.9 > 250ms").unwrap();
        assert_eq!((c.measure, c.limit), (Measure::Time(99.9), 250e3));
        let c = Condition::parse("rate < 5/s").unwrap();
        assert_eq!((c.measure, c.above, c.limit), (Measure::Rate, false, 5.0));
        let c = Condition::parse("RATE  >  800").unwrap();
        assert_eq!(
            (c.above, c.limit, c.text.as_str()),
            (true, 800.0, "RATE > 800")
        );
        for wrong in [
            "5xx > 2",
            "p99 > 1",
            "5xx < 2%",
            "latency > 1s",
            "rate",
            "rate > fast",
            "p200 > 1s",
        ] {
            assert!(Condition::parse(wrong).is_err(), "{wrong}");
        }
    }
}
