# Roadmap

Where pepe is going, in order. Each theme is a release or two; items
within a theme are listed in the order they're worth doing. Sizes are
rough: **S** is days, **M** a week or two, **L** more than that.

Shipped work moves to [CHANGELOG.md](CHANGELOG.md). Ideas and requests go
in [issues](https://github.com/omarmhaimdat/pepe/issues).

## 1. Project health

Done: tab completion and man pages that install themselves, installer
smoke tests after every release, a benchmark gate on release PRs with a
Linux record and a Linux profile on demand, recordings of each mode
made by script, the install page, and a Windows checklist. The
scheduled benchmark run was set aside; the gate on release PRs covers
what it was for.

## 2. Scale and operations

Runs that are longer, bigger, or part of a pipeline.

- **Docker image and a GitHub Action** (S). In progress: `ghcr.io/omarmhaimdat/pepe`
  and `uses: omarmhaimdat/pepe@master` with a URL, flags and a `fail-if`
  condition.
- **Soak mode** (S). Hours-long runs with periodic snapshots to disk, so a
  crash at hour six doesn't lose the data, and a report that covers the
  whole span.
- **Distributed runs** (L). Several pepe workers, one dashboard:
  `pepe worker --join host:port`. For when one machine's network is the
  limit. The share-nothing engine already thinks in shards; this makes a
  shard a machine.

## 3. Analysis and insights

Say more about what the numbers mean.

- **Latency by phase** (M). DNS, connect, TLS, time to first byte and
  download as separate percentiles, in the dashboard and the report.
- **Error clustering** (S). Group failures by cause and show the first
  response body of each group in the verdict, so "503 from the upstream"
  is visible without opening the log.
- **Anomaly notes during the run** (S). Call out "p99 jumped 4× at 00:42"
  as it happens, and keep the list in the report.
- **Server-Timing and trace headers** (S). Surface `Server-Timing`
  segments, and show the request ids of the slowest requests so backend
  logs can be searched.
- **Capacity estimate from a ramp** (M). Fit the ramp's curve and state
  where throughput saturates and where latency doubles, in the verdict's
  words.

## 4. Scenarios and realism

Load that looks like production.

- **Arrival rate** (M). `--rate 500/s`, an open model next to today's
  fixed concurrency, which is what an SLO question needs: can it hold
  500 requests a second at p99 under 200 ms.
- **Data-driven requests** (M). `--vars users.csv` and `{{email}}` in the
  URL, headers and body, so every request is different.
- **Config file** (S). A `pepe.toml` in a project, so `pepe` there runs
  the agreed test; the setup screen can write it.
- **Warm-up** (S). `--warmup 10s` left out of the numbers, so the first
  connections don't skew the verdict.
- **Request chaining** (L). A run that is a sequence, where a value from
  one response feeds the next; each step a row in the dashboard, as
  endpoints are in API mode.
- **Replay from access logs** (M). Read an nginx, Caddy or ALB log and
  send its URLs in their real proportions.

## 5. Output and integration

Get the results where they're needed.

- **Thresholds that fail CI** (S). `--fail-if 'p99 > 300ms'`, reusing the
  ramp's conditions, with a non-zero exit code.
- **Compare two runs** (M). `pepe compare before.json after.json` says
  what moved, in the verdict's language.
- **CSV and per-request export** (S). One line per request for
  spreadsheets and notebooks.
- **JUnit XML** (S). So CI systems show thresholds as test results.
- **Prometheus metrics** (M). `--metrics :9100` exposes the live numbers,
  so a soak run shows up in Grafana next to the server's own.
- **Webhook summary** (S). Post the verdict when a run ends.
- **HTML report** (M). One self-contained file with the charts, for
  people who weren't at the terminal.

## Not now

Considered and set aside, so they aren't re-argued every month:

- **More protocols** (HTTP/2, gRPC, WebSocket, HTTP/3). HTTP/1.1 is where
  pepe is best; these each deserve a release of their own once the above
  is done.
- **Dashboard extras** (replaying a saved run, side-by-side runs, themes).
  Nice, not pressing.
