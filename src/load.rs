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

use crate::direct;
use crate::flow::Flow;
use crate::request::{Request, Sender};
use crate::response::ResponseStats;
use crate::response::{Answer, Failure, Preview};
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
/// A `--rate` schedule more than this far behind is moved up to now; the
/// slots skipped are counted as missed (see `Control::pace`)
const RATE_BACKLOG: Duration = Duration::from_secs(1);
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

/// A thread this busy (percent of a core) is holding the run back:
/// `--threads auto` adds another, and without it the dashboard says so
pub const SATURATED: u8 = 90;

/// What a thread `--threads auto` adds has to raise the rate by, to stay:
/// a third of what one more thread on `threads` is worth at best. Less,
/// and what holds the run back is the machine (a target on the same
/// cores, say), which another thread only crowds. A third, not a few
/// percent: the rate over one second moves by more than that on its own.
fn worth_a_thread(threads: usize) -> f64 {
    1.0 / (3.0 * threads.max(1) as f64)
}

/// Why `--threads auto` stopped adding threads
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadLimit {
    /// There is one on every core
    Cores,
    /// The last one added sent no more requests, and was taken back
    NoGain,
}

/// What `--threads` asks for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThreadCount {
    /// Start with one, and add one whenever every sending thread is
    /// `SATURATED`, up to the machine's cores
    Auto,
    Fixed(u32),
}

impl std::str::FromStr for ThreadCount {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, String> {
        match text.trim() {
            "auto" => Ok(Self::Auto),
            number => match number.parse::<u32>() {
                Ok(n) if n >= 1 => Ok(Self::Fixed(n)),
                _ => Err(format!(
                    "{text:?} is neither a number of threads nor \"auto\""
                )),
            },
        }
    }
}

impl std::fmt::Display for ThreadCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Auto => f.write_str("auto"),
            Self::Fixed(n) => write!(f, "{n}"),
        }
    }
}

impl serde::Serialize for ThreadCount {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Fixed(n) => serializer.serialize_u32(*n),
        }
    }
}

impl<'de> serde::Deserialize<'de> for ThreadCount {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        /// `threads = 2` or `threads = "auto"`
        #[derive(serde::Deserialize)]
        #[serde(untagged)]
        enum Written {
            Number(u32),
            Word(String),
        }
        match Written::deserialize(deserializer)? {
            Written::Number(n) => n.to_string().parse(),
            Written::Word(word) => word.parse(),
        }
        .map_err(serde::de::Error::custom)
    }
}

/// The shard threads of a run: how many send from the start, and how many
/// there may come to be
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Threads {
    pub start: usize,
    pub most: usize,
}

impl From<usize> for Threads {
    fn from(fixed: usize) -> Self {
        Self {
            start: fixed.max(1),
            most: fixed.max(1),
        }
    }
}

/// Shard threads for a run: what `--threads` asked for, otherwise one. One
/// thread sends a hundred thousand plain requests a second or more, or ten
/// thousand TLS handshakes, at the lowest CPU per request; more raise the
/// peak against a target that can take it. The dashboard says when that's
/// the case (see `LoadHandle::busy`), and `--threads auto` acts on it.
/// Never more than the concurrency: a shard with no worker would have
/// nothing to do.
pub fn shards(concurrency: usize, requested: Option<ThreadCount>) -> Threads {
    let workers = concurrency.clamp(1, MAX_CONCURRENCY);
    match requested {
        None => 1.into(),
        Some(ThreadCount::Fixed(n)) => (n as usize).clamp(1, workers).into(),
        Some(ThreadCount::Auto) => Threads {
            start: 1,
            most: std::thread::available_parallelism()
                .map_or(1, |cores| cores.get())
                .min(workers),
        },
    }
}

/// What a run sends with: a sender for every shard thread it may come to
/// have, and how many of them send from the start
pub struct Senders {
    pub list: Vec<Sender>,
    pub start: usize,
}

impl From<Sender> for Senders {
    fn from(one: Sender) -> Self {
        Self {
            list: vec![one],
            start: 1,
        }
    }
}

