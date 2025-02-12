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
use curl_parser;
use hyper::{HeaderMap, Uri};
use reqwest::{header::USER_AGENT, Proxy};
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
    #[clap(long = "curl", help = "Curl command to convert to HTTP request")]
    curl: bool,
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
    #[clap(
        short,
        long,
        help = "Proxy server URL: http://user:pass@host:port or socks5://host:port"
    )]
    proxy: Option<String>,
    #[clap(short, long)]
    host: Option<String>,
    #[clap(long)]
    disable_compression: bool,
    #[clap(long)]
    disable_keepalive: bool,
    #[clap(long)]
    disable_redirects: bool,
    #[clap(default_value = "")]
    url: String,
    #[clap(last = true, default_value = "")]
    args: Vec<String>,
}

fn default_concurrency() -> u32 {
    available_parallelism()
        .unwrap_or(NonZeroUsize::new(8).unwrap())
        .get() as u32
}
impl Args {
    fn validate(&mut self) {
        if self.concurrency > self.number {
            eprintln!(
                "Error: Number of workers cannot be smaller than the number of requests. -c {} -n {}",
                self.concurrency, self.number
            );
            std::process::exit(1);
        }

        if self.curl == false && self.url.is_empty() {
            eprintln!("Error: URL cannot be empty.");
            std::process::exit(1);
        }

        // Verify Timeout
        if self.timeout == 0 || self.timeout > 120 {
            eprintln!("Error: Timeout cannot be 0, or greater than 120 seconds.");
            std::process::exit(1);
        }

        if self.curl {
            // Print the curl command
            let curl_command = self
                .args
                .iter()
                .map(|arg| {
                    if arg.contains(' ') || arg.contains('{') {
                        format!("'{}'", arg)
                    } else {
                        arg.clone()
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            println!("Curl command: {}", curl_command);
            let parsed_request = curl_parser::ParsedRequest::load(&curl_command, Some(()));
            if parsed_request.is_err() {
                eprintln!("Error: {}", parsed_request.err().unwrap());
                std::process::exit(1);
            }
            self.method = parsed_request.as_ref().unwrap().method.clone().to_string();
            self.url = parsed_request.as_ref().unwrap().url.clone().to_string();
            self.headers = parsed_request
                .as_ref()
                .unwrap()
                .headers
                .clone()
                .iter()
                .map(|(k, v)| format!("{}: {}", k, v.to_str().unwrap()))
                .collect();
            self.body = Some(parsed_request.as_ref().unwrap().body.clone().join(" "));
            // print body
            if let Some(body) = &self.body {
                println!("Body: {}", body);
            }
        }

        // Check if method is valid
        let method = reqwest::Method::from_bytes(self.method.as_bytes());
        if !method.is_ok() {
            eprintln!("Error: Invalid method: {}", self.method);
            std::process::exit(1);
        }

        if self.proxy.is_some() {
            if self.proxy.as_ref().unwrap().starts_with("socks4") {
                eprintln!("Error: Socks4 proxy is not supported by reqwest.");
                std::process::exit(1);
            }
            let proxy = Proxy::all(self.proxy.as_ref().unwrap());
            if proxy.is_err() {
                eprintln!("Error: Invalid proxy URL: {}", self.proxy.as_ref().unwrap());
                std::process::exit(1);
            }
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
        "x-vercel-cache",
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
    let mut client_builder = reqwest::Client::builder()
        .default_headers(request_headers)
        .timeout(std::time::Duration::from_secs(args.timeout as u64));

    if let Some(proxy_url) = &args.proxy {
        let proxy =
            Proxy::all(proxy_url).map_err(|e| PepeError::HeaderParseError(e.to_string()))?;
        client_builder = client_builder.proxy(proxy);
    }

    if args.disable_compression {
        client_builder = client_builder.no_gzip();
    }

    if args.disable_keepalive {
        client_builder = client_builder.connection_verbose(true);
    }

    if args.disable_redirects {
        client_builder = client_builder.redirect(reqwest::redirect::Policy::none());
    }

    client_builder = client_builder.timeout(std::time::Duration::from_secs(args.timeout as u64));

    let client = Arc::new(
        client_builder
            .build()
            .map_err(|e| PepeError::RequestError(e))?,
    );
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
                let body = args.body.clone();

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

                    // if post and body is not empty then send post request
                    let response = if method == reqwest::Method::POST && body.is_some() {
                        client
                            .request(method, &url)
                            .body(body.unwrap())
                            .send()
                            .await
                    } else {
                        client.request(method, &url).send().await
                    };

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
    let mut args = Args::parse();
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
