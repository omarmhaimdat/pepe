use std::collections::BTreeMap;
use std::time::Duration;

use serde::Serialize;

use crate::metrics::Metrics;

#[derive(Serialize)]
pub struct JsonReport {
    pub summary: JsonSummary,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub generator: Option<Generator>,
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

#[derive(Serialize)]
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
    /// Failed requests by cause, most frequent first, each with the first
    /// response body seen for it; left out when nothing failed
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<FailureCause>,
}

#[derive(Serialize, Clone)]
pub struct FailureCause {
    pub cause: String,
    pub count: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example_body: Option<String>,
}

#[derive(Serialize)]
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

    pub fn generate(metrics: &Metrics, elapsed: Duration, interrupted: bool) -> Self {
        Self {
            generator: None,
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
