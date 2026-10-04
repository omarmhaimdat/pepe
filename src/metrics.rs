use std::collections::HashMap;
use std::time::Duration;

use crate::cache::CacheCategory;
use crate::response::{ErrorKind, ResponseStats};

/// Distinct `Server-Timing` names kept; the rest are counted, not timed
pub const MAX_SEGMENTS: usize = 8;
/// Responses the slowest list holds
pub const SLOWEST_KEPT: usize = 5;

/// Aggregated results for a whole run. Everything is updated incrementally,
/// so memory stays flat no matter how many requests a run sends.
#[derive(Debug, Default, Clone)]
pub struct Metrics {
    /// Completed requests, whatever the outcome
    pub total: u64,
    /// 2xx responses
    pub success: u64,
    /// Responses with a non-2xx status
    pub failed: u64,
    /// Requests that hit the timeout
    pub timeouts: u64,
    /// Requests that failed without a response (connect, TLS, ...)
    pub errors: u64,
    /// Response body bytes received
    pub bytes: u64,
    pub status_codes: HashMap<u16, u64>,
    pub cache_hits: u64,
    failures: Failures,
    latency: Histogram,
    /// Time to the response headers, and from there to the end of the body
    first_byte: Histogram,
    download: Histogram,
    sum_us: f64,
    sum_sq_us: f64,
    dns_lookup_sum: Duration,
    dns_samples: u64,
    server_timing: ServerTimings,
}

impl Metrics {
    pub fn record(&mut self, stat: &ResponseStats) {
        self.total += 1;
        match (stat.status_code, stat.error) {
            (Some(code), _) => {
                *self.status_codes.entry(code.as_u16()).or_insert(0) += 1;
                if code.is_success() {
                    self.success += 1;
                } else {
                    self.failed += 1;
                }
            }
            (None, Some(ErrorKind::Timeout)) => self.timeouts += 1,
            (None, _) => self.errors += 1,
        }
        self.bytes += stat.body_bytes;
        if let Some(cause) = stat.failure_cause() {
            self.failures.count(cause, stat.preview_text());
        }

        let us = stat.duration.as_micros() as u64;
        self.latency.record(us);
        if let Some(ttfb) = stat.ttfb {
            self.first_byte.record(ttfb.as_micros() as u64);
            self.download
                .record(stat.duration.saturating_sub(ttfb).as_micros() as u64);
        }
        self.sum_us += us as f64;
        self.sum_sq_us += (us as f64) * (us as f64);

        if let Some((lookup, _)) = stat.dns_times {
            self.dns_lookup_sum += lookup;
            self.dns_samples += 1;
        }

        if let Some(ref cache_status) = stat.cache_status {
            if CacheCategory::from_cache_status(cache_status) == CacheCategory::Hit {
                self.cache_hits += 1;
            }
        }
        if let Some(timing) = &stat.server_timing {
            self.server_timing.record(timing);
        }
    }

    /// What the target's `Server-Timing` headers added up to
    pub fn server_timing(&self) -> &ServerTimings {
        &self.server_timing
    }

    /// Latency at percentile `q` (0-100), or zero when nothing was recorded
    pub fn percentile(&self, q: f64) -> Duration {
        Duration::from_micros(self.latency.percentile(q))
    }

    pub fn min(&self) -> Duration {
        Duration::from_micros(self.latency.min)
    }

    pub fn max(&self) -> Duration {
        Duration::from_micros(self.latency.max)
    }

    /// Share of requests (0-1) that were faster than `d`
    pub fn rank(&self, d: Duration) -> f64 {
        self.latency.fraction_below(d.as_micros() as u64)
    }

    pub fn latency(&self) -> &Histogram {
        &self.latency
    }

    /// Latency at percentile `q` of one phase of the requests that got a
    /// response: the wait for the headers, or the body after them
    pub fn phase(&self, phase: Phase) -> &Histogram {
        match phase {
            Phase::FirstByte => &self.first_byte,
            Phase::Download => &self.download,
        }
    }

