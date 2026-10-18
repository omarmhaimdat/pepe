//! Slack's layouts: the live message, the verdict it becomes, the lines in
//! its thread, and Pepe's face for each. Everything a channel needs to know
//! without opening a terminal: how far the run is, how it's going and
//! where it's heading (the sparklines), and at the end what happened, why,
//! against what thresholds, and against the last run of the same test.

use serde_json::{json, Value};

use super::{Message, Progress, Run, Tone};

/// Where Pepe's faces are: the site's images (site/build-avatars.py)
const FACES: &str = "https://pepe.mhaimdat.com/img";

/// The colours of the bar down a message's side
const RUNNING: &str = "#ff8a3d";
const PASSED: &str = "#2eb67d";
const DEGRADED: &str = "#ecb22e";
const PAUSED: &str = "#8a8578";
const FAILED: &str = "#e01e5a";

/// Pepe in `mood`, as an image Slack can show: a run's face
pub fn face(mood: &str) -> String {
    format!("{FACES}/pepe-{mood}.png")
}

fn ms(value: f64) -> String {
    crate::ui::format::latency(std::time::Duration::from_secs_f64(value.max(0.0) / 1e3))
}

fn count(n: u64) -> String {
    crate::ui::format::count(n)
}

fn span(d: std::time::Duration) -> String {
    crate::ui::format::span(d)
}

/// `▁▂▄▆█`: a series in eight heights, its own lowest to its highest
pub fn spark(values: &[f64]) -> String {
    const TICKS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let values: Vec<f64> = values.iter().copied().filter(|v| v.is_finite()).collect();
    let (Some(low), Some(high)) = (
        values.iter().copied().reduce(f64::min),
        values.iter().copied().reduce(f64::max),
    ) else {
        return String::new();
    };
    values
        .iter()
        .map(|v| {
            if (high - low).abs() < f64::EPSILON {
                TICKS[3]
            } else {
                TICKS[(((v - low) / (high - low)) * 7.0).round() as usize]
            }
        })
        .collect()
}

/// At most `most` points: the series squeezed by averaging neighbours
fn squeeze(values: &[f64], most: usize) -> Vec<f64> {
    if values.len() <= most {
        return values.to_vec();
    }
    (0..most)
        .map(|i| {
            let from = i * values.len() / most;
            let to = ((i + 1) * values.len() / most).max(from + 1);
            values[from..to].iter().sum::<f64>() / (to - from) as f64
        })
        .collect()
}

/// `▰▰▰▱▱▱▱▱▱▱`
pub(super) fn bar(done: f64) -> String {
    let filled = (done.clamp(0.0, 1.0) * 10.0).round() as usize;
    format!("{}{}", "▰".repeat(filled), "▱".repeat(10 - filled))
}

/// `POST api.acme.com/checkout`: the method and the URL without its scheme
fn short_target(run: &Run) -> String {
    run.target
        .replace("https://", "")
        .replace("http://", "")
        .trim_end_matches('/')
        .to_string()
}

/// What the message is about: the saved test, else what was sent
fn name(run: &Run) -> String {
    run.test.clone().unwrap_or_else(|| short_target(run))
}

/// The line under the title: what, how hard, where from, by whom
fn about(run: &Run) -> String {
    let mut parts = vec![format!("`{}`", short_target(run))];
    if run.concurrency > 0 && run.mode != "ping" && run.mode != "ramp" {
        parts.push(format!("{} at a time", run.concurrency));
    }
    match run.planned {
        Some(planned) => parts.push(span(planned)),
        None => parts.push(run.load.clone()),
    }
    match (&run.by, run.host.is_empty(), run.who.is_empty()) {
        (Some(by), false, _) => parts.push(format!("from Slack by <@{by}> · on {}", run.host)),
        (Some(by), true, _) => parts.push(format!("from Slack by <@{by}>")),
        (None, false, false) => parts.push(format!("on {} by {}", run.host, run.who)),
        (None, false, true) => parts.push(format!("on {}", run.host)),
        _ => {}
    }
    if run.mode != "run" {
        parts.insert(0, run.mode.clone());
    }
    parts.join(" · ")
}

