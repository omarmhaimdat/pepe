<div align="center">

<img src="assets/logo.svg" width="112" alt="Pepe, a pixel-art chili pepper">

# pepe

**An HTTP load generator with a live dashboard, built to cost less than the server it tests.**

[![CI](https://github.com/omarmhaimdat/pepe/actions/workflows/CI.yaml/badge.svg)](https://github.com/omarmhaimdat/pepe/actions/workflows/CI.yaml) [![Release](https://img.shields.io/github/v/release/omarmhaimdat/pepe?display_name=tag&color=brightgreen)](https://github.com/omarmhaimdat/pepe/releases/latest) [![Downloads](https://img.shields.io/github/downloads/omarmhaimdat/pepe/total?color=blue)](https://github.com/omarmhaimdat/pepe/releases) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) ![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange)

[Install](#install) · [Quick start](#quick-start) · [Usage](#usage) · [Dashboard](#the-dashboard) · [How it compares](#how-pepe-compares) · [Roadmap](ROADMAP.md) · [Contributing](#contributing)

</div>

![pepe load-testing a server: the live dashboard, the request log, and the verdict](assets/run.gif)

pepe sends requests to a URL, as many at once as you ask, and shows what came back as it happens: throughput, latency percentiles and a heatmap, status codes, failures by cause, and a log you can open any request from. When the run ends it gives a verdict in plain words. It takes a curl command as input, ramps load to find where a target stops keeping up, and load-tests every endpoint of an OpenAPI spec.

It is also light. One thread sends about 100k requests a second on an Apple M4 Pro, using 2.5× less CPU than comparable tools there and a fraction of their memory everywhere, and pepe tells you when it, rather than the target, is the limit.

## Highlights

- **Live dashboard** with a latency heatmap, percentile and throughput charts, status codes, failure causes and a scrollable, filterable request log. Press `enter` on a request to see its headers and body, formatted.
- **A verdict**, not just numbers: Healthy, Degraded or Failing, with findings such as two latency groups, a long tail, or throughput drifting over the run.
- **Interactive control**: pause, resume, stop, restart, and raise or lower concurrency while the run is going.
- **Any curl command** from a browser's "Copy as cURL", Postman or Insomnia, sent exactly as curl would.
- **Setup screen**: run `pepe` with no arguments and fill in every option on a form, with the equivalent command shown as you go.
- **Ramp mode**: raise concurrency step by step and find the level that held, where throughput stopped following the load, and where it broke.
- **API mode**: load-test the endpoints of an OpenAPI 3 or Swagger 2 spec, picked by tag, with parameters and credentials set on screen.
- **What the server says**: `Server-Timing` headers are added up and held against the latency measured here, and the slowest requests are listed with the ids their backend gave them (`X-Request-Id`, `traceparent`, `CF-Ray`, …), ready to search in its logs.
- **JSON output** for scripts and CI, with the same percentiles the dashboard shows.
- **Light**: a share-nothing engine that is measured against oha and vegeta on every release; the numbers and the method are in [bench/README.md](bench/README.md).

## Install

<details open>
<summary><b>macOS and Linux</b></summary>

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh
```
</details>

<details>
<summary><b>Windows</b> (PowerShell)</summary>

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://pepe.mhaimdat.com/install.ps1 | iex"
```
</details>

<details>
<summary><b>Homebrew</b> (macOS and Linux)</summary>

```bash
brew install omarmhaimdat/pepe/pepe
```
</details>

<details>
<summary><b>Nix</b></summary>

```bash
nix run github:omarmhaimdat/pepe -- https://example.com   # try it without installing
nix profile install github:omarmhaimdat/pepe              # install it
```
</details>

<details>
<summary><b>Docker</b></summary>

```bash
docker run --rm -it ghcr.io/omarmhaimdat/pepe -z 30s -c 50 https://example.com   # the dashboard needs -it
docker run --rm ghcr.io/omarmhaimdat/pepe --json -n 1000 https://example.com     # for scripts
```

An empty image with the static binary in it, a few megabytes, for `linux/amd64` and `linux/arm64`; `:latest` and `:<version>` tags. `docker build -t pepe .` builds the same from source.
</details>

<details>
<summary><b>Prebuilt binaries</b></summary>

Every [release](https://github.com/omarmhaimdat/pepe/releases) ships binaries for macOS (Apple Silicon and Intel), Linux (x86_64 and ARM64, statically linked) and Windows (x86_64), with SHA-256 checksums and signed build provenance:

```bash
gh attestation verify pepe-x86_64-unknown-linux-musl.tar.xz --repo omarmhaimdat/pepe
```

Each archive also carries the shell completions and man pages; `pepe completions --install` puts them in place for your shell (see [contrib/README.md](contrib/README.md)).
</details>

<details>
<summary><b>From source</b></summary>

```bash
cargo install --locked --git https://github.com/omarmhaimdat/pepe
```
</details>

### Staying up to date

pepe looks for a newer release once a day and says so when a run ends, with what changed and the command that updates your copy. Installer-based installs update themselves:

```bash
pepe self-update            # install the latest release
pepe self-update --check    # only say whether there is one (exit code 1 if so)
```

Homebrew and Nix installs update through `brew upgrade pepe` and `nix profile upgrade pepe`. Set `PEPE_NO_UPDATE_CHECK=1` to turn the check off; it is off in CI already.

Tab completion (bash, zsh, fish, PowerShell) and `man pepe` come with the install script and the Homebrew formula. Installed another way, `pepe completions --install` sets them up for the shell you're in; `--dry-run` shows what it would change.

## Quick start

Send 100 requests, 14 at a time (one per core), and watch:

```bash
pepe https://example.com
```

Run for thirty seconds at concurrency 50, with a header and a JSON body:

```bash
pepe -z 30s -c 50 -m POST -H 'Content-Type: application/json' -d '{"key":"value"}' https://httpbin.org/post
```

Load-test a request copied from your browser's dev tools:

```bash
pepe -z 30s --curl -- curl 'https://api.example.com/items' -H 'Authorization: Bearer …'
```

Open the setup screen and fill everything in on a form:

```bash
pepe
```

In the dashboard, `space` pauses, `+` and `-` change concurrency, `tab` switches view, `enter` inspects a request, `?` lists every key, `q` quits and leaves the verdict in your shell.

## Usage

```
pepe [OPTIONS] [URL]
pepe ramp [OPTIONS] [URL]      raise the load step by step
pepe api  [OPTIONS] <SPEC>     load-test an OpenAPI spec
pepe self-update [--check]     update pepe
```

### Options

| Option | Default | What it does |
| --- | --- | --- |
| `-n`, `--number <N>` | 100 | Requests to send |
| `-c`, `--concurrency <N>` | number of cores | Requests in flight at once |
| `-z`, `--duration <TIME>` | | Run for this long instead of a count: `30s`, `3m`, `2h` |
| `-m`, `--method <METHOD>` | GET | HTTP method |
| `-H`, `--headers <HEADER>` | | A header, `Name: value`; repeat for more |
| `-d`, `--body <TEXT>` | | Request body |
| `-t`, `--timeout <SECONDS>` | 20 | Time to wait for a response |
| `-u`, `--user-agent <UA>` | `pepe/<version>` | User-Agent header |
| `-p`, `--proxy <URL>` | | HTTP, HTTPS or SOCKS5 proxy, with credentials if needed |
| `-k`, `--insecure` | | Accept invalid TLS certificates |
| `--disable-compression` | | Don't ask for gzip |
| `--disable-keepalive` | | Open a new connection for every request |
| `--disable-redirects` | | Don't follow redirects |
| `--warmup <TIME>` | | Send for this long before counting anything (see [Warm-up](#warm-up)) |
| `--threads <N>` | 1 | Threads sending requests (see [Threads](#threads)) |
| `--rate <PER_SECOND>` | | Start this many requests a second, spread evenly (see [Arrival rate](#arrival-rate)) |
| `--curl` | | Read the request from a curl command (see below) |
| `-i`, `--setup` | | Open the setup screen filled in from the flags |
| `--json` | | No dashboard: run to completion and print a JSON report |
| `--config <FILE>` | `./pepe.toml` if present | Read settings from a file (see [Config file](#config-file)) |
| `--write-config <FILE>` | | Write the settings as given to a file and exit |
| `--trace-header <NAME>` | | Response header holding the request id, if not one of the usual ones |

### Config file

A load test can live next to the code it tests. Put a `pepe.toml` in the directory and `pepe` alone runs it; flags on the command line win over the file, and the file wins over pepe's defaults.

```toml
url = "https://api.example.com/health"
method = "GET"
headers = ["Accept: application/json", "Authorization: Bearer ..."]
duration = "1m"        # or requests = 1000
concurrency = 50
timeout = 5
rate = 500             # requests started per second; warmup = "5s" and trace-header work too
keep-alive = true      # compression, redirects, insecure, threads, proxy, user-agent, snapshot, body too

[ramp]                 # defaults for `pepe ramp`
from = 10
to = 200
step = 10
every = "15s"
until = ["p99 > 500ms", "errors > 1%"]

[api]                  # defaults for `pepe api`
spec = "openapi.yaml"
tag = ["Billing"]
```

Keys are the long flags' names; the on/off ones are said the positive way. A key pepe doesn't know is an error that names it. `--config FILE` reads another file; `--write-config FILE` writes the settings as given on the command line and exits, and `ctrl-s` on the setup screen writes the form the same way, so a test worked out on screen can be kept and run again with `pepe`.

### Headers, bodies and methods

```bash
pepe -n 1000 -c 20 -H 'Accept: application/json' -H 'X-Request-Id: load' https://example.com
pepe -n 1000 -c 10 -m PUT -d @payload.json -H 'Content-Type: application/json' https://example.com/items/1
```

Repeated headers are kept (several `Cookie` headers are sent as several), and a `User-Agent` given with `-H` wins over the default.

### Timed runs

`-z` keeps sending for a duration instead of a count. Pausing stops the clock, so a paused run still gets its full length of sending.

```bash
pepe -z 2m -c 100 https://example.com
```

### Warm-up

The first seconds of a run are not like the rest: connections open, caches fill, JITs settle. `--warmup 5s` sends at the run's concurrency for that long first and counts none of it: not in the numbers, the charts, the verdict or the report, and not against `-n` or `-z`, whose clock starts when the warm-up ends. The header says `warming up` with the time left while it goes, and the Stats tab and the JSON report (`generator.warmup_s`, `warmup_requests`) say how much was sent and not counted. A ramp has no warm-up: its first step is one.

```bash
pepe --warmup 5s -z 1m -c 50 https://example.com
```

### Arrival rate

Without `--rate`, pepe is a closed loop: each unit of concurrency sends its next request the moment the last one answers, so a slower target gets fewer requests. That finds the most a target can do. Real traffic doesn't wait for the target: users arrive at their own rate, and a target that slows down gets the same arrivals and a growing queue. `--rate` sends like that, an open loop:

```bash
pepe https://example.com --rate 500 -c 64 -z 2m
```

Starts are spread evenly over each second, across every sending thread. `-c` is then the most requests in flight at once, and it has to be enough: at 500 a second and 40 ms a response, 20 are in flight on average (Little's law), more when the target has a bad moment. When the concurrency can't carry the rate, the footer says so as it happens ("behind the rate: 410 of 500 req/s · all 8 in flight; try -c 25"), and the verdict says what was asked and what was sent. A schedule that falls more than a second behind is not caught up with a burst; those starts are counted as missed instead (`generator.rate_missed` in the JSON report), because a burst would say nothing true about the target. A paused run resumes on schedule.

### Soak runs

For a run that lasts hours, `--snapshot` writes the JSON report so far to a file every minute, whole or not at all, and once more when the run ends, so a crash at hour six or a lost terminal doesn't lose the numbers. The report carries a minute-by-minute timeline of the whole run (throughput, errors, p50, p90, p99), the dashboard's ten-minute charts still show the recent past, and the end-of-run verdict judges the whole run on those minutes.

```bash
pepe -z 6h -c 50 --snapshot soak.json https://example.com
jq '.timeline[-1], .snapshot' soak.json       # the last minute, and whether it is still running
```

### Load-testing a curl command

pepe sends the same request curl would: same method, URL, headers and body. Put `--` between pepe's options and the curl command, or give it as one quoted string, a file, or on stdin:

```bash
pepe -n 1000 -c 10 --curl -- curl -X POST 'https://httpbin.org/post' -H 'Content-Type: application/json' -d '{"key": "value"}'
pepe -z 30s --curl -- @request.txt
pbpaste | pepe -z 30s --curl
```

What's understood:

- **Quoting**: single and double quotes, backslash escapes, line continuations, bash `$'...'` (Chrome's "Copy as cURL (bash)") and Windows `^` escaping ("Copy as cURL (cmd)").
- **Methods**: `-X`, and the ones curl implies: POST for data and forms, PUT for `-T`, HEAD for `-I`, GET for `-G`.
- **Bodies**: `-d`/`--data`, `--data-raw`, `--data-binary`, `--data-urlencode`, `--json`, `@file` for any of them, `-F`/`--form` multipart (with file uploads), `-T` uploads, and `-G` to move data into the query string.
- **Headers**: `-H` (including `-H @file`, `-H 'Name;'` for an empty value, `-H 'Name:'` to drop one), `-u` basic auth, `--oauth2-bearer`, `-b` cookies, `-A`, `-e`, `-r`, `--compressed`.
- **Connection**: `-L` (like curl, redirects are only followed with `-L`), `-k`, `-x`, `-m`, `--no-keepalive`, `--url`, `--url-query`, bunched flags like `-sSLk` and attached values like `-XPOST`.

Output, logging and TLS options (`-o`, `-s`, `-v`, `-w`, `--cacert`, ...) are accepted and have no effect. An unknown option is an error, and anything pepe can't reproduce (such as a cookie file) is reported as a note.

### The setup screen

![The setup screen: a form with every option, a try-once response, and the command as flags](assets/setup.gif)

Run `pepe` with no URL and it opens a form with every option as a field: URL, method, headers, body, load, timeout, threads, redirects, keep-alive, TLS, proxy and user agent. Add `-i` to any command to open the form filled in from its flags.

```bash
pepe
pepe -i -c 50 -z 30s https://example.com
```

- `tab` switches mode: **Single URL**, **Ramp** or **API**. What the modes share is kept.
- `↑` `↓` move between fields, `←` `→` change a choice, `enter` starts.
- Paste a curl command anywhere and the form is filled in from it.
- `ctrl-t` sends the request once and shows the response, to check it before the run.
- The command card always shows the flags that reproduce the form. It's printed to your shell when you quit, and `E` in the dashboard brings you back to the form.

### Ramp mode: finding where the target stops keeping up

![Ramp mode: concurrency raised step by step, with each step measured on its own](assets/ramp.gif)

`pepe ramp` raises concurrency step by step, measures each step on its own, and says where the target holds, where it stops scaling and where it breaks.

```bash
pepe ramp https://example.com --from 10 --to 200 --step 10 --every 15s
pepe ramp https://example.com --until 'p99 > 500ms' --until 'errors > 1%'
```

| Flag | Default | What it does |
| --- | --- | --- |
| `--from`, `--to`, `--step` | 10, 100, 10 | Concurrency of the first step, the last, and what's added between |
| `--every` | 10s | How long each step is held |
| `--until <CONDITION>` | | End the ramp once a step crosses a limit: `p50 > 100ms`, `p99 > 2s`, `errors > 1%`; repeat for several |

`-m`, `-d`, `-H` and the other request options work as they do without `ramp`. Without a URL, the setup screen opens in Ramp mode.

The screen shows each step as a row (throughput, p50, p90, p99, the slowest request, errors) with a note when something changes, the run second by second, throughput and p99 at each concurrency, and the result: the level that held, where throughput stopped following the load, where it broke, and the command for a steady run at the level that held. Throughput counts successful responses only, so a target that sheds load quickly doesn't look fast.

Once four clean steps are in, a saturation curve (the Universal Scalability Law) is fitted to throughput against concurrency, and the result states the capacity read off it: "Capacity about 3.0k req/s · reached around 30 concurrent · median latency doubles around 34". When the curve is still climbing at the last step but has begun to bend, the estimate says so ("Capacity beyond the ramp … past the ramp's 50"); when it hasn't bent at all, no number is given, because none would be honest. The estimate is kept only when the curve reproduces every measured step within 25%.

| Key | Action |
| --- | --- |
| `↑` `↓` | Pick a step and see everything measured about it; `esc` goes back to following the run |
| `space` | Pause or resume; a step's clock stops while paused |
| `n` | End this step now and go on to the next |
| `s` | Stop the ramp here and keep the results |
| `r` / `e` | Run again / back to the setup screen |
| `q` / `Ctrl-C` | Quit; the table and the result are printed to your shell |

`--json` runs the ramp without a screen and prints every step, the findings, and `capacity` (`requests_per_second`, `concurrency`, `extrapolated`, `latency_doubles_at_concurrency`; `null` when the curve hadn't bent).

### API mode: load-testing an OpenAPI spec

![API mode: the endpoints of a spec picked on screen, then a dashboard with a row per endpoint](assets/api.gif)

`pepe api` reads an OpenAPI 3 (or Swagger 2) spec, from a file or a URL, in JSON or YAML, and turns its operations into requests.

```bash
pepe api openapi.yaml
pepe api https://api.example.com/openapi.json --auth bearer:$TOKEN -c 20 -z 1m
```

It opens on a plan screen. Nothing is sent, and no endpoint is switched on, until you say so.

- Endpoints are listed under the spec's tags. `space` switches an endpoint on or off, or a whole tag; `/` filters the list.
- `enter` on an endpoint goes to its parameters: path, query, header and cookie parameters with their type, description and the values the spec allows, then the body and the endpoint's share of the traffic. `enter` edits one, `space` steps through the spec's values, `del` leaves it out. Several values (`a, b`) are sent in turn, or together for array parameters.
- The request is shown as it will go out, and `t` sends it once and shows the answer.
- If the spec declares authentication and none was given, pepe asks for it, checks it with one request, and shows credentials masked from then on.
- `c`, `n`, `z` change concurrency, requests and duration; `u` the server; `a` the credentials; `g` starts the run.
- Endpoints that still need a value, and writes (POST, PUT, PATCH, DELETE), are never switched on in bulk: you switch those on one by one.

The run is the usual dashboard, with an **Endpoints** view in front: requests, throughput, p50, p99, errors and status codes for each endpoint. `enter` on one shows its requests, and `E` goes back to the plan.

Everything on the plan screen has a flag:

| Flag | What it does |
| --- | --- |
| `--auth` | Credentials: `bearer:TOKEN`, `basic:USER:PASSWORD`, `apikey:VALUE`, `header:NAME=VALUE` or `query:NAME=VALUE` |
| `--server` | Send requests here instead of the spec's server |
| `--all` | Switch on every endpoint that has the values it needs |
| `--tag`, `--only` | Switch on the endpoints with this tag, or matching a pattern such as `'GET /pets*'` |
| `--skip` | Leave out endpoints matching a pattern |
| `--set` | Give a parameter its value(s) wherever it appears: `--set id=1,2,3` |
| `--include-writes` | Let `--all`, `--tag` and `--only` switch on writes too |

With `--json` there is no screen, so name what to run with `--all`, `--tag` or `--only`; the report has a section per endpoint.

### Threads

pepe sends from one thread, whatever the concurrency. One thread sends about 100k plain requests a second, or 10k TLS handshakes a second, at the lowest CPU per request, and that is more than most targets can take. When it isn't, pepe says so: the dashboard's footer shows how busy the sending thread is once it passes 90% of a core, the end-of-run verdict notes it, and the JSON report has it under `generator`. Then `--threads` adds more:

```bash
pepe -c 500 --threads 4 -z 30s http://localhost:8080/
```

### JSON output

`--json` skips the dashboard, runs to completion and prints a report to stdout. Press Ctrl-C to stop early; the report then has `"interrupted": true`.

```bash
pepe --json -n 1000 -c 20 https://example.com > results.json
jq '.summary.latency.p99_ms' results.json
```

```json
{
  "summary": {
    "total_requests": 1000,
    "successful_requests": 1000,
    "failed_requests": 0,
    "http_errors": 0,
    "timeout_errors": 0,
    "connection_errors": 0,
    "interrupted": false,
    "duration_ms": 1843.2,
    "requests_per_second": 542.5,
    "data_transfer_bytes": 1256000,
    "latency": { "min_ms": 9.1, "max_ms": 212.4, "avg_ms": 36.1, "std_dev_ms": 18.0,
                 "median_ms": 31.9, "p90_ms": 58.2, "p95_ms": 71.0, "p99_ms": 120.3 },
    "status_codes": { "200": 1000 },
    "server_timing": { "responses": 1000, "total": { "count": 1000, "median_ms": 24.1, "p90_ms": 40.2, "p99_ms": 88.0 },
                       "segments": { "db": { "count": 1000, "median_ms": 18.3, "p90_ms": 31.0, "p99_ms": 70.2 },
                                     "app": { "count": 1000, "median_ms": 5.8, "p90_ms": 9.1, "p99_ms": 17.9 } } },
    "slowest_requests": [ { "at_s": 1.204, "latency_ms": 212.4, "status": 200,
                            "request_id": "8f3c1a2e-7b9d", "id_header": "x-request-id" } ]
  },
  "generator": { "threads": 1, "peak_busy_percent": 12 }
}
```

`server_timing` is there when the target sends `Server-Timing` headers, and `slowest_requests` lists the five slowest responses with the request id their backend gave them, so they can be found in its logs.

### In GitHub Actions

The repository is also an action: it installs a pinned release, runs `pepe --json`, puts the numbers in the job summary and in outputs, and can fail the job on a condition over the report.

```yaml
- uses: omarmhaimdat/pepe@master
  id: load
  with:
    url: https://staging.example.com/api/health
    args: -z 30s -c 20 -H 'Authorization: Bearer ${{ secrets.TOKEN }}'
    fail-if: ".summary.latency.p99_ms > 300 or .summary.failed_requests > 0"
- run: echo "p99 was ${{ steps.load.outputs.p99_ms }} ms at ${{ steps.load.outputs.requests_per_second }} req/s"
```

Outputs: `total_requests`, `failed_requests`, `requests_per_second`, `p50_ms`, `p99_ms`, and `report`, the path of the JSON. `version` pins a release (`0.9.0`); the default is the latest. Linux and macOS runners.

### Proxies

HTTP, HTTPS and SOCKS5, with or without credentials:

```bash
pepe -n 1000 -c 10 -p http://proxy:port https://example.com
pepe -n 1000 -c 10 -p socks5://username:password@proxy:port https://example.com
```

### Environment

| Variable | Effect |
| --- | --- |
| `PEPE_NO_UPDATE_CHECK` | Set to anything to skip the once-a-day look for a newer release |
| `PEPE_GITHUB_TOKEN` | A GitHub token for `pepe self-update`, for forks or rate-limited CI |
| `PEPE_CACHE_DIR` | Where the update check keeps its answer (defaults to the OS cache directory) |

## The dashboard

Three views (four in API mode, with **Endpoints** in front):

- **Live**: the headline numbers, a latency heatmap (time across, latency up, brighter cells mean more requests took that long) with p50 and p99 marked, throughput per second, and a panel with the detailed numbers.
- **Stats**: every number pepe collects, the test setup, and the latency distribution. When the target sends `Server-Timing`, a card shows its own time against the median measured here and each segment's p50 and p99. Another lists the five slowest responses with their request ids, taken from `X-Request-Id`, `traceparent`, `CF-Ray`, `X-Amzn-Trace-Id` and other common headers, or the one named with `--trace-header`.
- **Requests**: the last 2,000 requests (and older failures), filterable by status, latency and text. Press `enter` on one to inspect it: status, total time split into time to first byte and body download, how it ranks in the run, DNS, server address, protocol, cache status, the request as sent, and the full response headers and body, with JSON, HTML and XML indented and highlighted. Walk to the next request with `←`/`→`. Up to 1,000 responses a second are kept in full, an even sample above that (marked `●`, reached with `[`/`]`).

Pepe, the chili in the corner, reacts to how the run is going. While it runs, each second is compared with the thirty before it, and a p99 that jumps, throughput that falls or errors that appear are called out in the footer as they happen ("p99 jumped 4.5× to 45ms at 26s"), then repeated in the verdict and listed in the JSON report. When a run ends, the header turns into a verdict (Healthy, Degraded or Failing) with findings such as failed requests, two separate latency groups, a long tail, or throughput and latency drifting over the run. The same summary is printed to your shell when you quit.

| Key | Action |
| --- | --- |
| `space` / `p` | Pause or resume sending; a timed run's clock stops while paused |
| `+` / `-` | Raise or lower concurrency by about 10%, live |
| `s` / `i` | Stop sending and keep the results on screen |
| `r` | Restart with the same settings (and the current concurrency) |
| `E` | Back to the setup screen (or, in API mode, the plan), to change the settings and run again |
| `tab` / `←` `→` / `1` `2` `3` | Switch view |
| `↑` `↓` / `j` `k`, `PgUp` `PgDn`, `g` `G` | Select a request in the log (newer / older) |
| `f` | Filter requests by status: 2xx, 3xx, 4xx, 5xx, no response, failed |
| `l` | Filter requests by latency: at or above p50, p90 or p99 |
| `/` | Search the status and response text |
| `e` / `c` | Show only failed requests / clear all filters |
| `enter` | Inspect the selected request: `↑`/`↓` (or the trackpad), `u`/`d` and `g`/`G` scroll the response, `←`/`→` walk to the newer/older request, `[`/`]` jump to the nearest one kept in full, `v` switches between formatted and raw, `esc` goes back |
| `?` | Show all keys |
| `q` / `esc` / `Ctrl-C` | Quit |

### What pepe measures

Min, max, mean, median, p90, p95, p99 and standard deviation of latency, and where that time goes: opening connections (TCP and TLS, once per connection), waiting for the first byte, and downloading the body, each with its own percentiles; requests per second and bytes per second; total data transferred; the error rate, with failures counted separately as non-2xx responses, connection errors and timeouts, and grouped by cause with the first response body of each, so the verdict says what a 503 actually said; status codes; cache hit rate from `X-Cache`, `CF-Cache-Status`, `X-Vercel-Cache` and similar headers; and DNS lookup time, sampled once a second.

## How pepe compares

Measured with the suite in [`bench/`](bench/) against a local server that answers from memory, so the client is the cost being measured. CPU milliseconds per 1,000 requests, peak memory, and the requests per second each tool reported (the server tops out near 130k):

| Workload | pepe | oha | vegeta |
| --- | --- | --- | --- |
| GET, 64 connections | **9.6 ms · 14 MB** · 100k req/s | 23.4 ms · 79 MB · 107k req/s | 88.5 ms · 26 MB · 67k req/s |
| GET, 1,000 connections | **12.3 ms · 68 MB** · 80k req/s | 24.3 ms · 111 MB · 102k req/s | 48.8 ms · 94 MB · 125k req/s |
| HTTPS, 64 connections | **9.9 ms · 15 MB** · 97k req/s | 24.9 ms · 58 MB · 107k req/s | 72.7 ms · 31 MB · 80k req/s |

Those are single-thread numbers for pepe on an Apple M4 Pro. On a 4-vCPU Linux runner the memory advantage holds but the CPU per request does not yet (about 30% more than oha); the Linux table and what is known about why are in [bench/README.md](bench/README.md), with the method, every workload and the profiles. Where a target can take more than one thread sends, pepe says so and `--threads` raises the ceiling.

## Contributing

Issues and pull requests are welcome.

```bash
cargo build --release          # the binary, in target/release/pepe
cargo test                     # 160+ tests, including the dashboard at many terminal sizes
cargo clippy --all-targets && cargo fmt --check
go run bench/server.go &       # then bench/run.sh, to measure a change (see bench/README.md)
assets/record.sh               # re-record the GIFs above with vhs (assets/tapes/)
```

Before a release that touches the screens, the installers, or files and paths, go through [docs/windows-checklist.md](docs/windows-checklist.md) on a Windows machine; CI can't press keys there.

Releases are automated. Commits follow [conventional commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `perf:`), and merging to `master` keeps a release PR open that bumps the version and writes the changelog from them; the body of each commit becomes its release note. Merging that PR tags the release, which builds every platform and publishes the GitHub Release, installers, the Homebrew formula and the pepe.mhaimdat.com mirror.

## Roadmap

Next up: a Docker image and a GitHub Action, soak mode and distributed runs; then latency by phase, an arrival-rate mode and data-driven requests, and thresholds that fail CI. The whole plan, in order, is in [ROADMAP.md](ROADMAP.md).

## License

MIT. See [LICENSE](LICENSE).

## Acknowledgements

[reqwest](https://github.com/seanmonstar/reqwest) and [tokio](https://github.com/tokio-rs/tokio) for the requests, [ratatui](https://github.com/ratatui/ratatui) and [crossterm](https://github.com/crossterm-rs/crossterm) for the dashboard, [clap](https://github.com/clap-rs/clap) for the command line, and [oha](https://github.com/hatoo/oha) and [vegeta](https://github.com/tsenart/vegeta) for being good company on the benchmark table.
