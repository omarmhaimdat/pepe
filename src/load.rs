use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, watch, OwnedSemaphorePermit, Semaphore};
use tokio::task::{JoinHandle, JoinSet};

use crate::request::Request;
use crate::response::ResponseStats;
use crate::utils::resolve_dns;

/// Upper bound for live concurrency changes from the dashboard
pub const MAX_CONCURRENCY: usize = 100_000;
/// DNS is probed at most this often. Probing on every request put a blocking
/// `getaddrinfo` call in front of each one, which cost more than many of the
/// requests themselves.
const DNS_PROBE_INTERVAL: Duration = Duration::from_secs(1);

/// Full responses the inspector keeps per second, for successes and for
/// failures separately. Below this rate every response is kept; above it,
/// copying every one would cost real throughput, so they're sampled evenly.
const DETAILS_PER_SECOND: u32 = 1_000;

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
    semaphore: Arc<Semaphore>,
    concurrency: AtomicUsize,
    paused: watch::Sender<bool>,
}

impl LoadHandle {
    /// Stop sending and cancel in-flight requests
    pub fn stop(&self) {
        self.task.abort();
    }

    pub fn sent(&self) -> u64 {
        self.sent.load(Ordering::Relaxed)
    }

    pub fn concurrency(&self) -> usize {
        self.concurrency.load(Ordering::Relaxed)
    }

    /// Change how many requests may be in flight, while the run is going.
    /// Returns the new value, clamped to `1..=MAX_CONCURRENCY`.
    pub fn set_concurrency(&self, target: usize) -> usize {
        let target = target.clamp(1, MAX_CONCURRENCY);
        let old = self.concurrency.swap(target, Ordering::Relaxed);
        match target.cmp(&old) {
            std::cmp::Ordering::Greater => self.semaphore.add_permits(target - old),
            std::cmp::Ordering::Less => {
                // Retire permits as in-flight requests hand them back. The
                // semaphore is fair, so this waits ahead of new requests and
                // the lower limit applies as soon as enough requests finish.
                let semaphore = self.semaphore.clone();
                let surplus = (old - target) as u32;
                tokio::spawn(async move {
                    if let Ok(permits) = semaphore.acquire_many_owned(surplus).await {
                        permits.forget();
                    }
                });
            }
            std::cmp::Ordering::Equal => {}
        }
        target
    }

    /// Hold off starting new requests; in-flight ones still complete
    pub fn set_paused(&self, paused: bool) {
        self.paused.send_replace(paused);
    }

    pub fn is_paused(&self) -> bool {
        *self.paused.borrow()
    }
}

impl Drop for LoadHandle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Lets one request per interval measure DNS, without any locking
struct DnsSampler {
    epoch: Instant,
    /// Milliseconds since `epoch` when the next probe is due
    next_ms: AtomicU64,
}

impl DnsSampler {
    fn new() -> Self {
        Self {
            epoch: Instant::now(),
            next_ms: AtomicU64::new(0),
        }
    }

    /// True for exactly one caller per interval
    fn claim(&self) -> bool {
        let now = self.epoch.elapsed().as_millis() as u64;
        let next = self.next_ms.load(Ordering::Relaxed);
        now >= next
            && self
                .next_ms
                .compare_exchange(
                    next,
                    now + DNS_PROBE_INTERVAL.as_millis() as u64,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
    }
}

/// Picks which responses to keep in full: every one while the rate is low,
/// otherwise one in N spread evenly over each second (N from the previous
/// second's rate), up to `DETAILS_PER_SECOND`
struct DetailBudget {
    epoch: Instant,
    /// Second (since `epoch`) the counts below belong to
    second: AtomicU64,
    ok: Sampler,
    failed: Sampler,
}

#[derive(Default)]
struct Sampler {
    /// Responses seen and kept this second
    seen: std::sync::atomic::AtomicU32,
    kept: std::sync::atomic::AtomicU32,
    /// Keep one in this many
    stride: std::sync::atomic::AtomicU32,
}

impl Sampler {
    /// Start a new second, sampling so the last second's rate would fit
    fn roll(&self) {
        let seen = self.seen.swap(0, Ordering::Relaxed);
        self.kept.store(0, Ordering::Relaxed);
        self.stride
            .store(seen.div_ceil(DETAILS_PER_SECOND).max(1), Ordering::Relaxed);
    }

    fn claim(&self) -> bool {
        let n = self.seen.fetch_add(1, Ordering::Relaxed);
        let stride = self.stride.load(Ordering::Relaxed).max(1);
        n % stride == 0 && self.kept.fetch_add(1, Ordering::Relaxed) < DETAILS_PER_SECOND
    }
}

impl DetailBudget {
    fn new() -> Self {
        Self {
            epoch: Instant::now(),
            second: AtomicU64::new(0),
            ok: Sampler::default(),
            failed: Sampler::default(),
        }
    }

