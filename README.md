# Pepe - HTTP Load Generator

Pepe is a command-line HTTP load generator designed to test the performance and reliability of web servers. It allows you to send a large number of HTTP requests to a specified URL and measure various performance metrics such as response times, throughput, and error rates.

Pepe is written in Rust and uses the `reqwest` and `tokio` libraries for making HTTP requests asynchronously. It supports sending multiple requests concurrently, custom headers, request bodies, timeouts, basic authentication, and proxy servers.

![Pepe](assets/pepe.gif)

## Features

- **Concurrency**: Send multiple requests concurrently to simulate real-world load.
- **Custom Headers**: Add custom headers to the requests.
- **Request Body**: Send data in the request body from a string or a file.
- **Timeouts**: Set a timeout for each request.
- **Basic Authentication**: Use basic authentication for the requests.
- **Proxy Support**: Send requests through a proxy server.
- **Setup Screen**: Run `pepe` with no arguments to fill in every option on a form, with the equivalent command shown as you go.
- **Ramp Mode**: Raise the load step by step and find the concurrency where the target stops keeping up.
- **API Mode**: Load-test the endpoints of an OpenAPI spec, picked by tag, with their parameters and credentials set on screen.
- **Live Dashboard**: Throughput and latency charts, status codes, percentiles and a scrollable request log, updated as the test runs.
- **Interactive Control**: Pause, resume or stop a run, and raise or lower concurrency while it's going.
- **DNS Resolution Timing**: Sample DNS lookup time once a second during the run.
- **Detailed Statistics**: Measure and display various performance metrics such as min, max, average, median, percentiles, standard deviation, total data transferred, and error rate.
- **Light**: One thread sends about 100k requests a second at under half the CPU of comparable tools and a fraction of their memory, and pepe tells you when it, rather than the target, is the limit; see [bench/README.md](bench/README.md) for the measurements.

## Installation

### macOS and Linux (installer script)

```bash
curl -LsSf https://pepe.mhaimdat.com/install.sh | sh
```

### Windows (PowerShell)

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://pepe.mhaimdat.com/install.ps1 | iex"
```

Installer-based installs can update themselves:

```bash
pepe self-update
```

### Homebrew (macOS and Linux)

```bash
brew install omarmhaimdat/pepe/pepe
```

### Nix

```bash
nix run github:omarmhaimdat/pepe -- https://example.com   # try without installing
nix profile install github:omarmhaimdat/pepe              # install
```

### Prebuilt binaries

Every [release](https://github.com/omarmhaimdat/pepe/releases) ships binaries for macOS (Apple Silicon and Intel), Linux (x86_64 and ARM64, statically linked) and Windows (x86_64), with SHA-256 checksums and signed build provenance:

```bash
gh attestation verify pepe-x86_64-unknown-linux-musl.tar.xz --repo omarmhaimdat/pepe
```

### From source

```bash
cargo install --locked --git https://github.com/omarmhaimdat/pepe
```

pepe checks for a newer release when it exits. Set `PEPE_NO_UPDATE_CHECK=1` to disable this.

## Releasing

Releases are automated. Merging to `master` with [conventional commits](https://www.conventionalcommits.org/) (`feat:`, `fix:`, ...) keeps a **release PR** open that bumps the version and updates `CHANGELOG.md`. Merging that PR tags `vX.Y.Z`, which builds every platform and publishes the GitHub Release, installers, the Homebrew formula and the pepe.mhaimdat.com mirror.

## Usage

### Basic Usage

To send a simple GET request to a URL, use the following command:

```bash
pepe https://example.com
```

### Advanced Usage

```bash
pepe -n 1000 -c 20 -t 10 -u "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_11_2) AppleWebKit/601.3.9 (KHTML, like Gecko) Version/9.0.2 Safari/601.3.9" -H "Accept: application/json" -H "Content-Type: application/json" -m GET https://example.com
```

Let's break down the options used in this command:

- `-n 1000`: Send a total of 1000 requests.
- `-c 20`: Use 20 concurrent connections.
- `-t 10`: Set a timeout of 10 seconds for each request.
- `-u "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_11_2) AppleWebKit/601.3.9 (KHTML, like Gecko) Version/9.0.2 Safari/601.3.9"`: Set the User-Agent header to simulate a Safari browser on a Mac.
- `-H "Accept: application/json"`: Add a custom Accept header to the requests.
- `-H "Content-Type: application/json"`: Add a custom Content-Type header to the requests.
- `-m GET`: Use the GET HTTP method.
- `https://example.com`: The URL to send requests to.

