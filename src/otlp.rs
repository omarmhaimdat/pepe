//! `--otlp`: the service's own account of each request. pepe sends every
//! request with a `traceparent` of its own making (see `trace::propagate`),
//! takes the spans the service exports for them over OTLP/HTTP, and says
//! where the server's time went: which operation, at the median and in the
//! slowest requests.
//!
//! The receiver takes what an OpenTelemetry SDK's exporter sends to
//! `/v1/traces`, as protobuf or JSON. The protobuf is read by hand: the few
//! fields used here are a page of code, where a generated decoder would be
//! a dependency and a build step.

use std::collections::{HashMap, HashSet};
use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::metrics::Histogram;
use crate::trace;

/// Spans kept for the analysis; past this they are counted and dropped.
/// A span is 48 bytes, so this is 24 MB at most.
const MAX_SPANS: usize = 500_000;
/// The largest export taken: an SDK's batch is 512 spans, well under this
const MAX_BODY: usize = 16 * 1024 * 1024;
const MAX_HEAD: usize = 64 * 1024;
/// SDKs export a batch every five seconds, so the last requests' spans can
/// be that long behind them
const SETTLE_AT_LEAST: Duration = Duration::from_millis(5_500);
const SETTLE_AT_MOST: Duration = Duration::from_secs(15);
/// Once this long has passed with nothing new, after the least, it's over
const SETTLE_QUIET: Duration = Duration::from_millis(1_500);
/// Operations listed in the report
const OPERATIONS_KEPT: usize = 15;

static RECEIVER: OnceLock<Receiver> = OnceLock::new();

struct Receiver {
    addr: SocketAddr,
    /// The run's half of the trace ids
    run: u64,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    spans: Vec<Span>,
    ops: Vec<Op>,
    index: HashMap<Op, u32>,
    /// Spans whose parent is pepe's side of the trace: one per request the
    /// service answered and kept
    roots: u64,
    /// Spans of traces pepe didn't start
    other: u64,
    dropped: u64,
    /// Why exports were turned away, each said once
    refused: Vec<String>,
    last: Option<Instant>,
}

/// What a span did: the service, the span's name and its kind
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Op {
    pub service: Option<Box<str>>,
    pub name: Box<str>,
    pub kind: u8,
}

impl Op {
    fn kind_name(&self) -> &'static str {
        match self.kind {
            1 => "internal",
            2 => "server",
            3 => "client",
            4 => "producer",
            5 => "consumer",
            _ => "unspecified",
        }
    }

    /// `checkout: SELECT orders`
    pub fn label(&self) -> String {
        match &self.service {
            Some(service) => format!("{service}: {}", self.name),
            None => self.name.to_string(),
        }
    }
}

/// A span of one of the run's traces, as kept
#[derive(Debug, Clone, Copy, PartialEq)]
struct Span {
    /// The low half of its trace id: which request it belongs to
    trace: u64,
    id: u64,
    parent: u64,
    op: u32,
    /// Nanoseconds since the epoch
    start: u64,
    end: u64,
    error: bool,
}

/// A span as it is read from an export
#[derive(Debug, Default)]
struct Incoming<'a> {
    trace: [u8; 16],
    id: u64,
    parent: u64,
    name: &'a str,
    kind: u8,
    start: u64,
    end: u64,
    error: bool,
}

/// Start taking spans at `spec` (`:4318`, `4318`, `127.0.0.1:4318`), and
/// send a trace with every request from now on. The address it listens on.
pub async fn start(spec: &str) -> Result<SocketAddr, String> {
    let addr = crate::exporter::parse(spec)?;
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        format!(
            "can't listen on {addr}: {}",
            crate::response::root_cause(&e)
        )
    })?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let receiver = Receiver {
        addr,
        run: trace::propagate(),
        state: Mutex::new(State::default()),
    };
    if RECEIVER.set(receiver).is_err() {
        return Err("spans are already being taken".into());
    }
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(serve(socket));
        }
    });
    Ok(addr)
}

/// Where spans are taken, when they are
pub fn address() -> Option<SocketAddr> {
    RECEIVER.get().map(|r| r.addr)
}

fn state() -> Option<std::sync::MutexGuard<'static, State>> {
    RECEIVER
        .get()
        .map(|r| r.state.lock().unwrap_or_else(|e| e.into_inner()))
}

// ─── The receiver ────────────────────────────────────────────────────────────

/// One exporter's connection: requests one after another, kept open
async fn serve(mut socket: TcpStream) {
    let mut buf: Vec<u8> = Vec::with_capacity(8 * 1024);
    loop {
        let request = loop {
            match read_request(&buf) {
                Ok(Some(request)) => break request,
                Ok(None) => {}
                Err(status) => {
                    let _ = respond(&mut socket, status, false, true).await;
                    return;
                }
            }
            let mut chunk = [0u8; 16 * 1024];
            let read = tokio::time::timeout(Duration::from_secs(30), socket.read(&mut chunk));
            match read.await {
                Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&chunk[..n]),
                _ => return,
            }
        };
        let status = match (request.path.as_str(), request.gzip) {
            ("/v1/traces", true) => {
                refuse("an export came compressed: set OTEL_EXPORTER_OTLP_COMPRESSION=none on the service");
                "415 Unsupported Media Type"
            }
            ("/v1/traces", false) => {
                let body = &request.body;
                let read = match request.json {
                    true => take_json(body),
                    false => take_protobuf(body),
                };
                match read {
                    Ok(()) => "200 OK",
                    Err(why) => {
                        refuse(&format!("an export couldn't be read: {why}"));
                        "400 Bad Request"
                    }
                }
            }
            // The same endpoint takes every signal: metrics and logs are
            // answered and set aside, so the service doesn't log errors
            ("/v1/metrics" | "/v1/logs", _) => "200 OK",
            _ => "404 Not Found",
        };
        buf.drain(..request.len);
        if respond(&mut socket, status, request.json, request.close)
            .await
            .is_err()
            || request.close
        {
            return;
        }
    }
}

