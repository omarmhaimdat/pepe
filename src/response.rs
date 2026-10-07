use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use reqwest::header::HeaderMap;

use crate::cache::CacheStatus;
use crate::direct;
use crate::trace::{self, ServerTiming};

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
    pub fn label(&self) -> &'static str {
        match self {
            Self::Timeout => "TIMEOUT",
            Self::Connect => "CONNECT ERROR",
            Self::Other => "ERROR",
        }
    }
}

/// A request that got no complete response, and why
#[derive(Debug)]
pub struct Failure {
    pub kind: ErrorKind,
    /// The innermost error's words (see `root_cause`)
    pub message: String,
}

impl From<&reqwest::Error> for Failure {
    fn from(e: &reqwest::Error) -> Self {
        let kind = if e.is_timeout() {
            ErrorKind::Timeout
        } else if e.is_connect() {
            ErrorKind::Connect
        } else {
            ErrorKind::Other
        };
        Self {
            kind,
            message: root_cause(e),
        }
    }
}

/// A response's headers as plain names and values, which is what both
/// ways of receiving one can give without copying
pub fn fields(headers: &HeaderMap) -> impl Iterator<Item = (&[u8], &[u8])> {
    headers
        .iter()
        .map(|(name, value)| (name.as_str().as_bytes(), value.as_bytes()))
}

/// A response whose body is still to be read, from either of the two ways
/// a request goes out (see `direct`)
pub enum Answer<'a> {
    /// With the piece of the body read last
    Reqwest(Box<reqwest::Response>, Bytes),
    Direct(direct::Response<'a>, &'a reqwest::Url),
}

impl From<reqwest::Response> for Answer<'_> {
    fn from(response: reqwest::Response) -> Self {
        Self::Reqwest(Box::new(response), Bytes::new())
    }
}

impl Answer<'_> {
    fn status(&self) -> reqwest::StatusCode {
        match self {
            Self::Reqwest(r, _) => r.status(),
            Self::Direct(r, _) => r.status,
        }
    }

    /// What the response says about itself: whether a cache answered, the
    /// id the backend gave the request, its `Server-Timing`
    #[allow(clippy::type_complexity)]
    fn told(
        &self,
    ) -> (
        Option<CacheStatus>,
        Option<(&'static str, Box<str>)>,
        Option<Box<[ServerTiming]>>,
    ) {
        match self {
            Self::Reqwest(r, _) => (
                CacheStatus::parse_headers(fields(r.headers())),
                trace::request_id(fields(r.headers())),
                trace::server_timing(fields(r.headers())),
            ),
            Self::Direct(r, _) => (
                CacheStatus::parse_headers(r.headers()),
                trace::request_id(r.headers()),
                trace::server_timing(r.headers()),
            ),
        }
    }

    fn header_map(&self) -> HeaderMap {
        match self {
            Self::Reqwest(r, _) => r.headers().clone(),
            Self::Direct(r, _) => r.header_map(),
        }
    }

    fn detail(&self) -> Detail {
        let (version, remote_addr, final_url) = match self {
            Self::Reqwest(r, _) => (r.version(), r.remote_addr(), r.url().to_string()),
            Self::Direct(r, url) => (r.version, r.remote_addr, url.to_string()),
        };
        Detail {
            version,
            headers: self.header_map(),
            body: Bytes::new(),
            truncated: false,
            remote_addr,
            final_url,
        }
    }

    async fn chunk(&mut self) -> Result<Option<&[u8]>, Failure> {
        match self {
            Self::Reqwest(r, held) => match r.chunk().await {
                Ok(Some(chunk)) => {
                    *held = chunk;
                    Ok(Some(&held[..]))
                }
                Ok(None) => Ok(None),
                Err(e) => Err(Failure::from(&e)),
            },
            Self::Direct(r, _) => r.chunk().await,
        }
    }
}

/// Which responses to keep the first bytes of
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Preview {
    /// All of them: the dashboard shows them
    Always,
    /// Those that failed: the verdict shows the first body of each kind
    /// of failure
    OfFailures,
    Never,
}

/// What is kept of a response beyond its numbers, for the ones something
/// is kept of
struct Kept {
    /// The start of the body, for the dashboard and the verdict
    preview: Option<Vec<u8>>,
    /// The whole response, for the inspector, and its body so far
    detail: Option<Detail>,
    captured: Vec<u8>,
    /// Headers and body for a flow step's captures
    headers: Option<HeaderMap>,
    body: Vec<u8>,
}

