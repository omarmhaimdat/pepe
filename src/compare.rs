//! `pepe compare before.json after.json`: what moved between two runs,
//! said the way the verdict says things, and whether it moved more than
//! a run of that size usually wobbles

use std::path::Path;

use serde::Serialize;
use serde_json::Value;

use crate::insights::{Level, LONG_TAIL};
use crate::ui::format;

/// Below this, a change in a latency or a rate is never called real: two
/// runs of the same build differ by about this much on their own
const FLOOR: f64 = 0.05;
/// A change in the failure share, in percentage points, worth a word
const FAILURE_POINTS: f64 = 0.5;
/// How far a percentile wanders between identical runs, in units of the
/// latency's own relative spread over the square root of the requests
/// past it. Measured, not derived: against a local server, the p99 of
/// 2,000-request runs sat ±25% apart and that of 20,000-request runs ±1%,
/// while the median stayed within ±1%
const SPREAD: f64 = 6.0;
/// A capacity estimate is a curve fit; it moves this much between ramps
const CAPACITY_FLOOR: f64 = 0.10;

/// The one-word answer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    /// Fewer failures than before
    Better,
    Faster,
    Same,
    Slower,
    /// More failures than before
    Worse,
}

impl Outcome {
    pub fn headline(self) -> &'static str {
        match self {
            Outcome::Better => "Better",
            Outcome::Faster => "Faster",
            Outcome::Same => "About the same",
            Outcome::Slower => "Slower",
            Outcome::Worse => "Worse",
        }
    }

    /// Slower or Worse: what a gate stops on
    pub fn is_regression(self) -> bool {
        matches!(self, Outcome::Slower | Outcome::Worse)
    }

    fn level(self) -> Level {
        match self {
            Outcome::Better | Outcome::Faster | Outcome::Same => Level::Healthy,
            Outcome::Slower => Level::Degraded,
            Outcome::Worse => Level::Failing,
        }
    }
}

