//! `pepe logs --serve`: the logs screen in a browser, live.
//!
//! What it costs is the point. Once a second, while someone watches, one
//! tick takes what the page shows from the counts the reader keeps (the
//! lock held for that alone), and writes it as parts: a part that says
//! what it said a second ago isn't sent again. The step from one tick to
//! the next is written once, as the bytes of one event, and every viewer
//! that saw the tick before gets those same bytes; a viewer that fell
//! behind skips to the newest, so nothing queues for a slow one. With
//! nobody watching the tick does nothing at all. What only some tabs show
//! (every path, the slots, every cause, a path held) is built when it is
//! asked for, at most once a tick however many ask.

use std::collections::{HashMap, VecDeque};
use std::fmt::Write as _;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use http_body_util::{BodyExt, StreamBody};
use hyper::body::{Frame, Incoming};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::TokioIo;
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::watch;

use super::http::{self, Reply};
use crate::diagnose::Level;
use crate::logs::{self, Clock, Grain, Kind, Parser, Shared, Stats};
use crate::logs_verdict::{self, Condition, Finding, Scope};

/// Where it listens unless told
pub const DEFAULT_LISTEN: &str = "127.0.0.1:7880";
/// Seconds the live chart shows
const SECONDS: usize = 300;
/// Seconds sent again each tick: the second in progress, and the ones a
/// late line may still add to
const RESENT: usize = 4;
/// Log lines a viewer gets when it comes, and that the board keeps
const LINES: usize = 1_000;
/// Log lines sent a tick at most: the newest; past it the page says how
/// many went by
const LINES_A_TICK: usize = 200;
/// A line longer than this is cut
const LINE_MOST: usize = 2_000;
/// Busiest paths on the dashboard, and every path on its tab
const TOP: usize = 8;
const PATHS: usize = 150;
const CAUSES_SHORT: usize = 4;
const CAUSES: usize = 50;
const TICK: Duration = Duration::from_secs(1);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

/// What the page is served from
pub struct Hub {
    shared: Arc<Shared>,
    parser: Parser,
    exact_paths: bool,
    title: String,
    window: i64,
    conditions: Vec<Condition>,
    token: String,
    board: Mutex<Board>,
    /// The board's tick, for the streams to wait on
    ticked: watch::Sender<u64>,
    viewers: AtomicUsize,
    /// The parts built on demand, by what was asked, with the tick they
    /// were built at
    asked: Mutex<HashMap<String, (u64, Arc<Answer>)>>,
    /// What the tick costs: the last one's microseconds, the lock's share
    pub cost: Cost,
    /// One tick at a time, whoever starts it
    ticking: Mutex<()>,
    born: Instant,
    /// When the lines were last asked for, in milliseconds since `born`:
    /// they are taken from the reader only while a page shows them
    lines_wanted: AtomicU64,
    /// The last answers for the lines, by the tick and the question
    lines_said: Mutex<Vec<(u64, String, Arc<Answer>)>>,
}

#[derive(Default)]
pub struct Cost {
    pub ticks: AtomicU64,
    pub build_us: AtomicU64,
    pub locked_us: AtomicU64,
    pub bytes_out: AtomicU64,
}

#[derive(Default)]
struct Board {
    tick: u64,
    /// Each part as its JSON, and the tick it last changed at
    parts: Vec<(&'static str, String, u64)>,
    /// The last seconds, `[second, requests, 4xx, 5xx]`, on the log's clock
    seconds: VecDeque<[i64; 4]>,
    /// The lines taken, oldest first
    lines: VecDeque<Kept>,
    /// The number of the newest line taken
    last_line: u64,
    /// Lines were taken at the last tick: the next goes on from there
    taking: bool,
    /// The step from the tick before to this one, as one event
    step: Bytes,
}

impl Hub {
    pub fn new(
        shared: Arc<Shared>,
        parser: Parser,
        exact_paths: bool,
        title: String,
        window: i64,
        conditions: Vec<Condition>,
        token: String,
    ) -> Arc<Hub> {
        Arc::new(Hub {
            shared,
            parser,
            exact_paths,
            title,
            window,
            conditions,
            token,
            board: Mutex::new(Board::default()),
            ticked: watch::channel(0).0,
            viewers: AtomicUsize::new(0),
            asked: Mutex::new(HashMap::new()),
            cost: Cost::default(),
            ticking: Mutex::new(()),
            born: Instant::now(),
            lines_wanted: AtomicU64::new(0),
            lines_said: Mutex::new(Vec::new()),
        })
    }

    fn board(&self) -> std::sync::MutexGuard<'_, Board> {
        self.board.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Once a second, while anyone watches
    pub async fn ticking(self: Arc<Hub>) {
        let mut every = tokio::time::interval(TICK);
        every.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            every.tick().await;
            if self.viewers.load(Ordering::Relaxed) > 0 || self.lines_wanted() {
                let hub = self.clone();
                let _ = tokio::task::spawn_blocking(move || hub.tick()).await;
            }
        }
    }

