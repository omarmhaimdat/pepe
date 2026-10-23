//! `pepe logs`: nginx's logs, on a screen or as a report.

use super::*;

/// `pepe logs`: what nginx's logs say of its traffic, now and before
pub(crate) async fn run_logs(
    args: &Cli,
    what: &cli::LogsArgs,
) -> Result<(), Box<dyn std::error::Error>> {
    let fail = |message: String| -> ! {
        eprintln!("error: {message}");
        std::process::exit(2);
    };
    let parser = crate::logs::Parser::new(what.format.as_deref())
        .unwrap_or_else(|e| fail(format!("--format: {e}")));
    let window = crate::logs::span(&what.window).unwrap_or_else(|e| fail(format!("--window: {e}")));
    let conditions: Vec<crate::logs_verdict::Condition> = args
        .fail_if
        .iter()
        .map(|text| {
            crate::logs_verdict::Condition::parse(text)
                .unwrap_or_else(|e| fail(format!("--fail-if {e}")))
        })
        .collect();
    if let Some(on) = &args.webhook_on {
        crate::webhook::Event::parse_list(on)
            .unwrap_or_else(|e| fail(format!("--webhook-on: {e}")));
    }
    let hooks = args.hooks();
    if let Err(e) = crate::webhook::check(&hooks) {
        fail(format!("--webhook: {e}"));
    }
    let since = what
        .since
        .as_deref()
        .map(|since| crate::logs::span(since).unwrap_or_else(|e| fail(format!("--since: {e}"))))
        .map(|since| crate::logs::wall() - since);
    let until = what
        .until
        .as_deref()
        .map(|until| crate::logs::span(until).unwrap_or_else(|e| fail(format!("--until: {e}"))))
        .map(|until| crate::logs::wall() - until);
    if let (Some(since), Some(until)) = (since, until) {
        if until <= since {
            fail("--until is before --since: nothing is between them; --since 2h --until 1h is the hour before last".into());
        }
    }
    let mut statuses = Vec::new();
    for text in &what.statuses {
        statuses.extend(
            crate::logs::StatusMatch::parse_list(text)
                .unwrap_or_else(|e| fail(format!("--status {e}"))),
        );
    }
    for path in &what.paths {
        if !path.starts_with('/') && !path.starts_with('*') {
            fail(format!(
                "--path {path:?}: a path starts with /, as in /api/*"
            ));
        }
    }
    let filter = crate::logs::Filter {
        paths: what.paths.clone(),
        statuses,
        hosts: what.hosts.clone(),
        until,
    };
    let piped = !stdin().is_terminal();
    let mut files = what.files.clone();
    let dash = files.iter().any(|f| f.as_os_str() == "-");
    if dash && files.len() > 1 {
        fail("- reads what is piped in, and can't be mixed with files".into());
    }
    if dash {
        files.clear();
    } else if files.is_empty() && !piped {
        files = crate::logs::default_files();
        if files.is_empty() {
            fail(
                "no log to read: name one, as in `pepe logs /var/log/nginx/access.log`, \
                 or pipe one in"
                    .into(),
            );
        }
    }
    if let Some(missing) = files.iter().find(|f| !f.is_file()) {
        fail(format!("{} isn't a file", missing.display()));
    }
    // A log that is piped in has stdin; the screen needs it for its keys,
    // so the pipe is put aside and the terminal takes its place
    let mut screen = !args.json && stdout().is_terminal();
    let mut aside = None;
    if screen && files.is_empty() {
        aside = crate::logs::piped_aside();
        screen = aside.is_some();
    }
    // At a terminal a log that is being written is watched, not read: the
    // screen starts a few minutes back, enough for "now" to be right at
    // once and for the chart to have something in it. A log nobody is
    // writing has no now, and is read whole.
    let mut since = since;
    if screen && since.is_none() && until.is_none() && !what.all && !files.is_empty() {
        let wall = crate::logs::wall();
        if crate::logs::being_written(&files, &parser, wall) {
            since = Some(wall - window.max(crate::logs::LOOKBACK));
        }
    }
    let mut job = crate::logs::Job {
        files,
        piped: aside,
        parser,
        since,
        follow: screen,
        exact_paths: what.exact_paths,
        filter,
    };
    // A stretch in the past has nothing more coming
    if until.is_some() {
        job.follow = false;
    }
    // The title says what was kept, so a filtered log never reads as all
    let name = match job.filter.describe() {
        kept if kept.is_empty() => job.name(),
        kept => format!("{} · {kept}", job.name()),
    };
    let run = |load: String| crate::webhook::Run {
        mode: "logs".into(),
        target: name.clone(),
        command: command_line(),
        load,
        test: None,
        planned: None,
        rate: None,
        concurrency: 0,
        ..hook_run(args)
    };
    if !screen {
        job.follow = false;
        let shared = crate::logs::Shared::default();
        crate::logs::read(&job, &shared);
        let (report, checks) = {
            let stats = shared.lock();
            if let Some(trouble) = &stats.trouble {
                eprintln!("error: {trouble}");
            }
            let wall = crate::logs::wall();
            let clock = stats.clock(wall);
            let findings = crate::logs_verdict::findings(&stats, clock, window);
            let checks: Vec<crate::webhook::Check> = conditions
                .iter()
                .map(|c| c.check(&stats, clock, window, crate::logs_verdict::Scope::All))
                .collect();
            let report = full_report(&stats, &job, wall, window, what.rows, &findings, &checks);
            if args.json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!(
                    "{}",
                    with_verdict(
                        crate::logs::report(&stats, &name, wall, window, what.rows),
                        &findings,
                        &checks
                    )
                );
            }
            (report, checks)
        };
        if !hooks.is_empty() {
            crate::webhook::begin(run("the log as it was read".into()), &hooks);
            crate::webhook::end(
                &report,
                &crate::webhook::Ending {
                    checks: checks.clone(),
                    ..Default::default()
                },
            );
            crate::webhook::flush().await;
        }
        if checks.iter().any(|c| !c.held) {
            std::process::exit(4);
        }
        return Ok(());
    }
    let check = update::Check::start();
    let shared = crate::logs::start(job.clone());
    // --metrics: the page is rendered again every second while it follows
    if exporter::address().is_some() {
        let shared = shared.clone();
        let conditions = conditions.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                exporter::publish(|| {
                    exporter::logs_page(&shared.lock(), crate::logs::wall(), window, &conditions)
                });
            }
        });
    }
    if !hooks.is_empty() {
        crate::webhook::begin(run(format!("following {name}")), &hooks);
    }
    let watch = crate::logs_verdict::Watch::new(conditions, window);
    let (outcome, watch) = {
        let _watchdog = CtrlCWatchdog::arm();
        let _terminal = TerminalGuard::enter()?;
        let mut screen =
            ui::LogsScreen::new(shared.clone(), job.parser.clone(), name.clone(), window)
                .watching(watch);
        let outcome = screen.run().await;
        (outcome, screen.into_watch())
    };
    shared.stop();
    outcome?;
    let checks = watch.checks();
    {
        let stats = shared.lock();
        let wall = crate::logs::wall();
        let findings = crate::logs_verdict::findings(&stats, stats.clock(wall), window);
        print_report(&with_verdict(
            crate::logs::report(&stats, &name, wall, window, what.rows),
            &findings,
            &checks,
        ));
        if !hooks.is_empty() {
            let report = full_report(&stats, &job, wall, window, what.rows, &findings, &checks);
            crate::webhook::end(
                &report,
                &crate::webhook::Ending {
                    checks: checks.clone(),
                    ..Default::default()
                },
            );
        }
    }
    crate::webhook::flush().await;
    say_if_newer(check).await;
    if checks.iter().any(|c| !c.held) {
        std::process::exit(4);
    }
    Ok(())
}

