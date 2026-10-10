# Flows: a sequence of requests

![pepe flow: a chain of three steps, each a row on the dashboard](assets/flow.gif)

`pepe flow` runs a chain of requests where a value from one response feeds the next: log in, take the token, use it. Each unit of concurrency is one user walking the steps in order with its own values, over and over; each step is a row on the dashboard, as endpoints are in API mode, with its own throughput, latency and statuses.

```bash
pepe flow checkout.toml -c 20 -z 1m
pepe flow checkout.toml -n 500 --json > checkout.json
```

## The flow file

```toml
# checkout.toml
name = "checkout"

[vars]
host = "https://shop.example.com"

[[step]]
name = "login"
method = "POST"
url = "{{host}}/login"
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
headers = ["Authorization: Bearer {{token}}"]
expect = 201
```

| Key | What it is |
| --- | --- |
| `name` | The flow's name (the file's name without its extension when left out) |
| `[vars]` | Values every chain starts with, for `{{holes}}` |
| `[[step]].name` | The step's name on the dashboard (`step 1`, `step 2`, … when left out) |
| `method`, `url`, `headers`, `body` | The request; `url` is required, `method` is GET by default, `headers` are `Name: value` lines, and each can have `{{holes}}` |
| `expect` | The status the step must get; without it, any 2xx passes |
| `capture` | Variables to take from the response, each `name = "spec"` |

## Holes and captures

`{{name}}` holes are filled from `[vars]` and from earlier steps' captures; a hole nothing fills is an error when the file is read, naming the step and the variable. A capture is one of:

| Spec | Takes |
| --- | --- |
| `json:$.path.to[0].value` | A value in a JSON body, by path: fields, and `[n]` indexes; `$.` is optional |
| `header:Name` | A response header's value |
| `regex:pattern` | The first group of a regular expression over the body (the whole match without a group) |
| `body` | The whole body, trimmed |

A step passes when it gets a 2xx, or the status `expect` names; a step that fails, or whose capture finds nothing, ends the chain with that said in the failure causes, and the user starts over. Up to a megabyte of each response is kept for its captures.

## Running it

`-n` counts chains, not requests; `-c`, `-z`, `-H` (sent with every step), `-t`, `--rate` (which paces every request, steps included), `--warmup` and the other options work as usual. There is no setup screen for a flow: edit the file and `r` runs it again. `--json` prints the usual report plus `flow`: its name, how many chains started and completed, and `steps`, one entry per step with requests, failures, throughput, median, p99 and status codes. Piped, the dashboard's verdict is printed. `--metrics` serves a row per step.