    /// Take what the page shows, and write down what changed
    pub fn tick(&self) {
        let _one = self.ticking.lock().unwrap_or_else(|e| e.into_inner());
        let started = Instant::now();
        // The lines only while a page shows them: on from the last taken,
        // or the last `LINES` when they weren't taken a tick ago
        let lines_from = self.lines_wanted().then(|| {
            let board = self.board();
            if board.taking {
                board.last_line
            } else {
                0
            }
        });
        let (taken, locked) = {
            let stats = self.shared.lock();
            let at = Instant::now();
            let taken = Taken::from(&stats, self, logs::wall(), lines_from);
            (taken, at.elapsed())
        };
        let mut board = self.board();
        if lines_from == Some(0) {
            board.lines.clear();
        }
        board.taking = lines_from.is_some();
        let tick = board.tick + 1;
        let mut changed = Vec::new();
        for (name, value) in taken.parts {
            let text = value.to_string();
            match board.parts.iter_mut().find(|(n, _, _)| *n == name) {
                Some((_, old, _)) if *old == text => {}
                Some(part) => {
                    part.1 = text;
                    part.2 = tick;
                    changed.push(name);
                }
                None => {
                    board.parts.push((name, text, tick));
                    changed.push(name);
                }
            }
        }
        // Seconds: the ones said again are put right in place
        let seconds_moved = board.seconds.back() != taken.seconds.last();
        // New lines move the tick, so an answer kept for the one before
        // isn't given for them
        let lines_moved = !taken.lines.is_empty();
        for second in taken.seconds {
            let newest = board.seconds.back().map(|s| s[0]);
            if newest.is_none_or(|t| t < second[0]) {
                board.seconds.push_back(second);
            } else if let Some(held) = board.seconds.iter_mut().rev().find(|s| s[0] == second[0]) {
                *held = second;
            }
        }
        while board.seconds.len() > SECONDS {
            board.seconds.pop_front();
        }
        for line in taken.lines {
            board.last_line = line.n;
            board.lines.push_back(line);
        }
        while board.lines.len() > LINES {
            board.lines.pop_front();
        }
        if changed.is_empty() && !seconds_moved && !lines_moved && tick > 1 {
            drop(board);
            self.note(started, locked);
            return;
        }
        board.tick = tick;
        let mut step = String::with_capacity(256);
        let _ = write!(step, "id: {tick}\ndata: {{\"tick\":{tick}");
        for (name, text, at) in &board.parts {
            if *at == tick {
                let _ = write!(step, ",\"{name}\":{text}");
            }
        }
        let skip = board.seconds.len().saturating_sub(RESENT);
        push_seconds(&mut step, board.seconds.iter().skip(skip));
        let _ = write!(
            step,
            ",\"viewers\":{}}}\n\n",
            self.viewers.load(Ordering::Relaxed)
        );
        board.step = Bytes::from(step);
        drop(board);
        self.ticked.send_replace(tick);
        self.note(started, locked);
    }

    fn note(&self, started: Instant, locked: Duration) {
        self.cost.ticks.fetch_add(1, Ordering::Relaxed);
        self.cost
            .build_us
            .store(started.elapsed().as_micros() as u64, Ordering::Relaxed);
        self.cost
            .locked_us
            .store(locked.as_micros() as u64, Ordering::Relaxed);
    }

    fn lines_wanted(&self) -> bool {
        let asked = self.lines_wanted.load(Ordering::Relaxed);
        asked > 0 && (self.born.elapsed().as_millis() as u64).saturating_sub(asked) < 3_000
    }

    /// The lines after `after` that `only` keeps, for the log tab: taken
    /// from the reader from the first ask on, kept to what the page shows
    /// here rather than there, and the same answer for every page that
    /// asks the same in the same tick
    fn lines_after(&self, after: u64, only: &Only) -> Arc<Answer> {
        let was = self.lines_wanted();
        self.lines_wanted.store(
            (self.born.elapsed().as_millis() as u64).max(1),
            Ordering::Relaxed,
        );
        if !was || !self.board().taking {
            self.tick();
        }
        let board = self.board();
        let key = format!("{after}|{only:?}");
        let mut said = self.lines_said.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((_, _, answer)) = said.iter().find(|(t, k, _)| *t == board.tick && *k == key) {
            return answer.clone();
        }
        let first = board.lines.partition_point(|line| line.n <= after);
        let mut out = String::with_capacity(64 + (board.lines.len() - first) * 160);
        let _ = write!(out, "{{\"last\":{}", board.last_line);
        push_lines(
            &mut out,
            board.lines.range(first..).filter(|l| only.keeps(l)),
        );
        out.push('}');
        let answer = Answer::new(out);
        said.retain(|(t, _, _)| *t == board.tick);
        if said.len() < 32 {
            said.push((board.tick, key, answer.clone()));
        }
        answer
    }

