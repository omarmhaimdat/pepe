# pepe serve

`pepe serve` is pepe over HTTP: a script, an agent or a browser starts,
watches, steers and compares runs, and `pepe worker` on other machines
takes shares of a run. It sends no load itself: every run is a child
`pepe`, under the server's guardrails, the project's `pepe.toml` and the
machine's settings, exactly as on the command line.

```bash
pepe serve > token &                         # 127.0.0.1:7878; the token is stdout's one line
TOKEN=$(head -1 token)
curl -H "Authorization: Bearer $TOKEN" localhost:7878/api/runs \
     -d '{"url": "http://127.0.0.1:8080/health", "requests": 5000, "concurrency": 20}'
```

## The page

`http://127.0.0.1:7878/` is pepe in a browser, every mode of it: a load
test (or a curl command pasted in), an OpenAPI spec uploaded and its
endpoints picked, a flow written step by step, an access log replayed, a
ping, a ramp, each with its options (headers, body, limits, timeout,
threads, TLS, keep-alive, redirects, a proxy, values from a CSV) and its
plan checked before it starts. A run is live (its curves, its verdict as
it forms, pause, resume, concurrency and stop) and ends with what its
mode has to say. It reads as k6's dashboard does: an overview of tiles
and charts a second at a time (request rate and errors, p50/p90/p99,
data received, concurrency, with the anomalies marked), the run's
`--fail-if` limits held or crossed, a timings tab (the latency's whole
distribution, each percentile, where the time went, an open loop's
schedule), a summary of every trend and counter (status codes, failures
with their bodies, the slowest requests, Server-Timing, spans, whether
pepe itself was the limit), and each endpoint, step, URL, ramp step or
target. The runs list has each run's numbers; a saved test, its trend
run after run; compare, the two latency curves over each other. The
saved tests and their history, schedules, compare and the workers are
there too. It is one page in the
binary, no script from anywhere else. The link pepe serve prints opens it
with the token, which the page keeps for the tab and takes out of the
address bar; a report opens as a link of its own, with a ticket for that
run alone that runs out in ten minutes.

## Safe to leave running

- **Loopback by default.** `--listen` takes another address; any but
  loopback needs `--allow-host` and the `--max-requests`, `--max-rate`
  and `--max-concurrency` caps (in the flags, a `pepe.toml` or the
  settings), or the server doesn't start.
- **A token on every call** but `/api/health`: made at start and printed,
  or read from `--token-file` (`name token` a line, the name going in the
  audit log). A header, never a cookie, so a page in a browser can't
  start a run.
- **The guardrails hold every run.** A request can't loosen them: its own
  guardrail keys only tighten, a value is never read as a flag, and what
  reaches past them isn't taken (`webhook`, `slack`, `otlp`, `save`, a
  file outside the project).
- **`--max-runs`** (1 by default): runs going at once, here and on the
  workers; past it, `429`.
- **An audit log**: who did what, `.pepe/serve/audit.jsonl`.

## Runs

A run takes the arguments `pepe mcp`'s tools do (`url`, `requests`,
`duration`, `concurrency`, `rate`, `warmup`, `headers`, `body`, `method`,
`fail_if`, …), with `mode` for `ping`, `ramp`, `api`, `flow` or `replay`,
or a saved test by name.

