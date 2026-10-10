# Output and exit codes

What a run leaves behind, and what a script can rely on: the report in the shell, the JSON report and its schema, thresholds that set the exit code, live metrics for Prometheus, and the exit codes themselves.

## The report in the shell

Every screen leaves its report in the scrollback when you quit: the verdict and its findings for a run, the table and the estimate for a ramp, the summary for a ping, the report for the logs. Coloured when the shell shows colour, plain with `NO_COLOR`.

## Without a terminal

Piped, redirected, or run by a script, pepe doesn't try to draw: the run goes to its end and the report the dashboard would have left is printed, the same verdict and findings, so a forgotten `--json` costs nothing. `--snapshot` and `--metrics` work as they do on the dashboard. A ramp prints its steps and estimate; API mode needs its endpoints picked on the command line (`--all`, `--tag`, `--only`), since there is no plan screen to pick them on. A ping prints a line per ping, then its summary.

```bash
pepe -z 30s -c 20 https://example.com > run.txt
pepe ramp https://example.com --to 200 | tee ramp.txt
```

## JSON output

`--json` skips the dashboard, runs to completion and prints a report to stdout. Press Ctrl-C to stop early; the report then has `"interrupted": true`. Notes and errors go to stderr, so stdout stays valid JSON.

```bash
pepe --json -n 1000 -c 20 https://example.com > results.json
jq '.summary.latency.p99_ms' results.json
```

```json
{
  "schema_version": 1,
  "target": { "mode": "run", "method": "GET", "url": "https://example.com", "concurrency": 20 },
  "summary": {
    "total_requests": 1000,
    "successful_requests": 1000,
    "failed_requests": 0,
    "http_errors": 0,
    "timeout_errors": 0,
    "connection_errors": 0,
    "interrupted": false,
    "duration_ms": 1843.2,
    "requests_per_second": 542.5,
    "data_transfer_bytes": 1256000,
    "latency": { "min_ms": 9.1, "max_ms": 212.4, "avg_ms": 36.1, "std_dev_ms": 18.0,
                 "median_ms": 31.9, "p90_ms": 58.2, "p95_ms": 71.0, "p99_ms": 120.3,
                 "phases": { "connect": { "count": 20, "median_ms": 12.0, "p99_ms": 30.1, "max_ms": 31.0 },
                             "first_byte": { "count": 1000, "median_ms": 28.0, "p99_ms": 110.2, "max_ms": 201.0 },
                             "download": { "count": 1000, "median_ms": 0.4, "p99_ms": 2.1, "max_ms": 11.0 },
                             "dns_lookup_avg_ms": 1.2 } },
    "status_codes": { "200": 1000 },
    "server_timing": { "responses": 1000, "total": { "count": 1000, "median_ms": 24.1, "p90_ms": 40.2, "p99_ms": 88.0 },
                       "segments": { "db": { "count": 1000, "median_ms": 18.3, "p90_ms": 31.0, "p99_ms": 70.2 },
                                     "app": { "count": 1000, "median_ms": 5.8, "p90_ms": 9.1, "p99_ms": 17.9 } } },
    "slowest_requests": [ { "at_s": 1.204, "latency_ms": 212.4, "status": 200,
                            "request_id": "8f3c1a2e-7b9d", "id_header": "x-request-id" } ],
    "anomalies": [ { "at_s": 26.0, "kind": "latency", "text": "p99 jumped 4.5× to 45ms at 26s" } ],
    "failures": [ { "cause": "HTTP 503", "count": 3, "example_body": "upstream unavailable" } ]
  },
  "generator": { "threads": 1, "peak_busy_percent": 12 },
  "timeline": [ { "at_s": 60, "requests_per_second": 540.1, "errors_per_second": 0, "p50_ms": 31.0, "p90_ms": 57.9, "p99_ms": 118.0 } ]
}
```

| Section | What it holds |
| --- | --- |
| `target` | What was tested and with what load: `mode` (run, ramp, api, flow, replay), `method`, `url`, `concurrency` |
| `summary` | The counts, `duration_ms`, `requests_per_second`, `data_transfer_bytes`, `latency` with its percentiles and `phases`, `status_codes`, and, when there is something to say, `server_timing`, `slowest_requests`, `anomalies` and `failures` by cause with the first body of each |
| `generator` | pepe's own load: `threads`, `peak_busy_percent` (near 100, pepe was the limit), `warmup_s`, `warmup_requests`, `rate_per_second`, `rate_missed` |
| `timeline` | One point per minute over the whole run, for runs over a minute |
| `snapshot` | With `--snapshot`: when it was written and whether the run was still going |
| `endpoints`, `flow`, `replay` | API mode, flows and replays: one entry per endpoint, step or URL |