struct Posted {
    path: String,
    body: Vec<u8>,
    json: bool,
    gzip: bool,
    close: bool,
    /// How much of the buffer it took
    len: usize,
}

/// A whole request at the start of `buf`, None until it has all come
fn read_request(buf: &[u8]) -> Result<Option<Posted>, &'static str> {
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut request = httparse::Request::new(&mut headers);
    let head = match request.parse(buf) {
        Ok(httparse::Status::Complete(len)) => len,
        Ok(httparse::Status::Partial) if buf.len() > MAX_HEAD => {
            return Err("431 Request Header Fields Too Large")
        }
        Ok(httparse::Status::Partial) => return Ok(None),
        Err(_) => return Err("400 Bad Request"),
    };
    let path = request.path.unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path).to_string();
    let mut length = 0usize;
    let mut chunked = false;
    let mut json = false;
    let mut gzip = false;
    let mut close = request.version == Some(0);
    for header in request.headers.iter() {
        let value = std::str::from_utf8(header.value).unwrap_or("").trim();
        match header.name.to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().map_err(|_| "400 Bad Request")?,
            "transfer-encoding" => chunked = value.eq_ignore_ascii_case("chunked"),
            "content-type" => json = value.starts_with("application/json"),
            "content-encoding" => gzip = !value.eq_ignore_ascii_case("identity"),
            "connection" => close = value.eq_ignore_ascii_case("close"),
            _ => {}
        }
    }
    if length > MAX_BODY {
        return Err("413 Payload Too Large");
    }
    let (body, len) = if chunked {
        match dechunk(&buf[head..])? {
            Some((body, used)) => (body, head + used),
            None => return Ok(None),
        }
    } else {
        if buf.len() < head + length {
            return Ok(None);
        }
        (buf[head..head + length].to_vec(), head + length)
    };
    Ok(Some(Posted {
        path,
        body,
        json,
        gzip,
        close,
        len,
    }))
}

/// A chunked body at the start of `buf`, and how much of it it took
fn dechunk(buf: &[u8]) -> Result<Option<(Vec<u8>, usize)>, &'static str> {
    let line_end = |from: usize| {
        buf[from..]
            .windows(2)
            .position(|w| w == b"\r\n")
            .map(|p| from + p)
    };
    let mut body = Vec::new();
    let mut at = 0;
    loop {
        let Some(end) = line_end(at) else {
            return Ok(None);
        };
        let size = std::str::from_utf8(&buf[at..end]).map_err(|_| "400 Bad Request")?;
        let size = size.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size, 16).map_err(|_| "400 Bad Request")?;
        at = end + 2;
        if size == 0 {
            // Trailers, if any, then the empty line
            loop {
                let Some(end) = line_end(at) else {
                    return Ok(None);
                };
                let empty = end == at;
                at = end + 2;
                if empty {
                    return Ok(Some((body, at)));
                }
            }
        }
        if body.len() + size > MAX_BODY {
            return Err("413 Payload Too Large");
        }
        if buf.len() < at + size + 2 {
            return Ok(None);
        }
        body.extend_from_slice(&buf[at..at + size]);
        at += size + 2;
    }
}

/// An empty export response: the SDK reads it as all spans taken
async fn respond(
    socket: &mut TcpStream,
    status: &str,
    json: bool,
    close: bool,
) -> std::io::Result<()> {
    let ok = status.starts_with("200");
    let (kind, body) = match (ok, json) {
        (true, true) => ("application/json", "{}"),
        (true, false) => ("application/x-protobuf", ""),
        (false, _) => ("text/plain; charset=utf-8", status),
    };
    let connection = if close { "close" } else { "keep-alive" };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nConnection: {connection}\r\n\r\n{body}",
        body.len()
    );
    socket.write_all(response.as_bytes()).await
}

fn refuse(why: &str) {
    if let Some(mut state) = state() {
        if !state.refused.iter().any(|r| r == why) {
            state.refused.push(why.to_string());
        }
    }
}

