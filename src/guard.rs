//! Guardrails: where a run may be pointed and how much it may send, checked
//! before anything is sent; and `--dry-run`, which says what would be sent
//! and sends nothing. For a script or an agent that could get a flag
//! wrong, and for the person who hands pepe to one.

use std::time::Duration;

use crate::cli::Cli;
use crate::ui::format;

/// What `--allow-host`, `--max-requests`, `--max-rate` and
/// `--max-concurrency` allow
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Guard {
    /// Hosts a request may go to: `api.example.com` exactly, or
    /// `.example.com` for it and every subdomain; empty allows any
    pub hosts: Vec<String>,
    pub max_requests: Option<u64>,
    pub max_rate: Option<f64>,
    pub max_concurrency: Option<u32>,
}

impl Guard {
    pub fn of(cli: &Cli) -> Guard {
        Guard {
            hosts: cli.allow_host.clone(),
            max_requests: cli.max_requests,
            max_rate: cli.max_rate,
            max_concurrency: cli.max_concurrency,
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Guard::default()
    }

    /// Whether `host` is one of the allowed; any when none is named
    pub fn allows(&self, host: &str) -> bool {
        self.hosts.is_empty() || host_allowed(&self.hosts, host)
    }

    /// An error naming the host when `url` may not be sent to
    pub fn check_url(&self, url: &str) -> Result<(), String> {
        if self.hosts.is_empty() {
            return Ok(());
        }
        let host = reqwest::Url::parse(url)
            .ok()
            .and_then(|u| u.host_str().map(|h| h.to_string()))
            .ok_or_else(|| format!("{url:?} has no host to check against --allow-host"))?;
        self.check_host(&host)
    }

    pub fn check_host(&self, host: &str) -> Result<(), String> {
        if self.allows(host) {
            return Ok(());
        }
        Err(format!(
            "{host} isn't an allowed host (--allow-host {})",
            self.hosts.join(", ")
        ))
    }

    /// The load of a plain run, an API run, a flow or a replay: `-n` or
    /// `-z` with `--rate`, and `-c`
    pub fn check_run(&self, cli: &Cli) -> Result<(), String> {
        if let Some(max) = self.max_concurrency {
            if cli.concurrency > max {
                return Err(format!(
                    "-c {} is over --max-concurrency {max}",
                    cli.concurrency
                ));
            }
        }
        if let Some(max) = self.max_rate {
            match cli.rate {
                Some(rate) if rate > max => {
                    return Err(format!("--rate {rate} is over --max-rate {max}"))
                }
                Some(_) => {}
                None => {
                    return Err(format!(
                        "without --rate the run sends as fast as the target answers; --max-rate {max} needs a --rate under it"
                    ))
                }
            }
        }
        if let Some(max) = self.max_requests {
            match cli.run_duration() {
                None => {
                    if u64::from(cli.number) > max {
                        return Err(format!("-n {} is over --max-requests {max}", cli.number));
                    }
                }
                Some(duration) => match cli.rate {
                    Some(rate) => {
                        let would = rate * duration.as_secs_f64();
                        if would > max as f64 {
                            return Err(format!(
                                "--rate {rate} for {} is about {} requests, over --max-requests {max}",
                                format::span(duration),
                                format::count(would.round() as u64)
                            ));
                        }
                    }
                    None => {
                        return Err(format!(
                            "a timed run without --rate has no bound on its requests; --max-requests {max} needs -n, or -z with --rate"
                        ))
                    }
                },
            }
        }
        Ok(())
    }

    /// A ramp: its peak concurrency; it has no rate and no count
    pub fn check_ramp(&self, peak: u32) -> Result<(), String> {
        if let Some(max) = self.max_concurrency {
            if peak > max {
                return Err(format!(
                    "the ramp goes to {peak} concurrent, over --max-concurrency {max}"
                ));
            }
        }
        if let Some(max) = self.max_requests {
            return Err(format!(
                "a ramp sends as fast as the target answers at each step and has no bound on its requests; --max-requests {max} can't hold it (run `pepe -n` instead)"
            ));
        }
        if let Some(max) = self.max_rate {
            return Err(format!(
                "a ramp has no rate to cap; --max-rate {max} can't hold it"
            ));
        }
        Ok(())
    }

    /// A ping: a request every `every` to each of `targets`, `count`
    /// times or for `duration`
    pub fn check_ping(
        &self,
        targets: usize,
        every: Duration,
        count: Option<u64>,
        duration: Option<Duration>,
    ) -> Result<(), String> {
        let per_second = targets as f64 / every.as_secs_f64().max(1e-9);
        if let Some(max) = self.max_rate {
            if per_second > max {
                return Err(format!(
                    "{targets} target{} every {} is {per_second:.1} requests a second, over --max-rate {max}",
                    if targets == 1 { "" } else { "s" },
                    crate::ping::every_text(every)
                ));
            }
        }
        if let Some(max) = self.max_requests {
            let would = match (count, duration) {
                (Some(n), _) => n as f64 * targets as f64,
                (None, Some(d)) => {
                    (d.as_secs_f64() / every.as_secs_f64().max(1e-9)).ceil() * targets as f64
                }
                (None, None) => {
                    return Err(format!(
                        "without -n or -z a ping never ends; --max-requests {max} needs one"
                    ))
                }
            };
            if would > max as f64 {
                return Err(format!(
                    "that is about {} pings, over --max-requests {max}",
                    format::count(would as u64)
                ));
            }
        }
        Ok(())
    }

    /// The guard in a line, for the plan
    pub fn describe(&self) -> Option<String> {
        if self.is_empty() {
            return None;
        }
        let mut parts = Vec::new();
        if !self.hosts.is_empty() {
            parts.push(format!("hosts {}", self.hosts.join(", ")));
        }
        if let Some(n) = self.max_requests {
            parts.push(format!("at most {} requests", format::count(n)));
        }
        if let Some(r) = self.max_rate {
            parts.push(format!("at most {r} req/s"));
        }
        if let Some(c) = self.max_concurrency {
            parts.push(format!("at most {c} in flight"));
        }
        Some(parts.join(" · "))
    }
}

/// Whether `host` is one of `allowed`: a name exactly, or `.example.com`
/// (or `*.example.com`) for example.com and anything under it. Case
/// doesn't matter; a port or IPv6 brackets on either side are ignored.
pub fn host_allowed(allowed: &[String], host: &str) -> bool {
    let host = bare(host);
    allowed.iter().any(|pattern| {
        let pattern = bare(pattern);
        if pattern == "*" {
            return true;
        }
        if let Some(suffix) = pattern
            .strip_prefix("*.")
            .or_else(|| pattern.strip_prefix('.'))
        {
            return host == suffix || host.ends_with(&format!(".{suffix}"));
        }
        host == pattern
    })
}

/// A host name without its port or brackets, in lower case
fn bare(host: &str) -> String {
    let host = host.trim();
    let host = if let Some(rest) = host.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else if host.matches(':').count() == 1 {
        host.split(':').next().unwrap_or("")
    } else {
        host
    };
    host.trim_end_matches('.').to_ascii_lowercase()
}

// ─── The dry run ─────────────────────────────────────────────────────────────

/// What a run would do, said instead of done
#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub mode: &'static str,
    /// What would be sent to: one URL, a spec's endpoints, a flow's steps,
    /// a log's URLs, a ping's targets
    pub targets: Vec<String>,
    /// More targets than are listed
    pub more_targets: usize,
    pub method: Option<String>,
    /// Headers with secrets masked
    pub headers: Vec<String>,
    pub body_bytes: Option<usize>,
    /// The load in words: "1,000 requests, 20 at a time"
    pub load: String,
    /// Timeout, keep-alive, redirects, compression, proxy, threads
    pub settings: Vec<String>,
    pub guard: Option<String>,
    pub notes: Vec<String>,
}