/// One number held against its earlier self
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Change {
    pub metric: &'static str,
    pub before: f64,
    pub after: f64,
    /// Relative, in percent; absent when `before` is zero
    #[serde(skip_serializing_if = "Option::is_none")]
    pub change_percent: Option<f64>,
    /// How much this number wobbles between two runs like these, in
    /// percent: a change inside it is not called a change
    pub noise_percent: f64,
    pub significant: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Finding {
    pub level: &'static str,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct Comparison {
    /// The shape of this report (see `pepe schema compare`)
    pub schema_version: u32,
    pub verdict: Outcome,
    pub regression: bool,
    pub before: Side,
    pub after: Side,
    pub changes: Vec<Change>,
    pub findings: Vec<Finding>,
    /// The verdict in a badge's three parts, for shields.io and the like
    pub badge: Badge,
}

/// `pepe | slower · p99 +38%`, in a colour: what a badge service draws
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Badge {
    pub label: &'static str,
    pub message: String,
    /// A hex colour, without the `#`
    pub color: &'static str,
}

/// pepe's palette, as the dashboard draws it (src/ui/theme.rs)
mod palette {
    pub const GROUND: &str = "#140f0d";
    pub const SURFACE: &str = "#1d1613";
    pub const TEXT: &str = "#f3e8de";
    pub const EMBER: &str = "#ff8a3d";
    pub const LABEL: &str = "#9c8a7e";
    pub const GOOD: &str = "#9ccf6a";
    pub const WARN: &str = "#f0cf5a";
    pub const BAD: &str = "#ff5a6e";
}

/// What a report says, in the numbers the comparison is made of
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Side {
    pub file: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u64>,
    pub requests: u64,
    pub failed_percent: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p99_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests_per_second: Option<f64>,
    /// A ramp's estimate, when the report is a ramp's
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_per_second: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub holds_concurrency: Option<u64>,
    /// A ping's interval, when the report is a ping's
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every_s: Option<f64>,
    /// A ping's phases, their medians
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dns_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tls_resumed_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ttfb_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub download_ms: Option<f64>,
    /// Relative spread of one request's latency: std dev over the mean
    #[serde(skip)]
    cv: f64,
    /// The failure most often seen
    #[serde(skip)]
    main_failure: Option<String>,
}

impl Side {
    /// A run report (`pepe --json`, `--snapshot`, API, flow or replay),
    /// a ramp's, or a ping's
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let doc: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not a JSON report: {e}", path.display()))?;
        let file = path.display().to_string();
        if doc.get("mode").and_then(Value::as_str) == Some("ping") {
            return Self::of_ping(file, &doc);
        }
        if let Some(summary) = doc.get("summary") {
            return Ok(Self::of_run(file, &doc, summary));
        }
        if doc.get("steps").is_some() {
            return Ok(Self::of_ramp(file, &doc));
        }
        Err(format!(
            "{file} is not a pepe report: no `summary` (a run), no `steps` (a ramp) and no `targets` (a ping)"
        ))
    }

    /// A ping's report (`pepe ping --json`, `--save`): its first target.
    /// A lost ping is a failed request; the jitter stands in for the
    /// latency's spread, which a ping report doesn't have
    fn of_ping(file: String, doc: &Value) -> Result<Self, String> {
        let targets = doc
            .get("targets")
            .and_then(Value::as_array)
            .filter(|t| !t.is_empty())
            .ok_or_else(|| format!("{file} is a ping report with no target in it"))?;
        let t = &targets[0];
        let num = |v: &Value, key: &str| v.get(key).and_then(Value::as_f64);
        let sent = num(t, "sent").unwrap_or(0.0);
        let lost = num(t, "lost").unwrap_or(0.0);
        let latency = t.get("latency_ms");
        let avg = latency.and_then(|l| num(l, "avg")).unwrap_or(0.0);
        let jitter = latency.and_then(|l| num(l, "jitter")).unwrap_or(0.0);
        let phase = |name: &str| {
            t.get("phases_ms")
                .and_then(|p| p.get(name))
                .and_then(|p| num(p, "median_ms"))
        };
        let main_failure = t
            .get("failures")
            .and_then(Value::as_object)
            .and_then(|f| f.iter().max_by_key(|(_, n)| n.as_u64().unwrap_or(0)))
            .map(|(cause, _)| cause.clone());
        Ok(Side {
            file,
            target: t
                .get("target")
                .and_then(Value::as_str)
                .map(|url| format!("ping {url}")),
            concurrency: None,
            requests: sent as u64,
            failed_percent: if sent > 0.0 { lost / sent * 100.0 } else { 0.0 },
            median_ms: latency.and_then(|l| num(l, "p50")),
            p99_ms: latency.and_then(|l| num(l, "p99")),
            requests_per_second: None,
            capacity_per_second: None,
            holds_concurrency: None,
            every_s: num(doc, "every_s"),
            dns_ms: phase("dns"),
            connect_ms: phase("connect"),
            tls_ms: phase("tls_full"),
            tls_resumed_ms: phase("tls_resumed"),
            ttfb_ms: phase("ttfb"),
            download_ms: phase("download"),
            // The mean difference between one ping and the next is about
            // 1.1 standard deviations of a steady series
            cv: if avg > 0.0 { jitter / avg / 1.13 } else { 0.0 },
            main_failure,
        })
    }

    fn of_run(file: String, doc: &Value, summary: &Value) -> Self {
        let num = |v: &Value, key: &str| v.get(key).and_then(Value::as_f64);
        let total = num(summary, "total_requests").unwrap_or(0.0);
        let failed = num(summary, "failed_requests").unwrap_or(0.0);
        let latency = summary.get("latency");
        let avg = latency.and_then(|l| num(l, "avg_ms")).unwrap_or(0.0);
        let sd = latency.and_then(|l| num(l, "std_dev_ms")).unwrap_or(0.0);
        let target = doc.get("target");
        let main_failure = summary
            .get("failures")
            .and_then(Value::as_array)
            .and_then(|f| f.first())
            .and_then(|f| f.get("cause"))
            .and_then(Value::as_str)
            .map(str::to_string);
        Side {
            file,
            target: target.map(|t| {
                format!(
                    "{} {}",
                    t.get("method").and_then(Value::as_str).unwrap_or("GET"),
                    t.get("url").and_then(Value::as_str).unwrap_or("")
                )
            }),
            concurrency: target
                .and_then(|t| t.get("concurrency"))
                .and_then(Value::as_u64),
            requests: total as u64,
            failed_percent: if total > 0.0 {
                failed / total * 100.0
            } else {
                0.0
            },
            median_ms: latency.and_then(|l| num(l, "median_ms")),
            p99_ms: latency.and_then(|l| num(l, "p99_ms")),
            requests_per_second: num(summary, "requests_per_second"),
            capacity_per_second: None,
            holds_concurrency: None,
            every_s: None,
            dns_ms: None,
            connect_ms: None,
            tls_ms: None,
            tls_resumed_ms: None,
            ttfb_ms: None,
            download_ms: None,
            cv: if avg > 0.0 { sd / avg } else { 0.0 },
            main_failure,
        }
    }

    fn of_ramp(file: String, doc: &Value) -> Self {
        let num = |v: &Value, key: &str| v.get(key).and_then(Value::as_f64);
        let total = num(doc, "requests").unwrap_or(0.0);
        let failed = num(doc, "failed_requests").unwrap_or(0.0);
        let capacity = doc.get("capacity").filter(|c| !c.is_null());
        Side {
            file,
            target: None,
            concurrency: None,
            requests: total as u64,
            failed_percent: if total > 0.0 {
                failed / total * 100.0
            } else {
                0.0
            },
            median_ms: None,
            p99_ms: None,
            requests_per_second: None,
            capacity_per_second: capacity.and_then(|c| num(c, "requests_per_second")),
            holds_concurrency: doc.get("holds_concurrency").and_then(Value::as_u64),
            every_s: None,
            dns_ms: None,
            connect_ms: None,
            tls_ms: None,
            tls_resumed_ms: None,
            ttfb_ms: None,
            download_ms: None,
            cv: 0.0,
            main_failure: None,
        }
    }

    /// How much a percentile at `quantile` wobbles between runs of this
    /// size, relative: the latency's own spread, divided by the square root
    /// of how many requests sit past that quantile, scaled as measured
    fn noise(&self, quantile: f64) -> f64 {
        let n = (self.requests as f64 * (1.0 - quantile).max(0.005)).max(1.0);
        SPREAD * self.cv / n.sqrt()
    }
}

