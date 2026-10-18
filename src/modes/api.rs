//! `pepe api`: every endpoint of an OpenAPI spec.

use super::*;

/// The endpoints an API run is to send to, or an error saying how to pick
/// some, since there is no plan screen to do it on
pub(crate) fn endpoints_or_exit(run: &crate::api::ApiRun) -> Vec<usize> {
    let which = run.enabled();
    if which.is_empty() {
        let tags: Vec<&str> = run
            .spec
            .tags
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        eprintln!(
            "error: no endpoint to run. Pick some with --all, --tag NAME or --only PATTERN \
             (tags: {}); --set gives parameters their values, --include-writes allows writes",
            tags.join(", ")
        );
        std::process::exit(1);
    }
    which
}

/// API mode with `--json`: run the endpoints that are on, print one report
pub(crate) async fn run_api_json(
    args: &Cli,
    run: &crate::api::ApiRun,
) -> Result<(), Box<dyn std::error::Error>> {
    // Nobody to prompt here: say what's missing instead
    if let (Some(scheme), true) = (run.spec.auth.first(), run.credentials.is_empty()) {
        eprintln!(
            "note: this API declares {} and no credentials were given; pass {}",
            scheme.describe(),
            scheme.hint()
        );
    }
    let which = run.enabled();
    if which.is_empty() {
        let tags: Vec<&str> = run
            .spec
            .tags
            .iter()
            .map(|(name, _)| name.as_str())
            .collect();
        eprintln!(
            "error: no endpoint to run. Pick some with --all, --tag NAME or --only PATTERN \
             (tags: {}); --set gives parameters their values, --include-writes allows writes",
            tags.join(", ")
        );
        std::process::exit(1);
    }
    let targets = run.targets(args, &which)?;
    let row_labels: Vec<String> = which
        .iter()
        .map(|&index| run.endpoints[index].label.clone())
        .collect();
    let timeline = timeline::Timeline::default();
    let (clients, connects) = run.clients(args, shards(args, args.concurrency as usize))?;
    let mut load = load::start_targets(
        clients,
        targets,
        args.concurrency as usize,
        plan(args),
        false,
    );
    load.set_rate(args.rate);
    if let Some(warmup) = args.warmup() {
        load.set_warmup(warmup);
    }
    crate::webhook::begin(hook_run(args), &args.hooks());
    let mut started = Instant::now();
    let mut warming = load.warming();
    let mut warmup_requests = 0;
    let mut total = Metrics::default();
    let mut each = vec![Metrics::default(); which.len()];
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
        }
    }
    let elapsed = started.elapsed();
    let report = json_report::JsonReport::generate(&total, elapsed, interrupted)
        .with_generator(
            load.threads(),
            load.peak_busy(),
            load.rate().map(|r| (r, load.missed())),
        )
        .with_target("api", None, &run.spec.base_url, args.concurrency)
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
    let endpoints: Vec<serde_json::Value> = which
        .iter()
        .zip(&each)
        .map(|(&index, m)| {
            serde_json::json!({
                "endpoint": run.endpoints[index].label,
                "requests": m.total,
                "failed_requests": m.total - m.success,
                "requests_per_second": m.rps(elapsed),
                "median_ms": ms(m.percentile(50.0)),
                "p99_ms": ms(m.percentile(99.0)),
                "status_codes": m.status_codes.iter().map(|(k, v)| (k.to_string(), *v)).collect::<std::collections::BTreeMap<_, _>>(),
            })
        })
        .collect();
    report["endpoints"] = serde_json::Value::Array(endpoints);
    println!("{}", serde_json::to_string_pretty(&report)?);
    hooks_end(args, &report, Some(&total), Vec::new());
    exit_if_failed(args, &total).await;
    Ok(())
}

