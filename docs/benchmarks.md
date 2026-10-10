# Benchmarks

pepe is built to cost less than the server it tests. It is measured against oha, vegeta, wrk and k6 on every release, with the suite in [`bench/`](bench/README.md), against a local server that answers from memory, so the client is the cost being measured. CPU milliseconds per 1,000 requests, peak memory, and the requests per second each tool reported.

## Apple M4 Pro

Single-thread numbers for pepe, where the loopback tops out near 175k requests a second:

| Workload | pepe | oha | vegeta |
| --- | --- | --- | --- |
| GET, 64 connections | **6.2 ms · 9 MB** · 160k req/s | 23.0 ms · 78 MB · 161k req/s | 90.4 ms · 26 MB · 76k req/s |
| GET, 1,000 connections | **6.0 ms · 12 MB** · 163k req/s | 21.6 ms · 101 MB · 147k req/s | 52.7 ms · 92 MB · 131k req/s |
| HTTPS, 64 connections | **6.8 ms · 12 MB** · 142k req/s | 24.8 ms · 60 MB · 156k req/s | 69.8 ms · 30 MB · 90k req/s |

## Linux

A 4-vCPU arm64 VM, the musl build that is released, with wrk on one thread beside it:

| Workload | pepe | wrk | oha |
| --- | --- | --- | --- |
| GET, 64 connections | **2.4 ms · 4.0 MB** · 398k req/s | 3.5 ms · 4.5 MB · 282k req/s | 6.8 ms · 67 MB · 305k req/s |
| GET, 1,000 connections | **3.2 ms · 5.8 MB** · 296k req/s | 4.4 ms · 7.7 MB · 228k req/s | 5.6 ms · 73 MB · 183k req/s |
| HTTPS, 64 connections | **3.3 ms · 6.0 MB** · 284k req/s | 4.5 ms · 10.8 MB · 218k req/s | 8.1 ms · 42 MB · 250k req/s |
| 10 million requests, 256 connections | **2.9 ms · 4.5 MB** · 343k req/s | 3.3 ms · 4.6 MB · 304k req/s | 6.5 ms · 2,404 MB · 316k req/s |

## How

A share-nothing engine: each sending thread keeps its own connections and reads and writes them itself. The request is bytes made before the run; the response is parsed where it was read; nothing is allocated for a request, and a thread's connections share one read buffer. Proxies, redirects and flows go through a general client instead, and the benchmark's README says where pepe is level rather than ahead (a slow target at 1,000 connections, 16 KB bodies over TLS), what a glibc build changes, and how k6 does.

Where a target can take more than one thread sends, pepe says so, in the footer, the verdict and `generator.peak_busy_percent`, and `--threads auto` adds them ([Load testing: threads](load-test.md#threads)).

Every workload, the profiles and the method are in [bench/README.md](bench/README.md); a benchmark gate runs on every release pull request.