/// Keep `spans`, from a resource whose `service.name` is `service`
fn keep<'a>(service: Option<&str>, spans: impl IntoIterator<Item = Incoming<'a>>) {
    let Some(receiver) = RECEIVER.get() else {
        return;
    };
    let mut state = receiver.state.lock().unwrap_or_else(|e| e.into_inner());
    state.last = Some(Instant::now());
    for span in spans {
        let high = u64::from_be_bytes(span.trace[..8].try_into().expect("8 bytes"));
        if high != receiver.run {
            state.other += 1;
            continue;
        }
        if state.spans.len() >= MAX_SPANS {
            state.dropped += 1;
            continue;
        }
        let trace = u64::from_be_bytes(span.trace[8..].try_into().expect("8 bytes"));
        let op = Op {
            service: service.map(Into::into),
            name: span.name.into(),
            kind: span.kind,
        };
        let op = match state.index.get(&op) {
            Some(&index) => index,
            None => {
                let index = state.ops.len() as u32;
                state.ops.push(op.clone());
                state.index.insert(op, index);
                index
            }
        };
        if span.parent == trace::client_span(trace) {
            state.roots += 1;
        }
        state.spans.push(Span {
            trace,
            id: span.id,
            parent: span.parent,
            op,
            start: span.start,
            end: span.end.max(span.start),
            error: span.error,
        });
    }
}

// ─── OTLP, as protobuf ───────────────────────────────────────────────────────

/// A protobuf field's value, by its wire type
enum Wire<'a> {
    Varint(u64),
    Fixed64(u64),
    Bytes(&'a [u8]),
    Fixed32,
}

/// The fields of one protobuf message, in order
struct Fields<'a> {
    buf: &'a [u8],
}

impl<'a> Fields<'a> {
    fn of(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn varint(&mut self) -> Result<u64, &'static str> {
        let mut value = 0u64;
        for shift in (0..64).step_by(7) {
            let (&byte, rest) = self.buf.split_first().ok_or("a number is cut short")?;
            self.buf = rest;
            value |= u64::from(byte & 0x7f) << shift;
            if byte & 0x80 == 0 {
                return Ok(value);
            }
        }
        Err("a number is too long")
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], &'static str> {
        if self.buf.len() < n {
            return Err("a field is cut short");
        }
        let (taken, rest) = self.buf.split_at(n);
        self.buf = rest;
        Ok(taken)
    }

    fn next(&mut self) -> Result<Option<(u64, Wire<'a>)>, &'static str> {
        if self.buf.is_empty() {
            return Ok(None);
        }
        let key = self.varint()?;
        let value = match key & 7 {
            0 => Wire::Varint(self.varint()?),
            1 => Wire::Fixed64(u64::from_le_bytes(
                self.take(8)?.try_into().expect("8 bytes"),
            )),
            2 => {
                let n = self.varint()? as usize;
                Wire::Bytes(self.take(n)?)
            }
            5 => {
                self.take(4)?;
                Wire::Fixed32
            }
            _ => return Err("a field of a kind protobuf doesn't have"),
        };
        Ok(Some((key >> 3, value)))
    }
}

/// An `ExportTraceServiceRequest`
fn take_protobuf(body: &[u8]) -> Result<(), &'static str> {
    let mut request = Fields::of(body);
    while let Some((field, value)) = request.next()? {
        // resource_spans
        if let (1, Wire::Bytes(resource_spans)) = (field, value) {
            take_resource_spans(resource_spans)?;
        }
    }
    Ok(())
}

fn take_resource_spans(buf: &[u8]) -> Result<(), &'static str> {
    let mut service = None;
    let mut scopes = Vec::new();
    let mut fields = Fields::of(buf);
    while let Some((field, value)) = fields.next()? {
        match (field, value) {
            // resource, whose attributes name the service
            (1, Wire::Bytes(resource)) => service = service_name(resource)?,
            // scope_spans
            (2, Wire::Bytes(scope)) => scopes.push(scope),
            _ => {}
        }
    }
    let mut spans = Vec::new();
    for scope in scopes {
        let mut fields = Fields::of(scope);
        while let Some((field, value)) = fields.next()? {
            if let (2, Wire::Bytes(span)) = (field, value) {
                spans.push(read_span(span)?);
            }
        }
    }
    keep(service, spans);
    Ok(())
}

/// A `Resource`'s `service.name`
fn service_name(resource: &[u8]) -> Result<Option<&str>, &'static str> {
    let mut fields = Fields::of(resource);
    while let Some((field, value)) = fields.next()? {
        let (1, Wire::Bytes(pair)) = (field, value) else {
            continue;
        };
        let (mut key, mut text) = (None, None);
        let mut pair = Fields::of(pair);
        while let Some((field, value)) = pair.next()? {
            match (field, value) {
                (1, Wire::Bytes(k)) => key = Some(k),
                // AnyValue, whose string_value is field 1
                (2, Wire::Bytes(any)) => {
                    let mut any = Fields::of(any);
                    while let Some((field, value)) = any.next()? {
                        if let (1, Wire::Bytes(s)) = (field, value) {
                            text = Some(s);
                        }
                    }
                }
                _ => {}
            }
        }
        if key == Some(b"service.name".as_slice()) {
            return Ok(text.and_then(|t| std::str::from_utf8(t).ok()));
        }
    }
    Ok(None)
}

