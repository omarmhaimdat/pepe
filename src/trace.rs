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

// ─── Sending a trace with each request ──────────────────────────────────────

/// `--otlp`: the high half of every trace id this run sends. The service's
/// spans come back with it, which tells them from the spans of anything
/// else it served, and the low half says which request they belong to.
static PROPAGATE: OnceLock<u64> = OnceLock::new();

/// Send a `traceparent` with every request from now on. Called once,
/// before any request is sent; the run's half of the trace ids.
pub fn propagate() -> u64 {
    *PROPAGATE.get_or_init(|| {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        mix(now.as_nanos() as u64 ^ ((std::process::id() as u64) << 32)) | 1
    })
}

/// The run's half of the trace ids, when requests carry one
pub fn propagating() -> Option<u64> {
    PROPAGATE.get().copied()
}

/// The header a trace travels in (W3C Trace Context)
pub const TRACEPARENT: HeaderName = HeaderName::from_static("traceparent");

/// The length of a `traceparent` value: `00-<32 hex>-<16 hex>-01`
pub const TRACEPARENT_LEN: usize = 55;

/// A `traceparent` of the right length, written into each request in turn
pub const TRACEPARENT_BLANK: &str = "00-00000000000000000000000000000000-0000000000000000-01";

/// Request number `turn`'s half of its trace id. Spread over the 64 bits,
/// so a service that samples by the id's low bytes samples evenly. Never
/// 0: only 0 mixes to 0.
pub fn trace_of(turn: u64) -> u64 {
    mix(turn.wrapping_add(1))
}

/// The id of pepe's side of trace `trace`: the span the service's first
/// span names as its parent, which is how its root is found
pub fn client_span(trace: u64) -> u64 {
    mix(trace ^ 0x9e37_79b9_7f4a_7c15) | 1
}

/// The `traceparent` value for the trace `trace` of run `run`; the last
/// flag asks the service to keep it, whatever it samples otherwise
pub fn traceparent(run: u64, trace: u64) -> [u8; TRACEPARENT_LEN] {
    let mut out = [0u8; TRACEPARENT_LEN];
    out.copy_from_slice(TRACEPARENT_BLANK.as_bytes());
    hex_into(&mut out[3..19], run);
    hex_into(&mut out[19..35], trace);
    hex_into(&mut out[36..52], client_span(trace));
    out
}

/// A trace id as it is written: 32 hex digits
pub fn trace_id_hex(run: u64, trace: u64) -> String {
    format!("{run:016x}{trace:016x}")
}

fn hex_into(out: &mut [u8], value: u64) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = DIGITS[((value >> (60 - 4 * i)) & 0xf) as usize];
    }
}

/// splitmix64's finalizer: a bijection, so no two requests share an id
fn mix(mut x: u64) -> u64 {
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^ (x >> 31)
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
