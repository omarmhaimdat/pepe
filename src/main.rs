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
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = "HTTP load generator")]
// #[command(long_about = USAGE)]
#[clap(disable_help_flag = true)]
struct Args {
    #[arg(long, action = HelpLong)]
    help: Option<bool>,
    #[clap(short = 'n', long, default_value = "100")]
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
    #[clap(short, long, default_value = concat!("pepe/", env!("CARGO_PKG_VERSION")))]
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

// Add custom error type
#[derive(Debug)]
enum PepeError {
    HeaderParseError(String),
    RequestError(reqwest::Error),
    IoError(std::io::Error),
}

impl std::fmt::Display for PepeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderParseError(msg) => write!(f, "Header parse error: {}", msg),
            Self::RequestError(e) => write!(f, "Request error: {}", e),
            Self::IoError(e) => write!(f, "IO error: {}", e),
        }
    }
}

impl std::error::Error for PepeError {}

// Modified request headers handling
async fn run(
    args: &Args,
    tx: mpsc::Sender<ResponseStats>,
) -> Result<(Vec<ResponseStats>, std::time::Duration), PepeError> {
    let mut request_headers = reqwest::header::HeaderMap::new();

    // Add user agent
    request_headers.insert(
        USER_AGENT,
        args.user_agent
            .parse::<reqwest::header::HeaderValue>()
            .map_err(|e| PepeError::HeaderParseError(e.to_string()))?,
    );

    // Parse headers safely
    for header in &args.headers {
        let parts: Vec<&str> = header.splitn(2, ':').collect();
        if parts.len() == 2 {
            let name = reqwest::header::HeaderName::from_bytes(parts[0].trim().as_bytes())
                .map_err(|e| PepeError::HeaderParseError(e.to_string()))?;
            let value = reqwest::header::HeaderValue::from_str(parts[1].trim())
                .map_err(|e| PepeError::HeaderParseError(e.to_string()))?;
            request_headers.insert(name, value);
        }
    }

    let semaphore = Arc::new(Semaphore::new(args.concurrency as usize));
    let client = Arc::new(reqwest::Client::new());
    let all_start = std::time::Instant::now();

    // Spawn request handler
    let handler = tokio::spawn({
        let client = client.clone();
        let args = args.clone();
        async move {
            for _i in 0..args.number {
                let sem = semaphore.clone();
                let tx = tx.clone();
                let permit = sem.acquire_owned().await.expect("Semaphore acquire failed");
                let client = client.clone();
                let url = args.url.clone();
                let method = args.method.clone();
                let headers = request_headers.clone();

                tokio::spawn(async move {
                    let start = std::time::Instant::now();
                    let method = reqwest::Method::from_bytes(method.as_bytes())
                        .unwrap_or(reqwest::Method::GET);

                    let response = client
                        .request(method.clone(), &url)
                        .headers(headers)
                        .send()
                        .await;

                    let stats = match response {
                        Ok(resp) => {
                            let status_code = resp.status();
                            let content_length = resp.content_length();
                            let text = resp.text().await.unwrap_or_default();
                            let text = text.trim().replace("\n", " ").replace("\r", " ");
                            let truncated_text = if text.len() > 100 {
                                text[..100].to_string()
                            } else {
                                text
                            };

                            ResponseStats {
                                duration: start.elapsed(),
                                status_code,
                                content_length,
                                url,
                                method: method.to_string(),
                                elapsed: all_start.elapsed(),
                                total_requests: args.number as usize,
                                concurrency: args.concurrency as usize,
                                partial_response: Some(truncated_text),
                            }
                        }
                        Err(e) => {
                            return Err(PepeError::RequestError(e));
                        }
                    };

                    drop(permit);
                    if tx.send(stats).await.is_err() {
                        Ok(())
                    } else {
                        Ok(())
                    }
                });
            }
        }
    });

    handler.await.map_err(|e| {
        PepeError::IoError(std::io::Error::new(
            std::io::ErrorKind::Other,
            e.to_string(),
        ))
    })?;

    Ok((Vec::new(), all_start.elapsed()))
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

    'main: loop {
        let (tx, mut rx) = mpsc::channel(args.number as usize);

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
