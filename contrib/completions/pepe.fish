# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_pepe_global_optspecs
    string join \n h/help n/number= z/duration= curl m/method= H/headers= t/timeout= warmup= threads= rate= d/body= p/proxy= k/insecure disable-compression disable-keepalive disable-redirects json trace-header= vars= webhook= webhook-on= slack= export= snapshot= allow-host= max-requests= max-rate= max-concurrency= dry-run fail-if= metrics= otlp= i/setup config= write-config= load= keep= record c/concurrency= u/user-agent= V/version
end

function __fish_pepe_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_pepe_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_pepe_using_subcommand
    set -l cmd (__fish_pepe_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c pepe -n "__fish_pepe_needs_command" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_needs_command" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_needs_command" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_needs_command" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_needs_command" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_needs_command" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_needs_command" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_needs_command" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_needs_command" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_needs_command" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_needs_command" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_needs_command" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_needs_command" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_needs_command" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_needs_command" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_needs_command" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_needs_command" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_needs_command" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_needs_command" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_needs_command" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_needs_command" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_needs_command" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_needs_command" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_needs_command" -l otlp -d 'Take the service\'s OpenTelemetry spans at ADDR (OTLP over HTTP, as :4318) and say where its time went: each request carries its own traceparent, and the report breaks the server\'s time down by span. Point the service at it: OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318' -r
complete -c pepe -n "__fish_pepe_needs_command" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_needs_command" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_needs_command" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_needs_command" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_needs_command" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_needs_command" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_needs_command" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_needs_command" -l curl -d 'Load-test a curl command: pepe --curl -- curl -X POST http://localhost:8080, or pass it as one quoted string, as @file, or on stdin'
complete -c pepe -n "__fish_pepe_needs_command" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_needs_command" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_needs_command" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_needs_command" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_needs_command" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_needs_command" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_needs_command" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_needs_command" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_needs_command" -s V -l version -d 'Print version'
complete -c pepe -n "__fish_pepe_needs_command" -a "ping" -d 'Ping a URL: a request a second, each split into DNS, connect, TLS, first byte and download, on a graph'
complete -c pepe -n "__fish_pepe_needs_command" -a "ramp" -d 'Raise the load step by step to find where the target stops keeping up'
complete -c pepe -n "__fish_pepe_needs_command" -a "compare" -d 'Hold a run\'s JSON report against an earlier one and say what moved'
complete -c pepe -n "__fish_pepe_needs_command" -a "rerun" -d 'Run a saved test again, record the run, and say what moved since the run before'
complete -c pepe -n "__fish_pepe_needs_command" -a "history" -d 'The saved tests and their recorded runs'
complete -c pepe -n "__fish_pepe_needs_command" -a "api" -d 'Load-test every endpoint of an OpenAPI spec'
complete -c pepe -n "__fish_pepe_needs_command" -a "flow" -d 'Run a sequence of requests from a flow file, each step fed by the last'
complete -c pepe -n "__fish_pepe_needs_command" -a "replay" -d 'Send the URLs of an access log in their real proportions'
complete -c pepe -n "__fish_pepe_needs_command" -a "logs" -d 'Read nginx logs: requests per second now, against each minute, hour and day'
complete -c pepe -n "__fish_pepe_needs_command" -a "schema" -d 'Print the JSON Schema of a report: run (the default), ramp, ping or compare'
complete -c pepe -n "__fish_pepe_needs_command" -a "mcp" -d 'Serve pepe\'s modes as tools for an agent over stdio (the Model Context Protocol); the guardrail flags given here hold for every call'
complete -c pepe -n "__fish_pepe_needs_command" -a "self-update" -d 'Update pepe to the latest release, or say what\'s new in it'
complete -c pepe -n "__fish_pepe_needs_command" -a "completions" -d 'Tab completion for your shell: print the script, or --install it'
complete -c pepe -n "__fish_pepe_needs_command" -a "slack" -d 'Connect pepe to Slack from here, so --slack keeps a message there up to date as a run goes'
complete -c pepe -n "__fish_pepe_needs_command" -a "export" -d 'Turn a JSON report into a page, a Markdown summary, text or CSV'
complete -c pepe -n "__fish_pepe_needs_command" -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l every -d 'Time between pings, e.g. 1s, 500ms, 2m' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l window -d 'How much of the run the graph shows, e.g. 60s, 5m; + and - change it on screen, w shows the whole run' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l name -d 'What to call each target, in order: --name api --name cdn, or --name api,cdn' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l color -d 'A colour for each target\'s line, in order: red, green, yellow, blue, magenta, cyan, white, gray, their light- forms, or #RRGGBB' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l interface -d 'Send from this interface (en0, eth0) or local address' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l port -d 'With --tcp: the port of a target that names none' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l tcp-rst -d 'With --tcp: a connection refused counts as an answer (the host is there) rather than a failure' -r -f -a "pong\t'An answer: something is there to refuse'
fail\t'A failure'"
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l slo -d 'Limits a ping must meet, in milliseconds: total=500,ttfb=200, connect=100,dns=50,tls=150,download=100. Breaking one marks the ping, and the run exits 4 at the end' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l ymin -d 'The graph\'s floor, in milliseconds' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l ymax -d 'The graph\'s ceiling, in milliseconds; without it the graph fits what it shows' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l save -d 'Write the JSON report to this file when the run ends, whatever else is shown' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l save-body -d 'Write the last body received to this file when the run ends' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s 4 -d 'Resolve names to IPv4 addresses only'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s 6 -d 'Resolve names to IPv6 addresses only'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l all-ips -d 'Ping every address a name resolves to, each as a target of its own'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l tcp -d 'Only connect: a TCP ping of the port, with no request sent'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l cmd -d 'The targets are commands: run each one every interval and graph how long it takes; its exit code is the status'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l keep-alive -d 'Keep the connection between pings, as a browser would. The DNS, connect and TLS phases are then measured once; without it every ping measures all five'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l bell -d 'Ring the terminal bell when a ping fails or breaks the SLO'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s 0 -d 'Start the graph at zero (the same as --ymin 0)'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s s -l simple-graphics -d 'Draw the graph with dots rather than braille, for terminals and fonts that lack it'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l jsonl -d 'No screen: one JSON object per ping on stdout, as it happens'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l csv -d 'No screen: one CSV line per ping on stdout, under a header'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l show-body -d 'Keep the first kilobyte of each body for the inspector'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l http1 -d 'Speak HTTP/1.1 even when the server offers HTTP/2; without it a ping takes h2 when the server offers it through ALPN'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l once -d 'One diagnosis and out: three quick pings, then the last one\'s phases, what answered, and what to look at; no screen'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l from -d 'Concurrency of the first step' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l to -d 'Concurrency of the last step' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l step -d 'Concurrency added at each step' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l every -d 'How long each step is held, e.g. 10s, 1m' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l until -d 'End the ramp once a step crosses this: \'p99 > 500ms\', \'errors > 1%\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l svg -d 'Also write the verdict as a card, an SVG for a README or a page' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l gate -d 'Exit 1 when the verdict is Slower or Worse, for CI'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l gate -d 'Exit 1 when the verdict is Slower or Worse, for CI'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand rerun" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand history" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand history" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand history" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand history" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand history" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand history" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand history" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand history" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand history" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand history" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand history" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand history" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand history" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand history" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand history" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand api" -l auth -d 'Credentials: bearer:TOKEN, basic:USER:PASSWORD, apikey:VALUE, header:NAME=VALUE or query:NAME=VALUE' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l server -d 'Base URL to send requests to, instead of the spec\'s server' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l tag -d 'Run the endpoints with this tag, e.g. --tag Billing' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l only -d 'Run the endpoints matching this, e.g. \'GET /pets*\' or \'/pets/*\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l skip -d 'Leave out endpoints matching this' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l set -d 'A parameter\'s value(s), rotated through: --set id=1,2,3' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand api" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand api" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand api" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand api" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand api" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l all -d 'Run every endpoint that has the values it needs. Without --all, --tag or --only, nothing runs until it\'s picked on the plan screen'
complete -c pepe -n "__fish_pepe_using_subcommand api" -l include-writes -d 'Let --all, --tag and --only switch on POST, PUT, PATCH and DELETE too'
complete -c pepe -n "__fish_pepe_using_subcommand api" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand api" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand api" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand api" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand api" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand api" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand api" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand api" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand api" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l base-url -d 'Where to send the requests: put in front of paths, and in place of the host of full URLs, e.g. https://staging.example.com' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l rows -d 'URLs that get a row of their own on the dashboard, most frequent first; the rest share one' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l include-writes -d 'Replay POST, PUT, PATCH and DELETE too; only GET, HEAD and OPTIONS without it'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l format -d 'The access log\'s log_format as nginx.conf has it, on one line, when it isn\'t `combined`: \'$remote_addr [$time_local] "$request" $status $request_time\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l window -d 'What "now" is measured over, e.g. 10s, 1m, 5m' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l since -d 'How far back to start, e.g. 90m, 24h, 7d. At a terminal a log that is being written is shown live, from five minutes back, without it' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l rows -d 'Rows in each table of the report' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l all -d 'Read all of the log before following it, however far back it goes'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l exact-paths -d 'Count /items/1 and /items/2 apart. Without it the numbers and ids in a path count as one, /items/*'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand schema" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand schema" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand mcp" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l check -d 'Only say whether a newer release exists (exit code 1 if so) and what\'s in it; don\'t install it'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l verbose -d 'Show the installer\'s own output'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l install -d 'Put the script and the man pages in place and add the line the shell\'s startup file needs, so tab completion works in the next shell'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l dry-run -d 'With --install: say what would be done, and do nothing'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -f -a "connect" -d 'Make pepe\'s Slack app, take its token, pick a channel, post a test (the default)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -f -a "status" -d 'What is connected, checked with Slack'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -f -a "test" -d 'Post a test message'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -f -a "serve" -d 'Answer the buttons and /pepe: run this directory\'s saved tests for the people allowed, and stop them'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -f -a "allow" -d 'Who may press the buttons and use /pepe: @names, Slack ids, or * for anyone in the channel'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -f -a "forget" -d 'Remove the kept token'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -f -a "manifest" -d 'Print the app\'s manifest, for Slack\'s "From a manifest"'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and not __fish_seen_subcommand_from connect status test serve allow forget manifest help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l token -d 'The bot token (xoxb-…), instead of asking for it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l channel -d 'The channel, instead of asking which' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from connect" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from status" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from test" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from serve" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from allow" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from forget" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from manifest" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from help" -f -a "connect" -d 'Make pepe\'s Slack app, take its token, pick a channel, post a test (the default)'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from help" -f -a "status" -d 'What is connected, checked with Slack'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from help" -f -a "test" -d 'Post a test message'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from help" -f -a "serve" -d 'Answer the buttons and /pepe: run this directory\'s saved tests for the people allowed, and stop them'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from help" -f -a "allow" -d 'Who may press the buttons and use /pepe: @names, Slack ids, or * for anyone in the channel'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from help" -f -a "forget" -d 'Remove the kept token'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from help" -f -a "manifest" -d 'Print the app\'s manifest, for Slack\'s "From a manifest"'
complete -c pepe -n "__fish_pepe_using_subcommand slack; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c pepe -n "__fish_pepe_using_subcommand export" -s o -l out -d 'Where it goes, its format from its extension; beside the report by default' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand export" -l to -d 'The format when -o doesn\'t say: html (the default), json, md, txt, csv, or all' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -s n -l number -d 'Number of requests to perform' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -s z -l duration -d 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -s m -l method -d 'HTTP method, e.g. GET, POST, PUT, DELETE' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -s H -l headers -d 'HTTP headers, e.g. -H \'Accept: application/json\'' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -s t -l timeout -d 'Time in seconds to wait for a response' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l warmup -d 'Send for this long before measuring, e.g. 5s: connections open, caches fill and JITs settle without counting against the run' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l threads -d 'Threads sending requests (default 1), or "auto" to add one whenever those sending are all busy. One sends 100k requests a second or more; the dashboard says when it is the limit' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l rate -d 'Start this many requests a second, spread evenly, instead of as many as the concurrency allows; -c is then the most in flight at once, and pepe says when it holds the rate back' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -s d -l body -d 'HTTP request body' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -s p -l proxy -d 'Proxy server URL: http://user:pass@host:port or socks5://host:port' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l trace-header -d 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server\'s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l vars -d 'Fill the {{holes}} in the URL, headers and body from this CSV file, whose first line names them: each request takes the next row, and the rows go round again. In a flow, each chain starts with the next row\'s values' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand export" -l webhook -d 'Send the run\'s verdict to this URL when it ends: a Slack, Discord, Teams or Google Chat webhook gets a message laid out for it, anything else the JSON report; repeat for more. ${NAME} is taken from the environment. A [[webhook]] table in pepe.toml says more' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l webhook-on -d 'When the --webhook URLs are sent: start, progress, anomaly, end, fail (the end when a --fail-if was crossed), comma-separated; end by default' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l slack -d 'Keep a Slack message up to date as the run goes: posted at the start, edited with the numbers, anomalies in its thread, the verdict at the end. --slack alone posts in the channel `pepe slack connect` kept; --slack=#channel elsewhere' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l export -d 'Write the report to FILE as well, in the format its extension says: .html a self-contained page (curves, thresholds, the last run), .json the report, .md a summary for a PR, .txt what pepe prints, .csv the run\'s numbers a row a second; repeat for more' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand export" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand export" -l allow-host -d 'Refuse to send anywhere but these hosts: api.example.com exactly, or .example.com for it and its subdomains; repeat for more. Every target is checked before anything is sent' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l max-requests -d 'Refuse a run that would send more requests than this: -n above it, or -z without a --rate that bounds it' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l max-rate -d 'Refuse a --rate above this, and a run with no --rate at all, which sends as fast as the target answers' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l max-concurrency -d 'Refuse a concurrency above this' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l fail-if -d 'End with exit code 4 when the run crosses this, for CI and scripts: \'p99 > 300ms\', \'errors > 1%\'; repeat for more. The report is still printed. (pepe ping has --slo for the same.)' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand export" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand export" -l load -d 'Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml; flags on the command line win over it' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l keep -d 'Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml, then run. With --load and no NAME, the loaded test is updated in place; with another NAME, it\'s copied' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand export" -l open -d 'Open it once written'
complete -c pepe -n "__fish_pepe_using_subcommand export" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand export" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand export" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand export" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand export" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand export" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand export" -l dry-run -d 'Say what would be sent, to where and how much, and send nothing; as JSON with --json'
complete -c pepe -n "__fish_pepe_using_subcommand export" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand export" -l record -d 'Keep the run\'s report under its saved test when it ends, as ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test (--load NAME, or --keep NAME)'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "ping" -d 'Ping a URL: a request a second, each split into DNS, connect, TLS, first byte and download, on a graph'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "ramp" -d 'Raise the load step by step to find where the target stops keeping up'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "compare" -d 'Hold a run\'s JSON report against an earlier one and say what moved'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "rerun" -d 'Run a saved test again, record the run, and say what moved since the run before'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "history" -d 'The saved tests and their recorded runs'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "api" -d 'Load-test every endpoint of an OpenAPI spec'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "flow" -d 'Run a sequence of requests from a flow file, each step fed by the last'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "replay" -d 'Send the URLs of an access log in their real proportions'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "logs" -d 'Read nginx logs: requests per second now, against each minute, hour and day'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "schema" -d 'Print the JSON Schema of a report: run (the default), ramp, ping or compare'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "mcp" -d 'Serve pepe\'s modes as tools for an agent over stdio (the Model Context Protocol); the guardrail flags given here hold for every call'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "self-update" -d 'Update pepe to the latest release, or say what\'s new in it'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "completions" -d 'Tab completion for your shell: print the script, or --install it'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "slack" -d 'Connect pepe to Slack from here, so --slack keeps a message there up to date as a run goes'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "export" -d 'Turn a JSON report into a page, a Markdown summary, text or CSV'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from ping ramp compare rerun history api flow replay logs schema mcp self-update completions slack export help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c pepe -n "__fish_pepe_using_subcommand help; and __fish_seen_subcommand_from slack" -f -a "connect" -d 'Make pepe\'s Slack app, take its token, pick a channel, post a test (the default)'
complete -c pepe -n "__fish_pepe_using_subcommand help; and __fish_seen_subcommand_from slack" -f -a "status" -d 'What is connected, checked with Slack'
complete -c pepe -n "__fish_pepe_using_subcommand help; and __fish_seen_subcommand_from slack" -f -a "test" -d 'Post a test message'
complete -c pepe -n "__fish_pepe_using_subcommand help; and __fish_seen_subcommand_from slack" -f -a "serve" -d 'Answer the buttons and /pepe: run this directory\'s saved tests for the people allowed, and stop them'
complete -c pepe -n "__fish_pepe_using_subcommand help; and __fish_seen_subcommand_from slack" -f -a "allow" -d 'Who may press the buttons and use /pepe: @names, Slack ids, or * for anyone in the channel'
complete -c pepe -n "__fish_pepe_using_subcommand help; and __fish_seen_subcommand_from slack" -f -a "forget" -d 'Remove the kept token'
complete -c pepe -n "__fish_pepe_using_subcommand help; and __fish_seen_subcommand_from slack" -f -a "manifest" -d 'Print the app\'s manifest, for Slack\'s "From a manifest"'
