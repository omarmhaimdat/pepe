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
use hyper::{HeaderMap, Uri};
use reqwest::header::USER_AGENT;
use std::io::stdout;
use std::str::FromStr;
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
    #[clap(short = 'c', long, default_value_t = default_concurrency())]
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
    url: String,
}

fn default_concurrency() -> u32 {
    available_parallelism()
        .unwrap_or(NonZeroUsize::new(8).unwrap())
        .get() as u32
}
impl Args {
    fn validate(&self) {
        if self.concurrency > self.number {
            eprintln!(
                "Error: Number of workers cannot be smaller than the number of requests. -c {} -n {}",
                self.concurrency, self.number
            );
            std::process::exit(1);
        }

        // Check if method is valid
        let method = reqwest::Method::from_bytes(self.method.as_bytes());
        if !method.is_ok() {
            eprintln!("Error: Invalid method: {}", self.method);
            std::process::exit(1);
        }

        // Verify Timeout
        if self.timeout == 0 || self.timeout > 120 {
            eprintln!("Error: Timeout cannot be 0, or greater than 120 seconds.");
            std::process::exit(1);
        }
    }
}

#[derive(Debug, Clone)]
struct ResponseStats {
    duration: std::time::Duration,
    status_code: Option<reqwest::StatusCode>,
    content_length: Option<u64>,
    partial_response: Option<String>,
    dns_times: Option<(std::time::Duration, std::time::Duration)>,
    cache_status: Option<CacheStatus>,
}

#[derive(Debug, Clone)]
struct Sent {
    count: usize,
}

// Add custom error type
#[derive(Debug)]
enum PepeError {
    HeaderParseError(String),
    IoError(std::io::Error),
    RequestError(reqwest::Error),
    UrlParseError(hyper::http::uri::InvalidUri),
    HostParseError,
}

impl std::fmt::Display for PepeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::HeaderParseError(msg) => write!(f, "Header parse error: {}", msg),
            Self::RequestError(e) => write!(f, "Request error: {}", e),
            Self::IoError(e) => write!(f, "IO error: {}", e),
            Self::UrlParseError(e) => write!(f, "URL parse error: {}", e),
            Self::HostParseError => write!(f, "Host parse error"),
        }
    }
}

impl std::error::Error for PepeError {}

