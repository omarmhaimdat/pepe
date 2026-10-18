//! A run's report in the shape someone needs it: a page for people
//! (HTML), the report for scripts (JSON), a summary for a pull request or
//! an issue (Markdown), what pepe prints (text), the run's numbers a row at
//! a time (CSV). The screens' export page (F4), `--export FILE` and
//! `pepe export REPORT.json` all go through here.

use std::fmt::Write;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::Value;

use crate::html::{Extras, Point};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Html,
    Json,
    Markdown,
    Text,
    Csv,
}

impl Format {
    pub const ALL: [Format; 5] = [
        Format::Html,
        Format::Json,
        Format::Markdown,
        Format::Text,
        Format::Csv,
    ];

    pub fn ext(self) -> &'static str {
        match self {
            Format::Html => "html",
            Format::Json => "json",
            Format::Markdown => "md",
            Format::Text => "txt",
            Format::Csv => "csv",
        }
    }

    /// What it is, on the export page
    pub fn label(self) -> &'static str {
        match self {
            Format::Html => "Page",
            Format::Json => "Report",
            Format::Markdown => "Summary",
            Format::Text => "Text",
            Format::Csv => "Numbers",
        }
    }

    pub fn about(self) -> &'static str {
        match self {
            Format::Html => "curves, thresholds, the last run: for people",
            Format::Json => "the whole report: for scripts and pepe compare",
            Format::Markdown => "the verdict and numbers: for a PR or an issue",
            Format::Text => "what pepe prints when you quit",
            Format::Csv => "a row a second (a step, a target): for a sheet",
        }
    }

    /// The format a file's extension asks for
    pub fn of_path(path: &Path) -> Result<Format, String> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "html" | "htm" => Ok(Format::Html),
            "json" => Ok(Format::Json),
            "md" | "markdown" => Ok(Format::Markdown),
            "txt" | "text" => Ok(Format::Text),
            "csv" => Ok(Format::Csv),
            _ => Err(format!(
                "{}: the extension says the format: .html, .json, .md, .txt or .csv",
                path.display()
            )),
        }
    }

    pub fn parse(name: &str) -> Result<Format, String> {
        Format::of_path(Path::new(&format!("x.{name}")))
            .map_err(|_| format!("{name:?} isn't a format: html, json, md, txt or csv"))
    }
}

/// A run as it stands, for exporting
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub report: Value,
    pub extras: Extras,
    /// What pepe prints at the end, when the screen has it
    pub text: Option<String>,
}

fn ms(value: f64) -> String {
    crate::ui::format::latency(Duration::from_secs_f64(value.max(0.0) / 1e3))
}

fn count(n: f64) -> String {
    crate::ui::format::count(n.max(0.0).round() as u64)
}

fn span(seconds: f64) -> String {
    crate::ui::format::span(Duration::from_secs_f64(seconds.max(0.0)))
}

/// What the report is of: the saved test, else what it sent
pub fn name_of(report: &Value) -> String {
    if let Some(test) = report["test"].as_str() {
        return test.to_string();
    }
    let target = &report["target"];
    let url = target["url"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            // A ramp's report says its URL in its command; a ping's in its targets
            report["targets"][0]["target"]
                .as_str()
                .map(str::to_string)
                .unwrap_or_else(|| {
                    report["command"]
                        .as_str()
                        .and_then(|c| c.split_whitespace().last())
                        .unwrap_or("")
                        .trim_matches('\'')
                        .to_string()
                })
        });
    url.replace("https://", "").replace("http://", "")
}

/// A file name for it, made of its name and now: `checkout-2026-10-18-1430`
pub fn stem(report: &Value) -> String {
    // The host and path of a URL, without its query
    let name = name_of(report);
    let name = name.split('?').next().unwrap_or("").trim_end_matches('/');
    let name: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '-'
            }
        })
        .collect();
    let name = name.trim_matches('-');
    let name: String = name.chars().take(48).collect();
    let when = std::process::Command::new("date")
        .arg("+%Y-%m-%d-%H%M")
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    match (name.is_empty(), when.is_empty()) {
        (true, _) => format!("pepe-{when}"),
        (false, true) => name,
        (false, false) => format!("{name}-{when}"),
    }
}

/// The report in `format`
pub fn render(format: Format, snap: &Snapshot) -> String {
    match format {
        Format::Html => crate::html::render(&snap.report, &snap.extras),
        Format::Json => serde_json::to_string_pretty(&snap.report).unwrap_or_default() + "\n",
        Format::Markdown => markdown(snap),
        Format::Text => snap.text.clone().unwrap_or_else(|| plain(snap)),
        Format::Csv => csv(snap),
    }
}

