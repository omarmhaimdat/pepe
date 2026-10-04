# Benchmarks

How pepe's resource usage is measured, what it was before each optimization,
and what it is after. Everything here is reproducible with the scripts in
this directory.

## Method

**What is measured.** A load generator's job is to send requests and record
what came back; the cost that matters is how much CPU and memory it spends
per request, not how fast one laptop can go. So each workload sends a fixed
number of requests, and `/usr/bin/time -l` reports the process's user and
system CPU seconds and its peak resident memory. From those:

- **CPU ms per 1,000 requests** = (user + sys) × 1000 / requests. Lower is
  better; this is the headline number.
- **Peak RSS** in MB, the most memory the process held.
- **Requests per second** as the tool itself reported, for context. The
  target server is shared by every tool and tops out around 130k requests
  a second, so throughput near that is the server's limit, not the tool's.

**Target.** [`server.go`](server.go), a Go `net/http` server answering from
memory. It is faster than any of the clients, so the client is the
bottleneck. `go run bench/server.go` serves HTTP on `127.0.0.1:8089` and
HTTPS (self-signed) on `127.0.0.1:8090`.

**Workloads.** [`run.sh`](run.sh) runs each of these with pepe (`--json`, no
screen) and, where the same thing can be asked of them, with
[oha](https://github.com/hatoo/oha) and [vegeta](https://github.com/tsenart/vegeta):

| Workload | Requests | Concurrency | Path | Exercises |
| --- | --- | --- | --- | --- |
| tiny-c64 | 200,000 | 64 | `/` (16 B) | per-request overhead |
| tiny-c256 | 200,000 | 256 | `/` | more connections |
| tiny-c1000 | 200,000 | 1,000 | `/` | memory per connection |
| json-c64 | 200,000 | 64 | `/json` | cache-header parsing |
| body16k-c64 | 50,000 | 64 | `/16k` | body streaming |
| body256k-c16 | 5,000 | 16 | `/256k` | large bodies |
| slow20ms-c1000 | 50,000 | 1,000 | `/slow?ms=20` | many idle connections |
| status503-c64 | 100,000 | 64 | `/status/503` | the failure path |
| post-c64 | 200,000 | 64 | `/` POST + JSON body | request bodies |

vegeta has no request count, so it runs for 5 s at full speed and its CPU is
divided by the requests it managed.

**Dashboard mode.** [`tui.py`](tui.py) runs the real dashboard in a
pseudo-terminal (160×48), discards the frames as they're drawn, presses `q`
when the run is over and reports the same numbers. `--keys 3` switches to
the Requests tab first.

**Profiles.** `cargo build --profile profiling` is a release build with
symbols; `sample <pid> 5` (macOS) gives the call tree the findings below
come from.

**Regression gate.** [`compare.sh`](compare.sh) runs the four workloads
that matter most (plain, 16 KB bodies, 1,000 slow connections, TLS) with
two binaries taking turns, three rounds each, and compares the best round
of each. CI ([`.github/workflows/bench.yml`](../.github/workflows/bench.yml))
runs it on every release PR, candidate against the last released tag built
with the same toolchain, and fails the PR if CPU per 1,000 requests grew
by more than 15% or peak memory by more than 25%. The same workflow runs
the full suite for pepe and oha on the Linux runner and keeps the CSV; the
Linux table below comes from it.

**Machine.** Apple M4 Pro (14 cores), 24 GB, macOS 26.5, Rust 1.85,
pepe built with `--release` (LTO, one codegen unit). oha 1.x and vegeta
12.x from Homebrew. Numbers are from single runs unless noted; run-to-run
noise is about 5%.

```bash
go run bench/server.go &
cargo build --release
bench/run.sh                                  # the table, as CSV
bench/tui.py -- target/release/pepe -n 200000 -c 64 http://127.0.0.1:8089/
```

## Baseline (v0.5.1, before this work)

JSON mode, CPU ms per 1,000 requests / peak RSS MB / requests per second:

| Workload | pepe | oha | vegeta |
| --- | --- | --- | --- |
| tiny-c64 | 42.5 / 14.1 / 124k | 23.3 / 85.1 / 105k | 84.4 / 25.9 / 67k |
| tiny-c256 | 40.1 / 24.9 / 132k | 24.7 / 81.4 / 124k | 51.7 / 40.1 / 111k |
| tiny-c1000 | 40.2 / 115.6 / 126k | 21.9 / 108.2 / 105k | 50.1 / 87.8 / 122k |
| json-c64 | 40.2 / 14.1 / 127k | 23.6 / 76.6 / 115k | 78.8 / 27.3 / 73k |
| body16k-c64 | 51.6 / 17.1 / 93k | 35.2 / 49.3 / 75k | 393 / 28.5 / 17k |
| body256k-c16 | 88.0 / 23.8 / 36k | 86.0 / 61.6 / 32k | 692 / 80.3 / 7k |
| slow20ms-c1000 | 38.0 / 169.1 / 45k | 26.6 / 87.3 / 45k | 65.2 / 87.5 / 47k |
| status503-c64 | 42.9 / 13.9 / 131k | 24.1 / 55.0 / 102k | 78.8 / 27.5 / 74k |
| post-c64 | 42.2 / 13.9 / 123k | 23.4 / 84.8 / 109k | — |

Dashboard mode, 200,000 requests at concurrency 64: **8.45 s CPU**
(5.1 user + 3.3 sys), 24.5 MB peak, on the Live tab; 8.21 s on the Requests
tab.

Reading: pepe was already the fastest and by far the smallest of the three,
but it spent **1.8× oha's CPU** for the same requests, and almost all of the
difference was user time (4.85 s against 1.13 s for tiny-c64). Memory at
1,000 connections was also the highest of the three.

### Where the CPU went

Two experiments explain the baseline.

Tokio worker threads (`TOKIO_WORKER_THREADS`, tiny-c64, baseline binary):

| Threads | CPU s | user | sys | req/s | peak RSS at c=1000 |
| --- | --- | --- | --- | --- | --- |
| 1 | 3.43 | 1.44 | 1.99 | 75k | — |
| 2 | 4.87 | 2.75 | 2.12 | 88k | 64 MB |
| 4 | 6.31 | 3.70 | 2.61 | 115k | 145 MB |
| 8 | 7.72 | 4.55 | 3.17 | 129k | — |
| 14 (default) | 8.73 | 5.14 | 3.59 | 130k | 169 MB |

The same requests cost 2.5× the CPU on 14 threads as on one. The profile of
a worker thread said why:

- The largest single cost inside a request was a `std::sync::Mutex` in
  hyper-util's connection pool, taken on checkout and again on return and
  contended by every thread (`__psynch_mutexwait` under
  `Pooled::drop` and `Checkout::poll`).
- Every request parsed its URL from a string: `url::Url::parse`, including
  IDNA processing of the host and formatting the IPv4 address back into text
  (`idna::uts46`, `Ipv4Addr::fmt`, `core::fmt::write`).
- tokio's timer lock for the per-request timeout (`TimerEntry::drop` →
  `parking_lot::lock_slow`), contended the same way.