/// A Slack date that reads in each viewer's own time
fn now_token(prefix: &str) -> String {
    let unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("<!date^{unix}^{prefix} {{time_secs}}|{prefix}>")
}

fn header(text: String) -> Value {
    json!({"type": "header", "text": {"type": "plain_text", "text": super::cut(&text, 150), "emoji": true}})
}

fn context(text: String) -> Value {
    json!({"type": "context", "elements": [{"type": "mrkdwn", "text": super::cut(&text, 3000)}]})
}

fn section(text: String) -> Value {
    json!({"type": "section", "text": {"type": "mrkdwn", "text": super::cut(&text, 3000)}})
}

fn fields(pairs: &[(String, String)]) -> Value {
    let fields: Vec<Value> = pairs
        .iter()
        .take(10)
        .map(|(k, v)| json!({"type": "mrkdwn", "text": super::cut(&format!("*{k}*\n{v}"), 2000)}))
        .collect();
    json!({"type": "section", "fields": fields})
}

fn button(text: &str, action: &str, value: &str, style: Option<&str>) -> Value {
    let mut b = json!({
        "type": "button",
        "text": {"type": "plain_text", "text": text, "emoji": true},
        "action_id": action,
        "value": value,
    });
    if let Some(style) = style {
        b["style"] = json!(style);
    }
    b
}

/// The run's controls, while a run `pepe slack serve` started goes:
/// pause or resume, one less or one more in flight, stop
fn controls(run: &Run, progress: Option<&Progress>) -> Option<Value> {
    let id = run.served.as_ref()?;
    let paused = progress.is_some_and(|p| p.paused);
    let mut elements = vec![if paused {
        button("▶ Resume", "pepe:resume", id, Some("primary"))
    } else {
        button("⏸ Pause", "pepe:pause", id, None)
    }];
    // A ramp sets its own concurrency
    if run.mode != "ramp" {
        let now = progress
            .and_then(|p| p.concurrency)
            .unwrap_or(run.concurrency)
            .max(1);
        let less = now - 1;
        let more = now + 1;
        if less >= 1 {
            elements.push(button(
                &format!("− 1  ({less})"),
                "pepe:size",
                &format!("{id}:{less}"),
                None,
            ));
        }
        elements.push(button(
            &format!("+ 1  ({more})"),
            "pepe:size",
            &format!("{id}:{more}"),
            None,
        ));
    }
    let mut stop = button("■ Stop", "pepe:stop", id, Some("danger"));
    stop["confirm"] = json!({
        "title": {"type": "plain_text", "text": "Stop this run?"},
        "text": {"type": "mrkdwn", "text": "It ends now, and the message says what it measured."},
        "confirm": {"type": "plain_text", "text": "Stop"},
        "deny": {"type": "plain_text", "text": "Keep it going"},
        "style": "danger",
    });
    elements.push(stop);
    Some(json!({"type": "actions", "elements": elements}))
}

/// Run again, Edit and run, History: a saved test's verdict, when
/// `pepe slack serve` is set up to answer them
fn verdict_buttons(run: &Run) -> Option<Value> {
    let test = run.test.as_ref().filter(|_| run.buttons)?;
    Some(json!({"type": "actions", "elements": [
        button("↻ Run again", "pepe:run", test, Some("primary")),
        button("✎ Edit and run", "pepe:edit", test, None),
        button("History", "pepe:history", test, None),
    ]}))
}

fn message(text: String, color: &str, blocks: Vec<Value>, mood: &str) -> Value {
    json!({
        "text": text,
        "attachments": [{"color": color, "blocks": blocks, "fallback": text}],
        "icon_url": face(mood),
    })
}

/// Pepe's face for how a run is going
fn live_mood(p: Option<&Progress>) -> &'static str {
    let Some(p) = p else { return "waiting" };
    let errors = if p.requests == 0 {
        0.0
    } else {
        p.failed as f64 / p.requests as f64
    };
    if errors > 0.1 {
        "on-fire"
    } else if errors > 0.01 || p.anomalies > 0 {
        "sweating"
    } else {
        "happy"
    }
}