// Compute dns resolution time, dns lookup time
async fn resolve_dns(url: &str) -> Result<(std::time::Duration, std::time::Duration), PepeError> {
    let uri = Uri::from_str(url).map_err(|e| PepeError::UrlParseError(e))?;
    let host = uri.host().ok_or_else(|| PepeError::HostParseError)?;

    let start = std::time::Instant::now();
    let addrs = match tokio::net::lookup_host(format!("{}:0", host)).await {
        Ok(addrs) => addrs,
        Err(e) => {
            // eprintln!("DNS lookup failed for host {}: {}", host, e);
            return Err(PepeError::IoError(std::io::Error::new(
                std::io::ErrorKind::Other,
                e,
            )));
        }
    };
    let dns_lookup_time = start.elapsed();

    let start = std::time::Instant::now();
    let _ = addrs.collect::<Vec<_>>();
    let dns_resolution_time = start.elapsed();

    Ok((dns_lookup_time, dns_resolution_time))
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CacheStatus {
    Hit,
    Miss,
    Stale,
    Expired,
    Revalidated,
    Bypass,
    Dynamic,
    Error,
    Unknown,
}

#[derive(Hash, Debug, PartialEq, Eq, Clone)]
pub enum CacheCategory {
    Hit,
    Miss,
    Unknown,
}

impl CacheCategory {
    pub fn from_cache_status(status: &CacheStatus) -> CacheCategory {
        match status {
            CacheStatus::Hit | CacheStatus::Revalidated | CacheStatus::Stale => CacheCategory::Hit,
            CacheStatus::Miss
            | CacheStatus::Expired
            | CacheStatus::Bypass
            | CacheStatus::Dynamic => CacheCategory::Miss,
            CacheStatus::Error | CacheStatus::Unknown => CacheCategory::Unknown,
        }
    }
}

impl CacheStatus {
    // Parse a cache status string into the CacheStatus enum
    pub fn from_str(status: &str) -> CacheStatus {
        match status.to_lowercase().as_str() {
            "hit" => CacheStatus::Hit,
            "miss" => CacheStatus::Miss,
            "stale" => CacheStatus::Stale,
            "expired" => CacheStatus::Expired,
            "revalidated" => CacheStatus::Revalidated,
            "bypass" => CacheStatus::Bypass,
            "dynamic" => CacheStatus::Dynamic,
            "error" => CacheStatus::Error,
            _ => CacheStatus::Unknown,
        }
    }
}

/// Function to parse cache status from `reqwest` headers
pub fn parse_cache_status(headers: &HeaderMap) -> Option<CacheStatus> {
    let cache_headers = [
        "x-cache",
        "x-cache-status",
        "cf-cache-status",
        "x-cache-lookup",
        "x-cdn-cache-status",
        "x-backend-cache-status",
    ];

    for header in cache_headers {
        if let Some(value) = headers.get(header) {
            if let Ok(value_str) = value.to_str() {
                return Some(CacheStatus::from_str(value_str));
            }
        }
    }

    None
}

// Modified request headers handling
async fn run(
    args: &Args,
    tx: mpsc::Sender<ResponseStats>,
    sent_tx: mpsc::Sender<Sent>,
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
                let timeout = std::time::Duration::from_secs(args.timeout as u64);

                let sent_tx = sent_tx.clone();

                tokio::spawn(async move {
                    let start = std::time::Instant::now();
                    let method = reqwest::Method::from_bytes(method.as_bytes())
                        .unwrap_or(reqwest::Method::GET);

                    // Send a message to the sent channel that a request has been sent
                    let _ = sent_tx.send(Sent { count: 1 }).await;

                    // Compute dns resolution time, dns lookup time
                    let dns_times: (std::time::Duration, std::time::Duration) =
                        resolve_dns(&url).await.unwrap_or_default();

                    let response = client
                        .request(method.clone(), &url)
                        .headers(headers)
                        .timeout(timeout)
                        .send()
                        .await;

                    let response_headers = response
                        .as_ref()
                        .map(|r| r.headers().clone())
                        .unwrap_or_default();

                    // Parse cache status
                    let cache_status = parse_cache_status(&response_headers);

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
                                status_code: Some(status_code),
                                content_length,
                                partial_response: Some(truncated_text),
                                dns_times: Some(dns_times),
                                cache_status,
                            }
                        }
                        Err(e) => {
                            // Capture timeout errors
                            let status_code = e.status();
                            let content_length = None;
                            let partial_response = None;
                            ResponseStats {
                                duration: start.elapsed(),
                                status_code,
                                content_length,
                                partial_response,
                                dns_times: Some(dns_times),
                                cache_status,
                            }
                        }
                    };

                    drop(permit);
                    if tx.send(stats).await.is_err() {
                        Ok::<(), ()>(())
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
    args.validate();

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, Clear(ClearType::All), Hide)?;

    let interrupted = Arc::new(tokio::sync::Notify::new());

    'main: loop {
        let (tx, mut rx) = mpsc::channel(args.number as usize);
        let (sent_tx, mut sent_rx) = mpsc::channel(args.number as usize);

        let handler = tokio::spawn({
            let args = args.clone();
            async move { run(&args, tx, sent_tx).await }
        });

        let mut dashboard = ui::Dashboard::new(args.clone());

        let result: Result<KeyCode, Box<dyn std::error::Error>> =
            dashboard.run(&mut rx, &mut sent_rx);

        match result {
            Ok(KeyCode::Char('r')) => {
                handler.abort();
                continue 'main;
            }
            Ok(KeyCode::Char('q')) => break,
            Ok(KeyCode::Esc) => break,
            Ok(KeyCode::Enter) => break,
            Ok(KeyCode::Char('i')) => {
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

    execute!(stdout, LeaveAlternateScreen, Show)?;
    disable_raw_mode()?;
    Ok(())
}