- reqwest clones its `Connector` and merges the default headers (atomic
  refcounts on shared `Bytes`) for every request.
- Each request was its own spawned task behind a semaphore, with the Arc
  clones, JoinSet bookkeeping and cross-thread wakeups that come with that.

None of this is the HTTP work itself; it is coordination between threads
and repeated setup.

## What changed

Four changes, each measured on its own before the next.

### 1. A share-nothing load engine (`src/load.rs`)

Requests now go out from shard threads, one by default (`--threads` adds
more). Each shard has its own single-threaded tokio runtime, its own
`reqwest::Client` (so its own connection pool and timer wheel) and
long-lived worker tasks, one per unit of concurrency, instead of a task per
request behind a semaphore. The main runtime, which only runs the screens,
is single-threaded too. Nothing on the hot path is shared between shards
except a few atomic counters, so there is no lock for threads to queue on.

Alongside: the URL and method are parsed once, in `Request::new`, instead
of on every request (a bad URL is now an error before the run starts rather
than a dashboard full of failures); the direct `hyper` dependency went
away; and the results channel is drained on a 25 ms timer in `--json` mode
as the dashboard already did, because a send to a receiver that is waiting
has to wake it through the kernel, which at 100k results a second was a
fifth of the run's CPU.

tiny-c64, 200,000 requests, same binary with 1 to 4 shards (the server is
shared, so throughput past ~130k req/s is its limit):

