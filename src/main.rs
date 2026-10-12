use std::io::{stdin, stdout, IsTerminal};

use clap::{CommandFactory, FromArgMatches};

use crate::cli::Cli;
use crate::ramp::RampPlan;

mod api;
mod cache;
mod cert;
mod cli;
mod compare;
mod completions;
mod config;
mod contrib;
mod curl;
mod diagnose;
mod direct;
mod exporter;
mod flow;
mod guard;
mod insights;
mod json_report;
mod load;
mod logs;
mod mcp;
mod metrics;
mod modes;
mod openapi;
mod ping;
mod ramp;
mod replay;
mod request;
mod response;
mod schema;
mod store;
mod terminal;
mod timeline;
mod trace;
mod ui;
mod update;
mod utils;
mod wire;

#[derive(Debug)]
#[allow(clippy::enum_variant_names)]
enum PepeError {
    HeaderParseError(String),
    IoError(std::io::Error),
    RequestError(reqwest::Error),
    InvalidUrl(String),
}

impl std::fmt::Display for PepeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderParseError(msg) => write!(f, "{}", msg),
            Self::RequestError(e) => write!(f, "Request error: {}", e),
            Self::IoError(e) => write!(f, "{}", e),
            Self::InvalidUrl(e) => write!(f, "Invalid URL {}", e),
        }
    }
}

impl std::error::Error for PepeError {}

/// How often `--snapshot` writes the report so far
pub const SNAPSHOT_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

// The load engine has its own threads (see `load`); this runtime only runs
// the screens, the reports and the update check
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Parsed by hand so the config file can tell typed flags from defaults
    let matches = Cli::command().get_matches();
    let mut args = Cli::from_arg_matches(&matches).unwrap_or_else(|e| e.exit());
    let loaded = match &args.load {
        Some(name) => store::Store::local().load(name).map(Some),
        None => config::load(args.config.as_deref()),
    };
    match loaded {
        Ok(Some((path, file))) => {
            file.apply(&mut args, &matches);
            args.config_loaded = Some(path);
        }
        Ok(None) => {}
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
    if let Some(path) = args.write_config.take() {
        if let Err(e) = config::write(&path, &args) {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        println!(
            "wrote {} · run it with: pepe --config {}",
            path.display(),
            path.display()
        );
        return Ok(());
    }
    // Which response header carries the request id; `validate` rejects a
    // name that isn't one
    if let Some(name) = args
        .trace_header
        .as_deref()
        .and_then(|n| reqwest::header::HeaderName::from_bytes(n.as_bytes()).ok())
    {
        trace::use_id_header(name);
    }

    if let Some(cli::Command::SelfUpdate(what)) = &args.command {
        return update::self_update(what.check, what.verbose).await;
    }
    if let Some(cli::Command::Completions(what)) = &args.command {
        return completions::run(what);
    }
    if let Some(cli::Command::Schema(what)) = &args.command {
        return schema::print(Some(&what.which));
    }
    if let Some(cli::Command::Mcp(_)) = &args.command {
        return mcp::serve(mcp::guard_args(&args)).await;
    }

    // Release builds abort on panic; restore the terminal first so a crash
    // never leaves the shell in raw mode
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        terminal::restore_terminal();
        default_hook(info);
    }));

    // stderr, so `--json` output on stdout stays valid JSON
    if let Some(spec) = &args.metrics {
        match exporter::start(spec).await {
            Ok(addr) => eprintln!("metrics: http://{addr}/metrics"),
            Err(e) => {
                eprintln!("error: --metrics: {e}");
                std::process::exit(1);
            }
        }
    }

    if let Some(cli::Command::Replay(what)) = args.command.clone() {
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return modes::replay::run_replay(&args, &what).await;
    }
    if let Some(cli::Command::Logs(what)) = args.command.clone() {
        return modes::logs::run_logs(&args, &what).await;
    }
    if let Some(cli::Command::Ping(what)) = args.command.clone() {
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return modes::ping::run_ping(&args, &what, &matches).await;
    }
    if let Some(cli::Command::Compare(what)) = args.command.clone() {
        return modes::compare::run_compare(&args, &what);
    }
    if let Some(cli::Command::Flow(what)) = args.command.clone() {
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return modes::flow::run_flow(&args, &what).await;
    }
    if let Some(cli::Command::Api(api)) = args.command.clone() {
        if api.spec.is_empty() {
            // With nothing to load yet, the setup screen asks for the spec:
            // a file, a URL, or the document pasted in
            let interactive = !args.json && stdin().is_terminal() && stdout().is_terminal();
            if !interactive {
                eprintln!("error: api needs a spec: pepe api openapi.yaml, or `spec` under [api] in pepe.toml");
                std::process::exit(2);
            }
            return modes::run::run_screens(&args, true).await;
        }
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
        return modes::api::run_api(&args, &api).await;
    }

    // A ramp names its URL after `ramp`; from here on it's the URL
    let ramp = match &args.command {
        Some(cli::Command::Ramp(ramp)) => {
            args.url = ramp.url.clone();
            Some(ramp.clone())
        }
        _ => None,
    };

    // The setup screen opens on request, or when there's nothing to run
    // yet and someone is at the terminal to fill it in
    let interactive = !args.json && stdin().is_terminal() && stdout().is_terminal();
    let nothing_to_run = args.url.is_empty() && !args.curl;
    if nothing_to_run && !interactive {
        eprintln!(
            "error: nothing to run: pepe https://example.com, pepe --curl -- curl ... for a curl command, \
             or pepe alone at a terminal for the setup screen"
        );
        std::process::exit(2);
    }
    let setup = interactive && (args.setup || nothing_to_run);

    // With nothing to run yet, the setup screen does the checking instead
    if !nothing_to_run {
        if let Err(e) = args.validate() {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    }

    // The guardrails, then the dry run, before anything is sent
    if !nothing_to_run {
        let guard = guard::Guard::of(&args);
        let checked = guard.check_url(&args.url).and_then(|_| match &ramp {
            Some(ramp) => RampPlan::from_args(ramp)
                .map_err(|e| e.to_string())
                .and_then(|plan| guard.check_ramp(plan.peak())),
            None => guard.check_run(&args),
        });
        if let Err(e) = checked {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
        if args.dry_run {
            let plan = guard::Plan {
                mode: if ramp.is_some() { "ramp" } else { "run" },
                targets: vec![args.url.clone()],
                method: Some(args.method.clone()),
                headers: guard::Plan::headers_of(&args.headers),
                body_bytes: args.body().map(|b| b.len()),
                load: match &ramp {
                    Some(r) => format!(
                        "concurrency {} to {} by {}, each step held {}{}",
                        r.from,
                        r.to,
                        r.step,
                        r.every,
                        if r.until.is_empty() {
                            String::new()
                        } else {
                            format!(", until {}", r.until.join(" or "))
                        }
                    ),
                    None => guard::Plan::load_of(&args, "requests"),
                },
                settings: guard::Plan::settings_of(&args),
                guard: guard.describe(),
                ..Default::default()
            };
            modes::print_plan(&args, &plan)?;
            return Ok(());
        }
    }

    if let (Some(ramp), false) = (&ramp, nothing_to_run) {
        match RampPlan::from_args(ramp) {
            Ok(plan) if args.json => return modes::ramp::run_ramp_json(&args, plan).await,
            Ok(_) => {}
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
    }
    if args.json {
        return modes::run::run_json(&args).await;
    }
    if modes::headless(&args) {
        return modes::run_headless(&args, ramp).await;
    }
    modes::run::run_screens(&args, setup).await
}
