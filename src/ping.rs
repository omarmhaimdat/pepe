//! `pepe ping`: one request at a time, at a steady interval, to one or
//! more targets, with each request's time split into where it went: the
//! DNS lookup, the TCP connect, the TLS handshake, the wait for the first
//! byte and the download. Ping, with HTTP instead of ICMP, and the graph.
//!
//! Each target has a task that starts a ping every `every` and records
//! what came back in `Shared`, which the screen draws from and the headless
//! runner prints from. A ping opens a new connection unless `--keep-alive`
//! asks for the last one to be kept, so every one of them measures every
//! phase, as `httpstat` does; the TLS session is still resumed when the
//! server allows, and the handshake says so.

use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use reqwest::header::{HeaderMap, HeaderValue, ACCEPT, HOST, USER_AGENT};
use reqwest::{Method, Url};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpSocket, TcpStream};
use tokio_rustls::client::TlsStream;
use tokio_rustls::rustls::pki_types::ServerName;
use tokio_rustls::rustls::{HandshakeKind, ProtocolVersion};
use tokio_rustls::TlsConnector;

use crate::cert::{self, Cert};
use crate::metrics::Histogram;
use crate::response::root_cause;
use crate::ui::format;
use crate::wire::{self, Body};

/// Samples kept with everything about them, per target; the numbers of
/// every sample are kept in `Point`s, which are small
pub const RECENT: usize = 1_000;
/// Body bytes kept in a sample for the inspector
pub const BODY_PREVIEW: usize = 1024;
/// The most of a body kept for `--save-body`
pub const BODY_SAVED: usize = 16 * 1024 * 1024;
/// Redirects followed before giving up on a chain
const MAX_HOPS: usize = 10;
/// A CIDR that would be more targets than this is refused
const MAX_TARGETS: usize = 256;
/// Reads of a response at a time
const READ: usize = 64 * 1024;
/// Which addresses a name is pinged at

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Family {
    V4,
    V6,
}

impl Family {
    fn fits(self, ip: &IpAddr) -> bool {
        match self {
            Family::V4 => ip.is_ipv4(),
            Family::V6 => ip.is_ipv6(),
        }
    }
}

/// A limit on a phase: `--slo total=500,ttfb=200`
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Slo {
    pub total: Option<Duration>,
    pub dns: Option<Duration>,
    pub connect: Option<Duration>,
    pub tls: Option<Duration>,
    pub ttfb: Option<Duration>,
    pub download: Option<Duration>,
}

impl Slo {
    /// `total=500,connect=100,ttfb=200ms,dns=50,tls=1s`; a bare number
    /// is milliseconds
    pub fn parse(text: &str) -> Result<Slo, String> {
        let mut slo = Slo::default();
        for part in text.split(',').map(str::trim).filter(|p| !p.is_empty()) {
            let (key, value) = part
                .split_once('=')
                .ok_or_else(|| format!("{part:?} isn't KEY=MILLISECONDS"))?;
            let limit = interval(value.trim(), Duration::from_millis(1))
                .map_err(|e| format!("{part:?}: {e}"))?;
            let slot = match key.trim().to_ascii_lowercase().as_str() {
                "total" => &mut slo.total,
                "dns" => &mut slo.dns,
                "connect" | "tcp" => &mut slo.connect,
                "tls" | "ssl" => &mut slo.tls,
                "ttfb" | "server" | "first-byte" | "first_byte" => &mut slo.ttfb,
                "download" | "transfer" => &mut slo.download,
                other => {
                    return Err(format!(
                        "{other:?} isn't a phase; the keys are total, dns, connect, tls, ttfb and download"
                    ))
                }
            };
            *slot = Some(limit);
        }
        Ok(slo)
    }

    pub fn is_empty(&self) -> bool {
        *self == Slo::default()
    }

    /// Each limit with its name, in the order they are shown
    pub fn limits(&self) -> Vec<(&'static str, Duration)> {
        [
            ("total", self.total),
            ("dns", self.dns),
            ("connect", self.connect),
            ("tls", self.tls),
            ("ttfb", self.ttfb),
            ("download", self.download),
        ]
        .into_iter()
        .filter_map(|(name, limit)| limit.map(|l| (name, l)))
        .collect()
    }

    /// What `sample` broke
    fn check(&self, sample: &Sample) -> Vec<Violation> {
        let mut out = Vec::new();
        let mut check = |key: &'static str, limit: Option<Duration>, actual: Option<Duration>| {
            if let (Some(limit), Some(actual)) = (limit, actual) {
                if actual > limit {
                    out.push(Violation { key, limit, actual });
                }
            }
        };
        check("total", self.total, Some(sample.total));
        check("dns", self.dns, sample.phases.dns);
        check("connect", self.connect, sample.phases.connect);
        check("tls", self.tls, sample.phases.tls);
        check("ttfb", self.ttfb, sample.phases.ttfb);
        check("download", self.download, sample.phases.download);
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub key: &'static str,
    pub limit: Duration,
    pub actual: Duration,
}

/// `1s`, `500ms`, `0.5s`, `2m`; a bare number is in `bare`s
pub fn interval(text: &str, bare: Duration) -> Result<Duration, String> {
    let text = text.trim().to_ascii_lowercase();
    let split = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: f64 = number
        .parse()
        .map_err(|_| format!("{text:?} isn't a time, like 1s, 500ms or 2m"))?;
    let unit = match unit.trim() {
        "" => bare.as_secs_f64(),
        "ms" => 0.001,
        "s" | "sec" | "secs" => 1.0,
        "m" | "min" | "mins" => 60.0,
        "h" | "hr" | "hour" | "hours" => 3_600.0,
        other => return Err(format!("{other:?} isn't a unit; use ms, s, m or h")),
    };
    let seconds = number * unit;
    if !(seconds > 0.0 && seconds.is_finite()) {
        return Err(format!("{text:?} isn't a time greater than zero"));
    }
    Ok(Duration::from_secs_f64(seconds))
}

/// What one run of `pepe ping` shares between its targets
#[derive(Debug, Clone)]
pub struct Settings {
    pub every: Duration,
    pub timeout: Duration,
    pub family: Option<Family>,
    /// The local address to send from
    pub bind: Option<IpAddr>,
    pub insecure: bool,
    /// Keep the connection for the next ping, as a browser would; the
    /// DNS, connect and TLS phases are then only in the first
    pub keep_alive: bool,
    pub follow_redirects: bool,
    /// `--tcp`: a connection refused still counts as an answer
    pub refused_is_pong: bool,
    pub slo: Slo,
    /// Body bytes to keep of each response: `BODY_PREVIEW` for the
    /// inspector, `BODY_SAVED` for `--save-body`, 0 for neither
    pub keep_body: usize,
    pub proxy: Option<String>,
    pub user_agent: String,
    pub headers: HeaderMap,
    pub method: Method,
    pub body: Bytes,
    /// Pings per target, when the run is to end by itself
    pub count: Option<u64>,
    pub duration: Option<Duration>,
    pub compression: bool,
    /// Speak HTTP/1.1 even when the server offers HTTP/2 through ALPN
    pub http1: bool,
}

/// Where a target's pings go
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Http {
        url: Url,
        /// Connect here rather than to what the host resolves to
        /// (`--all-ips`); the Host header and the SNI stay the URL's
        connect_to: Option<IpAddr>,
    },
    /// `--tcp`: connect, and that's the ping
    Tcp {
        host: String,
        port: u16,
        connect_to: Option<IpAddr>,
    },
    /// `--cmd`: run this, and time it
    Cmd(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// What the screen calls it: `--name`, or the host
    pub name: String,
    /// The URL, the host and port, or the command
    pub shown: String,
    pub kind: Kind,
}

impl Target {
    fn host(&self) -> Option<&str> {
        match &self.kind {
            Kind::Http { url, .. } => url.host_str(),
            Kind::Tcp { host, .. } => Some(host),
            Kind::Cmd(_) => None,
        }
    }

    fn with_address(&self, ip: IpAddr) -> Target {
        let kind = match &self.kind {
            Kind::Http { url, .. } => Kind::Http {
                url: url.clone(),
                connect_to: Some(ip),
            },
            Kind::Tcp { host, port, .. } => Kind::Tcp {
                host: host.clone(),
                port: *port,
                connect_to: Some(ip),
            },
            Kind::Cmd(c) => Kind::Cmd(c.clone()),
        };
        Target {
            name: format!("{} {ip}", self.name),
            shown: self.shown.clone(),
            kind,
        }
    }
}

/// How the words on the command line are read as targets
pub struct Words<'a> {
    pub words: &'a [String],
    pub names: &'a [String],
    pub tcp: bool,
    /// The port for a `--tcp` target that names none
    pub port: u16,
    pub cmd: bool,
    pub all_ips: bool,
    pub family: Option<Family>,
}

/// The targets the words name; a name resolves to every address it has
/// when `all_ips` asks for it
pub async fn targets(words: Words<'_>) -> Result<Vec<Target>, String> {
    if words.words.is_empty() {
        return Err("nothing to ping: give a URL, a host, or a command with --cmd".into());
    }
    let mut targets = Vec::new();
    for word in words.words {
        if words.cmd {
            targets.push(Target {
                name: word.clone(),
                shown: word.clone(),
                kind: Kind::Cmd(word.clone()),
            });
            continue;
        }
        targets.extend(parse_target(word, words.tcp, words.port)?);
    }
    let names: Vec<&str> = words
        .names
        .iter()
        .flat_map(|n| n.split(','))
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .collect();
    for (target, name) in targets.iter_mut().zip(&names) {
        target.name = (*name).to_string();
    }
    if words.all_ips {
        let mut expanded = Vec::new();
        for target in targets {
            let Some(host) = target.host().filter(|h| h.parse::<IpAddr>().is_err()) else {
                expanded.push(target);
                continue;
            };
            let addresses: Vec<IpAddr> = tokio::net::lookup_host((host, 0))
                .await
                .map_err(|e| format!("{host}: {}", root_cause(&e)))?
                .map(|a| a.ip())
                .filter(|ip| words.family.is_none_or(|f| f.fits(ip)))
                .collect();
            if addresses.is_empty() {
                return Err(format!("{host} resolved to no address"));
            }
            expanded.extend(addresses.into_iter().map(|ip| target.with_address(ip)));
        }
        targets = expanded;
    }
    if targets.len() > MAX_TARGETS {
        return Err(format!(
            "{} targets is more than the {MAX_TARGETS} one screen can show",
            targets.len()
        ));
    }
    Ok(targets)
}

