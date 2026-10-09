//! Logs: read nginx's access and error logs, as they are written or after
//! the fact, and say how busy the server is now against how busy it was.
//!
//! Lines are read as a stream and folded into counts of a fixed size, so a
//! log of any length takes the same memory: requests per second for the
//! last hour, and per minute, hour and day further back; the most seen
//! paths, clients and user agents; the error log's messages by cause.
//!
//! Access logs are read as nginx's `combined` format (which Apache shares),
//! as the `log_format` given with `--format`, or as JSON lines. A line that
//! fits none of them is still searched for a time, a request and a status.

use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use crate::metrics::Histogram;
use crate::ui::format;

/// Seconds kept one by one: the rate now, and each slot's busiest second
const SECONDS_KEPT: usize = 3_600;
/// Lines kept as written, for the log view
pub const RECENT: usize = 2_000;
/// A line kept for the log view is cut here
const LINE_KEPT: usize = 8 * 1024;
/// A log whose last line is this close to the clock is being written now
const LIVE: i64 = 300;
/// How often a followed file is looked at again
const POLL: Duration = Duration::from_millis(250);
/// Read this much of a file at a time; the counts are locked once for each
const CHUNK: usize = 256 * 1024;
/// Distinct paths, clients, user agents and parameter names counted; what
/// comes after is counted together
const PATHS: usize = 20_000;
const CLIENTS: usize = 50_000;
const AGENTS: usize = 2_000;
const PARAMS: usize = 2_000;
const CAUSES: usize = 1_000;
/// A file is read by every core, this much of it at a time for each
const SPRINT_CHUNK: u64 = 1 << 20;
/// The last lines the log view keeps are in this much of a file's end,
/// each being at most `LINE_KEPT` long
const SPRINT_KEEP: u64 = (RECENT * LINE_KEPT * 2) as u64;
/// A file smaller than this is read before more threads could be started
const SPRINT_MIN: u64 = 16 << 20;
/// A thread hands its counts over this often, so the screen has something
/// to show while a long log is read
const SPRINT_MERGE: Duration = Duration::from_secs(1);

// ─── Time ────────────────────────────────────────────────────────────────────

/// When a line was written: seconds since the epoch, and the offset from
/// UTC its log wrote the time in
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub at: i64,
    pub offset: i32,
}

/// A time on the calendar
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
    /// Monday is 0
    pub weekday: u32,
}

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let year = if month <= 2 { year - 1 } else { year };
    let era = year.div_euclid(400);
    let yoe = year.rem_euclid(400);
    let mp = (month as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The calendar's reading of `local`: seconds since the epoch, already
/// moved by the offset to show it in
pub fn civil(local: i64) -> Civil {
    let days = local.div_euclid(86_400);
    let secs = local.rem_euclid(86_400) as u32;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    Civil {
        year: yoe + era * 400 + i64::from(month <= 2),
        month,
        day,
        hour: secs / 3_600,
        minute: secs % 3_600 / 60,
        second: secs % 60,
        weekday: (days + 3).rem_euclid(7) as u32,
    }
}

fn digits(b: &[u8]) -> Option<i64> {
    if b.is_empty() || b.len() > 18 {
        return None;
    }
    let mut n = 0i64;
    for &c in b {
        if !c.is_ascii_digit() {
            return None;
        }
        n = n * 10 + i64::from(c - b'0');
    }
    Some(n)
}

/// `+0200`, `+02:00` or `Z`
fn zone(b: &[u8]) -> Option<i32> {
    let (sign, rest) = match b.first()? {
        b'Z' | b'z' => return Some(0),
        b'+' => (1, &b[1..]),
        b'-' => (-1, &b[1..]),
        _ => return None,
    };
    let (hours, minutes) = match rest.len() {
        2 => (digits(rest)?, 0),
        4 => (digits(&rest[..2])?, digits(&rest[2..])?),
        5 if rest[2] == b':' => (digits(&rest[..2])?, digits(&rest[3..])?),
        _ => return None,
    };
    Some(sign * (hours * 3_600 + minutes * 60) as i32)
}

/// This machine's offset from UTC: what nginx writes its error log in
pub fn local_offset() -> i32 {
    static LOCAL: OnceLock<i32> = OnceLock::new();
    *LOCAL.get_or_init(|| {
        #[cfg(unix)]
        {
            extern "C" {
                fn tzset();
            }
            // SAFETY: tzset reads the environment's zone, which localtime_r
            // needn't do by itself; localtime_r fills the `tm` it is given
            unsafe {
                tzset();
                let now = wall() as libc::time_t;
                let mut tm: libc::tm = std::mem::zeroed();
                if libc::localtime_r(&now, &mut tm).is_null() {
                    0
                } else {
                    tm.tm_gmtoff as i32
                }
            }
        }
        #[cfg(not(unix))]
        {
            0
        }
    })
}

/// Seconds since the epoch, by this machine's clock
pub fn wall() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

impl Stamp {
    fn from_parts(date: (i64, i64, i64), time: (i64, i64, i64), offset: i32) -> Option<Stamp> {
        let (year, month, day) = date;
        let (hour, minute, second) = time;
        let valid = (1..=12).contains(&month)
            && (1..=31).contains(&day)
            && hour < 24
            && minute < 60
            && second < 61;
        valid.then(|| Stamp {
            at: days_from_civil(year, month as u32, day as u32) * 86_400
                + hour * 3_600
                + minute * 60
                + second
                - i64::from(offset),
            offset,
        })
    }

    /// nginx's `$time_local` (`10/Oct/2026:13:55:36 +0000`), `$time_iso8601`
    /// (`2026-10-10T13:55:36+00:00`), `$msec`, and the error log's
    /// `2026/10/10 13:55:36`. A time that names no offset is this machine's.
    pub fn parse(text: &str) -> Option<Stamp> {
        let b = text.trim().as_bytes();
        if b.len() >= 20 && b[2] == b'/' && b[6] == b'/' {
            let month = MONTHS
                .iter()
                .position(|m| m.as_bytes().eq_ignore_ascii_case(&b[3..6]))?;
            let offset = match b.get(21..) {
                Some(rest) if !rest.is_empty() => zone(rest)?,
                _ => local_offset(),
            };
            return Self::from_parts(
                (digits(&b[7..11])?, month as i64 + 1, digits(&b[..2])?),
                (
                    digits(&b[12..14])?,
                    digits(&b[15..17])?,
                    digits(&b[18..20])?,
                ),
                offset,
            );
        }
        if b.len() >= 19 && matches!(b[4], b'-' | b'/') && matches!(b[7], b'-' | b'/') {
            let mut rest = &b[19..];
            if matches!(rest.first(), Some(b'.' | b',')) {
                let fraction = rest[1..].iter().take_while(|c| c.is_ascii_digit()).count();
                rest = &rest[1 + fraction..];
            }
            let offset = match rest.trim_ascii() {
                [] => local_offset(),
                rest => zone(rest)?,
            };
            return Self::from_parts(
                (digits(&b[..4])?, digits(&b[5..7])?, digits(&b[8..10])?),
                (
                    digits(&b[11..13])?,
                    digits(&b[14..16])?,
                    digits(&b[17..19])?,
                ),
                offset,
            );
        }
        let whole = b.split(|&c| c == b'.').next()?;
        // Before 2001 it isn't a time anyone's log has
        let n = digits(whole).filter(|&n| n >= 1_000_000_000)?;
        Some(Stamp::epoch(n as f64))
    }

    /// `parse`, remembering the last answer: a log's lines come many to a
    /// second, each with the same time written the same way
    fn remembered(text: &str) -> Option<Stamp> {
        thread_local! {
            static LAST: std::cell::Cell<([u8; 40], usize, Option<Stamp>)> =
                const { std::cell::Cell::new(([0; 40], 0, None)) };
        }
        let b = text.as_bytes();
        if b.is_empty() || b.len() > 40 {
            return Stamp::parse(text);
        }
        let (bytes, len, stamp) = LAST.get();
        if len == b.len() && bytes[..len] == *b {
            return stamp;
        }
        let stamp = Stamp::parse(text);
        let mut bytes = [0; 40];
        bytes[..b.len()].copy_from_slice(b);
        LAST.set((bytes, b.len(), stamp));
        stamp
    }

    /// Seconds since the epoch, or milliseconds when it is too large for those
    fn epoch(n: f64) -> Stamp {
        let at = if n >= 1e11 { n / 1_000.0 } else { n } as i64;
        Stamp {
            at,
            offset: local_offset(),
        }
    }

    pub fn local(self) -> i64 {
        self.at + i64::from(self.offset)
    }

    /// `2026-10-10T13:55:36+00:00`
    pub fn iso(self) -> String {
        let c = civil(self.local());
        let sign = if self.offset < 0 { '-' } else { '+' };
        let offset = self.offset.unsigned_abs();
        format!(
            "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}{sign}{:02}:{:02}",
            c.year,
            c.month,
            c.day,
            c.hour,
            c.minute,
            c.second,
            offset / 3_600,
            offset % 3_600 / 60
        )
    }
}

/// `+02:00`, for saying which clock the times are on
pub fn offset_label(offset: i32) -> String {
    let sign = if offset < 0 { '-' } else { '+' };
    let offset = offset.unsigned_abs();
    format!("UTC{sign}{:02}:{:02}", offset / 3_600, offset % 3_600 / 60)
}

/// `08 Oct 13:55:36`
pub fn day_and_time(local: i64) -> String {
    let c = civil(local);
    format!(
        "{:02} {} {:02}:{:02}:{:02}",
        c.day,
        MONTHS[c.month as usize - 1],
        c.hour,
        c.minute,
        c.second
    )
}

/// `13:55:36`
pub fn time_of_day(local: i64) -> String {
    let c = civil(local);
    format!("{:02}:{:02}:{:02}", c.hour, c.minute, c.second)
}

/// How long ago, roughly: 45s, 12m, 3h, 5d
pub fn ago(seconds: i64) -> String {
    match seconds.max(0) {
        s @ 0..=119 => format!("{s}s"),
        s @ 120..=7_199 => format!("{}m", s / 60),
        s @ 7_200..=172_799 => format!("{}h", s / 3_600),
        s => format!("{}d", s / 86_400),
    }
}

/// `90s`, `15m`, `24h`, `7d`; a bare number is seconds
pub fn span(text: &str) -> Result<i64, String> {
    let text = text.trim().to_ascii_lowercase();
    let split = text
        .find(|c: char| !c.is_ascii_digit())
        .unwrap_or(text.len());
    let (number, unit) = text.split_at(split);
    let number: i64 = number
        .parse()
        .map_err(|_| format!("'{text}' isn't a length of time: try 90s, 15m, 24h or 7d"))?;
    let unit = match unit.trim() {
        "" | "s" | "sec" | "second" | "seconds" => 1,
        "m" | "min" | "minute" | "minutes" => 60,
        "h" | "hour" | "hours" => 3_600,
        "d" | "day" | "days" => 86_400,
        "w" | "week" | "weeks" => 7 * 86_400,
        other => {
            return Err(format!(
                "'{other}' isn't a unit of time: use s, m, h, d or w"
            ))
        }
    };
    match number * unit {
        0 => Err("a length of time can't be zero".into()),
        seconds => Ok(seconds),
    }
}

// ─── Lines ───────────────────────────────────────────────────────────────────

/// What an access log line says of its request, as far as it said it
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Record<'a> {
    pub at: Option<Stamp>,
    pub client: Cow<'a, str>,
    pub method: Cow<'a, str>,
    /// The path and its query
    pub target: Cow<'a, str>,
    /// 0 when the line had none
    pub status: u16,
    pub bytes: u64,
    pub host: Cow<'a, str>,
    pub agent: Cow<'a, str>,
    /// `$request_time`, in seconds
    pub time: Option<f64>,
    /// `$upstream_response_time`, in seconds; the sum when there were several
    pub upstream: Option<f64>,
}