/// A running load test. Results arrive on `rx`; the channel closes once every
/// request has finished (or the run was stopped). Dropping the handle stops it.
pub struct LoadHandle {
    rx: mpsc::UnboundedReceiver<Report>,
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
                Ok(report) => each(report.into()),
                Err(mpsc::error::TryRecvError::Empty) => return true,
                Err(mpsc::error::TryRecvError::Disconnected) => return false,
            }
        }
    }

    /// The next result, when it comes; None once the run is over and the
    /// last result was taken
    pub async fn recv(&mut self) -> Option<ResponseStats> {
        self.rx.recv().await.map(ResponseStats::from)
    }

    /// The next result if one is waiting
    pub fn try_recv(&mut self) -> Result<ResponseStats, mpsc::error::TryRecvError> {
        self.rx.try_recv().map(ResponseStats::from)
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
        self.control.active.load(Ordering::Relaxed)
    }

    /// Whether another thread is added when these are all busy: `--threads
    /// auto`, until it finds its limit
    pub fn can_grow(&self) -> bool {
        self.control.auto && self.thread_limit().is_none()
    }

    /// Why `--threads auto` adds no more threads, once it doesn't
    pub fn thread_limit(&self) -> Option<ThreadLimit> {
        match self.control.thread_limit.load(Ordering::Relaxed) {
            LIMIT_CORES => Some(ThreadLimit::Cores),
            LIMIT_NO_GAIN => Some(ThreadLimit::NoGain),
            _ => None,
        }
    }

    /// The busiest second of the busiest sending thread, in percent of a
    /// core, since the run last gained a thread: what limits the numbers
    /// is the threads the run ended with, not the fewer it began with
    pub fn peak_busy(&self) -> Option<u8> {
        match self.control.peak_busy.load(Ordering::Relaxed) {
            BUSY_UNKNOWN => None,
            peak => Some(peak),
        }
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
            // A paced run resumes on schedule from now, not with a burst
            c.next_slot_ns.store(now, Ordering::Relaxed);
            let pause = now - c.paused_at_ns.load(Ordering::Relaxed);
            // A warm-up still going gets its full length too
            let _ = c
                .warmup_end_ns
                .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |end| {
                    (end > c.paused_at_ns.load(Ordering::Relaxed)).then(|| end + pause)
                });
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

    /// `--warmup`: for this long from now, requests are sent but marked
    /// `warmup` and not counted against a Count plan; a Duration plan's
    /// clock starts after it. Set before the first request.
    pub fn set_warmup(&self, warmup: Duration) {
        let c = &self.control;
        let ns = warmup.as_nanos() as u64;
        c.warmup_end_ns.store(c.now_ns() + ns, Ordering::Relaxed);
        let _ = c
            .deadline_ns
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |d| {
                (d != NO_DEADLINE).then(|| d + ns)
            });
    }

    /// Still in the warm-up
    pub fn warming(&self) -> bool {
        self.control.warming()
    }

    /// What's left of the warm-up, while there is one
    pub fn warmup_left(&self) -> Option<Duration> {
        let c = &self.control;
        let end = c.warmup_end_ns.load(Ordering::Relaxed);
        let now = c.now_ns();
        (end > now).then(|| Duration::from_nanos(end - now))
    }

    /// `--rate`: start this many requests a second, spread evenly, instead
    /// of as many as the concurrency allows; None lifts it. The schedule
    /// starts now, so a change doesn't owe a burst.
    pub fn set_rate(&self, per_second: Option<f64>) {
        let c = &self.control;
        let interval = per_second
            .filter(|r| *r > 0.0 && r.is_finite())
            .map_or(0, |r| (1e9 / r).max(1.0) as u64);
        c.next_slot_ns.store(c.now_ns(), Ordering::Relaxed);
        c.interval_ns.store(interval, Ordering::Relaxed);
    }

    /// The rate asked for, in requests per second
    pub fn rate(&self) -> Option<f64> {
        match self.control.interval_ns.load(Ordering::Relaxed) {
            0 => None,
            ns => Some(1e9 / ns as f64),
        }
    }

    /// Requests the rate called for that were never started, because the
    /// concurrency was all in flight for more than a second at a time
    pub fn missed(&self) -> u64 {
        self.control.missed.load(Ordering::Relaxed)
    }
}

impl Drop for LoadHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

const NO_DEADLINE: u64 = u64::MAX;

/// `Control::thread_limit`: `--threads auto` may still add a shard, or why not
const LIMIT_NONE: u8 = 0;
const LIMIT_CORES: u8 = 1;
const LIMIT_NO_GAIN: u8 = 2;

/// What `--threads auto` keeps between one second and the next
#[derive(Default)]
struct Growth {
    /// Shards not started yet
    dormant: Vec<Arc<Shard>>,
    /// When requests were last counted, and how many had started by then
    counted: Option<(Instant, u64)>,
    /// The rate before the shard added last, until that shard has had a
    /// second to show what it is worth
    before: Option<f64>,
}

/// A result on its way from a shard to whoever counts them. Results wait
/// in a queue between two collections (see `LoadHandle::drain`), a few
/// thousand of them at a time when a run is fast, and nearly all of them
/// are a status, two times and a size: those travel as that, a sixth of
/// the size of the whole record.
enum Report {
    Plain {
        /// Nanoseconds
        duration: u64,
        ttfb: u64,
        body_bytes: u64,
        status: u16,
        endpoint: u16,
        warmup: bool,
        cache: Option<crate::cache::CacheStatus>,
    },
    Full(Box<ResponseStats>),
}

