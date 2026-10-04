use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::metrics::{Histogram, Metrics, Phase, Slowest};
use crate::timeline::{Sample, Timeline};

#[derive(Serialize, Clone)]
pub struct JsonReport {
    pub summary: JsonSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<Generator>,
    /// One point per minute over the whole run; empty for runs under a
    /// minute, and left out then
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub timeline: Vec<TimelinePoint>,
    /// Present when the report was written by `--snapshot`, which writes
    /// it every minute while the run goes and once more when it ends
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<Snapshot>,
}

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
pub struct TimelinePoint {
    /// Seconds since the run started, at the end of the minute
    pub at_s: f64,
    pub requests_per_second: f64,
    /// Non-2xx responses and failed requests per second
    pub errors_per_second: f64,
    pub p50_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
}

impl From<&Sample> for TimelinePoint {
    fn from(s: &Sample) -> Self {
        let round = |v: f64| (v * 1000.0).round() / 1000.0;
        Self {
            at_s: round(s.at),
            requests_per_second: round(s.rps),
            errors_per_second: round(s.errors),
            p50_ms: round(s.p50_ms),
            p90_ms: round(s.p90_ms),
            p99_ms: round(s.p99_ms),
        }
    }
}

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
pub struct Snapshot {
    /// When it was written, in seconds since the Unix epoch
    pub unix_time: u64,
    /// The run was still going; false in the last one
    pub running: bool,
}

/// pepe's own load during the run: whether pepe, rather than the target,
/// was the limit
#[derive(Serialize, Clone, Copy)]
pub struct Generator {
    /// Threads that were sending (`--threads`)
    pub threads: usize,
    /// The busiest second of the busiest sending thread, in percent of a
    /// core. Near 100, the numbers are pepe's limit; `--threads` adds more.
    /// Absent where the platform can't measure it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub peak_busy_percent: Option<u8>,
    /// `--rate`: the requests per second asked for
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_per_second: Option<f64>,
    /// Starts the rate called for that never happened, because every unit
    /// of concurrency was in flight for more than a second at a time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_missed: Option<u64>,
}

#[derive(Serialize, Clone)]
pub struct JsonSummary {
    pub total_requests: u64,
    /// 2xx responses
    pub successful_requests: u64,
    /// Every request that did not get a 2xx response
    pub failed_requests: u64,
    /// Responses with a non-2xx status
    pub http_errors: u64,
    pub timeout_errors: u64,
    /// Requests that failed without a response (connect, TLS, ...)
    pub connection_errors: u64,
    /// True when the run was stopped early (Ctrl-C)
    pub interrupted: bool,
    pub duration_ms: f64,
    pub requests_per_second: f64,
    pub data_transfer_bytes: u64,
    pub latency: LatencyStats,
    pub status_codes: BTreeMap<u16, u64>,
    /// What the target's `Server-Timing` headers said; left out when it
    /// sent none
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_timing: Option<ServerTimingStats>,
    /// The slowest responses, slowest first, with the request ids their
    /// backend gave them (from `X-Request-Id`, `traceparent`, `CF-Ray`,
    /// … or `--trace-header`); left out when nothing was tracked
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub slowest_requests: Vec<SlowRequest>,
    /// What changed during the run, as it was noticed; left out when nothing did
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub anomalies: Vec<AnomalyNote>,
    /// Failed requests by cause, most frequent first, each with the first
    /// response body seen for it; left out when nothing failed
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<FailureCause>,
}

#[derive(Serialize, Clone)]
pub struct ServerTimingStats {
    /// Responses that carried the header
    pub responses: u64,
    /// The server's own time per response: its `dur`s added up
    pub total: TimingStats,
    /// Each named entry, in the order first seen
    pub segments: BTreeMap<String, TimingStats>,
}

#[derive(Serialize, Clone)]
pub struct TimingStats {
    /// Entries seen, with or without a duration
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub median_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p90_ms: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub p99_ms: Option<f64>,
}

impl TimingStats {
    fn of(count: u64, durations: &Histogram) -> Self {
        let q =
            |q| (durations.count() > 0).then(|| ms(Duration::from_micros(durations.percentile(q))));
        Self {
            count,
            median_ms: q(50.0),
            p90_ms: q(90.0),
            p99_ms: q(99.0),
        }
    }
}