    /// Whether this response may be kept in full
    fn claim(&self, failed: bool) -> bool {
        let now = self.epoch.elapsed().as_secs();
        let second = self.second.load(Ordering::Relaxed);
        if now != second
            && self
                .second
                .compare_exchange(second, now, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
        {
            self.ok.roll();
            self.failed.roll();
        }
        if failed { &self.failed } else { &self.ok }.claim()
    }
}

/// What every request task shares
struct Shared {
    client: reqwest::Client,
    request: Request,
    dns: DnsSampler,
    /// Keep the start of each body for the dashboard's preview column, and
    /// some responses in full for its inspector
    previews: bool,
    details: DetailBudget,
}

pub fn start(
    client: reqwest::Client,
    request: Request,
    concurrency: usize,
    plan: Plan,
    previews: bool,
) -> LoadHandle {
    let (tx, rx) = mpsc::unbounded_channel();
    let (paused, paused_rx) = watch::channel(false);
    let sent = Arc::new(AtomicU64::new(0));
    let concurrency = concurrency.clamp(1, MAX_CONCURRENCY);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let shared = Arc::new(Shared {
        client,
        request,
        dns: DnsSampler::new(),
        previews,
        details: DetailBudget::new(),
    });
    let task = tokio::spawn(generate(
        shared,
        semaphore.clone(),
        plan,
        tx,
        sent.clone(),
        paused_rx,
    ));
    LoadHandle {
        rx,
        sent,
        task,
        semaphore,
        concurrency: AtomicUsize::new(concurrency),
        paused,
    }
}

async fn generate(
    shared: Arc<Shared>,
    semaphore: Arc<Semaphore>,
    plan: Plan,
    tx: mpsc::UnboundedSender<ResponseStats>,
    sent: Arc<AtomicU64>,
    mut paused: watch::Receiver<bool>,
) {
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

        let acquire = async {
            while *paused.borrow_and_update() {
                if paused.changed().await.is_err() {
                    break;
                }
            }
            semaphore.clone().acquire_owned().await
        };
        let permit = match deadline {
            // Don't sit waiting for a free slot (or a resume) past the end
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
        tasks.spawn(send_one(shared.clone(), tx.clone(), permit));

        // Reap finished tasks so long runs don't accumulate them
        while tasks.try_join_next().is_some() {}
    }

    while tasks.join_next().await.is_some() {}
    // `tx` and every clone are dropped now, which closes the channel
}

async fn send_one(
    shared: Arc<Shared>,
    tx: mpsc::UnboundedSender<ResponseStats>,
    permit: OwnedSemaphorePermit,
) {
    let request = &shared.request;
    let dns_times = if shared.dns.claim() {
        resolve_dns(&request.url).await.ok()
    } else {
        None
    };

    let mut builder = shared.client.request(request.method.clone(), &request.url);
    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }

    let start = Instant::now();
    let response = builder.send().await;
    let ttfb = start.elapsed();
    let capture = shared.previews
        && matches!(&response, Ok(r) if shared.details.claim(!r.status().is_success()));
    let stats =
        ResponseStats::from_response(response, start, ttfb, dns_times, shared.previews, capture)
            .await;

    drop(permit);
    let _ = tx.send(stats);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::request::RequestSettings;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    struct Server {
        url: String,
        /// Requests whose body was "hello"
        bodies: Arc<AtomicU64>,
        /// Requests currently being handled, and the most seen at once
        inflight: Arc<AtomicU64>,
        peak: Arc<AtomicU64>,
    }

