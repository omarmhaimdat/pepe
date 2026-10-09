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
    pub verdict: Outcome,
    pub regression: bool,
    pub before: Side,
    pub after: Side,
    pub changes: Vec<Change>,
    pub findings: Vec<Finding>,
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
    /// Relative spread of one request's latency: std dev over the mean
    #[serde(skip)]
    cv: f64,
    /// The failure most often seen
    #[serde(skip)]
    main_failure: Option<String>,
}

impl Side {
    /// A run report (`pepe --json`, `--snapshot`, API, flow or replay),
    /// or a ramp's
    pub fn read(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("can't read {}: {e}", path.display()))?;
        let doc: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not a JSON report: {e}", path.display()))?;
        let file = path.display().to_string();
        if let Some(summary) = doc.get("summary") {
            return Ok(Self::of_run(file, &doc, summary));
        }
        if doc.get("steps").is_some() {
            return Ok(Self::of_ramp(file, &doc));
        }
        Err(format!(
            "{file} is not a pepe report: no `summary` (a run) and no `steps` (a ramp)"
        ))
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
    Comparison {
        verdict: outcome,
        regression: outcome.is_regression(),
        before: before.clone(),
        after: after.clone(),
        changes,
        findings,
    }
}

impl Comparison {
    /// The report left in the shell, in the end-of-run report's shape
    pub fn report(&self) -> String {
        let level = self.verdict.level();
        let mut head = vec![format!(
            "{} → {} requests",
            format::count(self.before.requests),
            format::count(self.after.requests)
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
                "{} → {} failed",
                pct(self.before.failed_percent),
                pct(self.after.failed_percent)
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
}

fn round(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

fn pct(p: f64) -> String {
    if p < 1.0 {
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
        assert!(c.report().contains("· 0.00% → 2.2% failed"));

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
        std::fs::remove_dir_all(&dir).unwrap();
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
    }
}
