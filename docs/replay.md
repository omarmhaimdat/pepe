# Replaying an access log

Real traffic is not one URL. `pepe replay` reads an access log and sends its URLs in the proportions the log had: a path seen 3,000 times gets 30× the requests of one seen 100 times, mixed evenly rather than in bursts.

```bash
pepe replay access.log --base-url https://staging.example.com -c 50 -z 2m
pepe replay access.log --base-url https://staging.example.com --include-writes --json > replay.json
```

## What is read

nginx and Apache logs (common and combined), Caddy's JSON lines, AWS ALB logs, and plain lists of one URL or path per line. Rotated files can be given too. Lines that can't be read are counted and said in the report.

## Flags

| Flag | What it does |
| --- | --- |
| `--base-url <URL>` | Goes in front of paths and replaces the host of full URLs, so production's log can be sent at staging; without it, full URLs are sent where they point and paths can't be sent at all |
| `--include-writes` | Replay POST, PUT, PATCH and DELETE too; only GET, HEAD and OPTIONS without it. Request bodies aren't in access logs, so writes go without one |
| `--rows <N>` | URLs that get a row of their own on the dashboard, most frequent first (20 by default); the rest share one |

`-c`, `-z`, `-n`, `-H`, `--rate`, `--warmup` and the other request options work as usual.

## The run

The dashboard's first tab lists the most frequent URLs with each one's share of the log, throughput, latency and statuses, and one row for all the rest. There is no setup screen for a replay: the log is the setup, and `r` runs it again. `--json` adds `replay`: what the log had, what was left out and why (unparsed lines, writes, paths with no host, URLs past the 5,000 most frequent), and the same per-URL numbers under `urls`. Piped, the verdict is printed. `--metrics` serves a row per URL.
