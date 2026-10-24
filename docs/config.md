# Config files

Anything you can type on the command line can live in a file instead, next to the code it tests, committed and shared. pepe reads four kinds of TOML file, and you only ever need to write two of them by hand:

| File | Where | What it's for |
| --- | --- | --- |
| `pepe.toml` | The directory you run pepe in | A project's test and guardrails. `pepe` alone runs it |
| `.pepe/tests/NAME.toml` | Beside it | Named tests, the same format: `pepe --load NAME` |
| `settings.toml` | `~/.config/pepe/` | This machine's defaults and caps, for every project |
| A flow file | Anywhere | A sequence of requests: `pepe flow checkout.toml` |

The fastest way to a file is to let pepe write it. Type the run you want, add `--write-config`, and run `pepe` after:

```bash
pepe https://api.example.com/health -c 20 -z 30s --rate 200 --write-config pepe.toml
pepe
```

## pepe.toml

Every key is optional, and each one is a long flag's name: `--trace-header` is `trace-header`. This is all of them, with what they do:

```toml
# What to send
url = "https://api.example.com/carts/{{cart}}"  # when none is typed
method = "POST"                     # -m; GET when left out
headers = ["Content-Type: application/json"]    # -H, as many as needed
body = '{"cart": "{{cart}}"}'       # -d; ''' ... ''' for several lines
vars = "carts.csv"                  # --vars: rows fill the {{holes}}

# How much, and how fast
requests = 1000                     # -n
duration = "1m"                     # -z; wins over requests
concurrency = 50                    # -c; one per core when left out
rate = 500                          # --rate: started a second
warmup = "5s"                       # --warmup: sent, not counted
threads = "auto"                    # --threads: a number, or "auto"
timeout = 5                         # -t, in seconds; 20 when left out

# How the client behaves
user-agent = "pepe-ci"              # -u
proxy = "http://proxy:3128"         # -p
insecure = false                    # true is -k: any certificate
compression = true                  # false is --disable-compression
keep-alive = true                   # false is --disable-keepalive
redirects = true                    # false is --disable-redirects
trace-header = "X-Request-Id"       # the header with a request's id

# What to keep, and who to tell
export = ["report.html", "summary.md"]   # --export: the report as these
snapshot = "soak.json"              # --snapshot: on disk, every minute
webhook = "${SLACK_WEBHOOK_URL}"    # --webhook, or [[webhook]] tables

# Guardrails: checked before anything is sent, in every mode
allow-host = [".example.com", "localhost"]   # .example.com and under it
max-requests = 10000                # -n under it, -z with a --rate
max-rate = 500                      # --rate given, and under it
max-concurrency = 64                # -c, and a ramp's top step

[ramp]                              # defaults for `pepe ramp`
from = 10                           # the first step's concurrency (10)
to = 60                             # the last step's (100)
step = 10                           # added each step (10)
every = "15s"                       # how long each is held (10s)
until = ["p99 > 500ms", "errors > 1%"]   # stop when one is crossed

[api]                               # defaults for `pepe api`
spec = "openapi.yaml"               # a file, a URL, or the document
server = "https://staging.example.com"   # the base URL, over the spec's
auth = ["bearer:TOKEN"]             # or basic:U:P, apikey:, header:N=V
tag = ["Billing"]                   # the endpoints with these tags
only = ["GET /pets*"]               # or these
skip = ["/admin*"]                  # never these
set = ["id=1,2,3"]                  # values for the parameters
all = false                         # true: every endpoint it can
include-writes = false              # true lets the writes in

[ping]                              # defaults for `pepe ping`
targets = ["api.example.com", "cdn.example.com"]
name = ["api", "cdn"]               # one per target
every = "1s"                        # the time between pings
slo = "total=500,ttfb=200"          # limits in ms; exit 4 when broken
keep-alive = false                  # true: only the server is timed
http1 = false                       # true: HTTP/1.1, never HTTP/2
ipv4 = false                        # -4
ipv6 = false                        # -6
all-ips = false                     # each address a name has, a target
interface = "en0"                   # send from it, or an address
tcp = false                         # true: only the connect, a TCP ping
port = 80                           # the port a TCP ping uses
cmd = false                         # true: the targets are commands
```

### The rules