/// One word as targets: a URL, a host, `host:port`, `aws:REGION`, or a
/// CIDR range, which is many
fn parse_target(word: &str, tcp: bool, port: u16) -> Result<Vec<Target>, String> {
    let word = word.trim();
    if word.is_empty() {
        return Err("an empty target".into());
    }
    if let Some(region) = word.strip_prefix("aws:") {
        let url = format!("https://ec2.{region}.amazonaws.com/ping");
        return parse_target(&url, tcp, port).map(|mut t| {
            for target in &mut t {
                target.name = word.to_string();
            }
            t
        });
    }
    if let Some(hosts) = cidr(word) {
        let (hosts, port_in_word) = hosts?;
        let port = port_in_word.unwrap_or(if tcp { port } else { 80 });
        return hosts
            .into_iter()
            .map(|ip| parse_target(&format!("{ip}:{port}"), tcp, port))
            .collect::<Result<Vec<_>, _>>()
            .map(|t| t.into_iter().flatten().collect());
    }
    let text = if word.contains("://") {
        word.to_string()
    } else {
        // A host without a scheme: https, unless it has a port or is this
        // machine, where TLS is rare
        let (host, _) = split_port(word);
        let local = matches!(host, "localhost" | "127.0.0.1" | "[::1]" | "::1")
            || host.ends_with(".localhost");
        let has_port = split_port(word).1.is_some();
        let scheme = if tcp || has_port || local {
            "http"
        } else {
            "https"
        };
        // An IPv6 address needs its brackets in a URL
        let authority = if word.contains(':') && !word.starts_with('[') && !has_port {
            format!("[{word}]")
        } else {
            word.to_string()
        };
        format!("{scheme}://{authority}/")
    };
    let url = Url::parse(&text).map_err(|e| format!("{word:?} isn't a URL: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!(
            "{word:?}: only http and https URLs can be pinged{}",
            if tcp { "" } else { "; --tcp pings a port" }
        ));
    }
    let host = url
        .host_str()
        .ok_or_else(|| format!("{word:?} has no host"))?
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();
    if tcp {
        let port = match (url.port(), split_port(word).1) {
            (Some(p), _) => p,
            (None, _) if word.contains("://") => url.port_or_known_default().unwrap_or(port),
            _ => port,
        };
        return Ok(vec![Target {
            name: format!("{host}:{port}"),
            shown: format!("{host}:{port}"),
            kind: Kind::Tcp {
                host,
                port,
                connect_to: None,
            },
        }]);
    }
    Ok(vec![Target {
        name: host,
        shown: url.to_string(),
        kind: Kind::Http {
            url,
            connect_to: None,
        },
    }])
}

/// `host:port` apart, leaving an IPv6 address whole
fn split_port(word: &str) -> (&str, Option<u16>) {
    if let Some(rest) = word.strip_prefix('[') {
        if let Some((host, port)) = rest.split_once("]:") {
            return (host, port.parse().ok());
        }
        return (rest.trim_end_matches(']'), None);
    }
    match word.rsplit_once(':') {
        Some((host, port)) if !host.contains(':') => match port.parse() {
            Ok(port) => (host, Some(port)),
            Err(_) => (word, None),
        },
        _ => (word, None),
    }
}

/// `10.0.0.0/29` or `10.0.0.0/29:8080`: its hosts, and the port if given.
/// None when the word isn't a range.
#[allow(clippy::type_complexity)]
fn cidr(word: &str) -> Option<Result<(Vec<IpAddr>, Option<u16>), String>> {
    let (network, prefix) = word.split_once('/')?;
    let network: Ipv4Addr = network.parse().ok()?;
    let (prefix, port) = match prefix.split_once(':') {
        Some((prefix, port)) => (prefix, Some(port)),
        None => (prefix, None),
    };
    let prefix: u32 = prefix.parse().ok()?;
    let port = match port {
        Some(port) => match port.parse::<u16>() {
            Ok(port) => Some(port),
            Err(_) => return Some(Err(format!("{word:?}: {port:?} isn't a port"))),
        },
        None => None,
    };
    if prefix > 32 {
        return Some(Err(format!("{word:?}: a prefix is at most /32")));
    }
    let size = 1u64 << (32 - prefix);
    if size as usize > MAX_TARGETS + 2 {
        return Some(Err(format!(
            "{word:?} is {size} addresses; at most {MAX_TARGETS} can be pinged at once (/24)"
        )));
    }
    let base = u32::from(network) & (u32::MAX.checked_shl(32 - prefix).unwrap_or(0));
    // The network and broadcast addresses of a range aren't hosts
    let (from, to) = if prefix >= 31 {
        (0, size as u32)
    } else {
        (1, size as u32 - 1)
    };
    let hosts = (from..to)
        .map(|i| IpAddr::V4(Ipv4Addr::from(base + i)))
        .collect();
    Some(Ok((hosts, port)))
}

/// Where one ping's time went. A phase is None when nothing measured it:
/// the connection was kept from the last ping, or a proxy carried it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Phases {
    pub dns: Option<Duration>,
    pub connect: Option<Duration>,
    pub tls: Option<Duration>,
    /// From the request going out to the first byte of the answer
    pub ttfb: Option<Duration>,
    pub download: Option<Duration>,
}

impl Phases {
    /// The phases in order, each with its name
    pub fn each(&self) -> [(&'static str, Option<Duration>); 5] {
        [
            ("dns", self.dns),
            ("connect", self.connect),
            ("tls", self.tls),
            ("ttfb", self.ttfb),
            ("download", self.download),
        ]
    }

    fn add(&mut self, other: &Phases) {
        let sum = |a: &mut Option<Duration>, b: Option<Duration>| {
            if let Some(b) = b {
                *a = Some(a.unwrap_or_default() + b);
            }
        };
        sum(&mut self.dns, other.dns);
        sum(&mut self.connect, other.connect);
        sum(&mut self.tls, other.tls);
        sum(&mut self.ttfb, other.ttfb);
        sum(&mut self.download, other.download);
    }
}

/// What the TLS handshake settled on
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tls {
    pub version: String,
    pub cipher: String,
    pub alpn: Option<String>,
    /// The session was resumed: no certificate was sent or checked again
    pub resumed: bool,
    pub cert: Option<Cert>,
}

/// One leg of a redirect chain
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hop {
    pub url: String,
    pub status: u16,
    pub took: Duration,
}

/// One ping, with everything that came back
#[derive(Debug, Clone)]
pub struct Sample {
    pub target: usize,
    pub seq: u64,
    /// Since the run started
    pub at: Duration,
    /// Seconds since the epoch, for the log
    pub wall: i64,
    pub total: Duration,
    /// Something answered: a response, a connection, an exit code
    pub answered: bool,
    pub timeout: bool,
    /// The status of a response, or the exit code of a command
    pub status: Option<u16>,
    pub error: Option<String>,
    pub phases: Phases,
    /// Sent on a connection kept from the last ping
    pub reused: bool,
    pub bytes: u64,
    pub remote: Option<SocketAddr>,
    pub local: Option<SocketAddr>,
    pub version: Option<String>,
    pub tls: Option<Tls>,
    /// Each redirect followed, in order, before the final response
    pub hops: Vec<Hop>,
    /// Where the redirects ended, when there were any
    pub final_url: Option<String>,
    pub headers: Vec<(String, String)>,
    /// Up to `BODY_PREVIEW` bytes of the body
    pub body: Vec<u8>,
    pub violations: Vec<Violation>,
}

impl Sample {
    pub(crate) fn new(target: usize, seq: u64, at: Duration) -> Self {
        Sample {
            target,
            seq,
            at,
            wall: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |d| d.as_secs() as i64),
            total: Duration::ZERO,
            answered: false,
            timeout: false,
            status: None,
            error: None,
            phases: Phases::default(),
            reused: false,
            bytes: 0,
            remote: None,
            local: None,
            version: None,
            tls: None,
            hops: Vec::new(),
            final_url: None,
            headers: Vec::new(),
            body: Vec::new(),
            violations: Vec::new(),
        }
    }

    /// Answered, and well: a status under 400, or a command that exited 0
    pub fn ok(&self) -> bool {
        self.answered && self.status.is_none_or(|s| s < 400 || s == 0) && self.error.is_none()
    }

    /// The status, the failure, or the exit code, in a few characters
    pub fn outcome(&self) -> String {
        match (self.answered, self.status, &self.error) {
            (_, _, Some(e)) if self.timeout => format!("timeout: {e}"),
            (_, _, Some(e)) => e.clone(),
            (true, Some(s), None) => s.to_string(),
            (true, None, None) => "connected".into(),
            (false, _, None) => "no answer".into(),
        }
    }
}

/// A sample's numbers, which are all that is kept of most
#[derive(Debug, Clone, Copy)]
pub struct Point {
    /// Seconds since the run started
    pub at: f32,
    pub us: u32,
    pub answered: bool,
    pub timeout: bool,
    pub status: u16,
    /// It broke the SLO
    pub slow: bool,
}

/// A target's numbers, over the whole run or a window of it
#[derive(Debug, Clone, Copy, Default)]
pub struct Summary {
    pub sent: u64,
    pub answered: u64,
    pub timeouts: u64,
    /// Microseconds
    pub last: Option<u64>,
    pub min: u64,
    pub max: u64,
    pub avg: u64,
    /// Mean difference between one answer and the next
    pub jitter: u64,
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
}

impl Summary {
    pub fn loss(&self) -> f64 {
        if self.sent == 0 {
            0.0
        } else {
            1.0 - self.answered as f64 / self.sent as f64
        }
    }
}

/// Everything recorded for one target
#[derive(Debug)]
pub struct TargetStats {
    pub target: Target,
    pub points: Vec<Point>,
    pub recent: VecDeque<Sample>,
    pub sent: u64,
    pub answered: u64,
    pub timeouts: u64,
    pub in_flight: u32,
    pub finished: bool,
    pub total: Histogram,
    pub dns: Histogram,
    pub connect: Histogram,
    pub tls_full: Histogram,
    pub tls_resumed: Histogram,
    pub ttfb: Histogram,
    pub download: Histogram,
    pub reused: u64,
    sum_us: u128,
    jitter_sum: u128,
    jitter_n: u64,
    last_us: Option<u64>,
    pub bytes: u64,
    pub statuses: BTreeMap<u16, u64>,
    pub causes: BTreeMap<String, u64>,
    pub violations: u64,
    pub worst: Vec<(&'static str, Duration)>,
    pub last_tls: Option<Tls>,
    pub last_remote: Option<SocketAddr>,
    pub last_local: Option<SocketAddr>,
    pub last_version: Option<String>,
    /// The last body, whole, for `--save-body`
    pub last_body: Option<Vec<u8>>,
}

impl TargetStats {
    pub(crate) fn new(target: Target) -> Self {
        TargetStats {
            target,
            points: Vec::new(),
            recent: VecDeque::new(),
            sent: 0,
            answered: 0,
            timeouts: 0,
            in_flight: 0,
            finished: false,
            total: Histogram::default(),
            dns: Histogram::default(),
            connect: Histogram::default(),
            tls_full: Histogram::default(),
            tls_resumed: Histogram::default(),
            ttfb: Histogram::default(),
            download: Histogram::default(),
            reused: 0,
            sum_us: 0,
            jitter_sum: 0,
            jitter_n: 0,
            last_us: None,
            bytes: 0,
            statuses: BTreeMap::new(),
            causes: BTreeMap::new(),
            violations: 0,
            worst: Vec::new(),
            last_tls: None,
            last_remote: None,
            last_local: None,
            last_version: None,
            last_body: None,
        }
    }

