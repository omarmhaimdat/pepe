//! `pepe ping`: a request a second, split into its phases.

use super::*;

/// `pepe ping`: a request every so often, each with its phases, on a
/// graph; or a line per ping when there's no screen to draw on
pub(crate) async fn run_ping(
    args: &Cli,
    what: &cli::PingArgs,
    matches: &clap::ArgMatches,
) -> Result<(), Box<dyn std::error::Error>> {
    let fail = |message: String| -> ! {
        eprintln!("error: {message}");
        std::process::exit(2);
    };
    if !args.fail_if.is_empty() {
        fail("--fail-if is for load runs; a ping's limits are --slo total=500,ttfb=200".into());
    }
    let family = match (what.ipv4, what.ipv6) {
        (true, _) => Some(crate::ping::Family::V4),
        (_, true) => Some(crate::ping::Family::V6),
        _ => None,
    };
    let mut every = crate::ping::interval(&what.every, std::time::Duration::from_secs(1))
        .unwrap_or_else(|e| fail(format!("--every: {e}")));
    let typed = |name: &str| {
        matches
            .subcommand_matches("ping")
            .and_then(|m| m.value_source(name))
            .or_else(|| matches.value_source(name))
            == Some(clap::parser::ValueSource::CommandLine)
    };
    // --once: three pings, quickly, unless told otherwise
    if what.once && !typed("every") {
        every = std::time::Duration::from_millis(200);
    }
    let window = crate::ping::interval(&what.window, std::time::Duration::from_secs(1))
        .unwrap_or_else(|e| fail(format!("--window: {e}")));
    let slo = match &what.slo {
        Some(text) => crate::ping::Slo::parse(text).unwrap_or_else(|e| fail(format!("--slo: {e}"))),
        None => crate::ping::Slo::default(),
    };
    let bind = what.interface.as_deref().map(|name| {
        crate::ping::local_address(name, family)
            .unwrap_or_else(|e| fail(format!("--interface: {e}")))
    });
    let colors: Vec<ratatui::style::Color> = what
        .color
        .iter()
        .flat_map(|c| c.split(','))
        .map(str::trim)
        .filter(|c| !c.is_empty())
        .map(|c| {
            ui::parse_color(c).unwrap_or_else(|| fail(format!("--color: {c:?} isn't a colour")))
        })
        .collect();
    // Only a count that was typed, or a saved test's, ends the run: the
    // default is forever
    let count = if typed("number") || args.number_from_file {
        Some(u64::from(args.number))
    } else if what.once {
        Some(3)
    } else {
        None
    };
    let headers = {
        let mut map = reqwest::header::HeaderMap::new();
        for header in &args.headers {
            let (name, value) = request::parse_header(header).unwrap_or_else(|e| fail(e));
            map.append(name, value);
        }
        map
    };
    let settings = crate::ping::Settings {
        every,
        timeout: std::time::Duration::from_secs(u64::from(args.timeout)),
        family,
        bind,
        insecure: args.insecure,
        keep_alive: what.keep_alive,
        follow_redirects: !args.disable_redirects,
        refused_is_pong: what.tcp_rst == cli::Refused::Pong,
        slo,
        keep_body: if what.save_body.is_some() {
            crate::ping::BODY_SAVED
        } else if what.show_body {
            crate::ping::BODY_PREVIEW
        } else {
            0
        },
        proxy: args.proxy.clone(),
        user_agent: args.user_agent.clone(),
        headers,
        method: reqwest::Method::from_bytes(args.method.as_bytes()).unwrap_or(reqwest::Method::GET),
        body: args.body().map(bytes::Bytes::from).unwrap_or_default(),
        count,
        duration: args.run_duration(),
        compression: !args.disable_compression,
        http1: what.http1,
        allowed_hosts: crate::guard::Guard::of(args).hosts,
    };
    let targets = crate::ping::targets(crate::ping::Words {
        words: &what.targets,
        names: &what.name,
        tcp: what.tcp,
        port: what.port,
        cmd: what.cmd,
        all_ips: what.all_ips,
        family,
    })
    .await
    .unwrap_or_else(|e| fail(e));

    // The guardrails: each target's host, then the pace
    let guard = guard::Guard::of(args);
    for target in &targets {
        let checked = match &target.kind {
            crate::ping::Kind::Http { url, .. } => guard.check_url(url.as_str()),
            crate::ping::Kind::Tcp { host, .. } => guard.check_host(host),
            crate::ping::Kind::Cmd(_) if !guard.is_empty() => {
                Err("--cmd runs programs, which the guardrails can't check; not under --allow-host or a cap".into())
            }
            crate::ping::Kind::Cmd(_) => Ok(()),
        };
        if let Err(e) = checked {
            fail(e);
        }
    }
    if let Err(e) = guard
        .check_ping(targets.len(), every, count, args.run_duration())
        .and_then(|_| guard.check_proxy(args))
    {
        fail(e);
    }
    if args.dry_run {
        let plan = guard::Plan {
            mode: "ping",
            targets: targets.iter().take(20).map(|t| t.shown.clone()).collect(),
            more_targets: targets.len().saturating_sub(20),
            method: Some(args.method.clone()),
            headers: guard::Plan::headers_of(&args.headers),
            body_bytes: (!settings.body.is_empty()).then(|| settings.body.len()),
            load: format!(
                "one request every {} to each of {} target{}, {}",
                crate::ping::every_text(every),
                targets.len(),
                if targets.len() == 1 { "" } else { "s" },
                match (count, args.run_duration()) {
                    (Some(n), _) => format!("{n} times"),
                    (None, Some(d)) => format!("for {}", ui::format::span(d)),
                    (None, None) => "until stopped".into(),
                }
            ),
            settings: guard::Plan::settings_of(args),
            guard: guard.describe(),
            ..Default::default()
        };
        return print_plan(args, &plan);
    }

    let screen = !args.json
        && !what.jsonl
        && !what.csv
        && !what.once
        && stdout().is_terminal()
        && stdin().is_terminal();
    let shared = crate::ping::start(targets, settings.clone());
    crate::webhook::begin(hook_run(args), &args.hooks());
    if exporter::address().is_some() {
        let shared = shared.clone();
        tokio::spawn(async move {
            while !shared.stopped() {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                exporter::publish(|| exporter::ping_page(&shared.lock()));
            }
        });
    }
    let mut interrupted = false;
    let mut recorded = false;
    if screen {
        let check = update::Check::start();
        let outcome = {
            let _watchdog = CtrlCWatchdog::arm();
            let _terminal = TerminalGuard::enter()?;
            let mut screen = ui::PingScreen::new(
                shared.clone(),
                ui::Look {
                    window: window.as_secs_f32(),
                    ymin: if what.ymin_zero {
                        Some(0)
                    } else {
                        what.ymin.map(|ms| ms * 1000)
                    },
                    ymax: what.ymax.map(|ms| ms * 1000),
                    simple: what.simple_graphics,
                    colors,
                    bell: what.bell,
                    cli: args.clone(),
                    settings: settings.clone(),
                },
            );
            let outcome = screen.run().await;
            recorded = screen.recorded();
            outcome
        };
        shared.stop();
        outcome?;
        {
            let state = shared.lock();
            interrupted = !state.done;
            print_report(&crate::ping::report(&state, &settings));
        }
        say_if_newer(check).await;
    } else {
        // No screen: each ping as a line, as it happens, and the summary
        // at the end; JSON goes to stdout alone, the summary to stderr
        let (jsonl, csv, json) = (what.jsonl, what.csv, args.json);
        // --once says it all at the end, not a line per ping
        let quiet = what.once && !jsonl && !csv;
        if csv {
            println!("{}", crate::ping::CSV_HEADER);
        }
        // Samples of each target printed so far, out of all it recorded
        let mut printed: Vec<u64> = vec![0; shared.lock().targets.len()];
        let mut pump = tokio::time::interval(std::time::Duration::from_millis(50));
        let bell = what.bell;
        loop {
            tokio::select! {
                _ = pump.tick() => {}
                _ = tokio::signal::ctrl_c() => {
                    interrupted = true;
                    shared.stop();
                }
            }
            let mut state = shared.lock();
            let mut lines = Vec::new();
            for (index, t) in state.targets.iter().enumerate() {
                // What came since the last tick, of what `recent` still
                // holds. Counted in samples recorded, not pings sent: a
                // ping still in flight has no sample yet
                let held = t.recent.len() as u64;
                let new = t.recorded.saturating_sub(printed[index]).min(held);
                for sample in t.recent.iter().skip((held - new) as usize) {
                    let line = if jsonl {
                        crate::ping::sample_json(sample, &t.target).to_string()
                    } else if csv {
                        crate::ping::sample_csv(sample, &t.target)
                    } else if json || quiet {
                        continue;
                    } else {
                        crate::ping::sample_line(sample, &t.target)
                    };
                    lines.push((sample.at, line));
                }
                printed[index] = t.recorded;
            }
            if crate::events::due() {
                let us = |v: u64| (v as f64 / 10.0).round() / 100.0;
                let targets: Vec<serde_json::Value> = state
                    .targets
                    .iter()
                    .map(|t| {
                        let w = t.whole();
                        serde_json::json!({
                            "name": t.target.name,
                            "target": t.target.shown,
                            "sent": w.sent,
                            "answered": w.answered,
                            "timeouts": w.timeouts,
                            "last_ms": w.last.map(us),
                            "p50_ms": us(w.p50),
                            "p99_ms": us(w.p99),
                            "jitter_ms": us(w.jitter),
                        })
                    })
                    .collect();
                crate::events::ping_progress(serde_json::Value::Array(targets));
            }
            let rang = std::mem::take(&mut state.bells);
            let done = state.done || shared.stopped();
            drop(state);
            lines.sort_by_key(|l| l.0);
            for (_, line) in lines {
                println!("{line}");
            }
            if bell && rang > 0 {
                eprint!("\x07");
            }
            if done {
                break;
            }
        }
        let state = shared.lock();
        if json && args.rerun.is_none() {
            println!(
                "{}",
                serde_json::to_string_pretty(&crate::ping::json_report(
                    &state,
                    &settings,
                    interrupted
                ))?
            );
        } else if jsonl || csv {
            eprint!("{}", crate::ping::report(&state, &settings));
        } else if what.once {
            print_report(&diagnose::once(&state, &settings));
        } else {
            print_report(&crate::ping::report(&state, &settings));
        }
    }
    // The lock is let go before waiting on the webhooks
    let (comparison, code) = {
        let state = shared.lock();
        // `--record`: the report under the saved test, unless the screen did
        let comparison = if args.record && !recorded {
            let mut report = crate::ping::json_report(&state, &settings, interrupted);
            crate::store::stamp_test(&mut report, args);
            record_result(args, &serde_json::to_string_pretty(&report)?)?
        } else {
            None
        };
        if let (Some(comparison), true) = (&comparison, args.json) {
            println!("{}", comparison.to_json()?);
        } else if args.json && args.rerun.is_some() && !screen {
            // A rerun with nothing to compare with yet: the report after all
            println!(
                "{}",
                serde_json::to_string_pretty(&crate::ping::json_report(
                    &state,
                    &settings,
                    interrupted
                ))?
            );
        }
        if let Some(path) = &what.save {
            let report = serde_json::to_string_pretty(&crate::ping::json_report(
                &state,
                &settings,
                interrupted,
            ))?;
            if let Err(e) = std::fs::write(path, report) {
                eprintln!("couldn't write {}: {e}", path.display());
            }
        }
        if let Some(path) = &what.save_body {
            match state.targets.iter().find_map(|t| t.last_body.as_ref()) {
                Some(body) => {
                    if let Err(e) = std::fs::write(path, body) {
                        eprintln!("couldn't write {}: {e}", path.display());
                    }
                }
                None => eprintln!("no body to write to {}", path.display()),
            }
        }
        // Like ping: 1 when nothing ever answered; 4 when the SLO was broken
        let code = if state.any_violation() {
            4
        } else if state.nothing_answered() {
            1
        } else {
            0
        };
        let crossed = match code {
            4 => vec!["the SLO was broken".to_string()],
            1 => vec!["nothing answered".to_string()],
            _ => Vec::new(),
        };
        hooks_end(
            args,
            &crate::ping::json_report(&state, &settings, interrupted),
            None,
            crossed,
            None,
        );
        (comparison, code)
    };
    crate::webhook::flush().await;
    if code != 0 {
        std::process::exit(code);
    }
    finish_rerun(args, comparison.as_ref());
    Ok(())
}
