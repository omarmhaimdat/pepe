//! `--events FILE` (or `-` for stdout): the run as it goes, a JSON line at
//! a time. A `start` line, a `progress` line a second, an `anomaly` line
//! when something changes, a `steered` line when the run is paused,
//! resumed or resized, and a `report` line with the whole report last.
//! `pepe serve` gives each run it starts a file of its own and follows it,
//! which is how a browser or a script watches a run live; a person can
//! `pepe --events - URL | jq` the same lines.
//!
//! It rides on the webhooks' calls (`webhook::begin`, `progress`,
//! `anomaly`, `end`), which every mode makes already, so every mode has it.

use std::io::Write;
use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use crate::webhook::{Progress, Run};

/// A progress line this often
pub const EVERY: Duration = Duration::from_secs(1);

struct Sink {
    out: Box<dyn Write + Send>,
    /// The next line's number, from 1: what a reader that lost its place
    /// asks to go on from
    seq: u64,
    last_progress: Option<Instant>,
    started: Instant,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// Start writing events to `path`, `-` being stdout. The file is made
/// anew.
pub fn open(path: &Path) -> std::io::Result<()> {
    let out: Box<dyn Write + Send> = if path.as_os_str() == "-" {
        Box::new(std::io::stdout())
    } else {
        Box::new(std::fs::File::create(path)?)
    };
    if let Ok(mut sink) = SINK.lock() {
        *sink = Some(Sink {
            out,
            seq: 1,
            last_progress: None,
            started: Instant::now(),
        });
    }
    Ok(())
}

/// A progress line is due
pub fn due() -> bool {
    SINK.lock().is_ok_and(|s| {
        s.as_ref()
            .is_some_and(|s| s.last_progress.is_none_or(|at| at.elapsed() >= EVERY))
    })
}

fn write(kind: &str, mut body: Value) {
    let Ok(mut sink) = SINK.lock() else { return };
    let Some(sink) = sink.as_mut() else { return };
    // What it is and where it falls first, what it says after
    let mut line = serde_json::Map::new();
    line.insert("type".into(), json!(kind));
    line.insert("seq".into(), json!(sink.seq));
    line.insert(
        "at_s".into(),
        json!((sink.started.elapsed().as_secs_f64() * 1000.0).round() / 1000.0),
    );
    if let Some(object) = body.as_object_mut() {
        line.append(object);
    }
    let body = Value::Object(line);
    sink.seq += 1;
    // One line, whole, flushed: a reader following the file never sees
    // half of one
    let mut line = body.to_string();
    line.push('\n');
    let _ = sink.out.write_all(line.as_bytes());
    let _ = sink.out.flush();
}

fn ms(d: Duration) -> f64 {
    (d.as_secs_f64() * 1_000_000.0).round() / 1000.0
}

pub fn start(run: &Run) {
    write(
        "start",
        json!({
            "mode": run.mode,
            "target": run.target,
            "test": run.test,
            "load": run.load,
            "rate": run.rate,
            "concurrency": run.concurrency,
            "planned_s": run.planned.map(|d| d.as_secs_f64()),
        }),
    );
}

pub fn progress(p: &Progress) {
    if let Ok(mut sink) = SINK.lock() {
        if let Some(sink) = sink.as_mut() {
            sink.last_progress = Some(Instant::now());
        }
    }
    write(
        "progress",
        json!({
            "elapsed_s": p.elapsed.as_secs_f64(),
            "done": p.done,
            "requests": p.requests,
            "failed": p.failed,
            "requests_per_second": p.rps,
            "p50_ms": ms(p.p50),
            "p95_ms": ms(p.p95),
            "p99_ms": ms(p.p99),
            "bytes": p.bytes,
            "paused": p.paused,
            "concurrency": p.concurrency,
            "failures": p.failures.iter().map(|(cause, count)| json!({"cause": cause, "count": count})).collect::<Vec<_>>(),
            "anomalies": p.anomalies,
        }),
    );
}

/// A ping's progress: each target's numbers so far
pub fn ping_progress(targets: Value) {
    if let Ok(mut sink) = SINK.lock() {
        if let Some(sink) = sink.as_mut() {
            sink.last_progress = Some(Instant::now());
        }
    }
    write("progress", json!({ "targets": targets }));
}

pub fn anomaly(kind: &str, text: &str) {
    write("anomaly", json!({ "kind": kind, "text": text }));
}

pub fn steered(paused: bool, concurrency: usize, what: &str) {
    write(
        "steered",
        json!({ "paused": paused, "concurrency": concurrency, "what": what }),
    );
}

/// The whole report, last
pub fn report(report: &Value) {
    write("report", json!({ "report": report }));
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The lines are whole, numbered, and typed; the report comes last
    #[test]
    fn events_are_numbered_lines_with_the_report_last() {
        let dir = std::env::temp_dir().join(format!("pepe-events-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("e.jsonl");
        open(&path).unwrap();
        start(&Run {
            mode: "run".into(),
            target: "GET http://x/".into(),
            concurrency: 4,
            ..Default::default()
        });
        assert!(due());
        progress(&Progress {
            requests: 10,
            rps: 5.0,
            p99: Duration::from_millis(12),
            ..Default::default()
        });
        assert!(!due(), "a second between progress lines");
        anomaly("latency", "p99 jumped");
        report(&json!({"schema_version": 1}));
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<Value> = text
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let kinds: Vec<&str> = lines.iter().map(|l| l["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["start", "progress", "anomaly", "report"]);
        let seqs: Vec<u64> = lines.iter().map(|l| l["seq"].as_u64().unwrap()).collect();
        assert_eq!(seqs, [1, 2, 3, 4]);
        assert_eq!(lines[1]["p99_ms"], 12.0);
        assert_eq!(lines[3]["report"]["schema_version"], 1);
        *SINK.lock().unwrap() = None;
        let _ = std::fs::remove_dir_all(&dir);
    }
}
