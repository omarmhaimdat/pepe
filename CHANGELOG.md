# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
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