/// The live message: posted at the start, edited as the run goes
pub(super) fn live(m: &Message) -> Value {
    let run = &m.detail.run;
    let progress = m.detail.progress.as_ref();
    let mut blocks = Vec::new();
    let Some(p) = progress else {
        blocks.push(header(format!("🚀 Starting · {}", name(run))));
        blocks.push(context(about(run)));
        blocks.push(section(format!("_{}_", run.load)));
        blocks.extend(controls(run, None));
        blocks.push(context(format!(
            "{} · pepe {}",
            now_token("started"),
            crate::utils::version()
        )));
        return message(
            format!("🚀 Starting {}", name(run)),
            RUNNING,
            blocks,
            live_mood(None),
        );
    };
    // The concurrency as it stands, when it was changed from Slack
    let now = Run {
        concurrency: p.concurrency.unwrap_or(run.concurrency),
        ..run.clone()
    };
    blocks.push(header(if p.paused {
        format!("⏸ Paused · {}", name(run))
    } else {
        format!("⏳ Running · {}", name(run))
    }));
    blocks.push(context(about(&now)));
    let progress_line = match (p.done, run.planned) {
        (Some(done), Some(planned)) => format!(
            "`{}`  *{:.0}%* · {} of {} · {} left",
            bar(done),
            done * 100.0,
            span(p.elapsed),
            span(planned),
            span(planned.saturating_sub(p.elapsed))
        ),
        (Some(done), None) => format!(
            "`{}`  *{:.0}%* · {} in",
            bar(done),
            done * 100.0,
            span(p.elapsed)
        ),
        _ => format!("{} in", span(p.elapsed)),
    };
    blocks.push(section(progress_line));
    let rps: Vec<f64> = p.series.iter().map(|s| s.0).collect();
    let p99: Vec<f64> = p.series.iter().map(|s| s.1).collect();
    let errors = if p.requests == 0 {
        0.0
    } else {
        p.failed as f64 * 100.0 / p.requests as f64
    };
    let causes: Vec<String> = p
        .failures
        .iter()
        .take(3)
        .map(|(cause, n)| format!("{cause} ×{}", count(*n)))
        .collect();
    blocks.push(fields(&[
        (
            "Throughput".into(),
            format!(
                "{} req/s  `{}`",
                count(p.rps.round() as u64),
                spark(&squeeze(&rps, 16))
            ),
        ),
        (
            "Latency p50 · p99".into(),
            format!(
                "{} · {}  `{}`",
                ms(p.p50.as_secs_f64() * 1e3),
                ms(p.p99.as_secs_f64() * 1e3),
                spark(&squeeze(&p99, 16))
            ),
        ),
        (
            "Errors".into(),
            if p.failed == 0 {
                "none".into()
            } else {
                format!("{errors:.2}% · {}\n{}", count(p.failed), causes.join(" · "))
            },
        ),
        (
            "Sent".into(),
            format!(
                "{} requests{}",
                count(p.requests),
                match p.anomalies {
                    0 => String::new(),
                    1 => "\n1 change · in the thread".into(),
                    n => format!("\n{n} changes · in the thread"),
                }
            ),
        ),
    ]));
    blocks.extend(controls(run, progress));
    blocks.push(context(format!(
        "{}{} · pepe {}",
        p.note
            .as_ref()
            .map(|n| format!("{n} · "))
            .unwrap_or_default(),
        now_token("updated"),
        crate::utils::version()
    )));
    message(
        format!("⏳ {} · {}", name(run), m.status.trim_start_matches("⏳ ")),
        if p.paused { PAUSED } else { RUNNING },
        blocks,
        live_mood(progress),
    )
}

/// A line in the thread: something changed
pub(super) fn anomaly(m: &Message) -> (String, String) {
    let text = m.values.get("findings").cloned().unwrap_or_default();
    let (line, mood) = thread_line(m.detail.kind.as_deref(), &text);
    (line, face(mood))
}

