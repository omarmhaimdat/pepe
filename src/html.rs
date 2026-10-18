//! The report as a page: one self-contained HTML file a run leaves for
//! people who weren't at the terminal. Pepe's face for the verdict, the
//! story of the run in a sentence, the numbers that matter, the run's
//! curves with what changed marked on them, the latency spread, where the
//! time went, the thresholds, how it compares with the last run, what
//! pepe found, and the slowest requests. No script, no font, nothing
//! fetched: it opens anywhere, offline, light or dark as the reader likes.

use std::fmt::Write;
use std::time::Duration;

use serde_json::Value;

use crate::webhook::Check;

/// A second (or a minute) of the run: requests a second, p50 and p99 in
/// ms, errors a second
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Point {
    pub at: f64,
    pub rps: f64,
    pub p50: f64,
    pub p99: f64,
    pub errors: f64,
}

/// What the page knows besides the report
#[derive(Debug, Clone, Default)]
pub struct Extras {
    /// The run's seconds, when the run is the one drawing the page; the
    /// report's minutes otherwise
    pub seconds: Vec<Point>,
    /// Each `--fail-if`, whether it held
    pub checks: Vec<Check>,
    /// Why it failed besides those
    pub failed: Vec<String>,
    /// The last recorded run of the same test, against this one, and when
    pub compared: Option<(crate::compare::Comparison, String)>,
    pub host: String,
    /// When it ran, as the page says it
    pub when: String,
}

const PROUD: &[u8] = include_bytes!("../site/img/pepe-proud.png");
const WORRIED: &[u8] = include_bytes!("../site/img/pepe-worried.png");
const ON_FIRE: &[u8] = include_bytes!("../site/img/pepe-on-fire.png");
const DIZZY: &[u8] = include_bytes!("../site/img/pepe-dizzy.png");

fn esc(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
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

/// The verdict: a word, the class its colour comes from, Pepe's face
struct Verdict {
    word: String,
    class: &'static str,
    face: &'static [u8],
}

fn verdict(level: &str, crossed: &[String], interrupted: bool) -> Verdict {
    match (crossed.first(), level, interrupted) {
        (Some(first), _, _) => Verdict {
            word: format!("Failed · {}", first.split(':').next().unwrap_or(first)),
            class: "bad",
            face: WORRIED,
        },
        (None, "failing", _) => Verdict {
            word: "Failing".into(),
            class: "bad",
            face: ON_FIRE,
        },
        (None, _, true) => Verdict {
            word: "Stopped".into(),
            class: "warn",
            face: DIZZY,
        },
        (None, "degraded", _) => Verdict {
            word: "Passed, with a warning".into(),
            class: "warn",
            face: WORRIED,
        },
        _ => Verdict {
            word: "Passed".into(),
            class: "good",
            face: PROUD,
        },
    }
}

/// At most `most` points, neighbours averaged
fn squeeze(points: &[Point], most: usize) -> Vec<Point> {
    if points.len() <= most {
        return points.to_vec();
    }
    (0..most)
        .map(|i| {
            let from = i * points.len() / most;
            let to = ((i + 1) * points.len() / most).max(from + 1);
            let part = &points[from..to];
            let n = part.len() as f64;
            Point {
                at: part[part.len() - 1].at,
                rps: part.iter().map(|p| p.rps).sum::<f64>() / n,
                p50: part.iter().map(|p| p.p50).sum::<f64>() / n,
                p99: part.iter().map(|p| p.p99).sum::<f64>() / n,
                errors: part.iter().map(|p| p.errors).sum::<f64>() / n,
            }
        })
        .collect()
}

struct Series<'a> {
    label: &'a str,
    class: &'a str,
    values: Vec<(f64, f64)>,
    area: bool,
}