/// The plan screen, then the dashboard with one row per endpoint, and
/// back, until the user quits. Returns the report of the last run. The
/// terminal is already the dashboard's.
pub(crate) async fn api_session(
    args: &Cli,
    run: &mut crate::api::ApiRun,
) -> Result<(Option<String>, Option<Metrics>), Box<dyn std::error::Error>> {
    // The dashboard's title shows the API rather than one URL
    let mut shown = args.clone();
    shown.method = "API".into();
    shown.url = run.spec.base_url.clone();
    let mut planning = true;
    loop {
        if planning {
            match ui::PlanScreen::new(run, &mut shown).run().await? {
                ui::PlanOutcome::Start => planning = false,
                ui::PlanOutcome::Quit => return Ok((None, None)),
            }
            shown.url = run.spec.base_url.clone();
        }
        let which = run.enabled();
        let targets = run.targets(&shown, &which)?;
        let (clients, connects) =
            run.clients(&shown, shards(&shown, shown.concurrency as usize))?;
        let mut load = load::start_targets(
            clients,
            targets,
            shown.concurrency as usize,
            plan(&shown),
            true,
        );
        if let Some(warmup) = shown.warmup() {
            load.set_warmup(warmup);
        }
        load.set_rate(shown.rate);
        let mut dashboard = ui::Dashboard::new(shown.clone(), plan(&shown))
            .with_endpoints(run.views(&shown, &which))
            .with_connects(connects);
        let outcome = dashboard.run(&mut load).await?;
        shown.concurrency = load.concurrency() as u32;
        match outcome {
            ui::Outcome::Restart => {}
            ui::Outcome::Edit => planning = true,
            ui::Outcome::Quit => {
                return Ok((dashboard.report(), Some(dashboard.metrics().clone())))
            }
        }
    }
}

pub(crate) async fn run_api(
    args: &Cli,
    api: &cli::ApiArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut run = match crate::api::ApiRun::load(api).await {
        Ok(run) => run,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    let guard = guard::Guard::of(args);
    if let Err(e) = guard
        .check_url(&run.spec.base_url)
        .and_then(|_| guard.check_run(args))
    {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
    if args.dry_run {
        let which = run.enabled();
        let labels: Vec<String> = which
            .iter()
            .map(|&i| run.endpoints[i].label.clone())
            .collect();
        let plan = guard::Plan {
            mode: "api",
            targets: std::iter::once(run.spec.base_url.clone())
                .chain(labels.iter().take(20).cloned())
                .collect(),
            more_targets: labels.len().saturating_sub(20),
            headers: guard::Plan::headers_of(&args.headers),
            load: format!(
                "{} endpoint{} on; {}",
                labels.len(),
                if labels.len() == 1 { "" } else { "s" },
                guard::Plan::load_of(args, "requests")
            ),
            settings: guard::Plan::settings_of(args),
            guard: guard.describe(),
            notes: if labels.is_empty() {
                vec!["no endpoint is on: --all, --tag or --only pick some".into()]
            } else {
                Vec::new()
            },
            ..Default::default()
        };
        return print_plan(args, &plan);
    }
    if args.json {
        return run_api_json(args, &run).await;
    }
    if headless(args) {
        let which = endpoints_or_exit(&run);
        let targets = run.targets(args, &which)?;
        let (clients, connects) = run.clients(args, shards(args, args.concurrency as usize))?;
        let load = load::start_targets(
            clients,
            targets,
            args.concurrency as usize,
            plan(args),
            false,
        );
        load.set_rate(args.rate);
        if let Some(warmup) = args.warmup() {
            load.set_warmup(warmup);
        }
        let mut shown = args.clone();
        shown.method = "API".into();
        shown.url = run.spec.base_url.clone();
        let dashboard = ui::Dashboard::new(shown, plan(args))
            .with_endpoints(run.views(args, &which))
            .with_connects(connects);
        return headless_dashboard(args, dashboard, load).await;
    }

    let check = update::Check::start();
    let (report, metrics) = {
        let _watchdog = CtrlCWatchdog::arm();
        let _terminal = TerminalGuard::enter()?;
        api_session(args, &mut run).await?
    };
    if let Some(report) = report {
        print_report(&report);
    }
    say_if_newer(check).await;
    if let Some(metrics) = metrics {
        exit_if_failed(args, &metrics).await;
    }
    Ok(())
}
