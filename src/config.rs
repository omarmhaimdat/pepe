//! `pepe.toml`: a run's settings in a file, so a load test can live next
//! to the code it tests and be run with `pepe` alone. Flags on the command
//! line win over the file; the file wins over pepe's defaults.

use std::path::{Path, PathBuf};

use clap::parser::{ArgMatches, ValueSource};
use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::cli::{Cli, Command, RampArgs};
use crate::utils::default_user_agent;

/// The file read when `--config` names none
pub const DEFAULT_PATH: &str = "pepe.toml";

/// Everything a `pepe.toml` can say. Keys are the long flags' names, with
/// the on/off ones said the positive way (`keep-alive = false`).
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    /// `Name: value` lines, as `-H` takes them
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// `-n`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requests: Option<u32>,
    /// `-z`, e.g. "30s"; wins over `requests` when both are set
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concurrency: Option<u32>,
    /// Seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,
    /// A number, or "auto"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub threads: Option<crate::load::ThreadCount>,
    /// `--rate`: requests started per second
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate: Option<f64>,
    /// `--warmup`, e.g. "5s"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warmup: Option<String>,
    /// `--trace-header`: the response header holding the request id
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_agent: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub proxy: Option<String>,
    /// Accept invalid TLS certificates (`-k`)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insecure: Option<bool>,
    /// false: `--disable-compression`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compression: Option<bool>,
    /// false: `--disable-keepalive`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<bool>,
    /// false: `--disable-redirects`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redirects: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot: Option<PathBuf>,
    /// `--allow-host`: the hosts a run may be pointed at
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_host: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_requests: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_rate: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_concurrency: Option<u32>,
    /// Defaults for `pepe ramp`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ramp: Option<RampConfig>,
    /// Defaults for `pepe api`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<ApiConfig>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RampConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub step: Option<u32>,
    /// How long each step is held, e.g. "10s"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    /// Stop conditions, e.g. ["p99 > 500ms", "errors > 1%"]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<Vec<String>>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct ApiConfig {
    /// The OpenAPI spec: a file or URL
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spec: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tag: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub only: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skip: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub set: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_writes: Option<bool>,
}

/// The config to read: the one `--config` names, which has to exist, or
/// `pepe.toml` in the current directory, which may not
pub fn load(named: Option<&Path>) -> Result<Option<(PathBuf, Config)>, String> {
    let path = match named {
        Some(path) => path.to_path_buf(),
        None => {
            let path = PathBuf::from(DEFAULT_PATH);
            if !path.is_file() {
                return Ok(None);
            }
            path
        }
    };
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("couldn't read {}: {e}", path.display()))?;
    let config = parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Some((path, config)))
}

pub fn parse(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|e| e.message().to_string() + &where_in(&e))
}

/// " (line 4)" for an error that knows where it is
fn where_in(e: &toml::de::Error) -> String {
    match e.span() {
        Some(span) => format!(" (at character {})", span.start + 1),
        None => String::new(),
    }
}