/// A thing that changed, as a short line for the thread: what moved and
/// which way, to what, and when. "throughput fell 52% to 5.9k req/s at
/// 1m26s" becomes "📉 Throughput dropped 52% · 5.9k req/s · at 1m26s".
fn thread_line(kind: Option<&str>, text: &str) -> (String, &'static str) {
    let (what, when) = match text.rsplit_once(" at ") {
        Some((what, when)) => (what, Some(when)),
        None => (text, None),
    };
    let (icon, line, mood) = match kind {
        Some("throughput") => (
            "📉",
            what.replace("throughput fell ", "Throughput dropped ")
                .replace(" to ", " · "),
            "sweating",
        ),
        Some("latency") => (
            "📈",
            what.replace("p99 jumped ", "p99 up ")
                .replace(" to ", " · "),
            "sweating",
        ),
        Some("errors") => (
            "🔥",
            what.replace("errors rose to ", "Errors up to "),
            "on-fire",
        ),
        _ => ("⚠️", what.to_string(), "sweating"),
    };
    let line = match when {
        Some(when) => format!("{icon} {line} · at {when}"),
        None => format!("{icon} {line}"),
    };
    (line, mood)
}

/// How the run ended, in a word and a colour and a face
fn outcome(m: &Message) -> (&'static str, &'static str, &'static str, &'static str) {
    let failed = m
        .detail
        .ending
        .as_ref()
        .is_some_and(|e| !e.crossed().is_empty());
    let report = m.detail.report.as_ref();
    let interrupted = report.is_some_and(|r| r["summary"]["interrupted"] == true);
    let level = report
        .and_then(|r| r["verdict"]["level"].as_str())
        .unwrap_or(match m.tone {
            Tone::Bad => "failing",
            Tone::Warn => "degraded",
            _ => "healthy",
        });
    match (failed, level, interrupted) {
        (true, _, _) => ("❌", "Failed", FAILED, "worried"),
        (_, "failing", _) => ("❌", "Failing", FAILED, "on-fire"),
        (_, _, true) => ("⏹️", "Stopped", DEGRADED, "dizzy"),
        (_, "degraded", _) => ("⚠️", "Passed, with a warning", DEGRADED, "worried"),
        _ => ("✅", "Passed", PASSED, "proud"),
    }
}

/// The verdict: the live message, at the end
pub(super) fn verdict(m: &Message) -> Value {
    let run = &m.detail.run;
    let (icon, word, color, mood) = outcome(m);
    let ending = m.detail.ending.clone().unwrap_or_default();
    let crossed = ending.crossed();
    let mut blocks = vec![header(format!(
        "{icon} {word} · {}{}",
        name(run),
        match crossed.first() {
            Some(first) => format!(" · {}", first.split(':').next().unwrap_or(first)),
            None => String::new(),
        }
    ))];
    blocks.push(context(about(run)));
    let report = m.detail.report.clone().unwrap_or(Value::Null);
    let summary = &report["summary"];
    if summary.is_object() {
        blocks.push(section(story(&report, m)));
        let mut pairs = Vec::new();
        let lat = &summary["latency"];
        pairs.push((
            "Latency p50 · p95 · p99 · max".to_string(),
            format!(
                "{} · {} · {} · {}",
                ms(lat["median_ms"].as_f64().unwrap_or(0.0)),
                ms(lat["p95_ms"].as_f64().unwrap_or(0.0)),
                ms(lat["p99_ms"].as_f64().unwrap_or(0.0)),
                ms(lat["max_ms"].as_f64().unwrap_or(0.0))
            ),
        ));
        let (rps, p99) = series(&report, &m.detail.series);
        if !rps.is_empty() {
            pairs.push((
                "Over the run".to_string(),
                format!(
                    "req/s `{}`\np99 `{}`",
                    spark(&squeeze(&rps, 16)),
                    spark(&squeeze(&p99, 16))
                ),
            ));
        }
        if let Some(phases) = phases(
            &lat["phases"],
            summary["total_requests"].as_u64().unwrap_or(0),
        ) {
            pairs.push(("Where the time went".to_string(), phases));
        }
        if let Some(codes) = codes(&summary["status_codes"]) {
            pairs.push(("Answers".to_string(), codes));
        }
        if !ending.checks.is_empty() {
            let lines: Vec<String> = ending
                .checks
                .iter()
                .map(|c| {
                    format!(
                        "{} fail if `{}` · {}",
                        if c.held { "✓" } else { "✗" },
                        c.text,
                        c.measured
                    )
                })
                .collect();
            pairs.push(("Thresholds".to_string(), lines.join("\n")));
        }
        if let Some(c) = &ending.compared {
            pairs.push((
                "Against the last run".to_string(),
                format!(
                    "{}{}: {}\n_{}_",
                    if c.regression { "🔻 " } else { "" },
                    c.verdict,
                    c.summary,
                    c.when
                ),
            ));
        }
        blocks.push(fields(&pairs));
        let notes = notes(&report);
        if !notes.is_empty() {
            blocks.push(section(notes.join("\n")));
        }
    } else {
        // A ramp's or a ping's report: its own numbers
        blocks.push(section(m.status.clone()));
        if !m.fields.is_empty() {
            blocks.push(fields(&m.fields));
        }
        if !m.findings.is_empty() {
            let lines: Vec<String> = m
                .findings
                .iter()
                .take(6)
                .map(|f| format!("• {f}"))
                .collect();
            blocks.push(section(lines.join("\n")));
        }
        if !ending.checks.is_empty() || !ending.failed.is_empty() {
            blocks.push(section(
                crossed
                    .iter()
                    .map(|c| format!("✗ {c}"))
                    .collect::<Vec<_>>()
                    .join("\n"),
            ));
        }
    }
    blocks.extend(verdict_buttons(run));
    blocks.push(context(format!(
        "`{}`{} · pepe {}",
        run.command,
        if run.host.is_empty() {
            String::new()
        } else {
            format!(" · {}", run.host)
        },
        crate::utils::version()
    )));
    message(
        format!("{icon} {word} · {}", name(run)),
        color,
        blocks,
        mood,
    )
}

