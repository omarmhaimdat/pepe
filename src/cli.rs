use clap::{ArgAction::HelpLong, Args, CommandFactory, Error, Parser, Subcommand};
use reqwest::Proxy;

use crate::curl;
use crate::request::{parse_header, Request, RequestSettings};
use crate::utils::{default_user_agent, num_of_cores, version};
use crate::vars::{Fill, Vars};
use crate::PepeError;

#[derive(Parser, Debug, Clone)]
#[command(name = "pepe")]
#[command(version = version())]
#[command(author = "Omar MHAIMDAT")]
#[command(about = "HTTP load generator")]
#[clap(disable_help_flag = true)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Print help
    #[arg(short, long, action = HelpLong, global = true)]
    pub help: Option<bool>,

    /// Number of requests to perform
    #[arg(short, long, default_value_t = 100, global = true)]
    pub number: u32,

    /// Number of concurrent requests at a time
    #[arg(short, long, default_value_t = num_of_cores(), global = true)]
    pub concurrency: u32,

    /// Duration of the test, e.g. 10s, 3m, 2h (mutually exclusive with -n)
    #[arg(short = 'z', long, global = true)]
    pub duration: Option<String>,

    /// Load-test a curl command: pepe --curl -- curl -X POST http://localhost:8080,
    /// or pass it as one quoted string, as @file, or on stdin
    #[arg(long)]
    pub curl: bool,

    /// HTTP method, e.g. GET, POST, PUT, DELETE
    #[arg(short, long, default_value_t = String::from("GET"), global = true)]
    pub method: String,

    /// HTTP headers, e.g. -H 'Accept: application/json'
    #[arg(short = 'H', long, global = true)]
    pub headers: Vec<String>,

    /// Time in seconds to wait for a response
    #[arg(short, long, default_value_t = 20, global = true)]
    pub timeout: u32,

    /// Send for this long before measuring, e.g. 5s: connections open,
    /// caches fill and JITs settle without counting against the run
    #[arg(long, global = true, value_name = "TIME")]
    pub warmup: Option<String>,

    /// Threads sending requests (default 1), or "auto" to add one whenever
    /// those sending are all busy. One sends 100k requests a second or
    /// more; the dashboard says when it is the limit
    #[arg(long, global = true, value_name = "N|auto")]
    pub threads: Option<crate::load::ThreadCount>,

    /// Start this many requests a second, spread evenly, instead of as
    /// many as the concurrency allows; -c is then the most in flight at
    /// once, and pepe says when it holds the rate back
    #[arg(long, global = true, value_name = "PER_SECOND")]
    pub rate: Option<f64>,

    /// HTTP request body
    #[arg(short = 'd', long, global = true)]
    pub body: Option<String>,

    /// User-Agent string, default is pepe/{version}
    #[arg(short, long, default_value_t = default_user_agent(), global = true)]
    pub user_agent: String,

    /// Proxy server URL: http://user:pass@host:port or socks5://host:port
    #[arg(short, long, global = true)]
    pub proxy: Option<String>,

    /// Accept invalid TLS certificates (self-signed, expired, wrong host)
    #[arg(short = 'k', long, global = true)]
    pub insecure: bool,

    /// Disable HTTP compression, e.g. gzip
    #[arg(long, global = true)]
    pub disable_compression: bool,

    /// Disable HTTP keepalive, e.g. Connection: close
    #[arg(long, global = true)]
    pub disable_keepalive: bool,

    /// Prevent http redirects
    #[arg(long, global = true)]
    pub disable_redirects: bool,

    /// Output results in JSON format
    #[arg(long, global = true)]
    pub json: bool,

    /// Response header holding the request id to show for the slowest
    /// requests and in the inspector, so they can be found in the
    /// server's logs; without it, X-Request-Id, traceparent, CF-Ray,
    /// X-Amzn-Trace-Id and other common ones are looked for
    #[arg(long, global = true, value_name = "NAME")]
    pub trace_header: Option<String>,

    /// Fill the {{holes}} in the URL, headers and body from this CSV
    /// file, whose first line names them: each request takes the next
    /// row, and the rows go round again. In a flow, each chain starts
    /// with the next row's values
    #[arg(long, global = true, value_name = "FILE")]
    pub vars: Option<std::path::PathBuf>,

    /// Send the run's verdict to this URL when it ends: a Slack, Discord,
    /// Teams or Google Chat webhook gets a message laid out for it,
    /// anything else the JSON report; repeat for more. ${NAME} is taken
    /// from the environment. A [[webhook]] table in pepe.toml says more
    #[arg(long, global = true, value_name = "URL")]
    pub webhook: Vec<String>,

    /// When the --webhook URLs are sent: start, progress, anomaly, end,
    /// fail (the end when a --fail-if was crossed), comma-separated; end
    /// by default
    #[arg(long, global = true, value_name = "EVENTS")]
    pub webhook_on: Option<String>,

    /// Keep a Slack message up to date as the run goes: posted at the
    /// start, edited with the numbers, anomalies in its thread, the
    /// verdict at the end. --slack alone posts in the channel `pepe slack
    /// connect` kept; --slack=#channel elsewhere
    #[arg(long, global = true, value_name = "CHANNEL", num_args = 0..=1, require_equals = true, default_missing_value = "")]
    pub slack: Option<String>,

    /// Write the report to FILE as well, in the format its extension says:
    /// .html a self-contained page (curves, thresholds, the last run),
    /// .json the report, .md a summary for a PR, .txt what pepe prints,
    /// .csv the run's numbers a row a second; repeat for more
    #[arg(long, global = true, value_name = "FILE")]
    pub export: Vec<std::path::PathBuf>,

    /// Webhooks from a pepe.toml, the settings or the screens, when no
    /// --webhook was given
    #[arg(skip)]
    pub webhooks: Vec<crate::webhook::Webhook>,

    /// Write the JSON report so far to this file every minute while the
    /// run goes, and once more when it ends, so a long run's numbers
    /// survive a crash or a lost terminal; it has a minute-by-minute
    /// timeline of the whole run
    #[arg(long, global = true, value_name = "FILE")]
    pub snapshot: Option<std::path::PathBuf>,

    /// Refuse to send anywhere but these hosts: api.example.com exactly,
    /// or .example.com for it and its subdomains; repeat for more. Every
    /// target is checked before anything is sent.
    #[arg(long, global = true, value_name = "HOST")]
    pub allow_host: Vec<String>,

    /// The `allow-host` lists of the files that hold this run (a
    /// `pepe.toml`, the machine's settings, a saved test), each of which
    /// a host must be in as well as the command line's
    #[arg(skip)]
    pub held_hosts: Vec<Vec<String>>,

    /// Refuse a run that would send more requests than this: -n above
    /// it, or -z without a --rate that bounds it
    #[arg(long, global = true, value_name = "N")]
    pub max_requests: Option<u64>,

    /// Refuse a --rate above this, and a run with no --rate at all,
    /// which sends as fast as the target answers
    #[arg(long, global = true, value_name = "PER_SECOND")]
    pub max_rate: Option<f64>,

    /// Refuse a concurrency above this
    #[arg(long, global = true, value_name = "N")]
    pub max_concurrency: Option<u32>,

    /// Say what would be sent, to where and how much, and send nothing;
    /// as JSON with --json
    #[arg(long, global = true)]
    pub dry_run: bool,

    /// End with exit code 4 when the run crosses this, for CI and scripts:
    /// 'p99 > 300ms', 'errors > 1%'; repeat for more. The report is still
    /// printed. pepe logs takes '5xx > 2%', '4xx > 20%' and 'rate < 5/s'
    /// too. (pepe ping has --slo for the same.)
    #[arg(long, global = true, value_name = "CONDITION")]
    pub fail_if: Vec<String>,

    /// Serve the live numbers for Prometheus at http://ADDR/metrics while
    /// the run goes, e.g. :9100 or 127.0.0.1:9100, so a soak run or a long
    /// ping shows up in Grafana next to the server's own
    #[arg(long, global = true, value_name = "ADDR")]
    pub metrics: Option<String>,

    /// Take the service's OpenTelemetry spans at ADDR (OTLP over HTTP, as
    /// :4318) and say where its time went: each request carries its own
    /// traceparent, and the report breaks the server's time down by span.
    /// Point the service at it: OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4318
    #[arg(long, value_name = "ADDR")]
    pub otlp: Option<String>,

    /// Open the setup screen to review or change the settings before
    /// starting (it opens by itself when no URL is given)
    #[arg(short = 'i', long, global = true)]
    pub setup: bool,

    /// Read settings from this file instead of ./pepe.toml; flags on the
    /// command line win over it
    #[arg(long, global = true, value_name = "FILE")]
    pub config: Option<std::path::PathBuf>,

    /// Write the settings as they stand to this file, as a pepe.toml, and exit
    #[arg(long, global = true, value_name = "FILE")]
    pub write_config: Option<std::path::PathBuf>,

    /// Run a test saved on the setup screen (ctrl-s): ./.pepe/tests/NAME.toml;
    /// flags on the command line win over it
    #[arg(long, global = true, value_name = "NAME", conflicts_with = "config")]
    pub load: Option<String>,

    /// Keep the settings as they stand as a saved test, ./.pepe/tests/NAME.toml,
    /// then run. With --load and no NAME, the loaded test is updated in place;
    /// with another NAME, it's copied
    #[arg(long, global = true, value_name = "NAME", num_args = 0..=1, default_missing_value = "")]
    pub keep: Option<String>,

    /// Keep the run's report under its saved test when it ends, as
    /// ./.pepe/runs/NAME/<moment>.json; the run has to be a saved test
    /// (--load NAME, or --keep NAME)
    #[arg(long, global = true)]
    pub record: bool,

    /// `pepe rerun`: after recording, what moved since the run before
    #[arg(skip)]
    pub rerun: Option<RerunArgs>,

    /// `-n` came from a saved test's file rather than the command line; a
    /// ping, which runs forever by default, ends on it all the same
    #[arg(skip)]
    pub number_from_file: bool,

    /// The config file that was read, if one was
    #[arg(skip)]
    pub config_loaded: Option<std::path::PathBuf>,

    /// HTTP url to request
    #[arg(default_value_t = String::from(""))]
    pub url: String,

    /// List of arguments to pass to curl command
    #[arg(last = true, default_value = "")]
    pub args: Vec<String>,

    /// Request body as bytes, when it can't be text (from --curl)
    #[arg(skip)]
    pub body_bytes: Option<Vec<u8>>,
}

