# OpenAPI: load-testing a spec

![API mode: the endpoints of a spec picked on screen, then a dashboard with a row per endpoint](assets/api.gif)

`pepe api` reads an OpenAPI 3 (or Swagger 2) spec, from a file or a URL, in JSON or YAML, and turns its operations into requests. With nothing after it, it opens the setup screen and asks for the spec: type a path or a URL, or paste the whole document.

```bash
pepe api
pepe api openapi.yaml
pepe api https://api.example.com/openapi.json --auth bearer:$TOKEN -c 20 -z 1m
pepe api openapi.yaml --tag Billing --set customer_id=1,2,3 --json > billing.json
```

## The plan screen

Once the spec is loaded, it opens on a plan screen. Nothing is sent, and no endpoint is switched on, until you say so.

- Endpoints are listed under the spec's tags. `space` switches an endpoint on or off, or a whole tag; `/` filters the list.
- `enter` on an endpoint goes to its parameters: path, query, header and cookie parameters with their type, description and the values the spec allows, then the body and the endpoint's share of the traffic. `enter` edits one, `space` steps through the spec's values, `del` leaves it out. Several values (`a, b`) are sent in turn, or together for array parameters.
- The request is shown as it will go out, and `t` sends it once and shows the answer.
- If the spec declares authentication and none was given, pepe asks for it, checks it with one request, and shows credentials masked from then on.
- `c`, `n`, `z` change concurrency, requests and duration; `u` the server; `a` the credentials; `g` starts the run.
- Endpoints that still need a value, and writes (POST, PUT, PATCH, DELETE), are never switched on in bulk: you switch those on one by one.
- `?` lists the keys; `esc` goes back a level, then quits.

## The run

The run is the usual dashboard, with an **Endpoints** view in front: requests, throughput, p50, p99, errors and status codes for each endpoint. `enter` on one shows its requests, and `e` goes back to the plan. Bodies for writes come from the spec's examples and schemas; parameters rotate through the values given.

## Flags

Everything on the plan screen has a flag, so a run can be scripted:

| Flag | What it does |
| --- | --- |
| `--auth` | Credentials: `bearer:TOKEN`, `basic:USER:PASSWORD`, `apikey:VALUE`, `header:NAME=VALUE` or `query:NAME=VALUE`; repeat for several |
| `--server` | Send requests here instead of the spec's server |
| `--all` | Switch on every endpoint that has the values it needs |
| `--tag`, `--only` | Switch on the endpoints with this tag, or matching a pattern such as `'GET /pets*'` or `'/pets/*'` |
| `--skip` | Leave out endpoints matching a pattern |
| `--set` | Give a parameter its value(s) wherever it appears: `--set id=1,2,3` |
| `--include-writes` | Let `--all`, `--tag` and `--only` switch on writes too |

With `--json`, or piped, there is no screen, so name what to run with `--all`, `--tag` or `--only`; the report has a section per endpoint (`endpoints[]`, with requests, failures, throughput, median, p99 and status codes each). The `[api]` table of `pepe.toml` gives the same defaults ([Config file](load-test.md#config-file)). `--metrics` serves a row per endpoint ([Output](output.md#prometheus-metrics)).
