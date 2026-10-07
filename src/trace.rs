//! What a response says about itself: `Server-Timing` segments, and the id
//! a backend gave the request, to look it up in the server's logs

use std::sync::OnceLock;

use reqwest::header::HeaderName;

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
pub fn request_id<'h>(
    headers: impl Iterator<Item = (&'h [u8], &'h [u8])>,
) -> Option<(&'static str, Box<str>)> {
    // One pass over the response's few headers, instead of a lookup for
    // each of the twelve names: this runs for every response. Most
    // headers are ruled out by their length alone.
    const LENGTHS: u64 = crate::utils::lengths(&ID_HEADERS);
    let custom = CUSTOM_ID_HEADER.get();
    let mut best: Option<(usize, &str)> = None;
    for (name, value) in headers {
        if let Some((_, label)) =
            custom.filter(|(_, label)| label.as_bytes().eq_ignore_ascii_case(name))
        {
            // The header named on the command line wins over the list
            let value = std::str::from_utf8(value).unwrap_or("").trim();
            if !value.is_empty() {
                return Some((label, value.into()));
            }
        }
        if LENGTHS & (1 << name.len().min(63)) == 0 {
            continue;
        }
        let Some(rank) = ID_HEADERS
            .iter()
            .position(|h| h.as_bytes().eq_ignore_ascii_case(name))
        else {
            continue;
        };
        if best.is_some_and(|(found, _)| found <= rank) {
            continue;
        }
        let value = std::str::from_utf8(value).ok();
        if let Some(id) = value.and_then(|v| id_in(ID_HEADERS[rank], v)) {
            best = Some((rank, id));
        }
    }
    best.map(|(rank, id)| (ID_HEADERS[rank], id.into()))
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
pub fn server_timing<'h>(
    headers: impl Iterator<Item = (&'h [u8], &'h [u8])>,
) -> Option<Box<[ServerTiming]>> {
    // Most responses have none, and an empty Vec costs nothing
    let mut entries = Vec::new();
    for (name, value) in headers {
        if name.len() == "server-timing".len() && name.eq_ignore_ascii_case(b"server-timing") {
            parse_server_timing(value, &mut entries);
        }
    }
    (!entries.is_empty()).then(|| entries.into_boxed_slice())
}

fn parse_server_timing(value: &[u8], entries: &mut Vec<ServerTiming>) {
    let Ok(value) = std::str::from_utf8(value) else {
        return;
    };
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
    use crate::response::fields;
    use reqwest::header::{HeaderMap, HeaderValue};

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
        assert_eq!(
            request_id(fields(&h)),
            Some(("x-request-id", "req-42".into()))
        );
        let h = headers(&[(
            "traceparent",
            "00-0af7651916cd43dd8448eb211c80319c-b7ad6b7169203331-01",
        )]);
        assert_eq!(
            request_id(fields(&h)),
            Some(("traceparent", "0af7651916cd43dd8448eb211c80319c".into()))
        );
        let h = headers(&[(
            "x-amzn-trace-id",
            "Root=1-67891233-abcdef012345678912345678;Sampled=1",
        )]);
        assert_eq!(
            request_id(fields(&h)).unwrap().1.as_ref(),
            "1-67891233-abcdef012345678912345678"
        );
        let h = headers(&[(
            "x-cloud-trace-context",
            "105445aa7843bc8bf206b12000100000/1;o=1",
        )]);
        assert_eq!(
            request_id(fields(&h)).unwrap().1.as_ref(),
            "105445aa7843bc8bf206b12000100000"
        );
        assert_eq!(
            request_id(fields(&headers(&[("x-request-id", "  ")]))),
            None
        );
        assert_eq!(
            request_id(fields(&headers(&[("content-type", "text/plain")]))),
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
        let entries = server_timing(fields(&h)).unwrap();
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
        assert!(server_timing(fields(&headers(&[("server-timing", " , ")]))).is_none());
        assert!(server_timing(fields(&headers(&[]))).is_none());
    }
}