#[derive(Args, Debug, Clone, Default)]
pub struct SelfUpdateArgs {
    /// Only say whether a newer release exists (exit code 1 if so) and
    /// what's in it; don't install it
    #[arg(long)]
    pub check: bool,
    /// Show the installer's own output
    #[arg(long)]
    pub verbose: bool,
}

#[derive(Args, Debug, Clone, Default)]
pub struct CompletionsArgs {
    /// bash, zsh, fish or powershell; the current shell when left out
    pub shell: Option<crate::completions::Shell>,
    /// Put the script and the man pages in place and add the line the
    /// shell's startup file needs, so tab completion works in the next shell
    #[arg(long)]
    pub install: bool,
    /// With --install: say what would be done, and do nothing
    #[arg(long, requires = "install")]
    pub dry_run: bool,
}

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Ping a URL: a request a second, each split into DNS, connect, TLS, first byte and download, on a graph
    Ping(PingArgs),
    /// Raise the load step by step to find where the target stops keeping up
    Ramp(RampArgs),
    /// Hold a run's JSON report against an earlier one and say what moved
    Compare(CompareArgs),
    /// Run a saved test again, record the run, and say what moved since the run before
    Rerun(RerunArgs),
    /// The saved tests and their recorded runs
    History(HistoryArgs),
    /// Load-test every endpoint of an OpenAPI spec
    Api(ApiArgs),
    /// Run a sequence of requests from a flow file, each step fed by the last
    Flow(FlowArgs),
    /// Send the URLs of an access log in their real proportions
    Replay(ReplayArgs),
    /// Read nginx logs: requests per second now, against each minute, hour and day
    Logs(LogsArgs),
    /// Print the JSON Schema of a report: run (the default), ramp, ping or compare
    Schema(SchemaArgs),
    /// Serve pepe's modes as tools for an agent over stdio (the Model Context Protocol); the guardrail flags given here hold for every call
    Mcp(McpArgs),
    /// Update pepe to the latest release, or say what's new in it
    SelfUpdate(SelfUpdateArgs),
    /// Tab completion for your shell: print the script, or --install it
    Completions(CompletionsArgs),
    /// Connect pepe to Slack from here, so --slack keeps a message there up to date as a run goes
    Slack(SlackArgs),
    /// Turn a JSON report into a page, a Markdown summary, text or CSV
    Export(ExportArgs),
}

#[derive(clap::Args, Debug, Clone, PartialEq, Default)]
pub struct ExportArgs {
    /// The JSON report: a run's (--json, --snapshot, a recorded run), a ramp's or a ping's
    #[arg(value_name = "REPORT")]
    pub report: std::path::PathBuf,
    /// Where it goes, its format from its extension; beside the report by default
    #[arg(short, long, value_name = "FILE")]
    pub out: Option<std::path::PathBuf>,
    /// The format when -o doesn't say: html (the default), json, md, txt, csv, or all
    #[arg(long, value_name = "FORMAT")]
    pub to: Option<String>,
    /// Open it once written
    #[arg(long)]
    pub open: bool,
}

#[derive(clap::Args, Debug, Clone, PartialEq, Default)]
pub struct SlackArgs {
    #[command(subcommand)]
    pub what: Option<SlackCommand>,
}

#[derive(Subcommand, Debug, Clone, PartialEq)]
pub enum SlackCommand {
    /// Make pepe's Slack app, take its token, pick a channel, post a test (the default)
    Connect {
        /// The bot token (xoxb-…), instead of asking for it
        #[arg(long)]
        token: Option<String>,
        /// The channel, instead of asking which
        #[arg(long)]
        channel: Option<String>,
    },
    /// What is connected, checked with Slack
    Status,
    /// Post a test message
    Test {
        /// Where, if not the connected channel
        channel: Option<String>,
    },
    /// Answer the buttons and /pepe: run this directory's saved tests for the people allowed, and stop them
    Serve,
    /// Who may press the buttons and use /pepe: @names, Slack ids, or * for anyone in the channel
    Allow {
        #[arg(required = true, value_name = "WHO")]
        names: Vec<String>,
    },
    /// Remove the kept token
    Forget,
    /// Print the app's manifest, for Slack's "From a manifest"
    Manifest,
}

#[derive(clap::Args, Debug, Clone, PartialEq, Default)]
pub struct McpArgs {}

#[derive(clap::Args, Debug, Clone, PartialEq, Default)]
pub struct SchemaArgs {
    /// Which report: run, ramp, ping or compare
    #[arg(default_value = "run", value_name = "REPORT")]
    pub which: String,
}

impl PingArgs {
    /// The arguments as `pepe ping` has them before any flag: clap's
    /// defaults, which `Default` doesn't know
    pub fn usual() -> Self {
        match Cli::parse_from(["pepe", "ping", "x"]).command {
            Some(Command::Ping(mut ping)) => {
                ping.targets.clear();
                ping
            }
            _ => PingArgs::default(),
        }
    }
}

