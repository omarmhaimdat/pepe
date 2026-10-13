//! The plain run: `pepe URL`, with the screens, headless, or as `--json`.

use super::*;

/// `--json`: no dashboard; run to completion (or Ctrl-C), print the report
pub(crate) async fn run_json(args: &Cli) -> Result<(), Box<dyn std::error::Error>> {
    let (mut load, connects) = start_load(args, false)?;
    // The clock starts when the warm-up, if any, is over
    let mut started = Instant::now();
    let mut warming = load.warming();
    let mut warmup_requests = 0;
    let mut metrics = Metrics::default();
    let mut timeline = timeline::Timeline::default();
    let mut slowest = metrics::Slowest::default();
    let mut watch = insights::Watch::default();
    let mut anomalies: Vec<insights::Anomaly> = Vec::new();
    let mut interrupted = false;
    let mut pump = tokio::time::interval(PUMP);
    let mut snapshots = tokio::time::interval(SNAPSHOT_EVERY);
    snapshots.tick().await; // the first tick is now; the first snapshot is in a minute
    let report = |metrics: &Metrics,
                  timeline: &timeline::Timeline,
                  slowest: &metrics::Slowest,
                  anomalies: &[insights::Anomaly],
                  load: &LoadHandle,
                  elapsed: std::time::Duration,
                  warmup_requests: u64,
                  interrupted| {
        json_report::JsonReport::generate(metrics, elapsed, interrupted)
            .with_generator(
                load.threads(),
                load.peak_busy(),
                load.rate().map(|r| (r, load.missed())),
            )
            .with_target("run", Some(&args.method), &args.url, args.concurrency)
            .with_test(args)
            .with_warmup(args.warmup(), warmup_requests)
            .with_timeline(timeline)
            .with_slowest(slowest)
            .with_anomalies(anomalies)
            .with_connects(&connects)
            .with_verdict(&verdict_of(
                args,
                metrics,
                &timeline.whole_run(),
                interrupted,
                &connects,
                load.concurrency(),
            ))
    };

    loop {
        tokio::select! {
            _ = pump.tick() => {
                if warming && !load.warming() {
                    warming = false;
                    started = Instant::now();
                }
                let now = started.elapsed();
                let over = !load.drain(|stat| {
                    if stat.warmup {
                        warmup_requests += 1;
                        return;
                    }
                    metrics.record(&stat);
                    timeline.record(&stat);
                    slowest.record(&stat, now);
                });
                if !warming {
                    timeline.advance(started.elapsed());
                    anomalies.extend(watch.observe(timeline.samples(), false));
                }
                exporter::publish(|| {
                    exporter::run_page(
                        &args.target_label(),
                        &metrics,
                        &timeline,
                        started.elapsed(),
                        args.concurrency as usize,
                        &[],
                    )
                });
                if over {
                    break;
                }
            }
            _ = snapshots.tick(), if args.snapshot.is_some() => {
                let path = args.snapshot.as_ref().expect("checked");
                if let Err(e) = report(&metrics, &timeline, &slowest, &anomalies, &load, started.elapsed(), warmup_requests, interrupted).with_snapshot(true).write_to(path) {
                    eprintln!("couldn't write the snapshot to {}: {e}", path.display());
                }
            }
            _ = tokio::signal::ctrl_c(), if !interrupted => {
                interrupted = true;
                load.stop();
            }
        }
    }

    timeline.finish(started.elapsed());
    let report = report(
        &metrics,
        &timeline,
        &slowest,
        &anomalies,
        &load,
        started.elapsed(),
        warmup_requests,
        interrupted,
    );
    if let Some(path) = &args.snapshot {
        report.clone().with_snapshot(false).write_to(path)?;
    }
    let json = report.to_json()?;
    let comparison = if args.record {
        record_result(args, &json)?
    } else {
        None
    };
    // A rerun's answer is what moved; the run's own report is recorded
    match &comparison {
        Some(comparison) => println!("{}", comparison.to_json()?),
        None => println!("{json}"),
    }
    exit_if_failed(args, &metrics);
    finish_rerun(args, comparison.as_ref());
    Ok(())
}

