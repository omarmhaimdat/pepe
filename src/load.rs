use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

use crate::request::Request;
use crate::response::ResponseStats;
use crate::utils::resolve_dns;

/// How long a run lasts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Send exactly this many requests
    Count(u64),
    /// Keep sending until this much time has passed
    Duration(Duration),
}

/// A running load test. Results arrive on `rx`; the channel closes once every
/// request has finished (or the run was stopped). Dropping the handle stops it.
pub struct LoadHandle {
    pub rx: mpsc::UnboundedReceiver<ResponseStats>,
    /// Requests started so far
    pub sent: Arc<AtomicU64>,
    task: JoinHandle<()>,
}

impl LoadHandle {
    /// Stop sending and cancel in-flight requests
    pub fn stop(&self) {
        self.task.abort();
    }

    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }
}

impl Drop for LoadHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub fn start(
    client: reqwest::Client,
    request: Request,
    concurrency: usize,
    plan: Plan,
) -> LoadHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let sent = Arc::new(AtomicU64::new(0));
    let task = tokio::spawn(generate(
        Arc::new(client),
        Arc::new(request),
        concurrency.max(1),
        plan,
        tx,
        sent.clone(),
    ));
    LoadHandle { rx, sent, task }
}

async fn generate(
    client: Arc<reqwest::Client>,
    request: Arc<Request>,
    concurrency: usize,
    plan: Plan,
    tx: mpsc::UnboundedSender<ResponseStats>,
    sent: Arc<AtomicU64>,
) {
    let semaphore = Arc::new(Semaphore::new(concurrency));
    // Owning every request task here means aborting this task (restart,
    // interrupt, Ctrl-C) drops the JoinSet, which cancels them all.
    let mut tasks = JoinSet::new();
    let deadline = match plan {
        Plan::Duration(d) => Some(tokio::time::Instant::now() + d),
        Plan::Count(_) => None,
    };
    let mut started = 0u64;

    loop {
        if let Plan::Count(n) = plan {
            if started >= n {
                break;
            }
        }

        let acquire = semaphore.clone().acquire_owned();
        let permit = match deadline {
            // Don't sit waiting for a free slot past the end of the test
            Some(deadline) => tokio::select! {
                permit = acquire => permit,
                _ = tokio::time::sleep_until(deadline) => break,
            },
            None => acquire.await,
        };
        let Ok(permit) = permit else { break };
        if deadline.is_some_and(|d| tokio::time::Instant::now() >= d) {
            break;
        }

        started += 1;
        sent.fetch_add(1, Ordering::Relaxed);
        tasks.spawn(send_one(
            client.clone(),
            request.clone(),
            tx.clone(),
            permit,
        ));

        // Reap finished tasks so long runs don't accumulate them
        while tasks.try_join_next().is_some() {}
    }

    while tasks.join_next().await.is_some() {}
    // `tx` and every clone are dropped now, which closes the channel
}

async fn send_one(
    client: Arc<reqwest::Client>,
    request: Arc<Request>,
    tx: mpsc::UnboundedSender<ResponseStats>,
    permit: OwnedSemaphorePermit,
) {
    let dns_times = resolve_dns(&request.url).await.ok();

    let mut builder = client.request(request.method(), &request.url);
    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }

    let start = Instant::now();
    let response = builder.send().await;
    let stats = ResponseStats::from_response(response, start, dns_times).await;

    drop(permit);
    let _ = tx.send(stats);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RequestSettings;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Minimal HTTP server; returns its URL and a counter of received bodies
    async fn server(delay: Duration) -> (String, Arc<AtomicU64>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let bodies = Arc::new(AtomicU64::new(0));
        let seen = bodies.clone();
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let seen = seen.clone();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    if String::from_utf8_lossy(&buf[..n]).ends_with("\r\n\r\nhello") {
                        seen.fetch_add(1, Ordering::Relaxed);
                    }
                    tokio::time::sleep(delay).await;
                    let _ = sock
                        .write_all(
                            b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                        )
                        .await;
                });
            }
        });
        (url, bodies)
    }

    fn request(url: &str, method: &str, body: Option<&str>) -> Request {
        let settings = RequestSettings {
            timeout: 5,
            disable_compression: false,
            disable_keepalive: true,
            disable_redirects: false,
            proxy: None,
            user_agent: "pepe/test".into(),
        };
        Request::new(
            url.into(),
            method.into(),
            body.map(Into::into),
            &[],
            settings,
        )
        .unwrap()
    }

    async fn drain(mut load: LoadHandle) -> Vec<ResponseStats> {
        let mut out = Vec::new();
        while let Some(stat) = load.rx.recv().await {
            out.push(stat);
        }
        out
    }

    #[tokio::test]
    async fn count_plan_sends_exactly_n() {
        let (url, _) = server(Duration::ZERO).await;
        let req = request(&url, "GET", None);
        let load = start(req.build_client().unwrap(), req, 4, Plan::Count(25));
        let results = drain(load).await;
        assert_eq!(results.len(), 25);
        assert!(results
            .iter()
            .all(|r| r.status_code.map(|s| s.as_u16()) == Some(200)));
        assert!(results.iter().all(|r| r.body_bytes == 2));
    }

    #[tokio::test]
    async fn duration_plan_stops_on_time() {
        let (url, _) = server(Duration::from_millis(5)).await;
        let req = request(&url, "GET", None);
        let begin = Instant::now();
        let load = start(
            req.build_client().unwrap(),
            req,
            2,
            Plan::Duration(Duration::from_millis(300)),
        );
        let results = drain(load).await;
        assert!(!results.is_empty());
        assert!(
            begin.elapsed() < Duration::from_secs(2),
            "took {:?}",
            begin.elapsed()
        );
    }

    #[tokio::test]
    async fn stop_cancels_in_flight_requests() {
        // Server never answers in time, so only stop() can end the run
        let (url, _) = server(Duration::from_secs(30)).await;
        let req = request(&url, "GET", None);
        let load = start(req.build_client().unwrap(), req, 4, Plan::Count(1000));
        tokio::time::sleep(Duration::from_millis(100)).await;
        load.stop();
        let begin = Instant::now();
        let results = tokio::time::timeout(Duration::from_secs(2), drain(load)).await;
        assert!(
            results.is_ok(),
            "channel should close promptly after stop()"
        );
        assert!(begin.elapsed() < Duration::from_secs(2));
    }

    #[tokio::test]
    async fn body_is_sent_for_put_and_patch() {
        let (url, bodies) = server(Duration::ZERO).await;
        for method in ["POST", "PUT", "PATCH"] {
            let req = request(&url, method, Some("hello"));
            drain(start(req.build_client().unwrap(), req, 1, Plan::Count(1))).await;
        }
        assert_eq!(bodies.load(Ordering::Relaxed), 3);
    }
}