#[derive(clap::Args, Debug, Clone, PartialEq, Default)]
pub struct PingArgs {
    /// What to ping: URLs or hosts (https unless a port is given),
    /// host:port, aws:REGION, a range like 10.0.0.0/29, or commands with
    /// --cmd; several are graphed together
    pub targets: Vec<String>,

    /// Time between pings, e.g. 1s, 500ms, 2m
    #[arg(long, default_value = "1s", value_name = "TIME")]
    pub every: String,

    /// How much of the run the graph shows, e.g. 60s, 5m; + and - change
    /// it on screen, w shows the whole run
    #[arg(long, default_value = "60s", value_name = "TIME")]
    pub window: String,

    /// What to call each target, in order: --name api --name cdn, or
    /// --name api,cdn
    #[arg(long, value_name = "NAME")]
    pub name: Vec<String>,

    /// A colour for each target's line, in order: red, green, yellow,
    /// blue, magenta, cyan, white, gray, their light- forms, or #RRGGBB
    #[arg(long, value_name = "COLOR")]
    pub color: Vec<String>,

    /// Resolve names to IPv4 addresses only
    #[arg(short = '4', conflicts_with = "ipv6")]
    pub ipv4: bool,

    /// Resolve names to IPv6 addresses only
    #[arg(short = '6', conflicts_with = "ipv4")]
    pub ipv6: bool,

    /// Ping every address a name resolves to, each as a target of its own
    #[arg(long)]
    pub all_ips: bool,

    /// Send from this interface (en0, eth0) or local address
    #[arg(long, value_name = "NAME|IP")]
    pub interface: Option<String>,

    /// Only connect: a TCP ping of the port, with no request sent
    #[arg(long)]
    pub tcp: bool,

    /// With --tcp: the port of a target that names none
    #[arg(long, default_value_t = 80, value_name = "PORT")]
    pub port: u16,

    /// With --tcp: a connection refused counts as an answer (the host is
    /// there) rather than a failure
    #[arg(long, value_enum, default_value_t = Refused::Pong)]
    pub tcp_rst: Refused,

    /// The targets are commands: run each one every interval and graph
    /// how long it takes; its exit code is the status
    #[arg(long)]
    pub cmd: bool,

    /// Keep the connection between pings, as a browser would. The DNS,
    /// connect and TLS phases are then measured once; without it every
    /// ping measures all five.
    #[arg(long)]
    pub keep_alive: bool,

    /// Limits a ping must meet, in milliseconds: total=500,ttfb=200,
    /// connect=100,dns=50,tls=150,download=100. Breaking one marks the
    /// ping, and the run exits 4 at the end.
    #[arg(long, value_name = "KEY=MS,...")]
    pub slo: Option<String>,

    /// Ring the terminal bell when a ping fails or breaks the SLO
    #[arg(long)]
    pub bell: bool,

    /// The graph's floor, in milliseconds
    #[arg(long, value_name = "MS")]
    pub ymin: Option<u64>,

    /// The graph's ceiling, in milliseconds; without it the graph fits
    /// what it shows
    #[arg(long, value_name = "MS")]
    pub ymax: Option<u64>,

    /// Start the graph at zero (the same as --ymin 0)
    #[arg(short = '0', conflicts_with = "ymin")]
    pub ymin_zero: bool,

    /// Draw the graph with dots rather than braille, for terminals and
    /// fonts that lack it
    #[arg(short = 's', long)]
    pub simple_graphics: bool,

    /// No screen: one JSON object per ping on stdout, as it happens
    #[arg(long, conflicts_with = "csv")]
    pub jsonl: bool,

    /// No screen: one CSV line per ping on stdout, under a header
    #[arg(long)]
    pub csv: bool,

    /// Write the JSON report to this file when the run ends, whatever
    /// else is shown
    #[arg(long, value_name = "FILE")]
    pub save: Option<std::path::PathBuf>,

    /// Keep the first kilobyte of each body for the inspector
    #[arg(long)]
    pub show_body: bool,

    /// Write the last body received to this file when the run ends
    #[arg(long, value_name = "FILE")]
    pub save_body: Option<std::path::PathBuf>,

    /// Speak HTTP/1.1 even when the server offers HTTP/2; without it a
    /// ping takes h2 when the server offers it through ALPN
    #[arg(long)]
    pub http1: bool,

    /// One diagnosis and out: three quick pings, then the last one's
    /// phases, what answered, and what to look at; no screen
    #[arg(long)]
    pub once: bool,
}

/// `--tcp-rst`: what a connection refused means
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Refused {
    /// An answer: something is there to refuse
    #[default]
    Pong,
    /// A failure
    Fail,
}

#[derive(clap::Args, Debug, Clone, PartialEq)]
pub struct CompareArgs {
    /// The earlier report (`pepe --json`, `--snapshot`, or a ramp's), or
    /// the name of a saved test, whose last two recorded runs are compared
    pub before: std::path::PathBuf,
    /// The later one, of the same test
    pub after: Option<std::path::PathBuf>,

    /// Exit 1 when the verdict is Slower or Worse, for CI
    #[arg(long)]
    pub gate: bool,

    /// Also write the verdict as a card, an SVG for a README or a page
    #[arg(long, value_name = "FILE")]
    pub svg: Option<std::path::PathBuf>,
}

#[derive(clap::Args, Debug, Clone, PartialEq)]
pub struct RerunArgs {
    /// The saved test, as `pepe history` lists them
    pub name: String,

    /// Exit 1 when the verdict is Slower or Worse, for CI
    #[arg(long)]
    pub gate: bool,
}

#[derive(clap::Args, Debug, Clone, PartialEq)]
pub struct HistoryArgs {
    /// One saved test: its recorded runs, the latest last. Without it,
    /// every saved test with its last run
    pub name: Option<String>,
}

#[derive(clap::Args, Debug, Clone, PartialEq)]
pub struct LogsArgs {
    /// Access and error logs, rotated ones too; `-` or nothing reads what
    /// is piped in, and with nothing at all nginx's own in /var/log/nginx
    pub files: Vec<std::path::PathBuf>,

    /// The access log's log_format as nginx.conf has it, on one line, when
    /// it isn't `combined`: '$remote_addr [$time_local] "$request" $status $request_time'
    #[arg(long, value_name = "LOG_FORMAT")]
    pub format: Option<String>,

    /// What "now" is measured over, e.g. 10s, 1m, 5m
    #[arg(long, default_value = "60s", value_name = "TIME")]
    pub window: String,

    /// How far back to start, e.g. 90m, 24h, 7d. At a terminal a log that
    /// is being written is shown live, from five minutes back, without it
    #[arg(long, value_name = "TIME")]
    pub since: Option<String>,

    /// Read all of the log before following it, however far back it goes
    #[arg(long, conflicts_with = "since")]
    pub all: bool,

    /// Count /items/1 and /items/2 apart. Without it the numbers and ids
    /// in a path count as one, /items/*
    #[arg(long)]
    pub exact_paths: bool,

    /// Rows in each table of the report
    #[arg(long, default_value_t = 10, value_name = "N")]
    pub rows: usize,

    /// Count only the requests to these paths, with * for any run of
    /// characters: /api/*, /login; repeat for more. An error log line
    /// counts when its request does.
    #[arg(long = "path", value_name = "PATH")]
    pub paths: Vec<String>,

    /// Count only the requests that answered these: 404, 5xx, 4xx,503;
    /// repeat for more. Error log lines are still counted.
    #[arg(long = "status", value_name = "CODES")]
    pub statuses: Vec<String>,