/// Headers whose values are secrets
const SECRET_HEADERS: [&str; 6] = [
    "authorization",
    "proxy-authorization",
    "cookie",
    "x-api-key",
    "x-auth-token",
    "api-key",
];

impl Plan {
    /// The request's headers as `-H` gave them, secrets masked
    pub fn headers_of(headers: &[String]) -> Vec<String> {
        headers
            .iter()
            .map(|h| match h.split_once(':') {
                Some((name, value))
                    if SECRET_HEADERS.contains(&name.trim().to_ascii_lowercase().as_str()) =>
                {
                    let value = value.trim();
                    // The scheme of an Authorization header says what kind it is
                    let kept = value
                        .split_once(' ')
                        .filter(|(scheme, _)| scheme.chars().all(|c| c.is_ascii_alphanumeric()))
                        .map(|(scheme, _)| format!("{scheme} "))
                        .unwrap_or_default();
                    format!("{}: {kept}••••", name.trim())
                }
                _ => h.clone(),
            })
            .collect()
    }

    /// The load of a plain run, an API run, a flow or a replay
    pub fn load_of(cli: &Cli, unit: &str) -> String {
        let mut text = match cli.run_duration() {
            Some(duration) => format!("for {}", format::span(duration)),
            None => format!("{} {unit}", format::count(u64::from(cli.number))),
        };
        match cli.rate {
            Some(rate) => text.push_str(&format!(
                ", started at {rate} a second, up to {} in flight",
                cli.concurrency
            )),
            None => text.push_str(&format!(
                ", {} at a time, each sent as soon as the last answered",
                cli.concurrency
            )),
        }
        if let Some(warmup) = cli.warmup() {
            text.push_str(&format!(", after a {} warm-up", format::span(warmup)));
        }
        text
    }

