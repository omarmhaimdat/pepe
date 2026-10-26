//! `pepe.toml`: a run's settings in a file, so a load test can live next
//! to the code it tests and be run with `pepe` alone. Flags on the command
//! line win over the file; the file wins over pepe's defaults.

use std::path::{Path, PathBuf};

use clap::parser::{ArgMatches, ValueSource};
use clap::Parser;
use serde::{Deserialize, Serialize};

use crate::cli::{Cli, Command, PingArgs, RampArgs};
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
    /// `--vars`: the CSV file whose rows fill the request's `{{holes}}`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vars: Option<PathBuf>,
    /// `--export`: files the report is written to as well, each in the
    /// format its extension says
    #[serde(skip_serializing_if = "Option::is_none")]
    pub export: Option<Vec<PathBuf>>,
    /// Where the run is said: a URL, a `[[webhook]]` table, or a list
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "crate::webhook::deserialize",
        serialize_with = "crate::webhook::serialize"
    )]
    pub webhook: Option<Vec<crate::webhook::Webhook>>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ping: Option<PingConfig>,
}

/// `[ping]`: what `pepe ping` is told, beyond the request flags it shares
#[derive(Serialize, Deserialize, Debug, Clone, Default, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct PingConfig {
    /// URLs, hosts, host:port, ranges, or commands with `cmd = true`
    #[serde(skip_serializing_if = "Option::is_none")]
    pub targets: Option<Vec<String>>,
    /// `--every`, e.g. "1s"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub every: Option<String>,
    /// `--name`, one per target
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<Vec<String>>,
    /// `--slo`, e.g. "total=500,ttfb=200"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub slo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tcp: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub http1: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv4: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ipv6: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub all_ips: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interface: Option<String>,
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

