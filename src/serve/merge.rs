//! A distributed run's shares added up: their progress a second at a time,
//! and their reports into one when the last is in.
//!
//! Counts, bytes and rates add. Percentiles don't, so each worker's report
//! keeps its latency histogram whole (`--histogram`) and the histograms
//! are added bucket by bucket: the run's p99 is the p99 of every request
//! any worker sent, exactly as one machine would have measured it.

use std::collections::{BTreeMap, HashMap};

use serde_json::{json, Map, Value};

use crate::metrics::Histogram;

fn num(v: &Value, pointer: &str) -> f64 {
    v.pointer(pointer).and_then(Value::as_f64).unwrap_or(0.0)
}

fn round(x: f64) -> f64 {
    (x * 1000.0).round() / 1000.0
}

/// The workers' latest progress as one: counts and rates added; the
/// percentiles can't be, so the median is weighted by requests and the
/// p99 is the worst worker's, and said to be approximate
pub fn progress(by_worker: &HashMap<String, Value>) -> Value {
    let mut workers = Map::new();
    let (mut requests, mut failed, mut rps, mut bytes) = (0.0, 0.0, 0.0, 0.0);
    let (mut p50_weighted, mut p99_worst) = (0.0, 0.0f64);
    let mut names: Vec<&String> = by_worker.keys().collect();
    names.sort();
    for name in names {
        let p = &by_worker[name];
        let n = num(p, "/requests");
        requests += n;
        failed += num(p, "/failed");
        rps += num(p, "/requests_per_second");
        bytes += num(p, "/bytes");
        p50_weighted += num(p, "/p50_ms") * n;
        p99_worst = p99_worst.max(num(p, "/p99_ms"));
        workers.insert(
            name.clone(),
            json!({
                "requests": n,
                "requests_per_second": round(num(p, "/requests_per_second")),
                "p50_ms": num(p, "/p50_ms"),
                "p99_ms": num(p, "/p99_ms"),
                "done": p.get("done").cloned().unwrap_or(Value::Null),
            }),
        );
    }
    json!({
        "requests": requests,
        "failed": failed,
        "requests_per_second": round(rps),
        "bytes": bytes,
        "p50_ms": if requests > 0.0 { round(p50_weighted / requests) } else { 0.0 },
        "p99_ms": p99_worst,
        "approximate": ["p50_ms", "p99_ms"],
        "workers": workers,
    })
}

/// A histogram put together again from a report's `histogram_us`
fn histogram_of(report: &Value) -> Option<Histogram> {
    let buckets = report
        .pointer("/summary/latency/histogram_us")?
        .as_array()?;
    let mut hist = Histogram::default();
    let (mut min, mut max) = (u64::MAX, 0u64);
    for pair in buckets {
        let (Some(v), Some(n)) = (
            pair.get(0).and_then(Value::as_u64),
            pair.get(1).and_then(Value::as_u64),
        ) else {
            continue;
        };
        // The exact ends travel as entries of no count
        if n == 0 {
            min = min.min(v);
            max = max.max(v);
        } else {
            hist.record_n(v, n);
        }
    }
    if hist.count() > 0 && min <= max {
        hist.set_extremes(min, max);
    }
    Some(hist)
}

fn add(into: &mut Histogram, from: &Histogram) {
    into.absorb(from);
}

