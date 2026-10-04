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
- [ ] **A report when there's no terminal** (S). Piped, or run by a
      script without `--json`, pepe still tries to open the dashboard and
      fails. With no terminal it should run to completion and print the
      report, so a forgotten flag costs nothing.
- [ ] **A versioned report** (S). `"schema_version": 1` in the JSON and a
      JSON Schema published with each release, so a script or an agent
      can depend on the field names while sections keep being added.
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

## 6. Speed on Linux

On an M4 Pro pepe spends 2.5× less CPU per request than oha. On a 4-vCPU
Linux runner it spends about 30% more and reaches half the throughput
(see [bench/README.md](bench/README.md)), and Linux is where CI and most
agents run.

- [ ] **The reqwest re-parse fix, upstream** (S). The patch in
      `bench/patches/` skips a `Url::parse` on every response and
      measured 3.5–4% less CPU on Linux. Sent upstream, pepe gets it
      without carrying a fork.
- [ ] **Name the rest of the gap** (M). The profile so far explains about
      a tenth of pepe's CPU on Linux. Keep at it until the difference
      with oha is accounted for; that decides whether the last item here
      is needed.
- [ ] **Threads on their own** (S). `--threads auto` adds a sending
      thread when one passes 90% of a core. pepe already says when it is
      the limit, but a script can't read the footer.
- [ ] **A fixed machine for the numbers, with wrk and k6** (S). Shared
      runners move the same binary from 13 to 26 ms per 1,000 requests.
      A dedicated runner makes the absolutes mean something, and wrk and
      k6 next to oha and vegeta complete the table.
- [ ] **A lean HTTP/1.1 path** (L). What is left after the above is the
      shape of the reqwest and hyper-util stack. A sender of pepe's own
      for the plain case is a project of its own, and only worth it if
      the gap is still there.

## 7. Agents

A coding agent that changes a server should be able to load-test it the
way it runs the tests: one command, a result it can read, an answer it
can act on. The thresholds and the versioned report above are the base;
these build on them.

- [ ] **Agent docs** (S). An `AGENTS.md` and an `llms.txt`: the flags to
      always pass, the report fields to read, what each exit code means.
      It is what an agent reads first.
- [ ] **Guardrails** (S). `--allow-host`, caps on the rate and on total
      requests, and `--dry-run` to print the plan and send nothing, so an
      agent pointed at the wrong URL does no harm. Replay already leaves
      writes out unless asked; this is the same idea for every mode.
- [ ] **MCP server** (M). `pepe mcp` serves a run, a ramp, a flow, a
      replay and a comparison as typed tools, each returning the JSON
      report, so an agent calls pepe without composing a shell command.
- [ ] **The engine as a crate** (M). The load engine and the report as a
      library, for Rust harnesses and agents that would rather not start
      a process.

## Not now

Considered and set aside, so they aren't re-argued every month:

- **More protocols** (HTTP/2, gRPC, WebSocket, HTTP/3). HTTP/1.1 is where
  pepe is best; these each deserve a release of their own once the above
  is done.
- **Dashboard extras** (replaying a saved run, side-by-side runs, themes).
  Nice, not pressing.
