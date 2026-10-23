# Agents and scripts

pepe is built to be called by a program as readily as by a person: every mode runs without a screen, prints a versioned JSON report with a published schema, and says what went wrong in an exit code. This page is what a coding agent or a script needs to know; a skill file for agents is in the repository under `skills/pepe/`.

## The rules

- Pass `--json` for a report, or let the output be piped for the text verdict; never expect a screen.
- End the run: `-n` for a count or `-z` for a duration. A ping without either runs until stopped.
- Read `schema_version` first; the field names it covers never change. `pepe schema run|ramp|ping|compare` prints the schema.
- Use the exit code: 0 as asked, 1 couldn't start or nothing answered, 2 a usage error or a guardrail refusing the run, 4 a limit crossed ([Output: exit codes](output.md#exit-codes)).
- Only point pepe at what you are allowed to load. A load test is traffic. The guardrails below make that a setting rather than a hope.

The repository carries the same in [AGENTS.md](AGENTS.md), what an agent reads first, and the site serves [llms.txt](https://pepe.mhaimdat.com/llms.txt).

## Guardrails

Where a run may be pointed and how much it may send, checked in every mode before anything is sent. A refusal is exit code 2 with the reason on stderr. Set them on every call, or once in a `pepe.toml` next to the code (`allow-host`, `max-requests`, `max-rate`, `max-concurrency`), where the command line can't loosen them by leaving them out.

| Flag | Holds |
| --- | --- |
| `--allow-host .example.com` | Every target's host must be `example.com` or under it; `api.example.com` is that host exactly; repeat for more. A flow step whose host comes from a capture, and a ping of a command (`--cmd`), are refused under it |
| `--max-requests 5000` | `-n` must be under it; `-z` needs a `--rate` that bounds it; a ping needs `-n` or `-z`; a ramp is refused, having no bound |
| `--max-rate 500` | `--rate` must be under it, and must be given: without it a run sends as fast as the target answers. A ping's targets over its interval count |
| `--max-concurrency 64` | `-c`, and a ramp's top step, must be under it |
| `--dry-run` | Says what would be sent, to where and how much, with secrets in headers masked, and sends nothing; `--json` gives it as JSON with `"sent": 0`. Use it before the real call when the URL came from input |

```bash
pepe --allow-host .staging.example.com --max-requests 5000 --max-rate 500 --max-concurrency 64 \
     --json -z 10s --rate 200 -c 32 https://api.staging.example.com/health
pepe --allow-host .staging.example.com --dry-run -n 100 https://api.staging.example.com/health
```

```
pepe · dry run · nothing sent
  mode      run
  target    https://api.staging.example.com/health
  method    GET
  load      100 requests, 8 at a time, each sent as soon as the last answered
  settings  timeout 20s · connections kept alive · redirects followed
  guard     hosts .staging.example.com
```

## As an MCP server

`pepe mcp` serves the modes as tools over stdio, in the Model Context Protocol: `run`, `ping`, `ramp`, `api`, `flow`, `replay` and `compare`, each returning pepe's JSON report as text and as structured content. The guardrail flags `pepe mcp` is started with hold for every call, so whoever starts the server decides where it may send and how much, and a tool call can't loosen them.

```bash
pepe mcp --allow-host .staging.example.com --max-requests 10000 --max-rate 500 --max-concurrency 64
```

pepe is listed in the [MCP registry](https://registry.modelcontextprotocol.io) as `io.github.omarmhaimdat/pepe`, as the PyPI package and the Docker image, so a client that reads the registry can start it without an install: `uvx pepe-http mcp`, or `docker run -i --rm --network host ghcr.io/omarmhaimdat/pepe mcp`, each with the guardrail flags after `mcp`.

For Claude Code:

```bash
claude mcp add pepe -- pepe mcp --allow-host .staging.example.com --max-requests 10000 --max-rate 500
```

For a client configured with JSON (Claude Desktop, Cursor and the like):

```json
{
  "mcpServers": {
    "pepe": {
      "command": "pepe",
      "args": ["mcp", "--allow-host", ".staging.example.com", "--max-requests", "10000", "--max-rate", "500"]
    }
  }
}
```

In Cursor, opening this link adds the server with the plugin's caps:

```
cursor://anysphere.cursor-deeplink/mcp/install?name=pepe&config=eyJjb21tYW5kIjoicGVwZSIsImFyZ3MiOlsibWNwIiwiLS1tYXgtcmVxdWVzdHMiLCIxMDAwMCIsIi0tbWF4LXJhdGUiLCI1MDAiLCItLW1heC1jb25jdXJyZW5jeSIsIjY0Il19
```

A tool call that pepe refuses (a host outside the allowed, a cap crossed) comes back as an error with pepe's reason; a limit crossed during the run (`fail_if`, a ping's `slo`) comes back as the report with a note, since the report is the answer. A `ping` without `count` or `duration` sends five pings, so no call runs forever.

### In Codex

Codex reads `AGENTS.md`, and takes MCP servers the same way:

```bash
codex mcp add pepe -- pepe mcp --max-requests 10000 --max-rate 500 --max-concurrency 64
```

or in `~/.codex/config.toml`:

```toml
[mcp_servers.pepe]
command = "pepe"
args = ["mcp", "--max-requests", "10000", "--max-rate", "500", "--max-concurrency", "64"]
```

The repository is laid out as a Codex plugin too (a `plugin.json` and an `mcp.json` at its root, and `.agents/plugins/marketplace.json`), with the same skill and server: `codex plugin marketplace add omarmhaimdat/pepe`, then `/plugins` in Codex to install it.

### The skill alone

`npx skills add omarmhaimdat/pepe` puts the `pepe` skill where Claude Code, Codex and the other agents that read [Agent Skills](https://agentskills.io) look for it, without the server.

### As a Claude Code plugin

The repository is also a Claude Code plugin. Installing it brings the `pepe` skill (how to call pepe and what to read in each report) and the MCP server above, started as `pepe mcp --max-requests 10000 --max-rate 500 --max-concurrency 64`. The plugin does not install pepe: the binary must be on `PATH` first.

```bash
claude plugin marketplace add omarmhaimdat/pepe
claude plugin install pepe@pepe
```

The server ships with no `--allow-host`, since a host it is allowed to load is yours to name. Put one in a `pepe.toml` next to the code (see [Guardrails](#guardrails)); pepe reads it in every mode, the MCP server included.

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

Read `verdict`: its `level` and `findings`, what the dashboard would have said, including where the time went. Then `summary`: `total_requests`, `failed_requests`, `requests_per_second`, `latency.median_ms`, `latency.p99_ms`, `latency.phases`, `status_codes`, `failures` with the first body of each cause, `anomalies`. Read `generator.peak_busy_percent`: near 100, pepe rather than the target was the limit, so add `--threads auto` and run again. `--fail-if` names what the run must not cross; the exit code says whether it did.

## Find the capacity

```bash
pepe ramp --json https://api.example.com/health --to 200 --until 'p99 > 500ms'
```

Read `holds_concurrency` (the level that held), `capacity.requests_per_second` (the estimate, `null` when the curve hadn't bent; `extrapolated` when it was read past the ramp), each `steps[]` entry, and `findings`.

## Speed up an endpoint

The loop an agent can close on its own: measure, see which part of the service the time goes to, change the code, measure again, and let the comparison say whether it worked. `--otlp` (a run's `otlp` in the MCP server) takes the service's OpenTelemetry spans, and the report's `spans` name the operation behind the time ([Inside the service](load-test.md#inside-the-service)).

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 ./my-service &
pepe --json --otlp :4318 -n 2000 -c 16 http://localhost:8080/checkout > before.json
# change the code behind spans.operations[0], restart the service
pepe --json --otlp :4318 -n 2000 -c 16 http://localhost:8080/checkout > after.json
pepe compare before.json after.json --json --gate
```

Read `spans.operations[]`, the operation with the most of the slowest requests' time first: `name`, `service`, `kind`, `calls_per_request`, `median_ms`, `share_percent` (its own time over all requests) and `slowest_share_percent` (over the slowest); `spans.coverage_percent`, how many requests the service sent spans for; and `summary.slowest_requests[].spent`, one slow request's time by operation, with its `trace_id`. The comparison's `operations[]` holds each operation before and after, and its `findings` say what moved inside the service. No spans, or few, and the `verdict` findings say what to set on the service.

The MCP server has the loop as a prompt, `speed-up`, with a `url` and an optional `goal`: in Claude Code it is the `/pepe:speed-up` command (or `/mcp__pepe__speed-up`). The `run`, `ping`, `ramp`, `api`, `flow` and `replay` tools take a `save` path, so their reports can be handed to `compare`.

## Compare before and after

```bash
pepe --json -n 5000 -c 20 "$URL" > before.json
# change something
pepe --json -n 5000 -c 20 "$URL" > after.json
pepe compare before.json after.json --json --gate
```

Read `verdict` (`faster`, `same`, `slower`, `better`, `worse`), `regression`, each `changes[]` entry with `significant` (moved more than two such runs wobble) and `noise_percent`, and `findings`. Ping reports compare too, phase by phase.

## Watch nginx

```bash
pepe logs /var/log/nginx/access.log /var/log/nginx/error.log --since 15m --json --fail-if '5xx > 2%' --fail-if 'rate < 5/s'
```

Read `verdict.level` (`healthy`, `degraded`, `failing`) and `verdict.findings[]`, what is out of the usual in the last minute, most serious first, each with `level`, `kind`, `text`, and `path`, `since`, `now`, `usual` when they apply; and `fail_if[]` with `condition`, `held` and `measured`. A crossed limit is exit code 4, and `--webhook` sends the verdict, so a cron line is an alert. See [nginx logs](logs.md#what-is-wrong).

## Test a whole API or a journey

`pepe api openapi.yaml --all --json` runs every endpoint that has the values it needs (`--set id=1,2,3` gives values; writes need `--include-writes`), with `endpoints[]` in the report. `pepe flow checkout.toml --json` runs a sequence where each step feeds the next, with `flow.steps[]`. `pepe replay access.log --base-url "$STAGING" --json` sends a log's URLs in their real proportions, with `replay.urls[]`.

## The same request every mode

`-H 'Name: value'`, `-m`, `-d`, `-u`, `-p`, `-k`, `-t`, `--disable-compression`, `--disable-keepalive` and `--disable-redirects` apply to every mode. `--curl -- curl ...` takes a request as a curl command. `--vars users.csv` fills `{{holes}}` in the URL, headers and body from a CSV file, a row a request in turn ([Values from a file](load-test.md#values-from-a-file)). A `pepe.toml` next to the code holds a test's settings so that `pepe` alone runs it ([Config file](load-test.md#config-file)).
