//! `pepe flow`: a sequence of requests, each step fed by the one before.

use super::*;

/// API mode: read the spec, then the plan screen and the dashboard
/// `pepe flow FILE`: the steps in order, each worker one user
pub(crate) async fn run_flow(
    args: &Cli,
    what: &cli::FlowArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let flow = match args
        .vars
        .as_deref()
        .map(crate::vars::Vars::load)
        .transpose()
        .and_then(|rows| crate::flow::load(&what.file, rows))
    {
        Ok(flow) => flow,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };
    // The guardrails: each step's host, as its URL stands before the run
    // (a host filled in by a capture can't be checked, and is refused
    // when hosts are named), for every row of `--vars`
    let guard = guard::Guard::of(args);
    let chains = flow.rows.as_ref().map_or(1, |rows| rows.rows.len());
    let mut urls = Vec::new();
    for step in &flow.steps {
        match (0..chains)
            .map(|chain| step.url.render(&flow.start(chain)))
            .collect::<Result<Vec<_>, _>>()
        {
            Ok(filled) => {
                for url in &filled {
                    if let Err(e) = guard.check_url(url) {
                        eprintln!("error: step {:?}: {e}", step.name);
                        std::process::exit(2);
                    }
                }
                urls.push(format!("{} {} {}", step.name, step.method, filled[0]));
            }
            Err(var) if guard.hosts.is_empty() => {
                urls.push(format!(
                    "{} {} {}",
                    step.name,
                    step.method,
                    step.url.source()
                ));
                let _ = var;
            }
            Err(var) => {
                eprintln!(
                    "error: step {:?}: its URL is filled in at run time ({{{{{var}}}}}), so --allow-host can't check it",
                    step.name
                );
                std::process::exit(2);
            }
        }
    }
    if let Err(e) = guard.check_run(args) {
        eprintln!("error: {e}");
        std::process::exit(2);
    }
    if args.dry_run {
        let plan = guard::Plan {
            mode: "flow",
            targets: urls,
            headers: guard::Plan::headers_of(&args.headers),
            load: format!(
                "{} step{} a chain; {}",
                flow.steps.len(),
                if flow.steps.len() == 1 { "" } else { "s" },
                guard::Plan::load_of(args, "chains")
            ),
            settings: guard::Plan::settings_of(args),
            guard: guard.describe(),
            ..Default::default()
        };
        return print_plan(args, &plan);
    }
    // The dashboard's title shows the flow rather than one URL
    let mut shown = args.clone();
    shown.method = "FLOW".into();
    shown.url = flow.name.clone();
    if args.json {
        return run_flow_json(&shown, flow).await;
    }
    if headless(args) {
        let (clients, connects) = flow_clients(&shown, &flow, shown.concurrency as usize)?;
        let load = load::start_flow(
            clients,
            flow.clone(),
            shown.concurrency as usize,
            plan(&shown),
            false,
        );
        load.set_rate(shown.rate);
        if let Some(warmup) = shown.warmup() {
            load.set_warmup(warmup);
        }
        let dashboard = ui::Dashboard::new(shown.clone(), plan(&shown))
            .with_steps(flow.views())
            .with_connects(connects);
        return headless_dashboard(&shown, dashboard, load).await;
    }
    let check = update::Check::start();
    let (report, metrics) = {
        let _watchdog = CtrlCWatchdog::arm();
        let _terminal = TerminalGuard::enter()?;
        flow_session(&mut shown, flow).await?
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

/// One client per shard, built around the flow's first URL: the shared
/// headers and settings are what matter
pub(crate) fn flow_clients(
    args: &Cli,
    flow: &crate::flow::Flow,
    concurrency: usize,
) -> Result<(load::Senders, std::sync::Arc<request::ConnectTimes>), PepeError> {
    let base = request::Request::new(
        flow.base_url(),
        "GET".into(),
        None,
        &args.headers,
        args.settings(),
    )?;
    base.build_clients(shards(args, concurrency))
}

pub(crate) async fn flow_session(
    shown: &mut Cli,
    flow: crate::flow::Flow,
) -> Result<(Option<String>, Option<Metrics>), Box<dyn std::error::Error>> {
    loop {
        let (clients, connects) = flow_clients(shown, &flow, shown.concurrency as usize)?;
        let mut load = load::start_flow(
            clients,
            flow.clone(),
            shown.concurrency as usize,
            plan(shown),
            true,
        );
        load.set_rate(shown.rate);
        if let Some(warmup) = shown.warmup() {
            load.set_warmup(warmup);
        }
        let mut dashboard = ui::Dashboard::new(shown.clone(), plan(shown))
            .with_steps(flow.views())
            .with_connects(connects);
        let outcome = dashboard.run(&mut load).await?;
        shown.concurrency = load.concurrency() as u32;
        match outcome {
            // There is no setup screen for a flow: edit the file, run again
            ui::Outcome::Restart | ui::Outcome::Edit => {}
            ui::Outcome::Quit => {
                return Ok((dashboard.report(), Some(dashboard.metrics().clone())))
            }
        }
    }
}

pub(crate) async fn run_flow_json(
    args: &Cli,
    flow: crate::flow::Flow,
) -> Result<(), Box<dyn std::error::Error>> {
    let name = flow.name.clone();
    let views = flow.views();
    let row_labels: Vec<String> = views.iter().map(|v| v.label.clone()).collect();
    let timeline = timeline::Timeline::default();
    let (clients, connects) = flow_clients(args, &flow, args.concurrency as usize)?;
    let mut load = load::start_flow(clients, flow, args.concurrency as usize, plan(args), false);
    load.set_rate(args.rate);
    if let Some(warmup) = args.warmup() {
        load.set_warmup(warmup);
    }
    crate::webhook::begin(hook_run(args), &args.hooks());
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
                    crate::webhook::progress(|| hook_progress(&total, started.elapsed(), plan(args)));
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
        .with_target("flow", None, &name, args.concurrency)
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
    let steps: Vec<serde_json::Value> = views
        .iter()
        .zip(&each)
        .map(|(view, m)| {
            serde_json::json!({
                "step": view.label,
                "requests": m.total,
                "failed_requests": m.total - m.success,
                "requests_per_second": m.rps(elapsed),
                "median_ms": ms(m.percentile(50.0)),
                "p99_ms": ms(m.percentile(99.0)),
                "status_codes": m.status_codes.iter().map(|(k, v)| (k.to_string(), *v)).collect::<std::collections::BTreeMap<_, _>>(),
            })
        })
        .collect();
    // Chains that went all the way: the last step's successes
    report["flow"] = serde_json::json!({
        "name": args.url,
        "chains_started": each.first().map_or(0, |m| m.total),
        "chains_completed": each.last().map_or(0, |m| m.success),
        "steps": steps,
    });
    println!("{}", serde_json::to_string_pretty(&report)?);
    hooks_end(&report, &crossed(args, &total));
    exit_if_failed(args, &total).await;
    Ok(())
}
