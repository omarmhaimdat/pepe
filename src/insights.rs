//! Plain-language findings about a run, for the end-of-run verdict

use std::collections::VecDeque;
use std::time::Duration;

use crate::metrics::{Histogram, Metrics};
use crate::timeline::{bin_floor_us, bin_of, Sample, BINS, BINS_PER_DECADE};
use crate::ui::format;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    Healthy,
    Degraded,
    Failing,
}

impl Level {
    pub fn headline(self) -> &'static str {
        match self {
            Level::Healthy => "Healthy",
            Level::Degraded => "Degraded",
            Level::Failing => "Failing",
        }
    }

    pub fn symbol(self) -> &'static str {
        match self {
            Level::Healthy => "✔",
            Level::Degraded => "▲",
            Level::Failing => "✖",
        }
    }
}

/// One finding. `level` says how much it matters.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub level: Level,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    /// The worst level among the notes
    pub level: Level,
    pub notes: Vec<Note>,
}

/// Something that changed during the run, said as it happened
#[derive(Debug, Clone, PartialEq)]
pub struct Anomaly {
    /// Seconds since the run started, at the end of the second it was seen in
    pub at: Duration,
    pub kind: AnomalyKind,
    pub text: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnomalyKind {
    Latency,
    Throughput,
    Errors,
}

impl AnomalyKind {
    pub fn name(self) -> &'static str {
        match self {
            AnomalyKind::Latency => "latency",
            AnomalyKind::Throughput => "throughput",
            AnomalyKind::Errors => "errors",
        }
    }
}

/// Seconds a second is compared against: the median of these, before it
const BASELINE_SECS: usize = 30;
/// Fewer than this many seconds of history and nothing is said
const MIN_HISTORY: usize = 10;
/// Once something is said about a kind, it isn't said again for this long
const COOLDOWN: Duration = Duration::from_secs(30);
/// p99 this many times the baseline, and at least this much slower, is a jump
const LATENCY_JUMP: f64 = 3.0;
const LATENCY_FLOOR_MS: f64 = 5.0;
/// Throughput at or below this share of the baseline is a fall
const THROUGHPUT_FALL: f64 = 0.5;
/// Errors reaching this share of a second's requests, from under 1%, is a rise
const ERROR_SHARE: f64 = 0.05;

/// Watches the per-second timeline for things worth a word as they happen:
/// a latency jump, a throughput fall, errors appearing. Each second is
/// judged against the median of the thirty before it.
#[derive(Debug, Default)]
pub struct Watch {
    /// `at` of the last second judged
    seen: f64,
    /// `at` of the last word on each kind, for the cooldown
    said: [Option<f64>; 3],
    /// Say nothing about seconds ending before this: a resume leaves a
    /// partial second that looks like a fall
    quiet_until: f64,
}

impl Watch {
    /// Judge the seconds closed since the last call. `paused`: the run is
    /// paused now, so a quiet second is expected.
    pub fn observe(&mut self, samples: &VecDeque<Sample>, paused: bool) -> Vec<Anomaly> {
        let mut found = Vec::new();
        let Some(first_new) = samples.iter().position(|s| s.at > self.seen) else {
            return found;
        };
        for i in first_new..samples.len() {
            let s = samples[i];
            self.seen = s.at;
            if paused || s.at < self.quiet_until {
                continue;
            }
            let base: Vec<Sample> = samples
                .range(i.saturating_sub(BASELINE_SECS)..i)
                .filter(|b| b.rps > 0.0)
                .copied()
                .collect();
            if base.len() < MIN_HISTORY {
                continue;
            }
            let median = |pick: fn(&Sample) -> f64| {
                let mut v: Vec<f64> = base.iter().map(pick).collect();
                v.sort_by(f64::total_cmp);
                v[v.len() / 2]
            };
            let when = format::span(Duration::from_secs_f64(s.at));

            let p99_base = median(|b| b.p99_ms);
            if s.rps > 0.0
                && s.p99_ms >= LATENCY_JUMP * p99_base
                && s.p99_ms - p99_base >= LATENCY_FLOOR_MS
            {
                self.say(
                    &mut found,
                    s.at,
                    AnomalyKind::Latency,
                    format!(
                        "p99 jumped {:.1}× to {} at {when}",
                        s.p99_ms / p99_base,
                        format::latency_short((s.p99_ms * 1000.0) as u64)
                    ),
                );
            }

            let rps_base = median(|b| b.rps);
            if rps_base >= 10.0 && s.rps <= THROUGHPUT_FALL * rps_base {
                self.say(
                    &mut found,
                    s.at,
                    AnomalyKind::Throughput,
                    format!(
                        "throughput fell {}% to {} req/s at {when}",
                        // Floored: a fall to a trickle reads 99%, not 100%
                        ((1.0 - s.rps / rps_base) * 100.0).floor(),
                        format::compact(s.rps)
                    ),
                );
            }

            let share = if s.rps > 0.0 { s.errors / s.rps } else { 0.0 };
            let share_base = median(|b| b.errors / b.rps.max(f64::EPSILON));
            if share >= ERROR_SHARE && share_base < 0.01 {
                self.say(
                    &mut found,
                    s.at,
                    AnomalyKind::Errors,
                    format!("errors rose to {} at {when}", pct(share * 100.0)),
                );
            }
        }
        found
    }