/// Write it to `path`; the path written
pub fn save(format: Format, snap: &Snapshot, path: &Path) -> Result<PathBuf, String> {
    if format == Format::Html && snap.report["mode"] == "ping" {
        return Err("a ping has no page yet: its .json, .md, .txt or .csv".into());
    }
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    }
    std::fs::write(path, render(format, snap))
        .map_err(|e| format!("couldn't write {}: {e}", path.display()))?;
    Ok(path.to_path_buf())
}

/// How the run went, in a word and a mark
fn verdict(snap: &Snapshot) -> (String, &'static str) {
    let crossed: Vec<String> = snap
        .extras
        .checks
        .iter()
        .filter(|c| !c.held)
        .map(|c| c.text.clone())
        .chain(snap.extras.failed.iter().cloned())
        .collect();
    let r = &snap.report;
    let level = r["verdict"]["level"]
        .as_str()
        .unwrap_or(match r["findings"].as_array() {
            Some(f) if f.iter().any(|f| f["level"] == "failing") => "failing",
            Some(f) if f.iter().any(|f| f["level"] == "degraded") => "degraded",
            _ => "healthy",
        });
    let stopped = r["summary"]["interrupted"] == true
        || r["ended"].as_str().is_some_and(|e| e != "completed");
    match (crossed.first(), level, stopped) {
        (Some(first), _, _) => (format!("Failed · {first}"), "❌"),
        (None, "failing", _) => ("Failing".into(), "❌"),
        (None, _, true) => ("Stopped".into(), "⏹️"),
        (None, "degraded", _) => ("Passed, with a warning".into(), "⚠️"),
        _ => ("Passed".into(), "✅"),
    }
}

/// For a pull request or an issue: the verdict, the numbers, what held
/// and what didn't, how it compares, what pepe found, and the command
pub fn markdown(snap: &Snapshot) -> String {
    let r = &snap.report;
    let (word, mark) = verdict(snap);
    let mut out = format!("### {mark} {word} · {}\n\n", name_of(r));
    let s = &r["summary"];
    if s.is_object() {
        let total = s["total_requests"].as_f64().unwrap_or(0.0);
        let failed = s["failed_requests"].as_f64().unwrap_or(0.0);
        let lat = &s["latency"];
        out.push_str("| Requests | Throughput | p50 | p95 | p99 | max | Errors | Duration |\n");
        out.push_str("| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |\n");
        let _ = writeln!(
            out,
            "| {} | {} req/s | {} | {} | {} | {} | {:.2}% | {} |",
            count(total),
            count(s["requests_per_second"].as_f64().unwrap_or(0.0)),
            ms(lat["median_ms"].as_f64().unwrap_or(0.0)),
            ms(lat["p95_ms"].as_f64().unwrap_or(0.0)),
            ms(lat["p99_ms"].as_f64().unwrap_or(0.0)),
            ms(lat["max_ms"].as_f64().unwrap_or(0.0)),
            if total > 0.0 {
                failed * 100.0 / total
            } else {
                0.0
            },
            span(s["duration_ms"].as_f64().unwrap_or(0.0) / 1e3)
        );
        if let Some(failures) = s["failures"].as_array().filter(|f| !f.is_empty()) {
            out.push_str("\n**Failures**\n\n");
            for f in failures.iter().take(5) {
                let _ = write!(
                    out,
                    "- `{}` ×{}",
                    f["cause"].as_str().unwrap_or("?"),
                    count(f["count"].as_f64().unwrap_or(0.0))
                );
                if let Some(body) = f["example_body"].as_str().filter(|b| !b.trim().is_empty()) {
                    let body: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
                    let _ = write!(
                        out,
                        ": `{}`",
                        crate::webhook::layout_cut(&body, 120).replace('`', "'")
                    );
                }
                out.push('\n');
            }
        }
    } else if let Some(steps) = r["steps"].as_array() {
        let _ = writeln!(
            out,
            "Held **{}**{}.\n",
            r["holds_concurrency"]
                .as_u64()
                .map(|c| format!("{c} at a time"))
                .unwrap_or_else(|| "no step".into()),
            r["capacity"]["requests_per_second"]
                .as_f64()
                .map(|c| format!(" · capacity about **{} req/s**", count(c)))
                .unwrap_or_default()
        );
        out.push_str(
            "| Concurrency | req/s | p50 | p99 | Errors |\n| ---: | ---: | ---: | ---: | ---: |\n",
        );
        for st in steps {
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} | {:.2}% |",
                st["concurrency"].as_u64().unwrap_or(0),
                count(st["requests_per_second"].as_f64().unwrap_or(0.0)),
                ms(st["median_ms"].as_f64().unwrap_or(0.0)),
                ms(st["p99_ms"].as_f64().unwrap_or(0.0)),
                st["error_rate_percent"].as_f64().unwrap_or(0.0)
            );
        }
    } else if let Some(targets) = r["targets"].as_array() {
        out.push_str("| Target | Sent | Loss | p50 | p99 |\n| --- | ---: | ---: | ---: | ---: |\n");
        for t in targets {
            let lat = &t["latency_ms"];
            let _ = writeln!(
                out,
                "| `{}` | {} | {:.1}% | {} | {} |",
                t["name"].as_str().or(t["target"].as_str()).unwrap_or("?"),
                count(t["sent"].as_f64().unwrap_or(0.0)),
                t["loss"].as_f64().unwrap_or(0.0) * 100.0,
                ms(lat["p50"].as_f64().unwrap_or(0.0)),
                ms(lat["p99"].as_f64().unwrap_or(0.0))
            );
        }
    }
    if !snap.extras.checks.is_empty() || !snap.extras.failed.is_empty() {
        out.push_str("\n**Thresholds**\n\n");
        for c in &snap.extras.checks {
            let _ = writeln!(
                out,
                "- {} fail if `{}` · {}",
                if c.held { "✅" } else { "❌" },
                c.text,
                c.measured
            );
        }
        for f in &snap.extras.failed {
            let _ = writeln!(out, "- ❌ {f}");
        }
    }
    if let Some((comparison, when)) = &snap.extras.compared {
        let _ = writeln!(
            out,
            "\n**Against the last run** ({when}): {}\n",
            comparison.verdict.headline()
        );
        out.push_str("| | before | now | |\n| --- | ---: | ---: | ---: |\n");
        for c in &comparison.changes {
            let fmt = |v: f64| {
                if c.metric.ends_with("_ms") {
                    ms(v)
                } else if c.metric.contains("per_second") {
                    count(v)
                } else {
                    format!("{v:.2}")
                }
            };
            let _ = writeln!(
                out,
                "| {} | {} | {} | {} |",
                crate::compare::short_name(c.metric),
                fmt(c.before),
                fmt(c.after),
                c.change_percent
                    .map(|p| format!("{}{:.0}%", if p >= 0.0 { "+" } else { "−" }, p.abs()))
                    .unwrap_or_default()
            );
        }
    }
    let findings: Vec<(String, String)> = r["verdict"]["findings"]
        .as_array()
        .or(r["findings"].as_array())
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let text = f["text"].as_str().or(f["title"].as_str())?;
            Some((
                f["level"].as_str().unwrap_or("").to_string(),
                text.to_string(),
            ))
        })
        .collect();
    if !findings.is_empty() {
        out.push_str("\n**What pepe found**\n\n");
        for (level, text) in findings {
            let dot = match level.as_str() {
                "failing" | "bad" => "🔴",
                "degraded" | "warn" => "🟡",
                _ => "🟢",
            };
            let _ = writeln!(out, "- {dot} {text}");
        }
    }
    if let Some(command) = r["command"].as_str() {
        let _ = write!(out, "\n```\n{command}\n```\n");
    }
    let _ = writeln!(out, "\n<sub>pepe {}</sub>", crate::utils::version());
    out
}

