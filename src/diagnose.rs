//! What a ping's numbers mean: where a target's time goes and what would
//! change it, said in a few lines. The rules read what `ping` already
//! measured: the medians of each phase, how the TLS sessions went, what
//! the last response said about itself, the losses and the tail.

use std::time::Duration;

use crate::cache::CacheStatus;
use crate::ping::{Kind, Sample, Settings, TargetStats};
use crate::ui::format;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// Something is as it should be, worth saying
    Good,
    /// Worth knowing; nothing to fix
    Note,
    /// Costs time, or will soon
    Warn,
    /// Broken
    Bad,
}

impl Level {
    pub fn glyph(self) -> char {
        match self {
            Level::Good => '✔',
            Level::Note => '·',
            Level::Warn => '▲',
            Level::Bad => '✖',
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Level::Good => "good",
            Level::Note => "note",
            Level::Warn => "warn",
            Level::Bad => "bad",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub level: Level,
    pub text: String,
}

/// A phase's share of the total is worth a line past this
const SHARE_OF_NOTE: f64 = 0.3;
/// A DNS lookup slower than this is slow whatever the rest takes
const SLOW_DNS: Duration = Duration::from_millis(50);
/// Bodies this long and of a text type are expected to be compressed
const COMPRESSIBLE: u64 = 4 * 1024;
/// A body this long is worth a line on its own
const LARGE_BODY: u64 = 256 * 1024;
/// A tail this many times the median is a tail
const LONG_TAIL: f64 = 3.0;
/// A certificate with fewer days than this left is about to expire
const EXPIRING_DAYS: i64 = 30;

/// What is worth saying about `t`, most serious first
pub fn findings(t: &TargetStats, settings: &Settings, now: i64) -> Vec<Finding> {
    let mut out = Vec::new();
    let mut say = |level: Level, text: String| out.push(Finding { level, text });
    let whole = t.whole();
    let lat = |us: u64| format::latency(Duration::from_micros(us));

    // Answers, or the lack of them
    if whole.sent > 0 && whole.answered == 0 {
        let cause = t
            .causes
            .iter()
            .max_by_key(|(_, n)| **n)
            .map(|(c, _)| c.as_str())
            .unwrap_or("no answer");
        say(Level::Bad, format!("nothing answered: {cause}"));
        return out;
    }
    if whole.answered < whole.sent {
        let lost = whole.sent - whole.answered;
        let mut text = format!(
            "{lost} of {} pings got no answer ({})",
            whole.sent,
            crate::ping::percent(whole.loss())
        );
        if whole.timeouts > 0 {
            text.push_str(&format!(", {} timed out", whole.timeouts));
        }
        if let Some((cause, _)) = t.causes.iter().max_by_key(|(_, n)| **n) {
            text.push_str(&format!(": {cause}"));
        }
        say(
            if whole.loss() >= 0.05 {
                Level::Bad
            } else {
                Level::Warn
            },
            text,
        );
    }
    let last = t.recent.iter().rev().find(|s| s.answered);
    match &t.target.kind {
        Kind::Cmd(_) => {
            let failed: u64 = t
                .statuses
                .iter()
                .filter(|(c, _)| **c != 0)
                .map(|(_, n)| n)
                .sum();
            if failed > 0 {
                say(
                    Level::Bad,
                    format!("the command failed {failed} of {} times", whole.answered),
                );
            }
            tail(&whole, &mut say, &lat);
            return out;
        }
        Kind::Tcp { .. } => {
            if let Some(note) = last.and_then(|s| s.headers.iter().find(|(n, _)| n == "note")) {
                say(Level::Warn, note.1.clone());
            }
            tail(&whole, &mut say, &lat);
            return out;
        }
        Kind::Http { .. } => {}
    }

    // What it answered
    let failing: u64 = t.statuses.range(500..).map(|(_, n)| n).sum();
    if failing > 0 {
        say(
            if t.failing() >= 0.05 {
                Level::Bad
            } else {
                Level::Warn
            },
            format!("{failing} of {} answers were 5xx", whole.answered),
        );
    }
    if whole.answered > 0 {
        let all = |code: u16| t.statuses.get(&code).copied().unwrap_or(0) == whole.answered;
        if all(401) {
            say(
                Level::Warn,
                "401 on every ping: the request needs credentials (-H 'Authorization: …')".into(),
            );
        } else if all(403) {
            say(
                Level::Warn,
                "403 on every ping: the request is refused".into(),
            );
        } else if all(404) {
            say(
                Level::Warn,
                "404 on every ping: nothing is at this path".into(),
            );
        } else if all(429) {
            say(
                Level::Warn,
                "429 on every ping: the server is rate-limiting this client; ping it less often"
                    .into(),
            );
        }
    }
    let Some(last) = last else {
        return out;
    };
    let header = |name: &str| -> Option<&str> {
        last.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    };

    // Redirects
    let redirected = t
        .recent
        .iter()
        .filter(|s| s.answered)
        .all(|s| !s.hops.is_empty());
    if redirected && !last.hops.is_empty() {
        let cost: Duration = last.hops.iter().map(|h| h.took).sum();
        say(
            Level::Warn,
            format!(
                "every request is redirected ({}); point at {} to save {}",
                last.hops
                    .iter()
                    .map(|h| h.status.to_string())
                    .collect::<Vec<_>>()
                    .join(" → "),
                final_url(last).unwrap_or("the final URL"),
                format::latency(cost)
            ),
        );
    }

    // Where the time goes, by the medians
    let total = t.total.percentile(50.0).max(1);
    let median = |h: &crate::metrics::Histogram| (h.count() > 0).then(|| h.percentile(50.0));
    let share = |us: u64| us as f64 / total as f64;
    if let Some(dns) = median(&t.dns) {
        if dns >= SLOW_DNS.as_micros() as u64 || share(dns) >= SHARE_OF_NOTE {
            say(
                Level::Warn,
                format!(
                    "the DNS lookup takes {} of {}: the resolver is slow, or the name has a short TTL",
                    lat(dns),
                    lat(total)
                ),
            );
        }
    }
    if let Some(connect) = median(&t.connect) {
        if share(connect) >= SHARE_OF_NOTE {
            say(
                Level::Warn,
                format!(
                    "connecting takes {} of {}: a round trip of about {}; the server is far, and a kept connection would skip it",
                    lat(connect),
                    lat(total),
                    lat(connect)
                ),
            );
        }
    }
    let full = median(&t.tls_full);
    let resumed = median(&t.tls_resumed);
    match (full, resumed) {
        (Some(full), None) if t.tls_full.count() >= 3 => {
            let level = if share(full) >= SHARE_OF_NOTE {
                Level::Warn
            } else {
                Level::Note
            };
            say(
                level,
                format!(
                    "the TLS session is never resumed: every connection pays the full handshake, {} of {}",
                    lat(full),
                    lat(total)
                ),
            );
        }
        (Some(full), None) if share(full) >= SHARE_OF_NOTE => say(
            Level::Warn,
            format!("the TLS handshake is {} of {}", lat(full), lat(total)),
        ),
        (Some(full), Some(resumed)) => {
            let text = format!(
                "TLS sessions are resumed: {} after the first handshake's {}",
                lat(resumed),
                lat(full)
            );
            if share(resumed) >= SHARE_OF_NOTE {
                say(
                    Level::Warn,
                    format!("{text}, still {} of {}", lat(resumed), lat(total)),
                );
            } else {
                say(Level::Good, text);
            }
        }
        _ => {}
    }
    if header("connection").is_some_and(|v| v.eq_ignore_ascii_case("close")) && settings.keep_alive
    {
        say(
            Level::Warn,
            "the server closes the connection after each response (Connection: close), so every request connects again".into(),
        );
    }
    if let Some(ttfb) = median(&t.ttfb) {
        if share(ttfb) >= 0.5 {
            let mut text = format!(
                "the server takes {} of {} to start answering",
                lat(ttfb),
                lat(total)
            );
            if let Some(timing) = header("server-timing") {
                let said = crate::trace::server_timing(std::iter::once((
                    b"server-timing".as_slice(),
                    timing.as_bytes(),
                )));
                if let Some(segments) = said.filter(|s| !s.is_empty()) {
                    let parts: Vec<String> = segments
                        .iter()
                        .map(|s| match s.dur_ms {
                            Some(ms) => format!(
                                "{} {}",
                                s.name,
                                format::latency(Duration::from_secs_f64(ms / 1000.0))
                            ),
                            None => s.name.to_string(),
                        })
                        .collect();
                    text.push_str(&format!("; it says: {}", parts.join(", ")));
                }
            }
            say(Level::Note, text);
        }
    }
    let per_response = t.bytes.checked_div(whole.answered).unwrap_or(0);
    if let Some(download) = median(&t.download) {
        if share(download) >= SHARE_OF_NOTE || per_response >= LARGE_BODY {
            say(
                Level::Warn,
                format!(
                    "downloading {} takes {} of {}: the body, or the link",
                    format::bytes(per_response as f64),
                    lat(download),
                    lat(total)
                ),
            );
        }
    }

    // What the response says about itself
    let content_type = header("content-type").unwrap_or("").to_ascii_lowercase();
    let compressible = ["text/", "json", "javascript", "xml", "svg"]
        .iter()
        .any(|t| content_type.contains(t));
    if settings.compression
        && compressible
        && per_response >= COMPRESSIBLE
        && header("content-encoding").is_none()
    {
        say(
            Level::Warn,
            format!(
                "the {} body isn't compressed, though gzip and br were offered",
                format::bytes(per_response as f64)
            ),
        );
    }
    if let Some(status) = CacheStatus::parse_headers(
        last.headers
            .iter()
            .map(|(n, v)| (n.as_bytes(), v.as_bytes())),
    ) {
        let (name, value) = last
            .headers
            .iter()
            .find(|(n, _)| n.to_ascii_lowercase().contains("cache"))
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .unwrap_or(("cache", ""));
        match status {
            CacheStatus::Hit | CacheStatus::Revalidated | CacheStatus::Stale => say(
                Level::Note,
                format!("answered by a cache ({name}: {value}): the server itself wasn't measured"),
            ),
            _ => say(Level::Note, format!("not from a cache ({name}: {value})")),
        }
    }
    if let Some(tls) = &t.last_tls {
        if !settings.http1 && tls.alpn.as_deref() == Some("http/1.1") {
            say(
                Level::Note,
                "the server speaks HTTP/1.1 only: it didn't offer h2 when asked".into(),
            );
        }
    }
    if last.version.as_deref() == Some("HTTP/1.0") {
        say(
            Level::Warn,
            "the server answers in HTTP/1.0, which closes the connection after each response"
                .into(),
        );
    }

    // The certificate
    if let Some(cert) = t.last_tls.as_ref().and_then(|tls| tls.cert.as_ref()) {
        if !cert.valid_at(now) {
            say(
                Level::Bad,
                format!("the certificate isn't valid: it {}", cert.expiry(now)),
            );
        } else if cert.days_left(now) < EXPIRING_DAYS {
            say(Level::Warn, format!("the certificate {}", cert.expiry(now)));
        }
    }

    tail(&whole, &mut say, &lat);
    out.sort_by_key(|f| std::cmp::Reverse(f.level));
    out
}

/// A tail worth a line: the p99 against the median
fn tail(
    whole: &crate::ping::Summary,
    say: &mut impl FnMut(Level, String),
    lat: &impl Fn(u64) -> String,
) {
    if whole.answered >= 10 && whole.p50 > 0 && whole.p99 as f64 >= whole.p50 as f64 * LONG_TAIL {
        say(
            Level::Warn,
            format!(
                "a long tail: p99 {} against a median of {}; one ping in a hundred is {}× slower",
                lat(whole.p99),
                lat(whole.p50),
                (whole.p99 as f64 / whole.p50 as f64).round()
            ),
        );
    }
}

/// Where a redirected request ended up: the Location of its last hop,
/// if the last response's headers were kept; otherwise nothing
fn final_url(sample: &Sample) -> Option<&str> {
    sample.final_url.as_deref()
}

/// `pepe ping --once`: the last ping's phases, what answered, and the
/// findings, as a few lines
pub fn once(state: &crate::ping::State, settings: &Settings) -> String {
    use std::fmt::Write;
    let now = crate::logs::wall();
    let mut out = String::new();
    for t in &state.targets {
        let whole = t.whole();
        let _ = writeln!(out, "pepe ping · {} · {} pings", t.target.shown, whole.sent);
        if let Some(last) = t.recent.iter().rev().find(|s| s.answered) {
            let phases: Vec<String> = last
                .phases
                .each()
                .iter()
                .filter_map(|(name, took)| {
                    took.map(|d| format!("{} {}", phase_name(name), format::latency(d)))
                })
                .collect();
            if !phases.is_empty() {
                let _ = writeln!(
                    out,
                    "  {} · {} in all",
                    phases.join(" → "),
                    format::latency(last.total)
                );
            }
            let mut about: Vec<String> = Vec::new();
            match (&last.version, last.status) {
                (Some(version), Some(status)) => about.push(format!("{version} {status}")),
                (None, Some(status)) => about.push(status.to_string()),
                _ => {}
            }
            if let Some((_, ct)) = last
                .headers
                .iter()
                .find(|(n, _)| n.eq_ignore_ascii_case("content-type"))
            {
                about.push(ct.split(';').next().unwrap_or(ct).trim().to_string());
            }
            if last.bytes > 0 {
                about.push(format::bytes(last.bytes as f64));
            }
            if let Some(remote) = last.remote {
                about.push(match last.local {
                    Some(local) => format!("{remote} from {}", local.ip()),
                    None => remote.to_string(),
                });
            }
            if let Some(tls) = &last.tls {
                about.push(format!("{} {}", tls.version, tls.cipher));
            }
            if !about.is_empty() {
                let _ = writeln!(out, "  {}", about.join(" · "));
            }
            if let Some(cert) = last.tls.as_ref().and_then(|t| t.cert.as_ref()) {
                let _ = writeln!(
                    out,
                    "  certificate for {} by {}, {}",
                    cert.subject,
                    cert.issuer,
                    cert.expiry(now)
                );
            }
        }
        for finding in findings(t, settings, now) {
            let _ = writeln!(out, "  {} {}", finding.level.glyph(), finding.text);
        }
    }
    out
}

fn phase_name(name: &str) -> &str {
    match name {
        "ttfb" => "first byte",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cert::Cert;
    use crate::ping::{self, Phases, Target, Tls};
    use reqwest::Url;

    fn target() -> TargetStats {
        TargetStats::new(Target {
            name: "api".into(),
            shown: "https://api.test/".into(),
            kind: Kind::Http {
                url: Url::parse("https://api.test/").unwrap(),
                connect_to: None,
            },
        })
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
            slo: Default::default(),
            keep_body: 0,
            proxy: None,
            user_agent: "pepe/test".into(),
            headers: Default::default(),
            method: reqwest::Method::GET,
            body: Default::default(),
            count: None,
            duration: None,
            compression: true,
            http1: false,
        }
    }

    /// A ping that took `us` with the given phases and headers
    fn ping(seq: u64, phases: Phases, headers: &[(&str, &str)], tls: Option<Tls>) -> ping::Sample {
        let mut s = ping::Sample::new(0, seq, Duration::from_secs(seq));
        s.answered = true;
        s.status = Some(200);
        s.total = phases.each().iter().filter_map(|(_, d)| *d).sum();
        s.phases = phases;
        s.headers = headers
            .iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect();
        s.tls = tls;
        s.bytes = 20_000;
        s.version = Some("HTTP/1.1".into());
        s
    }

    fn ms(n: u64) -> Option<Duration> {
        Some(Duration::from_millis(n))
    }

    fn record(t: &mut TargetStats, s: ping::Sample) {
        t.sent += 1;
        t.in_flight += 1;
        t.record(s, None);
    }

    #[test]
    fn a_slow_handshake_never_resumed_is_said() {
        let mut t = target();
        let tls = Tls {
            version: "TLS 1.3".into(),
            cipher: "X".into(),
            alpn: None,
            resumed: false,
            cert: None,
        };
        for seq in 1..=5 {
            let phases = Phases {
                dns: ms(2),
                connect: ms(10),
                tls: ms(60),
                ttfb: ms(20),
                download: ms(1),
            };
            record(
                &mut t,
                ping(
                    seq,
                    phases,
                    &[("content-type", "text/html")],
                    Some(tls.clone()),
                ),
            );
        }
        let found = findings(&t, &settings(), 0);
        let texts: Vec<&str> = found.iter().map(|f| f.text.as_str()).collect();
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("the TLS session is never resumed")),
            "{texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("isn't compressed")),
            "{texts:?}"
        );
        assert!(found[0].level >= Level::Warn);
    }