    /// Count only the requests to these hosts (the log's $host): shop.test
    /// exactly, .example.com for it and every host under it; repeat for more
    #[arg(long = "host", value_name = "HOST")]
    pub hosts: Vec<String>,

    /// Leave out what is newer than this, e.g. 1h for up to an hour ago;
    /// with --since, a stretch in the past. A log read up to a time in the
    /// past isn't followed.
    #[arg(long, value_name = "TIME")]
    pub until: Option<String>,
}

#[derive(clap::Args, Debug, Clone, PartialEq)]
pub struct FlowArgs {
    /// The flow: a TOML file of [[step]]s (see the README)
    pub file: std::path::PathBuf,
}

#[derive(clap::Args, Debug, Clone, PartialEq)]
pub struct ReplayArgs {
    /// The log: nginx or Apache (common or combined), Caddy JSON, AWS ALB,
    /// or one URL or path per line
    pub log: std::path::PathBuf,

    /// Where to send the requests: put in front of paths, and in place of
    /// the host of full URLs, e.g. https://staging.example.com
    #[arg(long, value_name = "URL")]
    pub base_url: Option<String>,

    /// Replay POST, PUT, PATCH and DELETE too; only GET, HEAD and OPTIONS
    /// without it
    #[arg(long)]
    pub include_writes: bool,

    /// URLs that get a row of their own on the dashboard, most frequent
    /// first; the rest share one
    #[arg(long, default_value_t = 20, value_name = "N")]
    pub rows: usize,
}

#[derive(clap::Args, Debug, Clone, PartialEq)]
pub struct RampArgs {
    /// HTTP url to request; without one, the setup screen opens
    #[arg(default_value_t = String::new(), hide_default_value = true)]
    pub url: String,

    /// Concurrency of the first step
    #[arg(long, default_value_t = 10)]
    pub from: u32,

    /// Concurrency of the last step
    #[arg(long, default_value_t = 100)]
    pub to: u32,

    /// Concurrency added at each step
    #[arg(long, default_value_t = 10)]
    pub step: u32,

    /// How long each step is held, e.g. 10s, 1m
    #[arg(long, default_value = "10s")]
    pub every: String,

    /// End the ramp once a step crosses this: 'p99 > 500ms', 'errors > 1%'
    #[arg(long, value_name = "CONDITION")]
    pub until: Vec<String>,
}

impl Default for RampArgs {
    fn default() -> Self {
        RampArgs {
            url: String::new(),
            from: 10,
            to: 100,
            step: 10,
            every: "10s".into(),
            until: Vec::new(),
        }
    }
}

#[derive(clap::Args, Debug, Clone, Default)]
pub struct ApiArgs {
    /// The OpenAPI spec: a file, a URL or the document itself, JSON or
    /// YAML (or `spec` in pepe.toml). Without it, the setup screen asks
    #[arg(default_value_t = String::new(), hide_default_value = true)]
    pub spec: String,

    /// Credentials: bearer:TOKEN, basic:USER:PASSWORD, apikey:VALUE,
    /// header:NAME=VALUE or query:NAME=VALUE
    #[arg(long)]
    pub auth: Vec<String>,

    /// Base URL to send requests to, instead of the spec's server
    #[arg(long)]
    pub server: Option<String>,

    /// Run every endpoint that has the values it needs. Without --all,
    /// --tag or --only, nothing runs until it's picked on the plan screen
    #[arg(long)]
    pub all: bool,

    /// Run the endpoints with this tag, e.g. --tag Billing
    #[arg(long)]
    pub tag: Vec<String>,

    /// Run the endpoints matching this, e.g. 'GET /pets*' or '/pets/*'
    #[arg(long)]
    pub only: Vec<String>,

    /// Leave out endpoints matching this
    #[arg(long)]
    pub skip: Vec<String>,

    /// A parameter's value(s), rotated through: --set id=1,2,3
    #[arg(long = "set", value_name = "NAME=VALUE[,VALUE]")]
    pub set: Vec<String>,

    /// Let --all, --tag and --only switch on POST, PUT, PATCH and DELETE too
    #[arg(long)]
    pub include_writes: bool,
}

