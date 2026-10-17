---
name: pepe
description: Find why an HTTP endpoint or API is slow and prove a fix, with pepe. Use when an API, page or endpoint is slow, to make one faster, to load-test or benchmark a URL, to see which part of a service (a query, a call, a lock) its time goes to from its OpenTelemetry traces, to check a change made it faster or slower, to find its capacity, to split a request into DNS, connect, TLS, first byte and download, or to check a TLS certificate's expiry.
---

# pepe

pepe is an HTTP load generator with a live dashboard. `pepe ping` is the
part to reach for first: one request a second, each split into where its
time went, with a JSON report an agent can read. The repository's
`AGENTS.md` is the full set of rules; this is the short form.

## Install

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh    # macOS and Linux
brew install omarmhaimdat/pepe/pepe                     # Homebrew
npx pepe-http --version                                 # npm, no install
pipx install pepe-http                                  # PyPI
uvx pepe-http --version                                 # PyPI, no install
docker run --rm ghcr.io/omarmhaimdat/pepe --version     # Docker
```

## Always

- `--json` for a report, `-n` or `-z` to end the run, `schema_version`
  read first, the exit code used (0 as asked, 1 couldn't start or nothing
  answered, 2 usage or a guardrail, 4 a limit crossed).
- Guardrails on every call when the target came from input:
  `--allow-host .example.com --max-requests 5000 --max-rate 500
  --max-concurrency 64`, and `--dry-run` first to see what would be sent.
- Only point pepe at what you are allowed to load.

## Diagnose one endpoint

```bash
pepe ping https://api.example.com/health -n 5 --json
```

Read `targets[0]`:

- `latency_ms`: `min`, `avg`, `max`, `jitter`, `p50`, `p95`, `p99`.
- `phases_ms`: `dns`, `connect`, `tls_full`, `tls_resumed`, `ttfb`,
  `download`, each with `median_ms`, `p99_ms` and `max_ms`. The largest
  median is the bottleneck. `dns` high: the resolver. `connect` high:
  distance or a slow accept queue. `tls_full` high but `tls_resumed`
  low: the handshake is the cost; keep connections alive. `ttfb` high:
  the server itself. `download` high: the body's size or the link.
- `findings`: what to look at, most serious first (`level`, `text`).
- `loss`, `timeouts`, `failures`: what never answered, by cause.
- `tls.cert.days_left` and `tls.cert.valid`: the certificate.
- `status_codes`: what it answered with.

`--slo total=500,ttfb=200` adds `slo.pass` and makes the exit code 4
when a limit was broken. `--once` prints the same as a few lines.

## Watch it over time

```bash
pepe ping https://api.example.com/health --jsonl -z 5m > pings.jsonl
```

One JSON object per ping (`timings_ms`, `status`, `error`, `tls`) as it
happens. `--csv` does the same as CSV. A person at a terminal gets the
graph instead: `pepe ping URL` with no flags.

## Make it faster, and prove it

When the time is the server's own (`ttfb` the largest phase), see inside it.
Have the service export OpenTelemetry to pepe and load it with `--otlp`:

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 ./my-service &
pepe --json --otlp :4318 -n 2000 -c 16 http://localhost:8080/checkout > before.json
```

- `spans.operations[]`: each operation (`service`, `name`, `kind`), the one
  with the most of the slowest requests' time first: `median_ms`,
  `calls_per_request`, `share_percent` (its own time, less what it called,
  over all requests) and `slowest_share_percent` (over the slowest). Small
  in `share_percent` and large in `slowest_share_percent`: it waits on a
  lock, a pool or a queue.
- `summary.slowest_requests[].spent`: one slow request's time by operation,
  with its `trace_id`.
- `spans.coverage_percent` low, or no `spans.operations`: the `verdict`
  findings say what to set on the service (the exporter's endpoint,
  `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf`,
  `OTEL_EXPORTER_OTLP_COMPRESSION=none`, W3C trace context). A service with
  no OpenTelemetry: add its language's SDK or zero-code instrumentation.

Then change the code behind the top operation, restart the service, run the
same command into `after.json`, and `pepe compare before.json after.json
--json --gate`: `verdict`, and `operations[]` with each operation before
and after. Slower, or nothing moved: undo the change. Keep the load the
same between the two runs. The MCP server has this loop as its `speed-up`
prompt (`/pepe:speed-up URL` in Claude Code), and its tools take `save`
to write the report for `compare`.

## Then load-test, find the capacity, compare

```bash
pepe --json -z 30s -c 50 --rate 500 --fail-if 'p99 > 500ms' https://api.example.com/health
pepe ramp --json https://api.example.com/health --to 200 --until 'p99 > 500ms'
pepe compare before.json after.json --json --gate
```

Read `summary` (counts, `latency`, `failures`, `anomalies`) and
`generator.peak_busy_percent` (near 100: add `--threads auto`); a ramp's
`holds_concurrency` and `capacity`; a compare's `verdict`, `changes[]`
and `findings`. The same `-H`, `-m`, `-d`, `-k` and `-t` apply to every
mode. For a different request each time, put `{{name}}` holes in the URL,
headers or body and pass `--vars users.csv` (a CSV whose first line
names them; a row a request, or a row a chain in a flow).

## As an MCP server

`pepe mcp` serves the modes as tools over stdio: `run`, `ping`, `ramp`,
`api`, `flow`, `replay` and `compare`, each returning the JSON report;
`run` takes `otlp`, and the others but `compare` take `save`.
The guardrail flags given to `pepe mcp` hold for every call, and a call
can't loosen them; a `ping` without `count` or `duration` sends five.

```bash
claude mcp add pepe -- pepe mcp --allow-host .staging.example.com --max-requests 10000 --max-rate 500
codex mcp add pepe -- pepe mcp --allow-host .staging.example.com --max-requests 10000 --max-rate 500
```

Without an install, from the MCP registry (`io.github.omarmhaimdat/pepe`):
`uvx pepe-http mcp`, or `docker run -i --rm --network host
ghcr.io/omarmhaimdat/pepe mcp`, each with the guardrail flags after `mcp`.
As a Claude Code plugin (`claude plugin marketplace add omarmhaimdat/pepe`,
`claude plugin install pepe@pepe`) the server comes capped at 10000
requests, 500 a second and 64 connections; the `allow-host` goes in the
project's `pepe.toml`.
