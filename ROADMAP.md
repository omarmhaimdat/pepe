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
      messages by cause and the lines themselves — v0.19.0

## 3b. Every day

A load test is run now and then; a latency check is run all day. The
same engine at one request a second is the reason to have pepe open.

- [x] **`pepe ping`** (M): ping, with HTTP instead of ICMP, and a graph.
      One or several targets, each request split into DNS, connect, TLS,
      first byte and download; loss, jitter and percentiles over the
      window or the run; the TLS session (full or resumed) and the
      certificate's expiry; SLO limits that mark pings and set the exit
      code; TCP pings, commands, cloud regions and address ranges as
      targets; JSON, JSON Lines or CSV when there's no terminal — v0.25.0
- [ ] **A ping in the background** (S). `pepe ping --daemon` keeps the
      JSON Lines going to a file, so the screen can be opened on a run
      that started yesterday.

## 3c. Tests worth keeping

A test worked out on screen should be one keystroke from being run
again, and its results should pile up somewhere with a name. Plain files
in `.pepe/` next to the code, meant for git; no database.

- [x] **Saved tests** (S): `ctrl-s` on the setup form writes
      `.pepe/tests/NAME.toml`, `pepe --load NAME` runs it — v0.37.0
- [x] **A library to pick from** (M): `pepe` alone opens on the saved
      tests, the file decides the mode, and every screen takes the
      mouse — v0.38.0
- [x] **Any run kept** (S): `--keep NAME`, `ctrl-s` on the dashboard and
      the ramp, `c` to copy in the library — v0.39.0
- [x] **Recorded runs** (M): `--record` and `w` keep the report under the
      test; `pepe history`, `pepe rerun NAME --gate`, `pepe compare NAME`
      — v0.40.0
- [x] **A ping is a saved test too** (S): `[ping]` in the file, `ctrl-s`
      and `r` on its screen — v0.41.0
- [x] **This machine's defaults** (M): `F2` on every screen opens the
      settings, kept in `~/.config/pepe/settings.toml`: the theme, the
      defaults a run starts from, the guardrails — v0.42.0
- [ ] **Find the spec** (M). `pepe api http://localhost:8000` looks where
      FastAPI, Nest, Spring, .NET and Django serve their OpenAPI spec,
      so a local server needs no file.
- [ ] **`pepe init URL`** (S). A saved test per endpoint of a spec, to
      start a project's `.pepe/` from.
- [ ] **Server-Timing recipes** (S). A few lines per framework, in the
      docs, so the phases the verdict reads are there to read.

## 4. Scenarios and realism

Load that looks like production.

- [x] **Request chaining** (L): `pepe flow checkout.toml`, a sequence
      where a value from one response feeds the next; each step a row in
      the dashboard, as endpoints are in API mode — v0.13.0
- [x] **Replay from access logs** (M): `pepe replay access.log` reads an
      nginx, Apache, Caddy or ALB log and sends its URLs in their real
      proportions — v0.14.0
- [x] **Data-driven requests** (M). `--vars users.csv` and `{{email}}` in
      the URL, headers and body, so every request is different; in a
      flow, each chain starts with the next row — next release

## 5. Output and integration

Get the results where they're needed.

- [x] **Thresholds that fail CI** (S): `--fail-if 'p99 > 300ms'`, the
      ramp's conditions, in every mode that measures requests, with exit
      code 4 — v0.29.0
- [x] **A report when there's no terminal** (S): piped or run by a script,
      the run goes to its end and the dashboard's report is printed — v0.29.0
- [x] **A versioned report** (S): `"schema_version": 1` in every report,
      and the JSON Schema of each (`pepe schema run|ramp|ping|compare`)
      in every release and at pepe.mhaimdat.com/schema/ — v0.29.0
- [x] **Compare two runs** (M): `pepe compare before.json after.json` says
      what moved, in the verdict's language, and only when it moved more
      than runs of that size wobble; `--gate` exits 1 on a regression; ping reports
      compare phase by phase — v0.22.0
- [ ] **Per-request CSV** (S). A row a second is in `--export .csv`; one line per request for
      spreadsheets and notebooks.
- [ ] **JUnit XML** (S). So CI systems show thresholds as test results.
- [x] **Prometheus metrics** (M): `--metrics :9100` serves the live
      numbers at `/metrics`, for runs, API mode, flows, replays and pings,
      so a soak run shows up in Grafana next to the server's own — v0.27.0
- [x] **Webhook summary** (S). Post the verdict when a run ends: Slack,
      Discord, Teams, Google Chat or JSON, on start, progress, anomaly,
      end or fail, from flags, `pepe.toml` and F3 on any screen — next
      release
- [x] **Slack, live** (M). One message edited in place as the run
      goes, anomalies in its thread, `pepe slack connect` from the
      terminal and `S` on the webhook page — next release
- [x] **Slack, interactive** (M). Stop, Run again, Edit and run and
      History on the messages, and `/pepe`, answered by `pepe slack
      serve` over Socket Mode, saved tests only, allowed users — next
      release
- [x] **HTML report** (M). One self-contained file with the charts, for
      people who weren't at the terminal; with it a Markdown summary,
      text and CSV: `--export FILE`, F4 on a run screen, and `pepe export
      REPORT.json` for a report already written — next release