/// A chart of lines over time, in SVG; `marks` are lines where something
/// changed, with what
fn chart(
    series: &[Series],
    marks: &[(f64, String)],
    label: fn(f64) -> String,
    along: fn(f64) -> String,
    aria: &str,
) -> String {
    const W: f64 = 760.0;
    const H: f64 = 170.0;
    const LEFT: f64 = 56.0;
    const RIGHT: f64 = 12.0;
    const TOP: f64 = 14.0;
    const BOTTOM: f64 = 24.0;
    let xs = series.iter().flat_map(|s| s.values.iter().map(|v| v.0));
    let (x0, x1) = xs.fold((f64::MAX, f64::MIN), |(a, b), x| (a.min(x), b.max(x)));
    let top = series
        .iter()
        .flat_map(|s| s.values.iter().map(|v| v.1))
        .fold(0.0f64, f64::max)
        * 1.12;
    if x1.partial_cmp(&x0) != Some(std::cmp::Ordering::Greater) || top <= 0.0 {
        return String::new();
    }
    let x = |v: f64| LEFT + (v - x0) / (x1 - x0) * (W - LEFT - RIGHT);
    let y = |v: f64| TOP + (1.0 - v / top) * (H - TOP - BOTTOM);
    let mut out = format!(
        r#"<svg class="chart" viewBox="0 0 {W} {H}" role="img" aria-label="{}">"#,
        esc(aria)
    );
    for frac in [0.0, 0.5, 1.0] {
        let yy = y(top / 1.12 * frac);
        let _ = write!(
            out,
            r#"<line class="grid" x1="{LEFT}" y1="{yy:.1}" x2="{:.1}" y2="{yy:.1}"/><text class="axis" x="{:.1}" y="{:.1}" text-anchor="end">{}</text>"#,
            W - RIGHT,
            LEFT - 6.0,
            yy + 3.5,
            esc(&label(top / 1.12 * frac))
        );
    }
    let _ = write!(
        out,
        r#"<text class="axis" x="{LEFT}" y="{:.1}">{}</text><text class="axis" x="{:.1}" y="{:.1}" text-anchor="end">{}</text>"#,
        H - 6.0,
        esc(&along(x0)),
        W - RIGHT,
        H - 6.0,
        esc(&along(x1))
    );
    for s in series {
        if s.values.len() < 2 {
            continue;
        }
        let path: Vec<String> = s
            .values
            .iter()
            .enumerate()
            .map(|(i, (vx, vy))| {
                format!(
                    "{}{:.1} {:.1}",
                    if i == 0 { "M" } else { "L" },
                    x(*vx),
                    y(*vy)
                )
            })
            .collect();
        let line = path.join(" ");
        if s.area {
            let _ = write!(
                out,
                r#"<path class="area {}" d="{line} L{:.1} {:.1} L{:.1} {:.1}Z"/>"#,
                s.class,
                x(s.values[s.values.len() - 1].0),
                y(0.0),
                x(s.values[0].0),
                y(0.0)
            );
        }
        let _ = write!(
            out,
            r#"<path class="line {}" d="{line}"><title>{}</title></path>"#,
            s.class,
            esc(s.label)
        );
    }
    for (at, text) in marks {
        if *at < x0 || *at > x1 {
            continue;
        }
        let mx = x(*at);
        let anchor = if mx > W * 0.7 { "end" } else { "start" };
        let tx = if anchor == "end" { mx - 5.0 } else { mx + 5.0 };
        let _ = write!(
            out,
            r#"<line class="mark" x1="{mx:.1}" y1="{TOP}" x2="{mx:.1}" y2="{:.1}"/><circle class="dot" cx="{mx:.1}" cy="{TOP}" r="4"/><text class="marktext" x="{tx:.1}" y="{:.1}" text-anchor="{anchor}">{}</text>"#,
            H - BOTTOM,
            TOP + 4.0,
            esc(text)
        );
    }
    out.push_str("</svg>");
    out
}

fn legend(items: &[(&str, &str)]) -> String {
    let parts: Vec<String> = items
        .iter()
        .map(|(class, label)| {
            format!(
                r#"<span class="key"><i class="sw {class}"></i>{}</span>"#,
                esc(label)
            )
        })
        .collect();
    format!(r#"<div class="legend">{}</div>"#, parts.join(""))
}

fn kpi(label: &str, value: &str, class: &str) -> String {
    format!(
        r#"<div class="kpi"><div class="l">{}</div><div class="v {class}">{}</div></div>"#,
        esc(label),
        esc(value)
    )
}

fn dot(level: &str) -> &'static str {
    match level {
        "failing" | "bad" => "bad",
        "degraded" | "warn" => "warn",
        _ => "good",
    }
}

/// The page for `report`, a run's or a ramp's JSON report
pub fn render(report: &Value, x: &Extras) -> String {
    let crossed: Vec<String> = x
        .checks
        .iter()
        .filter(|c| !c.held)
        .map(|c| format!("{}: {}", c.text, c.measured))
        .chain(x.failed.iter().cloned())
        .collect();
    let ramp = report.get("steps").is_some() && report.get("summary").is_none();
    let level = if ramp {
        match report["findings"].as_array() {
            Some(f) if f.iter().any(|f| f["level"] == "failing") => "failing",
            Some(f) if f.iter().any(|f| f["level"] == "degraded") => "degraded",
            _ => "healthy",
        }
    } else {
        report["verdict"]["level"].as_str().unwrap_or("healthy")
    };
    let interrupted = report["summary"]["interrupted"] == true
        || report["ended"].as_str().is_some_and(|e| e != "completed");
    let v = verdict(level, &crossed, interrupted);
    let target = &report["target"];
    let name = report["test"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            format!(
                "{} {}",
                target["method"].as_str().unwrap_or(""),
                target["url"].as_str().unwrap_or("")
            )
            .trim()
            .to_string()
        });
    let command = report["command"].as_str().unwrap_or("");
    // A ramp's report has no target: its command ends with the URL
    let name = if ramp && report["test"].as_str().is_none() {
        let url = command
            .split_whitespace()
            .last()
            .unwrap_or("")
            .trim_matches('\'')
            .to_string();
        format!(
            "ramp · {}",
            url.replace("https://", "").replace("http://", "")
        )
    } else {
        name
    };
    let mut about = Vec::new();
    if report["test"].is_string() {
        about.push(format!(
            "{} {}",
            target["method"].as_str().unwrap_or(""),
            target["url"].as_str().unwrap_or("")
        ));
    }
    if let Some(c) = target["concurrency"].as_u64() {
        if !ramp {
            about.push(format!("{c} at a time"));
        }
    }
    if !x.host.is_empty() {
        about.push(x.host.clone());
    }
    if !x.when.is_empty() {
        about.push(x.when.clone());
    }
    about.push(format!("pepe {}", crate::utils::version()));

    let mut body = String::new();
    let _ = write!(
        body,
        r#"<header><img class="face" alt="Pepe, {}" src="data:image/png;base64,{}"><div class="title"><div class="row"><h1>{}</h1><span class="pill {}">{}</span></div><div class="about">{}</div></div></header>"#,
        esc(&v.word),
        crate::ws::base64(v.face),
        esc(&name),
        v.class,
        esc(&v.word),
        esc(about
            .iter()
            .filter(|a| !a.trim().is_empty())
            .cloned()
            .collect::<Vec<_>>()
            .join(" · ")
            .as_str())
    );
    if ramp {
        body.push_str(&ramp_body(report));
    } else {
        body.push_str(&run_body(report, x, &crossed));
    }
    let _ = write!(
        body,
        r#"<footer><code>{}</code><span>Made by <a href="https://pepe.mhaimdat.com/">pepe</a></span></footer>"#,
        esc(command)
    );
    page(&format!("{} · {} · pepe", name, v.word), &body)
}