#[derive(Serialize, Clone)]
pub struct SlowRequest {
    /// Seconds since the run started, when it finished
    pub at_s: f64,
    pub latency_ms: f64,
    pub status: u16,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    /// The response header the id came from
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id_header: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct AnomalyNote {
    /// Seconds since the run started
    pub at_s: f64,
    pub kind: &'static str,
    pub text: String,
}

#[derive(Serialize, Clone)]
pub struct FailureCause {
    pub cause: String,
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_body: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct LatencyStats {
    pub min_ms: f64,
    pub max_ms: f64,
    pub avg_ms: f64,
    pub std_dev_ms: f64,
    pub median_ms: f64,
    pub p90_ms: f64,
    pub p95_ms: f64,
    pub p99_ms: f64,
    /// Where the time went: opening connections, waiting for the headers,
    /// and reading the body
    pub phases: Phases,
}

#[derive(Serialize, Clone)]
pub struct Phases {
    /// Opening a connection, TCP and TLS together; one entry per connection
    /// opened, so a keep-alive run has few. Absent when nothing measured it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub connect: Option<PhaseStats>,
    /// From sending the request to the response headers
    pub first_byte: PhaseStats,
    /// From the headers to the end of the body
    pub download: PhaseStats,
    /// The DNS probe, sampled once a second
    pub dns_lookup_avg_ms: f64,
}

#[derive(Serialize, Clone)]
pub struct PhaseStats {
    pub count: u64,
    pub median_ms: f64,
    pub p99_ms: f64,
    pub max_ms: f64,
}

impl PhaseStats {
    fn of(hist: &crate::metrics::Histogram) -> Self {
        let ms = |us: u64| (us as f64) / 1000.0;
        Self {
            count: hist.count(),
            median_ms: ms(hist.percentile(50.0)),
            p99_ms: ms(hist.percentile(99.0)),
            max_ms: ms(hist.percentile(100.0)),
        }
    }
}

fn server_timing_stats(metrics: &Metrics) -> Option<ServerTimingStats> {
    let timing = metrics.server_timing();
    if timing.is_empty() {
        return None;
    }
    Some(ServerTimingStats {
        responses: timing.responses(),
        total: TimingStats::of(timing.total().count(), timing.total()),
        segments: timing
            .segments()
            .iter()
            .map(|s| (s.name.to_string(), TimingStats::of(s.count, &s.durations)))
            .collect(),
    })
}

fn ms(d: Duration) -> f64 {
    // Microsecond precision is plenty and keeps the output readable
    (d.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

impl JsonReport {
    /// `rate`: the `--rate` asked for and the starts it missed
    pub fn with_generator(
        mut self,
        threads: usize,
        peak_busy_percent: Option<u8>,
        rate: Option<(f64, u64)>,
    ) -> Self {
        self.generator = Some(Generator {
            threads,
            peak_busy_percent,
            rate_per_second: rate.map(|(r, _)| r),
            rate_missed: rate.map(|(_, missed)| missed),
        });
        self
    }

    /// What the watch noticed during the run
    pub fn with_anomalies(mut self, anomalies: &[crate::insights::Anomaly]) -> Self {
        self.summary.anomalies = anomalies
            .iter()
            .map(|a| AnomalyNote {
                at_s: (a.at.as_secs_f64() * 1000.0).round() / 1000.0,
                kind: a.kind.name(),
                text: a.text.clone(),
            })
            .collect();
        self
    }

    /// How long the run's connections took to open
    pub fn with_connects(mut self, connects: &crate::request::ConnectTimes) -> Self {
        let hist = connects.histogram();
        if hist.count() > 0 {
            self.summary.latency.phases.connect = Some(PhaseStats::of(&hist));
        }
        self
    }

    /// The minute-by-minute series of a run
    pub fn with_timeline(mut self, timeline: &Timeline) -> Self {
        self.timeline = timeline.minutes().iter().map(TimelinePoint::from).collect();
        self
    }

    /// Mark the report as a snapshot of a run, still going or just over
    pub fn with_snapshot(mut self, running: bool) -> Self {
        self.snapshot = Some(Snapshot {
            unix_time: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs()),
            running,
        });
        self
    }

    /// Write the report to `path`, whole or not at all: it goes to a file
    /// next to it first and is renamed into place, so a reader never sees
    /// half a report and a crash mid-write leaves the previous one
    pub fn write_to(&self, path: &Path) -> std::io::Result<()> {
        let json = self.to_json()?;
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(&tmp, path)
    }

    /// The slowest responses and their request ids
    pub fn with_slowest(mut self, slowest: &Slowest) -> Self {
        self.summary.slowest_requests = slowest
            .entries()
            .iter()
            .map(|e| SlowRequest {
                at_s: (e.at.as_secs_f64() * 1000.0).round() / 1000.0,
                latency_ms: ms(e.latency),
                status: e.status,
                request_id: e.request_id.as_ref().map(|(_, id)| id.to_string()),
                id_header: e.request_id.as_ref().map(|(name, _)| name.to_string()),
            })
            .collect();
        self
    }

    pub fn generate(metrics: &Metrics, elapsed: Duration, interrupted: bool) -> Self {
        Self {
            generator: None,
            timeline: Vec::new(),
            snapshot: None,
            summary: JsonSummary {
                total_requests: metrics.total,
                successful_requests: metrics.success,
                failed_requests: metrics.total - metrics.success,
                http_errors: metrics.failed,
                timeout_errors: metrics.timeouts,
                connection_errors: metrics.errors,
                interrupted,
                duration_ms: ms(elapsed),
                requests_per_second: metrics.rps(elapsed),
                data_transfer_bytes: metrics.bytes,
                latency: LatencyStats {
                    min_ms: ms(metrics.min()),
                    max_ms: ms(metrics.max()),
                    avg_ms: ms(metrics.mean()),
                    std_dev_ms: ms(metrics.std_dev()),
                    median_ms: ms(metrics.percentile(50.0)),
                    p90_ms: ms(metrics.percentile(90.0)),
                    p95_ms: ms(metrics.percentile(95.0)),
                    p99_ms: ms(metrics.percentile(99.0)),
                    phases: Phases {
                        connect: None,
                        first_byte: PhaseStats::of(metrics.phase(Phase::FirstByte)),
                        download: PhaseStats::of(metrics.phase(Phase::Download)),
                        dns_lookup_avg_ms: ms(metrics.avg_dns_lookup()),
                    },
                },
                // Sorted, so output is stable between runs
                status_codes: metrics.status_codes.iter().map(|(k, v)| (*k, *v)).collect(),
                server_timing: server_timing_stats(metrics),
                slowest_requests: Vec::new(),
                anomalies: Vec::new(),
                failures: metrics
                    .failures()
                    .top()
                    .into_iter()
                    .map(|(cause, c)| FailureCause {
                        cause: cause.to_string(),
                        count: c.count,
                        example_body: c.example.as_deref().map(str::to_string),
                    })
                    .collect(),
            },
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::response::{ErrorKind, ResponseStats};

    #[test]
    fn snapshots_carry_the_minutes_and_land_whole() {
        let mut timeline = Timeline::default();
        for second in 0..130u64 {
            timeline.record(&ResponseStats {
                duration: Duration::from_millis(20),
                status_code: reqwest::StatusCode::from_u16(200).ok(),
                ..Default::default()
            });
            timeline.advance(Duration::from_secs(second + 1));
        }
        let report = JsonReport::generate(&Metrics::default(), Duration::from_secs(130), false)
            .with_timeline(&timeline)
            .with_snapshot(true);
        assert_eq!(report.timeline.len(), 2);
        assert_eq!(report.timeline[1].at_s, 120.0);
        assert!(report.snapshot.unwrap().running);
        let json = report.to_json().unwrap();
        assert!(json.contains("\"timeline\"") && json.contains("\"running\": true"));

        let dir = std::env::temp_dir().join(format!("pepe-snapshot-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("soak.json");
        report.write_to(&path).unwrap();
        let again = JsonReport::generate(&Metrics::default(), Duration::from_secs(131), false)
            .with_snapshot(false);
        again.write_to(&path).unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("\"running\": false"),
            "the newer report replaced the older"
        );
        assert!(
            !dir.join("soak.json.tmp").exists(),
            "nothing left beside it"
        );
        // A report under a minute has no timeline key at all
        let short = JsonReport::generate(&Metrics::default(), Duration::from_secs(5), false);
        assert!(!short.to_json().unwrap().contains("timeline"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn report_covers_every_request() {
        let mut metrics = Metrics::default();
        for i in 0..250u64 {
            let (status, error) = match i % 5 {
                4 => (None, Some(ErrorKind::Timeout)),
                3 => (reqwest::StatusCode::from_u16(500).ok(), None),
                _ => (reqwest::StatusCode::from_u16(200).ok(), None),
            };
            metrics.record(&ResponseStats {
                duration: Duration::from_millis(10),
                status_code: status,
                body_bytes: 100,
                error,
                ..Default::default()
            });
        }

        let report = JsonReport::generate(&metrics, Duration::from_secs(2), false);
        let s = &report.summary;
        assert_eq!(s.total_requests, 250);
        assert_eq!(s.successful_requests, 150);
        assert_eq!(s.http_errors, 50);
        assert_eq!(s.timeout_errors, 50);
        assert_eq!(s.failed_requests, 100);
        assert_eq!(s.requests_per_second, 125.0);
        assert_eq!(s.data_transfer_bytes, 25_000);
        assert_eq!(s.status_codes.get(&200), Some(&150));

        let json: serde_json::Value = serde_json::from_str(&report.to_json().unwrap()).unwrap();
        assert_eq!(json["summary"]["latency"]["p99_ms"], 10.0);
        assert_eq!(json["summary"]["status_codes"]["500"], 50);
    }
}