Ramps print their own shape (every step, `findings`, `capacity`, `holds_concurrency`), pings theirs (every target with its latency, phases, TLS, certificate, SLO and findings), compares theirs (the verdict, each change, the badge), and the logs theirs.

## The schema

Every report starts with `"schema_version": 1`: fields are added within a version and never renamed, so a script can depend on the names. The JSON Schema of each report kind is in every release archive under `schema/`, at `https://pepe.mhaimdat.com/schema/`, and printed by `pepe schema`:

```bash
pepe schema run > run.schema.json
pepe schema ramp
pepe schema ping | jq '.properties.targets.items.required'
pepe schema compare
```

The schemas describe what a report always has and what it has when there is something to say; readers should take an absent optional section as "nothing to say", not an error.

## Thresholds that fail CI

`--fail-if` names what the run must not cross, in the ramp's condition language, and ends with exit code 4 when it does, the report still printed and the condition said on stderr with the number it saw. Repeat it for more than one; every mode that measures requests takes it (a plain run, a ramp, API mode, a flow, a replay, with or without `--json`). `pepe ping` has `--slo` for the same.

```bash
pepe --json -n 2000 -c 20 --fail-if 'p99 > 300ms' --fail-if 'errors > 1%' https://staging.example.com/api
```

```
✖ --fail-if p99 > 300ms: p99 was 412.0ms
```

A condition is `pNN > LATENCY` (any percentile, with a unit: `500ms`, `2s`, `800us`) or `errors > PERCENT`.

## Prometheus metrics

`--metrics :9100` serves the live numbers at `http://localhost:9100/metrics` while the run goes, in Prometheus's text form, so a soak run or a long ping shows up in Grafana next to the server's own metrics. It works with the dashboard and with `--json`, for plain runs, API mode, flows, replays and `pepe ping`; a ramp isn't served, its steps being the point of it. `:9100`, `9100` and `host:port` are all addresses; the one it listens on is printed to stderr at the start.

```bash
pepe -z 6h -c 50 --metrics :9100 https://example.com
pepe ping https://example.com --metrics 127.0.0.1:9100
curl -s localhost:9100/metrics
```

A load run has `pepe_requests_total`, `pepe_requests_succeeded_total`, `pepe_requests_timed_out_total`, `pepe_requests_errored_total`, `pepe_responses_total{status}`, `pepe_failures_total{cause}`, `pepe_response_bytes_total`, `pepe_cache_hits_total`, `pepe_requests_per_second` and `pepe_errors_per_second` over the last second, `pepe_latency_seconds{quantile}` (0.5, 0.9, 0.95, 0.99) and `pepe_first_byte_seconds{quantile}` so far, the histogram `pepe_request_duration_seconds` with fixed buckets from 1 ms to 30 s, `pepe_concurrency` and `pepe_run_elapsed_seconds`. Every line carries `target="GET https://…"`, and in API mode, a flow or a replay the same again per row, with `row="GET /pets/{id}"`.

A ping has `pepe_ping_sent_total`, `pepe_ping_answered_total`, `pepe_ping_lost_total`, `pepe_ping_timed_out_total`, `pepe_ping_slo_broken_total`, `pepe_ping_up`, `pepe_ping_last_seconds`, `pepe_ping_loss_ratio`, `pepe_ping_latency_seconds{quantile}`, `pepe_ping_jitter_seconds`, `pepe_ping_phase_seconds{phase}` (the median of dns, connect, tls, tls_resumed, ttfb and download), `pepe_ping_responses_total{status}`, `pepe_ping_tls_resumed_total`, `pepe_ping_cert_not_after_seconds` and the histogram `pepe_ping_duration_seconds`, each per `target`. The page is rendered once a second at most, whatever scrapes it.

## Soak runs

`--snapshot FILE` writes the JSON report so far every minute, whole or not at all, and once more at the end, with a minute-by-minute `timeline`, so a long run's numbers survive a crash or a lost terminal ([Load testing: soak runs](load-test.md#soak-runs)).

## Exit codes

| Code | Means |
| --- | --- |
| 0 | The run went as asked; `pepe compare` found no regression |
| 1 | It couldn't start (a bad file, no endpoint to run), nothing ever answered (a ping), `pepe compare --gate` found a regression, or `pepe self-update --check` found a newer release |
| 2 | A usage error: a flag or a value pepe doesn't take, or nothing to run |
| 4 | A limit was crossed: `--fail-if` on a run, `--slo` on a ping |
| 130 | Interrupted with Ctrl-C while the screen couldn't be left cleanly |

The report is printed before a non-zero code, so what crossed the limit is still there to read.