    /// Failed requests by cause
    pub fn failures(&self) -> &Failures {
        &self.failures
    }

    /// Requests that did not get a 2xx response, in percent
    pub fn error_rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        (self.total - self.success) as f64 / self.total as f64 * 100.0
    }

    pub fn mean(&self) -> Duration {
        if self.total == 0 {
            return Duration::ZERO;
        }
        Duration::from_micros((self.sum_us / self.total as f64) as u64)
    }

    /// Population standard deviation of latency
    pub fn std_dev(&self) -> Duration {
        if self.total == 0 {
            return Duration::ZERO;
        }
        let n = self.total as f64;
        let mean = self.sum_us / n;
        let variance = (self.sum_sq_us / n - mean * mean).max(0.0);
        Duration::from_micros(variance.sqrt() as u64)
    }

    pub fn avg_dns_lookup(&self) -> Duration {
        if self.dns_samples == 0 {
            return Duration::ZERO;
        }
        self.dns_lookup_sum / self.dns_samples as u32
    }

    /// Share of requests served from a cache, in percent
    pub fn cache_hit_rate(&self) -> f64 {
        if self.total == 0 {
            return 0.0;
        }
        self.cache_hits as f64 / self.total as f64 * 100.0
    }

    /// Requests per second over `elapsed`
    pub fn rps(&self, elapsed: Duration) -> f64 {
        per_second(self.total as f64, elapsed)
    }

    /// Body bytes per second over `elapsed`
    pub fn throughput(&self, elapsed: Duration) -> f64 {
        per_second(self.bytes as f64, elapsed)
    }
}

/// `Server-Timing` over the run: each named segment's durations, and the
/// server's total per response, to hold against the latency pepe measured
#[derive(Debug, Default, Clone)]
pub struct ServerTimings {
    segments: Vec<Segment>,
    /// Sum of a response's `dur`s, per response that had one
    total: Histogram,
    /// Responses that carried the header at all
    responses: u64,
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub name: Box<str>,
    /// Microseconds, like the latency histogram
    pub durations: Histogram,
    /// Entries seen, with or without a `dur`
    pub count: u64,
}

impl ServerTimings {
    fn record(&mut self, timing: &[crate::trace::ServerTiming]) {
        self.responses += 1;
        let mut total_ms = None;
        for entry in timing {
            if let Some(ms) = entry.dur_ms.filter(|ms| ms.is_finite() && *ms >= 0.0) {
                *total_ms.get_or_insert(0.0) += ms;
            }
            let index = match self.segments.iter().position(|s| s.name == entry.name) {
                Some(index) => index,
                None if self.segments.len() < MAX_SEGMENTS => {
                    self.segments.push(Segment {
                        name: entry.name.clone(),
                        durations: Histogram::default(),
                        count: 0,
                    });
                    self.segments.len() - 1
                }
                None => continue,
            };
            let segment = &mut self.segments[index];
            segment.count += 1;
            if let Some(ms) = entry.dur_ms.filter(|ms| ms.is_finite() && *ms >= 0.0) {
                segment.durations.record((ms * 1000.0) as u64);
            }
        }
        if let Some(ms) = total_ms {
            self.total.record((ms * 1000.0) as u64);
        }
    }

    pub fn is_empty(&self) -> bool {
        self.responses == 0
    }

    /// Responses that carried a `Server-Timing` header
    pub fn responses(&self) -> u64 {
        self.responses
    }

    /// Segments in the order first seen
    pub fn segments(&self) -> &[Segment] {
        &self.segments
    }

    /// The server's own time per response, summed over its segments
    pub fn total(&self) -> &Histogram {
        &self.total
    }

    /// The segment that takes the most of the server's time, by median
    pub fn largest(&self) -> Option<&Segment> {
        self.segments
            .iter()
            .filter(|s| s.durations.count() > 0)
            .max_by_key(|s| s.durations.percentile(50.0))
    }
}

