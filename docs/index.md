:::hero
![pepe](assets/logo.svg)
# pepe
An HTTP load generator with a live dashboard, built to cost less than the server it tests. Load-test a URL, ping it to see where the time goes, ramp to find the capacity, and read the result as a verdict or a JSON report.
[Get started](install.md) [Read the docs](load-test.md) [View on GitHub](https://github.com/omarmhaimdat/pepe)
:::

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh
pepe https://example.com
```

![pepe: a ping that says where the time goes, then a run with the live dashboard and the verdict it leaves](assets/home.gif)

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

The plan for what comes next is in [ROADMAP.md](ROADMAP.md), and what changed in each release in [CHANGELOG.md](CHANGELOG.md).