impl From<ResponseStats> for Report {
    fn from(stats: ResponseStats) -> Self {
        let plain = stats.preview.is_none()
            && stats.dns_times.is_none()
            && stats.error.is_none()
            && stats.error_message.is_none()
            && stats.detail.is_none()
            && stats.request_id.is_none()
            && stats.server_timing.is_none();
        match (plain, stats.status_code, stats.ttfb) {
            (true, Some(status), Some(ttfb)) => Self::Plain {
                duration: stats.duration.as_nanos() as u64,
                ttfb: ttfb.as_nanos() as u64,
                body_bytes: stats.body_bytes,
                status: status.as_u16(),
                endpoint: stats.endpoint,
                warmup: stats.warmup,
                cache: stats.cache_status,
            },
            _ => Self::Full(Box::new(stats)),
        }
    }
}

impl From<Report> for ResponseStats {
    fn from(report: Report) -> Self {
        match report {
            Report::Full(stats) => *stats,
            Report::Plain {
                duration,
                ttfb,
                body_bytes,
                status,
                endpoint,
                warmup,
                cache,
            } => ResponseStats {
                duration: Duration::from_nanos(duration),
                ttfb: Some(Duration::from_nanos(ttfb)),
                body_bytes,
                status_code: reqwest::StatusCode::from_u16(status).ok(),
                endpoint,
                warmup,
                cache_status: cache,
                ..Default::default()
            },
        }
    }
}

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
    /// Each shard's latest busy percentage (see `LoadHandle::busy`); one
    /// for every shard there may come to be
    busy: Vec<AtomicU8>,
    /// The highest of them since the last shard was added
    peak_busy: AtomicU8,
    /// Shards sending now; the concurrency is shared out between them
    active: AtomicUsize,
    /// `--threads auto`: shards may be added while the run goes
    auto: bool,
    /// Why no more are (`LIMIT_*`)
    thread_limit: AtomicU8,
    growth: std::sync::Mutex<Growth>,
    /// Requests started during the warm-up, which `sent` leaves out: the
    /// two together are what `--threads auto` reads the rate from
    warmed: AtomicU64,
    /// Nanoseconds on `clock` when the warm-up ends; 0 when there is none
    warmup_end_ns: AtomicU64,
    /// `--rate` as nanoseconds between starts; 0 when unpaced
    interval_ns: AtomicU64,
    /// Nanoseconds on `clock` of the next start the schedule allows
    next_slot_ns: AtomicU64,
    /// Scheduled starts skipped because the run fell too far behind
    missed: AtomicU64,
}

