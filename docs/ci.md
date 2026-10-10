# CI and Docker

pepe in a pipeline: the GitHub Action, a pull request held against its base branch, thresholds, Docker, and what to read afterwards.

## The GitHub Action

The repository is also an action: it installs a pinned release, runs `pepe --json`, puts the numbers in the job summary and in outputs, and can fail the job on a condition over the report.

```yaml
- uses: omarmhaimdat/pepe@v0
  id: load
  with:
    url: https://staging.example.com/api/health
    args: -z 30s -c 20 -H 'Authorization: Bearer ${{ secrets.TOKEN }}'
    fail-if: ".summary.latency.p99_ms > 300 or .summary.failed_requests > 0"
- run: echo "p99 was ${{ steps.load.outputs.p99_ms }} ms at ${{ steps.load.outputs.requests_per_second }} req/s"
```

### Inputs

| Input | Default | What it does |
| --- | --- | --- |
| `url` | | The URL to load-test; can be left empty when it is in `args` |
| `args` | `-n 100` | pepe's flags, e.g. `-n 1000 -c 20 -H 'Accept: application/json'`; don't pass `--json`, the action does |
| `version` | `latest` | The release to use, e.g. `0.9.0` |
| `fail-if` | | A `jq` condition on the JSON report; when true, the step fails. (pepe's own `--fail-if 'p99 > 300ms'` in `args` does the same with exit code 4.) |
| `baseline` | | `auto` keeps each branch's last report in the Actions cache and holds a pull request against its base branch's; a path names a report file to compare with |
| `comment` | `false` | On a pull request, post the result as a comment, updated on later runs; the job needs `pull-requests: write` |
| `gate` | `false` | Fail the step when the comparison says Slower or Worse |

### Outputs

| Output | What it is |
| --- | --- |
| `report` | Path of the JSON report |
| `total_requests`, `failed_requests`, `requests_per_second`, `p50_ms`, `p99_ms` | The headline numbers |
| `compare` | Path of `pepe compare --json`'s output; empty without a baseline |
| `verdict` | `faster`, `same`, `slower`, `better` or `worse`; empty without a baseline |
| `card` | Path of the verdict drawn as an SVG card, kept in the run's artifacts |

Linux and macOS runners.

## Against the base branch

With `baseline: auto`, every run on a branch keeps its report in the Actions cache, and a pull request is held against its base branch's last one with `pepe compare`. `comment: true` posts the result on the pull request, one comment updated on every push, and `gate: true` fails the step when it says Slower or Worse. The first run on the base branch after this is added makes the baseline; until then a pull request's comment says so.

```yaml
permissions:
  pull-requests: write
steps:
  - uses: omarmhaimdat/pepe@v0
    with:
      url: https://staging.example.com/api/health
      args: -n 5000 -c 20
      baseline: auto
      comment: true
      gate: true
```

> ### pepe · ▲ Slower than `main` · https://staging.example.com/api/health
> | | `main` | this PR | |
> |---|---|---|---|
> | p99 | 120.0 ms | 166.0 ms | ▲ up 38% |
> | median | 30.0 ms | 31.0 ms | within the usual spread (±5%) |
> | throughput | 260 req/s | 252 req/s | within the usual spread (±5%) |
> | failed | 0% | 0% | |
>
> - ▲ p99 up 38%: 120.0ms → 166.0ms
> - ▲ A long tail is new: p99 is 5.4× the median, was 4.0×

`baseline` can also name a report file, for a baseline kept in the repository or fetched from elsewhere. The comparison is in the job summary too, and in the `verdict`, `compare` and `card` outputs. The comment carries the verdict as a badge, since a comment can only show an image by URL.

## Any CI

Without the action, pepe is one binary and a few lines. Run it with `--json` for the report, or piped for the text verdict; `--fail-if` sets the exit code; `pepe compare --gate` holds a report against a kept one.

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh
pepe --json -n 2000 -c 20 --fail-if 'p99 > 300ms' --fail-if 'errors > 1%' "$URL" > report.json
pepe compare baseline.json report.json --gate --svg card.svg
```

`PEPE_NO_UPDATE_CHECK` is set by the action and worth setting elsewhere in CI, so no run ends with an update notice.

## Docker

```bash
docker run --rm -it ghcr.io/omarmhaimdat/pepe -z 30s -c 50 https://example.com   # the dashboard needs -it
docker run --rm ghcr.io/omarmhaimdat/pepe --json -n 1000 https://example.com     # for scripts
docker run --rm -p 9100:9100 ghcr.io/omarmhaimdat/pepe --metrics :9100 -z 10m https://example.com
```

An empty image with the static binary in it, a few megabytes, for `linux/amd64` and `linux/arm64`; `:latest` and `:<version>` tags. Inside Docker Compose or Kubernetes, point it at the service's name; `--metrics` needs the port published.

## Soak runs in the background

For a long run on a server, `--snapshot` keeps the report current on disk and `--metrics` puts the numbers in Prometheus, so the run can be read from anywhere while it goes and after ([Output](output.md)).

```bash
nohup pepe -z 12h -c 50 --snapshot soak.json --metrics :9100 https://example.com > soak.txt 2>&1 &
```
