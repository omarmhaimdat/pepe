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
    let since = what
        .since
        .as_deref()
        .map(|since| crate::logs::span(since).unwrap_or_else(|e| fail(format!("--since: {e}"))))
        .map(|since| crate::logs::wall() - since);
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
    if screen && since.is_none() && !what.all && !files.is_empty() {
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
    };
    let name = job.name();
    if !screen {
        job.follow = false;
        let shared = crate::logs::Shared::default();
        crate::logs::read(&job, &shared);
        let stats = shared.lock();
        if let Some(trouble) = &stats.trouble {
            eprintln!("error: {trouble}");
        }
        if args.json {
            let mut report =
                crate::logs::json_report(&stats, &job, crate::logs::wall(), window, what.rows);
            report["schema_version"] = serde_json::json!(1);
            println!("{}", serde_json::to_string_pretty(&report)?);
        } else {
            print!(
                "{}",
                crate::logs::report(&stats, &name, crate::logs::wall(), window, what.rows)
            );
        }
        return Ok(());
    }
    let check = update::Check::start();
    let shared = crate::logs::start(job.clone());
    let outcome = {
        let _watchdog = CtrlCWatchdog::arm();
        let _terminal = TerminalGuard::enter()?;
        ui::LogsScreen::new(shared.clone(), job.parser.clone(), name.clone(), window)
            .run()
            .await
    };
    shared.stop();
    outcome?;
    print_report(&crate::logs::report(
        &shared.lock(),
        &name,
        crate::logs::wall(),
        window,
        what.rows,
    ));
    say_if_newer(check).await;
    Ok(())
}