impl Config {
    /// Fill in what the command line left unsaid. `matches` says which
    /// flags were typed; those keep their value, everything else the file
    /// sets takes the file's.
    pub fn apply(&self, cli: &mut Cli, matches: &ArgMatches) {
        let sub = matches.subcommand().map(|(_, m)| m);
        let typed = |id: &str| {
            [Some(matches), sub].into_iter().flatten().any(|m| {
                m.try_contains_id(id).unwrap_or(false)
                    && m.value_source(id) == Some(ValueSource::CommandLine)
            })
        };
        macro_rules! take {
            ($field:ident, $id:literal) => {
                if let (Some(value), false) = (&self.$field, typed($id)) {
                    cli.$field = value.clone();
                }
            };
        }
        if let (Some(url), true) = (&self.url, cli.url.is_empty()) {
            cli.url = url.clone();
        }
        take!(method, "method");
        take!(headers, "headers");
        take!(concurrency, "concurrency");
        take!(timeout, "timeout");
        take!(user_agent, "user_agent");
        if let (Some(body), false, None) = (&self.body, typed("body"), &cli.body_bytes) {
            if cli.body.is_none() {
                cli.body = Some(body.clone());
            }
        }
        if !typed("number") && !typed("duration") {
            if let Some(n) = self.requests {
                cli.number = n;
            }
            if let Some(duration) = &self.duration {
                cli.duration = Some(duration.clone());
            }
        }
        if cli.threads.is_none() {
            cli.threads = self.threads;
        }
        if cli.rate.is_none() {
            cli.rate = self.rate;
        }
        if cli.warmup.is_none() {
            cli.warmup = self.warmup.clone();
        }
        if cli.trace_header.is_none() {
            cli.trace_header = self.trace_header.clone();
        }
        if cli.proxy.is_none() {
            cli.proxy = self.proxy.clone();
        }
        if cli.snapshot.is_none() {
            cli.snapshot = self.snapshot.clone();
        }
        // The guardrails: the file's hold unless the command line names
        // its own; a cap in the file can't be loosened by leaving it out
        if let (Some(hosts), true) = (&self.allow_host, cli.allow_host.is_empty()) {
            cli.allow_host = hosts.clone();
        }
        if cli.max_requests.is_none() {
            cli.max_requests = self.max_requests;
        }
        if cli.max_rate.is_none() {
            cli.max_rate = self.max_rate;
        }
        if cli.max_concurrency.is_none() {
            cli.max_concurrency = self.max_concurrency;
        }
        // Flags can only be switched on from the command line, so an "on"
        // there always wins, and "off" in the file can't undo it
        cli.insecure |= self.insecure.unwrap_or(false);
        cli.disable_compression |= self.compression == Some(false);
        cli.disable_keepalive |= self.keep_alive == Some(false);
        cli.disable_redirects |= self.redirects == Some(false);

        match (&mut cli.command, &self.ramp, &self.api) {
            (Some(Command::Ramp(ramp)), Some(file), _) => {
                if ramp.url.is_empty() {
                    ramp.url = self.url.clone().unwrap_or_default();
                }
                if let (Some(v), false) = (file.from, typed("from")) {
                    ramp.from = v;
                }
                if let (Some(v), false) = (file.to, typed("to")) {
                    ramp.to = v;
                }
                if let (Some(v), false) = (file.step, typed("step")) {
                    ramp.step = v;
                }
                if let (Some(v), false) = (&file.every, typed("every")) {
                    ramp.every = v.clone();
                }
                if let (Some(v), true) = (&file.until, ramp.until.is_empty()) {
                    ramp.until = v.clone();
                }
            }
            (Some(Command::Ramp(ramp)), None, _) => {
                if ramp.url.is_empty() {
                    ramp.url = self.url.clone().unwrap_or_default();
                }
            }
            (Some(Command::Api(api)), _, Some(file)) => {
                if api.spec.is_empty() {
                    api.spec = file.spec.clone().unwrap_or_default();
                }
                if api.server.is_none() {
                    api.server = file.server.clone();
                }
                for (ours, theirs) in [
                    (&mut api.auth, &file.auth),
                    (&mut api.tag, &file.tag),
                    (&mut api.only, &file.only),
                    (&mut api.skip, &file.skip),
                    (&mut api.set, &file.set),
                ] {
                    if let (true, Some(values)) = (ours.is_empty(), theirs) {
                        *ours = values.clone();
                    }
                }
                api.all |= file.all.unwrap_or(false);
                api.include_writes |= file.include_writes.unwrap_or(false);
            }
            _ => {}
        }
    }

