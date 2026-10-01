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
/// One request in the mix a run sends
pub struct Target {
    pub request: Request,
    /// Headers for this request only; those every request shares are the
    /// client's defaults
    pub headers: reqwest::header::HeaderMap,
    /// Which endpoint the results count toward
    pub endpoint: u16,
    /// Share of the traffic relative to the other targets
    pub weight: u32,
}

/// Longest repeating sequence used to mix targets by weight
const MAX_SCHEDULE: usize = 4_096;

/// The order targets are sent in: each appears in proportion to its weight,
/// spread out rather than in runs
fn schedule(targets: &[Target]) -> Vec<u16> {
    let total: u64 = targets.iter().map(|t| t.weight.max(1) as u64).sum();
    let scale = (MAX_SCHEDULE as f64 / total as f64).min(1.0);
    let counts: Vec<usize> = targets
        .iter()
        .map(|t| ((t.weight.max(1) as f64 * scale).round() as usize).max(1))
        .collect();
    let len: usize = counts.iter().sum();
    // Smooth weighted round-robin: always send the target furthest behind
    let mut credit = vec![0i64; targets.len()];
    let mut order = Vec::with_capacity(len);
    for _ in 0..len {
        for (c, &n) in credit.iter_mut().zip(&counts) {
            *c += n as i64;
        }
        let next = (0..targets.len()).max_by_key(|&i| credit[i]).unwrap_or(0);
        credit[next] -= len as i64;
        order.push(next as u16);
    }
    order
}

struct Shared {
    client: reqwest::Client,
    targets: Vec<Target>,
    /// Indexes into `targets`, cycled through
    schedule: Vec<u16>,
    next: AtomicUsize,
    dns: DnsSampler,
    /// Keep the start of each body for the dashboard's preview column, and
    /// some responses in full for its inspector
    previews: bool,
    details: DetailBudget,
}

/// Load one request
pub fn start(
    client: reqwest::Client,
    request: Request,
    concurrency: usize,
    plan: Plan,
    previews: bool,
) -> LoadHandle {
    let target = Target {
        request,
        headers: Default::default(),
        endpoint: 0,
        weight: 1,
    };
    start_targets(client, vec![target], concurrency, plan, previews)
}

/// Load a mix of requests, each in proportion to its weight
pub fn start_targets(
    client: reqwest::Client,
    targets: Vec<Target>,
    concurrency: usize,
    plan: Plan,
    previews: bool,
) -> LoadHandle {
    assert!(!targets.is_empty(), "a run needs at least one target");
    let (tx, rx) = mpsc::unbounded_channel();
    let (paused, paused_rx) = watch::channel(false);
    let sent = Arc::new(AtomicU64::new(0));
    let concurrency = concurrency.clamp(1, MAX_CONCURRENCY);
    let semaphore = Arc::new(Semaphore::new(concurrency));
    let shared = Arc::new(Shared {
        client,
        schedule: schedule(&targets),
        targets,
        next: AtomicUsize::new(0),
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
    let mut deadline = match plan {
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

        // Paused: wait for resume, then push the deadline back by the pause
        // so a duration run gets its full length of sending
        if *paused.borrow_and_update() {
            let since = tokio::time::Instant::now();
            while *paused.borrow_and_update() {
                if paused.changed().await.is_err() {
                    break;
                }
            }
            if let Some(deadline) = deadline.as_mut() {
                *deadline += since.elapsed();
            }
            continue;
        }

        let acquire = semaphore.clone().acquire_owned();
        let permit = match deadline {
            // Don't sit waiting for a free slot past the end, and notice a
            // pause that starts while waiting
            Some(deadline) => tokio::select! {
                permit = acquire => permit,
                _ = tokio::time::sleep_until(deadline) => break,
                _ = paused.changed() => continue,
            },
            None => tokio::select! {
                permit = acquire => permit,
                _ = paused.changed() => continue,
            },
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
    let turn = shared.next.fetch_add(1, Ordering::Relaxed);
    let target = &shared.targets[shared.schedule[turn % shared.schedule.len()] as usize];
    let request = &target.request;
    let dns_times = if shared.dns.claim() {
        resolve_dns(&request.url).await.ok()
    } else {
        None
    };

    let mut builder = shared.client.request(request.method.clone(), &request.url);
    if !target.headers.is_empty() {
        builder = builder.headers(target.headers.clone());
    }
    if let Some(body) = &request.body {
        builder = builder.body(body.clone());
    }

    let start = Instant::now();
    let response = builder.send().await;
    let ttfb = start.elapsed();
    let capture = shared.previews
        && matches!(&response, Ok(r) if shared.details.claim(!r.status().is_success()));
    let mut stats =
        ResponseStats::from_response(response, start, ttfb, dns_times, shared.previews, capture)
            .await;
    stats.endpoint = target.endpoint;

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
            insecure: false,
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
            body.map(|b| b.as_bytes().to_vec()),
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

    #[test]
    fn schedule_follows_weights_and_spreads_them() {
        let target = |weight| Target {
            request: request("http://x/", "GET", None),
            headers: Default::default(),
            endpoint: 0,
            weight,
        };
        let order = schedule(&[target(3), target(1)]);
        assert_eq!(order, [0, 1, 0, 0], "three to one, spread out");
        let order = schedule(&[target(1), target(1), target(1)]);
        assert_eq!(order.len(), 3);
        // Huge weights are scaled down, keeping the proportion
        let order = schedule(&[target(900_000), target(100_000)]);
        assert!(order.len() <= MAX_SCHEDULE + 2);
        let share = order.iter().filter(|&&i| i == 0).count() as f64 / order.len() as f64;
        assert!((0.89..=0.91).contains(&share), "{share}");
    }

    #[tokio::test]
    async fn targets_are_mixed_by_weight_and_tagged() {
        let srv = server(Duration::ZERO).await;
        let target = |path: &str, endpoint: u16, weight: u32| {
            let req = request(&format!("{}{path}", srv.url), "GET", None);
            let mut headers = reqwest::header::HeaderMap::new();
            headers.insert("x-endpoint", endpoint.to_string().parse().unwrap());
            Target {
                request: req,
                headers,
                endpoint,
                weight,
            }
        };
        let client = request(&srv.url, "GET", None).build_client().unwrap();
        let load = start_targets(
            client,
            vec![target("a", 0, 3), target("b", 1, 1)],
            2,
            Plan::Count(40),
            false,
        );
        let results = drain(load).await;
        let count = |e| results.iter().filter(|r| r.endpoint == e).count();
        assert_eq!((count(0), count(1)), (30, 10));
    }

    #[tokio::test]
    async fn pausing_a_duration_run_extends_it() {
        let srv = server(Duration::from_millis(5)).await;
        let req = request(&srv.url, "GET", None);
        let begin = Instant::now();
        let load = start(
            req.build_client().unwrap(),
            req,
            2,
            Plan::Duration(Duration::from_millis(400)),
            false,
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
        load.set_paused(true);
        // Paused past the original deadline: nothing ends the run
        tokio::time::sleep(Duration::from_millis(500)).await;
        let held = load.sent();
        load.set_paused(false);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(load.sent() > held, "still sending after the pause");
        let results = drain(load).await;
        // 400ms of sending plus the 500ms pause, give or take
        let took = begin.elapsed();
        assert!(took >= Duration::from_millis(850), "took {took:?}");
        assert!(took < Duration::from_millis(1_500), "took {took:?}");
        assert!(!results.is_empty());
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