/// The line under the verdict, shown in the channel: the result at a
/// glance where the channel is now, with the mention when it failed
pub(super) fn result_line(m: &Message, mention: Option<&str>) -> (String, String) {
    let (icon, word, _, mood) = outcome(m);
    let run = &m.detail.run;
    let report = m.detail.report.clone().unwrap_or(Value::Null);
    let s = &report["summary"];
    let numbers = if s.is_object() {
        let total = s["total_requests"].as_u64().unwrap_or(0);
        let failed = s["failed_requests"].as_u64().unwrap_or(0);
        format!(
            " · {} requests · p99 {} · {:.2}% errors",
            count(total),
            ms(s["latency"]["p99_ms"].as_f64().unwrap_or(0.0)),
            if total == 0 {
                0.0
            } else {
                failed as f64 * 100.0 / total as f64
            }
        )
    } else {
        String::new()
    };
    let crossed = m
        .detail
        .ending
        .as_ref()
        .map(|e| e.crossed())
        .unwrap_or_default();
    let why = match crossed.first() {
        Some(first) => format!("\n{first}"),
        None => String::new(),
    };
    let mention = match (mention, crossed.is_empty()) {
        (Some(m), false) => format!("{m} "),
        _ => String::new(),
    };
    (
        format!("{mention}{icon} *{word}* · {}{numbers}{why}", name(run)),
        face(mood),
    )
}

/// The run in two or three sentences: how much, how fast, what failed,
/// and the first thing that changed
fn story(report: &Value, m: &Message) -> String {
    let s = &report["summary"];
    let total = s["total_requests"].as_u64().unwrap_or(0);
    let failed = s["failed_requests"].as_u64().unwrap_or(0);
    let duration =
        std::time::Duration::from_secs_f64(s["duration_ms"].as_f64().unwrap_or(0.0).max(0.0) / 1e3);
    let mut out = format!(
        "*{} requests* in {} at *{} req/s*. p50 {}, p99 {}.",
        count(total),
        span(duration),
        count(s["requests_per_second"].as_f64().unwrap_or(0.0).round() as u64),
        ms(s["latency"]["median_ms"].as_f64().unwrap_or(0.0)),
        ms(s["latency"]["p99_ms"].as_f64().unwrap_or(0.0)),
    );
    if failed == 0 {
        out.push_str(" None failed.");
    } else {
        let pct = failed as f64 * 100.0 / total.max(1) as f64;
        match s["failures"].as_array().and_then(|f| f.first()) {
            Some(top) => out.push_str(&format!(
                " {pct:.2}% failed ({}), most of them {} ({}).",
                count(failed),
                top["cause"].as_str().unwrap_or("?"),
                count(top["count"].as_u64().unwrap_or(0))
            )),
            None => out.push_str(&format!(" {pct:.2}% failed ({}).", count(failed))),
        }
    }
    if let Some(first) = s["anomalies"].as_array().and_then(|a| a.first()) {
        if let Some(text) = first["text"].as_str() {
            out.push_str(&format!(" {}{}.", text[..1].to_uppercase(), &text[1..]));
        }
    }
    let _ = m;
    out
}

