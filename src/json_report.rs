use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;

use crate::metrics::Metrics;
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
    /// What changed during the run, as it was noticed; left out when nothing did
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub anomalies: Vec<AnomalyNote>,
    /// Failed requests by cause, most frequent first, each with the first
    /// response body seen for it; left out when nothing failed
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<FailureCause>,
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
}

fn ms(d: Duration) -> f64 {
    // Microsecond precision is plenty and keeps the output readable
    (d.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

impl JsonReport {
    pub fn with_generator(mut self, threads: usize, peak_busy_percent: Option<u8>) -> Self {
        self.generator = Some(Generator {
            threads,
            peak_busy_percent,
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
                },
                // Sorted, so output is stable between runs
                status_codes: metrics.status_codes.iter().map(|(k, v)| (*k, *v)).collect(),
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