    #[test]
    fn resumed_sessions_and_a_quiet_target_are_good_news() {
        let mut t = target();
        let full = Tls {
            version: "TLS 1.3".into(),
            cipher: "X".into(),
            alpn: None,
            resumed: false,
            cert: Some(Cert {
                subject: "api.test".into(),
                issuer: "Pepe".into(),
                not_before: 0,
                not_after: 86_400 * 100,
                names: vec![],
            }),
        };
        let resumed = Tls {
            resumed: true,
            ..full.clone()
        };
        for seq in 1..=12 {
            let phases = Phases {
                dns: ms(2),
                connect: ms(10),
                tls: if seq == 1 { ms(30) } else { ms(8) },
                ttfb: ms(40),
                download: ms(1),
            };
            let tls = if seq == 1 { &full } else { &resumed };
            record(
                &mut t,
                ping(
                    seq,
                    phases,
                    &[("content-type", "text/html"), ("content-encoding", "gzip")],
                    Some(tls.clone()),
                ),
            );
        }
        let found = findings(&t, &settings(), 0);
        let texts: Vec<String> = found
            .iter()
            .map(|f| format!("{} {}", f.level.glyph(), f.text))
            .collect();
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("✔ TLS sessions are resumed: 8.00ms after")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("· the server takes 40.00ms of")),
            "{texts:?}"
        );
        assert!(!texts.iter().any(|t| t.contains("compressed")), "{texts:?}");
        // A certificate with a hundred days left is nothing to say
        assert!(
            !texts.iter().any(|t| t.contains("certificate")),
            "{texts:?}"
        );
        let soon = findings(&t, &settings(), 86_400 * 95);
        assert!(soon
            .iter()
            .any(|f| f.text == "the certificate expires in 5 days"));
        let gone = findings(&t, &settings(), 86_400 * 101);
        assert_eq!(gone[0].level, Level::Bad);
    }

    #[test]
    fn losses_redirects_and_tails_are_said() {
        let mut t = target();
        for seq in 1..=20 {
            let mut s = ping(
                seq,
                Phases {
                    dns: ms(1),
                    connect: ms(5),
                    tls: None,
                    ttfb: ms(10),
                    download: ms(1),
                },
                &[],
                None,
            );
            s.hops.push(ping::Hop {
                url: "http://api.test/old".into(),
                status: 301,
                took: Duration::from_millis(9),
            });
            if seq == 20 {
                s.total = Duration::from_millis(200);
            }
            if seq == 7 {
                s.answered = false;
                s.status = None;
                s.timeout = true;
                s.error = Some("no answer in 5s".into());
            }
            record(&mut t, s);
        }
        let found = findings(&t, &settings(), 0);
        let texts: Vec<&str> = found.iter().map(|f| f.text.as_str()).collect();
        assert!(
            texts[0].starts_with("1 of 20 pings got no answer (5.0%), 1 timed out"),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("every request is redirected (301)")),
            "{texts:?}"
        );
        assert!(
            texts
                .iter()
                .any(|t| t.starts_with("a long tail: p99 200.0ms")),
            "{texts:?}"
        );
        let text = once(
            &ping::State {
                started: std::time::Instant::now(),
                targets: vec![t],
                done: true,
                paused: false,
                bells: 0,
            },
            &settings(),
        );
        assert!(
            text.contains("pepe ping · https://api.test/ · 20 pings"),
            "{text}"
        );
        assert!(
            text.contains("dns 1.00ms → connect 5.00ms → first byte 10.00ms → download 1.00ms"),
            "{text}"
        );
        assert!(text.contains("✖ 1 of 20 pings"), "{text}");
    }

    #[test]
    fn nothing_answering_is_the_only_finding() {
        let mut t = target();
        for seq in 1..=3 {
            let mut s = ping(seq, Phases::default(), &[], None);
            s.answered = false;
            s.status = None;
            s.error = Some("Connection refused".into());
            record(&mut t, s);
        }
        let found = findings(&t, &settings(), 0);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].text, "nothing answered: Connection refused");
        assert_eq!(found[0].level, Level::Bad);
    }
}