- **Flags win.** Anything typed on the command line is used over the file, so `pepe -c 100` runs this file at concurrency 100.
- **Length is said once.** Typing `-n` or `-z` drops both `requests` and `duration` from the file.
- **Under a ramp,** `max-requests` and `max-rate` refuse it: a ramp has no bound on what it sends.
- **On/off keys only switch on.** `insecure = true` is `-k`, `redirects = false` is `--disable-redirects`; a file can't undo a flag that was typed.
- **A key pepe doesn't know is an error** that names it and where it is, before anything is sent. A typo never runs a different test.
- **Only this directory.** pepe reads `./pepe.toml` where it runs, not a parent's. `--config FILE` reads another file, and that one must exist.
- **Every mode reads it.** `[ramp]`, `[api]` and `[ping]` hold defaults for when you type that mode; they don't pick the mode. `pepe ramp` runs the ramp, `pepe` alone runs the plain test.
- **`vars` is for a run, a ramp or a flow.** With it in the file, `pepe api` and `pepe ping` are refused; keep a CSV-driven test as a [saved test](#saved-tests) instead.

### Recipes

A file that keeps a project safe for anyone, and any agent, who runs pepe in it:

```toml
allow-host = [".staging.example.com", "localhost"]
max-requests = 10000
max-rate = 500
max-concurrency = 64
```

A soak test that runs for an hour at a steady rate and keeps its report current on disk:

```toml
url = "https://staging.example.com/checkout"
duration = "1h"
rate = 200
concurrency = 100
snapshot = "soak.json"
export = ["soak.html"]
```

A load test with a different user on every request, from a CSV whose first line names the columns:

```toml
url = "https://staging.example.com/users/{{id}}"
headers = ["Authorization: Bearer {{token}}"]
vars = "users.csv"        # id,token
duration = "30s"
concurrency = 20
```

A capacity search that stops at the first sign of trouble, run with `pepe ramp`:

```toml
url = "https://staging.example.com/search?q=pepper"

[ramp]
to = 400
step = 20
every = "20s"
until = ["p99 > 300ms", "errors > 0.5%"]
```

A watch on two endpoints with limits, run with `pepe ping`:

```toml
[ping]
targets = ["https://api.example.com/health", "https://cdn.example.com/"]
name = ["api", "cdn"]
slo = "total=300,ttfb=150"
```

## Webhooks

`webhook` is one URL, or as many `[[webhook]]` tables as you need. A table says when it's sent, how, and with what:

```toml
[[webhook]]
url = "${SLACK_WEBHOOK_URL}"
on = ["start", "progress", "end"]  # or anomaly, fail; end by default
every = "30s"                      # how often progress goes (60s)

[[webhook]]
url = "${PAGER_WEBHOOK_URL}"
on = ["fail"]                      # quiet until something is wrong
mention = "<!here>"                # before a failed run's message

[[webhook]]
url = "https://ci.example.com/hooks/pepe"
format = "json"                    # or auto, slack, discord, teams…
headers = ["Authorization: Bearer ${CI_TOKEN}"]
body = '{"status": "{{status}}", "p99": "{{p99}}", "test": "{{test}}"}'
```

| Key | What it is |
| --- | --- |
| `url` | Required. An `http(s)` URL, or `slack:#channel` (`slack:` alone for the connected channel) after `pepe slack connect` |
| `on` | When it's sent; `slack:` hooks default to `start`, `progress`, `anomaly` and `end` |
| `format` | `auto` lays a message out for Slack, Discord, Teams and Google Chat URLs, and sends the JSON report anywhere else |
| `every` | How often `progress` is sent |
| `mention` | Text before a failed run's message: `<!here>`, `<@U123>` |
| `headers` | Extra `Name: value` headers on the request |
| `body` | Your own body. Its holes: `event`, `status`, `level`, `title`, `text`, `mode`, `target`, `test`, `command`, `requests`, `failed`, `rps`, `p50`, `p95`, `p99`, `errors`, `duration`, `progress`, `verdict`, `findings`, `report`. Any other hole is an error before the run |

`--webhook` on the command line replaces the file's hooks. More on what each one sends: [Webhooks](webhooks.md).

## Secrets

`${NAME}` is read from the environment when the run starts, in a webhook's `url`, `headers`, `body` and `mention`, and nowhere else. A name that isn't set is an error before anything is sent, so a webhook's secret never has to be in the file.

A request's own headers and body are taken as written. Keep a token out of a committed file by typing it when you run:

```bash
pepe -H "Authorization: Bearer $API_TOKEN"
```

A saved test keeps its headers as they were given, tokens included, so look at `.pepe/tests/` before you commit it. Reports and messages mask them; the file doesn't.

## Saved tests

A project keeps as many tests as it needs in `.pepe/tests/`, one file each, in the same format as `pepe.toml`. You rarely write one by hand:

```bash
pepe https://api.example.com/health -n 1000 -c 10 --keep api-health   # keep it, then run it
pepe --load api-health                                                # run it again
pepe --load api-health -c 20 --keep                                   # change it in place
pepe --load api-health --keep api-health-20                           # or keep a copy
pepe rerun api-health --gate                                          # run, record, compare with the last
```

`ctrl-s` on the setup screen or a dashboard does the same as `--keep`. With saved tests in the project, `pepe` alone opens on the list of them.

- **A name** is letters, digits, `.`, `-` and `_`, up to 64, not starting with a dot.
- **The file picks the mode.** `--load` runs a test with a `[ramp]` table as a ramp, one with `[api]` as an API test, one with `[ping]` as a ping, anything else as a plain run.
- **`pepe.toml` isn't read** under `--load`; the test says it all, over the machine's settings. `--load` and `--config` don't go together.
- **A run, a ramp, an API test or a ping** can be kept. A flow is already a file, and a replay is its log.
- `--record` keeps each run's report under the test, as `.pepe/runs/NAME/<moment>.json`, for `pepe history NAME` and `pepe compare NAME`: [Recorded runs](load-test.md#recorded-runs).

## settings.toml

This machine's own file, written by the settings page (`F2` on any screen) and safe to edit by hand. It's `~/.config/pepe/settings.toml` (`$XDG_CONFIG_HOME/pepe/` when that's set, `%APPDATA%\pepe\` on Windows, or the directory `PEPE_CONFIG_DIR` names):

```toml
theme = "nord"            # PEPE_THEME wins over it
update-check = false      # never look for a newer release

[defaults]                # any pepe.toml key, for every run here
concurrency = 50
timeout = 5
allow-host = [".staging.example.com", "localhost"]
max-rate = 500
max-concurrency = 64

[defaults.ramp]           # and the tables, under defaults.
to = 200

[[defaults.webhook]]
url = "${SLACK_WEBHOOK_URL}"
on = ["fail"]
```

A settings file that can't be read is an error, never skipped, so a cap is never dropped without saying so.

## Which wins

From strongest to weakest:

1. What's typed on the command line
2. `pepe.toml`, or the saved test `--load` names
3. `[defaults]` in `settings.toml`
4. pepe's own defaults

A key a file leaves out is taken from the one under it, a table key by key (`[ramp]`, `[api]`) or whole (`[ping]`). `vars` and `export` from `[defaults]` apply only where there's no `pepe.toml` or saved test.

The guardrails are the exception: every place that sets them holds at once. `[defaults]`, `./pepe.toml` (read for its guardrails even when `--load` or `--config` names another file), the saved test or `--config` file, and the command line each count. Of their `max-requests`, `max-rate` and `max-concurrency` the lowest holds, and a host must be allowed by every `allow-host` list that names hosts. A flag can tighten a file's guardrails for one run, but never loosen them: to allow more, change the file.

## Flow files

A flow is a list of requests, each able to use what the ones before it took from their responses: log in, take the token, use it. Its file is named on the command line, `pepe flow checkout.toml`:

```toml
name = "checkout"                   # the file's name when left out

[vars]                              # every chain starts with these
host = "https://shop.example.com"

[[step]]
name = "login"                      # "step 1" when left out
method = "POST"                     # GET when left out
url = "{{host}}/login"              # required
headers = ["Content-Type: application/json"]
body = '{"user": "demo", "password": "demo"}'
capture = { token = "json:$.token", session = "header:Set-Cookie" }

[[step]]
name = "cart"
url = "{{host}}/cart"
headers = ["Authorization: Bearer {{token}}"]
capture = { cart = "json:$.items[0].id" }

[[step]]
name = "checkout"
method = "POST"
url = "{{host}}/cart/{{cart}}/checkout"
headers = ["Authorization: Bearer {{token}}", "Cookie: {{session}}"]
expect = 201                        # the status it must get; or any 2xx
```

| A capture | Takes |
| --- | --- |
| `json:$.path.to[0].value` | A value in a JSON body; `$.` is optional and `$["key"]` works |
| `header:Name` | A response header's value |
| `regex:pattern` | The pattern's first group in the body, or the whole match |
| `body` | The whole body, trimmed |

- `[vars]` values are strings: `id = "7"`, not `id = 7`.
- `{{name}}` works in `url`, `headers` and `body`, filled from `[vars]`, then the row of `--vars users.csv`, then the captures of earlier steps, each over the one before.
- **A hole nothing fills is an error** when the file is read, naming the step and the hole. A step can't use its own captures.
- **A chain ends** at a step that gets the wrong status or whose capture finds nothing, and the next user starts over.
- `pepe.toml`'s guardrails hold under `pepe flow`. With `allow-host`, a step whose host comes from a capture is refused, since pepe can't check it before the run.

Running it, and what the report says: [Flows](flow.md).

## slack.toml

Written by `pepe slack connect` beside `settings.toml`, readable by you alone, and changed by `pepe slack allow` and `pepe slack forget`. It holds the bot's token and the default channel; leave it to those commands. `PEPE_SLACK_TOKEN` and `PEPE_SLACK_APP_TOKEN` win over it. See [Webhooks](webhooks.md).

## When a file is refused

| What's wrong | pepe says |
| --- | --- |
| A key that isn't one | `pepe.toml: unknown field concurency, expected one of …` |
| A number in quotes | `invalid type: string "50", expected u32 (at character 15)` |
| A cap, and no rate | `--max-rate 500 needs a --rate under it` |
| A host not allowed | `evil.example.org isn't an allowed host` |
| A ramp under a cap | `a ramp … has no bound on its requests; --max-requests 10000 can't hold it` |
| A hole nothing fills | `step "cart" uses {{token}}, which no earlier step captures and [vars] doesn't set` |
| A flow with no step | `a flow needs at least one [[step]]` |
| No such saved test | `no test named api (saved: api-health, checkout)` |

Numbers and booleans go bare, never in quotes. The guardrails' refusals are exit code 2, the others 1. `--dry-run` reads every file, applies every rule, says what would be sent and where, and sends nothing: the way to check a file.