fn read_span(buf: &[u8]) -> Result<Incoming<'_>, &'static str> {
    let mut span = Incoming::default();
    let mut fields = Fields::of(buf);
    let id = |bytes: &[u8]| -> u64 {
        match bytes.len() {
            8 => u64::from_be_bytes(bytes.try_into().expect("8 bytes")),
            _ => 0,
        }
    };
    while let Some((field, value)) = fields.next()? {
        match (field, value) {
            (1, Wire::Bytes(t)) if t.len() == 16 => span.trace.copy_from_slice(t),
            (2, Wire::Bytes(s)) => span.id = id(s),
            (4, Wire::Bytes(p)) => span.parent = id(p),
            (5, Wire::Bytes(n)) => span.name = std::str::from_utf8(n).unwrap_or("?"),
            (6, Wire::Varint(k)) => span.kind = k.min(255) as u8,
            (7, Wire::Fixed64(t)) => span.start = t,
            (8, Wire::Fixed64(t)) => span.end = t,
            // Status, whose code is field 3; 2 is an error
            (15, Wire::Bytes(status)) => {
                let mut status = Fields::of(status);
                while let Some((field, value)) = status.next()? {
                    if let (3, Wire::Varint(code)) = (field, value) {
                        span.error = code == 2;
                    }
                }
            }
            _ => {}
        }
    }
    Ok(span)
}

// ─── OTLP, as JSON ───────────────────────────────────────────────────────────

/// The same export in OTLP's JSON: ids in hex, times as strings or numbers
fn take_json(body: &[u8]) -> Result<(), &'static str> {
    use serde_json::Value;
    let request: Value = serde_json::from_slice(body).map_err(|_| "not JSON")?;
    let list = |v: &Value, key: &str| -> Vec<Value> {
        v.get(key)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
    };
    let number = |v: Option<&Value>| -> u64 {
        match v {
            Some(Value::String(s)) => s.parse().unwrap_or(0),
            Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
            _ => 0,
        }
    };
    let hex = |v: Option<&Value>, out: &mut [u8]| -> bool {
        let Some(text) = v.and_then(Value::as_str) else {
            return false;
        };
        if text.len() != out.len() * 2 {
            return false;
        }
        for (i, byte) in out.iter_mut().enumerate() {
            match u8::from_str_radix(&text[2 * i..2 * i + 2], 16) {
                Ok(b) => *byte = b,
                Err(_) => return false,
            }
        }
        true
    };
    for resource_spans in list(&request, "resourceSpans") {
        let service = resource_spans
            .get("resource")
            .map(|r| list(r, "attributes"))
            .unwrap_or_default()
            .into_iter()
            .find(|a| a.get("key").and_then(Value::as_str) == Some("service.name"))
            .and_then(|a| {
                a.pointer("/value/stringValue")
                    .and_then(Value::as_str)
                    .map(str::to_string)
            });
        let mut names = Vec::new();
        let mut spans = Vec::new();
        for scope in list(&resource_spans, "scopeSpans") {
            for span in list(&scope, "spans") {
                let mut incoming = Incoming::default();
                if !hex(span.get("traceId"), &mut incoming.trace) {
                    continue;
                }
                let mut id = [0u8; 8];
                if hex(span.get("spanId"), &mut id) {
                    incoming.id = u64::from_be_bytes(id);
                }
                let mut parent = [0u8; 8];
                if hex(span.get("parentSpanId"), &mut parent) {
                    incoming.parent = u64::from_be_bytes(parent);
                }
                incoming.kind = number(span.get("kind")).min(255) as u8;
                incoming.start = number(span.get("startTimeUnixNano"));
                incoming.end = number(span.get("endTimeUnixNano"));
                incoming.error = number(span.pointer("/status/code")) == 2;
                names.push(
                    span.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("?")
                        .to_string(),
                );
                spans.push(incoming);
            }
        }
        keep(
            service.as_deref(),
            spans.into_iter().zip(&names).map(|(mut span, name)| {
                span.name = name;
                span
            }),
        );
    }
    Ok(())
}

// ─── After the run ───────────────────────────────────────────────────────────

