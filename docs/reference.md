# Command reference

Every command and flag, as `--help` prints them. Generated from the command definition by `cargo test`, so it can't drift from the binary; the pages before this one say what the flags are for.

The options under `pepe` are global: they work after any subcommand too (`pepe ping -H 'Accept: text/html' …`), where they make sense.

## pepe

```text
HTTP load generator

Usage: pepe [OPTIONS] [URL] [-- <ARGS>...] [COMMAND]

Commands:
  self-update  Update pepe to the latest release, or say what's new in it
  completions  Tab completion for your shell: print the script, or --install it
  api          Load-test every endpoint of an OpenAPI spec
  ramp         Raise the load step by step to find where the target stops keeping up
  replay       Send the URLs of an access log in their real proportions
  flow         Run a sequence of requests from a flow file, each step fed by the last
  logs         Read nginx logs: requests per second now, against each minute, hour and day
  ping         Ping a URL: a request a second, each split into DNS, connect, TLS, first byte and download, on a graph
  compare      Hold a run's JSON report against an earlier one and say what moved
  schema       Print the JSON Schema of a report: run (the default), ramp, ping or compare
  help         Print this message or the help of the given subcommand(s)

Arguments:
  [URL]
          HTTP url to request
          
          [default: ""]

  [ARGS]...
          List of arguments to pass to curl command
          
          [default: ""]

Options:
  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

      --curl
          Load-test a curl command: pepe --curl -- curl -X POST http://localhost:8080, or pass it as one quoted string, as @file, or on stdin

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit

  -V, --version
          Print version
```

## pepe ping

```text
Ping a URL: a request a second, each split into DNS, connect, TLS, first byte and download, on a graph

Usage: pepe ping [OPTIONS] [TARGETS]...

Arguments:
  [TARGETS]...
          What to ping: URLs or hosts (https unless a port is given), host:port, aws:REGION, a range like 10.0.0.0/29, or commands with --cmd; several are graphed together

Options:
      --every <TIME>
          Time between pings, e.g. 1s, 500ms, 2m
          
          [default: 1s]

      --window <TIME>
          How much of the run the graph shows, e.g. 60s, 5m; + and - change it on screen, w shows the whole run
          
          [default: 60s]

      --name <NAME>
          What to call each target, in order: --name api --name cdn, or --name api,cdn

      --color <COLOR>
          A colour for each target's line, in order: red, green, yellow, blue, magenta, cyan, white, gray, their light- forms, or #RRGGBB

  -4
          Resolve names to IPv4 addresses only

  -6
          Resolve names to IPv6 addresses only

      --all-ips
          Ping every address a name resolves to, each as a target of its own

      --interface <NAME|IP>
          Send from this interface (en0, eth0) or local address

      --tcp
          Only connect: a TCP ping of the port, with no request sent

      --port <PORT>
          With --tcp: the port of a target that names none
          
          [default: 80]

  -h, --help
          Print help

      --tcp-rst <TCP_RST>
          With --tcp: a connection refused counts as an answer (the host is there) rather than a failure

          Possible values:
          - pong: An answer: something is there to refuse
          - fail: A failure
          
          [default: pong]

      --cmd
          The targets are commands: run each one every interval and graph how long it takes; its exit code is the status

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

      --keep-alive
          Keep the connection between pings, as a browser would. The DNS, connect and TLS phases are then measured once; without it every ping measures all five

      --slo <KEY=MS,...>
          Limits a ping must meet, in milliseconds: total=500,ttfb=200, connect=100,dns=50,tls=150,download=100. Breaking one marks the ping, and the run exits 4 at the end

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

      --bell
          Ring the terminal bell when a ping fails or breaks the SLO

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

      --ymin <MS>
          The graph's floor, in milliseconds

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

      --ymax <MS>
          The graph's ceiling, in milliseconds; without it the graph fits what it shows

  -0
          Start the graph at zero (the same as --ymin 0)

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

  -s, --simple-graphics
          Draw the graph with dots rather than braille, for terminals and fonts that lack it

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --jsonl
          No screen: one JSON object per ping on stdout, as it happens

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --csv
          No screen: one CSV line per ping on stdout, under a header

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

      --save <FILE>
          Write the JSON report to this file when the run ends, whatever else is shown

      --show-body
          Keep the first kilobyte of each body for the inspector

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

      --save-body <FILE>
          Write the last body received to this file when the run ends

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --once
          One diagnosis and out: three quick pings, then the last one's phases, what answered, and what to look at; no screen

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```

