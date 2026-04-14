use serde::Serialize;
use crate::ResponseStats;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct JsonReport {
    pub summary: JsonSummary,
    pub requests: Vec<ResponseStats>,
}

#[derive(Serialize)]
pub struct JsonSummary {
    pub total_requests: usize,
    pub successful_requests: usize,
    pub failed_requests: usize,
    pub timeout_errors: usize,
    pub duration_ms: u128,
    pub requests_per_second: f64,
    pub data_transfer_bytes: u64,
    pub latency: LatencyStats,
    pub status_codes: HashMap<u16, usize>,
}

#[derive(Serialize)]
pub struct LatencyStats {
    pub min_ms: u128,
    pub max_ms: u128,
    pub avg_ms: u128,
    pub median_ms: u128,
    pub p95_ms: u128,
    pub p99_ms: u128,
}

impl JsonReport {
    pub fn generate(
        requests: &[ResponseStats],
        total_duration_ms: u128,
    ) -> Self {
        let total_requests = requests.len();
        let mut successful_requests = 0;
        let mut failed_requests = 0;
        let mut timeout_errors = 0;
        let mut data_transfer_bytes = 0u64;
        let mut latencies: Vec<u128> = Vec::new();
        let mut status_codes: HashMap<u16, usize> = HashMap::new();

        for request in requests {
            latencies.push(request.duration.as_millis());
            
            match request.status_code {
                Some(code) => {
                    *status_codes.entry(code.as_u16()).or_insert(0) += 1;
                    if code.is_success() {
                        successful_requests += 1;
                    } else {
                        failed_requests += 1;
                    }
                }
                None => {
                    timeout_errors += 1;
                    failed_requests += 1;
                }
            }

            if let Some(content_length) = request.content_length {
                data_transfer_bytes += content_length;
            }
        }

        latencies.sort();

        let min_ms = *latencies.iter().min().unwrap_or(&0);
        let max_ms = *latencies.iter().max().unwrap_or(&0);
        let avg_ms = if total_requests > 0 {
            latencies.iter().sum::<u128>() / total_requests as u128
        } else {
            0
        };

        let median_ms = if total_requests > 0 {
            latencies[total_requests / 2]
        } else {
            0
        };

        let p95_ms = if total_requests > 0 {
            latencies[(total_requests as f64 * 0.95) as usize]
        } else {
            0
        };

        let p99_ms = if total_requests > 0 {
            latencies[(total_requests as f64 * 0.99) as usize]
        } else {
            0
        };

        let requests_per_second = if total_duration_ms > 0 {
            (total_requests as f64 / total_duration_ms as f64) * 1000.0
        } else {
            0.0
        };

        Self {
            summary: JsonSummary {
                total_requests,
                successful_requests,
                failed_requests,
                timeout_errors,
                duration_ms: total_duration_ms,
                requests_per_second,
                data_transfer_bytes,
                latency: LatencyStats {
                    min_ms,
                    max_ms,
                    avg_ms,
                    median_ms,
                    p95_ms,
                    p99_ms,
                },
                status_codes,
            },
            requests: requests.to_vec(),
        }
    }

    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}
