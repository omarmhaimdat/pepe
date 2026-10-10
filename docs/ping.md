# Ping

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

## Targets

A host without a scheme is `https://`, unless it has a port or is this machine (`localhost`, `127.0.0.1`, `::1`), where it is `http://`. A target can be:

- a URL, or a host, or `host:port`;
- `aws:REGION`, a cloud region by shorthand (`ec2.REGION.amazonaws.com`);
- an IPv4 range, `10.0.0.0/29` or `10.0.0.0/29:8080`, every host of it a target (up to a /24);
- with `--all-ips`, every address a name resolves to, each a target of its own, named `host IP`;
- with `--tcp`, a port: the ping is the connection, nothing is sent; `--port` is the port for a target that names none (80), and `--tcp-rst fail` makes a refused connection a failure rather than the answer it is by default (something is there to refuse);
- with `--cmd`, a command: run in the shell every interval and timed, its exit code the status.

`--name api,cdn` names the targets in order, `--color red,#8cb8ff` colours their lines, `-4` and `-6` pick the address family, and `--interface en0` (or an address) sends from that interface. At most 256 targets.

## HTTP/2

A ping takes HTTP/2 when the server offers it through ALPN, which most do over TLS, and says so: `HTTP/2 200` in the once line and the pings list, `"http_version": "HTTP/2"` in the report, `h2` under the TLS details. The phases are the same, the first byte being the response's headers. `--http1` stays on HTTP/1.1, to compare the two. A server that offers only HTTP/1.1 is noted among the findings. The load-testing modes still speak HTTP/1.1, where pepe's engine is fastest.

## Each phase, every ping

Each ping opens its own connection, so every phase is measured every time, and the TLS session is still resumed when the server allows it: the first handshake is full, the next ones resumed, and the report says how long each kind takes. `--keep-alive` keeps the connection instead, as a browser would, and the pings after the first measure only the server (`dns`, `connect` and `tls` are then `kept`). Redirects are followed (unless `--disable-redirects`), each hop listed with its status and time, and the phases of the whole chain added up. Through a proxy (`-p`), the connection is the proxy's and only the first byte and the download are measured.

| Phase | What it is |
| --- | --- |
| `dns` | The name resolved, when the target is a name |
| `connect` | The TCP connection, to the first address that answers |
| `tls` | The handshake, full or resumed |
| `ttfb` | From the request going out to the first byte of the answer: the server's own time, plus a round trip |
| `download` | From the first byte to the end of the body |

## The screen

The table above the views has each target's last, min, avg, max, jitter (the mean difference between one ping and the next), p95, p99, loss and timeouts over what the graph shows, or over the whole run with `t`.

| View | Shows |
| --- | --- |
| **Graph** | Each target's latency as a line, failures marked on top (`✖`), SLO breaks (`▲`) and 5xx (`!`) too; `+` and `-` zoom, `w` the whole run, `0` a floor at zero, `l` a log scale, `s` dots instead of braille, `f` hides the marks |
| **Phases** | Where the picked target's last and median ping went, as a stacked bar per phase, with curl's running totals (`namelookup`, `connect`, `pretransfer`, `starttransfer`, `total`); the TLS version, cipher and ALPN, full against resumed handshakes; the certificate's subject, issuer and expiry; the addresses at both ends; and what to look at |
| **Pings** | Every ping, newest last, with each phase; `x` keeps failures and SLO breaks, `a` one target or all, `enter` opens one: its hops, headers, TLS and certificate, and the body with `--show-body` |

### Keys

| Key | Action |
| --- | --- |
| `tab` / `←` `→` / `1` `2` `3` | Graph, Phases or Pings |
| `↑` `↓` / `j` `k` | Pick a target, or a ping in the list; `PgUp` `PgDn` ten at a time, `home` `end` the ends |
| `space` | Pause or resume the pings |
| `+` / `-` | Half or twice the time the graph shows; `w` the whole run, and `esc` back |
| `0` / `l` / `s` / `f` | Start at zero; a log scale; dots instead of braille; hide or show the failures |
| `t` | The table over the whole run, or over what the graph shows |
| `a` / `x` | Pings: one target or all; failures and SLO breaks only |
| `enter` | Pings: everything about the picked one; `esc` closes it |
| `?` | Show all keys |
| `q` / `esc` / `Ctrl-C` | Quit; the summary is left in your shell |

