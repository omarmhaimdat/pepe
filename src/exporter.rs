//! `--metrics :9100`: the live numbers at `/metrics`, in Prometheus's text
//! form, so a soak run or a long ping shows up in Grafana next to the
//! server's own. One page is rendered a second at most, whatever asks for
//! it; the scrape is served from the last one.
//!
//! A tiny HTTP/1.1 server of its own: it answers one path with one text,
//! which takes a few dozen lines and no new dependency.

use std::net::SocketAddr;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use crate::metrics::{Histogram, Metrics};
use crate::timeline::Timeline;

/// How often the page is rendered again, at most
const EVERY: Duration = Duration::from_secs(1);
/// A request head longer than this isn't a scrape
const MAX_HEAD: usize = 8 * 1024;
/// The histogram buckets, in seconds, as Prometheus wants them: cumulative
/// and fixed, so two runs' histograms add up
const BUCKETS: [f64; 14] = [
    0.001, 0.0025, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0,
];

struct Exporter {
    text: Mutex<String>,
    rendered: Mutex<Option<Instant>>,
    addr: SocketAddr,
}

static EXPORTER: OnceLock<Exporter> = OnceLock::new();

/// `:9100`, `9100`, `127.0.0.1:9100` or `[::]:9100`: where to listen
fn parse(spec: &str) -> Result<SocketAddr, String> {
    let spec = spec.trim();
    let text = if let Ok(port) = spec.parse::<u16>() {
        format!("0.0.0.0:{port}")
    } else if let Some(port) = spec.strip_prefix(':') {
        format!("0.0.0.0:{port}")
    } else {
        spec.to_string()
    };
    text.parse().map_err(|_| {
        format!("{spec:?} isn't an address to listen on, like :9100 or 127.0.0.1:9100")
    })
}

/// Start serving `/metrics` at `spec`; the address it listens on
pub async fn start(spec: &str) -> Result<SocketAddr, String> {
    let addr = parse(spec)?;
    let listener = TcpListener::bind(addr).await.map_err(|e| {
        format!(
            "can't listen on {addr}: {}",
            crate::response::root_cause(&e)
        )
    })?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let exporter = Exporter {
        text: Mutex::new(String::from("# pepe: nothing measured yet\n")),
        rendered: Mutex::new(None),
        addr,
    };
    if EXPORTER.set(exporter).is_err() {
        return Err("metrics are already being served".into());
    }
    tokio::spawn(async move {
        loop {
            let Ok((socket, _)) = listener.accept().await else {
                continue;
            };
            tokio::spawn(async move {
                let _ = tokio::time::timeout(Duration::from_secs(5), answer(socket)).await;
            });
        }
    });
    Ok(addr)
}

/// Where the page is served, when it is
pub fn address() -> Option<SocketAddr> {
    EXPORTER.get().map(|e| e.addr)
}

/// Render the page with `render` and serve it, unless one was rendered
/// less than a second ago; `render` isn't called then. Nothing happens
/// when `--metrics` wasn't given.
pub fn publish(render: impl FnOnce() -> String) {
    let Some(exporter) = EXPORTER.get() else {
        return;
    };
    {
        let mut rendered = exporter.rendered.lock().unwrap_or_else(|e| e.into_inner());
        if rendered.is_some_and(|at| at.elapsed() < EVERY) {
            return;
        }
        *rendered = Some(Instant::now());
    }
    let text = render();
    *exporter.text.lock().unwrap_or_else(|e| e.into_inner()) = text;
}

/// The page as last rendered
fn page() -> String {
    EXPORTER
        .get()
        .map(|e| e.text.lock().unwrap_or_else(|p| p.into_inner()).clone())
        .unwrap_or_default()
}