fn run_body(report: &Value, x: &Extras, crossed: &[String]) -> String {
    let s = &report["summary"];
    let total = s["total_requests"].as_f64().unwrap_or(0.0);
    let failed = s["failed_requests"].as_f64().unwrap_or(0.0);
    let errors = if total > 0.0 {
        failed * 100.0 / total
    } else {
        0.0
    };
    let duration = s["duration_ms"].as_f64().unwrap_or(0.0) / 1e3;
    let lat = &s["latency"];
    let p99 = lat["p99_ms"].as_f64().unwrap_or(0.0);
    let p99_bad = crossed.iter().any(|c| c.starts_with("p99"));
    let mut out = String::new();

    // The story
    let mut story = format!(
        "{} requests in {} at {} req/s. p50 {}, p99 {}.",
        count(total),
        span(duration),
        count(s["requests_per_second"].as_f64().unwrap_or(0.0)),
        ms(lat["median_ms"].as_f64().unwrap_or(0.0)),
        ms(p99)
    );
    if failed == 0.0 {
        story.push_str(" None failed.");
    } else {
        match s["failures"].as_array().and_then(|f| f.first()) {
            Some(top) => {
                let _ = write!(
                    story,
                    " {errors:.2}% failed ({}), most of them {} ({}).",
                    count(failed),
                    top["cause"].as_str().unwrap_or("?"),
                    count(top["count"].as_f64().unwrap_or(0.0))
                );
            }
            None => {
                let _ = write!(story, " {errors:.2}% failed ({}).", count(failed));
            }
        }
    }
    if let Some(text) = s["anomalies"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|a| a["text"].as_str())
    {
        let mut chars = text.chars();
        if let Some(first) = chars.next() {
            let _ = write!(story, " {}{}.", first.to_uppercase(), chars.as_str());
        }
    }
    let _ = write!(out, r#"<p class="story">{}</p>"#, esc(&story));

    // The numbers
    out.push_str(r#"<div class="kpis">"#);
    out.push_str(&kpi("Requests", &count(total), ""));
    out.push_str(&kpi(
        "Throughput",
        &format!(
            "{}/s",
            count(s["requests_per_second"].as_f64().unwrap_or(0.0))
        ),
        "",
    ));
    out.push_str(&kpi(
        "p50",
        &ms(lat["median_ms"].as_f64().unwrap_or(0.0)),
        "",
    ));
    out.push_str(&kpi("p99", &ms(p99), if p99_bad { "bad" } else { "" }));
    out.push_str(&kpi(
        "Errors",
        &format!("{errors:.2}%"),
        if crossed.iter().any(|c| c.starts_with("errors")) || errors >= 5.0 {
            "bad"
        } else {
            ""
        },
    ));
    out.push_str(&kpi("Duration", &span(duration), ""));
    out.push_str("</div>");

    // The curves: the run's seconds, else the report's minutes
    let points: Vec<Point> = if x.seconds.len() >= 2 {
        squeeze(&x.seconds, 600)
    } else {
        report["timeline"]
            .as_array()
            .map(|t| {
                t.iter()
                    .map(|p| Point {
                        at: p["at_s"].as_f64().unwrap_or(0.0),
                        rps: p["requests_per_second"].as_f64().unwrap_or(0.0),
                        p50: p["p50_ms"].as_f64().unwrap_or(0.0),
                        p99: p["p99_ms"].as_f64().unwrap_or(0.0),
                        errors: p["errors_per_second"].as_f64().unwrap_or(0.0),
                    })
                    .collect()
            })
            .unwrap_or_default()
    };
    let marks: Vec<(f64, String)> = s["anomalies"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|a| Some((a["at_s"].as_f64()?, a["text"].as_str()?.to_string())))
        .collect();
    if points.len() >= 2 {
        out.push_str(r#"<section><h2>Over the run</h2>"#);
        let has_errors = points.iter().any(|p| p.errors > 0.0);
        let mut keys = vec![("ember", "requests a second")];
        if has_errors {
            keys.push(("bad", "errors a second"));
        }
        out.push_str(&legend(&keys));
        let mut series = vec![Series {
            label: "requests a second",
            class: "ember",
            values: points.iter().map(|p| (p.at, p.rps)).collect(),
            area: true,
        }];
        if has_errors {
            series.push(Series {
                label: "errors a second",
                class: "bad",
                values: points.iter().map(|p| (p.at, p.errors)).collect(),
                area: false,
            });
        }
        out.push_str(&chart(
            &series,
            &marks,
            |v| format!("{}/s", crate::ui::format::compact(v)),
            span,
            "Requests a second over the run",
        ));
        out.push_str(&legend(&[("violet", "p99"), ("teal", "p50")]));
        out.push_str(&chart(
            &[
                Series {
                    label: "p99",
                    class: "violet",
                    values: points.iter().map(|p| (p.at, p.p99)).collect(),
                    area: false,
                },
                Series {
                    label: "p50",
                    class: "teal",
                    values: points.iter().map(|p| (p.at, p.p50)).collect(),
                    area: false,
                },
            ],
            &marks,
            ms,
            span,
            "Latency over the run",
        ));
        if !marks.is_empty() {
            out.push_str(r#"<ul class="changes">"#);
            for (_, text) in &marks {
                let _ = write!(out, "<li>{}</li>", esc(text));
            }
            out.push_str("</ul>");
        }
        out.push_str("</section>");
    }

    // Latency and where the time went
    out.push_str(r#"<div class="two"><section><h2>Latency</h2>"#);
    let ladder = [
        ("p50", lat["median_ms"].as_f64()),
        ("p90", lat["p90_ms"].as_f64()),
        ("p95", lat["p95_ms"].as_f64()),
        ("p99", lat["p99_ms"].as_f64()),
        ("max", lat["max_ms"].as_f64()),
    ];
    let most = ladder.iter().filter_map(|l| l.1).fold(0.0f64, f64::max);
    for (label, value) in ladder {
        let Some(value) = value else { continue };
        let width = if most > 0.0 {
            (value / most * 100.0).max(1.0)
        } else {
            0.0
        };
        let class = match label {
            "p99" if p99_bad => "bad",
            "max" => "faint",
            _ => "ember",
        };
        let _ = write!(
            out,
            r#"<div class="ladder"><span>{label}</span><div class="track"><div class="fill {class}" style="width:{width:.1}%"></div></div><span class="num">{}</span></div>"#,
            esc(&ms(value))
        );
    }
    out.push_str("</section><section>");
    if let Some(phases) = phases(&lat["phases"], total) {
        out.push_str(&phases);
    }
    if !x.checks.is_empty() || !x.failed.is_empty() {
        out.push_str("<h2>Thresholds</h2><ul class=\"checks\">");
        for c in &x.checks {
            let _ = write!(
                out,
                r#"<li><span class="{}">{}</span> fail if <code>{}</code> · {}</li>"#,
                if c.held { "good" } else { "bad" },
                if c.held { "✓" } else { "✗" },
                esc(&c.text),
                esc(&c.measured)
            );
        }
        for f in &x.failed {
            let _ = write!(out, r#"<li><span class="bad">✗</span> {}</li>"#, esc(f));
        }
        out.push_str("</ul>");
    }
    out.push_str("</section></div>");

    // Against the last run, and what pepe found
    out.push_str(r#"<div class="two">"#);
    if let Some((comparison, when)) = &x.compared {
        let _ = write!(
            out,
            r#"<section><h2>Against the last run <span class="faint">· {}</span></h2><p class="{}">{}</p><table><tr><th></th><th>before</th><th>now</th><th></th></tr>"#,
            esc(when),
            if comparison.regression { "bad" } else { "" },
            esc(comparison.verdict.headline())
        );
        for c in &comparison.changes {
            let name = crate::compare::short_name(c.metric);
            let fmt = |v: f64| {
                if c.metric.ends_with("_ms") {
                    ms(v)
                } else if c.metric.contains("per_second") {
                    count(v)
                } else {
                    format!("{v:.2}")
                }
            };
            // Up is worse for a latency or a failure, better for a rate
            let worse = match c.change_percent {
                Some(p) if c.metric.contains("per_second") => p < 0.0,
                Some(p) => p > 0.0,
                None => false,
            };
            let change = c
                .change_percent
                .map(|p| format!("{}{:.0}%", if p >= 0.0 { "+" } else { "−" }, p.abs()))
                .unwrap_or_default();
            let _ = write!(
                out,
                r#"<tr><td>{}</td><td>{}</td><td>{}</td><td class="{}">{}</td></tr>"#,
                esc(name),
                esc(&fmt(c.before)),
                esc(&fmt(c.after)),
                match (c.significant, worse) {
                    (true, true) => "bad",
                    (true, false) => "good",
                    _ => "faint",
                },
                esc(&change)
            );
        }
        out.push_str("</table></section>");
    }
    let findings: Vec<(String, String)> = report["verdict"]["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            Some((
                f["level"].as_str()?.to_string(),
                f["text"].as_str()?.to_string(),
            ))
        })
        .collect();
    let failures = s["failures"].as_array().cloned().unwrap_or_default();
    if !findings.is_empty() || !failures.is_empty() {
        out.push_str(r#"<section><h2>What pepe found</h2><ul class="found">"#);
        for f in failures.iter().take(5) {
            let body = f["example_body"]
                .as_str()
                .map(|b| b.split_whitespace().collect::<Vec<_>>().join(" "))
                .filter(|b| !b.is_empty())
                .map(|b| {
                    format!(
                        " · <code>{}</code>",
                        esc(&crate::webhook::layout_cut(&b, 160))
                    )
                })
                .unwrap_or_default();
            let _ = write!(
                out,
                r#"<li><i class="d bad"></i><b>{}</b> ×{}{body}</li>"#,
                esc(f["cause"].as_str().unwrap_or("?")),
                count(f["count"].as_f64().unwrap_or(0.0))
            );
        }
        let mut findings = findings;
        findings.sort_by_key(|(l, _)| match l.as_str() {
            "failing" => 0,
            "degraded" => 1,
            _ => 2,
        });
        for (level, text) in findings {
            let _ = write!(
                out,
                r#"<li><i class="d {}"></i>{}</li>"#,
                dot(&level),
                esc(&text)
            );
        }
        out.push_str("</ul></section>");
    }
    out.push_str("</div>");

    // Answers, endpoints or steps, the slowest requests
    if let Some(codes) = s["status_codes"].as_object().filter(|c| !c.is_empty()) {
        let mut codes: Vec<(&String, f64)> = codes
            .iter()
            .map(|(k, v)| (k, v.as_f64().unwrap_or(0.0)))
            .collect();
        codes.sort_by(|a, b| b.1.total_cmp(&a.1));
        out.push_str(r#"<section><h2>Answers</h2><div class="chips">"#);
        for (code, n) in codes {
            let class = match code.chars().next() {
                Some('2') => "good",
                Some('3') => "faint",
                _ => "bad",
            };
            let _ = write!(
                out,
                r#"<span class="chip {class}">{} <b>×{}</b></span>"#,
                esc(code),
                count(n)
            );
        }
        out.push_str("</div></section>");
    }
    let rows = report["endpoints"]
        .as_array()
        .or(report["flow"]["steps"].as_array())
        .or(report["replay"]["urls"].as_array());
    if let Some(rows) = rows.filter(|r| !r.is_empty()) {
        let what = if report["endpoints"].is_array() {
            "Endpoints"
        } else if report["flow"].is_object() {
            "Steps"
        } else {
            "URLs"
        };
        let _ = write!(
            out,
            r#"<section><h2>{what}</h2><table><tr><th></th><th>requests</th><th>failed</th><th>req/s</th><th>p50</th><th>p99</th></tr>"#
        );
        for r in rows.iter().take(50) {
            let label = r["endpoint"]
                .as_str()
                .or(r["step"].as_str())
                .or(r["url"].as_str())
                .unwrap_or("?");
            let _ = write!(
                out,
                "<tr><td><code>{}</code></td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                esc(label),
                count(r["requests"].as_f64().unwrap_or(0.0)),
                count(r["failed_requests"].as_f64().unwrap_or(0.0)),
                count(r["requests_per_second"].as_f64().unwrap_or(0.0)),
                esc(&ms(r["median_ms"].as_f64().unwrap_or(0.0))),
                esc(&ms(r["p99_ms"].as_f64().unwrap_or(0.0)))
            );
        }
        out.push_str("</table></section>");
    }
    if let Some(slow) = s["slowest_requests"].as_array().filter(|s| !s.is_empty()) {
        let id = |r: &Value| {
            r["request_id"]
                .as_str()
                .or(r["trace_id"].as_str())
                .unwrap_or("")
                .to_string()
        };
        // A column of request ids only when the server gave some
        let ids = slow.iter().take(10).any(|r| !id(r).is_empty());
        out.push_str(r#"<section><h2>Slowest requests</h2><table><tr><th>at</th><th>latency</th><th>status</th>"#);
        if ids {
            out.push_str("<th>request id</th>");
        }
        out.push_str("</tr>");
        for r in slow.iter().take(10) {
            let _ = write!(
                out,
                "<tr><td>+{}</td><td>{}</td><td>{}</td>",
                esc(&span(r["at_s"].as_f64().unwrap_or(0.0))),
                esc(&ms(r["latency_ms"].as_f64().unwrap_or(0.0))),
                r["status"]
                    .as_u64()
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "—".into()),
            );
            if ids {
                let _ = write!(out, "<td><code>{}</code></td>", esc(&id(r)));
            }
            out.push_str("</tr>");
        }
        out.push_str("</table></section>");
    }
    out
}

/// Where a request's time went, as one bar: waiting for the first byte,
/// connecting (spread over the requests that reused a connection), reading
fn phases(p: &Value, total: f64) -> Option<String> {
    let wait = p["first_byte"]["median_ms"].as_f64()?;
    let read = p["download"]["median_ms"].as_f64().unwrap_or(0.0);
    let connect = match (
        p["connect"]["median_ms"].as_f64(),
        p["connect"]["count"].as_f64(),
    ) {
        (Some(m), Some(n)) if total > 0.0 => m * n / total,
        _ => 0.0,
    };
    let sum = wait + read + connect;
    if sum <= 0.0 {
        return None;
    }
    let mut out = String::from(r#"<h2>Where the time went</h2><div class="split">"#);
    let parts = [
        ("wait", wait, "ember"),
        ("connect", connect, "violet"),
        ("read", read, "teal"),
    ];
    for (name, v, class) in parts {
        if v / sum >= 0.002 {
            let _ = write!(
                out,
                r#"<div class="{class}" style="width:{:.1}%" title="{name} {:.0}%"></div>"#,
                v / sum * 100.0,
                v / sum * 100.0
            );
        }
    }
    out.push_str(r#"</div><div class="legend">"#);
    for (name, v, class) in parts {
        if v / sum >= 0.002 {
            let _ = write!(
                out,
                r#"<span class="key"><i class="sw {class}"></i>{name} {:.0}% · {}</span>"#,
                v / sum * 100.0,
                esc(&ms(v))
            );
        }
    }
    out.push_str("</div>");
    Some(out)
}

fn ramp_body(report: &Value) -> String {
    let mut out = String::new();
    let steps = report["steps"].as_array().cloned().unwrap_or_default();
    let holds = report["holds_concurrency"].as_u64();
    let capacity = report["capacity"]["requests_per_second"].as_f64();
    let mut story = format!(
        "{} requests over {} steps in {}.",
        count(report["requests"].as_f64().unwrap_or(0.0)),
        steps.len(),
        span(report["seconds"].as_f64().unwrap_or(0.0))
    );
    match holds {
        Some(c) => {
            let _ = write!(story, " It held {c} at a time.");
        }
        None => story.push_str(" No step held."),
    }
    if let Some(rps) = capacity {
        let _ = write!(story, " Capacity about {} req/s.", count(rps));
    }
    let _ = write!(
        out,
        r#"<p class="story">{}</p><div class="kpis">"#,
        esc(&story)
    );
    out.push_str(&kpi(
        "Held",
        &holds
            .map(|c| format!("{c} at once"))
            .unwrap_or_else(|| "none".into()),
        "",
    ));
    out.push_str(&kpi(
        "Capacity",
        &capacity
            .map(|r| format!("{}/s", count(r)))
            .unwrap_or_else(|| "not yet bent".into()),
        "",
    ));
    out.push_str(&kpi("Steps", &steps.len().to_string(), ""));
    out.push_str(&kpi(
        "Requests",
        &count(report["requests"].as_f64().unwrap_or(0.0)),
        "",
    ));
    out.push_str("</div>");
    let at = |s: &Value| s["concurrency"].as_f64().unwrap_or(0.0);
    let rps: Vec<(f64, f64)> = steps
        .iter()
        .map(|s| (at(s), s["requests_per_second"].as_f64().unwrap_or(0.0)))
        .collect();
    let p99: Vec<(f64, f64)> = steps
        .iter()
        .map(|s| (at(s), s["p99_ms"].as_f64().unwrap_or(0.0)))
        .collect();
    if rps.len() >= 2 {
        out.push_str(r#"<section><h2>As the concurrency rose</h2>"#);
        out.push_str(&legend(&[("ember", "requests a second")]));
        out.push_str(&chart_by(
            &rps,
            "ember",
            |v| format!("{}/s", crate::ui::format::compact(v)),
            "Requests a second at each concurrency",
            true,
        ));
        out.push_str(&legend(&[("violet", "p99")]));
        out.push_str(&chart_by(
            &p99,
            "violet",
            ms,
            "p99 at each concurrency",
            false,
        ));
        out.push_str("</section>");
    }
    out.push_str(r#"<section><h2>Steps</h2><table><tr><th>concurrency</th><th>req/s</th><th>p50</th><th>p99</th><th>errors</th></tr>"#);
    for s in &steps {
        let errors = s["error_rate_percent"].as_f64().unwrap_or(0.0);
        let _ = write!(
            out,
            r#"<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td class="{}">{errors:.2}%</td></tr>"#,
            at(s),
            count(s["requests_per_second"].as_f64().unwrap_or(0.0)),
            esc(&ms(s["median_ms"].as_f64().unwrap_or(0.0))),
            esc(&ms(s["p99_ms"].as_f64().unwrap_or(0.0))),
            if errors >= 1.0 { "bad" } else { "" }
        );
    }
    out.push_str("</table></section>");
    let findings = report["findings"].as_array().cloned().unwrap_or_default();
    if !findings.is_empty() {
        out.push_str(r#"<section><h2>What pepe found</h2><ul class="found">"#);
        for f in findings {
            let detail = f["detail"]
                .as_str()
                .map(|d| format!(" · {}", esc(d)))
                .unwrap_or_default();
            let _ = write!(
                out,
                r#"<li><i class="d {}"></i><b>{}</b>{detail}</li>"#,
                dot(f["level"].as_str().unwrap_or("")),
                esc(f["title"].as_str().or(f["text"].as_str()).unwrap_or(""))
            );
        }
        out.push_str("</ul></section>");
    }
    out
}

/// A line over the concurrency of each step, its axis the concurrency
fn chart_by(
    values: &[(f64, f64)],
    class: &str,
    label: fn(f64) -> String,
    aria: &str,
    area: bool,
) -> String {
    let series = [Series {
        label: aria,
        class,
        values: values.to_vec(),
        area,
    }];
    chart(&series, &[], label, |c| format!("{c:.0} at once"), aria)
}

fn page(title: &str, body: &str) -> String {
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>{}</title>
<meta name="generator" content="pepe {}">
<style>{CSS}</style></head>
<body><main>{body}</main></body></html>
"#,
        esc(title),
        crate::utils::version()
    )
}

const CSS: &str = r#"
:root{--ground:#fffaf5;--surface:#ffffff;--tile:#fbf1e8;--text:#2a1d17;--label:#7d6a5e;--line:#eadfd6;--ember:#e8702a;--violet:#6b62d6;--teal:#14866a;--good:#2e8a3a;--warn:#b7791f;--bad:#d1334f;--faint:#a99a90}
@media (prefers-color-scheme:dark){:root{--ground:#140f0d;--surface:#1d1613;--tile:#261d19;--text:#f3e8de;--label:#9c8a7e;--line:#33271f;--ember:#ff8a3d;--violet:#9d95ff;--teal:#4fd1a8;--good:#9ccf6a;--warn:#f0cf5a;--bad:#ff5a6e;--faint:#6e5f56}}
*{box-sizing:border-box}html{background:var(--ground)}
body{margin:0;color:var(--text);font:15px/1.55 ui-sans-serif,system-ui,-apple-system,"Segoe UI",Roboto,sans-serif;-webkit-font-smoothing:antialiased}
main{max-width:920px;margin:32px auto;padding:28px 32px;background:var(--surface);border:1px solid var(--line);border-radius:16px}
header{display:flex;gap:18px;align-items:center}
.face{width:72px;height:72px;border-radius:16px;image-rendering:pixelated;flex:none}
.title{min-width:0}.title .row{display:flex;gap:12px;align-items:center;flex-wrap:wrap}
h1{font-size:24px;line-height:1.2;margin:0;font-weight:600;overflow-wrap:anywhere}
h2{font-size:13px;font-weight:600;letter-spacing:.02em;color:var(--label);margin:26px 0 10px;text-transform:uppercase}
.about{color:var(--label);font:12.5px/1.5 ui-monospace,SFMono-Regular,Menlo,monospace;margin-top:4px;overflow-wrap:anywhere}
.pill{font-size:12.5px;font-weight:600;padding:3px 12px;border-radius:999px;border:1px solid currentColor}
.story{font-size:16px;margin:20px 0 4px;max-width:70ch}
.kpis{display:grid;grid-template-columns:repeat(auto-fit,minmax(118px,1fr));gap:10px;margin:18px 0 6px}
.kpi{background:var(--tile);border-radius:10px;padding:11px 14px}.kpi .l{font-size:12px;color:var(--label)}.kpi .v{font-size:22px;font-weight:600;font-variant-numeric:tabular-nums}
.two{display:grid;grid-template-columns:minmax(0,1fr) minmax(0,1fr);gap:28px}
@media (max-width:720px){main{margin:0;border-radius:0;padding:20px 16px}.two{grid-template-columns:1fr;gap:0}}
.chart{width:100%;height:auto;display:block;margin:2px 0 8px}
.grid{stroke:var(--line);stroke-width:1}.axis{fill:var(--label);font:10.5px ui-monospace,Menlo,monospace}
.line{fill:none;stroke-width:2;stroke-linejoin:round}.area{stroke:none;opacity:.14}
.line.ember{stroke:var(--ember)}.area.ember{fill:var(--ember)}.line.bad{stroke:var(--bad)}.line.violet{stroke:var(--violet)}.line.teal{stroke:var(--teal)}
.mark{stroke:var(--bad);stroke-width:1;stroke-dasharray:3 3}.dot{fill:var(--bad)}.marktext{fill:var(--bad);font-size:11px}
.legend{display:flex;gap:16px;flex-wrap:wrap;font-size:12px;color:var(--label);margin:4px 0}
.key{display:inline-flex;gap:6px;align-items:center}.sw{width:10px;height:10px;border-radius:3px;display:inline-block}
.sw.ember{background:var(--ember)}.sw.bad{background:var(--bad)}.sw.violet{background:var(--violet)}.sw.teal{background:var(--teal)}
.ladder{display:grid;grid-template-columns:38px minmax(0,1fr) 78px;gap:10px;align-items:center;font-size:13px;margin:6px 0}
.ladder .num{text-align:right;font-variant-numeric:tabular-nums}
.track{height:10px;border-radius:4px;background:var(--tile)}.fill{height:100%;border-radius:4px}
.fill.ember{background:var(--ember)}.fill.bad{background:var(--bad)}.fill.faint{background:var(--faint)}
.split{display:flex;height:14px;border-radius:5px;overflow:hidden;background:var(--tile)}
.split .ember{background:var(--ember)}.split .violet{background:var(--violet)}.split .teal{background:var(--teal)}
ul{margin:0;padding:0;list-style:none}.checks li,.found li,.changes li{margin:5px 0;font-size:14px}
.changes li::before{content:"● ";color:var(--bad)}
.d{width:9px;height:9px;border-radius:50%;display:inline-block;margin-right:8px}.d.good{background:var(--good)}.d.warn{background:var(--warn)}.d.bad{background:var(--bad)}
.good{color:var(--good)}.warn{color:var(--warn)}.bad{color:var(--bad)}.faint{color:var(--faint)}
.pill.good{color:var(--good)}.pill.warn{color:var(--warn)}.pill.bad{color:var(--bad)}
table{width:100%;border-collapse:collapse;font-size:13px;font-variant-numeric:tabular-nums}
th{color:var(--label);font-weight:500;text-align:left;padding:6px 8px;border-bottom:1px solid var(--line)}
td{padding:6px 8px;border-bottom:1px solid var(--line);overflow-wrap:anywhere}
code{font:12.5px ui-monospace,SFMono-Regular,Menlo,monospace;background:var(--tile);padding:1px 5px;border-radius:5px}
.chips{display:flex;gap:8px;flex-wrap:wrap}.chip{background:var(--tile);border-radius:999px;padding:3px 12px;font-size:13px}
footer{margin-top:30px;padding-top:14px;border-top:1px solid var(--line);display:flex;justify-content:space-between;gap:12px;flex-wrap:wrap;font-size:12px;color:var(--label)}
footer code{overflow-wrap:anywhere}a{color:var(--ember)}
@media print{html{background:#fff}main{border:0;margin:0}}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report() -> Value {
        json!({
            "schema_version": 1, "test": "checkout",
            "command": "pepe --load checkout --html out.html",
            "target": {"mode": "run", "method": "POST", "url": "https://api.acme.com/checkout", "concurrency": 50},
            "summary": {
                "total_requests": 4380000, "failed_requests": 18396, "requests_per_second": 2433.0, "duration_ms": 1800000.0,
                "interrupted": false,
                "latency": {"median_ms": 41.0, "p90_ms": 88.0, "p95_ms": 118.0, "p99_ms": 342.0, "max_ms": 1890.0,
                    "phases": {"first_byte": {"median_ms": 38.0}, "download": {"median_ms": 0.5}, "connect": {"median_ms": 40.0, "count": 500}}},
                "status_codes": {"200": 4361604, "503": 18396},
                "failures": [{"cause": "HTTP 503", "count": 18396, "example_body": "upstream <connect> error"}],
                "anomalies": [{"at_s": 660.0, "kind": "latency", "text": "p99 jumped 4.5× to 342ms at 11m"}],
                "slowest_requests": [{"at_s": 723.0, "latency_ms": 1890.0, "status": 503, "request_id": "req-8f2a"}]
            },
            "verdict": {"level": "degraded", "findings": [{"level": "failing", "text": "0.4% failed"}, {"level": "healthy", "text": "Steady throughput"}]},
            "timeline": [
                {"at_s": 60.0, "requests_per_second": 2400.0, "errors_per_second": 0.0, "p50_ms": 40.0, "p90_ms": 80.0, "p99_ms": 240.0},
                {"at_s": 660.0, "requests_per_second": 1900.0, "errors_per_second": 30.0, "p50_ms": 44.0, "p90_ms": 120.0, "p99_ms": 342.0},
                {"at_s": 1800.0, "requests_per_second": 2450.0, "errors_per_second": 0.0, "p50_ms": 41.0, "p90_ms": 85.0, "p99_ms": 230.0}
            ]
        })
    }

    #[test]
    fn a_run_page_says_it_all_and_escapes_what_it_quotes() {
        let extras = Extras {
            checks: vec![Check {
                text: "p99 > 300ms".into(),
                held: false,
                measured: "p99 was 342ms".into(),
            }],
            host: "ci-runner-3".into(),
            ..Extras::default()
        };
        let page = render(&report(), &extras);
        assert!(page.starts_with("<!doctype html>"));
        assert!(
            page.contains("Failed · p99 &gt; 300ms"),
            "the verdict names the crossed threshold"
        );
        assert!(
            page.contains("data:image/png;base64,"),
            "Pepe is in the page"
        );
        assert!(page.contains("4,380,000 requests"));
        assert!(
            page.contains("<svg class=\"chart\""),
            "the curves are drawn"
        );
        assert!(page.contains("p99 jumped 4.5×"), "what changed is marked");
        assert!(
            page.contains("upstream &lt;connect&gt; error"),
            "bodies are escaped"
        );
        assert!(!page.contains("upstream <connect>"));
        assert!(page.contains("req-8f2a"));
        assert!(page.contains("Where the time went"));
        assert!(!page.contains("<script"), "nothing runs");
        assert!(!page.contains("http://") || page.contains("https://pepe.mhaimdat.com/"));
    }

    #[test]
    fn a_ramp_page_draws_its_steps() {
        let ramp = json!({
            "schema_version": 1, "command": "pepe ramp https://a.io", "ended": "completed",
            "holds_concurrency": 40, "capacity": {"requests_per_second": 3000.0, "concurrency": 32.0, "extrapolated": false},
            "requests": 90000, "failed_requests": 0, "seconds": 60.0,
            "findings": [{"level": "healthy", "title": "Held to 40", "detail": "p99 under 100ms"}],
            "steps": [
                {"concurrency": 10, "requests_per_second": 1000.0, "median_ms": 9.0, "p99_ms": 20.0, "error_rate_percent": 0.0},
                {"concurrency": 20, "requests_per_second": 2000.0, "median_ms": 9.5, "p99_ms": 25.0, "error_rate_percent": 0.0},
                {"concurrency": 40, "requests_per_second": 2900.0, "median_ms": 13.0, "p99_ms": 60.0, "error_rate_percent": 0.0}
            ]
        });
        let page = render(&ramp, &Extras::default());
        assert!(page.contains("<h1>ramp · a.io</h1>"), "titled by its URL");
        assert!(page.contains("It held 40 at a time."));
        assert!(page.contains("Capacity about 3,000 req/s."));
        assert!(page.contains("Held to 40"), "a ramp's findings are titled");
        assert!(page.contains("<svg class=\"chart\""));
        assert!(page.contains(">Passed<"));
    }
}