/// Requests a second and p99 over the run: the report's minutes, or for a
/// run too short to have them, the seconds the live message last showed
fn series(report: &Value, seconds: &[(f64, f64)]) -> (Vec<f64>, Vec<f64>) {
    match report["timeline"].as_array() {
        Some(minutes) if minutes.len() >= 3 => (
            minutes
                .iter()
                .map(|p| p["requests_per_second"].as_f64().unwrap_or(0.0))
                .collect(),
            minutes
                .iter()
                .map(|p| p["p99_ms"].as_f64().unwrap_or(0.0))
                .collect(),
        ),
        _ => (
            seconds.iter().map(|s| s.0).collect(),
            seconds.iter().map(|s| s.1).collect(),
        ),
    }
}

/// `wait ████████░░ 91%` and the rest: each phase's share of a request's
/// median time, connecting spread over the requests that reused one
fn phases(p: &Value, total: u64) -> Option<String> {
    let wait = p["first_byte"]["median_ms"].as_f64()?;
    let read = p["download"]["median_ms"].as_f64().unwrap_or(0.0);
    let connect = match (
        p["connect"]["median_ms"].as_f64(),
        p["connect"]["count"].as_u64(),
    ) {
        (Some(median), Some(n)) if total > 0 => median * n as f64 / total as f64,
        _ => 0.0,
    };
    let sum = wait + read + connect;
    if sum <= 0.0 {
        return None;
    }
    let mut parts = [("wait", wait), ("connect", connect), ("read", read)];
    parts.sort_by(|a, b| b.1.total_cmp(&a.1));
    let (top, top_ms) = parts[0];
    let share = top_ms / sum;
    let filled = (share * 10.0).round() as usize;
    let rest: Vec<String> = parts[1..]
        .iter()
        .filter(|(_, v)| *v / sum >= 0.005)
        .map(|(n, v)| format!("{n} {:.0}%", v / sum * 100.0))
        .collect();
    Some(format!(
        "`{top} {}{} {:.0}%`{}",
        "█".repeat(filled),
        "░".repeat(10 - filled),
        share * 100.0,
        if rest.is_empty() {
            String::new()
        } else {
            format!("\n{}", rest.join(" · "))
        }
    ))
}

/// `200 ×1.2M · 503 ×1,180`
fn codes(map: &Value) -> Option<String> {
    let mut codes: Vec<(String, u64)> = map
        .as_object()?
        .iter()
        .map(|(k, v)| (k.clone(), v.as_u64().unwrap_or(0)))
        .collect();
    if codes.is_empty() {
        return None;
    }
    codes.sort_by_key(|a| std::cmp::Reverse(a.1));
    Some(
        codes
            .iter()
            .take(4)
            .map(|(k, n)| format!("{k} ×{}", count(*n)))
            .collect::<Vec<_>>()
            .join(" · "),
    )
}

