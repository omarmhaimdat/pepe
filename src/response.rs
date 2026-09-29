use std::time::{Duration, Instant};

use crate::cache::CacheStatus;

/// Characters of the response body kept for the "Partial Responses" panel
const PARTIAL_RESPONSE_CHARS: usize = 100;

/// Why a request produced no response
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Timeout,
    Connect,
    Other,
}

impl ErrorKind {
    fn from_reqwest(e: &reqwest::Error) -> Self {
        if e.is_timeout() {
            Self::Timeout
        } else if e.is_connect() {
            Self::Connect
        } else {
            Self::Other
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            Self::Timeout => "TIMEOUT",
            Self::Connect => "CONNECT ERROR",
            Self::Other => "ERROR",
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ResponseStats {
    pub duration: Duration,
    /// None when no complete response was received (see `error`)
    pub status_code: Option<reqwest::StatusCode>,
    /// Body bytes actually received (not the Content-Length header, which is
    /// missing for chunked or compressed responses)
    pub body_bytes: u64,
    pub partial_response: Option<String>,
    /// (lookup, resolution) time of the DNS query made before the request
    pub dns_times: Option<(Duration, Duration)>,
    pub cache_status: Option<CacheStatus>,
    pub error: Option<ErrorKind>,
}

impl ResponseStats {
    pub async fn from_response(
        resp: Result<reqwest::Response, reqwest::Error>,
        start: Instant,
        dns_times: Option<(Duration, Duration)>,
    ) -> Self {
        let resp = match resp {
            Ok(resp) => resp,
            Err(e) => return Self::failed(&e, start, dns_times),
        };

        let status_code = resp.status();
        let cache_status = CacheStatus::parse_headers(resp.headers());
        let body = match resp.bytes().await {
            Ok(body) => body,
            // The status arrived but the body did not (e.g. timed out mid-body)
            Err(e) => return Self::failed(&e, start, dns_times),
        };
        let duration = start.elapsed();

        ResponseStats {
            duration,
            status_code: Some(status_code),
            body_bytes: body.len() as u64,
            partial_response: Some(partial_response(&body)),
            dns_times,
            cache_status,
            error: None,
        }
    }

    fn failed(e: &reqwest::Error, start: Instant, dns_times: Option<(Duration, Duration)>) -> Self {
        ResponseStats {
            duration: start.elapsed(),
            dns_times,
            error: Some(ErrorKind::from_reqwest(e)),
            ..Default::default()
        }
    }
}

/// First characters of the body on one line, for display
fn partial_response(body: &[u8]) -> String {
    // 4 bytes per char at most, so this slice always covers enough characters
    let head = &body[..body.len().min(PARTIAL_RESPONSE_CHARS * 4)];
    String::from_utf8_lossy(head)
        .trim()
        .chars()
        .map(|c| if c == '\n' || c == '\r' { ' ' } else { c })
        .take(PARTIAL_RESPONSE_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_response_is_single_line_and_truncated() {
        let body = format!("  line one\r\nline two{}", "x".repeat(500));
        let partial = partial_response(body.as_bytes());
        assert!(partial.starts_with("line one  line two"));
        assert_eq!(partial.chars().count(), PARTIAL_RESPONSE_CHARS);
    }

    #[test]
    fn partial_response_survives_invalid_utf8() {
        assert_eq!(partial_response(&[b'o', b'k', 0xff]), "ok\u{fffd}");
    }
}