### Threads

pepe sends from one thread, whatever the concurrency. One thread sends about
100k plain requests a second, or 10k TLS handshakes a second, at the lowest
CPU per request, and that is more than most targets can take. When it isn't,
pepe says so: the dashboard's footer shows how busy the sending thread is
once it passes 90% of a core, the end-of-run verdict notes it, and the JSON
report has it under `generator`. Then `--threads` adds more:

```bash
pepe -c 500 --threads 4 -z 30s http://localhost:8080/
```


### Curl support

Load-test any curl command, including ones copied from a browser's dev tools ("Copy as cURL"), Postman or Insomnia. pepe sends the same request curl would: same method, URL, headers and body.

```bash
pepe -n 1000 -c 10 --curl -- curl -X POST 'https://httpbin.org/post' \
  -H 'Content-Type: application/json' \
  -d '{"key": "value"}'
```

Put `--` between pepe's options and the curl command. The command can also be given as one quoted string, read from a file, or piped in:

```bash
pepe -z 30s --curl -- @request.txt
pbpaste | pepe -z 30s --curl
```

What's understood:

- Quoting: single and double quotes, backslash escapes, line continuations, bash `$'...'` (Chrome's "Copy as cURL (bash)") and Windows `^` escaping ("Copy as cURL (cmd)").
- Methods: `-X`, and the ones curl implies: POST for data and forms, PUT for `-T`, HEAD for `-I`, GET for `-G`.
- Bodies: `-d`/`--data`, `--data-raw`, `--data-binary`, `--data-urlencode`, `--json`, `@file` for any of them, `-F`/`--form` multipart (with file uploads), `-T` uploads, and `-G` to move data into the query string.
- Headers: `-H` (including `-H @file`, `-H 'Name;'` for an empty value, `-H 'Name:'` to drop one), `-u` basic auth, `--oauth2-bearer`, `-b` cookies, `-A`, `-e`, `-r`, `--compressed`.
- Connection: `-L` (like curl, redirects are only followed with `-L`), `-k`, `-x`, `-m`, `--no-keepalive`, `--url`, `--url-query`, bunched flags like `-sSLk` and attached values like `-XPOST`.
- Output, logging and TLS options (`-o`, `-s`, `-v`, `-w`, `--cacert`, ...) are accepted and have no effect. An unknown option is an error, and anything pepe can't reproduce (such as a cookie file) is reported as a note.

### Setup screen

Run `pepe` with no URL and it opens a form with every option as a field: URL, method, headers, body, load, timeout, redirects, keep-alive, TLS, proxy and user agent. Add `-i` to any command to open the form filled in from its flags.

```bash
pepe
pepe -i -c 50 -z 30s https://example.com
```

- `tab` switches mode: **Single URL**, **Ramp** or **API**. What the modes share is kept.
- `↑` `↓` move between fields, `←` `→` change a choice, `enter` starts.
- Paste a curl command anywhere and the form is filled in from it.
- `ctrl-t` sends the request once and shows the response, to check it before the run.
- The command card always shows the flags that reproduce the form. It's printed to your shell when you quit, and `E` in the dashboard brings you back to the form.

### Ramp: finding where the target stops keeping up

`pepe ramp` raises concurrency step by step, measures each step on its own, and says where the target holds, where it stops scaling and where it breaks.

```bash
pepe ramp https://example.com --from 10 --to 200 --step 10 --every 15s
pepe ramp https://example.com --until 'p99 > 500ms' --until 'errors > 1%'
```

- `--from`, `--to`, `--step`: the concurrency of the first step, the last, and what's added between (10, 100 and 10 by default).
- `--every`: how long each step is held (`10s` by default).
- `--until`: ends the ramp once a step crosses a limit, so a failing target isn't hammered further. Latency percentiles (`p50 > 100ms`, `p99 > 2s`) and `errors > 1%` are understood; give it more than once for several limits.
- `-m`, `-d`, `-H` and the other request options work as they do without `ramp`. Without a URL, the setup screen opens in Ramp mode.

The screen shows each step as a row (throughput, p50, p90, p99, the slowest request, errors) with a note when something changes, the run second by second, throughput and p99 at each concurrency, and the result: the level that held, where throughput stopped following the load, where it broke, and the command for a steady run at the level that held. Throughput counts successful responses only, so a target that sheds load quickly doesn't look fast.

| Key | Action |
| --- | --- |
| `↑` `↓` | Pick a step and see everything measured about it; `esc` goes back to following the run |
| `space` | Pause or resume; a step's clock stops while paused |
| `n` | End this step now and go on to the next |
| `s` | Stop the ramp here and keep the results |
| `r` / `e` | Run again / back to the setup screen |
| `q` / `Ctrl-C` | Quit; the table and the result are printed to your shell |

`--json` runs the ramp without a screen and prints every step and the findings.

### API mode: load-testing an OpenAPI spec

`pepe api` reads an OpenAPI 3 (or Swagger 2) spec, from a file or a URL, in JSON or YAML, and turns its operations into requests.

```bash
pepe api openapi.yaml
pepe api https://api.example.com/openapi.json --auth bearer:$TOKEN -c 20 -z 1m
```

It opens on a plan screen. Nothing is sent, and no endpoint is switched on, until you say so.

- Endpoints are listed under the spec's tags. `space` switches an endpoint on or off, or a whole tag; `/` filters the list.
- `enter` on an endpoint goes to its parameters: path, query, header and cookie parameters with their type, description and the values the spec allows, then the body and the endpoint's share of the traffic. `enter` edits one, `space` steps through the spec's values, `del` leaves it out. Several values (`a, b`) are sent in turn, or together for array parameters.
- The request is shown as it will go out, and `t` sends it once and shows the answer.
- If the spec declares authentication and none was given, pepe asks for it, checks it with one request, and shows credentials masked from then on.
- `c`, `n`, `z` change concurrency, requests and duration; `u` the server; `a` the credentials; `g` starts the run.
- Endpoints that still need a value, and writes (POST, PUT, PATCH, DELETE), are never switched on in bulk: you switch those on one by one.

The run is the usual dashboard, with an **Endpoints** view in front: requests, throughput, p50, p99, errors and status codes for each endpoint. `enter` on one shows its requests, and `E` goes back to the plan.

Everything on the plan screen has a flag:

| Flag | What it does |
| --- | --- |
| `--auth` | Credentials: `bearer:TOKEN`, `basic:USER:PASSWORD`, `apikey:VALUE`, `header:NAME=VALUE` or `query:NAME=VALUE` |
| `--server` | Send requests here instead of the spec's server |
| `--all` | Switch on every endpoint that has the values it needs |
| `--tag`, `--only` | Switch on the endpoints with this tag, or matching a pattern such as `'GET /pets*'` |
| `--skip` | Leave out endpoints matching a pattern |
| `--set` | Give a parameter its value(s) wherever it appears: `--set id=1,2,3` |
| `--include-writes` | Let `--all`, `--tag` and `--only` switch on writes too |

With `--json` there is no screen, so name what to run with `--all`, `--tag` or `--only`; the report has a section per endpoint.

## Examples

### Sending a GET Request

```bash
pepe -n 1000 -c 10 -m GET https://example.com
```

This command sends 1000 GET requests to `https://example.com` with a concurrency of 10 requests at a time, -m GET specifies the HTTP method to use.

### Sending a POST Request

Send a POST request with a request body as raw text:

```bash
pepe -n 1000 -c 10 -m POST -d 'Hello, World!' https://httpbin.org/post
```

Send a POST request with a request body in json format:

```bash
pepe -n 1000 -c 10 -m POST -d '{"key": "value"}' -H 'Content-Type: application/json' https://httpbin.org/post
```

### Sending Requests with Custom Headers

```bash
pepe -n 100 -c 5 -H "User-Agent: Pepe/1.0" -H "X-Custom-Header: Value" https://example.com
```

### Running for a Fixed Time

Keep sending requests for a duration instead of a fixed count (`s`, `m` or `h`):

```bash
pepe -z 30s -c 20 https://example.com
```

### JSON Output for Scripts and CI

`--json` skips the dashboard, runs to completion and prints a JSON summary to stdout. Press Ctrl-C to stop early; the report then has `"interrupted": true`.

```bash
pepe --json -n 1000 -c 20 https://example.com > results.json
jq '.summary.latency.p99_ms' results.json
```

### Proxy Support

Send requests through a proxy server (HTTP or HTTPS, SOCKS5):

Without authentication:

```bash
pepe -n 1000 -c 10 -p http://proxy:port https://example.com
```


With authentication:

```bash
pepe -n 1000 -c 10 -p socks5://username:password@proxy:port https://example.com
```

## Output

Pepe provides detailed statistics about the performance of the web server, including:

- **Min Response Time**: The minimum response time observed.
- **Max Response Time**: The maximum response time observed.
- **Average Response Time**: The average response time.
- **Median Response Time**: The median response time.
- **90th Percentile**: The 90th percentile response time.
- **95th Percentile**: The 95th percentile response time.
- **99th Percentile**: The 99th percentile response time.
- **Standard Deviation**: The standard deviation of the response times.
- **Total Data Transferred**: The total amount of data transferred.
- **Error Rate**: The percentage of requests that did not get a 2xx response.
- **Failures by kind**: non-2xx responses, connection errors and timeouts are counted separately.
- **Cache Hit Rate**: The percentage of requests that were served from the cache.
- **Requests Per Second (RPS)**: The number of requests per second.
- **DNS Lookup Time**: The average time taken to resolve the host, sampled once a second.

### Dashboard

The dashboard has three views:

- **Live**: the headline numbers, a latency heatmap (time across, latency up, brighter cells mean more requests took that long) with p50 and p99 marked, throughput per second, and a panel with the detailed numbers.
- **Stats**: every number pepe collects, the test setup, and the latency distribution.
- **Requests**: the last 2,000 requests (and older failures), filterable by status, latency and text. Press `enter` on one to inspect it: status, total time split into time to first byte and body download, how it ranks in the run, DNS, server address, protocol, cache status, the request as sent, and the full response headers and body, with JSON, HTML and XML indented and highlighted. Walk to the next request with `←`/`→`. Up to 1,000 responses a second are kept in full, an even sample above that (marked `●`, reached with `[`/`]`).

Pepe, the chili in the corner, reacts to how the run is going. When a run ends, the header turns into a verdict (Healthy, Degraded or Failing) with findings such as failed requests, two separate latency groups, a long tail, or throughput and latency drifting over the run. The same summary is printed to your shell when you quit.

| Key | Action |
| --- | --- |
| `space` / `p` | Pause or resume sending; a timed run's clock stops while paused |
| `+` / `-` | Raise or lower concurrency by about 10%, live |
| `s` / `i` | Stop sending and keep the results on screen |
| `r` | Restart with the same settings (and the current concurrency) |
| `E` | Back to the setup screen (or, in API mode, the plan), to change the settings and run again |
| `tab` / `←` `→` / `1` `2` `3` | Switch view |
| `↑` `↓` / `j` `k`, `PgUp` `PgDn`, `g` `G` | Select a request in the log (newer / older) |
| `f` | Filter requests by status: 2xx, 3xx, 4xx, 5xx, no response, failed |
| `l` | Filter requests by latency: at or above p50, p90 or p99 |
| `/` | Search the status and response text |
| `e` / `c` | Show only failed requests / clear all filters |
| `enter` | Inspect the selected request: `↑`/`↓` (or the trackpad), `u`/`d` and `g`/`G` scroll the response, `←`/`→` walk to the newer/older request, `[`/`]` jump to the nearest one kept in full, `v` switches between formatted and raw, `esc` goes back |
| `?` | Show all keys |
| `q` / `esc` / `Ctrl-C` | Quit |


## Resource usage

A load generator should not be the bottleneck of its own test. pepe's CPU and
memory per request are measured against oha and vegeta on fixed workloads
with the scripts in [`bench/`](bench/); the method, every result and what
was changed to get there are in [bench/README.md](bench/README.md).

## Contributing

Contributions are welcome! Please open an issue or submit a pull request on GitHub.

## License

This project is licensed under the MIT License. See the [LICENSE](LICENSE) file for details.

## Acknowledgements

- [Clap](https://github.com/clap-rs/clap) for command-line argument parsing.
- [Reqwest](https://github.com/seanmonstar/reqwest) for HTTP requests.
- [Tokio](https://github.com/tokio-rs/tokio) for asynchronous runtime.
- [Crossterm](https://github.com/crossterm-rs/crossterm) for terminal handling.
- [Ratatui](https://github.com/ratatui/ratatui) for TUI components.


## Roadmap

- [x] Implement basic functionality for sending HTTP requests.
- [x] Improve support for curl when the protocol is not specified.
- [ ] Implement a config file for managing load test settings.
- [ ] Create an output format for easy integration with monitoring tools.
  - [x] JSON output.
  - [ ] CSV output.
  - [ ] Webhook integration.
- [ ] Chaining multiple requests.
- [ ] Implement more advanced load testing scenarios.
