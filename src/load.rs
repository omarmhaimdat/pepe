//! The load engine: sends a run's requests and reports every result.
//!
//! Requests go out from a few shard threads. Each shard has its own
//! single-threaded tokio runtime, its own `reqwest::Client` (so its own
//! connection pool and timer wheel) and long-lived worker tasks, one per unit
//! of concurrency. Nothing on the hot path is shared between shards except a
//! handful of counters, so there is no lock for threads to queue on, which
//! is where a shared multi-threaded runtime spent most of its CPU.

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::{mpsc, Notify};
use tokio::task::JoinSet;

use crate::flow::Flow;
use crate::request::Request;
use crate::response::ResponseStats;
use crate::utils::{resolve_dns, thread_cpu_time};

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

/// How often a shard measures how busy its thread is
const BUSY_SAMPLE: Duration = Duration::from_secs(1);
/// `Control::busy` entry of a shard that hasn't measured yet
const BUSY_UNKNOWN: u8 = u8::MAX;

/// How long a run lasts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plan {
    /// Send exactly this many requests
    Count(u64),
    /// Keep sending until this much time has passed
    Duration(Duration),
}

/// Shard threads for a run: what `--threads` asked for, otherwise one. One
/// thread sends about a hundred thousand plain requests a second, or ten
/// thousand TLS handshakes, at the lowest CPU per request; more raise the
/// peak against a target that can take it, and the dashboard says when
/// that's the case (see `LoadHandle::busy`). Never more than the
/// concurrency: a shard with no worker would have nothing to do.
pub fn shards(concurrency: usize, requested: Option<usize>) -> usize {
    requested
        .unwrap_or(1)
        .clamp(1, concurrency.clamp(1, MAX_CONCURRENCY))
}

/// A running load test. Results arrive on `rx`; the channel closes once every
/// request has finished (or the run was stopped). Dropping the handle stops it.
pub struct LoadHandle {
    pub rx: mpsc::UnboundedReceiver<ResponseStats>,
    control: Arc<Control>,
}

impl LoadHandle {
    /// Take every result waiting on the channel; false once the run is over
    /// and the last result was taken. Polling this on a timer costs less
    /// than awaiting each result: a send to a waiting receiver has to wake it
    /// up through the kernel, which at a hundred thousand results a second
    /// was a fifth of a run's CPU.
    pub fn drain(&mut self, mut each: impl FnMut(ResponseStats)) -> bool {
        loop {
            match self.rx.try_recv() {
                Ok(stat) => each(stat),
                Err(mpsc::error::TryRecvError::Empty) => return true,
                Err(mpsc::error::TryRecvError::Disconnected) => return false,
            }
        }
    }

    /// Stop sending and cancel in-flight requests
    pub fn stop(&self) {
        self.control.stopped.store(true, Ordering::Release);
        self.control.changed.notify_waiters();
    }

    /// Requests started so far
    pub fn sent(&self) -> u64 {
        self.control.sent.load(Ordering::Relaxed)
    }

    pub fn concurrency(&self) -> usize {
        self.control.concurrency.load(Ordering::Relaxed)
    }

    /// Shard threads sending
    pub fn threads(&self) -> usize {
        self.control.busy.len()
    }

    /// How busy the busiest sending thread was over its last second, in
    /// percent of a core. Near 100, pepe is the bottleneck rather than the
    /// target, and more threads would send more. None until measured, or
    /// where the platform can't say.
    pub fn busy(&self) -> Option<u8> {
        self.control
            .busy
            .iter()
            .map(|b| b.load(Ordering::Relaxed))
            .filter(|&b| b != BUSY_UNKNOWN)
            .max()
    }

    /// Change how many requests may be in flight, while the run is going.
    /// Returns the new value, clamped to `1..=MAX_CONCURRENCY`. Lowering it
    /// takes effect as the surplus requests finish.
    pub fn set_concurrency(&self, target: usize) -> usize {
        let target = target.clamp(1, MAX_CONCURRENCY);
        self.control.concurrency.store(target, Ordering::Relaxed);
        self.control.changed.notify_waiters();
        target
    }

