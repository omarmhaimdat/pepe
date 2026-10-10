# pepe for agents

pepe is an HTTP load generator and latency tool, one binary: `pepe` (a load
test), `pepe ping` (a request a second, split into DNS, connect, TLS, first
byte and download), `pepe ramp` (find the capacity), `pepe api` (an OpenAPI
spec), `pepe flow` (a sequence of requests), `pepe replay` (an access log),
`pepe logs` (nginx's logs), `pepe compare` (two reports). This file is what
a coding agent needs to call it safely and read the answer. The whole
documentation is at https://pepe.mhaimdat.com/docs/ and in `docs/`.

## Always

- Pass `--json` for a report on stdout, or let stdout be piped for the
  text verdict. There is never a screen to drive.
- End the run: `-n N` for a count, or `-z 30s` for a duration. A ping
  without either runs until stopped.
- Read `schema_version` first; the field names it covers never change.
  `pepe schema run|ramp|ping|compare` prints each report's JSON Schema.
- Use the exit code: 0 as asked; 1 couldn't start, nothing answered, or
  a regression under `pepe compare --gate`; 2 a usage error (including a
  guardrail refusing the run); 4 a limit crossed (`--fail-if`, a ping's
  `--slo`). The report is printed before a non-zero code.
- Only send to what you are allowed to load. A load test is traffic.
- `PEPE_NO_UPDATE_CHECK=1` in the environment keeps the update notice off.

## Guardrails

Set these on every call, or once in a `pepe.toml` next to the code, so a
wrong URL or a wrong number does no harm. They are checked before
anything is sent, in every mode, and a refusal is exit code 2 with the
reason on stderr.

| Flag | Holds |
| --- | --- |
| `--allow-host .example.com` | Every target's host must be `example.com` or under it; `api.example.com` is that host exactly; repeat for more. A flow step whose host comes from a capture, and a ping of a command (`--cmd`), are refused under it. |
| `--max-requests 5000` | `-n` must be under it; `-z` needs a `--rate` that bounds it; a ping needs `-n` or `-z`; a ramp is refused (it has no bound). |
| `--max-rate 500` | `--rate` must be under it, and must be given: without it a run sends as fast as the target answers. A ping's targets over its interval count. |
| `--max-concurrency 64` | `-c` and a ramp's top step must be under it. |
| `--dry-run` | Says what would be sent, to where and how much (secrets in headers masked), and sends nothing; `--json` gives it as JSON with `"sent": 0`. Use it before the real call when the URL came from input. |

```toml
# pepe.toml, next to the code: holds for every run in that directory
allow-host = [".staging.example.com", "localhost"]
max-requests = 10000
max-rate = 500
max-concurrency = 64
```

## Diagnose one endpoint

```bash
pepe ping https://api.example.com/health -n 5 --json
```

Read `targets[0]`: `latency_ms` (`min`, `avg`, `max`, `jitter`, `p50`,
`p95`, `p99`); `phases_ms` (`dns`, `connect`, `tls_full`, `tls_resumed`,
`ttfb`, `download`, each `median_ms`, `p99_ms`, `max_ms`; the largest
median is the bottleneck); `loss`, `timeouts`, `failures` by cause;
`tls.cert.days_left`, `tls.cert.valid`; `status_codes`; `findings`, what
to look at, most serious first, each with `level` (`bad`, `warn`,
`note`, `good`) and `text`. `--once` prints the same as a few lines.
`--slo total=500,ttfb=200` adds `slo.pass` and exit code 4 when broken.

## Load-test

```bash
pepe --json -z 30s -c 50 --rate 500 --fail-if 'p99 > 500ms' --fail-if 'errors > 1%' https://api.example.com/health
```

Read `summary`: `total_requests`, `failed_requests`,
`requests_per_second`, `latency.median_ms`, `latency.p99_ms`,
`latency.phases`, `status_codes`, `failures` with the first body of each
cause, `anomalies`. Read `generator.peak_busy_percent`: near 100, pepe
rather than the target was the limit, so add `--threads auto` and run
again. `--rate` is an open loop (arrivals at that rate, as users
arrive); without it the run is a closed loop that finds the most the
target can do.

## Find the capacity

```bash
pepe ramp --json https://api.example.com/health --to 200 --until 'p99 > 500ms'
```

Read `holds_concurrency`, `capacity.requests_per_second` (`null` when
the curve hadn't bent; `extrapolated` when read past the ramp), each
`steps[]`, and `findings`.

## Compare before and after

```bash
pepe --json -n 5000 -c 20 "$URL" > before.json
pepe --json -n 5000 -c 20 "$URL" > after.json
pepe compare before.json after.json --json --gate
```

Read `verdict` (`faster`, `same`, `slower`, `better`, `worse`),
`regression`, `changes[]` with `significant` and `noise_percent`, and
`findings`. Ping reports compare too, phase by phase.

## A whole API, a journey, real traffic

`pepe api openapi.yaml --all --json` (writes need `--include-writes`;
`--set id=1,2,3` gives values) with `endpoints[]`; `pepe flow
checkout.toml --json` with `flow.steps[]`; `pepe replay access.log
--base-url "$STAGING" --json` with `replay.urls[]`.

## The same request in every mode

`-H 'Name: value'`, `-m`, `-d`, `-u`, `-p`, `-k`, `-t`,
`--disable-compression`, `--disable-keepalive`, `--disable-redirects`.
`--curl -- curl ...` takes a request as a curl command.

## Live numbers

`--metrics :9100` serves Prometheus metrics at `/metrics` while a run or
a ping goes. `--snapshot soak.json` keeps a long run's report current on
disk every minute.

## As an MCP server

`pepe mcp` serves the modes above as tools over stdio (`run`, `ping`,
`ramp`, `compare`, `api`, `flow`, `replay`), each returning the JSON
report. Guardrail flags given to `pepe mcp` apply to every call.
