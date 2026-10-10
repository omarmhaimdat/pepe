# The dashboard

What a run shows while it goes, how to read it, and every key.

![pepe load-testing a server: the live dashboard, the request log, and the verdict](assets/run.gif)

## Views

Three views, four in API mode with **Endpoints** in front. `tab`, `←` `→` or `1` `2` `3` switch between them.

- **Live**: the headline numbers (requests, throughput, p50, p99, errors), a latency heatmap with time across and latency up (brighter cells mean more requests took that long) with p50 and p99 marked, throughput per second, and a panel with the detailed numbers. Pepe, the chili in the corner, reacts to how the run is going.
- **Stats**: every number pepe collects, the test setup, and the latency distribution. When the target sends `Server-Timing`, a card shows its own time against the median measured here and each segment's p50 and p99. Another lists the five slowest responses with their request ids, taken from `X-Request-Id`, `traceparent`, `CF-Ray`, `X-Amzn-Trace-Id` and other common headers, or the one named with `--trace-header`.
- **Requests**: the last 2,000 requests (and older failures, kept apart so a burst of successes can't push them out), filterable by status, latency and text. `enter` on one inspects it.
- **Endpoints** (API mode), **Steps** (a flow) or **URLs** (a replay): one row per endpoint, step or URL with requests, throughput, p50, p99, errors and status codes; `enter` on a row shows its requests.

## The header and the footer

The header has the target, the concurrency, the elapsed time against the plan, and, during a warm-up, `warming up` with the time left. The footer has the keys that matter now, and notices as they happen: a change of concurrency, the sending thread past 90% of a core, a rate the concurrency can't carry ("behind the rate: 410 of 500 req/s · all 8 in flight; try -c 25"), and anomalies.

## Anomalies and the verdict

While the run goes, each second is compared with the thirty before it. A p99 that jumps, throughput that falls or errors that appear are called out in the footer as they happen ("p99 jumped 4.5× to 45ms at 26s"), repeated in the verdict and listed in the JSON report under `summary.anomalies`.

When the run ends, the header turns into a verdict, **Healthy**, **Degraded** or **Failing**, with findings:

- failed requests, by cause, with what the first response of each kind said;
- two separate latency groups (a bimodal distribution), which usually means two paths, two backends or a cache;
- a long tail: a p99 many times the median;
- throughput or latency drifting over the run;
- where the time went, when it is somewhere worth knowing: connecting being a quarter or more of each request when every request opens a connection; connections opened many times over the concurrency, so the server closes them and keep-alive isn't holding; slow DNS lookups; downloading large bodies being the time; half or more of the responses answered by a cache, so the origin wasn't measured;
- pepe's own thread being the limit, with `--threads` suggested;
- a `--rate` that the concurrency couldn't carry, and what would.

The same summary is printed to your shell when you quit.

## The inspector

`enter` on a request opens it: status, total time split into time to first byte and body download, how it ranks in the run, DNS, server address, protocol, cache status, the request as sent, and the full response headers and body, with JSON, HTML and XML indented and highlighted. `↑`/`↓`, the trackpad, `u`/`d` and `g`/`G` scroll the response; `v` switches between formatted and raw; `←`/`→` walk to the newer or older request; `esc` goes back.

Up to 1,000 responses a second are kept in full, an even sample above that: those are marked `●`, and `[`/`]` jump to the nearest one kept in full. The memory the kept responses may use is bounded (32 MB), the oldest dropped first.

## Filters

On the Requests view: `f` cycles the status filter (2xx, 3xx, 4xx, 5xx, no response, failed), `l` the latency filter (at or above p50, p90 or p99, refreshed as results arrive), `/` searches the status and response text, `x` keeps only failed requests, and `c` clears every filter. `↑` `↓` (or `j` `k`), `PgUp` `PgDn`, `home` and `end` move through the log; the newest line is followed until you pick one, and `esc` lets go of the pick.

## Keys

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
| `?` | Show all keys, here and on the setup (`F1`), ramp, API and ping screens |
| `q` / `esc` / `Ctrl-C` | Quit; the verdict is left in your shell |

The ramp, plan, logs and ping screens have keys of their own, on their pages: [Ramp](ramp.md#keys), [OpenAPI](api.md#the-plan-screen), [nginx logs](logs.md#keys), [Ping](ping.md#keys). The setup screen's are on [Load testing](load-test.md#the-setup-screen).

## Size and colour

The dashboard draws at any size from 60×18 up and says so below it; charts and tables take the room they get. It redraws ten times a second while live, on input only once the run is over, so it costs no CPU while idle, and it survives whatever else writes to the terminal. Colour follows the terminal: pepe's own palette where true colour is available, the terminal's 256 colours otherwise, `PEPE_THEME=light` for a light background, `NO_COLOR` for none ([Install: themes](install.md#themes-and-colour)).

## Without a terminal

Piped or redirected, there is no dashboard: the run goes to its end and the same verdict is printed. `--json` prints the report instead. See [Output and exit codes](output.md).