    pub(crate) fn record(&mut self, sample: Sample, saved_body: Option<Vec<u8>>) {
        self.in_flight = self.in_flight.saturating_sub(1);
        let us = sample.total.as_micros() as u64;
        self.points.push(Point {
            at: sample.at.as_secs_f32(),
            us: us.min(u32::MAX as u64) as u32,
            answered: sample.answered,
            timeout: sample.timeout,
            status: sample.status.unwrap_or(0),
            slow: !sample.violations.is_empty(),
        });
        if sample.answered {
            self.answered += 1;
            self.total.record(us);
            self.sum_us += u128::from(us);
            if let Some(last) = self.last_us {
                self.jitter_sum += u128::from(last.abs_diff(us));
                self.jitter_n += 1;
            }
            self.last_us = Some(us);
            let record = |h: &mut Histogram, d: Option<Duration>| {
                if let Some(d) = d {
                    h.record(d.as_micros() as u64);
                }
            };
            record(&mut self.dns, sample.phases.dns);
            record(&mut self.connect, sample.phases.connect);
            match sample.tls.as_ref().map(|t| t.resumed) {
                Some(true) => record(&mut self.tls_resumed, sample.phases.tls),
                _ => record(&mut self.tls_full, sample.phases.tls),
            }
            record(&mut self.ttfb, sample.phases.ttfb);
            record(&mut self.download, sample.phases.download);
            if sample.reused {
                self.reused += 1;
            }
            self.bytes += sample.bytes;
            if let Some(status) = sample.status {
                *self.statuses.entry(status).or_default() += 1;
            }
        }
        if sample.timeout {
            self.timeouts += 1;
        }
        if let Some(error) = &sample.error {
            *self.causes.entry(error.clone()).or_default() += 1;
        }
        if !sample.violations.is_empty() {
            self.violations += 1;
            for v in &sample.violations {
                match self.worst.iter_mut().find(|(k, _)| *k == v.key) {
                    Some((_, worst)) => *worst = (*worst).max(v.actual),
                    None => self.worst.push((v.key, v.actual)),
                }
            }
        }
        if let Some(tls) = &sample.tls {
            self.last_tls = Some(tls.clone());
        }
        if sample.remote.is_some() {
            self.last_remote = sample.remote;
            self.last_local = sample.local;
        }
        if sample.version.is_some() {
            self.last_version = sample.version.clone();
        }
        if let Some(body) = saved_body {
            self.last_body = Some(body);
        }
        self.recent.push_back(sample);
        while self.recent.len() > RECENT {
            self.recent.pop_front();
        }
    }

    /// The whole run
    pub fn whole(&self) -> Summary {
        Summary {
            sent: self.sent,
            answered: self.answered,
            timeouts: self.timeouts,
            last: self.last_us,
            min: self.total.percentile(0.0),
            max: self.total.percentile(100.0),
            avg: if self.answered == 0 {
                0
            } else {
                (self.sum_us / u128::from(self.answered)) as u64
            },
            jitter: if self.jitter_n == 0 {
                0
            } else {
                (self.jitter_sum / u128::from(self.jitter_n)) as u64
            },
            p50: self.total.percentile(50.0),
            p95: self.total.percentile(95.0),
            p99: self.total.percentile(99.0),
        }
    }

    /// The last `window` of the run, ending at `now` seconds in
    pub fn window(&self, now: f32, window: f32) -> Summary {
        let from = self.points.partition_point(|p| p.at < now - window);
        let points = &self.points[from..];
        let mut values: Vec<u64> = points
            .iter()
            .filter(|p| p.answered)
            .map(|p| u64::from(p.us))
            .collect();
        let mut summary = Summary {
            sent: points.len() as u64,
            answered: values.len() as u64,
            timeouts: points.iter().filter(|p| p.timeout).count() as u64,
            last: self.last_us,
            ..Default::default()
        };
        if values.is_empty() {
            return summary;
        }
        summary.jitter = if values.len() < 2 {
            0
        } else {
            values.windows(2).map(|w| w[0].abs_diff(w[1])).sum::<u64>() / (values.len() as u64 - 1)
        };
        summary.avg =
            (values.iter().map(|&v| u128::from(v)).sum::<u128>() / values.len() as u128) as u64;
        values.sort_unstable();
        let at = |q: f64| {
            let rank = ((q / 100.0) * values.len() as f64).ceil() as usize;
            values[rank.clamp(1, values.len()) - 1]
        };
        summary.min = values[0];
        summary.max = values[values.len() - 1];
        summary.p50 = at(50.0);
        summary.p95 = at(95.0);
        summary.p99 = at(99.0);
        summary
    }

    /// The share of answers that were 5xx
    pub fn failing(&self) -> f64 {
        if self.answered == 0 {
            return 0.0;
        }
        let bad: u64 = self.statuses.range(500..).map(|(_, n)| n).sum();
        bad as f64 / self.answered as f64
    }
}

/// What the run has recorded, for the screen and the report
#[derive(Debug)]
pub struct State {
    pub started: Instant,
    pub targets: Vec<TargetStats>,
    /// Every target has sent its last and heard back or given up
    pub done: bool,
    pub paused: bool,
    /// Samples that failed or broke the SLO since the screen last rang
    /// its bell
    pub bells: u32,
}

impl State {
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn any_violation(&self) -> bool {
        self.targets.iter().any(|t| t.violations > 0)
    }

    /// No target ever answered, though something was sent
    pub fn nothing_answered(&self) -> bool {
        self.targets.iter().any(|t| t.sent > 0) && self.targets.iter().all(|t| t.answered == 0)
    }
}

pub struct Shared {
    state: Mutex<State>,
    stop: AtomicBool,
    pub settings: Settings,
}

impl Shared {
    pub fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn stopped(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }

    pub fn set_paused(&self, paused: bool) {
        self.lock().paused = paused;
    }
}

/// Start pinging: a task per target on this runtime
pub fn start(targets: Vec<Target>, settings: Settings) -> Arc<Shared> {
    let shared = shared(targets, settings);
    let count = shared.lock().targets.len();
    for index in 0..count {
        tokio::spawn(pinger(shared.clone(), index));
    }
    shared
}

/// The run's state, with nothing pinging yet
pub fn shared(targets: Vec<Target>, settings: Settings) -> Arc<Shared> {
    Arc::new(Shared {
        state: Mutex::new(State {
            started: Instant::now(),
            targets: targets.into_iter().map(TargetStats::new).collect(),
            done: false,
            paused: false,
            bells: 0,
        }),
        stop: AtomicBool::new(false),
        settings,
    })
}

/// What a target's pings share: the TLS setup, with its session cache,
/// and the connection kept between pings
struct Pinger {
    shared: Arc<Shared>,
    index: usize,
    target: Target,
    tls: Arc<TlsConnector>,
    kept: Option<Kept>,
}

/// A connection kept for the next ping
struct Kept {
    stream: Stream,
    remote: Option<SocketAddr>,
    local: Option<SocketAddr>,
    tls: Option<Tls>,
}

async fn pinger(shared: Arc<Shared>, index: usize) {
    let (target, started) = {
        let state = shared.lock();
        (state.targets[index].target.clone(), state.started)
    };
    let settings = shared.settings.clone();
    // HTTP/2 when the server offers it, unless told to stay on HTTP/1.1
    let mut config = crate::direct::tls_config(settings.insecure);
    if !settings.http1 {
        config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    }
    let tls = Arc::new(TlsConnector::from(Arc::new(config)));
    let mut pinger = Pinger {
        shared: shared.clone(),
        index,
        target,
        tls,
        kept: None,
    };
    let mut clock = tokio::time::interval(settings.every);
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut seq = 0u64;
    loop {
        clock.tick().await;
        if shared.stopped() {
            break;
        }
        if shared.lock().paused {
            continue;
        }
        if settings.count.is_some_and(|n| seq >= n) {
            break;
        }
        if settings.duration.is_some_and(|d| started.elapsed() >= d) {
            break;
        }
        seq += 1;
        {
            let mut state = shared.lock();
            state.targets[index].sent += 1;
            state.targets[index].in_flight += 1;
        }
        let at = started.elapsed();
        if settings.keep_alive || matches!(pinger.target.kind, Kind::Cmd(_)) {
            // One at a time: a kept connection carries one request at once
            pinger.ping(seq, at).await;
        } else {
            let one = Pinger {
                shared: shared.clone(),
                index,
                target: pinger.target.clone(),
                tls: pinger.tls.clone(),
                kept: None,
            };
            tokio::spawn(async move {
                let mut one = one;
                one.ping(seq, at).await;
            });
        }
    }
    // The last pings sent are still in the air
    loop {
        let in_flight = shared.lock().targets[index].in_flight;
        if in_flight == 0 || shared.stopped() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut state = shared.lock();
    state.targets[index].finished = true;
    if state.targets.iter().all(|t| t.finished) {
        state.done = true;
    }
}

impl Pinger {
    async fn ping(&mut self, seq: u64, at: Duration) {
        let shared = self.shared.clone();
        let settings = &shared.settings;
        let mut sample = Sample::new(self.index, seq, at);
        let mut saved = None;
        let started = Instant::now();
        let outcome =
            tokio::time::timeout(settings.timeout, self.send(&mut sample, &mut saved)).await;
        sample.total = started.elapsed();
        match outcome {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                sample.error = Some(e);
                self.kept = None;
            }
            Err(_) => {
                sample.timeout = true;
                sample.error = Some(format!("no answer in {}", format::span(settings.timeout)));
                self.kept = None;
            }
        }
        if sample.answered {
            sample.violations = settings.slo.check(&sample);
        }
        let trouble = !sample.ok() || !sample.violations.is_empty();
        let mut state = shared.lock();
        if trouble {
            state.bells += 1;
        }
        state.targets[self.index].record(sample, saved);
    }

    /// The ping itself; what it measured is in `sample` even when it
    /// fails or runs out of time
    async fn send(
        &mut self,
        sample: &mut Sample,
        saved: &mut Option<Vec<u8>>,
    ) -> Result<(), String> {
        let settings = self.shared.settings.clone();
        match self.target.kind.clone() {
            Kind::Cmd(command) => run_command(&command, sample).await,
            Kind::Tcp {
                host,
                port,
                connect_to,
            } => {
                let addresses = resolve(&host, port, connect_to, &settings, sample).await?;
                match connect(&addresses, &settings, sample).await {
                    Ok(_) => {
                        sample.answered = true;
                        Ok(())
                    }
                    Err(e) if settings.refused_is_pong && e.contains("refused") => {
                        sample.answered = true;
                        sample.error = None;
                        sample
                            .headers
                            .push(("note".into(), format!("{e}; counted as an answer")));
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
            Kind::Http { url, connect_to } => {
                if settings.proxy.is_some() {
                    return via_proxy(&url, &settings, sample, saved).await;
                }
                let mut url = url;
                let mut method = settings.method.clone();
                let mut body = settings.body.clone();
                let mut hops = 0;
                loop {
                    let began = Instant::now();
                    let (status, location) = self
                        .exchange(&url, &method, &body, connect_to, &settings, sample, saved)
                        .await?;
                    let redirect = matches!(status, 301 | 302 | 303 | 307 | 308);
                    let Some(location) = location.filter(|_| redirect && settings.follow_redirects)
                    else {
                        if !sample.hops.is_empty() {
                            sample.final_url = Some(url.to_string());
                        }
                        return Ok(());
                    };
                    hops += 1;
                    if hops > MAX_HOPS {
                        return Err(format!("more than {MAX_HOPS} redirects"));
                    }
                    let next = url
                        .join(&location)
                        .map_err(|e| format!("redirect to {location:?}: {e}"))?;
                    sample.hops.push(Hop {
                        url: url.to_string(),
                        status,
                        took: began.elapsed(),
                    });
                    // What a browser does: GET after a 301, 302 or 303
                    if matches!(status, 301..=303) && method != Method::HEAD {
                        method = Method::GET;
                        body = Bytes::new();
                    }
                    // A kept connection is to the last host
                    if next.host_str() != url.host_str()
                        || next.port_or_known_default() != url.port_or_known_default()
                    {
                        self.kept = None;
                    }
                    url = next;
                    sample.answered = false;
                    sample.headers.clear();
                    sample.body.clear();
                }
            }
        }
    }

    /// One request and its response on a connection, kept or opened;
    /// the status and the Location header, if any
    #[allow(clippy::too_many_arguments)]
    async fn exchange(
        &mut self,
        url: &Url,
        method: &Method,
        body: &Bytes,
        connect_to: Option<IpAddr>,
        settings: &Settings,
        sample: &mut Sample,
        saved: &mut Option<Vec<u8>>,
    ) -> Result<(u16, Option<String>), String> {
        let host = url
            .host_str()
            .ok_or("the URL has no host")?
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();
        let port = url.port_or_known_default().unwrap_or(80);
        let https = url.scheme() == "https";
        let mut phases = Phases::default();
        let mut kept = match self.kept.take() {
            Some(kept) => kept,
            None => {
                let addresses = resolve(&host, port, connect_to, settings, sample).await?;
                phases.dns = sample.phases.dns.take();
                let (stream, remote, local, _) = connect(&addresses, settings, sample).await?;
                // `connect` noted it on the sample, as `resolve` did the
                // lookup; both are moved here so a redirect's hop adds
                // its own rather than counting these twice
                phases.connect = sample.phases.connect.take();
                let (stream, tls) = if https {
                    let began = Instant::now();
                    let name = ServerName::try_from(host.clone())
                        .map_err(|_| format!("{host:?} isn't a name TLS can verify"))?;
                    let stream = self
                        .tls
                        .connect(name, stream)
                        .await
                        .map_err(|e| format!("TLS: {}", root_cause(&e)))?;
                    phases.tls = Some(began.elapsed());
                    let tls = tls_details(&stream);
                    if tls.alpn.as_deref() == Some("h2") {
                        let (send, connection) = h2::client::handshake(stream)
                            .await
                            .map_err(|e| format!("HTTP/2: {e}"))?;
                        // The connection is driven by its own task for as
                        // long as something holds its sender
                        tokio::spawn(async move {
                            let _ = connection.await;
                        });
                        (Stream::H2(send), Some(tls))
                    } else {
                        (Stream::Tls(Box::new(stream)), Some(tls))
                    }
                } else {
                    (Stream::Plain(stream), None)
                };
                Kept {
                    stream,
                    remote,
                    local,
                    tls,
                }
            }
        };
        let reused = phases.connect.is_none();
        sample.reused = reused;
        sample.remote = kept.remote;
        sample.local = kept.local;
        sample.tls = kept.tls.clone();
        if let Stream::H2(send) = &kept.stream {
            let send = send.clone();
            let answered = h2_exchange(
                send,
                url,
                method,
                &settings.headers,
                body,
                settings,
                sample,
                &mut phases,
                saved,
            )
            .await?;
            sample.phases.add(&phases);
            if settings.keep_alive {
                self.kept = Some(kept);
            }
            return Ok(answered);
        }

        // The request: the run's headers, Host, User-Agent and Accept
        let mut headers = settings.headers.clone();
        if !headers.contains_key(HOST) {
            let value = match url.port() {
                Some(port) => format!("{}:{port}", url.host_str().unwrap_or_default()),
                None => url.host_str().unwrap_or_default().to_string(),
            };
            headers.insert(
                HOST,
                value.parse().map_err(|_| "a Host header that isn't one")?,
            );
        }
        if !headers.contains_key(USER_AGENT) {
            headers.insert(
                USER_AGENT,
                settings
                    .user_agent
                    .parse()
                    .unwrap_or(HeaderValue::from_static("pepe")),
            );
        }
        if !headers.contains_key(ACCEPT) {
            headers.insert(ACCEPT, HeaderValue::from_static("*/*"));
        }
        if settings.compression && !headers.contains_key("accept-encoding") {
            headers.insert("accept-encoding", HeaderValue::from_static("gzip, br"));
        }
        if !settings.keep_alive && !headers.contains_key("connection") {
            headers.insert("connection", HeaderValue::from_static("close"));
        }
        let mut path = url.path().to_string();
        if let Some(query) = url.query() {
            path.push('?');
            path.push_str(query);
        }
        let request = wire::request(method, &path, &headers, body);
        let sent_at = Instant::now();
        kept.stream
            .write_all(&request)
            .await
            .map_err(|e| format!("sending the request: {}", root_cause(&e)))?;

        // The head, then the body, counted as it comes
        let mut buf: Vec<u8> = Vec::with_capacity(READ);
        let mut fields = Vec::new();
        let mut first_byte: Option<Instant> = None;
        let head = loop {
            let n = kept
                .stream
                .read_buf(&mut buf)
                .await
                .map_err(|e| format!("reading the response: {}", root_cause(&e)))?;
            if n == 0 {
                return Err(if buf.is_empty() && reused {
                    "the kept connection was closed".into()
                } else {
                    "the connection closed before the response".into()
                });
            }
            first_byte.get_or_insert_with(Instant::now);
            match wire::head(&buf, *method == Method::HEAD, &mut fields) {
                Ok(Some(head)) if (100..200).contains(&head.status) && head.status != 101 => {
                    buf.drain(..head.len);
                }
                Ok(Some(head)) => break head,
                Ok(None) => {
                    if buf.len() > 1024 * 1024 {
                        return Err("the response head is over a megabyte".into());
                    }
                    buf.reserve(READ);
                }
                Err(why) => return Err(why.to_string()),
            }
        };
        phases.ttfb = Some(first_byte.unwrap_or(sent_at).duration_since(sent_at));
        let body_began = Instant::now();
        sample.status = Some(head.status);
        sample.version = Some(format!("HTTP/1.{}", head.minor));
        sample.headers = fields
            .iter()
            .map(|f| {
                (
                    String::from_utf8_lossy(f.name(&buf)).into_owned(),
                    String::from_utf8_lossy(f.value(&buf)).into_owned(),
                )
            })
            .collect();
        let location = sample
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case("location"))
            .map(|(_, value)| value.clone());
        let keep_body = settings.keep_body;
        let mut kept_body: Vec<u8> = Vec::new();
        let mut bytes = 0u64;
        let mut body_state = head.body;
        buf.drain(..head.len);
        let mut chunked_input = Vec::new();
        let mut eof = false;
        loop {
            let piece: Vec<u8> = match &mut body_state {
                Body::None => break,
                Body::Length(left) => {
                    if *left == 0 {
                        break;
                    }
                    if buf.is_empty() {
                        if eof {
                            return Err("the connection closed before the body ended".into());
                        }
                        let n = kept
                            .stream
                            .read_buf(&mut buf)
                            .await
                            .map_err(|e| root_cause(&e))?;
                        eof = n == 0;
                        continue;
                    }
                    let take = (*left).min(buf.len() as u64) as usize;
                    *left -= take as u64;
                    buf.drain(..take).collect()
                }
                Body::Chunked(chunked) => {
                    if chunked.done() {
                        break;
                    }
                    if buf.is_empty() {
                        if eof {
                            return Err("the connection closed before the body ended".into());
                        }
                        let n = kept
                            .stream
                            .read_buf(&mut buf)
                            .await
                            .map_err(|e| root_cause(&e))?;
                        eof = n == 0;
                        continue;
                    }
                    chunked_input.clear();
                    chunked_input.extend_from_slice(&buf);
                    let step = chunked.step(&chunked_input).map_err(|e| e.to_string())?;
                    buf.drain(..step.consumed);
                    chunked_input[step.data].to_vec()
                }
                Body::UntilClose => {
                    if buf.is_empty() {
                        if eof {
                            break;
                        }
                        let n = kept
                            .stream
                            .read_buf(&mut buf)
                            .await
                            .map_err(|e| root_cause(&e))?;
                        eof = n == 0;
                        continue;
                    }
                    std::mem::take(&mut buf)
                }
            };
            bytes += piece.len() as u64;
            if kept_body.len() < keep_body {
                let want = (keep_body - kept_body.len()).min(piece.len());
                kept_body.extend_from_slice(&piece[..want]);
            }
        }
        phases.download = Some(body_began.elapsed());
        sample.bytes = bytes;
        sample.answered = true;
        sample.phases.add(&phases);
        if keep_body > BODY_PREVIEW {
            *saved = Some(kept_body.clone());
        }
        kept_body.truncate(BODY_PREVIEW);
        sample.body = kept_body;
        let reusable = settings.keep_alive
            && head.keep_alive
            && !matches!(body_state, Body::UntilClose)
            && buf.is_empty();
        if reusable {
            self.kept = Some(kept);
        }
        Ok((head.status, location))
    }
}

/// A connection's kinds of stream: plain, TLS, or TLS carrying HTTP/2,
/// which h2 reads and writes on its own
enum Stream {
    Plain(TcpStream),
    Tls(Box<TlsStream<TcpStream>>),
    H2(h2::client::SendRequest<Bytes>),
}

/// A request and its response over HTTP/2, on a connection h2 drives.
/// The first byte is the response's headers; the download is the rest.
#[allow(clippy::too_many_arguments)]
async fn h2_exchange(
    send: h2::client::SendRequest<Bytes>,
    url: &Url,
    method: &Method,
    run_headers: &HeaderMap,
    body: &Bytes,
    settings: &Settings,
    sample: &mut Sample,
    phases: &mut Phases,
    saved: &mut Option<Vec<u8>>,
) -> Result<(u16, Option<String>), String> {
    let mut request = http::Request::builder()
        .method(method.clone())
        .uri(url.as_str());
    for (name, value) in run_headers {
        // HTTP/2 has :authority for the host and no connection headers
        if name == HOST || name.as_str().eq_ignore_ascii_case("connection") {
            continue;
        }
        request = request.header(name.clone(), value.clone());
    }
    if !run_headers.contains_key(USER_AGENT) {
        request = request.header(USER_AGENT, settings.user_agent.as_str());
    }
    if !run_headers.contains_key(ACCEPT) {
        request = request.header(ACCEPT, "*/*");
    }
    if settings.compression && !run_headers.contains_key("accept-encoding") {
        request = request.header("accept-encoding", "gzip, br");
    }
    let request = request
        .body(())
        .map_err(|e| format!("HTTP/2 request: {e}"))?;
    let sent_at = Instant::now();
    let mut send = send.ready().await.map_err(|e| format!("HTTP/2: {e}"))?;
    let (response, mut stream) = send
        .send_request(request, body.is_empty())
        .map_err(|e| format!("HTTP/2: {e}"))?;
    if !body.is_empty() {
        stream
            .send_data(body.clone(), true)
            .map_err(|e| format!("HTTP/2 body: {e}"))?;
    }
    let response = response.await.map_err(|e| format!("HTTP/2: {e}"))?;
    phases.ttfb = Some(sent_at.elapsed());
    let body_began = Instant::now();
    let status = response.status().as_u16();
    sample.status = Some(status);
    sample.version = Some("HTTP/2".into());
    sample.headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let location = sample
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("location"))
        .map(|(_, value)| value.clone());
    let keep_body = settings.keep_body;
    let mut kept_body: Vec<u8> = Vec::new();
    let mut bytes = 0u64;
    let mut incoming = response.into_body();
    while let Some(chunk) = incoming.data().await {
        let chunk = chunk.map_err(|e| format!("HTTP/2 body: {e}"))?;
        bytes += chunk.len() as u64;
        if kept_body.len() < keep_body {
            let want = (keep_body - kept_body.len()).min(chunk.len());
            kept_body.extend_from_slice(&chunk[..want]);
        }
        let _ = incoming.flow_control().release_capacity(chunk.len());
    }
    phases.download = Some(body_began.elapsed());
    sample.bytes = bytes;
    sample.answered = true;
    if keep_body > BODY_PREVIEW {
        *saved = Some(kept_body.clone());
    }
    kept_body.truncate(BODY_PREVIEW);
    sample.body = kept_body;
    Ok((status, location))
}

impl Stream {
    async fn write_all(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        match self {
            Stream::Plain(s) => s.write_all(bytes).await,
            Stream::Tls(s) => {
                s.write_all(bytes).await?;
                s.flush().await
            }
            Stream::H2(_) => Err(std::io::Error::other(
                "an HTTP/2 connection is h2's to write",
            )),
        }
    }

    async fn read_buf(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize> {
        if buf.capacity() - buf.len() < 4096 {
            buf.reserve(READ);
        }
        match self {
            Stream::Plain(s) => s.read_buf(buf).await,
            Stream::Tls(s) => s.read_buf(buf).await,
            Stream::H2(_) => Err(std::io::Error::other(
                "an HTTP/2 connection is h2's to read",
            )),
        }
    }
}

/// The addresses to try, in order, timing the lookup into `sample`
async fn resolve(
    host: &str,
    port: u16,
    connect_to: Option<IpAddr>,
    settings: &Settings,
    sample: &mut Sample,
) -> Result<Vec<SocketAddr>, String> {
    if let Some(ip) = connect_to {
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    if let Ok(ip) = host.parse::<IpAddr>() {
        if let Some(family) = settings.family {
            if !family.fits(&ip) {
                return Err(format!("{ip} isn't an {} address", family_name(family)));
            }
        }
        return Ok(vec![SocketAddr::new(ip, port)]);
    }
    let began = Instant::now();
    let found: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
        .await
        .map_err(|e| format!("DNS: {}", root_cause(&e)))?
        .collect();
    sample.phases.dns = Some(began.elapsed());
    let addresses: Vec<SocketAddr> = found
        .into_iter()
        .filter(|a| settings.family.is_none_or(|f| f.fits(&a.ip())))
        .collect();
    if addresses.is_empty() {
        return Err(match settings.family {
            Some(family) => format!("{host} has no {} address", family_name(family)),
            None => format!("{host} resolved to no address"),
        });
    }
    Ok(addresses)
}

fn family_name(family: Family) -> &'static str {
    match family {
        Family::V4 => "IPv4",
        Family::V6 => "IPv6",
    }
}

/// Connect to the first address that answers, from `--interface` if one
/// was named; the stream, the ends of it, and how long it took
async fn connect(
    addresses: &[SocketAddr],
    settings: &Settings,
    sample: &mut Sample,
) -> Result<(TcpStream, Option<SocketAddr>, Option<SocketAddr>, Duration), String> {
    let began = Instant::now();
    let mut last = None;
    for address in addresses {
        let socket = match address {
            SocketAddr::V4(_) => TcpSocket::new_v4(),
            SocketAddr::V6(_) => TcpSocket::new_v6(),
        }
        .map_err(|e| root_cause(&e))?;
        if let Some(bind) = settings.bind {
            if bind.is_ipv4() == address.is_ipv4() {
                socket
                    .bind(SocketAddr::new(bind, 0))
                    .map_err(|e| format!("binding to {bind}: {}", root_cause(&e)))?;
            }
        }
        match socket.connect(*address).await {
            Ok(stream) => {
                let took = began.elapsed();
                let _ = stream.set_nodelay(true);
                let remote = stream.peer_addr().ok();
                let local = stream.local_addr().ok();
                sample.remote = remote;
                sample.local = local;
                sample.phases.connect = Some(took);
                return Ok((stream, remote, local, took));
            }
            Err(e) => last = Some(format!("{address}: {}", root_cause(&e))),
        }
    }
    Err(last.unwrap_or_else(|| "no address to connect to".into()))
}

/// What the handshake settled on, and the certificate it was shown
fn tls_details(stream: &TlsStream<TcpStream>) -> Tls {
    let (_, conn) = stream.get_ref();
    let version = match conn.protocol_version() {
        Some(ProtocolVersion::TLSv1_3) => "TLS 1.3".to_string(),
        Some(ProtocolVersion::TLSv1_2) => "TLS 1.2".to_string(),
        Some(other) => format!("{other:?}"),
        None => "TLS".to_string(),
    };
    let cipher = conn
        .negotiated_cipher_suite()
        .map(|s| format!("{:?}", s.suite()))
        .unwrap_or_default();
    let alpn = conn
        .alpn_protocol()
        .map(|p| String::from_utf8_lossy(p).into_owned());
    let resumed = conn.handshake_kind() == Some(HandshakeKind::Resumed);
    let cert = conn
        .peer_certificates()
        .and_then(|certs| certs.first())
        .and_then(|der| cert::parse(der.as_ref()));
    Tls {
        version,
        cipher,
        alpn,
        resumed,
        cert,
    }
}

/// Through a proxy the connection is reqwest's: the time to the first
/// byte and the download are measured, the phases before them aren't
async fn via_proxy(
    url: &Url,
    settings: &Settings,
    sample: &mut Sample,
    saved: &mut Option<Vec<u8>>,
) -> Result<(), String> {
    use crate::request::{Request, RequestSettings};
    let request = Request::new(
        url.to_string(),
        settings.method.to_string(),
        (!settings.body.is_empty()).then(|| settings.body.to_vec()),
        &[],
        RequestSettings {
            timeout: settings.timeout.as_secs().max(1) as u32,
            insecure: settings.insecure,
            disable_compression: !settings.compression,
            disable_keepalive: !settings.keep_alive,
            disable_redirects: !settings.follow_redirects,
            proxy: settings.proxy.clone(),
            user_agent: settings.user_agent.clone(),
            idle_connections: 1,
        },
    )
    .map_err(|e| e.to_string())?;
    let sender = request.build_client().map_err(|e| e.to_string())?;
    let client = sender.client().map_err(|e| e.message)?;
    let sent_at = Instant::now();
    let mut builder = client.request(settings.method.clone(), url.clone());
    for (name, value) in &settings.headers {
        builder = builder.header(name, value);
    }
    if !settings.body.is_empty() {
        builder = builder.body(settings.body.clone());
    }
    let mut response = builder.send().await.map_err(|e| root_cause(&e))?;
    sample.phases.ttfb = Some(sent_at.elapsed());
    let body_began = Instant::now();
    sample.status = Some(response.status().as_u16());
    sample.version = Some(format!("{:?}", response.version()));
    sample.remote = response.remote_addr();
    sample.headers = response
        .headers()
        .iter()
        .map(|(k, v)| {
            (
                k.to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let mut kept_body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|e| root_cause(&e))? {
        sample.bytes += chunk.len() as u64;
        if kept_body.len() < settings.keep_body {
            let want = (settings.keep_body - kept_body.len()).min(chunk.len());
            kept_body.extend_from_slice(&chunk[..want]);
        }
    }
    sample.phases.download = Some(body_began.elapsed());
    sample.answered = true;
    if settings.keep_body > BODY_PREVIEW {
        *saved = Some(kept_body.clone());
    }
    kept_body.truncate(BODY_PREVIEW);
    sample.body = kept_body;
    Ok(())
}

/// `--cmd`: run the command in the shell and time it; its exit code is
/// the status
async fn run_command(command: &str, sample: &mut Sample) -> Result<(), String> {
    #[cfg(windows)]
    let mut process = {
        let mut c = tokio::process::Command::new("cmd");
        c.arg("/C").arg(command);
        c
    };
    #[cfg(not(windows))]
    let mut process = {
        let mut c = tokio::process::Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    process
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true);
    let status = process
        .status()
        .await
        .map_err(|e| format!("running it: {}", root_cause(&e)))?;
    sample.answered = true;
    let code = status.code().unwrap_or(-1);
    sample.status = Some(code.clamp(0, u16::MAX as i32) as u16);
    if !status.success() {
        sample.error = Some(match status.code() {
            Some(code) => format!("exit code {code}"),
            None => "killed by a signal".into(),
        });
    }
    Ok(())
}

/// `--interface`: an address on this machine, by its interface's name or
/// as itself
pub fn local_address(name: &str, family: Option<Family>) -> Result<IpAddr, String> {
    if let Ok(ip) = name.parse::<IpAddr>() {
        return Ok(ip);
    }
    match interface_address(name, family) {
        Some(ip) => Ok(ip),
        None => Err(format!(
            "{name:?} isn't an interface with an address{}; give the address itself",
            family.map_or(String::new(), |f| format!(" ({})", family_name(f)))
        )),
    }
}

#[cfg(unix)]
fn interface_address(name: &str, family: Option<Family>) -> Option<IpAddr> {
    use std::ffi::CStr;
    let mut list: *mut libc::ifaddrs = std::ptr::null_mut();
    // SAFETY: getifaddrs fills `list` with a chain freed by freeifaddrs
    // below; every pointer read is checked for null first
    unsafe {
        if libc::getifaddrs(&mut list) != 0 {
            return None;
        }
        let mut found: Vec<IpAddr> = Vec::new();
        let mut cursor = list;
        while !cursor.is_null() {
            let entry = &*cursor;
            cursor = entry.ifa_next;
            if entry.ifa_name.is_null() || entry.ifa_addr.is_null() {
                continue;
            }
            if CStr::from_ptr(entry.ifa_name).to_str().ok() != Some(name) {
                continue;
            }
            let ip = match i32::from((*entry.ifa_addr).sa_family) {
                libc::AF_INET => {
                    let sin = &*(entry.ifa_addr as *const libc::sockaddr_in);
                    IpAddr::V4(Ipv4Addr::from(u32::from_be(sin.sin_addr.s_addr)))
                }
                libc::AF_INET6 => {
                    let sin6 = &*(entry.ifa_addr as *const libc::sockaddr_in6);
                    IpAddr::V6(std::net::Ipv6Addr::from(sin6.sin6_addr.s6_addr))
                }
                _ => continue,
            };
            found.push(ip);
        }
        libc::freeifaddrs(list);
        // A routable address before a link-local one
        found.sort_by_key(|ip| match ip {
            IpAddr::V6(v6) if (v6.segments()[0] & 0xffc0) == 0xfe80 => 1,
            _ => 0,
        });
        found
            .into_iter()
            .find(|ip| family.is_none_or(|f| f.fits(ip)))
    }
}

#[cfg(not(unix))]
fn interface_address(_name: &str, _family: Option<Family>) -> Option<IpAddr> {
    None
}

// ─── Output ──────────────────────────────────────────────────────────────────

/// `2026-10-10T13:55:36Z`
fn iso(secs: i64) -> String {
    let c = crate::logs::civil(secs);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        c.year, c.month, c.day, c.hour, c.minute, c.second
    )
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

fn ms_of(d: Option<Duration>) -> serde_json::Value {
    match d {
        Some(d) => serde_json::json!(ms(d)),
        None => serde_json::Value::Null,
    }
}

fn tls_json(tls: &Tls, now: i64) -> serde_json::Value {
    serde_json::json!({
        "version": tls.version,
        "cipher": tls.cipher,
        "alpn": tls.alpn,
        "resumed": tls.resumed,
        "cert": tls.cert.as_ref().map(|c| serde_json::json!({
            "subject": c.subject,
            "issuer": c.issuer,
            "names": c.names,
            "not_before": iso(c.not_before),
            "not_after": iso(c.not_after),
            "days_left": c.days_left(now),
            "valid": c.valid_at(now),
        })),
    })
}

/// One sample as a line of JSON, for `--jsonl`
pub fn sample_json(sample: &Sample, target: &Target) -> serde_json::Value {
    serde_json::json!({
        "schema_version": 1,
        "target": target.name,
        "url": target.shown,
        "seq": sample.seq,
        "time": iso(sample.wall),
        "at_s": (sample.at.as_secs_f64() * 1000.0).round() / 1000.0,
        "ok": sample.ok(),
        "answered": sample.answered,
        "timeout": sample.timeout,
        "status": sample.status,
        "error": sample.error,
        "timings_ms": {
            "dns": ms_of(sample.phases.dns),
            "connect": ms_of(sample.phases.connect),
            "tls": ms_of(sample.phases.tls),
            "ttfb": ms_of(sample.phases.ttfb),
            "download": ms_of(sample.phases.download),
            "total": ms(sample.total),
        },
        "reused_connection": sample.reused,
        "bytes": sample.bytes,
        "remote": sample.remote.map(|a| a.to_string()),
        "local": sample.local.map(|a| a.to_string()),
        "http_version": sample.version,
        "tls": sample.tls.as_ref().map(|t| tls_json(t, sample.wall)),
        "redirects": sample.hops.iter().map(|h| serde_json::json!({
            "url": h.url, "status": h.status, "took_ms": ms(h.took),
        })).collect::<Vec<_>>(),
        "slo": {
            "pass": sample.violations.is_empty(),
            "violations": sample.violations.iter().map(|v| serde_json::json!({
                "key": v.key, "threshold_ms": ms(v.limit), "actual_ms": ms(v.actual),
            })).collect::<Vec<_>>(),
        },
    })
}

pub const CSV_HEADER: &str =
    "time,target,seq,ok,status,dns_ms,connect_ms,tls_ms,ttfb_ms,download_ms,total_ms,bytes,remote,error";

/// One sample as a line of CSV, under `CSV_HEADER`
pub fn sample_csv(sample: &Sample, target: &Target) -> String {
    let field = |s: &str| {
        if s.contains([',', '"', '\n']) {
            format!("\"{}\"", s.replace('"', "\"\""))
        } else {
            s.to_string()
        }
    };
    let num = |d: Option<Duration>| d.map(|d| ms(d).to_string()).unwrap_or_default();
    [
        iso(sample.wall),
        field(&target.name),
        sample.seq.to_string(),
        sample.ok().to_string(),
        sample.status.map(|s| s.to_string()).unwrap_or_default(),
        num(sample.phases.dns),
        num(sample.phases.connect),
        num(sample.phases.tls),
        num(sample.phases.ttfb),
        num(sample.phases.download),
        ms(sample.total).to_string(),
        sample.bytes.to_string(),
        sample.remote.map(|a| a.to_string()).unwrap_or_default(),
        field(sample.error.as_deref().unwrap_or("")),
    ]
    .join(",")
}

/// One sample on one line, for a terminal that isn't a screen
pub fn sample_line(sample: &Sample, target: &Target) -> String {
    let mut out = format!("{:<24} seq={:<4}", target.name, sample.seq);
    if sample.timeout {
        out.push_str(&format!(" TIMEOUT after {}", format::latency(sample.total)));
        return out;
    }
    if let Some(error) = sample.error.as_ref().filter(|_| !sample.answered) {
        out.push_str(&format!(
            " FAILED {error} after {}",
            format::latency(sample.total)
        ));
        return out;
    }
    match (sample.status, &sample.error) {
        (Some(status), Some(error)) => out.push_str(&format!(" {status} {error}")),
        (Some(status), None) => out.push_str(&format!(" {status}")),
        (None, _) => out.push_str(" connected"),
    }
    for (name, took) in sample.phases.each() {
        if let Some(took) = took {
            out.push_str(&format!("  {name} {}", format::latency(took)));
        }
    }
    out.push_str(&format!("  total {}", format::latency(sample.total)));
    if sample.bytes > 0 {
        out.push_str(&format!("  {}", format::bytes(sample.bytes as f64)));
    }
    if let Some(remote) = sample.remote {
        out.push_str(&format!("  from {remote}"));
    }
    if let Some(tls) = &sample.tls {
        if tls.resumed {
            out.push_str("  (TLS resumed)");
        }
    }
    for v in &sample.violations {
        out.push_str(&format!("  SLO {} > {}", v.key, format::latency(v.limit)));
    }
    out
}

/// The run's report as JSON, for `--json` and `--save`
pub fn json_report(state: &State, settings: &Settings, interrupted: bool) -> serde_json::Value {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let hist = |h: &Histogram| {
        if h.count() == 0 {
            serde_json::Value::Null
        } else {
            serde_json::json!({
                "count": h.count(),
                "median_ms": h.percentile(50.0) as f64 / 1000.0,
                "p99_ms": h.percentile(99.0) as f64 / 1000.0,
                "max_ms": h.percentile(100.0) as f64 / 1000.0,
            })
        }
    };
    let us = |v: u64| v as f64 / 1000.0;
    let targets: Vec<serde_json::Value> = state
        .targets
        .iter()
        .map(|t| {
            let s = t.whole();
            serde_json::json!({
                "name": t.target.name,
                "target": t.target.shown,
                "sent": s.sent,
                "answered": s.answered,
                "lost": s.sent - s.answered,
                "loss": (s.loss() * 10_000.0).round() / 10_000.0,
                "timeouts": s.timeouts,
                "latency_ms": {
                    "last": s.last.map(us),
                    "min": us(s.min), "avg": us(s.avg), "max": us(s.max),
                    "jitter": us(s.jitter),
                    "p50": us(s.p50), "p95": us(s.p95), "p99": us(s.p99),
                },
                "phases_ms": {
                    "dns": hist(&t.dns),
                    "connect": hist(&t.connect),
                    "tls_full": hist(&t.tls_full),
                    "tls_resumed": hist(&t.tls_resumed),
                    "ttfb": hist(&t.ttfb),
                    "download": hist(&t.download),
                },
                "reused_connections": t.reused,
                "bytes": t.bytes,
                "status_codes": t.statuses.iter().map(|(k, v)| (k.to_string(), *v)).collect::<BTreeMap<_, _>>(),
                "failures": t.causes,
                "remote": t.last_remote.map(|a| a.to_string()),
                "local": t.last_local.map(|a| a.to_string()),
                "http_version": t.last_version,
                "tls": t.last_tls.as_ref().map(|tls| tls_json(tls, now)),
                "slo": {
                    "pass": t.violations == 0,
                    "violations": t.violations,
                    "worst_ms": t.worst.iter().map(|(k, d)| (k.to_string(), ms(*d))).collect::<BTreeMap<_, _>>(),
                },
                "findings": crate::diagnose::findings(t, settings, now).iter().map(|f| serde_json::json!({
                    "level": f.level.name(), "text": f.text,
                })).collect::<Vec<_>>(),
            })
        })
        .collect();
    serde_json::json!({
        "schema_version": 1,
        "mode": "ping",
        "every_s": settings.every.as_secs_f64(),
        "elapsed_s": (state.elapsed().as_secs_f64() * 1000.0).round() / 1000.0,
        "interrupted": interrupted,
        "slo": settings.slo.limits().iter().map(|(k, d)| (k.to_string(), ms(*d))).collect::<BTreeMap<_, _>>(),
        "targets": targets,
    })
}

/// How a target did: ✔, ▲ or ✖
pub fn glyph(t: &TargetStats) -> char {
    let s = t.whole();
    if s.sent == 0 {
        '▲'
    } else if s.answered == 0 || s.loss() >= 0.05 || t.failing() >= 0.05 {
        '✖'
    } else if s.loss() > 0.0 || t.violations > 0 || t.failing() > 0.0 {
        '▲'
    } else {
        '✔'
    }
}

/// The report left in the shell when the run ends
pub fn report(state: &State, settings: &Settings) -> String {
    use std::fmt::Write;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    let mut out = String::new();
    let elapsed = state.elapsed();
    let _ = writeln!(
        out,
        "pepe ping · {} · every {} · {}",
        match state.targets.len() {
            1 => state.targets[0].target.shown.clone(),
            n => format!("{n} targets"),
        },
        every_text(settings.every),
        if elapsed < Duration::from_secs(60) {
            format!("{:.1}s", elapsed.as_secs_f64())
        } else {
            format::span(elapsed)
        }
    );
    let lat = |us: u64| format::latency(Duration::from_micros(us));
    for t in &state.targets {
        let s = t.whole();
        let mut line = format!(
            "{} {} · {} sent, {} answered, {} loss",
            glyph(t),
            t.target.name,
            s.sent,
            s.answered,
            percent(s.loss())
        );
        if s.timeouts > 0 {
            let _ = write!(line, " ({} timed out)", s.timeouts);
        }
        if s.answered > 0 {
            let _ = write!(
                line,
                " · min {} avg {} max {} jitter {} · p50 {} p95 {} p99 {}",
                lat(s.min),
                lat(s.avg),
                lat(s.max),
                lat(s.jitter),
                lat(s.p50),
                lat(s.p95),
                lat(s.p99)
            );
        }
        let _ = writeln!(out, "{line}");
        let median = |h: &Histogram| (h.count() > 0).then(|| lat(h.percentile(50.0)));
        let mut phases: Vec<String> = Vec::new();
        if let Some(d) = median(&t.dns) {
            phases.push(format!("dns {d}"));
        }
        if let Some(d) = median(&t.connect) {
            phases.push(format!("connect {d}"));
        }
        match (median(&t.tls_full), median(&t.tls_resumed)) {
            (Some(full), Some(resumed)) => phases.push(format!("tls {full} ({resumed} resumed)")),
            (Some(full), None) => phases.push(format!("tls {full}")),
            (None, Some(resumed)) => phases.push(format!("tls {resumed} resumed")),
            (None, None) => {}
        }
        if let Some(d) = median(&t.ttfb) {
            phases.push(format!("first byte {d}"));
        }
        if let Some(d) = median(&t.download) {
            phases.push(format!("download {d}"));
        }
        if !phases.is_empty() {
            let mut line = format!("  medians: {}", phases.join(" · "));
            if t.bytes > 0 && t.answered > 0 {
                let _ = write!(
                    line,
                    " · {} a response",
                    format::bytes(t.bytes as f64 / t.answered as f64)
                );
            }
            if t.reused > 0 {
                let _ = write!(line, " · {} on a kept connection", t.reused);
            }
            let _ = writeln!(out, "{line}");
        }
        let mut about: Vec<String> = Vec::new();
        if let Some(tls) = &t.last_tls {
            about.push(tls.version.clone());
            if !tls.cipher.is_empty() {
                about.push(tls.cipher.clone());
            }
            if let Some(alpn) = &tls.alpn {
                about.push(alpn.clone());
            }
        }
        if let Some(version) = &t.last_version {
            if t.last_tls.is_none() {
                about.push(version.clone());
            }
        }
        if let Some(remote) = t.last_remote {
            about.push(remote.to_string());
        }
        if let Some(cert) = t.last_tls.as_ref().and_then(|t| t.cert.as_ref()) {
            let mut said = format!("certificate for {}", cert.subject);
            if !cert.issuer.is_empty() {
                let _ = write!(said, " by {}", cert.issuer);
            }
            let _ = write!(said, ", {}", cert.expiry(now));
            about.push(said);
        }
        if !about.is_empty() {
            let _ = writeln!(out, "  {}", about.join(" · "));
        }
        if !t.statuses.is_empty() {
            let codes: Vec<String> = t
                .statuses
                .iter()
                .map(|(code, n)| match t.target.kind {
                    Kind::Cmd(_) => format!("exit {code} ×{n}"),
                    _ => format!("HTTP {code} ×{n}"),
                })
                .collect();
            let _ = writeln!(out, "  {}", codes.join(" · "));
        }
        for (cause, n) in &t.causes {
            let _ = writeln!(out, "  ✖ {cause} ×{n}");
        }
        for finding in crate::diagnose::findings(t, settings, now) {
            let _ = writeln!(out, "  {} {}", finding.level.glyph(), finding.text);
        }
    }
    if !settings.slo.is_empty() {
        let broken: u64 = state.targets.iter().map(|t| t.violations).sum();
        let limits: Vec<String> = settings
            .slo
            .limits()
            .iter()
            .map(|(k, d)| format!("{k} ≤ {}", format::latency(*d)))
            .collect();
        if broken == 0 {
            let _ = writeln!(out, "✔ SLO held: {}", limits.join(", "));
        } else {
            let worst: Vec<String> = state
                .targets
                .iter()
                .flat_map(|t| t.worst.iter())
                .map(|(k, d)| format!("{k} {}", format::latency(*d)))
                .collect();
            let _ = writeln!(
                out,
                "✖ SLO broken {} times ({}) · worst {}",
                broken,
                limits.join(", "),
                worst.join(", ")
            );
        }
    }
    out
}

/// `1s`, `500ms`, `2m`: the interval as it was asked for
pub fn every_text(every: Duration) -> String {
    if every >= Duration::from_secs(1) && every.subsec_millis() == 0 {
        format::span(every)
    } else {
        format::latency_short(every.as_micros() as u64)
    }
}

pub fn percent(share: f64) -> String {
    let p = share * 100.0;
    if p == 0.0 {
        "0%".into()
    } else if p < 10.0 {
        format!("{p:.1}%")
    } else {
        format!("{p:.0}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_become_targets() {
        let one = |word: &str| parse_target(word, false, 80).unwrap().remove(0);
        assert_eq!(one("example.com").shown, "https://example.com/");
        assert_eq!(one("example.com:8080").shown, "http://example.com:8080/");
        assert_eq!(one("localhost").shown, "http://localhost/");
        assert_eq!(one("http://a.test/x?y=1").name, "a.test");
        assert_eq!(one("aws:eu-west-1").name, "aws:eu-west-1");
        assert_eq!(
            one("aws:eu-west-1").shown,
            "https://ec2.eu-west-1.amazonaws.com/ping"
        );
        assert_eq!(one("::1").shown, "http://[::1]/");
        assert!(parse_target("ftp://x", false, 80).is_err());
        assert!(parse_target("", false, 80).is_err());
    }

    #[test]
    fn tcp_targets_have_a_port() {
        let one = |word: &str| parse_target(word, true, 22).unwrap().remove(0);
        assert_eq!(one("example.com").shown, "example.com:22");
        assert_eq!(one("example.com:443").shown, "example.com:443");
        assert_eq!(one("https://example.com").shown, "example.com:443");
        assert!(matches!(
            one("[::1]:8080").kind,
            Kind::Tcp { port: 8080, .. }
        ));
    }

    #[test]
    fn a_range_is_its_hosts() {
        let targets = parse_target("10.0.0.0/30", false, 80).unwrap();
        let shown: Vec<&str> = targets.iter().map(|t| t.shown.as_str()).collect();
        assert_eq!(shown, ["http://10.0.0.1/", "http://10.0.0.2/"]);
        let targets = parse_target("10.0.0.0/31:8080", true, 80).unwrap();
        assert_eq!(targets.len(), 2);
        assert_eq!(targets[0].shown, "10.0.0.0:8080");
        assert!(parse_target("10.0.0.0/16", false, 80).is_err());
        assert!(parse_target("10.0.0.0/33", false, 80).is_err());
    }

    #[test]
    fn intervals_and_slos_parse() {
        let s = Duration::from_secs;
        assert_eq!(interval("1s", s(1)), Ok(s(1)));
        assert_eq!(interval("500ms", s(1)), Ok(Duration::from_millis(500)));
        assert_eq!(interval("0.5", s(1)), Ok(Duration::from_millis(500)));
        assert_eq!(interval("2m", s(1)), Ok(s(120)));
        assert!(interval("0", s(1)).is_err());
        assert!(interval("fast", s(1)).is_err());
        let slo = Slo::parse("total=500,ttfb=200ms, tls=1s").unwrap();
        assert_eq!(slo.total, Some(Duration::from_millis(500)));
        assert_eq!(slo.ttfb, Some(Duration::from_millis(200)));
        assert_eq!(slo.tls, Some(s(1)));
        assert_eq!(slo.limits().len(), 3);
        assert!(Slo::parse("speed=1").is_err());
        assert!(Slo::parse("total").is_err());
    }

    fn sample(seq: u64, us: u64, answered: bool) -> Sample {
        let mut s = Sample::new(0, seq, Duration::from_secs(seq));
        s.total = Duration::from_micros(us);
        s.answered = answered;
        s.status = answered.then_some(200);
        s.phases.ttfb = answered.then(|| Duration::from_micros(us / 2));
        s
    }

    #[test]
    fn stats_over_the_run_and_a_window() {
        let target = parse_target("example.com", false, 80).unwrap().remove(0);
        let mut t = TargetStats::new(target);
        for (seq, us) in [(1, 100), (2, 300), (3, 200), (4, 1_000)] {
            t.sent += 1;
            t.record(sample(seq, us, true), None);
        }
        t.sent += 1;
        let mut lost = sample(5, 5_000_000, false);
        lost.timeout = true;
        lost.error = Some("no answer in 5s".into());
        t.record(lost, None);
        let whole = t.whole();
        assert_eq!((whole.sent, whole.answered, whole.timeouts), (5, 4, 1));
        assert_eq!((whole.min, whole.max, whole.avg), (100, 1_000, 400));
        // |100-300| + |300-200| + |200-1000| over three steps
        assert_eq!(whole.jitter, (200 + 100 + 800) / 3);
        assert_eq!(whole.last, Some(1_000));
        assert_eq!(percent(whole.loss()), "20%");
        assert_eq!(glyph(&t), '✖');
        // The last three seconds: pings 3, 4 and the lost one
        let window = t.window(5.0, 2.5);
        assert_eq!((window.sent, window.answered), (3, 2));
        assert_eq!((window.min, window.max), (200, 1_000));
        assert_eq!(window.jitter, 800);
        assert_eq!(window.p50, 200);
        assert_eq!(t.causes.get("no answer in 5s"), Some(&1));
        let report = report(
            &State {
                started: Instant::now(),
                targets: vec![t],
                done: true,
                paused: false,
                bells: 0,
            },
            &settings(),
        );
        assert!(
            report.contains("5 sent, 4 answered, 20% loss (1 timed out)"),
            "{report}"
        );
        assert!(report.contains("HTTP 200 ×4"), "{report}");
    }

    fn settings() -> Settings {
        Settings {
            every: Duration::from_secs(1),
            timeout: Duration::from_secs(5),
            family: None,
            bind: None,
            insecure: false,
            keep_alive: false,
            follow_redirects: true,
            refused_is_pong: true,
            slo: Slo::default(),
            keep_body: 0,
            proxy: None,
            user_agent: "pepe/test".into(),
            headers: HeaderMap::new(),
            method: Method::GET,
            body: Bytes::new(),
            count: None,
            duration: None,
            compression: false,
            http1: false,
        }
    }

    #[test]
    fn a_sample_is_a_line_in_each_format() {
        let target = parse_target("example.com", false, 80).unwrap().remove(0);
        let mut s = sample(7, 44_000, true);
        s.phases.dns = Some(Duration::from_millis(2));
        s.phases.connect = Some(Duration::from_millis(10));
        s.bytes = 1234;
        s.wall = 1_791_460_536;
        let line = sample_line(&s, &target);
        assert!(
            line.contains("seq=7") && line.contains(" 200") && line.contains("dns 2.00ms"),
            "{line}"
        );
        assert!(
            line.contains("total 44.00ms") && line.contains("1.2 KiB"),
            "{line}"
        );
        let csv = sample_csv(&s, &target);
        assert!(
            csv.starts_with("2026-10-08T11:55:36Z,example.com,7,true,200,2,10,,22,,44,1234,,"),
            "{csv}"
        );
        let json = sample_json(&s, &target);
        assert_eq!(json["timings_ms"]["total"], 44.0);
        assert_eq!(json["timings_ms"]["tls"], serde_json::Value::Null);
        assert_eq!(json["ok"], true);
        let mut lost = sample(8, 5_000_000, false);
        lost.timeout = true;
        lost.error = Some("no answer in 5s".into());
        let line = sample_line(&lost, &target);
        assert!(line.contains("TIMEOUT after 5.00s"), "{line}");
        assert_eq!(every_text(Duration::from_secs(1)), "1s");
        assert_eq!(every_text(Duration::from_millis(300)), "300ms");
        assert_eq!(every_text(Duration::from_secs(120)), "2m");
    }

    #[test]
    fn slo_violations_are_found() {
        let slo = Slo::parse("total=30,ttfb=100").unwrap();
        let s = sample(1, 44_000, true);
        let broken = slo.check(&s);
        assert_eq!(broken.len(), 1);
        assert_eq!(broken[0].key, "total");
        assert_eq!(broken[0].actual, Duration::from_millis(44));
    }

    /// A server that answers every request with a small body and closes
    async fn server(body: &'static str) -> String {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let mut head = Vec::new();
                    while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                        match socket.read(&mut buf).await {
                            Ok(n) if n > 0 => head.extend_from_slice(&buf[..n]),
                            _ => return,
                        }
                    }
                    let answer = if head.starts_with(b"GET /moved") {
                        "HTTP/1.1 302 Found\r\nLocation: /\r\nContent-Length: 0\r\n\r\n".to_string()
                    } else {
                        format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: {}\r\n\r\n{body}",
                            body.len()
                        )
                    };
                    let _ = socket.write_all(answer.as_bytes()).await;
                });
            }
        });
        url
    }

    #[tokio::test]
    async fn pings_a_server_and_follows_a_redirect() {
        let url = server("hello").await;
        let mut settings = settings();
        settings.every = Duration::from_millis(10);
        settings.count = Some(3);
        settings.keep_body = BODY_PREVIEW;
        let moved = format!("{url}moved");
        let targets = parse_target(&moved, false, 80).unwrap();
        let shared = start(targets, settings.clone());
        for _ in 0..200 {
            if shared.lock().done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let state = shared.lock();
        assert!(state.done, "the run should end after three pings");
        let t = &state.targets[0];
        assert_eq!((t.sent, t.answered), (3, 3));
        assert_eq!(t.bytes, 15);
        let last = t.recent.back().unwrap();
        assert_eq!(last.status, Some(200));
        assert_eq!(last.hops.len(), 1);
        assert_eq!(last.hops[0].status, 302);
        assert_eq!(last.body, b"hello");
        assert!(last.phases.connect.is_some() && last.phases.ttfb.is_some());
        assert!(last.phases.tls.is_none());
        // The phases are parts of the total, counted once each
        let parts: Duration = last.phases.each().iter().filter_map(|(_, d)| *d).sum();
        assert!(parts <= last.total, "{parts:?} > {:?}", last.total);
        assert!(last.remote.is_some() && last.local.is_some());
        assert_eq!(last.version.as_deref(), Some("HTTP/1.1"));
        assert_eq!(glyph(t), '✔');
        let json = json_report(&state, &settings, false);
        assert_eq!(json["targets"][0]["answered"], 3);
        assert_eq!(json["targets"][0]["status_codes"]["200"], 3);
    }

    #[tokio::test]
    async fn a_closed_port_is_a_failure_and_tcp_pings_connect() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let mut settings = settings();
        settings.every = Duration::from_millis(10);
        settings.count = Some(2);
        // Windows takes a second or two to refuse a connection to a closed
        // port; the refusal, not a timeout, is what this test wants
        settings.timeout = Duration::from_secs(10);
        let targets = parse_target(&format!("http://127.0.0.1:{port}/"), false, 80).unwrap();
        let shared = start(targets, settings.clone());
        for _ in 0..2_000 {
            if shared.lock().done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        {
            let state = shared.lock();
            let t = &state.targets[0];
            assert_eq!((t.sent, t.answered), (2, 0));
            assert!(
                t.causes.keys().any(|c| c.contains("refused")),
                "{:?}",
                t.causes
            );
            assert!(state.nothing_answered());
        }

        // --tcp to an open port: connected, with no HTTP at all
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let targets = parse_target(&format!("127.0.0.1:{port}"), true, 80).unwrap();
        let shared = start(targets, settings.clone());
        for _ in 0..300 {
            if shared.lock().done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let state = shared.lock();
        let t = &state.targets[0];
        assert_eq!((t.sent, t.answered), (2, 2));
        assert!(t.recent.back().unwrap().status.is_none());
        assert_eq!(t.recent.back().unwrap().outcome(), "connected");
    }

    #[tokio::test]
    async fn speaks_http2_on_a_connection_that_negotiated_it() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut connection = h2::server::handshake(socket).await.unwrap();
            while let Some(next) = connection.accept().await {
                let (request, mut respond) = next.unwrap();
                let location = request.uri().path() == "/moved";
                let response = http::Response::builder()
                    .status(if location { 302 } else { 200 })
                    .header("content-type", "text/plain")
                    .header("location", "/")
                    .body(())
                    .unwrap();
                let mut stream = respond.send_response(response, false).unwrap();
                stream
                    .send_data(Bytes::from_static(b"hello h2"), true)
                    .unwrap();
            }
        });
        let stream = tokio::net::TcpStream::connect(addr).await.unwrap();
        let (send, connection) = h2::client::handshake(stream).await.unwrap();
        tokio::spawn(async move {
            let _ = connection.await;
        });
        let settings = settings();
        let mut sample = Sample::new(0, 1, Duration::ZERO);
        let mut phases = Phases::default();
        let mut saved = None;
        let url = Url::parse(&format!("http://{addr}/x")).unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("x-run", HeaderValue::from_static("1"));
        let (status, location) = h2_exchange(
            send.clone(),
            &url,
            &Method::GET,
            &headers,
            &Bytes::new(),
            &settings,
            &mut sample,
            &mut phases,
            &mut saved,
        )
        .await
        .unwrap();
        assert_eq!(status, 200);
        assert!(
            location.is_some(),
            "the test server sends one on every answer"
        );
        assert_eq!(sample.version.as_deref(), Some("HTTP/2"));
        assert_eq!(sample.bytes, 8);
        assert!(sample.answered);
        assert!(phases.ttfb.is_some() && phases.download.is_some());
        assert!(sample
            .headers
            .iter()
            .any(|(n, v)| n == "content-type" && v == "text/plain"));
        // The same sender carries the next request: that is keep-alive
        let moved = Url::parse(&format!("http://{addr}/moved")).unwrap();
        let mut again = Sample::new(0, 2, Duration::ZERO);
        let (status, _) = h2_exchange(
            send,
            &moved,
            &Method::GET,
            &headers,
            &Bytes::new(),
            &settings,
            &mut again,
            &mut Phases::default(),
            &mut None,
        )
        .await
        .unwrap();
        assert_eq!(status, 302);
    }

    #[tokio::test]
    async fn a_command_is_timed_and_its_exit_code_kept() {
        let mut settings = settings();
        settings.every = Duration::from_millis(10);
        settings.count = Some(1);
        let targets = vec![Target {
            name: "true".into(),
            shown: "true".into(),
            kind: Kind::Cmd("exit 3".into()),
        }];
        let shared = start(targets, settings);
        for _ in 0..300 {
            if shared.lock().done {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let state = shared.lock();
        let last = state.targets[0].recent.back().unwrap();
        assert_eq!(last.status, Some(3));
        assert!(!last.ok());
        assert_eq!(last.error.as_deref(), Some("exit code 3"));
    }
}
