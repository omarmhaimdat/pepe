//! `pepe replay`: an access log's URLs in their real proportions.

use super::*;

/// `pepe replay LOG`: the log's URLs, weighted as they were seen
pub(crate) async fn run_replay(
    args: &Cli,
    what: &cli::ReplayArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let replay = match crate::replay::load(&what.log, what.base_url.as_deref(), what.include_writes)
    {
        Ok(replay) => replay,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    // The guardrails: every URL the log would be sent to
    let guard = guard::Guard::of(args);
    for entry in &replay.urls {
        if let Err(e) = guard.check_url(&entry.url) {
            eprintln!("error: {e}");
            std::process::exit(2);
        }
    }
    if let Err(e) = guard.check_run(args) {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
    if args.dry_run {
        let plan = guard::Plan {
            mode: "replay",
            targets: replay.urls.iter().take(20).map(|e| e.url.clone()).collect(),
            more_targets: replay.urls.len().saturating_sub(20),
            headers: guard::Plan::headers_of(&args.headers),
            load: format!(
                "{} distinct URLs of {}, in the log's proportions; {}",
                replay.urls.len(),
                what.log.display(),
                guard::Plan::load_of(args, "requests")
            ),
            settings: guard::Plan::settings_of(args),
            guard: guard.describe(),
            notes: if what.include_writes {
                vec!["POST, PUT, PATCH and DELETE are replayed too, without bodies".into()]
            } else {
                vec!["only GET, HEAD and OPTIONS; --include-writes replays the rest".into()]
            },
            ..Default::default()
        };
        return print_plan(args, &plan);
    }
    // The dashboard's title shows the log rather than one URL
    let mut shown = args.clone();
    shown.method = "REPLAY".into();
    shown.url = what
        .log
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| what.log.display().to_string());
    if args.json {
        return run_replay_json(&shown, &replay, what).await;
    }
    if headless(args) {
        let (clients, connects) = replay_clients(&shown, &replay, shown.concurrency as usize)?;
        let load = load::start_targets(
            clients,
            replay.targets(&shown, what.rows)?,
            shown.concurrency as usize,
            plan(&shown),
            false,
        );
        load.set_rate(shown.rate);
        if let Some(warmup) = shown.warmup() {
            load.set_warmup(warmup);
        }
        let dashboard = ui::Dashboard::new(shown.clone(), plan(&shown))
            .with_rows(ui::Rows::Urls, replay.views(what.rows))
            .with_connects(connects);
        return headless_dashboard(&shown, dashboard, load).await;
    }
    let check = update::Check::start();
    let (report, metrics) = {
        let _watchdog = CtrlCWatchdog::arm();
        let _terminal = TerminalGuard::enter()?;
        replay_session(&mut shown, &replay, what).await?
    };
    if let Some(report) = report {
        print_report(&report);
    }
    say_if_newer(check).await;
    if let Some(metrics) = metrics {
        exit_if_failed(&shown, &metrics).await;
    }
    Ok(())
}

/// One client per shard, built around the most seen URL: the shared
/// headers and settings are what matter
pub(crate) fn replay_clients(
    args: &Cli,
    replay: &crate::replay::Replay,
    concurrency: usize,
) -> Result<(load::Senders, std::sync::Arc<request::ConnectTimes>), PepeError> {
    let base = request::Request::new(
        replay.urls[0].url.clone(),
        "GET".into(),
        None,
        &args.headers,
        args.settings(),
    )?;
    base.build_clients(shards(args, concurrency))
}

pub(crate) async fn replay_session(
    shown: &mut Cli,
    replay: &crate::replay::Replay,
    what: &cli::ReplayArgs,
) -> Result<(Option<String>, Option<Metrics>), Box<dyn std::error::Error>> {
    loop {
        let (clients, connects) = replay_clients(shown, replay, shown.concurrency as usize)?;
        let mut load = load::start_targets(
            clients,
            replay.targets(shown, what.rows)?,
            shown.concurrency as usize,
            plan(shown),
            true,
        );
        load.set_rate(shown.rate);
        if let Some(warmup) = shown.warmup() {
            load.set_warmup(warmup);
        }
        let mut dashboard = ui::Dashboard::new(shown.clone(), plan(shown))
            .with_rows(ui::Rows::Urls, replay.views(what.rows))
            .with_connects(connects);
        let outcome = dashboard.run(&mut load).await?;
        shown.concurrency = load.concurrency() as u32;
        match outcome {
            // There is no setup screen for a replay: the log is the setup
            ui::Outcome::Restart | ui::Outcome::Edit => {}
            ui::Outcome::Quit => {
                return Ok((dashboard.report(), Some(dashboard.metrics().clone())))
            }
        }
    }
}

pub(crate) async fn run_replay_json(
    args: &Cli,
    replay: &crate::replay::Replay,
    what: &cli::ReplayArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let views = replay.views(what.rows);
    let row_labels: Vec<String> = views.iter().map(|v| v.label.clone()).collect();
    let timeline = timeline::Timeline::default();
    let (clients, connects) = replay_clients(args, replay, args.concurrency as usize)?;
    let mut load = load::start_targets(
        clients,
        replay.targets(args, what.rows)?,
        args.concurrency as usize,
        plan(args),
        false,
    );
    load.set_rate(args.rate);
    if let Some(warmup) = args.warmup() {
        load.set_warmup(warmup);
    }
    crate::webhook::begin(hook_run(args), &args.hooks());
    let mut control = crate::control::listen();
    let mut started = Instant::now();
    let mut warming = load.warming();
    let mut warmup_requests = 0;
    let mut total = Metrics::default();
    let mut each = vec![Metrics::default(); views.len()];
    let mut interrupted = false;
    let mut pump = tokio::time::interval(PUMP);
    loop {
        tokio::select! {
            _ = pump.tick() => {
                if warming && !load.warming() {
                    warming = false;
                    started = Instant::now();
                }
                let over = !load.drain(|stat| {
                    if stat.warmup {
                        warmup_requests += 1;
                        return;
                    }
                    total.record(&stat);
                    if let Some(metrics) = each.get_mut(stat.endpoint as usize) {
                        metrics.record(&stat);
                    }
                });
                exporter::publish(|| {
                    let rows: Vec<(String, &Metrics)> =
                        row_labels.iter().cloned().zip(each.iter()).collect();
                    exporter::run_page(
                        &args.target_label(),
                        &total,
                        &timeline,
                        started.elapsed(),
                        args.concurrency as usize,
                        &rows,
                    )
                });
                if !warming {
                    crate::webhook::progress(|| hook_progress(&total, started.elapsed(), plan(args), None, 0));
                }
                if over {
                    break;
                }
            }
            _ = tokio::signal::ctrl_c(), if !interrupted => {
                interrupted = true;
                load.stop();
            }
            said = crate::control::next(&mut control) => {
                if crate::control::apply(&said, &load) && !interrupted {
                    interrupted = true;
                    load.stop();
                }
            }
        }
    }
    let elapsed = started.elapsed();
    let report = json_report::JsonReport::generate(&total, elapsed, interrupted)
        .with_generator(
            load.threads(),
            load.peak_busy(),
            load.rate().map(|r| (r, load.missed())),
        )
        .with_target(
            "replay",
            None,
            &what.log.display().to_string(),
            args.concurrency,
        )
        .with_warmup(args.warmup(), warmup_requests)
        .with_connects(&connects)
        .with_verdict(&verdict_of(
            args,
            &total,
            &[],
            interrupted,
            &connects,
            load.concurrency(),
        ));
    let mut report = serde_json::to_value(&report)?;
    let ms = |d: std::time::Duration| (d.as_secs_f64() * 1_000_000.0).round() / 1000.0;
    let kept = replay.kept().max(1) as f64;
    let urls: Vec<serde_json::Value> = views
        .iter()
        .zip(&each)
        .enumerate()
        .map(|(rank, (view, m))| {
            let share = match replay.urls.get(rank).filter(|_| rank < what.rows) {
                Some(entry) => entry.count as f64 / kept,
                None => replay.urls[what.rows.min(replay.urls.len())..].iter().map(|e| e.count).sum::<u64>() as f64 / kept,
            };
            serde_json::json!({
                "method": view.method,
                "url": if view.variants > 1 { format!("{} other URLs", view.variants) } else { view.url.clone() },
                "share_in_log": (share * 10_000.0).round() / 10_000.0,
                "requests": m.total,
                "failed_requests": m.total - m.success,
                "requests_per_second": m.rps(elapsed),
                "median_ms": ms(m.percentile(50.0)),
                "p99_ms": ms(m.percentile(99.0)),
                "status_codes": m.status_codes.iter().map(|(k, v)| (k.to_string(), *v)).collect::<std::collections::BTreeMap<_, _>>(),
            })
        })
        .collect();
    report["replay"] = serde_json::json!({
        "log": what.log.display().to_string(),
        "requests_in_log": replay.requests,
        "replayed_from_log": replay.kept(),
        "distinct_urls": replay.urls.len(),
        "left_out": {
            "unparsed_lines": replay.unparsed,
            "writes": replay.writes,
            "no_host": replay.no_host,
            "rare_urls": replay.tail,
        },
        "urls": urls,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    hooks_end(args, &report, Some(&total), Vec::new(), None);
    exit_if_failed(args, &total).await;
    Ok(())
}