impl Cli {
    pub fn validate(&mut self) -> Result<(), Error> {
        if let Some(warmup) = &self.warmup {
            Self::parse_duration_for("--warmup", warmup)?;
        }
        if let Some(rate) = self.rate {
            if !(rate >= crate::guard::MIN_RATE && rate.is_finite()) {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!(
                        "--rate {rate}: the rate is requests per second, at least one every 15 minutes ({:.4}), as in --rate 500",
                        crate::guard::MIN_RATE
                    ),
                ));
            }
        }
        if let Some(max) = self.max_rate {
            if !(max > 0.0 && max.is_finite()) {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("--max-rate {max}: the cap is requests per second, above 0, as in --max-rate 500"),
                ));
            }
        }
        for condition in &self.fail_if {
            crate::ramp::Condition::parse(condition).map_err(|e| {
                Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("--fail-if: {e}"),
                )
            })?;
        }
        if let Some(name) = &self.trace_header {
            if reqwest::header::HeaderName::from_bytes(name.as_bytes()).is_err() {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("--trace-header {name:?} is not a header name; letters, digits and dashes, as in X-Request-Id"),
                ));
            }
        }
        for path in &self.export {
            crate::export::Format::of_path(path).map_err(|e| {
                Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("--export {e}"),
                )
            })?;
        }
        if let Some(on) = &self.webhook_on {
            crate::webhook::Event::parse_list(on).map_err(|e| {
                Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("--webhook-on: {e}"),
                )
            })?;
        }
        crate::webhook::check(&self.hooks())
            .map_err(|e| Error::raw(clap::error::ErrorKind::ValueValidation, e))?;
        if self.otlp.is_some() && self.command.is_some() {
            return Err(Error::raw(
                clap::error::ErrorKind::ArgumentConflict,
                "--otlp is for a plain run for now: pepe --otlp :4318 -n 2000 URL",
            ));
        }
        // A ramp sets its own concurrency and runs for as long as its
        // steps; a ping sends one at a time
        let ramp = matches!(
            self.command,
            Some(Command::Ramp(_)) | Some(Command::Ping(_))
        );
        if self.concurrency > self.number && self.duration.is_none() && !ramp {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!(
                    "-c {} is more than the {} requests of -n: lower -c, raise -n, or run for a time instead with -z 30s",
                    self.concurrency, self.number
                ),
            ));
        }

        if !self.curl && self.url.is_empty() && self.command.is_none() {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "a URL is needed: pepe https://example.com, pepe --curl -- curl ... for a curl command, or pepe alone at a terminal for the setup screen",
            ));
        }

        if self.timeout == 0 || self.timeout > 120 {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!(
                    "-t {}: the timeout is in seconds, from 1 to 120, as in -t 30",
                    self.timeout
                ),
            ));
        }

        // Validate and parse duration if provided
        if let Some(ref duration_str) = self.duration {
            Self::parse_duration_for("-z", duration_str)?;
        }

        if self.curl {
            self.take_curl().map_err(|e| {
                Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("Invalid curl command: {e}"),
                )
            })?;
        }

        for header in &self.headers {
            parse_header(header)
                .map_err(|e| Error::raw(clap::error::ErrorKind::ValueValidation, e))?;
        }

        let method = reqwest::Method::from_bytes(self.method.as_bytes());
        if method.is_err() {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!(
                    "-m {}: not an HTTP method; GET, POST, PUT, PATCH, DELETE, HEAD or OPTIONS",
                    self.method
                ),
            ));
        }

        if let Some(proxy) = &self.proxy {
            if proxy.starts_with("socks4") {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("-p {proxy}: SOCKS4 isn't supported; use socks5://host:port or http://host:port"),
                ));
            }
            if Proxy::all(proxy).is_err() {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("-p {proxy} is not a proxy URL; http://host:port or socks5://host:port, with a user:password@ in front if it needs one"),
                ));
            }
        }
        // Parsed once more when the run starts; checked here for a clean
        // message rather than a dashboard full of failures
        if let Some(path) = &self.vars {
            self.check_vars(path)?;
        } else if !self.url.is_empty() {
            if let Err(e) = reqwest::Url::parse(&self.url) {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    Self::url_trouble(&self.url, e),
                ));
            }
        }
        Ok(())
    }

    /// What's wrong with a URL that didn't parse, and what to do: a host
    /// without its scheme is given one, and a bare word is told apart from
    /// the commands, which a pepe older than the word may not have
    fn url_trouble(url: &str, e: impl std::fmt::Display) -> String {
        let bare = !url.is_empty()
            && url
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
        if url.contains("://") {
            format!("Invalid URL {url:?}: {e}")
        } else if bare {
            let commands: Vec<String> = Cli::command()
                .get_subcommands()
                .map(|c| c.get_name().to_string())
                .collect();
            format!(
                "{url:?} isn't a URL, nor a command this pepe ({}) has.\n\
                 A URL starts with http:// or https://, as in pepe https://example.com\n\
                 The commands are {}; `pepe self-update` gets the newest ones",
                version(),
                commands.join(", ")
            )
        } else {
            format!("{url:?} has no scheme: a URL starts with http:// or https://, as in pepe https://{url}")
        }
    }

    /// `parse_duration`, with the flag the value came from in front of
    /// what's wrong with it
    pub fn parse_duration_for(flag: &str, duration_str: &str) -> Result<u64, Error> {
        Self::duration_ms(duration_str).map_err(|what| {
            Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!("{flag} {what}"),
            )
        })
    }

    /// Parse duration string like "10s", "5m", "2h" into milliseconds
    pub fn parse_duration(duration_str: &str) -> Result<u64, Error> {
        Self::duration_ms(duration_str)
            .map_err(|what| Error::raw(clap::error::ErrorKind::ValueValidation, what))
    }

    /// The milliseconds in "10s", "5m" or "2h", or what is wrong with it
    fn duration_ms(duration_str: &str) -> Result<u64, String> {
        let duration_str = duration_str.trim().to_lowercase();

        let (num_str, unit) = if let Some(idx) = duration_str.find(|c: char| c.is_alphabetic()) {
            duration_str.split_at(idx)
        } else {
            return Err(format!(
                "{duration_str:?} is not a duration: a number and a unit, as in 10s, 5m or 2h"
            ));
        };

        let num: u64 = num_str.trim().parse().map_err(|_| {
            format!("{duration_str:?} is not a duration: a whole number and a unit, as in 10s, 5m or 2h")
        })?;

        let unit_ms: u64 = match unit {
            "s" | "sec" | "second" | "seconds" => 1000,
            "m" | "min" | "minute" | "minutes" => 60 * 1000,
            "h" | "hour" | "hours" => 60 * 60 * 1000,
            _ => {
                return Err(format!(
                    "{duration_str:?}: the unit is s, m or h, as in 10s, 5m or 2h"
                ))
            }
        };
        let milliseconds = num.checked_mul(unit_ms).ok_or_else(|| {
            format!("{duration_str:?} is longer than a run can be; -z 8h is a long one")
        })?;

        if milliseconds == 0 {
            return Err(format!(
                "{duration_str:?}: a duration is more than 0, as in 10s"
            ));
        }

        Ok(milliseconds)
    }

    /// The curl command after `--curl --`, taken into the settings once:
    /// validation does it, or `--save` earlier, to keep the request
    pub fn take_curl(&mut self) -> Result<(), String> {
        if !self.curl {
            return Ok(());
        }
        let request = curl_request(&self.args)?;
        // stderr, so `--json` output on stdout stays valid JSON
        eprintln!("curl: {} {}", request.method, request.url);
        for note in &request.notes {
            eprintln!("  note: {note}");
        }
        self.apply_curl(request);
        self.curl = false;
        self.args.clear();
        Ok(())
    }

    /// Take the request a curl command describes. Like curl, redirects are
    /// only followed with -L.
    fn apply_curl(&mut self, request: curl::CurlRequest) {
        self.method = request.method;
        self.url = request.url;
        self.headers = request.headers;
        self.body = None;
        self.body_bytes = request.body;
        if let Some(user_agent) = request.user_agent {
            self.user_agent = user_agent;
        }
        if request.proxy.is_some() {
            self.proxy = request.proxy;
        }
        if let Some(timeout) = request.timeout_secs {
            self.timeout = timeout;
        }
        self.insecure |= request.insecure;
        self.disable_keepalive |= request.no_keepalive;
        self.disable_redirects = !request.follow_redirects;
    }

    /// `command_line` with the secrets in headers masked, for a report
    pub fn command_line_masked(&self) -> String {
        let mut masked = self.clone();
        masked.headers = crate::guard::Plan::headers_of(&self.headers);
        masked.command_line_of(true)
    }

    /// The pepe command that reproduces these settings, leaving out
    /// everything still at its default
    pub fn command_line(&self) -> String {
        self.command_line_of(false)
    }

    /// `masked`: a webhook's URL, its secret, as `webhook::mask` shows it
    fn command_line_of(&self, masked: bool) -> String {
        let defaults = Cli::parse_from(["pepe", "x"]);
        let mut parts = vec!["pepe".to_string()];
        match &self.command {
            Some(Command::Ramp(_)) => parts.push("ramp".into()),
            Some(Command::Api(_)) => parts.push("api".into()),
            Some(Command::Replay(_)) => parts.push("replay".into()),
            Some(Command::Flow(_)) => parts.push("flow".into()),
            Some(Command::Ping(_)) => parts.push("ping".into()),
            _ => {}
        }
        // `--slack` and `--slack=#channel`, added once `flag` is done
        let mut slack_flags: Vec<String> = Vec::new();
        let mut flag = |name: &str, value: &str| {
            parts.push(name.to_string());
            parts.push(shell_quote(value));
        };
        // A ramp sets its own load; the other modes take -c and -n or -z
        let ramp = match &self.command {
            Some(Command::Ramp(ramp)) => Some(ramp),
            _ => None,
        };
        // A ping has no concurrency, and may have several targets
        let ping = matches!(self.command, Some(Command::Ping(_)));
        let mut target = self.url.clone();
        let mut more_targets: Vec<String> = Vec::new();
        // Switches with no value, added once `flag` is done with `parts`
        let mut switches: Vec<&str> = Vec::new();
        match &self.command {
            Some(Command::Ping(ping)) => {
                let usual = PingArgs::usual();
                let mut targets = ping.targets.iter();
                target = targets.next().cloned().unwrap_or_default();
                more_targets = targets.cloned().collect();
                if ping.every != usual.every {
                    flag("--every", &ping.every);
                }
                for name in &ping.name {
                    flag("--name", name);
                }
                if let Some(slo) = &ping.slo {
                    flag("--slo", slo);
                }
                if ping.port != usual.port {
                    flag("--port", &ping.port.to_string());
                }
                if let Some(interface) = &ping.interface {
                    flag("--interface", interface);
                }
                for (on, name) in [
                    (ping.tcp, "--tcp"),
                    (ping.cmd, "--cmd"),
                    (ping.keep_alive, "--keep-alive"),
                    (ping.http1, "--http1"),
                    (ping.ipv4, "-4"),
                    (ping.ipv6, "-6"),
                    (ping.all_ips, "--all-ips"),
                ] {
                    if on {
                        switches.push(name);
                    }
                }
            }
            Some(Command::Ramp(ramp)) => {
                let usual = RampArgs::default();
                for (name, value, default) in [
                    ("--from", ramp.from, usual.from),
                    ("--to", ramp.to, usual.to),
                    ("--step", ramp.step, usual.step),
                ] {
                    if value != default {
                        flag(name, &value.to_string());
                    }
                }
                if ramp.every != usual.every {
                    flag("--every", &ramp.every);
                }
                for condition in &ramp.until {
                    flag("--until", condition);
                }
            }
            Some(Command::Api(api)) => {
                // A spec pasted on the setup screen has no name to give
                target = if crate::openapi::is_document(&api.spec) {
                    "<the pasted spec>".into()
                } else {
                    api.spec.clone()
                };
                if let Some(server) = &api.server {
                    flag("--server", server);
                }
            }
            Some(Command::Replay(replay)) => {
                target = replay.log.display().to_string();
                if let Some(base) = &replay.base_url {
                    flag("--base-url", base);
                }
                if replay.rows != 20 {
                    flag("--rows", &replay.rows.to_string());
                }
            }
            Some(Command::Flow(flow)) => target = flow.file.display().to_string(),
            _ => {}
        }
        if ramp.is_none() {
            if self.concurrency != defaults.concurrency && !ping {
                flag("-c", &self.concurrency.to_string());
            }
            match &self.duration {
                Some(duration) => flag("-z", duration),
                None if self.number != defaults.number => flag("-n", &self.number.to_string()),
                None => {}
            }
        }
        if let Some(threads) = self.threads {
            flag("--threads", &threads.to_string());
        }
        if let Some(warmup) = &self.warmup {
            flag("--warmup", warmup);
        }
        if let Some(path) = &self.vars {
            flag("--vars", &path.display().to_string());
        }
        for path in &self.export {
            flag("--export", &path.display().to_string());
        }
        // A hook a file says more of than a URL is the file's to say
        let hooks = self.hooks();
        if hooks.iter().all(|h| {
            h.headers.is_empty()
                && h.body.is_none()
                && h.every.is_none()
                && h.mention.is_none()
                && h.format == crate::webhook::Format::Auto
        }) {
            // `slack:` hooks as --slack; the rest as --webhook
            let (slack, hooks): (Vec<_>, Vec<_>) =
                hooks.iter().partition(|h| h.is_slack() && h.on.is_empty());
            for hook in &hooks {
                if masked {
                    flag("--webhook", &hook.masked_url());
                } else {
                    flag("--webhook", &hook.url);
                }
            }
            for hook in slack {
                match hook.url.trim_start().trim_start_matches("slack:") {
                    "" => slack_flags.push("--slack".into()),
                    channel => slack_flags.push(format!("--slack={}", shell_quote(channel))),
                }
            }
            if let Some(first) = hooks.first() {
                if !first.on.is_empty() && hooks.iter().all(|h| h.on == first.on) {
                    let on: Vec<&str> = first.on.iter().map(|e| e.name()).collect();
                    flag("--webhook-on", &on.join(","));
                }
            }
        }
        if let Some(rate) = self.rate {
            flag("--rate", &rate.to_string());
        }
        if let Some(path) = &self.snapshot {
            flag("--snapshot", &path.display().to_string());
        }
        if self.method != defaults.method {
            flag("-m", &self.method);
        }
        for header in &self.headers {
            flag("-H", header);
        }
        if let Some(body) = self.body() {
            flag("-d", &String::from_utf8_lossy(&body));
        }
        if self.timeout != defaults.timeout {
            flag("-t", &self.timeout.to_string());
        }
        if self.user_agent != defaults.user_agent {
            flag("-u", &self.user_agent);
        }
        if let Some(proxy) = &self.proxy {
            flag("-p", proxy);
        }
        let include_writes = matches!(&self.command, Some(Command::Replay(r)) if r.include_writes);
        for (on, name) in [
            (include_writes, "--include-writes"),
            (self.insecure, "-k"),
            (self.disable_redirects, "--disable-redirects"),
            (self.disable_keepalive, "--disable-keepalive"),
            (self.disable_compression, "--disable-compression"),
        ] {
            if on {
                parts.push(name.to_string());
            }
        }
        parts.extend(switches.iter().map(|s| s.to_string()));
        parts.extend(slack_flags);
        parts.push(shell_quote(&target));
        parts.extend(more_targets.iter().map(|t| shell_quote(t)));
        parts.join(" ")
    }

    /// `GET https://example.com/`: what the metrics call the run
    pub fn target_label(&self) -> String {
        format!("{} {}", self.method, self.url)
    }

    /// The request body, from -d or a curl command
    pub fn body(&self) -> Option<Vec<u8>> {
        self.body_bytes
            .clone()
            .or_else(|| self.body.as_ref().map(|b| b.clone().into_bytes()))
    }

    pub fn settings(&self) -> RequestSettings {
        RequestSettings {
            insecure: self.insecure,
            user_agent: self.user_agent.clone(),
            timeout: self.timeout,
            proxy: self.proxy.clone(),
            disable_compression: self.disable_compression,
            disable_keepalive: self.disable_keepalive,
            disable_redirects: self.disable_redirects,
            // Paced workers idle between requests; closing their
            // connections meanwhile would reopen them in waves
            idle_connections: match self.rate {
                Some(_) => (self.concurrency as usize).max(crate::request::IDLE_CONNECTIONS),
                None => crate::request::IDLE_CONNECTIONS,
            },
            allowed_hosts: crate::guard::Guard::of(self).hosts,
        }
    }

    /// Test length for `--duration` runs (already validated by `validate`)
    /// `--warmup`, parsed (already validated by `validate`)
    pub fn warmup(&self) -> Option<std::time::Duration> {
        self.warmup
            .as_deref()
            .and_then(|d| Self::parse_duration(d).ok())
            .map(std::time::Duration::from_millis)
            .filter(|d| !d.is_zero())
    }

    pub fn run_duration(&self) -> Option<std::time::Duration> {
        self.duration
            .as_deref()
            .and_then(|d| Self::parse_duration(d).ok())
            .map(std::time::Duration::from_millis)
    }

    /// The request; with `--vars`, the first row's
    pub fn request(&self) -> Result<Request, PepeError> {
        match self.rows()? {
            Some((vars, fill)) => self.row_request(&vars, &fill, 0),
            None => Request::new(
                self.url.clone(),
                self.method.clone(),
                self.body(),
                &self.headers,
                self.settings(),
            ),
        }
    }

    /// `--vars`: the file, and the request with holes its rows fill
    pub fn rows(&self) -> Result<Option<(Vars, Fill)>, PepeError> {
        let Some(path) = &self.vars else {
            return Ok(None);
        };
        let vars = Vars::load(path).map_err(PepeError::Vars)?;
        Ok(Some((vars, self.fill())))
    }

    /// The request of row `index`
    pub fn row_request(
        &self,
        vars: &Vars,
        fill: &Fill,
        index: usize,
    ) -> Result<Request, PepeError> {
        let row = fill.row(vars, index);
        Request::new(
            row.url,
            self.method.clone(),
            row.body,
            &row.headers,
            self.settings(),
        )
        .map_err(|e| {
            PepeError::Vars(format!(
                "--vars {} row {}: {e}",
                vars.path.display(),
                index + 1
            ))
        })
    }

    /// The run's webhooks: those the screens' webhook page set, else those
    /// of `--webhook`, else of a file
    pub fn hooks(&self) -> Vec<crate::webhook::Webhook> {
        if let Some(edited) = crate::webhook::edited() {
            return edited;
        }
        let mut hooks = self.flag_hooks();
        if let Some(channel) = &self.slack {
            let url = format!("slack:{channel}");
            // One Slack message a run: --slack takes the place of the file's
            // Slack hook, keeping what it says beside the channel
            match hooks.iter_mut().find(|h| h.is_slack()) {
                Some(hook) => hook.url = url,
                None => hooks.push(crate::webhook::Webhook::to(&url)),
            }
            hooks.retain({
                let mut seen = false;
                move |h| !h.is_slack() || !std::mem::replace(&mut seen, true)
            });
        }
        hooks
    }

    fn flag_hooks(&self) -> Vec<crate::webhook::Webhook> {
        if self.webhook.is_empty() {
            return self.webhooks.clone();
        }
        let on = self
            .webhook_on
            .as_deref()
            .and_then(|t| crate::webhook::Event::parse_list(t).ok())
            .unwrap_or_default();
        self.webhook
            .iter()
            .map(|url| crate::webhook::Webhook {
                on: on.clone(),
                ..crate::webhook::Webhook::to(url)
            })
            .collect()
    }

    /// The request as written, holes and all
    pub fn fill(&self) -> Fill {
        Fill::of(&self.url, &self.headers, self.body())
    }

    /// Every URL a run sends to, each once: the one, or with `--vars`
    /// each row's
    pub fn target_urls(&self) -> Result<Vec<String>, String> {
        let Some(path) = &self.vars else {
            return Ok(vec![self.url.clone()]);
        };
        let vars = Vars::load(path)?;
        let fill = self.fill();
        let mut urls: Vec<String> = (0..vars.rows.len())
            .map(|i| fill.row(&vars, i).url)
            .collect();
        urls.sort_unstable();
        urls.dedup();
        Ok(urls)
    }

    /// `--vars` with a run or a ramp: the file is read, every hole is one
    /// of its columns, and the first row makes a URL
    fn check_vars(&self, path: &std::path::Path) -> Result<(), Error> {
        let fail = |e: String| Error::raw(clap::error::ErrorKind::ValueValidation, e);
        match &self.command {
            None | Some(Command::Ramp(_)) | Some(Command::Flow(_)) => {}
            Some(_) => {
                return Err(fail(
                    "--vars fills the {{holes}} of a run, a ramp or a flow".into(),
                ))
            }
        }
        // A flow's holes are checked as the flow is read
        if matches!(self.command, Some(Command::Flow(_))) || self.url.is_empty() {
            return Ok(());
        }
        let vars = Vars::load(path).map_err(fail)?;
        let fill = self.fill();
        let holes = fill.holes();
        if holes.is_empty() {
            return Err(fail(format!(
                "--vars {}: the request has no {{{{holes}}}} to fill; put one where a value goes, as in https://api.example.com/users/{{{{{}}}}}",
                path.display(),
                vars.names[0]
            )));
        }
        vars.check(holes)
            .map_err(|e| fail(format!("--vars: {e}")))?;
        let url = fill.row(&vars, 0).url;
        reqwest::Url::parse(&url).map_err(|e| {
            fail(format!(
                "--vars {} row 1: {}",
                path.display(),
                Self::url_trouble(&url, e)
            ))
        })?;
        Ok(())
    }
}