    /// Everything, for a viewer that just came or fell behind: every part
    /// changed after `seen`, and the seconds
    fn catch_up(&self, seen: u64) -> (Bytes, u64) {
        let board = self.board();
        let mut out = String::with_capacity(16 * 1024);
        let _ = write!(out, "id: {}\ndata: {{\"tick\":{}", board.tick, board.tick);
        if seen == 0 {
            let _ = write!(out, ",\"title\":{}", json!(self.title));
        }
        for (name, text, at) in &board.parts {
            if *at > seen {
                let _ = write!(out, ",\"{name}\":{text}");
            }
        }
        push_seconds(&mut out, board.seconds.iter());
        let _ = write!(
            out,
            ",\"viewers\":{}}}\n\n",
            self.viewers.load(Ordering::Relaxed)
        );
        (Bytes::from(out), board.tick)
    }

    /// A part built when asked for, at most once a second: by the wall's
    /// second, not the board's tick, which stands still with nobody watching
    fn on_demand(&self, key: String, build: impl FnOnce(&Stats, Clock) -> Value) -> Arc<Answer> {
        let tick = logs::wall() as u64;
        if let Some((at, answer)) = self
            .asked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            if *at == tick {
                return answer.clone();
            }
        }
        let value = {
            let stats = self.shared.lock();
            let clock = stats.clock(logs::wall());
            build(&stats, clock)
        };
        let answer = Answer::new(value.to_string());
        let mut asked = self.asked.lock().unwrap_or_else(|e| e.into_inner());
        asked.retain(|_, (at, _)| *at + 2 >= tick);
        if asked.len() < 64 {
            asked.insert(key, (tick, answer.clone()));
        }
        answer
    }
}

/// An answer, kept as it is and, the first time a page that takes gzip
/// asks, gzip'd: log lines and paths say the same words again and again
pub struct Answer {
    plain: Bytes,
    gz: OnceLock<Bytes>,
}

/// Under this an answer goes as it is: gzip's header isn't worth it
const GZIP_FROM: usize = 1_024;

impl Answer {
    fn new(text: String) -> Arc<Answer> {
        Arc::new(Answer {
            plain: Bytes::from(text),
            gz: OnceLock::new(),
        })
    }

    fn reply(&self, gzip: bool) -> Reply {
        let builder = Response::builder()
            .status(StatusCode::OK)
            .header("content-type", "application/json")
            .header("cache-control", "no-store")
            .header("vary", "accept-encoding");
        if !gzip || self.plain.len() < GZIP_FROM {
            return builder
                .body(http::full(self.plain.clone()))
                .expect("a valid response");
        }
        let gz = self.gz.get_or_init(|| {
            use std::io::Write;
            let mut out = flate2::write::GzEncoder::new(
                Vec::with_capacity(self.plain.len() / 6),
                flate2::Compression::fast(),
            );
            let _ = out.write_all(&self.plain);
            Bytes::from(out.finish().unwrap_or_default())
        });
        builder
            .header("content-encoding", "gzip")
            .body(http::full(gz.clone()))
            .expect("a valid response")
    }
}

/// What the log tab keeps: a kind of line, a path, words in it
#[derive(Debug, Default, PartialEq)]
struct Only {
    kind: Option<String>,
    path: Option<String>,
    words: Option<String>,
}

impl Only {
    fn keeps(&self, line: &Kept) -> bool {
        let kind = match self.kind.as_deref() {
            Some("4xx") => (400..500).contains(&line.code),
            Some("5xx") => line.code >= 500,
            Some("error") => line.code < 0,
            _ => true,
        };
        kind && self
            .path
            .as_ref()
            .is_none_or(|p| line.path.as_ref() == Some(p))
            && self.words.as_ref().is_none_or(|w| {
                line.text.len() >= w.len()
                    && line
                        .text
                        .as_bytes()
                        .windows(w.len())
                        .any(|at| at.eq_ignore_ascii_case(w.as_bytes()))
            })
    }
}

/// A line taken for the log tab: what it is filtered by, and its JSON
struct Kept {
    n: u64,
    code: i64,
    path: Option<String>,
    text: String,
    json: String,
}

fn push_seconds<'a>(out: &mut String, seconds: impl Iterator<Item = &'a [i64; 4]>) {
    out.push_str(",\"sec\":[");
    for (i, [t, n, c4, c5]) in seconds.enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(out, "[{t},{n},{c4},{c5}]");
    }
    out.push(']');
}

fn push_lines<'a>(out: &mut String, lines: impl Iterator<Item = &'a Kept>) {
    out.push_str(",\"log\":[");
    for (i, line) in lines.enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push_str(&line.json);
    }
    out.push(']');
}

