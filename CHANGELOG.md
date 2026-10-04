# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
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