/// Hold a run to the guardrails of each of `files`: the lowest cap of any
/// of them and of the command line, and a host only if every file that
/// names hosts allows it as well as the command line. A flag can tighten
/// a file's guardrail, never loosen it.
pub fn hold(cli: &mut Cli, files: &[Config]) {
    fn lower<T: PartialOrd + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
        match (a, b) {
            (Some(a), Some(b)) => Some(if b < a { b } else { a }),
            (a, b) => a.or(b),
        }
    }
    for file in files {
        cli.max_requests = lower(cli.max_requests, file.max_requests);
        cli.max_rate = lower(cli.max_rate, file.max_rate);
        cli.max_concurrency = lower(cli.max_concurrency, file.max_concurrency);
        if let Some(hosts) = file.allow_host.as_ref().filter(|h| !h.is_empty()) {
            if !cli.held_hosts.contains(hosts) {
                cli.held_hosts.push(hosts.clone());
            }
        }
    }
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
    /// This file over the machine's defaults (the settings' `[defaults]`):
    /// what it leaves unsaid is taken from `under`
    pub fn over(mut self, under: &Config) -> Config {
        macro_rules! fill {
            ($($field:ident),*) => {
                $(if self.$field.is_none() {
                    self.$field = under.$field.clone();
                })*
            };
        }
        // How long a run is, is said once: a file with a count doesn't
        // take a duration from under it, which would win over the count
        if self.requests.is_none() && self.duration.is_none() {
            fill!(requests, duration);
        }
        fill!(
            url,
            method,
            headers,
            body,
            concurrency,
            timeout,
            threads,
            rate,
            warmup,
            trace_header,
            webhook,
            user_agent,
            proxy,
            insecure,
            compression,
            keep_alive,
            redirects,
            snapshot,
            allow_host,
            ping
        );
        // The caps: the lower of the two holds, so a file in a directory
        // can't loosen what the machine is held to
        fn lower<T: PartialOrd + Copy>(a: Option<T>, b: Option<T>) -> Option<T> {
            match (a, b) {
                (Some(a), Some(b)) => Some(if b < a { b } else { a }),
                (a, b) => a.or(b),
            }
        }
        self.max_requests = lower(self.max_requests, under.max_requests);
        self.max_rate = lower(self.max_rate, under.max_rate);
        self.max_concurrency = lower(self.max_concurrency, under.max_concurrency);
        self.ramp = match (self.ramp.take(), &under.ramp) {
            (Some(mut ramp), Some(under)) => {
                macro_rules! fill {
                    ($($field:ident),*) => {
                        $(if ramp.$field.is_none() {
                            ramp.$field = under.$field.clone();
                        })*
                    };
                }
                fill!(from, to, step, every, until);
                Some(ramp)
            }
            (ramp, under) => ramp.or_else(|| under.clone()),
        };
        self.api = match (self.api.take(), &under.api) {
            (Some(mut api), Some(under)) => {
                macro_rules! fill {
                    ($($field:ident),*) => {
                        $(if api.$field.is_none() {
                            api.$field = under.$field.clone();
                        })*
                    };
                }
                fill!(
                    spec,
                    server,
                    auth,
                    all,
                    tag,
                    only,
                    skip,
                    set,
                    include_writes
                );
                Some(api)
            }
            (api, under) => api.or_else(|| under.clone()),
        };
        self
    }

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
                cli.number_from_file = true;
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
        if cli.vars.is_none() {
            cli.vars = self.vars.clone();
        }
        if let (Some(files), true) = (&self.export, cli.export.is_empty()) {
            cli.export = files.clone();
        }
        if let (Some(hooks), true) = (&self.webhook, cli.webhook.is_empty()) {
            cli.webhooks = hooks.clone();
        }
        if cli.proxy.is_none() {
            cli.proxy = self.proxy.clone();
        }
        if cli.snapshot.is_none() {
            cli.snapshot = self.snapshot.clone();
        }
        // The guardrails: the file's hold, and the command line can only
        // tighten them (see `hold`)
        if let (Some(hosts), true) = (&self.allow_host, cli.allow_host.is_empty()) {
            cli.allow_host = hosts.clone();
        }
        hold(cli, std::slice::from_ref(self));
        // Flags can only be switched on from the command line, so an "on"
        // there always wins, and "off" in the file can't undo it
        cli.insecure |= self.insecure.unwrap_or(false);
        cli.disable_compression |= self.compression == Some(false);
        cli.disable_keepalive |= self.keep_alive == Some(false);
        cli.disable_redirects |= self.redirects == Some(false);

        if let (Some(Command::Ping(ping)), Some(file)) = (&mut cli.command, &self.ping) {
            if let (Some(targets), true) = (&file.targets, ping.targets.is_empty()) {
                ping.targets = targets.clone();
            }
            if let (Some(every), false) = (&file.every, typed("every")) {
                ping.every = every.clone();
            }
            if let (Some(names), true) = (&file.name, ping.name.is_empty()) {
                ping.name = names.clone();
            }
            if ping.slo.is_none() {
                ping.slo = file.slo.clone();
            }
            if let (Some(port), false) = (file.port, typed("port")) {
                ping.port = port;
            }
            if ping.interface.is_none() {
                ping.interface = file.interface.clone();
            }
            // Flags: on from the file unless the command line left them off
            ping.tcp |= file.tcp.unwrap_or(false);
            ping.cmd |= file.cmd.unwrap_or(false);
            ping.keep_alive |= file.keep_alive.unwrap_or(false);
            ping.http1 |= file.http1.unwrap_or(false);
            ping.ipv4 |= file.ipv4.unwrap_or(false);
            ping.ipv6 |= file.ipv6.unwrap_or(false);
            ping.all_ips |= file.all_ips.unwrap_or(false);
        }
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

    /// `apply` for a saved test: the file decides the mode too, when the
    /// command line named none (`pepe --load my-ramp` runs the ramp)
    pub fn apply_test(&self, cli: &mut Cli, matches: &ArgMatches) {
        self.pick_mode(cli);
        self.apply(cli, matches);
    }

    /// The mode a saved test runs in, when the command line named none.
    /// Asked of the test as saved, before the machine's defaults go under
    /// it: a `[ramp]` in the settings doesn't make every test a ramp.
    pub fn pick_mode(&self, cli: &mut Cli) {
        if cli.command.is_none() {
            if self.ramp.is_some() {
                cli.command = Some(Command::Ramp(RampArgs::default()));
            } else if self.api.is_some() {
                cli.command = Some(Command::Api(crate::cli::ApiArgs::default()));
            } else if self.ping.is_some() {
                cli.command = Some(Command::Ping(PingArgs::usual()));
            }
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
            body: cli
                .body()
                .map(|bytes| String::from_utf8_lossy(&bytes).into_owned()),
            requests: (cli.duration.is_none() && cli.number != defaults.number)
                .then_some(cli.number),
            duration: cli.duration.clone(),
            concurrency: Some(cli.concurrency),
            timeout: (cli.timeout != defaults.timeout).then_some(cli.timeout),
            threads: cli.threads,
            rate: cli.rate,
            warmup: cli.warmup.clone(),
            trace_header: cli.trace_header.clone(),
            vars: cli.vars.clone(),
            export: (!cli.export.is_empty()).then(|| cli.export.clone()),
            webhook: Some(cli.hooks()).filter(|h| !h.is_empty()),
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
            ping: None,
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
            Some(Command::Ping(ping)) => {
                let usual = PingArgs::usual();
                let list = |v: &Vec<String>| (!v.is_empty()).then(|| v.clone());
                // A ping has no load of its own, and no URL but its targets
                config.url = None;
                config.concurrency = None;
                config.duration = None;
                config.ping = Some(PingConfig {
                    targets: list(&ping.targets),
                    every: (ping.every != usual.every).then(|| ping.every.clone()),
                    name: list(&ping.name),
                    slo: ping.slo.clone(),
                    tcp: ping.tcp.then_some(true),
                    port: (ping.port != usual.port).then_some(ping.port),
                    cmd: ping.cmd.then_some(true),
                    keep_alive: ping.keep_alive.then_some(true),
                    http1: ping.http1.then_some(true),
                    ipv4: ping.ipv4.then_some(true),
                    ipv6: ping.ipv6.then_some(true),
                    all_ips: ping.all_ips.then_some(true),
                    interface: ping.interface.clone(),
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
        self.to_toml_with(&format!(
            "# pepe load test · run it with `pepe` in this directory, or `pepe --config {DEFAULT_PATH}`"
        ))
    }

    /// The file's text, under `heading`, a comment line saying how it's run
    pub fn to_toml_with(&self, heading: &str) -> String {
        let body = toml::to_string(self).unwrap_or_default();
        format!("{heading}\n# flags on the command line win over this file\n\n{body}")
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
    fn a_ping_is_a_test_too() {
        let (cli, _) = parse_argv(&[
            "pepe",
            "ping",
            "--every",
            "2s",
            "--slo",
            "total=500",
            "--keep-alive",
            "-n",
            "30",
            "https://a.io",
            "https://b.io",
        ]);
        let config = Config::from_cli(&cli);
        let text = config.to_toml();
        assert!(text.contains("[ping]"), "{text}");
        assert!(!text.contains("url ="), "{text}");
        assert!(text.contains("requests = 30"), "{text}");
        let read = parse(&text).unwrap();
        let ping = read.ping.as_ref().unwrap();
        assert_eq!(
            ping.targets.as_deref(),
            Some(&["https://a.io".to_string(), "https://b.io".to_string()][..])
        );
        assert_eq!(
            (ping.every.as_deref(), ping.slo.as_deref(), ping.keep_alive),
            (Some("2s"), Some("total=500"), Some(true))
        );
        assert_eq!(ping.port, None, "the usual port isn't written");
        // The file decides the mode and fills the ping in
        let (mut fresh, matches) = parse_argv(&["pepe"]);
        read.apply_test(&mut fresh, &matches);
        let Some(Command::Ping(args)) = &fresh.command else {
            panic!("not a ping: {:?}", fresh.command);
        };
        assert_eq!(args.targets, ["https://a.io", "https://b.io"]);
        assert_eq!(
            (args.every.as_str(), args.port, args.keep_alive),
            ("2s", PingArgs::usual().port, true)
        );
        assert_eq!((fresh.number, fresh.number_from_file), (30, true));
        // Typed flags still win
        let (mut typed, matches) = parse_argv(&["pepe", "ping", "--every", "5s", "https://c.io"]);
        read.apply(&mut typed, &matches);
        let Some(Command::Ping(args)) = &typed.command else {
            panic!()
        };
        assert_eq!(
            (args.every.as_str(), args.targets.as_slice()),
            ("5s", &["https://c.io".to_string()][..])
        );
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
            vars = "users.csv"
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
        assert_eq!(cli.vars.as_deref(), Some(Path::new("users.csv")));

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
            "--vars",
            "users.csv",
            "--webhook",
            "https://hooks.slack.com/services/T/B/x",
            "--webhook-on",
            "start,fail",
            "--disable-redirects",
            "-k",
            "https://example.com/",
        ]);
        let text = Config::from_cli(&cli).to_toml();
        assert!(text.starts_with("# pepe load test"), "{text}");
        assert!(
            text.contains("[[webhook]]") && text.contains(r#"on = ["start", "fail"]"#),
            "{text}"
        );
        // A report's command hides where the hook goes
        let masked = cli.command_line_masked();
        assert!(
            masked.contains("--webhook 'https://hooks.slack.com/••••'")
                && !masked.contains("/T/B/x"),
            "{masked}"
        );
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
    fn a_file_sits_over_the_machines_defaults() {
        let machine = parse(
            r#"
            concurrency = 8
            timeout = 5
            duration = "30s"
            keep-alive = false
            insecure = true
            max-requests = 10000
            max-rate = 500
            [ramp]
            to = 200
            every = "5s"
            "#,
        )
        .unwrap();
        let file = parse(
            r#"
            concurrency = 50
            requests = 2000
            insecure = false
            max-requests = 50000
            max-concurrency = 64
            [ramp]
            to = 80
            "#,
        )
        .unwrap();
        let both = file.over(&machine);
        assert_eq!((both.concurrency, both.timeout), (Some(50), Some(5)));
        assert_eq!(
            (both.requests, both.duration.as_deref()),
            (Some(2000), None),
            "the file's count, not the machine's duration"
        );
        assert_eq!((both.keep_alive, both.insecure), (Some(false), Some(false)));
        assert_eq!(
            (both.max_requests, both.max_rate, both.max_concurrency),
            (Some(10000), Some(500.0), Some(64)),
            "the lower cap holds"
        );
        let ramp = both.ramp.unwrap();
        assert_eq!((ramp.to, ramp.every.as_deref()), (Some(80), Some("5s")));

        // On their own, the machine's defaults fill in a bare command line
        // and give way to what's typed
        let (mut cli, matches) = parse_argv(&["pepe", "-t", "9", "https://example.com/"]);
        Config::default().over(&machine).apply(&mut cli, &matches);
        assert_eq!((cli.concurrency, cli.timeout), (8, 9));
        assert_eq!(cli.duration.as_deref(), Some("30s"));
        assert!(cli.disable_keepalive && cli.insecure);
        assert_eq!(cli.max_requests, Some(10000));

        // A saved test's mode is its own: a ramp in the machine's defaults
        // doesn't turn a plain test into one
        let test = parse("url = \"https://example.com/\"\n").unwrap();
        let (mut cli, matches) = parse_argv(&["pepe"]);
        test.pick_mode(&mut cli);
        test.over(&machine).apply(&mut cli, &matches);
        assert!(cli.command.is_none());
        assert_eq!(cli.concurrency, 8);
    }

    /// Every file's guardrails hold, whichever filled in the run, and a
    /// flag can only tighten them
    #[test]
    fn the_lowest_cap_and_every_host_list_hold() {
        let machine = parse("max-requests = 100\nallow-host = [\"localhost\"]\n").unwrap();
        let local = parse("max-rate = 5\nallow-host = [\"127.0.0.1\", \"localhost\"]\n").unwrap();
        let (mut cli, _) = parse_argv(&[
            "pepe",
            "--max-requests",
            "1000",
            "--max-rate",
            "2",
            "https://example.com/",
        ]);
        hold(&mut cli, &[machine, local]);
        assert_eq!(
            cli.max_requests,
            Some(100),
            "the flag can't raise the machine's"
        );
        assert_eq!(cli.max_rate, Some(2.0), "but it can lower the file's");
        let guard = crate::guard::Guard::of(&cli);
        assert!(guard.check_host("localhost").is_ok());
        assert!(
            guard.check_host("127.0.0.1").is_err(),
            "the machine doesn't allow it"
        );
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