## pepe ramp

```text
Raise the load step by step to find where the target stops keeping up

Usage: pepe ramp [OPTIONS] [URL]

Arguments:
  [URL]
          HTTP url to request; without one, the setup screen opens

Options:
      --from <FROM>
          Concurrency of the first step
          
          [default: 10]

      --to <TO>
          Concurrency of the last step
          
          [default: 100]

      --step <STEP>
          Concurrency added at each step
          
          [default: 10]

      --every <EVERY>
          How long each step is held, e.g. 10s, 1m
          
          [default: 10s]

      --until <CONDITION>
          End the ramp once a step crosses this: 'p99 > 500ms', 'errors > 1%'

  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```

## pepe api

```text
Load-test every endpoint of an OpenAPI spec

Usage: pepe api [OPTIONS] [SPEC]

Arguments:
  [SPEC]
          The OpenAPI spec: a file, a URL or the document itself, JSON or YAML (or `spec` in pepe.toml). Without it, the setup screen asks

Options:
      --auth <AUTH>
          Credentials: bearer:TOKEN, basic:USER:PASSWORD, apikey:VALUE, header:NAME=VALUE or query:NAME=VALUE

      --server <SERVER>
          Base URL to send requests to, instead of the spec's server

      --all
          Run every endpoint that has the values it needs. Without --all, --tag or --only, nothing runs until it's picked on the plan screen

      --tag <TAG>
          Run the endpoints with this tag, e.g. --tag Billing

      --only <ONLY>
          Run the endpoints matching this, e.g. 'GET /pets*' or '/pets/*'

      --skip <SKIP>
          Leave out endpoints matching this

      --set <NAME=VALUE[,VALUE]>
          A parameter's value(s), rotated through: --set id=1,2,3

      --include-writes
          Let --all, --tag and --only switch on POST, PUT, PATCH and DELETE too

  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```

## pepe replay

```text
Send the URLs of an access log in their real proportions

Usage: pepe replay [OPTIONS] <LOG>

Arguments:
  <LOG>
          The log: nginx or Apache (common or combined), Caddy JSON, AWS ALB, or one URL or path per line

Options:
      --base-url <URL>
          Where to send the requests: put in front of paths, and in place of the host of full URLs, e.g. https://staging.example.com

      --include-writes
          Replay POST, PUT, PATCH and DELETE too; only GET, HEAD and OPTIONS without it

      --rows <N>
          URLs that get a row of their own on the dashboard, most frequent first; the rest share one
          
          [default: 20]

  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```

## pepe flow

```text
Run a sequence of requests from a flow file, each step fed by the last

Usage: pepe flow [OPTIONS] <FILE>

Arguments:
  <FILE>
          The flow: a TOML file of [[step]]s (see the README)

Options:
  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```

## pepe logs