    /// Hold off starting new requests; in-flight ones still complete. A
    /// timed run's clock stops while paused.
    pub fn set_paused(&self, paused: bool) {
        let c = &self.control;
        if c.paused.swap(paused, Ordering::AcqRel) == paused {
            return;
        }
        let now = c.now_ns();
        if paused {
            c.paused_at_ns.store(now, Ordering::Relaxed);
        } else {
            let pause = now - c.paused_at_ns.load(Ordering::Relaxed);
            // Push the deadline back by the pause, so a duration run gets
            // its full length of sending
            let _ = c
                .deadline_ns
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                    (d != NO_DEADLINE).then(|| d + pause)
                });
        }
        c.changed.notify_waiters();
    }

    pub fn is_paused(&self) -> bool {
        self.control.paused.load(Ordering::Relaxed)
    }
}

impl Drop for LoadHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

const NO_DEADLINE: u64 = u64::MAX;

/// What the dashboard can change while a run is going, and how far along
/// the plan is. Workers read it before every request; every change is
/// announced on `changed`.
struct Control {
    /// Workers that may send at once, over all shards
    concurrency: AtomicUsize,
    paused: AtomicBool,
    /// The plan has ended: start nothing new, let in-flight requests finish
    draining: AtomicBool,
    /// Cancel everything, in-flight requests included
    stopped: AtomicBool,
    /// Requests started so far. A Count plan's limit is claimed from here,
    /// so it can't be overshot.
    sent: AtomicU64,
    limit: u64,
    /// Nanoseconds on `clock` when a Duration plan ends; `NO_DEADLINE` otherwise
    deadline_ns: AtomicU64,
    paused_at_ns: AtomicU64,
    clock: Instant,
    changed: Notify,
    /// Each shard's latest busy percentage (see `LoadHandle::busy`)
    busy: Vec<AtomicU8>,
}

impl Control {
    fn new(concurrency: usize, plan: Plan, shards: usize) -> Self {
        let (limit, deadline_ns) = match plan {
            Plan::Count(n) => (n, NO_DEADLINE),
            Plan::Duration(d) => (u64::MAX, d.as_nanos() as u64),
        };
        Self {
            concurrency: AtomicUsize::new(concurrency),
            paused: AtomicBool::new(false),
            draining: AtomicBool::new(false),
            stopped: AtomicBool::new(false),
            sent: AtomicU64::new(0),
            limit,
            deadline_ns: AtomicU64::new(deadline_ns),
            paused_at_ns: AtomicU64::new(0),
            clock: Instant::now(),
            changed: Notify::new(),
            busy: (0..shards).map(|_| AtomicU8::new(BUSY_UNKNOWN)).collect(),
        }
    }

    fn now_ns(&self) -> u64 {
        self.clock.elapsed().as_nanos() as u64
    }

    /// When sending ends, if the plan is timed
    fn deadline(&self) -> Option<tokio::time::Instant> {
        match self.deadline_ns.load(Ordering::Relaxed) {
            NO_DEADLINE => None,
            ns => Some((self.clock + Duration::from_nanos(ns)).into()),
        }
    }