/// The JSON report, with the verdict and each limit
fn full_report(
    stats: &crate::logs::Stats,
    job: &crate::logs::Job,
    wall: i64,
    window: i64,
    rows: usize,
    findings: &[crate::logs_verdict::Finding],
    checks: &[crate::webhook::Check],
) -> serde_json::Value {
    let mut report = crate::logs::json_report(stats, job, wall, window, rows);
    report["schema_version"] = serde_json::json!(1);
    let offset = stats.clock(wall).offset;
    if let serde_json::Value::Object(verdict) = crate::logs_verdict::json(findings, checks, offset)
    {
        for (key, value) in verdict {
            report[key.as_str()] = value;
        }
    }
    report
}

/// The text report, with the findings and the limits under its headline
fn with_verdict(
    report: String,
    findings: &[crate::logs_verdict::Finding],
    checks: &[crate::webhook::Check],
) -> String {
    let verdict = crate::logs_verdict::text(findings, checks);
    if verdict.is_empty() {
        return report;
    }
    // The title, and the headline: what is said follows them
    let mut lines = report.splitn(3, '\n');
    let (title, headline, rest) = (
        lines.next().unwrap_or(""),
        lines.next().unwrap_or(""),
        lines.next().unwrap_or(""),
    );
    if headline.is_empty() {
        return format!("{report}{verdict}");
    }
    format!("{title}\n{headline}\n{verdict}\n{rest}")
}

/// The command as it was typed, a webhook's URL masked: a webhook's path
/// is its secret
fn command_line() -> String {
    std::env::args()
        .enumerate()
        .map(|(i, arg)| {
            let arg = match i {
                0 => "pepe".to_string(),
                _ if arg.contains("://") => match arg.split_once('=') {
                    Some((flag, url)) if flag.starts_with("--") => {
                        format!("{flag}={}", crate::webhook::mask(url))
                    }
                    _ => crate::webhook::mask(&arg),
                },
                _ => arg,
            };
            match arg.contains(' ') || arg.contains('\'') || arg.contains('*') {
                true => format!("'{}'", arg.replace('\'', r"'\''")),
                false => arg,
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