    /// The run was resumed: give it a moment before judging again
    pub fn resumed(&mut self, now: Duration) {
        self.quiet_until = now.as_secs_f64() + 2.0;
    }

    fn say(&mut self, found: &mut Vec<Anomaly>, at: f64, kind: AnomalyKind, text: String) {
        let slot = &mut self.said[kind as usize];
        if slot.is_some_and(|last| at - last < COOLDOWN.as_secs_f64()) {
            return;
        }
        *slot = Some(at);
        found.push(Anomaly {
            at: Duration::from_secs_f64(at),
            kind,
            text,
        });
    }
}

/// Error share (percent) at which a run is failing, or merely degraded
const FAILING_ERROR_PCT: f64 = 5.0;
const DEGRADED_ERROR_PCT: f64 = 0.5;
/// p99 / p50 at which the tail is worth pointing out, and at which it hurts
const LONG_TAIL: f64 = 5.0;
const BAD_TAIL: f64 = 10.0;
/// Change between the start and the end of a run worth pointing out
const TREND: f64 = 0.25;

pub fn verdict(m: &Metrics, samples: &[Sample], interrupted: bool) -> Verdict {
    let mut notes = Vec::new();
    let note = |level, text: String| Note { level, text };

    if m.total == 0 {
        notes.push(note(Level::Failing, "No request completed".into()));
        return finish(notes);
    }

    // Errors
    let error_pct = m.error_rate();
    if m.success == 0 {
        notes.push(note(Level::Failing, "Every request failed".into()));
    } else if error_pct > 0.0 {
        let level = match error_pct {
            p if p >= FAILING_ERROR_PCT => Level::Failing,
            p if p >= DEGRADED_ERROR_PCT => Level::Degraded,
            _ => Level::Healthy,
        };
        notes.push(note(
            level,
            format!("{} failed, mostly {}", pct(error_pct), main_failure(m)),
        ));
    } else {
        notes.push(note(Level::Healthy, "No failed requests".into()));
    }

    // Latency shape
    let p50 = m.percentile(50.0).as_micros() as f64;
    let p99 = m.percentile(99.0).as_micros() as f64;
    if let Some(modes) = bimodal(m.latency()) {
        notes.push(note(
            Level::Degraded,
            format!(
                "Two latency groups: most near {}, {} near {}",
                ms(modes.low_us),
                pct(modes.high_share * 100.0),
                ms(modes.high_us)
            ),
        ));
    } else if p50 > 0.0 && p99 / p50 >= LONG_TAIL {
        let level = if p99 / p50 >= BAD_TAIL {
            Level::Degraded
        } else {
            Level::Healthy
        };
        notes.push(note(
            level,
            format!("Long tail: p99 is {:.1}× the median", p99 / p50),
        ));
    } else if p50 > 0.0 {
        notes.push(note(
            Level::Healthy,
            format!("Tight latency: p99 is {:.1}× the median", p99 / p50),
        ));
    }

    // Trends. The first and last buckets are ramp-up and drain; skip them.
    let steady = samples
        .get(1..samples.len().saturating_sub(1))
        .unwrap_or_default();
    if steady.len() >= 3 {
        let rps: Vec<f64> = steady.iter().map(|s| s.rps).collect();
        let rps_change = change(&rps);
        let (mean, spread) = mean_and_spread(&rps);
        if rps_change <= -TREND {
            notes.push(note(
                Level::Degraded,
                format!(
                    "Throughput fell {} from start to end",
                    pct(-rps_change * 100.0)
                ),
            ));
        } else if rps_change >= TREND {
            notes.push(note(
                Level::Healthy,
                format!(
                    "Throughput rose {} from start to end",
                    pct(rps_change * 100.0)
                ),
            ));
        } else if spread > TREND {
            let (lo, hi) = rps
                .iter()
                .fold((f64::MAX, 0.0f64), |(lo, hi), &v| (lo.min(v), hi.max(v)));
            notes.push(note(
                Level::Healthy,
                format!(
                    "Throughput swung between {} and {} req/s",
                    rate(lo),
                    rate(hi)
                ),
            ));
        } else {
            notes.push(note(
                Level::Healthy,
                format!(
                    "Steady throughput: {} req/s ±{:.0}%",
                    rate(mean),
                    spread * 100.0
                ),
            ));
        }

        let p50s: Vec<f64> = steady.iter().map(|s| s.p50_ms).collect();
        let drift = change(&p50s);
        if drift >= 1.0 {
            notes.push(note(
                Level::Degraded,
                format!("Latency climbed {:.1}× over the run", 1.0 + drift),
            ));
        } else if drift >= TREND {
            notes.push(note(
                Level::Healthy,
                format!("Latency crept up {} over the run", pct(drift * 100.0)),
            ));
        }
    }

    if interrupted {
        notes.push(note(Level::Healthy, "Stopped early".into()));
    }
    finish(notes)
}