/// What a tick takes while it holds the lock: values, made into text after
struct Taken {
    parts: Vec<(&'static str, Value)>,
    seconds: Vec<[i64; 4]>,
    lines: Vec<Kept>,
}

impl Taken {
    fn from(stats: &Stats, hub: &Hub, wall: i64, lines_from: Option<u64>) -> Taken {
        let clock = stats.clock(wall);
        let offset = i64::from(clock.offset);
        let window = hub.window;
        let now = stats.now(clock, window);
        let minutes = stats.rows(Grain::Minute, clock);
        let usual = logs::typical(&minutes);
        let most = logs::busiest(&minutes);
        let echo = logs::echo(&minutes, Grain::Minute).filter(|r| !r.partial);
        let findings = logs_verdict::findings(stats, clock, window);
        let ms = |us: u64| round(us as f64 / 1_000.0, 3);
        let at = |q: f64| (stats.time.count() > 0).then(|| ms(stats.time.percentile(q)));
        let head = json!({
            "state": state(stats, clock, now.share_5xx(), usual, now.rate, &findings),
            "live": clock.live,
            "now": clock.now + offset,
            "caught_up": stats.caught_up,
            "read": [stats.read_bytes, stats.total_bytes],
            "rate": round(now.rate, 2),
            "usual": usual.map(|u| round(u, 2)),
            "busiest": most.map(|r| round(r.rate, 2)),
            "busiest_at": most.map(|r| Grain::Minute.label(r.start)),
            "echo": echo.map(|r| round(r.rate, 2)),
            "echo_label": Grain::Minute.echo().1,
            "s5xx": round(now.share_5xx(), 5),
            "s4xx": round(now.share_4xx(), 5),
            "p50": at(50.0),
            "p95": at(95.0),
            "p99": at(99.0),
            "total": stats.requests,
            "bytes": stats.bytes,
            "faults": stats.faults,
            "unread": stats.unread,
            "window": window,
            "trouble": stats.trouble,
        });
        let findings = findings.iter().map(finding).collect::<Vec<Value>>();
        let limits = hub
            .conditions
            .iter()
            .map(|c| {
                let check = c.check(stats, clock, window, Scope::Window);
                json!({ "condition": check.text, "held": check.held, "measured": check.measured })
            })
            .collect::<Vec<Value>>();
        let statuses: serde_json::Map<String, Value> = stats
            .statuses
            .iter()
            .map(|(code, n)| (code.to_string(), json!(n)))
            .collect();
        // The busiest paths now: the most asked for over all, by their rate
        let mut top: Vec<(&str, f64, f64)> = stats
            .paths
            .top(TOP * 4, |p| p.requests)
            .into_iter()
            .map(|(path, _)| {
                let now = stats.path_now(clock, window, path);
                (path, now.rate, now.share_5xx())
            })
            .filter(|(_, rate, _)| *rate > 0.0)
            .collect();
        top.sort_by(|a, b| b.1.total_cmp(&a.1));
        top.truncate(TOP);
        let top: Vec<Value> = top
            .into_iter()
            .map(|(path, rate, s5xx)| json!([path, round(rate, 2), round(s5xx, 5)]))
            .collect();
        let causes = causes(stats, CAUSES_SHORT);
        let seconds = {
            let back = stats.seconds_back(clock, SECONDS);
            let end = clock.now + offset;
            back.iter()
                .enumerate()
                .map(|(i, [n, c4, c5])| {
                    [
                        end - (back.len() - 1 - i) as i64,
                        i64::from(*n),
                        i64::from(*c4),
                        i64::from(*c5),
                    ]
                })
                .collect()
        };
        // The lines read since the last tick, the newest of them; the last
        // `LINES` when a page starts asking. A line's number is where it is
        // in what was read, so a page sees the ones that went by as a gap.
        let lines = match lines_from {
            None => Vec::new(),
            Some(last) => {
                let base = stats.recent_base;
                let newest = base + stats.recent.len() as u64;
                let most = if last == 0 { LINES } else { LINES_A_TICK };
                let from = last.max(base).max(newest.saturating_sub(most as u64));
                (from..newest)
                    .map(|n| {
                        let line = &stats.recent[(n - base) as usize];
                        kept(n + 1, line, offset, &hub.parser, hub.exact_paths)
                    })
                    .collect()
            }
        };
        Taken {
            parts: vec![
                ("head", head),
                ("findings", Value::Array(findings)),
                ("limits", Value::Array(limits)),
                ("statuses", Value::Object(statuses)),
                ("top", Value::Array(top)),
                ("causes", causes),
            ],
            seconds,
            lines,
        }
    }
}

fn round(v: f64, places: i32) -> f64 {
    let k = 10f64.powi(places);
    (v * k).round() / k
}

/// The server in a word, as the screen says it
fn state(
    stats: &Stats,
    clock: Clock,
    share_5xx: f64,
    usual: Option<f64>,
    rate: f64,
    findings: &[Finding],
) -> &'static str {
    let worst = findings.iter().map(|f| f.level).max();
    let wrong = findings
        .iter()
        .any(|f| f.level == Level::Warn && f.kind != logs_verdict::Kind::TrafficUp);
    match (share_5xx, usual) {
        _ if !stats.caught_up => "reading",
        _ if stats.last.is_none() => "waiting",
        _ if worst == Some(Level::Bad) => "failing",
        _ if wrong => "degraded",
        (s, _) if s >= 0.05 => "failing",
        (s, _) if s >= 0.01 => "degraded",
        _ if !clock.live => "ended",
        (_, Some(usual)) if usual > 0.0 && rate >= usual * 2.0 => "busy",
        (_, Some(usual)) if rate <= usual * 0.5 => "quiet",
        _ => "steady",
    }
}