/// An error log line
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Fault<'a> {
    pub at: Option<Stamp>,
    /// debug, info, notice, warn, error, crit, alert or emerg
    pub level: &'a str,
    /// What went wrong, without who it happened to
    pub message: &'a str,
    pub client: &'a str,
    pub request: &'a str,
    pub upstream: &'a str,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Line<'a> {
    Request(Record<'a>),
    Fault(Fault<'a>),
    /// Nothing could be read from it
    Unread,
}

/// 0 for what is only said, 1 for a warning, 2 for an error and worse
pub fn severity(level: &str) -> u8 {
    match level {
        "warn" => 1,
        "error" | "crit" | "alert" | "emerg" => 2,
        _ => 0,
    }
}

/// What a `log_format` variable is to pepe
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Field {
    Client,
    At,
    Request,
    Method,
    Target,
    Status,
    Bytes,
    Host,
    Agent,
    Time,
    Upstream,
    Other,
}

impl Field {
    fn of(name: &str) -> Field {
        match name {
            "remote_addr" | "realip_remote_addr" => Field::Client,
            "time_local" | "time_iso8601" | "msec" => Field::At,
            "request" => Field::Request,
            "request_method" => Field::Method,
            "request_uri" | "uri" => Field::Target,
            "status" => Field::Status,
            "body_bytes_sent" | "bytes_sent" => Field::Bytes,
            "host" | "http_host" | "server_name" => Field::Host,
            "http_user_agent" => Field::Agent,
            "request_time" => Field::Time,
            "upstream_response_time" => Field::Upstream,
            _ => Field::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
enum Part {
    Text(String),
    Var(Field, String),
}

/// An nginx `log_format`: the text between its variables says where each
/// variable's value ends
#[derive(Debug, Clone, PartialEq)]
pub struct Format {
    parts: Vec<Part>,
}

/// nginx's `combined`, which a log has when nginx.conf names no other
pub const COMBINED: &str = "$remote_addr - $remote_user [$time_local] \"$request\" $status $body_bytes_sent \"$http_referer\" \"$http_user_agent\"";

impl Format {
    /// A `log_format` as nginx.conf has it, its quoted pieces joined into
    /// one line; `combined` and `common` name nginx's own
    pub fn parse(text: &str) -> Result<Format, String> {
        let text = match text.trim() {
            "combined" => COMBINED,
            "common" => {
                "$remote_addr - $remote_user [$time_local] \"$request\" $status $body_bytes_sent"
            }
            text => text,
        };
        let mut parts: Vec<Part> = Vec::new();
        let mut literal = String::new();
        let mut chars = text.chars().peekable();
        while let Some(c) = chars.next() {
            if c != '$' {
                literal.push(c);
                continue;
            }
            let braced = chars.next_if_eq(&'{').is_some();
            let mut name = String::new();
            while let Some(c) = chars.next_if(|c| c.is_ascii_alphanumeric() || *c == '_') {
                name.push(c);
            }
            if braced {
                chars.next_if_eq(&'}');
            }
            if name.is_empty() {
                literal.push('$');
                continue;
            }
            if !literal.is_empty() {
                parts.push(Part::Text(std::mem::take(&mut literal)));
            } else if let Some(Part::Var(_, before)) = parts.last() {
                return Err(format!(
                    "${before} and ${name} have nothing between them, so where one ends can't be told"
                ));
            }
            parts.push(Part::Var(Field::of(&name), name));
        }
        if !literal.is_empty() {
            parts.push(Part::Text(literal));
        }
        let known = |part: &Part| matches!(part, Part::Var(field, _) if *field != Field::Other);
        if !parts.iter().any(known) {
            return Err(
                "the format has none of the variables pepe reads: $time_local, $request, $status, \
                 $remote_addr, $body_bytes_sent, $request_time, $upstream_response_time, …"
                    .into(),
            );
        }
        Ok(Format { parts })
    }

    fn has(&self, field: Field) -> bool {
        self.parts
            .iter()
            .any(|p| matches!(p, Part::Var(f, _) if *f == field))
    }

    /// Give each variable its value, in order, as far as the line follows
    /// the format; what is left of the line comes back
    fn walk<'a>(&self, line: &'a str, mut each: impl FnMut(Field, &str, &'a str)) -> &'a str {
        let mut rest = line;
        for (i, part) in self.parts.iter().enumerate() {
            match part {
                Part::Text(text) => match rest.strip_prefix(text.as_str()) {
                    Some(after) => rest = after,
                    None => return rest,
                },
                Part::Var(field, name) => {
                    let end = match self.parts.get(i + 1) {
                        Some(Part::Text(text)) => find(rest, text),
                        _ => None,
                    };
                    // A line that ends early gives the variable what is left
                    let end = end.unwrap_or(rest.len());
                    each(*field, name, &rest[..end]);
                    rest = &rest[end..];
                }
            }
        }
        rest
    }

    fn read<'a>(&self, line: &'a str) -> (Option<Record<'a>>, &'a str) {
        let mut record = Record::default();
        let mut status = None;
        let rest = self.walk(line, |field, _, value| {
            let value = if value == "-" { "" } else { value };
            match field {
                Field::Client => record.client = value.into(),
                Field::At => record.at = Stamp::remembered(value),
                Field::Request => {
                    let (method, target) = request_line(value);
                    record.method = method.into();
                    record.target = target.into();
                }
                Field::Method => record.method = value.into(),
                Field::Target => record.target = value.into(),
                Field::Status => status = value.parse::<u16>().ok(),
                Field::Bytes => record.bytes = value.parse().unwrap_or(0),
                Field::Host => record.host = value.into(),
                Field::Agent => record.agent = value.into(),
                Field::Time => record.time = seconds(value),
                Field::Upstream => record.upstream = seconds(value),
                Field::Other => {}
            }
        });
        // The line is the format's if what the format always has is there
        let dated = !self.has(Field::At) || record.at.is_some();
        let answered = match status {
            Some(code @ 100..=599) => {
                record.status = code;
                true
            }
            _ => !self.has(Field::Status),
        };
        ((dated && answered).then_some(record), rest)
    }
}

/// Where `needle` starts in `hay`. The text between a format's variables
/// is a character or three, which is found sooner by looking for its first
/// byte than `str::find` has made itself a searcher.
fn find(hay: &str, needle: &str) -> Option<usize> {
    let (hay, needle) = (hay.as_bytes(), needle.as_bytes());
    let (first, rest) = needle.split_first()?;
    let mut from = 0;
    loop {
        let at = from + memchr::memchr(*first, hay.get(from..)?)?;
        let after = hay.get(at + 1..at + 1 + rest.len())?;
        if after.iter().zip(rest).all(|(a, b)| a == b) {
            return Some(at);
        }
        from = at + 1;
    }
}

/// `text` before and after the first `byte`, which is ASCII
fn split_at_byte(text: &str, byte: u8) -> Option<(&str, &str)> {
    let at = memchr::memchr(byte, text.as_bytes())?;
    Some((&text[..at], &text[at + 1..]))
}

/// The method and target of `GET /path HTTP/1.1`; a request nginx couldn't
/// read (a TLS handshake sent to port 80, say) is all target
fn request_line(request: &str) -> (&str, &str) {
    match split_at_byte(request, b' ') {
        Some((method, rest)) if is_method(method) => {
            let target = split_at_byte(rest, b' ').map_or(rest, |(target, _)| target);
            (method, target)
        }
        _ => ("", request),
    }
}

fn is_method(word: &str) -> bool {
    !word.is_empty() && word.len() <= 16 && word.bytes().all(|c| c.is_ascii_uppercase())
}

/// A time in seconds; nginx writes several, `0.004, 0.010 : 0.002`, when a
/// request went to more than one upstream, and they add up
fn seconds(value: &str) -> Option<f64> {
    // One time, as nginx writes it: `0.004`
    if let Some(one) = decimal(value) {
        return Some(one);
    }
    let mut sum = 0.0;
    let mut any = false;
    for part in value.split([',', ':']) {
        let part = part.trim();
        if part.is_empty() || part == "-" {
            continue;
        }
        sum += part.parse::<f64>().ok().filter(|v| *v >= 0.0)?;
        any = true;
    }
    any.then_some(sum)
}

/// `12.345` without the care `f64`'s own reading takes over digits a log
/// doesn't have
fn decimal(text: &str) -> Option<f64> {
    let b = text.as_bytes();
    if b.is_empty() || b.len() > 15 {
        return None;
    }
    let (mut n, mut scale, mut point) = (0u64, 1u64, false);
    for &c in b {
        match c {
            b'0'..=b'9' => {
                n = n * 10 + u64::from(c - b'0');
                if point {
                    scale *= 10;
                }
            }
            b'.' if !point => point = true,
            _ => return None,
        }
    }
    Some(n as f64 / scale as f64)
}

/// How access log lines are read
#[derive(Debug, Clone)]
pub struct Parser {
    /// The `log_format` given; `combined`, and then a search, without one
    given: Option<Format>,
    combined: Format,
}

impl Default for Parser {
    fn default() -> Self {
        Parser {
            given: None,
            combined: Format::parse(COMBINED).expect("nginx's own format"),
        }
    }
}

impl Parser {
    pub fn new(format: Option<&str>) -> Result<Parser, String> {
        Ok(Parser {
            given: format.map(Format::parse).transpose()?,
            ..Parser::default()
        })
    }

    pub fn read<'a>(&self, line: &'a str) -> Line<'a> {
        let line = unprefixed(line.trim());
        if line.starts_with('{') {
            return json(line).map_or(Line::Unread, Line::Request);
        }
        if let Some(fault) = fault(line) {
            return Line::Fault(fault);
        }
        if let Some(format) = &self.given {
            return format.read(line).0.map_or(Line::Unread, Line::Request);
        }
        let (record, rest) = self.combined.read(line);
        match record.or_else(|| search(line)) {
            Some(mut record) => {
                extras(rest, &mut record);
                Line::Request(record)
            }
            None => Line::Unread,
        }
    }

    /// Everything read from a line, named: what the log view shows of the
    /// line picked there
    pub fn inspect(&self, line: &str) -> Vec<(String, String)> {
        let line = unprefixed(line.trim());
        let mut rows: Vec<(String, String)> = Vec::new();
        let mut row = |name: &str, value: String| {
            if !value.is_empty() {
                rows.push((name.to_string(), value));
            }
        };
        match self.read(line) {
            Line::Request(r) => {
                let (path, query) = r.target.split_once('?').unwrap_or((&r.target, ""));
                row("time", r.at.map(Stamp::iso).unwrap_or_default());
                row("client", r.client.to_string());
                row("host", r.host.to_string());
                row("method", r.method.to_string());
                row("path", path.to_string());
                for pair in query.split('&').filter(|p| !p.is_empty()) {
                    let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
                    row(&format!("?{name}"), value.to_string());
                }
                if r.status > 0 {
                    row("status", r.status.to_string());
                }
                row("bytes sent", format::count(r.bytes));
                let time = |t: f64| format::latency(Duration::from_secs_f64(t));
                row("request time", r.time.map(time).unwrap_or_default());
                row("upstream time", r.upstream.map(time).unwrap_or_default());
                row("user agent", r.agent.to_string());
                // The variables pepe has no use for, as the format named them
                if let Some(format) = &self.given {
                    format.walk(line, |field, name, value| {
                        if field == Field::Other && value != "-" {
                            row(&format!("${name}"), value.to_string());
                        }
                    });
                } else if !line.starts_with('{') {
                    self.combined.walk(line, |field, name, value| {
                        if field == Field::Other && value != "-" {
                            row(&format!("${name}"), value.to_string());
                        }
                    });
                }
            }
            Line::Fault(f) => {
                row("time", f.at.map(Stamp::iso).unwrap_or_default());
                row("level", f.level.to_string());
                row("message", f.message.to_string());
                row("client", f.client.to_string());
                row("request", f.request.to_string());
                row("upstream", f.upstream.to_string());
            }
            Line::Unread => row(
                "not read",
                "the line fits neither the access log's format nor the error log's".into(),
            ),
        }
        rows
    }
}

/// The line without the `nginx-1  | ` that `docker compose logs` puts in
/// front of each one
fn unprefixed(line: &str) -> &str {
    let b = line.as_bytes();
    let name = b
        .iter()
        .take(64)
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.'))
        .count();
    let spaces = b[name..].iter().take_while(|c| **c == b' ').count();
    let bar = name + spaces;
    if name > 0 && spaces > 0 && b.get(bar) == Some(&b'|') && b.get(bar + 1) == Some(&b' ') {
        line[bar + 2..].trim_start()
    } else {
        line
    }
}

/// `rt=0.005 urt="0.004"` after the format's end, as nginx's own example
/// of timing in a log has them
fn extras(rest: &str, record: &mut Record) {
    let b = rest.as_bytes();
    let mut from = 0;
    while let Some(equals) = memchr::memchr(b'=', &b[from..]).map(|i| from + i) {
        let key = memchr::memrchr(b' ', &b[from..equals]).map_or(from, |i| from + i + 1);
        // A quoted value may have spaces: `urt="0.004, 0.010"`
        let (value, next) = if b.get(equals + 1) == Some(&b'"') {
            let open = equals + 2;
            let close = memchr::memchr(b'"', &b[open..]).map_or(b.len(), |i| open + i);
            (&rest[open..close], (close + 1).min(b.len()))
        } else {
            let end = memchr::memchr(b' ', &b[equals + 1..]).map_or(b.len(), |i| equals + 1 + i);
            (&rest[equals + 1..end], end)
        };
        match &rest[key..equals] {
            "rt" | "request_time" => record.time = seconds(value),
            "urt" | "upstream_response_time" => record.upstream = seconds(value),
            _ => {}
        }
        from = next;
    }
}

/// A line that isn't `combined`: look for the quoted request, a time in
/// brackets before it, and the status and size after it
fn search(line: &str) -> Option<Record<'_>> {
    let mut record = Record::default();
    let mut from = 0;
    let mut after = None;
    while let Some(open) = line[from..].find('"').map(|i| from + i + 1) {
        let close = open + line[open..].find('"')?;
        let (method, target) = request_line(&line[open..close]);
        if !method.is_empty() {
            record.method = method.into();
            record.target = target.into();
            after = Some(close + 1);
            break;
        }
        from = close + 1;
    }
    let after = after?;
    let before = &line[..after];
    record.at = before
        .find('[')
        .and_then(|open| {
            let close = open + before[open..].find(']')?;
            Stamp::parse(&before[open + 1..close])
        })
        .or_else(|| before.split_whitespace().take(4).find_map(Stamp::parse));
    let mut words = line[after..].split_whitespace();
    if let Some(code @ 100..=599) = words.next().and_then(|w| w.parse::<u16>().ok()) {
        record.status = code;
        record.bytes = words.next().and_then(|w| w.parse().ok()).unwrap_or(0);
    }
    let first = line.split_whitespace().next().unwrap_or("");
    let address = first.contains(['.', ':'])
        && first
            .bytes()
            .all(|c| c.is_ascii_hexdigit() || c == b'.' || c == b':');
    if address {
        record.client = first.into();
    }
    // In `combined` and its kin the user agent is the last thing quoted
    let quoted: Vec<&str> = line[after..].split('"').collect();
    if quoted.len() >= 4 {
        let agent = quoted[quoted.len() - 2];
        if agent != "-" {
            record.agent = agent.into();
        }
    }
    (record.at.is_some() || record.status > 0).then_some(record)
}

/// nginx's error log: `2026/10/08 13:55:36 [error] 12#12: *34 message,
/// client: 1.2.3.4, server: _, request: "GET / HTTP/1.1", upstream: "…"`
fn fault(line: &str) -> Option<Fault<'_>> {
    let b = line.as_bytes();
    if b.len() < 23 || b[4] != b'/' || b[7] != b'/' || &b[19..21] != b" [" {
        return None;
    }
    let close = 21 + line[21..].find(']')?;
    let level = &line[21..close];
    if !level.bytes().all(|c| c.is_ascii_lowercase()) {
        return None;
    }
    // `12#12: *34 message`: the process, and the connection when there is one
    let rest = line[close + 1..].trim_start();
    let rest = match rest.split_once(": ") {
        Some((process, after)) if process.contains('#') && !process.contains(' ') => after,
        _ => rest,
    };
    let rest = match rest.strip_prefix('*').and_then(|r| r.split_once(' ')) {
        Some((connection, after)) if digits(connection.as_bytes()).is_some() => after,
        _ => rest,
    };
    let (message, context) = match rest.find(", client: ") {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let said = |key: &str| {
        let start = context.find(key)? + key.len();
        let value = &context[start..];
        Some(match value.strip_prefix('"') {
            Some(quoted) => &quoted[..quoted.find('"').unwrap_or(quoted.len())],
            None => &value[..value.find(", ").unwrap_or(value.len())],
        })
    };
    Some(Fault {
        at: Stamp::parse(&line[..19]),
        level,
        message,
        client: said(", client: ").unwrap_or(""),
        request: said(", request: ").unwrap_or(""),
        upstream: said(", upstream: ").unwrap_or(""),
    })
}

/// A JSON line: nginx with an `escape=json` format, under the names its
/// variables have, or Caddy's access log
fn json(line: &str) -> Option<Record<'static>> {
    let value: serde_json::Value = serde_json::from_str(line).ok()?;
    let request = value.get("request");
    let nested = request.filter(|r| r.is_object());
    let find = |keys: &[&str]| {
        keys.iter()
            .find_map(|key| value.get(key).or_else(|| nested.and_then(|n| n.get(key))))
            .filter(|v| !v.is_null())
    };
    let text = |keys: &[&str]| {
        find(keys).map(|v| match v {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        })
    };
    let number = |keys: &[&str]| {
        find(keys).and_then(|v| match v {
            serde_json::Value::String(s) => seconds(s),
            other => other.as_f64(),
        })
    };
    let mut record = Record {
        at: find(&[
            "time_local",
            "time_iso8601",
            "time",
            "timestamp",
            "@timestamp",
            "msec",
            "ts",
        ])
        .and_then(|v| match v {
            serde_json::Value::String(s) => Stamp::parse(s),
            other => other.as_f64().filter(|n| *n >= 1e9).map(Stamp::epoch),
        }),
        ..Record::default()
    };
    let owned = |s: Option<String>| Cow::Owned(s.filter(|s| s != "-").unwrap_or_default());
    match request.and_then(|r| r.as_str()) {
        Some(request) => {
            let (method, target) = request_line(request);
            record.method = Cow::Owned(method.to_string());
            record.target = Cow::Owned(target.to_string());
        }
        None => {
            record.method = owned(text(&["request_method", "method"]));
            record.target = owned(text(&["request_uri", "uri", "path", "url"]));
        }
    }
    let status = number(&["status"]).map_or(0, |s| s as u16);
    if (100..=599).contains(&status) {
        record.status = status;
    }
    record.bytes =
        number(&["body_bytes_sent", "bytes_sent", "size", "bytes"]).map_or(0, |b| b as u64);
    record.client = owned(text(&["remote_addr", "remote_ip", "client_ip", "client"]));
    record.host = owned(text(&["host", "http_host", "server_name"]));
    record.agent = owned(text(&["http_user_agent", "user_agent"]).or_else(|| {
        let agent = nested?.get("headers")?.get("User-Agent")?;
        Some(agent.get(0).unwrap_or(agent).as_str()?.to_string())
    }));
    record.time = number(&["request_time", "duration"]);
    record.upstream = number(&["upstream_response_time"]);
    (record.status > 0 || !record.target.is_empty()).then_some(record)
}

// ─── Counts ──────────────────────────────────────────────────────────────────

/// The slots traffic is counted in
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grain {
    Minute,
    Hour,
    Day,
}

impl Grain {
    pub const ALL: [Grain; 3] = [Grain::Minute, Grain::Hour, Grain::Day];

    pub fn seconds(self) -> i64 {
        match self {
            Grain::Minute => 60,
            Grain::Hour => 3_600,
            Grain::Day => 86_400,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Grain::Minute => "minute",
            Grain::Hour => "hour",
            Grain::Day => "day",
        }
    }

    /// Slots kept: a day of minutes, ninety days of hours, ten years of days
    fn kept(self) -> usize {
        match self {
            Grain::Minute => 1_440,
            Grain::Hour => 2_160,
            Grain::Day => 3_660,
        }
    }

    /// The slot this one is naturally held against, and what to call it:
    /// the same minute an hour before, the same hour a day before, the
    /// same weekday a week before
    pub fn echo(self) -> (i64, &'static str) {
        match self {
            Grain::Minute => (60, "an hour ago"),
            Grain::Hour => (24, "a day ago"),
            Grain::Day => (7, "a week ago"),
        }
    }

    /// A slot's name: `13:55`, `08 Oct 13:00`, `Thu 08 Oct`
    pub fn label(self, start_local: i64) -> String {
        let c = civil(start_local);
        let month = MONTHS[c.month as usize - 1];
        match self {
            Grain::Minute => format!("{:02} {month} {:02}:{:02}", c.day, c.hour, c.minute),
            Grain::Hour => format!("{:02} {month} {:02}:00", c.day, c.hour),
            Grain::Day => format!(
                "{} {:02} {month} {}",
                WEEKDAYS[c.weekday as usize], c.day, c.year
            ),
        }
    }
}

/// What one slot of time saw
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Slot {
    pub requests: u64,
    pub c4xx: u64,
    pub c5xx: u64,
    pub bytes: u64,
    /// Sum of `$request_time`, in microseconds, over the `timed` requests
    pub time_us: u64,
    pub timed: u64,
    /// Requests in its busiest second
    pub peak: u32,
    /// Error log lines at `error` and above
    pub faults: u64,
}

/// Counts by time, the newest `cap` of them. The one being counted into
/// is held outside the map: a log is written in order, so nearly every
/// line lands where the line before it did, and finds it without a search.
#[derive(Debug, Clone)]
struct Rolling<V> {
    map: BTreeMap<i64, V>,
    open: Option<(i64, V)>,
    cap: usize,
}

impl<V: Default + Copy> Rolling<V> {
    fn new(cap: usize) -> Self {
        Rolling {
            map: BTreeMap::new(),
            open: None,
            cap,
        }
    }

    /// The count at `key`; none when it is older than all those kept
    fn at(&mut self, key: i64) -> Option<&mut V> {
        if self.open.as_ref().is_some_and(|(open, _)| *open == key) {
            return self.open.as_mut().map(|(_, v)| v);
        }
        if let Some((open, v)) = self.open.take() {
            self.map.insert(open, v);
        }
        let v = match self.map.remove(&key) {
            Some(v) => v,
            None => {
                if self.map.len() >= self.cap {
                    if self.map.keys().next().is_some_and(|oldest| key < *oldest) {
                        return None;
                    }
                    self.map.pop_first();
                }
                V::default()
            }
        };
        self.open = Some((key, v));
        self.open.as_mut().map(|(_, v)| v)
    }

    /// The count held out of the map: the one `at` last gave
    fn held(&mut self) -> Option<&mut V> {
        self.open.as_mut().map(|(_, v)| v)
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.map.len() + usize::from(self.open.is_some())
    }

    /// The counts from `from` to `to`, both included, in no order
    fn range(&self, from: i64, to: i64) -> impl Iterator<Item = (i64, &V)> {
        let open = self
            .open
            .as_ref()
            .filter(|(key, _)| (from..=to).contains(key));
        self.map
            .range(from..=to)
            .map(|(key, v)| (*key, v))
            .chain(open.map(|(key, v)| (*key, v)))
    }

    /// Every count, oldest first
    fn all(&self) -> Vec<(i64, V)> {
        let mut all: Vec<(i64, V)> = self.map.iter().map(|(key, v)| (*key, *v)).collect();
        if let Some((key, v)) = self.open {
            all.insert(all.partition_point(|(k, _)| *k < key), (key, v));
        }
        all
    }
}

/// A grain's slots, by their place on the log's own clock
#[derive(Debug, Clone)]
pub struct Slots {
    grain: Grain,
    slots: Rolling<Slot>,
}

impl Slots {
    fn new(grain: Grain) -> Self {
        Slots {
            grain,
            slots: Rolling::new(grain.kept()),
        }
    }

    /// The slot `local` falls in; none when it is older than all those kept
    fn at(&mut self, local: i64) -> Option<&mut Slot> {
        self.slots.at(local.div_euclid(self.grain.seconds()))
    }
}

/// A slot as the screens and reports show it
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Row {
    /// Start of the slot, on the log's clock
    pub start: i64,
    pub slot: Slot,
    /// Requests per second over the part of the slot the log covers
    pub rate: f64,
    /// The slot is still being filled, or the log starts inside it
    pub partial: bool,
}

/// Counts by name, for as many names as `cap`; requests of names that came
/// after are counted in `other`
#[derive(Debug, Clone)]
pub struct Top<V> {
    /// Where each name's count is in `counts`: a name already seen is
    /// hashed once to find it
    places: HashMap<String, u32, Keyed>,
    counts: Vec<V>,
    cap: usize,
    pub other: u64,
}

/// What a name's count needs for two readers' counts to be made one
pub trait Tally: Default {
    fn add(&mut self, other: &Self);
    fn requests(&self) -> u64;
}

impl Tally for u64 {
    fn add(&mut self, other: &Self) {
        *self += other;
    }

    fn requests(&self) -> u64 {
        *self
    }
}

impl<V: Tally> Top<V> {
    fn new(cap: usize) -> Self {
        Top {
            places: HashMap::with_hasher(Keyed::new()),
            counts: Vec::new(),
            cap,
            other: 0,
        }
    }

    fn get(&mut self, name: &str) -> Option<&mut V> {
        if let Some(&place) = self.places.get(name) {
            return self.counts.get_mut(place as usize);
        }
        if self.counts.len() >= self.cap {
            self.other += 1;
            return None;
        }
        self.places
            .insert(name.to_string(), self.counts.len() as u32);
        self.counts.push(V::default());
        self.counts.last_mut()
    }

    /// Add what another reader counted
    fn absorb(&mut self, other: Top<V>) {
        self.other += other.other;
        for (name, place) in other.places {
            let theirs = &other.counts[place as usize];
            if let Some(&mine) = self.places.get(name.as_str()) {
                self.counts[mine as usize].add(theirs);
            } else if self.counts.len() >= self.cap {
                self.other += theirs.requests();
            } else {
                self.places.insert(name, self.counts.len() as u32);
                let mut count = V::default();
                count.add(theirs);
                self.counts.push(count);
            }
        }
    }

    pub fn len(&self) -> usize {
        self.counts.len()
    }

    /// The `n` names with the most of `by`, most first
    pub fn top(&self, n: usize, by: impl Fn(&V) -> u64) -> Vec<(&str, &V)> {
        let mut all: Vec<(&str, &V)> = self
            .places
            .iter()
            .map(|(name, place)| (name.as_str(), &self.counts[*place as usize]))
            .filter(|(_, v)| by(v) > 0)
            .collect();
        let order = |a: &(&str, &V), b: &(&str, &V)| by(b.1).cmp(&by(a.1)).then(a.0.cmp(b.0));
        if all.len() > n && n > 0 {
            all.select_nth_unstable_by(n - 1, order);
            all.truncate(n);
        }
        all.sort_unstable_by(order);
        all.truncate(n);
        all
    }
}

/// The hash the names are found by: eight bytes at a time through a
/// multiply folded onto itself, under a key drawn when pepe starts. The
/// names in a log are chosen by whoever sends the requests, so the key is
/// what keeps them from being chosen to collide; SipHash, which the
/// standard map would use, costs several times as much for a user agent.
#[derive(Debug, Clone, Copy)]
struct Keyed {
    seed: u64,
    key: u64,
}

impl Keyed {
    fn new() -> Keyed {
        use std::hash::BuildHasher;
        static KEYS: OnceLock<(u64, u64)> = OnceLock::new();
        let (seed, key) = *KEYS.get_or_init(|| {
            let random = std::collections::hash_map::RandomState::new();
            (random.hash_one(1u8), random.hash_one(2u8) | 1)
        });
        Keyed { seed, key }
    }
}

impl std::hash::BuildHasher for Keyed {
    type Hasher = Folded;

    fn build_hasher(&self) -> Folded {
        Folded {
            hash: self.seed,
            key: self.key,
        }
    }
}

struct Folded {
    hash: u64,
    key: u64,
}

fn fold_multiply(a: u64, b: u64) -> u64 {
    let wide = u128::from(a).wrapping_mul(u128::from(b));
    (wide as u64) ^ ((wide >> 64) as u64)
}

impl std::hash::Hasher for Folded {
    fn write(&mut self, bytes: &[u8]) {
        let mut hash = self.hash ^ (bytes.len() as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        let mut words = bytes.chunks_exact(8);
        for word in &mut words {
            let word = u64::from_le_bytes(word.try_into().expect("eight bytes"));
            hash = fold_multiply(hash ^ word, self.key);
        }
        let rest = words.remainder();
        if !rest.is_empty() {
            let mut word = [0; 8];
            word[..rest.len()].copy_from_slice(rest);
            hash = fold_multiply(hash ^ u64::from_le_bytes(word), self.key);
        }
        self.hash = hash;
    }

    // What `str` adds after its bytes, which their length has already said
    fn write_u8(&mut self, _: u8) {}

    fn finish(&self) -> u64 {
        fold_multiply(self.hash, self.key ^ 0xD6E8_FEB8_6659_FD93)
    }
}

/// What a path saw
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct PathStat {
    pub requests: u64,
    pub c4xx: u64,
    pub c5xx: u64,
    pub time_us: u64,
    pub timed: u64,
}

impl Tally for PathStat {
    fn add(&mut self, other: &Self) {
        self.requests += other.requests;
        self.c4xx += other.c4xx;
        self.c5xx += other.c5xx;
        self.time_us += other.time_us;
        self.timed += other.timed;
    }

    fn requests(&self) -> u64 {
        self.requests
    }
}

impl PathStat {
    pub fn mean_time(&self) -> Option<Duration> {
        (self.timed > 0).then(|| Duration::from_micros(self.time_us / self.timed))
    }
}

/// Error log lines that say the same thing
#[derive(Debug, Clone, PartialEq)]
pub struct Cause {
    pub level: String,
    pub count: u64,
    pub last: Option<Stamp>,
    /// The first line that said it, as written
    pub example: String,
    /// Where that line was in what was read
    order: u64,
}

/// A line kept for the log view
#[derive(Debug, Clone, PartialEq)]
pub struct Recent {
    pub text: String,
    pub kind: Kind,
    /// When it was written; a line that doesn't say has the time of the
    /// line read before it
    pub at: i64,
    /// Where it was in what was read: lines of one second stay in order
    order: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Request { status: u16 },
    Fault { severity: u8 },
    Unread,
}

impl Kind {
    /// A response that failed, or an error log line that isn't just talk
    pub fn is_error(self) -> bool {
        match self {
            Kind::Request { status } => status >= 400,
            Kind::Fault { severity } => severity >= 1,
            Kind::Unread => false,
        }
    }
}

/// Which second is "now", and whether the log is being written
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    /// The clock's second when the log is live; the log's last otherwise
    pub now: i64,
    pub live: bool,
    /// The offset the log's times are shown in
    pub offset: i32,
}

/// The rate over the seconds before now
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Now {
    pub requests: u64,
    pub rate: f64,
    pub c4xx: u64,
    pub c5xx: u64,
}

impl Now {
    pub fn share_5xx(&self) -> f64 {
        self.c5xx as f64 / self.requests.max(1) as f64
    }

    pub fn share_4xx(&self) -> f64 {
        self.c4xx as f64 / self.requests.max(1) as f64
    }
}

/// Everything counted from the lines read so far
#[derive(Debug)]
pub struct Stats {
    pub lines: u64,
    pub requests: u64,
    pub bytes: u64,
    /// Lines nothing could be read from, and the first of them
    pub unread: u64,
    pub first_unread: Option<String>,
    first_unread_order: u64,
    /// Where the next line is in what is read: its number, or, while a
    /// file is read by several threads, a place that keeps their lines in
    /// the file's order
    order: u64,
    /// The time of the last line that had one
    carry: i64,
    /// The second of the last request, while the counts held out of
    /// their maps are that second's, its minute's, its hour's and its day's
    hot: Option<Stamp>,
    /// Where a path with ids in it is written without them
    scratch: String,
    /// Lines are kept for the log view. Off while a thread reads a part
    /// of a file too far from its end to have any of its last lines.
    keeping: bool,
    /// The requests of one second that came one after the other at the
    /// start of a stretch of the file, and those at its end so far: where
    /// a second is cut in two by the threads reading it, the halves are
    /// put together again (see `merge`)
    head: Option<(Stamp, u32)>,
    run: Option<(Stamp, u32)>,
    /// The runs at the ends of the stretches read
    edges: Vec<(Stamp, u32)>,
    /// What the edges of each second add up to
    stitched: HashMap<i64, u32>,
    /// Requests whose line had no time
    pub undated: u64,
    /// Lines older than `--since`
    pub skipped: u64,
    /// The second nothing older than is counted
    pub since: Option<i64>,
    pub first: Option<Stamp>,
    pub last: Option<Stamp>,
    /// A line as recent as the clock was read: the log is being written
    pub live: bool,
    pub statuses: BTreeMap<u16, u64>,
    pub methods: Top<u64>,
    pub paths: Top<PathStat>,
    pub clients: Top<u64>,
    pub agents: Top<u64>,
    /// Names of query parameters
    pub params: Top<u64>,
    /// `$request_time` and `$upstream_response_time`, in microseconds
    pub time: Histogram,
    pub upstream: Histogram,
    /// (requests, 4xx, 5xx) of each of the last seconds
    seconds: Rolling<[u32; 3]>,
    /// The busiest second, and its requests
    pub peak: Option<(Stamp, u32)>,
    slots: [Slots; 3],
    /// Error log lines, by level and by what they say
    pub faults: u64,
    pub levels: BTreeMap<String, u64>,
    pub causes: HashMap<String, Cause>,
    pub other_causes: u64,
    pub recent: VecDeque<Recent>,
    /// Lines pushed out of `recent` so far: a line's number is this plus
    /// its place there
    pub recent_base: u64,
    /// How far the reading is
    pub read_bytes: u64,
    pub total_bytes: u64,
    /// Every file has been read to its end once
    pub caught_up: bool,
    /// Count `/items/1` and `/items/2` apart
    pub exact_paths: bool,
    /// Why a file couldn't be read
    pub trouble: Option<String>,
}

impl Default for Stats {
    fn default() -> Self {
        Stats {
            lines: 0,
            requests: 0,
            bytes: 0,
            unread: 0,
            first_unread: None,
            first_unread_order: 0,
            order: 0,
            carry: 0,
            hot: None,
            scratch: String::new(),
            keeping: true,
            head: None,
            run: None,
            edges: Vec::new(),
            stitched: HashMap::new(),
            undated: 0,
            skipped: 0,
            since: None,
            first: None,
            last: None,
            live: false,
            statuses: BTreeMap::new(),
            methods: Top::new(64),
            paths: Top::new(PATHS),
            clients: Top::new(CLIENTS),
            agents: Top::new(AGENTS),
            params: Top::new(PARAMS),
            time: Histogram::default(),
            upstream: Histogram::default(),
            seconds: Rolling::new(SECONDS_KEPT),
            peak: None,
            slots: Grain::ALL.map(Slots::new),
            faults: 0,
            levels: BTreeMap::new(),
            causes: HashMap::new(),
            other_causes: 0,
            recent: VecDeque::new(),
            recent_base: 0,
            read_bytes: 0,
            total_bytes: 0,
            caught_up: false,
            exact_paths: false,
            trouble: None,
        }
    }
}

/// An error message without what changes from one time to the next: what
/// is quoted becomes `…` and numbers `N`, except the errno after a `(`
fn cause_of(message: &str) -> String {
    let mut out = String::with_capacity(message.len());
    let mut chars = message.chars().peekable();
    let mut last = ' ';
    while let Some(c) = chars.next() {
        if c == '"' {
            out.push_str("\"…\"");
            for c in chars.by_ref() {
                if c == '"' {
                    break;
                }
            }
            last = '"';
        } else if c.is_ascii_digit() && !last.is_ascii_alphabetic() {
            let mut number = String::from(c);
            while let Some(c) = chars.next_if(|c| c.is_ascii_digit() || *c == '.') {
                number.push(c);
            }
            if last == '(' {
                out.push_str(&number);
            } else {
                out.push('N');
            }
            last = 'N';
        } else {
            out.push(c);
            last = c;
        }
    }
    out
}

/// A path with what names one thing among many made `*`: numbers, UUIDs
/// and long hex ids. `/items/42/photos` and `/items/43/photos` are one path.
#[cfg(test)]
pub fn grouped(path: &str) -> Cow<'_, str> {
    let mut scratch = String::new();
    match grouped_into(path, &mut scratch) {
        made if made.len() == path.len() && made == path => Cow::Borrowed(path),
        made => Cow::Owned(made.to_string()),
    }
}