```text
Read nginx logs: requests per second now, against each minute, hour and day

Usage: pepe logs [OPTIONS] [FILES]...

Arguments:
  [FILES]...
          Access and error logs, rotated ones too; `-` or nothing reads what is piped in, and with nothing at all nginx's own in /var/log/nginx

Options:
      --format <LOG_FORMAT>
          The access log's log_format as nginx.conf has it, on one line, when it isn't `combined`: '$remote_addr [$time_local] "$request" $status $request_time'

      --window <TIME>
          What "now" is measured over, e.g. 10s, 1m, 5m
          
          [default: 60s]

      --since <TIME>
          How far back to start, e.g. 90m, 24h, 7d. At a terminal a log that is being written is shown live, from five minutes back, without it

      --all
          Read all of the log before following it, however far back it goes

      --exact-paths
          Count /items/1 and /items/2 apart. Without it the numbers and ids in a path count as one, /items/*

      --rows <N>
          Rows in each table of the report
          
          [default: 10]

  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```

## pepe compare

```text
Hold a run's JSON report against an earlier one and say what moved

Usage: pepe compare [OPTIONS] <BEFORE> <AFTER>

Arguments:
  <BEFORE>
          The earlier report (`pepe --json`, `--snapshot`, or a ramp's)

  <AFTER>
          The later one, of the same test

Options:
      --gate
          Exit 1 when the verdict is Slower or Worse, for CI

      --svg <FILE>
          Also write the verdict as a card, an SVG for a README or a page

  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```

## pepe schema

```text
Print the JSON Schema of a report: run (the default), ramp, ping or compare

Usage: pepe schema [OPTIONS] [REPORT]

Arguments:
  [REPORT]
          Which report: run, ramp, ping or compare
          
          [default: run]

Options:
  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```

## pepe self-update

```text
Update pepe to the latest release, or say what's new in it

Usage: pepe self-update [OPTIONS]

Options:
      --check
          Only say whether a newer release exists (exit code 1 if so) and what's in it; don't install it

      --verbose
          Show the installer's own output

  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```

## pepe completions

```text
Tab completion for your shell: print the script, or --install it

Usage: pepe completions [OPTIONS] [SHELL]

Arguments:
  [SHELL]
          bash, zsh, fish or powershell; the current shell when left out
          
          [possible values: bash, zsh, fish, powershell]

Options:
      --install
          Put the script and the man pages in place and add the line the shell's startup file needs, so tab completion works in the next shell

      --dry-run
          With --install: say what would be done, and do nothing

  -h, --help
          Print help

  -n, --number <NUMBER>
          Number of requests to perform
          
          [default: 100]

  -c, --concurrency <CONCURRENCY>
          Number of concurrent requests at a time
          
          [default: "one per core"]

  -z, --duration <DURATION>
          Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)

  -m, --method <METHOD>
          HTTP method, e.g. GET, POST, PUT, DELETE
          
          [default: GET]

  -H, --headers <HEADERS>
          HTTP headers, e.g. -H 'Accept: application/json'

  -t, --timeout <TIMEOUT>
          Time in seconds to wait for a response
          
          [default: 20]

      --warmup <TIME>
          Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run

      --threads <N|auto>
          Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit

      --rate <PER_SECOND>
          Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back

  -d, --body <BODY>
          HTTP request body

  -u, --user-agent <USER_AGENT>
          User-Agent string, default is pepe/{version}

  -p, --proxy <PROXY>
          Proxy server URL: http://user:pass@host:port or socks5://host:port

  -k, --insecure
          Accept invalid TLS certificates (self-signed, expired, wrong host)

      --disable-compression
          Disable HTTP compression, e.g. gzip

      --disable-keepalive
          Disable HTTP keepalive, e.g. Connection: close

      --disable-redirects
          Prevent http redirects

      --json
          Output results in JSON format

      --trace-header <NAME>
          Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server's logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for

      --snapshot <FILE>
          Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run's numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run

      --fail-if <CONDITION>
          End with exit code 4 when the run crosses this, for CI and scripts: 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)

      --metrics <ADDR>
          Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server's own

  -i, --setup
          Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)

      --config <FILE>
          Read settings from this file instead of ./pepe.toml; flags on the command line win over it

      --write-config <FILE>
          Write the settings as they stand to this file, as a pepe.toml, and exit
```