/// What pepe found, worst first, and each kind of failure with the first
/// body it came with
fn notes(report: &Value) -> Vec<String> {
    let dot = |level: &str| match level {
        "failing" | "bad" => "🔴",
        "degraded" | "warn" => "🟡",
        _ => "🟢",
    };
    let mut out = Vec::new();
    for f in report["summary"]["failures"]
        .as_array()
        .into_iter()
        .flatten()
        .take(3)
    {
        let body = f["example_body"]
            .as_str()
            .map(|b| b.split_whitespace().collect::<Vec<_>>().join(" "))
            .filter(|b| !b.is_empty())
            .map(|b| format!(" · `{}`", super::cut(&b, 120)))
            .unwrap_or_default();
        out.push(format!(
            "🔴 *{}* ×{}{body}",
            f["cause"].as_str().unwrap_or("?"),
            count(f["count"].as_u64().unwrap_or(0))
        ));
    }
    let mut findings: Vec<(&str, &str)> = report["verdict"]["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| Some((f["level"].as_str()?, f["text"].as_str()?)))
        .collect();
    let rank = |l: &str| match l {
        "failing" => 0,
        "degraded" => 1,
        _ => 2,
    };
    findings.sort_by_key(|(l, _)| rank(l));
    for (level, text) in findings.into_iter().take(5) {
        out.push(format!("{} {text}", dot(level)));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_only_where_something_answers_them() {
        let run = Run::default();
        assert!(controls(&run, None).is_none() && verdict_buttons(&run).is_none());
        let served = Run {
            served: Some("41-1".into()),
            test: Some("checkout".into()),
            buttons: true,
            ..Run::default()
        };
        let going = Progress {
            concurrency: Some(40),
            ..Progress::default()
        };
        let run = Run {
            mode: "run".into(),
            ..served.clone()
        };
        let row = controls(&run, Some(&going)).unwrap();
        let ids: Vec<(&str, &str)> = row["elements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| {
                (
                    b["action_id"].as_str().unwrap(),
                    b["value"].as_str().unwrap(),
                )
            })
            .collect();
        assert_eq!(
            ids,
            [
                ("pepe:pause", "41-1"),
                ("pepe:size", "41-1:39"),
                ("pepe:size", "41-1:41"),
                ("pepe:stop", "41-1")
            ]
        );
        assert!(
            row["elements"][3]["confirm"].is_object(),
            "a stop is confirmed"
        );
        let paused = Progress {
            paused: true,
            ..going
        };
        let ramp = Run {
            mode: "ramp".into(),
            ..served.clone()
        };
        let row = controls(&ramp, Some(&paused)).unwrap();
        assert_eq!(row["elements"][0]["action_id"], "pepe:resume");
        assert_eq!(
            row["elements"].as_array().unwrap().len(),
            2,
            "a ramp sets its own concurrency"
        );
        let after = verdict_buttons(&served).unwrap();
        let ids: Vec<&str> = after["elements"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b["action_id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, ["pepe:run", "pepe:edit", "pepe:history"]);
        // A run that isn't a saved test has nothing to run again
        let adhoc = Run {
            buttons: true,
            ..Run::default()
        };
        assert!(verdict_buttons(&adhoc).is_none());
    }

    #[test]
    fn the_thread_says_what_moved_in_a_line() {
        let say = |kind, text| thread_line(Some(kind), text).0;
        assert_eq!(
            say("throughput", "throughput fell 52% to 5.9k req/s at 1m26s"),
            "📉 Throughput dropped 52% · 5.9k req/s · at 1m26s"
        );
        assert_eq!(
            say("latency", "p99 jumped 4.5× to 45ms at 26s"),
            "📈 p99 up 4.5× · 45ms · at 26s"
        );
        assert_eq!(
            say("errors", "errors rose to 12% at 2m"),
            "🔥 Errors up to 12% · at 2m"
        );
        assert_eq!(thread_line(Some("errors"), "x").1, "on-fire");
        assert_eq!(thread_line(None, "something").0, "⚠️ something");
    }

    #[test]
    fn sparklines_and_bars_read_at_a_glance() {
        assert_eq!(spark(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0]), "▁▂▃▄▅▆▇█");
        assert_eq!(spark(&[5.0, 5.0]), "▄▄", "flat stays in the middle");
        assert_eq!(spark(&[]), "");
        assert_eq!(squeeze(&[1.0, 3.0, 5.0, 7.0], 2), [2.0, 6.0]);
        assert_eq!(bar(0.62), "▰▰▰▰▰▰▱▱▱▱");
    }

    #[test]
    fn the_time_goes_mostly_to_waiting() {
        let p = json!({
            "first_byte": {"median_ms": 38.0},
            "download": {"median_ms": 1.0},
            "connect": {"median_ms": 40.0, "count": 50},
        });
        let line = phases(&p, 1000).unwrap();
        assert!(line.starts_with("`wait █████████░ 93%`"), "{line}");
        assert!(
            line.contains("connect 5%") && line.contains("read 2%"),
            "{line}"
        );
        assert_eq!(
            codes(&json!({"200": 1200, "503": 30})).unwrap(),
            "200 ×1,200 · 503 ×30"
        );
    }
}
