---
name: pepe
description: Measure an HTTP endpoint's latency with pepe. Use when asked why an API or page is slow, to split a request's time into DNS, connect, TLS, first byte and download, to watch an endpoint's latency over time, to check a TLS certificate's expiry, or to load-test a URL.
---

# pepe

pepe is an HTTP load generator with a live dashboard. `pepe ping` is the
part to reach for first: one request a second, each split into where its
time went, with a JSON report an agent can read.

## Install

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh    # macOS and Linux
brew install omarmhaimdat/pepe/pepe                     # Homebrew
```

## Diagnose one endpoint

Always pass `--json` and `-n`: without a terminal pepe prints a line per
ping, and without `-n` or `-z` it never ends.

```bash
pepe ping https://api.example.com/health -n 5 --json
```

Read `targets[0]`:

- `latency_ms`: `min`, `avg`, `max`, `jitter`, `p50`, `p95`, `p99`.
- `phases_ms`: `dns`, `connect`, `tls_full`, `tls_resumed`, `ttfb`,
  `download`, each with `median_ms`, `p99_ms` and `max_ms`. The largest
  median is the bottleneck. `dns` high: the resolver. `connect` high:
  distance or a slow accept queue. `tls_full` high but `tls_resumed`
  low: the handshake is the cost; keep connections alive. `ttfb` high:
  the server itself. `download` high: the body's size or the link.
- `loss`, `timeouts`, `failures`: what never answered, by cause.
- `tls.cert.days_left` and `tls.cert.valid`: the certificate.
- `status_codes`: what it answered with.

`--slo total=500,ttfb=200` adds `slo.pass` and makes the exit code 4
when a limit was broken; exit code 1 means nothing ever answered.

## Watch it over time

```bash
pepe ping https://api.example.com/health --jsonl -z 5m > pings.jsonl
```

One JSON object per ping (`timings_ms`, `status`, `error`, `tls`) as it
happens. `--csv` does the same as CSV. A person at a terminal gets the
graph instead: `pepe ping URL` with no flags.

## Then load-test

```bash
pepe --json -z 30s -c 50 https://api.example.com/health
pepe ramp --json https://api.example.com/health --to 200 --until 'p99 > 500ms'
```

The same `-H`, `-m`, `-d`, `-k` and `-t` apply to every mode. Only
point pepe at what you are allowed to load.