/// Hold `after` against `before`
pub fn compare(before: &Side, after: &Side) -> Comparison {
    let mut changes = Vec::new();
    let mut findings = Vec::new();
    let note = |level: Level, text: String| Finding {
        level: level.headline(),
        text,
    };

    // Is this even the same test?
    if let (Some(a), Some(b)) = (&before.target, &after.target) {
        if a != b {
            findings.push(note(
                Level::Degraded,
                format!("Different targets: {a}, then {b}"),
            ));
        }
    }
    if let (Some(a), Some(b)) = (before.concurrency, after.concurrency) {
        if a != b {
            findings.push(note(
                Level::Degraded,
                format!("Different concurrency: {a}, then {b}"),
            ));
        }
    }
    if let (Some(a), Some(b)) = (before.every_s, after.every_s) {
        if a != b {
            findings.push(note(
                Level::Degraded,
                format!("Different intervals: a ping every {a}s, then every {b}s"),
            ));
        }
    }

    // Failures first: they outrank speed
    let failed = after.failed_percent - before.failed_percent;
    let mut outcome = Outcome::Same;
    if before.failed_percent == 0.0 && after.failed_percent > 0.0 {
        outcome = Outcome::Worse;
        findings.push(note(
            Level::Failing,
            format!(
                "Failures appeared: {} of requests{}",
                pct(after.failed_percent),
                after
                    .main_failure
                    .as_deref()
                    .map_or(String::new(), |f| format!(", mostly {f}"))
            ),
        ));
    } else if failed >= FAILURE_POINTS {
        outcome = Outcome::Worse;
        findings.push(note(
            Level::Failing,
            format!(
                "Failures up: {} → {} of requests",
                pct(before.failed_percent),
                pct(after.failed_percent)
            ),
        ));
    } else if before.failed_percent > 0.0 && after.failed_percent == 0.0 {
        outcome = Outcome::Better;
        findings.push(note(
            Level::Healthy,
            format!("Failures gone: {} → none", pct(before.failed_percent)),
        ));
    } else if failed <= -FAILURE_POINTS {
        outcome = Outcome::Better;
        findings.push(note(
            Level::Healthy,
            format!(
                "Failures down: {} → {} of requests",
                pct(before.failed_percent),
                pct(after.failed_percent)
            ),
        ));
    }

    // Speed: each number against the spread both runs would show on their own
    let mut slower = false;
    let mut faster = false;
    let mut speed = |metric: &'static str,
                     what: &str,
                     a: Option<f64>,
                     b: Option<f64>,
                     noise: f64,
                     up_is_bad: bool,
                     show: &dyn Fn(f64) -> String| {
        let (Some(a), Some(b)) = (a, b) else {
            return;
        };
        let change = (a > 0.0).then(|| (b - a) / a);
        let significant = change.is_some_and(|c| c.abs() > noise);
        changes.push(Change {
            metric,
            before: a,
            after: b,
            change_percent: change.map(|c| round(c * 100.0)),
            noise_percent: round(noise * 100.0),
            significant,
        });
        let Some(change) = change else {
            return;
        };
        if !significant {
            findings.push(note(
                Level::Healthy,
                format!(
                    "{what} within the usual spread: {} → {} (±{:.0}%)",
                    show(a),
                    show(b),
                    noise * 100.0
                ),
            ));
            return;
        }
        let bad = (change > 0.0) == up_is_bad;
        if bad {
            slower = true;
        } else {
            faster = true;
        }
        findings.push(note(
            if bad { Level::Degraded } else { Level::Healthy },
            format!(
                "{what} {} {}: {} → {}",
                if change > 0.0 { "up" } else { "down" },
                pct(change.abs() * 100.0),
                show(a),
                show(b)
            ),
        ));
    };
    let latency_noise = |q: f64| {
        (before.noise(q).powi(2) + after.noise(q).powi(2))
            .sqrt()
            .max(FLOOR)
    };
    speed(
        "p99_ms",
        "p99",
        before.p99_ms,
        after.p99_ms,
        latency_noise(0.99),
        true,
        &ms,
    );
    speed(
        "median_ms",
        "Median",
        before.median_ms,
        after.median_ms,
        latency_noise(0.5),
        true,
        &ms,
    );
    // A ping's phases: each wobbles about as much as the whole does
    for (metric, what, a, b) in [
        ("dns_ms", "DNS lookup", before.dns_ms, after.dns_ms),
        ("connect_ms", "Connect", before.connect_ms, after.connect_ms),
        ("tls_ms", "TLS handshake", before.tls_ms, after.tls_ms),
        (
            "tls_resumed_ms",
            "Resumed TLS handshake",
            before.tls_resumed_ms,
            after.tls_resumed_ms,
        ),
        ("ttfb_ms", "First byte", before.ttfb_ms, after.ttfb_ms),
        (
            "download_ms",
            "Download",
            before.download_ms,
            after.download_ms,
        ),
    ] {
        speed(metric, what, a, b, latency_noise(0.5), true, &ms);
    }
    // With fixed concurrency the rate follows the median, and wobbles as
    // much as it does
    speed(
        "requests_per_second",
        "Throughput",
        before.requests_per_second,
        after.requests_per_second,
        latency_noise(0.5),
        false,
        &rate,
    );
    speed(
        "capacity_per_second",
        "Capacity",
        before.capacity_per_second,
        after.capacity_per_second,
        CAPACITY_FLOOR,
        false,
        &rate,
    );
    if let (Some(a), Some(b)) = (before.holds_concurrency, after.holds_concurrency) {
        if a != b {
            let bad = b < a;
            if bad {
                slower = true;
            } else {
                faster = true;
            }
            findings.push(note(
                if bad { Level::Degraded } else { Level::Healthy },
                format!("The level that held: {a} → {b} concurrent"),
            ));
        }
    }

    // The shape of the latency: a tail that appeared is worth more than
    // the p99 number alone
    if let (Some(a50), Some(b50), Some(a99), Some(b99)) = (
        before.median_ms,
        after.median_ms,
        before.p99_ms,
        after.p99_ms,
    ) {
        if a50 > 0.0 && b50 > 0.0 {
            let (was, now) = (a99 / a50, b99 / b50);
            if now >= LONG_TAIL && was < LONG_TAIL {
                findings.push(note(
                    Level::Degraded,
                    format!("A long tail is new: p99 is {now:.1}× the median, was {was:.1}×"),
                ));
            } else if was >= LONG_TAIL && now < LONG_TAIL {
                findings.push(note(
                    Level::Healthy,
                    format!("The long tail is gone: p99 is {now:.1}× the median, was {was:.1}×"),
                ));
            }
        }
    }

    if outcome == Outcome::Same {
        outcome = match (slower, faster) {
            (true, _) => Outcome::Slower,
            (false, true) => Outcome::Faster,
            (false, false) => Outcome::Same,
        };
    }
    if findings.is_empty() {
        findings.push(note(
            Level::Healthy,
            "Nothing to compare: the reports hold no numbers in common".into(),
        ));
    }
    let badge = badge(outcome, &changes, before, after);
    Comparison {
        schema_version: 1,
        verdict: outcome,
        regression: outcome.is_regression(),
        before: before.clone(),
        after: after.clone(),
        changes,
        findings,
        badge,
    }
}