/// The slowest responses of the run, slowest first, with the ids their
/// backend gave them. Requests that got no response aren't here: a
/// timeout is always the slowest and has no id to look up.
#[derive(Debug, Default, Clone)]
pub struct Slowest {
    entries: Vec<SlowResponse>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SlowResponse {
    /// When it finished, since the run started
    pub at: Duration,
    pub latency: Duration,
    pub status: u16,
    pub request_id: Option<(&'static str, Box<str>)>,
}

impl Slowest {
    pub fn record(&mut self, stat: &ResponseStats, at: Duration) {
        let Some(status) = stat.status_code else {
            return;
        };
        let full = self.entries.len() == SLOWEST_KEPT;
        if full && stat.duration <= self.entries[SLOWEST_KEPT - 1].latency {
            return;
        }
        let entry = SlowResponse {
            at,
            latency: stat.duration,
            status: status.as_u16(),
            request_id: stat.request_id.clone(),
        };
        let index = self
            .entries
            .iter()
            .position(|e| e.latency < entry.latency)
            .unwrap_or(self.entries.len());
        self.entries.insert(index, entry);
        self.entries.truncate(SLOWEST_KEPT);
    }

    pub fn entries(&self) -> &[SlowResponse] {
        &self.entries
    }
}

fn per_second(amount: f64, elapsed: Duration) -> f64 {
    let secs = elapsed.as_secs_f64();
    if secs > 0.0 {
        amount / secs
    } else {
        0.0
    }
}

/// The two parts of a request's time that every response has
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    FirstByte,
    Download,
}

/// Distinct failure causes counted; any more are counted as "other"
const MAX_FAILURE_CAUSES: usize = 32;

/// Failed requests grouped by cause ("HTTP 503", "Connection refused (os
/// error 61)"), each with the first response body seen for it, so the
/// verdict can show what the target actually said
#[derive(Debug, Default, Clone)]
pub struct Failures {
    causes: HashMap<Box<str>, Cause>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Cause {
    pub count: u64,
    /// The first response body (its start, one line) seen for this cause;
    /// failures without a response have none
    pub example: Option<Box<str>>,
}

impl Failures {
    fn count(&mut self, cause: String, example: Option<String>) {
        if let Some(entry) = self.causes.get_mut(cause.as_str()) {
            entry.count += 1;
            return;
        }
        let key: Box<str> = if self.causes.len() < MAX_FAILURE_CAUSES {
            cause.into()
        } else {
            "other".into()
        };
        let entry = self.causes.entry(key).or_default();
        entry.count += 1;
        if entry.example.is_none() {
            entry.example = example.filter(|e| !e.is_empty()).map(Into::into);
        }
    }

    /// Causes, most frequent first (ties by name, so the order is stable)
    pub fn top(&self) -> Vec<(&str, &Cause)> {
        let mut causes: Vec<(&str, &Cause)> =
            self.causes.iter().map(|(k, v)| (k.as_ref(), v)).collect();
        causes.sort_by(|a, b| b.1.count.cmp(&a.1.count).then(a.0.cmp(b.0)));
        causes
    }
}

/// Log-linear histogram (HDR-style). Values below 2^SUB_BITS are exact;
/// larger values land in buckets whose width is under 1% of their value.
#[derive(Debug, Default, Clone)]
pub struct Histogram {
    counts: Vec<u64>,
    count: u64,
    min: u64,
    max: u64,
}

const SUB_BITS: u32 = 7;
const SUB_COUNT: u64 = 1 << SUB_BITS;

impl Histogram {
    fn index(value: u64) -> usize {
        if value < SUB_COUNT {
            return value as usize;
        }
        let exp = 63 - value.leading_zeros();
        let shift = exp - SUB_BITS;
        let top = value >> shift; // in [SUB_COUNT, 2 * SUB_COUNT)
        (((shift as u64 + 1) << SUB_BITS) + (top - SUB_COUNT)) as usize
    }