fn finish(notes: Vec<Note>) -> Verdict {
    let level = notes
        .iter()
        .map(|n| n.level)
        .max()
        .unwrap_or(Level::Healthy);
    Verdict { level, notes }
}

/// What most failures were: a status code, timeouts or connection errors
fn main_failure(m: &Metrics) -> String {
    let worst_status = m
        .status_codes
        .iter()
        .filter(|(code, _)| !(200..300).contains(*code))
        .max_by_key(|(_, &n)| n)
        .map(|(&code, &n)| (n, format!("HTTP {code}")));
    [
        worst_status,
        Some((m.timeouts, "timeouts".to_string())),
        Some((m.errors, "connection errors".to_string())),
    ]
    .into_iter()
    .flatten()
    .max_by_key(|(n, _)| *n)
    .map(|(_, what)| what)
    .unwrap_or_default()
}

/// Relative change between the first and last third of `values`
fn change(values: &[f64]) -> f64 {
    let third = (values.len() / 3).max(1);
    let mean = |v: &[f64]| v.iter().sum::<f64>() / v.len() as f64;
    let (first, last) = (
        mean(&values[..third]),
        mean(&values[values.len() - third..]),
    );
    if first > 0.0 {
        last / first - 1.0
    } else {
        0.0
    }
}

/// Mean and coefficient of variation
fn mean_and_spread(values: &[f64]) -> (f64, f64) {
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let var = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    (mean, if mean > 0.0 { var.sqrt() / mean } else { 0.0 })
}

/// Two separate latency clusters
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modes {
    pub low_us: f64,
    pub high_us: f64,
    /// Share of requests in the slower group (0-1)
    pub high_share: f64,
}

/// Minimum share of requests a latency group needs to count
const MODE_MIN_SHARE: f64 = 0.02;

/// Find two latency peaks at least 3× apart with a clear valley between them
pub fn bimodal(h: &Histogram) -> Option<Modes> {
    let total = h.count() as f64;
    if total < 100.0 {
        return None;
    }
    let mut bins = [0u64; BINS];
    for (value, n) in h.buckets() {
        bins[bin_of(value)] += n;
    }
    // Smooth over ±1 bin so single-bin noise isn't read as a peak
    let smooth: Vec<u64> = (0..BINS)
        .map(|i| bins[i.saturating_sub(1)..(i + 2).min(BINS)].iter().sum())
        .collect();

    let peaks: Vec<usize> = (0..BINS)
        .filter(|&i| {
            let left = if i == 0 { 0 } else { smooth[i - 1] };
            let right = smooth.get(i + 1).copied().unwrap_or(0);
            smooth[i] > left && smooth[i] >= right && smooth[i] as f64 >= total * MODE_MIN_SHARE
        })
        .collect();
    let mut by_height = peaks.clone();
    by_height.sort_by_key(|&i| std::cmp::Reverse(smooth[i]));
    let (a, b) = match by_height[..] {
        [x, y, ..] => (x.min(y), x.max(y)),
        _ => return None,
    };

    // At least 3× apart
    let min_gap = (3f64.log10() * BINS_PER_DECADE as f64).ceil() as usize;
    if b - a < min_gap {
        return None;
    }
    let (valley, &depth) = smooth[a..=b]
        .iter()
        .enumerate()
        .min_by_key(|(_, &v)| v)
        .map(|(i, v)| (a + i, v))?;
    if depth as f64 > 0.5 * smooth[a].min(smooth[b]) as f64 {
        return None;
    }

    let high: u64 = bins[valley..].iter().sum();
    let high_share = high as f64 / total;
    if !(MODE_MIN_SHARE..=1.0 - MODE_MIN_SHARE).contains(&high_share) {
        return None;
    }
    let center = |bin: usize| (bin_floor_us(bin) * bin_floor_us(bin + 1)).sqrt();
    Some(Modes {
        low_us: center(a),
        high_us: center(b),
        high_share,
    })
}

