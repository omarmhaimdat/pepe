# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]
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
