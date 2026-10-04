//! Replay: read an access log and send its URLs in their real proportions.
//! nginx and Apache (common and combined), Caddy's JSON lines, AWS ALB
//! logs and plain lists of URLs or paths are read; what a line doesn't
//! say is left out and counted.

use std::collections::HashMap;
use std::path::Path;

use crate::cli::Cli;
use crate::load::Target;
use crate::ui::EndpointView;
use crate::PepeError;

/// Distinct URLs kept; the long tail past this is dropped and counted
pub const MAX_URLS: usize = 5_000;
/// Methods that don't change anything; the rest need `--include-writes`
const READS: [&str; 3] = ["GET", "HEAD", "OPTIONS"];

/// What a log adds up to
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Replay {
    /// (method, URL, times seen), most seen first
    pub urls: Vec<Entry>,
    /// Lines that said a request
    pub requests: u64,
    /// Lines nothing could be read from
    pub unparsed: u64,
    /// Requests left out for writing (POST, PUT, …)
    pub writes: u64,
    /// Requests left out for having a path and no host
    pub no_host: u64,
    /// Distinct URLs past `MAX_URLS`, left out
    pub tail: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Entry {
    pub method: String,
    pub url: String,
    pub count: u64,
}

/// One line's request, as far as the line said
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub method: String,
    /// A full URL, or a path when the line had no host
    pub target: String,
}

pub fn load(path: &Path, base_url: Option<&str>, include_writes: bool) -> Result<Replay, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let replay = parse(&text, base_url, include_writes);
    if replay.urls.is_empty() {
        let why = if replay.requests == 0 {
            "no request could be read from it: nginx, Apache, Caddy JSON and ALB logs are understood, or one URL or path per line".to_string()
        } else if replay.no_host > 0 {
            format!(
                "its {} requests have paths but no host: say where to send them with --base-url",
                replay.no_host
            )
        } else {
            format!(
                "its {} requests are all writes (POST, PUT, …), left out unless --include-writes is given",
                replay.writes
            )
        };
        return Err(format!("{}: {why}", path.display()));
    }
    Ok(replay)
}

/// Count every line's request. `base_url` is put in front of paths that
/// have no host, and replaces the host of those that do.
pub fn parse(text: &str, base_url: Option<&str>, include_writes: bool) -> Replay {
    let base = base_url.map(|b| b.trim_end_matches('/'));
    let mut counts: HashMap<(String, String), u64> = HashMap::new();
    let mut replay = Replay::default();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some(hit) = parse_line(line) else {
            replay.unparsed += 1;
            continue;
        };
        replay.requests += 1;
        if !include_writes && !READS.contains(&hit.method.as_str()) {
            replay.writes += 1;
            continue;
        }
        let url = match (hit.target.starts_with('/'), base) {
            (true, Some(base)) => format!("{base}{}", hit.target),
            (true, None) => {
                replay.no_host += 1;
                continue;
            }
            (false, Some(base)) => match hit
                .target
                .find("://")
                .and_then(|i| hit.target[i + 3..].find('/'))
            {
                Some(slash) => {
                    let after_scheme = hit.target.find("://").map_or(0, |i| i + 3);
                    format!("{base}{}", &hit.target[after_scheme + slash..])
                }
                None => format!("{base}/"),
            },
            (false, None) => hit.target,
        };
        *counts.entry((hit.method, url)).or_insert(0) += 1;
    }
    let mut urls: Vec<Entry> = counts
        .into_iter()
        .map(|((method, url), count)| Entry { method, url, count })
        .collect();
    // Most seen first; ties by URL, so the order is the same every time
    urls.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(a.url.cmp(&b.url))
            .then(a.method.cmp(&b.method))
    });
    if urls.len() > MAX_URLS {
        replay.tail = urls.len() as u64 - MAX_URLS as u64;
        urls.truncate(MAX_URLS);
    }
    replay.urls = urls;
    replay
}

/// The request a log line records, in any of the formats understood
pub fn parse_line(line: &str) -> Option<Hit> {
    if line.starts_with('{') {
        return parse_json(line);
    }
    // nginx, Apache and ALB: the request is quoted, `"GET /path HTTP/1.1"`
    if let Some(quoted) = QUOTED.get_or_init(quoted_request).captures(line) {
        return Some(Hit {
            method: quoted[1].to_uppercase(),
            target: quoted[2].to_string(),
        });
    }
    // A plain list: `/path`, `GET /path` or a URL
    let mut words = line.split_whitespace();
    let first = words.next()?;
    let (method, target) = if is_method(first) {
        (first.to_uppercase(), words.next()?)
    } else {
        ("GET".to_string(), first)
    };
    (target.starts_with('/') || target.contains("://")).then(|| Hit {
        method,
        target: target.to_string(),
    })
}