## What to look at

The numbers come with what they mean, in the summary, in the phases view and under `findings` in the JSON, most serious first: nothing answered, and why; pings lost and timed out; 5xx; 401, 403, 404 or 429 on every ping; every request redirected, with the final URL to point at and what the chain costs; a slow DNS lookup; connecting as a share of the request, read as a round trip; the TLS session never resumed, so every connection pays the full handshake, or resumed and how much it saves; the server closing every connection; the server's own share of the time, with its `Server-Timing` when it sends one; a large or slow download; a text body that isn't compressed though gzip and br were offered; a cache answering, so the server itself wasn't measured; HTTP/1.0; a certificate expired or expiring within 30 days; and a long tail.

`pepe ping URL --once` is the quick version: three pings 200 ms apart, the last one's phases, what answered, the findings, and out.

```
pepe ping · https://example.com/ · 3 pings
  dns 1.84ms → connect 49.88ms → tls 66.57ms → first byte 63.72ms → download 33µs · 182.5ms in all
  HTTP/1.1 200 · text/html · 334 B · 104.20.23.154:443 from 192.168.0.164 · TLS 1.3 TLS13_AES_256_GCM_SHA384
  certificate for example.com by SSL Corporation, expires in 76 days
  ▲ TLS sessions are resumed: 66.82ms after the first handshake's 118.6ms, still 66.82ms of 190.0ms
  · answered by a cache (cf-cache-status: HIT): the server itself wasn't measured
```

## Limits

`--slo total=500,ttfb=200,connect=100,dns=50,tls=150,download=100`, in milliseconds, marks a ping that goes over any of them, counts them in the report, and makes the exit code 4. The exit code is 1 when nothing ever answered. `--bell` rings the terminal on a failed or slow ping. (A load run's limits are `--fail-if`; a ping refuses it and points here.)

## Flags

| Flag | Default | What it does |
| --- | --- | --- |
| `--every <TIME>` | 1s | Time between pings: `1s`, `500ms`, `2m` |
| `--window <TIME>` | 60s | How much of the run the graph shows |
| `--name <NAME>` | | What to call each target, in order; `a,b` or repeated |
| `--color <COLOR>` | | A colour per line: red, green, yellow, blue, magenta, cyan, white, gray, their light- forms, or `#RRGGBB` |
| `-4` / `-6` | | IPv4 or IPv6 addresses only |
| `--all-ips` | | Every address a name resolves to, each a target |
| `--interface <NAME\|IP>` | | Send from this interface or local address |
| `--tcp`, `--port`, `--tcp-rst` | 80, pong | A TCP ping of the port; what a refused connection means |
| `--cmd` | | The targets are commands |
| `--keep-alive` | | Keep the connection between pings |
| `--http1` | | HTTP/1.1 even when the server offers HTTP/2 |
| `--slo <KEY=MS,...>` | | Limits a ping must meet |
| `--bell` | | Ring the terminal on a failed or slow ping |
| `--ymin`, `--ymax <MS>`, `-0` | | The graph's floor and ceiling; `-0` starts at zero |
| `-s`, `--simple-graphics` | | Dots rather than braille |
| `--jsonl`, `--csv` | | No screen: a line per ping, JSON or CSV |
| `--save <FILE>` | | Write the JSON report at the end, whatever else is shown |
| `--show-body` | | Keep the first kilobyte of each body for the inspector |
| `--save-body <FILE>` | | Write the last body received |
| `--once` | | Three quick pings, the diagnosis, and out |

`-n` and `-z` end the run; without them it runs until `q`. `-t` is the timeout, `-H`, `-m`, `-d`, `-u`, `-p`, `-k`, `--disable-compression` and `--disable-redirects` shape the request, and `--metrics` serves the live numbers ([Output](output.md#prometheus-metrics)).

## Without a screen

Piped, or with `--jsonl`, `--csv` or `--json`, there is no screen: a line per ping as it happens, as text, JSON or CSV, then the summary on stderr, or the JSON report on stdout. `pepe schema ping` prints the report's schema, and the shape of a `--jsonl` line is under its `sample` definition. Two ping reports can be held against each other with `pepe compare`, phase by phase ([Compare](compare.md)).

```bash
pepe ping https://example.com --jsonl -z 5m > pings.jsonl
pepe ping https://example.com --json -n 10 --save before.json
```
