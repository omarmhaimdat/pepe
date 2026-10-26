# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
## [0.56.0](https://github.com/omarmhaimdat/pepe/compare/v0.55.2...v0.56.0) - 2026-10-26

### Added

- *(serve)* pepe over HTTP, with workers and schedules ([#236](https://github.com/omarmhaimdat/pepe/pull/236))

  Stacked on #235, which brings the guardrails and the numbers onto
  `master`. Retarget this to `master` once that merges, rather than
  merging it into `fix/guardrails`.

  This is the API from all three phases of the design
  (`docs/design/serve.md`). The web UI isn't in it. The docs are
  `docs/serve.md`, also built into the docs site.

  ## What

  - **`pepe serve`.** Every run is a child `pepe`, under the server's
  guardrails, the project's `pepe.toml` and the machine's settings. pepe's
  own `--dry-run` checks each request first, so the guardrails decide
  exactly as they do on the command line.
  - **Same arguments as MCP.** A request takes the MCP tools' arguments,
  so a value is never read as a flag.
  - **Not taken over the API:** `webhook`, `slack`, `otlp`, `svg`, and
  files outside the project.
  - **Locked down by default:**
    - It listens on loopback only.
  - A token is required, either made and printed, or read with its names
  from `--token-file`.
  - Off loopback, it refuses to start without `--allow-host` and all three
  caps.
  - `--max-runs` (default 1) and an audit log in
  `.pepe/serve/audit.jsonl`.
  - **Runs:**
    - Start one, or `dry_run` for the plan.
    - List runs, and read one's state and report.
    - Watch it live as Server-Sent Events, resumable from `Last-Event-ID`.
    - Get its report as `json`, `html`, `md`, `txt` or `csv`.
  - Pause, resume, stop, and change concurrency. A ping can be stopped and
  still gives its report.
  - **Tests:** list, read, save (as JSON keys or TOML), delete, their
  recorded runs (shared with `pepe history`), `compare` with an optional
  gate, and the report schemas.
  - **Schedules:** a saved test every so often, at most once a minute. A
  schedule that comes due while the server is full is skipped and says so.
  - **Workers:**
  - **Joining:** `pepe worker --join URL --name eu-1` dials out over a
  WebSocket, so it works from behind NAT. `src/ws.rs` gains the server
  side.
  - **Splitting:** `"workers": "all"` or a list of names shares the count,
  concurrency and rate among them.
    - **Guardrails:** each worker runs its share under its own guardrails.
  - **Merging:** each share returns its report with the latency histogram
  whole (hidden `--histogram`), and the server adds them bucket by bucket,
  so the merged p99 is exact.
  - **`--events FILE`:** any run as JSON lines (start, progress each
  second, anomalies, steering, the report last). It hooks into the
  existing webhook calls, so every mode has it, with a ping progress line
  of its own.

  ## Checked end to end

  Against the benchmark server, with a server in a scratch project, two
  workers and a `pepe.toml` allowing only 127.0.0.1:

  | | |
  | --- | --- |
  | No token, wrong token | 401 |
  | `localhost` under `allow-host = ["127.0.0.1"]` | 422, "localhost isn't
  an allowed host" |
  | `webhook` in a request; `"file": "/etc/passwd"` | 400 each |
  | `--listen 0.0.0.0` with no `max-rate` | refuses to start, naming
  `--max-rate` |
  | A 3 s run at 200/s | 600 requests; events: start, progress ×4, report,
  end; `Last-Event-ID: 3` resumes at 4 |
  | Pause, resume, concurrency 2, stop | each a `steered` event; the
  report says interrupted |
  | A third run under `--max-runs 2` | 429 |
  | A ping, then pause; then stop | 409 "can be stopped, not steered";
  then stopped with its report |
  | A test saved as JSON, run twice, compared | both recorded under
  `.pepe/runs/`, compare says `same` |
  | 40,000 requests at 8 on `"workers": "all"` | 20,000 each; merged 158k
  req/s, `exact_percentiles: true` |
  | `-c 8` on a worker started with `--max-concurrency 4` | its share
  refused by its own guardrail |
  | A worker killed mid-run | the run ends on the other share, failing,
  naming the one that left |
  | A schedule every 60 s | it ran as `schedule:s1` |

  455 tests pass and clippy is clean. New tests cover tokens, the
  off-loopback rule, events, merging (two shares whose p99 only the merged
  histogram gets right, checked against the run schema), schedules, the
  handshake key (the RFC's example), splitting, and the join URL.

  ## Costs and limits

  - **Binary size:** 8.38 → 9.05 MB (+8%), for hyper's server, its body
  types and this code.
  - **Distributed runs** are plain runs only, for now (a URL, or a saved
  test of one); the other modes run on the server.
  - **Orphaned shares:** a worker's process killed outright leaves its
  share's child running to the end of its count or time. Every run is
  bounded, so it can't run forever.



### Fixed

- the guardrails and the numbers, onto master ([#235](https://github.com/omarmhaimdat/pepe/pull/235))

  #233 (guardrails) and #234 (numbers) were merged into their stacked
  bases (`perf/generator` and `fix/guardrails`), not into `master`, so
  neither is in `master` or a release yet. This brings both onto `master`,
  with master merged in. Both were already reviewed in their own PRs.

  It also carries one fix to #233:

  - **The MCP `run` tool sent nothing.** #233 put a `--` before a tool
  call's URL, so a value starting with a dash couldn't be read as a flag.
  But after a `--`, a plain run reads a curl command (`--curl -- curl …`),
  so the URL went there and the run had nothing to send.
  - **The fix:** what isn't a flag goes as it is now, and one that starts
  with a dash is refused. A new test parses each tool's command the way
  pepe will.
  - **Checked through `pepe mcp`:** the `run` tool sends its 50 requests,
  and a target of `--allow-host=127.0.0.1` is refused.

  442 tests pass, clippy is clean.


## [0.55.2](https://github.com/omarmhaimdat/pepe/compare/v0.55.1...v0.55.2) - 2026-10-24

### Performance

- *(load)* flows on the direct path, and a run sends what it was asked to ([#230](https://github.com/omarmhaimdat/pepe/pull/230))

  ## What

  **Flows go direct.** A step is written to the wire when it is sent
  (`direct::Route::of`), with its values filled in, over the worker's
  connection to the step's origin. The worker keeps that connection from
  step to step and from chain to chain. Proxies, credentials in the URL
  and the rest of a redirect still go through reqwest.

  Two steps, 100,000 chains at 64, best of three on the M4 Pro:

  | | wall | CPU ms / 1k | peak RSS |
  | --- | ---: | ---: | ---: |
  | v0.55.0 | 3.38 s | 14.7 | 14.6 MB |
  | this branch | 1.60 s | 7.1 | 6.2 MB |
  | plain run, same 200k requests | 1.28 s | 5.6 | 6.5 MB |

  **The engine sends what a run asks for.** Each fix below was reproduced
  against a counting server, before and after:

  | | v0.55.0 | this branch |
  | --- | --- | --- |
  | `-n 20 -c 4 -m POST` answered with a 307 | 24 POSTs to the start | 20
  |
  | `-n 4 -c 4 --rate 1` | ends after 7.0 s, reports 0.57 req/s | 3.0 s |
  | `-z 3s -c 16 --rate 1` | ends after 18 s | 3.0 s |
  | replay of a 5,000-URL log, `/hot` at 99.5% | `/hot` got 44.9% | 99.5%
  |
  | flow `-n 1 --warmup 1s` | 13,860 requests counted as measured | 2 |

  - **Redirects:** `Sent::Redirected` carries the status and the
  `Location`. reqwest takes it on from there with the method and body
  reqwest's own policy would use: 301, 302 and 303 become a GET, 307 and
  308 keep the method and body. reqwest is no longer handed the original
  request to send again. The hop goes through a client without the run's
  headers, with those headers added per request, so `Authorization`,
  `Cookie` and the like stay behind when the redirect leaves the origin.
  - **Pacing:**
    - A worker claims its request before it takes a schedule slot.
  - A wait on a slot ends on a stop, or when a timed run reaches its
  deadline. A counted run still sends every request it has claimed.
  - The backlog reset no longer loses its compare-exchange to other
  workers.
  - **Schedule:** weights adding up to 4,096 or less are still an exact
  cycle. Larger totals are spread over a golden-ratio sequence rather than
  rounding every rare target up to a slot.
  - **Flow warm-up:** every step of a chain begun in the warm-up is marked
  as warm-up.
  - **Drain:** at most 65,536 results are taken per drain, so a stop or a
  frame never waits behind a backlog.
  - **`bench/tui.py`:** finds the end of a run again on the v0.52 screens
  (the title says HEALTHY now) and counts frames by bursts.

  `bench/compare.sh` against v0.55.0, five rounds: every workload within
  ±5% CPU, the stated noise. The signs flip between runs.

  ## Tests

  429 pass, clippy is clean. New tests:

  - a redirect is not sent twice
  - a POST answered with a 307 or a 303 goes on as the status says, sent
  once
  - a flow keeps one connection per worker across steps and chains
  - a paced run ends with its last request, and a timed one at its
  deadline
  - rare targets don't take the busy one's share

  ## Not in this PR

  Reviewing the engine turned up more, all reproduced on v0.55.0. They are
  left for their own PRs:

  - **Guardrails:**
  - `--rate=1e-300` gets past every cap: 32,826 requests under
  `--max-requests 20`.
    - `--warmup` and flow steps aren't counted against `--max-requests`.
    - Redirects aren't checked against `--allow-host`.
    - `--load`, `rerun` and the Slack runner don't read `pepe.toml`.
    - MCP tool arguments can add `--allow-host`.
    - `pepe api` paths can change the host.
    - A proxy's host isn't checked.
    - Live concurrency changes go past `--max-concurrency`.
  - **Numbers:**
    - `pepe compare` calls a 2.4× p99 regression "same".
  - `--rate` latency leaves out the time requests waited for a slot, and
  the verdict says healthy.
    - `pepe ramp` reports capacity `null` on a hard plateau.
    - Ping's `--jsonl` and `--csv` drop and repeat samples.
    - p99.9 is off by one rank.


## [0.55.1](https://github.com/omarmhaimdat/pepe/compare/v0.55.0...v0.55.1) - 2026-10-24

### Fixed

- *(plugin)* the plugin manifests carry the release's version ([#229](https://github.com/omarmhaimdat/pepe/pull/229))

## [0.55.0](https://github.com/omarmhaimdat/pepe/compare/v0.54.0...v0.55.0) - 2026-10-23

### Added

- *(logs)* --path, --status, --host and --until, .gz read as is, --metrics, and a logs MCP tool ([#227](https://github.com/omarmhaimdat/pepe/pull/227))

  Third of three PRs making `pepe logs` more powerful (after #223's
  per-path traffic and #225's findings and alerts).

  ## What changes

  **Filters.** `--path '/api/*'`, `--status 5xx` (or `404`, `4xx,503`),
  `--host .example.com` (or an exact host) and `--until 1h` keep only
  those lines.
  - The filters apply everywhere: every view, the findings, `--fail-if`
  and the report.
  - The title shows what was kept (`access.log · /api/* · 5xx`), and the
  report counts what was left out (`logs.lines_filtered_out`).
  - Error-log lines are kept by their `request:` and `host:` (added to the
  error-log parser). A status filter never drops them.
  - With `--status`, 5xx/4xx share findings are skipped, since they'd read
  "5xx 100% now, 100% usually".
  - `--since 2h --until 1h` reads a stretch in the past, and such a log
  isn't followed. Bad input exits 2 with the reason.

  **`.gz` read as is.** Rotated `access.log.2.gz` files are recognised by
  their first two bytes, including multi-member gzip.
  - Unpacking runs on a thread of its own, passing chunks to the reader
  through a bounded channel.
  - A `.gz` is never followed, not checked for "being written", and never
  split across cores. Read progress counts its size on disk.
  - The docs no longer suggest `zcat ... | pepe logs -`.
  - New dependency: `flate2` with its pure-Rust backend (`miniz_oxide`),
  no C to build.

  **`--metrics` while following.** The Prometheus page is re-rendered
  every second:
  - `pepe_logs_requests_total{class}`, `_bytes_total`,
  `_error_log_lines_total{level}`, `_unread_lines_total`
  - last window: `_requests_per_second`, `_5xx_ratio`, `_4xx_ratio`,
  `_live`
  - histograms: `_request_duration_seconds`, `_upstream_duration_seconds`
  - `pepe_logs_findings{level}` and `pepe_logs_fail_if_crossed{condition}`

  **MCP `logs` tool.** It takes `files` (nginx's own if none are named),
  `since`, `until`, `window`, `path`, `status`, `host`, `fail_if`,
  `format`, `exact_paths` and `rows`, and is marked read-only with no
  outside effects. Exit 4 comes back as an answer, not an error. The
  server instructions, AGENTS.md, docs/agents.md, llms.txt and the skill
  list it.

  ## Checked
  - 425 tests pass; clippy 1.98 is clean. New tests cover glob, status and
  host matching, the filter on requests and error-log lines, `--until`, a
  two-member `.gz` giving the same report as the plain file (and not
  "being written"), and the MCP argument mapping.
  - By hand:
  - `pepe mcp` answered a `logs` call over stdio with the verdict and
  `fail_if`.
  - `--metrics` was scraped from a live screen driven in a pty, and the
  session quit cleanly.
  - The filters, and bad `--status` / `--until` values, were run on the
  generated log.

  ## Cost (bench/README.md)
  | | before | after |
  | --- | ---: | ---: |
  | 862 MB log, every core, no filter (against v0.54) | 0.35 s, 3.1 s CPU,
  ~140 MB | same |
  | 189 MB log as `.gz`: `zcat \| pepe logs -` → `pepe logs file.gz` |
  0.52 s | 0.51 s, 28 MB |

  Unpacking on the reading thread took 0.71 s, which is why it gets a
  thread of its own. A first version checked the filter on every line and
  cost about 4% CPU; it is now checked once per chunk.

  ## Caveats
  - The MCP `logs` tool reads any file the server's user can read, like
  `replay` and `compare` do. The report can quote a line it couldn't parse
  (`first_unread_line`) and error-log examples. Whoever starts `pepe mcp`
  decides what it runs as.
  - `--host` needs `$host` in the log format; the default `combined`
  format has none.

  ## 1.0
  This adds `pepe logs` flags (`--path`, `--status`, `--host`, `--until`),
  a report field (`lines_filtered_out`) and an MCP tool, so it restarts
  the month without a flag or schema change.

  Please squash-merge with this title so release-plz picks it up.


## [0.54.0](https://github.com/omarmhaimdat/pepe/compare/v0.53.0...v0.54.0) - 2026-10-23

### Added

- *(logs)* findings, --fail-if limits and webhooks for nginx's logs ([#225](https://github.com/omarmhaimdat/pepe/pull/225))

  Second of three PRs making `pepe logs` more powerful (after #223's
  per-path traffic; next: MCP tool, flag filters, `.gz`).

  ## What changes

  **Findings.** The last minute (`--window`) is compared with the minutes
  before it and with the same hour a day before. What is out of the usual
  is listed most serious first, on the dashboard, in the text report and
  in `--json`:

  | Level | Rule |
  | --- | --- |
  | bad | ≥5% of requests answer 5xx · a path answers 5xx to ≥10% or 4xx
  to ≥50% · error log at ≥10× its usual · traffic under a tenth of usual
  while live · p99 ≥5× a usual minute and over 1s |
  | warn | the same, lower: ≥1% 5xx and 3× usual · path ≥2% 5xx / ≥30% 4xx
  and 3× usual · error log 3× · traffic under a third or twice usual · p99
  2× and over 200ms |
  | note | one client sends ≥20% of requests and twice the next ·
  unreadable lines |
  | good | nothing out of the usual |

  - A path's share needs ≥5 failed requests in the window, so one failure
  in forty isn't a finding.
  - Each finding says since when and what the usual was. A latency finding
  names the path whose mean time changed most, and whether that time is
  upstream (the app) or inside nginx.
  - On the dashboard, `↑` `↓` pick a finding and `enter` opens it: Traffic
  held to its path ([#223](https://github.com/omarmhaimdat/pepe/pull/223)), or the Errors or Paths tab.
  - The verdict word now follows the findings (any bad → Failing).

  **`--fail-if`** gets its own grammar in `pepe logs`: `5xx > 2%`, `4xx >
  20%`, `p99 > 1s`, `rate < 5/s` (rate is the one measure that also takes
  `<`).
  - Read once (piped, `--json`, cron), it is checked against everything
  read, so `--since 15m` sets the period.
  - While following, it is checked against the last `--window` every
  second.
  - A crossed limit gives exit code 4 after the report. On the screen,
  crossing it even once counts, and the report says when.

  **Webhooks and Slack** now work in `pepe logs`:
  - Read once: `end`, and `fail` when a limit was crossed.
  - While following: a finding turning bad, a limit crossed or held again,
  and a finding going away are sent as `anomaly` events. `--slack` keeps
  one message up to date. Nothing is sent until the log has been read up
  to where it is being written.
  - The webhook layout reads a logs report: requests, rate now, 5xx share,
  p50/p99, time span, and the findings.


## [0.53.0](https://github.com/omarmhaimdat/pepe/compare/v0.52.0...v0.53.0) - 2026-10-22

### Added

- *(logs)* traffic split by path, and the screen held to one path ([#223](https://github.com/omarmhaimdat/pepe/pull/223))

  First of three PRs making `pepe logs` more powerful (then findings and
  alerts, then the MCP tool, flag filters and `.gz`).

  ## What changes

  **Traffic tab, every path**
  - Each bar is split by the four busiest paths of the slots shown, in
  their own colours (none of them green, yellow or red, so nothing reads
  as health); the rest in the usual bar colour.
  - Beside the chart (110 columns and up): the picked slot's paths with
  their rate then, how that holds against their usual slot, their 5xx
  share and mean time, and the others.
  - The table gets each slot's p50 and p99 and its busiest path with its
  share, in place of the bar.

  **One path (`p`)**
  - `p` on Traffic, Paths or Log opens a picker: letters in order are
  enough (`srch` → `/search`), `enter` holds the screen to the path.
  - Traffic is then that path's alone: its bars, its rate now against its
  usual, and a table of its slots with requests, 4xx, 5xx, mean time and
  share of all traffic. Beside the chart: its status codes, p50/p95/p99,
  and the share of its time spent waiting on the upstream.
  - Log keeps the path's lines, the error log's lines about it included
  (by their `request:`). It starts with those among the last 2,000 and
  keeps 2,000 of its own from then on. `l` goes there from Traffic; `c` or
  `esc` lets go.

  ## How it is counted
  - Each minute, hour and day keeps a table of its 64 busiest paths, found
  by an index so a request needs no search. When a slot has more, a new
  path replaces the least requested one only if it has as many requests.
  - Times are kept in buckets an eighth of a doubling wide (176 × u32) for
  every slot and every path; a test holds percentiles within 5% of the
  truth.
  - The threads that read a file merge per-path slots by name; a test
  checks that every core reading the file counts what one thread would,
  slot by slot and path by path.

  ## Cost (bench/README.md)
  862 MB, 9-day generated log, same machine, before → after:

  | | time | memory |
  | --- | ---: | ---: |
  | the file, every core | 0.27 s → 0.28 s (CPU 2.69 → 2.93 s) | 103 → 138
  MB |
  | piped, one thread | 2.14 → 2.21 s | 18 → 22 MB |

  The `--json` report is byte-identical before and after.

  ## 1.0
  No flag, `pepe.toml` key, report field or MCP tool changes, so this
  doesn't move the 1.0 clock. New keys only: `p`, `l`, and `c` on Traffic.

  Please squash-merge with this title so release-plz picks it up.


## [0.52.0](https://github.com/omarmhaimdat/pepe/compare/v0.51.1...v0.52.0) - 2026-10-20

### Added

- *(ui)* the screens in the pepper's colours: a band for the mode, cards, tags ([#220](https://github.com/omarmhaimdat/pepe/pull/220))

  ## What

  pepe's screens take the site's look, starting with the shared pieces and
  the load-test dashboard.

  - **The palette.**
  - `pepe` now has an ink ground, cream text and the pepper's red as the
  one accent (it was ember), lime for good and gold for warnings.
    - The heatmap runs from ink to red to cream.
    - `paper` takes the site's cream.
  - There are three new roles, each the same in every theme: the ink
  written on a band or a badge, a card's hard shadow (red on dark, ink on
  light), and the modes' band colours.
  - **The kit** ([src/ui/kit.rs](src/ui/kit.rs)):
  - `band`: the mode's colour across the top, with `pepe` in an ink chip,
  the title in ink and the site's checker strip under it.
  - `card`: a square frame on the panel surface, its title a tag set into
  the border, and an optional hard shadow half a cell right and below. The
  frame sits in the panel's existing padding, so nothing drawn inside
  moves.
  - `tag`, `badge`, `corner`: tags are reverse video, so they read in
  every theme, including `NO_COLOR`.
  - Every panel is square now, and key chips and tabs are tags. The picked
  tab has the red corner square, and titles inside a card are small
  capitals.
  - **The dashboard:**
  - The ember band holds the method, the URL, the state, the concurrency
  and the clock.
  - The big numbers' names are tags, and the verdict is a badge in ink on
  its level's colour.
  - On the live tab, the charts, the totals, the latest requests and the
  errors are cards, and the charts card carries the shadow.

  Ping, ramp and setup get their bands (lime, gold, red) in follow-ups.
  The palette already holds those colours, and the kit's `Band` takes a
  variant per screen as each one moves over. Those screens already get the
  square frames, tags and chips from this change.

  ## Why

  The site and the docs have the pepper's look
  ([omarmhaimdat/pepe#216](https://github.com/omarmhaimdat/pepe/pull/216)),
  and the screens didn't match. The mock-up that came before this work
  recorded the real screens and laid them out again in the site's style.
  This PR builds that design for the dashboard.

  ## Checked

  - `cargo test`: 409 passed. Four tests that read tab titles now expect
  small capitals.
  - `cargo +1.98 clippy --all-targets -- -D warnings` and `cargo fmt
  --check`: clean.
  - Recorded the live dashboard at 120×36 against `bench/server` on
  localhost in a hidden terminal, in the `pepe` and `paper` themes, and
  compared it with the mock-up.
  - Ping and setup were recorded too, to check the square frames, tag
  titles, chips and tabs there.
  - `python3 site/build-docs.py --check`: current. The themes table in
  `docs/install.md` describes the new palettes.


## [0.51.1](https://github.com/omarmhaimdat/pepe/compare/v0.51.0...v0.51.1) - 2026-10-19

### Performance

- drop axoupdater, so macOS no longer links Security.framework ([#217](https://github.com/omarmhaimdat/pepe/pull/217))

  Measured first: the bench/ suite on macOS (M4 Pro), with wrk and k6 in
  it for the first time beside oha and vegeta. pepe's CPU was level with
  wrk's and a quarter of oha's. Its memory was not first: 10.7 to 15.0 MB
  where wrk held 3.1, and `pepe --version` alone held 8.4 MB.

  **Cause.** `otool -L` showed the binary linking `Security.framework` and
  `CoreFoundation.framework`, which dyld loads at every launch. They came
  from axoupdater's second reqwest (0.13), whose platform certificate
  verifier is Security.framework on macOS. pepe's own HTTP uses rustls
  with webpki roots. A C hello-world goes from 1.4 to 5.9 MB RSS just by
  linking those two frameworks.

  **Change.** `pepe self-update` now does axoupdater's job itself:
  - reads `pepe-receipt.json` (`$XDG_CONFIG_HOME/pepe`, `~/.config/pepe`,
  `%LOCALAPPDATA%\pepe`) and checks it is for this executable, keeping the
  cargo-dist 0.10–0.14 `bin` workaround;
  - downloads the release's `pepe-installer.sh` / `.ps1` with pepe's own
  reqwest;
  - runs it with `PEPE_INSTALL_DIR`, `CARGO_DIST_FORCE_INSTALL_DIR` and
  `PEPE_NO_MODIFY_PATH` as the receipt says.

  On Windows the running exe is renamed aside before the install and
  restored if the install fails, as axoupdater did. One difference: the
  `.previous.exe` is removed at the next self-update rather than
  self-deleted. New tests cover the receipt and run a stand-in installer
  to check its environment and the error message.

  The binary now links only `libSystem` and `libiconv`. It is 10.2 → 8.0
  MB on disk, and 62 of 286 crates leave the lockfile.

  **Numbers (macOS; full tables in bench/README.md under "What the binary
  links", CSVs in bench/results/)**

  | | before | after |
  | --- | --- | --- |
  | tiny-c64, peak MB | 10.8–11.6 | 6.9 |
  | tiny-c1000, peak MB | 13.8–14.6 | 9.5–10.1 |
  | tls-c64, peak MB | 15.0 | 10.0–10.5 |
  | million-c64, peak MB | 10.7–12.0 | 6.2 |
  | every workload, memory | | −29 to −45% |
  | 1M GETs, CPU ms/1k (median of 5 alternating) | 6.10 | 6.08 |
  | 400k TLS, CPU ms/1k (median of 5 alternating) | 6.55 | 6.53 |
  | dashboard, 200k at 64 | 1.24–1.28 s · 15.4–15.9 MB | 1.26–1.28 s ·
  11.3–11.5 MB |

  `bench/compare.sh`, best of 3: CPU +0.8 to +4.9% (inside the rounds'
  spread; the 5-round medians above show no change), memory −30 to −44%.

  **Where pepe still isn't first (macOS):**
  - Memory against wrk at ≤256 connections: 6.2–7.6 MB vs 3.1–4.7. What is
  left is clean code pages; the physical footprint is 3.3 MB vs wrk's 2.3.
  From 1,000 connections, and over TLS, pepe now holds less than wrk.
  - CPU at 1,000 slow connections: wrk 6.4–6.7 vs pepe 7.2–8.0 ms/1k,
  mostly kernel time.
  - TLS requests a second: oha and k6 on 14 threads beat pepe's one. wrk
  on one thread is level with it.

  **Not measured:** Linux. Docker wasn't running here. On Linux the
  frameworks don't exist, and the second reqwest's verifier is pure Rust,
  so the published 4.0 MB musl number isn't expected to move. The
  Benchmarks workflow checks it on the next release PR. docs/benchmarks.md
  quotes Linux numbers only, so it is unchanged.



### Other

- the site in the pepper's colours, and a page for every config file ([#216](https://github.com/omarmhaimdat/pepe/pull/216))

  ## What

  **The site, restyled.** The front page and every docs page take the
  mascot's own palette: its cream (`#FFF1E0`) and ink, pepper red, lime,
  gold and ember, with Geist for prose and Geist Mono for code, square
  edges and hard shadows.

  - **The front page** (`site/index.html`) is bands of colour, one a mode:
  ping split into its five phases, a ramp bending at its capacity,
  compare's before and after, the guardrails for agents, every mode, and
  the benchmarks. Requests cross the hero behind a live ping card. The
  install is one line to copy, with `via curl · brew · powershell · docker
  · nix · cargo` to swap it.
  - **The docs** stay calm, with no bands. A tag sits above each title
  with its group and command (`MODES` · `pepe ping`), and the subject is
  selected. Code blocks mark `pepe` and dim the comments, and TOML tables
  are coloured. The sidebar's current page is an ink block, and the
  columns have no scroll bar until the pointer is over them.

  **A new page: [Config files](docs/config.md).** It covers everything a
  TOML file can say, in one place:

  - **`pepe.toml`:** every key with what it does and its flag; the rules
  (flags win, length is said once, on/off keys only switch on, unknown
  keys are errors, this directory only); and recipes (guardrails, a soak,
  a CSV-driven run, a ramp, a ping watch).
  - **The other files:**
    - `[[webhook]]` tables, with every key and hole;
    - where `${NAME}` is read, and where it isn't;
  - saved tests: how they're made and named, and how `--load` picks the
  mode;
    - `settings.toml`;
    - which file wins, including the caps;
    - flow files;
    - `slack.toml`.
  - **Refusals:** what pepe says when a file is refused.

  It's linked from the overview, `llms.txt` and the load-testing page's
  config section, and it sits in the sidebar under Reference.

  ## Why

  The site looked like a generic docs theme rather than pepe. What a
  config file can hold was spread over five pages (load testing, install,
  flows, webhooks, agents), and some of it was on none of them:

  - `vars` refuses `pepe api` and `pepe ping`.
  - `max-requests` and `max-rate` refuse a ramp.
  - `${NAME}` is filled only in webhooks.
  - A saved test keeps its headers, tokens included.
  - `vars` and `export` from `[defaults]` don't reach under a `pepe.toml`.
  - A cap typed on the command line replaces the file's.

  ## Checked

  - `python3 site/build-docs.py --check`: current.
  - Every TOML example on the new page passes `--dry-run` in its own mode
  (run, ramp, ping, flow, settings), with each error message the page
  quotes produced by pepe itself.
  - Looked at the front page and the docs in light and dark, at desktop
  and phone widths, with no sideways scroll.
  - No Rust changed.


- *(nix)* update flake.lock ([#215](https://github.com/omarmhaimdat/pepe/pull/215))

  Automated changes by the
  [update-flake-lock](https://github.com/DeterminateSystems/update-flake-lock)
  GitHub Action.

  ```
  Flake lock file updates:

  • Updated input 'nixpkgs':
      'github:NixOS/nixpkgs/6510408d147d8c5e6d76043e899e9251756f6a75?narHash=sha256-XK4KceonkwD8LuMAt/vNrz982mXJM5jeHAxMwsUXXAQ%3D' (2026-10-10)
    → 'github:NixOS/nixpkgs/fe13242281a67641c0102cdb1087f0ebe586c7df?narHash=sha256-zp8ToDMnqZcu4o4IH9pdwXFXGp7yLm/M0dEnNWnyd7I%3D' (2026-10-15)
  ```

  ### Running GitHub Actions on this PR

  GitHub Actions will not run workflows on pull requests which are opened
  by a GitHub Action.

  **To run GitHub Actions workflows on this PR, close and re-open this
  pull request.**


## [0.51.0](https://github.com/omarmhaimdat/pepe/compare/v0.50.0...v0.51.0) - 2026-10-18

### Added

- *(slack)* a quiet thread: what changed goes in the message, and @pepe answers questions there ([#213](https://github.com/omarmhaimdat/pepe/pull/213))

  Feedback from using it: a reply in the thread for every anomaly was too
  many messages. The data now lives in one denser message, and the thread
  is for questions.

  ## The live message

  It's still edited every 10 s, and now also immediately when something
  changes or someone steers the run.

  **Six fields:**
  - **Throughput:** current value, the run's peak, and which way it has
  moved lately.
  - **Latency:** p50 · p95 · p99, with a sparkline.
  - **Errors:** by cause, with the first body the server sent.
  - **Where the time goes:** waiting for the first byte versus reading the
  body.
  - **Sent:** requests and bytes received.
  - **Load:** connections in flight and the rate cap, plus who changed it
  last.

  **What changed:** each anomaly is one timestamped line in the message,
  latest five, e.g. `` `1m26s` 📉 Throughput dropped 52% · 5.9k req/s ``.
  The verdict lists them too.

  The thread gets nothing for anomalies. The only unprompted thread
  message left is the single result line at the end. It's also shown in
  the channel and carries the `mention` when a run fails.

  ## Ask in the thread

  With `pepe slack serve` running, mentioning `@pepe` in the thread of a
  run it started gets an answer there. Topics: *p99*, *errors* (what
  failed, what the server said, when it began), *slowest*, *where does the
  time go*, *codes*, *compare* (against the test's last recorded run),
  *status*, *help*.

  - **While the run goes,** the run answers from its live numbers. `serve`
  passes the question over the control channel; the run told `serve` its
  message id on stderr when it posted the message.
  - **After it ends,** `serve` answers from the recorded report, compared
  with the run before it.
  - **A mention anywhere else** gets a note on where to ask.

  The manifest adds `app_mentions:read` and the `app_mention` event. **An
  existing app needs `pepe slack connect` again.**

  ## Checked

  - 407 tests pass; clippy 1.98 is clean. New tests cover:
    - the format of the "What changed" lines;
    - the live message's six fields and its "What changed" list;
    - matching a question to its topic, and each topic's answer;
    - the `ask` command on the control channel.
  - Against the fake Slack (which rejects repeated action ids) and a fake
  Socket Mode server:
  - "why are errors up?" and "p99?" mid-run were answered in the live
  message's thread;
    - the live edits were accepted;
    - "compare" after the stop was answered from the recorded report;
    - a mention in another thread got the note;
    - no anomaly replies were posted.


## [0.50.0](https://github.com/omarmhaimdat/pepe/compare/v0.49.1...v0.50.0) - 2026-10-18

### Added

- *(export)* the report as a page, a summary, text or CSV: F4 on any run screen, --export FILE, pepe export ([#211](https://github.com/omarmhaimdat/pepe/pull/211))

  The report in the shape someone needs it, from the screen, the command
  line, or a saved report. Built to the design bar for user-facing output.

  ```bash
  pepe -z 5m -c 50 --fail-if 'p99 > 300ms' --export checkout.html --export checkout.md https://api.example.com/checkout
  pepe export report.json --to all
  ```

  On any run screen (dashboard, ramp, ping), **F4** opens the export page,
  during the run or after it.

  ## The five shapes

  The file's extension picks the shape.

  - **`.html`**: one self-contained page, about 15 KB. It loads nothing
  and runs no script, and it follows the reader's light or dark setting.
  It contains:
    - Pepe's face for the verdict;
    - the run summed up in a sentence, and the key numbers;
    - the run's curves second by second, with each anomaly marked on them;
    - the latency spread from p50 to the max;
    - where the time went;
    - each `--fail-if` with the measured value;
    - for a saved test, a before/now table against its last recorded run;
    - findings with the first body of each kind of failure;
    - status codes, endpoints or steps, and the slowest requests.

  A ramp's page plots throughput and p99 against concurrency, with a table
  of steps.
  - **`.json`**: the report itself.
  - **`.md`**: a summary for a pull request or an issue: verdict, a table
  of numbers, thresholds, comparison, findings, command.
  - **`.txt`**: what pepe prints when you quit.
  - **`.csv`**: one row per second for a run, per step for a ramp, per
  target for a ping.

  ## Where you get them

  - **F4 on the dashboard, ramp and ping screens**, during the run or
  after:
  - save any one shape, or all of them, to `exports/` (Tab changes the
  location);
    - optionally open the page in the browser;
    - copy the Markdown summary;
    - send the verdict to the run's webhooks now.

  It exports the run as it stands at that moment. The footer shows "F4
  export" on those screens.
  - **`--export FILE`**, repeatable, or `export = [...]` in `pepe.toml`.
  The file is written when the run ends, at the same point as the Slack
  verdict, so it has the same thresholds and comparison.
  - **`pepe export REPORT.json [-o FILE] [--to html|json|md|txt|csv|all]
  [--open]`** for a report already written. Saved reports only keep
  per-minute points, and they have no thresholds or comparison.

  `--html` and `pepe html` from the first commit here became `--export`
  and `pepe export`. Neither was ever released.

  ## Also

  A ramp's findings are named `title`, not `text`, so the Slack verdict
  for a ramp was dropping them. Fixed and tested.

  ## Checked

  - 403 tests pass; clippy 1.98 is clean. New tests cover the page (run
  and ramp), the Markdown summary, the text fallback, the CSV,
  format-from-extension, file names, and the export page's keys (save one,
  save all, Where, close).
  - Pages from real runs against the bench server, rendered in headless
  Chrome in light and dark. Fixed after looking:
    - an empty request-id column;
    - the version wrapping mid-number;
    - "against" said twice in the comparison heading;
    - no title on the ramp page.
  - The F4 page driven on the real dashboard in a pseudo-terminal:
    - opened mid-run;
    - "Everything" wrote all five files;
    - after the run ended, the Markdown summary saved correctly.
  - `--export` with three files, `pepe export --to all`, and unknown
  extensions, which are refused with the reason.


## [0.49.1](https://github.com/omarmhaimdat/pepe/compare/v0.49.0...v0.49.1) - 2026-10-18

### Fixed

- *(slack)* the live message is posted again: − 1 and + 1 had the same action id, which Slack refuses ([#209](https://github.com/omarmhaimdat/pepe/pull/209))

  Found by running #207 against a real workspace.

  ## The bug

  A run's controls had two buttons with the action id `pepe:size` (− 1 and
  + 1). Slack refuses a block whose buttons repeat an id
  (`invalid_blocks`). The consequences:
  - the run's live message and all of its updates were rejected;
  - its thread replies are plain text, so they went to the channel on
  their own.

  Nothing told you about it: the rejection went to the run's stderr, which
  `serve` only showed when a run failed to start.

  ## Fixes

  - **Distinct ids:** − 1 is `pepe:less` and + 1 is `pepe:more`.
  `pepe:size` is still accepted. A test checks that each id appears once
  in the row.
  - **Warnings in the terminal:** `pepe slack serve` streams a run's
  warnings as they happen (a message Slack refused, a hook that failed).
  - **The card follows the run:**
  - after ▶ Run, the test card turns into the run's controls (Pause,
  Stop);
  - Run on a test that's already going answers with those controls instead
  of "stop it first";
    - the edit form's run answers with the card too.
  - **Trend and wording:** the trend shows only with three runs or more,
  and a single run reads "1 run, just now".

  ## Checked

  - 397 tests pass; clippy 1.98 is clean.
  - The fake Slack now refuses repeated action ids, as Slack does. With
  it, the full scenario passes:
    - the card turned into the controls after Run;
    - the live message posted with − 1 (3) and + 1 (5);
    - every edit was accepted: pause, a concurrency change, resume, stop;
    - the verdict followed.


## [0.49.0](https://github.com/omarmhaimdat/pepe/compare/v0.48.0...v0.49.0) - 2026-10-18

### Added

- *(slack)* pause, resize and stop a run from its message, and /pepe as a dropdown of the saved tests ([#207](https://github.com/omarmhaimdat/pepe/pull/207))

  Builds on #205: richer commands, and control over a run while it goes.

  ## While a run goes

  A run started from Slack gets these controls on its message:
  - **⏸ Pause / ▶ Resume.** While paused, the message turns grey and says
  who paused it.
  - **− 1 / + 1** concurrency. Each button shows the value it sets, e.g.
  "+ 1 (51)". At 1 there is no − button. A ramp sets its own concurrency,
  so it gets Pause only.
  - **■ Stop**, with a confirmation.

  Every change shows in the message at once ("<@maya> raised it to 51 at
  once"), instead of at the next 10 s update.

  ## Shorter thread replies

  Each change during a run is one short line in the thread, saying what
  moved and which way:
  - `📉 Throughput dropped 52% · 5.9k req/s · at 1m26s`
  - `📈 p99 up 4.5× · 45ms · at 26s`
  - `🔥 Errors up to 12% · at 2m` (posted with Pepe's on-fire face)

  How it works:
  - `serve` sends commands over the stdin of the run it started
  (`src/control.rs`: `pause U1`, `concurrency 63 U1`, `stop U1`).
  - Every `--json` run loop listens for them when `serve` started it: run,
  flow, API, replay and ramp. They're applied the same way as the
  dashboard's keys.
  - A stop the run can't hear falls back to a ctrl-c signal.

  ## /pepe

  - **`/pepe`, `/pepe list`, or `/pepe run` with no name** opens a card:
  the saved tests in a dropdown, plus what's running with its controls.
  - **Picking a test, or `/pepe NAME`,** shows that test's card in place:
    - what it sends;
    - its last run;
    - its trend over the last eight runs, as sparklines;
    - Run, Edit and run, History.
  - **History** is one aligned row per run: outcome, when, req/s, p99,
  errors.
  - **`/pepe status`** shows what's running.
  - **Runs started from Slack are now recorded,** so the history and trend
  stay current.

  ## Checked

  - 397 tests pass; clippy 1.98 is clean.
  - Against the fake Slack and fake Socket Mode, with a 20 s saved test:
    1. `/pepe` answered with the dropdown card.
    2. Picking a test replaced the card in place.
    3. Run started the test.
    4. Pause: the message turned grey with Resume and "<@U1> paused it".
  5. +1 raised concurrency and recomputed both buttons. (Exercised with
  the earlier 25% steps; the ±1 buttons and the thread lines are covered
  by unit tests.)
    6. Resume restarted it.
    7. `/pepe status` showed the running card.
  8. Stop ended it: the verdict appeared with its buttons, followed by the
  result line.
    9. History listed the run, recorded.

  Not checked against a real Slack workspace.


## [0.48.0](https://github.com/omarmhaimdat/pepe/compare/v0.47.0...v0.48.0) - 2026-10-18

### Added

- *(slack)* Stop, Run again and Edit and run on the messages, and /pepe, answered by pepe slack serve ([#205](https://github.com/omarmhaimdat/pepe/pull/205))

  Part 3 of the Slack work: run saved tests from the channel.

  ```bash
  pepe slack connect      # steps 5 and 6: the app token, and who may press the buttons
  pepe slack serve        # in the project, left running
  ```

  ## What it adds

  - **On the messages:**
  - a saved test's verdict gets **↻ Run again**, **✎ Edit and run** and
  **History**;
  - a run started from Slack gets **■ Stop** while it goes, with a
  confirmation;
    - the line under the title says who started it.
  - **`/pepe`:** `run NAME`, `stop`, `history NAME` and `list`, in any
  channel the app is in.
  - **`pepe slack serve`:** the one listener. Slack sends every click of
  an app to one of its Socket Mode connections, not to the run that posted
  the message, so a single process answers them all. It:
  - runs this directory's saved tests as `pepe --load NAME
  --slack=<channel>` child processes, under this directory's `pepe.toml`
  guardrails;
  - stops the runs it started the way ctrl-c does, so the verdict still
  says what was measured;
  - opens the edit form, filled from the saved test, and shows a bad value
  beside its field;
    - logs each click, run and result in its terminal;
    - reconnects with backoff.
  - **Who may use it:**
  - `pepe slack allow @a @b` (or `*`) sets the list; names are looked up
  in the workspace;
    - anyone else gets a refusal only they can see;
  - nothing can be run from Slack that isn't a saved test in that
  directory.
  - **`pepe slack connect`** gains two steps:
  - 5, the app-level token, checked by opening a Socket Mode connection,
  or skipped with Enter;
    - 6, who may press the buttons.

  The manifest turns on Socket Mode, interactivity and `/pepe`, and adds
  the `commands` and `users:read` permissions. An app made by an earlier
  pepe needs `pepe slack connect` run again.
  - **`--slack` replaces a saved test's own Slack hook,** keeping its
  `every` and `mention`, so a run started from Slack still gets one
  message, not two.
  - **`src/ws.rs`:** pepe's own websocket client. It handles the handshake
  over the same rustls, masked text frames, fragments, pings and close. No
  new dependency.

  ## Checked

  - **Rust tests (395 pass):**
  - the websocket client against a test server: messages both ways, a
  message split in three, a ping answered, close;
    - how clicks and commands are read;
    - the allow list;
    - turning the form's values into flags, and the errors for bad values;
    - the form filled from a saved test;
    - buttons only where something answers them.
  - **Lint:** clippy 1.98 is clean, and the docs and avatar checks pass.
  - **`pepe slack serve` against a fake Socket Mode server** (Python,
  standard library), which scripted:
  1. Run again from an allowed user. The child posted its message with
  Stop.
  2. `/pepe stop`. The run ended "Stopped", its verdict showed the three
  buttons, and the result line followed.
  3. A click from someone not allowed. The refusal went to `response_url`
  only.
  4. The edit form with concurrency 0. The acknowledgement carried a field
  error.
    5. Edit and run. The form opened (`views.open`).
    6. `/pepe list`.

    When the fake closed, serve reconnected with backoff.
  - **The six-step connect checklist** was driven in a pseudo-terminal.
  `slack.toml` kept the app token and the allowed id.

  **Not checked against a real workspace:**
  - Socket Mode against Slack's own servers;
  - the slash command registered from the manifest;
  - how the form and the buttons look in Slack.

  ## 1.0

  Adds `pepe slack serve`, `pepe slack allow` and `PEPE_SLACK_APP_TOKEN`.
  That restarts the freeze clock. The report schema is unchanged.


## [0.47.0](https://github.com/omarmhaimdat/pepe/compare/v0.46.0...v0.47.0) - 2026-10-18

### Added

- *(slack)* stylish, informative messages with Pepe's face, a checklist for pepe slack connect, and webhook cards ([#203](https://github.com/omarmhaimdat/pepe/pull/203))

  A redesign of everything Slack-facing, plus the F3 page.

  ## Slack messages (`src/webhook/layout.rs`)

  Each message has a colour down its side, and Pepe the pepper is its
  avatar.

  **While a run goes** (edited every 10 s):
  - what is being sent, how hard, from which machine and by whom;
  - a progress bar with time left;
  - throughput and p50 · p99, each with a sparkline of the last minute;
  - errors by status and cause;
  - a count of anomalies, each also posted as a reply in the thread.

  **At the end**, the same message becomes the verdict:
  - a summary sentence: how much was sent, how fast, what failed, and the
  first thing that changed;
  - latency p50, p95, p99 and max;
  - sparklines over the whole run;
  - where the time went: waiting, connecting, reading;
  - the answers by status code;
  - each `--fail-if` with ✓ or ✗ and the measured value;
  - for a saved test, the significant changes against its last recorded
  run, with a warning when the two runs weren't set up alike;
  - findings, worst first, with the first body of each kind of failure.

  **Under the verdict:** a thread reply that's also shown in the channel.
  It carries the result in one line with Pepe proud, worried or on fire,
  plus the `mention` when the run failed.

  ## Pepe's faces

  - `site/build-avatars.py` draws 512 px PNGs straight from the mascot's
  sprite in `src/ui/mascot.rs`. It uses only the Python standard library,
  and CI checks the images are current.
  - They're served from the site, which already publishes
  `site/img/*.png`.
  - The app manifest now asks for `chat:write.customize`, which Slack
  requires for custom avatars. Apps made with 0.46 need `pepe slack
  connect` run again; the docs say so.

  ## `pepe slack connect`

  At a terminal, it's a checklist redrawn in place:
  - Pepe's sprite beside the title.
  - The prefilled app page opens in the browser. If no browser opens, the
  link is copied to the clipboard and shown as a clickable link instead of
  a wall of encoded text.
  - The token is shown as dots and checked with a spinner. A rejected
  token gets a clear error and the field clears for another try.
  - The channel is picked with the arrow keys from a list filtered as you
  type, showing member counts, whether it's private, and whether pepe is
  already in it.
  - At the end:
    - the test message's permalink;
    - Pepe's icon saved for the app;
    - commands to try.

  Outside a terminal, or with `--token` and `--channel`, it still asks
  nothing and reports one step per line.

  ## F3 webhook page

  Each webhook is a framed card. Its title shows where it goes (Slack and
  the channel, Discord, Teams, Google Chat, or JSON and the host). Its
  corner shows the last delivery: sent and how long ago, or the error.

  ## Checked

  - `cargo test`: 389 pass, including new tests for sparklines, the
  progress bar, the time split and status codes. Clippy is clean, and the
  docs and avatar checks pass.
  - Against a fake Slack API, two saved-test runs with `--slack` (one with
  `--fail-if` crossed, one passing). Each produced, in order:
    1. a start post with Pepe's waiting face;
    2. a live edit with the bar and sparklines;
    3. the verdict, with thresholds, the comparison and findings;
  4. the result reply with the proud or worried face, shown in the
  channel.
  - The connect wizard and the F3 cards were driven in a pseudo-terminal
  and the screens replayed: wrong token, then right token, filtered
  channel pick, summary.

  **Not checked against a real Slack workspace**: whether the avatars, the
  colour bars and the `<!date>` timestamps look in Slack the way the
  payload intends.

  Next, in its own PR on top of this once it's merged: buttons to stop,
  run again and edit and run, through `pepe slack serve` with Socket Mode,
  for saved tests only and allowed users.


## [0.46.0](https://github.com/omarmhaimdat/pepe/compare/v0.45.0...v0.46.0) - 2026-10-17

### Added

- *(slack)* --slack keeps one Slack message up to date as a run goes, and pepe slack connect sets it up from the terminal ([#201](https://github.com/omarmhaimdat/pepe/pull/201))

  Part 2 of the webhooks work: Slack, live, set up from the terminal.
  Nothing is hosted; the app belongs to the user's own workspace.

  ```bash
  pepe slack connect                                   # once per machine
  pepe -z 10m -c 50 --slack https://api.example.com/health
  ```

  ## `pepe slack connect`

  1. Opens Slack's "create an app" page with the manifest in the link:
  name, permissions (`chat:write`, `chat:write.public`, `channels:read`,
  `channels:join`, `groups:read`). The user picks a workspace, then clicks
  **Create**, **Install** and **Allow**.
  2. Takes the pasted bot token, shown as dots, and checks it right away
  with `auth.test`.
  3. Picks the channel by name or part of one, joins it, posts a test
  message, and saves the token and channel to `slack.toml` beside the
  settings. The file is `0600`, readable only by the user.


## [0.45.0](https://github.com/omarmhaimdat/pepe/compare/v0.44.0...v0.45.0) - 2026-10-17

### Added

- *(webhooks)* --webhook sends the verdict to Slack, Discord, Teams, Google Chat or any URL, and F3 sets them on any screen ([#199](https://github.com/omarmhaimdat/pepe/pull/199))

  Part 1 of the webhooks work: easy by default, customisable in every
  place a run is configured.

  ```bash
  pepe -z 30s -c 50 --fail-if 'p99 > 300ms' --webhook "$SLACK_WEBHOOK_URL" https://api.example.com/health
  ```

  ## What it does

  - **The URL picks the layout.**
    - `hooks.slack.com`: Slack blocks.
    - `discord.com/api/webhooks`: a Discord embed coloured by the outcome.
  - Teams (`*.webhook.office.com`, workflows on `*.logic.azure.com`): an
  Adaptive Card.
    - `chat.googleapis.com`: Google Chat text.
    - Anything else: JSON with the event, the run and the whole report.

    `format` can force any of these, or `text`.
  - **Events**: `start`, `progress` (a bar, requests, req/s, p50, p99 and
  errors, every `every`), `anomaly` (sent as it happens), `end`, and
  `fail`. `fail` fires when a `--fail-if` was crossed, a ping's SLO was
  broken, or nothing answered. The default is `end`.
  - **Config**: `webhook = "URL"` or `[[webhook]]` tables in `pepe.toml`.
  The same key works under the settings' `[defaults]` (machine-wide) and
  in saved tests (`--keep`, ctrl-s). Each table takes `on`, `format`,
  `headers`, `every`, `mention`, and a `body` template with `{{holes}}`
  (status, p99, rps, report, …).
  - **Secrets**: `${NAME}` is read from the environment. An unset name, a
  bad URL or an unknown hole is refused before anything is sent. URLs are
  masked in reports and in the recorded command
  (`https://hooks.slack.com/••••`).
  - **F3 on every screen** opens a webhook page next to F2's settings.
  - On the setup form, it sets the hooks of the run you're about to start.
    - During a run, a hook added or changed is sent to from then on.
  - Paste or type a URL; `1`–`5` tick events; ← → step the format; `t`
  sends a test and shows the answer; del removes the hook.
  - Edits are kept for restarts, ctrl-s and the command printed at the
  end.
  - The dashboard, ramp and ping screens now pass pasted text to the
  pages.
  - **Every mode sends**: run, flow, api and replay (on screen, headless,
  `--json`), plus ramp and ping.
  - **Delivery**:
  - Each webhook has its own in-order queue, with three tries on a dropped
  connection, a 429 or a 5xx, following `Retry-After`.
  - The run never waits on webhooks. pepe waits up to 15 s at the end and
  reports failures on stderr; the exit code is unchanged.
    - `--dry-run` lists the webhooks and sends nothing.
  - **MCP**: the tools take `webhook` and `webhook_on`.

  ## Checked

  - Unit tests:
    - format detection;
    - the file forms (URL, table, list) round-trip;
    - `${ENV}` and masking;
    - each layout;
    - templates;
    - the progress line;
    - in-order delivery with a retried 503;
    - the page's keys and drawing;
    - saved tests keep hooks and mask them in the command.
  - By hand against a local receiver: flags with start/end/fail,
  `pepe.toml` tables with headers, progress, Slack layout and a template.
  - Driving the real TUI in a pseudo-terminal: F3 mid-run, paste a URL,
  and the run's end arrives there. Also F3 on the setup form, then start.
  The recorded command shows the URL masked.
  - Speed is unchanged (bench server, 200k at 64, three runs each): master
  6.10–6.20 CPU ms per 1k, this branch 6.05–6.10.

  ## Not in this PR (the next two)

  - A Slack bot token: one message edited in place as the run goes, with
  anomalies threaded under it.
  - Socket Mode: stop, rerun and edit-and-rerun buttons, and `/pepe`.
  These act on saved tests only, for allowed users.

  ## 1.0

  Adds `--webhook`, `--webhook-on`, the `webhook` key and the MCP
  arguments, which restarts the freeze clock. The report schema is
  unchanged.


## [0.44.0](https://github.com/omarmhaimdat/pepe/compare/v0.43.1...v0.44.0) - 2026-10-17

### Added

- *(vars)* --vars users.csv fills the {{holes}} in a request, a row a request ([#197](https://github.com/omarmhaimdat/pepe/pull/197))

  The data-driven requests from the roadmap (section 4), and the last 1.0
  item that could shape `pepe.toml`'s keys.

  ```bash
  pepe -z 30s -c 20 --vars users.csv \
    -H 'Authorization: Bearer {{token}}' -m POST -d '{"email": "{{email}}"}' \
    'https://api.example.com/users/{{id}}'
  pepe flow checkout.toml -z 1m --vars users.csv
  ```

  ## What it does

  - `{{name}}` holes in the URL (host included), headers and body take
  their values from a CSV file whose first line names them (RFC 4180
  quoting, a BOM and CRLF are fine).
  - **Run and ramp**: one request per row, in the file's order, round
  again.
  - **Flow**: each chain starts with the next row's values, over `[vars]`;
  captures go over both. Columns count as known when the flow is checked.
  - **Checked before anything is sent**: every hole must be a column, and
  the request must have at least one hole (both are errors that name the
  cause). Every row's host is held to `--allow-host`. `--dry-run` says how
  many rows there are and what the columns are. `api`, `replay` and `ping`
  refuse `--vars`.
  - **Config, saved tests and MCP**: `vars = "users.csv"` in `pepe.toml`
  (and so in `--keep` tests and `--write-config`); a `vars` argument on
  the MCP `run`, `ramp` and `flow` tools. It is not taken from the
  machine's `[defaults]`.
  - Without `--vars`, nothing changes: `{{` in a URL is sent as written.

  ## Cost

  Each row becomes a prepared request before the run, on the direct path
  like any target, so sending is unchanged (bench server, 200k requests at
  64, three runs each, CPU ms per 1k):

  | | CPU ms / 1k | Peak RSS |
  | --- | --- | --- |
  | master | 6.10 – 6.45 | 10.1 – 10.6 MB |
  | this branch, no `--vars` | 6.20 – 6.30 | 9.6 – 10.2 MB |
  | `--vars` 10,000 rows | 6.25 – 6.40 | 18.3 – 18.6 MB |

  Memory is about 1.6 KB per row (190 MB for 100k rows, 1.6 GB for a
  million, which is the cap). A million rows take about 2 s to prepare.
  Two fixes keep it at that size:
  - Each row keeps only the headers that have holes.
  - Targets with equal weights are sent in turn. The weighted round-robin
  cost the square of their number to compute.

  The schedule index went from `u16` to `u32`, so more than 65,535 targets
  now fit. Recorded in `bench/README.md`.

  ## 1.0

  This adds a flag (`--vars`), a `pepe.toml` key (`vars`) and an MCP
  argument. That restarts the month-without-a-flag-change clock. It is
  also the 1.0 prerequisite ticked in ROADMAP.md. The report schema is
  unchanged: the command line the report already records includes
  `--vars`.

  ## Tests

  - CSV parsing and error locations.
  - Filling a request row by row.
  - A live run checked request by request: rows in order, templated and
  static headers, body.
  - Validation, including the modes that refuse `--vars`.
  - The config round-trip.
  - Checked by hand: flows, ramps, guardrails, dry runs.

  `site/docs/*.html` is rebuilt here from master's generator. It will
  conflict with omarmhaimdat/pepe#196; rebuild with `python3
  site/build-docs.py` after whichever lands second.



### Other

- *(site)* titles, descriptions and structured data a search engine reads, a sitemap and robots.txt ([#196](https://github.com/omarmhaimdat/pepe/pull/196))

  The documentation site and the install page, made easier to find.

  - **Titles**: each docs page's `<title>` carries the words someone would
  search for (\`pepe ping · HTTP ping with latency split by phase\`); the
  sidebar keeps the short names.
  - **Descriptions**: the page's opening, cut at a sentence end near 160
  characters instead of mid-word at 300.
  - **Cards**: Open Graph (`og:url`, `og:site_name`, image size and alt)
  and Twitter large-image cards on every page.
  - **Structured data**: `TechArticle` and `BreadcrumbList` JSON-LD on
  docs pages; the install page's `SoftwareApplication` now names the npm
  and PyPI packages, the install and docs pages, plus a `WebSite` entry.
  - **Sitemap and robots.txt**: `build-docs.py` writes `site/sitemap.xml`
  (no dates, so it isn't stale after every merge); `robots.txt` points to
  it; `publish.sh` uploads both, and `publish-site.yml` checks they answer
  200 after publishing.

  No change to the binary, flags or reports.

  `python3 site/build-docs.py --check` passes.


## [0.43.1](https://github.com/omarmhaimdat/pepe/compare/v0.43.0...v0.43.1) - 2026-10-17

### Fixed

- *(docker)* the from-source image copies the schemas and the plugin marketplace it embeds ([#192](https://github.com/omarmhaimdat/pepe/pull/192))

  The from-source `Dockerfile` copied `src` and `contrib` only, but the
  binary embeds `schema/*.schema.json` and
  `.claude-plugin/marketplace.json` with `include_str!`, so `docker build
  .` failed with `couldn't read src/../schema/run.schema.json` (seen on
  #185's "The from-source image" check). It now copies both.


- *(ui)* the screen no longer freezes when the terminal falls behind ([#191](https://github.com/omarmhaimdat/pepe/pull/191))

  The dashboard could freeze, for example in the request inspector while
  scrolling a response with the trackpad.

  ## Why

  Every screen wrote its frames on the same loop that reads keys and the
  mouse. When the terminal stopped reading for a moment, that write
  blocked and pepe stopped reading input. A terminal that was itself
  waiting to hand pepe input (a flood of wheel events, and since v0.38
  every pointer move) then never read again. A stack sample of the frozen
  process shows the main thread blocked in `write()`.
  [a3fae55](https://github.com/omarmhaimdat/pepe/commit/a3fae55) made
  Ctrl-C work in that state; this removes the state itself.

  ## What changes

  - **Frames are written on a thread of their own** (`terminal::Screen`).
  A screen skips drawing while the terminal hasn't taken the last frame,
  then draws again as soon as it has (`caught_up()`), so input is always
  read. Leaving the screen waits up to a second for the last frames, so
  none lands on the shell. All eight screens use it.
  - **Only clicks and the wheel are requested** (modes 1000, 1015, 1006).
  crossterm's `EnableMouseCapture` also turned on 1002 and 1003 (every
  pointer move), which no screen uses and which redrew the whole screen on
  each move.
  - **Split mouse reports no longer act as keys.** When a read ended right
  after the ESC of a mouse report, crossterm reported Esc, and
  `[<65;80;25M` arrived as typed keys: the inspector closed, the digits
  changed tabs, and a second Esc quit. Now an Esc followed within 30ms by
  `[` is dropped up to the end of the sequence (`terminal::input()`).

  ## Testing

  - `cargo test`: 361 pass, including new tests for the split-sequence
  filter. `cargo clippy --all-targets` is clean.
  - Each harness drove the release binary in a pty against a local server
  returning a 27 KiB JSON body, with the inspector open.
  - **A terminal that writes input before reading output:** master froze
  in 7 of 7 runs; this branch froze in none of 6, with about 20,000 events
  in 8 seconds each.
  - **A flood of motion and wheel events:** master quit on a stray Esc;
  this branch kept running and still answered a key afterwards.
  - Quitting still leaves the alternate screen before the verdict is
  printed.
  - Not yet tried by hand in Ghostty or iTerm2.



### Other

- *(roadmap)* the versions that shipped each item, and the saved tests ([#186](https://github.com/omarmhaimdat/pepe/pull/186))

  - Every "next release" now names the version that shipped it (v0.17.0 to
  v0.36.0).
  - A new section, **3c. Tests worth keeping**: saved tests, the library,
  `--keep`, recorded runs, a ping as a saved test, and the settings page
  (v0.37.0 to v0.42.0), and three open items: finding the spec from a base
  URL, `pepe init URL`, and Server-Timing recipes.
  - Agents: the Claude Code plugin, npm and PyPI, and the MCP registry.
  - 1.0: `cargo audit` gets its version; the month without a flag change
  restarted with v0.42.0, so 2026-11-13 at the earliest.
  - Not now: themes shipped, so they're out of the dashboard extras.


- bump the github-actions group across 1 directory with 3 updates ([#185](https://github.com/omarmhaimdat/pepe/pull/185))

  Bumps the github-actions group with 3 updates in the / directory:
  [actions/cache](https://github.com/actions/cache),
  [actions/setup-node](https://github.com/actions/setup-node) and
  [actions/setup-python](https://github.com/actions/setup-python).

  Updates `actions/cache` from 4 to 6
  <details>
  <summary>Release notes</summary>
  <p><em>Sourced from <a
  href="https://github.com/actions/cache/releases">actions/cache's
  releases</a>.</em></p>
  <blockquote>
  <h2>v6.0.0</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>Update packages, migrate to ESM by <a
  href="https://github.com/Samirat"><code>@​Samirat</code></a> in <a
  href="https://redirect.github.com/actions/cache/pull/1760">actions/cache#1760</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/cache/compare/v5...v6.0.0">https://github.com/actions/cache/compare/v5...v6.0.0</a></p>
  <h2>v5.1.0</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>Bump <code>@​actions/cache</code> to v5.1.0 - handle read-only cache
  access by <a
  href="https://github.com/jasongin"><code>@​jasongin</code></a> in <a
  href="https://redirect.github.com/actions/cache/pull/1775">actions/cache#1775</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/cache/compare/v5...v5.1.0">https://github.com/actions/cache/compare/v5...v5.1.0</a></p>
  <h2>v5.0.5</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>Update ts-http-runtime dependency by <a
  href="https://github.com/yacaovsnc"><code>@​yacaovsnc</code></a> in <a
  href="https://redirect.github.com/actions/cache/pull/1747">actions/cache#1747</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/cache/compare/v5...v5.0.5">https://github.com/actions/cache/compare/v5...v5.0.5</a></p>
  <h2>v5.0.4</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>Add release instructions and update maintainer docs by <a
  href="https://github.com/Link"><code>@​Link</code></a>- in <a
  href="https://redirect.github.com/actions/cache/pull/1696">actions/cache#1696</a></li>
  <li>Potential fix for code scanning alert no. 52: Workflow does not
  contain permissions by <a
  href="https://github.com/Link"><code>@​Link</code></a>- in <a
  href="https://redirect.github.com/actions/cache/pull/1697">actions/cache#1697</a></li>
  <li>Fix workflow permissions and cleanup workflow names / formatting by
  <a href="https://github.com/Link"><code>@​Link</code></a>- in <a
  href="https://redirect.github.com/actions/cache/pull/1699">actions/cache#1699</a></li>
  <li>docs: Update examples to use the latest version by <a
  href="https://github.com/XZTDean"><code>@​XZTDean</code></a> in <a
  href="https://redirect.github.com/actions/cache/pull/1690">actions/cache#1690</a></li>
  <li>Fix proxy integration tests by <a
  href="https://github.com/Link"><code>@​Link</code></a>- in <a
  href="https://redirect.github.com/actions/cache/pull/1701">actions/cache#1701</a></li>
  <li>Fix cache key in examples.md for bun.lock by <a
  href="https://github.com/RyPeck"><code>@​RyPeck</code></a> in <a
  href="https://redirect.github.com/actions/cache/pull/1722">actions/cache#1722</a></li>
  <li>Update dependencies &amp; patch security vulnerabilities by <a
  href="https://github.com/Link"><code>@​Link</code></a>- in <a
  href="https://redirect.github.com/actions/cache/pull/1738">actions/cache#1738</a></li>
  </ul>
  <h2>New Contributors</h2>
  <ul>
  <li><a href="https://github.com/XZTDean"><code>@​XZTDean</code></a> made
  their first contribution in <a
  href="https://redirect.github.com/actions/cache/pull/1690">actions/cache#1690</a></li>
  <li><a href="https://github.com/RyPeck"><code>@​RyPeck</code></a> made
  their first contribution in <a
  href="https://redirect.github.com/actions/cache/pull/1722">actions/cache#1722</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/cache/compare/v5...v5.0.4">https://github.com/actions/cache/compare/v5...v5.0.4</a></p>
  <h2>v5.0.3</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>Bump <code>@actions/cache</code> to v5.0.5 (Resolves: <a
  href="https://github.com/actions/cache/security/dependabot/33">https://github.com/actions/cache/security/dependabot/33</a>)</li>
  <li>Bump <code>@actions/core</code> to v2.0.3</li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/cache/compare/v5...v5.0.3">https://github.com/actions/cache/compare/v5...v5.0.3</a></p>
  <h2>v.5.0.2</h2>
  <h1>v5.0.2</h1>
  <h2>What's Changed</h2>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Changelog</summary>
  <p><em>Sourced from <a
  href="https://github.com/actions/cache/blob/main/RELEASES.md">actions/cache's
  changelog</a>.</em></p>
  <blockquote>
  <h1>Releases</h1>
  <h2>How to prepare a release</h2>
  <blockquote>
  <p>[!NOTE]
  Relevant for maintainers with write access only.</p>
  </blockquote>
  <ol>
  <li>Switch to a new branch from <code>main</code>.</li>
  <li>Run <code>npm test</code> to ensure all tests are passing.</li>
  <li>Update the version in <a
  href="https://github.com/actions/cache/blob/main/package.json"><code>https://github.com/actions/cache/blob/main/package.json</code></a>.</li>
  <li>Run <code>npm run build</code> to update the compiled files.</li>
  <li>Update this <a
  href="https://github.com/actions/cache/blob/main/RELEASES.md"><code>https://github.com/actions/cache/blob/main/RELEASES.md</code></a>
  with the new version and changes in the <code>## Changelog</code>
  section.</li>
  <li>Run <code>licensed cache</code> to update the license report.</li>
  <li>Run <code>licensed status</code> and resolve any warnings by
  updating the <a
  href="https://github.com/actions/cache/blob/main/.licensed.yml"><code>https://github.com/actions/cache/blob/main/.licensed.yml</code></a>
  file with the exceptions.</li>
  <li>Commit your changes and push your branch upstream.</li>
  <li>Open a pull request against <code>main</code> and get it reviewed
  and merged.</li>
  <li>Draft a new release <a
  href="https://github.com/actions/cache/releases">https://github.com/actions/cache/releases</a>
  use the same version number used in <code>package.json</code>
  <ol>
  <li>Create a new tag with the version number.</li>
  <li>Auto generate release notes and update them to match the changes you
  made in <code>RELEASES.md</code>.</li>
  <li>Toggle the set as the latest release option.</li>
  <li>Publish the release.</li>
  </ol>
  </li>
  <li>Navigate to <a
  href="https://github.com/actions/cache/actions/workflows/release-new-action-version.yml">https://github.com/actions/cache/actions/workflows/release-new-action-version.yml</a>
  <ol>
  <li>There should be a workflow run queued with the same version
  number.</li>
  <li>Approve the run to publish the new version and update the major tags
  for this action.</li>
  </ol>
  </li>
  </ol>
  <h2>Changelog</h2>
  <h3>6.1.0</h3>
  <ul>
  <li>Bump <code>@actions/cache</code> to v6.1.0 to pick up <a
  href="https://redirect.github.com/actions/toolkit/pull/2435">actions/toolkit#2435
  Handle cache write error due to read-only token</a></li>
  <li>Switch redundant &quot;Cache save failed&quot; warning to debug log
  in save-only</li>
  </ul>
  <h3>6.0.0</h3>
  <ul>
  <li>Updated <code>@actions/cache</code> to ^6.0.1,
  <code>@actions/core</code> to ^3.0.1, <code>@actions/exec</code> to
  ^3.0.0, <code>@actions/io</code> to ^3.0.2</li>
  <li>Migrated to ESM module system</li>
  <li>Upgraded Jest to v30 and test infrastructure to be ESM
  compatible</li>
  </ul>
  <h3>5.0.4</h3>
  <ul>
  <li>Bump <code>minimatch</code> to v3.1.5 (fixes ReDoS via globstar
  patterns)</li>
  <li>Bump <code>undici</code> to v6.24.1 (WebSocket decompression bomb
  protection, header validation fixes)</li>
  <li>Bump <code>fast-xml-parser</code> to v5.5.6</li>
  </ul>
  <h3>5.0.3</h3>
  <ul>
  <li>Bump <code>@actions/cache</code> to v5.0.5 (Resolves: <a
  href="https://github.com/actions/cache/security/dependabot/33">https://github.com/actions/cache/security/dependabot/33</a>)</li>
  <li>Bump <code>@actions/core</code> to v2.0.3</li>
  </ul>
  <h3>5.0.2</h3>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/actions/cache/commit/55cc8345863c7cc4c66a329aec7e433d2d1c52a9"><code>55cc834</code></a>
  Merge pull request <a
  href="https://redirect.github.com/actions/cache/issues/1768">#1768</a>
  from jasongin/readonly-cache</li>
  <li><a
  href="https://github.com/actions/cache/commit/d8cd72f230726cdf4457ebb61ec1b593a8d12337"><code>d8cd72f</code></a>
  Bump <code>@​actions/cache</code> to v6.1.0 - handle cache write error
  due to RO token</li>
  <li><a
  href="https://github.com/actions/cache/commit/2c8a9bd7457de244a408f35966fab2fb45fda9c8"><code>2c8a9bd</code></a>
  Merge pull request <a
  href="https://redirect.github.com/actions/cache/issues/1760">#1760</a>
  from actions/samirat/esm_migration_and_package_update</li>
  <li><a
  href="https://github.com/actions/cache/commit/e9b91fdc3fea7d79165fceb79042ef45c2d51023"><code>e9b91fd</code></a>
  Prettier fixes</li>
  <li><a
  href="https://github.com/actions/cache/commit/e4884b8ff7f92ef6b52c79eda480bbc86e685adb"><code>e4884b8</code></a>
  Rebuild dist</li>
  <li><a
  href="https://github.com/actions/cache/commit/10baf0191a3c426ea0fa4a3253a5c04233b6e18f"><code>10baf01</code></a>
  Fixed licenses</li>
  <li><a
  href="https://github.com/actions/cache/commit/e39b386c9004d72a15d864ade8c0b3a702d47a37"><code>e39b386</code></a>
  Fix test mock return order</li>
  <li><a
  href="https://github.com/actions/cache/commit/b6928203372a8571ff984c0c883ef3a1adfb0c06"><code>b692820</code></a>
  PR feedback</li>
  <li><a
  href="https://github.com/actions/cache/commit/60749128a44d25d3c520a489e576380cf00ff3f1"><code>6074912</code></a>
  Rebuild dist bundles as ESM to match type:module</li>
  <li><a
  href="https://github.com/actions/cache/commit/5a912e8b4af820fa082a0e75cfd2c782f8fbfe0e"><code>5a912e8</code></a>
  Fix lint and jest issues</li>
  <li>Additional commits viewable in <a
  href="https://github.com/actions/cache/compare/v4...v6">compare
  view</a></li>
  </ul>
  </details>
  <br />

  Updates `actions/setup-node` from 6 to 7
  <details>
  <summary>Release notes</summary>
  <p><em>Sourced from <a
  href="https://github.com/actions/setup-node/releases">actions/setup-node's
  releases</a>.</em></p>
  <blockquote>
  <h2>v7.0.0</h2>
  <h2>What's Changed</h2>
  <h3>Enhancements:</h3>
  <ul>
  <li>Add cache-primary-key and cache-matched-key as outputs by <a
  href="https://github.com/gowridurgad"><code>@​gowridurgad</code></a> in
  <a
  href="https://redirect.github.com/actions/setup-node/pull/1577">actions/setup-node#1577</a></li>
  <li>Migrate to ESM and upgrade dependencies by <a
  href="https://github.com/gowridurgad"><code>@​gowridurgad</code></a> in
  <a
  href="https://redirect.github.com/actions/setup-node/pull/1574">actions/setup-node#1574</a></li>
  </ul>
  <h3>Bug fixes:</h3>
  <ul>
  <li>Remove dummy NODE_AUTH_TOKEN export by <a
  href="https://github.com/gowridurgad"><code>@​gowridurgad</code></a> in
  <a
  href="https://redirect.github.com/actions/setup-node/pull/1558">actions/setup-node#1558</a></li>
  <li>Only use <code>mirrorToken</code> in <code>getManifest</code> if
  it's provided by <a
  href="https://github.com/deiga"><code>@​deiga</code></a> in <a
  href="https://redirect.github.com/actions/setup-node/pull/1548">actions/setup-node#1548</a></li>
  </ul>
  <h3>Documentation updates:</h3>
  <ul>
  <li>Add documentation for publishing to npm with Trusted Publisher
  (OIDC) by <a
  href="https://github.com/chiranjib-swain"><code>@​chiranjib-swain</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-node/pull/1536">actions/setup-node#1536</a></li>
  <li>docs: Update restore-only cache documentation by <a
  href="https://github.com/priya-kinthali"><code>@​priya-kinthali</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-node/pull/1550">actions/setup-node#1550</a></li>
  <li>docs: Update caching recommendations to mitigate cache poisoning
  risks by <a
  href="https://github.com/chiranjib-swain"><code>@​chiranjib-swain</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-node/pull/1567">actions/setup-node#1567</a></li>
  </ul>
  <h3>Dependency update:</h3>
  <ul>
  <li>Upgrade <code>@​actions/cache</code> to 5.1.0, log cache write
  denied by <a
  href="https://github.com/jasongin"><code>@​jasongin</code></a> in <a
  href="https://redirect.github.com/actions/setup-node/pull/1569">actions/setup-node#1569</a></li>
  </ul>
  <h2>New Contributors</h2>
  <ul>
  <li><a
  href="https://github.com/chiranjib-swain"><code>@​chiranjib-swain</code></a>
  made their first contribution in <a
  href="https://redirect.github.com/actions/setup-node/pull/1536">actions/setup-node#1536</a></li>
  <li><a href="https://github.com/deiga"><code>@​deiga</code></a> made
  their first contribution in <a
  href="https://redirect.github.com/actions/setup-node/pull/1548">actions/setup-node#1548</a></li>
  <li><a href="https://github.com/jasongin"><code>@​jasongin</code></a>
  made their first contribution in <a
  href="https://redirect.github.com/actions/setup-node/pull/1569">actions/setup-node#1569</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/setup-node/compare/v6...v7.0.0">https://github.com/actions/setup-node/compare/v6...v7.0.0</a></p>
  <h2>v6.5.0</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>Update <code>@​actions/cache</code> to 5.1.0 and add security
  overrides for undici and fast-xml-parser by <a
  href="https://github.com/HarithaVattikuti"><code>@​HarithaVattikuti</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-node/pull/1579">actions/setup-node#1579</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/setup-node/compare/v6.4.0...v6.5.0">https://github.com/actions/setup-node/compare/v6.4.0...v6.5.0</a></p>
  <h2>v6.4.0</h2>
  <h2>What's Changed</h2>
  <h3>Dependency updates:</h3>
  <ul>
  <li>Upgrade <a
  href="https://github.com/actions"><code>@​actions</code></a>
  dependencies by <a
  href="https://github.com/Copilot"><code>@​Copilot</code></a> in <a
  href="https://redirect.github.com/actions/setup-node/pull/1525">actions/setup-node#1525</a></li>
  <li>Update Node.js versions in versions.yml and bump package to v6.4.0
  by <a
  href="https://github.com/priya-kinthali"><code>@​priya-kinthali</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-node/pull/1533">actions/setup-node#1533</a></li>
  </ul>
  <h2>New Contributors</h2>
  <ul>
  <li><a href="https://github.com/Copilot"><code>@​Copilot</code></a> made
  their first contribution in <a
  href="https://redirect.github.com/actions/setup-node/pull/1525">actions/setup-node#1525</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/setup-node/compare/v6...v6.4.0">https://github.com/actions/setup-node/compare/v6...v6.4.0</a></p>
  <h2>v6.3.0</h2>
  <h2>What's Changed</h2>
  <h3>Enhancements:</h3>
  <ul>
  <li>Support parsing <code>devEngines</code> field by <a
  href="https://github.com/susnux"><code>@​susnux</code></a> in <a
  href="https://redirect.github.com/actions/setup-node/pull/1283">actions/setup-node#1283</a></li>
  </ul>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/actions/setup-node/commit/949feb2413d6458794dcd2491c4babbbce0c15c1"><code>949feb2</code></a>
  Bump eslint from 10.5.0 to 10.11.0 and package.json from 7.0.0 to 7.1.0
  (<a
  href="https://redirect.github.com/actions/setup-node/issues/1635">#1635</a>)</li>
  <li><a
  href="https://github.com/actions/setup-node/commit/7207c4f1189c7ee83ddf20c7c68bd1af6fdd751c"><code>7207c4f</code></a>
  chore: update dependencies and add license files (<a
  href="https://redirect.github.com/actions/setup-node/issues/1642">#1642</a>)</li>
  <li><a
  href="https://github.com/actions/setup-node/commit/54337e0d02de9202d0948aaff36ad556375497b9"><code>54337e0</code></a>
  Bump eslint-plugin-n from 18.1.0 to 18.3.0 (<a
  href="https://redirect.github.com/actions/setup-node/issues/1638">#1638</a>)</li>
  <li><a
  href="https://github.com/actions/setup-node/commit/fb54fa8c14c63410cc752f0ae3b7b3ca6af161fc"><code>fb54fa8</code></a>
  Bump jest-each from 30.4.1 to 30.5.2 (<a
  href="https://redirect.github.com/actions/setup-node/issues/1636">#1636</a>)</li>
  <li><a
  href="https://github.com/actions/setup-node/commit/7f84f1d52d0c56afb45c62fee31a93b843cea4d6"><code>7f84f1d</code></a>
  Bump pnpm/action-setup from 6.0.10 to 6.1.0 (<a
  href="https://redirect.github.com/actions/setup-node/issues/1631">#1631</a>)</li>
  <li><a
  href="https://github.com/actions/setup-node/commit/0c54c7b07420dcba69a7d832587f55ebdf771c9e"><code>0c54c7b</code></a>
  Bump <code>@​typescript-eslint/parser</code> from 8.62.0 to 8.70.0 (<a
  href="https://redirect.github.com/actions/setup-node/issues/1615">#1615</a>)</li>
  <li><a
  href="https://github.com/actions/setup-node/commit/a8294190049a9c296f01b9a432aff41dd83d8284"><code>a829419</code></a>
  Bump <code>@​types/semver</code> from 7.7.1 to 7.8.0 (<a
  href="https://redirect.github.com/actions/setup-node/issues/1616">#1616</a>)</li>
  <li><a
  href="https://github.com/actions/setup-node/commit/680d1e489b82f1243de15bc7665c0007e3c9a860"><code>680d1e4</code></a>
  Support absolute paths in node-version-file (<a
  href="https://redirect.github.com/actions/setup-node/issues/1633">#1633</a>)</li>
  <li><a
  href="https://github.com/actions/setup-node/commit/db0fb3a98ed314b69026b6e8c9be26a7e2fe65a8"><code>db0fb3a</code></a>
  Bump baseline-browser-mapping from 2.10.38 to 2.11.22 (<a
  href="https://redirect.github.com/actions/setup-node/issues/1629">#1629</a>)</li>
  <li><a
  href="https://github.com/actions/setup-node/commit/94196ee1d15439c1b6651cd87ef14e88ec435966"><code>94196ee</code></a>
  Bump pnpm/action-setup from 6.0.9 to 6.0.10 (<a
  href="https://redirect.github.com/actions/setup-node/issues/1602">#1602</a>)</li>
  <li>Additional commits viewable in <a
  href="https://github.com/actions/setup-node/compare/v6...v7">compare
  view</a></li>
  </ul>
  </details>
  <br />

  Updates `actions/setup-python` from 6 to 7
  <details>
  <summary>Release notes</summary>
  <p><em>Sourced from <a
  href="https://github.com/actions/setup-python/releases">actions/setup-python's
  releases</a>.</em></p>
  <blockquote>
  <h2>v7.0.0</h2>
  <h2>What's Changed</h2>
  <h3>Enhancements</h3>
  <ul>
  <li>Migrate to ESM and upgrade dependencies by <a
  href="https://github.com/priyagupta108"><code>@​priyagupta108</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-python/pull/1330">actions/setup-python#1330</a></li>
  <li>Pin SHA commits and update docs with latest versions by <a
  href="https://github.com/HarithaVattikuti"><code>@​HarithaVattikuti</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-python/pull/1338">actions/setup-python#1338</a></li>
  <li>Remove the pip-install input by <a
  href="https://github.com/gowridurgad"><code>@​gowridurgad</code></a> in
  <a
  href="https://redirect.github.com/actions/setup-python/pull/1336">actions/setup-python#1336</a></li>
  </ul>
  <h3>Bug Fix</h3>
  <ul>
  <li>Fix to Classify stderr warning messages as warnings instead of
  errors in annotations by <a
  href="https://github.com/lmvysakh"><code>@​lmvysakh</code></a> in <a
  href="https://redirect.github.com/actions/setup-python/pull/1335">actions/setup-python#1335</a></li>
  <li>Validate and retry manifest fetch to prevent silent failures by <a
  href="https://github.com/priyagupta108"><code>@​priyagupta108</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-python/pull/1332">actions/setup-python#1332</a></li>
  </ul>
  <h3>Dependency Upgrade</h3>
  <ul>
  <li>Bump certifi from 2020.6.20 to 2024.7.4 in
  /<strong>tests</strong>/data by <a
  href="https://github.com/dependabot"><code>@​dependabot</code></a> in <a
  href="https://redirect.github.com/actions/setup-python/pull/1328">actions/setup-python#1328</a></li>
  <li>Remove EOL Python versions and Bumps numpy text fixture by <a
  href="https://github.com/priya-kinthali"><code>@​priya-kinthali</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-python/pull/1333">actions/setup-python#1333</a></li>
  <li>Upgrade <code>@​actions/cache</code> to 6.2.0 by <a
  href="https://github.com/philip-gai"><code>@​philip-gai</code></a> in <a
  href="https://redirect.github.com/actions/setup-python/pull/1337">actions/setup-python#1337</a></li>
  </ul>
  <h2>New Contributors</h2>
  <ul>
  <li><a href="https://github.com/lmvysakh"><code>@​lmvysakh</code></a>
  made their first contribution in <a
  href="https://redirect.github.com/actions/setup-python/pull/1335">actions/setup-python#1335</a></li>
  <li><a
  href="https://github.com/philip-gai"><code>@​philip-gai</code></a> made
  their first contribution in <a
  href="https://redirect.github.com/actions/setup-python/pull/1337">actions/setup-python#1337</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/setup-python/compare/v6...v7.0.0">https://github.com/actions/setup-python/compare/v6...v7.0.0</a></p>
  <h2>v6.3.0</h2>
  <h2>What's Changed</h2>
  <h3>Enhancement</h3>
  <ul>
  <li>Add RHEL support and include Linux distro in cache keys by <a
  href="https://github.com/priyagupta108"><code>@​priyagupta108</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-python/pull/1323">actions/setup-python#1323</a></li>
  <li>Fix pip cache error handling on Windows by <a
  href="https://github.com/priyagupta108"><code>@​priyagupta108</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-python/pull/1040">actions/setup-python#1040</a></li>
  </ul>
  <h3>Dependency update</h3>
  <ul>
  <li>Upgrade minimatch from 3.1.2 to 3.1.5 by <a
  href="https://github.com/dependabot"><code>@​dependabot</code></a> in <a
  href="https://redirect.github.com/actions/setup-python/pull/1281">actions/setup-python#1281</a></li>
  <li>Upgrade actions dependencies by <a
  href="https://github.com/gowridurgad"><code>@​gowridurgad</code></a>
  with <a href="https://github.com/Copilot"><code>@​Copilot</code></a> in
  <a
  href="https://redirect.github.com/actions/setup-python/pull/1303">actions/setup-python#1303</a></li>
  <li>Upgrade <code>@​actions/cache</code> to 5.1.0, log cache write
  denied by <a
  href="https://github.com/jasongin"><code>@​jasongin</code></a> in <a
  href="https://redirect.github.com/actions/setup-python/pull/1324">actions/setup-python#1324</a></li>
  <li>Upgrade dependency versions and test workflow configuration by <a
  href="https://github.com/HarithaVattikuti"><code>@​HarithaVattikuti</code></a>
  in <a
  href="https://redirect.github.com/actions/setup-python/pull/1322">actions/setup-python#1322</a></li>
  </ul>
  <h3>Documentation</h3>
  <ul>
  <li>Update advanced-usage.md by <a
  href="https://github.com/Dunky-Z"><code>@​Dunky-Z</code></a> in <a
  href="https://redirect.github.com/actions/setup-python/pull/811">actions/setup-python#811</a></li>
  </ul>
  <h2>New Contributors</h2>
  <ul>
  <li><a
  href="https://github.com/gowridurgad"><code>@​gowridurgad</code></a>
  with <a href="https://github.com/Copilot"><code>@​Copilot</code></a>
  made their first contribution in <a
  href="https://redirect.github.com/actions/setup-python/pull/1303">actions/setup-python#1303</a></li>
  <li><a href="https://github.com/jasongin"><code>@​jasongin</code></a>
  made their first contribution in <a
  href="https://redirect.github.com/actions/setup-python/pull/1324">actions/setup-python#1324</a></li>
  <li><a href="https://github.com/Dunky-Z"><code>@​Dunky-Z</code></a> made
  their first contribution in <a
  href="https://redirect.github.com/actions/setup-python/pull/811">actions/setup-python#811</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/setup-python/compare/v6.2.0...v6.3.0">https://github.com/actions/setup-python/compare/v6.2.0...v6.3.0</a></p>
  <h2>v6.2.0</h2>
  <h2>What's Changed</h2>
  <h3>Dependency Upgrades</h3>
  <ul>
  <li>Upgrade dependencies to Node 24 compatible versions by <a
  href="https://github.com/salmanmkc"><code>@​salmanmkc</code></a> in <a
  href="https://redirect.github.com/actions/setup-python/pull/1259">actions/setup-python#1259</a></li>
  </ul>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/actions/setup-python/commit/5fda3b95a4ea91299a34e894583c3862153e4b97"><code>5fda3b9</code></a>
  Pin SHA commits and update docs with latest versions (<a
  href="https://redirect.github.com/actions/setup-python/issues/1338">#1338</a>)</li>
  <li><a
  href="https://github.com/actions/setup-python/commit/4ab7e95f05e168b4356aebde89dd84f59c283d8e"><code>4ab7e95</code></a>
  Merge pull request <a
  href="https://redirect.github.com/actions/setup-python/issues/1337">#1337</a>
  from actions/philip-gai/bump-actions-cache-6-2-0</li>
  <li><a
  href="https://github.com/actions/setup-python/commit/0f3a009f475dbea83c0371cd85d099690fee8c5c"><code>0f3a009</code></a>
  Remove the pip-install input (<a
  href="https://redirect.github.com/actions/setup-python/issues/1336">#1336</a>)</li>
  <li><a
  href="https://github.com/actions/setup-python/commit/f8cf4291c8b8e273ddd26e569454615c7315d932"><code>f8cf429</code></a>
  Migrate to ESM and upgrade dependencies (<a
  href="https://redirect.github.com/actions/setup-python/issues/1330">#1330</a>)</li>
  <li><a
  href="https://github.com/actions/setup-python/commit/54baeea5b34417d10a7479663a23cca53ea209b5"><code>54baeea</code></a>
  Validate and retry manifest fetch to prevent silent failures (<a
  href="https://redirect.github.com/actions/setup-python/issues/1332">#1332</a>)</li>
  <li><a
  href="https://github.com/actions/setup-python/commit/c7092773a316760f4ecfe498e4af668a4dafeac5"><code>c709277</code></a>
  Annotation code fix (<a
  href="https://redirect.github.com/actions/setup-python/issues/1335">#1335</a>)</li>
  <li><a
  href="https://github.com/actions/setup-python/commit/6849080452e69b330395e8a6d23cf90f56d76a1a"><code>6849080</code></a>
  remove EOL Python versions and Bumps numpy text fixture (<a
  href="https://redirect.github.com/actions/setup-python/issues/1333">#1333</a>)</li>
  <li><a
  href="https://github.com/actions/setup-python/commit/0903b469fbf4441aadfe4f4b249dc5b1fba3a73e"><code>0903b46</code></a>
  Bump certifi from 2020.6.20 to 2024.7.4 in /<strong>tests</strong>/data
  (<a
  href="https://redirect.github.com/actions/setup-python/issues/1328">#1328</a>)</li>
  <li>See full diff in <a
  href="https://github.com/actions/setup-python/compare/v6...v7">compare
  view</a></li>
  </ul>
  </details>
  <br />

  ---------


## [0.43.0](https://github.com/omarmhaimdat/pepe/compare/v0.42.2...v0.43.0) - 2026-10-16

### Added

- *(agents)* --otlp says which operation inside the service the time went to, and the loop to make it faster ([#189](https://github.com/omarmhaimdat/pepe/pull/189))

  ## Why

  Every load tool, pepe included, stops at "p99 is 480 ms". The next
  question is always which part of the code that is, and answering it
  means leaving the tool. This PR answers it in the same command, and
  gives an agent a loop it can close on its own: measure, find the slow
  operation, change the code, measure again, compare.

  ## What it does

  **`--otlp :4318`** (plain runs for now)
  - Each request carries a `traceparent` of pepe's making. The run id is
  the trace id's high half and the request number, mixed, is the low half.
  The sampled flag is set, so a parent-based sampler keeps every request.
  - pepe runs an OTLP/HTTP receiver at `/v1/traces` that takes protobuf or
  JSON. The protobuf reader is about 150 lines, hand-written, so there's
  no new dependency. It keeps only spans from its own traces.
  - `/v1/metrics` and `/v1/logs` get a 200 and are ignored. Gzip gets a
  415, and the verdict says to set `OTEL_EXPORTER_OTLP_COMPRESSION=none`.
  - When the run ends, pepe waits for the exporters' last batch. It stops
  early once every request that got a response has its root span, and
  gives up after 15 s at most.
  - `spans` in the report: coverage, the server's own time (median, p90,
  p99), and `operations[]`. Each operation has calls per request, median
  and p99 call time, `share_percent` (its own time, less its children's,
  over all requests) and `slowest_share_percent` (the same over requests
  at or above the server's p99).
  - `summary.slowest_requests[]` gains `trace_id`, `server_ms` and `spent`
  (the top 3 operations).
  - Verdict findings: where the time went; an operation that's small at
  the median but large in the tail (it waits on a lock, a pool or a
  queue); the gap between pepe's latency and the server's own time; and
  what to set when no spans arrive.
  - The text report (headless, or after the dashboard closes) prints a
  "Where the server's time went" table.

  **`pepe compare`**: two reports with spans give `operations[]` (each
  operation before and after) and a finding for each one that moved. The
  verdict still comes from pepe's own measurements.

  **MCP**
  - `run` takes `otlp`.
  - `run`, `ping`, `ramp`, `api`, `flow` and `replay` take `save`, so an
  agent has a file to pass to `compare`.
  - A `speed-up` prompt (`/pepe:speed-up URL [goal]` in Claude Code) walks
  the loop.
  - Every tool has annotations: `compare` is read-only and the others
  reach outside the machine.
  - The server instructions now lead with the tasks pepe is for. They load
  at session start, while tool definitions may only be looked up later.

  **Discoverability**
  - The skill's description puts its trigger words first ("slow",
  "faster", "benchmark", "OpenTelemetry"), since that text is what Claude
  and Codex match on. It also has a section on the loop.
  - Codex: a root `plugin.json` and `mcp.json` (the portable format
  OpenAI's docs describe), plus `.agents/plugins/marketplace.json`. Codex
  also reads `.claude-plugin/marketplace.json` as a legacy marketplace.
  - The docs now cover `codex mcp add`, `~/.codex/config.toml`, a Cursor
  install link, and `npx skills add omarmhaimdat/pepe`.
  - `AGENTS.md`, `llms.txt` and the Claude plugin manifests mention the
  loop.
  - This PR includes the marketplace `source` fix from
  omarmhaimdat/pepe#188 (same lines), so it works whichever merges first.

  ## Tested

  - **End to end against a real service**: a Python service instrumented
  with the official OpenTelemetry SDK (1.45.1, http/protobuf exporter).
  Its `/checkout` makes a cache read and a query that holds a lock.
  - 2,000 requests at 16 concurrent: 100% coverage. `SELECT orders` was
  98% of the slowest requests' server time, and the slowest requests'
  `spent` agreed.
  - With the lock removed: `pepe compare` said "SELECT orders down 95%:
  47.74ms → 2.57ms a call". It also showed the bottleneck *moving* to the
  cache read, which is real: at 1.6k req/s the Python service is
  GIL-bound.
  - The same loop through `pepe mcp` (guardrails `--allow-host 127.0.0.1
  --max-requests 5000`, then `run` with `otlp` and `save`).
  - JSON export accepted, gzip refused with the message, metrics and logs
  answered.
  - **Unit tests**: the breakdown (shares, tail, self time with
  overlapping children, warm-up excluded), the protobuf reader, requests
  framed by length and by chunks, compare by operation, MCP
  prompt/`save`/annotations, schema fit. `cargo +1.98 test`: 367 passed.
  `clippy -D warnings` passes on 1.98.
  - **Performance**: `bench/compare.sh master branch 3`, with `--otlp`
  off. Results below; no regression.

  | Workload | CPU ms/1k, master | branch | Peak MB, master | branch |
  |---|---|---|---|---|
  | tiny-c64 | 5.55 | 5.45 | 10.1 | 10.2 |
  | body16k-c64 | 6.40 | 6.40 | 10.0 | 10.0 |
  | slow20ms-c1000 | 6.80 | 7.00 | 13.1 | 13.0 |
  | tls-c64 | 5.90 | 5.90 | 13.6 | 13.2 |

  ## Not verified
  - **Codex**: Codex isn't installed here, so the plugin files follow
  OpenAI's documentation but haven't been loaded by Codex. The MCP and
  AGENTS.md routes are standard.
  - **Cursor**: the install link hasn't been clicked.

  ## For 1.0
  This adds a flag (`--otlp`), report fields (`spans`, and the new
  `slowest_requests` fields), a compare field (`operations`), MCP inputs
  (`otlp`, `save`) and a prompt. All of them are additions, and `schema/`
  is updated, but per ROADMAP.md they restart the month-without-changes
  clock.


## [0.42.2](https://github.com/omarmhaimdat/pepe/compare/v0.42.1...v0.42.2) - 2026-10-16

### Fixed

- *(agents)* the plugin marketplace is one Claude Code takes ([#188](https://github.com/omarmhaimdat/pepe/pull/188))

  ## What was wrong

  The install line in the docs fails on master:

  ```
  $ claude plugin marketplace add omarmhaimdat/pepe
  ✘ Failed to add marketplace: … marketplace.json: Invalid schema: … plugins.0.source: Invalid input
  ```

  Claude Code wants a plugin's `source` to be a path that starts with
  `./`, and ours was `"."`. Since the marketplace can't be added, `claude
  plugin install pepe@pepe` fails too. I checked it with Claude Code
  2.1.63, adding a checkout of master as a marketplace under a throwaway
  `HOME`.

  ## The fix

  - `"source": "./"`. `claude plugin validate .` passes now, and the same
  throwaway-`HOME` test adds the marketplace and installs `pepe@pepe`.
  - `metadata.description` on the marketplace, which the validator asks
  for.
  - A test (`the_plugin_marketplace_is_one_claude_code_takes`) that holds
  every plugin source to `./`, so this can't come back unnoticed.

  It's separate from the agent-loop PR so it can ship on its own as a
  patch release.


## [0.42.1](https://github.com/omarmhaimdat/pepe/compare/v0.42.0...v0.42.1) - 2026-10-15

### Fixed

- *(deps)* bump toml from 0.9.6 to 1.1.8+spec-1.1.0 ([#183](https://github.com/omarmhaimdat/pepe/pull/183))

  Bumps [toml](https://github.com/toml-rs/toml) from 0.9.6 to
  1.1.8+spec-1.1.0.
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/toml-rs/toml/commit/ceac6e4412420d5a5c19e4f7293ad005cf1a31b2"><code>ceac6e4</code></a>
  chore: Release</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/0fe9e93d6d6299400e168681f7cdd7515c0693d2"><code>0fe9e93</code></a>
  docs: Update changelog</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/e3a64aa201ec339abacb570417d75b327490cab4"><code>e3a64aa</code></a>
  fix(display): Ensure all keys are encoded (<a
  href="https://redirect.github.com/toml-rs/toml/issues/1245">#1245</a>)</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/76484d005a0e68e8c1876f8a3417b71a3921623f"><code>76484d0</code></a>
  fix(display): Ensure all keys are encoded</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/711b1ba1ecb11e61df9a1609e24fa3b17d11d961"><code>711b1ba</code></a>
  refactor(display): Clarify that we are working with encoded keys</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/4f73ebdc8bd7fdf0e6bd379baeb96dedcee6bb2b"><code>4f73ebd</code></a>
  test(display): Reproduce the key bug</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/6c6a5cdcdc4d9beb16ff9bdf2dcbf360fc2bc84e"><code>6c6a5cd</code></a>
  fix(parser): Remove risk of unsoundness with synthetic parser events and
  'uns...</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/9c5e95d8d67514f2191afbee137ba40e85e4f441"><code>9c5e95d</code></a>
  fix(parser): Remove risk of unsoundness with synthetic parser events and
  'uns...</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/4d45944be2f6ee3b938dbba3d64a6d9c280bd9e9"><code>4d45944</code></a>
  perf(parser): Remove no-op code (<a
  href="https://redirect.github.com/toml-rs/toml/issues/1243">#1243</a>)</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/189c91c3d06a0f56e18f85cb00d3425534defd71"><code>189c91c</code></a>
  perf(parser): Remove no-op code</li>
  <li>Additional commits viewable in <a
  href="https://github.com/toml-rs/toml/compare/toml-v0.9.6...toml-v1.1.8">compare
  view</a></li>
  </ul>
  </details>
  <br />


  [![Dependabot compatibility
  score](https://dependabot-badges.githubapp.com/badges/compatibility_score?dependency-name=toml&package-manager=cargo&previous-version=0.9.6&new-version=1.1.8+spec-1.1.0)](https://docs.github.com/en/github/managing-security-vulnerabilities/about-dependabot-security-updates#about-compatibility-scores)

  Dependabot will resolve any conflicts with this PR as long as you don't
  alter it yourself. You can also trigger a rebase manually by commenting
  `@dependabot rebase`.


## [0.42.0](https://github.com/omarmhaimdat/pepe/compare/v0.41.0...v0.42.0) - 2026-10-13

### Added

- *(ui)* themes, and a settings page on every screen that keeps this machine's defaults ([#181](https://github.com/omarmhaimdat/pepe/pull/181))

  ## What

  **Themes.** Seven true colour themes beside `terminal`, `light` and
  `none`: `pepe`, `nord`, `dracula`, `gruvbox`, `tokyo-night`,
  `catppuccin`, and `paper` for a light ground. Each is a dozen colours in
  `src/ui/theme.rs`; the rest of its palette is mixed from those, so the
  screens still draw in one dark palette and `adjust` repaints the frame.

  **A settings page on every screen.** `F2` opens it over the setup, saved
  tests, history, API plan, dashboard, ramp, ping and logs screens, and
  each screen's `?` help lists it.

  - The first line is the theme: `←` `→` step through them and the screen
  repaints at once.
  - Below it: the defaults a run starts from (requests, duration,
  concurrency, timeout, threads, rate, warm-up, user agent, proxy,
  keep-alive, compression, redirects, TLS verification), a ramp's steps,
  the guardrails, and whether pepe looks for new releases.
  - `enter` types a value, which is checked; `del` goes back to pepe's
  own; `esc` closes. Every change is saved as it's made.

  **Kept on the host.** `~/.config/pepe/settings.toml`
  (`$XDG_CONFIG_HOME/pepe`, `%APPDATA%\pepe` on Windows, or the directory
  `PEPE_CONFIG_DIR` names):

  ```toml
  theme = "nord"
  update-check = false

  [defaults]             # what a pepe.toml can say, for every run on this machine
  concurrency = 50
  max-rate = 500
  ```

  ## Why

  The theme could only be picked with `PEPE_THEME`, between pepe's palette
  and the terminal's, and a default could only be changed per directory in
  a `pepe.toml`.

  ## For the reviewer

  - **Precedence:** flags, then `pepe.toml`, then the settings'
  `[defaults]`, then pepe's own. The defaults are read when pepe starts,
  so only the theme changes mid-session.
  - **Caps:** where a `pepe.toml` and the settings both set
  `max-requests`, `max-rate` or `max-concurrency`, the lower holds, so a
  file in a directory can't loosen a machine-wide cap. This is a choice,
  and the place to push back if the project file should simply win.
  - **Every mode:** the saved defaults apply with `--json` and under `pepe
  mcp` too, as a `pepe.toml` does.
  - **A settings file that doesn't parse** stops pepe with exit 1 and
  names the file, rather than dropping a cap it holds.
  - **`PEPE_THEME`** still wins over the saved theme at startup (the page
  says so when it's set); `NO_COLOR` wins over everything. A theme name
  this pepe doesn't know is not an error.
  - **`update-check = false`** also stops the setup screen mentioning a
  cached newer release.
  - Where the page covers the mascot, the page shows: the kept pixel-art
  area is cut around it.
  - A saved test (`--load`, `pepe rerun`) sits over the defaults like a
  `pepe.toml`; its mode is read from the test alone, so a `[ramp]` in the
  settings doesn't make every test a ramp.
  - While the page is up, the mouse is no screen's.
  - No `--theme` flag and no `pepe settings` subcommand.

  ## Checked

  - After merging master: `cargo test` (357 pass), `cargo clippy
  --all-targets -- -D warnings`, `cargo fmt --check`.
  - `UPDATE_CONTRIB=1 cargo test contrib` for the man page's environment
  section.
  - `python3 site/build-docs.py` **not run**: `site/` had unrelated
  uncommitted changes locally, so the built pages are not in this PR.
  - By hand, in tmux with `PEPE_CONFIG_DIR` pointed at a scratch
  directory: opened the page from the setup screen and from a finished
  dashboard (40 requests at a local `python3 -m http.server`), stepped the
  themes and read the escape codes for the new palette, typed and cleared
  defaults, read the file back, and confirmed with `--dry-run` that a
  saved `concurrency` reaches a run and that `-c` wins over it.
  - The palettes are held to contrast floors by a test; the shades
  themselves were not judged by eye in a terminal.


## [0.41.0](https://github.com/omarmhaimdat/pepe/compare/v0.40.0...v0.41.0) - 2026-10-13

### Added

- *(ping)* a ping is a saved test too: --keep, --load, --record, ctrl-s and r on its screen ([#179](https://github.com/omarmhaimdat/pepe/pull/179))

  ## What

  A ping is a saved test like the others.

  - The saved-test file gains a `[ping]` section: targets, `every`,
  `name`, `slo`, `tcp`, `port`, `cmd`, `keep-alive`, `http1`, `ipv4`,
  `ipv6`, `all-ips`, `interface`. `pepe ping … --keep NAME` writes one;
  `pepe --load NAME` runs it again, the file deciding the mode, and a ping
  ends on the file's `-n` where it would otherwise run forever.

    ```
  $ pepe ping https://api.example.com/health -n 30 --every 2s --slo
  total=500 --keep prod-health --record --json
    saved .pepe/tests/prod-health.toml · `pepe --load prod-health` runs it
  recorded .pepe/runs/prod-health/2026-10-12T23-29-20Z.json · pepe history
  prod-health
    ```

  - `--record` keeps the ping's report under the test, and `pepe rerun`,
  `pepe history NAME` and `pepe compare NAME` work on pings, phase by
  phase, as `pepe compare` already knew how to read them.
  - On the ping screen, `ctrl-s` saves the test (offering the first target
  as the name) and `r` records the pings so far. The footer says where
  they went.
  - The library lists a ping with its targets, interval, count or "until
  stopped", and SLO. A report's `command` renders a ping as a `pepe ping
  …` line that parses back, several targets included.
  - A report recorded from a screen right after the test was saved there
  now names the test explicitly, since the settings were not loaded from
  the file. The dashboard and the ramp had the same gap.

  The ping and load-test pages document it.

  ## Why

  A ping is the test most worth keeping over time: "prod health, every two
  seconds, total under 500ms" is what you'd run again next month and hold
  against today's phases. With v0.40's recorded runs in place, the pieces
  fit the existing design without new concepts.

  ## Checked

  - `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt
  --check`
  - `UPDATE_CONTRIB=1 cargo test contrib` and `python3 site/build-docs.py`
  - Against a local HTTP server: `pepe ping … --keep --record --json`,
  `--load` running the ping and ending on its count, `--record` twice,
  `pepe rerun`, `pepe history NAME`, `pepe compare NAME`, the recorded
  report's `test` and `command`, and `--load … --dry-run` showing the ping
  as kept.


## [0.40.0](https://github.com/omarmhaimdat/pepe/compare/v0.39.1...v0.40.0) - 2026-10-12

### Added

- *(store)* recorded runs: --record, w on the dashboard, pepe history, pepe rerun, pepe compare NAME ([#177](https://github.com/omarmhaimdat/pepe/pull/177))

  ## What

  A saved test keeps its results too, when asked, and pepe can read them
  back.

  - `--record` on a run of a saved test writes the report when it ends, as
  `.pepe/runs/NAME/<moment>.json`, in every path: `--json`, headless, the
  dashboard and the ramp screen. It refuses a run that is not a saved test
  (`--load NAME`, or `--keep NAME`), and modes other than a run or a ramp.
  - `w` on the dashboard and the ramp screen records the run once it has
  ended, with a notice saying where.
  - Every report carries `test` and `command`, the command with header
  secrets masked, so a recorded run can be run again from the file alone.
  Both JSON schemas gained the two fields.

    ```
  $ pepe -n 1000 -c 10 https://api.example.com/health --keep api-health
  --record --json
    saved .pepe/tests/api-health.toml · `pepe --load api-health` runs it
  recorded .pepe/runs/api-health/2026-10-12T22-55-04Z.json · pepe history
  api-health
    ```

  - `pepe history` lists the saved tests with their run counts and last
  run; `pepe history NAME` lists a test's runs, each against the one
  before. Both take `--json`.
  - `pepe rerun NAME` is `--load NAME --record` followed by the comparison
  with the previous run; with `--json` the comparison is what gets
  printed, the run's own report being on disk; `--gate` exits 1 on a
  regression.
  - `pepe compare NAME` holds a test's last two recorded runs against each
  other. Two runs recorded in the same second are told apart and kept in
  order.
  - In the library, each test shows its last run (rps, p99, failures, age)
  when the terminal is wide enough, and `h` opens its history: the runs
  newest first, with a card saying what moved since the run before. Mouse
  and wheel work there as everywhere.
  - The plan screen's parameters pane takes clicks: one picks a parameter,
  a second edits it.

  The reference, completions and man pages know the two commands and the
  flag; the load-test, compare, dashboard and agents pages document them.

  ## Why

  v0.37 to v0.39 gave pepe saved tests, a library to pick from, and a way
  to keep any run. The payoff of the store is results next to the test
  they came from: a trend over time instead of two files on a desk, and
  "did my change regress" as one command that CI can gate on. The report
  carrying its own invocation means no result is ever an orphan.

  ## Checked

  - `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt
  --check`
  - `UPDATE_CONTRIB=1 cargo test contrib` and `python3 site/build-docs.py`
  - Against a local HTTP server: `--keep … --record --json` (report
  carries `test` and the masked command), a headless `--load … --record`,
  `pepe history` in both forms and as JSON, `pepe compare NAME`, `pepe
  rerun NAME` as text and as JSON, the refusals for an unsaved run and an
  unknown name, and two runs in one second kept in order.


## [0.39.1](https://github.com/omarmhaimdat/pepe/compare/v0.39.0...v0.39.1) - 2026-10-12

### Fixed

- *(dashboard)* the filter bar over the requests is clickable, part by part ([#174](https://github.com/omarmhaimdat/pepe/pull/174))

  ## What

  The requests tab's filter bar, the line with STATUS, LATENCY and SEARCH
  over the table, takes the mouse like the rest of the dashboard:

  - a group's name or its key letter (`f`, `l`, `/`) presses that key, so
  STATUS cycles the status filter and SEARCH opens the search for typing;
  - a status or a latency choice sets that filter outright, rather than
  cycling to it;
  - the ENDPOINT chip, when an endpoint filter is on, lets it go.

  ## Why

  v0.38.0 made every screen clickable through regions registered as they
  are drawn. The filter bar drew its hints and choices as plain text and
  registered nothing, so the three controls a user reaches for most on
  that tab were the ones that took no click.

  ## Checked

  - `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt
  --check`
  - `python3 site/build-docs.py`
  - A test renders the requests tab, clicks each part and checks the
  filter it sets, then checks the regions are gone on another tab.


## [0.39.0](https://github.com/omarmhaimdat/pepe/compare/v0.38.0...v0.39.0) - 2026-10-12

### Added

- *(setup)* any run is kept, updated or copied: --keep, ctrl-s on the dashboard and ramp, c in the library ([#173](https://github.com/omarmhaimdat/pepe/pull/173))

  ## What

  Any run can be kept as a saved test, updated in place, or copied: from
  the command line and from every screen that runs one.

  - `pepe -n 1000 -c 10 https://example.com --keep NAME` writes the
  settings as given to `.pepe/tests/NAME.toml`, says so on stderr, then
  runs. With `--load` and no name, `--keep` updates the loaded test in
  place, so `pepe --load api-health -c 20 --keep` keeps the new
  concurrency; with another name it copies it. A curl command is kept too,
  body included. Ping, flow, replay and the rest refuse with exit code 2,
  since a saved test is a load test.

    ```
    $ pepe -n 1000 -c 10 https://api.example.com/health --keep api-health
    saved .pepe/tests/api-health.toml · `pepe --load api-health` runs it
    ```

  - `ctrl-s` on the dashboard and the ramp screen saves the run's settings
  the same way, with the concurrency as dialed in with `+` and `-`. A run
  started from a saved test offers its own name, so enter updates it
  without asking; another name makes a copy; a taken name asks once. The
  result shows as a notice in the footer.
  - `c` in the library copies the selected test under another name,
  offering `NAME-copy`, and selects the new file.
  - One "save as" prompt now serves the setup form, the dashboard, the
  ramp and the library.

  The flag is `--keep` rather than `--save` because `pepe ping --save
  FILE` already writes a report and a global flag reaches every
  subcommand, so `--save` stays the word for reports. The reference,
  completions and man page know `--keep`; the load-test, dashboard and
  agents pages document it.

  ## Why

  v0.37.0 saved a test from the setup form and v0.38.0 gave the saved
  tests a library. A run worked out on the command line, or dialed in on
  the dashboard, deserves keeping just as much, and a saved test has to be
  updatable and copyable without going through the form.

  ## Checked

  - `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt
  --check`
  - `UPDATE_CONTRIB=1 cargo test contrib` and `python3 site/build-docs.py`
  - By hand from a scratch directory: `--keep` of a run, an update in
  place with `--load … --keep`, a copy under another name, a ramp kept as
  a ramp, a curl command kept with its body, and the refusals for a
  missing name, nothing to keep, and a ping; `pepe ping --save` still
  takes its file.


## [0.38.0](https://github.com/omarmhaimdat/pepe/compare/v0.37.0...v0.38.0) - 2026-10-12

### Added

- *(setup)* a library of the saved tests to pick from, and the mouse on every screen ([#171](https://github.com/omarmhaimdat/pepe/pull/171))

  ## What

  The second step of the saved tests that v0.37.0 began.

  - With saved tests in the project, `pepe` alone opens on the library:
  each test with its mode, target, load and age, and the command it runs
  as. Enter puts one in the setup form, where it can still be changed
  before the start; `n` starts from an empty form; `ctrl-o` on the form
  goes back to the library. A file that doesn't read is listed with its
  error rather than hidden.
  - A saved test's file decides the mode when the command line names none,
  so `pepe --load my-ramp` runs the ramp and the library opens a ramp into
  the ramp form. A typed mode still wins.
  - Every screen takes the mouse: a click on a row selects it and a second
  click opens it, a tab shows it, a key chip in the footer presses its
  key, and the wheel scrolls. Screens register what they draw as clickable
  regions and look a click up against them, so what is clickable is
  exactly what is on screen. Mouse capture is on for the whole session and
  off again on every way out. The keyboard stays the primary path; with
  the mouse captured, selecting text in the terminal needs shift held.

  The load-test and dashboard pages document the library and the mouse.

  ## Why

  Saving tests under names is only half of it: there has to be a place to
  see them and pick one, and a list to pick from has to be clickable. A
  mouse that works on one screen only is a surprise, so it works on all of
  them, through one shared mechanism rather than seven.

  ## Checked

  - `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt
  --check`
  - `python3 site/build-docs.py` (no flag changed, so the completions and
  man page are as they were)
  - By hand from a scratch directory with four saved tests, one of them
  broken: the library at 80 and 170 columns, clicks and a double click on
  its rows, and `--load` of a saved ramp running as a ramp with `--dry-run
  --json`.



### Other

- *(nix)* update flake.lock ([#170](https://github.com/omarmhaimdat/pepe/pull/170))

  Automated changes by the
  [update-flake-lock](https://github.com/DeterminateSystems/update-flake-lock)
  GitHub Action.

  ```
  Flake lock file updates:

  • Updated input 'nixpkgs':
      'github:NixOS/nixpkgs/a7868a727837f3c09cee2ce0ca671c76b1589fed?narHash=sha256-KgItSKML8Xvte0B7/uGnBDsYzSnnKHcOaiUbgWBXLXw%3D' (2026-10-03)
    → 'github:NixOS/nixpkgs/6510408d147d8c5e6d76043e899e9251756f6a75?narHash=sha256-XK4KceonkwD8LuMAt/vNrz982mXJM5jeHAxMwsUXXAQ%3D' (2026-10-10)
  ```

  ### Running GitHub Actions on this PR

  GitHub Actions will not run workflows on pull requests which are opened
  by a GitHub Action.

  **To run GitHub Actions workflows on this PR, close and re-open this
  pull request.**


## [0.37.0](https://github.com/omarmhaimdat/pepe/compare/v0.36.1...v0.37.0) - 2026-10-12

### Added

- *(setup)* ctrl-s saves the form as a named test, run again with --load ([#168](https://github.com/omarmhaimdat/pepe/pull/168))

  ## What

  A project keeps more than one load test. `ctrl-s` on the setup screen
  now asks for a name and writes the form as `.pepe/tests/NAME.toml`, the
  same format as `pepe.toml`, meant to be committed with the code. The
  name it offers comes from the URL (`api.example.com-v1-health`); a taken
  name asks once before it is replaced, and a bad name is explained in the
  same line.

  `pepe --load NAME` runs a saved test, flags on the command line winning
  as usual. It conflicts with `--config`, and a missing name says what is
  saved:

  ```
  $ pepe --load nope
  error: no test named `nope` (saved: api-health, checkout)
  ```

  The one-line text editor the plan screen used moves into the ui kit, so
  both screens share it. The reference, the completions and the man page
  know the new flag; the load-test page has a "Saved tests" section.

  ## Why

  A test worked out on the setup screen had one place to go, `pepe.toml`,
  so a second one overwrote the first and reports piled up as nameless
  files. This is the first step of a project-local store under `.pepe/`:
  named tests now, a library screen in front of the form and results kept
  per test next.

  ## Checked

  - `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt
  --check`
  - `UPDATE_CONTRIB=1 cargo test contrib` and `python3 site/build-docs.py`
  - By hand from a scratch directory: `--load` with `--dry-run --json`, a
  flag overriding the file, the `--config` conflict (exit 2), a bad name
  and the missing-name message with and without saved tests; the setup
  screen at 80 columns with the prompt open.



### Other

- *(agents)* the skill, llms.txt and the install page know the MCP server ([#167](https://github.com/omarmhaimdat/pepe/pull/167))

  ## What changed

  AGENTS.md and the agents page already described `pepe mcp`, the MCP
  registry listing and the Claude Code plugin; the skill, `llms.txt` and
  the install page did not. Each now says what the others say.

  - `skills/pepe/SKILL.md`: an "As an MCP server" section with the seven
  tools, the `claude mcp add` line, the two ways the registry starts it
  (`uvx pepe-http mcp`, the `ghcr.io/omarmhaimdat/pepe` image) and the
  plugin's caps. The install block gains `uvx` and Docker.
  - `llms.txt`: one paragraph on the MCP server, the registry name
  `io.github.omarmhaimdat/pepe` and the plugin; the install line gains
  `uvx pepe-http`.
  - `docs/install.md`: `uvx pepe-http` under PyPI, with a note that the
  package carries a `pepe-http` command for it, and a pointer from the
  Docker section to the registry.
  - `site/docs/install.html` and `site/docs/search.json`: regenerated with
  `site/build-docs.py`; `--check` passes.

  ## Why

  A model or an agent reads whichever of these it reaches first. The MCP
  server and the registry listing (#162, #165) were only in two of the
  five.

  ## For the reviewer

  Every fact was checked against the code: the tool names in `src/mcp.rs`,
  the `pepe-http` script in `packaging/pypi/pyproject.toml`, the image's
  entrypoint. No code changes.


## [0.36.1](https://github.com/omarmhaimdat/pepe/compare/v0.36.0...v0.36.1) - 2026-10-11

### Fixed

- *(agents)* the registry listing names the image by its canonical reference alone ([#165](https://github.com/omarmhaimdat/pepe/pull/165))

  The v0.36.0 release's `publish-mcp-registry` job was refused by the
  registry: an OCI package must not carry `registryBaseUrl`, since the
  identifier (`ghcr.io/omarmhaimdat/pepe:0.36.0`) already says where the
  image is. The field is removed from the OCI entry; the PyPI entry keeps
  its, which the registry wants. `server.json` still validates against the
  registry's schema. Everything else in that release shipped.

  A `fix`, so the release it brings makes the first registry publish.



### Other

- *(smoke)* the npm and PyPI packages, tested after each release like the installers ([#163](https://github.com/omarmhaimdat/pepe/pull/163))

  pepe is on npm (`pepe-http`, cargo-dist's package) and on PyPI
  (`pepe-http`, the wrapper in `packaging/pypi`). The post-release smoke
  workflow covered the shell and PowerShell installers and the Homebrew
  formula; this adds the two packages, so a release that publishes but
  doesn't install or run shows up within minutes, like the others.

  ## What the jobs do

  Both run on ubuntu, macos and windows, after the `expected` job, so
  `workflow_dispatch` with a version still works for them.

  **npm** (`actions/setup-node@v6`, node 22)
  - `npx -y pepe-http@$EXPECTED --version` must print the expected
  version. The registry can take a minute to serve a just-published
  version, so the call is retried up to 10 times, 30s apart.
  - `npx -y pepe-http@$EXPECTED self-update --check` must exit 0 and say
  "installed with npm" (the update path from #160).
  - On ubuntu only: `npm install -g pepe-http@$EXPECTED`, then `pepe
  --version`, to cover the global install and the `pepe` command.

  **PyPI** (`actions/setup-python@v6`, python 3.x)
  - `pipx install pepe-http==$EXPECTED`, with the same retry for PyPI's
  propagation delay; pipx is installed with pip first if the runner lacks
  it, and pipx's bin directory is added to `GITHUB_PATH` in case a runner
  doesn't have it on PATH.
  - `pepe --version` must print the expected version (the first run
  downloads and checks the release binary), and `pepe self-update --check`
  must exit 0 and say "installed with pip".
  - `pipx run --spec pepe-http==$EXPECTED pepe --json --dry-run -n 10
  https://example.com` must report `sent: 0` with that target. `--spec`
  because the package is `pepe-http` and the command is `pepe`; a plain
  `pipx run pepe-http==X` looks for a `pepe-http` app and fails.

  Windows steps use `shell: bash`, as the workflow does elsewhere.

  ## Verified

  The workflow itself could not be run from here; it runs on the next
  release or by hand with `workflow_dispatch`. What was checked locally,
  on macOS, against 0.35.1:

  - The YAML parses (Ruby's YAML) and the job graph is the one above.
  - `npx -y pepe-http@0.35.1 --version` prints `pepe 0.35.1`, and
  `self-update --check` says "installed with npm".
  - `pip install pepe-http==0.35.1` in a fresh venv, then `pepe
  --version`: downloads the binary, prints `pepe 0.35.1`.
  - `pipx run --spec pepe-http==0.35.1 pepe --json --dry-run -n 10
  https://example.com` prints the dry-run report with `"sent": 0`.
  - The new steps' shell, run verbatim with `EXPECTED=0.35.1`: every
  assertion passes, and the retry loop gives up as it should on a version
  that doesn't exist.

  Not checked here: the Windows and Linux runners themselves (pipx's bin
  dir on PATH, `npx` under Git Bash).


## [0.36.0](https://github.com/omarmhaimdat/pepe/compare/v0.35.1...v0.36.0) - 2026-10-11

### Added

- *(agents)* pepe in the MCP registry, as the PyPI package and the Docker image ([#162](https://github.com/omarmhaimdat/pepe/pull/162))

  `server.json` at the root describes `pepe mcp` for the official MCP
  registry (registry.modelcontextprotocol.io) as
  `io.github.omarmhaimdat/pepe`, with two packages:

  - **PyPI** `pepe-http`, run as `uvx pepe-http mcp`. uvx runs the command
  named like the package, so the package gains a `pepe-http` command
  alongside `pepe` (same entry point). The README carries the registry's
  ownership mark, `<!-- mcp-name: io.github.omarmhaimdat/pepe -->`.
  - **Docker** `ghcr.io/omarmhaimdat/pepe:<version>`, run with `--network
  host` so the tools reach services on the host. The image gains the
  `io.modelcontextprotocol.server.name` label, which is how the registry
  checks ownership of an image.

  The npm package is not listed, because its `package.json` is generated
  by cargo-dist and cannot carry the `mcpName` field the registry requires
  there.

  Both packages take the guardrail flags as arguments after `mcp`,
  described in the listing with their defaults.

  **Publishing** is a new `publish-mcp-registry` job at the end of
  `release.yml` (a hand edit that `allow-dirty` keeps), after
  `publish-pypi` and the Docker job, since the registry checks the live
  package and image. It puts the release's version into `server.json` with
  jq, authenticates with the job's OIDC token (the `io.github` namespace
  needs no secret), and publishes. `announce` waits for it like the other
  publish jobs.


## [0.35.1](https://github.com/omarmhaimdat/pepe/compare/v0.35.0...v0.35.1) - 2026-10-11

### Fixed

- *(update)* a pepe from npm or PyPI is told to update through them ([#160](https://github.com/omarmhaimdat/pepe/pull/160))

  The daily update notice and `pepe self-update` told every copy the
  installer had not put in place to run `pepe self-update`, which is the
  wrong tool for the npm and PyPI packages: each version of those runs the
  pepe of that version, so the package is what gets updated. The install
  method detection now recognises both by where the binary lives (under
  `node_modules`, or in the `pepe-http` cache the PyPI package keeps on
  each platform) and says `npm update -g pepe-http` or `pipx upgrade
  pepe-http`, with a test for the paths on macOS, Linux and Windows.

  The skill file and `llms.txt` list the two packages among the ways to
  install, as the install page already does.

  This is a `fix`, so release-plz opens the release pull request; merging
  that is what publishes pepe-http to PyPI for the first time (through the
  plain `publish-pypi` job from #159) and validates npm's trusted
  publisher.



### Other

- *(pypi)* the publish as a plain job in release.yml, so trusted publishing can match it ([#159](https://github.com/omarmhaimdat/pepe/pull/159))

- npm through trusted publishing, and the PyPI publisher's workflow name ([#158](https://github.com/omarmhaimdat/pepe/pull/158))

  Two fixes from the v0.35.0 release run, whose npm and PyPI jobs failed
  while everything else shipped.

  **npm.** The change moving `publish-npm` to npm's trusted publishing was
  pushed to the packaging branch after #154 had been merged, so master
  still had the token-based job and no `NPM_TOKEN`, hence `ENEEDAUTH`.
  This lands it: the job gets `id-token: write`, updates npm (trusted
  publishing wants 11.5.1 or newer), and publishes with no secret. The
  trusted publisher on npmjs.com already names `release.yml`.

  **PyPI.** `publish-pypi.yml` is a reusable workflow called from
  `release.yml`, and PyPI reads the calling workflow's name off the OIDC
  token, so a publisher set to `publish-pypi.yml` can never match (the
  run's error says exactly that). The publisher on pypi.org must name
  `release.yml`; the workflow's comment now says so. No code change is
  needed for that job: once the publisher is corrected, the failed job can
  be rerun on its own.


- *(roadmap)* what 1.0 means and what comes before it ([#155](https://github.com/omarmhaimdat/pepe/pull/155))

  A section in ROADMAP.md naming what 1.0 freezes: the reports'
  `schema_version` and fields, the exit codes, every flag's name and
  meaning, `pepe.toml`'s keys, the MCP tools' names and inputs, and the
  install lines. Then the boxes to tick before the tag: a month without a
  schema or flag change on master, the data-driven requests flag (the one
  still likely to shape the config file), and a release note that states
  the freeze.


## [0.35.0](https://github.com/omarmhaimdat/pepe/compare/v0.34.0...v0.35.0) - 2026-10-11

### Added

- *(install)* pepe-http on npm and PyPI ([#154](https://github.com/omarmhaimdat/pepe/pull/154))

  pepe installs from npm and from PyPI, both as `pepe-http`, both giving
  the command `pepe`.

  ```bash
  npm install -g pepe-http      # or: npx pepe-http -n 100 https://example.com
  pipx install pepe-http        # or: pip install pepe-http
  ```

  ## The name

  `pepe-cli` is taken on both registries (on npm by ttoss/pepe, on PyPI by
  a protein-embedding tool), and `pepe` itself is taken on PyPI.
  `pepe-http` is free on both, says what pepe is, and reads the same
  everywhere: `npx pepe-http`, `pipx install pepe-http`, the command
  `pepe`. The Python module is `pepe_http` to match.

  ## npm

  dist's built-in npm installer: `installers` gains `"npm"`, `npm-package
  = "pepe-http"` (no scope), and `publish-jobs` gains `"npm"`. `dist
  generate` (0.33.0, the pinned version) regenerated
  `.github/workflows/release.yml`; since `allow-dirty = ["ci"]` keeps dist
  from touching the file, I ran it with that line off and then put back
  the two hand edits the setting protects: Dependabot's
  `actions/checkout@v7` (dist writes v6) and `contents: write` on the
  action job. The diff of release.yml is additions only: the `publish-npm`
  job, the `custom-publish-pypi` job, and both in `announce`. `dist plan`
  lists `pepe-npm-package.tar.gz`. The npm package takes `description` and
  `homepage` from Cargo.toml.

  ## PyPI

  There is no dist installer for PyPI, so `packaging/pypi/` is a small
  pure-Python package: `pepe_http/__init__.py`, with `__main__.py`,
  `pyproject.toml`, `README.md` and the root `LICENSE` copied. Standard
  library only, Python 3.9+. On first run it maps the platform to a
  release target (macOS arm64/x86_64, Linux x86_64/aarch64 musl, Windows
  x86_64; Windows on ARM gets the x86_64 build), downloads
  `pepe-<triple>.tar.xz`/`.zip` and its `.sha256` from the GitHub release
  of the package's own version, refuses a mismatch, reads only the
  `pepe`/`pepe.exe` member out of the archive, writes it next to its place
  and moves it in, under `~/.cache/pepe-http`
  (`~/Library/Caches/pepe-http` on macOS, `%LOCALAPPDATA%\pepe-http\Cache`
  on Windows; `PEPE_HTTP_CACHE_DIR` overrides), then `os.execv`s it (on
  Windows, a subprocess whose exit code is passed on). Every later run
  only runs it.

  The version lives in `pepe_http.__version__` (what `pyproject.toml`
  reads through `dynamic`), so it works from a checkout too;
  `.github/workflows/publish-pypi.yml`, a custom dist publish job like
  `publish-homebrew.yml`, sets it from the release tag, builds with
  `python -m build`, installs the wheel and runs `pepe --version` against
  the release the host job has just created, and publishes with
  `pypa/gh-action-pypi-publish@release/v1` under `id-token: write`
  (trusted publishing, no token).



### Other

- main.rs split by mode, the help in order of use, usage errors that say what to do ([#153](https://github.com/omarmhaimdat/pepe/pull/153))

  Three things in one pass over the same files.

  **The split.** `src/main.rs` was 2,100 lines with every mode's runner in
  it. It is now the dispatch alone (270 lines); the modes are in
  `src/modes/`, one file each (run, ping, ramp, compare, api, flow,
  replay, logs), what they share in `modes/mod.rs`, and the terminal
  handling (raw mode, Ctrl-C, the way out when the terminal stops reading)
  in `src/terminal.rs`. A pure move: no behaviour changed.

  **The help.** `pepe --help` lists the commands in the order people reach
  for them: ping, ramp, compare, api, flow, replay, logs, then schema and
  mcp, then self-update and completions. Man pages, completions and the
  reference page regenerated.

  **The errors.** Every usage error names the flag and says what to give
  instead. Before and after:

  ```
  Duration must have a unit (s, m, h). Examples: 10s, 5m, 2h
  -z "10" is not a duration: a number and a unit, as in 10s, 5m or 2h

  Concurrency cannot be greater than the number of requests. -c 50 -n 10
  -c 50 is more than the 10 requests of -n: lower -c, raise -n, or run for a time instead with -z 30s

  Invalid header "Nope": expected 'Name: value'
  header "Nope": a header is Name: value, as in -H 'Accept: application/json'
  ```

  Same for the timeout, the method, the proxy, the rate, `--trace-header`,
  `--every` and `--warmup`. A pepe with nothing to run and no terminal
  says so in one line instead of printing the whole help.

  `cargo test`, clippy with `-D warnings`, and the site check pass.


- *(site)* the front page in the docs' style ([#152](https://github.com/omarmhaimdat/pepe/pull/152))

  pepe.mhaimdat.com was a page of its own, a terminal in monospace, beside
  a documentation site that looked nothing like it. It is now one site in
  one style: the front page opens like the docs' overview (logo, name, one
  sentence, three buttons), then the install line with a tab per platform,
  the home recording, a card per mode, the three benchmark numbers and a
  footer, using the docs' stylesheet and script, light and dark.

  The four dashboard screenshots the old page showed are removed with it
  (the recordings show the same); the publish job's URL check no longer
  lists them. Checked in the browser at phone and desktop widths.


## [0.34.0](https://github.com/omarmhaimdat/pepe/compare/v0.33.0...v0.34.0) - 2026-10-11

### Added

- *(agents)* pepe as a Claude Code plugin ([#151](https://github.com/omarmhaimdat/pepe/pull/151))

  The repository installs as a Claude Code plugin.

  ```bash
  claude plugin marketplace add omarmhaimdat/pepe
  claude plugin install pepe@pepe
  ```

  What the plugin brings:

  - The `pepe` skill (`skills/pepe/SKILL.md`, unchanged): how to call pepe
  and what to read in each report.
  - The MCP server, from `.mcp.json`: `pepe mcp --max-requests 10000
  --max-rate 500 --max-concurrency 64`. No `--allow-host` is shipped,
  since a placeholder would refuse every target; a project names its hosts
  in `pepe.toml`, which the server's subprocess reads from the working
  directory like every other mode.
  - The binary is not installed by the plugin; it must be on `PATH`.



### Other

- cargo audit on every PR, and the bench server past its time advisory ([#150](https://github.com/omarmhaimdat/pepe/pull/150))

  Dependabot's alert 13 is the `time` crate pinned by
  `bench/server/Cargo.lock` (GHSA-r6v5-fh4h-64xc, stack exhaustion). It is
  now 0.3.55, which wants Rust 1.88; the bench server is built with stable
  and is not under the MSRV job, so that is fine.

  An `Audit` job in CI runs `cargo audit --deny warnings` on both lock
  files, so the next advisory fails a pull request instead of arriving by
  email.


- the overview opens like a landing page ([#149](https://github.com/omarmhaimdat/pepe/pull/149))

  The docs' first page now opens like a landing page: the logo, the name,
  one sentence and three buttons (Get started, Read the docs, View on
  GitHub), then the install lines, the home recording and the cards. The
  site's generator learns a `:::hero` block for it and takes the page
  description from the hero's sentence; the CSS adds a few lines for the
  hero. The rest of the site is unchanged.

  Checked with `python3 site/build-docs.py --check` and in the browser at
  phone and desktop widths, light and dark.


- the leftover scripts out, and the standard files in ([#148](https://github.com/omarmhaimdat/pepe/pull/148))

  ## What

  - **Leftovers removed.** `quick_check.sh` and `test_simple.sh` were from
  v0.3 and checked for files that no longer exist (`release.yaml`, the old
  install script); `.vscode/` was one editor's launch config. All gone;
  `.gitignore` already kept new copies out, these were just tracked from
  before.
  - **`SECURITY.md`**: how to report a vulnerability privately (GitHub's
  private advisories, or email), what is in scope (the binary and its
  inputs, the installers and release artifacts, the action, the MCP
  server), what isn't, and the guardrails for what pepe is pointed at.
  - **Issue templates**: a bug form (version, platform, the exact command
  with a note that `--dry-run` is safe to paste, expected, actual, the
  terminal's size for a screen) and a feature form that asks for the
  situation before the feature, with a pointer to the roadmap. The chooser
  links to the docs, discussions and private security reporting.
  - **A pull request template** that explains the conventional title the
  changelog and the release are built from, and asks what was checked,
  including the generated files.

  ## Also worth doing, by hand

  Two repository settings are off and don't come from a file: Dependabot
  security updates and secret scanning with push protection (Settings →
  Code security). Both are free for a public repository and both are
  things listing reviewers look at.

  ## Checked

  - The three YAML templates parse; the forms render in GitHub's chooser
  once merged
  - Nothing referenced the removed scripts


- *(readme)* a front page, with the manual on the site ([#146](https://github.com/omarmhaimdat/pepe/pull/146))

  ## What

  The README was 769 lines: the whole manual, which since #133 and #135
  lives on the docs site. This makes it a front page: the pitch, the
  recording, install, a quick start, a table of the modes with a line and
  a link each, what makes pepe usable from scripts, CI and agents (the
  JSON report and its schema, the exit codes, the guardrails, the action,
  the metrics, the MCP server), the benchmark tables, and where the
  documentation, the changelog and the roadmap are. 100 lines.

  What was stale went with it: "160+ tests" (there are 300), the roadmap
  paragraph that still listed things that shipped, and the `-d @file`
  example fixed in #137.

  - `CONTRIBUTING.md` holds what the Contributing section had, and more:
  the build and the checks, the generated files and how to regenerate
  them, the recordings, the benchmarks, the Windows checklist, commits and
  releases, and a map of the source tree.
  - The install page's "docs" links point at the docs site rather than the
  README's old Usage section.
  - The modes table names the long flags, as the review in #13 asked; that
  issue is answered point by point and closed.

  Nothing in the manual is lost: every section the README had is a page on
  the site, written from it.

  ## Checked

  - Every file the README links to exists; the docs-site check is clean
  - No source or workflow linked into README sections (the action's
  listing shows the README's front page, which is the intent)


## [0.33.0](https://github.com/omarmhaimdat/pepe/compare/v0.32.0...v0.33.0) - 2026-10-10

### Added

- *(mcp)* pepe mcp serves the modes as tools for an agent ([#145](https://github.com/omarmhaimdat/pepe/pull/145))

## [0.32.0](https://github.com/omarmhaimdat/pepe/compare/v0.31.0...v0.32.0) - 2026-10-10

### Added

- *(ping)* HTTP/2 when the server offers it ([#142](https://github.com/omarmhaimdat/pepe/pull/142))

  ## What

  A ping takes HTTP/2 when the server offers it through ALPN, which most
  do over TLS, and says so: `HTTP/2 200` in the `--once` line and the
  pings list, `"http_version": "HTTP/2"` in the report, `h2` under the TLS
  details. The phases are the same, the first byte being the response's
  headers. `--http1` stays on HTTP/1.1, to compare the two. A server that
  offers only HTTP/1.1 is noted among the findings. `--keep-alive` keeps
  the HTTP/2 connection, and the pings after the first reuse it.

  ```
  $ pepe ping https://example.com --once
    dns 2.02ms → connect 58.16ms → tls 60.32ms → first byte 67.05ms → download 3.04ms · 190.8ms in all
    HTTP/2 200 · text/html · 334 B · 104.20.23.154:443 from 192.168.0.164 · TLS 1.3 TLS13_AES_256_GCM_SHA384
  ```

  ## Why

  Ping makes the protocol gap visible: curl and httpstat show `HTTP/2 200`
  where pepe showed `HTTP/1.1`, and the phase breakdown is the most useful
  thing pepe has for a single request, so it should see what the server
  actually speaks.

  ## How

  - The ping's TLS config offers `h2` then `http/1.1` (unless `--http1`);
  after the handshake, if `h2` was negotiated, `h2::client::handshake`
  takes the stream and its connection is driven by a task of its own. The
  kept connection has a third kind, the h2 sender, so `--keep-alive` works
  as it does over HTTP/1.1.
  - `h2_exchange` builds the request from the run's headers less `Host`
  and `Connection` (HTTP/2 has `:authority` and no connection headers),
  times the first byte at the response's headers and the download to the
  end of the body, releasing flow-control capacity as it reads. Redirects
  go through the same loop as before.
  - The dependency is hyper's `h2` crate (and `http`, already in the
  tree); only `h2` and `tokio-util` are new in the lock file. The
  load-testing modes still speak HTTP/1.1, where pepe's engine is fastest;
  the roadmap's "not now" entry says so.

  ## Checked

  - `cargo test --locked`: 296 passed, including an in-process h2 server
  the exchange runs against twice on one connection; clippy with `-D
  warnings` and `cargo fmt --check` clean
  - By hand against example.com: `--once` over HTTP/2 and with `--http1`,
  and a `--keep-alive --json` ping showing `HTTP/2`, `h2`, and two of
  three pings on the kept connection


## [0.31.0](https://github.com/omarmhaimdat/pepe/compare/v0.30.0...v0.31.0) - 2026-10-10

### Added

- the verdict says where the time went, and the JSON report carries it ([#140](https://github.com/omarmhaimdat/pepe/pull/140))

  ## What

  - **Findings about where the time went**, in the verdict of a run, when
  it is somewhere worth knowing:
  - connecting being a quarter or more of each request when every request
  opens a connection (`--disable-keepalive`);
  - connections opened three times over the concurrency or more, so the
  server closes them and keep-alive isn't holding;
    - DNS lookups over 50 ms on average;
  - downloading large bodies (32 KiB and up) being a quarter or more of
  each request;
  - half or more of the responses answered by a cache, by their headers,
  so the origin wasn't measured for those.

  These are the rules `pepe ping`'s findings use, read from what a load
  run measures anyway (the connect histogram, the first-byte and download
  phases, the DNS probe, the cache headers).

  - **The verdict in the JSON report.** `--json` reports of a run, API
  mode, a flow and a replay now carry `verdict`: `level` (healthy,
  degraded, failing) and `findings`, each with a level and its text, what
  the dashboard would have said. Until now the JSON had the numbers and
  the anomalies but not the judgement. The schema and the docs have it;
  the agents page says to read it first.

  - A trend is no longer read off the empty seconds that trail the last
  request, which could call a short run's throughput "fallen 100%".

  ## Where

  - `src/insights.rs`: `phase_notes`, and the trailing-seconds trim in
  `verdict`
  - `src/ui/mod.rs`: the dashboard appends the phase notes to its verdict
  - `src/json_report.rs`, `src/main.rs`: `with_verdict`, built by
  `verdict_of` in every `--json` runner
  - `schema/run.schema.json`, `docs/output.md`, `docs/dashboard.md`,
  `docs/agents.md`

  ## Checked

  - `cargo test --locked`: 296 passed, including each phase rule and the
  keep-alive reconnect rule; clippy with `-D warnings` and `cargo fmt
  --check` clean; the docs site current
  - A real `--json` run and a piped run of example.com: the cache finding
  appears in both, and the short run no longer says its throughput fell


## [0.30.0](https://github.com/omarmhaimdat/pepe/compare/v0.29.0...v0.30.0) - 2026-10-10

### Added

- guardrails for scripts and agents, and what an agent reads first ([#138](https://github.com/omarmhaimdat/pepe/pull/138))

  ## What

  Two roadmap items under *Agents*.

  ### Guardrails

  Where a run may be pointed and how much it may send, checked in every
  mode (a run, a ramp, API mode, a flow, a replay, a ping) before anything
  is sent. A refusal is exit code 2 with the reason on stderr.

  | Flag | Holds |
  | --- | --- |
  | `--allow-host .example.com` | Every target's host must be
  `example.com` or under it; `api.example.com` is that host exactly;
  repeat for more. A flow step whose host comes from a capture, and a ping
  of a command (`--cmd`), are refused under it. |
  | `--max-requests N` | `-n` must be under it; `-z` needs a `--rate` that
  bounds it; a ping needs `-n` or `-z`; a ramp is refused, having no
  bound. |
  | `--max-rate N` | `--rate` must be under it, and must be given: without
  it a run sends as fast as the target answers. A ping's targets over its
  interval count. |
  | `--max-concurrency N` | `-c`, and a ramp's top step. |
  | `--dry-run` | What would be sent, to where and how much, with secrets
  in headers masked, and nothing sent; `--json` gives it as JSON with
  `"sent": 0`. |

  The same four keys work in `pepe.toml` (`allow-host`, `max-requests`,
  `max-rate`, `max-concurrency`), where leaving a flag off the command
  line can't loosen them.

  ```
  $ pepe --allow-host .example.com --max-requests 100 --max-concurrency 8 -n 50 -c 4 --dry-run -H 'Authorization: Bearer abc' https://api.example.com/health
  pepe · dry run · nothing sent
    mode      run
    target    https://api.example.com/health
    method    GET
    headers   Authorization: Bearer ••••
    load      50 requests, 4 at a time, each sent as soon as the last answered
    settings  timeout 20s · connections kept alive · redirects followed
    guard     hosts .example.com · at most 100 requests · at most 8 in flight
  ```

  ### What an agent reads first

  `AGENTS.md` in the repository, `llms.txt` at the site's root (published
  with the site), the agents page on the docs site, and the skill file,
  all saying the same: the flags to always pass, the fields to read in
  each report, the exit codes, and the guardrails.

  ## How

  `src/guard.rs` holds the checks (`Guard`) and the plan (`Plan`); each
  mode's runner calls them after loading what it needs (a spec's base URL,
  a flow's steps rendered with its `[vars]`, a replay's URLs, a ping's
  targets) and before starting. Host matching strips ports and brackets
  and ignores case; `.example.com` and `*.example.com` mean the same.

  ## Checked

  - `cargo test --locked`: 299 passed, including host matching, each cap,
  and the plan's masking; clippy with `-D warnings` and `cargo fmt
  --check` clean
  - By hand: a URL outside the allowed hosts refused, `--max-requests`
  against a timed run without `--rate`, a ping over its cap, a ramp over
  `--max-concurrency`, and dry runs as text and JSON for a run, a ramp and
  a ping



### Other

- *(readme)* -d takes the body itself; only a curl command reads @file ([#137](https://github.com/omarmhaimdat/pepe/pull/137))

  The README's example sent `-d @payload.json`, which pepe's own `-d`
  doesn't read as a file (a curl command's `-d @file` is read, through
  `--curl`). The docs site already had it right; the README now does too.

  Looked at the install page's four screenshots while here and left them:
  they are hand-made 2896×1836 captures of a worked scenario with matching
  captions and alt text, and frames cut from the 1280×720 recordings would
  be a step down.


- every recording made again, one for the home page, and the site published on its own ([#136](https://github.com/omarmhaimdat/pepe/pull/136))

  ## What

  - **Every recording, again**, with the current screens: the run, the
  setup screen, the ramp, API mode and the logs, and four new ones:
  - `ping.gif`: two targets on the graph (example.com and a local
  service), then the phases of one with its TLS session, certificate and
  findings, then the pings with one opened;
  - `flow.gif`: a three-step flow against `assets/demo/checkout.toml`,
  each step fed by the one before, each a row on the dashboard;
  - `compare.gif`: two reports under the same load, the second of a slower
  endpoint, then what moved (Slower, with the p99, the median and the
  throughput);
  - `home.gif`, for the docs' home page: `pepe ping URL --once` saying
  where the time goes, then a run with the dashboard, ended with the
  verdict in the shell.
  `assets/record.sh` records them all from `assets/tapes/`, against the
  local target it starts, as before.
  - **The site goes out on its own.** It was published only by a release,
  and a docs change never makes one, so the documentation site merged in
  #133 and #135 had not been deployed. `publish-site.yml` uploads the
  install page, the docs, the recordings and the schemas whenever any of
  them change on master (and on request), checks the built site is current
  first, and verifies the public URLs afterwards. The release workflow
  uploads the same files through the same script, `site/publish.sh`, so
  the two can't drift.
  - The ping, flow and compare pages and the README's ping section show
  their recordings; the home page shows its own.

  ## Checked

  - Each recording's frames looked at: the home page's two halves, the
  ping's graph and phases, the flow's three rows all answering, the
  compare's verdict; the compare tape re-recorded once with longer pauses
  after its runs overlapped the typing
  - The demo flow completes its chains against the local server (5 of 5,
  every step 200)
  - `python3 site/build-docs.py --check` clean; the contrib and
  docs-reference tests pass
  - Recordings total 13 MB, in line with the five there were

  Merging this triggers the first deployment of the documentation site.


- the site as a documentation site, not a terminal ([#135](https://github.com/omarmhaimdat/pepe/pull/135))

  ## What

  The docs pages were the install page's terminal look stretched over long
  prose: one monospace face for everything, dark only, dense, hard to
  scan. This rebuilds them as a documentation site.

  - **Type and colour.** Prose in a proportional face, code in a monospace
  one. Light by default, dark when the system or the switch in the top bar
  says so; the choice is remembered. The accent stays the dashboard's
  ember.
  - **Finding things.** A top bar with search over every page (an index is
  written beside the pages at build time, loaded on first use; `/` focuses
  the box, arrows pick a result, results say which page and section), a
  sidebar grouped into Start here, Modes, Integrate and Reference, an "on
  this page" column that follows the scroll, and previous/next links at
  the bottom of each page.
  - **Reading.** A 76-character measure, headings with room around them
  and anchors on hover, code blocks with the language and a copy button,
  tables with borders and a hover, callouts, framed images, an
  edit-on-GitHub link.
  - **The landing page.** A card per page with what it is for, and where
  to start.
  - **Phones.** The sidebar behind a menu button, the search in the bar,
  tables that wrap, no horizontal scroll.

  ## How

  Same Markdown sources under `docs/`. `site/build-docs.py` gets the new
  frame, two small blocks of its own (`:::cards` and `:::note`), the
  search index (`search.json`), and the sidebar groups.
  `site/docs/docs.css` is the new stylesheet and `site/docs/docs.js` the
  page's few behaviours (theme, menu, copy, search, the scroll-following
  list). No dependency anywhere. The release workflow publishes the script
  and the index with the pages, and the Lint job's check of the built site
  is unchanged.

  ## Checked

  - `python3 site/build-docs.py --check` clean; the built site is
  committed
  - Served locally and checked in the browser: the landing page and its
  cards, a long page with its tables and code blocks, search (results for
  `slo` across pages), the theme switch and that it is remembered, the
  phone layout with its menu, and no horizontal scroll at 375px


## [0.29.0](https://github.com/omarmhaimdat/pepe/compare/v0.28.0...v0.29.0) - 2026-10-10

### Added

- a report without a terminal, --fail-if, a versioned report with its schema, and the documentation site ([#133](https://github.com/omarmhaimdat/pepe/pull/133))

  ## What

  Two things, the second built on the first.

  ### Three roadmap items that make pepe safe to call from a script

  - **Without a terminal, a report.** Piped, redirected or run by a script
  without `--json`, pepe used to try to open the dashboard and fail. Now
  the run goes to its end and the report the dashboard would have left is
  printed: the same verdict and findings. A plain run, a ramp, API mode, a
  flow and a replay all do it; `--snapshot` and `--metrics` work as on the
  dashboard. API mode needs its endpoints picked on the command line,
  since there is no plan screen.
  - **`--fail-if 'p99 > 300ms'`**, in the ramp's condition language,
  repeatable, in every mode that measures requests, with or without
  `--json`. The report is still printed, each crossed condition is said on
  stderr with the number it saw (`✖ --fail-if p99 > 300ms: p99 was
  412.0ms`), and the exit code is 4, the same as a ping's `--slo`. `pepe
  ping` refuses it and points at `--slo`.
  - **A versioned report.** Every JSON report starts with
  `"schema_version": 1`. The JSON Schema of each report kind is in
  `schema/`, embedded and printed by `pepe schema run|ramp|ping|compare`,
  shipped in every release archive, and published at
  `pepe.mhaimdat.com/schema/`. A test holds a real report of each kind
  against its schema.

  ```bash
  pepe -z 30s -c 20 https://example.com > run.txt
  pepe --json -n 2000 --fail-if 'p99 > 300ms' --fail-if 'errors > 1%' https://staging.example.com/api
  pepe schema ping | jq '.properties.targets.items.required'
  ```

  ### The documentation site, at pepe.mhaimdat.com/docs

  Every feature of pepe, page by page, with examples: overview, install,
  load testing (every option, the load's shape, the config file, the curl
  command, the setup screen), the dashboard and its keys, ramp, OpenAPI,
  flows, replay, ping, nginx logs, compare, output and exit codes, CI and
  Docker, agents and scripts, benchmarks, and a command reference.

  - The sources are Markdown files under `docs/`, readable on GitHub as
  they are.
  - `site/build-docs.py` builds them into `site/docs/` with no dependency:
  the install page's look, a sidebar, an "on this page" list, anchors on
  every heading, and a phone layout. The built site is committed, and the
  Lint job checks it is current.
  - `docs/reference.md`, every command's `--help`, is generated from the
  command definition by `cargo test` (`UPDATE_CONTRIB=1` rewrites it), so
  it can't drift from the binary. It documents `pepe schema` and
  `--fail-if`, which is why the two parts share a branch.
  - The release workflow publishes `site/docs/` with the rest of the site
  and verifies two of its pages; the install page's "docs" link and the
  README point at it.

  ## How

  - `Dashboard::run_headless` collects results on the same pump as the
  dashboard, writes snapshots, and leaves `report()` and `metrics()` to
  read afterwards. Each mode's runner gains a headless branch. The ramp's
  climb is factored out of `--json` (`climb`) and shared with the piped
  path.
  - `exit_if_failed` runs `Condition::crossed` against the run's final
  metrics; the interactive sessions return their metrics with their report
  so it runs after the screen closes.
  - `src/schema.rs` embeds the four schema files and carries a small
  checker (required fields, types, `$ref` into `$defs`, items) used by the
  tests.

  ## Checked

  - `cargo test --locked`: 295 passed; clippy with `-D warnings`, `cargo
  fmt --check` and `python3 site/build-docs.py --check` clean
  - By hand: a piped plain run and a piped ramp print their reports;
  `--fail-if` exits 4 with and without `--json`; `pepe schema ping`
  prints, `pepe schema nope` says what exists
  - The site served locally and checked in the browser at desktop and
  phone widths: sidebar, tables, code blocks, images from `assets/`,
  anchors; no horizontal scroll on a phone


## [0.28.0](https://github.com/omarmhaimdat/pepe/compare/v0.27.0...v0.28.0) - 2026-10-10

### Added

- *(compare)* two ping reports compare, phase by phase ([#129](https://github.com/omarmhaimdat/pepe/pull/129))

  ## What

  `pepe compare` reads ping reports (`pepe ping --json`, or `--save
  report.json`) and holds them against each other: the median, the p99,
  the pings lost, and the median of each phase, so a slower connect or a
  slower handshake is told from a slower server.

  ```
  pepe · compare before.json → after.json
  ▲ Slower · 600 → 600 pings · p99 50.00ms → 72.00ms
    ▲ Median up 50%: 40.00ms → 60.00ms
    ▲ Connect up 150%: 10.00ms → 25.00ms
    ▲ TLS handshake up 60%: 25.00ms → 40.00ms
    ✔ First byte within the usual spread: 6.00ms → 6.00ms (±5%)
  ```

  ## Why

  Ping reports were the one kind of report `pepe compare` refused. A
  before-and-after of a deploy, a DNS change or a CDN move is exactly what
  they are for, and the phases say which part moved.

  ## How

  - `Side::read` recognises `"mode": "ping"` and reads the first target:
  `sent`, `lost`, the latencies, and each phase's median (`dns`,
  `connect`, `tls_full`, `tls_resumed`, `ttfb`, `download`), plus
  `every_s`.
  - A lost ping is a failed request, so failures outrank speed as before;
  the head and the findings say "lost" and "pings" for a ping report.
  - A ping report has no latency spread, so the jitter (the mean
  difference between one ping and the next) stands in for it, scaled to a
  standard deviation, and the usual spread-based threshold applies. Small
  runs get more room, as they do for load reports.
  - Each phase is a `Change` of its own in the JSON, named in the badge
  when it moved most (`slower · connect +150%`), and drawn on the SVG
  card.
  - A different target or a different interval is said first, as different
  concurrency is.

  ## Where

  - `src/compare.rs`: `Side::of_ping`, the phase changes, the names, the
  tests
  - README: the compare section and a line in the ping section

  ## Checked

  - `cargo test --locked`: 285 passed; clippy with `-D warnings` and
  `cargo fmt --check` clean
  - Two real ping reports of example.com compared (https against http),
  and a report against itself ("about the same")


## [0.27.0](https://github.com/omarmhaimdat/pepe/compare/v0.26.0...v0.27.0) - 2026-10-10

### Added

- --metrics :9100 serves the live numbers for Prometheus ([#130](https://github.com/omarmhaimdat/pepe/pull/130))

  ## What

  `--metrics :9100` serves the live numbers at
  `http://localhost:9100/metrics` while the run goes, in Prometheus's text
  exposition form, so a soak run or a long ping shows up in Grafana next
  to the server's own metrics.

  ```bash
  pepe -z 6h -c 50 --metrics :9100 https://example.com
  pepe ping https://example.com --metrics 127.0.0.1:9100
  curl -s localhost:9100/metrics
  ```

  ## Why

  The roadmap's "Prometheus metrics" item: a soak run's numbers belong on
  the same board as the server's. With `pepe ping` in, a long ping belongs
  there too.

  ## How

  - `src/exporter.rs`: a server of a few dozen lines (tokio `TcpListener`,
  the request line parsed by hand, one text answered) and a page writer
  for gauges, counters and histograms. No new dependency. The page is
  rendered once a second at most, whatever scrapes it; the scrape is
  served from the last one. `--metrics` takes `:9100`, `9100` or
  `host:port`.
  - **Where it hooks in.** The dashboard publishes after each drain (so
  every mode on the dashboard is covered: plain runs, API mode, flows,
  replays); the `--json` loops of those modes publish on their pump tick;
  a ping publishes from a task once a second. A ramp isn't served, its
  steps being the point of it; said in the README.
  - **A load run** has requests finished, succeeded, timed out and
  errored, responses by status, failures by cause, bytes, cache hits,
  throughput and errors over the last second, latency quantiles (0.5, 0.9,
  0.95, 0.99) and first-byte quantiles, `pepe_request_duration_seconds`
  with fixed buckets from 1 ms to 30 s so two runs' histograms add up, the
  concurrency and the elapsed time. Every line carries `target="GET
  https://…"`; API mode, flows and replays have the same again per `row`.
  - **A ping** has sent, answered, lost, timed out, SLO breaks,
  `pepe_ping_up`, the last ping's time, the loss ratio, latency quantiles,
  jitter, each phase's median, responses by status, resumed TLS sessions,
  `pepe_ping_cert_not_after_seconds` and a histogram, each per `target`.
  - The address is printed to stderr at the start (`metrics:
  http://127.0.0.1:9100/metrics`), so `--json` output stays valid JSON.

  ## Where

  - `src/exporter.rs`: the server, the page, the two renderers, and their
  tests
  - `src/main.rs`: starting it, and publishing from the headless loops and
  the ping
  - `src/ui/mod.rs`: publishing from the dashboard
  - `src/cli.rs`: the flag, and `target_label()` shared by the runners
  - README (options table and a section), ROADMAP, man page, completions

  ## Checked

  - `cargo test --locked`: 288 passed, including the page format
  (cumulative buckets, escaping, one declaration per family), a run page
  with rows, and a served page fetched over TCP; clippy with `-D warnings`
  and `cargo fmt --check` clean
  - Scraped a live `--json` run of example.com three seconds in, and a
  live ping of example.com and a dead port, with `curl`


## [0.26.0](https://github.com/omarmhaimdat/pepe/compare/v0.25.0...v0.26.0) - 2026-10-10

### Added

- *(ping)* what the numbers mean, said in a few lines, and --once for a quick diagnosis ([#127](https://github.com/omarmhaimdat/pepe/pull/127))

  ## What

  `pepe ping` now says what to look at, next to the numbers: in the
  summary, in the phases view (a "look at" block), and under `findings` in
  the JSON report. And `pepe ping URL --once` is the quick version: three
  pings 200 ms apart, the last one's phases, what answered, the findings,
  and out.

  ```
  pepe ping · https://example.com/ · 3 pings
    dns 2.40ms → connect 69.7ms → tls 40.4ms → first byte 44.2ms → download 47µs · 157ms in all
    HTTP/1.1 200 · text/html · 334 B · 104.20.23.154:443 from 192.168.0.164 · TLS 1.3 TLS13_AES_256_GCM_SHA384
    certificate for example.com by SSL Corporation, expires in 76 days
    ▲ connecting takes 69.7ms of 157ms: a round trip of about 69.7ms; the server is far, and a kept connection would skip it
    ✔ TLS sessions are resumed: 30.1ms after the first handshake's 78.0ms
    · answered by a cache (cf-cache-status: HIT): the server itself wasn't measured
  ```

  ## Why

  Ping measures everything needed to say *why* a URL is slow, but until
  now only reported numbers. These few lines are the verdict's voice
  applied to a single request: the thing to paste into a channel.

  ## How

  `src/diagnose.rs` holds the rules, each reading what ping already has
  (the medians of each phase, the TLS sessions, the last response's
  headers, the losses, the tail):

  - nothing answered, and the cause; pings lost and timed out; 5xx; 401,
  403, 404 and 429 on every ping
  - every request redirected, with the final URL to point at and what the
  chain costs
  - a slow DNS lookup; connecting as a share of the request, read as a
  round trip; the TLS session never resumed (every connection pays the
  full handshake) or resumed and how much it saves; the server closing
  every connection
  - the server's own share of the time, with its `Server-Timing` segments
  when it sends them; a large or slow download; a text body over 4 KB that
  isn't compressed though gzip and br were offered; a cache answering (and
  so the server itself not measured); HTTP/1.0
  - a certificate expired or expiring within 30 days; a long tail (p99
  three times the median or more)

  Findings are ordered most serious first, with a glyph and a level
  (`bad`, `warn`, `note`, `good`) in the JSON.

  **Also fixed:** the connect phase was counted twice in a ping's
  breakdown (once by `connect`, once more when the phases were summed), so
  the phases added up to more than the total. A test now checks that the
  parts never exceed the whole.

  ## Where

  - `src/diagnose.rs`: the rules, `--once`'s output, and their tests
  - `src/ping.rs`: findings in the text report and the JSON; a sample
  keeps the URL its redirects ended at; the connect fix
  - `src/ui/ping.rs`: the "look at" block under the phases
  - `src/cli.rs`, `src/main.rs`: `--once`
  - README, man page and completions

  ## Checked

  - `cargo test --locked`: 284 passed; clippy with `-D warnings` and
  `cargo fmt --check` clean
  - `--once` and `--json` against example.com over https and http; the
  phases now add up to the total


## [0.25.0](https://github.com/omarmhaimdat/pepe/compare/v0.24.0...v0.25.0) - 2026-10-10

### Added

- *(ping)* pepe ping, a request a second split into DNS, connect, TLS, first byte and download, on a graph ([#124](https://github.com/omarmhaimdat/pepe/pull/124))

  ## What

  A new subcommand, `pepe ping`: ping, with HTTP instead of ICMP, and a
  graph. One request a second to one target or several, each split into
  where its time went, with the loss, jitter and percentiles of `ping`,
  the phase breakdown of `httpstat`, and the graph of `gping`.

  ```bash
  pepe ping https://example.com
  pepe ping api.example.com cdn.example.com --name api,cdn --every 500ms
  pepe ping https://api.example.com/health --slo total=300,ttfb=100 --bell
  pepe ping example.com --all-ips -4
  pepe ping db.internal --tcp --port 5432
  pepe ping 10.0.0.0/29:8080
  pepe ping --cmd 'dig example.com'
  pepe ping https://example.com --jsonl > pings.jsonl
  ```

  ## Why

  A load test is run now and then; a latency check is run all day. This is
  the same engine at one request a second, and the reason to have pepe
  open. It was scoped from what gping and httpstat have and what their
  issue trackers ask for: packet loss, text output when piped, stats over
  the whole run, a fixed y axis, named and coloured targets, CSV,
  thresholds with an alert, every address of a name, CIDR ranges, TCP
  pings, SLO limits, certificate details, redirect chains, JSON, the IPs
  at both ends, and full against resumed TLS handshakes.

  ## How

  - **Every phase, every ping.** Each ping opens its own connection, so
  DNS, connect, TLS, first byte and download are measured every time. The
  TLS session is still resumed when the server allows, and the report
  gives full and resumed handshake medians apart. `--keep-alive` keeps the
  connection, as a browser would. Redirects are followed, each hop with
  its status and time.
  - **Three views.** The graph (braille or dots, zoom, whole run, a floor
  at zero, a log scale, failures and SLO breaks marked on top); the phases
  of the picked target's last and median ping as a stacked bar, with
  curl's running totals, the TLS version, cipher and ALPN, and the
  certificate's subject, issuer and expiry; and the pings themselves, each
  openable to its hops, headers, TLS and body.
  - **The table.** last, min, avg, max, jitter, p95, p99, loss, timeouts
  and sent, over what the graph shows or the whole run.
  - **Targets.** URLs, bare hosts, host:port, `aws:REGION`, address
  ranges, `--all-ips`, `--tcp` with `--tcp-rst`, `--cmd`; `-4`, `-6` and
  `--interface`.
  - **Limits and output.** `--slo total=500,ttfb=200,...` marks a ping and
  sets the exit code to 4; `--bell` rings the terminal; exit 1 when
  nothing ever answered. Piped, or with `--jsonl`, `--csv` or `--json`,
  there is no screen. `--save` writes the JSON report, `--save-body` the
  last body.
  - A small X.509 reader (`src/cert.rs`) for the subject, issuer, validity
  and DNS names, with no new dependency.
  - Man page, completions, README section, roadmap entry, and an agent
  skill under `skills/pepe/` (as httpstat ships).

  ## Where

  - `src/ping.rs`: targets, the engine, stats, and the text, JSON, JSON
  Lines and CSV outputs
  - `src/ui/ping.rs`: the screen
  - `src/cert.rs`: the certificate reader
  - `src/cli.rs`, `src/main.rs`: the subcommand and its wiring
  - `src/direct.rs`: `tls_config` made `pub(crate)`, nothing else

  ## Checked

  - `cargo test`: 261 passed, including a live ping against a local server
  with a redirect, a closed port, a TCP ping and a command; the screen
  rendered at several sizes through `TestBackend`
  - `cargo clippy --locked --all-targets -- -D warnings` and `cargo fmt
  --check` clean
  - The release binary against example.com in text, `--jsonl`, `--csv` and
  `--json` modes; exit codes 4 (`--slo`) and 1 (nothing answered);
  `--save` and `--save-body`; the live screen recorded in a
  pseudo-terminal

  Left out on purpose: packet size (meaningless for HTTP; `-d` sends a
  body), shell completion of hostnames, an HTML report (on the roadmap
  already), gping's `--clear` (pepe leaves only the summary), and HTTP/2
  (the roadmap says not now).


## [0.24.0](https://github.com/omarmhaimdat/pepe/compare/v0.23.0...v0.24.0) - 2026-10-10

### Added

- *(compare)* the verdict as an SVG card and as a badge ([#119](https://github.com/omarmhaimdat/pepe/pull/119))

  Third of three, on top of #118 (the action's baseline and comment),
  which is on top of #117 (`pepe compare`). Merge in that order.

  - `pepe compare --svg card.svg` draws the verdict the way the dashboard
  draws: the word in its colour, then p99, median, throughput, capacity
  and failures before and after, each with what moved or the spread it
  stayed within. Self-contained SVG in pepe's palette, for a README, a
  site or a report. The README shows one:

  <img
  src="https://raw.githubusercontent.com/omarmhaimdat/pepe/feat/compare-card/assets/compare-card.svg"
  width="460">

  - The JSON gains `badge`: the verdict as a badge's three parts (`pepe`,
  `slower · p99 +38%`, a colour) for shields.io and the like.
  - The action's comment shows that badge under its heading, since a
  comment can only show an image by URL and nothing in the action hosts a
  file. The card goes to the run's artifacts, with a `card` output for its
  path; a repository that wants the card itself on the comment can push it
  to a branch and link it.

  **Squash-merge with the title as it is.**


## [0.23.0](https://github.com/omarmhaimdat/pepe/compare/v0.22.1...v0.23.0) - 2026-10-09

### Added

- *(action)* a pull request is held against its base branch, in a comment ([#118](https://github.com/omarmhaimdat/pepe/pull/118))

  Second of three, on top of #117 (`pepe compare`). Merge #117 first and
  let its release go out: the action runs the released binary, so merged
  before that, `baseline` would fail with "needs pepe 0.21 or newer". The
  CI on this PR doesn't wait for it: the action's test now builds pepe
  from the branch, which is what the action prefers to installing a
  release.

  Three inputs on the action:

  - `baseline: auto` keeps every branch's last report in the Actions cache
  and holds a pull request against its base branch's with `pepe compare`;
  a path names a report file instead. A pull request only reads the cache;
  a branch's own runs write it, so the first run on the base branch after
  this is added makes the baseline, and until then the comment says so.
  - `comment: true` posts the result on the pull request as one comment,
  found again by its marker and updated on every push: the verdict in the
  heading, a table of p99, median, throughput, capacity and failures
  before and after, each with its change or "within the usual spread",
  then the findings. Needs `pull-requests: write`; warns rather than fails
  without it, as on a fork.
  - `gate: true` fails the step on Slower or Worse.

  Two new outputs, `verdict` and `compare`. The README's action section
  shows the comment.

  The action's test runs it twice, the second time against the first as
  its baseline, gated, and commenting on this very pull request, so the
  comment should appear below.

  **Squash-merge with the title as it is.**


## [0.22.1](https://github.com/omarmhaimdat/pepe/compare/v0.22.0...v0.22.1) - 2026-10-09

### Fixed

- *(logs)* the traffic chart fills its panel instead of half of it ([#121](https://github.com/omarmhaimdat/pepe/pull/121))

  The bug in the screenshot: the dashboard's traffic chart has its bars in
  the right half of the panel and the left half blank.

  The chart took the finest round grain whose bars fit the panel, then
  widened the bars only when they covered less than half of it. An hour at
  30s a bar is 120 bars, so on a terminal 230 cells wide the bars sat in
  the right half; ten minutes at 5s a bar did the same on anything wider
  than 240 cells, and the README's own recording ([#114](https://github.com/omarmhaimdat/pepe/pull/114)) shows it too.

  Now there is a bar per column, each over its share of the seconds, and a
  bar holds a rate rather than a count, so bars of 4 and of 5 seconds
  stand level; the title says "~4s a bar" when the share isn't whole.
  Short spans still get wider bars. A test draws the dashboard at six
  widths, 80 to 300 columns, and wants the bars over at least three
  quarters of the panel; before the fix it failed at 240 and 300.

  Once this and #114 are both in, `assets/record.sh
  assets/tapes/logs.tape` should be run again so the README's logs
  recording shows a full chart.

  **Squash-merge with the title as it is.**


## [0.22.0](https://github.com/omarmhaimdat/pepe/compare/v0.21.0...v0.22.0) - 2026-10-09

### Added

- *(compare)* `pepe compare before.json after.json` says what moved ([#117](https://github.com/omarmhaimdat/pepe/pull/117))

  First of three: this, then the action's baseline and PR comment ([#2](https://github.com/omarmhaimdat/pepe/pull/2)),
  then the card ([#3](https://github.com/omarmhaimdat/pepe/pull/3)). Each is based on master and contains the one before;
  merge them in order, and a release has to carry this one before the
  action PR is merged, because the action runs the released binary.

  Two runs of the same build never give the same p99. `pepe compare
  before.json after.json` holds a report against an earlier one and calls
  a number a change only when it moved more than two runs like these
  wobble on their own: the run's own latency spread, scaled by how many
  requests sit past the percentile. Measured against a local server, the
  p99 of 2,000-request runs sat ±25% apart and that of 20,000-request runs
  ±1%; the model is fitted to that, with a 5% floor, and 10% for a ramp's
  capacity fit.

  - The verdict is **Faster**, **About the same**, **Slower**, or, when
  failures appeared or rose, **Worse** (**Better** when they fell):
  failures outrank speed.
  - Findings in the end-of-run report's shape, coloured in the shell: `p99
  up 38%: 120.0ms → 166.0ms`, `Median within the usual spread: 30.00ms →
  31.00ms (±5%)`, `A long tail is new: p99 is 5.4× the median, was 4.0×`.
  Different targets or concurrency are said first.
  - Run, API, flow and replay reports compare their summary; ramp reports
  their capacity estimate and the level that held.
  - `--gate` exits 1 on Slower or Worse, for CI. `--json` gives the
  verdict, every number with its change and the spread it was held
  against, and the findings.
  - Every report now carries a `target` block (mode, method, URL or
  source, concurrency), so the comparison can tell two tests apart.

  Tried on real reports: two identical 2,000-request runs read "About the
  same (p99 ±24%)"; the same run against a slower endpoint reads "Slower",
  and `--gate` exits 1.

  **Squash-merge with the title as it is.**


## [0.21.0](https://github.com/omarmhaimdat/pepe/compare/v0.20.2...v0.21.0) - 2026-10-09

### Added

- *(api)* `pepe api` alone asks for the spec, which can be pasted whole ([#114](https://github.com/omarmhaimdat/pepe/pull/114))

  `pepe api` with nothing after it printed an error. Now, at a terminal,
  it opens the setup screen in API mode with the Spec field focused, the
  way `pepe` alone opens it for a URL. The field takes a file, a URL, or
  the OpenAPI document itself: paste the whole spec anywhere on the form
  (multi-line text, or text opening a JSON object, is the document; one
  line is a name) and the field says "the pasted spec, 312 lines";
  backspace or typing starts over with a name. Piped, `pepe api` still
  errors as before.

  - The loader takes the document as a source too, so `pepe api "$(cat
  spec.json)"` works, and a pasted spec that names servers with `://` is
  no longer taken for a URL origin. The command printed on quit says `api
  '<the pasted spec>'` where there is no file to name.
  - The README's recordings are redone for the panel UI: run, setup, ramp
  and API, which now shows `pepe api` asking for the spec, and a new one
  for `pepe logs` in the nginx logs section. `record.sh` writes a day of
  nginx logs for it and keeps appending to them while recording, so "now"
  is a live number rather than a log that stopped.
  - Completions and man page regenerated for the new help text.

  **Squash-merge with the title as it is.**


## [0.20.2](https://github.com/omarmhaimdat/pepe/compare/v0.20.1...v0.20.2) - 2026-10-09

### Fixed

- *(action)* a description short enough for the Marketplace ([#113](https://github.com/omarmhaimdat/pepe/pull/113))

  The Marketplace refuses to publish the action: its description must be
  under 125 characters, and the one in action.yml was 137. Now 124, saying
  the same thing.

  The Marketplace reads action.yml at the release's tag, so this needs a
  release before the "Publish this Action to the GitHub Marketplace" box
  can be ticked. **Squash-merge with the title as it is** so release-plz
  opens the release PR; publish from that release.



### Other

- *(action)* every release moves the v0 tag, so the action is pinned like others ([#111](https://github.com/omarmhaimdat/pepe/pull/111))

  The repository is also a GitHub Action, and the README told people to
  use it at `@master`. Actions are pinned to a floating major tag
  (`actions/checkout@v7`), and the Marketplace lists releases, so pepe
  should have both.

  - `publish-action.yml` is a dist publish job like the Homebrew, R2 and
  Docker ones: once the GitHub Release for vX.Y.Z exists, it moves the
  `vX` tag (`v0` today, `v1` after 1.0) to the same commit, with
  `GITHUB_TOKEN`, which starts no further workflow.
  - The README and the site now say `omarmhaimdat/pepe@v0`.
  - Added to `release.yml` by hand: `dist generate` refuses to rewrite it
  while `allow-dirty = ["ci"]` is set (it is, so Dependabot can bump the
  actions in it).

  Not in this PR, because GitHub has no API for it: the Marketplace
  listing takes a one-time click on one release's edit page, "Publish this
  Action to the GitHub Marketplace", after accepting the Marketplace
  Developer Agreement. After that, the listing follows the releases by
  itself.

  **Squash-merge with the title as it is.** It's a `ci:` commit, so it
  rides along with the next feat/fix release; that release is the first
  one to move `v0`.


## [0.20.1](https://github.com/omarmhaimdat/pepe/compare/v0.20.0...v0.20.1) - 2026-10-09

### Fixed

- *(ui)* a card's name and the numbers beside it no longer run together ([#110](https://github.com/omarmhaimdat/pepe/pull/110))

  ## What

  On the load test's dashboard (the tall layout with the four number
  cards), the first card's name and the text at its right overlapped once
  the rate reached three digits:

  ```
  requests / savg 249 · peak 268      before
  requests / s        avg 249         after
  ```

  The card is 30 cells wide inside; `requests / s` is 12 and `avg 249 ·
  peak 268` is 18, so they touched with nothing between. At four digits
  and up (`avg 12.3k · peak 15.6k`) the numbers were drawn over the end of
  the name.

  ## Fix

  In `render_cards` (`src/ui/view.rs`), what is beside a card's name gets
  the room the name leaves, less a two-cell gap, through the existing
  `fit_parts`: the parts that fit are shown, and the rest give way from
  the right. So at three digits and up the card says `avg 249` and drops
  `peak 268`; the peak is still on the Live view's throughput chart and in
  the Stats view. The other three cards have one part each and are
  unchanged at any value seen so far.

  ## For the reviewer

  - It is the peak that goes, not the average, only because it is second.
  If the peak is the one worth keeping, swap the two.
  - Found while making the site's pictures: `site/img/api.png` shows the
  overlap ("requests / savg 249"). It can be redrawn once this is in; I
  have not redrawn it here.

  ## Tested

  - New test `a_card_keeps_a_gap_between_its_name_and_what_is_beside_it`:
  the header drawn at 40, 240 and 12,000 requests a second. Both parts at
  two digits, a gap and no "savg" at three, a gap at thousands.
  - 249 tests pass; clippy 1.98 `-D warnings` and `cargo fmt --check`
  clean.
  - Not looked at in a terminal; the test reads the drawn row.


## [0.20.0](https://github.com/omarmhaimdat/pepe/compare/v0.19.3...v0.20.0) - 2026-10-09

### Added

- *(logs)* a dashboard opens first, and every view is on panels ([#108](https://github.com/omarmhaimdat/pepe/pull/108))

  The same change as #105, this time into `master`.

  #105 was opened on top of #104's branch (`fix/logs-live`) so that its
  diff would show only the screen. #104 was then squash-merged, its branch
  stayed, and #105 was merged into that branch, not into `master`. So the
  dashboard never reached `master` and release-plz had nothing to release.
  This is #105's one commit cherry-picked onto today's `master`; nothing
  else is in it.

  **Squash-merge it with the title as it is** (`feat(logs): …`), so
  release-plz opens the release PR. The site merged in #107 already shows
  this dashboard in its Logs picture, so the release that carries the site
  should carry this too.

  ## What (from #105)

  `pepe logs` opened on a table of minutes. It now opens on a
  **Dashboard**, and Traffic, Paths, Errors and Log are views 2 to 5.

  - **Three numbers drawn large**: the rate now with a sparkline and how
  it stands against a usual minute; the share answering 5xx; the request
  time's p50 with p90 and p99.
  - **A verdict** beside them, in the colour of the server's health and
  never of its load: Steady, Busy, Quiet, Degraded, Failing, or Ended,
  with the path answering the most 5xx and the error log's most frequent
  message.
  - **Traffic**, a bar for every few seconds as far back as is known, up
  to an hour, with 4xx and 5xx in yellow and red in proportion, and a mark
  under bars whose 5xx are too few to show.
  - **Top paths, status codes, the error log by message**, and the newest
  lines.
  - The other views are each on a panel, with the numbers in one line
  above them; bars that say how much are one colour everywhere, so green,
  yellow and red mean health.
  - The number keys are now 1 to 5: Traffic was `1` and is `2`.

  ## Tested

  - 248 tests pass on this branch; clippy 1.98 `-D warnings` and `cargo
  fmt --check` clean.
  - As in #105: every view was rendered through pepe's theme to an image
  and looked at, and the release binary was driven through all five views
  on a live log in a pseudo-terminal. That was on #105's branch; on this
  one only the tests and lints were run, the code being the same commit.



### Other

- *(site)* pepe.mhaimdat.com as a terminal, drawn the way pepe draws ([#107](https://github.com/omarmhaimdat/pepe/pull/107))

  ## What

  `site/index.html` was an install box and a GIF. It is now a site that
  looks and behaves like pepe itself. (The first commit here is a
  conventional landing page; the second replaces it, after it was rightly
  called generic. Squash them.)

  - **One typeface, flat panels on the warm ground, no gradients**: the
  palette is `src/ui/theme.rs`'s, and the tabs, the key chips on the
  bottom line and the bar on the hovered table row are drawn as the
  dashboard draws them.
  - **Pepe and the big digits are pixels**: Pepe is taken from the cells
  the dashboard draws him in, and `398`, `2.4`, `4.0` are set in
  `bigtext.rs`'s block face, both as inline SVG.
  - **Four of pepe's screens, in the page without a frame**: Run, Ramp,
  API and Logs, each with the command that makes it. The pictures have the
  page's own background, so they read as part of it.
  - **The keys work**: `1`-`4`, `tab` and the arrows switch screens; `i`
  goes to the install line and `c` copies it; `g`, `d`, `b` open GitHub,
  the docs and the benchmarks; `?` lists them.
  - **What it does** as a table of eleven commands, and **against the
  others** as bars: the README's Linux figures for pepe, wrk and oha, with
  a link to the benchmark notes for the method and for where pepe is level
  rather than ahead.
  - The install box as before (opens on the visitor's platform, copy
  button), now with a Docker tab.
  - Title, description, canonical, Open Graph and Twitter tags, JSON-LD,
  and `img/og.png` for links to unfold into.

  One static file, no build step, no dependencies, no tracking. It stacks
  down to phone widths, where the table drops its last column.

  ## The pictures

  `site/img/{run,ramp,api,logs}.png` are pepe's own drawing code, rendered
  to cells through the pepe theme (as the `preview` test in
  `src/ui/view.rs` does) and drawn at 2× by headless Chrome. The data is
  made up: the tests' sample run, a simulated ramp that saturates, ten
  invented endpoints, a generated nginx log.

  ## Deploy

  `publish-r2.yml` uploads `site/img/*.png` to `/img/` and checks they
  answer 200. The site goes out with the next release; `docs:` doesn't
  make one on its own.

  ## For the reviewer

  - **`logs.png` shows the dashboard from #105**, which is not merged. If
  #105 doesn't land, that picture wants replacing.
  - **`https://pepe.mhaimdat.com/` answers 404 today; only `/index.html`
  answers.** The bucket has no index for the root, and the repo's website
  link, the canonical and `og:url` all point at the root. It needs a rule
  on the Cloudflare side.
  - **A bug the API picture shows**: in the dashboard's first card,
  "requests / s" and "avg 249 · peak 268" overlap ("requests / savg") when
  the numbers are three digits. That is in `src/ui/view.rs`, not here.
  - The verdict panel beside the numbers is an example of the wording.
  - `og.png` is still the first design's (headline, command, the run
  screen); it suits either.
  - Single-key shortcuts are ignored while a modifier is held or a field
  has focus. `tab` switches screens only when nothing on the page has
  focus, so keyboard navigation of the links still works once you have
  tabbed in.

  ## Tested

  - Looked at in headless Chrome at 1400 and 520 px wide. Two things found
  that way and fixed: the hero Pepe had picked up the edge of the card
  beside him, and a table header wrapped.
  - The keys and the copy button are not exercised by anything automatic,
  and I have not pressed them in a real browser: headless Chrome only took
  pictures. Safari and Firefox are unchecked.


## [0.19.3](https://github.com/omarmhaimdat/pepe/compare/v0.19.2...v0.19.3) - 2026-10-09

### Fixed

- *(logs)* at a terminal a log being written is shown live, not read from its start ([#104](https://github.com/omarmhaimdat/pepe/pull/104))

  ## What

  `pepe logs` on a host read the whole log before the screen had anything
  to say about now, and the numbers on it were the log's history rather
  than the server's present. At a terminal, a log that is being written is
  now shown live.

  - **Live by default.** If the last line of any of the files is from the
  last five minutes, the screen starts five minutes back (or `--window`
  back, if that is longer) and follows from there. Five minutes rather
  than none, so that "now" is right from the first frame and the chart has
  bars in it. The title says where the counts start: `● live · from
  15:33:40`.
  - **`--since 24h`** starts further back, as before; **`--all`** (new)
  reads everything first. The two conflict.
  - **A log nobody is writing is read whole**, as before: it has no now,
  and starting five minutes before the clock would show nothing.
  - **Unchanged**: piped out or `--json` (a report of the whole log, or of
  `--since`), and what is piped in.

  ## How

  - `seek_since` finds where a time starts in a file by halving it, a log
  being in order of time, and reading 64 KB at each step. What it can't
  tell (no dated line in the 64 KB) it settles toward the top of the file,
  so more is read, never less. Lines before the time that are still read
  are left out by the filter that was already there.
  - This serves `--since` everywhere, not only the live default: on a
  two-day, 193 MB log `--since 1h` takes 0.03 s where the whole log takes
  0.11 s, and the gap grows with the file.
  - The multi-threaded read takes a place to start from, so `--since 30d`
  of a large log still uses every core.
  - `being_written` reads the last 64 KB of each file for its last
  timestamp.

  ## For the reviewer

  - This changes what `pepe logs access.log` shows at a terminal on a live
  host: the cards (a usual minute, the busiest) now describe the last
  minutes until the screen has been open longer, and "an hour ago" / "a
  day ago" are empty without `--since` or `--all`.
  - A file whose lines aren't in time order can be taken up at the wrong
  place; `--all` reads it whole.
  - The error log's times are this machine's, so on a machine in another
  zone than the server it can be judged written or not wrongly; `--since`
  and `--all` say it outright.
  - Titled `fix` so it is a patch release.

  ## Tested

  - New test: a six-hour log with undated lines, taken up 1 s, 5 min, 1 h
  and nearly 6 h back; the place is a line's start, at most 128 KB before
  the first line wanted, the counts are exactly the requests since, and
  every byte is accounted for in the progress. Also before the log's
  start, after its end, a missing file, and `being_written` either side of
  five minutes.
  - 247 tests pass; clippy 1.98 `-D warnings` and `cargo fmt --check`
  clean; man page and completions regenerated for `--all`.
  - By hand: a generated two-day log ending now, with its error log, on
  the screen in a pseudo-terminal: it opens five minutes back (11,058
  requests of 1.3M), shows `● live`, and counts lines appended while it
  runs. Reading a whole 3.45 GB log takes the same time as on master,
  within the noise of alternating runs.
  - Not run against a real nginx host, nor on Linux or Windows.


## [0.19.2](https://github.com/omarmhaimdat/pepe/compare/v0.19.1...v0.19.2) - 2026-10-09

### Fixed

- *(logs)* the screen survives what else is written to the terminal, and reads compose's colour ([#102](https://github.com/omarmhaimdat/pepe/pull/102))

  ## What

  `docker compose logs -f nginx | pepe logs` left the screen in ruins
  (doubled header rows, blank path names, shifted columns, `nginx_twitter`
  listed as a client). Two causes, both outside pepe's drawing code:

  - compose's **stderr** still points at the terminal; its `WARN[0000] …
  version is obsolete` line landed at the bottom row, where the hidden
  cursor sat, and scrolled the alternate screen up one row. ratatui only
  redraws what changed, so every later frame was one row off the truth.
  - compose **colours** the `nginx_twitter | ` prefix even into a pipe;
  the escapes went into the parse (client = container name) and into drawn
  cells, where the terminal interpreted them and shifted columns.

  ## Changes

  - **Lines are cleaned at ingest** (`logs::clean`): CSI/OSC escapes
  stripped, tabs → spaces, other control characters dropped. Nothing read
  from a log can move the cursor.
  - **The screen repairs itself**: the hidden cursor is parked at the
  top-left after every frame, so stray output overwrites a row rather than
  scrolling; and the whole frame is rewritten cell-for-cell every second
  (`REPAINT`), so damage heals. Verified against a fake coloured compose
  stream writing to stderr every few seconds.
  - **A pipe is caught up with from its first chunk** — it has no end to
  be short of. The title no longer says `reading` forever; an empty pipe
  says *Waiting for the first line*.
  - **Design**: number cards are fixed-width panels packed from the left
  (they were stretched across all 200 columns); chart bars cap at 4 wide
  with the picked one in the accent colour, and the pick's marker no
  longer stamps over the date label; the paths list is capped at 96
  columns so the numbers sit next to the names.
  - **CLI**: a bare word given as the URL (`pepe logs` on a 0.16 binary
  gave `Invalid URL "logs": relative URL without a base`) now says it
  isn't a URL nor a command *this* pepe has, lists the commands from clap,
  and points at `pepe self-update`; a host with no scheme is shown with
  `https://` in front.

  ## Notes for review

  - The full repaint is ~15 KB/s over SSH on a 200×60 terminal; `REPAINT`
  is one constant if that ever needs slowing.
  - `view::panel` and `view::inset` are now `pub(super)` so the logs
  screen shares the dashboard's cards.
  - Tests: colour/control stripping, the piped caught-up state, the
  empty-pipe wording, and the three URL messages. 246 pass; clippy and fmt
  clean.

  🤖 Generated with [Claude Code](https://claude.com/claude-code)


## [0.19.1](https://github.com/omarmhaimdat/pepe/compare/v0.19.0...v0.19.1) - 2026-10-08

### Fixed

- *(logs)* a log is read by every core, ten times as fast ([#100](https://github.com/omarmhaimdat/pepe/pull/100))

  ## What

  `pepe logs` read a file on one thread. What a file already has is now
  read by every core, and the per-line work is cheaper for every way of
  reading. No flag, no change to what is shown: the `--json` report is the
  same to the byte as before.

  | 862 MB, 5.86M lines, M4 Pro (10P + 4E) | time | lines/s | MB/s |
  memory |
  | --- | ---: | ---: | ---: | ---: |
  | master, a file | 1.99 s | 2.9M | 433 | 16 MB |
  | this PR, a file | 0.20 s | 29M | 4,300 | 119 MB |
  | master, piped in | 2.00 s | 2.9M | 431 | 16 MB |
  | this PR, piped in (one thread) | 1.54 s | 3.8M | 560 | 16 MB |
  | `wc -l`, for scale | 0.65 s | | 1,330 | |

  Best of five, file in the page cache. A 3.45 GB file takes 0.84 s.
  Details and the list of what was found are in `bench/README.md`.

  **This is ten times, not a hundred.** A hundred times the starting point
  would be 43 GB/s, which is more than this machine can copy out of the
  page cache, let alone parse. What is left is spread evenly over reading
  from the kernel, finding the fields and looking up three names a line.

  ## How

  - **Every core** (`sprint` in `src/logs.rs`): threads take the file a
  megabyte at a time with `pread`, count the lines that start in their
  megabyte into counts of their own, and add those to the shared ones once
  a second, so the screen fills in while a long log is read. The file's
  last partial megabyte, and everything appended after, goes to the one
  thread that follows the file, as before.
  - **Same result in any order** (`Stats::merge`): sums, minima and
  maxima. The one thing a cut can split is the count of the second it
  falls in, which the "busiest second" figures need whole; each stretch's
  first and last runs of one second are kept and put together again when
  merged. The first unread line and each error message's example are
  chosen by place in the file, and the last 2,000 lines by time and then
  place.
  - **A keyed hash in place of SipHash** for the maps of paths, clients,
  user agents and parameter names: eight bytes at a time through a folded
  multiply, under a key drawn from `RandomState` when pepe starts. The
  names come from whoever sends requests, which is why it is keyed.
  - **Less per line**: a request in the same second as the one before it
  counts into the slots already held, with no lookup; a timestamp written
  as the last one was isn't worked out again; a path with an id in it is
  rewritten into a kept buffer; lines are copied for the log view only
  within 32 MB of a file's end.

  ## For the reviewer

  - **Memory while a file is read goes from 16 MB to about 120 MB** with
  14 threads on this log (each thread's megabyte, and its own counts of up
  to 50,000 client addresses between merges). It doesn't grow with the
  file. Piped input is unchanged at 16 MB.
  - Files under 16 MB, stdin and followed appends take the single-threaded
  path.
  - Once a log has more distinct names than a cap (20,000 paths, 50,000
  clients), which names are kept past the cap can differ from run to run,
  since threads merge in no fixed order. Totals don't change.
  - A log whose lines aren't in time order can report a different busiest
  second per slot than one thread would: both are approximations there.
  - The hash is not SipHash. It is keyed per process, but it has had no
  cryptanalysis; if that trade isn't wanted, `Keyed` is one type to swap
  back.
  - `seek_read` is used on Windows in place of `pread`. That path is
  compiled and tested only by CI.
  - The title says `fix` so release-plz makes this a patch release; by the
  changelog's own groups it is `perf`.

  ## Tested

  - New: `every_core_reading_counts_what_one_would` reads one log (seconds
  of 0 to 40 requests, error log lines, unreadable lines, a line longer
  than a stretch, bytes that aren't UTF-8, an unfinished last line,
  `--since`) on one thread and then with stretches of 1 KB, 3 KB, 64 KB
  and 1 MB on 2 to 8 threads, and compares the JSON report, the kept
  lines, the busiest second and the first unread line.
  `names_are_found_by_a_keyed_hash` covers the hash and merging at a cap.
  - 245 tests pass; `cargo clippy --all-targets -- -D warnings` on 1.98
  and `cargo fmt --check` are clean.
  - The release binary's `--json` for the 862 MB access log plus its error
  log is identical (`cmp`) to master's.
  - The live screen was driven in a pseudo-terminal on the same files,
  with lines appended while it ran.
  - Not measured on Linux or Windows, and not against another log reader.


## [0.19.0](https://github.com/omarmhaimdat/pepe/compare/v0.18.0...v0.19.0) - 2026-10-08

### Added

- pepe logs, nginx's traffic now against each minute, hour and day ([#98](https://github.com/omarmhaimdat/pepe/pull/98))

  ## What

  A new subcommand, `pepe logs`, that reads nginx's access and error logs
  and says how busy the server is now against how busy it has been.

  ```bash
  pepe logs /var/log/nginx/access.log /var/log/nginx/error.log
  zcat access.log.*.gz | pepe logs -
  docker compose logs -f -n 1000 nginx | pepe logs
  pepe logs access.log --since 24h --json > traffic.json
  ```

  - **Now** is the request rate over the last minute (`--window`) of the
  log's own timestamps. A log whose last line is older than five minutes
  is held at its last line instead of the clock.
  - **Each minute, hour and day** has its requests, req/s, busiest second,
  4xx and 5xx shares, mean request time and error log lines, and how now
  compares (`+12%`, `×3.4`, `÷2.5`). Cards say what a usual slot sees (the
  median), which was the busiest, and what the same slot an hour, a day or
  a week ago saw.
  - **Read as a stream**: lines are folded into counts of a fixed size (an
  hour of seconds, a day of minutes, ninety days of hours, ten years of
  days; capped maps of paths, clients, user agents), so memory doesn't
  grow with the log.
  - **Four views**: Traffic (a bar per slot with the rate now drawn
  across, and the table; `m` `h` `d`), Paths (by requests, 5xx, 4xx or
  mean time, with statuses, clients, user agents, methods and query
  parameter names), Errors (the error log grouped by cause, and the paths
  answering 5xx and 4xx), Log (the last 2,000 lines of all files in time
  order; `x` errors only, `/` search, `enter` everything read from a
  line).
  - **Formats**: nginx `combined` (and `rt=`/`urt=` after it), a `--format
  '<log_format>'` as nginx.conf has it, JSON lines under nginx's or
  Caddy's names, and the error log. A line that fits none is still
  searched for a time, a request and a status; what can't be read is
  counted and the first such line shown.
  - At a terminal the files are followed, through rotation. Piped out, or
  with `--json`, they are read to the end and a report is printed. With no
  file named and nothing piped in, `/var/log/nginx/access.log` and
  `error.log` are read if they are there.

  ## Where

  - `src/logs.rs`: timestamps, the line readers, the counts, the file
  follower, the text and JSON reports.
  - `src/ui/logs.rs`: the screen.
  - `src/cli.rs`, `src/main.rs`: `LogsArgs` and `run_logs`.
  - README section "Reading nginx logs", a ROADMAP entry, `pepe-logs(1)`
  and the completions regenerated.

  No new dependency: dates are worked out by hand, and the local offset
  comes from `libc`, which is already there on Unix.

  ## For the reviewer

  - **Paths are grouped by default**: numbers, UUIDs and long hex ids in a
  path count as one (`/items/*`); `--exact-paths` turns that off.
  - **Piped input with the screen** (Unix): the pipe is put aside with
  `dup` and the terminal, opened by its own name from `ttyname_r(stdout)`,
  takes descriptor 0, so crossterm reads keys as usual. `/dev/tty` itself
  can't be polled on macOS. On Windows piped input gets the report, not
  the screen.
  - **The error log names no time zone**; its times are taken to be this
  machine's.
  - **No `.gz`**: that would need a dependency; `zcat … | pepe logs -`
  does it.
  - Only `/var/log/nginx/` is looked in by default; nginx.conf isn't read
  for other paths or for the `log_format`.
  - The global load flags (`-n`, `-c`, …) show in `pepe logs --help` and
  do nothing there, as with `completions` and `self-update`.

  ## Tested

  - 14 new tests (243 in all pass; `cargo clippy --all-targets -- -D
  warnings` and `cargo fmt --check` clean): every timestamp form, combined
  / custom format / JSON / error log lines, cause grouping, slot rates and
  now-versus, flat memory over three days of lines, files read oldest
  first, followed and reopened after rotation, and every view drawn at
  60×16 up to 200×60 with its keys.
  - By hand on a generated three-day log (1.96M lines, 288 MB, plus an
  error log): read in about 1.4 s with 17 MB resident on an M-series Mac.
  This is one run, not a `bench/` record.
  - The live screen was driven in a pseudo-terminal, with lines appended
  to the file and with input piped in the shape `docker compose logs`
  gives. It has not been run against a real nginx or a real Docker
  container, nor on Linux or Windows.


## [0.18.0](https://github.com/omarmhaimdat/pepe/compare/v0.17.1...v0.18.0) - 2026-10-08

### Added

- *(ui)* pepe's own theme, a new Pepe, and the dashboard on panels ([#96](https://github.com/omarmhaimdat/pepe/pull/96))

  ## What

  - **pepe's own palette**, the default in true-colour terminals: warm
  darks on a painted ground, one ember accent, a heatmap that glows from
  embers to flame. `PEPE_THEME=pepe|terminal|light|none` picks a theme;
  otherwise `COLORTERM` (`truecolor`, `24bit`) chooses pepe's palette and
  `COLORFGBG` still picks light. `NO_COLOR` wins over all.
  - **A new Pepe**, with arms that pose with his mood, in two sizes: 16×16
  everywhere, 26×26 in the big header. The update notice prints
  true-colour cells too.
  - **The dashboard on panels.** On terminals 146×46 and up the header has
  the big Pepe, the four numbers on cards in a new 5×7 face, and a run
  panel: progress, where p99 sits on a heat scale from bell to ghost, and
  what the run says so far. The Live charts, latest requests, errors,
  stats column and each Stats card sit on panels; outside pepe's palette
  panels keep only their spacing.
  - An ignored test, `preview`, writes each tab as HTML for screenshots.

  ## Review fixes

  - The big header started at 120 columns, where the run panel had 14
  cells and its text collided; it now starts at 146, and the size sweep
  renders 120×46 and 146×46.
  - The p99 heat is coloured by where it sits rather than always yellow.
  - `inspector_says_the_list_is_paused_while_live` failed with
  `COLORTERM=truecolor`; it no longer depends on the theme.
  - Unreachable big-mascot branches in the small header are removed.
  - `load`: the shard-peak test reads the peak once the first shard's
  early requests are answered.

  ## Testing

  - `cargo fmt --check`, `cargo clippy --locked --all-targets -- -D
  warnings`
  - `cargo test` (230 pass) with `COLORTERM=truecolor`, `COLORTERM` unset,
  `PEPE_THEME=light` and `NO_COLOR=1`
  - `preview` rendered at 146×46 in pepe's theme and the run panel checked

  🤖 Generated with [Claude Code](https://claude.com/claude-code)

  ---------


## [0.17.1](https://github.com/omarmhaimdat/pepe/compare/v0.17.0...v0.17.1) - 2026-10-08

### Fixed

- *(deps)* bump toml from 0.8.23 to 0.9.6 ([#89](https://github.com/omarmhaimdat/pepe/pull/89))

  Bumps [toml](https://github.com/toml-rs/toml) from 0.8.23 to 0.9.6.
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/toml-rs/toml/commit/4695fb02fc3902345ffbfb54fd5df6adcc3bbd4d"><code>4695fb0</code></a>
  chore: Release</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/6a77ed71cf68369e823f7827b34eaa2a06d0126d"><code>6a77ed7</code></a>
  docs: Update changelog</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/c1e81979644a7a80141ab2d0ca284a4560eb4079"><code>c1e8197</code></a>
  refactor: Switch serde dependency to serde_core (<a
  href="https://redirect.github.com/toml-rs/toml/issues/1036">#1036</a>)</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/d85d6cd61cf122ee44db8834bc2a55e881bb0750"><code>d85d6cd</code></a>
  refactor: Switch serde dependency to serde_core</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/9154dcb3b2eea8a84db183806411adf081bc0977"><code>9154dcb</code></a>
  chore: Release</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/38f445c94071cfc0bbb2f4a3c0254457a3fde8cb"><code>38f445c</code></a>
  docs: Update changelog</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/1ce8a75f2d5faa778deb43e9abd3960247f0d5b2"><code>1ce8a75</code></a>
  feat(edit): Expose Table::span (<a
  href="https://redirect.github.com/toml-rs/toml/issues/1031">#1031</a>)</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/290c28fa6078b2e61b897fe7a71afc26c70daa76"><code>290c28f</code></a>
  feat(edit): Expose Table::span</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/b2bc739b201d14ed0dafabf4784bb56f9318c5be"><code>b2bc739</code></a>
  chore(deps): Update Rust Stable to v1.89 (<a
  href="https://redirect.github.com/toml-rs/toml/issues/1026">#1026</a>)</li>
  <li><a
  href="https://github.com/toml-rs/toml/commit/bd21148c49c784cb9136e5d069471dfeae13a339"><code>bd21148</code></a>
  chore: Release</li>
  <li>Additional commits viewable in <a
  href="https://github.com/toml-rs/toml/compare/toml-v0.8.23...toml-v0.9.6">compare
  view</a></li>
  </ul>
  </details>
  <br />



### Other

- bump the github-actions group across 1 directory with 5 updates ([#94](https://github.com/omarmhaimdat/pepe/pull/94))

  Bumps the github-actions group with 5 updates in the / directory:

  | Package | From | To |
  | --- | --- | --- |
  | [actions/checkout](https://github.com/actions/checkout) | `6` | `7` |
  |
  [docker/setup-buildx-action](https://github.com/docker/setup-buildx-action)
  | `3` | `4` |
  |
  [docker/build-push-action](https://github.com/docker/build-push-action)
  | `6` | `7` |
  |
  [docker/setup-qemu-action](https://github.com/docker/setup-qemu-action)
  | `3` | `4` |
  | [docker/login-action](https://github.com/docker/login-action) | `3` |
  `4` |


  Updates `actions/checkout` from 6 to 7
  <details>
  <summary>Release notes</summary>
  <p><em>Sourced from <a
  href="https://github.com/actions/checkout/releases">actions/checkout's
  releases</a>.</em></p>
  <blockquote>
  <h2>v7.0.0</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>block checking out fork pr for pull_request_target and workflow_run
  by <a href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2454">actions/checkout#2454</a></li>
  <li>Bump actions/publish-immutable-action from 0.0.3 to 0.0.4 in the
  minor-actions-dependencies group across 1 directory by <a
  href="https://github.com/dependabot"><code>@​dependabot</code></a>[bot]
  in <a
  href="https://redirect.github.com/actions/checkout/pull/2458">actions/checkout#2458</a></li>
  <li>Bump flatted from 3.3.1 to 3.4.2 by <a
  href="https://github.com/dependabot"><code>@​dependabot</code></a>[bot]
  in <a
  href="https://redirect.github.com/actions/checkout/pull/2460">actions/checkout#2460</a></li>
  <li>Bump js-yaml from 4.1.0 to 4.2.0 by <a
  href="https://github.com/dependabot"><code>@​dependabot</code></a>[bot]
  in <a
  href="https://redirect.github.com/actions/checkout/pull/2461">actions/checkout#2461</a></li>
  <li>Bump <code>@​actions/core</code> and
  <code>@​actions/tool-cache</code> and Remove uuid by <a
  href="https://github.com/dependabot"><code>@​dependabot</code></a>[bot]
  in <a
  href="https://redirect.github.com/actions/checkout/pull/2459">actions/checkout#2459</a></li>
  <li>upgrade module to esm and update dependencies by <a
  href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2463">actions/checkout#2463</a></li>
  <li>Bump the minor-npm-dependencies group across 1 directory with 3
  updates by <a
  href="https://github.com/dependabot"><code>@​dependabot</code></a>[bot]
  in <a
  href="https://redirect.github.com/actions/checkout/pull/2462">actions/checkout#2462</a></li>
  <li>getting ready for checkout v7 release by <a
  href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2464">actions/checkout#2464</a></li>
  <li>update error wording by <a
  href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2467">actions/checkout#2467</a></li>
  </ul>
  <h2>New Contributors</h2>
  <ul>
  <li><a href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> made
  their first contribution in <a
  href="https://redirect.github.com/actions/checkout/pull/2454">actions/checkout#2454</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/checkout/compare/v6.0.3...v7.0.0">https://github.com/actions/checkout/compare/v6.0.3...v7.0.0</a></p>
  <h2>v6.1.0</h2>
  <h2>What's Changed</h2>
  <ul>
  <li><strong>[BREAKING]</strong> backport
  <code>allow-unsafe-pr-checkout</code> to v6 by <a
  href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2500">actions/checkout#2500</a></li>
  <li>backport fixes to releases-v6 by <a
  href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2527">actions/checkout#2527</a></li>
  </ul>
  <p><a
  href="https://github.blog/changelog/2026-06-18-safer-pull_request_target-defaults-for-github-actions-checkout/">https://github.blog/changelog/2026-06-18-safer-pull_request_target-defaults-for-github-actions-checkout/</a>
  for more details about this breaking change</p>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/checkout/compare/v6.0.3...v6.1.0">https://github.com/actions/checkout/compare/v6.0.3...v6.1.0</a></p>
  <h2>v6.0.3</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>Update changelog by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2357">actions/checkout#2357</a></li>
  <li>fix: expand merge commit SHA regex and add SHA-256 test cases by <a
  href="https://github.com/yaananth"><code>@​yaananth</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2414">actions/checkout#2414</a></li>
  <li>Fix checkout init for SHA-256 repositories by <a
  href="https://github.com/yaananth"><code>@​yaananth</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2439">actions/checkout#2439</a></li>
  <li>Update changelog for v6.0.3 by <a
  href="https://github.com/yaananth"><code>@​yaananth</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2446">actions/checkout#2446</a></li>
  </ul>
  <h2>New Contributors</h2>
  <ul>
  <li><a href="https://github.com/yaananth"><code>@​yaananth</code></a>
  made their first contribution in <a
  href="https://redirect.github.com/actions/checkout/pull/2414">actions/checkout#2414</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/checkout/compare/v6...v6.0.3">https://github.com/actions/checkout/compare/v6...v6.0.3</a></p>
  <h2>v6.0.2</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>Add orchestration_id to git user-agent when ACTIONS_ORCHESTRATION_ID
  is set by <a
  href="https://github.com/TingluoHuang"><code>@​TingluoHuang</code></a>
  in <a
  href="https://redirect.github.com/actions/checkout/pull/2355">actions/checkout#2355</a></li>
  <li>Fix tag handling: preserve annotations and explicit fetch-tags by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2356">actions/checkout#2356</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/actions/checkout/compare/v6.0.1...v6.0.2">https://github.com/actions/checkout/compare/v6.0.1...v6.0.2</a></p>
  <h2>v6.0.1</h2>
  <h2>What's Changed</h2>
  <ul>
  <li>Update all references from v5 and v4 to v6 by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2314">actions/checkout#2314</a></li>
  <li>Add worktree support for persist-credentials includeIf by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2327">actions/checkout#2327</a></li>
  <li>Clarify v6 README by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2328">actions/checkout#2328</a></li>
  </ul>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Changelog</summary>
  <p><em>Sourced from <a
  href="https://github.com/actions/checkout/blob/main/CHANGELOG.md">actions/checkout's
  changelog</a>.</em></p>
  <blockquote>
  <h1>Changelog</h1>
  <h2>v7.0.1</h2>
  <ul>
  <li>Skip running unsafe pr check if input is default by <a
  href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2518">actions/checkout#2518</a></li>
  <li>Trim only ascii whitespace for branch by <a
  href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2521">actions/checkout#2521</a></li>
  <li>Escape values passed to --unset by <a
  href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2530">actions/checkout#2530</a></li>
  <li>Various dependency updates</li>
  </ul>
  <h2>v7.0.0</h2>
  <ul>
  <li>Block checking out fork PR for pull_request_target and workflow_run
  by <a href="https://github.com/aiqiaoy"><code>@​aiqiaoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2454">actions/checkout#2454</a></li>
  <li>Various dependency updates</li>
  </ul>
  <h2>v6.0.3</h2>
  <ul>
  <li>Fix checkout init for SHA-256 repositories by <a
  href="https://github.com/yaananth"><code>@​yaananth</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2439">actions/checkout#2439</a></li>
  <li>fix: expand merge commit SHA regex and add SHA-256 test cases by <a
  href="https://github.com/yaananth"><code>@​yaananth</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2414">actions/checkout#2414</a></li>
  </ul>
  <h2>v6.0.2</h2>
  <ul>
  <li>Fix tag handling: preserve annotations and explicit fetch-tags by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2356">actions/checkout#2356</a></li>
  </ul>
  <h2>v6.0.1</h2>
  <ul>
  <li>Add worktree support for persist-credentials includeIf by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2327">actions/checkout#2327</a></li>
  </ul>
  <h2>v6.0.0</h2>
  <ul>
  <li>Persist creds to a separate file by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2286">actions/checkout#2286</a></li>
  <li>Update README to include Node.js 24 support details and requirements
  by <a href="https://github.com/salmanmkc"><code>@​salmanmkc</code></a>
  in <a
  href="https://redirect.github.com/actions/checkout/pull/2248">actions/checkout#2248</a></li>
  </ul>
  <h2>v5.0.1</h2>
  <ul>
  <li>Port v6 cleanup to v5 by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2301">actions/checkout#2301</a></li>
  </ul>
  <h2>v5.0.0</h2>
  <ul>
  <li>Update actions checkout to use node 24 by <a
  href="https://github.com/salmanmkc"><code>@​salmanmkc</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2226">actions/checkout#2226</a></li>
  </ul>
  <h2>v4.3.1</h2>
  <ul>
  <li>Port v6 cleanup to v4 by <a
  href="https://github.com/ericsciple"><code>@​ericsciple</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2305">actions/checkout#2305</a></li>
  </ul>
  <h2>v4.3.0</h2>
  <ul>
  <li>docs: update README.md by <a
  href="https://github.com/motss"><code>@​motss</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/1971">actions/checkout#1971</a></li>
  <li>Add internal repos for checking out multiple repositories by <a
  href="https://github.com/mouismail"><code>@​mouismail</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/1977">actions/checkout#1977</a></li>
  <li>Documentation update - add recommended permissions to Readme by <a
  href="https://github.com/benwells"><code>@​benwells</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2043">actions/checkout#2043</a></li>
  <li>Adjust positioning of user email note and permissions heading by <a
  href="https://github.com/joshmgross"><code>@​joshmgross</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2044">actions/checkout#2044</a></li>
  <li>Update README.md by <a
  href="https://github.com/nebuk89"><code>@​nebuk89</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2194">actions/checkout#2194</a></li>
  <li>Update CODEOWNERS for actions by <a
  href="https://github.com/TingluoHuang"><code>@​TingluoHuang</code></a>
  in <a
  href="https://redirect.github.com/actions/checkout/pull/2224">actions/checkout#2224</a></li>
  <li>Update package dependencies by <a
  href="https://github.com/salmanmkc"><code>@​salmanmkc</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/2236">actions/checkout#2236</a></li>
  </ul>
  <h2>v4.2.2</h2>
  <ul>
  <li><code>url-helper.ts</code> now leverages well-known environment
  variables by <a href="https://github.com/jww3"><code>@​jww3</code></a>
  in <a
  href="https://redirect.github.com/actions/checkout/pull/1941">actions/checkout#1941</a></li>
  <li>Expand unit test coverage for <code>isGhes</code> by <a
  href="https://github.com/jww3"><code>@​jww3</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/1946">actions/checkout#1946</a></li>
  </ul>
  <h2>v4.2.1</h2>
  <ul>
  <li>Check out other refs/* by commit if provided, fall back to ref by <a
  href="https://github.com/orhantoy"><code>@​orhantoy</code></a> in <a
  href="https://redirect.github.com/actions/checkout/pull/1924">actions/checkout#1924</a></li>
  </ul>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/actions/checkout/commit/3d3c42e5aac5ba805825da76410c181273ba90b1"><code>3d3c42e</code></a>
  prep v7.0.1 release (<a
  href="https://redirect.github.com/actions/checkout/issues/2531">#2531</a>)</li>
  <li><a
  href="https://github.com/actions/checkout/commit/28802689a136bfcdb721715abd713740beecbe07"><code>2880268</code></a>
  escape values passed to --unset (<a
  href="https://redirect.github.com/actions/checkout/issues/2530">#2530</a>)</li>
  <li><a
  href="https://github.com/actions/checkout/commit/12cd2235efa0937479335606d7c3ac9f6c0973b1"><code>12cd223</code></a>
  trim only ascii whitespace for branch (<a
  href="https://redirect.github.com/actions/checkout/issues/2521">#2521</a>)</li>
  <li><a
  href="https://github.com/actions/checkout/commit/62661c4e71a304b2823ed026347b8d34c3eac541"><code>62661c4</code></a>
  skip running unsafe pr check if input is default (<a
  href="https://redirect.github.com/actions/checkout/issues/2518">#2518</a>)</li>
  <li><a
  href="https://github.com/actions/checkout/commit/e8d4307400f9427dba7cb98e488d6ab85f1cec5f"><code>e8d4307</code></a>
  Bump the minor-actions-dependencies group with 2 updates (<a
  href="https://redirect.github.com/actions/checkout/issues/2499">#2499</a>)</li>
  <li><a
  href="https://github.com/actions/checkout/commit/631c942040754b6e095e929c1677c07e10ed4f87"><code>631c942</code></a>
  eslint 9 (<a
  href="https://redirect.github.com/actions/checkout/issues/2474">#2474</a>)</li>
  <li><a
  href="https://github.com/actions/checkout/commit/4f1f4aec02e41874fa0262ea8ff5172d7978ad1e"><code>4f1f4ae</code></a>
  Bump actions/upload-artifact from 4 to 7 (<a
  href="https://redirect.github.com/actions/checkout/issues/2476">#2476</a>)</li>
  <li><a
  href="https://github.com/actions/checkout/commit/ba097532fb203f7e88c9c3c0b899b49469908a92"><code>ba09753</code></a>
  Bump actions/checkout from 6 to 7 (<a
  href="https://redirect.github.com/actions/checkout/issues/2488">#2488</a>)</li>
  <li><a
  href="https://github.com/actions/checkout/commit/b9e0990d219a03df7633c93f6f005a8fecbcab22"><code>b9e0990</code></a>
  Bump docker/login-action from 3.3.0 to 4.2.0 (<a
  href="https://redirect.github.com/actions/checkout/issues/2479">#2479</a>)</li>
  <li><a
  href="https://github.com/actions/checkout/commit/e8cb398be4a550817e382abf69e4c12c76fce1f2"><code>e8cb398</code></a>
  Bump docker/build-push-action from 6.5.0 to 7.2.0 (<a
  href="https://redirect.github.com/actions/checkout/issues/2478">#2478</a>)</li>
  <li>Additional commits viewable in <a
  href="https://github.com/actions/checkout/compare/v6...v7">compare
  view</a></li>
  </ul>
  </details>
  <br />

  Updates `docker/setup-buildx-action` from 3 to 4
  <details>
  <summary>Release notes</summary>
  <p><em>Sourced from <a
  href="https://github.com/docker/setup-buildx-action/releases">docker/setup-buildx-action's
  releases</a>.</em></p>
  <blockquote>
  <h2>v4.0.0</h2>
  <ul>
  <li>Node 24 as default runtime (requires <a
  href="https://github.com/actions/runner/releases/tag/v2.327.1">Actions
  Runner v2.327.1</a> or later) by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/483">docker/setup-buildx-action#483</a></li>
  <li>Remove deprecated inputs/outputs by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/464">docker/setup-buildx-action#464</a></li>
  <li>Switch to ESM and update config/test wiring by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/481">docker/setup-buildx-action#481</a></li>
  <li>Bump <code>@​actions/core</code> from 1.11.1 to 3.0.0 in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/475">docker/setup-buildx-action#475</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.63.0 to 0.79.0 in
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/482">docker/setup-buildx-action#482</a>
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/485">docker/setup-buildx-action#485</a></li>
  <li>Bump js-yaml from 4.1.0 to 4.1.1 in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/452">docker/setup-buildx-action#452</a></li>
  <li>Bump lodash from 4.17.21 to 4.17.23 in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/472">docker/setup-buildx-action#472</a></li>
  <li>Bump minimatch from 3.1.2 to 3.1.5 in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/480">docker/setup-buildx-action#480</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-buildx-action/compare/v3.12.0...v4.0.0">https://github.com/docker/setup-buildx-action/compare/v3.12.0...v4.0.0</a></p>
  <h2>v3.12.0</h2>
  <ul>
  <li>Deprecate <code>install</code> input by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/455">docker/setup-buildx-action#455</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.62.1 to 0.63.0 in
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/434">docker/setup-buildx-action#434</a></li>
  <li>Bump brace-expansion from 1.1.11 to 1.1.12 in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/436">docker/setup-buildx-action#436</a></li>
  <li>Bump form-data from 2.5.1 to 2.5.5 in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/432">docker/setup-buildx-action#432</a></li>
  <li>Bump undici from 5.28.4 to 5.29.0 in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/435">docker/setup-buildx-action#435</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-buildx-action/compare/v3.11.1...v3.12.0">https://github.com/docker/setup-buildx-action/compare/v3.11.1...v3.12.0</a></p>
  <h2>v3.11.1</h2>
  <ul>
  <li>Fix <code>keep-state</code> not being respected by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/429">docker/setup-buildx-action#429</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-buildx-action/compare/v3.11.0...v3.11.1">https://github.com/docker/setup-buildx-action/compare/v3.11.0...v3.11.1</a></p>
  <h2>v3.11.0</h2>
  <ul>
  <li>Keep BuildKit state support by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/427">docker/setup-buildx-action#427</a></li>
  <li>Remove aliases created when installing by default by <a
  href="https://github.com/hashhar"><code>@​hashhar</code></a> in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/139">docker/setup-buildx-action#139</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.56.0 to 0.62.1 in
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/422">docker/setup-buildx-action#422</a>
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/425">docker/setup-buildx-action#425</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-buildx-action/compare/v3.10.0...v3.11.0">https://github.com/docker/setup-buildx-action/compare/v3.10.0...v3.11.0</a></p>
  <h2>v3.10.0</h2>
  <ul>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.54.0 to 0.56.0 in
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/408">docker/setup-buildx-action#408</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-buildx-action/compare/v3.9.0...v3.10.0">https://github.com/docker/setup-buildx-action/compare/v3.9.0...v3.10.0</a></p>
  <h2>v3.9.0</h2>
  <ul>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.48.0 to 0.54.0 in
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/402">docker/setup-buildx-action#402</a>
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/404">docker/setup-buildx-action#404</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-buildx-action/compare/v3.8.0...v3.9.0">https://github.com/docker/setup-buildx-action/compare/v3.8.0...v3.9.0</a></p>
  <h2>v3.8.0</h2>
  <ul>
  <li>Make cloud prefix optional to download buildx if driver is cloud by
  <a href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/390">docker/setup-buildx-action#390</a></li>
  <li>Bump <code>@​actions/core</code> from 1.10.1 to 1.11.1 in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/370">docker/setup-buildx-action#370</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.39.0 to 0.48.0 in
  <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/389">docker/setup-buildx-action#389</a></li>
  <li>Bump cross-spawn from 7.0.3 to 7.0.6 in <a
  href="https://redirect.github.com/docker/setup-buildx-action/pull/382">docker/setup-buildx-action#382</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-buildx-action/compare/v3.7.1...v3.8.0">https://github.com/docker/setup-buildx-action/compare/v3.7.1...v3.8.0</a></p>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/f87e5991a6d7451dcb8d9637bfbc97413f497069"><code>f87e599</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-buildx-action/issues/624">#624</a>
  from crazy-max/skip-pull-with-endpoint</li>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/e7002743e035c0054da46ca559364576b2fce022"><code>e700274</code></a>
  chore: update generated content</li>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/3061c919c67ba542099ba309c9181d1900cecc07"><code>3061c91</code></a>
  skip BuildKit image pre-pulls for explicit endpoints</li>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/594f3bf4285d9ea8dc53c9a0c9c4092420091003"><code>594f3bf</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-buildx-action/issues/609">#609</a>
  from crazy-max/pull-buildkit-image-before-create</li>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/bd6e702fc33b636671900d5b5edfab64698c9c25"><code>bd6e702</code></a>
  chore: update generated content</li>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/6268c9da9abbd1309c8a16a75f92a878715c3032"><code>6268c9d</code></a>
  pull BuildKit image before builder creation</li>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/e8235251b82e23c90e6fad50016f0a78b7f28f11"><code>e823525</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-buildx-action/issues/621">#621</a>
  from docker/dependabot/github_actions/codeql-actions-...</li>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/533ed8ed095b0b133ef16fb495aad119524e220d"><code>533ed8e</code></a>
  build(deps): bump the codeql-actions group with 2 updates</li>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/bedaf135699075c88620cd30772b9b6eadc9ba99"><code>bedaf13</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-buildx-action/issues/620">#620</a>
  from crazy-max/shared-error-helpers</li>
  <li><a
  href="https://github.com/docker/setup-buildx-action/commit/d5079fba84d5edd23d25ba7f3045122175ca6ee2"><code>d5079fb</code></a>
  chore: update generated content</li>
  <li>Additional commits viewable in <a
  href="https://github.com/docker/setup-buildx-action/compare/v3...v4">compare
  view</a></li>
  </ul>
  </details>
  <br />

  Updates `docker/build-push-action` from 6 to 7
  <details>
  <summary>Release notes</summary>
  <p><em>Sourced from <a
  href="https://github.com/docker/build-push-action/releases">docker/build-push-action's
  releases</a>.</em></p>
  <blockquote>
  <h2>v7.0.0</h2>
  <ul>
  <li>Node 24 as default runtime (requires <a
  href="https://github.com/actions/runner/releases/tag/v2.327.1">Actions
  Runner v2.327.1</a> or later) by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1470">docker/build-push-action#1470</a></li>
  <li>Remove deprecated <code>DOCKER_BUILD_NO_SUMMARY</code> and
  <code>DOCKER_BUILD_EXPORT_RETENTION_DAYS</code> envs by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1473">docker/build-push-action#1473</a></li>
  <li>Remove legacy export-build tool support for build summary by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1474">docker/build-push-action#1474</a></li>
  <li>Switch to ESM and update config/test wiring by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1466">docker/build-push-action#1466</a></li>
  <li>Bump <code>@​actions/core</code> from 1.11.1 to 3.0.0 in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1454">docker/build-push-action#1454</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.62.1 to 0.79.0 in
  <a
  href="https://redirect.github.com/docker/build-push-action/pull/1453">docker/build-push-action#1453</a>
  <a
  href="https://redirect.github.com/docker/build-push-action/pull/1472">docker/build-push-action#1472</a>
  <a
  href="https://redirect.github.com/docker/build-push-action/pull/1479">docker/build-push-action#1479</a></li>
  <li>Bump minimatch from 3.1.2 to 3.1.5 in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1463">docker/build-push-action#1463</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/build-push-action/compare/v6.19.2...v7.0.0">https://github.com/docker/build-push-action/compare/v6.19.2...v7.0.0</a></p>
  <h2>v6.19.2</h2>
  <ul>
  <li>Preserve port in <code>GIT_AUTH_TOKEN</code> host by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1458">docker/build-push-action#1458</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/build-push-action/compare/v6.19.1...v6.19.2">https://github.com/docker/build-push-action/compare/v6.19.1...v6.19.2</a></p>
  <h2>v6.19.1</h2>
  <ul>
  <li>Derive <code>GIT_AUTH_TOKEN</code> host from GitHub server URL by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1456">docker/build-push-action#1456</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/build-push-action/compare/v6.19.0...v6.19.1">https://github.com/docker/build-push-action/compare/v6.19.0...v6.19.1</a></p>
  <h2>v6.19.0</h2>
  <ul>
  <li>Scope default git auth token to <code>github.com</code> by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1451">docker/build-push-action#1451</a></li>
  <li>Bump brace-expansion from 1.1.11 to 1.1.12 in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1396">docker/build-push-action#1396</a></li>
  <li>Bump form-data from 2.5.1 to 2.5.5 in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1391">docker/build-push-action#1391</a></li>
  <li>Bump js-yaml from 3.14.1 to 3.14.2 in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1429">docker/build-push-action#1429</a></li>
  <li>Bump lodash from 4.17.21 to 4.17.23 in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1446">docker/build-push-action#1446</a></li>
  <li>Bump tmp from 0.2.3 to 0.2.4 in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1398">docker/build-push-action#1398</a></li>
  <li>Bump undici from 5.28.4 to 5.29.0 in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1397">docker/build-push-action#1397</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/build-push-action/compare/v6.18.0...v6.19.0">https://github.com/docker/build-push-action/compare/v6.18.0...v6.19.0</a></p>
  <h2>v6.18.0</h2>
  <ul>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.61.0 to 0.62.1 in
  <a
  href="https://redirect.github.com/docker/build-push-action/pull/1381">docker/build-push-action#1381</a></li>
  </ul>
  <blockquote>
  <p>[!NOTE]
  <a
  href="https://docs.docker.com/build/ci/github-actions/build-summary/">Build
  summary</a> is now supported with <a
  href="https://docs.docker.com/build-cloud/">Docker Build Cloud</a>.</p>
  </blockquote>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/build-push-action/compare/v6.17.0...v6.18.0">https://github.com/docker/build-push-action/compare/v6.17.0...v6.18.0</a></p>
  <h2>v6.17.0</h2>
  <ul>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.59.0 to 0.61.0 by
  <a href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in
  <a
  href="https://redirect.github.com/docker/build-push-action/pull/1364">docker/build-push-action#1364</a></li>
  </ul>
  <blockquote>
  <p>[!NOTE]
  Build record is now exported using the <a
  href="https://docs.docker.com/reference/cli/docker/buildx/history/export/"><code>buildx
  history export</code></a> command instead of the legacy export-build
  tool.</p>
  </blockquote>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/build-push-action/compare/v6.16.0...v6.17.0">https://github.com/docker/build-push-action/compare/v6.16.0...v6.17.0</a></p>
  <h2>v6.16.0</h2>
  <ul>
  <li>Handle no default attestations env var by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/build-push-action/pull/1343">docker/build-push-action#1343</a></li>
  </ul>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/docker/build-push-action/commit/c3c9e263c25d99ce0380d002d59b67737d91b0dc"><code>c3c9e26</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/build-push-action/issues/1621">#1621</a>
  from docker/dependabot/npm_and_yarn/docker/actions-t...</li>
  <li><a
  href="https://github.com/docker/build-push-action/commit/459b6741834dcd35f946352017e7675bd2089d42"><code>459b674</code></a>
  [dependabot skip] chore: update generated content</li>
  <li><a
  href="https://github.com/docker/build-push-action/commit/4dedcb23c91d79c1629bf53ec2c3bcfffef5b34e"><code>4dedcb2</code></a>
  chore(deps): Bump <code>@​docker/actions-toolkit</code> from 0.99.0 to
  0.100.0</li>
  <li><a
  href="https://github.com/docker/build-push-action/commit/379bf63a979bd70751945601fa04c50674509952"><code>379bf63</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/build-push-action/issues/1620">#1620</a>
  from crazy-max/buildx-error-message</li>
  <li><a
  href="https://github.com/docker/build-push-action/commit/9877975c9e0b0b661592ff61049069507f9bc2f6"><code>9877975</code></a>
  chore: update generated content</li>
  <li><a
  href="https://github.com/docker/build-push-action/commit/7ed0556ffafb8eb312463411ef0a84a1dfe24d94"><code>7ed0556</code></a>
  use the shared Buildx error summary helper</li>
  <li><a
  href="https://github.com/docker/build-push-action/commit/91670ba5a4df99a24efff8637a78c83fd1b0f6b1"><code>91670ba</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/build-push-action/issues/1618">#1618</a>
  from docker/dependabot/npm_and_yarn/docker/actions-t...</li>
  <li><a
  href="https://github.com/docker/build-push-action/commit/80dbc8614a5c0ce4356740f69179cf829ecdc79a"><code>80dbc86</code></a>
  [dependabot skip] chore: update generated content</li>
  <li><a
  href="https://github.com/docker/build-push-action/commit/50cac3a3b6f55e6015d6483d1dd72a3ecb90d20d"><code>50cac3a</code></a>
  chore(deps): Bump <code>@​docker/actions-toolkit</code> from 0.98.0 to
  0.99.0</li>
  <li><a
  href="https://github.com/docker/build-push-action/commit/03b4d6cac0163b44733e1fa60adfd6da560ee4d1"><code>03b4d6c</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/build-push-action/issues/1617">#1617</a>
  from crazy-max/fix-metadata-workflow-commands</li>
  <li>Additional commits viewable in <a
  href="https://github.com/docker/build-push-action/compare/v6...v7">compare
  view</a></li>
  </ul>
  </details>
  <br />

  Updates `docker/setup-qemu-action` from 3 to 4
  <details>
  <summary>Release notes</summary>
  <p><em>Sourced from <a
  href="https://github.com/docker/setup-qemu-action/releases">docker/setup-qemu-action's
  releases</a>.</em></p>
  <blockquote>
  <h2>v4.0.0</h2>
  <ul>
  <li>Node 24 as default runtime (requires <a
  href="https://github.com/actions/runner/releases/tag/v2.327.1">Actions
  Runner v2.327.1</a> or later) by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/245">docker/setup-qemu-action#245</a></li>
  <li>Switch to ESM and update config/test wiring by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/241">docker/setup-qemu-action#241</a></li>
  <li>Bump <code>@​actions/core</code> from 1.11.1 to 3.0.0 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/244">docker/setup-qemu-action#244</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.67.0 to 0.77.0 in
  <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/243">docker/setup-qemu-action#243</a></li>
  <li>Bump <code>@​isaacs/brace-expansion</code> from 5.0.0 to 5.0.1 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/240">docker/setup-qemu-action#240</a></li>
  <li>Bump js-yaml from 3.14.1 to 3.14.2 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/231">docker/setup-qemu-action#231</a></li>
  <li>Bump lodash from 4.17.21 to 4.17.23 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/238">docker/setup-qemu-action#238</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-qemu-action/compare/v3.7.0...v4.0.0">https://github.com/docker/setup-qemu-action/compare/v3.7.0...v4.0.0</a></p>
  <h2>v3.7.0</h2>
  <ul>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.56.0 to 0.67.0 in
  <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/217">docker/setup-qemu-action#217</a>
  <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/230">docker/setup-qemu-action#230</a></li>
  <li>Bump brace-expansion from 1.1.11 to 1.1.12 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/220">docker/setup-qemu-action#220</a></li>
  <li>Bump form-data from 2.5.1 to 2.5.5 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/218">docker/setup-qemu-action#218</a></li>
  <li>Bump tmp from 0.2.3 to 0.2.4 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/221">docker/setup-qemu-action#221</a></li>
  <li>Bump undici from 5.28.4 to 5.29.0 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/219">docker/setup-qemu-action#219</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-qemu-action/compare/v3.6.0...v3.7.0">https://github.com/docker/setup-qemu-action/compare/v3.6.0...v3.7.0</a></p>
  <h2>v3.6.0</h2>
  <ul>
  <li>Display binfmt version by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/202">docker/setup-qemu-action#202</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-qemu-action/compare/v3.5.0...v3.6.0">https://github.com/docker/setup-qemu-action/compare/v3.5.0...v3.6.0</a></p>
  <h2>v3.5.0</h2>
  <ul>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.54.0 to 0.56.0 in
  <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/205">docker/setup-qemu-action#205</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-qemu-action/compare/v3.4.0...v3.5.0">https://github.com/docker/setup-qemu-action/compare/v3.4.0...v3.5.0</a></p>
  <h2>v3.4.0</h2>
  <ul>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.49.0 to 0.54.0 in
  <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/193">docker/setup-qemu-action#193</a>
  <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/197">docker/setup-qemu-action#197</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-qemu-action/compare/v3.3.0...v3.4.0">https://github.com/docker/setup-qemu-action/compare/v3.3.0...v3.4.0</a></p>
  <h2>v3.3.0</h2>
  <ul>
  <li>Add <code>cache-image</code> input to enable/disable caching of
  binfmt image by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/130">docker/setup-qemu-action#130</a></li>
  <li>Bump <code>@​actions/core</code> from 1.10.1 to 1.11.1 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/172">docker/setup-qemu-action#172</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.35.0 to 0.49.0 in
  <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/187">docker/setup-qemu-action#187</a></li>
  <li>Bump cross-spawn from 7.0.3 to 7.0.6 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/182">docker/setup-qemu-action#182</a></li>
  <li>Bump path-to-regexp from 6.2.2 to 6.3.0 in <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/162">docker/setup-qemu-action#162</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-qemu-action/compare/v3.2.0...v3.3.0">https://github.com/docker/setup-qemu-action/compare/v3.2.0...v3.3.0</a></p>
  <h2>v3.2.0</h2>
  <ul>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.31.0 to 0.35.0 in
  <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/154">docker/setup-qemu-action#154</a>
  <a
  href="https://redirect.github.com/docker/setup-qemu-action/pull/155">docker/setup-qemu-action#155</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/setup-qemu-action/compare/v3.1.0...v3.2.0">https://github.com/docker/setup-qemu-action/compare/v3.1.0...v3.2.0</a></p>
  <h2>v3.1.0</h2>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/99012661954931238ded8c8b007157a8430204e1"><code>9901266</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-qemu-action/issues/342">#342</a>
  from docker/dependabot/npm_and_yarn/js-yaml-4.3.2</li>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/9364d8b005eaf92e44e311ad68698e0e4d1fc775"><code>9364d8b</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-qemu-action/issues/340">#340</a>
  from docker/dependabot/npm_and_yarn/humanfs/node-0.16.8</li>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/95c3240fd1cc4bb767d163c4336f203f1f98c8c1"><code>95c3240</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-qemu-action/issues/337">#337</a>
  from docker/dependabot/npm_and_yarn/postcss-selector-...</li>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/c24671a9f635f27130098a37aaa28a0fe899c1f8"><code>c24671a</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-qemu-action/issues/338">#338</a>
  from docker/dependabot/github_actions/codeql-actions-...</li>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/fa83965153d27e053b76aa86ad2b4753aad68c34"><code>fa83965</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-qemu-action/issues/345">#345</a>
  from crazy-max/shared-error-helpers</li>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/f396a658a52c1a0725759b74ea48a6fb8e0e575e"><code>f396a65</code></a>
  build(deps): bump the codeql-actions group across 1 directory with 2
  updates</li>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/a63df2fb68af6df486c0bcef37b58fa32674f8da"><code>a63df2f</code></a>
  chore: update generated content</li>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/3e4165fc6d948cdd283528a2da5d91dca944f777"><code>3e4165f</code></a>
  use the shared error helper for Docker commands</li>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/18c52d952150b11a3c9f65b4b5b751ba623d1fd5"><code>18c52d9</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/setup-qemu-action/issues/344">#344</a>
  from docker/dependabot/npm_and_yarn/docker/actions-to...</li>
  <li><a
  href="https://github.com/docker/setup-qemu-action/commit/896cbedb64417124655bfd0b9a72fac2f0140044"><code>896cbed</code></a>
  [dependabot skip] chore: update generated content</li>
  <li>Additional commits viewable in <a
  href="https://github.com/docker/setup-qemu-action/compare/v3...v4">compare
  view</a></li>
  </ul>
  </details>
  <br />

  Updates `docker/login-action` from 3 to 4
  <details>
  <summary>Release notes</summary>
  <p><em>Sourced from <a
  href="https://github.com/docker/login-action/releases">docker/login-action's
  releases</a>.</em></p>
  <blockquote>
  <h2>v4.0.0</h2>
  <ul>
  <li>Node 24 as default runtime (requires <a
  href="https://github.com/actions/runner/releases/tag/v2.327.1">Actions
  Runner v2.327.1</a> or later) by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/login-action/pull/929">docker/login-action#929</a></li>
  <li>Switch to ESM and update config/test wiring by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/login-action/pull/927">docker/login-action#927</a></li>
  <li>Bump <code>@​actions/core</code> from 1.11.1 to 3.0.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/919">docker/login-action#919</a></li>
  <li>Bump <code>@​aws-sdk/client-ecr</code> from 3.890.0 to 3.1000.0 in
  <a
  href="https://redirect.github.com/docker/login-action/pull/909">docker/login-action#909</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/920">docker/login-action#920</a></li>
  <li>Bump <code>@​aws-sdk/client-ecr-public</code> from 3.890.0 to
  3.1000.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/909">docker/login-action#909</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/920">docker/login-action#920</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.63.0 to 0.77.0 in
  <a
  href="https://redirect.github.com/docker/login-action/pull/910">docker/login-action#910</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/928">docker/login-action#928</a></li>
  <li>Bump <code>@​isaacs/brace-expansion</code> from 5.0.0 to 5.0.1 in <a
  href="https://redirect.github.com/docker/login-action/pull/921">docker/login-action#921</a></li>
  <li>Bump js-yaml from 4.1.0 to 4.1.1 in <a
  href="https://redirect.github.com/docker/login-action/pull/901">docker/login-action#901</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/login-action/compare/v3.7.0...v4.0.0">https://github.com/docker/login-action/compare/v3.7.0...v4.0.0</a></p>
  <h2>v3.7.0</h2>
  <ul>
  <li>Add <code>scope</code> input to set scopes for the authentication
  token by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/login-action/pull/912">docker/login-action#912</a></li>
  <li>Add support for AWS European Sovereign Cloud ECR by <a
  href="https://github.com/dphi"><code>@​dphi</code></a> in <a
  href="https://redirect.github.com/docker/login-action/pull/914">docker/login-action#914</a></li>
  <li>Ensure passwords are redacted with <code>registry-auth</code> input
  by <a href="https://github.com/crazy-max"><code>@​crazy-max</code></a>
  in <a
  href="https://redirect.github.com/docker/login-action/pull/911">docker/login-action#911</a></li>
  <li>build(deps): bump lodash from 4.17.21 to 4.17.23 in <a
  href="https://redirect.github.com/docker/login-action/pull/915">docker/login-action#915</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/login-action/compare/v3.6.0...v3.7.0">https://github.com/docker/login-action/compare/v3.6.0...v3.7.0</a></p>
  <h2>v3.6.0</h2>
  <ul>
  <li>Add <code>registry-auth</code> input for raw authentication to
  registries by <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/login-action/pull/887">docker/login-action#887</a></li>
  <li>Bump <code>@​aws-sdk/client-ecr</code> to 3.890.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/882">docker/login-action#882</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/890">docker/login-action#890</a></li>
  <li>Bump <code>@​aws-sdk/client-ecr-public</code> to 3.890.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/882">docker/login-action#882</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/890">docker/login-action#890</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.62.1 to 0.63.0 in
  <a
  href="https://redirect.github.com/docker/login-action/pull/883">docker/login-action#883</a></li>
  <li>Bump brace-expansion from 1.1.11 to 1.1.12 in <a
  href="https://redirect.github.com/docker/login-action/pull/880">docker/login-action#880</a></li>
  <li>Bump undici from 5.28.4 to 5.29.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/879">docker/login-action#879</a></li>
  <li>Bump tmp from 0.2.3 to 0.2.4 in <a
  href="https://redirect.github.com/docker/login-action/pull/881">docker/login-action#881</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/login-action/compare/v3.5.0...v3.6.0">https://github.com/docker/login-action/compare/v3.5.0...v3.6.0</a></p>
  <h2>v3.5.0</h2>
  <ul>
  <li>Support dual-stack endpoints for AWS ECR by <a
  href="https://github.com/Spacefish"><code>@​Spacefish</code></a> <a
  href="https://github.com/crazy-max"><code>@​crazy-max</code></a> in <a
  href="https://redirect.github.com/docker/login-action/pull/874">docker/login-action#874</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/876">docker/login-action#876</a></li>
  <li>Bump <code>@​aws-sdk/client-ecr</code> to 3.859.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/860">docker/login-action#860</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/878">docker/login-action#878</a></li>
  <li>Bump <code>@​aws-sdk/client-ecr-public</code> to 3.859.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/860">docker/login-action#860</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/878">docker/login-action#878</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.57.0 to 0.62.1 in
  <a
  href="https://redirect.github.com/docker/login-action/pull/870">docker/login-action#870</a></li>
  <li>Bump form-data from 2.5.1 to 2.5.5 in <a
  href="https://redirect.github.com/docker/login-action/pull/875">docker/login-action#875</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/login-action/compare/v3.4.0...v3.5.0">https://github.com/docker/login-action/compare/v3.4.0...v3.5.0</a></p>
  <h2>v3.4.0</h2>
  <ul>
  <li>Bump <code>@​actions/core</code> from 1.10.1 to 1.11.1 in <a
  href="https://redirect.github.com/docker/login-action/pull/791">docker/login-action#791</a></li>
  <li>Bump <code>@​aws-sdk/client-ecr</code> to 3.766.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/789">docker/login-action#789</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/856">docker/login-action#856</a></li>
  <li>Bump <code>@​aws-sdk/client-ecr-public</code> to 3.758.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/789">docker/login-action#789</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/856">docker/login-action#856</a></li>
  <li>Bump <code>@​docker/actions-toolkit</code> from 0.35.0 to 0.57.0 in
  <a
  href="https://redirect.github.com/docker/login-action/pull/801">docker/login-action#801</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/806">docker/login-action#806</a>
  <a
  href="https://redirect.github.com/docker/login-action/pull/858">docker/login-action#858</a></li>
  <li>Bump cross-spawn from 7.0.3 to 7.0.6 in <a
  href="https://redirect.github.com/docker/login-action/pull/814">docker/login-action#814</a></li>
  <li>Bump https-proxy-agent from 7.0.5 to 7.0.6 in <a
  href="https://redirect.github.com/docker/login-action/pull/823">docker/login-action#823</a></li>
  <li>Bump path-to-regexp from 6.2.2 to 6.3.0 in <a
  href="https://redirect.github.com/docker/login-action/pull/777">docker/login-action#777</a></li>
  </ul>
  <p><strong>Full Changelog</strong>: <a
  href="https://github.com/docker/login-action/compare/v3.3.0...v3.4.0">https://github.com/docker/login-action/compare/v3.3.0...v3.4.0</a></p>
  <!-- raw HTML omitted -->
  </blockquote>
  <p>... (truncated)</p>
  </details>
  <details>
  <summary>Commits</summary>
  <ul>
  <li><a
  href="https://github.com/docker/login-action/commit/dbcb813823bdd20940b903addbd779551569679f"><code>dbcb813</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/login-action/issues/1051">#1051</a>
  from docker/dependabot/npm_and_yarn/aws-sdk-dependen...</li>
  <li><a
  href="https://github.com/docker/login-action/commit/5bcb015ee6ec720ecdeaef2dc1164122e9b209fc"><code>5bcb015</code></a>
  [dependabot skip] chore: update generated content</li>
  <li><a
  href="https://github.com/docker/login-action/commit/b30b2f2d3196c1714318ba0c3c3bec211d949752"><code>b30b2f2</code></a>
  build(deps): bump the aws-sdk-dependencies group across 1 directory with
  2 up...</li>
  <li><a
  href="https://github.com/docker/login-action/commit/9087f1e6d666fe0292409e3c819680c18526e108"><code>9087f1e</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/login-action/issues/1057">#1057</a>
  from docker/dependabot/npm_and_yarn/js-yaml-5.2.2</li>
  <li><a
  href="https://github.com/docker/login-action/commit/0009830ea169ca16c24c0ea4cac1c325bfa3aee4"><code>0009830</code></a>
  [dependabot skip] chore: update generated content</li>
  <li><a
  href="https://github.com/docker/login-action/commit/23255232d3e43c8f0052d9a0dba82a515a88ce92"><code>2325523</code></a>
  build(deps): bump js-yaml from 5.2.1 to 5.2.2</li>
  <li><a
  href="https://github.com/docker/login-action/commit/4ec1d4a769e8b05a89a7396551dc38b329211688"><code>4ec1d4a</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/login-action/issues/1056">#1056</a>
  from docker/dependabot/npm_and_yarn/postcss-8.5.22</li>
  <li><a
  href="https://github.com/docker/login-action/commit/5fc99ba47bca274c5a499688f71c7ea79c0ea1b3"><code>5fc99ba</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/login-action/issues/1053">#1053</a>
  from docker/dependabot/github_actions/aws-actions/co...</li>
  <li><a
  href="https://github.com/docker/login-action/commit/e512bd59d16c53d79ea5c0f0e345fe554453c4bb"><code>e512bd5</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/login-action/issues/1052">#1052</a>
  from docker/dependabot/github_actions/codeql-actions...</li>
  <li><a
  href="https://github.com/docker/login-action/commit/a146c91b8f371700d323bae808af7cbdc2766ed5"><code>a146c91</code></a>
  Merge pull request <a
  href="https://redirect.github.com/docker/login-action/issues/1059">#1059</a>
  from crazy-max/harden-buildx-scope-paths</li>
  <li>Additional commits viewable in <a
  href="https://github.com/docker/login-action/compare/v3...v4">compare
  view</a></li>
  </ul>
  </details>
  <br />


  Dependabot will resolve any conflicts with this PR as long as you don't
  alter it yourself. You can also trigger a rebase manually by commenting
  `@dependabot rebase`.


## [0.17.0](https://github.com/omarmhaimdat/pepe/compare/v0.16.1...v0.17.0) - 2026-10-07

### Added

- pepe's own HTTP/1.1 sender, --threads auto, a Rust bench server ([#91](https://github.com/omarmhaimdat/pepe/pull/91))

  Roadmap section 6, Speed on Linux. Through v0.16 pepe spent as much CPU
  per request as oha on Linux, twice wrk's, and reached half oha's
  throughput. This measures why and removes it.

  ## What

  - **pepe reads and writes its connections itself** (`src/direct.rs`,
  `src/wire.rs`). Each worker keeps one HTTP/1.1 connection per origin.
  The request is bytes made before the run and sent with one write; the
  response is parsed where it was read (httparse for the head, a chunked
  decoder of pepe's own); nothing is allocated for a request, and a
  thread's connections share one 128 KB read buffer. reqwest is left what
  only it does: proxies, redirects (the first redirect a target answers
  with hands that target to reqwest), URLs with credentials, and flows.
  - **Less per result and per connection**: results cross to the counting
  thread in 32 bytes instead of 184; reqwest and the TLS configuration are
  built when first needed; one timer per worker instead of one per
  request; the response's headers are read in one pass instead of twenty
  lookups; a worker keeps the body of four failures per status, not of
  every one.
  - **`--threads auto`** (also `threads = "auto"` in `pepe.toml`): adds a
  sending thread when those sending are all past 90% of a core, and takes
  it back if it brought less than a third of what a thread is worth. The
  default stays one thread.
  - **Benchmarks**: the target server is Rust (`bench/server`, its own
  crate; `bench/server.go` is gone). `bench/run.sh` measures wrk and k6
  when installed, has a `million-c64` workload and `MILLIONS=N`. The CI
  Linux record builds pepe for musl, as the releases are.

  ## Numbers

  Linux (4-vCPU arm64 VM, musl build), one thread each, CPU ms per 1,000
  requests / peak MB / req/s, better of two runs:

  | Workload | v0.16.0 | this | wrk `-t1` | oha |
  |---|---|---|---|---|
  | GET, 64 connections | 7.8 / 7.8 / 127k | **2.4 / 4.0 / 398k** | 3.5 /
  4.5 / 282k | 6.8 / 67 / 305k |
  | GET, 1,000 connections | 13.3 / 51.8 / 73k | **3.2 / 5.8 / 296k** |
  4.4 / 7.7 / 228k | 5.6 / 73 / 183k |
  | 16 KB bodies | 9.2 / 9.8 / 99k | **4.2 / 3.9 / 198k** | 5.8 / 4.5 /
  172k | 9.6 / 25 / 196k |
  | HTTPS | 7.6 / 9.3 / 129k | **3.3 / 6.0 / 284k** | 4.5 / 10.8 / 218k |
  8.1 / 42 / 250k |
  | 10M requests, 256 connections | — | **2.9 / 4.5 / 343k** | 3.3 / 4.6 /
  304k | 6.5 / 2,404 / 316k |

  macOS (M4 Pro): 9.8 / 14.0 / 102k → 6.2 / 8.8 / 160k for plain GETs; oha
  23.0 / 77.5 / 161k.

  Every workload, each step's own measurement, the profiles and the CSVs
  are in `bench/README.md` under "The direct path".

  ## Where pepe is not first

  - A slow target at 1,000 connections, and 16 KB bodies over TLS: level
  with wrk on CPU, inside each other's run-to-run range.
  - Throughput against all-core tools: one pepe thread trails oha's four
  on three rows; `--threads 2` leads or is level.
  - Memory on a glibc build: 6.0 MB against wrk's 4.5 at 64 connections
  (the binary's code pages and libc). On the musl build pepe holds less.

  ## For the reviewer

  - **Behaviour change**: a header given twice with `-H` now goes out
  twice. Through reqwest's default headers only the last was sent.
  - A request on a kept connection that turns out closed is sent again
  once on a new one, only for GET, HEAD, OPTIONS and TRACE.
  - The Linux numbers are from an arm64 VM on a laptop that was in use;
  single rows moved by about 20% between runs. The x86 numbers come from
  the Benchmarks workflow on this PR ("bench" in the title runs it).
  - Not run on Windows beyond what CI does; the direct path has no
  platform-specific code.
  - `site/index.html` still quotes the old figures.
  - clippy on Rust 1.99 reports `fetch_update` as deprecated (four uses
  already on master, one added here); CI pins 1.98.

  ## Checked

  - 226 tests on macOS, Linux glibc and Linux musl; `cargo check` on 1.85;
  `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`.
  - 10M and 50M request runs on Linux and four concurrent 5M runs on macOS
  held against the server's own count: exact, none failed.
  - Same outcomes and messages as v0.16.0 on refused connections,
  timeouts, bad certificates, DNS failures, 503s, redirects,
  `--disable-keepalive`, and real HTTPS sites with chunked bodies.


## [0.16.1](https://github.com/omarmhaimdat/pepe/compare/v0.16.0...v0.16.1) - 2026-10-05

### Fixed

- *(deps)* bump crossterm from 0.28.1 to 0.29.0 ([#23](https://github.com/omarmhaimdat/pepe/pull/23))

  Bumps [crossterm](https://github.com/crossterm-rs/crossterm) from 0.28.1
  to 0.29.0.
  <details>
  <summary>Release notes</summary>
  <p><em>Sourced from <a
  href="https://github.com/crossterm-rs/crossterm/releases">crossterm's
  releases</a>.</em></p>
  <blockquote>
  <h2>0.29</h2>
  <h1>Version 0.29</h1>
  <h2>Added ⭐</h2>
  <ul>
  <li>Copy to clipboard using OSC52 (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/974">#974</a>)</li>
  <li>Derive standard traits for &quot;SetCursorStyle&quot; (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/909">#909</a>)</li>
  <li>Add query_keyboard_enhancement_flags to read enabled flags (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/958">#958</a>)</li>
  <li>Add is_* and as_* methods to the event enums (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/949">#949</a>)</li>
  <li>Add a feature flag for derive_more impls (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/970">#970</a>)</li>
  <li>Update rustix to 1.0 (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/982">#982</a>)</li>
  <li>Upgrade various dependencies</li>
  </ul>
  <h2>Breaking ⚠️</h2>
  <ul>
  <li>Correctly fix KeyModifiers Display impl Properly adding + in between
  modifiers (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/979">#979</a>)</li>
  </ul>
  <p><a href="https://github.com/joshka"><code>@​joshka</code></a> <a
  href="https://github.com/linrongbin16"><code>@​linrongbin16</code></a>
  <a href="https://github.com/kmicklas"><code>@​kmicklas</code></a> <a
  href="https://github.com/maciek50322"><code>@​maciek50322</code></a> <a
  href="https://github.com/rosew0od"><code>@​rosew0od</code></a> <a
  href="https://github.com/sxyazi"><code>@​sxyazi</code></a> <a
  href="https://github.com/the-mikedavis"><code>@​the-mikedavis</code></a>
  <a href="https://github.com/hthuz"><code>@​hthuz</code></a> <a
  href="https://github.com/aschey"><code>@​aschey</code></a> <a
  href="https://github.com/naseschwarz"><code>@​naseschwarz</code></a> <a
  href="https://github.com/Flokkq"><code>@​Flokkq</code></a> <a
  href="https://github.com/gaesa"><code>@​gaesa</code></a> <a
  href="https://github.com/WindSoilder"><code>@​WindSoilder</code></a></p>
  </blockquote>
  </details>
  <details>
  <summary>Changelog</summary>
  <p><em>Sourced from <a
  href="https://github.com/crossterm-rs/crossterm/blob/master/CHANGELOG.md">crossterm's
  changelog</a>.</em></p>
  <blockquote>
  <h1>Unreleased</h1>
  <h2>Breaking ⚠️</h2>
  <ul>
  <li>Raise the minimum supported Rust version from 1.63 to 1.85.</li>
  <li>Remove <code>IsTty</code> trait.
  Use the standard library's <a
  href="https://doc.rust-lang.org/std/io/trait.IsTerminal.html"><code>std::io::IsTerminal</code></a>
  trait instead,
  which provides equivalent functionality.</li>
  </ul>
  <h2>Changed ⚙️</h2>
  <ul>
  <li>Migrate the crate to the Rust 2024 edition. This does not raise the
  MSRV beyond Rust 1.85.</li>
  </ul>
  <h2>Fixed 🐛</h2>
  <ul>
  <li>Fix color commands emitting a bare <code>CSI m</code> when colors
  are disabled via
  <code>NO_COLOR</code>, which reset every attribute instead of doing
  nothing.
  Affects <code>SetForegroundColor</code>,
  <code>SetBackgroundColor</code>, <code>SetUnderlineColor</code>,
  and <code>SetColors</code>.</li>
  <li>Fix integer underflow in mouse / cursor-position parsers when coord
  bytes encoded the protocol origin (panic in debug, wrap to 65535
  in release). Affects <code>parse_csi_normal_mouse</code>,
  <code>parse_csi_rxvt_mouse</code>,
  <code>parse_csi_sgr_mouse</code>, and
  <code>parse_csi_cursor_position</code>.</li>
  <li>Fix <code>Colors::from(Colored::UnderlineColor(_))</code> setting
  the background
  color. <code>Colors</code> has no underline field, so the color is now
  dropped
  instead of being applied to the background.</li>
  </ul>
  <h1>Version 0.29</h1>
  <h2>Added ⭐</h2>
  <ul>
  <li>Copy to clipboard using OSC52 (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/974">#974</a>)</li>
  <li>Derive standard traits for &quot;SetCursorStyle&quot; (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/909">#909</a>)</li>
  <li>Add query_keyboard_enhancement_flags to read enabled flags (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/958">#958</a>)</li>
  <li>Add is_* and as_* methods to the event enums (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/949">#949</a>)</li>
  <li>Add a feature flag for derive_more impls (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/970">#970</a>)</li>
  <li>Update rustix to 1.0 (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/982">#982</a>)</li>
  </ul>
  <h2>Breaking ⚠️</h2>
  <ul>
  <li>Correctly fix KeyModifiers Display impl Properly adding + in between
  modifiers (<a
  href="https://redirect.github.com/crossterm-rs/crossterm/issues/979">#979</a>)</li>
  </ul>
  </blockquote>
  </details>
  <details>
  <summary>Commits</summary>
  <ul>
  <li>See full diff in <a
  href="https://github.com/crossterm-rs/crossterm/commits/0.29">compare
  view</a></li>
  </ul>
  </details>
  <br />



### Other

- *(nix)* update flake.lock ([#86](https://github.com/omarmhaimdat/pepe/pull/86))

  Automated changes by the
  [update-flake-lock](https://github.com/DeterminateSystems/update-flake-lock)
  GitHub Action.

  ```
  Flake lock file updates:

  • Added input 'nixpkgs':
      'github:NixOS/nixpkgs/a7868a727837f3c09cee2ce0ca671c76b1589fed?narHash=sha256-KgItSKML8Xvte0B7/uGnBDsYzSnnKHcOaiUbgWBXLXw%3D' (2026-10-03)
  ```

  ### Running GitHub Actions on this PR

  GitHub Actions will not run workflows on pull requests which are opened
  by a GitHub Action.

  **To run GitHub Actions workflows on this PR, close and re-open this
  pull request.**


## [0.16.0](https://github.com/omarmhaimdat/pepe/compare/v0.15.0...v0.16.0) - 2026-10-04

### Added

- colour means health ([#84](https://github.com/omarmhaimdat/pepe/pull/84))

  Colour on pepe's screens used to rank: percentiles ran green to red by
  position, methods had traffic-light colours, redirects were accent cyan,
  the maximum was always red. None of it said whether anything was wrong.
  This makes colour mean one thing: whether a number is healthy.

  ## What changes on screen
  - **Latency** is amber only when it is ten times the median, the
  threshold at which the verdict calls the tail degraded; otherwise it is
  in the terminal's own colour. The maximum and the slowest-requests list
  are the tail by definition, so they stay plain.
  - **Methods** are magenta everywhere, as the dashboard drew them. A
  **3xx** is neither good nor bad. **Labels and faint text** move to
  indexes that read at 4.8:1 or better, on the selected line too.
  - **Every screen has a `?` overlay** with all its keys (F1 on the setup
  screen, where `?` is a character you can type), and footer chips that
  drop from the left and always keep help and quit.
  - **Keys:** edit is `e` on every screen (`E` still works); the
  failed-only filter moves from `e` to `x`; `home`/`end` join `g`/`G`.
  README, man page and the key tables are updated.
  - Charts say why they are empty during the first second, and the request
  log says what ● means.

  ## Themes and `NO_COLOR`
  A theme layer adjusts each drawn frame after rendering, so no drawing
  code knows about it:
  - **light** (`PEPE_THEME=light`, or `COLORFGBG` where the terminal sets
  it): the same roles mapped to a light background; the gray ramp turns
  around so the heatmap's busiest band is the darkest.
  - **none** (`NO_COLOR`): no colour at all; what had a background is
  drawn in reverse video, the heatmap in shades ░▒▓█, and the mascot stays
  home.
  - The shell report, the update notice and `pepe self-update` follow the
  same rule: the verdict comes out coloured when stdout is a terminal and
  `NO_COLOR` is unset, plain otherwise.

  ## Checked
  - Theme detection from `NO_COLOR`, `PEPE_THEME` and `COLORFGBG`; the
  light mapping keeps the heat ramp ordered and leaves the mascot's pixels
  alone; `NO_COLOR` reverses fills and shades the heatmap; the report
  colouring puts each glyph in its colour and leaves the rest alone.
  - Latency colouring: healthy ratios stay plain, a degraded tail is
  amber, no median means no judgment.
  - The suite passes plainly and with `NO_COLOR=1 PEPE_THEME=light` set,
  198 tests each way: the mascot's tests draw the sprite directly, so
  nothing in the tests depends on the environment.
  - `cargo clippy --all-targets`, `cargo fmt --check`. No engine code
  changes, so no benchmark gate.



### Other

- the roadmap gains speed on Linux and agents ([#83](https://github.com/omarmhaimdat/pepe/pull/83))

  Two themes and two items for what pepe is meant to become: the fastest
  load generator, and the one an agent reaches for.

  ## What
  - **Output and integration** gains two items after thresholds: a report
  when there's no terminal (without `--json`, a piped pepe still tries to
  open the dashboard), and a versioned report with a published JSON
  Schema.
  - **6. Speed on Linux**: the reqwest re-parse fix sent upstream, naming
  the rest of the CPU gap with oha, `--threads auto`, a fixed machine for
  the numbers with wrk and k6 in the table, and a lean HTTP/1.1 path if
  the gap is still there.
  - **7. Agents**: agent docs, guardrails (`--allow-host`, caps and
  `--dry-run`), an MCP server, and the engine as a crate.
  - The README's "Next up" sentence named work that has shipped; it now
  names what is open, in this order.
  - `bench/README.md` pointed at "Not now" for the lean HTTP path, which
  was never listed there; it points at "Speed on Linux".

  ## Notes
  - Speed comes before agents, since themes are in the order they're worth
  doing.
  - The flag names are proposals; none of them exist yet.
  - Docs only, no code changes.

  🤖 Generated with [Claude Code](https://claude.com/claude-code)


- the roadmap says what shipped, and in which release ([#82](https://github.com/omarmhaimdat/pepe/pull/82))

  ROADMAP.md with check boxes: every item that landed is ticked with the
  release that carried it, from the completions in v0.8.0 to the config
  keys in v0.15.0. What remains keeps its size and its order within the
  theme.

  Shipped, by theme:

  - **Project health**: completions and man pages (v0.8.0, out of the box
  in v0.9.0); installer smoke tests, the benchmark gate with the Linux
  record, the Linux profile, the recordings, the install page and the
  Windows checklist (all v0.10.0). The scheduled benchmark run is struck
  through as set aside.
  - **Scale and operations**: Docker image and GitHub Action, soak mode
  (v0.10.0); `--rate` (v0.10.2); `--warmup` (v0.11.0); `pepe.toml`
  (v0.12.0, with the three newer keys in v0.15.0). Distributed runs stay
  open.
  - **Analysis and insights**: all five shipped, error clustering in
  v0.10.0, latency by phase in v0.10.1, anomaly notes, Server-Timing and
  the capacity estimate in v0.10.2.
  - **Scenarios and realism**: flows (v0.13.0) and replay (v0.14.0);
  data-driven requests stay open, with a note that flows already have the
  `{{holes}}` it would fill.
  - **Output and integration**: all seven still open; `--fail-if` notes
  that the Action's input covers CI today.

  Arrival rate, config file and warm-up were listed under scenarios and
  are moved to scale and operations, which is the theme they were
  prioritised and shipped under. A `docs:` commit, so no release is
  triggered.


## [0.15.0](https://github.com/omarmhaimdat/pepe/compare/v0.14.0...v0.15.0) - 2026-10-04

### Added

- rate, warmup and trace-header in pepe.toml ([#78](https://github.com/omarmhaimdat/pepe/pull/78))

  The follow-up noted in #68: `pepe.toml` was written beside `--rate`
  ([#67](https://github.com/omarmhaimdat/pepe/pull/67)), `--warmup` ([#69](https://github.com/omarmhaimdat/pepe/pull/69)) and `--trace-header` ([#65](https://github.com/omarmhaimdat/pepe/pull/65)), so its schema didn't
  include them.

  ## What
  - `rate`, `warmup` and `trace-header` keys, with the same precedence as
  every other key: a typed flag wins, the file fills in what was left
  unsaid.
  - `--write-config` and `ctrl-s` on the setup screen write them when set,
  and they read back to the same command line.
  - README's example gains a `rate` line and names the other two.

  ## Checked
  - The config tests cover the three keys both ways: read from the file
  into an untyped run, and written from a command line that sets all
  three, then parsed back and compared.
  - `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.


## [0.14.0](https://github.com/omarmhaimdat/pepe/compare/v0.13.0...v0.14.0) - 2026-10-04

### Added

- pepe replay, an access log's URLs in their real proportions ([#71](https://github.com/omarmhaimdat/pepe/pull/71))

  Roadmap item 13 (scenarios and realism): read an nginx, Caddy or ALB log
  and send its URLs in their real proportions.

  ## What
  - **`pepe replay LOG [--base-url URL] [--include-writes] [--rows N]`**
  with the usual `-c`, `-n`, `-z`, `-H`, `-t`. Each distinct (method, URL)
  becomes a target weighted by how often the log had it, and the engine's
  smooth weighted schedule mixes them evenly, so the mix holds at every
  moment of the run rather than only on average.
  - **Formats:** nginx and Apache common and combined logs and AWS ALB
  logs (the quoted `"GET /path HTTP/1.1"`), Caddy's JSON access lines
  (`request.method`, `request.host`, `request.uri`, `https` when `tls` is
  present) and flat JSON with `method` and `url`/`path`, and plain lists
  of `/path`, `GET /path` or a full URL per line. Blank lines and `#`
  comments are skipped; anything else is counted as unparsed.
  - **`--base-url`** goes in front of paths and replaces the host of full
  URLs, so production's log runs against staging. Without it, full URLs go
  where they point and paths can't go anywhere: `its 2000 requests have
  paths but no host: say where to send them with --base-url`. A log with
  nothing readable, or only writes, says that instead.
  - **Writes** (POST, PUT, PATCH, DELETE) are left out unless
  `--include-writes`; access logs have no bodies, so they go without one.
  The 5,000 most frequent URLs are kept; a longer tail is counted and
  dropped, and the report says so.
  - **Dashboard:** the first tab is **URLs**: the top `--rows` (20) with
  their share of the log, throughput, p50, p99, errors and statuses, then
  one row `4.9% 1,204 other URLs`. The title reads `REPLAY access.log`.
  - **JSON:** `replay: { log, requests_in_log, replayed_from_log,
  distinct_urls, left_out: {unparsed_lines, writes, no_host, rare_urls},
  urls: [{method, url, share_in_log, requests, failed_requests,
  requests_per_second, median_ms, p99_ms, status_codes}] }`.
  - Man page `pepe-replay(1)` and completions regenerated; README section
  "Replaying an access log". New dependency: `regex-lite`.

  ## Checked
  - Unit tests: a combined-log line, an ALB line with a full URL and port,
  a Caddy JSON line (host and TLS assembled into an https URL), plain
  forms, and lines that are nothing; counting with writes left out or
  kept, paths with and without a base, a base replacing a host; the
  dashboard's rows and the shared tail row, and targets with the right
  weights and row tags.
  - End to end: a generated log of 2,000 GETs over four paths
  (60/25/10/5%), 40 POSTs and a garbage line, replayed against the bench
  server with `-n 4000 -c 16 --json`: shares sent 0.589 / 0.263 / 0.100 /
  0.049 against 0.589 / 0.263 / 0.100 / 0.049 in the log, writes and the
  garbage line reported as left out; `--include-writes` sends the POSTs;
  no `--base-url` exits with the message above; the dashboard (recorded in
  a pty) shows the URLs tab with shares and the "other URLs" row.
  - `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.


## [0.13.0](https://github.com/omarmhaimdat/pepe/compare/v0.12.0...v0.13.0) - 2026-10-04

### Added

- pepe flow, a sequence of requests where each step feeds the next ([#70](https://github.com/omarmhaimdat/pepe/pull/70))

  Roadmap item 12 (scenarios and realism): request chaining, a run that is
  a sequence where a value from one response feeds the next, each step a
  row on the dashboard.

  ## What
  - **`pepe flow FILE`** with the usual `-c`, `-n`, `-z`, `-H` (sent with
  every step), `-t` and friends. Each unit of concurrency is one user: it
  walks the steps in order with its own variables, then starts a new
  chain. `-n` counts chains; a chain that has begun finishes when the plan
  ends (a stop still cuts it).
  - **The file:** TOML, `[vars]` for starting values and `[[step]]`s with
  `name`, `method`, `url`, `headers`, `body`, `expect` and `capture`.
  Captures are `json:$.path.to[0].value` (dotted and bracketed, numbers
  and booleans as text, null as nothing), `header:Name`, `regex:pattern`
  (first group, via `regex-lite`) or `body`. `{{name}}` holes in the url,
  headers and body are filled per chain; a hole that no `[vars]` entry or
  earlier capture fills is rejected when the file is read: `step "cart"
  uses {{token}}, which no earlier step captures and [vars] doesn't set`.
  Unknown keys are rejected too.
  - **Failing steps.** A step passes on a 2xx or the status `expect`
  names. Otherwise, or when a capture finds nothing, the chain ends and
  the step counts as a failed request with the reason as its cause
  (`nothing for {{token}} in the response`, `HTTP 401 where 200 was
  expected`), so the verdict's failure clustering says what went wrong.
  - **Dashboard:** the Endpoints tab becomes **Steps**, one row per step
  with its throughput, p50, p99, errors and statuses; the title reads
  `FLOW checkout`. `E` restarts like `r`: there is no setup screen for a
  flow, the file is the setup.
  - **JSON:** the usual report plus `flow: { name, chains_started,
  chains_completed, steps: [{step, requests, failed_requests,
  requests_per_second, median_ms, p99_ms, status_codes}] }`.
  - **Engine:** a flow worker beside the target worker;
  `ResponseStats::with_body` keeps the headers and up to 1 MiB of body
  only for steps that capture something, everything else streams and
  counts as before. Clients are built around the first step's URL so the
  shared headers and settings apply to every step.
  - Man page `pepe-flow(1)` and completions regenerated; the README has a
  "Flows" section with a checkout example.

  ## Checked
  - Unit tests: templates and the names they miss; captures from JSON
  (nested, indexed, quoted keys, non-strings, null), headers, regexes and
  bodies, with bad specs named; flow parsing with every check; a step
  building its request from captured values.
  - Engine test against an in-process token server: five chains of two
  steps give ten successes with the token carried in a header and two
  values in the URL; a capture that finds nothing fails step one and never
  runs step two; `expect = 200` against a 401 fails with the message
  above.
  - End to end: a three-step login → cart → checkout flow against a local
  server, `-n 40 -c 4 --json`: 40 chains started and completed, 120
  requests, `login 200 ×40, cart 200 ×40, checkout 201 ×40`. A file with
  an unfilled hole exits 1 with the step and variable named. The dashboard
  (recorded in a pty) shows the Steps tab with the three rows and the
  flow's name in the title.
  - `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`.


## [0.12.0](https://github.com/omarmhaimdat/pepe/compare/v0.11.0...v0.12.0) - 2026-10-04

### Added

- pepe.toml, a load test that lives next to the code it tests ([#68](https://github.com/omarmhaimdat/pepe/pull/68))

  Roadmap item 10 (scale and operations): a config file, which the setup
  screen can write.

  ## What
  - **`pepe.toml`** in the current directory is read by every run;
  `--config FILE` reads another (and must exist). Precedence is command
  line > file > defaults. The parser's value sources tell typed flags from
  defaults, so `-c 100` typed wins over `concurrency = 50` in the file
  even though 100 is the default. On/off settings can only be switched on
  from the command line, so the file says them the positive way
  (`keep-alive = false`) and can't undo a flag.
  - **Schema:** `url`, `method`, `headers`, `body`, `requests`,
  `duration`, `concurrency`, `timeout`, `threads`, `user-agent`, `proxy`,
  `insecure`, `compression`, `keep-alive`, `redirects`, `snapshot`, plus
  `[ramp]` (`from`, `to`, `step`, `every`, `until`) and `[api]` (`spec`,
  `server`, `auth`, `all`, `tag`, `only`, `skip`, `set`, `include-writes`)
  as defaults for those modes. Unknown keys are rejected with the key
  named and the known ones listed. (`rate` and `trace-header` join the
  schema once #67 and #65 are in.)
  - **Writing it:** `--write-config FILE` writes the settings as given and
  exits, with the line to run it. `ctrl-s` on the setup screen writes the
  form to the file that was read, or `./pepe.toml`, and the status line
  says `✔ saved pepe.toml · pepe here runs it`. The file carries two
  comment lines on how it's used, and leaves defaults out so it says only
  what was chosen (a ramp's file says the ramp, not a concurrency). The
  setup header shows `from pepe.toml` when one was read.
  - `pepe api` with no spec on either side says `api needs a spec: pepe
  api openapi.yaml, or spec under [api] in pepe.toml` instead of a parse
  error.
  - New dependency: `toml` 0.8.

  ## Checked
  - Unit tests: the file fills in what the command line left unsaid and
  typed flags win (including ones equal to the default); `[ramp]` and
  `[api]` feed their subcommands with typed values winning; settings
  written with `--write-config` read back to the same command line, for a
  plain run and a ramp; a typo is named with its position.
  - End to end: `--write-config` then `pepe --json` in that directory runs
  the file's 30 POSTs; `-n 7` on top sends 7; a typo'd file, a missing
  `--config` file and `pepe api` without a spec each exit with the message
  above. The setup screen, driven in a pty: typing a URL and `ctrl-s`
  writes the file and shows the message; reopening shows `from pepe.toml`
  with the URL prefilled.
  - `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`;
  contrib completions and man page regenerated.


## [0.11.0](https://github.com/omarmhaimdat/pepe/compare/v0.10.2...v0.11.0) - 2026-10-04

### Added

- --warmup, sending before counting ([#69](https://github.com/omarmhaimdat/pepe/pull/69))

  Roadmap item 11 (scale and operations): a warm-up, so a run's first
  seconds don't set its numbers.

  ## What
  - **`--warmup <TIME>`** (and a Warm-up field on the setup screen): for
  that long from the start, requests go out at the run's concurrency but
  count for nothing. The engine marks each request it starts during the
  warm-up; a Count plan doesn't claim them, so `-n 50 --warmup 2s` still
  sends 50 counted requests; a Duration plan's deadline moves back by the
  warm-up, so `-z 3s --warmup 2s` measures 3 s and runs 5. A pause during
  the warm-up extends it, like it does the deadline.
  - **On screen:** the title says `◌ warming up` with the time left in
  place of the clock, the progress line reads `0% warming up · 1,204 sent,
  not counted`, and the Stats tab's test card has a `warm-up 5s · 1,204
  not counted` row. The run's clock, timeline and verdict start when the
  warm-up ends; nothing from it reaches the metrics, the request log or
  the report.
  - **JSON:** `generator.warmup_s` and `generator.warmup_requests`, in
  `--json`, API mode and the dashboard's report.
  - A ramp has no warm-up: its first step is one. A value without a unit
  is rejected like `-z` would.

  ## Checked
  - Engine tests: a Count plan of 10 with a 200 ms warm-up yields exactly
  10 unmarked results plus some marked ones; a 200 ms Duration plan with a
  200 ms warm-up runs at least 390 ms and has both kinds.
  - End to end against the bench server: `--warmup 2s -n 50` reports 50
  requests over 174 ms with 668 warm-up requests, in 2.8 s of wall time;
  `--warmup 2s -z 3s` reports 3,026 ms measured in 5.0 s of wall time; the
  dashboard (recorded in a pty) shows the warming-up title, the time left
  and the progress line, then a verdict over the measured part only.
  - `cargo test`, `cargo clippy --all-targets`, `cargo fmt --check`;
  contrib completions and man page regenerated.


## [0.10.2](https://github.com/omarmhaimdat/pepe/compare/v0.10.1...v0.10.2) - 2026-10-04

### Fixed

- *(ci)* release-plz saw only merge commits, so no release came ([#74](https://github.com/omarmhaimdat/pepe/pull/74))

  Since v0.10.1, four feature PRs merged (#66, #63, #65, #67) and every
  release-plz run ended with `pepe: no commit matches the release_commits
  regex`, so no release PR appeared.

  ## Why
  Reproduced locally with the same release-plz (0.3.169) on a clone of
  master. It walks `git log` in commit-date order, checking each commit
  out until it reaches one that is an ancestor of the last release tag.
  The feature commits were authored on Oct 3 and merged on Oct 4, after
  the v0.10.1 tag (Oct 4, 09:27), so the walk reaches the tag first and
  never sees them. The commits it does see are the merge commits, whose
  subjects start with `Merge pull request` or `Merge master into`, which
  `release_commits = "^(feat|fix|perf)[(:!]"` rejects. v0.10.1 only
  happened because #72 was a single commit newer than the v0.10.0 tag,
  which git's path simplification walks straight into.

  Widening the regex is not a fix: git skips two of the four merge commits
  as tree-same with their branch, so the result was a patch bump with a
  changelog of "Merge master into…" lines.

  ## Fix
  1. **Squash-merge pull requests** from now on, with the PR title as the
  commit subject. That gives master one conventional commit per PR, dated
  when it lands, which is what release-plz expects. This is a repository
  setting, not something in this PR:
     ```bash
  gh api -X PATCH repos/omarmhaimdat/pepe -F allow_squash_merge=true -f
  squash_merge_commit_title=PR_TITLE -f
  squash_merge_commit_message=PR_BODY -F allow_merge_commit=false
     ```
  2. This PR makes the changelog skip `Merge …` commits, so a merge commit
  that does get counted doesn't become an entry.
  3. The four features that already merged are written under `Unreleased`
  in CHANGELOG.md by hand. Verified locally: release-plz folds that block
  into the next release's section, so 0.11.0 lists them alongside whatever
  lands squashed.

  Merging this PR (squashed, or as it is: it is one commit newer than the
  tag) is itself what makes release-plz open the next release PR.



### Added

- a capacity estimate from the ramp's curve (#66)

  Once four clean steps are in, a saturation curve is fitted to throughput
  against concurrency and the result states what it reads off it:
  "Capacity about 3.0k req/s · reached around 30 concurrent · median
  latency doubles around 34", or that the curve points past the ramp.
  `--json` has `capacity`.
- anomaly notes during the run (#63)

  Each second is judged against the median of the thirty before it; a p99
  jump, a throughput fall or errors appearing are said in the footer as
  they happen, repeated in the verdict and listed in the JSON report under
  `summary.anomalies`.
- Server-Timing and request ids, to read what the server says (#65)

  `Server-Timing` headers are added up and held against the latency
  measured here, and the five slowest responses are listed with the id
  their backend gave them (`X-Request-Id`, `traceparent`, `CF-Ray`, … or
  `--trace-header`), in the Stats tab, the inspector and the JSON report.
- `--rate`, an arrival rate instead of a closed loop (#67)

  Start a fixed number of requests a second, spread evenly across every
  sending thread; the footer and the verdict say when `-c` can't carry the
  rate and what would. A paused run resumes on schedule. Paced runs keep
  up to their concurrency of idle connections.
## [0.10.1](https://github.com/omarmhaimdat/pepe/compare/v0.10.0...v0.10.1) - 2026-10-04

### Fixed

- *(ci)* the Docker publish job asked for more than the release grants

  The v0.10.0 release failed at startup: release.yml calls
  publish-docker.yml with `packages: write` and `id-token: write`, and a
  called workflow may only request a subset of that. The job asked for
  `contents: read` as well, which made the whole workflow invalid before
  any job ran.

  The job now inherits the caller's permissions, like the Homebrew and R2
  publish jobs do. Checkout of this public repository works without
  `contents: read`, as those two jobs have shown on every release.



### Other

- Merge pull request #61 from omarmhaimdat/feat/latency-phases

- Merge master into feat/latency-phases

## [0.10.0](https://github.com/omarmhaimdat/pepe/compare/v0.9.0...v0.10.0) - 2026-10-03

### Added

- failures grouped by cause, with what the target said

  The verdict now names the main causes of failure with the first response
  body seen for each, so a run that failed on 503s says what the 503 said
  without opening the log. Failures are counted by cause in the metrics
  ("HTTP 503", "Connection refused (os error 61)"), which the dashboard's
  errors panel reads too, and the JSON report lists them under
  summary.failures with the example body. Failed responses keep the start
  of their body and their error's words in --json mode as well, where
  previews are otherwise off.



### Other

- Merge pull request #62 from omarmhaimdat/feat/error-clustering

- what the Linux profile found

  The Linux CPU gap to oha is reqwest's per-request plumbing, not
  syscalls: its follow-redirect layer re-parses the URL on every response,
  the connector is cloned per request, and so on. A reqwest patched to skip
  the re-parse is 3.5-4% cheaper on Linux and is kept as a patch file for
  an upstream change; mimalloc was 1-4% cheaper for 50-90% more memory and
  is dropped. The record says so, and the profile workflow keeps the
  reqwest variant for re-measuring.


- the Linux profile also measures mimalloc and a reqwest without the per-response URL re-parse

- a Linux profile of pepe on demand

- Merge pull request #53 from omarmhaimdat/docs/recordings

- the Linux numbers, and what they say

  The Linux record from the Benchmarks workflow goes into bench/README.md:
  pepe keeps its memory advantage there, but spends about 30% more CPU per
  request than oha and reaches half its throughput, the reverse of the M4
  Pro, while oha costs the same on both. So the CPU claims are stated as
  macOS measurements until the Linux cost is understood, in both READMEs.


- build the target before starting it; find oha's JSON flag

- oha 1.16 has no -j; use --json

- say why a tool gave no result; raise the file limit on the runner

- a benchmark gate on release PRs, and a Linux benchmark record

  bench/compare.sh runs the plain, 16 KB body, 1,000 slow connections and
  TLS workloads with two binaries taking turns, three rounds each, and
  compares the best round of each: it fails when CPU per 1,000 requests
  grew by more than 15% or peak memory by more than 25%. The Benchmarks
  workflow runs it on release PRs, candidate against the last released
  tag built with the same toolchain, so a regression can't ship unnoticed;
  it also runs the full suite for pepe and oha on the Linux runner and
  keeps the CSV, which is where bench/README.md's Linux numbers come from.

  bench/measure.sh reports CPU per 1,000 requests in milliseconds, like
  run.sh does.


## [0.9.0](https://github.com/omarmhaimdat/pepe/compare/v0.8.0...v0.9.0) - 2026-10-03

### Added

- tab completion and man pages that work out of the box

  The completions and man pages shipped in 0.8.0's archives, but nothing
  put them in place. Now:

  - pepe completions --install sets up tab completion for the shell you're
    in (bash, zsh, fish or PowerShell) and installs the man pages: it
    writes the files under ~/.local/share and appends the line the shell's
    startup file needs, once. --dry-run says what it would change;
    `pepe completions zsh` prints the script for packagers. The files are
    built into the binary, so this works however pepe was installed.
  - The install script at pepe.mhaimdat.com runs it after installing, and
    pepe self-update runs it again for the shells it was set up for, so
    completions never fall behind the binary.
  - The Homebrew formula installs the completions and man pages where
    Homebrew activates them, through a publish job of our own in place of
    dist's.


## [0.8.0](https://github.com/omarmhaimdat/pepe/compare/v0.7.0...v0.8.0) - 2026-10-02

### Added

- shell completions and man pages, shipped with every release

  - Completions for bash, zsh, fish and PowerShell, and man pages for pepe
    and each subcommand, generated from the command definition so they
    can't drift from the flags. The main page also has the keys of every
    screen, examples and the environment variables, which --help doesn't.
  - They live under contrib/, go into every release archive, and Homebrew
    puts them under share/pepe. contrib/README.md says where each shell
    wants them.
  - cargo test fails when the files are out of date;
    UPDATE_CONTRIB=1 cargo test contrib rewrites them. The pages carry no
    version and no machine-specific default, so they're the same on every
    machine and don't change on a release PR.



### Other

- the setup-screen test no longer depends on the runner's core count

  The command card leaves out -c when it equals the default, which is the
  machine's core count; the test chose 5, and GitHub's macOS runners now
  have five cores.


## [0.7.0](https://github.com/omarmhaimdat/pepe/compare/v0.6.1...v0.7.0) - 2026-10-02

### Added

- pepe says when a newer release is out, and self-update shows what's new

  - Once a day, pepe looks for a newer release while a run is going, and
    when there is one Pepe says so after the report: the version, what
    changed since yours (from the changelog), and the command that updates
    this copy, for however it was installed. It used to ask GitHub every
    time it quit, and quitting waited for the answer; now the look starts
    with the run and quitting never waits more than a moment for it. In CI,
    or with PEPE_NO_UPDATE_CHECK set, nothing is asked or said. The setup
    screen mentions a newer release the last look found.
  - pepe self-update shows what's new before installing, keeps the
    installer's output behind a spinner (--verbose shows it), and says
    where the new pepe went. For copies installed with Homebrew, Nix or
    cargo it says so and gives the command that updates them.
  - pepe self-update --check only reports, with exit code 1 when a newer
    release exists, so scripts can ask.



### Other

- one changelog, newest first

  CHANGELOG.md had two changelogs in it, an old header halfway down with
  the newest releases under it, so 0.6.0 and 0.6.1 sat below 0.5.1. It is
  one file again, newest release first. The update notice reads what's
  new from here, so the order matters to more than readers.



## [0.6.1](https://github.com/omarmhaimdat/pepe/compare/v0.6.0...v0.6.1) - 2026-10-02

### Performance

- share-nothing load engine, 4× less CPU for the same requests

  pepe spent most of its CPU coordinating 14 tokio threads rather than
  sending requests: every request crossed threads through the connection
  pool's mutex and the timer lock, re-parsed its URL and ran as its own
  spawned task. Measured before and after (bench/README.md), 200,000
  requests at concurrency 64 now cost 1.9 s of CPU instead of 8.5 s with
  the same throughput, which is 2.5× less than oha spends on them.

  What makes it faster:

  - Requests go out from shard threads, one by default, each with its own
    single-threaded tokio runtime, its own reqwest client (so its own
    connection pool and timer wheel) and long-lived worker tasks, one per
    unit of concurrency. Nothing on the hot path is shared between
    threads, so there is no lock to queue on and no task spawned per
    request.
  - The URL and method are parsed once, when the run starts, instead of
    on every request; that parse ran IDNA on the host and formatted the
    IP address back to text each time. A bad URL is now a clean error
    before the run instead of a dashboard full of failures.
  - --json mode collects results on a 25 ms timer, as the dashboard
    already did, instead of waking for each one: a send to a waiting
    receiver goes through the kernel, and at 100k results a second that
    was a fifth of the run's CPU.
  - The main runtime is single-threaded too; it only runs the screens.
  - The client keeps at most four idle connections per host. Whenever a
    request finds no idle connection, the pool races a new one against
    waiting for one and keeps the loser as a spare, so a run at -c 1000
    held 1,999 connections. It now holds 1,003, and peak memory there
    fell from 116 MB to 68 MB.

  So that one thread is a safe default, pepe now says when it is the
  bottleneck: each sending thread measures its own CPU time once a
  second, and past 90% of a core the dashboard's footer shows it, the
  end-of-run verdict adds a note, and the JSON report carries it under
  "generator". --threads N adds threads; it is also a field on the setup
  screen.

  The suite behind the numbers is in bench/: a Go target server, run.sh
  (pepe against oha and vegeta on fixed workloads), tui.py (the dashboard
  in a pseudo-terminal) and the raw results.



### Other

- Merge pull request #41 from omarmhaimdat/perf/share-nothing-engine

- release notes carry each commit's explanation

  The changelog, and so the GitHub release that is built from it, listed
  only commit subjects. Each entry is now the subject followed by the
  commit's body, and perf commits get their own Performance section
  instead of landing under Other.

## [0.6.0](https://github.com/omarmhaimdat/pepe/compare/v0.5.1...v0.6.0) - 2026-10-01

### Added

- ramp mode, and one setup screen for every mode
- API mode, load-testing the endpoints of an OpenAPI spec
- setup screen to fill in every option before a run

### Other

- the setup screen, ramp mode and API mode

## [0.5.1](https://github.com/omarmhaimdat/pepe/compare/v0.5.0...v0.5.1) - 2026-09-30

### Fixed

- Windows curl pastes keep backslashes; clippy 1.98 lint
- parse curl commands the way curl and a shell do

### Other

- stop release-plz from opening duplicate and empty release PRs
- release v0.5.0

## [0.5.0](https://github.com/omarmhaimdat/pepe/compare/v0.4.0...v0.5.0) - 2026-09-30

### Added

- format and highlight JSON, HTML and XML in the request inspector
- inspect any request in full from the Requests tab

### Fixed

- pause stops a timed run's clock; the inspector scrolls like a pager

## [0.4.0](https://github.com/omarmhaimdat/pepe/compare/v0.3.1...v0.4.0) - 2026-09-30

### Added

- rework the Stats view into cards over a full-height distribution
- richer Live view, compact monochrome heatmap
- request filters, calmer colors, redrawn mascot
- latency heatmap, end-of-run verdict and a chili mascot
- interactive dashboard with live charts, and a much faster load path

### Fixed

- dashboard freezing when the terminal falls behind, slow resizes

### Other

- bump the minor version for feature releases before 1.0

## [0.3.1](https://github.com/omarmhaimdat/pepe/compare/v0.3.0...v0.3.1) - 2026-09-29

### Other

- release v0.3.1
- fix false failure in R2 publish verification

## [0.3.0] - 2026-09-29

### Added
- `--duration` / `-z`: run for a fixed time (`10s`, `5m`, `2h`) instead of a fixed request count
- `--json`: run without the dashboard and print a JSON summary (latency min/max/avg/std dev and P50/P90/P95/P99, status codes, error breakdown, throughput) to stdout. Exits when the run completes; Ctrl-C stops early and reports `"interrupted": true`
- `pepe self-update` for installs made with the shell or PowerShell installer
- Prebuilt binaries for macOS (Apple Silicon, Intel), Linux (x86_64, ARM64, statically linked) and Windows (x86_64), published automatically with checksums and build attestations
- New install methods: `irm https://pepe.mhaimdat.com/install.ps1 | iex` (Windows) and a Nix flake (`nix run github:omarmhaimdat/pepe`)
- Dashboard: "Errors" (connection failures, now separate from timeouts) and "Error Rate"

### Fixed
- Crash in duration mode once more requests completed than `-n` (default 100), which also left the terminal in raw mode
- The terminal is restored if pepe ever panics
- Restart (`r`) and stop (`i`) now cancel the running load; `i` stops and keeps the results on screen instead of restarting
- Statistics cover every request; the JSON report previously only used the last 100 and counted failures twice
- Request body (`-d`) is sent for every method, not only POST
- `--disable-keepalive` now opens a new connection per request
- Connection errors are no longer reported as timeouts
- Data transferred counts the bytes actually received (previously 0 for chunked or compressed responses)
- Standard deviation used the previous average
- Invalid `-H` headers are rejected with an error instead of crashing; repeated headers are kept
- No crash on narrow terminals; memory no longer grows during long runs
- Update notice no longer suggests "updating" to an older version, goes to stderr, and can be disabled with `PEPE_NO_UPDATE_CHECK=1`
- Fixed the hosted `install.sh`

### Changed
- DNS is resolved with the system resolver (reqwest `hickory-dns` removed), matching the DNS timing pepe reports
- Latency is measured from sending the request, excluding pepe's own DNS timing lookup
- Latency is shown with sub-millisecond precision
- Security updates for quinn-proto, rustls-webpki, ring, bytes and tokio

## [0.2.9] - 2025-02-22

## [0.2.8] - 2025-02-18

## [0.2.7] - 2025-02-15

## [0.2.6] - 2025-02-15

## [0.2.5] - 2025-02-15

## [0.2.4] - 2025-02-14

### Added
- System hostname display in header section
- Author information in CLI output
- Nginx-style log format for Recent Requests
- Redesigned progress bar UI

### Changed
- Updated header information layout
- Enhanced request log visualization
- Improved progress tracking display

## [0.2.3] - 2025-02-13

### Added
- Generated reqwest client from CLI configuration

### Changed
- Improved event handling structure
- Break large functions into smaller ones
- Use default_value_t instead of default_value
- Remove redundant short and long attributes in clap
- Replace #[clap] with #[arg]
- Improve validation by returning errors instead of exit

### Removed
- Redundant comments


<!-- next-url -->
[Unreleased]: https://github.com/omarmhaimdat/pepe/compare/v0.2.9...HEAD

[0.2.9]: https://github.com/omarmhaimdat/pepe/compare/v0.2.8...v0.2.9

[0.2.8]: https://github.com/omarmhaimdat/pepe/compare/v0.2.7...v0.2.8

[0.2.7]: https://github.com/omarmhaimdat/pepe/compare/v0.2.6...v0.2.7

[0.2.6]: https://github.com/omarmhaimdat/pepe/compare/v0.2.5...v0.2.6

[0.2.5]: https://github.com/omarmhaimdat/pepe/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/omarmhaimdat/pepe/releases/tag/v0.2.4