impl Kept {
    /// Keep what is wanted of the next piece of the body
    fn take(&mut self, chunk: &[u8], body_cap: usize) {
        if let Some(buf) = self.preview.as_mut() {
            let want = PREVIEW_BYTES.saturating_sub(buf.len());
            buf.extend_from_slice(&chunk[..want.min(chunk.len())]);
        }
        if self.detail.is_some() {
            let want = BODY_CAPTURE.saturating_sub(self.captured.len());
            self.captured
                .extend_from_slice(&chunk[..want.min(chunk.len())]);
        }
        if body_cap > 0 {
            let want = body_cap.saturating_sub(self.body.len());
            self.body.extend_from_slice(&chunk[..want.min(chunk.len())]);
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
    /// Sent during `--warmup`: shown as such, counted in nothing
    pub warmup: bool,
    /// The id the backend gave this request, and the header it came in,
    /// to find it in the server's logs
    pub request_id: Option<(&'static str, Box<str>)>,
    /// The response's `Server-Timing` entries, when it had any
    pub server_timing: Option<Box<[ServerTiming]>>,
}

impl ResponseStats {
    /// `ttfb`: when `send` returned. `capture`: keep the full response.
    /// Not an `async fn` of its own around `read`: each layer of those
    /// holds its arguments once more, in every worker's future.
    pub fn from_response(
        resp: Result<Answer<'_>, Failure>,
        start: Instant,
        ttfb: Duration,
        dns_times: Option<(Duration, Duration)>,
        keep_preview: Preview,
        capture: bool,
    ) -> impl std::future::Future<Output = (Self, Option<(HeaderMap, Bytes)>)> + '_ {
        Self::read(resp, start, ttfb, dns_times, keep_preview, capture, 0)
    }

    /// Like `from_response`, and also the response's headers and up to
    /// `body_cap` bytes of its body, for a flow step's captures
    pub async fn with_body(
        resp: Result<Answer<'_>, Failure>,
        start: Instant,
        ttfb: Duration,
        keep_preview: Preview,
        capture: bool,
        body_cap: usize,
    ) -> (Self, Option<(HeaderMap, Bytes)>) {
        Self::read(resp, start, ttfb, None, keep_preview, capture, body_cap).await
    }

    async fn read(
        resp: Result<Answer<'_>, Failure>,
        start: Instant,
        ttfb: Duration,
        dns_times: Option<(Duration, Duration)>,
        keep_preview: Preview,
        capture: bool,
        body_cap: usize,
    ) -> (Self, Option<(HeaderMap, Bytes)>) {
        let mut resp = match resp {
            Ok(resp) => resp,
            Err(e) => return (Self::failed(e, start, dns_times), None),
        };
        let status_code = resp.status();
        let (cache_status, request_id, server_timing) = resp.told();
        // A failed response keeps its start whatever the caller wants: the
        // verdict shows the first body of each kind of failure
        let preview = match keep_preview {
            Preview::Always => true,
            Preview::OfFailures => !status_code.is_success(),
            Preview::Never => false,
        };
        // Most responses of a fast run are only counted, and for those
        // nothing below is set up: what waits on the body is a few numbers
        let mut kept = (preview || capture || body_cap > 0).then(|| {
            Box::new(Kept {
                preview: preview.then(Vec::new),
                detail: capture.then(|| resp.detail()),
                captured: Vec::new(),
                headers: (body_cap > 0).then(|| resp.header_map()),
                body: Vec::new(),
            })
        });
        // Stream the body and count it instead of buffering it whole, so
        // large responses cost no memory beyond one chunk
        let mut body_bytes = 0u64;
        loop {
            match resp.chunk().await {
                Ok(Some(chunk)) => {
                    body_bytes += chunk.len() as u64;
                    if let Some(kept) = kept.as_mut() {
                        kept.take(chunk, body_cap);
                    }
                }
                Ok(None) => break,
                // The status arrived but the body did not (e.g. timed out mid-body)
                Err(e) => return (Self::failed(e, start, dns_times), None),
            }
        }
        let duration = start.elapsed();
        let mut stats = ResponseStats {
            duration,
            ttfb: Some(ttfb),
            status_code: Some(status_code),
            body_bytes,
            dns_times,
            cache_status,
            request_id,
            server_timing,
            ..Default::default()
        };
        let Some(kept) = kept else {
            return (stats, None);
        };
        let Kept {
            preview,
            detail,
            captured,
            headers,
            body,
        } = *kept;
        stats.preview = preview.map(Bytes::from);
        stats.detail = detail.map(|mut detail| {
            detail.truncated = body_bytes as usize > captured.len();
            detail.body = Bytes::from(captured);
            Arc::new(detail)
        });
        (stats, headers.map(|h| (h, Bytes::from(body))))
    }

    /// Turn a response into a failed step: the request got an answer, but
    /// not the one the flow needed (a wrong status, a capture that found
    /// nothing), so it counts as failed with that said
    pub fn fail_step(&mut self, why: String) {
        self.status_code = None;
        self.error = Some(ErrorKind::Other);
        self.error_message = Some(why.into());
        self.preview = None;
    }

    fn failed(e: Failure, start: Instant, dns_times: Option<(Duration, Duration)>) -> Self {
        ResponseStats {
            duration: start.elapsed(),
            dns_times,
            error: Some(e.kind),
            error_message: Some(e.message.into()),
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
pub fn root_cause(e: &(dyn std::error::Error + 'static)) -> String {
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