    /// Take one of the plan's requests; false once they're all started
    fn claim(&self) -> bool {
        self.sent
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                (n < self.limit).then_some(n + 1)
            })
            .is_ok()
    }

    fn drain(&self) {
        self.draining.store(true, Ordering::Release);
        self.changed.notify_waiters();
    }

    /// No more requests are to be started
    fn over(&self) -> bool {
        self.draining.load(Ordering::Acquire) || self.stopped.load(Ordering::Acquire)
    }

    /// Resolves once the run is stopped
    async fn stopped(&self) {
        loop {
            // Registered before the check, so a stop in between isn't missed
            let changed = self.changed.notified();
            if self.stopped.load(Ordering::Acquire) {
                return;
            }
            changed.await;
        }
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

/// What every shard shares: the targets and the run-wide samplers
struct Shared {
    targets: Vec<Target>,
    /// Indexes into `targets`, cycled through
    schedule: Vec<u16>,
    next: AtomicUsize,
    dns: DnsSampler,
    /// Keep the start of each body for the dashboard's preview column, and
    /// some responses in full for its inspector
    previews: bool,
    details: DetailBudget,
    /// A flow instead of targets: workers walk its steps in order
    flow: Option<Flow>,
}

/// Load one request
pub fn start(
    clients: Vec<reqwest::Client>,
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
    start_targets(clients, vec![target], concurrency, plan, previews)
}

/// Load a mix of requests, each in proportion to its weight. One shard
/// thread is started per client; `shards` says how many to build.
pub fn start_targets(
    clients: Vec<reqwest::Client>,
    targets: Vec<Target>,
    concurrency: usize,
    plan: Plan,
    previews: bool,
) -> LoadHandle {
    assert!(!targets.is_empty(), "a run needs at least one target");
    start_shared(clients, targets, None, concurrency, plan, previews)
}

/// Load a flow: each worker walks its steps in order, with its own
/// values, and starts over when the chain ends or a step fails. A Count
/// plan counts chains, not requests.
pub fn start_flow(
    clients: Vec<reqwest::Client>,
    flow: Flow,
    concurrency: usize,
    plan: Plan,
    previews: bool,
) -> LoadHandle {
    assert!(!flow.steps.is_empty(), "a flow needs at least one step");
    start_shared(clients, Vec::new(), Some(flow), concurrency, plan, previews)
}

fn start_shared(
    clients: Vec<reqwest::Client>,
    targets: Vec<Target>,
    flow: Option<Flow>,
    concurrency: usize,
    plan: Plan,
    previews: bool,
) -> LoadHandle {
    assert!(!clients.is_empty(), "a run needs at least one client");
    let (tx, rx) = mpsc::unbounded_channel();
    let concurrency = concurrency.clamp(1, MAX_CONCURRENCY);
    let control = Arc::new(Control::new(concurrency, plan, clients.len()));
    let shared = Arc::new(Shared {
        schedule: schedule(&targets),
        targets,
        next: AtomicUsize::new(0),
        dns: DnsSampler::new(),
        previews,
        details: DetailBudget::new(),
        flow,
    });
    let count = clients.len();
    for (index, client) in clients.into_iter().enumerate() {
        let shard = Arc::new(Shard {
            index,
            count,
            client,
            shared: shared.clone(),
            control: control.clone(),
            tx: tx.clone(),
            wake: Notify::new(),
        });
        std::thread::Builder::new()
            .name(format!("pepe-load-{index}"))
            .spawn(move || run_shard(shard))
            .expect("spawn a load thread");
    }
    // Only the shards hold senders now, so the channel closes when they end
    drop(tx);
    LoadHandle { rx, control }
}

/// One load thread: its client and its share of the workers
struct Shard {
    index: usize,
    /// How many shards the run has
    count: usize,
    client: reqwest::Client,
    shared: Arc<Shared>,
    control: Arc<Control>,
    tx: mpsc::UnboundedSender<ResponseStats>,
    /// Woken whenever `control` changed, so parked workers look again
    wake: Notify,
}

/// Shard `index` of `count`'s part of `total`; a remainder goes to the first
/// shards
fn share_of(total: usize, index: usize, count: usize) -> usize {
    total / count + usize::from(index < total % count)
}

impl Shard {
    /// This shard's part of the concurrency right now
    fn share(&self) -> usize {
        share_of(
            self.control.concurrency.load(Ordering::Relaxed),
            self.index,
            self.count,
        )
    }

    /// Between a chain's steps: wait out a pause; false once the run is
    /// stopped outright (a plan that merely ended lets the chain finish)
    async fn between_steps(&self) -> bool {
        loop {
            let wake = self.wake.notified();
            if self.control.stopped.load(Ordering::Acquire) {
                return false;
            }
            if !self.control.paused.load(Ordering::Relaxed) {
                return true;
            }
            wake.await;
        }
    }

    /// Wait until worker `slot` may send; false once the run is over
    async fn turn(&self, slot: usize) -> bool {
        loop {
            // Registered before the checks, so a change in between isn't missed
            let wake = self.wake.notified();
            if self.control.over() {
                return false;
            }
            if !self.control.paused.load(Ordering::Relaxed) && slot < self.share() {
                return true;
            }
            wake.await;
        }
    }
}

fn run_shard(shard: Arc<Shard>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build a tokio runtime");
    runtime.block_on(async {
        tokio::select! {
            _ = supervise(&shard) => {}
            // A stop cancels in-flight requests: the runtime is dropped
            // with them still in it
            _ = shard.control.stopped() => {}
        }
    });
}

/// Keeps the shard's workers matching its share of the concurrency, ends
/// the sending when a timed plan reaches its deadline, and measures how
/// busy the thread is
async fn supervise(shard: &Arc<Shard>) {
    let control = &shard.control;
    let mut workers = JoinSet::new();
    let mut meter = BusyMeter::start();
    let mut sample = tokio::time::interval(BUSY_SAMPLE);
    sample.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        let changed = control.changed.notified();
        // Workers are only ever added; surplus ones park themselves
        while workers.len() < shard.share() {
            workers.spawn(worker(shard.clone(), workers.len()));
        }
        shard.wake.notify_waiters();
        if control.over() {
            break;
        }
        // A paused run's clock is stopped, so its deadline waits too
        let deadline = async {
            match control.deadline() {
                Some(at) if !control.paused.load(Ordering::Relaxed) => {
                    tokio::time::sleep_until(at).await
                }
                _ => std::future::pending().await,
            }
        };
        tokio::select! {
            _ = changed => {}
            _ = deadline => control.drain(),
            _ = sample.tick() => {
                if let Some(busy) = meter.sample() {
                    control.busy[shard.index].store(busy, Ordering::Relaxed);
                }
            }
        }
    }
    // Let in-flight requests finish and be reported
    while workers.join_next().await.is_some() {}
}

