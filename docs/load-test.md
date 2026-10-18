# Load testing

`pepe URL` sends requests to a URL, as many at once as you ask, and shows what came back as it happens. This page is the plain run: its options, how the load is shaped, where the settings can live, and what to read when it ends. The screen itself is on [The dashboard](dashboard.md); the other modes have pages of their own.

## Quick start

Send 100 requests, one per core at a time, and watch:

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

## Options

| Option | Default | What it does |
| --- | --- | --- |
| `-n`, `--number <N>` | 100 | Requests to send |
| `-c`, `--concurrency <N>` | number of cores | Requests in flight at once |
| `-z`, `--duration <TIME>` | | Run for this long instead of a count: `30s`, `3m`, `2h` |
| `-m`, `--method <METHOD>` | GET | HTTP method |
| `-H`, `--headers <HEADER>` | | A header, `Name: value`; repeat for more |
| `-d`, `--body <TEXT>` | | Request body (a curl command's `-d @file` reads a file) |
| `-t`, `--timeout <SECONDS>` | 20 | Time to wait for a response (1 to 120) |
| `-u`, `--user-agent <UA>` | `pepe/<version>` | User-Agent header |
| `-p`, `--proxy <URL>` | | HTTP, HTTPS or SOCKS5 proxy, with credentials if needed |
| `-k`, `--insecure` | | Accept invalid TLS certificates |
| `--disable-compression` | | Don't ask for gzip |
| `--disable-keepalive` | | Open a new connection for every request |
| `--disable-redirects` | | Don't follow redirects |
| `--warmup <TIME>` | | Send for this long before counting anything ([Warm-up](#warm-up)) |
| `--threads <N\|auto>` | 1 | Threads sending requests; `auto` adds them as they're needed ([Threads](#threads)) |
| `--rate <PER_SECOND>` | | Start this many requests a second, spread evenly ([Arrival rate](#arrival-rate)) |
| `--curl` | | Read the request from a curl command ([A curl command](#a-curl-command)) |
| `-i`, `--setup` | | Open the setup screen filled in from the flags |
| `--json` | | No dashboard: run to completion and print a JSON report ([Output](output.md)) |
| `--snapshot <FILE>` | | Write the JSON report so far to this file every minute ([Soak runs](#soak-runs)) |
| `--metrics <ADDR>` | | Serve the live numbers for Prometheus at `http://ADDR/metrics` ([Output](output.md#prometheus-metrics)) |
| `--fail-if <CONDITION>` | | Exit 4 when the run crosses it: `'p99 > 300ms'`, `'errors > 1%'` ([Output](output.md#thresholds-that-fail-ci)) |
| `--config <FILE>` | `./pepe.toml` if present | Read settings from a file ([Config file](#config-file)) |
| `--write-config <FILE>` | | Write the settings as given to a file and exit |
| `--trace-header <NAME>` | | Response header holding the request id, if not one of the usual ones |
| `--vars <FILE>` | | Fill the `{{holes}}` in the URL, headers and body from a CSV file, a row a request |
| `--webhook <URL>` | | Send the verdict to Slack, Discord, Teams, Google Chat or any URL when the run ends ([Webhooks](webhooks.md)) |
| `--webhook-on <EVENTS>` | `end` | When: `start`, `progress`, `anomaly`, `end`, `fail` |
| `--html <FILE>` | | Write the report as a self-contained HTML page too ([An HTML page](output.md#an-html-page)) |
| `--slack[=CHANNEL]` | | One Slack message kept up to date as the run goes; `pepe slack connect` first ([Slack](webhooks.md#slack-live)) |

The full list, including every subcommand's own flags, is in the [command reference](reference.md).

## Headers, bodies and methods

```bash
pepe -n 1000 -c 20 -H 'Accept: application/json' -H 'X-Request-Id: load' https://example.com
pepe -n 1000 -c 10 -m PUT -d "$(cat payload.json)" -H 'Content-Type: application/json' https://example.com/items/1
```

Repeated headers are kept (several `Cookie` headers are sent as several), and a `User-Agent` given with `-H` wins over the default. A body's `Content-Length` is added unless a header already says how the body ends.

## Values from a file

`--vars users.csv` makes every request different: `{{name}}` holes in the URL, the headers and the body take their values from a CSV file whose first line names them. Each request takes the next row, in the file's order, and the rows go round again.

```csv
id,email,token
1,ada@example.com,aaa
2,"lin, the second",bbb
```

```bash
pepe -z 30s -c 20 --vars users.csv \
  -H 'Authorization: Bearer {{token}}' \
  -m POST -d '{"email": "{{email}}"}' \
  'https://api.example.com/users/{{id}}'
```

Values are put in as they are written, so a value that goes in a URL is written the way the URL needs it. A value with a comma, a quote or a line break in it is quoted, a quote inside doubled (RFC 4180). Every hole must be a column of the file, and the request must have one: either is an error that names it before anything is sent. A hole can be anywhere in the URL, the host included, and `--allow-host` checks every row's host. `pepe ramp` takes `--vars` the same way, and [a flow](flow.md#values-from-a-file) starts each chain with the next row's values. Without `--vars`, `{{` in a URL is sent as written.

The rows are made into requests before the run starts, so a request costs the same as one without values; each row takes about 1.6 KB of memory, and a file may have up to a million of them.

## Timed runs

`-z` keeps sending for a duration instead of a count. Pausing stops the clock, so a paused run still gets its full length of sending.

```bash
pepe -z 2m -c 100 https://example.com
```

## Warm-up

The first seconds of a run are not like the rest: connections open, caches fill, JITs settle. `--warmup 5s` sends at the run's concurrency for that long first and counts none of it: not in the numbers, the charts, the verdict or the report, and not against `-n` or `-z`, whose clock starts when the warm-up ends. The header says `warming up` with the time left while it goes, and the Stats tab and the JSON report (`generator.warmup_s`, `warmup_requests`) say how much was sent and not counted. A ramp has no warm-up: its first step is one.

```bash
pepe --warmup 5s -z 1m -c 50 https://example.com
```

## Arrival rate

Without `--rate`, pepe is a closed loop: each unit of concurrency sends its next request the moment the last one answers, so a slower target gets fewer requests. That finds the most a target can do. Real traffic doesn't wait for the target: users arrive at their own rate, and a target that slows down gets the same arrivals and a growing queue. `--rate` sends like that, an open loop:

```bash
pepe https://example.com --rate 500 -c 64 -z 2m
```

Starts are spread evenly over each second, across every sending thread. `-c` is then the most requests in flight at once, and it has to be enough: at 500 a second and 40 ms a response, 20 are in flight on average (Little's law), more when the target has a bad moment. When the concurrency can't carry the rate, the footer says so as it happens ("behind the rate: 410 of 500 req/s · all 8 in flight; try -c 25"), and the verdict says what was asked and what was sent. A schedule that falls more than a second behind is not caught up with a burst; those starts are counted as missed instead (`generator.rate_missed` in the JSON report), because a burst would say nothing true about the target. A paused run resumes on schedule.

## Threads

pepe sends from one thread, whatever the concurrency. One thread sends 150k to 400k plain requests a second, depending on the machine, or 10k TLS handshakes a second, at the lowest CPU per request, and that is more than most targets can take. When it isn't, pepe says so: the dashboard's footer shows how busy the sending thread is once it passes 90% of a core, the end-of-run verdict notes it, and the JSON report has it under `generator`. Then `--threads` adds more:

```bash
pepe -c 500 --threads 4 -z 30s http://localhost:8080/
pepe -c 500 --threads auto -z 30s http://localhost:8080/
```

`--threads auto` starts with one and adds another whenever those sending are all past 90% of a core, a second apart, up to one a core. A thread that doesn't pay for itself (a third of what a thread is worth at best) is taken back and no other is tried: then the limit is the machine, as when the target runs on the same cores. It is the setting for a script or an agent, which can't read the footer: the run finds the threads the target needs, and `generator.threads` in the report says how many that was. In `pepe.toml` it is `threads = "auto"`.

## Connections

Connections are kept alive and reused, one per unit of concurrency, as a client would hold them. `--disable-keepalive` opens a new connection for every request, which measures the connect and TLS handshake every time (the ping does that by default, with each phase apart: [Ping](ping.md)). Redirects are followed unless `--disable-redirects`; with a curl command, as with curl, they are followed only with `-L`. `--disable-compression` leaves `Accept-Encoding` off, so bodies come uncompressed and their size is the server's. `-k` accepts any certificate, still doing the handshake's work. A name that resolves to both IPv4 and IPv6 addresses is connected the way browsers do it, the resolver's first family given a head start.

## Proxies

HTTP, HTTPS and SOCKS5, with or without credentials:

```bash
pepe -n 1000 -c 10 -p http://proxy:port https://example.com
pepe -n 1000 -c 10 -p socks5://username:password@proxy:port https://example.com
```

`HTTP_PROXY`, `HTTPS_PROXY` and `ALL_PROXY` in the environment are honoured too.

## Soak runs

For a run that lasts hours, `--snapshot` writes the JSON report so far to a file every minute, whole or not at all, and once more when the run ends, so a crash at hour six or a lost terminal doesn't lose the numbers. The report carries a minute-by-minute timeline of the whole run (throughput, errors, p50, p90, p99), the dashboard's ten-minute charts still show the recent past, and the end-of-run verdict judges the whole run on those minutes. `--metrics :9100` serves the live numbers for Prometheus alongside ([Output](output.md#prometheus-metrics)).

```bash
pepe -z 6h -c 50 --snapshot soak.json https://example.com
jq '.timeline[-1], .snapshot' soak.json       # the last minute, and whether it is still running
```

## A curl command

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
- **Connection**: `-L` (redirects are only followed with `-L`), `-k`, `-x`, `-m`, `--no-keepalive`, `--url`, `--url-query`, bunched flags like `-sSLk` and attached values like `-XPOST`.

Output, logging and TLS options (`-o`, `-s`, `-v`, `-w`, `--cacert`, …) are accepted and have no effect. An unknown option is an error, and anything pepe can't reproduce (such as a cookie file) is reported as a note. The setup screen takes a pasted curl command the same way.

## The setup screen

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
- `ctrl-s` saves the form under a name, as `.pepe/tests/NAME.toml` next to the code; `pepe --load NAME` runs it again. The name it offers comes from the URL, and a taken name asks before it's replaced.
- `ctrl-s` on the dashboard and the ramp screen saves the run's settings the same way, with the concurrency as dialed in. A run started from a saved test offers its own name, so enter updates it in place, and another name makes a copy.
- `ctrl-o` opens the saved tests. When the project has any, `pepe` alone opens on them: a list with each test's mode, target, load and age, and the command it runs as. `enter` or a double click puts one in the form, `n` starts from an empty form, `c` copies the selected test under another name.
- The command card always shows the flags that reproduce the form. It's printed to your shell when you quit, and `e` in the dashboard brings you back to the form.
- `F1` (or `?` outside a text field) lists the keys; `esc` quits.
- The mouse works here as on every pepe screen: click a field to focus it, a mode tab to switch, or a key at the bottom to press it; the wheel moves between fields.

## Config file

A load test can live next to the code it tests. Put a `pepe.toml` in the directory and `pepe` alone runs it; flags on the command line win over the file, and the file wins over this machine's [settings](install.md#settings) and pepe's defaults.

```toml
url = "https://api.example.com/health"
method = "GET"
headers = ["Accept: application/json", "Authorization: Bearer ..."]
body = '{"ping": true}'
duration = "1m"        # or requests = 1000; duration wins when both are set
concurrency = 50
timeout = 5            # seconds
threads = "auto"       # or a number
rate = 500             # requests started per second
warmup = "5s"
trace-header = "X-Request-Id"
vars = "users.csv"     # values for the {{holes}}, a row a request
html = "report.html"   # the report as a page too
webhook = "${SLACK_WEBHOOK_URL}"   # or [[webhook]] tables: see Webhooks
user-agent = "pepe-ci"
proxy = "http://proxy:3128"
insecure = false
compression = true     # false is --disable-compression
keep-alive = true      # false is --disable-keepalive
redirects = true       # false is --disable-redirects
snapshot = "soak.json"

[ramp]                 # defaults for `pepe ramp`
from = 10
to = 200
step = 10
every = "15s"
until = ["p99 > 500ms", "errors > 1%"]

[api]                  # defaults for `pepe api`
spec = "openapi.yaml"
server = "https://staging.example.com"
auth = ["bearer:TOKEN"]
all = false
tag = ["Billing"]
only = ["GET /pets*"]
skip = ["/admin*"]
set = ["id=1,2,3"]
include-writes = false
```

Keys are the long flags' names; the on/off ones are said the positive way. A key pepe doesn't know is an error that names it. `--config FILE` reads another file; `--write-config FILE` writes the settings as given on the command line and exits.

### Saved tests

A project keeps more than one. `ctrl-s` on the setup screen asks for a name and writes the form as `.pepe/tests/NAME.toml`, the same format as `pepe.toml`, and `pepe --load NAME` runs it again, flags on the command line winning as usual; the file decides the mode, so a saved ramp runs as a ramp. A ping saves the same way, with a `[ping]` section for its targets, interval and SLO, and `ctrl-s` on its own screen. `--keep NAME` keeps the settings as given on the command line the same way, then runs: `pepe -n 1000 -c 10 https://api.example.com/health --keep api-health`. With `--load` and no name, `--keep` updates the loaded test in place, so `pepe --load api-health -c 20 --keep` keeps the new concurrency; with another name it makes a copy. (`--save FILE` is a ping's, and writes its report.) Once the project has saved tests, `pepe` alone opens on the list of them. The files are meant to be committed with the code, so a team shares its tests and CI runs them by name.

### Recorded runs

A saved test keeps its results too, when asked. `--record` on a run of a saved test writes the report when it ends, as `.pepe/runs/NAME/<moment>.json`; on the dashboard and the ramp screen, `w` does the same once the run has ended. The report carries the test's name and the command that reproduces it, secrets in headers masked, so any recorded run can be run again from the report alone.

```bash
pepe --load api-health --record            # run it, keep the report
pepe history                               # every saved test, with its last run
pepe history api-health                    # its runs, each against the one before
pepe compare api-health                    # the last two runs, as pepe compare says it
pepe rerun api-health --gate               # run, record, compare with the run before; exit 1 on a regression
```

`pepe rerun NAME` is `--load NAME --record` followed by the comparison; with `--json` the comparison is what's printed, the run's own report being on disk. In the library, `h` opens a test's recorded runs, each with what moved since the run before. Everything is plain JSON under `.pepe/runs`, so it can be committed with the tests, or kept out of git with a line in `.gitignore`.

## Environment

| Variable | Effect |
| --- | --- |
| `PEPE_NO_UPDATE_CHECK` | Set to anything to skip the once-a-day look for a newer release |
| `PEPE_GITHUB_TOKEN` | A GitHub token for `pepe self-update`, for forks or rate-limited CI |
| `PEPE_CACHE_DIR` | Where the update check keeps its answer (defaults to the OS cache directory) |
| `NO_COLOR` | Set to anything to draw without colour: selections in reverse video, the heatmap in shades, the verdict uncoloured |
| `PEPE_THEME` | A [theme](install.md#themes-and-colour) by name: `pepe`, `nord`, `dracula`, `gruvbox`, `tokyo-night`, `catppuccin`, `paper`, `terminal`, `light`, `none`. Unset, the theme saved in the settings, else what `COLORFGBG` and `COLORTERM` say of the terminal |
| `PEPE_CONFIG_DIR` | Where `settings.toml` is kept (defaults to `~/.config/pepe`) ([Settings](install.md#settings)) |
| `HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY` | Proxies, as curl reads them |

## Inside the service

A load test says how slow; the service's own traces say why. With `--otlp`, pepe takes the [OpenTelemetry](https://opentelemetry.io) spans a service exports and breaks its time down by operation: the query, the call to another service, the handler's own code.

```bash
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318 ./my-service &
pepe --otlp :4318 -n 2000 -c 16 http://localhost:8080/checkout
```

```
Where the server's time went  (2000 of 2000 requests traced · server median 41ms · p99 50ms)
  operation                                    calls/req    median       all   slowest
  shop: SELECT orders                               1.0   40.32ms       97%       98%
  shop: GET /checkout                               1.0   41.34ms        2%        1%
  shop: redis GET cart                              1.0     395µs        1%        1%
```

Each request goes out with a `traceparent` of its own, its sampled flag set, so a service with the usual parent-based sampler keeps every one of them; pepe listens at the address given for OTLP over HTTP (`/v1/traces`, protobuf or JSON), the way an SDK's exporter sends it, and keeps the spans of its own traces. When the run ends it waits for the last batch (an SDK exports every five seconds), then for each operation says how many calls a request made, how long a call took, and what share of the server's time was its own (less what it called), over all requests and over the slowest one percent. Something with a small share in a typical request and a large one in the slowest waits on something: a lock, a pool, a queue. The slowest requests in the report carry their trace id and what their time went on, and the verdict says the same in words. Two reports with spans [compare](compare.md) operation by operation.

The service needs W3C trace context propagation, which OpenTelemetry SDKs have on by default, and its exporter pointed at pepe: `OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318`, with `OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf` (or `http/json`; gRPC isn't taken) and `OTEL_EXPORTER_OTLP_COMPRESSION=none`. When no spans come, or spans come for other traces, or compressed, the verdict says which and what to set. Metrics and logs sent to the same address are answered and set aside. `--otlp` is for a plain run for now, and doesn't change how requests are sent otherwise: the trace id is written into the request as it goes out.

## What pepe measures

Min, max, mean, median, p90, p95, p99 and standard deviation of latency, and where that time goes: opening connections (TCP and TLS, once per connection), waiting for the first byte, and downloading the body, each with its own percentiles; requests per second and bytes per second; total data transferred; the error rate, with failures counted separately as non-2xx responses, connection errors and timeouts, and grouped by cause with the first response body of each, so the verdict says what a 503 actually said; status codes; cache hit rate from `X-Cache`, `CF-Cache-Status`, `X-Vercel-Cache` and similar headers; DNS lookup time, sampled once a second; `Server-Timing` segments, added up and held against the latency measured here; and the request ids the backend gave the slowest responses, from `X-Request-Id`, `traceparent`, `CF-Ray`, `X-Amzn-Trace-Id` and other common headers, or the one named with `--trace-header`.

## When the run ends

The header turns into a verdict: **Healthy**, **Degraded** or **Failing**, with findings such as failed requests (and what the first one said), two separate latency groups, a long tail, throughput or latency drifting over the run, pepe itself being the limit, or a rate it couldn't carry. The same summary is printed to your shell when you quit, so it stays in the scrollback. Piped or redirected, the run goes to its end and prints it without a screen; `--json` prints the report instead; `--fail-if` turns a limit into an exit code. All of that is on [Output and exit codes](output.md).
