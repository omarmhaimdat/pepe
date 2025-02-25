use std::io::stdout;
use std::sync::Arc;

use clap::Parser;
use crossterm::{
    cursor::Show,
    event::KeyCode,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, LeaveAlternateScreen},
};
use tokio::sync::{mpsc, Semaphore};

use crate::cli::Cli;
use crate::request::Request;
use crate::response::ResponseStats;
use crate::utils::resolve_dns;

mod cache;
mod cli;
mod request;
mod response;
mod ui;
mod utils;

#[derive(Debug, Clone)]
struct Sent {
    count: usize,
}

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

async fn handle_request(
    client: Arc<reqwest::Client>,
    request: Request,
    tx: mpsc::Sender<ResponseStats>,
    sent_tx: mpsc::Sender<Sent>,
    permit: tokio::sync::OwnedSemaphorePermit,
) {
    let start = std::time::Instant::now();
    let method = request.method();

    let _ = sent_tx.send(Sent { count: 1 }).await;

    let dns_times = resolve_dns(&request.url).await.unwrap_or_default();

    let response = if method == reqwest::Method::POST && request.body.is_some() {
        client
            .request(method, &request.url)
            .body(request.body.unwrap())
            .send()
            .await
    } else {
        client.request(method, &request.url).send().await
    };

    let stats = ResponseStats::from_response(response, start, dns_times).await;

    drop(permit);
    let _ = tx.send(stats).await;
}

async fn run_request(
    args: &Cli,
    tx: mpsc::Sender<ResponseStats>,
    sent_tx: mpsc::Sender<Sent>,
) -> Result<(Vec<ResponseStats>, std::time::Duration), PepeError> {
    let request = args.request();
    let client = Arc::new(request.build_client()?);
    let all_start = std::time::Instant::now();
    let semaphore = Arc::new(Semaphore::new(args.concurrency as usize));

    let handler = tokio::spawn({
        let client = client.clone();
        let tx = tx;
        let sent_tx = sent_tx;
        let number = args.number;

        async move {
            for _ in 0..number {
                let semaphore = semaphore.clone();
                let permit = semaphore
                    .acquire_owned()
                    .await
                    .expect("Semaphore acquire failed");

                tokio::spawn(handle_request(
                    client.clone(),
                    request.clone(),
                    tx.clone(),
                    sent_tx.clone(),
                    permit,
                ));
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
    let mut args = Cli::parse();

    if let Err(e) = args.validate() {
        eprintln!("{}", e);
        std::process::exit(1);
    }

    enable_raw_mode()?;
    let mut stdout = stdout();

    let interrupted = Arc::new(tokio::sync::Notify::new());

    'main: loop {
        let (tx, mut rx) = mpsc::channel(args.number as usize);
        let (sent_tx, mut sent_rx) = mpsc::channel(args.number as usize);

        let handler = tokio::spawn({
            let args = args.clone();
            async move { run_request(&args.clone(), tx, sent_tx).await }
        });

        let mut dashboard = ui::Dashboard::new(args.clone());

        let result: Result<KeyCode, Box<dyn std::error::Error>> =
            dashboard.run(&mut rx, &mut sent_rx);

        match result {
            Ok(KeyCode::Char('r')) => {
                handler.abort();
                continue 'main;
            }
            Ok(KeyCode::Char('q')) | Ok(KeyCode::Esc) | Ok(KeyCode::Enter) => {
                break;
            }
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

    execute!(
        stdout,
        LeaveAlternateScreen,
        Show,
        crossterm::terminal::Clear(crossterm::terminal::ClearType::All),
        crossterm::cursor::MoveTo(0, 0)
    )?;

    disable_raw_mode()?;
    args.check_for_updates().await?;
    Ok(())
}
