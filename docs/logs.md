# Reading nginx logs

![Reading nginx logs: the dashboard with the rate now and a verdict, then traffic by the hour, paths, errors and the log](assets/logs.gif)

What the server already knows about its traffic: `pepe logs` reads nginx's access and error logs and says how busy the server is now against how busy it has been.

```bash
pepe logs /var/log/nginx/access.log /var/log/nginx/error.log
pepe logs                                      # nginx's own, in /var/log/nginx
zcat access.log.*.gz | pepe logs -
docker compose logs -f -n 1000 nginx | pepe logs
pepe logs access.log --since 24h --json > traffic.json
```

## How it reads

At a terminal a log that is being written is shown live: the screen starts five minutes back, so that now is right at once, and follows the files as they are written, through rotation. `--since 24h` starts further back and `--all` reads everything first; either way the place to start is found in the file without reading what is before it. A log whose last line is older than five minutes has no now, and is read whole. Piped out, or with `--json`, the files are read to the end and a report is printed.

Lines are counted as a stream, so memory doesn't grow with the log: every second of the last hour, a day of minutes, ninety days of hours and ten years of days are kept, with capped tables of paths, clients and user agents. What a file already has is read by every core at once, some gigabytes a second, and what is appended after that by one. Rotated files can be given in any order. What is piped in is followed too, until its writer ends, and the `nginx-1  | ` that `docker compose logs` puts in front of each line is left out, with the colour it writes into a pipe.

## Now against before

**Now** is the request rate over the last minute (`--window`) of the log's own timestamps. A log whose last line is older than five minutes isn't being written, and is held at its last line instead of the clock. Each minute, hour and day has its requests, its rate, its busiest second, its 4xx and 5xx shares, its mean request time and its error log lines, and how now compares: `+12%`, `×3.4`, `÷2.5`. The cards on top say what a usual slot sees (the median), which was the busiest, and what the same minute an hour ago, the same hour a day ago or the same day a week ago saw.

## Views

| View | Shows |
| --- | --- |
| **Dashboard** | Opens first. The rate now, the share answering 5xx and the request time drawn large; a verdict in a word (Steady, Busy, Quiet, Degraded, Failing) with what fails and what the error log says of it; traffic as a bar for every few seconds of the last minutes or hour, 4xx and 5xx in their colours; the top paths, the status codes, the error log's messages and the newest lines |
| **Traffic** | A bar per slot with the rate now drawn across them, and the table of slots; `m`, `h`, `d` switch between minutes, hours and days |
| **Paths** | The paths by requests, 5xx, 4xx or mean time (`s`), with status codes, clients, user agents, methods and query parameter names beside them |
| **Errors** | The error log's messages grouped by cause, most frequent first, each with the first line that said it; the paths answering 5xx and 4xx |
| **Log** | The last 2,000 lines of all the files in time order; `x` keeps failures, `/` searches, `enter` shows everything read from a line, query parameters one by one |

### Keys

| Key | Action |
| --- | --- |
| `tab` / `←` `→` / `1`-`5` | Next or previous view, or one by number |
| `m` `h` `d` | Traffic per minute, hour or day; `g` goes round |
| `↑` `↓` / `j` `k` | Pick a slot, a message or a line; `PgUp` `PgDn` ten at a time, `home` `end` the ends |
| `s` | Paths: most requested, most 5xx, most 4xx, slowest |
| `x` | Log: only failed requests and error log lines |
| `/` | Log: search the lines; `c` clears the filters |
| `enter` | Log: everything read from the picked line |
| `esc` | Let go of the pick, then quit |
| `?` | Show all keys |
| `q` / `Ctrl-C` | Quit; the report is left in your shell |

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

`--json` prints the report as JSON (with `schema_version`); piped, the text report.