| `--threads` | CPU s | user | sys | ms / 1k | req/s |
| --- | --- | --- | --- | --- | --- |
| 1 (default) | 1.88 | 0.94 | 0.94 | 9.4 | 103k |
| 2 | 3.17 | 1.78 | 1.39 | 15.9 | 116k |
| 3 | 3.73 | 1.99 | 1.74 | 18.7 | 127k |
| 4 | 4.26 | 2.24 | 2.02 | 21.3 | 125k |
| baseline (14 tokio threads) | 8.49 | 4.85 | 3.64 | 42.5 | 124k |

One shard costs **4.5× less CPU than the baseline** and 2.5× less than
oha for the same requests, at about the same throughput as oha. The
single-thread profile is now dominated by the `writev` and `recvfrom`
syscalls and hyper's parsing, with user time equal to oha's.

Why one thread is the default: every extra thread costs about 60% more CPU
for the same requests, and memory with it, while raising the peak only
against a target that can take more than ~100k requests a second (or 10k
TLS handshakes a second: 5,000 handshakes at `-c 64` took 0.48 s on one
thread, 0.28 s on two). Most targets can't, and when one can, pepe says so
instead of guessing.

### 2. pepe says when it is the bottleneck

Each shard measures its thread's CPU time once a second
(`CLOCK_THREAD_CPUTIME_ID`). Past 90% of a core the dashboard's footer
shows it, the end-of-run verdict gets a note (`pepe's sending thread
reached 99% of a core, so these numbers are pepe's limit as much as the
target's; run again with --threads 2`), and the JSON report carries it:

```json
"generator": { "threads": 1, "peak_busy_percent": 99 }
```

A load generator that silently caps the result is worse than one that uses
a bit more CPU; this keeps the one-thread default honest.

### 3. One connection per worker

Counting established connections during a `-c 1000` run against the fast
target found **1,999** for pepe and 1,065 for oha. hyper-util's pool races
a new connection against waiting for an idle one whenever a request finds
none; the first responses hand their connections to workers still
connecting, and those workers' own connections arrive as spares, kept idle
for the rest of the run. (This was true of the baseline too; the old
multi-threaded engine held about the same.)

Capping the pool's idle connections per host at 4
(`pool_max_idle_per_host`) drops spares as they arrive, because a worker's
own connection is idle only for the moment between its requests:

| Idle cap | connections at `-c 64` | at `-c 1000` | req/s at 1000 | reconnects |
| --- | --- | --- | --- | --- |
| unlimited (before) | 64 | 1,999 | 87k | none |
| 16 | 64 | 1,015 | 82k | none |
| 4 (now) | 64 | 1,003 | 85k | none |

