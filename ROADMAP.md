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

- [x] **Reading nginx logs** (M): `pepe logs access.log error.log` follows
      the logs and holds the request rate now against each minute, hour
      and day they go back, with the paths, the clients, the error log's
      messages by cause and the lines themselves — next release

## 3b. Every day

A load test is run now and then; a latency check is run all day. The
same engine at one request a second is the reason to have pepe open.

- [x] **`pepe ping`** (M): ping, with HTTP instead of ICMP, and a graph.
      One or several targets, each request split into DNS, connect, TLS,
      first byte and download; loss, jitter and percentiles over the
      window or the run; the TLS session (full or resumed) and the
      certificate's expiry; SLO limits that mark pings and set the exit
      code; TCP pings, commands, cloud regions and address ranges as
      targets; JSON, JSON Lines or CSV when there's no terminal — next
      release
- [ ] **A ping in the background** (S). `pepe ping --daemon` keeps the
      JSON Lines going to a file, so the screen can be opened on a run
      that started yesterday.

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
- [x] **Compare two runs** (M): `pepe compare before.json after.json` says
      what moved, in the verdict's language, and only when it moved more
      than runs of that size wobble; `--gate` exits 1 on a regression — next release
- [ ] **CSV and per-request export** (S). One line per request for
      spreadsheets and notebooks.
- [ ] **JUnit XML** (S). So CI systems show thresholds as test results.
- [x] **Prometheus metrics** (M): `--metrics :9100` serves the live
      numbers at `/metrics`, for runs, API mode, flows, replays and pings,
      so a soak run shows up in Grafana next to the server's own — next
      release
- [ ] **Webhook summary** (S). Post the verdict when a run ends.
- [ ] **HTML report** (M). One self-contained file with the charts, for
      people who weren't at the terminal.

## 6. Speed on Linux

Linux is where CI and most agents run. Through v0.16 pepe spent 2.5×
less CPU per request than oha on an M4 Pro, but as much as oha or more
on Linux, twice wrk's, and reached half oha's throughput. The profile
said why, and the ticked items here are what it led to (the numbers are
in [bench/README.md](bench/README.md)).

- [x] **Name the gap** (M): on Linux a third of pepe's CPU was reqwest's
      per-request plumbing (the pool, the connector, the timer, the
      URL parsed again for every response), and a twentieth was pepe
      looking up twenty header names in every response — next release
- [x] **A lean HTTP/1.1 path** (L): each worker keeps its own connection
      and reads and writes it itself. The request is bytes made before
      the run; the response is parsed where it was read; nothing is
      allocated for a request, and a thread's connections share one read
      buffer. reqwest is left the proxies, the redirects and the flows.
      On Linux 37 to 76% less CPU per request and 35 to 89% less memory
      than v0.16.0: ahead of wrk on both, on one thread, with ten
      million requests in 4.5 MB — next release
- [x] **Threads on their own** (S): `--threads auto` adds a sending
      thread whenever those sending are all past 90% of a core, and
      takes it back if it didn't pay, so a script gets the throughput
      without reading the footer — next release
- [x] **wrk and k6 in the table** (S): `bench/run.sh` measures them next
      to oha and vegeta when they are installed, and the suite has a
      million-request workload; its target server is Rust now, at half
      the CPU of the Go one it replaces, and the Linux record is taken
      with the musl build that is released — next release
- [ ] **A fixed machine for the numbers** (S). Shared runners move the
      same binary from 13 to 26 ms per 1,000 requests, and the Linux
      tables so far are from a VM on a laptop that was in use. A
      dedicated runner makes the absolutes mean something.
- [ ] **Flows on the direct path** (M). A flow builds a new request at
      every step, so it still goes through reqwest and pays what plain
      runs no longer do.
- [ ] **TLS without the copies** (M). With 16 KB bodies over TLS pepe is
      level with wrk rather than ahead: rustls decrypts into a buffer of
      its own and copies out. Its unbuffered API takes a record from the
      socket to the count in place.
- [ ] **Less of the binary in memory** (M). Of pepe's 4 MB, 3.2 are its
      own code, mapped 64 KB at a time around whatever a run touches. A
      profile-guided link puts what a run uses together; on a glibc
      build, where libc is mapped too and wrk holds 1.5 MB less than
      pepe, it is the only way under.
- [ ] **Fewer trips to the kernel** (L). Four fifths of the cost is now
      one `send` and one `recv` per request and the kernel's side of
      them. Doing better means several requests per system call
      (io_uring), which is a project of its own and only for Linux.
- [ ] **The reqwest re-parse fix, upstream** (S). The patch in
      `bench/patches/` skips a `Url::parse` on every response. pepe's
      plain runs no longer pass through it; flows and proxied runs do,
      and so does everyone else who uses reqwest.

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