/// Runs the setup screen (when asked for, or when there's nothing to run
/// yet) and then the screen of the mode it chose, until the user quits
pub(crate) async fn run_interactive(
    args: &Cli,
    mut setup: bool,
) -> Result<Farewell, Box<dyn std::error::Error>> {
    // Declared first so it's dropped last: restoring the terminal writes to
    // it, which can block too, and Ctrl-C must still get out then
    let _watchdog = CtrlCWatchdog::arm();
    let _terminal = TerminalGuard::enter()?;
    // What the command line gave: a saved test opens on top of it
    let base = args.clone();
    let mut args = args.clone();
    let mut farewell = Farewell::default();
    // Why the last start didn't happen, for the setup screen to say
    let mut error: Option<String> = None;
    // The library opens first when nothing at all was asked for and the
    // project has saved tests; ctrl-o on the form comes back to it
    let mut library =
        setup && args.url.is_empty() && args.command.is_none() && args.config_loaded.is_none();
    loop {
        if setup {
            if library {
                library = false;
                let store = crate::store::Store::local();
                let entries = store.entries();
                if !entries.is_empty() {
                    match ui::LibraryScreen::new(store.clone(), entries).run().await? {
                        ui::LibraryOutcome::Open(name) => match store.open(&name, &base) {
                            Ok(opened) => args = opened,
                            Err(e) => error = Some(e),
                        },
                        ui::LibraryOutcome::New => {}
                        ui::LibraryOutcome::Quit => return Ok(farewell),
                    }
                }
            }
            match ui::Setup::new(&args).with_error(error.take()).run().await? {
                ui::SetupOutcome::Start(chosen) => {
                    args = *chosen;
                    farewell.command = Some(args.command_line());
                }
                ui::SetupOutcome::Library => {
                    library = true;
                    continue;
                }
                ui::SetupOutcome::Quit => return Ok(farewell),
            }
            setup = false;
        }
        match args.command.clone() {
            Some(cli::Command::Ramp(ramp)) => {
                let plan = match RampPlan::from_args(&ramp) {
                    Ok(plan) => plan,
                    Err(e) => {
                        error = Some(e);
                        setup = true;
                        continue;
                    }
                };
                let request = args.request()?;
                let mut load = load::start(
                    clients_for(&request, &args, plan.peak() as usize)?.0,
                    request,
                    plan.levels[0] as usize,
                    Plan::Duration(UNTIL_STOPPED),
                    false,
                );
                let mut screen = ui::RampScreen::new(args.clone(), plan);
                match screen.run(&mut load).await? {
                    ui::Outcome::Restart => continue,
                    ui::Outcome::Edit => setup = true,
                    ui::Outcome::Quit => {
                        farewell.report = screen.report();
                        farewell.metrics = Some(screen.metrics().clone());
                        if !screen.recorded() {
                            farewell.result =
                                Some(serde_json::to_string_pretty(&screen.result_json())?);
                        }
                        return Ok(farewell);
                    }
                }
            }
            Some(cli::Command::Api(api)) => {
                let mut run = match crate::api::ApiRun::load(&api).await {
                    Ok(run) => run,
                    Err(e) => {
                        error = Some(e);
                        setup = true;
                        continue;
                    }
                };
                let (report, metrics) = api::api_session(&args, &mut run).await?;
                farewell.report = report;
                farewell.metrics = metrics;
                return Ok(farewell);
            }
            _ => {
                let (mut load, connects) = start_load(&args, true)?;
                let mut dashboard =
                    ui::Dashboard::new(args.clone(), plan(&args)).with_connects(connects);
                let outcome = dashboard.run(&mut load).await?;
                // Keep any concurrency the user dialed in during the run.
                // Dropping `load` stops the previous run before the next starts.
                args.concurrency = load.concurrency() as u32;
                match outcome {
                    ui::Outcome::Restart => continue,
                    ui::Outcome::Edit => setup = true,
                    ui::Outcome::Quit => {
                        farewell.report = dashboard.report();
                        farewell.metrics = Some(dashboard.metrics().clone());
                        if !dashboard.recorded() {
                            farewell.result = Some(dashboard.report_json(&load).to_json()?);
                        }
                        return Ok(farewell);
                    }
                }
            }
        }
    }
}

/// The screens (the setup form when `setup`, then the run), and what they
/// leave in the shell
pub(crate) async fn run_screens(args: &Cli, setup: bool) -> Result<(), Box<dyn std::error::Error>> {
    let check = update::Check::start();
    let farewell = run_interactive(args, setup).await?;
    if let Some(report) = farewell.report {
        print_report(&report);
    }
    if let Some(command) = farewell.command {
        println!("Run this again with:\n  {command}");
    }
    let comparison = match (&farewell.result, args.record) {
        (Some(json), true) => record_result(args, json)?,
        _ => None,
    };
    say_if_newer(check).await;
    if let Some(metrics) = farewell.metrics {
        exit_if_failed(args, &metrics);
    }
    finish_rerun(args, comparison.as_ref());
    Ok(())
}
