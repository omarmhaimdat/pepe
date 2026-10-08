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
  target server is shared by every tool, so throughput near its ceiling is
  the server's limit, not the tool's.

**Target.** [`server/`](server/), a Rust server (hyper on every core)
answering from memory. It is faster than any of the clients, so the client
is the bottleneck. `cargo run --release --manifest-path
bench/server/Cargo.toml` serves HTTP on `127.0.0.1:8089` and HTTPS
(self-signed) on `127.0.0.1:8090`; `/count` says how many requests it has
answered, to hold against what a client says it sent. Until v0.16 the
target was a Go `net/http` server with the same routes, and the tables
down to [The direct path](#the-direct-path) were measured against it. The
Rust one answers the same load with half the CPU and has a higher ceiling
(on the M4 Pro about 170k requests a second against 150k, which is where
macOS's loopback gives out), so it stays out of the client's way for
longer.

**Workloads.** [`run.sh`](run.sh) runs each of these with pepe (`--json`, no
screen) and, where the same thing can be asked of them, with whichever of
[oha](https://github.com/hatoo/oha), [vegeta](https://github.com/tsenart/vegeta),
[wrk](https://github.com/wg/wrk) and [k6](https://github.com/grafana/k6)
are installed:

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
| million-c64 | 1,000,000 | 64 | `/` | anything that grows with the count |
| *N*m-c256 | *N* million | 256 | `/` | the same, with `MILLIONS=N` |

vegeta and wrk have no request count, so they run for 5 s at full speed and
their CPU is divided by the requests they managed; wrk on one thread
(`-t1`), to be read against pepe's one. k6 runs a script, here one that
does nothing but the request.

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
cargo run --release --manifest-path bench/server/Cargo.toml &
cargo build --release
bench/run.sh                                  # the table, as CSV
MILLIONS=10 bench/run.sh                      # and ten million requests at the end
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
- **Reading `Server-Timing` and request ids** (v0.10). Every response is
  looked at for a `Server-Timing` header and for one of twelve request-id
  headers. When the target sends none, the gate's four workloads moved
  0–4%, inside their round-to-round noise. When it sends both (the bench
  server's `/timed` route: three `Server-Timing` entries and an
  `X-Request-Id`), parsing them and keeping the id costs 9.45 → 10.00 ms
  of CPU per 1,000 requests (+6%) and 0.9 MB, best of three alternating
  rounds on macOS. That is the price of the slowest-requests list and the
  server's own timing in the report; a run that doesn't get the headers
  doesn't pay it.

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

What follows is the record up to v0.16: what Linux cost before
[the direct path](#the-direct-path), and what was tried. The numbers
that hold now are in that section.

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
  project (see "Speed on Linux" in ROADMAP.md).
- **Noise.** Shared runners vary a lot: the same pepe binary cost 25.7 ms
  per 1,000 requests on one `ubuntu-latest` run and 13.4 on another an
  hour later, with oha moving from 19.8 to 10.1 alongside it, so the
  ratio held while the absolutes halved. Shared runners also vary: pepe's tiny-c64 cost came out at 16.9,
  22.5, 17.4 and 25.8 ms per 1,000 requests in four runs of the same
  binary. That is why the gate compares two binaries taking turns in one
  job, and why these absolute numbers are for the shape of the
  comparison, not for a decimal place.

## The direct path

After v0.16.0. The Linux section above ends on "what's left is the shape
of the reqwest and hyper-util stack". This is that, measured and then
removed, and after it everything else between a worker and its socket.

**Machines.** Linux: a 4-vCPU arm64 VM (Docker Desktop on the M4 Pro,
kernel 5.10, Debian 12, Rust 1.99), with the target server in the same
VM. It is not GitHub's x86 runner, where the earlier Linux table was
taken; the Benchmarks workflow fills that one in on the next release PR.
pepe is **built for musl there, as the Linux releases are**: it is the
binary people install, and it differs from a glibc build in both
directions (see "glibc" below). macOS: the M4 Pro as before. The target
is the Rust server ([`server/`](server/)), for the "before" columns too:
v0.16.0 was measured again against it, built the same way, so each row
compares like with like. oha 1.16 on Linux and 1.8 on macOS, vegeta
12.12 and 12.11, wrk 4.1, k6 1.3. The tables are the better of two runs
of the suite for every tool; the machine was in use while they ran, and
single rows moved by 20% between runs.

### Where the CPU went

`perf` on v0.16.0 (a glibc build), plain GET at 64 connections, on Linux.
By where the samples fell: 40% in pepe's own code and its crates, 35% in
the kernel, 22% in libc (the system-call stubs, `malloc` and `free`). By
what was being done:

| | share of all CPU |
| --- | --- |
| hyper's connection task: writing the request, reading and parsing the response | 30%, of which the `writev` call 25% |
| reqwest sending one request (`execute_request`, `Pending::poll`, dropping it) | 33% |
| … of which `Url::parse` of the request's own URL, for every response | 5.9% |
| pepe reading the response (`ResponseStats::read`) | 9.3% |
| … of which `HeaderMap::get` with a `&str` | 4.3% |

So a third of the CPU went to getting a request from pepe to a
connection pepe already had: the connector cloned and dropped, the pool
checked out by a hash of the authority and checked in again, the default
headers merged, a timer armed and disarmed, the retry and redirect
layers each wrapping the future in another, and the URL formatted and
parsed back for the redirect policy. And a twentieth went to pepe asking
every response for twenty headers by name (seven cache headers, twelve
request-id headers, `Server-Timing`), each `get` lowercasing and hashing
its name before finding it absent.

### What changed

Each step was measured before the next; the numbers given with a step
are that step's own, on Linux unless they say otherwise.

**1. The headers are read in one pass** ([`cache.rs`](../src/cache.rs),
[`trace.rs`](../src/trace.rs)): each of a response's few headers is held
against the lists, most ruled out by their length, instead of each name
on the lists being looked up in the response. A million plain GETs, best
of four rounds taking turns: 6.65 → 6.42 ms of CPU per 1,000 requests
(−3.5%).

**2. A worker sends on its own connection, first with hyper.** Each
worker holds one HTTP/1.1 connection per origin; what is the same for
every request is built once before the run. reqwest is left what only it
does: proxies, URLs with credentials in them, flows, and redirects (the
first redirect a target answers with hands that target to reqwest for
the rest of the run). With hyper's connection API under it this took
plain GETs from 6.9 to 4.1 ms per 1,000 and a thread from 143k to 235k
requests a second, and left pepe short of wrk (3.3 ms) and, at 1,000
connections, of oha (6.0 against 4.8). The profile said what was left:
two tasks and a channel per connection, a header table allocated for
every request and another for every response, and 40 KB held per
connection.

**3. Then without hyper** ([`direct.rs`](../src/direct.rs),
[`wire.rs`](../src/wire.rs)). The request is bytes made before the run,
head and body, and sending it is one write. The response is parsed where
it was read (httparse for the head, a decoder of pepe's own for chunked
bodies); its headers are ranges of the read buffer and its body is
counted as it passes. Nothing is allocated for a request. Same rustls,
same roots, same bytes on the wire, except that a header given twice
with `-H` now goes out twice, as the code always meant it to; reqwest's
default headers kept the last. A million GETs: 4.03 → 2.69 ms per 1,000
at 64 connections, 6.75 → 3.56 at 1,000, and 50.8 → 12.1 MB there.

**4. Connections share a read buffer.** A connection borrows a 128 KB
buffer from its thread when its socket has something to read and gives
it back before it waits again, so a thousand connections read into the
same memory, which is in the cache, and a connection is a task and a
socket. Peak memory with 16 KB bodies at 64 connections: 5.4 → 4.0 MB;
with 256 KB bodies at 16: 5.6 → 3.8 MB; CPU unchanged. Against a target
that takes 20 ms to answer 1,000 connections: 9.1 → 4.5 ms per 1,000,
best of three.

**5. Results cross to the counting thread in 32 bytes.** A result was a
184-byte record, queued between two collections 25 ms apart: at 400,000
a second that is ten thousand of them waiting. Nearly all are a status,
two times, a size and whether a cache answered, and travel as that.

**6. What a run doesn't use isn't set up.** The reqwest client is built
the first time a request needs it, and the TLS configuration for the
first HTTPS connection. The paths a request rarely takes (connecting,
the DNS probe, reqwest) are boxed, and what is kept of a response beyond
its numbers is boxed too, so a worker's future, of which there is one
per connection, went from about 3 KB to 1 KB. Steps 5 and 6 together,
on a glibc build at full speed: 7.7 → 6.0 MB at 64 connections,
12.2 → 8.1 MB at 1,000.

**7. One timer per worker, not one per request.** A worker keeps a
single timer and each request only notes its deadline; the timer, when
it fires, is moved to the deadline of whichever request is waiting then.

**8. TLS is read through a 32 KB buffer.** rustls asks its socket for
4 KB at a time, which is four or five reads for one 16 KB record. 16 KB
bodies over TLS: 10.0–10.7 → 8.6–9.2 ms per 1,000.

**9. A worker keeps the body of four failures per status, not of every
one.** The verdict shows one body per kind of failure. A target
answering 503 to everything: 2.9 → 2.5 ms per 1,000 and 6.2 → 4.1 MB.

Tried and not kept: building the dependencies that aren't on the hot
path for size (`opt-level = "s"`) made the binary 1 MB smaller and its
resident memory 0.1 MB smaller, for a slower dashboard.

### Results

CPU ms per 1,000 requests / peak RSS MB / requests per second. pepe on
one thread, as it runs by default; wrk on one thread (`-t1`); oha,
vegeta and k6 as they come, on every core.

Linux:

| Workload | pepe before | **pepe now** | change | wrk | oha | vegeta | k6 |
| --- | --- | --- | --- | --- | --- | --- | --- |
| tiny-c64 | 7.8 / 7.8 / 127k | **2.4 / 4.0 / 398k** | -69% | 3.5 / 4.5 / 282k | 6.8 / 67.0 / 305k | 19.0 / 16.6 / 148k | 32.8 / 151.3 / 92k |
| tiny-c256 | 9.7 / 16.9 / 101k | **2.5 / 4.4 / 380k** | -75% | 3.5 / 4.6 / 279k | 5.2 / 66.4 / 402k | 20.2 / 25.7 / 145k | 33.6 / 191.0 / 93k |
| tiny-c1000 | 13.3 / 51.8 / 73k | **3.2 / 5.8 / 296k** | -76% | 4.4 / 7.7 / 228k | 5.6 / 72.5 / 183k | 24.2 / 64.8 / 126k | 44.2 / 336.8 / 74k |
| json-c64 | 7.4 / 7.9 / 133k | **2.4 / 4.0 / 399k** | -68% | 3.8 / 4.7 / 265k | 6.8 / 89.4 / 308k | 19.2 / 16.7 / 139k | 33.4 / 153.6 / 90k |
| body16k-c64 | 9.2 / 9.8 / 99k | **4.2 / 3.9 / 198k** | -54% | 5.8 / 4.5 / 172k | 9.6 / 25.2 / 196k | 101.3 / 23.6 / 26k | 42.4 / 116.7 / 58k |
| body256k-c16 | 66.0 / 9.0 / 14k | **18.0 / 3.8 / 40k** | -73% | 38.2 / 4.4 / 26k | 32.0 / 26.9 / 47k | 759.1 / 43.0 / 3k | 92.0 / 54.2 / 32k |
| slow20ms-c1000 | 13.6 / 48.0 / 41k | **6.8 / 5.7 / 40k** | -50% | 5.9 / 7.7 / 43k | 8.8 / 52.8 / 42k | 26.3 / 62.0 / 43k | 48.2 / 292.6 / 40k |
| status503-c64 | 7.9 / 7.9 / 125k | **2.6 / 4.1 / 362k** | -67% | 3.1 / 4.5 / 316k | 6.3 / 50.5 / 323k | 17.5 / 16.5 / 160k | 30.3 / 133.0 / 100k |
| tls-c64 | 7.6 / 9.3 / 129k | **3.3 / 6.0 / 284k** | -57% | 4.5 / 10.8 / 218k | 8.1 / 42.2 / 250k | 20.3 / 18.7 / 137k | 30.6 / 142.3 / 100k |
| tls16k-c64 | 14.3 / 11.6 / 63k | **9.0 / 6.9 / 99k** | -37% | 9.8 / 11.5 / 101k | 15.0 / 23.0 / 121k | 80.7 / 24.0 / 33k | 39.0 / 103.8 / 87k |
| post-c64 | 7.8 / 7.9 / 125k | **2.5 / 4.0 / 378k** | -68% | — | 7.2 / 88.1 / 293k | — | — |
| million-c64 | 7.3 / 7.9 / 137k | **2.5 / 4.1 / 412k** | -66% | 3.2 / 4.5 / 315k | 6.5 / 352.3 / 335k | 17.3 / 17.1 / 159k | 28.8 / 301.8 / 100k |
| 10m-c256 | — | **2.9 / 4.5 / 343k** | — | 3.3 / 4.6 / 304k | 6.5 / 2403.6 / 316k | 20.0 / 26.6 / 147k | 31.8 / 2294.5 / 83k |

macOS (wrk and k6 were not installed there):

| Workload | pepe before | **pepe now** | change | oha | vegeta |
| --- | --- | --- | --- | --- | --- |
| tiny-c64 | 9.8 / 14.0 / 102k | **6.2 / 8.8 / 160k** | -36% | 23.0 / 77.5 / 161k | 90.4 / 25.7 / 76k |
| tiny-c256 | 10.1 / 27.4 / 99k | **5.9 / 9.5 / 166k** | -42% | 23.4 / 83.1 / 161k | 58.8 / 38.3 / 127k |
| tiny-c1000 | 11.2 / 72.8 / 89k | **6.0 / 11.9 / 163k** | -46% | 21.6 / 100.8 / 147k | 52.7 / 91.5 / 131k |
| json-c64 | 9.2 / 14.1 / 108k | **5.7 / 8.8 / 174k** | -38% | 22.6 / 77.5 / 159k | 92.7 / 26.5 / 79k |
| body16k-c64 | 10.2 / 17.8 / 95k | **6.4 / 8.7 / 153k** | -37% | 23.0 / 47.4 / 128k | 403.5 / 28.9 / 18k |
| body256k-c16 | 22.0 / 28.9 / 39k | **16.0 / 8.6 / 49k** | -27% | 78.0 / 61.2 / 40k | 650.3 / 56.0 / 8k |
| slow20ms-c1000 | 12.6 / 76.5 / 42k | **7.2 / 11.8 / 43k** | -43% | 27.0 / 77.9 / 42k | 73.5 / 81.7 / 43k |
| status503-c64 | 9.3 / 13.7 / 108k | **5.7 / 9.0 / 173k** | -39% | 22.9 / 53.5 / 161k | 84.0 / 26.0 / 77k |
| tls-c64 | 10.1 / 15.4 / 97k | **6.8 / 12.4 / 142k** | -33% | 24.8 / 60.1 / 156k | 69.8 / 30.3 / 90k |
| tls16k-c64 | 16.7 / 22.1 / 57k | **10.7 / 12.7 / 92k** | -36% | 46.3 / 44.4 / 132k | 265.4 / 36.4 / 29k |
| post-c64 | 9.8 / 13.6 / 101k | **6.2 / 8.8 / 160k** | -38% | 22.9 / 75.4 / 161k | — |
| million-c64 | 9.5 / 14.2 / 106k | **5.8 / 8.6 / 171k** | -38% | 22.6 / 229.4 / 163k | 87.6 / 25.5 / 74k |
| 10m-c256 | — | **5.9 / 9.3 / 170k** | — | 24.0 / 2021.4 / 163k | 59.8 / 40.6 / 126k |

The regression gate ([`compare.sh`](compare.sh), best of three rounds,
the two binaries taking turns), v0.16.0 against this. Linux:

| Workload | CPU ms / 1k, before | after | change | peak MB, before | after | change |
|---|---|---|---|---|---|---|
| tiny-c64 | 7.30 | 2.45 | −66.4% | 7.8 | 4.0 | −48.7% |
| body16k-c64 | 8.40 | 3.20 | −61.9% | 9.9 | 3.9 | −60.6% |
| slow20ms-c1000 | 13.20 | 8.40 | −36.4% | 47.9 | 5.7 | −88.1% |
| tls-c64 | 7.70 | 2.90 | −62.3% | 8.8 | 6.0 | −31.8% |

macOS:

| Workload | CPU ms / 1k, before | after | change | peak MB, before | after | change |
|---|---|---|---|---|---|---|
| tiny-c64 | 9.75 | 5.70 | −41.5% | 13.6 | 8.5 | −37.5% |
| body16k-c64 | 10.40 | 6.40 | −38.5% | 18.6 | 8.7 | −53.2% |
| slow20ms-c1000 | 13.00 | 7.00 | −46.2% | 71.1 | 11.5 | −83.8% |
| tls-c64 | 10.30 | 6.30 | −38.8% | 14.8 | 12.4 | −16.2% |

Reading the Linux table:

- **CPU per request: pepe is first on every workload but two, where it
  is level with wrk**: by 12 to 53% over wrk, which was the floor, and
  by 23 to 65% over oha. Against a slow target at 1,000 connections the
  table has wrk ahead (5.9 against 6.8); five 5-second runs each, taking
  turns, had pepe at 3.9 to 10.9 and wrk at 5.6 to 11.1, medians 9.8 and
  9.4, and four fifths of that workload is kernel time for either. With
  16 KB bodies over TLS the table has pepe ahead, 9.0 against 9.8, and
  repeats put each inside the other's range.
- **Memory: pepe is first on every workload**, 4.0 MB where wrk holds
  4.5, 5.8 at 1,000 connections where wrk holds 7.7, and a little over
  half of wrk's over TLS. Of those 4 MB, 3.2 are the pages of pepe's own code that the
  kernel has mapped and 0.5 are pepe's data.
- **Requests per second, one thread against wrk's one: first on every
  workload but two** (398k against 282k for plain GETs). With 16 KB
  bodies over TLS they are level, 99k and 101k. Against the slow target
  every tool is at what 1,000 connections and 20 ms allow, and pepe's
  40k counts the opening of its connections in a run of 50,000 requests
  where the others' 43k is over five seconds. oha on four threads is
  ahead of pepe's one on three rows (16 KB bodies over TLS, 256 KB
  bodies, one of the plain ones), and those are what `--threads` is
  for: see the next table.
- **Against v0.16.0**: 37 to 76% less CPU and 35 to 89% less memory.
  v0.16.0's 51.8 MB at 1,000 connections is 5.8.

All cores, where one thread is not enough. Six-second runs; pepe with
`--threads 2`, oha as it comes, wrk with `-t4`:

| Workload | pepe, 2 threads | oha | wrk -t4 |
| --- | --- | --- | --- |
| GET, 256 connections | 3.8 ms · 4.8 MB · 479k req/s | 6.1 ms · 613 MB · 356k | 4.9 ms · 5.3 MB · 382k |
| GET, 1,000 connections | 4.0 ms · 6.1 MB · 412k req/s | 7.8 ms · 386 MB · 202k | 4.1 ms · 9.0 MB · 418k |
| 256 KB bodies, 16 connections | 29.4 ms · 4.0 MB · 58k req/s | 31.5 ms · 120 MB · 52k | 46.8 ms · 4.8 MB · 46k |
| TLS, 16 KB bodies, 64 connections | 13.1–14.1 ms · 7.1 MB · 108–138k req/s | 15.4–15.7 ms · 210 MB · 123–129k | 13.5–14.1 ms · 12.1 MB · 123–135k |

With the server on the same four cores the throughput here is as much
the server's as the client's: at 1,000 connections and over TLS the
three tools are within each other's run-to-run range, and at 64
connections a second pepe thread sends less than one (365k against
417k), because the server needed the core more.

On macOS pepe spends a quarter of oha's CPU (5.7 to 6.8 ms against 22
to 25) in a ninth of its memory, and one thread reaches the loopback's
ceiling (160k to 174k requests a second) wherever responses are small
and plain; over TLS oha's fourteen threads send more than pepe's one
(156k against 142k, and 132k against 92k with 16 KB bodies). Dashboard mode,
200,000 requests at concurrency 64 on the Live tab: 1.28 s of CPU and
12.6 MB (2.01 s and 24.2 MB before).

**glibc.** Built against glibc (`cargo build --release`, as the suite's
earlier tables were), pepe costs the same CPU or a little less (2.4 to
2.9 ms for plain GETs) and v0.16.0 costs less than it does on musl (6.9
against 7.8; with 256 KB bodies 26 against 66, musl's `malloc` and
`memcpy` being what they are). Memory is the other way: 6.0 MB at 64
connections and 8.4 at 1,000, against wrk's 4.5 and 7.7, because libc,
libm and the loader are mapped beside the binary's own 3.3 MB of code.
pepe's own data is 0.75 MB of it. So on a glibc build wrk holds less
memory; on the build that is released, pepe does. The CSV is in
[`results/`](results/).

### Millions of requests

The suite's `million-c64` and `MILLIONS=10` rows are there because some
costs only show with the count. pepe's doesn't move: it keeps
histograms, not requests.

| Run | pepe | wrk | oha | k6 |
| --- | --- | --- | --- | --- |
| 1 million, `-c 64`, Linux | 2.5 ms / 1k · 4.1 MB | 3.2 ms · 4.5 MB | 6.5 ms · 352 MB | 28.8 ms · 302 MB |
| 10 million, `-c 256`, Linux | 2.9 ms / 1k · 4.5 MB | 3.3 ms · 4.6 MB | 6.5 ms · 2,404 MB | 31.8 ms · 2,295 MB |
| 10 million, `-c 256`, macOS | 5.9 ms / 1k · 9.3 MB | — | 24.0 ms · 2,021 MB | — |
| 50 million, `-c 256 --threads auto`, Linux | 3.9 ms / 1k · 5.3 MB | — | — | — |

(wrk runs for five seconds, so its row is as many requests as it sent in
that.) The long runs were held against the server's own count (`/count`
before and after). Ten million on Linux: ten million answered, none
failed, on exactly 256 connections. The 50 million: fifty million
answered, in 114 s on the two threads `auto` settled on (438k requests a
second). Four pepe processes at once on macOS, five million requests
each against one server: twenty million answered, 8.5 MB a process.

### Threads on their own

`--threads auto` starts with one thread and, once a second, adds one if
every sending thread is past 90% of a core. A second later it looks at
what the new thread was worth: a thread on *n* is worth 1/*n* more
requests a second at best, and one that brings less than a third of
that is taken back and no other is tried, because what holds the run
back is then the machine or the target, not the number of threads. A
third, not a few percent: the rate over one second moves by more than
that on its own.

Plain GETs at `-c 256`, requests per second:

| `--threads` | Linux VM (4 vCPUs, server included) | macOS |
| --- | --- | --- |
| 1 | 386k | 157k |
| 2 | 479k | 190k |
| 4 | — | 172k |
| auto | 2 threads, or 1 | 1 thread: 178k |
| oha, every core | 356k | 162k |

A machine that also runs the target is the hard case, and auto errs on
the side of fewer threads there: on the VM it settles on two when the
second's worth shows in its first second and on one when that second
was a noisy one, and on macOS, where the loopback gives out near 180k,
the second thread's 20% is under the bar. The settling shows in a short
run (a second on one thread, a second trying a thread too many), so a
run that wants only the settled numbers gives it a `--warmup`, and one
that knows what it wants says `--threads 2`.

### What is left

The profile now, by where the samples fall: 63% in the kernel, 19% in
libc (all but a point of it the `send` and `recv` stubs), 4% reading the
clock, 14% in pepe and its crates. One `send` and one `recv` per request
and the kernel's side of them are four fifths of the cost, and wrk makes
the same two calls. What is still there to take:

- **Sixteen-kilobyte records over TLS** are where pepe is level with
  wrk rather than ahead: rustls decrypts a record in its own buffer and
  copies it out, and the buffer in front of it (step 8) is one copy
  more. Its unbuffered API would take both away.
- **Memory on glibc** is the binary's code pages, which the kernel maps
  64 KB at a time around every page it touches: 3.3 of the 4.7 MB of
  code are resident. Laying the code out so that what a run uses is
  together (a profile-guided link) would halve that; it is a change to
  how every release is built.
- **Flows** build a request at every step and still go through reqwest.
- **Fewer trips to the kernel**: several requests per system call
  (io_uring) is the only thing left that is larger than a few percent,
  and it is Linux only.

## Reading logs

`pepe logs` reads on one thread. Measured on an M4 Pro, macOS, on a
generated log (`bench/gen-logs.py`, three days of it three times over:
865 MB, 5.88M `combined` lines with `rt=` and `urt=` after them), read
to the end with the report printed; the best of five runs each.

| | time | lines/s | MB/s |
| --- | ---: | ---: | ---: |
| `pepe logs`, as first written | 4.09 s | 1.44M | 211 |
| `pepe logs`, now | 2.09 s | 2.81M | 414 |
| `wc -l` (reads, parses nothing) | 0.69 s | | 1,250 |
| `awk '{c[$9]++}'` (counts one field) | 13.28 s | 0.44M | 65 |

Memory is 16 MB whatever the size. What the profile showed, and what was
done about each:

- `str::find` with a string to find builds a searcher every call, and
  the format's walk made eight calls a line: the text between two
  variables is now found by its first byte (`memchr`).
- Every line was checked for UTF-8 and split off by a byte-at-a-time
  loop: a chunk is now validated once and cut at newlines by `memchr`.
- Each count was looked up twice, once to see if it was there. A name
  seen before is now hashed once; the minute, hour, day and second being
  counted into are held outside their maps, so a log in time order
  finds them without a search.
- A `String` was allocated for every line, to keep the last 2,000: the
  line that leaves now gives its buffer to the one that comes.
- `$request_time` went through `f64`'s full parser: `0.004` is now read
  as digits.

What is left is spread evenly: hashing the path, the client and the user
agent (SipHash, kept because the names come from whoever sends
requests), reading the fields, and the counting. The next step that
would be larger than a few percent is reading on several threads. No
other log reader has been measured against it yet.

## Reproducing

```bash
cargo run --release --manifest-path bench/server/Cargo.toml &
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