/// The reports of a distributed run's shares as one report, in the shape
/// of a run's (`schema/run.schema.json`), with `distributed` saying who
/// sent what. `failed` names the shares that ended without a report.
pub fn reports(reports: &[(String, Value)], failed: &[String]) -> Value {
    let sum = |pointer: &str| -> f64 { reports.iter().map(|(_, r)| num(r, pointer)).sum() };
    let max = |pointer: &str| -> f64 {
        reports
            .iter()
            .map(|(_, r)| num(r, pointer))
            .fold(0.0, f64::max)
    };
    let total = sum("/summary/total_requests");

    let mut whole = Histogram::default();
    let mut exact = true;
    let (mut min, mut max_us) = (u64::MAX, 0u64);
    for (_, report) in reports {
        match histogram_of(report) {
            Some(hist) => {
                let (lo, hi) = hist.extremes();
                if hist.count() > 0 {
                    min = min.min(lo);
                    max_us = max_us.max(hi);
                }
                add(&mut whole, &hist);
            }
            None => exact = false,
        }
    }
    if whole.count() > 0 {
        whole.set_extremes(min, max_us);
    }
    let at = |q: f64| round(whole.percentile(q) as f64 / 1000.0);
    // The mean and the spread, from each share's
    let mean = if total > 0.0 {
        reports
            .iter()
            .map(|(_, r)| num(r, "/summary/latency/avg_ms") * num(r, "/summary/total_requests"))
            .sum::<f64>()
            / total
    } else {
        0.0
    };
    let spread = if total > 0.0 {
        let second = reports
            .iter()
            .map(|(_, r)| {
                let (m, s) = (
                    num(r, "/summary/latency/avg_ms"),
                    num(r, "/summary/latency/std_dev_ms"),
                );
                num(r, "/summary/total_requests") * (s * s + m * m)
            })
            .sum::<f64>()
            / total;
        (second - mean * mean).max(0.0).sqrt()
    } else {
        0.0
    };
    // A phase: counts added, the median weighted, the rest the worst
    let phase = |name: &str| -> Value {
        let base = format!("/summary/latency/phases/{name}");
        let count = sum(&format!("{base}/count"));
        let median = if count > 0.0 {
            reports
                .iter()
                .map(|(_, r)| {
                    num(r, &format!("{base}/median_ms")) * num(r, &format!("{base}/count"))
                })
                .sum::<f64>()
                / count
        } else {
            0.0
        };
        json!({
            "count": count as u64,
            "median_ms": round(median),
            "p99_ms": max(&format!("{base}/p99_ms")),
            "max_ms": max(&format!("{base}/max_ms")),
        })
    };
    let mut status_codes: BTreeMap<String, u64> = BTreeMap::new();
    let mut failures: BTreeMap<String, (u64, Option<String>)> = BTreeMap::new();
    for (_, report) in reports {
        if let Some(codes) = report
            .pointer("/summary/status_codes")
            .and_then(Value::as_object)
        {
            for (code, n) in codes {
                *status_codes.entry(code.clone()).or_default() += n.as_u64().unwrap_or(0);
            }
        }
        for failure in report
            .pointer("/summary/failures")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            let cause = failure["cause"].as_str().unwrap_or("?").to_string();
            let entry = failures.entry(cause).or_insert((0, None));
            entry.0 += failure["count"].as_u64().unwrap_or(0);
            if entry.1.is_none() {
                entry.1 = failure["example_body"].as_str().map(str::to_string);
            }
        }
    }
    let mut failures: Vec<(String, (u64, Option<String>))> = failures.into_iter().collect();
    failures.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let interrupted = reports
        .iter()
        .any(|(_, r)| r.pointer("/summary/interrupted") == Some(&json!(true)));

    // The verdict: the worst share's level, and what each share found
    const LEVELS: [&str; 3] = ["healthy", "degraded", "failing"];
    let level = reports
        .iter()
        .filter_map(|(_, r)| r.pointer("/verdict/level").and_then(Value::as_str))
        .filter_map(|l| LEVELS.iter().position(|x| *x == l))
        .max()
        .map_or("healthy", |i| LEVELS[i]);
    let level = if failed.is_empty() { level } else { "failing" };
    let mut findings = vec![json!({
        "level": "healthy",
        "text": format!("Shared out to {} worker{}", reports.len() + failed.len(), if reports.len() + failed.len() == 1 { "" } else { "s" }),
    })];
    for f in failed {
        findings.push(json!({ "level": "failing", "text": format!("No report from {f}") }));
    }
    for (name, report) in reports {
        for finding in report
            .pointer("/verdict/findings")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            findings.push(json!({
                "level": finding["level"],
                "text": format!("{name}: {}", finding["text"].as_str().unwrap_or("")),
            }));
        }
    }

    let first = reports.first().map(|(_, r)| r);
    let concurrency: f64 = sum("/target/concurrency");
    let mut report = json!({
        "schema_version": 1,
        "target": {
            "mode": "run",
            "method": first.and_then(|r| r.pointer("/target/method")).cloned().unwrap_or(json!("GET")),
            "url": first.and_then(|r| r.pointer("/target/url")).cloned().unwrap_or(json!("")),
            "concurrency": concurrency as u64,
        },
        "summary": {
            "total_requests": total as u64,
            "successful_requests": sum("/summary/successful_requests") as u64,
            "failed_requests": sum("/summary/failed_requests") as u64,
            "http_errors": sum("/summary/http_errors") as u64,
            "timeout_errors": sum("/summary/timeout_errors") as u64,
            "connection_errors": sum("/summary/connection_errors") as u64,
            "interrupted": interrupted,
            "duration_ms": max("/summary/duration_ms"),
            // The shares ran side by side: their rates add
            "requests_per_second": round(sum("/summary/requests_per_second")),
            "data_transfer_bytes": sum("/summary/data_transfer_bytes") as u64,
            "latency": {
                "min_ms": if whole.count() > 0 { at(0.0) } else { 0.0 },
                "max_ms": if whole.count() > 0 { at(100.0) } else { 0.0 },
                "avg_ms": round(mean),
                "std_dev_ms": round(spread),
                "median_ms": at(50.0),
                "p90_ms": at(90.0),
                "p95_ms": at(95.0),
                "p99_ms": at(99.0),
                "phases": {
                    "first_byte": phase("first_byte"),
                    "download": phase("download"),
                    "dns_lookup_avg_ms": round(
                        if total > 0.0 {
                            reports.iter().map(|(_, r)| num(r, "/summary/latency/phases/dns_lookup_avg_ms") * num(r, "/summary/total_requests")).sum::<f64>() / total
                        } else { 0.0 }
                    ),
                },
                "quantiles_ms": crate::json_report::QUANTILES.iter().map(|&q| json!({"q": q, "ms": at(q)})).collect::<Vec<_>>(),
            },
            "status_codes": status_codes,
            "failures": failures.iter().map(|(cause, (count, body))| json!({"cause": cause, "count": count, "example_body": body})).collect::<Vec<_>>(),
        },
        "generator": {
            "threads": sum("/generator/threads") as u64,
        },
        "verdict": { "level": level, "findings": findings },
        "distributed": {
            "workers": reports.iter().map(|(name, r)| json!({
                "name": name,
                "requests": num(r, "/summary/total_requests"),
                "requests_per_second": round(num(r, "/summary/requests_per_second")),
                "p99_ms": num(r, "/summary/latency/p99_ms"),
                "level": r.pointer("/verdict/level").cloned().unwrap_or(Value::Null),
            })).collect::<Vec<_>>(),
            "failed": failed,
            "exact_percentiles": exact,
        },
    });
    let rate = sum("/generator/rate_per_second");
    if rate > 0.0 {
        report["generator"]["rate_per_second"] = json!(round(rate));
        report["generator"]["rate_missed"] = json!(sum("/generator/rate_missed") as u64);
    }
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A share's report as a worker's run writes it, of `n` requests that
    /// took `fast` ms but for `slow_n` of them at `slow` ms
    fn share(n: u64, fast: u64, slow_n: u64, slow: u64, rps: f64) -> Value {
        let mut hist = Histogram::default();
        for _ in 0..n - slow_n {
            hist.record(fast * 1000);
        }
        for _ in 0..slow_n {
            hist.record(slow * 1000);
        }
        let (min, max) = hist.extremes();
        let mut buckets: Vec<Value> = vec![json!([min, 0])];
        buckets.extend(hist.buckets().map(|(v, c)| json!([v, c])));
        buckets.push(json!([max, 0]));
        json!({
            "schema_version": 1,
            "target": {"mode": "run", "method": "GET", "url": "http://x/", "concurrency": 10},
            "summary": {
                "total_requests": n, "successful_requests": n, "failed_requests": 0,
                "http_errors": 0, "timeout_errors": 0, "connection_errors": 0,
                "interrupted": false, "duration_ms": 10_000.0, "requests_per_second": rps,
                "data_transfer_bytes": n * 100,
                "latency": {"avg_ms": fast as f64, "std_dev_ms": 1.0, "p99_ms": hist.percentile(99.0) as f64 / 1000.0,
                            "histogram_us": buckets,
                            "phases": {"first_byte": {"count": n, "median_ms": fast as f64, "p99_ms": slow as f64, "max_ms": slow as f64},
                                       "download": {"count": n, "median_ms": 0.1, "p99_ms": 0.2, "max_ms": 0.3},
                                       "dns_lookup_avg_ms": 0.0}},
                "status_codes": {"200": n},
            },
            "generator": {"threads": 1},
            "verdict": {"level": "healthy", "findings": [{"level": "healthy", "text": "No failed requests"}]},
        })
    }

    /// Two shares' p99 isn't either's: one has no slow requests and one
    /// has 2% of them; together 1% are slow, so the p99 is at the edge of
    /// them, which only the histograms added up can say
    #[test]
    fn shares_add_up_to_one_runs_numbers() {
        let a = share(10_000, 10, 0, 0, 1000.0);
        let b = share(10_000, 10, 200, 300, 900.0);
        let merged = reports(&[("a".into(), a), ("b".into(), b)], &[]);
        let s = &merged["summary"];
        assert_eq!(s["total_requests"], 20_000);
        assert_eq!(s["requests_per_second"], 1900.0);
        assert_eq!(s["status_codes"]["200"], 20_000);
        let p99 = s["latency"]["p99_ms"].as_f64().unwrap();
        let p995 = merged["summary"]["latency"]["quantiles_ms"]
            .as_array()
            .unwrap()
            .iter()
            .find(|q| q["q"] == 99.5)
            .unwrap()["ms"]
            .as_f64()
            .unwrap();
        assert!(
            p99 <= 11.0,
            "200 of 20,000 is 1%: the p99 is still fast ({p99})"
        );
        assert!(p995 >= 290.0, "past 99% it is the slow ones ({p995})");
        assert_eq!(s["latency"]["max_ms"], 300.0);
        assert_eq!(merged["distributed"]["exact_percentiles"], true);
        assert_eq!(merged["target"]["concurrency"], 20);
        let schema: Value = serde_json::from_str(crate::schema::RUN).unwrap();
        let problems = crate::schema::check(&schema, &merged);
        assert!(problems.is_empty(), "{problems:?}");
    }

    #[test]
    fn a_share_with_no_report_fails_the_run() {
        let a = share(100, 10, 0, 0, 10.0);
        let merged = reports(&[("a".into(), a)], &["b: exit 1".into()]);
        assert_eq!(merged["verdict"]["level"], "failing");
        assert_eq!(merged["distributed"]["failed"][0], "b: exit 1");
    }

    #[test]
    fn progress_adds_up_and_says_what_is_approximate() {
        let mut by = HashMap::new();
        by.insert(
            "a".to_string(),
            json!({"requests": 100, "requests_per_second": 50.0, "p50_ms": 10.0, "p99_ms": 20.0}),
        );
        by.insert(
            "b".to_string(),
            json!({"requests": 300, "requests_per_second": 150.0, "p50_ms": 30.0, "p99_ms": 90.0}),
        );
        let p = progress(&by);
        assert_eq!(p["requests"], 400.0);
        assert_eq!(p["requests_per_second"], 200.0);
        assert_eq!(p["p50_ms"], 25.0);
        assert_eq!(p["p99_ms"], 90.0);
        assert_eq!(p["approximate"][1], "p99_ms");
    }
}
