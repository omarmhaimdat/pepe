use std::time::{Duration, Instant};

use bytes::Bytes;

use crate::cache::CacheStatus;

/// Raw body bytes kept for the dashboard's response preview
const PREVIEW_BYTES: usize = 256;
/// Characters of the preview shown on one line
const PREVIEW_CHARS: usize = 120;

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
    /// Start of the body, copied only when the caller asked for previews.
    /// Kept raw; decoding happens at display time for the few rows on screen.
    pub preview: Option<Bytes>,
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
        keep_preview: bool,
    ) -> Self {
        let mut resp = match resp {
            Ok(resp) => resp,
            Err(e) => return Self::failed(&e, start, dns_times),
        };

        let status_code = resp.status();
        let cache_status = CacheStatus::parse_headers(resp.headers());
        // Stream the body and count it instead of buffering it whole, so
        // large responses cost no memory beyond one chunk
        let mut body_bytes = 0u64;
        let mut preview = keep_preview.then(Vec::new);
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    body_bytes += chunk.len() as u64;
                    if let Some(buf) = preview.as_mut() {
                        let want = PREVIEW_BYTES.saturating_sub(buf.len());
                        buf.extend_from_slice(&chunk[..want.min(chunk.len())]);
                    }
                }
                Ok(None) => break,
                // The status arrived but the body did not (e.g. timed out mid-body)
                Err(e) => return Self::failed(&e, start, dns_times),
            }
        }
        let duration = start.elapsed();

        ResponseStats {
            duration,
            status_code: Some(status_code),
            body_bytes,
            preview: preview.map(Bytes::from),
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

impl ResponseStats {
    /// First characters of the body on one line, for display
    pub fn preview_text(&self) -> Option<String> {
        self.preview.as_deref().map(preview_text)
    }
}

fn preview_text(body: &[u8]) -> String {
    // The preview may end mid-character; drop that tail rather than show U+FFFD
    let body = match std::str::from_utf8(body) {
        Err(e) if e.error_len().is_none() => &body[..e.valid_up_to()],
        _ => body,
    };
    String::from_utf8_lossy(body)
        .trim()
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(PREVIEW_CHARS)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_single_line_and_truncated() {
        let body = format!("  line one\r\nline\ttwo{}", "x".repeat(500));
        let preview = preview_text(body.as_bytes());
        assert!(preview.starts_with("line one  line two"));
        assert_eq!(preview.chars().count(), PREVIEW_CHARS);
    }

    #[test]
    fn preview_survives_invalid_utf8() {
        assert_eq!(preview_text(&[b'o', b'k', 0xff]), "ok\u{fffd}");
        // Cut in the middle of "é" (0xC3 0xA9)
        assert_eq!(preview_text(&[b'o', b'k', 0xc3]), "ok");
    }
}