(reconnects: TIME_WAIT sockets grew by the same ~2,200 in every case, which
is the run's own connections closing at the end). Peak memory at
`-c 1000` fell from 105 MB to 68 MB with it. Holding each worker's first
response unread until the whole shard had connected was tried first and
only halved the spares: for small bodies hyper has read the whole response
with the headers and freed the connection before the caller sees it.

### 4. Measured and left alone

- **The dashboard.** Its thread spends 98% of its time parked; recording a
  result costs well under a microsecond and a frame is drawn ten times a
  second. Dashboard mode now costs the same CPU as `--json` mode for the
  same requests (1.96 s against 1.88 s for tiny-c64).
- **Memory per connection.** About 38 KB, of which pepe's own worker task
  is 1 KB; the rest is hyper's 8 KB read and write buffers and the
  connection task, which reqwest exposes no knobs for. 1,000 connections
  cost 66–68 MB against oha's 86–110 MB and vegeta's 79–94 MB.
- **Plain HTTP with a new connection per request** got slower with more
  threads for every tool, pepe and oha alike. That is macOS running out of
  ephemeral ports (TIME_WAIT), not the client, so it isn't in the suite.
- **Paced runs** (`--rate`, v0.10). A paced worker sleeps until its
  next start, so pacing costs a timer wake-up per request on top of the
  request itself: at 20,000 a second with `-c 64`, 3 s of plain HTTP cost
  pepe 1.33 s of CPU and 14.7 MB against 1.47 s unpaced for five times the
  requests. The same rate cost oha 1.8 (`-q`) 2.40 s and 46.8 MB and vegeta
  3.44 s and 27.2 MB; at 5,000 a second, pepe 0.72 s and 12.0 MB, oha 1.04 s
  and 33.9 MB, vegeta 0.97 s and 24.5 MB (macOS, one round each). The first
  paced build cost 23 s of system time for the same run: paced workers idle
  between requests, so their connections sat idle, the four-connection idle
  cap closed them, and every wake-up reconnected (`sample` showed the load
  thread in `connect`). A paced run now keeps up to its concurrency of idle
  connections.

## Results

Same suite as the baseline, final binary, default settings (one thread).
CPU ms per 1,000 requests / peak RSS MB / requests per second:

| Workload | pepe before | **pepe now** | oha | vegeta |
| --- | --- | --- | --- | --- |
| tiny-c64 | 42.5 / 14.1 / 124k | **9.6 / 13.6 / 100k** | 23.4 / 78.7 / 107k | 88.5 / 26.3 / 67k |
| tiny-c256 | 40.1 / 24.9 / 132k | **10.0 / 24.9 / 99k** | 24.7 / 81.8 / 112k | 51.2 / 40.0 / 117k |
| tiny-c1000 | 40.2 / 115.6 / 126k | **12.3 / 68.0 / 80k** | 24.3 / 110.7 / 102k | 48.8 / 94.2 / 125k |
| json-c64 | 40.2 / 14.1 / 127k | **9.4 / 13.6 / 104k** | 23.2 / 79.1 / 116k | 81.0 / 26.8 / 73k |
| body16k-c64 | 51.6 / 17.1 / 93k | **12.6 / 17.8 / 74k** | 33.8 / 49.7 / 79k | 403 / 29.3 / 17k |
| body256k-c16 | 88.0 / 23.8 / 36k | **26.0 / 34.8 / 33k** | 82.0 / 59.2 / 32k | 684 / 53.6 / 7k |
| slow20ms-c1000 | 38.0 / 169.1 / 45k | **11.6 / 66.9 / 44k** | 27.6 / 85.6 / 43k | 74.0 / 78.8 / 48k |
| status503-c64 | 42.9 / 13.9 / 131k | **9.4 / 13.0 / 102k** | 22.7 / 54.3 / 120k | 88.1 / 26.2 / 69k |
| tls-c64 | — | **9.9 / 15.1 / 97k** | 24.9 / 57.8 / 107k | 72.7 / 30.9 / 80k |
| tls16k-c64 | — | **18.7 / 20.0 / 50k** | 51.7 / 46.0 / 86k | 246 / 52.9 / 26k |
| post-c64 | 42.2 / 13.9 / 123k | **9.7 / 13.0 / 100k** | 22.5 / 86.4 / 117k | — |

(The TLS workloads were added after the baseline was taken.)

Dashboard mode, 200,000 requests at concurrency 64, Live tab: **1.96 s
CPU** (was 8.15–8.45 s), 23.5 MB peak; Requests tab 1.95 s. At `-c 1000`
against the 20 ms target, 50,000 requests: 0.63 s CPU (was 2.26 s), 85 MB.

In short, for the same requests pepe now uses:

- **4 to 4.5× less CPU than before**, on every workload;
- **2.4 to 3.2× less CPU than oha** and 5 to 25× less than vegeta;
- **less memory than either**: 13 MB at 64 connections (oha 79 MB), 68 MB
  at 1,000 (oha 111 MB), and 40% less than before at 1,000.

Throughput on one thread is 100k requests a second for small responses,
within a few percent of oha on 14 threads; `--threads 3` reaches the
server's limit at about twice oha's CPU efficiency. Where pepe is slower on
the table (tiny-c1000, tls16k) one core is saturated, which the dashboard
and the report now say, and `--threads 2` is the answer.

## Linux

The same suite on GitHub's `ubuntu-latest` runner (4 vCPUs, Azure), from
the Benchmarks workflow, pepe 0.9.0 and oha 1.16. CPU ms per 1,000
requests / peak RSS MB / requests per second:

| Workload | pepe | oha |
| --- | --- | --- |
| tiny-c64 | 25.8 / 9.6 / 38k | 19.8 / 96.2 / 69k |
| tiny-c256 | 26.9 / 17.4 / 37k | 19.1 / 99.4 / 72k |
| tiny-c1000 | 33.5 / 47.5 / 30k | 21.4 / 124.1 / 68k |
| json-c64 | 25.8 / 9.5 / 38k | 20.4 / 95.5 / 61k |
| body16k-c64 | 40.0 / 12.0 / 24k | 31.6 / 50.0 / 40k |
| body256k-c16 | 70.0 / 18.3 / 13k | 70.0 / 46.7 / 18k |
| slow20ms-c1000 | 37.4 / 45.6 / 26k | 23.6 / 81.1 / 43k |
| status503-c64 | 25.9 / 9.4 / 38k | 20.1 / 68.4 / 67k |
| tls-c64 | 27.6 / 10.5 / 36k | 24.0 / 83.7 / 56k |
| tls16k-c64 | 55.3 / 14.5 / 18k | 59.0 / 41.3 / 26k |
| post-c64 | 26.3 / 9.5 / 38k | 19.8 / 96.0 / 67k |

Two things to read off this, and one caveat.

- **Memory** is the same story as on macOS: pepe holds a tenth of oha's at
  64 connections and under half at 1,000.
- **CPU is not.** On this machine pepe spends about 30% more CPU per
  request than oha and reaches half its throughput, where on the M4 Pro
  it spent 2.5× less. oha costs about the same on both platforms, so the
  difference is pepe's: something in its per-request path is more
  expensive on Linux than on macOS, and one sending thread on a slow
  vCPU caps throughput sooner. It is the next thing to profile; until it
  is understood, the CPU claims above are macOS measurements.
- **Where the Linux cost is**, from `perf` on the runner (the Profile on
  Linux workflow): per request, pepe and oha both make exactly one
  `writev`, one `recvfrom` and an `epoll_wait` every fifteen or so, so it
  isn't syscalls. The difference is user space, and it is reqwest's
  per-request plumbing: reqwest's tower follow-redirect layer formats the
  request URI to a string and runs a full `Url::parse` (IDNA included) on
  **every** response, redirect or not (`client.rs`, `Pending::poll`); the
  connector is cloned and dropped per request; the pool checkout hashes the
  authority; the default headers' `Bytes` are refcounted up and down.
  Together about a tenth of pepe's CPU on Linux, less on macOS where
  those paths are cheaper. Two things were tried, each measured on the same
  runner against the same baseline:
  - **reqwest without the per-response re-parse**
    ([`patches/`](patches/)): 3.5–4% less CPU on Linux, 2–5% on macOS, no
    memory change. It belongs upstream; pepe doesn't carry a fork.
  - **mimalloc as the global allocator**: 1–4% less CPU on Linux but
    50–90% more peak memory (10 → 18 MB at 64 connections, 45 → 68 MB at
    1,000), the gate flagged it, and it is not kept. On macOS it was 5–7%
    less CPU and slightly less memory, which is not enough to carry a
    platform split.
  What's left is the shape of the reqwest and hyper-util stack; removing
  it means a leaner HTTP/1.1 path of pepe's own, which is a different
  project (see "Not now" in ROADMAP.md).
- **Noise.** Shared runners vary a lot: the same pepe binary cost 25.7 ms
  per 1,000 requests on one `ubuntu-latest` run and 13.4 on another an
  hour later, with oha moving from 19.8 to 10.1 alongside it, so the
  ratio held while the absolutes halved. Shared runners also vary: pepe's tiny-c64 cost came out at 16.9,
  22.5, 17.4 and 25.8 ms per 1,000 requests in four runs of the same
  binary. That is why the gate compares two binaries taking turns in one
  job, and why these absolute numbers are for the shape of the
  comparison, not for a decimal place.

## Reproducing

```bash
go run bench/server.go &
cargo build --release
bench/run.sh > results.csv
bench/run.sh target/release/pepe-before > before.csv      # any other binary
bench/tui.py --show -- target/release/pepe -n 200000 -c 64 http://127.0.0.1:8089/
cargo build --profile profiling && sample $(pgrep -n pepe) 5   # a CPU profile, macOS
```

The connection count during a run, on macOS:

```bash
netstat -an -p tcp | awk '$5 == "127.0.0.1.8089" && $6 == "ESTABLISHED"' | wc -l
```

The raw CSVs behind the tables are in [`results/`](results/).