    pub fn settings_of(cli: &Cli) -> Vec<String> {
        let mut out = vec![format!("timeout {}s", cli.timeout)];
        out.push(if cli.disable_keepalive {
            "a new connection per request".into()
        } else {
            "connections kept alive".into()
        });
        out.push(if cli.disable_redirects {
            "redirects not followed".into()
        } else {
            "redirects followed".into()
        });
        if cli.disable_compression {
            out.push("no compression asked for".into());
        }
        if cli.insecure {
            out.push("any TLS certificate accepted".into());
        }
        if let Some(proxy) = &cli.proxy {
            out.push(format!("through {proxy}"));
        }
        if let Some(threads) = &cli.threads {
            out.push(format!("threads {threads:?}").to_lowercase());
        }
        out
    }

    pub fn text(&self) -> String {
        use std::fmt::Write;
        let mut out = String::from("pepe · dry run · nothing sent\n");
        let _ = writeln!(out, "  mode      {}", self.mode);
        for (i, target) in self.targets.iter().enumerate() {
            let label = if i == 0 { "target" } else { "" };
            let _ = writeln!(out, "  {label:<9} {target}");
        }
        if self.more_targets > 0 {
            let _ = writeln!(out, "            … and {} more", self.more_targets);
        }
        if let Some(method) = &self.method {
            let _ = writeln!(out, "  method    {method}");
        }
        for (i, header) in self.headers.iter().enumerate() {
            let label = if i == 0 { "headers" } else { "" };
            let _ = writeln!(out, "  {label:<9} {header}");
        }
        if let Some(bytes) = self.body_bytes {
            let _ = writeln!(out, "  body      {}", format::bytes(bytes as f64));
        }
        if !self.load.is_empty() {
            let _ = writeln!(out, "  load      {}", self.load);
        }
        if !self.settings.is_empty() {
            let _ = writeln!(out, "  settings  {}", self.settings.join(" · "));
        }
        if let Some(guard) = &self.guard {
            let _ = writeln!(out, "  guard     {guard}");
        }
        for note in &self.notes {
            let _ = writeln!(out, "  note      {note}");
        }
        out
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::json!({
            "schema_version": 1,
            "mode": "dry-run",
            "would_run": self.mode,
            "targets": self.targets,
            "more_targets": self.more_targets,
            "method": self.method,
            "headers": self.headers,
            "body_bytes": self.body_bytes,
            "load": self.load,
            "settings": self.settings,
            "guard": self.guard,
            "notes": self.notes,
            "sent": 0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn cli(args: &[&str]) -> Cli {
        Cli::parse_from(["pepe"].iter().chain(args.iter()))
    }

    #[test]
    fn hosts_match_exactly_or_under_a_dot() {
        let allowed = vec![".example.com".to_string(), "localhost".to_string()];
        assert!(host_allowed(&allowed, "example.com"));
        assert!(host_allowed(&allowed, "api.example.com"));
        assert!(host_allowed(&allowed, "API.Example.com:8443"));
        assert!(host_allowed(&allowed, "localhost"));
        assert!(!host_allowed(&allowed, "notexample.com"));
        assert!(!host_allowed(&allowed, "example.com.evil.net"));
        assert!(!host_allowed(&allowed, "127.0.0.1"));
        let exact = vec!["api.example.com".to_string()];
        assert!(host_allowed(&exact, "api.example.com"));
        assert!(!host_allowed(&exact, "www.example.com"));
        assert!(host_allowed(
            &["*.example.com".to_string()],
            "a.b.example.com"
        ));
        assert!(host_allowed(&["[::1]".to_string()], "::1"));
    }

    #[test]
    fn a_url_outside_the_allowed_hosts_is_refused() {
        let guard = Guard::of(&cli(&["--allow-host", ".example.com", "https://x"]));
        assert!(guard.check_url("https://api.example.com/health").is_ok());
        let err = guard.check_url("https://evil.net/").unwrap_err();
        assert!(err.contains("evil.net isn't an allowed host"), "{err}");
        assert!(Guard::of(&cli(&["https://x"]))
            .check_url("https://anything/")
            .is_ok());
    }

    #[test]
    fn the_load_is_held_under_the_caps() {
        let guard = Guard::of(&cli(&[
            "--max-requests",
            "1000",
            "--max-rate",
            "100",
            "--max-concurrency",
            "8",
            "https://x",
        ]));
        assert!(guard
            .check_run(&cli(&["-n", "500", "-c", "8", "--rate", "50", "https://x"]))
            .is_ok());
        assert!(guard
            .check_run(&cli(&[
                "-n",
                "5000",
                "-c",
                "8",
                "--rate",
                "50",
                "https://x"
            ]))
            .unwrap_err()
            .contains("over --max-requests"));
        assert!(guard
            .check_run(&cli(&["-n", "500", "-c", "9", "--rate", "50", "https://x"]))
            .unwrap_err()
            .contains("over --max-concurrency"));
        assert!(guard
            .check_run(&cli(&["-n", "500", "-c", "8", "https://x"]))
            .unwrap_err()
            .contains("needs a --rate"));
        // 30s at 50 a second is 1,500: over
        assert!(guard
            .check_run(&cli(&["-z", "30s", "-c", "8", "--rate", "50", "https://x"]))
            .unwrap_err()
            .contains("over --max-requests"));
        assert!(guard
            .check_run(&cli(&["-z", "10s", "-c", "8", "--rate", "50", "https://x"]))
            .is_ok());
        assert!(guard
            .check_ramp(200)
            .unwrap_err()
            .contains("max-concurrency"));
        assert!(Guard::of(&cli(&["--max-requests", "10", "https://x"]))
            .check_ramp(5)
            .unwrap_err()
            .contains("no bound"));
        let s = Duration::from_secs;
        assert!(guard.check_ping(2, s(1), Some(100), None).is_ok());
        assert!(guard
            .check_ping(2, s(1), None, None)
            .unwrap_err()
            .contains("never ends"));
        assert!(guard
            .check_ping(2, s(1), None, Some(s(600)))
            .unwrap_err()
            .contains("over --max-requests"));
        assert!(guard
            .check_ping(200, Duration::from_millis(100), Some(1), None)
            .unwrap_err()
            .contains("over --max-rate"));
    }

    #[test]
    fn the_plan_masks_secrets_and_says_the_load() {
        let c = cli(&[
            "-n",
            "1000",
            "-c",
            "20",
            "-H",
            "Authorization: Bearer s3cret",
            "-H",
            "Accept: text/html",
            "--allow-host",
            "example.com",
            "https://example.com/",
        ]);
        let plan = Plan {
            mode: "run",
            targets: vec!["https://example.com/".into()],
            method: Some("GET".into()),
            headers: Plan::headers_of(&c.headers),
            body_bytes: None,
            load: Plan::load_of(&c, "requests"),
            settings: Plan::settings_of(&c),
            guard: Guard::of(&c).describe(),
            ..Default::default()
        };
        let text = plan.text();
        assert!(
            text.starts_with("pepe · dry run · nothing sent\n"),
            "{text}"
        );
        assert!(
            text.contains("Authorization: Bearer ••••") && !text.contains("s3cret"),
            "{text}"
        );
        assert!(text.contains("Accept: text/html"), "{text}");
        assert!(text.contains("1,000 requests, 20 at a time"), "{text}");
        assert!(text.contains("guard     hosts example.com"), "{text}");
        let json = plan.json();
        assert_eq!(json["sent"], 0);
        assert_eq!(json["would_run"], "run");
        assert_eq!(json["headers"][0], "Authorization: Bearer ••••");
        let timed = cli(&["-z", "30s", "--rate", "500", "-c", "64", "https://x"]);
        assert_eq!(
            Plan::load_of(&timed, "requests"),
            "for 30s, started at 500 a second, up to 64 in flight"
        );
    }
}