    /// The settings as they stand, in a form that reads back the same.
    /// Defaults are left out, so the file says only what was chosen.
    pub fn from_cli(cli: &Cli) -> Config {
        let defaults = Cli::parse_from(["pepe", "x"]);
        let differs = |a: &str, b: &str| (a != b).then(|| a.to_string());
        let mut config = Config {
            url: (!cli.url.is_empty()).then(|| cli.url.clone()),
            method: differs(&cli.method, &defaults.method),
            headers: (!cli.headers.is_empty()).then(|| cli.headers.clone()),
            body: cli.body.clone(),
            requests: (cli.duration.is_none() && cli.number != defaults.number)
                .then_some(cli.number),
            duration: cli.duration.clone(),
            concurrency: Some(cli.concurrency),
            timeout: (cli.timeout != defaults.timeout).then_some(cli.timeout),
            threads: cli.threads,
            rate: cli.rate,
            warmup: cli.warmup.clone(),
            trace_header: cli.trace_header.clone(),
            user_agent: differs(&cli.user_agent, &default_user_agent()),
            proxy: cli.proxy.clone(),
            insecure: cli.insecure.then_some(true),
            compression: cli.disable_compression.then_some(false),
            keep_alive: cli.disable_keepalive.then_some(false),
            redirects: cli.disable_redirects.then_some(false),
            snapshot: cli.snapshot.clone(),
            allow_host: (!cli.allow_host.is_empty()).then(|| cli.allow_host.clone()),
            max_requests: cli.max_requests,
            max_rate: cli.max_rate,
            max_concurrency: cli.max_concurrency,
            ramp: None,
            api: None,
        };
        match &cli.command {
            Some(Command::Ramp(ramp)) => {
                let usual = RampArgs::default();
                if config.url.is_none() && !ramp.url.is_empty() {
                    config.url = Some(ramp.url.clone());
                }
                // A ramp sets its own load
                config.concurrency = None;
                config.requests = None;
                config.duration = None;
                config.ramp = Some(RampConfig {
                    from: Some(ramp.from),
                    to: Some(ramp.to),
                    step: Some(ramp.step),
                    every: (ramp.every != usual.every).then(|| ramp.every.clone()),
                    until: (!ramp.until.is_empty()).then(|| ramp.until.clone()),
                });
            }
            Some(Command::Api(api)) => {
                let list = |v: &Vec<String>| (!v.is_empty()).then(|| v.clone());
                config.api = Some(ApiConfig {
                    spec: (!api.spec.is_empty()).then(|| api.spec.clone()),
                    server: api.server.clone(),
                    auth: list(&api.auth),
                    all: api.all.then_some(true),
                    tag: list(&api.tag),
                    only: list(&api.only),
                    skip: list(&api.skip),
                    set: list(&api.set),
                    include_writes: api.include_writes.then_some(true),
                });
            }
            _ => {}
        }
        config
    }

    /// The file's text, with a line on how it's used
    pub fn to_toml(&self) -> String {
        let body = toml::to_string(self).unwrap_or_default();
        format!(
            "# pepe load test · run it with `pepe` in this directory, or `pepe --config {DEFAULT_PATH}`\n\
             # flags on the command line win over this file\n\n{body}"
        )
    }
}

