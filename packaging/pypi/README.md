# pepe

HTTP load generator and latency tool, one binary: a load test with a live
dashboard, `pepe ping` (a request a second, split into DNS, connect, TLS,
first byte and download), `pepe ramp` (find the capacity), `pepe api` (an
OpenAPI spec), `pepe flow` (a sequence of requests), `pepe replay` (an
access log), `pepe compare` (two reports). The documentation is at
[pepe.mhaimdat.com/docs](https://pepe.mhaimdat.com/docs/).

```bash
pipx install pepe-load    # or: pip install pepe-load
pepe https://example.com
```

pepe is written in Rust, and this package is the `pepe` command that
fetches it: the first run downloads the release of the same version for
your platform (macOS, Linux, Windows) from
[GitHub releases](https://github.com/omarmhaimdat/pepe/releases), checks
it against the SHA-256 the release carries, keeps it in your cache
directory (`~/.cache/pepe-load`, `~/Library/Caches/pepe-load` on macOS,
`%LOCALAPPDATA%\pepe-load\Cache` on Windows; `PEPE_LOAD_CACHE_DIR` moves
it) and runs it; every later run only runs it. There are no dependencies.
`python -m pepe_load` is the same command.

Upgrade with `pipx upgrade pepe-load` or `pip install -U pepe-load`: each
version of the package runs the pepe of that version, so `pepe
self-update` does not apply here. The other ways to install, with tab
completion and man pages, are on the
[install page](https://pepe.mhaimdat.com/docs/install.html).