impl Control {
    fn new(concurrency: usize, plan: Plan, threads: Threads) -> Self {
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
            busy: (0..threads.most)
                .map(|_| AtomicU8::new(BUSY_UNKNOWN))
                .collect(),
            peak_busy: AtomicU8::new(BUSY_UNKNOWN),
            active: AtomicUsize::new(threads.start),
            auto: threads.most > threads.start,
            thread_limit: AtomicU8::new(LIMIT_NONE),
            growth: std::sync::Mutex::new(Growth {
                // Counting from the start, so the first second already
                // has a rate to hold a new thread against
                counted: Some((Instant::now(), 0)),
                ..Default::default()
            }),
            warmed: AtomicU64::new(0),
            warmup_end_ns: AtomicU64::new(0),
            interval_ns: AtomicU64::new(0),
            next_slot_ns: AtomicU64::new(0),
            missed: AtomicU64::new(0),
        }
    }

    /// A shard measured how busy its thread was over the last second
    fn measured(&self, shard: usize, busy: u8) {
        self.busy[shard].store(busy, Ordering::Relaxed);
        // BUSY_UNKNOWN is the highest value there is, so it can't be a max
        let _ = self
            .peak_busy
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |peak| {
                (peak == BUSY_UNKNOWN || busy > peak).then_some(busy)
            });
        self.grow_if_saturated();
    }

    /// `--threads auto`, once a second: when every sending thread is busy
    /// the target can take more than they send, so start another, and a
    /// second later see what it was worth
    fn grow_if_saturated(&self) {
        let Ok(mut growth) = self.growth.lock() else {
            return;
        };
        if !self.auto || self.thread_limit.load(Ordering::Relaxed) != LIMIT_NONE || self.over() {
            return;
        }
        // A pause says nothing about what a thread is worth
        if self.paused.load(Ordering::Relaxed) {
            growth.counted = None;
            return;
        }
        let now = Instant::now();
        let started = self.sent.load(Ordering::Relaxed) + self.warmed.load(Ordering::Relaxed);
        let rate = match growth.counted {
            // Every shard asks once a second; one answer a second is enough
            Some((at, _)) if now.duration_since(at) < BUSY_SAMPLE.mul_f32(0.9) => return,
            Some((at, before)) => {
                started.saturating_sub(before) as f64 / now.duration_since(at).as_secs_f64()
            }
            None => {
                growth.counted = Some((now, started));
                return;
            }
        };
        growth.counted = Some((now, started));
        self.consider(&mut growth, rate);
    }

    /// One step of `--threads auto`, given the rate over the last second
    fn consider(&self, growth: &mut Growth, rate: f64) {
        let active = self.active.load(Ordering::Relaxed);
        if let Some(before) = growth.before.take() {
            if rate < before * (1.0 + worth_a_thread(active - 1)) {
                // Back to what it was. The shard added last keeps its
                // thread and gets no share (see `Shard::share`).
                self.active.store(active - 1, Ordering::Relaxed);
                self.settle(growth, LIMIT_NO_GAIN);
                return;
            }
            if growth.dormant.is_empty() {
                self.settle(growth, LIMIT_CORES);
                return;
            }
        }
        // A shard just added has no measurement yet, and holds the next
        // one back until it has
        let all_busy = self.busy[..active].iter().all(|busy| {
            let busy = busy.load(Ordering::Relaxed);
            busy != BUSY_UNKNOWN && busy >= SATURATED
        });
        if !all_busy || growth.dormant.is_empty() {
            return;
        }
        let shard = growth.dormant.remove(0);
        growth.before = Some(rate);
        self.active.store(active + 1, Ordering::Relaxed);
        self.peak_busy.store(BUSY_UNKNOWN, Ordering::Relaxed);
        shard.spawn();
        // Every shard's share just changed
        self.changed.notify_waiters();
    }

    /// `--threads auto` has its answer: no shard is added from here on
    fn settle(&self, growth: &mut Growth, limit: u8) {
        growth.dormant.clear();
        self.thread_limit.store(limit, Ordering::Relaxed);
        self.peak_busy.store(BUSY_UNKNOWN, Ordering::Relaxed);
        self.changed.notify_waiters();
    }

    /// The run is over: shards that never started hold a sender on the
    /// results channel, which closes only when they are gone
    fn retire_dormant(&self) {
        if let Ok(mut growth) = self.growth.lock() {
            growth.dormant.clear();
        }
    }

    fn warming(&self) -> bool {
        let end = self.warmup_end_ns.load(Ordering::Relaxed);
        end != 0 && self.now_ns() < end
    }

    /// `--rate`: take the next start on the schedule and say how long until
    /// it; None right away when unpaced or the start is already due. One
    /// atomic add per request, and sleeps that round up to a millisecond
    /// only add jitter: the schedule itself keeps the long-run rate true.
    /// A schedule more than `RATE_BACKLOG` behind is moved up to now and
    /// the starts in between counted as missed: the concurrency couldn't
    /// carry the rate, and a burst of the backlog would say nothing true.
    fn pace(&self) -> Option<Duration> {
        let interval = self.interval_ns.load(Ordering::Relaxed);
        if interval == 0 {
            return None;
        }
        let now = self.now_ns();
        let slot = self.next_slot_ns.fetch_add(interval, Ordering::Relaxed);
        if slot > now {
            return Some(Duration::from_nanos(slot - now));
        }
        let behind = now - slot;
        if behind > RATE_BACKLOG.as_nanos() as u64
            && self
                .next_slot_ns
                .compare_exchange(
                    slot + interval,
                    now + interval,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                )
                .is_ok()
        {
            self.missed.fetch_add(behind / interval, Ordering::Relaxed);
        }
        None
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

    /// Take one of the plan's requests; false once they're all started.
    /// Warm-up requests aren't the plan's: they're always allowed.
    fn claim(&self) -> bool {
        if self.warming() {
            self.warmed.fetch_add(1, Ordering::Relaxed);
            return true;
        }
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

/// Bodies of failed responses a worker keeps for each status it meets
const FAILURE_BODIES: u8 = 4;

/// Which failed responses a worker has kept the body of, by status
#[derive(Default)]
struct FailureBodies(Vec<(reqwest::StatusCode, u8)>);

impl FailureBodies {
    /// Whether to keep the body of one more response with this status
    fn wants(&mut self, status: reqwest::StatusCode) -> bool {
        let at = match self.0.iter().position(|(seen, _)| *seen == status) {
            Some(at) => at,
            None => {
                self.0.push((status, 0));
                self.0.len() - 1
            }
        };
        let kept = &mut self.0[at].1;
        *kept = kept.saturating_add(1);
        *kept <= FAILURE_BODIES
    }
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
    /// The targets the direct path sends, made ready for it
    routes: direct::Routes,
}

/// Load one request
pub fn start(
    clients: impl Into<Senders>,
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
    clients: impl Into<Senders>,
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
    clients: impl Into<Senders>,
    flow: Flow,
    concurrency: usize,
    plan: Plan,
    previews: bool,
) -> LoadHandle {
    assert!(!flow.steps.is_empty(), "a flow needs at least one step");
    start_shared(clients, Vec::new(), Some(flow), concurrency, plan, previews)
}

fn start_shared(
    clients: impl Into<Senders>,
    targets: Vec<Target>,
    flow: Option<Flow>,
    concurrency: usize,
    plan: Plan,
    previews: bool,
) -> LoadHandle {
    let Senders {
        list: clients,
        start,
    } = clients.into();
    assert!(!clients.is_empty(), "a run needs at least one client");
    let (tx, rx) = mpsc::unbounded_channel();
    let concurrency = concurrency.clamp(1, MAX_CONCURRENCY);
    let threads = Threads {
        start: start.clamp(1, clients.len()),
        most: clients.len(),
    };
    let control = Arc::new(Control::new(concurrency, plan, threads));
    // The same for every shard: they were built from one request
    let routes = match &clients[0].direct {
        Some(setup) => direct::Routes::new(setup, targets.iter().map(|t| (&t.request, &t.headers))),
        None => direct::Routes::none(targets.len()),
    };
    let shared = Arc::new(Shared {
        routes,
        schedule: schedule(&targets),
        targets,
        next: AtomicUsize::new(0),
        dns: DnsSampler::new(),
        previews,
        details: DetailBudget::new(),
        flow,
    });
    let mut dormant = Vec::new();
    for (index, sender) in clients.into_iter().enumerate() {
        let shard = Arc::new(Shard {
            index,
            sender,
            shared: shared.clone(),
            control: control.clone(),
            tx: tx.clone(),
            wake: Notify::new(),
        });
        if index < threads.start {
            shard.spawn();
        } else {
            dormant.push(shard);
        }
    }
    if let Ok(mut growth) = control.growth.lock() {
        growth.dormant = dormant;
    }
    // Only the shards hold senders now, so the channel closes when they end
    drop(tx);
    LoadHandle { rx, control }
}

/// One load thread: its client and its share of the workers
struct Shard {
    index: usize,
    /// The direct path's setup when the run can use it, and reqwest for
    /// what the direct path leaves to it
    sender: Sender,
    shared: Arc<Shared>,
    control: Arc<Control>,
    tx: mpsc::UnboundedSender<Report>,
    /// Woken whenever `control` changed, so parked workers look again
    wake: Notify,
}

/// Shard `index` of `count`'s part of `total`; a remainder goes to the first
/// shards
fn share_of(total: usize, index: usize, count: usize) -> usize {
    total / count + usize::from(index < total % count)
}

impl Shard {
    /// Start this shard's thread
    fn spawn(self: Arc<Self>) {
        std::thread::Builder::new()
            .name(format!("pepe-load-{}", self.index))
            .spawn(move || run_shard(self))
            .expect("spawn a load thread");
    }

    /// This shard's part of the concurrency right now; none for a shard
    /// that `--threads auto` added and took back
    fn share(&self) -> usize {
        let active = self.control.active.load(Ordering::Relaxed);
        if self.index >= active {
            return 0;
        }
        share_of(
            self.control.concurrency.load(Ordering::Relaxed),
            self.index,
            active,
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

    /// Wait until worker `slot` may send; false once the run is over.
    /// `surplus` is called when the worker is one more than this shard's
    /// share (the concurrency was lowered, or a shard was added): it has
    /// nothing to send until that changes.
    async fn turn(&self, slot: usize, mut surplus: impl FnMut()) -> bool {
        loop {
            // Registered before the checks, so a change in between isn't missed
            let wake = self.wake.notified();
            if self.control.over() {
                return false;
            }
            let mine = slot < self.share();
            if mine && !self.control.paused.load(Ordering::Relaxed) {
                return true;
            }
            if !mine {
                surplus();
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
    shard.control.retire_dormant();
}

/// Keeps the shard's workers matching its share of the concurrency, ends
/// the sending when a timed plan reaches its deadline, and measures how
/// busy the thread is
async fn supervise(shard: &Arc<Shard>) {
    let control = &shard.control;
    let mut workers = JoinSet::new();
    let mut meter = BusyMeter::start();
    // The first sample after a full interval: an interval's first tick is
    // immediate, and a reading over no time at all says nothing
    let mut sample =
        tokio::time::interval_at(tokio::time::Instant::now() + BUSY_SAMPLE, BUSY_SAMPLE);
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
                    control.measured(shard.index, busy);
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
        return Box::pin(flow_worker(shard, slot)).await;
    }
    let mut lines = shard.shared.routes.lines();
    let mut failure_bodies = FailureBodies::default();
    // A worker with no turn coming holds no connection open
    while shard.turn(slot, || lines.close()).await {
        if let Some(wait) = shard.control.pace() {
            tokio::time::sleep(wait).await;
            if shard.control.over() {
                break;
            }
        }
        if !shard.control.claim() {
            // The last request of the plan is out: tell every shard
            shard.control.drain();
            break;
        }
        let warmup = shard.control.warming();
        let mut stats = send_one(&shard, &mut lines, &mut failure_bodies).await;
        stats.warmup = warmup;
        if shard.tx.send(stats.into()).is_err() {
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
    'chains: while shard.turn(slot, || {}).await {
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
            // `--rate` paces every request, steps included
            if let Some(wait) = shard.control.pace() {
                tokio::time::sleep(wait).await;
            }
            let (stats, ok) = send_step(&shard, step, index as u16, &mut vars).await;
            if shard.tx.send(stats.into()).is_err() {
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
    let response = match shard.sender.client() {
        Ok(client) => client.execute(request).await.map_err(|e| Failure::from(&e)),
        Err(failure) => Err(failure),
    };
    let ttfb = start.elapsed();
    let capture = shared.previews
        && matches!(&response, Ok(r) if shared.details.claim(!r.status().is_success()));
    let response = response.map(Answer::from);
    let body_cap = if step.captures.is_empty() {
        0
    } else {
        crate::flow::BODY_CAP
    };
    let (mut stats, kept) =
        ResponseStats::with_body(response, start, ttfb, shared.preview(), capture, body_cap).await;
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

async fn send_one(
    shard: &Shard,
    lines: &mut direct::Lines,
    failure_bodies: &mut FailureBodies,
) -> ResponseStats {
    let shared = &shard.shared;
    let turn = shared.next.fetch_add(1, Ordering::Relaxed);
    let index = shared.schedule[turn % shared.schedule.len()] as usize;
    let target = &shared.targets[index];
    let dns_times = match target.request.url.host_str() {
        // Boxed, as are the other paths a request rarely takes: a worker's
        // future is as large as everything it might be waiting on at once,
        // and there is one for every connection
        Some(host) if shared.dns.claim() => Box::pin(resolve_dns(host)).await.ok(),
        _ => None,
    };

    if let (Some(setup), Some(route)) = (&shard.sender.direct, shared.routes.get(index)) {
        let start = Instant::now();
        let sent = lines.send(setup, route, start).await;
        let ttfb = start.elapsed();
        let response = match sent {
            Ok(direct::Sent::Answered(response)) => Ok(response),
            Err(failure) => Err(failure),
            // Sent again below, the way that follows redirects
            Ok(direct::Sent::Redirected) => {
                return Box::pin(pooled(shard, target, dns_times)).await
            }
        };
        let capture = shared.previews
            && matches!(&response, Ok(r) if shared.details.claim(!r.status.is_success()));
        // The verdict shows a body for each kind of failure, and takes it
        // from the first of its kind: a worker keeps a few, not the body
        // of every 503 of a target that is down
        let preview = match &response {
            Ok(r)
                if !shared.previews
                    && !r.status.is_success()
                    && !failure_bodies.wants(r.status) =>
            {
                Preview::Never
            }
            _ => shared.preview(),
        };
        let response = response.map(|r| Answer::Direct(r, &target.request.url));
        let mut stats =
            ResponseStats::from_response(response, start, ttfb, dns_times, preview, capture)
                .await
                .0;
        lines.done(setup, route, stats.error.is_some());
        stats.endpoint = target.endpoint;
        return stats;
    }
    Box::pin(pooled(shard, target, dns_times)).await
}

/// One request through reqwest and its connection pool: what the direct
/// path doesn't do (see `direct`)
async fn pooled(
    shard: &Shard,
    target: &Target,
    dns_times: Option<(Duration, Duration)>,
) -> ResponseStats {
    let shared = &shard.shared;
    let start = Instant::now();
    let response = match shard.sender.client() {
        Ok(client) => client
            .execute(target.build())
            .await
            .map_err(|e| Failure::from(&e)),
        Err(failure) => Err(failure),
    };
    let ttfb = start.elapsed();
    let capture = shared.previews
        && matches!(&response, Ok(r) if shared.details.claim(!r.status().is_success()));
    let response = response.map(Answer::from);
    let mut stats =
        ResponseStats::from_response(response, start, ttfb, dns_times, shared.preview(), capture)
            .await
            .0;
    stats.endpoint = target.endpoint;
    stats
}

impl Shared {
    /// Which responses to keep the start of
    fn preview(&self) -> Preview {
        match self.previews {
            true => Preview::Always,
            false => Preview::OfFailures,
        }
    }
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
            idle_connections: crate::request::IDLE_CONNECTIONS,
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
        while let Some(stat) = load.recv().await {
            out.push(stat);
        }
        out
    }

    #[tokio::test]
    async fn count_plan_sends_exactly_n() {
        let srv = server(Duration::ZERO).await;
        let req = request(&srv.url, "GET", None);
        let load = start(
            req.build_clients(2).unwrap().0,
            req,
            4,
            Plan::Count(25),
            true,
        );
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
            req.build_clients(1).unwrap().0,
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
            req.build_clients(1).unwrap().0,
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
            req.build_clients(1).unwrap().0,
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
    async fn a_rate_spreads_the_starts_and_counts_what_it_could_not_carry() {
        let srv = server(Duration::ZERO).await;
        let req = request(&srv.url, "GET", None);
        let begin = Instant::now();
        let load = start(
            req.build_clients(1).unwrap().0,
            req,
            4,
            Plan::Count(20),
            false,
        );
        load.set_rate(Some(50.0));
        assert_eq!(load.rate().map(f64::round), Some(50.0));
        let results = drain(load).await;
        let took = begin.elapsed();
        assert_eq!(results.len(), 20);
        // 19 gaps of 20ms, give or take timer slack; not the instant an
        // unpaced run of 20 would be
        assert!(took >= Duration::from_millis(300), "took {took:?}");
        assert!(took <= Duration::from_millis(1_500), "took {took:?}");

        // One worker against 30ms answers can carry ~33/s; asked for
        // 1,000/s, it falls behind within a second and the rest is missed
        let srv = server(Duration::from_millis(30)).await;
        let req = request(&srv.url, "GET", None);
        let load = start(
            req.build_clients(1).unwrap().0,
            req,
            1,
            Plan::Duration(Duration::from_millis(1_600)),
            false,
        );
        load.set_rate(Some(1_000.0));
        let mut rx_load = load;
        let mut n = 0;
        while rx_load.recv().await.is_some() {
            n += 1;
        }
        assert!(n < 100, "sent {n}");
        assert!(rx_load.missed() > 100, "missed {}", rx_load.missed());
    }

    #[tokio::test]
    async fn warmup_requests_are_marked_and_not_the_plans() {
        let srv = server(Duration::ZERO).await;
        let req = request(&srv.url, "GET", None);
        let load = start(
            req.build_clients(1).unwrap().0,
            req,
            2,
            Plan::Count(10),
            false,
        );
        load.set_warmup(Duration::from_millis(200));
        assert!(load.warming() && load.warmup_left().is_some());
        let results = drain(load).await;
        let warm = results.iter().filter(|r| r.warmup).count();
        assert_eq!(
            results.len() - warm,
            10,
            "the plan's ten, after the warm-up"
        );
        assert!(warm > 0, "warm-up requests were sent");

        // A timed run's clock starts after the warm-up
        let req = request(&srv.url, "GET", None);
        let begin = Instant::now();
        let load = start(
            req.build_clients(1).unwrap().0,
            req,
            1,
            Plan::Duration(Duration::from_millis(200)),
            false,
        );
        load.set_warmup(Duration::from_millis(200));
        let results = drain(load).await;
        assert!(
            begin.elapsed() >= Duration::from_millis(390),
            "{:?}",
            begin.elapsed()
        );
        assert!(results.iter().any(|r| r.warmup) && results.iter().any(|r| !r.warmup));
    }

    #[tokio::test]
    async fn duration_plan_stops_on_time() {
        let srv = server(Duration::from_millis(5)).await;
        let req = request(&srv.url, "GET", None);
        let begin = Instant::now();
        let load = start(
            req.build_clients(2).unwrap().0,
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
            req.build_clients(2).unwrap().0,
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
                req.build_clients(2).unwrap().0,
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
        let fixed = |n| Some(ThreadCount::Fixed(n));
        assert_eq!(shards(1, None), 1.into());
        assert_eq!(shards(100_000, None), 1.into());
        assert_eq!(shards(100_000, fixed(16)), 16.into(), "--threads wins");
        assert_eq!(
            shards(3, fixed(16)),
            3.into(),
            "never more shards than workers"
        );
        assert_eq!(shards(10, fixed(0)), 1.into());
        // auto starts with one and may reach the cores, or the workers
        let cores = std::thread::available_parallelism().unwrap().get();
        let auto = shards(100_000, Some(ThreadCount::Auto));
        assert_eq!((auto.start, auto.most), (1, cores));
        assert_eq!(shards(1, Some(ThreadCount::Auto)), 1.into());
    }

    #[test]
    fn threads_are_a_number_or_auto() {
        assert_eq!("auto".parse(), Ok(ThreadCount::Auto));
        assert_eq!(" 4 ".parse(), Ok(ThreadCount::Fixed(4)));
        assert!("0".parse::<ThreadCount>().is_err());
        assert!("many".parse::<ThreadCount>().is_err());
        assert_eq!(ThreadCount::Auto.to_string(), "auto");
        assert_eq!(ThreadCount::Fixed(3).to_string(), "3");
    }

    /// A paused run with `most` shards, one of them sending, for stepping
    /// `--threads auto` by hand
    async fn growing(most: usize) -> (Server, LoadHandle) {
        let srv = server(Duration::from_millis(20)).await;
        let req = request(&srv.url, "GET", None);
        let senders = Senders {
            list: req.build_clients(most).unwrap().0.list,
            start: 1,
        };
        let plan = Plan::Duration(Duration::from_secs(30));
        let load = start(senders, req, 6, plan, false);
        assert_eq!(load.threads(), 1);
        assert!(load.can_grow());
        (srv, load)
    }

    /// Shard `shard` measured `busy`, and the run sent `rate` a second
    fn step(load: &LoadHandle, shard: usize, busy: u8, rate: f64) {
        let c = &load.control;
        c.busy[shard].store(busy, Ordering::Relaxed);
        c.consider(&mut c.growth.lock().unwrap(), rate);
    }

    #[tokio::test]
    async fn a_shard_is_added_while_they_are_all_busy_and_it_pays() {
        let (srv, load) = growing(3).await;
        // Not busy, not grown
        step(&load, 0, 40, 100.0);
        assert_eq!(load.threads(), 1);
        // Busy: a second shard
        step(&load, 0, 97, 100.0);
        assert_eq!(load.threads(), 2);
        // It nearly doubled the rate, so it stays; it hasn't measured
        // yet, so nothing more is added
        step(&load, 0, 99, 190.0);
        assert_eq!(load.threads(), 2);
        // Both busy: the third and last, which pays too
        step(&load, 1, 95, 190.0);
        assert_eq!(load.threads(), 3);
        assert!(load.can_grow(), "until the third has shown its worth");
        step(&load, 2, 99, 270.0);
        assert_eq!(load.threads(), 3);
        assert_eq!(load.thread_limit(), Some(ThreadLimit::Cores));
        assert!(!load.can_grow());
        // Every shard takes its share of the six workers, once the requests
        // the first shard sent while it still had more of them are answered
        tokio::time::sleep(Duration::from_millis(200)).await;
        srv.peak.store(0, Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(srv.peak.load(Ordering::SeqCst), 6);
        load.stop();
        drain(load).await;
    }

    #[tokio::test]
    async fn a_shard_that_sends_no_more_is_taken_back() {
        let (srv, load) = growing(4).await;
        step(&load, 0, 97, 100.0);
        assert_eq!(load.threads(), 2);
        // A fifth more for a whole thread: the machine is the limit
        step(&load, 0, 97, 120.0);
        assert_eq!(load.threads(), 1);
        assert_eq!(load.thread_limit(), Some(ThreadLimit::NoGain));
        // ... and that was the last attempt
        step(&load, 0, 99, 120.0);
        assert_eq!(load.threads(), 1);
        assert!(!load.can_grow());
        // The six workers are the first shard's again. Watched for a
        // while: at any one instant a worker may be between requests.
        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut most = 0;
        for _ in 0..30 {
            most = most.max(srv.inflight.load(Ordering::SeqCst));
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(most, 6);
        load.stop();
        drain(load).await;
    }

    #[tokio::test]
    async fn a_fixed_number_of_threads_never_grows() {
        let srv = server(Duration::ZERO).await;
        let req = request(&srv.url, "GET", None);
        let clients = req.build_clients(2).unwrap().0;
        let load = start(clients, req, 4, Plan::Count(4), false);
        assert_eq!((load.threads(), load.can_grow()), (2, false));
        step(&load, 0, 99, 100.0);
        step(&load, 1, 99, 100.0);
        assert_eq!((load.threads(), load.thread_limit()), (2, None));
        drain(load).await;
    }

    #[tokio::test]
    async fn a_run_ends_with_shards_that_never_started() {
        let srv = server(Duration::ZERO).await;
        let req = request(&srv.url, "GET", None);
        let senders = Senders {
            list: req.build_clients(4).unwrap().0.list,
            start: 1,
        };
        let load = start(senders, req, 4, Plan::Count(10), false);
        // The channel closes although three shards still held a sender
        assert_eq!(drain(load).await.len(), 10);
    }

    #[tokio::test]
    async fn busy_is_measured_once_the_run_has_gone_a_second() {
        let srv = server(Duration::ZERO).await;
        let req = request(&srv.url, "GET", None);
        let load = start(
            req.build_clients(1).unwrap().0,
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
        let client = request(&srv.url, "GET", None).build_clients(2).unwrap().0;
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
            req.build_clients(2).unwrap().0,
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
            req.build_clients(2).unwrap().0,
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
            req.build_clients(2).unwrap().0,
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
