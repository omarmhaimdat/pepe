# Contributing

Issues and pull requests are welcome. This is what you need to know to
work on pepe; the user-facing documentation is at
[pepe.mhaimdat.com/docs](https://pepe.mhaimdat.com/docs/).

## Building and testing

```bash
cargo build --release          # the binary, in target/release/pepe
cargo test                     # 300+ tests, including every screen at many terminal sizes
cargo clippy --all-targets -- -D warnings && cargo fmt --check
```

CI runs the same on macOS, Linux and Windows, plus the minimum Rust
version (1.85) and a Nix build. A pull request that passes locally
passes there.

## Generated files

Some files are generated from the code and checked by `cargo test`, so
they can't drift. After changing a flag, a subcommand or a page:

```bash
UPDATE_CONTRIB=1 cargo test contrib   # contrib/: completions and man pages; docs/reference.md
python3 site/build-docs.py            # site/docs/: the documentation site, from docs/*.md
```

The docs site's sources are the Markdown files under `docs/`; `site/build-docs.py`
builds them with no dependency, and `--check` is what CI runs.

## Recordings

The GIFs in `assets/` are recorded with [vhs](https://github.com/charmbracelet/vhs)
from the tapes in `assets/tapes/`:

```bash
cargo build --release
assets/record.sh                 # all of them, against a local target it starts
assets/record.sh assets/tapes/ping.tape
```

## Benchmarks

`bench/` has the suite that measures pepe against oha, vegeta, wrk and
k6, and [bench/README.md](bench/README.md) the method and the numbers.
To measure a change:

```bash
cargo run --release --manifest-path bench/server/Cargo.toml &
bench/run.sh
```

A benchmark gate runs on release pull requests.

## Windows

Before a release that touches the screens, the installers, or files and
paths, go through [docs/windows-checklist.md](docs/windows-checklist.md)
on a Windows machine; CI can't press keys there.

## Commits and releases

Commits follow [conventional commits](https://www.conventionalcommits.org/):
`feat:`, `fix:`, `perf:` make a release, `docs:` and `chore:` ride along
with the next one. The body of a commit becomes its release note, so say
what changed and why in it. Merging to `master` keeps a release PR open
that bumps the version and writes the changelog from the commits; merging
that PR tags the release, which builds every platform and publishes the
GitHub Release, the installers, the Homebrew formula, the Docker image,
the action and the site.

## Where things are

| Path | What |
| --- | --- |
| `src/main.rs` | The runners for each mode, and where a run starts |
| `src/cli.rs` | Every flag and subcommand (clap) |
| `src/load.rs`, `src/direct.rs`, `src/wire.rs` | The load engine: workers, the direct HTTP/1.1 path, the protocol |
| `src/ping.rs`, `src/diagnose.rs` | `pepe ping` and its findings |
| `src/ramp.rs`, `src/api.rs`, `src/openapi.rs`, `src/flow.rs`, `src/replay.rs`, `src/logs.rs`, `src/compare.rs` | The other modes |
| `src/ui/` | The screens (ratatui): the dashboard, setup, plan, ramp, logs, ping |
| `src/insights.rs` | The verdict and the anomalies |
| `src/json_report.rs`, `schema/` | The JSON reports and their schemas |
| `src/guard.rs`, `src/mcp.rs` | The guardrails and the MCP server |
| `src/exporter.rs` | `--metrics` for Prometheus |
| `docs/`, `site/` | The documentation site's sources and build |
| `action.yml` | The GitHub Action |