/// `grouped`, written into `scratch` when the path has an id in it: a
/// line's path is looked up and let go of, and needs no memory of its own
fn grouped_into<'a>(path: &'a str, scratch: &'a mut String) -> &'a str {
    // An id has a digit in it, and most paths have none
    if !path.bytes().any(|c| c.is_ascii_digit()) {
        return path;
    }
    let is_id = |part: &str| {
        let hex = part.len() >= 16 && part.bytes().all(|c| c.is_ascii_hexdigit() || c == b'-');
        !part.is_empty() && (hex || part.bytes().all(|c| c.is_ascii_digit()))
    };
    scratch.clear();
    let mut any = false;
    let mut rest = path;
    loop {
        let (part, after) = match split_at_byte(rest, b'/') {
            Some((part, after)) => (part, Some(after)),
            None => (rest, None),
        };
        if is_id(part) {
            scratch.push('*');
            any = true;
        } else {
            scratch.push_str(part);
        }
        match after {
            Some(after) => {
                scratch.push('/');
                rest = after;
            }
            None => break,
        }
    }
    if any {
        scratch
    } else {
        path
    }
}

/// Where a line kept for the log view ends
fn cut_at(line: &str) -> usize {
    let mut end = line.len().min(LINE_KEPT);
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    end
}

