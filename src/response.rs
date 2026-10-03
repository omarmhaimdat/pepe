use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::header::HeaderMap;

use crate::cache::CacheStatus;

/// Raw body bytes kept for the dashboard's response preview
const PREVIEW_BYTES: usize = 256;
/// Characters of the preview shown on one line
const PREVIEW_CHARS: usize = 120;
/// Body bytes kept for the request inspector
pub const BODY_CAPTURE: usize = 32 * 1024;

/// Everything about a response, kept for the few requests the dashboard's
/// inspector can show in full (see `load::DetailBudget`)
#[derive(Debug)]
pub struct Detail {
    pub version: reqwest::Version,
    pub headers: HeaderMap,
    /// Up to `BODY_CAPTURE` bytes of the body
    pub body: Bytes,
    /// The body was longer than what's kept
    pub truncated: bool,
    pub remote_addr: Option<SocketAddr>,
    /// Where the request ended up, after any redirects
    pub final_url: String,
}

impl Detail {
    /// Rough memory held, for the dashboard's budget
    pub fn size(&self) -> usize {
        let headers: usize = self
            .headers
            .iter()
            .map(|(k, v)| k.as_str().len() + v.len() + 32)
            .sum();
        std::mem::size_of::<Self>() + headers + self.body.len() + self.final_url.len()
    }
}

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
    /// Why the request failed, in the words of the innermost error (e.g.
    /// "Connection refused (os error 61)")
    pub error_message: Option<Box<str>>,
    /// Which endpoint of the run this request went to (API mode); 0 otherwise
    pub endpoint: u16,
    /// Time until the response headers arrived
    pub ttfb: Option<Duration>,
    /// Full headers and body, when this request was picked for capture
    pub detail: Option<Arc<Detail>>,
}

impl ResponseStats {
    /// `ttfb`: when `send` returned. `capture`: keep the full response.
    pub async fn from_response(
        resp: Result<reqwest::Response, reqwest::Error>,
        start: Instant,
        ttfb: Duration,
        dns_times: Option<(Duration, Duration)>,
        keep_preview: bool,
        capture: bool,
    ) -> Self {
        let mut resp = match resp {
            Ok(resp) => resp,
            Err(e) => return Self::failed(&e, start, dns_times),
        };

        let status_code = resp.status();
        let cache_status = CacheStatus::parse_headers(resp.headers());
        let mut detail = capture.then(|| Detail {
            version: resp.version(),
            headers: resp.headers().clone(),
            body: Bytes::new(),
            truncated: false,
            remote_addr: resp.remote_addr(),
            final_url: resp.url().to_string(),
        });
        let mut captured = Vec::new();
        // Stream the body and count it instead of buffering it whole, so
        // large responses cost no memory beyond one chunk
        let mut body_bytes = 0u64;
        // A failed response keeps its start whatever the caller wants: the
        // verdict shows the first body of each kind of failure
        let mut preview = (keep_preview || !status_code.is_success()).then(Vec::new);
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    body_bytes += chunk.len() as u64;
                    if let Some(buf) = preview.as_mut() {
                        let want = PREVIEW_BYTES.saturating_sub(buf.len());
                        buf.extend_from_slice(&chunk[..want.min(chunk.len())]);
                    }
                    if detail.is_some() {
                        let want = BODY_CAPTURE.saturating_sub(captured.len());
                        captured.extend_from_slice(&chunk[..want.min(chunk.len())]);
                    }
                }
                Ok(None) => break,
                // The status arrived but the body did not (e.g. timed out mid-body)
                Err(e) => return Self::failed(&e, start, dns_times),
            }
        }
        let duration = start.elapsed();
        if let Some(detail) = detail.as_mut() {
            detail.truncated = body_bytes as usize > captured.len();
            detail.body = Bytes::from(captured);
        }

        ResponseStats {
            duration,
            endpoint: 0,
            ttfb: Some(ttfb),
            detail: detail.map(Arc::new),
            status_code: Some(status_code),
            body_bytes,
            preview: preview.map(Bytes::from),
            dns_times,
            cache_status,
            error: None,
            error_message: None,
        }
    }

    fn failed(e: &reqwest::Error, start: Instant, dns_times: Option<(Duration, Duration)>) -> Self {
        ResponseStats {
            duration: start.elapsed(),
            dns_times,
            error: Some(ErrorKind::from_reqwest(e)),
            error_message: Some(root_cause(e).into()),
            ..Default::default()
        }
    }

    /// What to call this failure when counting failures by cause: the
    /// status for a response, the innermost error's words otherwise
    pub fn failure_cause(&self) -> Option<String> {
        match (self.status_code, &self.error_message, self.error) {
            (Some(code), _, _) if code.is_success() => None,
            (Some(code), _, _) => Some(format!("HTTP {}", code.as_u16())),
            (None, Some(message), _) => Some(message.to_string()),
            (None, None, Some(kind)) => Some(kind.label().to_lowercase()),
            (None, None, None) => Some("error".to_string()),
        }
    }
}

impl ResponseStats {
    /// First characters of the body on one line, for display
    pub fn preview_text(&self) -> Option<String> {
        self.preview.as_deref().map(preview_text)
    }
}

/// The innermost error's message: reqwest's own is generic ("error sending
/// request for url ..."), the cause is what's useful
fn root_cause(e: &(dyn std::error::Error + 'static)) -> String {
    let mut cause = e;
    while let Some(source) = cause.source() {
        cause = source;
    }
    cause.to_string()
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
    fn root_cause_is_the_innermost_error() {
        let io = std::io::Error::new(std::io::ErrorKind::ConnectionRefused, "Connection refused");
        let outer = std::io::Error::other(io);
        assert_eq!(root_cause(&outer), "Connection refused");
    }

    #[test]
    fn preview_survives_invalid_utf8() {
        assert_eq!(preview_text(&[b'o', b'k', 0xff]), "ok\u{fffd}");
        // Cut in the middle of "é" (0xC3 0xA9)
        assert_eq!(preview_text(&[b'o', b'k', 0xc3]), "ok");
    }
}