/// Write the settings to `path`
pub fn write(path: &Path, cli: &Cli) -> Result<(), String> {
    std::fs::write(path, Config::from_cli(cli).to_toml())
        .map_err(|e| format!("couldn't write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::{CommandFactory, FromArgMatches};

    /// A command line, parsed the way `main` does
    fn parse_argv(argv: &[&str]) -> (Cli, ArgMatches) {
        let matches = Cli::command().get_matches_from(argv);
        (Cli::from_arg_matches(&matches).unwrap(), matches)
    }

    #[test]
    fn the_file_fills_in_what_the_command_line_left_unsaid() {
        let config = parse(
            r#"
            url = "https://api.example.com/health"
            method = "POST"
            headers = ["Accept: application/json"]
            body = '{"ping": 1}'
            duration = "30s"
            concurrency = 50
            timeout = 5
            keep-alive = false
            rate = 500
            warmup = "5s"
            trace-header = "X-Req"
            "#,
        )
        .unwrap();
        // Nothing typed: everything comes from the file
        let (mut cli, matches) = parse_argv(&["pepe"]);
        config.apply(&mut cli, &matches);
        assert_eq!(cli.url, "https://api.example.com/health");
        assert_eq!(cli.method, "POST");
        assert_eq!(cli.headers, ["Accept: application/json"]);
        assert_eq!(cli.body.as_deref(), Some(r#"{"ping": 1}"#));
        assert_eq!(
            (cli.duration.as_deref(), cli.concurrency, cli.timeout),
            (Some("30s"), 50, 5)
        );
        assert!(cli.disable_keepalive && !cli.disable_redirects);
        assert_eq!(
            (cli.rate, cli.warmup.as_deref(), cli.trace_header.as_deref()),
            (Some(500.0), Some("5s"), Some("X-Req"))
        );

        // Typed flags win, including ones that equal the default
        let (mut cli, matches) = parse_argv(&[
            "pepe",
            "-c",
            "100",
            "-n",
            "100",
            "-m",
            "GET",
            "https://other/",
        ]);
        config.apply(&mut cli, &matches);
        assert_eq!(cli.url, "https://other/");
        assert_eq!(
            (cli.concurrency, cli.number, cli.duration),
            (100, 100, None)
        );
        assert_eq!(cli.method, "GET");
        assert_eq!(cli.timeout, 5, "not typed, so the file's");
    }

    #[test]
    fn ramp_and_api_tables_feed_their_subcommands() {
        let config = parse(
            r#"
            url = "https://api.example.com/"
            [ramp]
            from = 5
            to = 50
            every = "5s"
            until = ["p99 > 500ms"]
            [api]
            spec = "openapi.yaml"
            tag = ["Billing"]
            all = true
            "#,
        )
        .unwrap();
        let (mut cli, matches) = parse_argv(&["pepe", "ramp", "--to", "80"]);
        config.apply(&mut cli, &matches);
        let Some(Command::Ramp(ramp)) = &cli.command else {
            panic!()
        };
        assert_eq!(
            (ramp.url.as_str(), ramp.from, ramp.to, ramp.step),
            ("https://api.example.com/", 5, 80, 10)
        );
        assert_eq!(
            (ramp.every.as_str(), ramp.until.as_slice()),
            ("5s", &["p99 > 500ms".to_string()][..])
        );

        let (mut cli, matches) = parse_argv(&["pepe", "api", "--tag", "Pets"]);
        config.apply(&mut cli, &matches);
        let Some(Command::Api(api)) = &cli.command else {
            panic!()
        };
        assert_eq!((api.spec.as_str(), api.all), ("openapi.yaml", true));
        assert_eq!(api.tag, ["Pets"], "typed tags win");
    }

    #[test]
    fn written_settings_read_back_the_same() {
        let (cli, _) = parse_argv(&[
            "pepe",
            "-c",
            "20",
            "-z",
            "1m",
            "-m",
            "PUT",
            "-H",
            "X-A: 1",
            "-d",
            "hi",
            "-t",
            "3",
            "--threads",
            "2",
            "--rate",
            "250",
            "--warmup",
            "3s",
            "--trace-header",
            "X-Req",
            "--disable-redirects",
            "-k",
            "https://example.com/",
        ]);
        let text = Config::from_cli(&cli).to_toml();
        assert!(text.starts_with("# pepe load test"), "{text}");
        let again = parse(&text).unwrap();
        assert_eq!(again, Config::from_cli(&cli));
        let (mut fresh, matches) = parse_argv(&["pepe"]);
        again.apply(&mut fresh, &matches);
        assert_eq!(fresh.command_line(), cli.command_line());

        // A ramp's file says the ramp, not a concurrency
        let (cli, _) = parse_argv(&[
            "pepe",
            "ramp",
            "https://example.com/",
            "--from",
            "2",
            "--until",
            "errors > 1%",
        ]);
        let text = Config::from_cli(&cli).to_toml();
        assert!(
            text.contains("[ramp]") && text.contains("from = 2") && !text.contains("concurrency"),
            "{text}"
        );
        let (mut fresh, matches) = parse_argv(&["pepe", "ramp"]);
        parse(&text).unwrap().apply(&mut fresh, &matches);
        // `main` lifts a ramp's URL to the top level before anything else
        let lifted = |mut cli: Cli| {
            if let Some(Command::Ramp(ramp)) = &cli.command {
                cli.url = ramp.url.clone();
            }
            cli
        };
        assert_eq!(lifted(fresh).command_line(), lifted(cli).command_line());
    }

    #[test]
    fn threads_are_a_number_or_auto_in_the_file_too() {
        use crate::load::ThreadCount;
        assert_eq!(
            parse("threads = 3\n").unwrap().threads,
            Some(ThreadCount::Fixed(3))
        );
        assert_eq!(
            parse("threads = \"auto\"\n").unwrap().threads,
            Some(ThreadCount::Auto)
        );
        assert!(parse("threads = 0\n").is_err());
        assert!(parse("threads = \"plenty\"\n").is_err());
        // `--threads auto` is written as it was typed, and reads back
        let (cli, _) = parse_argv(&["pepe", "https://example.com/", "--threads", "auto"]);
        let text = Config::from_cli(&cli).to_toml();
        assert!(text.contains("threads = \"auto\""), "{text}");
        assert_eq!(parse(&text).unwrap().threads, Some(ThreadCount::Auto));
        assert!(cli.command_line().contains("--threads auto"));
    }

    #[test]
    fn a_typo_is_named() {
        let err = parse("concurency = 5\n").unwrap_err();
        assert!(err.contains("unknown field `concurency`"), "{err}");
        assert!(err.contains("character 1"), "{err}");
        assert!(parse("").unwrap() == Config::default());
    }
}