    /// Minimal HTTP server that answers every request after `delay`
    async fn server(delay: Duration) -> Server {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/", listener.local_addr().unwrap());
        let bodies = Arc::new(AtomicU64::new(0));
        let inflight = Arc::new(AtomicU64::new(0));
        let peak = Arc::new(AtomicU64::new(0));
        let (seen, active, most) = (bodies.clone(), inflight.clone(), peak.clone());
        tokio::spawn(async move {
            loop {
                let (mut sock, _) = listener.accept().await.unwrap();
                let (seen, active, most) = (seen.clone(), active.clone(), most.clone());
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    let n = sock.read(&mut buf).await.unwrap_or(0);
                    let now = active.fetch_add(1, Ordering::SeqCst) + 1;
                    most.fetch_max(now, Ordering::SeqCst);
                    if String::from_utf8_lossy(&buf[..n]).ends_with("\r\n\r\nhello") {
                        seen.fetch_add(1, Ordering::Relaxed);
                    }
                    tokio::time::sleep(delay).await;
                    active.fetch_sub(1, Ordering::SeqCst);
                    let _ = sock
                        .write_all(
                            b"HTTP/1.1 200 OK\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok",
                        )
                        .await;
                });
            }
        });
        Server {
            url,
            bodies,
            inflight,
            peak,
        }
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
        let srv = server(Duration::ZERO).await;
        let req = request(&srv.url, "GET", None);
        let load = start(req.build_client().unwrap(), req, 4, Plan::Count(25), true);
        let results = drain(load).await;
        assert_eq!(results.len(), 25);
        assert!(results
            .iter()
            .all(|r| r.status_code.map(|s| s.as_u16()) == Some(200)));
        assert!(results.iter().all(|r| r.body_bytes == 2));
        assert!(results
            .iter()
            .all(|r| r.preview_text().as_deref() == Some("ok")));
        // A small run is captured in full
        let detail = results[0].detail.as_ref().expect("captured");
        assert_eq!(&detail.body[..], b"ok");
        assert_eq!(detail.headers.get("content-length").unwrap(), "2");
        assert!(!detail.truncated && detail.remote_addr.is_some());
        assert!(results
            .iter()
            .all(|r| r.ttfb.is_some_and(|t| t <= r.duration)));
        // DNS is sampled, not probed for every request
        let probes = results.iter().filter(|r| r.dns_times.is_some()).count();
        assert!((1..5).contains(&probes), "probes={probes}");
    }

    #[tokio::test]
    async fn duration_plan_stops_on_time() {
        let srv = server(Duration::from_millis(5)).await;
        let req = request(&srv.url, "GET", None);
        let begin = Instant::now();
        let load = start(
            req.build_client().unwrap(),
            req,
            2,
            Plan::Duration(Duration::from_millis(300)),
            false,
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
        let srv = server(Duration::from_secs(30)).await;
        let req = request(&srv.url, "GET", None);
        let load = start(
            req.build_client().unwrap(),
            req,
            4,
            Plan::Count(1000),
            false,
        );
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
        let srv = server(Duration::ZERO).await;
        for method in ["POST", "PUT", "PATCH"] {
            let req = request(&srv.url, method, Some("hello"));
            drain(start(
                req.build_client().unwrap(),
                req,
                1,
                Plan::Count(1),
                false,
            ))
            .await;
        }
        assert_eq!(srv.bodies.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn detail_sampling_spreads_over_the_second() {
        let sampler = Sampler::default();
        // First second: nothing known yet, keep up to the limit
        let kept = (0..3_000).filter(|_| sampler.claim()).count();
        assert_eq!(kept, DETAILS_PER_SECOND as usize);
        // Next second, sized from 3,000/s: one in three, all through it
        sampler.roll();
        let picks: Vec<usize> = (0..3_000).filter(|_| sampler.claim()).collect();
        assert_eq!(picks.len(), 1_000);
        assert_eq!(
            *picks.last().unwrap(),
            2_997,
            "reaches the end of the second"
        );
        // A quiet second: every response kept
        sampler.roll();
        sampler.roll();
        assert!((0..10).all(|_| sampler.claim()));
    }

    #[test]
    fn failures_have_their_own_budget() {
        let budget = DetailBudget::new();
        for _ in 0..5_000 {
            budget.claim(false);
        }
        assert!(budget.claim(true));
    }

    #[tokio::test]
    async fn concurrency_can_change_mid_run() {
        let srv = server(Duration::from_millis(40)).await;
        let req = request(&srv.url, "GET", None);
        let load = start(
            req.build_client().unwrap(),
            req,
            2,
            Plan::Count(10_000),
            false,
        );

        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(srv.peak.load(Ordering::SeqCst), 2);

        assert_eq!(load.set_concurrency(6), 6);
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(srv.peak.load(Ordering::SeqCst), 6);

        // Lowering waits for in-flight requests, then holds the new limit
        load.set_concurrency(1);
        tokio::time::sleep(Duration::from_millis(150)).await;
        srv.peak.store(0, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(srv.peak.load(Ordering::SeqCst), 1);
        assert_eq!(load.concurrency(), 1);

        assert_eq!(load.set_concurrency(0), 1, "clamped to at least one");
    }

    #[tokio::test]
    async fn pause_holds_new_requests_until_resumed() {
        let srv = server(Duration::from_millis(5)).await;
        let req = request(&srv.url, "GET", None);
        let load = start(
            req.build_client().unwrap(),
            req,
            2,
            Plan::Count(10_000),
            false,
        );
        tokio::time::sleep(Duration::from_millis(50)).await;

        load.set_paused(true);
        assert!(load.is_paused());
        tokio::time::sleep(Duration::from_millis(50)).await;
        let held = load.sent();
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert_eq!(load.sent(), held, "nothing sent while paused");
        assert_eq!(srv.inflight.load(Ordering::SeqCst), 0);

        load.set_paused(false);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(load.sent() > held);
    }
}
