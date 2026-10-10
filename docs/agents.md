# Agents and scripts

pepe is built to be called by a program as readily as by a person: every mode runs without a screen, prints a versioned JSON report with a published schema, and says what went wrong in an exit code. This page is what a coding agent or a script needs to know; a skill file for agents is in the repository under `skills/pepe/`.

## The rules

- Pass `--json` for a report, or let the output be piped for the text verdict; never expect a screen.
- End the run: `-n` for a count or `-z` for a duration. A ping without either runs until stopped.
- Read `schema_version` first; the field names it covers never change. `pepe schema run|ramp|ping|compare` prints the schema.
- Use the exit code: 0 as asked, 1 couldn't start or nothing answered, 2 a usage error, 4 a limit crossed ([Output: exit codes](output.md#exit-codes)).
- Only point pepe at what you are allowed to load. A load test is traffic.

## Diagnose one endpoint

```bash
pepe ping https://api.example.com/health -n 5 --json
```

Read `targets[0]`: `latency_ms` (`min`, `avg`, `max`, `jitter`, `p50`, `p95`, `p99`); `phases_ms` (`dns`, `connect`, `tls_full`, `tls_resumed`, `ttfb`, `download`, each with `median_ms`, `p99_ms` and `max_ms`: the largest median is the bottleneck); `loss`, `timeouts` and `failures` by cause; `tls.cert.days_left` and `tls.cert.valid`; `status_codes`; and `findings`, what to look at, most serious first, each with a `level` (`bad`, `warn`, `note`, `good`) and a `text`. `--once` prints the same as a few lines of text.

`--slo total=500,ttfb=200` adds `slo.pass` and makes the exit code 4 when a limit was broken.

## Watch it over time

```bash
pepe ping https://api.example.com/health --jsonl -z 5m > pings.jsonl
```

One JSON object per ping (`timings_ms`, `status`, `error`, `tls`) as it happens; `--csv` does the same as CSV. `--metrics :9100` serves the live numbers for Prometheus.

## Load-test

```bash
pepe --json -z 30s -c 50 --fail-if 'p99 > 500ms' --fail-if 'errors > 1%' https://api.example.com/health
```

Read `summary`: `total_requests`, `failed_requests`, `requests_per_second`, `latency.median_ms`, `latency.p99_ms`, `latency.phases`, `status_codes`, `failures` with the first body of each cause, `anomalies`. Read `generator.peak_busy_percent`: near 100, pepe rather than the target was the limit, so add `--threads auto` and run again. `--fail-if` names what the run must not cross; the exit code says whether it did.

## Find the capacity

```bash
pepe ramp --json https://api.example.com/health --to 200 --until 'p99 > 500ms'
```

Read `holds_concurrency` (the level that held), `capacity.requests_per_second` (the estimate, `null` when the curve hadn't bent; `extrapolated` when it was read past the ramp), each `steps[]` entry, and `findings`.

## Compare before and after

```bash
pepe --json -n 5000 -c 20 "$URL" > before.json
# change something
pepe --json -n 5000 -c 20 "$URL" > after.json
pepe compare before.json after.json --json --gate
```

Read `verdict` (`faster`, `same`, `slower`, `better`, `worse`), `regression`, each `changes[]` entry with `significant` (moved more than two such runs wobble) and `noise_percent`, and `findings`. Ping reports compare too, phase by phase.

## Test a whole API or a journey

`pepe api openapi.yaml --all --json` runs every endpoint that has the values it needs (`--set id=1,2,3` gives values; writes need `--include-writes`), with `endpoints[]` in the report. `pepe flow checkout.toml --json` runs a sequence where each step feeds the next, with `flow.steps[]`. `pepe replay access.log --base-url "$STAGING" --json` sends a log's URLs in their real proportions, with `replay.urls[]`.

## The same request every mode

`-H 'Name: value'`, `-m`, `-d`, `-u`, `-p`, `-k`, `-t`, `--disable-compression`, `--disable-keepalive` and `--disable-redirects` apply to every mode. `--curl -- curl ...` takes a request as a curl command. A `pepe.toml` next to the code holds a test's settings so that `pepe` alone runs it ([Config file](load-test.md#config-file)).
