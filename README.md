<div align="center">

<img src="assets/logo.svg" width="112" alt="Pepe, a pixel-art chili pepper">

# pepe

**An HTTP load generator with a live dashboard, built to cost less than the server it tests.**

[![CI](https://github.com/omarmhaimdat/pepe/actions/workflows/CI.yaml/badge.svg)](https://github.com/omarmhaimdat/pepe/actions/workflows/CI.yaml) [![Release](https://img.shields.io/github/v/release/omarmhaimdat/pepe?display_name=tag&color=brightgreen)](https://github.com/omarmhaimdat/pepe/releases/latest) [![Downloads](https://img.shields.io/github/downloads/omarmhaimdat/pepe/total?color=blue)](https://github.com/omarmhaimdat/pepe/releases) [![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE) ![Rust 1.85+](https://img.shields.io/badge/rust-1.85%2B-orange)

[Install](#install) · [Quick start](#quick-start) · [Usage](#usage) · [Dashboard](#the-dashboard) · [How it compares](#how-pepe-compares) · [Documentation](https://pepe.mhaimdat.com/docs/) · [Roadmap](ROADMAP.md) · [Contributing](#contributing)

</div>

![pepe load-testing a server: the live dashboard, the request log, and the verdict](assets/run.gif)

pepe sends requests to a URL, as many at once as you ask, and shows what came back as it happens: throughput, latency percentiles and a heatmap, status codes, failures by cause, and a log you can open any request from. When the run ends it gives a verdict in plain words. It takes a curl command as input, ramps load to find where a target stops keeping up, and load-tests every endpoint of an OpenAPI spec. `pepe ping` is the same engine at one request a second: a graph of a URL's latency over time, each ping split into DNS, connect, TLS, first byte and download.

It is also light. One thread sends 160k requests a second on an Apple M4 Pro and 400k on Linux, for less CPU and less memory per request than [wrk](https://github.com/wg/wrk), a quarter to a half of [oha](https://github.com/hatoo/oha)'s CPU and a tenth of its memory or less, and pepe tells you when it, rather than the target, is the limit.

## Highlights

- **`pepe ping`**: ping, with HTTP instead of ICMP, and a graph. A request a second to one URL or several, each split into DNS, connect, TLS, first byte and download, with loss, jitter and percentiles, the TLS session and the certificate's expiry, SLO limits that mark pings and fail the exit code, and JSON, JSON Lines or CSV when there's no terminal.
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
pepe ping [OPTIONS] <TARGET>...  a request a second, with a graph
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
| `--threads <N\|auto>` | 1 | Threads sending requests; `auto` adds them as they're needed (see [Threads](#threads)) |
| `--rate <PER_SECOND>` | | Start this many requests a second, spread evenly (see [Arrival rate](#arrival-rate)) |
| `--curl` | | Read the request from a curl command (see below) |
| `-i`, `--setup` | | Open the setup screen filled in from the flags |
| `--json` | | No dashboard: run to completion and print a JSON report |
| `--config <FILE>` | `./pepe.toml` if present | Read settings from a file (see [Config file](#config-file)) |
| `--write-config <FILE>` | | Write the settings as given to a file and exit |
| `--trace-header <NAME>` | | Response header holding the request id, if not one of the usual ones |
| `--metrics <ADDR>` | | Serve the live numbers for Prometheus at `http://ADDR/metrics`, e.g. `:9100` (see [Prometheus metrics](#prometheus-metrics)) |
| `--fail-if <CONDITION>` | | Exit 4 when the run crosses it: `'p99 > 300ms'`, `'errors > 1%'`; repeat for more (see [Thresholds that fail CI](#thresholds-that-fail-ci)) |

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
pepe -n 1000 -c 10 -m PUT -d "$(cat payload.json)" -H 'Content-Type: application/json' https://example.com/items/1
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

### Prometheus metrics

`--metrics :9100` serves the live numbers at `http://localhost:9100/metrics` while the run goes, in Prometheus's text form, so a soak run or a long ping shows up in Grafana next to the server's own metrics. It works with the dashboard and with `--json`, for plain runs, API mode, flows, replays and `pepe ping`; a ramp isn't served, its steps being the point of it.

```bash
pepe -z 6h -c 50 --metrics :9100 https://example.com
pepe ping https://example.com --metrics 127.0.0.1:9100
curl -s localhost:9100/metrics
```

A load run has `pepe_requests_total`, `pepe_requests_succeeded_total`, `pepe_requests_timed_out_total`, `pepe_requests_errored_total`, `pepe_responses_total{status}`, `pepe_failures_total{cause}`, `pepe_response_bytes_total`, `pepe_cache_hits_total`, `pepe_requests_per_second` and `pepe_errors_per_second` over the last second, `pepe_latency_seconds{quantile}` (0.5, 0.9, 0.95, 0.99) and `pepe_first_byte_seconds{quantile}` so far, the histogram `pepe_request_duration_seconds` with fixed buckets from 1 ms to 30 s, `pepe_concurrency` and `pepe_run_elapsed_seconds`. Every line carries `target="GET https://…"`, and in API mode, a flow or a replay the same again per row, with `row="GET /pets/{id}"`. A ping has `pepe_ping_sent_total`, `pepe_ping_answered_total`, `pepe_ping_lost_total`, `pepe_ping_timed_out_total`, `pepe_ping_slo_broken_total`, `pepe_ping_up`, `pepe_ping_last_seconds`, `pepe_ping_loss_ratio`, `pepe_ping_latency_seconds{quantile}`, `pepe_ping_jitter_seconds`, `pepe_ping_phase_seconds{phase}` (the median of dns, connect, tls, tls_resumed, ttfb and download), `pepe_ping_responses_total{status}`, `pepe_ping_tls_resumed_total`, `pepe_ping_cert_not_after_seconds` and the histogram `pepe_ping_duration_seconds`, each per `target`. The page is rendered once a second at most, whatever scrapes it.

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

- `tab` switches mode: **Single URL**, **Ramp** or **API**. What the modes share is kept. In API mode the spec is a file, a URL, or the OpenAPI document itself, pasted in.
- `↑` `↓` move between fields, `←` `→` change a choice, `enter` starts.
- Paste a curl command anywhere and the form is filled in from it.
- `ctrl-t` sends the request once and shows the response, to check it before the run.
- The command card always shows the flags that reproduce the form. It's printed to your shell when you quit, and `e` in the dashboard brings you back to the form.

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
| `?` | Show all keys |
| `q` / `Ctrl-C` | Quit; the table and the result are printed to your shell |

`--json` runs the ramp without a screen and prints every step, the findings, and `capacity` (`requests_per_second`, `concurrency`, `extrapolated`, `latency_doubles_at_concurrency`; `null` when the curve hadn't bent).

### Flows: a sequence of requests

`pepe flow` runs a chain of requests where a value from one response feeds the next: log in, take the token, use it. Each unit of concurrency is one user walking the steps in order with its own values, over and over; each step is a row on the dashboard, as endpoints are in API mode, with its own throughput, latency and statuses.

```bash
pepe flow checkout.toml -c 20 -z 1m
```

```toml
# checkout.toml
name = "checkout"

[vars]
host = "https://shop.example.com"

[[step]]
name = "login"
method = "POST"
url = "{{host}}/login"
headers = ["Content-Type: application/json"]
body = '{"user": "demo", "password": "demo"}'
capture = { token = "json:$.token", session = "header:Set-Cookie" }

[[step]]
name = "cart"
url = "{{host}}/cart"
headers = ["Authorization: Bearer {{token}}"]
capture = { cart = "json:$.items[0].id" }

[[step]]
name = "checkout"
method = "POST"
url = "{{host}}/cart/{{cart}}/checkout"
headers = ["Authorization: Bearer {{token}}"]
expect = 201
```

`{{name}}` holes are filled from `[vars]` and from earlier steps' captures; a hole nothing fills is an error when the file is read, naming the step and the variable. A capture is `json:$.path.to[0].value`, `header:Name`, `regex:pattern` (the first group) or `body`. A step passes when it gets a 2xx, or the status `expect` names; a step that fails, or whose capture finds nothing, ends the chain with that said in the failure causes, and the user starts over. `-n` counts chains, not requests; `-c`, `-z`, `-H` (sent with every step), `-t`, `--rate` (which paces every request, steps included) and the other options work as usual. `--json` prints the usual report plus `flow.steps`, one entry per step, and how many chains started and completed.
### Replaying an access log

Real traffic is not one URL. `pepe replay` reads an access log and sends its URLs in the proportions the log had: a path seen 3,000 times gets 30× the requests of one seen 100 times, mixed evenly rather than in bursts.

```bash
pepe replay access.log --base-url https://staging.example.com -c 50 -z 2m
```

nginx and Apache logs (common and combined), Caddy's JSON lines, AWS ALB logs, and plain lists of one URL or path per line are read. `--base-url` goes in front of paths and replaces the host of full URLs, so production's log can be sent at staging; without it, full URLs are sent where they point and paths can't be sent at all. Only GET, HEAD and OPTIONS are replayed unless `--include-writes` is given. Request bodies aren't in access logs, so writes go without one.

The dashboard's first tab lists the most frequent URLs (`--rows`, 20 by default) with each one's share of the log, throughput, latency and statuses, and one row for all the rest. `--json` adds `replay`: what the log had, what was left out and why (unparsed lines, writes, paths with no host, URLs past the 5,000 most frequent), and the same per-URL numbers.

### Reading nginx logs

![Reading nginx logs: the dashboard with the rate now and a verdict, then traffic by the hour, paths, errors and the log](assets/logs.gif)

What the server already knows about its traffic: `pepe logs` reads nginx's access and error logs and says how busy the server is now against how busy it has been.

```bash
pepe logs /var/log/nginx/access.log /var/log/nginx/error.log
zcat access.log.*.gz | pepe logs -
docker compose logs -f -n 1000 nginx | pepe logs
pepe logs access.log --since 24h --json > traffic.json
```

At a terminal a log that is being written is shown live: the screen starts five minutes back, so that now is right at once, and follows the files as they are written, through rotation. `--since 24h` starts further back and `--all` reads everything first; either way the place to start is found in the file without reading what is before it. A log whose last line is older than five minutes has no now, and is read whole. Piped out, or with `--json`, the files are read to the end and a report is printed. Lines are counted as a stream, so memory doesn't grow with the log: every second of the last hour, a day of minutes, ninety days of hours and ten years of days are kept. What a file already has is read by every core at once, some gigabytes a second, and what is appended after that by one. Rotated files can be given in any order. What is piped in is followed too, until its writer ends, and the `nginx-1  | ` that `docker compose logs` puts in front of each line is left out, with the colour it writes into a pipe.

**Now** is the request rate over the last minute (`--window`) of the log's own timestamps. A log whose last line is older than five minutes isn't being written, and is held at its last line instead of the clock. Each minute, hour and day has its requests, its rate, its busiest second, its 4xx and 5xx shares, its mean request time and its error log lines, and how now compares: `+12%`, `×3.4`, `÷2.5`. The cards on top say what a usual slot sees (the median), which was the busiest, and what the same minute an hour ago, the same hour a day ago or the same day a week ago saw.

| View | Shows |
| --- | --- |
| **Dashboard** | Opens first. The rate now, the share answering 5xx and the request time drawn large; a verdict in a word (Steady, Busy, Quiet, Degraded, Failing) with what fails and what the error log says of it; traffic as a bar for every few seconds of the last minutes or hour, 4xx and 5xx in their colours; the top paths, the status codes, the error log's messages and the newest lines |
| **Traffic** | A bar per slot with the rate now drawn across them, and the table of slots; `m`, `h`, `d` switch between minutes, hours and days |
| **Paths** | The paths by requests, 5xx, 4xx or mean time (`s`), with status codes, clients, user agents, methods and query parameter names beside them |
| **Errors** | The error log's messages grouped by cause, most frequent first, each with the first line that said it; the paths answering 5xx and 4xx |
| **Log** | The last 2,000 lines of all the files in time order; `x` keeps failures, `/` searches, `enter` shows everything read from a line, query parameters one by one |

Access logs are read as nginx's `combined` format (Apache's too), with `rt=` and `urt=` timings after it if they are there, or as JSON lines under nginx's variable names or Caddy's. A log with a `log_format` of its own needs it said, as nginx.conf has it, on one line:

```bash
pepe logs access.log --format '$remote_addr [$time_local] "$request" $status $body_bytes_sent $request_time $upstream_response_time'
```

`$time_local`, `$time_iso8601`, `$msec`, `$request` (or `$request_method` and `$request_uri`), `$status`, `$body_bytes_sent`, `$remote_addr`, `$host`, `$http_user_agent`, `$request_time` and `$upstream_response_time` are used; the rest are shown when a line is opened. Lines that couldn't be read are counted and the first is shown. Numbers and ids in a path count as one (`/items/*`) unless `--exact-paths` is given. The error log names no time zone, so its times are taken to be this machine's.

### Pinging a URL

![pepe ping: two targets on the graph, then the phases of one, then the pings themselves with one opened](assets/ping.gif)

Ping, with HTTP instead of ICMP, and a graph. `pepe ping` sends one request every second to each target and draws its latency over time, with each ping split into where its time went: the DNS lookup, the TCP connect, the TLS handshake, the wait for the first byte and the download. It is what to leave open in a corner of the screen while something is deployed, and the on-ramp to a load test: the same `-H`, `-m`, `-d`, `-k`, `-p` and `-t` apply.

```bash
pepe ping https://example.com                       # a request a second, forever; q leaves the summary
pepe ping api.example.com cdn.example.com --name api,cdn   # several, each a line on the graph
pepe ping https://example.com --every 200ms --window 5m    # faster, with five minutes on screen
pepe ping https://api.example.com/health --slo total=300,ttfb=100 --bell   # limits; exit 4 if broken
pepe ping example.com --all-ips -4                  # every IPv4 address it resolves to, each its own line
pepe ping db.internal --tcp --port 5432             # only connect: a TCP ping of the port
pepe ping 10.0.0.0/29:8080                          # every host of a range
pepe ping --cmd 'dig example.com' 'curl -s https://example.com'   # commands, timed
pepe ping aws:eu-west-1 aws:us-east-1               # cloud regions, by shorthand
pepe ping https://example.com --jsonl > pings.jsonl # no screen: one JSON object per ping
pepe ping https://example.com --csv -n 100          # one CSV line per ping, a hundred of them
pepe ping https://example.com --json -n 10          # the summary as JSON
pepe ping https://example.com --once                # three quick pings, then why it is slow, and out
```

A host without a scheme is `https://`, unless it has a port or is this machine. Each ping opens its own connection, so every phase is measured every time, and the TLS session is still resumed when the server allows it: the first handshake is full, the next ones resumed, and the report says how long each kind takes. `--keep-alive` keeps the connection instead, as a browser would, and the pings after the first measure only the server. Redirects are followed, each hop listed with its status and time. `-n` and `-z` end the run; without them it runs until `q`.

| View | Shows |
| --- | --- |
| **Graph** | Each target's latency as a line, failures marked on top (`✖`), SLO breaks (`▲`) and 5xx (`!`) too; `+` and `-` zoom, `w` the whole run, `0` a floor at zero, `l` a log scale, `s` dots instead of braille, `f` hides the marks |
| **Phases** | Where the picked target's last and median ping went, as a stacked bar per phase, with curl's running totals (`namelookup`, `connect`, `pretransfer`, `starttransfer`, `total`); the TLS version, cipher and ALPN, full against resumed handshakes; the certificate's subject, issuer and expiry; the addresses at both ends |
| **Pings** | Every ping, newest last, with each phase; `x` keeps failures and SLO breaks, `a` one target or all, `enter` opens one: its hops, headers, TLS and certificate, and the body with `--show-body` |

The table above the views has each target's last, min, avg, max, jitter, p95, p99, loss and timeouts over what the graph shows, or over the whole run with `t`. `space` pauses. The summary left in the shell has the same, with the medians of each phase and the certificate's expiry; `--save report.json` writes it as JSON too, and `--save-body page.html` the last body.

**What to look at.** The numbers come with what they mean, in the summary, in the phases view and under `findings` in the JSON: the DNS lookup is a third of every request; connecting is a round trip of 90 ms, so the server is far; the TLS session is never resumed, so every connection pays the full handshake; TLS sessions are resumed after the first; every request is redirected, point at the final URL; the server takes 40 ms of 44 to start answering, and its `Server-Timing` says where; the body isn't compressed; it was answered by a cache; the certificate expires in 9 days; one ping in a hundred is five times slower. `pepe ping URL --once` is the quick version: three pings, the last one's phases, what answered, the findings, and out.

```
pepe ping · https://example.com/ · 3 pings
  dns 1.84ms → connect 49.88ms → tls 66.57ms → first byte 63.72ms → download 33µs · 182.5ms in all
  HTTP/1.1 200 · text/html · 334 B · 104.20.23.154:443 from 192.168.0.164 · TLS 1.3 TLS13_AES_256_GCM_SHA384
  certificate for example.com by SSL Corporation, expires in 76 days
  ▲ TLS sessions are resumed: 66.82ms after the first handshake's 118.6ms, still 66.82ms of 190.0ms
  · answered by a cache (cf-cache-status: HIT): the server itself wasn't measured
```

Two ping reports can be held against each other with `pepe compare before.json after.json`, phase by phase (see [Comparing two runs](#comparing-two-runs)). `--slo total=500,ttfb=200,connect=100,dns=50,tls=150,download=100`, in milliseconds, marks a ping that goes over any of them, counts them in the report, and makes the exit code 4. The exit code is 1 when nothing ever answered. `--bell` rings the terminal on a failed or slow ping. `--interface en0` sends from that interface, `-4` and `-6` pick the address family, `--tcp-rst fail` makes a refused connection a failure in `--tcp` mode rather than the answer it is by default, `--color red,#8cb8ff` colours the lines, and `--ymin` and `--ymax` fix the graph's range. Piped, or with `--jsonl`, `--csv` or `--json`, there is no screen: a line per ping as it happens, then the summary on stderr, or the JSON report on stdout.

### API mode: load-testing an OpenAPI spec

![API mode: the endpoints of a spec picked on screen, then a dashboard with a row per endpoint](assets/api.gif)

`pepe api` reads an OpenAPI 3 (or Swagger 2) spec, from a file or a URL, in JSON or YAML, and turns its operations into requests. With nothing after it, it opens the setup screen and asks for the spec: type a path or a URL, or paste the whole document.

```bash
pepe api
pepe api openapi.yaml
pepe api https://api.example.com/openapi.json --auth bearer:$TOKEN -c 20 -z 1m
```

Once the spec is loaded, it opens on a plan screen. Nothing is sent, and no endpoint is switched on, until you say so.

- Endpoints are listed under the spec's tags. `space` switches an endpoint on or off, or a whole tag; `/` filters the list.
- `enter` on an endpoint goes to its parameters: path, query, header and cookie parameters with their type, description and the values the spec allows, then the body and the endpoint's share of the traffic. `enter` edits one, `space` steps through the spec's values, `del` leaves it out. Several values (`a, b`) are sent in turn, or together for array parameters.
- The request is shown as it will go out, and `t` sends it once and shows the answer.
- If the spec declares authentication and none was given, pepe asks for it, checks it with one request, and shows credentials masked from then on.
- `c`, `n`, `z` change concurrency, requests and duration; `u` the server; `a` the credentials; `g` starts the run.
- Endpoints that still need a value, and writes (POST, PUT, PATCH, DELETE), are never switched on in bulk: you switch those on one by one.

The run is the usual dashboard, with an **Endpoints** view in front: requests, throughput, p50, p99, errors and status codes for each endpoint. `enter` on one shows its requests, and `e` goes back to the plan.

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

pepe sends from one thread, whatever the concurrency. One thread sends 150k to 400k plain requests a second, depending on the machine, or 10k TLS handshakes a second, at the lowest CPU per request, and that is more than most targets can take. When it isn't, pepe says so: the dashboard's footer shows how busy the sending thread is once it passes 90% of a core, the end-of-run verdict notes it, and the JSON report has it under `generator`. Then `--threads` adds more:

```bash
pepe -c 500 --threads 4 -z 30s http://localhost:8080/
pepe -c 500 --threads auto -z 30s http://localhost:8080/
```

`--threads auto` starts with one and adds another whenever those sending are all past 90% of a core, a second apart, up to one a core. A thread that doesn't pay for itself (a third of what a thread is worth at best) is taken back and no other is tried: then the limit is the machine, as when the target runs on the same cores. It is the setting for a script or an agent, which can't read the footer: the run finds the threads the target needs, and `generator.threads` in the report says how many that was. In `pepe.toml` it is `threads = "auto"`.

### Thresholds that fail CI

`--fail-if` names what the run must not cross, in the ramp's words, and ends with exit code 4 when it does, the report still printed and the condition said on stderr. Repeat it for more than one; every mode that measures requests takes it (a plain run, a ramp, API mode, a flow, a replay, with or without `--json`). `pepe ping` has `--slo` for the same.

```bash
pepe --json -n 2000 -c 20 --fail-if 'p99 > 300ms' --fail-if 'errors > 1%' https://staging.example.com/api
```

```
✖ --fail-if p99 > 300ms: p99 was 412.0ms
```

Exit codes, across pepe: 0 when the run went as asked, 1 when it couldn't start or nothing answered, 2 for a usage error, 4 when a limit was crossed (`--fail-if`, a ping's `--slo`), and `pepe compare --gate` exits 1 on a regression.

### Without a terminal

Piped, redirected, or run by a script, pepe doesn't try to draw: the run goes to its end and the report the dashboard would have left is printed, the same verdict and findings, so a forgotten `--json` costs nothing. `--snapshot` and `--metrics` work as they do on the dashboard. A ramp prints its steps and estimate; API mode needs its endpoints picked on the command line (`--all`, `--tag`, `--only`), since there is no plan screen to pick them on.

```bash
pepe -z 30s -c 20 https://example.com > run.txt
pepe ramp https://example.com --to 200 | tee ramp.txt
```

### JSON output

`--json` skips the dashboard, runs to completion and prints a report to stdout. Press Ctrl-C to stop early; the report then has `"interrupted": true`. Every report starts with `"schema_version": 1`: fields are added within a version and never renamed, and the JSON Schema of each report ships with every release and is printed by `pepe schema` (`run`, `ramp`, `ping` or `compare`), so a script or an agent can depend on the names.

```bash
pepe schema run > run.schema.json
pepe schema ping | jq '.properties.targets.items.required'
```

```bash
pepe --json -n 1000 -c 20 https://example.com > results.json
jq '.summary.latency.p99_ms' results.json
```

```json
{
  "schema_version": 1,
  "target": { "mode": "run", "method": "GET", "url": "https://example.com", "concurrency": 20 },
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

### Comparing two runs

`pepe compare` holds a report against an earlier one of the same test and says what moved, in the verdict's words. A number is only called a change when it moved more than two runs like these wobble on their own: the run's own latency spread, scaled by how many requests back the number, so a p99 from 200 requests is given more room than one from 20,000.

```bash
pepe --json -n 5000 -c 20 https://staging.example.com/api > before.json
# ... deploy ...
pepe --json -n 5000 -c 20 https://staging.example.com/api > after.json
pepe compare before.json after.json
```

```
pepe · compare before.json → after.json
▲ Slower · 5,000 → 5,000 requests · p99 120.0ms → 166.0ms · 260 req/s → 252 req/s
  ▲ p99 up 38%: 120.0ms → 166.0ms
  ✔ Median within the usual spread: 30.00ms → 31.00ms (±5%)
  ✔ Throughput within the usual spread: 260 req/s → 252 req/s (±5%)
  ▲ A long tail is new: p99 is 5.4× the median, was 4.0×
```

The verdict is one of **Faster**, **About the same**, **Slower**, and, when failures appeared or rose, **Worse** (or **Better** when they fell): failures outrank speed. Two reports of different targets or concurrency are compared all the same, with that said first. Ramp reports compare their capacity estimate and the level that held. Ping reports (`pepe ping --json`, or `--save`) compare the median, the p99, the pings lost, and each phase's median, so a slower connect or a slower handshake is told from a slower server:

```
pepe · compare before.json → after.json
▲ Slower · 600 → 600 pings · p99 50.00ms → 72.00ms
  ▲ Median up 50%: 40.00ms → 60.00ms
  ▲ Connect up 150%: 10.00ms → 25.00ms
  ▲ TLS handshake up 60%: 25.00ms → 40.00ms
  ✔ First byte within the usual spread: 6.00ms → 6.00ms (±5%)
``` `--gate` exits 1 on Slower or Worse, for CI, and `--json` prints the verdict, each number before and after with its change and the spread it was held against, the findings, and the verdict as a badge's three parts (`pepe | slower · p99 +38%`, in a colour) for shields.io and the like.

`--svg card.svg` also draws the verdict as a card, the way the dashboard draws, for a README, a site or a report:

<img src="assets/compare-card.svg" width="460" alt="pepe · compare: Slower. p99 10.14ms → 22.21ms, up 119%; median 6.51ms → 18.11ms, up 178%; throughput 1.2k → 439 req/s, down 63%; failed 0% → 0%">

### In GitHub Actions

The repository is also an action: it installs a pinned release, runs `pepe --json`, puts the numbers in the job summary and in outputs, and can fail the job on a condition over the report.

```yaml
- uses: omarmhaimdat/pepe@v0
  id: load
  with:
    url: https://staging.example.com/api/health
    args: -z 30s -c 20 -H 'Authorization: Bearer ${{ secrets.TOKEN }}'
    fail-if: ".summary.latency.p99_ms > 300 or .summary.failed_requests > 0"
- run: echo "p99 was ${{ steps.load.outputs.p99_ms }} ms at ${{ steps.load.outputs.requests_per_second }} req/s"
```

Outputs: `total_requests`, `failed_requests`, `requests_per_second`, `p50_ms`, `p99_ms`, and `report`, the path of the JSON. `version` pins a release (`0.9.0`); the default is the latest. Linux and macOS runners.

**Against the base branch.** With `baseline: auto`, every run on a branch keeps its report in the Actions cache, and a pull request is held against its base branch's last one with `pepe compare`. `comment: true` posts the result on the pull request, one comment updated on every push, and `gate: true` fails the step when it says Slower or Worse. The first run on the base branch after this is added makes the baseline; until then a pull request's comment says so.

```yaml
permissions:
  pull-requests: write
steps:
  - uses: omarmhaimdat/pepe@v0
    with:
      url: https://staging.example.com/api/health
      args: -n 5000 -c 20
      baseline: auto
      comment: true
      gate: true
```

> ### pepe · ▲ Slower than `main` · https://staging.example.com/api/health
> | | `main` | this PR | |
> |---|---|---|---|
> | p99 | 120.0 ms | 166.0 ms | ▲ up 38% |
> | median | 30.0 ms | 31.0 ms | within the usual spread (±5%) |
> | throughput | 260 req/s | 252 req/s | within the usual spread (±5%) |
> | failed | 0% | 0% | |
>
> - ▲ p99 up 38%: 120.0ms → 166.0ms
> - ▲ A long tail is new: p99 is 5.4× the median, was 4.0×

`baseline` can also name a report file, for a baseline kept in the repository or fetched from elsewhere. The comparison is in the job summary too, and in three more outputs: `verdict` (`faster`, `same`, `slower`, `better`, `worse`), `compare`, the path of `pepe compare --json`'s output, and `card`, the verdict drawn as an SVG, which the run keeps in its artifacts. The comment carries the verdict as a badge, since a comment can only show an image by URL.

### Proxies

HTTP, HTTPS and SOCKS5, with or without credentials:

```bash
pepe -n 1000 -c 10 -p http://proxy:port https://example.com
pepe -n 1000 -c 10 -p socks5://username:password@proxy:port https://example.com
```

### For scripts and agents

Every mode runs without a screen, prints a versioned JSON report with a published schema, and says what went wrong in an exit code; [AGENTS.md](AGENTS.md) is what an agent reads first. Guardrails hold a run where it may go and how much it may send, checked before anything is sent: `--allow-host .example.com` (the hosts a target may have), `--max-requests`, `--max-rate` (which needs a `--rate`), `--max-concurrency`, and `--dry-run`, which says what would be sent, with secrets masked, and sends nothing. A refusal is exit code 2. They can live in `pepe.toml` (`allow-host`, `max-requests`, `max-rate`, `max-concurrency`), where leaving a flag off the command line can't loosen them.

```bash
pepe --allow-host .staging.example.com --max-requests 5000 --max-rate 500 --json -z 10s --rate 200 https://api.staging.example.com/health
pepe --allow-host .staging.example.com --dry-run -n 100 https://api.staging.example.com/health
```

### Environment

| Variable | Effect |
| --- | --- |
| `PEPE_NO_UPDATE_CHECK` | Set to anything to skip the once-a-day look for a newer release |
| `PEPE_GITHUB_TOKEN` | A GitHub token for `pepe self-update`, for forks or rate-limited CI |
| `PEPE_CACHE_DIR` | Where the update check keeps its answer (defaults to the OS cache directory) |
| `NO_COLOR` | Set to anything to draw without colour: selections in reverse video, the heatmap in shades, the verdict uncoloured |
| `PEPE_THEME` | `light` or `dark`, to match the terminal's background. Unset, pepe reads `COLORFGBG` where the terminal sets it, else assumes dark |

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
| `e` | Back to the setup screen (or, in API mode, the plan), to change the settings and run again |
| `tab` / `←` `→` / `1` `2` `3` | Switch view |
| `↑` `↓` / `j` `k`, `PgUp` `PgDn`, `home` `end` | Select a request in the log (newer / older) |
| `f` | Filter requests by status: 2xx, 3xx, 4xx, 5xx, no response, failed |
| `l` | Filter requests by latency: at or above p50, p90 or p99 |
| `/` | Search the status and response text |
| `x` / `c` | Show only failed requests / clear all filters |
| `enter` | Inspect the selected request: `↑`/`↓` (or the trackpad), `u`/`d` and `g`/`G` scroll the response, `←`/`→` walk to the newer/older request, `[`/`]` jump to the nearest one kept in full, `v` switches between formatted and raw, `esc` goes back |
| `?` | Show all keys, here and on the setup (`F1`), ramp and API screens |
| `q` / `esc` / `Ctrl-C` | Quit |

### What pepe measures

Min, max, mean, median, p90, p95, p99 and standard deviation of latency, and where that time goes: opening connections (TCP and TLS, once per connection), waiting for the first byte, and downloading the body, each with its own percentiles; requests per second and bytes per second; total data transferred; the error rate, with failures counted separately as non-2xx responses, connection errors and timeouts, and grouped by cause with the first response body of each, so the verdict says what a 503 actually said; status codes; cache hit rate from `X-Cache`, `CF-Cache-Status`, `X-Vercel-Cache` and similar headers; and DNS lookup time, sampled once a second.

## How pepe compares

Measured with the suite in [`bench/`](bench/) against a local server that answers from memory, so the client is the cost being measured. CPU milliseconds per 1,000 requests, peak memory, and the requests per second each tool reported:

| Workload | pepe | oha | vegeta |
| --- | --- | --- | --- |
| GET, 64 connections | **6.2 ms · 9 MB** · 160k req/s | 23.0 ms · 78 MB · 161k req/s | 90.4 ms · 26 MB · 76k req/s |
| GET, 1,000 connections | **6.0 ms · 12 MB** · 163k req/s | 21.6 ms · 101 MB · 147k req/s | 52.7 ms · 92 MB · 131k req/s |
| HTTPS, 64 connections | **6.8 ms · 12 MB** · 142k req/s | 24.8 ms · 60 MB · 156k req/s | 69.8 ms · 30 MB · 90k req/s |

Those are single-thread numbers for pepe on an Apple M4 Pro, where the loopback tops out near 175k requests a second. On Linux (a 4-vCPU arm64 VM, the musl build that is released), with wrk on one thread beside it:

| Workload | pepe | wrk | oha |
| --- | --- | --- | --- |
| GET, 64 connections | **2.4 ms · 4.0 MB** · 398k req/s | 3.5 ms · 4.5 MB · 282k req/s | 6.8 ms · 67 MB · 305k req/s |
| GET, 1,000 connections | **3.2 ms · 5.8 MB** · 296k req/s | 4.4 ms · 7.7 MB · 228k req/s | 5.6 ms · 73 MB · 183k req/s |
| HTTPS, 64 connections | **3.3 ms · 6.0 MB** · 284k req/s | 4.5 ms · 10.8 MB · 218k req/s | 8.1 ms · 42 MB · 250k req/s |
| 10 million requests, 256 connections | **2.9 ms · 4.5 MB** · 343k req/s | 3.3 ms · 4.6 MB · 304k req/s | 6.5 ms · 2,404 MB · 316k req/s |

Every workload, where pepe is level rather than ahead (a slow target at 1,000 connections, 16 KB bodies over TLS), what a glibc build changes, k6, the profiles and the method are in [bench/README.md](bench/README.md). Where a target can take more than one thread sends, pepe says so, and `--threads auto` adds them.

## Documentation

Everything here and more, page by page, at [pepe.mhaimdat.com/docs](https://pepe.mhaimdat.com/docs/): the sources are the Markdown files under [docs/](docs/), and `python3 site/build-docs.py` builds the site into `site/docs/` (CI checks it is current). The command reference there is generated from the command definition by `cargo test`.

## Contributing

Issues and pull requests are welcome.

```bash
cargo build --release          # the binary, in target/release/pepe
cargo test                     # 160+ tests, including the dashboard at many terminal sizes
cargo clippy --all-targets && cargo fmt --check
cargo run --release --manifest-path bench/server/Cargo.toml &   # then bench/run.sh, to measure a change (see bench/README.md)
assets/record.sh               # re-record the GIFs above with vhs (assets/tapes/)
```

Before a release that touches the screens, the installers, or files and paths, go through [docs/windows-checklist.md](docs/windows-checklist.md) on a Windows machine; CI can't press keys there.

Releases are automated. Commits follow [conventional commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, `perf:`), and merging to `master` keeps a release PR open that bumps the version and writes the changelog from them; the body of each commit becomes its release note. Merging that PR tags the release, which builds every platform and publishes the GitHub Release, installers, the Homebrew formula and the pepe.mhaimdat.com mirror.

## Roadmap

Next up: thresholds that fail CI, a versioned JSON report and comparing two runs; then data-driven requests, closing the CPU gap on Linux, and an MCP server and guardrails for agents. The whole plan, in order, is in [ROADMAP.md](ROADMAP.md).

## License

MIT. See [LICENSE](LICENSE).

## Acknowledgements

[tokio](https://github.com/tokio-rs/tokio), [rustls](https://github.com/rustls/rustls), [httparse](https://github.com/seanmonstar/httparse) and [reqwest](https://github.com/seanmonstar/reqwest) for the requests, [ratatui](https://github.com/ratatui/ratatui) and [crossterm](https://github.com/crossterm-rs/crossterm) for the dashboard, [clap](https://github.com/clap-rs/clap) for the command line, and [oha](https://github.com/hatoo/oha), [vegeta](https://github.com/tsenart/vegeta), [wrk](https://github.com/wg/wrk) and [k6](https://github.com/grafana/k6) for being good company on the benchmark table.
