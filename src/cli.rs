use clap::{ArgAction::HelpLong, Args, Error, Parser, Subcommand};
use reqwest::Proxy;

use crate::curl;
use crate::request::{parse_header, Request, RequestSettings};
use crate::utils::{default_user_agent, num_of_cores, version};
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

    /// Threads sending requests (default 1). One sends about 100k requests
    /// a second; the dashboard says when it is the limit
    #[arg(long, global = true, value_parser = clap::value_parser!(u32).range(1..))]
    pub threads: Option<u32>,

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

    /// Open the setup screen to review or change the settings before
    /// starting (it opens by itself when no URL is given)
    #[arg(short = 'i', long, global = true)]
    pub setup: bool,

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

#[derive(Subcommand, Debug, Clone)]
pub enum Command {
    /// Update pepe to the latest release, or say what's new in it
    SelfUpdate(SelfUpdateArgs),
    /// Load-test every endpoint of an OpenAPI spec
    Api(ApiArgs),
    /// Raise the load step by step to find where the target stops keeping up
    Ramp(RampArgs),
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
    /// The OpenAPI spec: a file or URL, JSON or YAML
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
        // A ramp sets its own concurrency and runs for as long as its steps
        let ramp = matches!(self.command, Some(Command::Ramp(_)));
        if self.concurrency > self.number && self.duration.is_none() && !ramp {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!(
                    "Concurrency cannot be greater than the number of requests. -c {} -n {}",
                    self.concurrency, self.number
                ),
            ));
        }

        if !self.curl && self.url.is_empty() && self.command.is_none() {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "URL is required",
            ));
        }

        if self.timeout == 0 || self.timeout > 120 {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "Timeout must be between 1 and 120 seconds",
            ));
        }

        // Validate and parse duration if provided
        if let Some(ref duration_str) = self.duration {
            Self::parse_duration(duration_str)?;
        }

        if self.curl {
            let request = curl_request(&self.args).map_err(|e| {
                Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("Invalid curl command: {e}"),
                )
            })?;
            // stderr, so `--json` output on stdout stays valid JSON
            eprintln!("curl: {} {}", request.method, request.url);
            for note in &request.notes {
                eprintln!("  note: {note}");
            }
            self.apply_curl(request);
        }

        for header in &self.headers {
            parse_header(header)
                .map_err(|e| Error::raw(clap::error::ErrorKind::ValueValidation, e))?;
        }

        let method = reqwest::Method::from_bytes(self.method.as_bytes());
        if method.is_err() {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!("Invalid method: {}", self.method),
            ));
        }

        if let Some(proxy) = &self.proxy {
            if proxy.starts_with("socks4") {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    "Socks4 proxy is not supported by reqwest.",
                ));
            }
            if Proxy::all(proxy).is_err() {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("Invalid proxy URL: {}", proxy),
                ));
            }
        }
        // Parsed once more when the run starts; checked here for a clean
        // message rather than a dashboard full of failures
        if !self.url.is_empty() {
            if let Err(e) = reqwest::Url::parse(&self.url) {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("Invalid URL {:?}: {e}", self.url),
                ));
            }
        }
        Ok(())
    }

    /// Parse duration string like "10s", "5m", "2h" into milliseconds
    pub fn parse_duration(duration_str: &str) -> Result<u64, Error> {
        let duration_str = duration_str.trim().to_lowercase();

        let (num_str, unit) = if let Some(idx) = duration_str.find(|c: char| c.is_alphabetic()) {
            duration_str.split_at(idx)
        } else {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "Duration must have a unit (s, m, h). Examples: 10s, 5m, 2h",
            ));
        };

        let num: u64 = num_str.trim().parse().map_err(|_| {
            Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!("Invalid duration number: {}", num_str),
            )
        })?;

        let unit_ms: u64 = match unit {
            "s" | "sec" | "second" | "seconds" => 1000,
            "m" | "min" | "minute" | "minutes" => 60 * 1000,
            "h" | "hour" | "hours" => 60 * 60 * 1000,
            _ => {
                return Err(Error::raw(
                    clap::error::ErrorKind::ValueValidation,
                    format!("Invalid duration unit: {}. Valid units: s, m, h", unit),
                ))
            }
        };
        let milliseconds = num.checked_mul(unit_ms).ok_or_else(|| {
            Error::raw(
                clap::error::ErrorKind::ValueValidation,
                format!("Duration is too long: {}", duration_str),
            )
        })?;

        if milliseconds == 0 {
            return Err(Error::raw(
                clap::error::ErrorKind::ValueValidation,
                "Duration must be greater than 0",
            ));
        }

        Ok(milliseconds)
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

    /// The pepe command that reproduces these settings, leaving out
    /// everything still at its default
    pub fn command_line(&self) -> String {
        let defaults = Cli::parse_from(["pepe", "x"]);
        let mut parts = vec!["pepe".to_string()];
        match &self.command {
            Some(Command::Ramp(_)) => parts.push("ramp".into()),
            Some(Command::Api(_)) => parts.push("api".into()),
            _ => {}
        }
        let mut flag = |name: &str, value: &str| {
            parts.push(name.to_string());
            parts.push(shell_quote(value));
        };
        // A ramp sets its own load; the other modes take -c and -n or -z
        let ramp = match &self.command {
            Some(Command::Ramp(ramp)) => Some(ramp),
            _ => None,
        };
        let mut target = self.url.clone();
        match &self.command {
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
                target = api.spec.clone();
                if let Some(server) = &api.server {
                    flag("--server", server);
                }
            }
            _ => {}
        }
        if ramp.is_none() {
            if self.concurrency != defaults.concurrency {
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
        for (on, name) in [
            (self.insecure, "-k"),
            (self.disable_redirects, "--disable-redirects"),
            (self.disable_keepalive, "--disable-keepalive"),
            (self.disable_compression, "--disable-compression"),
        ] {
            if on {
                parts.push(name.to_string());
            }
        }
        parts.push(shell_quote(&target));
        parts.join(" ")
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
        }
    }

    /// Test length for `--duration` runs (already validated by `validate`)
    pub fn run_duration(&self) -> Option<std::time::Duration> {
        self.duration
            .as_deref()
            .and_then(|d| Self::parse_duration(d).ok())
            .map(std::time::Duration::from_millis)
    }

    pub fn request(&self) -> Result<Request, PepeError> {
        Request::new(
            self.url.clone(),
            self.method.clone(),
            self.body(),
            &self.headers,
            self.settings(),
        )
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
        let mut load =
            crate::load::start(vec![client], request, 1, crate::load::Plan::Count(1), false);
        while load.rx.recv().await.is_some() {}
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