/// Measures the share of wall time this thread spends on the CPU
struct BusyMeter {
    cpu: Option<Duration>,
    at: Instant,
}

impl BusyMeter {
    fn start() -> Self {
        Self {
            cpu: thread_cpu_time(),
            at: Instant::now(),
        }
    }

    /// Percent of the time since the last sample spent on the CPU
    fn sample(&mut self) -> Option<u8> {
        let (cpu, now) = (thread_cpu_time()?, Instant::now());
        let used = cpu.saturating_sub(self.cpu?).as_secs_f64();
        let passed = now.duration_since(self.at).as_secs_f64();
        self.cpu = Some(cpu);
        self.at = now;
        Some(
            (used / passed.max(f64::EPSILON) * 100.0)
                .round()
                .clamp(0.0, 100.0) as u8,
        )
    }
}

/// Sends requests one after another, whenever it has a turn
async fn worker(shard: Arc<Shard>, slot: usize) {
    if shard.shared.flow.is_some() {
        return flow_worker(shard, slot).await;
    }
    while shard.turn(slot).await {
        if !shard.control.claim() {
            // The last request of the plan is out: tell every shard
            shard.control.drain();
            break;
        }
        let stats = send_one(&shard).await;
        if shard.tx.send(stats).is_err() {
            break;
        }
    }
}

/// One user of a flow: the steps in order with its own values, over and
/// over. A step that fails (no response, a status other than expected, a
/// capture that finds nothing) ends the chain; the next starts clean.
async fn flow_worker(shard: Arc<Shard>, slot: usize) {
    let flow = shard
        .shared
        .flow
        .as_ref()
        .expect("a flow worker has a flow");
    let mut vars = std::collections::HashMap::new();
    'chains: while shard.turn(slot).await {
        if !shard.control.claim() {
            shard.control.drain();
            break;
        }
        vars.clear();
        vars.extend(flow.vars.iter().cloned());
        for (index, step) in flow.steps.iter().enumerate() {
            // A chain that has started gets to finish when the plan ends;
            // a pause holds it between steps, a stop ends it
            if index > 0 && !shard.between_steps().await {
                break 'chains;
            }
            let (stats, ok) = send_step(&shard, step, index as u16, &mut vars).await;
            if shard.tx.send(stats).is_err() {
                break 'chains;
            }
            if !ok {
                break;
            }
        }
    }
}