## 6. Speed on Linux

Linux is where CI and most agents run. Through v0.16 pepe spent 2.5×
less CPU per request than oha on an M4 Pro, but as much as oha or more
on Linux, twice wrk's, and reached half oha's throughput. The profile
said why, and the ticked items here are what it led to (the numbers are
in [bench/README.md](bench/README.md)).

- [x] **Name the gap** (M): on Linux a third of pepe's CPU was reqwest's
      per-request plumbing (the pool, the connector, the timer, the
      URL parsed again for every response), and a twentieth was pepe
      looking up twenty header names in every response — v0.17.0
- [x] **A lean HTTP/1.1 path** (L): each worker keeps its own connection
      and reads and writes it itself. The request is bytes made before
      the run; the response is parsed where it was read; nothing is
      allocated for a request, and a thread's connections share one read
      buffer. reqwest is left the proxies, the redirects and the flows.
      On Linux 37 to 76% less CPU per request and 35 to 89% less memory
      than v0.16.0: ahead of wrk on both, on one thread, with ten
      million requests in 4.5 MB — v0.17.0
- [x] **Threads on their own** (S): `--threads auto` adds a sending
      thread whenever those sending are all past 90% of a core, and
      takes it back if it didn't pay, so a script gets the throughput
      without reading the footer — v0.17.0
- [x] **wrk and k6 in the table** (S): `bench/run.sh` measures them next
      to oha and vegeta when they are installed, and the suite has a
      million-request workload; its target server is Rust now, at half
      the CPU of the Go one it replaces, and the Linux record is taken
      with the musl build that is released — v0.17.0
- [ ] **A fixed machine for the numbers** (S). Shared runners move the
      same binary from 13 to 26 ms per 1,000 requests, and the Linux
      tables so far are from a VM on a laptop that was in use. A
      dedicated runner makes the absolutes mean something.
- [x] **Flows on the direct path** (M): a step is written to the wire
      as it is sent, with its values filled in, over the worker's
      connection to its origin, kept from step to step and chain to
      chain. A two-step flow costs half the CPU it did and runs 2.2×
      faster, in 6 MB rather than 15 — next release
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

- [x] **Agent docs** (S): `AGENTS.md`, `llms.txt` at the site's root,
      and a page on the docs site — v0.30.0
- [x] **Guardrails** (S): `--allow-host`, `--max-requests`, `--max-rate`,
      `--max-concurrency` and `--dry-run`, in every mode, in `pepe.toml`
      too — v0.30.0
- [x] **MCP server** (M): `pepe mcp` serves a run, a ping, a ramp, an
      API, a flow, a replay and a comparison as typed tools over stdio,
      each returning the JSON report, under the guardrails it was started
      with — v0.33.0
- [x] **Where agents look** (S): a Claude Code plugin with the skill and
      the MCP server, capped by default — v0.34.0; `pepe-http` on npm and
      PyPI — v0.35.0; `io.github.omarmhaimdat/pepe` in the MCP registry —
      v0.36.0
- [ ] **The engine as a crate** (M). The load engine and the report as a
      library, for Rust harnesses and agents that would rather not start
      a process.

## 1.0

What 1.0 means: the parts a script, a CI job or an agent depends on
stop changing without a major version. It is a promise about surfaces,
not a feature. It is tagged when every box below is ticked.

Frozen at 1.0:

- **The reports.** `schema_version` stays 1 for the run, ramp, ping and
  compare reports; fields are only ever added. The JSON Schemas in
  `schema/` are the contract.
- **The exit codes.** 0, 1, 2 and 4 as documented in
  [Output and exit codes](docs/output.md#exit-codes).
- **The flags.** Every flag in `pepe --help` and the subcommands' help
  keeps its name and meaning; a flag may be added, never removed or
  renamed without a major version. The same for the keys in `pepe.toml`.
- **The MCP tools.** The names and inputs of `pepe mcp`'s tools.
- **The install lines.** The installer URLs, the Homebrew tap, the
  Docker image name, the GitHub Action's inputs.

Before the tag:

- [x] **Agent docs, guardrails and the MCP server** (section 7)
- [x] **A versioned report with a published schema** (section 5)
- [x] **`cargo audit` in CI** and Dependabot on — v0.34.0
- [ ] **A month without a schema or flag change** on master, so the
      freeze is a description rather than a wish. The clock restarted
      with v0.42.0 (2026-10-13, the settings page); 2026-11-13 at the
      earliest
- [x] **The data-driven requests** (`--vars`, section 4), the one flag
      still likely to shape the config file's keys — next release
- [ ] **A release note that says it**, with the frozen list above

## Not now

Considered and set aside, so they aren't re-argued every month:

- **More protocols** (gRPC, WebSocket, HTTP/3). HTTP/1.1 is where pepe's
  load engine is best; these each deserve a release of their own once the
  above is done. `pepe ping` speaks HTTP/2 since the ping is one request
  at a time and the phases are what matter there; the load path doesn't.
- **Dashboard extras** (replaying a saved run on screen, two runs side by
  side). Nice, not pressing; `pepe compare NAME` covers most of the
  second, and the themes shipped in v0.42.0.