fn finding(f: &Finding) -> Value {
    let mut out = json!({ "level": f.level.name(), "kind": f.kind.name(), "text": f.text });
    if let Some(path) = &f.path {
        out["path"] = json!(path);
    }
    if let Some(since) = f.since {
        out["since"] = json!(logs::time_of_day(since));
    }
    out
}

/// The error log's causes; `last` is when, on the log's clock, so the
/// part changes when a count does, not every second as "12s ago" would
fn causes(stats: &Stats, n: usize) -> Value {
    let rows: Vec<Value> = stats
        .top_causes(n)
        .iter()
        .map(|(what, cause)| {
            let message = what.split_once(' ').map_or(*what, |(_, m)| m);
            let last = cause.last.map(|at| at.at + i64::from(at.offset));
            json!({ "level": cause.level, "text": message, "n": cause.count, "last": last })
        })
        .collect();
    json!({ "faults": stats.faults, "levels": stats.levels, "rows": rows })
}

/// A line as the page takes it: `[number, time, code, path, text]`, the
/// code a status, or minus one less the severity for the error log, or
/// 0 for a line nothing was read from
fn kept(n: u64, line: &logs::Recent, offset: i64, parser: &Parser, exact: bool) -> Kept {
    let code: i64 = match line.kind {
        Kind::Request { status } => i64::from(status),
        Kind::Fault { severity } => -1 - i64::from(severity),
        Kind::Unread => 0,
    };
    let text = match line.text.char_indices().nth(LINE_MOST) {
        Some((cut, _)) => &line.text[..cut],
        None => line.text.as_str(),
    };
    let path = match line.kind {
        Kind::Unread => None,
        _ => logs::line_path(&parser.read(text), exact),
    };
    let json = json!([n, logs::time_of_day(line.at + offset), code, path, text]).to_string();
    Kept {
        n,
        code,
        path,
        text: text.to_string(),
        json,
    }
}

/// Every path, for its tab: `[path, requests, 4xx, 5xx, req/s now, p50, p99, upstream share]`
fn paths(stats: &Stats, clock: Clock, window: i64) -> Value {
    let ms = |d: Option<Duration>| d.map(|d| round(d.as_secs_f64() * 1_000.0, 2));
    let rows: Vec<Value> = stats
        .paths
        .top(PATHS, |p| p.requests)
        .into_iter()
        .map(|(path, p)| {
            let now = stats.path_now(clock, window, path);
            json!([
                path,
                p.requests,
                p.c4xx,
                p.c5xx,
                round(now.rate, 2),
                ms(p.times.percentile(0.5)),
                ms(p.times.percentile(0.99)),
                p.upstream_share().map(|s| round(s, 3)),
            ])
        })
        .collect();
    json!({ "rows": rows, "timed": stats.time.count() > 0 })
}

/// A grain's slots: `[start, req/s, 4xx/s, 5xx/s, partial]`
fn slots(rows: &[logs::Row]) -> Value {
    let rows_json: Vec<Value> = rows
        .iter()
        .map(|row| {
            let per = |n: u64| round(row.rate * n as f64 / row.slot.requests.max(1) as f64, 3);
            json!([
                row.start,
                round(row.rate, 3),
                per(row.slot.c4xx),
                per(row.slot.c5xx),
                row.partial
            ])
        })
        .collect();
    json!({ "rows": rows_json, "usual": logs::typical(rows).map(|u| round(u, 3)) })
}

/// A path the page is held to: its numbers now, and its minutes
fn held(stats: &Stats, clock: Clock, window: i64, path: &str) -> Value {
    let Some(p) = stats.paths.count(path) else {
        return json!({ "path": path, "known": false });
    };
    let now = stats.path_now(clock, window, path);
    let ms = |q: f64| {
        p.times
            .percentile(q)
            .map(|d| round(d.as_secs_f64() * 1_000.0, 2))
    };
    let minutes = stats.path_rows(Grain::Minute, clock, path);
    let skip = minutes.len().saturating_sub(120);
    json!({
        "path": path,
        "known": true,
        "rate": round(now.rate, 2),
        "s5xx": round(now.share_5xx(), 5),
        "s4xx": round(now.share_4xx(), 5),
        "requests": p.requests,
        "p50": ms(0.5), "p95": ms(0.95), "p99": ms(0.99),
        "statuses": p.statuses(),
        "minutes": slots(&minutes[skip..])["rows"],
    })
}

/// Bind, and say where
pub async fn listen(spec: &str) -> Result<TcpListener, String> {
    let addr = crate::exporter::parse(spec)?;
    if !addr.ip().is_loopback() {
        // Off loopback the token is all that keeps the log's addresses and
        // paths to those who have it: said, not refused
        eprintln!(
            "note: --serve {addr} is reachable from other machines; the token in the link \
             is what keeps the logs to those who have it"
        );
    }
    TcpListener::bind(addr)
        .await
        .map_err(|e| format!("--serve {addr}: {e}"))
}