    /// Midpoint of the values that map to bucket `index`
    fn value(index: usize) -> u64 {
        let index = index as u64;
        if index < SUB_COUNT {
            return index;
        }
        let shift = (index >> SUB_BITS) - 1;
        let top = (index & (SUB_COUNT - 1)) + SUB_COUNT;
        (top << shift) + ((1u64 << shift) >> 1)
    }

    pub fn record(&mut self, value: u64) {
        let index = Self::index(value);
        if index >= self.counts.len() {
            self.counts.resize(index + 1, 0);
        }
        self.counts[index] += 1;
        if self.count == 0 || value < self.min {
            self.min = value;
        }
        self.max = self.max.max(value);
        self.count += 1;
    }

    /// Empty the histogram but keep its allocation for reuse
    pub fn clear(&mut self) {
        self.counts.fill(0);
        self.count = 0;
        self.min = 0;
        self.max = 0;
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    /// Share of recorded values (0-1) in buckets below `value`'s bucket
    pub fn fraction_below(&self, value: u64) -> f64 {
        if self.count == 0 {
            return 0.0;
        }
        let bucket = Self::index(value).min(self.counts.len());
        self.counts[..bucket].iter().sum::<u64>() as f64 / self.count as f64
    }

    /// Non-empty buckets as (representative value, count), smallest first
    pub fn buckets(&self) -> impl Iterator<Item = (u64, u64)> + '_ {
        self.counts
            .iter()
            .enumerate()
            .filter(|(_, &count)| count > 0)
            .map(|(index, &count)| (Self::value(index), count))
    }