static QUOTED: std::sync::OnceLock<regex_lite::Regex> = std::sync::OnceLock::new();

fn quoted_request() -> regex_lite::Regex {
    regex_lite::Regex::new(r#""([A-Za-z]+) (\S+) HTTP/[0-9.]+""#).expect("a fixed pattern")
}

fn is_method(word: &str) -> bool {
    matches!(
        word.to_ascii_uppercase().as_str(),
        "GET" | "HEAD" | "POST" | "PUT" | "PATCH" | "DELETE" | "OPTIONS" | "TRACE" | "CONNECT"
    )
}

/// Caddy: `{"request":{"method":"GET","host":"h","uri":"/p"},…}`; also
/// flat `{"method":…,"url":…}` or `{"method":…,"path":…}` lines
fn parse_json(line: &str) -> Option<Hit> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let request = value.get("request").unwrap_or(&value);
    let method = request
        .get("method")
        .and_then(|m| m.as_str())
        .unwrap_or("GET")
        .to_uppercase();
    let text = |key: &str| {
        request
            .get(key)
            .and_then(|v| v.as_str())
            .map(str::to_string)
    };
    let target = text("url")
        .or_else(|| text("uri"))
        .or_else(|| text("path"))?;
    let target = match (target.starts_with('/'), text("host")) {
        (true, Some(host)) => {
            let scheme = request
                .get("proto")
                .and_then(|p| p.as_str())
                .map_or("http", |_| "http");
            let tls = value.get("tls").is_some() || request.get("tls").is_some();
            format!("{}://{host}{target}", if tls { "https" } else { scheme })
        }
        _ => target,
    };
    Some(Hit { method, target })
}

impl Replay {
    /// What the run sends: one target per URL, weighted by how often it
    /// was seen; results count toward the top `rows` URLs' rows, the rest
    /// toward one shared "other" row
    pub fn targets(&self, cli: &Cli, rows: usize) -> Result<Vec<Target>, PepeError> {
        let mut targets = Vec::with_capacity(self.urls.len());
        for (rank, entry) in self.urls.iter().enumerate() {
            targets.push(Target {
                request: crate::request::Request::new(
                    entry.url.clone(),
                    entry.method.clone(),
                    None,
                    &[],
                    cli.settings(),
                )?,
                headers: Default::default(),
                endpoint: rank.min(rows) as u16,
                weight: entry.count.min(u32::MAX as u64) as u32,
            });
        }
        Ok(targets)
    }

    /// The dashboard's rows: the top URLs, then the rest as one
    pub fn views(&self, rows: usize) -> Vec<EndpointView> {
        let total: u64 = self.urls.iter().map(|e| e.count).sum::<u64>().max(1);
        let mut views: Vec<EndpointView> = self
            .urls
            .iter()
            .take(rows)
            .map(|e| EndpointView {
                label: format!(
                    "{:>4.1}%  {}",
                    e.count as f64 / total as f64 * 100.0,
                    path_of(&e.url)
                ),
                method: e.method.clone(),
                url: e.url.clone(),
                variants: 1,
                headers: Vec::new(),
                body: None,
            })
            .collect();
        if self.urls.len() > rows {
            let rest = &self.urls[rows..];
            let share: u64 = rest.iter().map(|e| e.count).sum();
            views.push(EndpointView {
                label: format!(
                    "{:>4.1}%  {} other URLs",
                    share as f64 / total as f64 * 100.0,
                    crate::ui::format::count(rest.len() as u64)
                ),
                method: "…".into(),
                url: rest[0].url.clone(),
                variants: rest.len(),
                headers: Vec::new(),
                body: None,
            });
        }
        views
    }

    /// Requests the log had for the URLs kept
    pub fn kept(&self) -> u64 {
        self.urls.iter().map(|e| e.count).sum()
    }
}

