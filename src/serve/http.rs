//! The little of HTTP the API needs on top of hyper: JSON in and out,
//! errors as `{"error": …}`, the token, and an event stream.

use std::convert::Infallible;
use std::time::Duration;

use bytes::Bytes;
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Request, Response, StatusCode};
use serde_json::{json, Value};

pub type Body = http_body_util::combinators::BoxBody<Bytes, Infallible>;
pub type Reply = Response<Body>;

/// The most a request's body may be
const MOST: usize = 1024 * 1024;

pub fn full(bytes: impl Into<Bytes>) -> Body {
    Full::new(bytes.into()).boxed()
}

pub fn json(status: StatusCode, value: &Value) -> Reply {
    let mut text = serde_json::to_string_pretty(value).unwrap_or_default();
    text.push('\n');
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .header("cache-control", "no-store")
        .body(full(text))
        .expect("a valid response")
}

pub fn ok(value: &Value) -> Reply {
    json(StatusCode::OK, value)
}

pub fn error(status: StatusCode, message: impl Into<String>) -> Reply {
    json(status, &json!({ "error": message.into() }))
}

pub fn not_found() -> Reply {
    error(StatusCode::NOT_FOUND, "no such thing here")
}

pub fn text(status: StatusCode, content_type: &str, body: String) -> Reply {
    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("cache-control", "no-store")
        .body(full(body))
        .expect("a valid response")
}

/// The caller's token, matched against the server's: the name it goes by.
/// Compared without stopping at the first byte that differs.
pub fn who<'a>(request: &Request<Incoming>, tokens: &'a [(String, String)]) -> Option<&'a str> {
    let given = request
        .headers()
        .get("authorization")?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?
        .trim()
        .as_bytes();
    tokens
        .iter()
        .find(|(_, token)| same(token.as_bytes(), given))
        .map(|(name, _)| name.as_str())
}

fn same(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// The body, as JSON; an empty one is `{}`. Boxed, the error: a whole
/// response is large to carry through every `?`
pub async fn body(request: Request<Incoming>) -> Result<Value, Box<Reply>> {
    let bytes = read(request).await?;
    if bytes.iter().all(|b| b.is_ascii_whitespace()) {
        return Ok(json!({}));
    }
    serde_json::from_slice(&bytes).map_err(|e| {
        Box::new(error(
            StatusCode::BAD_REQUEST,
            format!("the body isn't JSON: {e}"),
        ))
    })
}

/// The body's bytes, up to `MOST`
pub async fn read(request: Request<Incoming>) -> Result<Bytes, Box<Reply>> {
    let limited = http_body_util::Limited::new(request.into_body(), MOST);
    match limited.collect().await {
        Ok(collected) => Ok(collected.to_bytes()),
        Err(_) => Err(Box::new(error(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the body is over 1 MB, or it broke off",
        ))),
    }
}

/// The query's `name=value` pairs, decoded
pub fn query(request: &Request<Incoming>) -> Vec<(String, String)> {
    let Some(query) = request.uri().query() else {
        return Vec::new();
    };
    reqwest::Url::parse(&format!("http://x/?{query}"))
        .map(|url| {
            url.query_pairs()
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect()
        })
        .unwrap_or_default()
}

/// One Server-Sent Event, as the stream writes it
pub fn event(id: u64, kind: &str, data: &str) -> String {
    format!("id: {id}\nevent: {kind}\ndata: {data}\n\n")
}

/// A stream of Server-Sent Events, fed by whoever holds the sender: what
/// it sends goes out as it is (see `event`), and a comment every
/// `KEEP_ALIVE` keeps proxies from closing a quiet one. The stream ends
/// when the sender is dropped; the sender learns the reader left when a
/// send fails.
pub fn events(rx: tokio::sync::mpsc::Receiver<String>) -> Reply {
    const KEEP_ALIVE: Duration = Duration::from_secs(15);
    let stream = futures::stream::unfold(rx, |mut rx| async move {
        let text = tokio::select! {
            text = rx.recv() => text?,
            _ = tokio::time::sleep(KEEP_ALIVE) => ": still here\n\n".to_string(),
        };
        Some((Ok::<_, Infallible>(Frame::data(Bytes::from(text))), rx))
    });
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-store")
        .header("x-accel-buffering", "no")
        .body(StreamBody::new(stream).boxed())
        .expect("a valid response")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_compare_whole() {
        assert!(same(b"abcdef", b"abcdef"));
        assert!(!same(b"abcdef", b"abcdeg"));
        assert!(!same(b"abc", b"abcdef"));
    }
}