fn cut(line: &str) -> String {
    match cut_at(line) {
        end if end == line.len() => line.to_string(),
        end => format!("{}…", &line[..end]),
    }
}

/// The line without the colour codes `docker compose logs` writes into a
/// pipe, and without the control characters that would move the cursor
/// when the line is drawn: a tab is a space, the rest are left out
pub fn clean(text: &str) -> Cow<'_, str> {
    if !text.bytes().any(|b| b < 0x20 || b == 0x7f) {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => match chars.next() {
                // ESC [ parameters, then one final byte: a colour, most often
                Some('[') => {
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                }
                // ESC ] text, to a BEL or an ESC \: a title, a link
                Some(']') => {
                    while let Some(n) = chars.next() {
                        if n == '\x07' || (n == '\x1b' && chars.next().is_some()) {
                            break;
                        }
                    }
                }
                _ => {}
            },
            '\t' => out.push(' '),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    Cow::Owned(out)
}

impl Stats {
    /// Count one line. `wall` is the clock's second, for telling a log
    /// that is being written from one that was.
    pub fn fold(&mut self, line: Line<'_>, raw: &str, wall: i64) {
        self.lines += 1;
        self.order += 1;
        let at = match &line {
            Line::Request(r) => r.at,
            Line::Fault(f) => f.at,
            Line::Unread => None,
        };
        let kind = match line {
            Line::Request(r) => {
                self.request(&r, wall);
                Kind::Request { status: r.status }
            }
            Line::Fault(f) => {
                self.fault(&f, raw, wall);
                Kind::Fault {
                    severity: severity(f.level),
                }
            }
            Line::Unread => {
                self.unread += 1;
                if self.first_unread.is_none() {
                    self.first_unread = Some(cut(raw));
                    self.first_unread_order = self.order;
                }
                Kind::Unread
            }
        };
        if let Some(at) = at {
            self.carry = at.at;
        }
        if self.keeping {
            self.keep(raw, kind, self.carry);
        } else {
            self.recent_base += 1;
        }
    }

    /// Keep a line for the log view, in its place by time: an error log
    /// read after the access log has its lines among the requests they
    /// were written between
    fn keep(&mut self, raw: &str, kind: Kind, at: i64) {
        let full = self.recent.len() == RECENT;
        let in_order = self.recent.back().is_none_or(|last| last.at <= at);
        let place = if in_order {
            self.recent.len()
        } else {
            self.recent.partition_point(|kept| kept.at <= at)
        };
        if full {
            self.recent_base += 1;
            // Older than everything kept: it would be the first to go
            if place == 0 {
                return;
            }
        }
        // The line that goes makes room for the one that comes: while a
        // long log is read, nothing is allocated for the lines passing by
        let mut text = match full.then(|| self.recent.pop_front()).flatten() {
            Some(gone) => gone.text,
            None => String::new(),
        };
        text.clear();
        let cut = cut_at(raw);
        text.push_str(&raw[..cut]);
        if cut < raw.len() {
            text.push('…');
        }
        let order = self.order;
        self.recent.insert(
            place - usize::from(full),
            Recent {
                text,
                kind,
                at,
                order,
            },
        );
    }

    fn seen(&mut self, at: Stamp, wall: i64) {
        if self.first.is_none_or(|first| at.at < first.at) {
            self.first = Some(at);
        }
        if self.last.is_none_or(|last| at.at >= last.at) {
            self.last = Some(at);
        }
        self.live |= at.at >= wall - LIVE;
    }

    fn request(&mut self, r: &Record, wall: i64) {
        self.requests += 1;
        self.bytes += r.bytes;
        *self.statuses.entry(r.status).or_default() += 1;
        let (c4xx, c5xx) = (
            u64::from((400..500).contains(&r.status)),
            u64::from(r.status >= 500),
        );
        let time_us = r.time.map(|t| (t * 1e6) as u64);
        let (path, query) = split_at_byte(&r.target, b'?').unwrap_or((&r.target, ""));
        let mut scratch = std::mem::take(&mut self.scratch);
        let path = if self.exact_paths {
            path
        } else {
            grouped_into(path, &mut scratch)
        };
        if let Some(stat) = self.paths.get(path) {
            stat.requests += 1;
            stat.c4xx += c4xx;
            stat.c5xx += c5xx;
            if let Some(us) = time_us {
                stat.time_us += us;
                stat.timed += 1;
            }
        }
        self.scratch = scratch;
        let mut query = query;
        while !query.is_empty() {
            let (pair, rest) = split_at_byte(query, b'&').unwrap_or((query, ""));
            let name = split_at_byte(pair, b'=').map_or(pair, |(name, _)| name);
            if !name.is_empty() && name.len() <= 64 {
                if let Some(count) = self.params.get(name) {
                    *count += 1;
                }
            }
            query = rest;
        }
        for (top, name) in [
            (&mut self.methods, &r.method),
            (&mut self.clients, &r.client),
            (&mut self.agents, &r.agent),
        ] {
            if !name.is_empty() {
                if let Some(count) = top.get(name) {
                    *count += 1;
                }
            }
        }
        if let Some(us) = time_us {
            self.time.record(us);
        }
        if let Some(upstream) = r.upstream {
            self.upstream.record((upstream * 1e6) as u64);
        }
        let Some(at) = r.at else {
            self.undated += 1;
            return;
        };
        self.seen(at, wall);
        match &mut self.run {
            Some((second, n)) if second.at == at.at => *n += 1,
            run => {
                if self.head.is_none() {
                    self.head = *run;
                }
                *run = Some((at, 1));
            }
        }

        // This second's count, which is also how a slot knows its busiest.
        // A request in the second the one before it was in counts where
        // that one did, with nothing to look up.
        let hot = self.hot == Some(at);
        let mut found = true;
        let mut this_second = 0;
        let second = match hot {
            true => self.seconds.held(),
            false => self.seconds.at(at.at),
        };
        if let Some(second) = second {
            second[0] += 1;
            second[1] += c4xx as u32;
            second[2] += c5xx as u32;
            this_second = second[0];
            if self.peak.is_none_or(|(_, most)| this_second > most) {
                self.peak = Some((at, this_second));
            }
        } else {
            found = false;
        }
        for slots in &mut self.slots {
            let slot = match hot {
                true => slots.slots.held(),
                false => slots.at(at.local()),
            };
            let Some(slot) = slot else {
                found = false;
                continue;
            };
            slot.requests += 1;
            slot.c4xx += c4xx;
            slot.c5xx += c5xx;
            slot.bytes += r.bytes;
            if let Some(us) = time_us {
                slot.time_us += us;
                slot.timed += 1;
            }
            slot.peak = slot.peak.max(this_second);
        }
        self.hot = found.then_some(at);
    }

    fn fault(&mut self, f: &Fault, raw: &str, wall: i64) {
        self.hot = None;
        self.faults += 1;
        match self.levels.get_mut(f.level) {
            Some(count) => *count += 1,
            None => {
                self.levels.insert(f.level.to_string(), 1);
            }
        }
        let key = format!("{} {}", f.level, cause_of(f.message));
        let room = self.causes.len() < CAUSES;
        match self.causes.get_mut(&key) {
            Some(cause) => {
                cause.count += 1;
                if f.at.is_some() {
                    cause.last = f.at;
                }
            }
            None if room => {
                self.causes.insert(
                    key,
                    Cause {
                        level: f.level.to_string(),
                        count: 1,
                        last: f.at,
                        example: cut(raw),
                        order: self.order,
                    },
                );
            }
            None => self.other_causes += 1,
        }
        if let Some(at) = f.at {
            self.seen(at, wall);
            if severity(f.level) >= 2 {
                for slots in &mut self.slots {
                    if let Some(slot) = slots.at(at.local()) {
                        slot.faults += 1;
                    }
                }
            }
        }
    }

    /// A stretch of the file ends here: its first and last seconds may go
    /// on in the stretches beside it
    fn end_stretch(&mut self) {
        self.edges.extend(self.head.take());
        self.edges.extend(self.run.take());
    }

    /// A second had this many requests: its slots and the log may not
    /// have seen one busier
    fn busy(&mut self, at: Stamp, requests: u32) {
        self.hot = None;
        let busier = self
            .peak
            .is_none_or(|(when, most)| requests > most || (requests == most && at.at < when.at));
        if busier {
            self.peak = Some((at, requests));
        }
        for slots in &mut self.slots {
            if let Some(slot) = slots.at(at.local()) {
                slot.peak = slot.peak.max(requests);
            }
        }
    }

    /// Add what another reader counted from another part of the same logs.
    /// The sum is what one reader would have counted from all of it, in
    /// whatever order the parts are added.
    pub fn merge(&mut self, other: Stats) {
        self.hot = None;
        self.lines += other.lines;
        self.requests += other.requests;
        self.bytes += other.bytes;
        self.unread += other.unread;
        self.undated += other.undated;
        self.skipped += other.skipped;
        self.faults += other.faults;
        self.other_causes += other.other_causes;
        self.read_bytes += other.read_bytes;
        self.live |= other.live;
        self.order = self.order.max(other.order);
        self.carry = self.carry.max(other.carry);
        if other.first_unread.is_some()
            && (self.first_unread.is_none() || other.first_unread_order < self.first_unread_order)
        {
            self.first_unread = other.first_unread;
            self.first_unread_order = other.first_unread_order;
        }
        if let Some(first) = other.first {
            if self.first.is_none_or(|mine| first.at < mine.at) {
                self.first = Some(first);
            }
        }
        if let Some(last) = other.last {
            if self.last.is_none_or(|mine| last.at >= mine.at) {
                self.last = Some(last);
            }
        }
        for (code, n) in other.statuses {
            *self.statuses.entry(code).or_default() += n;
        }
        self.methods.absorb(other.methods);
        self.paths.absorb(other.paths);
        self.clients.absorb(other.clients);
        self.agents.absorb(other.agents);
        self.params.absorb(other.params);
        self.time.absorb(&other.time);
        self.upstream.absorb(&other.upstream);
        // A second both readers had lines of is as busy as the two together
        let offset = other.last.map_or(0, |last| last.offset);
        for (key, theirs) in other.seconds.all() {
            let Some(mine) = self.seconds.at(key) else {
                continue;
            };
            let shared = mine[0] > 0;
            for (mine, theirs) in mine.iter_mut().zip(theirs) {
                *mine += theirs;
            }
            let whole = mine[0];
            if shared {
                self.busy(Stamp { at: key, offset }, whole);
            }
        }
        for (mine, theirs) in self.slots.iter_mut().zip(&other.slots) {
            for (key, slot) in theirs.slots.all() {
                if let Some(mine) = mine.slots.at(key) {
                    mine.requests += slot.requests;
                    mine.c4xx += slot.c4xx;
                    mine.c5xx += slot.c5xx;
                    mine.bytes += slot.bytes;
                    mine.time_us += slot.time_us;
                    mine.timed += slot.timed;
                    mine.faults += slot.faults;
                    mine.peak = mine.peak.max(slot.peak);
                }
            }
        }
        if let Some((at, most)) = other.peak {
            self.busy(at, most);
        }
        // A second cut in two by the readers: each has its half as a run at
        // an edge, and the halves together are what the second saw
        for (at, requests) in other.edges {
            let whole = self.stitched.entry(at.at).or_default();
            *whole += requests;
            let whole = *whole;
            self.busy(at, whole);
        }
        for (level, n) in other.levels {
            *self.levels.entry(level).or_default() += n;
        }
        for (what, theirs) in other.causes {
            let room = self.causes.len() < CAUSES;
            match self.causes.get_mut(&what) {
                Some(mine) => {
                    mine.count += theirs.count;
                    if theirs.last.map(|at| at.at) > mine.last.map(|at| at.at) {
                        mine.last = theirs.last;
                    }
                    if theirs.order < mine.order {
                        mine.example = theirs.example;
                        mine.order = theirs.order;
                    }
                }
                None if room => {
                    self.causes.insert(what, theirs);
                }
                None => self.other_causes += theirs.count,
            }
        }
        // The last lines of both, by time and then by place in the file
        self.recent_base += other.recent_base;
        if !other.recent.is_empty() {
            let mut lines: Vec<Recent> = self.recent.drain(..).chain(other.recent).collect();
            lines.sort_by_key(|line| (line.at, line.order));
            let over = lines.len().saturating_sub(RECENT);
            self.recent_base += over as u64;
            self.recent.extend(lines.into_iter().skip(over));
        }
    }

    /// Which second the rates are held against
    pub fn clock(&self, wall: i64) -> Clock {
        let offset = self.last.map_or_else(local_offset, |last| last.offset);
        match self.last {
            Some(last) if !self.live => Clock {
                now: last.at,
                live: false,
                offset,
            },
            _ => Clock {
                now: wall,
                live: self.live,
                offset,
            },
        }
    }

    /// The `window` seconds up to now
    pub fn now(&self, clock: Clock, window: i64) -> Now {
        let mut now = Now::default();
        for (_, second) in self.seconds.range(clock.now - window + 1, clock.now) {
            now.requests += u64::from(second[0]);
            now.c4xx += u64::from(second[1]);
            now.c5xx += u64::from(second[2]);
        }
        now.rate = now.requests as f64 / window as f64;
        now
    }

    /// Requests in each of the `n` seconds up to now, oldest first
    pub fn last_seconds(&self, clock: Clock, n: usize) -> Vec<u32> {
        let from = clock.now - n as i64 + 1;
        let mut out = vec![0; n];
        for (at, second) in self.seconds.range(from, clock.now) {
            out[(at - from) as usize] = second[0];
        }
        out
    }

    /// A grain's slots, oldest first, each with its rate over the part of
    /// it the log covers
    pub fn rows(&self, grain: Grain, clock: Clock) -> Vec<Row> {
        let size = grain.seconds();
        let first = self.first.map_or(i64::MIN, Stamp::local);
        // Now on the log's clock; the second in progress counts
        let now = clock.now + i64::from(clock.offset) + 1;
        self.slots[grain as usize]
            .slots
            .all()
            .iter()
            .map(|(key, slot)| {
                let start = key * size;
                let from = start.max(first);
                let to = (start + size).min(now.max(from + 1));
                Row {
                    start,
                    slot: *slot,
                    rate: slot.requests as f64 / (to - from).max(1) as f64,
                    partial: to - from < size,
                }
            })
            .collect()
    }

    /// Error log causes, most frequent first
    pub fn top_causes(&self, n: usize) -> Vec<(&str, &Cause)> {
        let mut causes: Vec<(&str, &Cause)> =
            self.causes.iter().map(|(k, c)| (k.as_str(), c)).collect();
        causes.sort_unstable_by(|a, b| {
            severity(&b.1.level)
                .min(2)
                .cmp(&severity(&a.1.level).min(2))
                .then(b.1.count.cmp(&a.1.count))
                .then(a.0.cmp(b.0))
        });
        causes.truncate(n);
        causes
    }
}

