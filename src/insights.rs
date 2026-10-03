//! Plain-language findings about a run, for the end-of-run verdict

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
        notes.extend(causes(m, Level::Failing));
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
        notes.extend(causes(m, level));
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

    // What the server says its own share of the time was
    let timing = m.server_timing();
    if timing.total().count() > 0 {
        let measured = m.percentile(50.0).as_micros() as f64;
        let server = timing.total().percentile(50.0) as f64;
        if measured > 0.0 {
            let share = (server / measured * 100.0).min(100.0);
            let mostly = timing
                .largest()
                .filter(|_| timing.segments().len() > 1)
                .map_or_else(String::new, |s| format!(", mostly {}", s.name));
            notes.push(note(
                Level::Healthy,
                format!(
                    "The server says {} of the {} median was its own{mostly}",
                    pct(share),
                    ms(measured)
                ),
            ));
        }
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
/// Causes of failure shown in the verdict, and how much of a body
const MAX_CAUSE_NOTES: usize = 2;
const EXAMPLE_CHARS: usize = 72;

/// The main causes of failure, each with the first body the target sent
/// for it, so "503" comes with what the 503 said
fn causes(m: &Metrics, level: Level) -> Vec<Note> {
    m.failures()
        .top()
        .into_iter()
        .take(MAX_CAUSE_NOTES)
        .map(|(cause, c)| {
            let mut text = format!("{cause} ×{}", format::count(c.count));
            if let Some(example) = &c.example {
                let mut shown: String = example.chars().take(EXAMPLE_CHARS).collect();
                if example.chars().count() > EXAMPLE_CHARS {
                    shown.push('…');
                }
                text.push_str(&format!(": {shown}"));
            }
            Note { level, text }
        })
        .collect()
}

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

    #[test]
    fn empty_run_is_failing() {
        assert_eq!(
            verdict(&Metrics::default(), &[], false).level,
            Level::Failing
        );
    }
}
