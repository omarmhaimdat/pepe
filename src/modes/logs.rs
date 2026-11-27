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
    let mut hooks = args.hooks();
    if let Err(e) = crate::webhook::check(&hooks) {
        fail(format!("--webhook: {e}"));
    }
    // A dry run posts nothing, a webhook included
    if args.dry_run {
        hooks.clear();
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
    let serving = what.serve.is_some();
    if serving && args.json {
        fail(
            "--serve shows the logs as a page while they are followed; --json reads them once"
                .into(),
        );
    }
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
    if (screen || serving) && since.is_none() && until.is_none() && !what.all && !files.is_empty() {
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
        follow: screen || serving,
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
    if !screen && !serving {
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
    // --serve: bound before anything is read, so a taken port says so at once
    let listener = match &what.serve {
        Some(spec) => Some(
            crate::serve::logs::listen(spec)
                .await
                .unwrap_or_else(|e| fail(e)),
        ),
        None => None,
    };
    let shared = crate::logs::start(job.clone());
    // The links to the page, for the screen to show again
    let mut page = Vec::new();
    if let Some(listener) = listener {
        let token = crate::serve::new_token();
        let addr = listener.local_addr()?;
        let hub = crate::serve::logs::Hub::new(
            shared.clone(),
            job.parser.clone(),
            job.exact_paths,
            name.clone(),
            window,
            conditions.clone(),
            token.clone(),
        );
        tokio::spawn(crate::serve::logs::serve(hub, listener));
        let ways = crate::serve::logs::links(addr, &token);
        eprintln!("pepe logs · {name}");
        let wide = ways.iter().map(|w| w.link.len()).max().unwrap_or(0);
        for (i, way) in ways.iter().enumerate() {
            let head = if i == 0 { "  page   " } else { "         " };
            eprintln!("{head}{:<wide$}   {}", way.link, way.from);
        }
        if screen {
            // The screen is about to take the terminal: the links stay
            // until enter is pressed, and `w` on the screen shows them again
            if crate::slack::open_browser(&ways[0].link) {
                eprintln!("  opened in the browser; w on the screen shows these links again");
            } else {
                eprintln!("  open a link above; w on the screen shows them again");
            }
            if !until_enter(START_WAIT).await {
                shared.stop();
                std::process::exit(130);
            }
            page = ways;
        }
    }
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
    let (outcome, watch) = match screen {
        true => {
            let _watchdog = CtrlCWatchdog::arm();
            let _terminal = TerminalGuard::enter()?;
            let mut screen =
                ui::LogsScreen::new(shared.clone(), job.parser.clone(), name.clone(), window)
                    .watching(watch)
                    .serving(page);
            let outcome = screen.run().await;
            (outcome, screen.into_watch())
        }
        // Followed until stopped, the page the only thing shown: the limits
        // are watched as the screen watches them, a crossing kept
        false => (Ok(()), followed(&shared, watch).await),
    };
    shared.stop();
    outcome?;
    let checks = watch.checks();
    {
        let stats = shared.lock();
        if !screen {
            if let Some(trouble) = &stats.trouble {
                eprintln!("error: {trouble}");
            }
        }
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

/// How long the links are left on the terminal before the screen starts
/// on its own
const START_WAIT: std::time::Duration = std::time::Duration::from_secs(60);

/// Wait for enter, or `wait`, counting down on one line; false when asked
/// to quit (q, esc, Ctrl-C). The page is served all the while: the wait is
/// on a thread of its own
async fn until_enter(wait: std::time::Duration) -> bool {
    use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
    use std::io::Write as _;
    let waited = tokio::task::spawn_blocking(move || {
        if crossterm::terminal::enable_raw_mode().is_err() {
            return true;
        }
        let end = std::time::Instant::now() + wait;
        let mut shown = u64::MAX;
        let go = loop {
            let left = end.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                break true;
            }
            let secs = left.as_secs() + 1;
            if secs != shown {
                shown = secs;
                eprint!("\r  enter starts the screen · q quits · on its own in {secs} s \x1b[K");
                let _ = std::io::stderr().flush();
            }
            if !event::poll(left.min(std::time::Duration::from_millis(250))).unwrap_or(false) {
                continue;
            }
            match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => match key.code {
                    KeyCode::Enter | KeyCode::Char(' ') => break true,
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        break false
                    }
                    KeyCode::Char('q') | KeyCode::Esc => break false,
                    _ => {}
                },
                Ok(_) => {}
                Err(_) => break true,
            }
        };
        let _ = crossterm::terminal::disable_raw_mode();
        eprint!("\r\x1b[K");
        go
    });
    waited.await.unwrap_or(true)
}

/// Follow the logs with no screen until stopped, by Ctrl-C or, as a
/// service or a container is stopped, SIGTERM: the limits ticked each second
async fn followed(
    shared: &crate::logs::Shared,
    mut watch: crate::logs_verdict::Watch,
) -> crate::logs_verdict::Watch {
    let stop = stopped();
    tokio::pin!(stop);
    let mut every = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        tokio::select! {
            _ = &mut stop => return watch,
            _ = every.tick() => watch.tick(&shared.lock(), crate::logs::wall()),
        }
    }
}

/// Resolves on Ctrl-C, or SIGTERM where there is one
async fn stopped() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        if let Ok(mut term) = signal(SignalKind::terminate()) {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => {}
                _ = term.recv() => {}
            }
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
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
