# pepe

pepe is an HTTP load generator with a live dashboard, built to cost less than the server it tests. It sends requests to a URL, as many at once as you ask, and shows what came back as it happens: throughput, latency percentiles and a heatmap, status codes, failures by cause, and a log you can open any request from. When the run ends it gives a verdict in plain words. One binary, written in Rust, for macOS, Linux and Windows.

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh
pepe https://example.com
```

![pepe load-testing a server: the live dashboard, the request log, and the verdict](assets/run.gif)

## What it does

| Command | What for | Page |
| --- | --- | --- |
| `pepe URL` | A load test with the live dashboard; `--json` for a report, piped for the verdict | [Load testing](load-test.md), [The dashboard](dashboard.md) |
| `pepe ping URL...` | A request a second, each split into DNS, connect, TLS, first byte and download, on a graph; `--once` for a quick diagnosis | [Ping](ping.md) |
| `pepe ramp URL` | Raise the load step by step and find the level that held, where throughput stopped following, and where it broke | [Ramp](ramp.md) |
| `pepe api SPEC` | Load-test the endpoints of an OpenAPI 3 or Swagger 2 spec, picked on screen or by tag | [OpenAPI](api.md) |
| `pepe flow FILE` | A sequence of requests, each step fed by the one before: log in, take the token, use it | [Flows](flow.md) |
| `pepe replay LOG` | The URLs of an access log, in their real proportions, sent at another host | [Replay](replay.md) |
| `pepe logs FILES` | What nginx's logs say: the request rate now against each minute, hour and day, paths, errors and the lines themselves | [nginx logs](logs.md) |
| `pepe compare A B` | What moved between two reports, in the verdict's words; a badge and an SVG card for CI | [Compare](compare.md) |
| `pepe schema` | The JSON Schema of each report | [Output and exit codes](output.md) |
| `pepe self-update` | The newest release, with what changed | [Install](install.md) |

Everything a mode does has a flag, a config key and a JSON field, so what works at the terminal works in a script, a CI job or an agent: see [Output and exit codes](output.md), [CI and Docker](ci.md) and [Agents and scripts](agents.md). Every flag of every command is in the [command reference](reference.md).

## Highlights

- **Live dashboard** with a latency heatmap, percentile and throughput charts, status codes, failure causes and a scrollable, filterable request log. `enter` on a request shows its headers and body, formatted.
- **A verdict**, not just numbers: Healthy, Degraded or Failing, with findings such as two latency groups, a long tail, or throughput drifting over the run, and anomalies called out as they happen.
- **Interactive control**: pause, resume, stop, restart, and raise or lower concurrency while the run is going.
- **Any curl command** from a browser's "Copy as cURL", Postman or Insomnia, sent exactly as curl would.
- **A setup screen**: run `pepe` with no arguments and fill in every option on a form, with the equivalent command shown as you go.
- **What the server says**: `Server-Timing` headers added up and held against the latency measured here, and the slowest requests listed with the ids their backend gave them, ready to search in its logs.
- **Light**: a share-nothing engine measured against oha, vegeta, wrk and k6 on every release; one thread sends 160k requests a second on an Apple M4 Pro and 400k on Linux, and pepe says when it, rather than the target, is the limit. The numbers are on the [benchmarks](benchmarks.md) page.

## Where to start

1. [Install](install.md) it, with the one-line installer, Homebrew, Nix, Docker or a prebuilt binary.
2. Run `pepe https://your.service/health` and read the [dashboard](dashboard.md).
3. Leave `pepe ping https://your.service/health` open while you deploy ([Ping](ping.md)).
4. Put the test next to the code as a `pepe.toml` ([Load testing: config file](load-test.md#config-file)) and run it in CI with the [GitHub Action](ci.md).

The plan for what comes next is in [ROADMAP.md](ROADMAP.md), and what changed in each release in [CHANGELOG.md](CHANGELOG.md).