/// The median rate of the slots that are whole: what a slot usually sees
pub fn typical(rows: &[Row]) -> Option<f64> {
    let mut rates: Vec<f64> = rows.iter().filter(|r| !r.partial).map(|r| r.rate).collect();
    if rates.is_empty() {
        return None;
    }
    rates.sort_unstable_by(f64::total_cmp);
    Some(rates[rates.len() / 2])
}

/// The whole slot with the highest rate
pub fn busiest(rows: &[Row]) -> Option<&Row> {
    rows.iter()
        .filter(|r| !r.partial)
        .max_by(|a, b| a.rate.total_cmp(&b.rate))
}

/// The slot `back` slots before the newest
pub fn echo(rows: &[Row], grain: Grain) -> Option<&Row> {
    let then = rows.last()?.start - grain.echo().0 * grain.seconds();
    rows.iter().find(|r| r.start == then)
}

/// Now against then: `+12%`, `-30%`, `×3.4`, `÷2.5`; `new` when there was
/// nothing then
pub fn versus(now: f64, then: f64) -> String {
    if then <= 0.0 && now <= 0.0 {
        return "=".into();
    }
    if then <= 0.0 {
        return "new".into();
    }
    if now <= 0.0 {
        return "-100%".into();
    }
    let ratio = now / then;
    if ratio >= 2.0 {
        format!("×{ratio:.1}")
    } else if ratio <= 0.5 {
        format!("÷{:.1}", 1.0 / ratio)
    } else if (ratio - 1.0).abs() < 0.005 {
        "=".into()
    } else {
        format!("{:+.0}%", (ratio - 1.0) * 100.0)
    }
}

/// `1 line`, `2,000 lines`
pub fn counted(n: u64, what: &str) -> String {
    format!(
        "{} {what}{}",
        format::count(n),
        if n == 1 { "" } else { "s" }
    )
}

/// A rate with the precision its size wants: 0.03, 4.2, 950, 12.3k
pub fn rate(per_second: f64) -> String {
    if per_second > 0.0 && per_second < 0.1 {
        format!("{per_second:.2}")
    } else {
        format::compact(per_second)
    }
}

pub fn percent(share: f64) -> String {
    match share * 100.0 {
        p if p <= 0.0 => "0%".into(),
        p if p < 0.1 => "<0.1%".into(),
        p if p < 10.0 => format!("{p:.1}%"),
        p => format!("{p:.0}%"),
    }
}

// ─── Reading ─────────────────────────────────────────────────────────────────

/// What the reader and the screen share
#[derive(Debug, Default)]
pub struct Shared {
    pub stats: Mutex<Stats>,
    stop: AtomicBool,
}

impl Shared {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn lock(&self) -> std::sync::MutexGuard<'_, Stats> {
        self.stats.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// What to read, and how
#[derive(Debug, Clone)]
pub struct Job {
    /// Files; none reads stdin
    pub files: Vec<PathBuf>,
    /// What was piped in, when stdin itself has been given to the terminal
    /// for the screen's keys
    pub piped: Option<Arc<std::fs::File>>,
    pub parser: Parser,
    /// Leave out lines older than this second
    pub since: Option<i64>,
    /// Keep reading what is appended, and reopen files that are rotated
    pub follow: bool,
    /// Count `/items/1` and `/items/2` apart
    pub exact_paths: bool,
}

impl Job {
    /// What the title shows: `access.log`, `access.log +2`, `stdin`
    pub fn name(&self) -> String {
        let name = |path: &PathBuf| {
            path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into(),
            )
        };
        match self.files.as_slice() {
            [] => "stdin".into(),
            [one] => name(one),
            [first, rest @ ..] => format!("{} +{}", name(first), rest.len()),
        }
    }
}

/// The time of a file's first line that has one: rotated logs are read
/// oldest first, so that every second is counted whole
fn first_stamp(path: &Path, parser: &Parser) -> Option<i64> {
    let file = std::fs::File::open(path).ok()?;
    let mut lines = BufReader::new(file).split(b'\n');
    for _ in 0..50 {
        let line = lines.next()?.ok()?;
        let at = match parser.read(&String::from_utf8_lossy(&line)) {
            Line::Request(r) => r.at,
            Line::Fault(f) => f.at,
            Line::Unread => None,
        };
        if let Some(at) = at {
            return Some(at.at);
        }
    }
    None
}

/// Count the lines of `chunk`, which ends where a line does
fn fold_lines(chunk: &[u8], job: &Job, stats: &mut Stats, wall: i64) {
    // Nearly every chunk is UTF-8 as it stands, and is found to be all at
    // once; one that isn't is looked at by line
    let valid = std::str::from_utf8(chunk).is_ok();
    let mut from = 0;
    while from < chunk.len() {
        let end = memchr::memchr(b'\n', &chunk[from..]).map_or(chunk.len(), |i| from + i);
        let line = &chunk[from..end];
        from = end + 1;
        let text = if valid {
            // SAFETY: the chunk is UTF-8, and a line of it, cut at the
            // ASCII newlines, is too
            Cow::Borrowed(unsafe { std::str::from_utf8_unchecked(line) })
        } else {
            String::from_utf8_lossy(line)
        };
        let text = clean(text.trim());
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        let line = job.parser.read(text);
        let at = match &line {
            Line::Request(r) => r.at,
            Line::Fault(f) => f.at,
            Line::Unread => None,
        };
        if let (Some(since), Some(at)) = (job.since, at) {
            if at.at < since {
                stats.skipped += 1;
                continue;
            }
        }
        stats.fold(line, text, wall);
    }
}

#[cfg(unix)]
fn read_at(file: &std::fs::File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::unix::fs::FileExt::read_at(file, buf, offset)
}

#[cfg(windows)]
fn read_at(file: &std::fs::File, buf: &mut [u8], offset: u64) -> std::io::Result<usize> {
    std::os::windows::fs::FileExt::seek_read(file, buf, offset)
}

/// Fill `buf` from `offset` on, as far as the file goes; how far that was
fn read_full_at(file: &std::fs::File, buf: &mut [u8], offset: u64) -> usize {
    let mut filled = 0;
    while filled < buf.len() {
        match read_at(file, &mut buf[filled..], offset + filled as u64) {
            Ok(0) => break,
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(_) => break,
        }
    }
    filled
}

/// Read what a file has now on every core: each thread takes the next
/// stretch of it, counts the lines that start there, and adds its counts
/// to the shared ones. A line belongs to the stretch it starts in, so no
/// line is counted twice or missed, and since adding counts gives the
/// same sum in any order, the result is what one thread reading from the
/// top would have had. `origin` is the start of the line to begin at.
/// Returns where the reading got to, which is the start of a line; the rest, and what is appended, is the tail's.
fn sprint(path: &Path, job: &Job, shared: &Shared, origin: u64, chunk: u64, threads: usize) -> u64 {
    let Ok(file) = std::fs::File::open(path) else {
        return origin;
    };
    let len = file.metadata().map_or(0, |meta| meta.len());
    let chunks = len.saturating_sub(origin) / chunk;
    if chunks < 2 || threads < 2 {
        return origin;
    }
    let base = shared.lock().order;
    let next = std::sync::atomic::AtomicU64::new(0);
    let reached = std::sync::atomic::AtomicU64::new(0);
    let wall = wall();
    let fresh = || Stats {
        exact_paths: job.exact_paths,
        ..Stats::default()
    };
    std::thread::scope(|scope| {
        for _ in 0..threads.min(chunks as usize) {
            scope.spawn(|| {
                let mut local = fresh();
                let mut buf: Vec<u8> = Vec::new();
                let mut merged = std::time::Instant::now();
                while !shared.stop.load(Ordering::Relaxed) {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= chunks {
                        break;
                    }
                    // One byte before the stretch says whether a line
                    // starts at its first byte
                    let from = origin + (i * chunk).saturating_sub(1);
                    buf.resize((origin + (i + 1) * chunk - from) as usize, 0);
                    let filled = read_full_at(&file, &mut buf, from);
                    buf.truncate(filled);
                    let start = match i {
                        0 => 0,
                        _ => match memchr::memchr(b'\n', &buf) {
                            Some(newline) => newline + 1,
                            None => buf.len(),
                        },
                    };
                    // The last line that starts here may end further on
                    let mut end = buf.len();
                    while start < buf.len() && buf.last() != Some(&b'\n') {
                        let had = buf.len();
                        buf.resize(had + 64 * 1024, 0);
                        let more = read_full_at(&file, &mut buf[had..], from + had as u64);
                        buf.truncate(had + more);
                        if let Some(newline) = memchr::memchr(b'\n', &buf[had..]) {
                            end = had + newline + 1;
                            break;
                        }
                        if more == 0 {
                            // The file ends inside a line still being written
                            end = memchr::memrchr(b'\n', &buf)
                                .map_or(start, |i| i + 1)
                                .max(start);
                            break;
                        }
                        end = buf.len();
                    }
                    if start < end {
                        local.order = base + from + start as u64;
                        local.keeping = origin + (i + 1) * chunk + SPRINT_KEEP >= len;
                        fold_lines(&buf[start..end], job, &mut local, wall);
                        local.end_stretch();
                        local.read_bytes += (end - start) as u64;
                    }
                    reached.fetch_max(from + end.max(start) as u64, Ordering::Relaxed);
                    if merged.elapsed() >= SPRINT_MERGE {
                        shared.lock().merge(std::mem::replace(&mut local, fresh()));
                        merged = std::time::Instant::now();
                    }
                }
                shared.lock().merge(local);
            });
        }
    });
    let mut stats = shared.lock();
    stats.stitched = HashMap::new();
    reached.load(Ordering::Relaxed).max(origin)
}

/// The time of the first line that has one among those starting in the
/// 64 KB from `offset` on
fn stamp_after(file: &std::fs::File, offset: u64, parser: &Parser) -> Option<i64> {
    let mut buf = vec![0; 64 * 1024];
    let filled = read_full_at(file, &mut buf, offset.saturating_sub(1));
    buf.truncate(filled);
    // Lines that start here: after the first newline, unless at the top
    let start = match offset {
        0 => 0,
        _ => memchr::memchr(b'\n', &buf)? + 1,
    };
    buf[start..].split(|&c| c == b'\n').find_map(|line| {
        match parser.read(&String::from_utf8_lossy(line)) {
            Line::Request(r) => r.at,
            Line::Fault(f) => f.at,
            Line::Unread => None,
        }
        .map(|at| at.at)
    })
}

/// Where in a file to start for lines from `since` on: the start of a
/// line with every such line after it, found by halving, since a log is
/// written in order of time. A day of a year's log is read without the
/// rest of the year being looked at. What isn't known is settled toward
/// the top of the file: more is read then, never less.
fn seek_since(path: &Path, parser: &Parser, since: i64) -> u64 {
    let Ok(file) = std::fs::File::open(path) else {
        return 0;
    };
    let len = file.metadata().map_or(0, |meta| meta.len());
    let (mut low, mut high) = (0u64, len);
    while high - low > 64 * 1024 {
        let middle = low + (high - low) / 2;
        match stamp_after(&file, middle, parser) {
            Some(at) if at < since => low = middle,
            _ => high = middle,
        }
    }
    if low == 0 {
        return 0;
    }
    // The line that starts at or after `low`
    let mut buf = vec![0; 64 * 1024];
    let filled = read_full_at(&file, &mut buf, low - 1);
    match memchr::memchr(b'\n', &buf[..filled]) {
        Some(newline) => low + newline as u64,
        None => 0,
    }
}

/// The time of a file's last line that has one
fn last_stamp(path: &Path, parser: &Parser) -> Option<i64> {
    let file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let mut buf = vec![0; 64 * 1024];
    let from = len.saturating_sub(buf.len() as u64);
    let filled = read_full_at(&file, &mut buf, from);
    buf[..filled]
        .rsplit(|&c| c == b'\n')
        .take(50)
        .find_map(|line| {
            match parser.read(&String::from_utf8_lossy(line)) {
                Line::Request(r) => r.at,
                Line::Fault(f) => f.at,
                Line::Unread => None,
            }
            .map(|at| at.at)
        })
}

/// How far back the screen starts in a log that is being written
pub const LOOKBACK: i64 = 300;

/// One of the files has a line from the last minutes: it is being written
pub fn being_written(files: &[PathBuf], parser: &Parser, wall: i64) -> bool {
    files
        .iter()
        .filter_map(|path| last_stamp(path, parser))
        .any(|at| at >= wall - LIVE)
}

/// One file being read
struct Tail {
    path: Option<PathBuf>,
    input: Box<dyn BufRead + Send>,
    /// Bytes read from this file
    position: u64,
    /// Which file the path named when it was opened
    identity: u64,
    /// The start of a line whose end hasn't been written yet
    pending: Vec<u8>,
}

#[cfg(unix)]
fn identity(meta: &std::fs::Metadata) -> u64 {
    std::os::unix::fs::MetadataExt::ino(meta)
}

/// Windows has no inode to ask for; when a file was made tells two apart,
/// except that a file made under a name just vacated is given the old
/// one's time. Such a rotation is still seen when the new file is shorter
/// than what was read of the old.
#[cfg(not(unix))]
fn identity(meta: &std::fs::Metadata) -> u64 {
    meta.created()
        .ok()
        .and_then(|made| made.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |since| since.as_nanos() as u64)
}

impl Tail {
    fn open(path: &Path) -> Result<Tail, String> {
        Tail::open_at(path, 0)
    }

    /// The file from `position` on, which is where a line starts
    fn open_at(path: &Path, position: u64) -> Result<Tail, String> {
        use std::io::Seek;
        let unreadable = |e: std::io::Error| format!("couldn't read {}: {e}", path.display());
        let mut file = std::fs::File::open(path).map_err(unreadable)?;
        file.seek(std::io::SeekFrom::Start(position))
            .map_err(unreadable)?;
        let meta = file.metadata().map_err(unreadable)?;
        if meta.is_dir() {
            return Err(format!("{} is a directory", path.display()));
        }
        Ok(Tail {
            path: Some(path.to_path_buf()),
            input: Box::new(BufReader::with_capacity(CHUNK, file)),
            position,
            identity: identity(&meta),
            pending: Vec::new(),
        })
    }

    fn stdin(piped: Option<&std::fs::File>) -> Tail {
        let input: Box<dyn BufRead + Send> = match piped.and_then(|file| file.try_clone().ok()) {
            Some(file) => Box::new(BufReader::with_capacity(CHUNK, file)),
            None => Box::new(BufReader::with_capacity(CHUNK, std::io::stdin())),
        };
        Tail {
            path: None,
            input,
            position: 0,
            identity: 0,
            pending: Vec::new(),
        }
    }

    /// Read to the end of what is there and count its whole lines. At the
    /// `last` reading a line with no end is counted too.
    fn drain(&mut self, job: &Job, shared: &Shared, last: bool) {
        loop {
            if shared.stop.load(Ordering::Relaxed) {
                return;
            }
            // Waiting for input, when there is waiting, holds no lock
            let read = match self.input.fill_buf() {
                Ok(chunk) => {
                    self.pending.extend_from_slice(chunk);
                    chunk.len()
                }
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(e) => {
                    shared.lock().trouble = Some(match &self.path {
                        Some(path) => format!("couldn't read {}: {e}", path.display()),
                        None => format!("couldn't read stdin: {e}"),
                    });
                    0
                }
            };
            self.input.consume(read);
            self.position += read as u64;
            let whole = match (read, last) {
                (0, true) => self.pending.len(),
                _ => memchr::memrchr(b'\n', &self.pending).map_or(0, |i| i + 1),
            };
            if whole > 0 {
                let mut stats = shared.lock();
                let wall = wall();
                stats.read_bytes += whole as u64;
                fold_lines(&self.pending[..whole], job, &mut stats, wall);
                // A pipe has no end to be short of: once read from, it is
                // caught up with, and the screen is live from its first line
                if self.path.is_none() {
                    stats.caught_up = true;
                }
                drop(stats);
                self.pending.drain(..whole);
            }
            if read == 0 {
                return;
            }
        }
    }

