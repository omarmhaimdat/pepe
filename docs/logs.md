# Reading nginx logs

![Reading nginx logs: the dashboard with the rate now and a verdict, then traffic by the hour, paths, errors and the log](assets/logs.gif)

What the server already knows about its traffic: `pepe logs` reads nginx's access and error logs and says how busy the server is now against how busy it has been.

```bash
pepe logs /var/log/nginx/access.log /var/log/nginx/error.log
pepe logs                                      # nginx's own, in /var/log/nginx
pepe logs /var/log/nginx/access.log*             # rotated ones too, .gz as they are
docker compose logs -f -n 1000 nginx | pepe logs
pepe logs access.log --since 24h --json > traffic.json
pepe logs access.log --path '/api/*' --status 5xx --since 2h --until 1h
```

## How it reads

At a terminal a log that is being written is shown live: the screen starts five minutes back, so that now is right at once, and follows the files as they are written, through rotation. `--since 24h` starts further back and `--all` reads everything first; either way the place to start is found in the file without reading what is before it. A log whose last line is older than five minutes has no now, and is read whole. Piped out, or with `--json`, the files are read to the end and a report is printed.

Lines are counted as a stream, so memory doesn't grow with the log: every second of the last hour, a day of minutes, ninety days of hours and ten years of days are kept, with capped tables of paths, clients and user agents. What a file already has is read by every core at once, some gigabytes a second, and what is appended after that by one. Rotated files can be given in any order, and gzip'd ones (`access.log.2.gz`) are read as they are, unpacked as they're read, on one thread since a gzip stream can't be taken up in the middle. What is piped in is followed too, until its writer ends, and the `nginx-1  | ` that `docker compose logs` puts in front of each line is left out, with the colour it writes into a pipe.

## Keeping only some lines

| Flag | Keeps |
| --- | --- |
| `--path '/api/*'` | Requests to that path, `*` for any run of characters; repeat for more. Matched against the path as written, without its query |
| `--status 5xx` | Requests that answered that: a code (`404`), a class (`5xx`), or several (`4xx,503`); repeat for more |
| `--host .example.com` | Requests to that `$host`: `shop.test` exactly, `.example.com` for it and every host under it; a log whose format has no `$host` has nothing to keep |
| `--until 1h` | What was written up to an hour ago; with `--since 2h`, the hour before last. A log read up to a time in the past isn't followed |

Every view, the findings and the report count only what the filters keep, and the title says what that is (`access.log · /api/* · 5xx`). An error log line is kept when its request and its host are (nginx writes both in it); a status says nothing of it. With `--status`, a share answering 5xx says nothing either, and the findings leave shares out. The report counts the lines left out (`lines_filtered_out` in JSON).

## Now against before

**Now** is the request rate over the last minute (`--window`) of the log's own timestamps. A log whose last line is older than five minutes isn't being written, and is held at its last line instead of the clock. Each minute, hour and day has its requests, its rate, its busiest second, its 4xx and 5xx shares, its mean request time and its error log lines, and how now compares: `+12%`, `×3.4`, `÷2.5`. The cards on top say what a usual slot sees (the median), which was the busiest, and what the same minute an hour ago, the same hour a day ago or the same day a week ago saw.

## Views

| View | Shows |
| --- | --- |
| **Dashboard** | Opens first. The rate now, the share answering 5xx and the request time drawn large; a verdict in a word (Steady, Busy, Quiet, Degraded, Failing) with what fails and what the error log says of it; traffic as a bar for every few seconds of the last minutes or hour, 4xx and 5xx in their colours; the top paths, the status codes, the error log's messages and the newest lines |
| **Traffic** | A bar per slot, split by the busiest paths in their colours, with the rate now drawn across them; beside it, the picked slot's paths with their rate, how that holds against their usual, their 5xx and their mean time; under it, the table of slots with their p50, p99 and busiest path. `m`, `h`, `d` switch between minutes, hours and days; `p` holds it to one path |
| **Paths** | The paths by requests, 5xx, 4xx or mean time (`s`), with status codes, clients, user agents, methods and query parameter names beside them |
| **Errors** | The error log's messages grouped by cause, most frequent first, each with the first line that said it; the paths answering 5xx and 4xx |
| **Log** | The last 2,000 lines of all the files in time order; `x` keeps failures, `/` searches, `p` keeps one path's, `enter` shows everything read from a line, query parameters one by one |