    pub fn percentile(&self, q: f64) -> u64 {
        if self.count == 0 {
            return 0;
        }
        let rank = ((q.clamp(0.0, 100.0) / 100.0) * self.count as f64).ceil() as u64;
        let rank = rank.clamp(1, self.count);
        let mut seen = 0;
        for (index, &count) in self.counts.iter().enumerate() {
            seen += count;
            if seen >= rank {
                return Self::value(index).clamp(self.min, self.max);
            }
        }
        self.max
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::StatusCode;

    fn stat(ms: u64, status: Option<u16>, error: Option<ErrorKind>) -> ResponseStats {
        ResponseStats {
            duration: Duration::from_millis(ms),
            status_code: status.map(|s| StatusCode::from_u16(s).unwrap()),
            body_bytes: 10,
            error,
            ..Default::default()
        }
    }

    #[test]
    fn histogram_is_exact_for_small_values() {
        let mut h = Histogram::default();
        for v in 0..SUB_COUNT {
            h.record(v);
        }
        assert_eq!(h.percentile(0.0), 0);
        assert_eq!(h.percentile(50.0), 63);
        assert_eq!(h.percentile(100.0), SUB_COUNT - 1);
    }

    #[test]
    fn histogram_relative_error_is_under_one_percent() {
        for v in [200u64, 1_000, 12_345, 999_999, 30_000_000, u32::MAX as u64] {
            let mut h = Histogram::default();
            h.record(1);
            h.record(v);
            let got = h.percentile(100.0);
            let err = (got as f64 - v as f64).abs() / v as f64;
            assert!(err < 0.01, "v={v} got={got} err={err}");
            // Bucket round-trips to itself
            assert_eq!(
                Histogram::index(Histogram::value(Histogram::index(v))),
                Histogram::index(v)
            );
        }
    }

    #[test]
    fn cleared_histogram_starts_over() {
        let mut h = Histogram::default();
        h.record(5_000);
        h.clear();
        assert_eq!((h.count(), h.percentile(50.0)), (0, 0));
        h.record(7);
        assert_eq!(h.percentile(50.0), 7);
        assert_eq!(h.buckets().collect::<Vec<_>>(), vec![(7, 1)]);
    }

    #[test]
    fn rank_is_the_share_of_faster_requests() {
        let mut m = Metrics::default();
        for ms in 1..=100 {
            m.record(&stat(ms, Some(200), None));
        }
        let rank = m.rank(Duration::from_millis(90));
        assert!((0.88..=0.90).contains(&rank), "rank={rank}");
        assert_eq!(m.rank(Duration::ZERO), 0.0);
        assert_eq!(Metrics::default().rank(Duration::from_millis(5)), 0.0);
    }

    #[test]
    fn percentiles_use_nearest_rank() {
        let mut m = Metrics::default();
        for ms in 1..=100 {
            m.record(&stat(ms, Some(200), None));
        }
        assert_eq!(m.min(), Duration::from_millis(1));
        assert_eq!(m.max(), Duration::from_millis(100));
        let p50 = m.percentile(50.0).as_millis();
        let p99 = m.percentile(99.0).as_millis();
        assert!((49..=51).contains(&p50), "p50={p50}");
        assert!((98..=100).contains(&p99), "p99={p99}");
        assert_eq!(m.mean(), Duration::from_micros(50_500));
    }

    #[test]
    fn std_dev_matches_population_formula() {
        let mut m = Metrics::default();
        for ms in [2, 4, 4, 4, 5, 5, 7, 9] {
            m.record(&stat(ms, Some(200), None));
        }
        // Classic example: mean 5, population std dev 2
        assert_eq!(m.mean(), Duration::from_millis(5));
        assert_eq!(m.std_dev(), Duration::from_millis(2));
    }

    #[test]
    fn outcomes_are_counted_once_each() {
        let mut m = Metrics::default();
        m.record(&stat(1, Some(200), None));
        m.record(&stat(1, Some(503), None));
        m.record(&stat(1, None, Some(ErrorKind::Timeout)));
        m.record(&stat(1, None, Some(ErrorKind::Connect)));
        m.record(&stat(1, None, Some(ErrorKind::Other)));
        assert_eq!(
            (m.total, m.success, m.failed, m.timeouts, m.errors),
            (5, 1, 1, 1, 2)
        );
        assert_eq!(m.status_codes.get(&503), Some(&1));
        assert_eq!(m.bytes, 50);
    }

    #[test]
    fn server_timing_adds_up_per_response_and_caps_the_names() {
        use crate::trace::ServerTiming;
        let entry = |name: &str, ms: Option<f64>| ServerTiming {
            name: name.into(),
            dur_ms: ms,
            desc: None,
        };
        let mut m = Metrics::default();
        let mut s = stat(50, Some(200), None);
        s.server_timing = Some(vec![entry("db", Some(30.0)), entry("app", Some(10.0))].into());
        m.record(&s);
        s.server_timing = Some(vec![entry("db", Some(20.0)), entry("cache", None)].into());
        m.record(&s);
        m.record(&stat(5, Some(200), None));
        let t = m.server_timing();
        assert_eq!(t.responses(), 2);
        let names: Vec<&str> = t.segments().iter().map(|s| s.name.as_ref()).collect();
        assert_eq!(names, ["db", "app", "cache"]);
        assert_eq!(t.segments()[2].count, 1);
        assert_eq!(
            t.segments()[2].durations.count(),
            0,
            "no dur, nothing timed"
        );
        assert_eq!(t.total().count(), 2);
        assert!((39_000..=41_000).contains(&t.total().percentile(100.0)));
        assert_eq!(t.largest().unwrap().name.as_ref(), "db");

        let mut many = Metrics::default();
        let entries: Vec<ServerTiming> = (0..12)
            .map(|i| entry(&format!("s{i}"), Some(1.0)))
            .collect();
        s.server_timing = Some(entries.into());
        many.record(&s);
        assert_eq!(many.server_timing().segments().len(), MAX_SEGMENTS);
        assert!(Metrics::default().server_timing().is_empty());
    }

    #[test]
    fn slowest_keeps_the_top_few_responses_with_their_ids() {
        let mut slowest = Slowest::default();
        for (i, ms) in [30u64, 10, 50, 20, 40, 60, 5].iter().enumerate() {
            let mut s = stat(*ms, Some(200), None);
            s.request_id = Some(("x-request-id", format!("r{ms}").into()));
            slowest.record(&s, Duration::from_secs(i as u64));
        }
        // A timeout is slower than all of them but has no response
        slowest.record(
            &stat(1_000, None, Some(ErrorKind::Timeout)),
            Duration::from_secs(9),
        );
        let order: Vec<u64> = slowest
            .entries()
            .iter()
            .map(|e| e.latency.as_millis() as u64)
            .collect();
        assert_eq!(order, [60, 50, 40, 30, 20]);
        assert_eq!(
            slowest.entries()[0].request_id.as_ref().unwrap().1.as_ref(),
            "r60"
        );
        assert_eq!(slowest.entries()[0].at, Duration::from_secs(5));
    }

    #[test]
    fn phases_split_the_requests_that_got_a_response() {
        let mut m = Metrics::default();
        for _ in 0..10 {
            m.record(&ResponseStats {
                duration: Duration::from_millis(30),
                ttfb: Some(Duration::from_millis(20)),
                status_code: StatusCode::from_u16(200).ok(),
                ..Default::default()
            });
        }
        // A failed request has no phases
        m.record(&stat(500, None, Some(ErrorKind::Connect)));
        assert_eq!(m.phase(Phase::FirstByte).count(), 10);
        assert_eq!(m.phase(Phase::Download).count(), 10);
        let p50 = |phase| Duration::from_micros(m.phase(phase).percentile(50.0));
        assert!((19..=21).contains(&p50(Phase::FirstByte).as_millis()));
        assert!((9..=11).contains(&p50(Phase::Download).as_millis()));
    }

    #[test]
    fn failures_are_grouped_by_cause_with_the_first_body() {
        let mut m = Metrics::default();
        let failed = |code: u16, body: &str| ResponseStats {
            status_code: StatusCode::from_u16(code).ok(),
            preview: Some(bytes::Bytes::copy_from_slice(body.as_bytes())),
            ..Default::default()
        };
        m.record(&failed(503, "{\"error\":\"upstream timed out\"}"));
        m.record(&failed(503, "a later body, not kept"));
        m.record(&failed(500, ""));
        m.record(&ResponseStats {
            error: Some(ErrorKind::Connect),
            error_message: Some("Connection refused (os error 61)".into()),
            ..Default::default()
        });
        m.record(&stat(1, Some(200), None));
        let top = m.failures().top();
        assert_eq!(top[0].0, "HTTP 503");
        assert_eq!(top[0].1.count, 2);
        assert_eq!(
            top[0].1.example.as_deref(),
            Some("{\"error\":\"upstream timed out\"}")
        );
        // Ties go by name, so the order is the same every time
        assert_eq!(top[1].0, "Connection refused (os error 61)");
        assert_eq!(top[1].1.example, None, "no response, no body");
        assert_eq!(top[2].0, "HTTP 500");
        assert_eq!(top[2].1.example, None, "an empty body is no example");
        for code in 400..450 {
            m.record(&failed(code, "x"));
        }
        assert_eq!(
            m.failures().top().len(),
            MAX_FAILURE_CAUSES + 1,
            "capped, plus other"
        );
    }

    #[test]
    fn empty_metrics_are_zero() {
        let m = Metrics::default();
        assert_eq!(m.percentile(99.0), Duration::ZERO);
        assert_eq!(m.mean(), Duration::ZERO);
        assert_eq!(m.std_dev(), Duration::ZERO);
        assert_eq!(m.rps(Duration::ZERO), 0.0);
    }
}
