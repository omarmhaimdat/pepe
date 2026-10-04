
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'pepe' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'pepe'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'pepe' {
            [CompletionResult]::new('-n', '-n', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('--number', '--number', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('-z', '-z', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('--duration', '--duration', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('-m', '-m', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('--method', '--method', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('-H', '-H ', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('--headers', '--headers', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('-t', '-t', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--timeout', '--timeout', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--threads', '--threads', [CompletionResultType]::ParameterName, 'Threads sending requests (default 1). One sends about 100k requests a second; the dashboard says when it is the limit')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('--body', '--body', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('-p', '-p', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--proxy', '--proxy', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--trace-header', '--trace-header', [CompletionResultType]::ParameterName, 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server''s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for')
            [CompletionResult]::new('--snapshot', '--snapshot', [CompletionResultType]::ParameterName, 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run''s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run')
            [CompletionResult]::new('-c', '-c', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('--concurrency', '--concurrency', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('-u', '-u', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('--user-agent', '--user-agent', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--curl', '--curl', [CompletionResultType]::ParameterName, 'Load-test a curl command: pepe --curl -- curl -X POST http://localhost:8080, or pass it as one quoted string, as @file, or on stdin')
            [CompletionResult]::new('-k', '-k', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--insecure', '--insecure', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--disable-compression', '--disable-compression', [CompletionResultType]::ParameterName, 'Disable HTTP compression, e.g. gzip')
            [CompletionResult]::new('--disable-keepalive', '--disable-keepalive', [CompletionResultType]::ParameterName, 'Disable HTTP keepalive, e.g. Connection: close')
            [CompletionResult]::new('--disable-redirects', '--disable-redirects', [CompletionResultType]::ParameterName, 'Prevent http redirects')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Output results in JSON format')
            [CompletionResult]::new('-i', '-i', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            [CompletionResult]::new('--setup', '--setup', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('self-update', 'self-update', [CompletionResultType]::ParameterValue, 'Update pepe to the latest release, or say what''s new in it')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Tab completion for your shell: print the script, or --install it')
            [CompletionResult]::new('api', 'api', [CompletionResultType]::ParameterValue, 'Load-test every endpoint of an OpenAPI spec')
            [CompletionResult]::new('ramp', 'ramp', [CompletionResultType]::ParameterValue, 'Raise the load step by step to find where the target stops keeping up')
            [CompletionResult]::new('flow', 'flow', [CompletionResultType]::ParameterValue, 'Run a sequence of requests from a flow file, each step fed by the last')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'pepe;self-update' {
            [CompletionResult]::new('-n', '-n', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('--number', '--number', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('-z', '-z', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('--duration', '--duration', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('-m', '-m', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('--method', '--method', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('-H', '-H ', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('--headers', '--headers', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('-t', '-t', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--timeout', '--timeout', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--threads', '--threads', [CompletionResultType]::ParameterName, 'Threads sending requests (default 1). One sends about 100k requests a second; the dashboard says when it is the limit')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('--body', '--body', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('-p', '-p', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--proxy', '--proxy', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--trace-header', '--trace-header', [CompletionResultType]::ParameterName, 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server''s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for')
            [CompletionResult]::new('--snapshot', '--snapshot', [CompletionResultType]::ParameterName, 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run''s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run')
            [CompletionResult]::new('-c', '-c', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('--concurrency', '--concurrency', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('-u', '-u', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('--user-agent', '--user-agent', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('--check', '--check', [CompletionResultType]::ParameterName, 'Only say whether a newer release exists (exit code 1 if so) and what''s in it; don''t install it')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Show the installer''s own output')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('-k', '-k', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--insecure', '--insecure', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--disable-compression', '--disable-compression', [CompletionResultType]::ParameterName, 'Disable HTTP compression, e.g. gzip')
            [CompletionResult]::new('--disable-keepalive', '--disable-keepalive', [CompletionResultType]::ParameterName, 'Disable HTTP keepalive, e.g. Connection: close')
            [CompletionResult]::new('--disable-redirects', '--disable-redirects', [CompletionResultType]::ParameterName, 'Prevent http redirects')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Output results in JSON format')
            [CompletionResult]::new('-i', '-i', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            [CompletionResult]::new('--setup', '--setup', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            break
        }
        'pepe;completions' {
            [CompletionResult]::new('-n', '-n', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('--number', '--number', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('-z', '-z', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('--duration', '--duration', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('-m', '-m', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('--method', '--method', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('-H', '-H ', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('--headers', '--headers', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('-t', '-t', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--timeout', '--timeout', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--threads', '--threads', [CompletionResultType]::ParameterName, 'Threads sending requests (default 1). One sends about 100k requests a second; the dashboard says when it is the limit')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('--body', '--body', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('-p', '-p', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--proxy', '--proxy', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--trace-header', '--trace-header', [CompletionResultType]::ParameterName, 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server''s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for')
            [CompletionResult]::new('--snapshot', '--snapshot', [CompletionResultType]::ParameterName, 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run''s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run')
            [CompletionResult]::new('-c', '-c', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('--concurrency', '--concurrency', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('-u', '-u', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('--user-agent', '--user-agent', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('--install', '--install', [CompletionResultType]::ParameterName, 'Put the script and the man pages in place and add the line the shell''s startup file needs, so tab completion works in the next shell')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'With --install: say what would be done, and do nothing')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('-k', '-k', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--insecure', '--insecure', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--disable-compression', '--disable-compression', [CompletionResultType]::ParameterName, 'Disable HTTP compression, e.g. gzip')
            [CompletionResult]::new('--disable-keepalive', '--disable-keepalive', [CompletionResultType]::ParameterName, 'Disable HTTP keepalive, e.g. Connection: close')
            [CompletionResult]::new('--disable-redirects', '--disable-redirects', [CompletionResultType]::ParameterName, 'Prevent http redirects')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Output results in JSON format')
            [CompletionResult]::new('-i', '-i', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            [CompletionResult]::new('--setup', '--setup', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            break
        }
        'pepe;api' {
            [CompletionResult]::new('--auth', '--auth', [CompletionResultType]::ParameterName, 'Credentials: bearer:TOKEN, basic:USER:PASSWORD, apikey:VALUE, header:NAME=VALUE or query:NAME=VALUE')
            [CompletionResult]::new('--server', '--server', [CompletionResultType]::ParameterName, 'Base URL to send requests to, instead of the spec''s server')
            [CompletionResult]::new('--tag', '--tag', [CompletionResultType]::ParameterName, 'Run the endpoints with this tag, e.g. --tag Billing')
            [CompletionResult]::new('--only', '--only', [CompletionResultType]::ParameterName, 'Run the endpoints matching this, e.g. ''GET /pets*'' or ''/pets/*''')
            [CompletionResult]::new('--skip', '--skip', [CompletionResultType]::ParameterName, 'Leave out endpoints matching this')
            [CompletionResult]::new('--set', '--set', [CompletionResultType]::ParameterName, 'A parameter''s value(s), rotated through: --set id=1,2,3')
            [CompletionResult]::new('-n', '-n', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('--number', '--number', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('-z', '-z', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('--duration', '--duration', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('-m', '-m', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('--method', '--method', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('-H', '-H ', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('--headers', '--headers', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('-t', '-t', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--timeout', '--timeout', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--threads', '--threads', [CompletionResultType]::ParameterName, 'Threads sending requests (default 1). One sends about 100k requests a second; the dashboard says when it is the limit')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('--body', '--body', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('-p', '-p', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--proxy', '--proxy', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--trace-header', '--trace-header', [CompletionResultType]::ParameterName, 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server''s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for')
            [CompletionResult]::new('--snapshot', '--snapshot', [CompletionResultType]::ParameterName, 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run''s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run')
            [CompletionResult]::new('-c', '-c', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('--concurrency', '--concurrency', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('-u', '-u', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('--user-agent', '--user-agent', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('--all', '--all', [CompletionResultType]::ParameterName, 'Run every endpoint that has the values it needs. Without --all, --tag or --only, nothing runs until it''s picked on the plan screen')
            [CompletionResult]::new('--include-writes', '--include-writes', [CompletionResultType]::ParameterName, 'Let --all, --tag and --only switch on POST, PUT, PATCH and DELETE too')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('-k', '-k', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--insecure', '--insecure', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--disable-compression', '--disable-compression', [CompletionResultType]::ParameterName, 'Disable HTTP compression, e.g. gzip')
            [CompletionResult]::new('--disable-keepalive', '--disable-keepalive', [CompletionResultType]::ParameterName, 'Disable HTTP keepalive, e.g. Connection: close')
            [CompletionResult]::new('--disable-redirects', '--disable-redirects', [CompletionResultType]::ParameterName, 'Prevent http redirects')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Output results in JSON format')
            [CompletionResult]::new('-i', '-i', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            [CompletionResult]::new('--setup', '--setup', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            break
        }
        'pepe;ramp' {
            [CompletionResult]::new('--from', '--from', [CompletionResultType]::ParameterName, 'Concurrency of the first step')
            [CompletionResult]::new('--to', '--to', [CompletionResultType]::ParameterName, 'Concurrency of the last step')
            [CompletionResult]::new('--step', '--step', [CompletionResultType]::ParameterName, 'Concurrency added at each step')
            [CompletionResult]::new('--every', '--every', [CompletionResultType]::ParameterName, 'How long each step is held, e.g. 10s, 1m')
            [CompletionResult]::new('--until', '--until', [CompletionResultType]::ParameterName, 'End the ramp once a step crosses this: ''p99 > 500ms'', ''errors > 1%''')
            [CompletionResult]::new('-n', '-n', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('--number', '--number', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('-z', '-z', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('--duration', '--duration', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('-m', '-m', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('--method', '--method', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('-H', '-H ', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('--headers', '--headers', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('-t', '-t', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--timeout', '--timeout', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--threads', '--threads', [CompletionResultType]::ParameterName, 'Threads sending requests (default 1). One sends about 100k requests a second; the dashboard says when it is the limit')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('--body', '--body', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('-p', '-p', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--proxy', '--proxy', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--trace-header', '--trace-header', [CompletionResultType]::ParameterName, 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server''s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for')
            [CompletionResult]::new('--snapshot', '--snapshot', [CompletionResultType]::ParameterName, 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run''s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run')
            [CompletionResult]::new('-c', '-c', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('--concurrency', '--concurrency', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('-u', '-u', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('--user-agent', '--user-agent', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('-k', '-k', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--insecure', '--insecure', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--disable-compression', '--disable-compression', [CompletionResultType]::ParameterName, 'Disable HTTP compression, e.g. gzip')
            [CompletionResult]::new('--disable-keepalive', '--disable-keepalive', [CompletionResultType]::ParameterName, 'Disable HTTP keepalive, e.g. Connection: close')
            [CompletionResult]::new('--disable-redirects', '--disable-redirects', [CompletionResultType]::ParameterName, 'Prevent http redirects')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Output results in JSON format')
            [CompletionResult]::new('-i', '-i', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            [CompletionResult]::new('--setup', '--setup', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            break
        }
        'pepe;flow' {
            [CompletionResult]::new('-n', '-n', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('--number', '--number', [CompletionResultType]::ParameterName, 'Number of requests to perform')
            [CompletionResult]::new('-z', '-z', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('--duration', '--duration', [CompletionResultType]::ParameterName, 'Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)')
            [CompletionResult]::new('-m', '-m', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('--method', '--method', [CompletionResultType]::ParameterName, 'HTTP method, e.g. GET, POST, PUT, DELETE')
            [CompletionResult]::new('-H', '-H ', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('--headers', '--headers', [CompletionResultType]::ParameterName, 'HTTP headers, e.g. -H ''Accept: application/json''')
            [CompletionResult]::new('-t', '-t', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--timeout', '--timeout', [CompletionResultType]::ParameterName, 'Time in seconds to wait for a response')
            [CompletionResult]::new('--threads', '--threads', [CompletionResultType]::ParameterName, 'Threads sending requests (default 1). One sends about 100k requests a second; the dashboard says when it is the limit')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('--body', '--body', [CompletionResultType]::ParameterName, 'HTTP request body')
            [CompletionResult]::new('-p', '-p', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--proxy', '--proxy', [CompletionResultType]::ParameterName, 'Proxy server URL: http://user:pass@host:port or socks5://host:port')
            [CompletionResult]::new('--trace-header', '--trace-header', [CompletionResultType]::ParameterName, 'Response header holding the request id to show for the slowest requests and in the inspector, so they can be found in the server''s logs; without it, X-Request-Id, traceparent, CF-Ray, X-Amzn-Trace-Id and other common ones are looked for')
            [CompletionResult]::new('--snapshot', '--snapshot', [CompletionResultType]::ParameterName, 'Write the JSON report so far to this file every minute while the run goes, and once more when it ends, so a long run''s numbers survive a crash or a lost terminal; it has a minute-by-minute timeline of the whole run')
            [CompletionResult]::new('-c', '-c', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('--concurrency', '--concurrency', [CompletionResultType]::ParameterName, 'Number of concurrent requests at a time')
            [CompletionResult]::new('-u', '-u', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('--user-agent', '--user-agent', [CompletionResultType]::ParameterName, 'User-Agent string, default is pepe/{version}')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('-k', '-k', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--insecure', '--insecure', [CompletionResultType]::ParameterName, 'Accept invalid TLS certificates (self-signed, expired, wrong host)')
            [CompletionResult]::new('--disable-compression', '--disable-compression', [CompletionResultType]::ParameterName, 'Disable HTTP compression, e.g. gzip')
            [CompletionResult]::new('--disable-keepalive', '--disable-keepalive', [CompletionResultType]::ParameterName, 'Disable HTTP keepalive, e.g. Connection: close')
            [CompletionResult]::new('--disable-redirects', '--disable-redirects', [CompletionResultType]::ParameterName, 'Prevent http redirects')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Output results in JSON format')
            [CompletionResult]::new('-i', '-i', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            [CompletionResult]::new('--setup', '--setup', [CompletionResultType]::ParameterName, 'Open the setup screen to review or change the settings before starting (it opens by itself when no URL is given)')
            break
        }
        'pepe;help' {
            [CompletionResult]::new('self-update', 'self-update', [CompletionResultType]::ParameterValue, 'Update pepe to the latest release, or say what''s new in it')
            [CompletionResult]::new('completions', 'completions', [CompletionResultType]::ParameterValue, 'Tab completion for your shell: print the script, or --install it')
            [CompletionResult]::new('api', 'api', [CompletionResultType]::ParameterValue, 'Load-test every endpoint of an OpenAPI spec')
            [CompletionResult]::new('ramp', 'ramp', [CompletionResultType]::ParameterValue, 'Raise the load step by step to find where the target stops keeping up')
            [CompletionResult]::new('flow', 'flow', [CompletionResultType]::ParameterValue, 'Run a sequence of requests from a flow file, each step fed by the last')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'pepe;help;self-update' {
            break
        }
        'pepe;help;completions' {
            break
        }
        'pepe;help;api' {
            break
        }
        'pepe;help;ramp' {
            break
        }
        'pepe;help;flow' {
            break
        }
        'pepe;help;help' {
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