/// Answer the page and its calls until the process ends
pub async fn serve(hub: Arc<Hub>, listener: TcpListener) {
    tokio::spawn(hub.clone().ticking());
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        let hub = hub.clone();
        tokio::spawn(async move {
            let service = hyper::service::service_fn(move |request| {
                let hub = hub.clone();
                async move { Ok::<_, std::convert::Infallible>(answer(hub, request).await) }
            });
            let _ = hyper::server::conn::http1::Builder::new()
                .timer(hyper_util::rt::TokioTimer::new())
                .header_read_timeout(Duration::from_secs(10))
                .serve_connection(TokioIo::new(stream), service)
                .await;
        });
    }
}

/// The link that opens the page, the token after `#` so it is never sent
/// in a request, logged by a proxy or kept in a history's address
pub fn link(addr: SocketAddr, token: &str) -> String {
    let host = match addr.ip() {
        ip if ip.is_unspecified() => "127.0.0.1".to_string(),
        std::net::IpAddr::V6(ip) => format!("[{ip}]"),
        ip => ip.to_string(),
    };
    format!("http://{host}:{}/#token={token}", addr.port())
}

const PAGE: &str = include_str!("ui/logs.html");
const SCRIPT: &str = include_str!("ui/logs.js");
const STYLE: &str = include_str!("ui/logs.css");

async fn answer(hub: Arc<Hub>, request: Request<Incoming>) -> Reply {
    let path = request.uri().path().to_string();
    if request.method() != hyper::Method::GET {
        return http::error(StatusCode::METHOD_NOT_ALLOWED, "the page only reads");
    }
    if !path.starts_with("/api/") {
        return match path.as_str() {
            "/" | "/index.html" => super::ui::asset("text/html; charset=utf-8", PAGE),
            "/ui/logs.js" => super::ui::asset("text/javascript; charset=utf-8", SCRIPT),
            "/ui/logs.css" => super::ui::asset("text/css; charset=utf-8", STYLE),
            _ if path.starts_with("/ui/fonts/") || path.starts_with("/ui/img/") => {
                super::ui::serve(&path)
            }
            _ => http::not_found(),
        };
    }
    let tokens = [("page".to_string(), hub.token.clone())];
    if http::who(&request, &tokens).is_none() {
        return http::error(
            StatusCode::UNAUTHORIZED,
            "the token: open the link pepe printed, with its #token=",
        );
    }
    let gzip = request
        .headers()
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').any(|e| e.trim().starts_with("gzip")));
    let query = http::query(&request);
    // What follows takes the reader's lock, and may tick: off the runtime's
    // threads, so a page asking doesn't hold up the others
    tokio::task::spawn_blocking(move || route(hub, &path, &query, gzip))
        .await
        .unwrap_or_else(|_| http::error(StatusCode::INTERNAL_SERVER_ERROR, "the answer failed"))
}

/// An API call, with the token checked
fn route(hub: Arc<Hub>, path: &str, query: &[(String, String)], gzip: bool) -> Reply {
    let get = |name: &str| {
        query
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };
    let window = hub.window;
    match path {
        "/api/stream" => stream(hub),
        "/api/lines" => {
            let after = get("after").and_then(|a| a.parse().ok()).unwrap_or(0);
            let given = |name: &str| {
                get(name)
                    .filter(|v| !v.is_empty() && v.len() <= 512)
                    .map(str::to_string)
            };
            let only = Only {
                kind: given("kind").filter(|k| k != "all"),
                path: given("path"),
                words: given("q"),
            };
            hub.lines_after(after, &only).reply(gzip)
        }
        "/api/paths" => hub
            .on_demand("paths".into(), |s, c| paths(s, c, window))
            .reply(gzip),
        "/api/causes" => hub
            .on_demand("causes".into(), |s, _| causes(s, CAUSES))
            .reply(gzip),
        "/api/slots" => {
            let grain = match get("grain") {
                Some("hour") => Grain::Hour,
                Some("day") => Grain::Day,
                _ => Grain::Minute,
            };
            hub.on_demand(format!("slots:{}", grain.name()), move |s, c| {
                let rows = s.rows(grain, c);
                let skip = rows.len().saturating_sub(match grain {
                    Grain::Minute => 180,
                    Grain::Hour => 168,
                    Grain::Day => 90,
                });
                slots(&rows[skip..])
            })
            .reply(gzip)
        }
        "/api/path" => match get("path") {
            Some(path) if path.len() <= 512 => {
                let path = path.to_string();
                hub.on_demand(format!("path:{path}"), move |s, c| {
                    held(s, c, window, &path)
                })
                .reply(gzip)
            }
            _ => http::error(StatusCode::BAD_REQUEST, "?path=/the/path"),
        },
        "/api/report" => {
            let stats = hub.shared.lock();
            let wall = logs::wall();
            let clock = stats.clock(wall);
            let findings = logs_verdict::findings(&stats, clock, window);
            let checks: Vec<_> = hub
                .conditions
                .iter()
                .map(|c| c.check(&stats, clock, window, Scope::Window))
                .collect();
            let mut report = json!({ "schema_version": 1, "title": hub.title });
            if let Value::Object(verdict) = logs_verdict::json(&findings, &checks, clock.offset) {
                for (k, v) in verdict {
                    report[k.as_str()] = v;
                }
            }
            http::ok(&report)
        }
        "/api/cost" => http::ok(&json!({
            "viewers": hub.viewers.load(Ordering::Relaxed),
            "ticks": hub.cost.ticks.load(Ordering::Relaxed),
            "tick_us": hub.cost.build_us.load(Ordering::Relaxed),
            "locked_us": hub.cost.locked_us.load(Ordering::Relaxed),
            "bytes_out": hub.cost.bytes_out.load(Ordering::Relaxed),
        })),
        _ => http::not_found(),
    }
}