/// Quote a value for a POSIX shell, only when it needs it
fn shell_quote(value: &str) -> String {
    let plain = !value.is_empty()
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_@%+=:,./-".contains(c));
    if plain {
        value.to_string()
    } else {
        format!("'{}'", value.replace('\'', r"'\''"))
    }
}

/// The curl command to parse: the words after `--`, a single quoted
/// string, `@file`, or the command piped on stdin
fn curl_request(args: &[String]) -> Result<curl::CurlRequest, String> {
    // No words after `--` leaves clap's default of one empty string
    let args = if args == [""] { &[][..] } else { args };
    match args {
        [] => {
            use std::io::{IsTerminal, Read};
            if std::io::stdin().is_terminal() {
                return Err(
                    "give the command after --, as @file, or pipe it in: pbpaste | pepe --curl"
                        .into(),
                );
            }
            let mut command = String::new();
            std::io::stdin()
                .read_to_string(&mut command)
                .map_err(|e| format!("can't read stdin: {e}"))?;
            curl::parse_command(&command)
        }
        [one] if one.starts_with('@') => {
            let path = &one[1..];
            let command =
                std::fs::read_to_string(path).map_err(|e| format!("can't read {path}: {e}"))?;
            curl::parse_command(&command)
        }
        // The whole command quoted as one argument
        [one] if one.contains(char::is_whitespace) => curl::parse_command(one),
        words => curl::parse_words(words),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_durations() {
        assert_eq!(Cli::parse_duration("10s").unwrap(), 10_000);
        assert_eq!(Cli::parse_duration(" 5 min ").unwrap(), 300_000);
        assert_eq!(Cli::parse_duration("2H").unwrap(), 7_200_000);
    }

    #[test]
    fn rejects_bad_durations() {
        for bad in ["10", "s", "0s", "-1s", "10x", "1.5s", "99999999999999999h"] {
            assert!(
                Cli::parse_duration(bad).is_err(),
                "{bad} should be rejected"
            );
        }
    }

    #[test]
    fn validate_rejects_invalid_header() {
        let mut cli = Cli::parse_from(["pepe", "-c", "1", "-H", "not a header", "http://x"]);
        assert!(cli.validate().is_err());
    }

    #[test]
    fn duration_mode_ignores_request_count() {
        let mut cli = Cli::parse_from(["pepe", "-z", "5s", "-n", "1", "-c", "8", "http://x"]);
        assert!(cli.validate().is_ok());
        assert_eq!(cli.run_duration(), Some(std::time::Duration::from_secs(5)));
    }

    /// Accept one request and return exactly what arrived on the wire
    async fn capture(listener: tokio::net::TcpListener) -> Vec<u8> {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let (mut sock, _) = listener.accept().await.unwrap();
        let mut raw = Vec::new();
        let mut buf = [0u8; 8192];
        loop {
            let n = sock.read(&mut buf).await.unwrap();
            raw.extend_from_slice(&buf[..n]);
            let Some(end) = raw.windows(4).position(|w| w == b"\r\n\r\n") else {
                continue;
            };
            let head = String::from_utf8_lossy(&raw[..end]).to_lowercase();
            let length = head
                .lines()
                .find_map(|l| l.strip_prefix("content-length: "))
                .map_or(0, |v| v.trim().parse().unwrap());
            if raw.len() >= end + 4 + length {
                break;
            }
        }
        let _ = sock
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
            .await;
        raw
    }

    /// Run a curl command through pepe once and return what the server got
    async fn sent_by_curl(command: impl Fn(&str) -> Vec<String>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let server = tokio::spawn(capture(listener));
        let mut argv = vec![
            "pepe".to_string(),
            "-n".into(),
            "1".into(),
            "-c".into(),
            "1".into(),
            "--curl".into(),
            "--".into(),
        ];
        argv.extend(command(&addr));
        let mut cli = Cli::parse_from(argv);
        cli.validate().unwrap();
        let request = cli.request().unwrap();
        let client = request.build_client().unwrap();
        let mut load = crate::load::start(client, request, 1, crate::load::Plan::Count(1), false);
        while load.recv().await.is_some() {}
        String::from_utf8_lossy(&server.await.unwrap()).into_owned()
    }

    #[tokio::test]
    async fn curl_words_after_double_dash_reach_the_server() {
        let raw = sent_by_curl(|addr| {
            [
                "curl",
                "-X",
                "PATCH",
                &format!("{addr}/items/7?x=1"),
                "-H",
                "Content-Type: application/json",
                "-H",
                "User-Agent: my-agent/1.0",
                "-u",
                "ada:secret",
                "--data-raw",
                r#"{"name":"it's \"quoted\" $HOME"}"#,
            ]
            .map(String::from)
            .to_vec()
        })
        .await;
        assert!(raw.starts_with("PATCH /items/7?x=1 HTTP/1.1\r\n"), "{raw}");
        let lower = raw.to_lowercase();
        assert!(
            lower.contains("content-type: application/json\r\n"),
            "{raw}"
        );
        assert!(
            lower.contains("user-agent: my-agent/1.0\r\n"),
            "-H wins over the default: {raw}"
        );
        assert!(!lower.contains("user-agent: pepe/"), "{raw}");
        assert!(
            lower.contains("authorization: basic ywrhonnly3jlda==\r\n"),
            "{raw}"
        );
        assert!(
            raw.ends_with(r#"{"name":"it's \"quoted\" $HOME"}"#),
            "{raw}"
        );
    }

    #[tokio::test]
    async fn curl_as_one_string_with_a_binary_upload() {
        let dir = std::env::temp_dir().join(format!("pepe-cli-curl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("blob.bin");
        std::fs::write(&file, [0u8, 0xff, 0xfe, b'\n', 7]).unwrap();
        let file = file.display().to_string();
        let raw = sent_by_curl(|addr| {
            vec![format!(
                "curl -sS {addr}/upload \\\n  -F 'meta={{\"a\":1}};type=application/json' \\\n  -F 'file=@{file}'"
            )]
        })
        .await;
        std::fs::remove_dir_all(dir).ok();
        assert!(raw.starts_with("POST /upload HTTP/1.1\r\n"), "{raw}");
        assert!(
            raw.to_lowercase()
                .contains("content-type: multipart/form-data; boundary="),
            "{raw}"
        );
        assert!(
            raw.contains("name=\"meta\"\r\nContent-Type: application/json\r\n\r\n{\"a\":1}\r\n"),
            "{raw}"
        );
        assert!(
            raw.contains("filename=\"blob.bin\"\r\nContent-Type: application/octet-stream"),
            "{raw}"
        );
    }

    #[test]
    fn a_pings_command_line_round_trips() {
        let argv = [
            "pepe",
            "ping",
            "--every",
            "2s",
            "--slo",
            "total=500",
            "--keep-alive",
            "-n",
            "30",
            "--name",
            "a",
            "--name",
            "b",
            "https://a.io",
            "https://b.io",
        ];
        let cli = Cli::parse_from(argv);
        let line = cli.command_line();
        assert!(line.starts_with("pepe ping "), "{line}");
        assert!(!line.contains("-c "), "{line}");
        let again = Cli::parse_from(crate::curl::split(&line).unwrap());
        let (Some(Command::Ping(a)), Some(Command::Ping(b))) = (&cli.command, &again.command)
        else {
            panic!("{line}");
        };
        assert_eq!(a, b, "{line}");
        assert_eq!(again.number, 30, "{line}");
    }

    #[test]
    fn command_line_round_trips_through_the_parser() {
        let argv = [
            "pepe",
            "-c",
            "50",
            "-z",
            "30s",
            "-m",
            "POST",
            "-H",
            "Authorization: Bearer it's",
            "-H",
            "Accept: */*",
            "-d",
            r#"{"a": 1}"#,
            "-t",
            "5",
            "-k",
            "--disable-redirects",
            "https://x.io/items?a=1&b=2",
        ];
        let cli = Cli::parse_from(argv);
        let line = cli.command_line();
        assert_eq!(
            line,
            r#"pepe -c 50 -z 30s -m POST -H 'Authorization: Bearer it'\''s' -H 'Accept: */*' -d '{"a": 1}' -t 5 -k --disable-redirects 'https://x.io/items?a=1&b=2'"#
        );
        // What it prints parses back to the same settings
        let again = Cli::parse_from(crate::curl::split(&line).unwrap());
        assert_eq!(again.command_line(), line);
        assert_eq!(
            (again.headers, again.body, again.url),
            (cli.headers, cli.body, cli.url)
        );

        // Defaults are left out
        assert_eq!(
            Cli::parse_from(["pepe", "http://x.io"]).command_line(),
            "pepe http://x.io"
        );
    }

    #[test]
    fn the_cli_definition_is_consistent() {
        use clap::CommandFactory;
        Cli::command().debug_assert();
    }

    #[test]
    fn a_url_that_isnt_one_is_told_what_it_needs() {
        let trouble = |word: &str| {
            let mut args = Cli::parse_from(["pepe", word]);
            args.validate().unwrap_err().to_string()
        };
        let bare = trouble("foo");
        assert!(
            bare.contains("\"foo\" isn't a URL, nor a command"),
            "{bare}"
        );
        assert!(
            bare.contains("ping, ramp, compare, rerun, history, api, flow"),
            "{bare}"
        );
        assert!(bare.contains("pepe self-update"), "{bare}");
        let host = trouble("example.com/x");
        assert!(host.contains("has no scheme"), "{host}");
        assert!(host.contains("pepe https://example.com/x"), "{host}");
        let odd = trouble("http://exa mple.com");
        assert!(
            odd.starts_with("error: Invalid URL \"http://exa mple.com\""),
            "{odd}"
        );
        assert!(Cli::parse_from(["pepe", "https://example.com"])
            .validate()
            .is_ok());
    }

    #[test]
    fn api_subcommand_takes_shared_flags_on_either_side() {
        for argv in [
            vec![
                "pepe",
                "api",
                "spec.yaml",
                "-c",
                "5",
                "-z",
                "10s",
                "--auth",
                "bearer:t",
                "--set",
                "id=1,2",
            ],
            vec![
                "pepe",
                "-c",
                "5",
                "-z",
                "10s",
                "api",
                "spec.yaml",
                "--auth",
                "bearer:t",
                "--set",
                "id=1,2",
            ],
        ] {
            let mut cli = Cli::parse_from(argv);
            cli.validate().unwrap();
            assert_eq!((cli.concurrency, cli.duration.as_deref()), (5, Some("10s")));
            let Some(Command::Api(api)) = &cli.command else {
                panic!("api subcommand not parsed")
            };
            assert_eq!(
                (api.spec.as_str(), api.auth.len(), api.set.len()),
                ("spec.yaml", 1, 1)
            );
        }
        // The default mode is unchanged
        let cli = Cli::parse_from(["pepe", "-c", "5", "http://x.io"]);
        assert!(cli.command.is_none() && cli.url == "http://x.io");
    }

    #[test]
    fn curl_settings_carry_over() {
        let mut cli = Cli::parse_from([
            "pepe",
            "-n",
            "1",
            "-c",
            "1",
            "--curl",
            "--",
            "curl",
            "-k",
            "-m",
            "7",
            "-x",
            "proxy:8080",
            "--no-keepalive",
            "https://x.io",
        ]);
        cli.validate().unwrap();
        assert!(cli.insecure && cli.disable_keepalive);
        assert_eq!(cli.timeout, 7);
        assert_eq!(cli.proxy.as_deref(), Some("http://proxy:8080"));
        assert!(
            cli.disable_redirects,
            "curl doesn't follow redirects without -L"
        );

        let mut cli = Cli::parse_from([
            "pepe", "-n", "1", "-c", "1", "--curl", "--", "curl", "-L", "x.io",
        ]);
        cli.validate().unwrap();
        assert!(!cli.disable_redirects);
        assert_eq!(cli.url, "http://x.io");
    }

    #[test]
    fn bad_curl_commands_are_rejected_with_the_reason() {
        let mut cli = Cli::parse_from([
            "pepe",
            "-n",
            "1",
            "-c",
            "1",
            "--curl",
            "--",
            "curl",
            "--nope",
            "https://x.io",
        ]);
        let err = cli.validate().unwrap_err().to_string();
        assert!(err.contains("unknown curl option --nope"), "{err}");
    }
}