### One path

`p` on the traffic, the paths or the log opens a picker: type a part of a path, letters in order are enough (`srch` finds `/search`), and `enter` holds the screen to it.

- **Traffic** is then that path's alone: its bars with its 5xx, its rate now against its usual minute, hour or day, and a table of slots with its requests, failures, mean time and share of all the traffic. Beside the chart, its status codes, its p50, p95 and p99, and how much of its time was spent waiting on the upstream rather than in nginx (from `$upstream_response_time`).
- **Log** keeps the path's lines: its requests and the error log lines about them (by the `request:` nginx writes in them). It starts with those among the last 2,000 lines, and from then on keeps 2,000 of its own, however busy the other paths are. `l` goes there from the traffic.

`c` or `esc` lets go of it.

Each minute, hour and day keeps its 64 busiest paths. When a slot sees more, a new path takes the place of the least asked for one only if it has as many requests, so a path asked for often keeps its count; the rest are counted as the others. Percentiles are read from buckets an eighth of a doubling wide, within a few percent.

### Keys

| Key | Action |
| --- | --- |
| `tab` / `←` `→` / `1`-`5` | Next or previous view, or one by number |
| `m` `h` `d` | Traffic per minute, hour or day; `g` goes round |
| `↑` `↓` / `j` `k` | Pick a slot, a message or a line; `PgUp` `PgDn` ten at a time, `home` `end` the ends |
| `s` | Paths: most requested, most 5xx, most 4xx, slowest |
| `x` | Log: only failed requests and error log lines |
| `/` | Log: search the lines; `c` clears the filters |
| `p` | Traffic, paths, log: hold the screen to one path |
| `l` | Traffic: the held path's lines |
| `c` | Traffic: let go of the path; log: of every filter |
| `enter` | Log: everything read from the picked line |
| `esc` | Let go of the pick, then quit |
| `?` | Show all keys |
| `q` / `Ctrl-C` | Quit; the report is left in your shell |

## What is wrong

The last minute (`--window`) is held against the minutes before it, and against the same hour a day before, and what is out of the usual is said as findings, most serious first: on the dashboard, in the report and in `--json`. `↑` `↓` pick one on the dashboard, and `enter` opens what it is about: the traffic held to its path, the errors, the paths.

| Level | Said when |
| --- | --- |
| bad | 5% of requests answer 5xx; a path answers 5xx to 10% of its requests, or 4xx to half of them; the error log has ten times its usual errors; traffic is under a tenth of its usual while the log is being written; p99 is five times a usual minute's and over a second |
| warn | 1% answer 5xx, three times the usual; a path answers 5xx to 2% or 4xx to 30%, three times its usual; the error log has three times its usual; traffic is under a third of its usual, or twice it; p99 is twice a usual minute's and over 200ms |
| note | one client sends a fifth of all requests and twice the next one; lines couldn't be read |
| good | nothing out of the usual |

A path's share needs at least five failed requests in the window, so that one failure in forty isn't a finding. A finding says since when, from the minutes it has been true in, and what the usual was. Slower requests name the path whose mean time changed most, and whether its time is spent in nginx or waiting on the upstream (`$upstream_response_time`), which is the app.

## Limits and alerts

`--fail-if` holds the logs to a limit, and ends with exit code 4 when it is crossed, after the report:

```bash
pepe logs /var/log/nginx/access.log /var/log/nginx/error.log --since 15m \
  --fail-if '5xx > 2%' --fail-if 'p99 > 1s' --fail-if 'rate < 5/s' \
  --webhook "$SLACK_WEBHOOK_URL"
```

| Measure | Takes | Means |
| --- | --- | --- |
| `5xx` (or `errors`) | a percentage | the share answering 5xx |
| `4xx` | a percentage | the share answering 4xx |
| `p50`, `p99`, `p99.9`… | a time: `500ms`, `2s` | that percentile of `$request_time` |
| `rate` | requests a second: `5/s` | the request rate; the one measure that also takes `<` |

Read once (piped, `--json`, from cron) a limit is held against everything read, so `--since` says over what. While a log is followed it is held against the last `--window`, every second; a limit crossed even once makes the exit code 4 when the screen is left, and the report says when it was crossed. The dashboard shows each limit under the findings.