/// Wait for the spans of the run's last requests: until every request in
/// `expected` has its root, or the exporters have had time to send theirs
pub async fn settle(expected: u64) {
    let Some(receiver) = RECEIVER.get() else {
        return;
    };
    let began = Instant::now();
    let mut said = false;
    loop {
        let (roots, last) = {
            let state = receiver.state.lock().unwrap_or_else(|e| e.into_inner());
            (state.roots, state.last)
        };
        let waited = began.elapsed();
        let quiet = last.is_none_or(|at| at.elapsed() >= SETTLE_QUIET);
        if roots >= expected || waited >= SETTLE_AT_MOST || (waited >= SETTLE_AT_LEAST && quiet) {
            return;
        }
        if !said {
            said = true;
            eprintln!(
                "waiting for the service's spans on {} ({roots} of {expected} requests so far)",
                receiver.addr
            );
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// What a run asks of the spans once it is over
pub struct Query {
    /// Requests measured that the service answered, and requests sent
    /// during the warm-up
    pub requests: u64,
    pub warmup: u64,
    /// The warm-up's traces
    pub excluded: HashSet<u64>,
    /// The slowest requests' traces, broken down one by one
    pub keep: Vec<u64>,
}

impl Query {
    /// Wait for the last requests' spans, then break them all down
    pub async fn answer(self) -> Option<Breakdown> {
        settle(self.requests + self.warmup).await;
        analyse(self.requests, &self.excluded, &self.keep)
    }
}

/// Where the server's time went, from the spans of the run's requests
#[derive(Debug, Default, Clone)]
pub struct Breakdown {
    /// The run's half of the trace ids
    pub run: u64,
    pub addr: Option<SocketAddr>,
    /// Requests measured that the service answered, and how many of them
    /// it sent spans for
    pub requests: u64,
    pub traces: u64,
    pub spans: u64,
    pub other: u64,
    pub dropped: u64,
    pub refused: Vec<String>,
    /// The server's time for each request: its first span, start to end,
    /// in microseconds
    pub server: Histogram,
    /// The slowest requests' traces are those at or over this, in µs
    pub tail_from: u64,
    pub tail_traces: u64,
    /// Most of the slowest requests' time first
    pub operations: Vec<Operation>,
    /// The traces asked for by `analyse`'s `keep`
    pub kept: HashMap<u64, TraceTime>,
}

#[derive(Debug, Clone)]
pub struct Operation {
    pub op: Op,
    pub calls: u64,
    pub errors: u64,
    /// Each call, start to end, in µs
    pub durations: Histogram,
    /// Its own time (less its children's) as a share of the server's time,
    /// over all requests and over the slowest
    pub share: f64,
    pub tail_share: f64,
}

/// One request's server time and what it was spent on, most first
#[derive(Debug, Clone, PartialEq)]
pub struct TraceTime {
    pub server_ns: u64,
    pub spent: Vec<(Op, u64)>,
}

/// Read the spans taken so far against the requests measured. `excluded`:
/// traces sent during the warm-up. `keep`: traces to break down one by one
/// (the slowest requests').
pub fn analyse(requests: u64, excluded: &HashSet<u64>, keep: &[u64]) -> Option<Breakdown> {
    let receiver = RECEIVER.get()?;
    let state = receiver.state.lock().unwrap_or_else(|e| e.into_inner());
    let mut breakdown = breakdown_of(&state.spans, &state.ops, requests, excluded, keep);
    breakdown.run = receiver.run;
    breakdown.addr = Some(receiver.addr);
    breakdown.other = state.other;
    breakdown.dropped = state.dropped;
    breakdown.refused = state.refused.clone();
    Some(breakdown)
}

fn breakdown_of(
    spans: &[Span],
    ops: &[Op],
    requests: u64,
    excluded: &HashSet<u64>,
    keep: &[u64],
) -> Breakdown {
    let mut spans: Vec<Span> = spans
        .iter()
        .filter(|s| !excluded.contains(&s.trace))
        .copied()
        .collect();
    spans.sort_unstable_by_key(|s| (s.trace, s.parent, s.start));

    #[derive(Default, Clone)]
    struct Tally {
        calls: u64,
        traces: u64,
        errors: u64,
        durations: Histogram,
        own: u64,
        tail_own: u64,
    }
    let mut tallies = vec![Tally::default(); ops.len()];
    let mut server = Histogram::default();
    // Each trace's server time and its operations' own time, for a second
    // pass once the slowest are known
    let mut traces: Vec<(u64, u64, std::ops::Range<usize>)> = Vec::new();
    let mut spent: Vec<(u32, u64)> = Vec::new();
    let mut kept = HashMap::new();
    let mut total_spans = 0u64;

    for group in spans.chunk_by(|a, b| a.trace == b.trace) {
        let trace = group[0].trace;
        total_spans += group.len() as u64;
        // Sorted by parent: a span's children are one run of the group
        let children = |id: u64| {
            let from = group.partition_point(|s| s.parent < id);
            let to = group.partition_point(|s| s.parent <= id);
            &group[from..to]
        };
        let client = trace::client_span(trace);
        let mut roots: Vec<&Span> = children(client).iter().collect();
        if roots.is_empty() {
            // A service that started the trace again: its spans with no
            // parent in it
            let ids: HashSet<u64> = group.iter().map(|s| s.id).collect();
            roots = group.iter().filter(|s| !ids.contains(&s.parent)).collect();
        }
        let first = roots.iter().map(|s| s.start).min().unwrap_or(0);
        let last = roots.iter().map(|s| s.end).max().unwrap_or(0);
        let server_ns = last.saturating_sub(first);
        server.record(server_ns / 1000);

        let from = spent.len();
        for span in group {
            let own = span.end - span.start - covered(span, children(span.id));
            let tally = &mut tallies[span.op as usize];
            tally.calls += 1;
            tally.errors += span.error as u64;
            tally.durations.record((span.end - span.start) / 1000);
            match spent[from..].iter_mut().find(|(op, _)| *op == span.op) {
                Some((_, time)) => *time += own,
                None => spent.push((span.op, own)),
            }
        }
        for &(op, own) in &spent[from..] {
            tallies[op as usize].traces += 1;
            tallies[op as usize].own += own;
        }
        if keep.contains(&trace) {
            let mut times: Vec<(Op, u64)> = spent[from..]
                .iter()
                .map(|&(op, own)| (ops[op as usize].clone(), own))
                .collect();
            times.sort_by_key(|t| std::cmp::Reverse(t.1));
            kept.insert(
                trace,
                TraceTime {
                    server_ns,
                    spent: times,
                },
            );
        }
        traces.push((trace, server_ns, from..spent.len()));
    }

    // The slowest: the requests at or over the server's p99
    let tail_from = server.percentile(99.0);
    let (mut all, mut tail) = (0u64, 0u64);
    let mut tail_traces = 0;
    for (_, server_ns, range) in &traces {
        all += server_ns;
        if server_ns / 1000 >= tail_from {
            tail += server_ns;
            tail_traces += 1;
            for &(op, own) in &spent[range.clone()] {
                tallies[op as usize].tail_own += own;
            }
        }
    }
    let share = |own: u64, of: u64| match of {
        0 => 0.0,
        _ => (own as f64 / of as f64 * 100.0).min(100.0),
    };
    let mut operations: Vec<Operation> = tallies
        .into_iter()
        .enumerate()
        .filter(|(_, t)| t.calls > 0)
        .map(|(index, t)| Operation {
            op: ops[index].clone(),
            calls: t.calls,
            errors: t.errors,
            durations: t.durations,
            share: share(t.own, all),
            tail_share: share(t.tail_own, tail),
        })
        .collect();
    operations.sort_by(|a, b| {
        b.tail_share
            .total_cmp(&a.tail_share)
            .then(b.share.total_cmp(&a.share))
    });

    Breakdown {
        requests,
        traces: traces.len() as u64,
        spans: total_spans,
        server,
        tail_from,
        tail_traces,
        operations,
        kept,
        ..Default::default()
    }
}

/// How much of `span` its children cover, each moment counted once
fn covered(span: &Span, children: &[Span]) -> u64 {
    let mut parts: Vec<(u64, u64)> = children
        .iter()
        .map(|c| (c.start.max(span.start), c.end.min(span.end)))
        .filter(|(start, end)| start < end)
        .collect();
    parts.sort_unstable();
    let mut total = 0;
    let mut reached = span.start;
    for (start, end) in parts {
        let start = start.max(reached);
        if end > start {
            total += end - start;
            reached = end;
        }
    }
    total
}

// ─── What it says ────────────────────────────────────────────────────────────

impl Breakdown {
    pub fn coverage(&self) -> f64 {
        match self.requests {
            0 => 0.0,
            n => (self.traces as f64 / n as f64 * 100.0).min(100.0),
        }
    }

    /// What the verdict says about the spans: whether they came, and
    /// where the time went
    pub fn notes(&self, client_median: Duration) -> Vec<crate::insights::Note> {
        use crate::insights::{Level, Note};
        let note = |level, text: String| Note { level, text };
        let addr = self
            .addr
            .map(|a| format!("http://localhost:{}", a.port()))
            .unwrap_or_else(|| "http://localhost:4318".into());
        let mut notes = Vec::new();
        for why in &self.refused {
            notes.push(note(Level::Degraded, format!("Spans: {why}.")));
        }
        if self.traces == 0 {
            let text = if self.other > 0 {
                format!(
                    "Spans arrived ({}), but none for pepe's requests: the service doesn't carry on the traceparent it is sent. Turn on W3C trace context propagation (OTEL_PROPAGATORS=tracecontext).",
                    self.other
                )
            } else {
                format!(
                    "No spans arrived. Point the service's exporter at pepe: OTEL_EXPORTER_OTLP_ENDPOINT={addr} and OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf (or http/json)."
                )
            };
            notes.push(note(Level::Degraded, text));
            return notes;
        }
        if self.coverage() < 90.0 {
            notes.push(note(
                Level::Healthy,
                format!(
                    "Spans came for {:.0}% of the requests ({} of {}): the service samples, or dropped some. The breakdown is of those that came; OTEL_TRACES_SAMPLER=parentbased_always_on keeps them all.",
                    self.coverage(),
                    self.traces,
                    self.requests
                ),
            ));
        }
        let ms = |us: u64| crate::ui::format::latency(Duration::from_micros(us));
        let server_median = self.server.percentile(50.0);
        if let Some(top) = self.operations.first() {
            let tail = if self.tail_traces < self.traces {
                format!(
                    "{:.0}% of the slowest requests' server time (server p99 {}), {:.0}% over all of them",
                    top.tail_share,
                    ms(self.tail_from),
                    top.share
                )
            } else {
                format!("{:.0}% of the server's time", top.share)
            };
            notes.push(note(
                Level::Healthy,
                format!(
                    "Where the time went: {} is {tail}; its median call takes {}.",
                    top.op.label(),
                    ms(top.durations.percentile(50.0)),
                ),
            ));
            // Something small in a typical request and large in a slow one
            // waits on something: a lock, a pool, a queue
            if let Some(grows) = self.operations.iter().skip(1).find(|o| {
                self.tail_traces < self.traces
                    && o.tail_share >= 20.0
                    && o.tail_share >= 3.0 * o.share.max(1.0)
            }) {
                notes.push(note(
                    Level::Healthy,
                    format!(
                        "{} is {:.0}% of a typical request's server time and {:.0}% of the slowest: it waits on something (a lock, a pool, a queue) when it is slow.",
                        grows.op.label(),
                        grows.share,
                        grows.tail_share
                    ),
                ));
            }
        }
        let client_us = client_median.as_micros() as u64;
        if server_median > 0 && client_us > 2 * server_median && client_us - server_median > 1000 {
            notes.push(note(
                Level::Healthy,
                format!(
                    "The service's own time is {} of the {} median pepe measured: the rest is the network and what is in front of the service (a proxy, a load balancer, its accept queue).",
                    ms(server_median),
                    ms(client_us)
                ),
            ));
        }
        notes
    }

    /// The report's `spans` section
    pub fn json(&self) -> SpansJson {
        let ms = |us: u64| us as f64 / 1000.0;
        let round = |v: f64| (v * 10.0).round() / 10.0;
        SpansJson {
            receiver: self.addr.map(|a| a.to_string()),
            requests: self.requests,
            traces: self.traces,
            spans: self.spans,
            coverage_percent: round(self.coverage()),
            other_spans: self.other,
            dropped_spans: self.dropped,
            refused: self.refused.clone(),
            server: (self.traces > 0).then(|| ServerJson {
                median_ms: ms(self.server.percentile(50.0)),
                p90_ms: ms(self.server.percentile(90.0)),
                p99_ms: ms(self.server.percentile(99.0)),
                slowest_requests: self.tail_traces,
            }),
            operations: self
                .operations
                .iter()
                .take(OPERATIONS_KEPT)
                .map(|o| OperationJson {
                    name: o.op.name.to_string(),
                    service: o.op.service.as_deref().map(str::to_string),
                    kind: o.op.kind_name(),
                    calls: o.calls,
                    calls_per_request: round(o.calls as f64 / self.traces.max(1) as f64),
                    errors: o.errors,
                    median_ms: ms(o.durations.percentile(50.0)),
                    p99_ms: ms(o.durations.percentile(99.0)),
                    share_percent: round(o.share),
                    slowest_share_percent: round(o.tail_share),
                })
                .collect(),
        }
    }

    /// The breakdown as a few lines, for the text report
    pub fn text(&self) -> String {
        let mut out = String::new();
        if self.traces == 0 {
            return out;
        }
        let ms = |us: u64| crate::ui::format::latency(Duration::from_micros(us));
        out.push_str(&format!(
            "\nWhere the server's time went  ({} of {} requests traced · server median {} · p99 {})\n",
            self.traces,
            self.requests,
            ms(self.server.percentile(50.0)),
            ms(self.server.percentile(99.0)),
        ));
        out.push_str(&format!(
            "  {:<44} {:>8} {:>9} {:>9} {:>9}\n",
            "operation", "calls/req", "median", "all", "slowest"
        ));
        for o in self.operations.iter().take(8) {
            let mut label = o.op.label();
            if label.chars().count() > 44 {
                label = label.chars().take(43).collect::<String>() + "…";
            }
            out.push_str(&format!(
                "  {:<44} {:>8.1} {:>9} {:>8.0}% {:>8.0}%\n",
                label,
                o.calls as f64 / self.traces.max(1) as f64,
                ms(o.durations.percentile(50.0)),
                o.share,
                o.tail_share
            ));
        }
        out
    }
}

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct SpansJson {
    /// Where the spans were taken
    #[serde(skip_serializing_if = "Option::is_none")]
    pub receiver: Option<String>,
    /// Requests measured that the service answered, and how many of them
    /// it sent spans for
    pub requests: u64,
    pub traces: u64,
    pub spans: u64,
    pub coverage_percent: f64,
    /// Spans of traces pepe didn't start, set aside
    #[serde(skip_serializing_if = "is_zero")]
    pub other_spans: u64,
    /// Spans past what is kept, not in the numbers
    #[serde(skip_serializing_if = "is_zero")]
    pub dropped_spans: u64,
    /// Why exports were turned away
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub refused: Vec<String>,
    /// The server's time for each request: its first span, start to end
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<ServerJson>,
    /// Most of the slowest requests' time first
    pub operations: Vec<OperationJson>,
}

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct ServerJson {
    pub median_ms: f64,
    pub p90_ms: f64,
    pub p99_ms: f64,
    /// Requests at or over the server's p99: what `slowest_share_percent` is of
    pub slowest_requests: u64,
}

#[derive(serde::Serialize, Clone, Debug, PartialEq)]
pub struct OperationJson {
    /// The span's name
    pub name: String,
    /// The `service.name` of the service that sent it
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    /// server, client, internal, producer or consumer
    pub kind: &'static str,
    pub calls: u64,
    pub calls_per_request: f64,
    /// Calls whose status was an error
    pub errors: u64,
    /// Each call, start to end
    pub median_ms: f64,
    pub p99_ms: f64,
    /// Its own time (less what it called) as a share of the server's time
    /// over all requests, and over the slowest
    pub share_percent: f64,
    pub slowest_share_percent: f64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(name: &str) -> Op {
        Op {
            service: Some("shop".into()),
            name: name.into(),
            kind: 2,
        }
    }

    /// A request whose handler spent `db` ms in the database and the rest
    /// of `total` ms in its own code
    fn request(trace: u64, total: u64, db: u64) -> Vec<Span> {
        let root = trace * 10 + 1;
        let ms = 1_000_000;
        vec![
            Span {
                trace,
                id: root,
                parent: trace::client_span(trace),
                op: 0,
                start: 0,
                end: total * ms,
                error: false,
            },
            Span {
                trace,
                id: root + 1,
                parent: root,
                op: 1,
                start: ms,
                end: (1 + db) * ms,
                error: false,
            },
        ]
    }

    #[test]
    fn the_slowest_requests_time_is_put_on_what_made_them_slow() {
        let ops = [op("GET /checkout"), op("SELECT orders")];
        let mut spans = Vec::new();
        // 99 requests of 10 ms, 2 of them in the database; one of 200 ms,
        // 190 of them in the database
        for trace in 1..100 {
            spans.extend(request(trace, 10, 2));
        }
        spans.extend(request(100, 200, 190));
        let b = breakdown_of(&spans, &ops, 100, &HashSet::new(), &[100]);
        assert_eq!((b.traces, b.spans, b.tail_traces), (100, 200, 1));
        let top = &b.operations[0];
        assert_eq!(&*top.op.name, "SELECT orders");
        assert_eq!(top.tail_share.round(), 95.0);
        assert_eq!(top.calls, 100);
        // Over all of them: (99 × 2 + 190) of (99 × 10 + 200)
        assert_eq!(top.share.round(), 33.0);
        let kept = &b.kept[&100];
        assert_eq!(kept.server_ns, 200_000_000);
        assert_eq!(&*kept.spent[0].0.name, "SELECT orders");
        assert_eq!(kept.spent[0].1, 190_000_000);
        assert_eq!(kept.spent[1].1, 10_000_000);
    }

    #[test]
    fn warmup_traces_are_left_out() {
        let ops = [op("GET /"), op("db")];
        let mut spans = request(1, 10, 2);
        spans.extend(request(2, 10, 2));
        let b = breakdown_of(&spans, &ops, 1, &HashSet::from([1]), &[]);
        assert_eq!(b.traces, 1);
    }

    #[test]
    fn overlapping_children_are_counted_once() {
        let parent = Span {
            trace: 1,
            id: 1,
            parent: 0,
            op: 0,
            start: 0,
            end: 100,
            error: false,
        };
        let child = |start, end| Span {
            id: 2,
            parent: 1,
            start,
            end,
            ..parent
        };
        assert_eq!(
            covered(&parent, &[child(10, 50), child(30, 60), child(90, 150)]),
            60
        );
        assert_eq!(covered(&parent, &[]), 0);
    }

    #[test]
    fn requests_are_read_whole_whether_framed_by_length_or_chunks() {
        let posted = b"POST /v1/traces HTTP/1.1\r\nContent-Type: application/json\r\nContent-Length: 2\r\n\r\n{}";
        assert!(read_request(&posted[..posted.len() - 1]).unwrap().is_none());
        let request = read_request(posted).unwrap().unwrap();
        assert_eq!(
            (request.path.as_str(), request.json, request.len),
            ("/v1/traces", true, posted.len())
        );
        assert_eq!(request.body, b"{}");

        let chunked = b"POST /v1/traces HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n3\r\nabc\r\n2;x=y\r\nde\r\n0\r\n\r\nNEXT";
        let request = read_request(chunked).unwrap().unwrap();
        assert_eq!(request.body, b"abcde");
        assert_eq!(&chunked[request.len..], b"NEXT");
        assert!(read_request(&chunked[..chunked.len() - 8])
            .unwrap()
            .is_none());
    }

    /// Protobuf, written by hand: a key, then the value
    fn field(out: &mut Vec<u8>, number: u64, bytes: &[u8]) {
        varint(out, (number << 3) | 2);
        varint(out, bytes.len() as u64);
        out.extend_from_slice(bytes);
    }

    fn varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push(v as u8 | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }

    #[test]
    fn a_protobuf_export_is_read() {
        let mut span = Vec::new();
        let mut trace = [0u8; 16];
        trace[..8].copy_from_slice(&7u64.to_be_bytes());
        trace[8..].copy_from_slice(&42u64.to_be_bytes());
        field(&mut span, 1, &trace);
        field(&mut span, 2, &9u64.to_be_bytes());
        field(&mut span, 4, &trace::client_span(42).to_be_bytes());
        field(&mut span, 5, b"GET /checkout");
        varint(&mut span, 6 << 3);
        varint(&mut span, 2);
        varint(&mut span, (7 << 3) | 1);
        span.extend_from_slice(&1_000u64.to_le_bytes());
        varint(&mut span, (8 << 3) | 1);
        span.extend_from_slice(&5_000u64.to_le_bytes());
        let mut status = Vec::new();
        varint(&mut status, 3 << 3);
        varint(&mut status, 2);
        field(&mut span, 15, &status);

        let read = read_span(&span).unwrap();
        assert_eq!(read.trace, trace);
        assert_eq!((read.id, read.parent), (9, trace::client_span(42)));
        assert_eq!((read.name, read.kind), ("GET /checkout", 2));
        assert_eq!((read.start, read.end, read.error), (1_000, 5_000, true));

        let mut any = Vec::new();
        field(&mut any, 1, b"checkout");
        let mut pair = Vec::new();
        field(&mut pair, 1, b"service.name");
        field(&mut pair, 2, &any);
        let mut resource = Vec::new();
        field(&mut resource, 1, &pair);
        assert_eq!(service_name(&resource).unwrap(), Some("checkout"));
        assert!(read_span(&[0x0a, 0x05, 1]).is_err());
    }
}
