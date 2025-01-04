use clap::{ArgAction::HelpLong, Parser};
use crossterm::{
    cursor::{Hide, Show},
    event::KeyCode,
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, Clear, ClearType, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use reqwest::header::USER_AGENT;
use std::io::stdout;
use std::sync::Arc;
use std::{num::NonZeroUsize, thread::available_parallelism};
use tokio::sync::{mpsc, Semaphore};

mod ui;

#[derive(Parser, Debug, Clone)]
#[command(name = "pepe")]
#[command(version = "0.0.1")]
#[command(about = "HTTP load generator")]
// #[command(long_about = USAGE)]
#[clap(disable_help_flag = true)]
struct Args {
    #[arg(long, action = HelpLong)]
    help: Option<bool>,
    #[clap(short = 'n', long, default_value = "200")]
    number: u32,
    #[clap(short = 'c', long, default_value = "50")]
    concurrency: u32,
    #[clap(short = 'q', long, default_value = "0")]
    rate_limit: u32,
    #[clap(short = 'z', long)]
    duration: Option<String>,
    #[clap(short = 'o', long)]
    output: Option<String>,
    #[clap(short = 'm', long, default_value = "GET")]
    method: String,
    #[clap(short = 'H', long)]
    headers: Vec<String>,
    #[clap(short = 't', long, default_value = "20")]
    timeout: u32,
    #[clap(short = 'A', long)]
    accept: Option<String>,
    #[clap(short = 'd', long)]
    body: Option<String>,
    #[clap(short = 'D', long)]
    body_file: Option<String>,
    #[clap(short = 'T', long, default_value = "text/html")]
    content_type: String,
    #[clap(short, long, default_value = "pepe/0.0.1")]
    user_agent: String,
    #[clap(short, long)]
    basic_auth: Option<String>,
    #[clap(short, long)]
    proxy: Option<String>,
    #[clap(short, long)]
    host: Option<String>,
    #[clap(long)]
    disable_compression: bool,
    #[clap(long)]
    disable_keepalive: bool,
    #[clap(long)]
    disable_redirects: bool,
    #[clap(long)]
    cpus: Option<u32>,
    url: String,
}

fn default_values(args: &Args) {
    if let Some(default_n_threads) = NonZeroUsize::new(8) {
        let n_threads: usize = available_parallelism()
            .unwrap_or(NonZeroUsize::new(8).unwrap_or(default_n_threads))
            .get();
        if args.cpus.is_none() {
            eprintln!(
                "Warning: Number of CPUs not specified. Using {} CPUs.",
                n_threads
            );
        }
    }
    if args.concurrency > args.number {
        eprintln!(
            "Error: Number of workers cannot be smaller than the number of requests. -c {} -n {}",
            args.concurrency, args.number
        );
        std::process::exit(1);
    }

    // Check if method is valid
    let method = reqwest::Method::from_bytes(args.method.as_bytes());
    if !method.is_ok() {
        eprintln!("Error: Invalid method: {}", args.method);
        std::process::exit(1);
    }
}

#[derive(Debug, Clone)]
struct ResponseStats {
    url: String,
    method: String,
    duration: std::time::Duration,
    status_code: reqwest::StatusCode,
    content_length: Option<u64>,
    elapsed: std::time::Duration,
    total_requests: usize,
    concurrency: usize,
    partial_response: Option<String>,
}

async fn run(
    args: &Args,
    tx: mpsc::Sender<ResponseStats>,
) -> (Vec<ResponseStats>, std::time::Duration) {
    // Clone values we need before spawning tasks
    let number = args.number;
    let url = args.url.clone();
    let method = args.method.clone();
    let user_agent = args.user_agent.clone();
    let headers = args.headers.clone();
    let concurrency = args.concurrency;

    let mut request_headers = reqwest::header::HeaderMap::new();
    // Add user agent if not present
    request_headers.insert(USER_AGENT, user_agent.parse().unwrap());
    request_headers.extend(
        headers
            .iter()
            .map(|header| {
                let parts: Vec<&str> = header.splitn(2, ':').collect();
                if parts.len() == 2 {
                    (
                        reqwest::header::HeaderName::from_bytes(parts[0].as_bytes()).unwrap(),
                        reqwest::header::HeaderValue::from_str(parts[1]).unwrap(),
                    )
                } else {
                    (
                        reqwest::header::HeaderName::from_bytes(parts[0].as_bytes()).unwrap(),
                        reqwest::header::HeaderValue::from_str("").unwrap(),
                    )
                }
            })
            .collect::<reqwest::header::HeaderMap>(),
    );
    

    let semaphore = Arc::new(Semaphore::new(concurrency as usize));
    let client = Arc::new(reqwest::Client::new());

    let all_start = std::time::Instant::now();
    // Spawn request handler
    let handler = tokio::spawn({
        let client = client.clone();

        async move {
            for _i in 0..number {
                let sem = semaphore.clone();
                let tx = tx.clone();
                let permit = sem.acquire_owned().await.unwrap();
                let client = client.clone();
                let url = url.clone();
                let method = method.clone();
                let headers = request_headers.clone();

                tokio::spawn(async move {
                    let start = std::time::Instant::now();
                    let response = client
                        .request(
                            reqwest::Method::from_bytes(method.as_bytes()).unwrap(),
                            &url,
                        )
                        .headers(
                            headers
                        )
                        .send()
                        .await;

                    let stats = match response {
                        Ok(resp) => ResponseStats {
                            duration: start.elapsed(),
                            status_code: resp.status(),
                            content_length: resp.content_length(),
                            url,
                            method,
                            elapsed: all_start.elapsed(),
                            total_requests: number as usize,
                            concurrency: concurrency as usize,
                            // Add partial response if it's not too long max 1000 chars
                            partial_response: resp
                                .text()
                                .await
                                .ok()
                                .map(|text| text.replace("\n", " ").replace("\r", " "))
                                .map(|text| {
                                    if text.len() > 100 {
                                        text[..100].to_string()
                                    } else {
                                        text
                                    }
                                }),
                        },
                        Err(e) => ResponseStats {
                            duration: start.elapsed(),
                            status_code: reqwest::StatusCode::INTERNAL_SERVER_ERROR,
                            content_length: None,
                            url,
                            method,
                            elapsed: all_start.elapsed(),
                            total_requests: number as usize,
                            concurrency: concurrency as usize,
                            partial_response: Some(e.to_string()),
                        },
                    };

                    drop(permit);
                    let _ = tx.send(stats).await;
                });
            }
        }
    });

    handler.await.unwrap();

    let elapsed = all_start.elapsed();

    let responses = Vec::new();

    return (responses, elapsed);
}

#[derive(Default)]
struct Stats {
    count: usize,
    success: usize,
    failed: usize,
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    default_values(&args);

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, Clear(ClearType::All), Hide)?;

    let interrupted = Arc::new(tokio::sync::Notify::new());

    // Main application loop
    'main: loop {
        let (tx, mut rx) = mpsc::channel(args.number as usize);

        // Spawn request handler
        let handler = tokio::spawn({
            let args = args.clone();
            async move { run(&args, tx).await }
        });

        // Run dashboard
        let mut dashboard = ui::Dashboard::new();
        let result: Result<KeyCode, Box<dyn std::error::Error>> = dashboard.run(&mut rx);

        // Handle restart command
        match result {
            Ok(KeyCode::Char('r')) => {
                handler.abort();
                continue 'main;
            }
            Ok(KeyCode::Char('q')) => break,
            Ok(KeyCode::Esc) => break,
            Ok(KeyCode::Enter) => break,
            Ok(KeyCode::Char('i')) => {
                // Interrupt all tasks and keep the dashboard running
                interrupted.notify_one();
                handler.abort();
            }
            Err(e) => {
                execute!(stdout, LeaveAlternateScreen, Show)?;
                disable_raw_mode()?;
                return Err(e.into());
            }
            _ => break,
        }
    }

    // Cleanup
    execute!(stdout, LeaveAlternateScreen, Show)?;
    disable_raw_mode()?;
    Ok(())
}
