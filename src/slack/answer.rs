//! What `@pepe` says when asked in a run's thread: a short answer, from the
//! run's report as it stands (or as it ended), to the question as people
//! put it: "p99?", "why are errors up?", "slowest", "compare", "where does
//! the time go". One reply per question; the thread stays quiet otherwise.

use std::time::Duration;

use serde_json::Value;

fn ms(value: f64) -> String {
    crate::ui::format::latency(Duration::from_secs_f64(value.max(0.0) / 1e3))
}

fn count(n: f64) -> String {
    crate::ui::format::count(n.max(0.0).round() as u64)
}

fn span(seconds: f64) -> String {
    crate::ui::format::span(Duration::from_secs_f64(seconds.max(0.0)))
}

/// What the question is about
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Topic {
    Latency,
    Errors,
    Slowest,
    Compare,
    Time,
    Codes,
    Help,
    Status,
}

pub fn topic(question: &str) -> Topic {
    let q = question.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| q.contains(w));
    if has(&["help", "what can", "commands"]) {
        Topic::Help
    } else if has(&["slowest", "worst request", "slow request"]) {
        Topic::Slowest
    } else if has(&[
        "compare",
        "last run",
        "before",
        "yesterday",
        "previous",
        "regress",
    ]) {
        Topic::Compare
    } else if has(&[
        "error", "fail", "5xx", "4xx", "timeout", "broken", "503", "500", "502", "504",
    ]) {
        Topic::Errors
    } else if has(&[
        "where",
        "phase",
        "time go",
        "spend",
        "tls",
        "connect",
        "ttfb",
        "first byte",
    ]) {
        Topic::Time
    } else if has(&["code", "status code", "answers"]) {
        Topic::Codes
    } else if has(&["p99", "p95", "p50", "latency", "median", "slow", "fast"]) {
        Topic::Latency
    } else {
        Topic::Status
    }
}

