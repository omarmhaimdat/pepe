# Roadmap

Where pepe is going, in order. Each theme is a release or two; items
within a theme are listed in the order they're worth doing. Sizes are
rough: **S** is days, **M** a week or two, **L** more than that.

A ticked box names the release that shipped it; the details are in
[CHANGELOG.md](CHANGELOG.md). Ideas and requests go in
[issues](https://github.com/omarmhaimdat/pepe/issues).

## 1. Project health

- [x] **Shell completions and man pages**, shipped with every release — v0.8.0
- [x] **Completions that work out of the box**: the installers, the
      Homebrew formula and `pepe self-update` put them in place — v0.9.0
- [x] **Installer smoke tests** after every release, on macOS, Linux and
      Windows — v0.10.0
- [x] **A benchmark gate on release PRs**, with a Linux benchmark record
      next to the macOS one — v0.10.0
- [x] **A Linux profile on demand** (`perf`, `strace`, a mimalloc and a
      patched-reqwest build) — v0.10.0
- [x] **Recordings of each mode**, made by script so they stay current — v0.10.0
- [x] **The install page** at pepe.mhaimdat.com — v0.10.0
- [x] **A Windows checklist** for things CI can't see — v0.10.0
- [ ] ~~Scheduled benchmark run~~ — set aside; the gate on release PRs
      covers what it was for.

## 2. Scale and operations

Runs that are longer, bigger, or part of a pipeline.

- [x] **Docker image and a GitHub Action** (S): `ghcr.io/omarmhaimdat/pepe`
      and `uses: omarmhaimdat/pepe@master` with a URL, flags and a
      `fail-if` condition — v0.10.0
- [x] **Soak mode** (S): `--snapshot FILE` writes the report every minute
      and once more at the end, with a minute-by-minute timeline, so a
      crash at hour six doesn't lose the data — v0.10.0
- [x] **Arrival rate** (M): `--rate 500`, an open model next to the fixed
      concurrency; the footer and the verdict say when `-c` can't carry
      it and what would — v0.10.2
- [x] **Config file** (S): a `pepe.toml` in a project, so `pepe` there runs
      the agreed test; `--write-config` and `ctrl-s` on the setup screen
      write it — v0.12.0, with `rate`, `warmup` and `trace-header` keys
      in v0.15.0
- [x] **Warm-up** (S): `--warmup 10s`, sent and left out of the numbers,
      so the first connections don't skew the verdict — v0.11.0
- [ ] **Distributed runs** (L). Several pepe workers, one dashboard:
      `pepe worker --join host:port`. For when one machine's network is
      the limit. First a merge of snapshot reports, later a live
      coordinator. The share-nothing engine already thinks in shards;
      this makes a shard a machine.

## 3. Analysis and insights

Say more about what the numbers mean.

- [x] **Error clustering** (S): failures grouped by cause, each with the
      first response body, so "503 from the upstream" is in the verdict
      without opening the log — v0.10.0
- [x] **Latency by phase** (M): connect (TCP and TLS), time to first byte
      and download as separate percentiles, in the dashboard and the
      report — v0.10.1
- [x] **Anomaly notes during the run** (S): "p99 jumped 4.5× to 45ms at
      26s" in the footer as it happens, repeated in the verdict and kept
      in the report — v0.10.2
- [x] **Server-Timing and trace headers** (S): `Server-Timing` segments
      held against the latency measured here, and the slowest requests
      listed with the ids their backend gave them — v0.10.2
- [x] **Capacity estimate from a ramp** (M): a saturation curve fitted to
      the ramp, read out as "Capacity about 3.0k req/s · reached around
      30 concurrent · median latency doubles around 34" — v0.10.2

## 4. Scenarios and realism

Load that looks like production.

- [x] **Request chaining** (L): `pepe flow checkout.toml`, a sequence
      where a value from one response feeds the next; each step a row in
      the dashboard, as endpoints are in API mode — v0.13.0
- [x] **Replay from access logs** (M): `pepe replay access.log` reads an
      nginx, Apache, Caddy or ALB log and sends its URLs in their real
      proportions — v0.14.0
- [ ] **Data-driven requests** (M). `--vars users.csv` and `{{email}}` in
      the URL, headers and body, so every request is different. Flows
      already have the `{{holes}}`; this fills them from a file.

## 5. Output and integration

Get the results where they're needed.

- [ ] **Thresholds that fail CI** (S). `--fail-if 'p99 > 300ms'`, reusing
      the ramp's conditions, with a non-zero exit code. The GitHub Action's
      `fail-if` input covers this in CI today.
- [ ] **Compare two runs** (M). `pepe compare before.json after.json` says
      what moved, in the verdict's language.
- [ ] **CSV and per-request export** (S). One line per request for
      spreadsheets and notebooks.
- [ ] **JUnit XML** (S). So CI systems show thresholds as test results.
- [ ] **Prometheus metrics** (M). `--metrics :9100` exposes the live
      numbers, so a soak run shows up in Grafana next to the server's own.
- [ ] **Webhook summary** (S). Post the verdict when a run ends.
- [ ] **HTML report** (M). One self-contained file with the charts, for
      people who weren't at the terminal.

## Not now

Considered and set aside, so they aren't re-argued every month:

- **More protocols** (HTTP/2, gRPC, WebSocket, HTTP/3). HTTP/1.1 is where
  pepe is best; these each deserve a release of their own once the above
  is done.
- **Dashboard extras** (replaying a saved run, side-by-side runs, themes).
  Nice, not pressing.