/// One viewer: everything when it comes, then each tick's step. The same
/// bytes go to every viewer that is up to date
fn stream(hub: Arc<Hub>) -> Reply {
    struct Viewer {
        hub: Arc<Hub>,
        rx: watch::Receiver<u64>,
        seen: u64,
        first: bool,
    }
    impl Drop for Viewer {
        fn drop(&mut self) {
            self.hub.viewers.fetch_sub(1, Ordering::Relaxed);
        }
    }
    let first_viewer = hub.viewers.fetch_add(1, Ordering::Relaxed) == 0;
    // The first to come doesn't wait a second for a picture
    if first_viewer || hub.board().tick == 0 {
        hub.tick();
    }
    let viewer = Viewer {
        rx: hub.ticked.subscribe(),
        hub,
        seen: 0,
        first: true,
    };
    let stream = futures::stream::unfold(viewer, |mut v| async move {
        let bytes = if v.first {
            v.first = false;
            v.rx.borrow_and_update();
            let (bytes, tick) = v.hub.catch_up(0);
            v.seen = tick;
            bytes
        } else {
            tokio::select! {
                changed = v.rx.changed() => {
                    changed.ok()?;
                    let tick = *v.rx.borrow_and_update();
                    let board = v.hub.board();
                    if board.tick == v.seen + 1 && tick == board.tick {
                        // Up to date: the step everyone gets
                        v.seen = board.tick;
                        board.step.clone()
                    } else {
                        drop(board);
                        let (bytes, tick) = v.hub.catch_up(v.seen);
                        v.seen = tick;
                        bytes
                    }
                }
                _ = tokio::time::sleep(KEEP_ALIVE) => Bytes::from_static(b": still here\n\n"),
            }
        };
        v.hub
            .cost
            .bytes_out
            .fetch_add(bytes.len() as u64, Ordering::Relaxed);
        Some((Ok::<_, std::convert::Infallible>(Frame::data(bytes)), v))
    });
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/event-stream")
        .header("cache-control", "no-store")
        .header("x-accel-buffering", "no")
        .body(StreamBody::new(stream).boxed())
        .expect("a valid response")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hub_with(lines: &[String]) -> Arc<Hub> {
        let parser = Parser::default();
        let shared = Arc::new(Shared::default());
        {
            let mut stats = shared.lock();
            let wall = logs::wall();
            for line in lines {
                stats.fold(parser.read(line), line, wall);
            }
            stats.caught_up = true;
        }
        Hub::new(
            shared,
            parser,
            false,
            "access.log".into(),
            60,
            vec![Condition::parse("5xx > 2%").unwrap()],
            "0123456789abcdef0123".into(),
        )
    }

    fn combined(at: i64, path: &str, status: u16) -> String {
        let c = logs::civil(at);
        const MONTHS: [&str; 12] = [
            "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
        ];
        format!(
            "203.0.113.9 - - [{:02}/{}/{}:{:02}:{:02}:{:02} +0000] \"GET {path} HTTP/1.1\" {status} 512 \"-\" \"curl/8\"",
            c.day,
            MONTHS[c.month as usize - 1],
            c.year,
            c.hour,
            c.minute,
            c.second
        )
    }

    fn data(bytes: &Bytes) -> Value {
        let text = std::str::from_utf8(bytes).unwrap();
        let data = text
            .lines()
            .find_map(|l| l.strip_prefix("data: "))
            .expect("a data line");
        serde_json::from_str(data).expect("the data is JSON")
    }

    fn lines(bytes: &Bytes) -> Value {
        serde_json::from_slice(bytes).expect("the lines are JSON")
    }

    #[test]
    fn a_viewer_that_comes_gets_everything_and_then_only_what_changed() {
        let wall = logs::wall();
        let read: Vec<String> = (0..50)
            .map(|i| combined(wall - 50 + i, if i % 2 == 0 { "/a" } else { "/b/12" }, 200))
            .collect();
        let hub = hub_with(&read);
        hub.tick();
        let (first, tick) = hub.catch_up(0);
        let first = data(&first);
        assert_eq!(first["title"], "access.log");
        assert_eq!(first["head"]["total"], 50);
        assert_eq!(first["limits"][0]["held"], true);
        assert!(first["sec"].as_array().unwrap().len() >= 50);
        assert!(first.get("log").is_none(), "the lines aren't in the stream");
        assert_eq!(tick, 1);

        // Nothing new: a tick that changes nothing sends nothing
        let before = hub.board().step.clone();
        hub.tick();
        let after = hub.board();
        if after.tick == 1 {
            assert_eq!(after.step, before);
        } else {
            // the clock moved on a second: the seconds, nothing said before
            let step = data(&after.step);
            assert!(step.get("title").is_none() && step.get("limits").is_none());
        }
        drop(after);

        // A 503: the step has the new code, and a few seconds at most
        {
            let mut stats = hub.shared.lock();
            let line = combined(wall, "/c", 503);
            stats.fold(hub.parser.read(&line), &line, wall);
        }
        hub.tick();
        let step = data(&hub.board().step);
        assert!(step["sec"].as_array().unwrap().len() <= RESENT);
        assert!(step.get("statuses").is_some(), "the 503 is a new code");
        assert!(step.get("findings").is_some() || step.get("head").is_some());

        // A viewer a tick behind catches up from where it was
        let (caught, _) = hub.catch_up(tick);
        let caught = data(&caught);
        assert!(caught.get("title").is_none() && caught.get("statuses").is_some());
    }

    #[test]
    fn the_lines_are_taken_only_while_asked_for() {
        let wall = logs::wall();
        let read: Vec<String> = (0..50)
            .map(|i| combined(wall - 50 + i, if i % 2 == 0 { "/a" } else { "/b/12" }, 200))
            .collect();
        let hub = hub_with(&read);
        hub.tick();
        assert!(hub.board().lines.is_empty(), "nobody asked");
        let all = Only::default();
        let first = lines(&hub.lines_after(0, &all).plain);
        assert_eq!(first["log"].as_array().unwrap().len(), 50);
        assert_eq!(first["log"][0][3], "/a");
        assert_eq!(
            first["log"][1][3], "/b/*",
            "paths are grouped as the screen groups them"
        );
        assert_eq!(first["last"], 50);
        // Many at once: the newest go, the rest a gap in the numbers
        {
            let mut stats = hub.shared.lock();
            for _ in 0..(LINES_A_TICK + 300) {
                let line = combined(wall, "/a", 500);
                stats.fold(hub.parser.read(&line), &line, wall);
            }
        }
        hub.tick();
        let next = lines(&hub.lines_after(50, &all).plain);
        let log = next["log"].as_array().unwrap();
        assert_eq!(log.len(), LINES_A_TICK);
        assert_eq!(log[0][0], 50 + 300 + 1);
        assert_eq!(log[0][2], 500);
        // The same question in the same tick is the same answer
        assert!(Arc::ptr_eq(
            &hub.lines_after(50, &all),
            &hub.lines_after(50, &all)
        ));
        // Kept to what the page shows: the error lines only, or words
        let only = |kind: &str, words: Option<&str>| Only {
            kind: Some(kind.into()),
            path: None,
            words: words.map(str::to_string),
        };
        let fives = lines(&hub.lines_after(0, &only("5xx", None)).plain);
        assert!(fives["log"].as_array().unwrap().iter().all(|l| l[2] == 500));
        let fours = lines(&hub.lines_after(0, &only("4xx", None)).plain);
        assert!(fours["log"].as_array().unwrap().is_empty());
        let words = lines(&hub.lines_after(0, &only("all", Some("GET /B/"))).plain);
        assert_eq!(
            words["log"].as_array().unwrap().len(),
            25,
            "words are matched in any case"
        );
        // Gzip'd once, when a page takes it
        let answer = hub.lines_after(50, &all);
        let reply = answer.reply(true);
        assert_eq!(reply.headers()["content-encoding"], "gzip");
        assert!(answer.gz.get().unwrap().len() * 5 < answer.plain.len());
    }

    #[test]
    fn parts_on_demand_are_built_once_a_tick() {
        let wall = logs::wall();
        let hub = hub_with(&[combined(wall - 1, "/a", 200), combined(wall, "/b", 500)]);
        hub.tick();
        let built = AtomicUsize::new(0);
        for _ in 0..3 {
            hub.on_demand("paths".into(), |s, c| {
                built.fetch_add(1, Ordering::Relaxed);
                paths(s, c, 60)
            });
        }
        assert_eq!(built.load(Ordering::Relaxed), 1);
        let stats = hub.shared.lock();
        let held = held(&stats, stats.clock(wall), 60, "/b");
        assert_eq!(held["requests"], 1);
    }

    #[test]
    fn the_link_carries_the_token_after_the_hash() {
        let addr: SocketAddr = "0.0.0.0:7880".parse().unwrap();
        assert_eq!(link(addr, "t0k"), "http://127.0.0.1:7880/#token=t0k");
        let addr: SocketAddr = "[::1]:80".parse().unwrap();
        assert_eq!(link(addr, "t"), "http://[::1]:80/#token=t");
    }

    #[test]
    fn the_page_sets_text_never_markup() {
        for bad in [
            "innerHTML",
            "outerHTML",
            "insertAdjacentHTML",
            "eval(",
            "document.write",
        ] {
            assert!(!SCRIPT.contains(bad), "{bad}");
        }
        assert!(PAGE.contains("/ui/logs.js") && PAGE.contains("/ui/logs.css"));
        assert!(
            !PAGE.contains("<script>"),
            "no inline script: the policy allows none"
        );
    }
}