/// One request on one connection: the page, a pointer to it, or a 404
async fn answer(mut socket: tokio::net::TcpStream) {
    let mut head = Vec::with_capacity(1024);
    let mut buf = [0u8; 1024];
    loop {
        let Ok(n) = socket.read(&mut buf).await else {
            return;
        };
        if n == 0 {
            return;
        }
        head.extend_from_slice(&buf[..n]);
        if head.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
        if head.len() > MAX_HEAD {
            return;
        }
    }
    let line = String::from_utf8_lossy(&head);
    let path = line
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .unwrap_or("/");
    let path = path.split('?').next().unwrap_or(path);
    let (status, body) = match path {
        "/metrics" => ("200 OK", page()),
        "/" => ("200 OK", "pepe: the metrics are at /metrics\n".to_string()),
        _ => (
            "404 Not Found",
            "not here; the metrics are at /metrics\n".to_string(),
        ),
    };
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/plain; version=0.0.4; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = socket.write_all(response.as_bytes()).await;
    let _ = socket.shutdown().await;
}

// ─── The page ────────────────────────────────────────────────────────────────

/// A page being written: each metric's lines under its HELP and TYPE
#[derive(Default)]
pub struct Page {
    out: String,
}

impl Page {
    fn family(&mut self, name: &str, kind: &str, help: &str) {
        self.out
            .push_str(&format!("# HELP {name} {help}\n# TYPE {name} {kind}\n"));
    }

    fn line(&mut self, name: &str, labels: &[(&str, &str)], value: f64) {
        self.out.push_str(name);
        if !labels.is_empty() {
            self.out.push('{');
            for (i, (k, v)) in labels.iter().enumerate() {
                if i > 0 {
                    self.out.push(',');
                }
                self.out.push_str(&format!("{k}=\"{}\"", escape(v)));
            }
            self.out.push('}');
        }
        self.out.push_str(&format!(" {}\n", number(value)));
    }

    /// One family with one line per entry
    pub fn gauge(&mut self, name: &str, help: &str, lines: &[(&[(&str, &str)], f64)]) {
        self.family(name, "gauge", help);
        for (labels, value) in lines {
            self.line(name, labels, *value);
        }
    }

    pub fn counter(&mut self, name: &str, help: &str, lines: &[(&[(&str, &str)], f64)]) {
        self.family(name, "counter", help);
        for (labels, value) in lines {
            self.line(name, labels, *value);
        }
    }

    /// A histogram of microseconds, in seconds, over the fixed buckets
    pub fn histogram(&mut self, name: &str, help: &str, lines: &[(&[(&str, &str)], &Histogram)]) {
        self.family(name, "histogram", help);
        for (labels, hist) in lines {
            let mut buckets: Vec<(u64, u64)> = hist.buckets().collect();
            buckets.sort_unstable();
            let mut at = 0;
            let mut seen = 0u64;
            let mut sum_us = 0.0;
            for (us, n) in &buckets {
                sum_us += *us as f64 * *n as f64;
            }
            for le in BUCKETS {
                let limit = (le * 1e6) as u64;
                while at < buckets.len() && buckets[at].0 <= limit {
                    seen += buckets[at].1;
                    at += 1;
                }
                let mut with: Vec<(&str, &str)> = labels.to_vec();
                let le_text = number(le);
                with.push(("le", &le_text));
                self.line(&format!("{name}_bucket"), &with, seen as f64);
            }
            let mut with: Vec<(&str, &str)> = labels.to_vec();
            with.push(("le", "+Inf"));
            self.line(&format!("{name}_bucket"), &with, hist.count() as f64);
            self.line(&format!("{name}_sum"), labels, sum_us / 1e6);
            self.line(&format!("{name}_count"), labels, hist.count() as f64);
        }
    }

    pub fn finish(self) -> String {
        self.out
    }
}