`--webhook`, `--webhook-on` and `--slack` work as they do for a load test (see [webhooks](webhooks.md)). Read once, the hooks get the verdict at the end, and `fail` when a limit was crossed. While a log is followed, a finding turning bad, a limit crossed or held again, and a finding going away are `anomaly` events, and `--slack` keeps one message up to date with the rate, the 5xx and the findings; the verdict goes out when the screen is left. A plain URL is sent the end only, unless `--webhook-on anomaly,end` says more.

## Prometheus

`--metrics :9100` serves the numbers at `http://ADDR/metrics` while a log is followed, rendered again every second: `pepe_logs_requests_total{class}`, `pepe_logs_bytes_total`, `pepe_logs_error_log_lines_total{level}`, `pepe_logs_unread_lines_total`; for the last `--window`, `pepe_logs_requests_per_second`, `pepe_logs_5xx_ratio`, `pepe_logs_4xx_ratio` and `pepe_logs_live`; the histograms `pepe_logs_request_duration_seconds` and `pepe_logs_upstream_duration_seconds`; `pepe_logs_findings{level}`, what is out of the usual now; and `pepe_logs_fail_if_crossed{condition}` for each `--fail-if`.

## As an MCP tool

`pepe mcp` has a `logs` tool: `files` (nginx's own when none), `since`, `until`, `window`, `path`, `status`, `host`, `fail_if`, `format`, `exact_paths` and `rows`, the same as the flags, returning the JSON report. It reads files and sends nothing; a limit crossed is an answer, not an error.

## Formats

Access logs are read as nginx's `combined` format (Apache's too), with `rt=` and `urt=` timings after it if they are there, or as JSON lines under nginx's variable names or Caddy's. A log with a `log_format` of its own needs it said, as nginx.conf has it, on one line:

```bash
pepe logs access.log --format '$remote_addr [$time_local] "$request" $status $body_bytes_sent $request_time $upstream_response_time'
```

`$time_local`, `$time_iso8601`, `$msec`, `$request` (or `$request_method` and `$request_uri`), `$status`, `$body_bytes_sent`, `$remote_addr`, `$host`, `$http_user_agent`, `$request_time` and `$upstream_response_time` are used; the rest are shown when a line is opened. Lines that couldn't be read are counted and the first is shown. Numbers and ids in a path count as one (`/items/*`) unless `--exact-paths` is given. The error log names no time zone, so its times are taken to be this machine's.

## Flags

| Flag | Default | What it does |
| --- | --- | --- |
| `FILES...` | nginx's own | Access and error logs, rotated ones too; `-` reads what is piped in |
| `--format <LOG_FORMAT>` | combined | The access log's `log_format`, on one line |
| `--window <TIME>` | 60s | What "now" is measured over |
| `--since <TIME>` | | Leave out what is older than this: `90m`, `24h`, `7d` |
| `--all` | | Read everything before following, rather than starting five minutes back |
| `--exact-paths` | | Count `/items/1` and `/items/2` apart |
| `--rows <N>` | 10 | Rows in each table of the report |
| `--fail-if <CONDITION>` | | Exit code 4 when crossed: `'5xx > 2%'`, `'4xx > 20%'`, `'p99 > 1s'`, `'rate < 5/s'`; repeat for more |
| `--webhook <URL>` | | Send the verdict there; `--webhook-on` says when |
| `--slack[=#channel]` | | Keep a Slack message up to date while the log is followed |
| `--path <PATH>` | | Only requests to these paths: `/api/*` |
| `--status <CODES>` | | Only requests that answered these: `5xx`, `404`, `4xx,503` |
| `--host <HOST>` | | Only requests to these hosts: `shop.test`, `.example.com` |
| `--until <TIME>` | | Leave out what is newer than this: `1h` |
| `--metrics <ADDR>` | | Serve Prometheus metrics at `/metrics` while the log is followed |

`--json` prints the report as JSON (with `schema_version`); piped, the text report. Both have the verdict: `verdict.level` (`healthy`, `degraded`, `failing`) and `verdict.findings[]`, each with `level`, `kind` (`errors`, `path_errors`, `path_refused`, `error_log`, `traffic_down`, `traffic_up`, `latency`, `client`, `unread`, `steady`), `text`, and when they apply `path`, `since`, `now`, `usual` and `against`; and `fail_if[]`, each `--fail-if` with `condition`, `held` and `measured`.