/// One step of a chain; false when the chain can't go on
async fn send_step(
    shard: &Shard,
    step: &crate::flow::Step,
    index: u16,
    vars: &mut std::collections::HashMap<String, String>,
) -> (ResponseStats, bool) {
    let shared = &shard.shared;
    let request = match step.build(vars) {
        Ok(request) => request,
        Err(why) => {
            let mut stats = ResponseStats {
                endpoint: index,
                ..Default::default()
            };
            stats.fail_step(format!("couldn't build the request: {why}"));
            return (stats, false);
        }
    };
    let start = Instant::now();
    let response = shard.client.execute(request).await;
    let ttfb = start.elapsed();
    let capture = shared.previews
        && matches!(&response, Ok(r) if shared.details.claim(!r.status().is_success()));
    let body_cap = if step.captures.is_empty() {
        0
    } else {
        crate::flow::BODY_CAP
    };
    let (mut stats, kept) =
        ResponseStats::with_body(response, start, ttfb, shared.previews, capture, body_cap).await;
    stats.endpoint = index;
    let Some(status) = stats.status_code else {
        return (stats, false);
    };
    let wanted = match step.expect {
        Some(code) => status.as_u16() == code,
        None => status.is_success(),
    };
    if !wanted {
        if let Some(code) = step.expect {
            stats.fail_step(format!(
                "HTTP {} where {code} was expected",
                status.as_u16()
            ));
        }
        return (stats, false);
    }
    if let Some((headers, body)) = kept {
        if let Err(name) = step.capture(&headers, &body, vars) {
            stats.fail_step(format!("nothing for {{{{{name}}}}} in the response"));
            return (stats, false);
        }
    }
    (stats, true)
}

async fn send_one(shard: &Shard) -> ResponseStats {
    let shared = &shard.shared;
    let turn = shared.next.fetch_add(1, Ordering::Relaxed);
    let target = &shared.targets[shared.schedule[turn % shared.schedule.len()] as usize];
    let dns_times = match target.request.url.host_str() {
        Some(host) if shared.dns.claim() => resolve_dns(host).await.ok(),
        _ => None,
    };

    let start = Instant::now();
    let response = shard.client.execute(target.build()).await;
    let ttfb = start.elapsed();
    let capture = shared.previews
        && matches!(&response, Ok(r) if shared.details.claim(!r.status().is_success()));
    let mut stats =
        ResponseStats::from_response(response, start, ttfb, dns_times, shared.previews, capture)
            .await;
    stats.endpoint = target.endpoint;
    stats
}

impl Target {
    /// The request as reqwest sends it. The URL and method were parsed
    /// once, and the body's bytes are shared, so this is a few small copies.
    fn build(&self) -> reqwest::Request {
        let r = &self.request;
        let mut request = reqwest::Request::new(r.method.clone(), r.url.clone());
        if !self.headers.is_empty() {
            *request.headers_mut() = self.headers.clone();
        }
        if let Some(body) = &r.body {
            *request.body_mut() = Some(body.clone().into());
        }
        request
    }
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
        let load = start(req.build_clients(2).unwrap(), req, 4, Plan::Count(25), true);
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