    /// The path names another file now, or this one was emptied: logrotate
    /// has been. Read the old file out and start on the new one.
    fn reopen_if_rotated(&mut self, job: &Job, shared: &Shared) {
        let Some(path) = self.path.clone() else {
            return;
        };
        let Ok(meta) = std::fs::metadata(&path) else {
            return;
        };
        if identity(&meta) == self.identity && meta.len() >= self.position {
            return;
        }
        // What the old file still has, last line included, is read out of
        // the handle to it. A file emptied in place has nothing more.
        self.drain(job, shared, true);
        if let Ok(fresh) = Tail::open(&path) {
            *self = fresh;
        }
    }
}

/// Read the job's files on the calling thread: to their ends, and then,
/// when following, whatever is appended until `shared` is stopped
pub fn read(job: &Job, shared: &Shared) {
    let mut files = job.files.clone();
    files.sort_by_cached_key(|path| first_stamp(path, &job.parser).unwrap_or(i64::MAX));
    let mut tails: Vec<Tail> = Vec::new();
    if files.is_empty() {
        tails.push(Tail::stdin(job.piped.as_deref()));
    }
    for path in &files {
        match Tail::open(path) {
            Ok(tail) => tails.push(tail),
            Err(e) => shared.lock().trouble = Some(e),
        }
    }
    shared.lock().exact_paths = job.exact_paths;
    shared.lock().total_bytes = files
        .iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum();
    // Stdin ends when its writer does; a file being followed never does
    let following = job.follow && !files.is_empty();
    let threads = std::thread::available_parallelism().map_or(1, usize::from);
    shared.lock().since = job.since;
    for tail in &mut tails {
        // With a time to start from, the file is taken up where that
        // time is. What it has from there is first read by every core,
        // when that is worth it; its last lines and what comes after,
        // by this thread.
        if let Some(path) = tail.path.clone() {
            let origin = job
                .since
                .map_or(0, |since| seek_since(&path, &job.parser, since));
            shared.lock().read_bytes += origin;
            let rest = std::fs::metadata(&path).map_or(0, |meta| meta.len().saturating_sub(origin));
            let reached = match rest >= SPRINT_MIN {
                true => sprint(&path, job, shared, origin, SPRINT_CHUNK, threads),
                false => origin,
            };
            if reached > 0 {
                if let Ok(rest) = Tail::open_at(&path, reached) {
                    *tail = rest;
                }
            }
        }
        tail.drain(job, shared, !following);
    }
    shared.lock().caught_up = true;
    while following && !shared.stop.load(Ordering::Relaxed) {
        std::thread::sleep(POLL);
        for tail in &mut tails {
            tail.reopen_if_rotated(job, shared);
            tail.drain(job, shared, false);
        }
    }
}

/// Read on a thread of its own, for the screen to watch
pub fn start(job: Job) -> Arc<Shared> {
    let shared = Arc::new(Shared::default());
    let theirs = shared.clone();
    std::thread::Builder::new()
        .name("pepe-logs".into())
        .spawn(move || read(&job, &theirs))
        .expect("a thread to read the logs on");
    shared
}

/// Keep what is piped in to read the log from, and make the terminal
/// stdin again: the screen reads its keys there. None when there is no
/// terminal to be had.
#[cfg(unix)]
pub fn piped_aside() -> Option<Arc<std::fs::File>> {
    use std::os::fd::{AsRawFd, FromRawFd};
    // The terminal by its own name, which stdout has: macOS can't wait
    // for keys on /dev/tty
    let mut name = [0 as libc::c_char; 256];
    // SAFETY: ttyname_r writes a NUL-terminated name into the buffer it is
    // given the length of
    let name = unsafe {
        if libc::ttyname_r(libc::STDOUT_FILENO, name.as_mut_ptr(), name.len()) != 0 {
            return None;
        }
        std::ffi::CStr::from_ptr(name.as_ptr()).to_str().ok()?
    };
    let tty = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(name)
        .ok()?;
    // SAFETY: dup gives a descriptor nothing else owns, which the File
    // takes; dup2 only repoints descriptor 0
    unsafe {
        let pipe = libc::dup(libc::STDIN_FILENO);
        if pipe < 0 {
            return None;
        }
        let pipe = std::fs::File::from_raw_fd(pipe);
        (libc::dup2(tty.as_raw_fd(), libc::STDIN_FILENO) >= 0).then(|| Arc::new(pipe))
    }
}

#[cfg(not(unix))]
pub fn piped_aside() -> Option<Arc<std::fs::File>> {
    None
}

/// The files to read when none is named: nginx's own, where they are
pub fn default_files() -> Vec<PathBuf> {
    ["/var/log/nginx/access.log", "/var/log/nginx/error.log"]
        .iter()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
        .collect()
}

// ─── Reports ─────────────────────────────────────────────────────────────────

/// How the rate now stands against the log, in a line: the headline of the
/// report and of the screen
pub fn headline(stats: &Stats, clock: Clock, window: i64) -> String {
    let now = stats.now(clock, window);
    let when = if clock.live {
        format!(
            "now, over the last {}",
            format::span(Duration::from_secs(window as u64))
        )
    } else {
        format!(
            "at the end of the log, {}",
            day_and_time(clock.now + i64::from(clock.offset))
        )
    };
    format!("{} req/s {when}", rate(now.rate))
}

/// The report left in the shell
pub fn report(stats: &Stats, name: &str, wall: i64, window: i64, rows: usize) -> String {
    use std::fmt::Write;
    let clock = stats.clock(wall);
    let now = stats.now(clock, window);
    let local = |at: Stamp| at.at + i64::from(clock.offset);
    let mut out = String::new();
    let _ = write!(
        out,
        "pepe logs · {name} · {}",
        counted(stats.requests, "request")
    );
    if stats.faults > 0 {
        let _ = write!(out, " · {}", counted(stats.faults, "error log line"));
    }
    if let (Some(first), Some(last)) = (stats.first, stats.last) {
        let _ = write!(
            out,
            " · {} → {} ({})",
            day_and_time(local(first)),
            day_and_time(local(last)),
            offset_label(clock.offset)
        );
    }
    out.push('\n');
    if stats.lines == 0 {
        out.push_str("▲ Nothing to read: the log is empty\n");
        return out;
    }

    let glyph = match now.share_5xx() {
        s if s >= 0.05 => '✖',
        s if s >= 0.01 => '▲',
        _ => '✔',
    };
    let _ = writeln!(out, "{glyph} {}", headline(stats, clock, window));
    if now.requests > 0 && now.c4xx + now.c5xx > 0 {
        let _ = writeln!(
            out,
            "  {} 5xx {} · 4xx {} of those requests",
            if now.share_5xx() >= 0.01 { '▲' } else { ' ' },
            percent(now.share_5xx()),
            percent(now.share_4xx())
        );
    }
    for grain in Grain::ALL {
        let slots = stats.rows(grain, clock);
        let (Some(usual), Some(most)) = (typical(&slots), busiest(&slots)) else {
            continue;
        };
        let _ = write!(
            out,
            "  a usual {} {} req/s ({} now) · the busiest {} req/s, {}",
            grain.name(),
            rate(usual),
            versus(now.rate, usual),
            rate(most.rate),
            grain.label(most.start)
        );
        if let Some(then) = echo(&slots, grain).filter(|r| !r.partial) {
            let _ = write!(
                out,
                " · {} {} req/s ({} now)",
                grain.echo().1,
                rate(then.rate),
                versus(now.rate, then.rate)
            );
        }
        out.push('\n');
    }
    if let Some((at, most)) = stats.peak.filter(|(_, most)| *most > 1) {
        let _ = writeln!(
            out,
            "  the busiest second had {} requests, {}",
            format::count(u64::from(most)),
            day_and_time(local(at))
        );
    }
    if stats.time.count() > 0 {
        let at = |q: f64| format::latency(Duration::from_micros(stats.time.percentile(q)));
        let _ = writeln!(
            out,
            "  request time p50 {} · p90 {} · p99 {}",
            at(50.0),
            at(90.0),
            at(99.0)
        );
    }
    if stats.unread > 0 {
        let _ = writeln!(
            out,
            "  ▲ {} couldn't be read; if the log has a format of its own, give it with --format. The first: {}",
            counted(stats.unread, "line"),
            stats.first_unread.as_deref().map(|l| l.chars().take(160).collect::<String>()).unwrap_or_default()
        );
    }
    if stats.undated > 0 {
        let _ = writeln!(
            out,
            "  ▲ {} had no time, and so are in no slot",
            counted(stats.undated, "request")
        );
    }

    for grain in Grain::ALL {
        let slots = stats.rows(grain, clock);
        if slots.len() < 2 && grain != Grain::Minute {
            continue;
        }
        let _ = writeln!(
            out,
            "\nBy {:<21}requests    req/s   peak/s     4xx     5xx   now vs",
            grain.name()
        );
        if slots.len() > rows {
            let _ = writeln!(
                out,
                "  … {} earlier",
                format::count((slots.len() - rows) as u64)
            );
        }
        for row in slots.iter().skip(slots.len().saturating_sub(rows)) {
            let of = |n: u64| percent(n as f64 / row.slot.requests.max(1) as f64);
            let _ = writeln!(
                out,
                "  {:<18}{:>12} {:>8} {:>8} {:>7} {:>7} {:>8}{}",
                grain.label(row.start),
                format::count(row.slot.requests),
                rate(row.rate),
                row.slot.peak,
                of(row.slot.c4xx),
                of(row.slot.c5xx),
                versus(now.rate, row.rate),
                if row.partial { "  (partial)" } else { "" }
            );
        }
    }

    if !stats.statuses.is_empty() {
        let mut codes: Vec<(&u16, &u64)> = stats.statuses.iter().collect();
        codes.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
        let codes: Vec<String> = codes
            .iter()
            .take(8)
            .map(|(code, n)| {
                let code = match **code {
                    0 => "no status".to_string(),
                    code => code.to_string(),
                };
                format!(
                    "{code} {}",
                    percent(**n as f64 / stats.requests.max(1) as f64)
                )
            })
            .collect();
        let _ = writeln!(out, "\nStatus  {}", codes.join(" · "));
    }
    let paths = stats.paths.top(rows, |p| p.requests);
    if !paths.is_empty() {
        let _ = writeln!(out, "\nPaths{:>39}     4xx     5xx   mean time", "requests");
        for (path, stat) in paths {
            let of = |n: u64| percent(n as f64 / stat.requests.max(1) as f64);
            let _ = writeln!(
                out,
                "  {:<30}{:>12} {:>7} {:>7} {:>11}",
                path.chars().take(30).collect::<String>(),
                format::count(stat.requests),
                of(stat.c4xx),
                of(stat.c5xx),
                stat.mean_time().map(format::latency).unwrap_or_default()
            );
        }
        if stats.paths.other > 0 {
            let _ = writeln!(
                out,
                "  … and {} requests of paths past the first {}",
                format::count(stats.paths.other),
                format::count(PATHS as u64)
            );
        }
    }
    let causes = stats.top_causes(rows);
    if !causes.is_empty() {
        out.push_str("\nError log\n");
        for (what, cause) in causes {
            let glyph = match severity(&cause.level) {
                2 => '✖',
                1 => '▲',
                _ => ' ',
            };
            let _ = writeln!(
                out,
                "  {glyph} {:>8}×  {}{}",
                format::count(cause.count),
                what.chars().take(110).collect::<String>(),
                cause
                    .last
                    .map(|at| format!(" · last {}", day_and_time(local(at))))
                    .unwrap_or_default()
            );
        }
    }
    out
}