/// The verdict and the one number that says it most
fn badge(outcome: Outcome, changes: &[Change], before: &Side, after: &Side) -> Badge {
    let word = outcome.headline().to_lowercase();
    let detail = match outcome {
        Outcome::Worse | Outcome::Better => Some(format!("{} failed", pct(after.failed_percent))),
        Outcome::Faster | Outcome::Slower => changes
            .iter()
            .filter(|c| c.significant)
            .max_by(|a, b| {
                let size = |c: &Change| c.change_percent.unwrap_or(0.0).abs();
                size(a).total_cmp(&size(b))
            })
            .and_then(|c| {
                let change = c.change_percent?;
                let name = match c.metric {
                    "p99_ms" => "p99",
                    "median_ms" => "median",
                    "requests_per_second" => "req/s",
                    "capacity_per_second" => "capacity",
                    "dns_ms" => "dns",
                    "connect_ms" => "connect",
                    "tls_ms" => "tls",
                    "tls_resumed_ms" => "tls resumed",
                    "ttfb_ms" => "first byte",
                    "download_ms" => "download",
                    other => other,
                };
                Some(format!(
                    "{name} {}{}%",
                    if change > 0.0 { "+" } else { "−" },
                    change.abs().round()
                ))
            }),
        Outcome::Same => None,
    };
    let _ = before;
    Badge {
        label: "pepe",
        message: match detail {
            Some(detail) => format!("{word} · {detail}"),
            None => word,
        },
        color: match outcome.level() {
            Level::Healthy => &palette::GOOD[1..],
            Level::Degraded => &palette::WARN[1..],
            Level::Failing => &palette::BAD[1..],
        },
    }
}