/// The summary without its markup, when there's no text report to give
fn plain(snap: &Snapshot) -> String {
    markdown(snap)
        .lines()
        .filter(|l| !l.starts_with("| ---") && !l.starts_with("```") && !l.starts_with("<sub>"))
        .map(|l| {
            l.trim_start_matches("### ")
                .replace("**", "")
                .replace('`', "")
                .trim_matches('|')
                .replace(" | ", "  ")
                .trim()
                .to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

/// The run's numbers a row at a time: a second (or a minute) for a run, a
/// step for a ramp, a target for a ping
pub fn csv(snap: &Snapshot) -> String {
    let r = &snap.report;
    let mut out = String::new();
    if let Some(steps) = r["steps"].as_array().filter(|_| r.get("summary").is_none()) {
        out.push_str("concurrency,seconds,requests,failed_requests,requests_per_second,error_rate_percent,median_ms,p99_ms\n");
        for s in steps {
            let _ = writeln!(
                out,
                "{},{},{},{},{:.3},{:.3},{:.3},{:.3}",
                s["concurrency"].as_u64().unwrap_or(0),
                s["seconds"].as_f64().unwrap_or(0.0),
                s["requests"].as_u64().unwrap_or(0),
                s["failed_requests"].as_u64().unwrap_or(0),
                s["requests_per_second"].as_f64().unwrap_or(0.0),
                s["error_rate_percent"].as_f64().unwrap_or(0.0),
                s["median_ms"].as_f64().unwrap_or(0.0),
                s["p99_ms"].as_f64().unwrap_or(0.0)
            );
        }
        return out;
    }
    if let Some(targets) = r["targets"].as_array() {
        out.push_str("target,sent,answered,lost,loss,p50_ms,p99_ms,max_ms\n");
        for t in targets {
            let lat = &t["latency_ms"];
            let name = t["target"].as_str().unwrap_or("").replace('"', "\"\"");
            let _ = writeln!(
                out,
                "\"{name}\",{},{},{},{:.4},{:.3},{:.3},{:.3}",
                t["sent"].as_u64().unwrap_or(0),
                t["answered"].as_u64().unwrap_or(0),
                t["lost"].as_u64().unwrap_or(0),
                t["loss"].as_f64().unwrap_or(0.0),
                lat["p50"].as_f64().unwrap_or(0.0),
                lat["p99"].as_f64().unwrap_or(0.0),
                lat["max"].as_f64().unwrap_or(0.0)
            );
        }
        return out;
    }
    let points: Vec<Point> = if snap.extras.seconds.is_empty() {
        r["timeline"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|p| Point {
                at: p["at_s"].as_f64().unwrap_or(0.0),
                rps: p["requests_per_second"].as_f64().unwrap_or(0.0),
                p50: p["p50_ms"].as_f64().unwrap_or(0.0),
                p99: p["p99_ms"].as_f64().unwrap_or(0.0),
                errors: p["errors_per_second"].as_f64().unwrap_or(0.0),
            })
            .collect()
    } else {
        snap.extras.seconds.clone()
    };
    out.push_str("at_s,requests_per_second,errors_per_second,p50_ms,p99_ms\n");
    for p in points {
        let _ = writeln!(
            out,
            "{:.3},{:.3},{:.3},{:.3},{:.3}",
            p.at, p.rps, p.errors, p.p50, p.p99
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snap() -> Snapshot {
        Snapshot {
            report: json!({
                "test": "checkout", "command": "pepe --load checkout",
                "target": {"mode": "run", "method": "GET", "url": "https://a.io/x", "concurrency": 8},
                "summary": {"total_requests": 1000, "failed_requests": 10, "requests_per_second": 250.0, "duration_ms": 4000.0,
                    "latency": {"median_ms": 12.0, "p95_ms": 40.0, "p99_ms": 80.0, "max_ms": 200.0},
                    "failures": [{"cause": "HTTP 503", "count": 10, "example_body": "no `upstream`"}]},
                "verdict": {"level": "degraded", "findings": [{"level": "degraded", "text": "1% of requests failed"}]}
            }),
            extras: Extras {
                checks: vec![crate::webhook::Check {
                    text: "p99 > 50ms".into(),
                    held: false,
                    measured: "p99 was 80ms".into(),
                }],
                seconds: vec![Point {
                    at: 1.0,
                    rps: 250.0,
                    p50: 12.0,
                    p99: 80.0,
                    errors: 2.5,
                }],
                ..Extras::default()
            },
            text: None,
        }
    }

    #[test]
    fn formats_are_told_by_their_extension() {
        assert_eq!(Format::of_path(Path::new("r.HTML")), Ok(Format::Html));
        assert_eq!(
            Format::of_path(Path::new("x/summary.md")),
            Ok(Format::Markdown)
        );
        assert_eq!(Format::parse("csv"), Ok(Format::Csv));
        assert!(Format::of_path(Path::new("r.pdf"))
            .unwrap_err()
            .contains(".html, .json"));
    }

    #[test]
    fn the_summary_is_ready_to_paste() {
        let md = markdown(&snap());
        assert!(
            md.starts_with("### ❌ Failed · p99 > 50ms · checkout"),
            "{md}"
        );
        assert!(md.contains("| 1,000 | 250 req/s | 12.00ms |"), "{md}");
        assert!(
            md.contains("- ❌ fail if `p99 > 50ms` · p99 was 80ms"),
            "{md}"
        );
        assert!(
            md.contains("`HTTP 503` ×10: `no 'upstream'`"),
            "backticks in a body can't break the code: {md}"
        );
        assert!(md.contains("- 🟡 1% of requests failed"));
        assert!(md.contains("```\npepe --load checkout\n```"));
        let text = render(Format::Text, &snap());
        assert!(
            !text.contains("**") && !text.contains("| ---") && text.contains("Failed"),
            "{text}"
        );
    }

    #[test]
    fn numbers_go_a_row_at_a_time() {
        let csv = csv(&snap());
        assert_eq!(
            csv,
            "at_s,requests_per_second,errors_per_second,p50_ms,p99_ms\n1.000,250.000,2.500,12.000,80.000\n"
        );
        assert!(stem(&snap().report).starts_with("checkout-"));
        let url = serde_json::json!({"target": {"url": "http://127.0.0.1:8089/slow?ms=2"}});
        assert!(
            stem(&url).starts_with("127.0.0.1-8089-slow-"),
            "{}",
            stem(&url)
        );
    }
}