    /// A two-route server: /login hands out a token in JSON and a header,
    /// /me wants it back as a bearer
    async fn token_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            loop {
                let (mut socket, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let mut buf = vec![0u8; 4096];
                    loop {
                        let mut head = Vec::new();
                        loop {
                            let n = socket.read(&mut buf).await.unwrap_or(0);
                            if n == 0 {
                                return;
                            }
                            head.extend_from_slice(&buf[..n]);
                            if head.windows(4).any(|w| w == b"\r\n\r\n") {
                                break;
                            }
                        }
                        let text = String::from_utf8_lossy(&head).to_string();
                        let (status, body) = if text.starts_with("POST /login") {
                            ("200 OK", r#"{"token":"t-123","user":{"id":7}}"#)
                        } else if text.contains("authorization: Bearer t-123")
                            || text.contains("Authorization: Bearer t-123")
                        {
                            ("200 OK", r#"{"me":7}"#)
                        } else {
                            ("401 Unauthorized", "no")
                        };
                        let response = format!(
                            "HTTP/1.1 {status}\r\nContent-Length: {}\r\nX-Sid: s9\r\nContent-Type: application/json\r\n\r\n{body}",
                            body.len()
                        );
                        if socket.write_all(response.as_bytes()).await.is_err() {
                            return;
                        }
                    }
                });
            }
        });
        url
    }

    #[tokio::test]
    async fn a_flow_feeds_one_step_into_the_next_and_counts_chains() {
        let url = token_server().await;
        let flow = crate::flow::parse(&format!(
            r#"
            [[step]]
            name = "login"
            method = "POST"
            url = "{url}/login"
            capture = {{ token = "json:$.token", sid = "header:X-Sid", id = "json:$.user.id" }}
            [[step]]
            name = "me"
            url = "{url}/me/{{{{id}}}}?sid={{{{sid}}}}"
            headers = ["Authorization: Bearer {{{{token}}}}"]
            "#
        ))
        .unwrap();
        let req = request(&url, "GET", None);
        let load = start_flow(
            req.build_clients(1).unwrap(),
            flow,
            2,
            Plan::Count(5),
            false,
        );
        let results = drain(load).await;
        assert_eq!(results.len(), 10, "five chains of two steps");
        assert!(
            results
                .iter()
                .all(|r| r.status_code.is_some_and(|s| s.is_success())),
            "{results:?}"
        );
        assert_eq!(results.iter().filter(|r| r.endpoint == 1).count(), 5);

        // A capture that finds nothing fails the step and ends the chain
        let flow = crate::flow::parse(&format!(
            r#"
            [[step]]
            method = "POST"
            url = "{url}/login"
            capture = {{ token = "json:$.nope" }}
            [[step]]
            url = "{url}/me"
            headers = ["Authorization: Bearer {{{{token}}}}"]
            "#
        ))
        .unwrap();
        let req = request(&url, "GET", None);
        let load = start_flow(
            req.build_clients(1).unwrap(),
            flow,
            1,
            Plan::Count(3),
            false,
        );
        let results = drain(load).await;
        assert_eq!(results.len(), 3, "the second step never runs");
        assert!(results
            .iter()
            .all(|r| r.endpoint == 0 && r.status_code.is_none()));
        assert_eq!(
            results[0].error_message.as_deref(),
            Some("nothing for {{token}} in the response")
        );

        // A status other than the expected one fails the step too
        let flow =
            crate::flow::parse(&format!("[[step]]\nurl = \"{url}/me\"\nexpect = 200\n")).unwrap();
        let req = request(&url, "GET", None);
        let load = start_flow(
            req.build_clients(1).unwrap(),
            flow,
            1,
            Plan::Count(2),
            false,
        );
        let results = drain(load).await;
        assert_eq!(results.len(), 2);
        assert_eq!(
            results[0].error_message.as_deref(),
            Some("HTTP 401 where 200 was expected")
        );
    }

    #[tokio::test]
    async fn duration_plan_stops_on_time() {
        let srv = server(Duration::from_millis(5)).await;
        let req = request(&srv.url, "GET", None);
        let begin = Instant::now();
        let load = start(
            req.build_clients(2).unwrap(),
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
            req.build_clients(2).unwrap(),
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
                req.build_clients(2).unwrap(),
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
    fn one_shard_unless_asked_and_never_more_than_workers() {
        assert_eq!(shards(1, None), 1);
        assert_eq!(shards(100_000, None), 1);
        assert_eq!(shards(100_000, Some(16)), 16, "--threads wins");
        assert_eq!(shards(3, Some(16)), 3, "never more shards than workers");
        assert_eq!(shards(10, Some(0)), 1);
    }

    #[tokio::test]
    async fn busy_is_measured_once_the_run_has_gone_a_second() {
        let srv = server(Duration::ZERO).await;
        let req = request(&srv.url, "GET", None);
        let load = start(
            req.build_clients(1).unwrap(),
            req,
            2,
            Plan::Duration(Duration::from_millis(1_300)),
            false,
        );
        assert_eq!((load.threads(), load.busy()), (1, None));
        tokio::time::sleep(Duration::from_millis(1_150)).await;
        if cfg!(unix) {
            assert!(load.busy().is_some(), "measured after a second");
        }
        drain(load).await;
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
        let client = request(&srv.url, "GET", None).build_clients(2).unwrap();
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
            req.build_clients(2).unwrap(),
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
            req.build_clients(2).unwrap(),
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
            req.build_clients(2).unwrap(),
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