impl Comparison {
    /// The report left in the shell, in the end-of-run report's shape
    pub fn report(&self) -> String {
        let level = self.verdict.level();
        let mut head = vec![format!(
            "{} → {} {}",
            format::count(self.before.requests),
            format::count(self.after.requests),
            if self.before.every_s.is_some() {
                "pings"
            } else {
                "requests"
            }
        )];
        if let (Some(a), Some(b)) = (self.before.p99_ms, self.after.p99_ms) {
            head.push(format!("p99 {} → {}", ms(a), ms(b)));
        }
        if let (Some(a), Some(b)) = (
            self.before.requests_per_second,
            self.after.requests_per_second,
        ) {
            head.push(format!("{} → {}", rate(a), rate(b)));
        }
        if let (Some(a), Some(b)) = (
            self.before.capacity_per_second,
            self.after.capacity_per_second,
        ) {
            head.push(format!("capacity {} → {}", rate(a), rate(b)));
        }
        if self.before.failed_percent > 0.0 || self.after.failed_percent > 0.0 {
            head.push(format!(
                "{} → {} {}",
                pct(self.before.failed_percent),
                pct(self.after.failed_percent),
                if self.before.every_s.is_some() {
                    "lost"
                } else {
                    "failed"
                }
            ));
        }
        let mut out = format!(
            "pepe · compare {} → {}\n{} {} · {}\n",
            self.before.file,
            self.after.file,
            level.symbol(),
            self.verdict.headline(),
            head.join(" · ")
        );
        for finding in &self.findings {
            let symbol = match finding.level {
                "Failing" => Level::Failing,
                "Degraded" => Level::Degraded,
                _ => Level::Healthy,
            }
            .symbol();
            out += &format!("  {symbol} {}\n", finding.text);
        }
        out
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// A card of the verdict, drawn as the dashboard draws: the verdict in
    /// its colour, then each number before and after with what moved.
    /// Self-contained SVG, for a README, a site or a report
    pub fn svg(&self) -> String {
        use palette::*;
        struct Row {
            name: &'static str,
            before: String,
            after: String,
            word: String,
            tone: &'static str,
        }
        let level = self.verdict.level();
        let color = match level {
            Level::Healthy => GOOD,
            Level::Degraded => WARN,
            Level::Failing => BAD,
        };
        let mut rows: Vec<Row> = Vec::new();
        for change in &self.changes {
            let (name, show, up_is_bad): (&'static str, &dyn Fn(f64) -> String, bool) =
                match change.metric {
                    "p99_ms" => ("p99", &ms, true),
                    "median_ms" => ("median", &ms, true),
                    "requests_per_second" => ("throughput", &rate, false),
                    "capacity_per_second" => ("capacity", &rate, false),
                    "dns_ms" => ("dns", &ms, true),
                    "connect_ms" => ("connect", &ms, true),
                    "tls_ms" => ("tls", &ms, true),
                    "tls_resumed_ms" => ("tls resumed", &ms, true),
                    "ttfb_ms" => ("first byte", &ms, true),
                    "download_ms" => ("download", &ms, true),
                    _ => continue,
                };
            let (word, tone) = match (change.significant, change.change_percent) {
                (true, Some(c)) => (
                    format!("{} {}%", if c > 0.0 { "▲" } else { "▼" }, c.abs().round()),
                    if (c > 0.0) == up_is_bad { WARN } else { GOOD },
                ),
                _ => (format!("±{}%", change.noise_percent.round()), LABEL),
            };
            rows.push(Row {
                name,
                before: show(change.before),
                after: show(change.after),
                word,
                tone,
            });
        }
        let more_failed = self.after.failed_percent > self.before.failed_percent;
        rows.push(Row {
            name: "failed",
            before: pct(self.before.failed_percent),
            after: pct(self.after.failed_percent),
            word: if more_failed {
                "▲".into()
            } else {
                String::new()
            },
            tone: if more_failed { BAD } else { LABEL },
        });
        let width = 460;
        let height = 72 + rows.len() * 22;
        let mut out = format!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" font-family="ui-monospace, SFMono-Regular, Menlo, Consolas, monospace" font-size="13">
<title>pepe · {verdict}</title>
<rect width="{width}" height="{height}" rx="8" fill="{GROUND}"/>
<rect x="1" y="1" width="{inner}" height="{inner_h}" rx="7" fill="{SURFACE}" stroke="#2e231d"/>
<text x="16" y="30" fill="{EMBER}" font-weight="bold">pepe</text>
<text x="58" y="30" fill="{LABEL}">· compare</text>
<text x="{right}" y="30" fill="{color}" font-weight="bold" font-size="16" text-anchor="end">{glyph} {verdict}</text>
<text x="16" y="52" fill="{LABEL}" font-size="11">{before}</text>
<text x="{right}" y="52" fill="{LABEL}" font-size="11" text-anchor="end">{after}</text>
<line x1="12" y1="60" x2="{line_end}" y2="60" stroke="#2e231d"/>
"##,
            verdict = escape(self.verdict.headline()),
            glyph = level.symbol(),
            inner = width - 2,
            inner_h = height - 2,
            right = width - 16,
            line_end = width - 12,
            before = escape(&file_name(&self.before.file)),
            after = escape(&file_name(&self.after.file)),
        );
        for (i, row) in rows.iter().enumerate() {
            let y = 82 + i * 22;
            out += &format!(
                r##"<text x="16" y="{y}" fill="{LABEL}">{name}</text>
<text x="150" y="{y}" fill="{TEXT}">{a}</text>
<text x="250" y="{y}" fill="{LABEL}">→</text>
<text x="270" y="{y}" fill="{TEXT}">{b}</text>
<text x="{right}" y="{y}" fill="{tone}" text-anchor="end">{word}</text>
"##,
                name = row.name,
                a = escape(&row.before),
                b = escape(&row.after),
                word = escape(&row.word),
                tone = row.tone,
                right = width - 16,
            );
        }
        out += "</svg>\n";
        out
    }
}

/// The file's name without its directories, for the card's small print
fn file_name(path: &str) -> String {
    std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string())
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn pct(p: f64) -> String {
    if p == 0.0 {
        "0%".into()
    } else if p < 1.0 {
        format!("{p:.2}%")
    } else if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

fn ms(ms: f64) -> String {
    format::latency(std::time::Duration::from_micros((ms * 1000.0) as u64))
}

fn rate(v: f64) -> String {
    format!("{} req/s", format::compact(v))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(requests: u64, failed: u64, median_ms: f64, p99_ms: f64, sd_ms: f64, rps: f64) -> Side {
        let doc = serde_json::json!({
            "target": { "method": "GET", "url": "http://x.io/", "concurrency": 8 },
            "summary": {
                "total_requests": requests,
                "failed_requests": failed,
                "requests_per_second": rps,
                "latency": { "avg_ms": median_ms * 1.1, "std_dev_ms": sd_ms,
                             "median_ms": median_ms, "p99_ms": p99_ms },
                "failures": if failed > 0 { serde_json::json!([{ "cause": "503", "count": failed }]) } else { serde_json::json!([]) }
            }
        });
        Side::of_run("run.json".into(), &doc, &doc["summary"])
    }

    #[test]
    fn the_same_build_twice_is_about_the_same() {
        // A 3% wobble on p99, with the spread a run of 5,000 requests has
        let a = run(5000, 0, 30.0, 120.0, 5.0, 260.0);
        let b = run(5000, 0, 30.4, 123.5, 5.0, 256.0);
        let c = compare(&a, &b);
        assert_eq!(c.verdict, Outcome::Same, "{c:#?}");
        assert!(!c.regression);
        assert!(c.changes.iter().all(|x| !x.significant));
        assert!(
            c.findings[0].text.contains("within the usual spread"),
            "{c:#?}"
        );
        let text = c.report();
        assert!(text.starts_with("pepe · compare run.json → run.json\n✔ About the same · 5,000 → 5,000 requests · p99 120.0ms → 123.5ms"), "{text}");
    }

    #[test]
    fn a_slower_p99_is_slower_and_a_regression() {
        let a = run(5000, 0, 30.0, 120.0, 5.0, 260.0);
        let b = run(5000, 0, 31.0, 166.0, 5.0, 252.0);
        let c = compare(&a, &b);
        assert_eq!(c.verdict, Outcome::Slower);
        assert!(c.regression);
        let p99 = c.changes.iter().find(|x| x.metric == "p99_ms").unwrap();
        assert!(p99.significant);
        assert_eq!(p99.change_percent, Some(38.3));
        assert_eq!(c.findings[0].text, "p99 up 38%: 120.0ms → 166.0ms");
        assert_eq!(c.findings[0].level, "Degraded");
        assert!(c.report().contains("▲ Slower"));
    }

    #[test]
    fn faster_when_a_number_moved_the_right_way_and_none_the_wrong() {
        let a = run(5000, 0, 30.0, 120.0, 5.0, 260.0);
        let b = run(5000, 0, 20.0, 80.0, 8.0, 390.0);
        let c = compare(&a, &b);
        assert_eq!(c.verdict, Outcome::Faster);
        assert!(c
            .findings
            .iter()
            .any(|f| f.text == "Throughput up 50%: 260 req/s → 390 req/s"));
    }

    #[test]
    fn failures_outrank_speed() {
        let a = run(5000, 0, 30.0, 120.0, 5.0, 260.0);
        let b = run(5000, 110, 20.0, 80.0, 8.0, 390.0);
        let c = compare(&a, &b);
        assert_eq!(c.verdict, Outcome::Worse);
        assert_eq!(
            c.findings[0].text,
            "Failures appeared: 2.2% of requests, mostly 503"
        );
        assert!(c
            .report()
            .starts_with("pepe · compare run.json → run.json\n✖ Worse"));
        assert!(c.report().contains("· 0% → 2.2% failed"));

        let c = compare(&b, &a);
        assert_eq!(c.verdict, Outcome::Better);
        assert_eq!(c.findings[0].text, "Failures gone: 2.2% → none");
    }

    #[test]
    fn a_small_run_is_given_more_room() {
        // The same 10% move on p99: real with 20,000 requests behind it, not
        // with 200
        let big = compare(
            &run(20000, 0, 30.0, 120.0, 5.0, 260.0),
            &run(20000, 0, 30.0, 132.0, 5.0, 260.0),
        );
        let small = compare(
            &run(200, 0, 30.0, 120.0, 5.0, 260.0),
            &run(200, 0, 30.0, 132.0, 5.0, 260.0),
        );
        assert_eq!(big.verdict, Outcome::Slower);
        assert_eq!(small.verdict, Outcome::Same, "{small:#?}");
        let noise = |c: &Comparison| c.changes[0].noise_percent;
        assert!(noise(&small) > noise(&big));
        assert!(noise(&big) >= FLOOR * 100.0, "never below the floor");
    }

    #[test]
    fn a_tail_that_appeared_is_said() {
        let a = run(5000, 0, 30.0, 60.0, 5.0, 260.0);
        let b = run(5000, 0, 30.0, 200.0, 5.0, 260.0);
        let c = compare(&a, &b);
        assert!(
            c.findings
                .iter()
                .any(|f| f.text == "A long tail is new: p99 is 6.7× the median, was 2.0×"),
            "{c:#?}"
        );
    }

    #[test]
    fn different_tests_are_said_first() {
        let a = run(5000, 0, 30.0, 120.0, 5.0, 260.0);
        let mut b = run(5000, 0, 30.0, 120.0, 5.0, 260.0);
        b.target = Some("POST http://y.io/".into());
        b.concurrency = Some(50);
        let c = compare(&a, &b);
        assert_eq!(
            c.findings[0].text,
            "Different targets: GET http://x.io/, then POST http://y.io/"
        );
        assert_eq!(c.findings[1].text, "Different concurrency: 8, then 50");
        assert_eq!(c.verdict, Outcome::Same);
    }

    #[test]
    fn ramps_compare_their_capacity() {
        let ramp = |rps: f64, holds: u64| {
            let doc = serde_json::json!({
                "requests": 10000, "failed_requests": 0, "holds_concurrency": holds,
                "capacity": { "requests_per_second": rps, "concurrency": 30.0 },
                "steps": []
            });
            Side::of_ramp("ramp.json".into(), &doc)
        };
        let c = compare(&ramp(3000.0, 40), &ramp(2200.0, 30));
        assert_eq!(c.verdict, Outcome::Slower);
        assert_eq!(
            c.findings[0].text,
            "Capacity down 27%: 3.0k req/s → 2.2k req/s"
        );
        assert_eq!(
            c.findings[1].text,
            "The level that held: 40 → 30 concurrent"
        );
        assert!(c.report().contains("capacity 3.0k req/s → 2.2k req/s"));
        // A fit wobbles by itself
        assert_eq!(
            compare(&ramp(3000.0, 40), &ramp(2800.0, 40)).verdict,
            Outcome::Same
        );
    }

    #[test]
    fn reads_reports_and_refuses_what_is_not_one() {
        let dir = std::env::temp_dir().join(format!("pepe-compare-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("x.json");
        std::fs::write(&path, r#"{"hello": 1}"#).unwrap();
        assert!(Side::read(&path).unwrap_err().contains("not a pepe report"));
        std::fs::write(&path, "nope").unwrap();
        assert!(Side::read(&path).unwrap_err().contains("not a JSON report"));
        assert!(Side::read(&dir.join("missing.json"))
            .unwrap_err()
            .contains("can't read"));
        std::fs::write(&path, r#"{"summary": {"total_requests": 10, "failed_requests": 0, "requests_per_second": 5.0, "latency": {"median_ms": 1.0, "p99_ms": 2.0, "avg_ms": 1.1, "std_dev_ms": 0.2}}}"#).unwrap();
        let side = Side::read(&path).unwrap();
        assert_eq!(
            (side.requests, side.p99_ms, side.target),
            (10, Some(2.0), None)
        );
        std::fs::write(
            &path,
            ping(60, 3, 40.0, 44.0, 61.0, 2.0, 10.0, 25.0, 6.0).to_string(),
        )
        .unwrap();
        let side = Side::read(&path).unwrap();
        assert_eq!(side.target.as_deref(), Some("ping https://api.test/"));
        assert_eq!((side.requests, side.failed_percent), (60, 5.0));
        assert_eq!((side.median_ms, side.p99_ms), (Some(44.0), Some(61.0)));
        assert_eq!(
            (side.connect_ms, side.tls_ms, side.ttfb_ms),
            (Some(10.0), Some(25.0), Some(6.0))
        );
        assert_eq!(side.every_s, Some(1.0));
        std::fs::write(&path, r#"{"mode": "ping", "targets": []}"#).unwrap();
        assert!(Side::read(&path).unwrap_err().contains("no target"));
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A ping report of one target: `sent` pings, `lost` of them, with
    /// the latencies and the medians of its phases
    #[allow(clippy::too_many_arguments)]
    fn ping(
        sent: u64,
        lost: u64,
        avg: f64,
        p50: f64,
        p99: f64,
        dns: f64,
        connect: f64,
        tls: f64,
        ttfb: f64,
    ) -> Value {
        let phase = |median: f64| serde_json::json!({"count": sent - lost, "median_ms": median, "p99_ms": median * 2.0, "max_ms": median * 3.0});
        serde_json::json!({
            "schema_version": 1,
            "mode": "ping",
            "every_s": 1.0,
            "targets": [{
                "name": "api",
                "target": "https://api.test/",
                "sent": sent,
                "answered": sent - lost,
                "lost": lost,
                "timeouts": lost,
                "latency_ms": {"avg": avg, "jitter": 2.0, "p50": p50, "p99": p99, "min": p50 * 0.9, "max": p99},
                "phases_ms": {"dns": phase(dns), "connect": phase(connect), "tls_full": phase(tls), "tls_resumed": null, "ttfb": phase(ttfb), "download": phase(0.5)},
                "failures": if lost > 0 { serde_json::json!({"no answer in 5s": lost}) } else { serde_json::json!({}) }
            }]
        })
    }

    #[test]
    fn two_ping_reports_compare_their_phases() {
        let a = Side::of_ping(
            "before.json".into(),
            &ping(600, 0, 40.0, 40.0, 50.0, 2.0, 10.0, 25.0, 6.0),
        )
        .unwrap();
        // The server moved further away: connecting and the handshake
        // both take longer, the server itself is as it was
        let b = Side::of_ping(
            "after.json".into(),
            &ping(600, 0, 60.0, 60.0, 72.0, 2.0, 25.0, 40.0, 6.0),
        )
        .unwrap();
        let c = compare(&a, &b);
        assert_eq!(c.verdict, Outcome::Slower);
        let texts: Vec<&str> = c.findings.iter().map(|f| f.text.as_str()).collect();
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("Connect up 150%: 10.00ms → 25.00ms")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.starts_with("TLS handshake up 60%")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("First byte within the usual spread")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.starts_with("Median up 50%")),
            "{texts:?}"
        );
        assert!(!texts.iter().any(|t| t.contains("Throughput")), "{texts:?}");
        assert_eq!(c.badge.message, "slower · connect +150%");
        assert!(
            c.report()
                .contains("pepe · compare before.json → after.json\n▲ Slower · 600 → 600 pings"),
            "{}",
            c.report()
        );
        // Lost pings are failures, said as lost
        let lossy = Side::of_ping(
            "lossy.json".into(),
            &ping(600, 30, 40.0, 40.0, 50.0, 2.0, 10.0, 25.0, 6.0),
        )
        .unwrap();
        let c = compare(&a, &lossy);
        assert_eq!(c.verdict, Outcome::Worse);
        assert!(c.report().contains("0% → 5.0% lost"), "{}", c.report());
        assert!(
            c.findings[0]
                .text
                .starts_with("Failures appeared: 5.0% of requests, mostly no answer in 5s"),
            "{}",
            c.findings[0].text
        );
        // Different intervals are said first
        let mut faster = ping(600, 0, 40.0, 40.0, 50.0, 2.0, 10.0, 25.0, 6.0);
        faster["every_s"] = serde_json::json!(0.2);
        let f = Side::of_ping("fast.json".into(), &faster).unwrap();
        let c = compare(&a, &f);
        assert_eq!(
            c.findings[0].text,
            "Different intervals: a ping every 1s, then every 0.2s"
        );
    }

    #[test]
    fn the_badge_says_the_verdict_and_what_moved_most() {
        let slower = compare(
            &run(5000, 0, 30.0, 120.0, 5.0, 260.0),
            &run(5000, 0, 31.0, 166.0, 5.0, 252.0),
        );
        assert_eq!(
            slower.badge,
            Badge {
                label: "pepe",
                message: "slower · p99 +38%".into(),
                color: "f0cf5a"
            }
        );
        let same = compare(
            &run(5000, 0, 30.0, 120.0, 5.0, 260.0),
            &run(5000, 0, 30.0, 121.0, 5.0, 260.0),
        );
        assert_eq!(
            (same.badge.message.as_str(), same.badge.color),
            ("about the same", "9ccf6a")
        );
        let worse = compare(
            &run(5000, 0, 30.0, 120.0, 5.0, 260.0),
            &run(5000, 110, 30.0, 120.0, 5.0, 260.0),
        );
        assert_eq!(
            (worse.badge.message.as_str(), worse.badge.color),
            ("worse · 2.2% failed", "ff5a6e")
        );
        let faster = compare(
            &run(5000, 0, 30.0, 120.0, 5.0, 260.0),
            &run(5000, 0, 20.0, 110.0, 5.0, 390.0),
        );
        assert_eq!(faster.badge.message, "faster · req/s +50%");
    }

    #[test]
    fn the_card_is_an_svg_of_the_verdict() {
        let mut a = run(5000, 0, 30.0, 120.0, 5.0, 260.0);
        a.file = "reports/main & co.json".into();
        let b = run(5000, 0, 31.0, 166.0, 5.0, 252.0);
        let c = compare(&a, &b);
        let svg = c.svg();
        assert!(svg.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\""));
        assert!(svg.trim_end().ends_with("</svg>"));
        assert_eq!(svg.matches("<text").count(), svg.matches("</text>").count());
        assert!(svg.contains(">▲ Slower</text>"));
        assert!(
            svg.contains("main &amp; co.json"),
            "the file's name, escaped, without its directory"
        );
        assert!(
            svg.contains(">p99</text>")
                && svg.contains(">120.0ms</text>")
                && svg.contains(">166.0ms</text>")
        );
        assert!(svg.contains(">▲ 38%</text>"), "{svg}");
        assert!(
            svg.contains(">±5%</text>"),
            "a number within the spread shows the spread"
        );
        assert!(svg.contains("fill=\"#f0cf5a\""), "the verdict's colour");
        assert!(!svg.contains("http://x.io"), "no target on the card");
    }

    #[test]
    fn json_has_the_verdict_and_every_change() {
        let c = compare(
            &run(5000, 0, 30.0, 120.0, 5.0, 260.0),
            &run(5000, 0, 31.0, 166.0, 5.0, 252.0),
        );
        let json: Value = serde_json::from_str(&c.to_json().unwrap()).unwrap();
        assert_eq!(json["verdict"], "slower");
        assert_eq!(json["regression"], true);
        assert_eq!(json["changes"][0]["metric"], "p99_ms");
        assert_eq!(json["changes"][0]["significant"], true);
        assert_eq!(json["before"]["p99_ms"], 120.0);
        assert_eq!(json["after"]["target"], "GET http://x.io/");
        assert_eq!(json["findings"][0]["level"], "Degraded");
        assert_eq!(json["badge"]["message"], "slower · p99 +38%");
    }
}
