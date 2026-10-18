//! `pepe ramp`: the steps, climbed with the screens or as `--json`.

use super::*;

/// Climb the ramp's steps to its end, or Ctrl-C
pub(crate) async fn climb(
    args: &Cli,
    plan: RampPlan,
) -> Result<(Ramp, LoadHandle), Box<dyn std::error::Error>> {
    let (mut load, _) = start_requests(
        args,
        plan.peak() as usize,
        plan.levels[0] as usize,
        Plan::Duration(UNTIL_STOPPED),
        false,
    )?;
    let mut ramp = Ramp::new(plan, Instant::now());
    crate::webhook::begin(hook_run(args), &args.hooks());
    let mut control = crate::control::listen();
    let mut clock = tokio::time::interval(std::time::Duration::from_millis(50));
    let mut pump = tokio::time::interval(PUMP);
    while ramp.end.is_none() {
        tokio::select! {
            _ = pump.tick() => {
                let now = Instant::now();
                if !load.drain(|stat| ramp.record(&stat, now)) {
                    break;
                }
                crate::webhook::progress(|| ramp_progress(&ramp, now));
            }
            _ = clock.tick() => {
                if let Tick::Level(level) = ramp.tick(Instant::now()) {
                    load.set_concurrency(level as usize);
                }
            }
            _ = tokio::signal::ctrl_c() => {
                ramp.stop(Instant::now());
            }
            // A ramp sets its own concurrency: it can be paused or stopped
            said = crate::control::next(&mut control) => {
                if !matches!(said.0, crate::control::Command::Concurrency(_))
                    && crate::control::apply(&said, &load)
                {
                    ramp.stop(Instant::now());
                }
            }
        }
    }
    load.stop();
    let mut report = crate::ramp::json(&ramp);
    crate::store::stamp_test(&mut report, args);
    hooks_end(args, &report, Some(&ramp.total), Vec::new(), None);
    Ok((ramp, load))
}

/// Ramp mode with `--json`: climb the steps, print what each measured
pub(crate) async fn run_ramp_json(
    args: &Cli,
    plan: RampPlan,
) -> Result<(), Box<dyn std::error::Error>> {
    let (ramp, load) = climb(args, plan).await?;
    let mut report = crate::ramp::json(&ramp);
    report["generator"] = serde_json::to_value(json_report::Generator {
        threads: load.threads(),
        peak_busy_percent: load.peak_busy(),
        warmup_s: None,
        warmup_requests: None,
        rate_per_second: None,
        rate_missed: None,
    })?;
    crate::store::stamp_test(&mut report, args);
    let json = serde_json::to_string_pretty(&report)?;
    let comparison = if args.record {
        record_result(args, &json)?
    } else {
        None
    };
    match &comparison {
        Some(comparison) => println!("{}", comparison.to_json()?),
        None => println!("{json}"),
    }
    exit_if_failed(args, &ramp.total).await;
    finish_rerun(args, comparison.as_ref());
    Ok(())
}
