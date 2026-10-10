<div align="center">

<img src="assets/logo.svg" width="112" alt="Pepe, a pixel-art chili pepper">

# pepe

**An HTTP load generator with a live dashboard, built to cost less than the server it tests.**

[![CI](https://github.com/omarmhaimdat/pepe/actions/workflows/CI.yaml/badge.svg)](https://github.com/omarmhaimdat/pepe/actions/workflows/CI.yaml) [![Release](https://img.shields.io/github/v/release/omarmhaimdat/pepe?display_name=tag&color=brightgreen)](https://github.com/omarmhaimdat/pepe/releases/latest) [![Downloads](https://img.shields.io/github/downloads/omarmhaimdat/pepe/total?color=blue)](https://github.com/omarmhaimdat/pepe/releases) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) ![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange)

[Install](#install) · [Quick start](#quick-start) · [What it does](#what-it-does) · [Scripts, CI and agents](#scripts-ci-and-agents) · [How it compares](#how-pepe-compares) · [Documentation](https://pepe.mhaimdat.com/docs/) · [Roadmap](ROADMAP.md)

</div>

![pepe load-testing a server: the live dashboard, the request log, and the verdict](assets/run.gif)

pepe sends requests to a URL, as many at once as you ask, and shows what came back as it happens: throughput, latency percentiles and a heatmap, status codes, failures by cause, and a log you can open any request from. When the run ends it gives a verdict in plain words, and says where the time went. It takes a curl command as input, ramps load to find where a target stops keeping up, load-tests every endpoint of an OpenAPI spec, replays an access log, and runs a flow of requests where each step feeds the next. `pepe ping` is the same engine at one request a second: a graph of a URL's latency over time, each ping split into DNS, connect, TLS, first byte and download, with what to look at.

It is also light. One thread sends 160k requests a second on an Apple M4 Pro and 400k on Linux, for less CPU and less memory per request than [wrk](https://github.com/wg/wrk), a quarter to a half of [oha](https://github.com/hatoo/oha)'s CPU and a tenth of its memory or less, and pepe tells you when it, rather than the target, is the limit.

## Install

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh        # macOS and Linux
brew install omarmhaimdat/pepe/pepe                         # Homebrew
nix run github:omarmhaimdat/pepe -- https://example.com     # Nix, without installing
docker run --rm -it ghcr.io/omarmhaimdat/pepe https://example.com
cargo install --locked --git https://github.com/omarmhaimdat/pepe
```

Windows, in PowerShell: `powershell -ExecutionPolicy Bypass -c "irm https://pepe.mhaimdat.com/install.ps1 | iex"`. Every [release](https://github.com/omarmhaimdat/pepe/releases) also ships binaries for macOS, Linux and Windows with checksums and signed provenance, the shell completions and the man pages. `pepe self-update` installs the latest release; pepe looks for one once a day and says so when a run ends. All of it, with the details, on the [install page](https://pepe.mhaimdat.com/docs/install.html).

## Quick start

```bash
pepe https://example.com                                     # 100 requests, one per core at a time, and watch
pepe -z 30s -c 50 -m POST -H 'Content-Type: application/json' -d '{"key":"value"}' https://httpbin.org/post
pepe -z 30s --curl -- curl 'https://api.example.com/items' -H 'Authorization: Bearer …'   # from "Copy as cURL"
pepe ping https://example.com --once                         # three pings, then why it is slow, and out
pepe                                                         # the setup screen: every option on a form
```

In the dashboard, `space` pauses, `+` and `-` change concurrency, `tab` switches view, `enter` inspects a request, `?` lists every key, `q` quits and leaves the verdict in your shell.

## What it does

| Command | What for | Docs |
| --- | --- | --- |
| `pepe URL` | A load test with the live dashboard: the load's shape (`--number`, `--duration`, `--concurrency`, `--rate`, `--warmup`, `--threads`), any header, body or method, a curl command, a `pepe.toml` next to the code, soak runs with `--snapshot` | [Load testing](https://pepe.mhaimdat.com/docs/load-test.html), [The dashboard](https://pepe.mhaimdat.com/docs/dashboard.html) |
| `pepe ping URL…` | A request a second, each split into DNS, connect, TLS, first byte and download, on a graph; several targets, TCP pings, `--slo` limits, HTTP/2, the certificate's expiry, and findings that say what to look at | [Ping](https://pepe.mhaimdat.com/docs/ping.html) |
| `pepe ramp URL` | Raise the concurrency step by step and find the level that held, where throughput stopped following, where it broke, and the capacity estimate | [Ramp](https://pepe.mhaimdat.com/docs/ramp.html) |
| `pepe api SPEC` | The endpoints of an OpenAPI 3 or Swagger 2 spec, picked on screen or by tag, with their parameters and credentials | [OpenAPI](https://pepe.mhaimdat.com/docs/api.html) |
| `pepe flow FILE` | A sequence of requests, each step fed by the one before: log in, take the token, use it | [Flows](https://pepe.mhaimdat.com/docs/flow.html) |
| `pepe replay LOG` | An access log's URLs in their real proportions, sent at another host | [Replay](https://pepe.mhaimdat.com/docs/replay.html) |
| `pepe logs FILES` | What nginx's logs say: the request rate now against each minute, hour and day, paths, errors and the lines themselves, live | [nginx logs](https://pepe.mhaimdat.com/docs/logs.html) |
| `pepe compare A B` | What moved between two reports, in the verdict's words, with a badge and an SVG card | [Compare](https://pepe.mhaimdat.com/docs/compare.html) |

Every flag of every command is in the [command reference](https://pepe.mhaimdat.com/docs/reference.html), and `man pepe` has the same.

## Scripts, CI and agents

Every mode runs without a screen and is built to be called by a program:

- **A versioned JSON report** with `--json` (`schema_version: 1`; `pepe schema` prints each report's JSON Schema), with the verdict and its findings in it. Piped without `--json`, the text verdict.
- **Exit codes that mean something**: 0 as asked, 1 couldn't start or nothing answered, 2 a usage error or a guardrail, 4 a limit crossed (`--fail-if 'p99 > 300ms'`, a ping's `--slo`).
- **Guardrails** checked before anything is sent: `--allow-host .example.com`, `--max-requests`, `--max-rate`, `--max-concurrency`, and `--dry-run`, which says what would be sent and sends nothing. They can live in `pepe.toml`.
- **A GitHub Action** that runs a test, puts the numbers in the job summary and outputs, holds a pull request against its base branch with `pepe compare`, comments the result and can fail the job: [CI and Docker](https://pepe.mhaimdat.com/docs/ci.html).
- **Prometheus metrics** with `--metrics :9100`, for a soak run or a long ping in Grafana.
- **An MCP server** (`pepe mcp`) that serves the modes as tools for an agent, under the guardrails it was started with; [AGENTS.md](AGENTS.md) is what an agent reads first.

```yaml
- uses: omarmhaimdat/pepe@v0
  with:
    url: https://staging.example.com/api/health
    args: -z 30s -c 20 --fail-if 'p99 > 300ms'
    baseline: auto
    comment: true
```

## How pepe compares

Measured with the suite in [`bench/`](bench/) against a local server that answers from memory, so the client is the cost being measured. CPU milliseconds per 1,000 requests, peak memory, and the requests per second each tool reported:

| Workload | pepe | oha | vegeta |
| --- | --- | --- | --- |
| GET, 64 connections | **6.2 ms · 9 MB** · 160k req/s | 23.0 ms · 78 MB · 161k req/s | 90.4 ms · 26 MB · 76k req/s |
| GET, 1,000 connections | **6.0 ms · 12 MB** · 163k req/s | 21.6 ms · 101 MB · 147k req/s | 52.7 ms · 92 MB · 131k req/s |
| HTTPS, 64 connections | **6.8 ms · 12 MB** · 142k req/s | 24.8 ms · 60 MB · 156k req/s | 69.8 ms · 30 MB · 90k req/s |

Those are single-thread numbers for pepe on an Apple M4 Pro, where the loopback tops out near 175k requests a second. On Linux (a 4-vCPU arm64 VM, the musl build that is released), with wrk on one thread beside it:

| Workload | pepe | wrk | oha |
| --- | --- | --- | --- |
| GET, 64 connections | **2.4 ms · 4.0 MB** · 398k req/s | 3.5 ms · 4.5 MB · 282k req/s | 6.8 ms · 67 MB · 305k req/s |
| GET, 1,000 connections | **3.2 ms · 5.8 MB** · 296k req/s | 4.4 ms · 7.7 MB · 228k req/s | 5.6 ms · 73 MB · 183k req/s |
| HTTPS, 64 connections | **3.3 ms · 6.0 MB** · 284k req/s | 4.5 ms · 10.8 MB · 218k req/s | 8.1 ms · 42 MB · 250k req/s |
| 10 million requests, 256 connections | **2.9 ms · 4.5 MB** · 343k req/s | 3.3 ms · 4.6 MB · 304k req/s | 6.5 ms · 2,404 MB · 316k req/s |

Every workload, where pepe is level rather than ahead (a slow target at 1,000 connections, 16 KB bodies over TLS), what a glibc build changes, k6, the profiles and the method are in [bench/README.md](bench/README.md). Where a target can take more than one thread sends, pepe says so, and `--threads auto` adds them.

## Documentation

The manual, page by page, with every option and example, is at [pepe.mhaimdat.com/docs](https://pepe.mhaimdat.com/docs/). Its sources are the Markdown files under [docs/](docs/), and the [command reference](https://pepe.mhaimdat.com/docs/reference.html) is generated from the command definition, so it can't drift from the binary. What changed in each release is in [CHANGELOG.md](CHANGELOG.md); what comes next, in order, in [ROADMAP.md](ROADMAP.md).

## Contributing

Issues and pull requests are welcome; [CONTRIBUTING.md](CONTRIBUTING.md) has the build, the tests, the generated files, the recordings, the benchmarks and how releases are made.

## License

MIT. See [LICENSE](LICENSE).

## Acknowledgements

[tokio](https://github.com/tokio-rs/tokio), [rustls](https://github.com/rustls/rustls), [httparse](https://github.com/seanmonstar/httparse), [h2](https://github.com/hyperium/h2) and [reqwest](https://github.com/seanmonstar/reqwest) for the requests, [ratatui](https://github.com/ratatui/ratatui) and [crossterm](https://github.com/crossterm-rs/crossterm) for the dashboard, [clap](https://github.com/clap-rs/clap) for the command line, and [oha](https://github.com/hatoo/oha), [vegeta](https://github.com/tsenart/vegeta), [wrk](https://github.com/wg/wrk) and [k6](https://github.com/grafana/k6) for being good company on the benchmark table.