/// The path and query of a URL, for a row that has no room for the host
fn path_of(url: &str) -> &str {
    url.find("://")
        .and_then(|i| url[i + 3..].find('/').map(|j| &url[i + 3 + j..]))
        .unwrap_or(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn every_format_gives_up_its_request() {
        let hit = |line: &str| parse_line(line).unwrap();
        // nginx / Apache combined
        let h = hit(
            r#"203.0.113.9 - - [10/Oct/2026:13:55:36 +0000] "GET /index.html?x=1 HTTP/1.1" 200 2326 "-" "Mozilla/5.0""#,
        );
        assert_eq!(
            (h.method.as_str(), h.target.as_str()),
            ("GET", "/index.html?x=1")
        );
        // ALB: the request has the full URL
        let h = hit(
            r#"https 2026-10-10T13:55:36.123456Z app/my-alb/50dc6c495c0c9188 203.0.113.9:2817 10.0.0.1:80 0.000 0.001 0.000 200 200 34 366 "POST https://www.example.com:443/api/items HTTP/1.1" "curl/7.46.0" ECDHE-RSA-AES128-GCM-SHA256 TLSv1.2 arn:aws:elasticloadbalancing:us-east-1:1:targetgroup/t/1 "Root=1-58337262-36d228ad5d99923122bbe354" "-" "-" 0 2026-10-10T13:55:36.120000Z "forward" "-" "-" "10.0.0.1:80" "200" "-" "-""#,
        );
        assert_eq!(
            (h.method.as_str(), h.target.as_str()),
            ("POST", "https://www.example.com:443/api/items")
        );
        // Caddy JSON
        let h = hit(
            r#"{"level":"info","ts":1,"logger":"http.log.access","msg":"handled request","request":{"remote_ip":"1.2.3.4","proto":"HTTP/2.0","method":"GET","host":"example.com","uri":"/blog/?page=2","tls":{"version":772}},"status":200}"#,
        );
        assert_eq!(
            (h.method.as_str(), h.target.as_str()),
            ("GET", "https://example.com/blog/?page=2")
        );
        // Plain lists
        assert_eq!(hit("/health").target, "/health");
        assert_eq!(hit("post /items").method, "POST");
        assert_eq!(hit("https://a.test/x").target, "https://a.test/x");
        assert_eq!(parse_line("just words here"), None);
        assert_eq!(parse_line("{\"nothing\":1}"), None);
    }

    #[test]
    fn urls_are_counted_in_proportion_with_the_rest_said() {
        let log = "\
            # a comment\n\
            1.1.1.1 - - [d] \"GET /a HTTP/1.1\" 200 1\n\
            1.1.1.1 - - [d] \"GET /a HTTP/1.1\" 200 1\n\
            1.1.1.1 - - [d] \"GET /a HTTP/1.1\" 200 1\n\
            1.1.1.1 - - [d] \"GET /b HTTP/1.1\" 200 1\n\
            1.1.1.1 - - [d] \"POST /b HTTP/1.1\" 201 1\n\
            1.1.1.1 - - [d] \"HEAD /c HTTP/1.1\" 200 1\n\
            garbage\n";
        let r = parse(log, Some("https://staging.test/"), false);
        assert_eq!(
            (r.requests, r.unparsed, r.writes, r.no_host, r.tail),
            (6, 1, 1, 0, 0)
        );
        let seen: Vec<(&str, &str, u64)> = r
            .urls
            .iter()
            .map(|e| (e.method.as_str(), e.url.as_str(), e.count))
            .collect();
        assert_eq!(
            seen,
            [
                ("GET", "https://staging.test/a", 3),
                ("GET", "https://staging.test/b", 1),
                ("HEAD", "https://staging.test/c", 1)
            ]
        );
        assert_eq!(r.kept(), 5);
        // Writes come along when asked, and paths without a base are left out
        assert_eq!(parse(log, Some("https://s"), true).urls.len(), 4);
        let r = parse(log, None, false);
        assert_eq!((r.urls.len(), r.no_host), (0, 5));
        // A base replaces the host a line had
        let r = parse(
            "\"GET https://prod.test/x?y=1 HTTP/1.1\"\n",
            Some("http://localhost:8080"),
            false,
        );
        assert_eq!(r.urls[0].url, "http://localhost:8080/x?y=1");
        let r = parse(
            "\"GET https://prod.test HTTP/1.1\"\n",
            Some("http://l"),
            false,
        );
        assert_eq!(r.urls[0].url, "http://l/");
    }

    #[test]
    fn the_dashboard_gets_the_top_rows_and_one_for_the_rest() {
        let log: String = (0..30)
            .flat_map(|i| std::iter::repeat_n(format!("/p{i}\n"), 30 - i))
            .collect();
        let r = parse(&log, Some("http://h"), false);
        let views = r.views(3);
        assert_eq!(views.len(), 4);
        assert!(
            views[0].label.ends_with("/p0") && views[0].label.starts_with(" 6.5%"),
            "{}",
            views[0].label
        );
        assert!(
            views[3].label.contains("27 other URLs"),
            "{}",
            views[3].label
        );
        assert_eq!(views[3].variants, 27);
        let cli = Cli::parse_from(["pepe", "x"]);
        let targets = r.targets(&cli, 3).unwrap();
        assert_eq!(targets.len(), 30);
        assert_eq!((targets[0].weight, targets[0].endpoint), (30, 0));
        assert_eq!(
            (targets[29].weight, targets[29].endpoint),
            (1, 3),
            "the tail shares a row"
        );
    }
}