fn escape(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

/// Numbers as Prometheus reads them: integers whole, the rest short
fn number(v: f64) -> String {
    if v.is_nan() {
        "NaN".into()
    } else if v.is_infinite() {
        if v > 0.0 { "+Inf" } else { "-Inf" }.into()
    } else if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        let text = format!("{v:.6}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// One set of request metrics under its labels
fn requests(page: &mut Page, rows: &[(&[(&str, &str)], &Metrics)]) {
    let owned = |rows: &[(&[(&str, &str)], &Metrics)],
                 pick: &dyn Fn(&Metrics) -> f64|
     -> Vec<(Vec<(String, String)>, f64)> {
        rows.iter()
            .map(|(labels, m)| {
                (
                    labels
                        .iter()
                        .map(|(k, v)| (k.to_string(), v.to_string()))
                        .collect(),
                    pick(m),
                )
            })
            .collect()
    };
    let emit = |page: &mut Page,
                kind: &str,
                name: &str,
                help: &str,
                lines: Vec<(Vec<(String, String)>, f64)>| {
        page.family(name, kind, help);
        for (labels, value) in &lines {
            let borrowed: Vec<(&str, &str)> = labels
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            page.line(name, &borrowed, *value);
        }
    };
    emit(
        page,
        "counter",
        "pepe_requests_total",
        "Requests finished, whatever the outcome",
        owned(rows, &|m| m.total as f64),
    );
    emit(
        page,
        "counter",
        "pepe_requests_succeeded_total",
        "Requests answered with a 2xx",
        owned(rows, &|m| m.success as f64),
    );
    emit(
        page,
        "counter",
        "pepe_requests_timed_out_total",
        "Requests that hit the timeout",
        owned(rows, &|m| m.timeouts as f64),
    );
    emit(
        page,
        "counter",
        "pepe_requests_errored_total",
        "Requests that failed without a response",
        owned(rows, &|m| m.errors as f64),
    );
    emit(
        page,
        "counter",
        "pepe_response_bytes_total",
        "Body bytes received",
        owned(rows, &|m| m.bytes as f64),
    );
    emit(
        page,
        "counter",
        "pepe_cache_hits_total",
        "Responses a cache answered, by their headers",
        owned(rows, &|m| m.cache_hits as f64),
    );

    // Status codes and failure causes, one line each
    page.family(
        "pepe_responses_total",
        "counter",
        "Responses, by status code",
    );
    for (labels, m) in rows {
        let mut codes: Vec<(&u16, &u64)> = m.status_codes.iter().collect();
        codes.sort_unstable();
        for (code, n) in codes {
            let code = code.to_string();
            let mut with: Vec<(&str, &str)> = labels.to_vec();
            with.push(("status", &code));
            page.line("pepe_responses_total", &with, *n as f64);
        }
    }
    page.family(
        "pepe_failures_total",
        "counter",
        "Failed requests, by cause",
    );
    for (labels, m) in rows {
        for (cause, c) in m.failures().top() {
            let mut with: Vec<(&str, &str)> = labels.to_vec();
            with.push(("cause", cause));
            page.line("pepe_failures_total", &with, c.count as f64);
        }
    }

    // The latency: its percentiles, and the whole distribution
    page.family(
        "pepe_latency_seconds",
        "gauge",
        "Latency so far, at a quantile",
    );
    for (labels, m) in rows {
        for (q, name) in [(50.0, "0.5"), (90.0, "0.9"), (95.0, "0.95"), (99.0, "0.99")] {
            let mut with: Vec<(&str, &str)> = labels.to_vec();
            with.push(("quantile", name));
            page.line("pepe_latency_seconds", &with, m.percentile(q).as_secs_f64());
        }
    }
    page.family(
        "pepe_first_byte_seconds",
        "gauge",
        "Time to the response headers, at a quantile",
    );
    for (labels, m) in rows {
        let h = m.phase(crate::metrics::Phase::FirstByte);
        if h.count() == 0 {
            continue;
        }
        for (q, name) in [(50.0, "0.5"), (99.0, "0.99")] {
            let mut with: Vec<(&str, &str)> = labels.to_vec();
            with.push(("quantile", name));
            page.line(
                "pepe_first_byte_seconds",
                &with,
                h.percentile(q) as f64 / 1e6,
            );
        }
    }
    let hists: Vec<(&[(&str, &str)], &Histogram)> =
        rows.iter().map(|(l, m)| (*l, m.latency())).collect();
    page.histogram(
        "pepe_request_duration_seconds",
        "Latency of every request",
        &hists,
    );
}

/// The page of a load run: the whole run, and each of its rows (an API's
/// endpoints, a flow's steps, a replay's URLs) when it has them
pub fn run_page(
    target: &str,
    metrics: &Metrics,
    timeline: &Timeline,
    elapsed: Duration,
    concurrency: usize,
    rows: &[(String, &Metrics)],
) -> String {
    let mut page = Page::default();
    let target_label: [(&str, &str); 1] = [("target", target)];
    page.gauge(
        "pepe_run_elapsed_seconds",
        "How long the run has been going, pauses left out",
        &[(&target_label, elapsed.as_secs_f64())],
    );
    page.gauge(
        "pepe_concurrency",
        "Requests in flight at once",
        &[(&target_label, concurrency as f64)],
    );
    let last = timeline.last().copied().unwrap_or_default();
    page.gauge(
        "pepe_requests_per_second",
        "Throughput over the last second",
        &[(&target_label, last.rps)],
    );
    page.gauge(
        "pepe_errors_per_second",
        "Failed and non-2xx requests over the last second",
        &[(&target_label, last.errors)],
    );
    let mut all: Vec<(Vec<(&str, &str)>, &Metrics)> = vec![(vec![("target", target)], metrics)];
    for (row, m) in rows {
        all.push((vec![("target", target), ("row", row.as_str())], m));
    }
    let borrowed: Vec<(&[(&str, &str)], &Metrics)> =
        all.iter().map(|(l, m)| (l.as_slice(), *m)).collect();
    requests(&mut page, &borrowed);
    page.finish()
}

/// The page of a ping: each target's counts, latency and phases
pub fn ping_page(state: &crate::ping::State) -> String {
    let mut page = Page::default();
    let names: Vec<String> = state
        .targets
        .iter()
        .map(|t| t.target.name.clone())
        .collect();
    let label = |i: usize| -> [(&str, &str); 1] { [("target", names[i].as_str())] };
    let labels: Vec<[(&str, &str); 1]> = (0..names.len()).map(label).collect();
    let each = |pick: &dyn Fn(&crate::ping::TargetStats) -> f64| -> Vec<(&[(&str, &str)], f64)> {
        state
            .targets
            .iter()
            .enumerate()
            .map(|(i, t)| (labels[i].as_slice(), pick(t)))
            .collect()
    };
    page.gauge(
        "pepe_ping_elapsed_seconds",
        "How long the pings have been going",
        &[(&[], state.elapsed().as_secs_f64())],
    );
    page.counter(
        "pepe_ping_sent_total",
        "Pings sent",
        &each(&|t| t.sent as f64),
    );
    page.counter(
        "pepe_ping_answered_total",
        "Pings answered",
        &each(&|t| t.answered as f64),
    );
    page.counter(
        "pepe_ping_lost_total",
        "Pings that got no answer",
        &each(&|t| (t.sent - t.answered) as f64),
    );
    page.counter(
        "pepe_ping_timed_out_total",
        "Pings that hit the timeout",
        &each(&|t| t.timeouts as f64),
    );
    page.counter(
        "pepe_ping_slo_broken_total",
        "Pings over an --slo limit",
        &each(&|t| t.violations as f64),
    );
    page.counter(
        "pepe_ping_bytes_total",
        "Body bytes received",
        &each(&|t| t.bytes as f64),
    );
    page.counter(
        "pepe_ping_tls_resumed_total",
        "Connections whose TLS session was resumed",
        &each(&|t| t.tls_resumed.count() as f64),
    );
    page.gauge(
        "pepe_ping_up",
        "1 when the last ping was answered",
        &each(&|t| {
            t.recent
                .back()
                .map_or(0.0, |s| f64::from(u8::from(s.answered)))
        }),
    );
    page.gauge(
        "pepe_ping_last_seconds",
        "The last ping's time",
        &each(&|t| {
            t.recent
                .back()
                .filter(|s| s.answered)
                .map_or(f64::NAN, |s| s.total.as_secs_f64())
        }),
    );
    page.gauge(
        "pepe_ping_loss_ratio",
        "Pings lost over pings sent, 0 to 1",
        &each(&|t| t.whole().loss()),
    );
    page.family(
        "pepe_ping_latency_seconds",
        "gauge",
        "Ping time so far, at a quantile",
    );
    for (i, t) in state.targets.iter().enumerate() {
        let s = t.whole();
        for (value, name) in [(s.p50, "0.5"), (s.p95, "0.95"), (s.p99, "0.99")] {
            page.line(
                "pepe_ping_latency_seconds",
                &[("target", &names[i]), ("quantile", name)],
                value as f64 / 1e6,
            );
        }
    }
    page.gauge(
        "pepe_ping_jitter_seconds",
        "Mean difference between one ping and the next",
        &each(&|t| t.whole().jitter as f64 / 1e6),
    );
    page.family(
        "pepe_ping_phase_seconds",
        "gauge",
        "Median of each phase of a ping",
    );
    for (i, t) in state.targets.iter().enumerate() {
        for (phase, h) in [
            ("dns", &t.dns),
            ("connect", &t.connect),
            ("tls", &t.tls_full),
            ("tls_resumed", &t.tls_resumed),
            ("ttfb", &t.ttfb),
            ("download", &t.download),
        ] {
            if h.count() > 0 {
                page.line(
                    "pepe_ping_phase_seconds",
                    &[("target", &names[i]), ("phase", phase)],
                    h.percentile(50.0) as f64 / 1e6,
                );
            }
        }
    }
    page.family(
        "pepe_ping_responses_total",
        "counter",
        "Answers, by status code",
    );
    for (i, t) in state.targets.iter().enumerate() {
        for (code, n) in &t.statuses {
            page.line(
                "pepe_ping_responses_total",
                &[("target", &names[i]), ("status", &code.to_string())],
                *n as f64,
            );
        }
    }
    page.family(
        "pepe_ping_cert_not_after_seconds",
        "gauge",
        "When the certificate expires, as seconds since the epoch",
    );
    for (i, t) in state.targets.iter().enumerate() {
        if let Some(cert) = t.last_tls.as_ref().and_then(|tls| tls.cert.as_ref()) {
            page.line(
                "pepe_ping_cert_not_after_seconds",
                &labels[i],
                cert.not_after as f64,
            );
        }
    }
    let hists: Vec<(&[(&str, &str)], &Histogram)> = state
        .targets
        .iter()
        .enumerate()
        .map(|(i, t)| (labels[i].as_slice(), &t.total))
        .collect();
    page.histogram(
        "pepe_ping_duration_seconds",
        "Time of every answered ping",
        &hists,
    );
    page.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_are_read_in_their_short_forms() {
        assert_eq!(parse(":9100").unwrap().to_string(), "0.0.0.0:9100");
        assert_eq!(parse("9100").unwrap().to_string(), "0.0.0.0:9100");
        assert_eq!(
            parse("127.0.0.1:9100").unwrap().to_string(),
            "127.0.0.1:9100"
        );
        assert_eq!(parse("[::1]:9100").unwrap().to_string(), "[::1]:9100");
        assert!(parse("nine").is_err());
    }

    #[test]
    fn a_page_has_its_families_and_cumulative_buckets() {
        let mut hist = Histogram::default();
        for us in [500, 2_000, 2_000, 40_000, 3_000_000] {
            hist.record(us);
        }
        let mut page = Page::default();
        page.gauge("x_up", "Is it up", &[(&[("target", "a\"b")], 1.0)]);
        page.histogram("x_seconds", "How long", &[(&[("target", "a")], &hist)]);
        let text = page.finish();
        assert!(
            text.contains("# HELP x_up Is it up\n# TYPE x_up gauge\nx_up{target=\"a\\\"b\"} 1\n"),
            "{text}"
        );
        assert!(
            text.contains("x_seconds_bucket{target=\"a\",le=\"0.001\"} 1\n"),
            "{text}"
        );
        assert!(
            text.contains("x_seconds_bucket{target=\"a\",le=\"0.0025\"} 3\n"),
            "{text}"
        );
        assert!(
            text.contains("x_seconds_bucket{target=\"a\",le=\"0.05\"} 4\n"),
            "{text}"
        );
        assert!(
            text.contains("x_seconds_bucket{target=\"a\",le=\"2.5\"} 4\n"),
            "{text}"
        );
        assert!(
            text.contains("x_seconds_bucket{target=\"a\",le=\"+Inf\"} 5\n"),
            "{text}"
        );
        assert!(text.contains("x_seconds_count{target=\"a\"} 5\n"), "{text}");
        // The sum is of the buckets' representative values, near enough
        let sum: f64 = text
            .lines()
            .find(|l| l.starts_with("x_seconds_sum"))
            .and_then(|l| l.split(' ').nth(1))
            .and_then(|v| v.parse().ok())
            .unwrap();
        assert!((sum - 3.0445).abs() < 0.05, "{sum}");
        assert_eq!(number(0.25), "0.25");
        assert_eq!(number(3.0), "3");
        assert_eq!(number(f64::NAN), "NaN");
    }

    #[test]
    fn a_run_page_names_the_target_and_its_rows() {
        let mut m = Metrics::default();
        let mut stat = crate::response::ResponseStats {
            duration: Duration::from_millis(20),
            status_code: Some(reqwest::StatusCode::OK),
            ttfb: Some(Duration::from_millis(15)),
            body_bytes: 100,
            ..Default::default()
        };
        m.record(&stat);
        stat.status_code = Some(reqwest::StatusCode::SERVICE_UNAVAILABLE);
        m.record(&stat);
        let timeline = Timeline::default();
        let rows = vec![("GET /a".to_string(), &m)];
        let text = run_page(
            "GET http://x/",
            &m,
            &timeline,
            Duration::from_secs(3),
            8,
            &rows,
        );
        assert!(
            text.contains("pepe_run_elapsed_seconds{target=\"GET http://x/\"} 3\n"),
            "{text}"
        );
        assert!(
            text.contains("pepe_requests_total{target=\"GET http://x/\"} 2\n"),
            "{text}"
        );
        assert!(
            text.contains("pepe_requests_total{target=\"GET http://x/\",row=\"GET /a\"} 2\n"),
            "{text}"
        );
        assert!(
            text.contains("pepe_responses_total{target=\"GET http://x/\",status=\"503\"} 1\n"),
            "{text}"
        );
        assert!(
            text.contains("pepe_failures_total{target=\"GET http://x/\",cause=\"HTTP 503\"} 1\n"),
            "{text}"
        );
        assert!(
            text.contains("pepe_latency_seconds{target=\"GET http://x/\",quantile=\"0.5\"} 0.02\n"),
            "{text}"
        );
        assert!(
            text.contains(
                "pepe_first_byte_seconds{target=\"GET http://x/\",quantile=\"0.5\"} 0.015\n"
            ),
            "{text}"
        );
        assert!(
            text.contains("pepe_request_duration_seconds_count{target=\"GET http://x/\"} 2\n"),
            "{text}"
        );
        // Each family is declared once
        assert_eq!(text.matches("# TYPE pepe_requests_total ").count(), 1);
    }

    #[tokio::test]
    async fn the_page_is_served_at_metrics() {
        let addr = start("127.0.0.1:0").await.unwrap();
        assert_eq!(address(), Some(addr));
        publish(|| "pepe_test 1\n".to_string());
        // A second render within the second is skipped
        publish(|| panic!("rendered twice in a second"));
        let fetch = |path: &'static str| async move {
            let mut socket = tokio::net::TcpStream::connect(addr).await.unwrap();
            socket
                .write_all(format!("GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").as_bytes())
                .await
                .unwrap();
            let mut out = Vec::new();
            socket.read_to_end(&mut out).await.unwrap();
            String::from_utf8(out).unwrap()
        };
        let page = fetch("/metrics").await;
        assert!(page.starts_with("HTTP/1.1 200 OK\r\n"), "{page}");
        assert!(
            page.contains("Content-Type: text/plain; version=0.0.4"),
            "{page}"
        );
        assert!(page.ends_with("\r\n\r\npepe_test 1\n"), "{page}");
        assert!(fetch("/").await.contains("the metrics are at /metrics"));
        assert!(fetch("/nope").await.starts_with("HTTP/1.1 404"));
    }
}