| Call | Does |
| --- | --- |
| `POST /api/runs` | Starts one: `{"url": …, "requests": 1000}`, `{"mode": "ping", "targets": […]}`, `{"test": "checkout"}` (recorded with the test's runs). `202` and its id |
| `POST /api/runs` with `"dry_run": true` | The plan, nothing sent |
| `GET /api/runs` | Recent runs, and how many are going; each ended one with its `glance`: verdict, requests, req/s, p50, p99, failed % |
| `GET /api/runs/ID` | How it stands, and its report once it has one |
| `GET /api/runs/ID/events` | The run live, as Server-Sent Events: `start`, `progress` a second (the run so far, and a load test's last second on its own as `second`), `anomaly`, `steered`, `report`, `end`. `Last-Event-ID` (or `?after=`) goes on from there |
| `GET /api/runs/ID/report` | The report: `?format=json` (the default), `html`, `md`, `txt` or `csv` |
| `POST /api/runs/ID/ticket` | A ticket that opens the run's report as a plain link (`?ticket=…`), and nothing else, for ten minutes |
| `POST /api/runs/ID/pause` · `/resume` · `/stop` | Steers it; a ping can be stopped |
| `POST /api/runs/ID/concurrency` `{"n": 40}` | Held to `--max-concurrency` |

A refusal says why in pepe's words: `422` when a guardrail holds,
`400` for what pepe can't run, `409` for what can't be done now.

```bash
curl -N -H "Authorization: Bearer $TOKEN" localhost:7878/api/runs/$ID/events
```

The same lines come from `pepe --events FILE` (or `-` for stdout) on any
run, server or not.

## Tests, history, compare

| Call | Does |
| --- | --- |
| `GET /api/tests` | The saved tests (`.pepe/tests/`) |
| `GET /api/tests/NAME` | One, as its keys and as its file |
| `PUT /api/tests/NAME` | Saves one: `{"config": {"url": …}}`, with a `pepe.toml`'s keys, or `{"toml": "…"}` |
| `DELETE /api/tests/NAME` | |
| `GET /api/tests/NAME/runs` | Its recorded runs, newest first, as `NAME/WHEN` |
| `GET /api/compare?before=A&after=B` | `pepe compare`: a run's id or `NAME/WHEN` each; `&gate=1` for `regression` |
| `GET /api/schema/run` | A report's JSON Schema: `run`, `ramp`, `ping`, `compare` |

Runs started from the server and from the command line are the same
files: `pepe history NAME` lists both.

## Files and helpers

| Call | Does |
| --- | --- |
| `PUT /api/files/NAME` | Keeps the body (up to 256 MB) as `.pepe/serve/files/NAME`, a file a run can name: a spec, a log, a flow, a CSV |
| `GET /api/files` · `DELETE /api/files/NAME` | |
| `GET /api/spec?file=PATH` | A spec in the project: its title, servers, tags, auth and every operation, to pick from |
| `POST /api/curl` `{"command": "curl …"}` | The request a curl command is, as a run's arguments, and what of it pepe can't honour |

## Schedules

| Call | Does |
| --- | --- |
| `POST /api/schedules` `{"test": "checkout", "every": "1h"}` | Runs it every hour, from an hour from now; a minute at the least. `"workers"` shares it out |
| `GET /api/schedules` | With when each last ran, or why it was skipped (the server at `--max-runs`) |
| `DELETE /api/schedules/ID` | |

## Workers

A worker joins a server from its own machine, so a run can come from
several places at once, and works from behind NAT: it dials out.

```bash
pepe --allow-host .staging.example.com --max-concurrency 200 \
     worker --join http://pepe.internal:7878 --name eu-1 --token-file token
```

- **Its own guardrails hold.** A worker runs its share under the flags it
  was started with, the `pepe.toml` where it runs and its settings,
  whatever the server asks.
- **A run shared out**: `POST /api/runs` with `"workers": "all"` or a list
  of names. The count, the concurrency and the rate are split among them
  (a duration is each one's); the run's events carry each worker's, and a
  progress line a second adds them up.
- **Its report is one run's.** Each share's latency histogram comes back
  whole, and they're added bucket by bucket, so the p99 is the p99 of
  every request any worker sent. `distributed` says what each sent.
- A worker that leaves mid-run ends its share as failed; the run ends on
  the others, failing. A worker's process killed outright leaves its
  share's child to the end of its count or its time.
- `GET /api/workers` lists them, and what each is busy with.

A plain run and a saved test of one are shared out; the other modes run
on the server.

## Health

`GET /api/health` (no token): pepe's version and the reports'
`schema_version`, for a load balancer or a script waiting for the server.
