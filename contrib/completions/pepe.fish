# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_pepe_global_optspecs
    string join \n h/help n/number= z/duration= curl m/method= H/headers= t/timeout= warmup= threads= rate= d/body= p/proxy= k/insecure disable-compression disable-keepalive disable-redirects json trace-header= snapshot= metrics= i/setup config= write-config= c/concurrency= u/user-agent= V/version
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
complete -c pepe -n "__fish_pepe_needs_command" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_needs_command" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_needs_command" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_needs_command" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_needs_command" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_needs_command" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_needs_command" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_needs_command" -l curl -d 'Load-test a curl command: pepe --curl -- curl -X POST http://localhost:8080, or pass it as one quoted string, as @file, or on stdin'
complete -c pepe -n "__fish_pepe_needs_command" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_needs_command" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_needs_command" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_needs_command" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_needs_command" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_needs_command" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_needs_command" -s V -l version -d 'Print version'
complete -c pepe -n "__fish_pepe_needs_command" -a "self-update" -d 'Update pepe to the latest release, or say what\'s new in it'
complete -c pepe -n "__fish_pepe_needs_command" -a "completions" -d 'Tab completion for your shell: print the script, or --install it'
complete -c pepe -n "__fish_pepe_needs_command" -a "api" -d 'Load-test every endpoint of an OpenAPI spec'
complete -c pepe -n "__fish_pepe_needs_command" -a "ramp" -d 'Raise the load step by step to find where the target stops keeping up'
complete -c pepe -n "__fish_pepe_needs_command" -a "replay" -d 'Send the URLs of an access log in their real proportions'
complete -c pepe -n "__fish_pepe_needs_command" -a "flow" -d 'Run a sequence of requests from a flow file, each step fed by the last'
complete -c pepe -n "__fish_pepe_needs_command" -a "logs" -d 'Read nginx logs: requests per second now, against each minute, hour and day'
complete -c pepe -n "__fish_pepe_needs_command" -a "ping" -d 'Ping a URL: a request a second, each split into DNS, connect, TLS, first byte and download, on a graph'
complete -c pepe -n "__fish_pepe_needs_command" -a "compare" -d 'Hold a run\'s JSON report against an earlier one and say what moved'
complete -c pepe -n "__fish_pepe_needs_command" -a "help" -d 'Print this message or the help of the given subcommand(s)'
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
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
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
complete -c pepe -n "__fish_pepe_using_subcommand self-update" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
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
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand completions" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
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
complete -c pepe -n "__fish_pepe_using_subcommand api" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand api" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand api" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand api" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
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
complete -c pepe -n "__fish_pepe_using_subcommand api" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
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
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand ramp" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
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
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l include-writes -d 'Replay POST, PUT, PATCH and DELETE too; only GET, HEAD and OPTIONS without it'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand replay" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
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
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand flow" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
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
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand logs" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
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
complete -c pepe -n "__fish_pepe_using_subcommand logs" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
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
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
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
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l once -d 'One diagnosis and out: three quick pings, then the last one\'s phases, what answered, and what to look at; no screen'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand ping" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
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
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l snapshot -d 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run\'s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l metrics -d 'Serve the live numbers for Prometheus at http://ADDR/metrics while the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long ping shows up in Grafana next to the server\'s own' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l config -d 'Read settings from this file instead of ./pepe.toml; flags on the command line win over it' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l write-config -d 'Write the settings as they stand to this file, as a pepe.toml, and exit' -r -F
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s c -l concurrency -d 'Number of concurrent requests at a time' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s u -l user-agent -d 'User-Agent string, default is pepe/{version}' -r
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l gate -d 'Exit 1 when the verdict is Slower or Worse, for CI'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s h -l help -d 'Print help'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s k -l insecure -d 'Accept invalid TLS certificates (self-signed, expired, wrong host)'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l disable-compression -d 'Disable HTTP compression, e.g. gzip'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l disable-keepalive -d 'Disable HTTP keepalive, e.g. Connection: close'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l disable-redirects -d 'Prevent http redirects'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -l json -d 'Output results in JSON format'
complete -c pepe -n "__fish_pepe_using_subcommand compare" -s i -l setup -d 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "self-update" -d 'Update pepe to the latest release, or say what\'s new in it'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "completions" -d 'Tab completion for your shell: print the script, or --install it'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "api" -d 'Load-test every endpoint of an OpenAPI spec'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "ramp" -d 'Raise the load step by step to find where the target stops keeping up'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "replay" -d 'Send the URLs of an access log in their real proportions'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "flow" -d 'Run a sequence of requests from a flow file, each step fed by the last'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "logs" -d 'Read nginx logs: requests per second now, against each minute, hour and day'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "ping" -d 'Ping a URL: a request a second, each split into DNS, connect, TLS, first byte and download, on a graph'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "compare" -d 'Hold a run\'s JSON report against an earlier one and say what moved'
complete -c pepe -n "__fish_pepe_using_subcommand help; and not __fish_seen_subcommand_from self-update completions api ramp replay flow logs ping compare help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