/// The answer to `question` from `report`; `compared` is the test's last
/// recorded run against this one, when there is one
pub fn answer(
    question: &str,
    report: &Value,
    compared: Option<&(crate::compare::Comparison, String)>,
) -> String {
    let s = &report["summary"];
    if !s.is_object() {
        // A ramp's report, or anything else: its findings say it
        let findings: Vec<String> = report["findings"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|f| {
                f["title"]
                    .as_str()
                    .or(f["text"].as_str())
                    .map(str::to_string)
            })
            .collect();
        return if findings.is_empty() {
            "Nothing to say yet.".into()
        } else {
            findings.join(" · ")
        };
    }
    let total = s["total_requests"].as_f64().unwrap_or(0.0);
    let failed = s["failed_requests"].as_f64().unwrap_or(0.0);
    let lat = &s["latency"];
    let anomalies: Vec<&Value> = s["anomalies"]
        .as_array()
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    match topic(question) {
        Topic::Help => "Ask me here: *p99* (latency), *errors* (what failed and what it said), *slowest* (the slowest requests), *where does the time go*, *codes* (the answers), *compare* (against the last run), or *status*.".into(),
        Topic::Latency => {
            let mut out = format!(
                "⏱️ p50 {} · p90 {} · p95 {} · p99 {} · max {}.",
                ms(lat["median_ms"].as_f64().unwrap_or(0.0)),
                ms(lat["p90_ms"].as_f64().unwrap_or(0.0)),
                ms(lat["p95_ms"].as_f64().unwrap_or(0.0)),
                ms(lat["p99_ms"].as_f64().unwrap_or(0.0)),
                ms(lat["max_ms"].as_f64().unwrap_or(0.0))
            );
            if let Some(a) = anomalies.iter().find(|a| a["kind"] == "latency") {
                if let Some(text) = a["text"].as_str() {
                    out.push_str(&format!(" It moved: {text}."));
                }
            } else {
                out.push_str(" Steady so far: no jump in p99.");
            }
            out
        }
        Topic::Errors => {
            if failed == 0.0 {
                return format!("✅ None failed: {} requests, all answered.", count(total));
            }
            let mut out = format!(
                "🔥 {} failed of {} ({:.2}%).",
                count(failed),
                count(total),
                failed * 100.0 / total.max(1.0)
            );
            for f in s["failures"].as_array().into_iter().flatten().take(3) {
                out.push_str(&format!(
                    "\n• *{}* ×{}",
                    f["cause"].as_str().unwrap_or("?"),
                    count(f["count"].as_f64().unwrap_or(0.0))
                ));
                if let Some(body) = f["example_body"].as_str().filter(|b| !b.trim().is_empty()) {
                    let body: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
                    out.push_str(&format!(": `{}`", crate::webhook::layout_cut(&body, 140).replace('`', "'")));
                }
            }
            if let Some(a) = anomalies.iter().find(|a| a["kind"] == "errors") {
                if let Some(text) = a["text"].as_str() {
                    out.push_str(&format!("\nIt began: {text}."));
                }
            }
            out
        }
        Topic::Slowest => {
            let slow: Vec<String> = s["slowest_requests"]
                .as_array()
                .into_iter()
                .flatten()
                .take(5)
                .map(|r| {
                    let mut line = format!(
                        "{} ({}",
                        ms(r["latency_ms"].as_f64().unwrap_or(0.0)),
                        r["status"].as_u64().map(|s| s.to_string()).unwrap_or_else(|| "no answer".into())
                    );
                    if let Some(id) = r["request_id"].as_str() {
                        line.push_str(&format!(", `{id}`"));
                    }
                    line.push_str(&format!(", +{})", span(r["at_s"].as_f64().unwrap_or(0.0))));
                    line
                })
                .collect();
            if slow.is_empty() {
                "No slow requests kept yet.".into()
            } else {
                format!("🐢 The {} slowest: {}", slow.len(), slow.join(" · "))
            }
        }
        Topic::Compare => match compared {
            Some((c, when)) => {
                let moved: Vec<String> = c
                    .changes
                    .iter()
                    .filter_map(|ch| {
                        let pct = ch.change_percent?;
                        let fmt = |v: f64| {
                            if ch.metric.ends_with("_ms") {
                                ms(v)
                            } else if ch.metric.contains("per_second") {
                                count(v)
                            } else {
                                format!("{v:.2}")
                            }
                        };
                        Some(format!(
                            "{} {}{:.0}% ({} → {})",
                            crate::compare::short_name(ch.metric),
                            if pct >= 0.0 { "+" } else { "−" },
                            pct.abs(),
                            fmt(ch.before),
                            fmt(ch.after)
                        ))
                    })
                    .collect();
                format!(
                    "{} against the last run ({when}): {}.",
                    c.verdict.headline(),
                    if moved.is_empty() { "nothing moved".into() } else { moved.join(" · ") }
                )
            }
            None => "There's no recorded run of this test to compare with yet.".into(),
        },
        Topic::Time => {
            let p = &lat["phases"];
            let wait = p["first_byte"]["median_ms"].as_f64().unwrap_or(0.0);
            let read = p["download"]["median_ms"].as_f64().unwrap_or(0.0);
            let connect = p["connect"]["median_ms"].as_f64();
            let mut out = format!(
                "⏳ A request waits {} for its first byte and reads for {}",
                ms(wait),
                ms(read)
            );
            if let (Some(c), Some(n)) = (connect, p["connect"]["count"].as_f64()) {
                out.push_str(&format!(
                    "; {} new connections took {} each",
                    count(n),
                    ms(c)
                ));
            }
            out.push_str(if wait > 4.0 * read.max(0.001) {
                ". The time is the server's: it's in the wait."
            } else {
                "."
            });
            out
        }
        Topic::Codes => {
            let mut codes: Vec<(String, f64)> = s["status_codes"]
                .as_object()
                .map(|o| o.iter().map(|(k, v)| (k.clone(), v.as_f64().unwrap_or(0.0))).collect())
                .unwrap_or_default();
            codes.sort_by(|a, b| b.1.total_cmp(&a.1));
            if codes.is_empty() {
                "No answers yet.".into()
            } else {
                codes.iter().map(|(k, n)| format!("{k} ×{}", count(*n))).collect::<Vec<_>>().join(" · ")
            }
        }
        Topic::Status => {
            let mut out = format!(
                "📊 {} requests in {} at {} req/s · p50 {} · p99 {} · {:.2}% failed.",
                count(total),
                span(s["duration_ms"].as_f64().unwrap_or(0.0) / 1e3),
                count(s["requests_per_second"].as_f64().unwrap_or(0.0)),
                ms(lat["median_ms"].as_f64().unwrap_or(0.0)),
                ms(lat["p99_ms"].as_f64().unwrap_or(0.0)),
                failed * 100.0 / total.max(1.0)
            );
            if !anomalies.is_empty() {
                out.push_str(&format!(" {} change{} so far.", anomalies.len(), if anomalies.len() == 1 { "" } else { "s" }));
            }
            out
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn report() -> Value {
        json!({"summary": {
            "total_requests": 1000, "failed_requests": 12, "requests_per_second": 250.0, "duration_ms": 4000.0,
            "latency": {"median_ms": 10.0, "p90_ms": 20.0, "p95_ms": 30.0, "p99_ms": 80.0, "max_ms": 300.0,
                "phases": {"first_byte": {"median_ms": 9.0}, "download": {"median_ms": 0.5}, "connect": {"median_ms": 3.0, "count": 8}}},
            "status_codes": {"200": 988, "503": 12},
            "failures": [{"cause": "HTTP 503", "count": 12, "example_body": "upstream   connect `error`"}],
            "anomalies": [{"at_s": 20.0, "kind": "errors", "text": "errors rose to 12% at 20s"}],
            "slowest_requests": [{"at_s": 21.0, "latency_ms": 300.0, "status": 503, "request_id": "req-1"}]
        }})
    }

    #[test]
    fn questions_find_their_topic() {
        assert_eq!(topic("why are errors up?"), Topic::Errors);
        assert_eq!(topic("p99?"), Topic::Latency);
        assert_eq!(topic("what are the slowest"), Topic::Slowest);
        assert_eq!(topic("compare with yesterday"), Topic::Compare);
        assert_eq!(topic("where does the time go"), Topic::Time);
        assert_eq!(topic("how is it going"), Topic::Status);
        assert_eq!(topic("help"), Topic::Help);
    }

    #[test]
    fn answers_say_what_the_report_says() {
        let r = report();
        let errors = answer("errors?", &r, None);
        assert!(
            errors.starts_with("🔥 12 failed of 1,000 (1.20%)."),
            "{errors}"
        );
        assert!(
            errors.contains("*HTTP 503* ×12: `upstream connect 'error'`"),
            "{errors}"
        );
        assert!(
            errors.contains("It began: errors rose to 12% at 20s."),
            "{errors}"
        );
        assert!(answer("p99", &r, None).contains("p99 80.00ms"));
        assert!(
            answer("slowest", &r, None).contains(" (503, `req-1`, +21s)"),
            "{}",
            answer("slowest", &r, None)
        );
        assert!(answer("where does the time go", &r, None).contains("it's in the wait"));
        assert!(answer("compare", &r, None).contains("no recorded run"));
        assert_eq!(answer("codes", &r, None), "200 ×988 · 503 ×12");
    }
}
