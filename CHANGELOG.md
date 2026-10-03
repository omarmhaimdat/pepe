# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
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
