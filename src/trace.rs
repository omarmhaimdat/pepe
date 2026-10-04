//! What a response says about itself: `Server-Timing` segments, and the id
//! a backend gave the request, to look it up in the server's logs

use std::sync::OnceLock;

use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

/// Response headers that carry a request or trace id, most specific first.
/// The value is taken whole, except where noted in `id_in`.
const ID_HEADERS: [&str; 12] = [
    "x-request-id",
    "request-id",
    "x-correlation-id",
    "x-amzn-requestid",
    "x-amzn-trace-id",
    "cf-ray",
    "x-vercel-id",
    "fly-request-id",
    "x-cloud-trace-context",
    "x-b3-traceid",
    "x-trace-id",
    "traceparent",
];

/// `--trace-header`: a header named on the command line is looked at first
static CUSTOM_ID_HEADER: OnceLock<(HeaderName, &'static str)> = OnceLock::new();

/// Name the header that carries the request id for this run. Set once,
/// before any request is sent.
pub fn use_id_header(name: HeaderName) {
    let label = Box::leak(name.as_str().to_owned().into_boxed_str());
    let _ = CUSTOM_ID_HEADER.set((name, label));
}

/// The request's id, as the backend knows it, and the header it came from
pub fn request_id(headers: &HeaderMap) -> Option<(&'static str, Box<str>)> {
    if let Some((name, label)) = CUSTOM_ID_HEADER.get() {
        if let Some(value) = headers.get(name).and_then(|v| v.to_str().ok()) {
            let value = value.trim();
            if !value.is_empty() {
                return Some((label, value.into()));
            }
        }
    }
    ID_HEADERS.iter().find_map(|&name| {
        let value = headers.get(name)?.to_str().ok()?;
        let id = id_in(name, value)?;
        Some((name, id.into()))
    })
}

/// The id part of a header's value: most carry the id alone, a few wrap it
fn id_in<'v>(name: &str, value: &'v str) -> Option<&'v str> {
    let value = value.trim();
    let id = match name {
        // "00-<trace-id>-<parent-id>-01": the trace id is what logs index
        "traceparent" => value.split('-').nth(1)?,
        // "Root=1-67891233-abcdef012345678912345678;Parent=…;Sampled=1"
        "x-amzn-trace-id" => value
            .split(';')
            .find_map(|part| part.trim().strip_prefix("Root="))
            .unwrap_or(value),
        // "<trace-id>/<span-id>;o=1"
        "x-cloud-trace-context" => value.split(['/', ';']).next()?,
        _ => value,
    };
    (!id.is_empty()).then_some(id)
}

/// One `Server-Timing` entry: `db;dur=53.2;desc="primary"`
#[derive(Debug, Clone, PartialEq)]
pub struct ServerTiming {
    pub name: Box<str>,
    /// Milliseconds, when the entry has a `dur`
    pub dur_ms: Option<f64>,
    pub desc: Option<Box<str>>,
}

/// Every entry of every `Server-Timing` header, in order; None when the
/// response has none
pub fn server_timing(headers: &HeaderMap) -> Option<Box<[ServerTiming]>> {
    let mut entries = Vec::new();
    for value in headers.get_all("server-timing") {
        parse_server_timing(value, &mut entries);
    }
    (!entries.is_empty()).then(|| entries.into_boxed_slice())
}

fn parse_server_timing(value: &HeaderValue, entries: &mut Vec<ServerTiming>) {
    let Ok(value) = value.to_str() else { return };
    for entry in split_unquoted(value, ',') {
        let mut parts = split_unquoted(entry, ';');
        let Some(name) = parts.next().map(str::trim).filter(|n| !n.is_empty()) else {
            continue;
        };
        let mut timing = ServerTiming {
            name: name.into(),
            dur_ms: None,
            desc: None,
        };
        for param in parts {
            let Some((key, val)) = param.split_once('=') else {
                continue;
            };
            let val = val.trim().trim_matches('"');
            match key.trim().to_ascii_lowercase().as_str() {
                "dur" => timing.dur_ms = val.parse().ok(),
                "desc" => timing.desc = Some(val.into()),
                _ => {}
            }
        }
        entries.push(timing);
    }
}

/// Split on `sep` outside double quotes, since a `desc` may hold either
fn split_unquoted(s: &str, sep: char) -> impl Iterator<Item = &str> {
    let mut quoted = false;
    s.split(move |c: char| {
        if c == '"' {
            quoted = !quoted;
        }
        c == sep && !quoted
    })
    .map(str::trim)
    .filter(|part| !part.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut h = HeaderMap::new();
        for (name, value) in pairs {
            h.append(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        h
    }

    #[test]
    fn the_id_is_taken_from_the_most_specific_header() {
        let h = headers(&[
            (
                "traceparent",
                "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
            ),
            ("x-request-id", " req-42 "),
        ]);
        assert_eq!(request_id(&h), Some(("x-request-id", "req-42".into())));
        let h = headers(&[(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        )]);
        assert_eq!(
            request_id(&h),
            Some(("traceparent", "0af7651916cd43dd8448eb211c80319c".into()))
        );
        let h = headers(&[(
            "x-amzn-trace-id",
            "Root=1-67891233-abcdef012345678912345678;Sampled=1",
        )]);
        assert_eq!(
            request_id(&h).unwrap().1.as_ref(),
            "1-67891233-abcdef012345678912345678"
        );
        let h = headers(&[(
            "x-cloud-trace-context",
            "105445aa7843bc8bf206b12000100000/1;o=1",
        )]);
        assert_eq!(
            request_id(&h).unwrap().1.as_ref(),
            "105445aa7843bc8bf206b12000100000"
        );
        assert_eq!(request_id(&headers(&[("x-request-id", "  ")])), None);
        assert_eq!(
            request_id(&headers(&[("content-type", "text/plain")])),
            None
        );
    }

    #[test]
    fn server_timing_entries_keep_their_order_and_survive_quoted_commas() {
        let h = headers(&[
            (
                "server-timing",
                "db;dur=53.2, app;dur=47;desc=\"render, twice\"",
            ),
            ("server-timing", "cache;desc=HIT, miss"),
        ]);
        let entries = server_timing(&h).unwrap();
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_ref()).collect();
        assert_eq!(names, ["db", "app", "cache", "miss"]);
        assert_eq!(entries[0].dur_ms, Some(53.2));
        assert_eq!(entries[1].desc.as_deref(), Some("render, twice"));
        assert_eq!(
            (entries[2].dur_ms, entries[2].desc.as_deref()),
            (None, Some("HIT"))
        );
        assert_eq!(
            entries[3],
            ServerTiming {
                name: "miss".into(),
                dur_ms: None,
                desc: None
            }
        );
        assert!(server_timing(&headers(&[("server-timing", " , ")])).is_none());
        assert!(server_timing(&headers(&[])).is_none());
    }
}
