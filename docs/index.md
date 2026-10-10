# pepe documentation

pepe is an HTTP load generator with a live dashboard, built to cost less than the server it tests. One binary for macOS, Linux and Windows: load-test a URL and watch, ping it and see where the time goes, ramp to find the capacity, replay a log, test a whole OpenAPI spec, and read the result as a verdict in plain words or as a versioned JSON report.

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh
pepe https://example.com
```

![pepe: a ping that says where the time goes, then a run with the live dashboard and the verdict it leaves](assets/home.gif)

## Pick a page

:::cards
- [Install](install.md) — The installer, Homebrew, Nix, Docker, prebuilt binaries, updates, completions.
- [Load testing](load-test.md) — `pepe URL`: every option, the load's shape, warm-up, arrival rate, threads, the config file, a curl command, the setup screen.
- [The dashboard](dashboard.md) — The views, the verdict, the inspector, the filters, and every key.
- [Ping](ping.md) — `pepe ping`: a request a second split into DNS, connect, TLS, first byte and download, with a graph and what to look at.
- [Ramp](ramp.md) — `pepe ramp`: raise the load step by step, find the level that held and the capacity.
- [OpenAPI](api.md) — `pepe api`: the endpoints of a spec, picked on screen or by tag, with their parameters.
- [Flows](flow.md) — `pepe flow`: a sequence of requests, each step fed by the one before.
- [Replay](replay.md) — `pepe replay`: an access log's URLs in their real proportions, sent at another host.
- [nginx logs](logs.md) — `pepe logs`: the request rate now against each minute, hour and day, paths, errors and lines.
- [Compare](compare.md) — `pepe compare`: what moved between two reports, as a verdict, a badge and a card.
- [Output and exit codes](output.md) — The report in the shell, JSON and its schema, `--fail-if`, Prometheus metrics, exit codes.
- [CI and Docker](ci.md) — The GitHub Action, a pull request held against its base branch, Docker, soak runs.
- [Agents and scripts](agents.md) — What a program needs to know to call pepe and read the answer.
- [Command reference](reference.md) — Every command and flag, as `--help` prints them.
:::

## Where to start

1. [Install](install.md) it.
2. Run `pepe https://your.service/health` and read the [dashboard](dashboard.md); `q` leaves the verdict in your shell.
3. Leave `pepe ping https://your.service/health` open while you deploy, or run `pepe ping URL --once` to see why it is slow ([Ping](ping.md)).
4. Put the test next to the code as a `pepe.toml` ([config file](load-test.md#config-file)) and run it in CI with the [GitHub Action](ci.md) or `--fail-if` ([Output](output.md#thresholds-that-fail-ci)).

## Why pepe

- **A verdict, not just numbers.** Healthy, Degraded or Failing, with findings such as two latency groups, a long tail, throughput drifting, or what a 503 actually said. Anomalies are called out as they happen.
- **Everything live and in hand.** Pause, raise or lower the concurrency, open any request, filter the log, restart.
- **Any request.** Headers, bodies and methods; a curl command from a browser's "Copy as cURL"; an OpenAPI spec; a flow file; an access log.
- **Light.** One thread sends 160k requests a second on an Apple M4 Pro and 400k on Linux, at less CPU and memory per request than wrk, oha, vegeta or k6, and pepe says when it rather than the target is the limit ([Benchmarks](benchmarks.md)).
- **Made for scripts and agents too.** Every mode runs without a screen, prints a versioned JSON report with a published schema, and says what went wrong in an exit code.

The plan for what comes next is in [ROADMAP.md](ROADMAP.md), and what changed in each release in [CHANGELOG.md](CHANGELOG.md).