/// `--json`: the same counts, for a program to read
pub fn json_report(
    stats: &Stats,
    job: &Job,
    wall: i64,
    window: i64,
    rows: usize,
) -> serde_json::Value {
    use serde_json::json;
    let clock = stats.clock(wall);
    let now = stats.now(clock, window);
    let round = |v: f64| (v * 1_000.0).round() / 1_000.0;
    let stamp = |local: i64| {
        Stamp {
            at: local - i64::from(clock.offset),
            offset: clock.offset,
        }
        .iso()
    };
    let ms = |us: u64| us as f64 / 1_000.0;
    let slots = |grain: Grain| -> Vec<serde_json::Value> {
        stats
            .rows(grain, clock)
            .iter()
            .map(|row| {
                json!({
                    "start": stamp(row.start),
                    "partial": row.partial,
                    "requests": row.slot.requests,
                    "requests_per_second": round(row.rate),
                    "peak_per_second": row.slot.peak,
                    "status_4xx": row.slot.c4xx,
                    "status_5xx": row.slot.c5xx,
                    "bytes": row.slot.bytes,
                    "mean_request_time_ms": (row.slot.timed > 0).then(|| ms(row.slot.time_us / row.slot.timed)),
                    "error_log_lines": row.slot.faults,
                    "now_over_this": (row.rate > 0.0).then(|| round(now.rate / row.rate)),
                })
            })
            .collect()
    };
    let usual = |grain: Grain| {
        let rows = stats.rows(grain, clock);
        json!({
            "typical_requests_per_second": typical(&rows).map(round),
            "now_over_typical": typical(&rows).filter(|t| *t > 0.0).map(|t| round(now.rate / t)),
            "busiest_requests_per_second": busiest(&rows).map(|r| round(r.rate)),
            "busiest_start": busiest(&rows).map(|r| stamp(r.start)),
        })
    };
    let counted = |top: &Top<u64>| -> Vec<serde_json::Value> {
        top.top(rows, |n| *n)
            .iter()
            .map(|(name, n)| json!({ "name": name, "requests": n }))
            .collect()
    };
    let percentiles = |h: &Histogram| {
        (h.count() > 0).then(|| {
            json!({
                "p50": ms(h.percentile(50.0)),
                "p90": ms(h.percentile(90.0)),
                "p99": ms(h.percentile(99.0)),
            })
        })
    };
    json!({
        "logs": {
            "files": job.files.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
            "lines": stats.lines,
            "requests": stats.requests,
            "error_log_lines": stats.faults,
            "unread_lines": stats.unread,
            "first_unread_line": stats.first_unread,
            "requests_without_time": stats.undated,
            "lines_before_since": stats.skipped,
            "bytes_sent": stats.bytes,
            "first": stats.first.map(Stamp::iso),
            "last": stats.last.map(Stamp::iso),
            "live": clock.live,
        },
        "now": {
            "at": Stamp { at: clock.now, offset: clock.offset }.iso(),
            "window_s": window,
            "requests": now.requests,
            "requests_per_second": round(now.rate),
            "status_4xx": now.c4xx,
            "status_5xx": now.c5xx,
        },
        "busiest_second": stats.peak.map(|(at, n)| json!({ "at": at.iso(), "requests": n })),
        "minute": usual(Grain::Minute),
        "hour": usual(Grain::Hour),
        "day": usual(Grain::Day),
        "per_minute": slots(Grain::Minute),
        "per_hour": slots(Grain::Hour),
        "per_day": slots(Grain::Day),
        "status_codes": stats.statuses.iter().map(|(code, n)| (code.to_string(), *n)).collect::<BTreeMap<_, _>>(),
        "methods": counted(&stats.methods),
        "paths": stats.paths.top(rows, |p| p.requests).iter().map(|(path, p)| json!({
            "path": path,
            "requests": p.requests,
            "status_4xx": p.c4xx,
            "status_5xx": p.c5xx,
            "mean_request_time_ms": p.mean_time().map(|t| ms(t.as_micros() as u64)),
        })).collect::<Vec<_>>(),
        "clients": counted(&stats.clients),
        "user_agents": counted(&stats.agents),
        "query_parameters": counted(&stats.params),
        "request_time_ms": percentiles(&stats.time),
        "upstream_response_time_ms": percentiles(&stats.upstream),
        "error_log": {
            "levels": stats.levels,
            "messages": stats.top_causes(rows).iter().map(|(what, cause)| json!({
                "level": cause.level,
                "message": what.split_once(' ').map_or(*what, |(_, m)| m),
                "count": cause.count,
                "last": cause.last.map(Stamp::iso),
                "example": cause.example,
            })).collect::<Vec<_>>(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const LINE: &str = r#"203.0.113.9 - bob [08/Oct/2026:13:55:36 +0200] "GET /search?q=tea&page=2 HTTP/1.1" 200 2326 "https://ref.test/" "Mozilla/5.0 (X11)""#;

    fn request<'a>(parser: &Parser, line: &'a str) -> Record<'a> {
        match parser.read(line) {
            Line::Request(r) => r,
            other => panic!("{line}: {other:?}"),
        }
    }

    /// 08 Oct 2026 11:55:36 UTC
    const AT: i64 = 1_791_460_536;

    #[test]
    fn times_are_read_in_every_way_nginx_writes_them() {
        let at = Stamp::parse("08/Oct/2026:13:55:36 +0200").unwrap();
        assert_eq!((at.at, at.offset), (AT, 7_200));
        assert_eq!(at.iso(), "2026-10-08T13:55:36+02:00");
        assert_eq!(Stamp::parse("2026-10-08T11:55:36+00:00").unwrap().at, AT);
        assert_eq!(Stamp::parse("2026-10-08T11:55:36.123Z").unwrap().at, AT);
        assert_eq!(Stamp::parse("2026-10-08T06:25:36-0530").unwrap().at, AT);
        assert_eq!(Stamp::parse("1791460536.123").unwrap().at, AT);
        assert_eq!(Stamp::parse("1791460536123").unwrap().at, AT);
        // The error log names no offset: it is this machine's
        let local = Stamp::parse("2026/10/08 11:55:36").unwrap();
        assert_eq!(local.at, AT - i64::from(local_offset()));
        for not in [
            "",
            "-",
            "200",
            "08/Oct/2026",
            "32/Oct/2026:13:55:36 +0000",
            "2026-13-08T11:55:36Z",
        ] {
            assert_eq!(Stamp::parse(not), None, "{not}");
        }
        let c = civil(AT);
        assert_eq!(
            (c.year, c.month, c.day, c.hour, c.minute, c.second, c.weekday),
            (2026, 10, 8, 11, 55, 36, 3),
            "a Thursday"
        );
        assert_eq!(civil(0).weekday, 3);
        assert_eq!(civil(951_782_400).day, 29, "2000 was a leap year");
        assert_eq!(day_and_time(AT), "08 Oct 11:55:36");
        assert_eq!(Grain::Day.label(AT), "Thu 08 Oct 2026");
        assert_eq!(Grain::Hour.label(AT - AT % 3_600), "08 Oct 11:00");
        assert_eq!(span("7d"), Ok(604_800));
        assert_eq!(span("90"), Ok(90));
        assert!(span("soon").is_err() && span("0s").is_err() && span("3y").is_err());
    }

    #[test]
    fn combined_lines_give_up_every_field() {
        let parser = Parser::default();
        let r = request(&parser, LINE);
        assert_eq!(r.at.unwrap().at, AT);
        assert_eq!(
            (&*r.client, &*r.method, &*r.target, r.status, r.bytes, &*r.agent),
            (
                "203.0.113.9",
                "GET",
                "/search?q=tea&page=2",
                200,
                2326,
                "Mozilla/5.0 (X11)"
            )
        );
        assert_eq!((r.time, r.upstream), (None, None));
        // Timings after the format, as nginx's documentation writes them
        let timed = format!("{LINE} rt=0.250 uct=\"0.001\" urt=\"0.100, 0.120\"");
        let r = request(&parser, &timed);
        assert_eq!(r.time, Some(0.25));
        assert!((r.upstream.unwrap() - 0.22).abs() < 1e-9);
        // `common` ends after the size
        let r = request(
            &parser,
            r#"::1 - - [08/Oct/2026:13:55:36 +0200] "POST /a HTTP/2.0" 201 0"#,
        );
        assert_eq!((&*r.client, &*r.method, r.status), ("::1", "POST", 201));
        // What nginx couldn't read as a request is still a response it gave
        let r = request(
            &parser,
            r#"1.2.3.4 - - [08/Oct/2026:13:55:36 +0200] "\x16\x03\x01" 400 157 "-" "-""#,
        );
        assert_eq!(
            (&*r.method, &*r.target, r.status, &*r.agent),
            ("", r"\x16\x03\x01", 400, "")
        );
        // A format with something in front is searched
        let prefixed = format!("example.com {LINE}");
        let r = request(&parser, &prefixed);
        assert_eq!(
            (r.at.unwrap().at, &*r.target, r.status, r.bytes),
            (AT, "/search?q=tea&page=2", 200, 2326)
        );
        assert_eq!(&*r.agent, "Mozilla/5.0 (X11)");
        // `docker compose logs` names the container in front of each line
        let r = request(&parser, "nginx-1  | 172.18.0.1 - - [08/Oct/2026:13:55:36 +0200] \"GET / HTTP/1.1\" 200 5 \"-\" \"curl/8\" \"-\"");
        assert_eq!(
            (&*r.client, r.at.unwrap().at, r.status),
            ("172.18.0.1", AT, 200)
        );
        assert!(matches!(
            parser.read("shop-nginx-1  | 2026/10/08 11:55:36 [notice] 1#1: start worker processes"),
            Line::Fault(Fault {
                level: "notice",
                message: "start worker processes",
                ..
            })
        ));
        assert_eq!(unprefixed("GET /a | b"), "GET /a | b");
        // Into a pipe, compose colours the name; a tab in a line is a space
        // and the other control characters are left out, so no line read
        // can move the cursor when it is drawn
        let coloured = "\x1b[36mnginx_twitter  |\x1b[0m 172.18.0.1 - - [08/Oct/2026:13:55:36 +0200] \"GET /\tHTTP/1.1\" 200 5 \"-\" \"curl/8\"\r\x07";
        let cleaned = clean(coloured);
        assert_eq!(
            &*cleaned,
            "nginx_twitter  | 172.18.0.1 - - [08/Oct/2026:13:55:36 +0200] \"GET / HTTP/1.1\" 200 5 \"-\" \"curl/8\""
        );
        assert_eq!(&*request(&parser, &cleaned).client, "172.18.0.1");
        assert!(matches!(clean("plain"), Cow::Borrowed("plain")));
        assert_eq!(&*clean("\x1b]8;;http://x\x1b\\link\x1b]8;;\x07"), "link");
        for not in [
            "",
            "hello world",
            "# a comment",
            "1.2.3.4 - - [nonsense] \"x\" abc 1",
        ] {
            assert_eq!(parser.read(not), Line::Unread, "{not}");
        }
    }

    #[test]
    fn a_log_format_of_ones_own_is_followed() {
        let parser = Parser::new(Some(
            r#"$time_iso8601 $host $remote_addr "$request" $status $body_bytes_sent $request_time $upstream_response_time $upstream_cache_status"#,
        ))
        .unwrap();
        let line = r#"2026-10-08T11:55:36+00:00 shop.test 10.0.0.7 "GET /cart HTTP/1.1" 502 0 1.500 - MISS"#;
        let r = request(&parser, line);
        assert_eq!(
            (r.at.unwrap().at, &*r.host, &*r.client, &*r.target, r.status),
            (AT, "shop.test", "10.0.0.7", "/cart", 502)
        );
        assert_eq!((r.time, r.upstream), (Some(1.5), None));
        let fields = parser.inspect(line);
        let field = |name: &str| {
            fields
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(field("$upstream_cache_status"), Some("MISS"));
        assert_eq!(field("request time"), Some("1.50s"));
        assert_eq!(field("status"), Some("502"));
        // A line of another shape isn't forced into it
        assert_eq!(parser.read(LINE), Line::Unread);

        assert!(Format::parse("$status$request_time")
            .unwrap_err()
            .contains("nothing between"));
        assert!(Format::parse("no variables").is_err());
        assert!(Format::parse("$foo $bar").is_err());
        assert_eq!(
            Format::parse("combined").unwrap(),
            Format::parse(COMBINED).unwrap()
        );
        assert_eq!(
            Format::parse("${status} x").unwrap(),
            Format::parse("$status x").unwrap()
        );
    }

    #[test]
    fn the_inspector_names_the_query_parameters() {
        let fields = Parser::default().inspect(LINE);
        let named: Vec<(&str, &str)> = fields
            .iter()
            .map(|(n, v)| (n.as_str(), v.as_str()))
            .collect();
        assert_eq!(
            named,
            [
                ("time", "2026-10-08T13:55:36+02:00"),
                ("client", "203.0.113.9"),
                ("method", "GET"),
                ("path", "/search"),
                ("?q", "tea"),
                ("?page", "2"),
                ("status", "200"),
                ("bytes sent", "2,326"),
                ("user agent", "Mozilla/5.0 (X11)"),
                ("$remote_user", "bob"),
                ("$http_referer", "https://ref.test/"),
            ]
        );
    }

    #[test]
    fn json_lines_are_read_under_nginx_and_caddy_names() {
        let parser = Parser::default();
        let r = request(
            &parser,
            r#"{"time_iso8601":"2026-10-08T11:55:36+00:00","remote_addr":"1.2.3.4","request":"GET /a?b=1 HTTP/1.1","status":"404","body_bytes_sent":"12","request_time":"0.004","http_user_agent":"curl/8"}"#,
        );
        assert_eq!(
            (
                r.at.unwrap().at,
                &*r.client,
                &*r.method,
                &*r.target,
                r.status,
                r.bytes,
                r.time,
                &*r.agent
            ),
            (
                AT,
                "1.2.3.4",
                "GET",
                "/a?b=1",
                404,
                12,
                Some(0.004),
                "curl/8"
            )
        );
        let r = request(
            &parser,
            r#"{"level":"info","ts":1791460536.5,"request":{"remote_ip":"1.2.3.4","method":"GET","host":"example.com","uri":"/blog/","headers":{"User-Agent":["curl/8"]}},"duration":0.02,"size":512,"status":200}"#,
        );
        assert_eq!(
            (
                r.at.unwrap().at,
                &*r.client,
                &*r.host,
                &*r.target,
                r.status,
                r.bytes,
                r.time,
                &*r.agent
            ),
            (
                AT,
                "1.2.3.4",
                "example.com",
                "/blog/",
                200,
                512,
                Some(0.02),
                "curl/8"
            )
        );
        assert_eq!(parser.read(r#"{"nothing":1}"#), Line::Unread);
        assert_eq!(parser.read("{not json"), Line::Unread);
    }

    const FAULT: &str = r#"2026/10/08 11:55:36 [error] 31#31: *1024 connect() failed (111: Connection refused) while connecting to upstream, client: 10.0.0.7, server: shop.test, request: "GET /cart HTTP/1.1", upstream: "http://127.0.0.1:8080/cart", host: "shop.test""#;

    #[test]
    fn error_log_lines_are_read_and_grouped_by_cause() {
        let parser = Parser::default();
        let Line::Fault(f) = parser.read(FAULT) else {
            panic!("not a fault");
        };
        assert_eq!(
            (f.level, f.message, f.client, f.request, f.upstream),
            (
                "error",
                "connect() failed (111: Connection refused) while connecting to upstream",
                "10.0.0.7",
                "GET /cart HTTP/1.1",
                "http://127.0.0.1:8080/cart"
            )
        );
        let Line::Fault(f) =
            parser.read("2026/10/08 11:55:36 [notice] 1#1: signal process started")
        else {
            panic!("not a fault");
        };
        assert_eq!(
            (f.level, f.message, f.client),
            ("notice", "signal process started", "")
        );
        assert_eq!(
            cause_of(r#"open() "/var/www/a.ico" failed (2: No such file or directory)"#),
            r#"open() "…" failed (2: No such file or directory)"#
        );
        assert_eq!(
            cause_of("client intended to send too large body: 10485761 bytes"),
            "client intended to send too large body: N bytes"
        );
        assert_eq!(
            cause_of("limiting requests, excess: 20.123 by zone \"api\""),
            "limiting requests, excess: N by zone \"…\""
        );
        assert_eq!(
            cause_of("upstream sent HTTP/1.1 via h2"),
            "upstream sent HTTP/N via h2"
        );

        let mut stats = Stats::default();
        for client in ["10.0.0.7", "10.0.0.8", "10.0.0.9"] {
            let line = FAULT.replace("10.0.0.7", client).replace("*1024", "*77");
            stats.fold(parser.read(&line), &line, 0);
        }
        let other = r#"2026/10/08 11:55:40 [warn] 31#31: *5 an upstream response is buffered to a temporary file /var/cache/nginx/proxy_temp/1/00/0000000001 while reading upstream, client: 10.0.0.7"#;
        stats.fold(parser.read(other), other, 0);
        assert_eq!((stats.faults, stats.causes.len()), (4, 2));
        let causes = stats.top_causes(10);
        assert_eq!(
            (causes[0].1.level.as_str(), causes[0].1.count),
            ("error", 3)
        );
        assert!(
            causes[0].0.starts_with("error connect() failed (111:"),
            "{}",
            causes[0].0
        );
        assert_eq!(causes[0].1.example, FAULT.replace("*1024", "*77"));
        assert_eq!(stats.levels["warn"], 1);
        // Errors, and not warnings, count in the slots
        let clock = stats.clock(0);
        assert_eq!(stats.rows(Grain::Hour, clock)[0].slot.faults, 3);
    }

    /// A line `seconds` after `AT`
    fn hit(seconds: i64, path: &str, status: u16) -> String {
        let at = Stamp {
            at: AT + seconds,
            offset: 0,
        };
        let c = civil(at.at);
        format!(
            "1.1.1.{} - - [{:02}/{}/{}:{:02}:{:02}:{:02} +0000] \"GET {path} HTTP/1.1\" {status} 100 \"-\" \"ua\"",
            seconds.rem_euclid(3),
            c.day,
            MONTHS[c.month as usize - 1],
            c.year,
            c.hour,
            c.minute,
            c.second
        )
    }

    fn counted(lines: &[String], wall: i64) -> Stats {
        let parser = Parser::default();
        let mut stats = Stats::default();
        for line in lines {
            stats.fold(parser.read(line), line, wall);
        }
        stats
    }

    #[test]
    fn slots_have_their_rate_and_now_is_held_against_them() {
        // AT is 11:55:36. Two whole minutes at 2 and 4 req/s, then a third
        // minute of which 30 seconds were written, at 1 req/s
        let start = 24; // 11:56:00
        let mut lines = Vec::new();
        for s in 0..60 {
            lines.extend([hit(start + s, "/a", 200), hit(start + s, "/b?x=1", 200)]);
        }
        for s in 60..120 {
            lines.extend((0..4).map(|i| hit(start + s, "/a", if i == 0 { 503 } else { 200 })));
        }
        lines.extend((120..150).map(|s| hit(start + s, "/c", 404)));
        // The log ended a day ago: now is its last second, not the clock's
        let stats = counted(&lines, AT + 86_400);
        let clock = stats.clock(AT + 86_400);
        assert_eq!((clock.now, clock.live), (AT + start + 149, false));
        let now = stats.now(clock, 60);
        // The last 60 seconds: 30 at 4 req/s and 30 at 1
        assert_eq!((now.requests, now.rate), (150, 2.5));
        assert_eq!((now.c4xx, now.c5xx), (30, 30));

        let minutes = stats.rows(Grain::Minute, clock);
        let seen: Vec<(u64, f64, bool, u32)> = minutes
            .iter()
            .map(|r| (r.slot.requests, r.rate, r.partial, r.slot.peak))
            .collect();
        assert_eq!(
            seen,
            [
                (120, 2.0, false, 2),
                (240, 4.0, false, 4),
                (30, 1.0, true, 1)
            ]
        );
        assert_eq!(minutes[1].slot.c5xx, 60);
        assert_eq!(Grain::Minute.label(minutes[0].start), "08 Oct 11:56");
        assert_eq!(typical(&minutes), Some(4.0));
        assert_eq!(busiest(&minutes).unwrap().rate, 4.0);
        assert_eq!(versus(now.rate, 4.0), "-38%");
        // One hour, which the log starts inside of and hasn't finished
        let hours = stats.rows(Grain::Hour, clock);
        assert_eq!(hours.len(), 1);
        assert!(hours[0].partial && (hours[0].rate - 390.0 / 150.0).abs() < 1e-9);
        assert_eq!(typical(&hours), None);
        assert_eq!(stats.peak.unwrap().1, 4);
        assert_eq!(stats.last_seconds(clock, 3), [1, 1, 1]);

        assert_eq!(
            (stats.requests, stats.statuses[&404], stats.statuses[&503]),
            (390, 30, 60)
        );
        let paths = stats.paths.top(2, |p| p.requests);
        assert_eq!(
            (paths[0].0, paths[0].1.requests, paths[0].1.c5xx),
            ("/a", 300, 60)
        );
        assert_eq!((paths[1].0, paths[1].1.requests), ("/b", 60));
        assert_eq!(stats.params.top(5, |n| *n), [("x", &60)]);
        assert_eq!(stats.clients.len(), 3);

        // The same log, being written: now is the clock's, and the minute
        // in progress is measured to it
        let stats = counted(&lines, AT + start + 150);
        let clock = stats.clock(AT + start + 179);
        assert!(clock.live);
        // The last 60 seconds have the 30 the third minute saw
        assert_eq!(stats.now(clock, 60).requests, 30);
        let minutes = stats.rows(Grain::Minute, clock);
        assert_eq!((minutes[2].rate, minutes[2].partial), (0.5, false));
    }

    #[test]
    fn versus_and_rates_read_well() {
        assert_eq!(versus(112.0, 100.0), "+12%");
        assert_eq!(versus(340.0, 100.0), "×3.4");
        assert_eq!(versus(40.0, 100.0), "÷2.5");
        assert_eq!(versus(100.0, 100.0), "=");
        assert_eq!(versus(0.0, 0.0), "=");
        assert_eq!(versus(5.0, 0.0), "new");
        assert_eq!(versus(0.0, 5.0), "-100%");
        assert_eq!(rate(0.034), "0.03");
        assert_eq!(rate(4.25), "4.2");
        assert_eq!(rate(12_300.0), "12.3k");
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(0.0004), "<0.1%");
        assert_eq!(percent(0.042), "4.2%");
        assert_eq!(percent(0.5), "50%");
        assert_eq!(ago(45), "45s");
        assert_eq!(ago(600), "10m");
        assert_eq!(ago(3 * 86_400), "3d");
    }

    #[test]
    fn memory_stays_the_same_however_long_the_log() {
        let mut stats = Stats::default();
        let parser = Parser::default();
        // Three days, a request every 20 seconds, each to a path of its own
        for i in 0..(3 * 86_400 / 20) {
            let line = hit(i * 20, &format!("/item/{i}"), 200);
            stats.fold(parser.read(&line), &line, 0);
        }
        let clock = stats.clock(0);
        assert_eq!(stats.rows(Grain::Minute, clock).len(), 1_440);
        assert_eq!(stats.rows(Grain::Hour, clock).len(), 73);
        assert_eq!(stats.rows(Grain::Day, clock).len(), 4);
        assert!(stats.seconds.len() <= SECONDS_KEPT);
        assert_eq!(stats.seconds.all().len(), stats.seconds.len());
        assert_eq!(stats.recent.len(), RECENT);
        assert_eq!(stats.recent_base + RECENT as u64, stats.lines);
        assert_eq!(stats.paths.len(), 1, "/item/*");
        assert_eq!(grouped("/item/42/photos/7"), "/item/*/photos/*");
        assert_eq!(grouped("/u/123e4567-e89b-12d3-a456-426614174000"), "/u/*");
        assert_eq!(grouped("/v2/items/a1"), "/v2/items/a1");
        // An error log read after the access log has its lines in their place
        let then = civil(stats.last.unwrap().at - 30 + i64::from(local_offset()));
        let late = FAULT.replace(
            "2026/10/08 11:55:36",
            &format!(
                "{}/{:02}/{:02} {:02}:{:02}:{:02}",
                then.year, then.month, then.day, then.hour, then.minute, then.second
            ),
        );
        stats.fold(parser.read(&late), &late, 0);
        let place = stats.recent.iter().position(|l| l.text == late).unwrap();
        assert_eq!(place, RECENT - 3, "before the last two requests");
        let early = FAULT;
        stats.fold(parser.read(early), early, 0);
        assert!(
            !stats.recent.iter().any(|l| l.text == early),
            "too old to keep"
        );
        assert_eq!(stats.recent.len(), RECENT);
        // A day ago, the same hour: there to be held against
        let hours = stats.rows(Grain::Hour, clock);
        assert_eq!(echo(&hours, Grain::Hour).unwrap().slot.requests, 180);
        // Past the cap, names are counted together
        let mut top: Top<u64> = Top::new(2);
        for name in ["a", "b", "c", "a", "d"] {
            if let Some(n) = top.get(name) {
                *n += 1;
            }
        }
        assert_eq!((top.len(), top.other), (2, 2));
        assert_eq!(top.top(1, |n| *n), [("a", &2)]);
    }

    #[test]
    fn files_are_read_oldest_first_followed_and_reopened_when_rotated() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("pepe-logs-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let (old, new) = (dir.join("access.log.1"), dir.join("access.log"));
        std::fs::write(
            &old,
            format!("{}\n{}\n", hit(0, "/old", 200), hit(1, "/old", 200)),
        )
        .unwrap();
        // The last line has no end yet
        std::fs::write(
            &new,
            format!(
                "{}\nnot a line\n{}",
                hit(100, "/new", 200),
                hit(101, "/new", 500)
            ),
        )
        .unwrap();
        let job = Job {
            // Given newest first, as a shell's glob would
            files: vec![new.clone(), old.clone()],
            piped: None,
            parser: Parser::default(),
            since: Some(AT + 1),
            follow: false,
            exact_paths: false,
        };
        assert_eq!(job.name(), "access.log +1");
        // Piped in, a log is caught up with from its first chunk: there is
        // no end to be short of, and its colour is left out
        let piped = dir.join("piped");
        std::fs::write(
            &piped,
            format!("\x1b[36mweb-1  |\x1b[0m {}\n", hit(200, "/piped", 200)),
        )
        .unwrap();
        let shared = Shared::default();
        let mut tail = Tail::stdin(Some(&std::fs::File::open(&piped).unwrap()));
        tail.drain(&job, &shared, false);
        {
            let stats = shared.lock();
            assert!(stats.caught_up);
            assert_eq!(stats.requests, 1);
            assert_eq!(stats.paths.top(1, |p| p.requests)[0].0, "/piped");
        }
        let shared = Shared::default();
        read(&job, &shared);
        {
            let stats = shared.lock();
            assert_eq!((stats.requests, stats.unread, stats.skipped), (3, 1, 1));
            assert_eq!(stats.first_unread.as_deref(), Some("not a line"));
            assert!(stats.caught_up && stats.trouble.is_none());
            assert!(stats.recent[0].text.contains("/old"), "oldest file first");
            assert_eq!(
                stats.recent.back().unwrap().kind,
                Kind::Request { status: 500 }
            );
            let text = report(&stats, &job.name(), AT + 86_400, 60, 10);
            assert!(text.starts_with("pepe logs · access.log +1 · 3 requests · 08 Oct 11:55:37 → 08 Oct 11:57:17 (UTC+00:00)\n"), "{text}");
            assert!(
                text.contains("✖ 0.03 req/s at the end of the log, 08 Oct 11:57:17"),
                "{text}"
            );
            assert!(text.contains("1 line couldn't be read"), "{text}");
            assert!(
                text.contains("  /new                                     2      0%     50%"),
                "{text}"
            );
            let json = json_report(&stats, &job, AT + 86_400, 60, 10);
            assert_eq!(json["logs"]["requests"], 3);
            assert_eq!(json["now"]["requests_per_second"], 0.033);
            assert_eq!(json["per_minute"].as_array().unwrap().len(), 2);
            assert_eq!(json["status_codes"]["500"], 1);
            assert_eq!(json["paths"][0]["path"], "/new");
        }

        // Following: what is appended is read, and so is the file that
        // takes the path's place
        let job = Job {
            files: vec![new.clone()],
            since: None,
            follow: true,
            ..job
        };
        let shared = start(job);
        let wait = |want: u64| {
            for _ in 0..200 {
                if shared.lock().requests == want {
                    return;
                }
                std::thread::sleep(Duration::from_millis(25));
            }
            panic!("{} requests, not {want}", shared.lock().requests);
        };
        // The line without an end waits for it
        wait(1);
        let mut file = std::fs::OpenOptions::new().append(true).open(&new).unwrap();
        writeln!(file, "\n{}", hit(102, "/appended", 200)).unwrap();
        wait(3);
        std::fs::rename(&new, dir.join("access.log.0")).unwrap();
        writeln!(file, "{}", hit(103, "/before-reopen", 200)).unwrap();
        std::fs::write(&new, format!("{}\n", hit(104, "/rotated", 200))).unwrap();
        wait(5);
        assert!(shared
            .lock()
            .recent
            .back()
            .unwrap()
            .text
            .contains("/rotated"));
        shared.stop();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn every_core_reading_counts_what_one_would() {
        let dir = std::env::temp_dir().join(format!("pepe-sprint-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("access.log");
        // An hour and a half in which seconds have from none to forty
        // requests, so that most stretches cut one in two; error log lines,
        // lines that can't be read, a line longer than a stretch, and bytes
        // that aren't UTF-8
        let mut log = String::new();
        let mut n = 0u64;
        for s in 0..5_400 {
            let here = (s * 7 % 11) * (s % 4) + u64::from(s % 900 == 0) * 10;
            for _ in 0..here {
                n += 1;
                let status = if n % 53 == 0 {
                    502
                } else if n % 17 == 0 {
                    404
                } else {
                    200
                };
                log.push_str(&hit(
                    s as i64,
                    &format!("/p{}/{}?k{}=1", n % 7, n % 3_000, n % 5),
                    status,
                ));
                log.push_str(&format!(" rt=0.{:03}\n", n % 400));
            }
            if s % 97 == 0 {
                let at = civil(AT + s as i64 + i64::from(local_offset()));
                log.push_str(&format!(
                    "{}/{:02}/{:02} {:02}:{:02}:{:02} [error] 1#1: *{s} upstream timed out (110: Connection timed out) while reading, client: 10.0.0.{}\n",
                    at.year, at.month, at.day, at.hour, at.minute, at.second, s % 5
                ));
            }
            if s % 1_300 == 5 {
                log.push_str(&format!("not a line {s}\n"));
            }
            if s == 2_000 {
                log.push_str(&hit(s as i64, &"/long".repeat(1_000), 200));
                log.push('\n');
            }
        }
        let mut bytes = log.into_bytes();
        let at = bytes.len() / 3;
        let line_start = bytes[..at].iter().rposition(|&c| c == b'\n').unwrap() + 1;
        bytes[line_start] = 0xff;
        bytes.extend_from_slice(hit(5_400, "/unfinished", 200).as_bytes());
        std::fs::write(&path, &bytes).unwrap();

        let job = Job {
            files: vec![path.clone()],
            piped: None,
            parser: Parser::default(),
            since: Some(AT + 10),
            follow: false,
            exact_paths: false,
        };
        let wall = AT + 86_400;
        let one = Shared::default();
        Tail::open(&path).unwrap().drain(&job, &one, false);
        let one = one.lock();
        assert!(
            one.requests > 30_000 && one.unread == 5 && one.skipped > 0,
            "{}",
            one.requests
        );
        let expected = json_report(&one, &job, wall, 60, 50);
        let lines = |stats: &Stats| -> Vec<String> {
            stats.recent.iter().map(|line| line.text.clone()).collect()
        };

        for (chunk, threads) in [(1_024, 4), (3_000, 3), (64 * 1024, 8), (1 << 20, 2)] {
            let many = Shared::default();
            let reached = sprint(&path, &job, &many, 0, chunk, threads);
            assert!(
                reached > 0 && reached <= bytes.len() as u64,
                "{chunk}: {reached}"
            );
            assert_eq!(
                bytes[reached as usize - 1],
                b'\n',
                "{chunk}: at a line's start"
            );
            Tail::open_at(&path, reached)
                .unwrap()
                .drain(&job, &many, false);
            let many = many.lock();
            assert_eq!(
                json_report(&many, &job, wall, 60, 50),
                expected,
                "{chunk} × {threads}"
            );
            assert_eq!(many.read_bytes, one.read_bytes, "{chunk}");
            assert_eq!(many.peak, one.peak, "{chunk}");
            assert_eq!(lines(&many), lines(&one), "{chunk}");
            assert_eq!(many.recent_base, one.recent_base, "{chunk}");
            assert_eq!(many.first_unread, one.first_unread, "{chunk}");
            assert!(many.stitched.is_empty());
        }
        // Too small to be worth a second thread
        assert_eq!(sprint(&path, &job, &Shared::default(), 0, 1 << 30, 8), 0);
        assert_eq!(sprint(&path, &job, &Shared::default(), 0, 1_024, 1), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn names_are_found_by_a_keyed_hash() {
        use std::hash::BuildHasher;
        let keyed = Keyed::new();
        // The same key for every map of a run, so counts can be added
        assert_eq!(keyed.hash_one("/a"), Keyed::new().hash_one("/a"));
        let mut seen = std::collections::HashSet::new();
        for i in 0..20_000u32 {
            assert!(
                seen.insert(keyed.hash_one(format!("/items/{i}").as_str())),
                "{i}"
            );
        }
        // Length counts, and so does what is past the last whole word
        for (a, b) in [
            ("", "\0"),
            ("abcdefgh", "abcdefgh\0"),
            ("abcdefghi", "abcdefghj"),
        ] {
            assert_ne!(keyed.hash_one(a), keyed.hash_one(b), "{a:?} {b:?}");
        }
        let mut top: Top<PathStat> = Top::new(2);
        let mut other: Top<PathStat> = Top::new(2);
        for (top, names) in [(&mut top, ["a", "b"]), (&mut other, ["b", "c"])] {
            for name in names {
                top.get(name).unwrap().requests += 2;
            }
        }
        top.absorb(other);
        assert_eq!((top.len(), top.other), (2, 2), "c came after the cap");
        assert_eq!(
            top.top(1, |p| p.requests)[0],
            (
                "b",
                &PathStat {
                    requests: 4,
                    ..Default::default()
                }
            )
        );
    }

    #[test]
    fn a_log_is_taken_up_at_a_time_without_reading_what_is_before() {
        let dir = std::env::temp_dir().join(format!("pepe-seek-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("access.log");
        // Six hours, three requests a second, with lines that have no
        // time and an error log's among them
        let mut log = String::new();
        for s in 0..21_600i64 {
            for i in 0..3 {
                log.push_str(&hit(s, &format!("/p{i}"), 200));
                log.push('\n');
            }
            if s % 500 == 0 {
                log.push_str("no time on this one\n");
            }
        }
        std::fs::write(&path, &log).unwrap();
        let parser = Parser::default();
        assert_eq!(last_stamp(&path, &parser), Some(AT + 21_599));
        assert!(being_written(
            std::slice::from_ref(&path),
            &parser,
            AT + 21_599 + LIVE
        ));
        assert!(!being_written(
            std::slice::from_ref(&path),
            &parser,
            AT + 21_600 + LIVE
        ));
        assert!(!being_written(&[dir.join("none.log")], &parser, AT));

        for back in [1, 300, 3_600, 21_000] {
            let since = AT + 21_600 - back;
            let origin = seek_since(&path, &parser, since) as usize;
            assert!(
                origin == 0 || log.as_bytes()[origin - 1] == b'\n',
                "{back}: a line's start"
            );
            // Every line from `since` on is after it, and little else is
            let first = log.find(&hit(21_600 - back, "/p0", 200)).unwrap();
            assert!(origin <= first, "{back}: {origin} > {first}");
            assert!(
                first - origin <= 2 * 64 * 1024,
                "{back}: {} before",
                first - origin
            );

            let job = Job {
                files: vec![path.clone()],
                piped: None,
                parser: parser.clone(),
                since: Some(since),
                follow: false,
                exact_paths: false,
            };
            let shared = Shared::default();
            read(&job, &shared);
            let stats = shared.lock();
            assert_eq!(stats.requests, back as u64 * 3, "{back}");
            assert_eq!(stats.first.unwrap().at, since, "{back}");
            assert_eq!(stats.since, Some(since));
            assert!(
                stats.skipped < 2_000,
                "{back}: {} read to be left out",
                stats.skipped
            );
            assert_eq!(
                stats.read_bytes,
                log.len() as u64,
                "{back}: all accounted for"
            );
        }
        // Before the log starts, and after it ends
        assert_eq!(seek_since(&path, &parser, AT - 5), 0);
        let end = seek_since(&path, &parser, AT + 99_999) as usize;
        assert!(log.len() - end <= 2 * 64 * 1024, "{}", log.len() - end);
        assert_eq!(seek_since(&dir.join("none.log"), &parser, AT), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
