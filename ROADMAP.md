# Roadmap

Where pepe is going, in order. Each theme is a release or two; items
within a theme are listed in the order they're worth doing. Sizes are
rough: **S** is days, **M** a week or two, **L** more than that.

Shipped work moves to [CHANGELOG.md](CHANGELOG.md). Ideas and requests go
in [issues](https://github.com/omarmhaimdat/pepe/issues).

## Done

Project health: tab completion and man pages that install themselves,
installer smoke tests after every release, a benchmark gate on release
PRs with a Linux record and a Linux profile on demand, recordings of each
mode made by script, the install page, and a Windows checklist. Scale:
the Docker image on GHCR, the GitHub Action, and `--snapshot` for runs
that last hours.

## Next, in this order

Decided 2026-10-03. One pull request each.

1. **Latency by phase** (M). DNS, connect and TLS, time to first byte and
   download as separate percentiles, in the dashboard and the report.
2. **Error clustering** (S). Failures grouped by cause, with the first
   response body of each group in the verdict, so "503 from the upstream"
   is visible without opening the log.
3. **Anomaly notes during the run** (S). "p99 jumped 4× at 00:42" as it
   happens, kept in the report.
4. **Server-Timing and trace headers** (S). `Server-Timing` segments
   surfaced; the request ids of the slowest requests shown so backend
   logs can be searched.
5. **Capacity estimate from a ramp** (M). Where throughput saturates and
   where latency doubles, stated in the verdict.
6. **Arrival rate** (M). `--rate 500/s`, an open model next to fixed
   concurrency: can it hold 500 a second at p99 under 200 ms.
7. **Config file** (S). `pepe.toml` in a project; the setup screen can
   write it.
8. **Warm-up** (S). `--warmup 10s` left out of the numbers.
9. **Request chaining** (L). A value from one response feeds the next;
   each step a row in the dashboard.
10. **Replay from access logs** (M). URLs in their real proportions from
    an nginx, Caddy or ALB log.

## Later

Kept, not scheduled: distributed runs (merging snapshot reports first, a
live coordinator later); data-driven requests (`--vars users.csv`);
thresholds that fail CI (`--fail-if`; the GitHub Action's `fail-if` input
covers it today); comparing two runs; CSV per-request export; JUnit XML;
a Prometheus endpoint; a webhook summary; an HTML report.

## Not now

More protocols (HTTP/2, gRPC, WebSocket, HTTP/3) and dashboard extras
(replaying a saved run, side-by-side runs, themes): each deserves a
release of its own once the above is done.