fn pct(p: f64) -> String {
    if p < 1.0 {
        format!("{p:.2}%")
    } else {
        format!("{p:.0}%")
    }
}

fn ms(us: f64) -> String {
    format::latency(std::time::Duration::from_micros(us as u64))
}

fn rate(v: f64) -> String {
    format::compact(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::{ErrorKind, ResponseStats};
    use reqwest::StatusCode;
    use std::time::Duration;

    fn stat(us: u64, status: Option<u16>) -> ResponseStats {
        ResponseStats {
            duration: Duration::from_micros(us),
            status_code: status.map(|s| StatusCode::from_u16(s).unwrap()),
            error: status.is_none().then_some(ErrorKind::Timeout),
            ..Default::default()
        }
    }

    fn metrics(latencies: impl IntoIterator<Item = u64>) -> Metrics {
        let mut m = Metrics::default();
        for us in latencies {
            m.record(&stat(us, Some(200)));
        }
        m
    }

    fn samples(rps: &[f64], p50: &[f64]) -> Vec<Sample> {
        rps.iter()
            .zip(p50)
            .enumerate()
            .map(|(i, (&rps, &p50_ms))| Sample {
                at: i as f64 + 1.0,
                rps,
                p50_ms,
                ..Default::default()
            })
            .collect()
    }

    #[test]
    fn healthy_run_reads_healthy() {
        let m = metrics((0..1_000).map(|i| 5_000 + i % 1_000));
        let s = samples(&[900.0, 1_000.0, 1_010.0, 990.0, 1_000.0, 400.0], &[5.5; 6]);
        let v = verdict(&m, &s, false);
        assert_eq!(v.level, Level::Healthy, "{v:?}");
        assert_eq!(v.notes[0].text, "No failed requests");
        assert!(v
            .notes
            .iter()
            .any(|n| n.text.starts_with("Steady throughput: 1.0k")));
    }

    #[test]
    fn errors_set_the_level() {
        let mut m = metrics((0..90).map(|_| 1_000));
        for _ in 0..7 {
            m.record(&stat(1_000, Some(503)));
        }
        for _ in 0..3 {
            m.record(&stat(1_000, None));
        }
        let v = verdict(&m, &[], false);
        assert_eq!(v.level, Level::Failing);
        assert_eq!(v.notes[0].text, "10% failed, mostly HTTP 503");
    }

    #[test]
    fn finds_two_latency_groups() {
        // 95% near 7ms, 5% near 50ms: the shape from a real run
        let m = metrics((0..2_000).map(|i| {
            if i % 20 == 0 {
                48_000 + (i % 7) * 1_000
            } else {
                6_500 + (i % 11) * 100
            }
        }));
        let modes = bimodal(m.latency()).expect("bimodal");
        assert!((6_000.0..8_000.0).contains(&modes.low_us), "{modes:?}");
        assert!((45_000.0..56_000.0).contains(&modes.high_us), "{modes:?}");
        assert!((0.04..0.06).contains(&modes.high_share), "{modes:?}");
        let v = verdict(&m, &[], false);
        assert!(v.notes[1].text.starts_with("Two latency groups"), "{v:?}");
    }

    #[test]
    fn one_wide_group_is_not_bimodal() {
        assert_eq!(
            bimodal(metrics((0..5_000).map(|i| 1_000 + i * 20)).latency()),
            None
        );
        assert_eq!(
            bimodal(metrics([1_000, 50_000]).latency()),
            None,
            "too few samples"
        );
    }

    #[test]
    fn trends_ignore_ramp_up_and_drain() {
        let m = metrics((0..1_000).map(|_| 5_000));
        // Ramp-up and drain at the ends, flat in between
        let flat = samples(&[10.0, 1_000.0, 1_000.0, 1_000.0, 1_000.0, 5.0], &[5.0; 6]);
        assert_eq!(verdict(&m, &flat, false).level, Level::Healthy);

        let falling = samples(
            &[1_000.0, 1_000.0, 1_000.0, 800.0, 500.0, 500.0, 500.0],
            &[5.0, 5.0, 5.0, 8.0, 12.0, 12.0, 12.0],
        );
        let v = verdict(&m, &falling, true);
        assert_eq!(v.level, Level::Degraded);
        let texts: Vec<&str> = v.notes.iter().map(|n| n.text.as_str()).collect();
        assert!(
            texts.contains(&"Throughput fell 50% from start to end"),
            "{texts:?}"
        );
        assert!(
            texts.contains(&"Latency climbed 2.4× over the run"),
            "{texts:?}"
        );
        assert_eq!(texts.last(), Some(&"Stopped early"));
    }

    fn seconds(n: usize, rps: f64, p99: f64, errors: f64) -> VecDeque<Sample> {
        (1..=n)
            .map(|i| Sample {
                at: i as f64,
                rps,
                errors,
                p50_ms: p99 / 2.0,
                p90_ms: p99 * 0.8,
                p99_ms: p99,
            })
            .collect()
    }

    #[test]
    fn a_jump_is_said_once_and_not_before_there_is_history() {
        let mut watch = Watch::default();
        let mut s = seconds(9, 1_000.0, 10.0, 0.0);
        assert!(watch.observe(&s, false).is_empty());
        // Tenth second: still no baseline of ten before it
        s.push_back(Sample {
            at: 10.0,
            p99_ms: 100.0,
            ..s[0]
        });
        assert!(watch.observe(&s, false).is_empty(), "no history yet");
        for i in 11..=25 {
            s.push_back(Sample {
                at: i as f64,
                ..s[0]
            });
        }
        assert!(watch.observe(&s, false).is_empty(), "flat is quiet");
        s.push_back(Sample {
            at: 26.0,
            p99_ms: 45.0,
            ..s[0]
        });
        let found = watch.observe(&s, false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].kind, AnomalyKind::Latency);
        assert_eq!(found[0].text, "p99 jumped 4.5× to 45ms at 26s");
        // Still slow five seconds later: said already
        for i in 27..=31 {
            s.push_back(Sample {
                at: i as f64,
                p99_ms: 45.0,
                ..s[0]
            });
        }
        assert!(watch.observe(&s, false).is_empty(), "within the cooldown");
    }

    #[test]
    fn falls_and_errors_are_said_but_not_while_paused() {
        let mut watch = Watch::default();
        let mut s = seconds(20, 1_000.0, 10.0, 0.0);
        assert!(watch.observe(&s, false).is_empty());
        s.push_back(Sample {
            at: 21.0,
            rps: 300.0,
            ..s[0]
        });
        let found = watch.observe(&s, false);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "throughput fell 70% to 300 req/s at 21s");
        s.push_back(Sample {
            at: 22.0,
            rps: 1_000.0,
            errors: 80.0,
            ..s[0]
        });
        let found = watch.observe(&s, false);
        assert_eq!(found[0].kind, AnomalyKind::Errors);
        assert!(
            found[0].text.starts_with("errors rose to 8%"),
            "{}",
            found[0].text
        );
        // A quiet second while paused is expected, and so is the one after resuming
        s.push_back(Sample {
            at: 23.0,
            rps: 0.0,
            ..s[0]
        });
        assert!(watch.observe(&s, true).is_empty());
        watch.resumed(Duration::from_secs(23));
        s.push_back(Sample {
            at: 24.0,
            rps: 100.0,
            ..s[0]
        });
        assert!(
            watch.observe(&s, false).is_empty(),
            "the second after a resume"
        );
    }

    #[test]
    fn empty_run_is_failing() {
        assert_eq!(
            verdict(&Metrics::default(), &[], false).level,
            Level::Failing
        );
    }
}
