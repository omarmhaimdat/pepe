//! Drawing. Everything here reads `Dashboard` state and never changes it.

use std::time::Duration;

use gethostname::gethostname;
use ratatui::{
    buffer::Buffer,
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph, Row, Table, Tabs, Wrap},
    Frame,
};

use super::{bigtext, filter, format, mascot, progress_percent, Dashboard, LogEntry, Tab};
use crate::insights::Level;
use crate::load::Plan;
use crate::response::ResponseStats;
use crate::timeline::{bin_floor_us, bin_of, LatencyBins, BINS, BINS_PER_DECADE};
use crate::utils::num_of_cores;

// ─── Palette ─────────────────────────────────────────────────────────────────
// 256-color indexes are stable across themes; the semantic colors use the
// terminal's own green/yellow/red so they match the user's theme.

/// The one accent color: cyan
const ACCENT: Color = Color::Indexed(81);
/// Labels: readable, but quieter than values
const LABEL: Color = Color::Indexed(246);
/// Rules and axes
const RULE: Color = Color::Indexed(239);
const GOOD: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;
/// Heatmap ramp, few requests → many: dark gray to white
const HEAT: [Color; 9] = [
    Color::Indexed(237),
    Color::Indexed(239),
    Color::Indexed(241),
    Color::Indexed(243),
    Color::Indexed(245),
    Color::Indexed(247),
    Color::Indexed(250),
    Color::Indexed(253),
    Color::Indexed(255),
];
/// Cells holding less than this share of a column stay empty, so a few
/// stray requests don't read as a pattern
const HEAT_FLOOR: f64 = 0.002;

/// Smallest terminal the layout is designed for
const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 18;
/// Width at which the Live tab gets its stats column
const STATS_COLUMN_MIN_WIDTH: u16 = 110;
const STATS_COLUMN: u16 = 36;
const PERCENTILES: [(&str, f64); 7] = [
    ("p50", 50.0),
    ("p75", 75.0),
    ("p90", 90.0),
    ("p95", 95.0),
    ("p99", 99.0),
    ("p99.9", 99.9),
    ("p99.99", 99.99),
];

pub fn render(d: &Dashboard, f: &mut Frame) {
    let area = f.area();
    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        let msg = format!(
            "Terminal too small: {}×{} (need {MIN_WIDTH}×{MIN_HEIGHT})",
            area.width, area.height
        );
        f.render_widget(
            Paragraph::new(msg).alignment(Alignment::Center).fg(WARN),
            center(area, area.width, 1),
        );
        return;
    }

    let [header, _, tabs, _, body, footer] = Layout::vertical([
        Constraint::Length(header_height(d)),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    render_header(d, f, header);
    render_tabs(d, f, tabs);
    match d.tab {
        Tab::Live => render_live(d, f, body),
        Tab::Stats => render_stats_tab(d, f, body),
        Tab::Requests => render_requests_tab(d, f, body),
    }
    render_footer(d, f, footer);

    if d.show_help {
        render_help(f, area);
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn label(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(LABEL))
}

fn value(text: impl Into<String>, color: Color) -> Span<'static> {
    Span::styled(text.into(), Style::new().fg(color).bold())
}

fn heading(text: &str) -> Line<'static> {
    Line::from(Span::styled(
        text.to_uppercase(),
        Style::new().fg(LABEL).bold(),
    ))
}

/// Section title followed by a rule to the edge: "LATENCY ────────"
fn section(f: &mut Frame, area: Rect, title: &str, right: Option<Line<'static>>) {
    let title = format!("{} ", title.to_uppercase());
    let right_width = right.as_ref().map_or(0, |r| r.width() + 1);
    let rule = (area.width as usize).saturating_sub(title.chars().count() + right_width);
    let mut spans = vec![
        Span::styled(title, Style::new().fg(LABEL).bold()),
        Span::styled("─".repeat(rule), Style::new().fg(RULE)),
    ];
    if let Some(right) = right {
        spans.push(Span::raw(" "));
        spans.extend(right.spans);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// `label        value` row, value right-aligned to `width`
fn kv(name: &str, val: String, color: Color, width: usize) -> Line<'static> {
    let pad = width.saturating_sub(name.chars().count() + val.chars().count());
    Line::from(vec![
        label(name.to_string()),
        Span::raw(" ".repeat(pad)),
        value(val, color),
    ])
}

fn status_color(code: u16) -> Color {
    match code {
        100..=199 => Color::Blue,
        200..=299 => GOOD,
        300..=399 => Color::Cyan,
        400..=499 => WARN,
        _ => BAD,
    }
}

fn status_span(stat: &ResponseStats) -> Span<'static> {
    match (stat.status_code, stat.error) {
        (Some(code), _) => value(code.as_u16().to_string(), status_color(code.as_u16())),
        (None, error) => value(error.map(|e| e.label()).unwrap_or("ERROR"), BAD),
    }
}

fn level_color(level: Level) -> Color {
    match level {
        Level::Healthy => GOOD,
        Level::Degraded => WARN,
        Level::Failing => BAD,
    }
}

/// Horizontal bar with 1/8-cell resolution
fn bar(fraction: f64, width: usize) -> String {
    const PARTIAL: [char; 8] = [' ', '▏', '▎', '▍', '▌', '▋', '▊', '▉'];
    let eighths = (fraction.clamp(0.0, 1.0) * width as f64 * 8.0).round() as usize;
    let mut out = "█".repeat(eighths / 8);
    if eighths % 8 > 0 {
        out.push(PARTIAL[eighths % 8]);
    }
    out
}

/// Green → yellow → red as `i` goes from 0 to `n - 1`
fn heat(i: usize, n: usize) -> Color {
    let t = i as f64 / n.saturating_sub(1).max(1) as f64;
    match t {
        t if t < 0.45 => GOOD,
        t if t < 0.8 => WARN,
        _ => BAD,
    }
}

fn center(area: Rect, width: u16, height: u16) -> Rect {
    let [area] = Layout::horizontal([Constraint::Length(width.min(area.width))])
        .flex(Flex::Center)
        .areas(area);
    let [area] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(area);
    area
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

fn placeholder(f: &mut Frame, area: Rect, message: &str) {
    f.render_widget(
        Paragraph::new(Span::styled(
            message.to_string(),
            Style::new().fg(LABEL).italic(),
        ))
        .alignment(Alignment::Center),
        center(area, area.width, 1),
    );
}

// ─── Header: mascot, title, progress, headline numbers or verdict ────────────

/// Findings shown in the header once the run is over
const MAX_HEADER_NOTES: usize = 4;

fn header_height(d: &Dashboard) -> u16 {
    let content = match &d.verdict {
        // title, progress, gap, headline, gap, notes
        Some(v) => 5 + v.notes.len().min(MAX_HEADER_NOTES) as u16,
        // title, progress, gap, labels, three rows of digits
        None => 7,
    };
    // The mascot plus its speech line
    content.max(mascot::HEIGHT + 1)
}

fn render_header(d: &Dashboard, f: &mut Frame, area: Rect) {
    let show_mascot = area.width >= 80;
    let [pet, _, main] = Layout::horizontal([
        Constraint::Length(if show_mascot { mascot::WIDTH } else { 0 }),
        Constraint::Length(if show_mascot { 2 } else { 0 }),
        Constraint::Min(0),
    ])
    .areas(area);

    if show_mascot {
        let mood = d.mood();
        let mut lines = mascot::lines(mood, d.frame);
        lines.push(Line::styled(mood.says(), Style::new().fg(ACCENT).italic()));
        f.render_widget(Paragraph::new(lines), pet);
    }

    let [title, progress, _, rest] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(main);
    render_title(d, f, title);
    render_progress(d, f, progress);
    match &d.verdict {
        Some(_) => render_verdict(d, f, rest),
        None => render_hero(d, f, rest),
    }
}

fn render_title(d: &Dashboard, f: &mut Frame, area: Rect) {
    let (state, color) = if d.interrupted {
        ("■ stopped", BAD)
    } else if d.finished.is_some() {
        ("✔ done", GOOD)
    } else if d.paused {
        ("‖ paused", WARN)
    } else {
        ("● running", GOOD)
    };
    let right = Line::from(vec![
        value(state, color),
        label("   concurrency "),
        value(d.concurrency.to_string(), Color::Reset),
        label("   "),
        value(format::clock(d.elapsed()), Color::Reset),
    ]);

    let prefix = format!("pepe  {} ", d.args.method);
    let room = (area.width as usize).saturating_sub(right.width() + prefix.chars().count() + 2);
    let left = Line::from(vec![
        value("pepe", ACCENT),
        Span::raw("  "),
        value(d.args.method.clone(), Color::Magenta),
        Span::raw(" "),
        Span::raw(truncate(&d.args.url, room)),
    ]);
    f.render_widget(Paragraph::new(left), area);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
}

fn render_progress(d: &Dashboard, f: &mut Frame, area: Rect) {
    let m = &d.metrics;
    let elapsed = d.elapsed();
    let finished = d.finished.is_some();
    let percent = progress_percent(d.plan, m.total, elapsed, finished);

    let detail = match d.plan {
        Plan::Count(n) => {
            let mut s = format!("{} of {}", format::count(m.total), format::count(n));
            let rate = m.rps(elapsed);
            if !finished && rate > 0.0 {
                let eta = n.saturating_sub(m.total) as f64 / rate;
                s += &format!(
                    " · {} left",
                    format::span(Duration::from_secs_f64(eta.ceil()))
                );
            }
            s
        }
        Plan::Duration(total) => {
            let mut s = format!(
                "{} of {}",
                format::span(elapsed.min(total)),
                format::span(total)
            );
            if !finished {
                s += &format!(" · {} left", format::span(total.saturating_sub(elapsed)));
            }
            s
        }
    };
    let color = if d.interrupted {
        BAD
    } else if finished {
        GOOD
    } else if d.paused {
        WARN
    } else {
        ACCENT
    };

    let tail = format!("  {percent}%  {detail}");
    let bar_width = (area.width as usize).saturating_sub(tail.chars().count());
    let filled = bar_width * percent as usize / 100;
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("━".repeat(filled), Style::new().fg(color)),
            Span::styled("━".repeat(bar_width - filled), Style::new().fg(RULE)),
            value(format!("  {percent}%"), color),
            label(format!("  {detail}")),
        ])),
        area,
    );
}

/// The four numbers that matter, in big digits when they fit
fn render_hero(d: &Dashboard, f: &mut Frame, area: Rect) {
    let m = &d.metrics;
    let current_rps = match d.timeline.last() {
        Some(s) => s.rps,
        None => m.rps(d.elapsed()),
    };
    let success = 100.0 - m.error_rate();
    let (success_text, success_color) = match m.total {
        0 => ("—".to_string(), LABEL),
        _ => (
            format!("{success:.1}%"),
            match success {
                s if s >= 99.0 => GOOD,
                s if s >= 95.0 => WARN,
                _ => BAD,
            },
        ),
    };
    let heroes = [
        ("req/s", format::compact(current_rps), ACCENT),
        ("p50", format::latency(m.percentile(50.0)), GOOD),
        ("p99", format::latency(m.percentile(99.0)), WARN),
        ("success", success_text, success_color),
    ];

    let widths: Vec<usize> = heroes
        .iter()
        .map(|(name, text, _)| {
            let (num, unit) = bigtext::split_unit(text);
            (bigtext::width(num) + unit.chars().count() + 1).max(name.len())
        })
        .collect();
    let gap = 5;
    let big = widths.iter().sum::<usize>() + gap * heroes.len() <= area.width as usize
        && area.height as usize > bigtext::HEIGHT;

    let columns = Layout::horizontal(widths.iter().map(|&w| {
        Constraint::Length(if big {
            (w + gap) as u16
        } else {
            area.width / heroes.len() as u16
        })
    }))
    .split(area);
    for ((name, text, color), column) in heroes.into_iter().zip(columns.iter()) {
        let mut lines = vec![Line::from(label(name.to_uppercase()))];
        if big {
            let (num, unit) = bigtext::split_unit(&text);
            let unit = if num.is_empty() { text.as_str() } else { unit };
            lines.extend(bigtext::lines(
                num,
                unit,
                Style::new().fg(color),
                Style::new().fg(color).bold(),
            ));
        } else {
            lines.push(Line::from(value(text, color)));
        }
        f.render_widget(Paragraph::new(lines), *column);
    }
}

/// End-of-run report card
fn render_verdict(d: &Dashboard, f: &mut Frame, area: Rect) {
    let Some(verdict) = &d.verdict else { return };
    let color = level_color(verdict.level);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!(" {} {} ", verdict.level.symbol(), verdict.level.headline()),
                Style::new().fg(Color::Black).bg(color).bold(),
            ),
            Span::raw("  "),
            Span::styled(
                fit_parts(&d.summary_parts(), area.width.saturating_sub(16) as usize),
                Style::new().bold(),
            ),
        ]),
        Line::raw(""),
    ];
    for note in verdict.notes.iter().take(MAX_HEADER_NOTES) {
        lines.push(Line::from(vec![
            Span::styled(
                format!(" {} ", note.level.symbol()),
                Style::new().fg(level_color(note.level)),
            ),
            Span::raw(note.text.clone()),
        ]));
    }
    f.render_widget(Paragraph::new(lines), area);
}

/// Join as many leading `parts` as fit in `width`
fn fit_parts(parts: &[String], width: usize) -> String {
    let mut out = String::new();
    for part in parts {
        let extra = if out.is_empty() { 0 } else { 3 };
        if out.chars().count() + extra + part.chars().count() > width {
            break;
        }
        if !out.is_empty() {
            out.push_str(" · ");
        }
        out.push_str(part);
    }
    out
}

fn render_tabs(d: &Dashboard, f: &mut Frame, area: Rect) {
    let titles = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, t)| Line::from(format!("{} {}", i + 1, t.title().to_uppercase())));
    f.render_widget(
        Tabs::new(titles)
            .select(d.tab.index())
            .style(Style::new().fg(LABEL))
            .highlight_style(Style::new().fg(Color::Black).bg(ACCENT).bold())
            .divider(" ")
            .padding(" ", " "),
        area,
    );
}

fn key_hints(pairs: &[(&'static str, &'static str)]) -> Vec<Span<'static>> {
    let mut spans = Vec::with_capacity(pairs.len() * 2);
    for (key, action) in pairs {
        spans.push(Span::styled(
            format!(" {key} "),
            Style::new().fg(ACCENT).bold(),
        ));
        spans.push(label(format!("{action} ")));
    }
    spans
}

fn render_footer(d: &Dashboard, f: &mut Frame, area: Rect) {
    let mut hints: Vec<(&str, &str)> = vec![("q", "quit"), ("r", "restart")];
    if d.finished.is_none() {
        hints.push(("space", if d.paused { "resume" } else { "pause" }));
        hints.push(("+/-", "concurrency"));
        hints.push(("s", "stop"));
    }
    if d.filter.editing {
        hints = vec![("enter", "done"), ("esc", "clear search")];
    } else if d.inspecting && d.tab == Tab::Requests {
        hints = vec![
            ("esc", "back"),
            ("↑↓", "newer / older"),
            ("PgUp/PgDn", "scroll response"),
            ("[ ]", "nearest kept in full"),
            ("g/G", "newest / oldest"),
            ("q", "quit"),
        ];
    } else if d.tab == Tab::Requests {
        hints.push(("↑↓", "select"));
        hints.push(("enter", "inspect"));
        hints.push(("f", "status"));
        hints.push(("l", "latency"));
        hints.push(("/", "search"));
        if d.filter.is_active() {
            hints.push(("c", "clear"));
        }
    } else {
        hints.push(("tab", "view"));
    }
    hints.push(("?", "help"));
    f.render_widget(Paragraph::new(Line::from(key_hints(&hints))), area);

    if let Some((notice, _)) = &d.notice {
        f.render_widget(
            Paragraph::new(Span::styled(
                format!(" {notice} "),
                Style::new().fg(Color::Black).bg(WARN).bold(),
            ))
            .alignment(Alignment::Right),
            area,
        );
    }
}

fn render_help(f: &mut Frame, area: Rect) {
    let rows: [(&str, &str); 17] = [
        ("space / p", "pause or resume sending"),
        ("+ / -", "raise or lower concurrency by ~10%"),
        ("s / i", "stop the run, keep the results"),
        ("r", "restart with the same settings"),
        ("tab / ← →", "switch view"),
        ("1 2 3", "live, stats, requests"),
        ("↑ ↓ / j k", "select a request (newer / older)"),
        ("enter", "inspect it: stats, request, full response"),
        ("[ ]", "inspector: nearest request kept in full"),
        ("f", "filter requests by status"),
        ("l", "filter requests by latency (slow ones)"),
        ("/", "search status and response text"),
        ("PgUp PgDn", "scroll faster"),
        ("g / G", "newest / oldest request"),
        ("e / c", "failed requests only / clear filters"),
        ("?", "close this help"),
        ("q / esc", "quit"),
    ];
    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(key, action)| {
            Line::from(vec![
                Span::styled(format!("  {key:<12}"), Style::new().fg(ACCENT).bold()),
                Span::raw(*action),
            ])
        })
        .collect();
    lines.extend([
        Line::raw(""),
        Line::from(label(
            "  Heatmap: each column is a slice of the run. Brighter",
        )),
        Line::from(label("  cells mean more requests took that long.")),
        Line::raw(""),
        Line::from(label(format!(
            "  pepe {} · {}/{} · {} cores · {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            num_of_cores(),
            gethostname().to_string_lossy()
        ))),
    ]);

    let popup = center(area, 60, lines.len() as u16 + 2);
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(ACCENT))
                .title(Span::styled(" keys ", Style::new().fg(ACCENT).bold())),
        ),
        popup,
    );
}

// ─── Live: latency heatmap and lines, throughput, latest, errors ─────────────

/// Left axis and right gutter widths around the time charts
const AXIS: u16 = 8;
const GUTTER: u16 = 13;
/// While a run is going, the time axis spans at least this many seconds, so
/// the first seconds don't stretch across the whole width
const MIN_SPAN_SECS: usize = 30;
/// Latency lines: name, value, color
type LatencyLine = (&'static str, fn(&ColumnLatency) -> f64, Color);
const LINES: [LatencyLine; 3] = [
    ("p50", |l| l.p50, ACCENT),
    ("p90", |l| l.p90, Color::Indexed(250)),
    ("p99", |l| l.p99, WARN),
];

fn render_live(d: &Dashboard, f: &mut Frame, body: Rect) {
    let with_stats = body.width >= STATS_COLUMN_MIN_WIDTH;
    let [left, _, stats] = Layout::horizontal([
        Constraint::Min(0),
        Constraint::Length(if with_stats { 3 } else { 0 }),
        Constraint::Length(if with_stats { STATS_COLUMN } else { 0 }),
    ])
    .areas(body);

    // The charts share one time axis and take a fixed share of the height,
    // leaving the rest to the latest requests and errors
    let h = left.height;
    let heat_rows = (h / 5).clamp(3, 8);
    let line_rows = (h / 7).clamp(3, 6);
    let rps_rows = if h >= 30 { 4 } else { 3 };
    let [heat_title, heat, line_title, lines, rps_title, rps, axis, _, bottom] =
        Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(heat_rows),
            Constraint::Length(1),
            Constraint::Length(line_rows),
            Constraint::Length(1),
            Constraint::Length(rps_rows),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .areas(left);

    // Heatmap legend: the ramp itself, one cell per shade
    let ramp: Vec<Span> = HEAT
        .iter()
        .map(|&c| Span::styled("█", Style::new().fg(c)))
        .collect();
    let mut legend = vec![label("fewer ")];
    legend.extend(ramp);
    legend.push(label(" more"));
    section(
        f,
        heat_title,
        "latency distribution",
        Some(Line::from(legend)),
    );

    let mut line_legend = Vec::new();
    for (name, _, color) in LINES {
        line_legend.push(Span::styled("━ ", Style::new().fg(color)));
        line_legend.push(label(format!("{name} ")));
    }
    section(
        f,
        line_title,
        "latency percentiles",
        Some(Line::from(line_legend)),
    );
    section(
        f,
        rps_title,
        "throughput",
        Some(throughput_detail(
            d,
            rps_title.width.saturating_sub(16) as usize,
        )),
    );

    if d.timeline.samples().is_empty() {
        placeholder(f, heat, "collecting the first second…");
    } else {
        let columns = Columns::new(d, heat.width.saturating_sub(AXIS + GUTTER));
        render_heatmap(d, &columns, f.buffer_mut(), heat);
        render_latency_lines(&columns, f.buffer_mut(), lines);
        render_throughput(&columns, f.buffer_mut(), rps);
        render_time_axis(&columns, f, axis);
    }

    if bottom.height >= 4 {
        let [latest, _, errors] = Layout::horizontal([
            Constraint::Fill(3),
            Constraint::Length(3),
            Constraint::Fill(2),
        ])
        .areas(bottom);
        render_latest(d, f, latest);
        render_errors(d, f, errors);
    }

    if with_stats {
        render_stats_column(d, f, stats);
    }
}

/// "now 7.2k req/s · 1.9 MiB/s · 1.1 KiB avg body · 16 in flight", keeping
/// only the leading items that fit in `width`
fn throughput_detail(d: &Dashboard, width: usize) -> Line<'static> {
    let m = &d.metrics;
    let now = d.timeline.last().map_or(0.0, |s| s.rps);
    let avg_body = if m.total > 0 {
        m.bytes as f64 / m.total as f64
    } else {
        0.0
    };
    let items = [
        vec![
            label("now "),
            value(format!("{} req/s", format::compact(now)), ACCENT),
        ],
        vec![value(
            format!("{}/s", format::bytes(m.throughput(d.elapsed()))),
            Color::Reset,
        )],
        vec![
            value(format::bytes(avg_body), Color::Reset),
            label(" avg body"),
        ],
        vec![
            value(format::count(d.sent.saturating_sub(m.total)), Color::Reset),
            label(" in flight"),
        ],
    ];
    let mut spans: Vec<Span> = Vec::new();
    let mut used = 0;
    for item in items {
        let sep = if spans.is_empty() { 0 } else { 3 };
        let w: usize = item.iter().map(|s| s.width()).sum();
        if used + sep + w > width {
            break;
        }
        if sep > 0 {
            spans.push(label(" · "));
        }
        spans.extend(item);
        used += sep + w;
    }
    Line::from(spans)
}

/// Latency lines for one column, in milliseconds (None: no requests)
struct ColumnLatency {
    p50: f64,
    p90: f64,
    p99: f64,
}

/// Timeline samples grouped into chart columns. While a run is going the
/// axis covers its planned length, so the chart fills in from the left.
struct Columns {
    /// Latency bins per column with data
    bins: Vec<LatencyBins>,
    rps: Vec<f64>,
    errors: Vec<f64>,
    latency: Vec<Option<ColumnLatency>>,
    /// Columns the axis has room for, including ones still to come
    slots: usize,
    /// Cells available for the columns
    width: u16,
    /// Seconds covered by the axis
    span: (f64, f64),
}

impl Columns {
    fn new(d: &Dashboard, width: u16) -> Self {
        let samples = d.timeline.samples();
        let bins = d.timeline.bins();
        let n = samples.len();
        let seconds = if d.finished.is_some() {
            n
        } else {
            match d.plan {
                Plan::Duration(t) => n.max(t.as_secs() as usize).min(n.max(600)),
                Plan::Count(_) => n.max(MIN_SPAN_SECS),
            }
        };
        let width = (width as usize).max(1);
        let group = seconds.div_ceil(width).max(1);
        let start = samples.front().map_or(0.0, |s| s.at - 1.0).max(0.0);

        let mut out = Columns {
            bins: Vec::new(),
            rps: Vec::new(),
            errors: Vec::new(),
            latency: Vec::new(),
            slots: seconds.div_ceil(group).max(1),
            width: width as u16,
            span: (start, start + seconds as f64),
        };
        for first in (0..n).step_by(group) {
            let end = (first + group).min(n);
            let mut merged = [0u32; BINS];
            for column in bins.range(first..end) {
                for (m, c) in merged.iter_mut().zip(column) {
                    *m += c;
                }
            }
            let len = (end - first) as f64;
            let group_samples = || samples.range(first..end);
            out.bins.push(merged);
            out.rps
                .push(group_samples().map(|s| s.rps).sum::<f64>() / len);
            out.errors
                .push(group_samples().map(|s| s.errors).sum::<f64>() / len);
            // Average the seconds that had requests
            let busy: Vec<_> = group_samples().filter(|s| s.rps > 0.0).collect();
            out.latency.push((!busy.is_empty()).then(|| {
                let avg = |pick: fn(&crate::timeline::Sample) -> f64| {
                    busy.iter().map(|s| pick(s)).sum::<f64>() / busy.len() as f64
                };
                ColumnLatency {
                    p50: avg(|s| s.p50_ms),
                    p90: avg(|s| s.p90_ms),
                    p99: avg(|s| s.p99_ms),
                }
            }));
        }
        out
    }

    /// Cells of `area` a column covers; columns share the axis evenly
    fn cells(&self, area: Rect, column: usize) -> std::ops::Range<u16> {
        let edge = |c: usize| area.x + AXIS + (c * self.width as usize / self.slots) as u16;
        edge(column)..edge(column + 1).max(edge(column) + 1)
    }
}

/// Left axis: a rule, with labels at the top and bottom rows
fn draw_axis(buf: &mut Buffer, area: Rect, top: &str, bottom: &str) {
    for row in 0..area.height {
        buf.set_string(area.x + AXIS - 2, area.y + row, "│", Style::new().fg(RULE));
    }
    let dim = Style::new().fg(LABEL);
    buf.set_stringn(area.x, area.y, format!("{top:>6} ┤"), AXIS as usize, dim);
    buf.set_stringn(
        area.x,
        area.bottom() - 1,
        format!("{bottom:>6} ┤"),
        AXIS as usize,
        dim,
    );
}

/// Time × latency heatmap. Two latency rows per cell using half blocks; each
/// column is normalized on its own, so the shape of the distribution shows
/// whatever the request rate.
fn render_heatmap(d: &Dashboard, columns: &Columns, buf: &mut Buffer, area: Rect) {
    let pixels = area.height as usize * 2;

    // Latency range with data, padded by a bin, at least half a decade tall
    let mut lo = BINS;
    let mut hi = 0;
    for bins in &columns.bins {
        if let Some(first) = bins.iter().position(|&n| n > 0) {
            lo = lo.min(first);
            hi = hi.max(bins.iter().rposition(|&n| n > 0).unwrap_or(first));
        }
    }
    if lo > hi {
        return;
    }
    // A handful of outliers shouldn't squash everything else: cap the range a
    // little above p99.9 and pile anything slower into the top row
    let p999 = d.metrics.percentile(99.9).as_micros() as f64;
    let p99 = d.metrics.percentile(99.0).as_micros() as u64;
    let cap = bin_of((p999 * 1.5) as u64).max(bin_of(p99) + 2);
    let capped = hi > cap;
    let hi = hi.min(cap);
    let extra = (BINS_PER_DECADE / 2).saturating_sub(hi - lo + 1);
    let lo = lo.saturating_sub(1 + extra / 2);
    let hi = (hi + 2 + extra - extra / 2).min(BINS);
    let span = (hi - lo) as f64;

    let pixel_bins = |p: usize| {
        let b0 = lo + (p as f64 * span / pixels as f64) as usize;
        let b1 = (lo + ((p + 1) as f64 * span / pixels as f64) as usize).max(b0 + 1);
        if p == pixels - 1 {
            b0..BINS
        } else {
            b0..b1.min(BINS)
        }
    };

    for (c, bins) in columns.bins.iter().enumerate() {
        let values: Vec<u32> = (0..pixels)
            .map(|p| bins[pixel_bins(p)].iter().sum())
            .collect();
        let peak = values.iter().copied().max().unwrap_or(0);
        let total: u32 = values.iter().sum();
        if peak == 0 {
            continue;
        }
        // Square-root scale: dense bands stand out, sparse cells stay dim
        let shade = |v: u32| -> Option<Color> {
            (v > 0 && v as f64 >= total as f64 * HEAT_FLOOR).then(|| {
                let t = (v as f64 / peak as f64).sqrt();
                HEAT[((t * (HEAT.len() - 1) as f64).round() as usize).min(HEAT.len() - 1)]
            })
        };
        for row in 0..area.height as usize {
            let upper = shade(values[pixels - 1 - 2 * row]);
            let lower = shade(values[pixels - 2 - 2 * row]);
            for x in columns.cells(area, c) {
                let Some(cell) = buf.cell_mut((x, area.y + row as u16)) else {
                    continue;
                };
                match (upper, lower) {
                    (Some(u), Some(l)) => cell.set_char('▀').set_fg(u).set_bg(l),
                    (Some(u), None) => cell.set_char('▀').set_fg(u),
                    (None, Some(l)) => cell.set_char('▄').set_fg(l),
                    (None, None) => cell,
                };
            }
        }
    }

    let top = format::latency_short(bin_floor_us(hi) as u64);
    let top = if capped { format!("≥{top}") } else { top };
    draw_axis(
        buf,
        area,
        &top,
        &format::latency_short(bin_floor_us(lo) as u64),
    );

    // Run-wide p50 and p99 marked in the right gutter
    let row_of = |us: f64| -> u16 {
        let p = (bin_of(us as u64).saturating_sub(lo) as f64 / span * pixels as f64) as u16;
        area.height.saturating_sub(1).saturating_sub(p / 2)
    };
    let gutter = area.right().saturating_sub(GUTTER) + 1;
    let mark = |buf: &mut Buffer, row: u16, name: &str, us: f64, color: Color| {
        let text = format!("◂ {name} {}", format::latency_short(us as u64));
        buf.set_stringn(
            gutter,
            area.y + row,
            text,
            GUTTER as usize - 1,
            Style::new().fg(color).bold(),
        );
    };
    let p50 = d.metrics.percentile(50.0).as_micros() as f64;
    let p99 = p99 as f64;
    let (r50, r99) = (row_of(p50), row_of(p99));
    mark(buf, r99, "p99", p99, WARN);
    if r50 != r99 {
        mark(buf, r50, "p50", p50, ACCENT);
    }
}

/// p50, p90 and p99 over time as braille lines on a log scale
fn render_latency_lines(columns: &Columns, buf: &mut Buffer, area: Rect) {
    let known: Vec<&ColumnLatency> = columns.latency.iter().flatten().collect();
    if known.is_empty() || area.height == 0 {
        return;
    }
    let lo = known
        .iter()
        .map(|l| l.p50)
        .fold(f64::MAX, f64::min)
        .max(0.001)
        * 0.8;
    let hi = known
        .iter()
        .map(|l| l.p99)
        .fold(0.0, f64::max)
        .max(lo * 2.0)
        * 1.2;
    let (width, height) = (columns.width as usize * 2, area.height as usize * 4);
    let dot_row = |ms: f64| -> usize {
        let t = ((ms.max(lo)).ln() - lo.ln()) / (hi.ln() - lo.ln());
        height - 1 - ((t * (height - 1) as f64).round() as usize).min(height - 1)
    };

    // Braille dots per cell, and which line owns each cell (highest wins)
    let cols = columns.width as usize;
    let mut bits = vec![0u32; cols * area.height as usize];
    let mut owner = vec![None::<usize>; cols * area.height as usize];
    for (line, (_, pick, _)) in LINES.iter().enumerate() {
        let mut prev: Option<usize> = None;
        for (c, latency) in columns.latency.iter().enumerate() {
            let Some(latency) = latency else {
                prev = None;
                continue;
            };
            let y = dot_row(pick(latency));
            let cells = columns.cells(area, c);
            let x0 = (cells.start - area.x - AXIS) as usize * 2;
            let x1 = ((cells.end - area.x - AXIS) as usize * 2).min(width);
            for x in x0..x1 {
                // Join a step from the previous column with a vertical run
                let (a, b) = match (x == x0, prev) {
                    (true, Some(p)) => (p.min(y), p.max(y)),
                    _ => (y, y),
                };
                for yy in a..=b {
                    let i = (yy / 4) * cols + x / 2;
                    bits[i] |= braille_bit(x % 2, yy % 4);
                    owner[i] = Some(owner[i].map_or(line, |o| o.max(line)));
                }
            }
            prev = Some(y);
        }
    }
    for row in 0..area.height as usize {
        for col in 0..cols {
            let i = row * cols + col;
            if let (Some(line), true) = (owner[i], bits[i] != 0) {
                if let Some(cell) = buf.cell_mut((area.x + AXIS + col as u16, area.y + row as u16))
                {
                    cell.set_char(char::from_u32(0x2800 + bits[i]).unwrap_or(' '))
                        .set_fg(LINES[line].2);
                }
            }
        }
    }

    let us = |ms: f64| format::latency_short((ms * 1000.0) as u64);
    draw_axis(buf, area, &us(hi), &us(lo));

    // Latest values in the gutter, nudged apart when they'd share a row
    let Some(last) = columns.latency.iter().rev().flatten().next() else {
        return;
    };
    let gutter = area.right().saturating_sub(GUTTER) + 1;
    let mut taken = Vec::new();
    for (name, pick, color) in LINES.iter().rev() {
        let mut row = (dot_row(pick(last)) / 4) as u16;
        while taken.contains(&row) && row + 1 < area.height {
            row += 1;
        }
        taken.push(row);
        buf.set_stringn(
            gutter,
            area.y + row,
            format!("◂ {name} {}", us(pick(last))),
            GUTTER as usize - 1,
            Style::new().fg(*color).bold(),
        );
    }
}

/// Braille bit for the dot at (column, row) within a cell
fn braille_bit(dx: usize, dy: usize) -> u32 {
    match (dx, dy) {
        (0, 3) => 0x40,
        (1, 3) => 0x80,
        (0, dy) => 1 << dy,
        (_, dy) => 1 << (dy + 3),
    }
}

/// Filled area chart of requests per second, aligned with the heatmap
fn render_throughput(columns: &Columns, buf: &mut Buffer, area: Rect) {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let peak = columns.rps.iter().copied().fold(0.0, f64::max);
    if peak <= 0.0 {
        return;
    }
    let steps = area.height as usize * 8;
    for (c, (&rps, &errors)) in columns.rps.iter().zip(&columns.errors).enumerate() {
        let level = ((rps / peak) * steps as f64).round() as usize;
        // A dim fill with a bright top edge; red where requests failed
        let failing = rps > 0.0 && errors / rps >= 0.01;
        let (edge, fill_color) = if failing {
            (BAD, Color::Indexed(52))
        } else {
            (ACCENT, Color::Indexed(238))
        };
        let top_row = steps.saturating_sub(level) / 8;
        for row in 0..area.height as usize {
            let from_bottom = area.height as usize - 1 - row;
            let fill = level.saturating_sub(from_bottom * 8).min(8);
            if fill == 0 {
                continue;
            }
            let color = if row == top_row { edge } else { fill_color };
            for x in columns.cells(area, c) {
                if let Some(cell) = buf.cell_mut((x, area.y + row as u16)) {
                    cell.set_char(LEVELS[fill - 1]).set_fg(color);
                }
            }
        }
    }
    draw_axis(buf, area, &format::compact(peak), "0");
    let gutter = area.right().saturating_sub(GUTTER) + 1;
    buf.set_stringn(
        gutter,
        area.y,
        "◂ peak",
        GUTTER as usize - 1,
        Style::new().fg(LABEL),
    );
}

fn render_time_axis(columns: &Columns, f: &mut Frame, area: Rect) {
    let axis = Rect {
        x: area.x + AXIS,
        width: columns.width.min(area.width.saturating_sub(AXIS + GUTTER)),
        ..area
    };
    let secs = |s: f64| format::span(Duration::from_secs_f64(s.max(0.0)));
    f.render_widget(Paragraph::new(label(secs(columns.span.0))), axis);
    f.render_widget(
        Paragraph::new(label(secs(columns.span.1))).alignment(Alignment::Right),
        axis,
    );
}

/// What a request returned, or why it failed, on one line
fn outcome_text(stat: &ResponseStats) -> String {
    stat.preview_text()
        .or_else(|| stat.error_message.as_deref().map(str::to_string))
        .unwrap_or_default()
}

/// The newest requests, one per row
fn render_latest(d: &Dashboard, f: &mut Frame, area: Rect) {
    let [title, rows] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    section(
        f,
        title,
        "latest requests",
        Some(Line::from(label("3 for all"))),
    );
    if d.log.is_empty() {
        return placeholder(f, rows, "no responses yet");
    }
    let lines: Vec<Line> = d
        .log
        .iter()
        .rev()
        .take(rows.height as usize)
        .map(|e| {
            let failed = e.is_error();
            Line::from(vec![
                Span::raw(format!("{:<12}", status_span(&e.stat).content))
                    .style(status_span(&e.stat).style),
                Span::raw(format!("{:>9}", format::latency(e.stat.duration))),
                label(format!(" {:>9}  ", format::bytes(e.stat.body_bytes as f64))),
                Span::styled(
                    outcome_text(&e.stat),
                    Style::new().fg(if failed { BAD } else { LABEL }),
                ),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), rows);
}

/// Failures grouped by kind, and what the latest one said
fn render_errors(d: &Dashboard, f: &mut Frame, area: Rect) {
    let m = &d.metrics;
    let failed = m.total - m.success;
    let [title, rows] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let summary = if failed == 0 {
        Line::from(value("none", GOOD))
    } else {
        Line::from(vec![
            value(format::count(failed), BAD),
            label(format!(" · {:.1}%", m.error_rate())),
        ])
    };
    section(f, title, "errors", Some(summary));
    if failed == 0 {
        let msg = if m.total == 0 {
            "no responses yet"
        } else {
            "no errors so far"
        };
        return placeholder(f, rows, msg);
    }

    let mut kinds: Vec<(String, u64)> = m
        .status_codes
        .iter()
        .filter(|(code, _)| !(200..300).contains(*code))
        .map(|(code, &n)| (format!("HTTP {code}"), n))
        .collect();
    kinds.push(("timeout".into(), m.timeouts));
    kinds.push(("connection".into(), m.errors));
    kinds.retain(|(_, n)| *n > 0);
    kinds.sort_by_key(|(_, n)| std::cmp::Reverse(*n));

    let w = rows.width as usize;
    let count_width = kinds
        .iter()
        .map(|k| format::count(k.1).len())
        .max()
        .unwrap_or(1);
    let bar_width = w.saturating_sub(11 + count_width + 8);
    let mut lines: Vec<Line> = kinds
        .into_iter()
        .map(|(name, n)| {
            let share = n as f64 / failed as f64;
            Line::from(vec![
                value(format!("{name:<11}"), BAD),
                Span::styled(
                    format!("{:<bar_width$}", bar(share, bar_width)),
                    Style::new().fg(BAD),
                ),
                Span::raw(format!(" {:>count_width$}", format::count(n))),
                label(format!(" {:>5.1}%", share * 100.0)),
            ])
        })
        .collect();

    if let Some(last) = d.error_log.back() {
        lines.push(Line::raw(""));
        lines.push(Line::from(label("latest failure")));
        let what = match last.stat.status_code {
            Some(code) => format!("HTTP {} · {}", code.as_u16(), outcome_text(&last.stat)),
            None => outcome_text(&last.stat),
        };
        lines.push(Line::from(Span::raw(truncate(&what, w))));
    }
    f.render_widget(Paragraph::new(lines), rows);
}

/// Everything worth knowing at a glance, as label/value rows
fn render_stats_column(d: &Dashboard, f: &mut Frame, area: Rect) {
    let m = &d.metrics;
    let w = area.width as usize;
    let elapsed = d.elapsed();
    let in_flight = d.sent.saturating_sub(m.total);
    let peak = d
        .timeline
        .samples()
        .iter()
        .map(|s| s.rps)
        .fold(0.0, f64::max);
    let now = d.timeline.last().map_or(0.0, |s| s.rps);
    let quiet = |n: u64, color: Color| if n > 0 { color } else { LABEL };

    let mut lines = vec![
        heading("requests"),
        kv("completed", format::count(m.total), Color::Reset, w),
        kv("in flight", format::count(in_flight), Color::Reset, w),
        kv("ok (2xx)", format::count(m.success), GOOD, w),
        kv(
            "http errors",
            format::count(m.failed),
            quiet(m.failed, WARN),
            w,
        ),
        kv(
            "timeouts",
            format::count(m.timeouts),
            quiet(m.timeouts, BAD),
            w,
        ),
        kv(
            "conn errors",
            format::count(m.errors),
            quiet(m.errors, BAD),
            w,
        ),
        Line::raw(""),
        heading("throughput"),
        kv("now", format!("{} req/s", format::compact(now)), ACCENT, w),
        kv(
            "average",
            format!("{} req/s", format::compact(m.rps(elapsed))),
            Color::Reset,
            w,
        ),
        kv(
            "peak",
            format!("{} req/s", format::compact(peak)),
            Color::Reset,
            w,
        ),
        kv(
            "transfer",
            format!("{}/s", format::bytes(m.throughput(elapsed))),
            Color::Reset,
            w,
        ),
        kv("received", format::bytes(m.bytes as f64), Color::Reset, w),
        Line::raw(""),
        heading("latency"),
        kv("min", format::latency(m.min()), Color::Reset, w),
    ];
    for (i, &(name, q)) in PERCENTILES.iter().enumerate().take(6) {
        lines.push(kv(name, format::latency(m.percentile(q)), heat(i, 6), w));
    }
    lines.extend([
        kv("max", format::latency(m.max()), BAD, w),
        kv(
            "mean ± sd",
            format!(
                "{} ± {}",
                format::latency(m.mean()),
                format::latency(m.std_dev())
            ),
            Color::Reset,
            w,
        ),
        Line::raw(""),
        heading("status"),
    ]);
    lines.extend(status_rows(d, w));
    f.render_widget(Paragraph::new(lines), area);
}

/// One row per status code and failure kind: label, bar, count, share
fn status_rows(d: &Dashboard, width: usize) -> Vec<Line<'static>> {
    let m = &d.metrics;
    if m.total == 0 {
        return vec![Line::from(label("no responses yet").italic())];
    }
    let mut rows: Vec<(String, Color, u64)> = m
        .status_codes
        .iter()
        .map(|(&code, &n)| (code.to_string(), status_color(code), n))
        .collect();
    rows.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    if m.timeouts > 0 {
        rows.push(("timeout".into(), BAD, m.timeouts));
    }
    if m.errors > 0 {
        rows.push(("conn err".into(), BAD, m.errors));
    }
    let count_width = rows
        .iter()
        .map(|r| format::count(r.2).len())
        .max()
        .unwrap_or(1);
    let bar_width = width.saturating_sub(8 + count_width + 8);
    rows.into_iter()
        .map(|(name, color, n)| {
            let share = n as f64 / m.total as f64;
            Line::from(vec![
                value(format!("{name:<8}"), color),
                Span::styled(
                    format!("{:<bar_width$}", bar(share, bar_width)),
                    Style::new().fg(color),
                ),
                Span::raw(format!(" {:>count_width$}", format::count(n))),
                label(format!(" {:>5.1}%", share * 100.0)),
            ])
        })
        .collect()
}

// ─── Stats: every number, as cards, over the latency distribution ───────────

/// Cards are laid out in as many columns of about this width as fit
const CARD_WIDTH: u16 = 48;
const CARD_GAP: u16 = 4;

/// A titled block of rows, built for a given width
struct Card {
    lines: Vec<Line<'static>>,
}

impl Card {
    fn new(title: &str) -> Self {
        Card {
            lines: vec![heading(title)],
        }
    }

    fn row(mut self, name: &str, val: String, color: Color, width: usize) -> Self {
        self.lines.push(kv(name, val, color, width));
        self
    }

    fn line(mut self, line: Line<'static>) -> Self {
        self.lines.push(line);
        self
    }
}

fn render_stats_tab(d: &Dashboard, f: &mut Frame, area: Rect) {
    let columns = ((area.width + CARD_GAP) / (CARD_WIDTH + CARD_GAP)).clamp(1, 4);
    let width = (area.width + CARD_GAP) / columns - CARD_GAP;
    let w = width as usize;
    let cards = [
        requests_card(d, w),
        latency_card(d, w),
        throughput_card(d, w),
        errors_card(d, w),
        status_card(d, w),
        test_card(d, w),
        network_card(d, w),
    ];

    // Masonry: each card goes to the shortest column so far
    let mut heights = vec![0u16; columns as usize];
    let mut placed = Vec::with_capacity(cards.len());
    for card in cards {
        let col = (0..heights.len()).min_by_key(|&c| heights[c]).unwrap_or(0);
        placed.push((col, heights[col], card));
        heights[col] += card_height(&placed.last().unwrap().2) + 1;
    }
    let grid_height = heights.iter().copied().max().unwrap_or(0).min(area.height);

    for (col, y, card) in placed {
        let x = area.x + col as u16 * (width + CARD_GAP);
        let height = card_height(&card).min(grid_height.saturating_sub(y));
        if height == 0 {
            continue;
        }
        let rect = Rect::new(x, area.y + y, width, height);
        f.render_widget(Paragraph::new(card.lines), rect);
    }

    // The distribution takes whatever height is left
    let rest = area.height.saturating_sub(grid_height);
    if rest >= 8 {
        let [title, chart] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)])
            .areas(Rect::new(area.x, area.y + grid_height, area.width, rest));
        section(
            f,
            title,
            "latency distribution",
            Some(Line::from(label("requests per latency, √ scale"))),
        );
        render_distribution(d, f.buffer_mut(), chart);
    }
}

fn card_height(card: &Card) -> u16 {
    card.lines.len() as u16
}

fn requests_card(d: &Dashboard, w: usize) -> Card {
    let m = &d.metrics;
    let failed = m.total - m.success;
    let share = |n: u64| match m.total {
        0 => String::new(),
        total => format!("  {:>6.2}%", n as f64 / total as f64 * 100.0),
    };
    let quiet = |n: u64, color: Color| if n > 0 { color } else { LABEL };
    Card::new("requests")
        .row("sent", format::count(d.sent), Color::Reset, w)
        .row("completed", format::count(m.total), Color::Reset, w)
        .row(
            "in flight",
            format::count(d.sent.saturating_sub(m.total)),
            Color::Reset,
            w,
        )
        .row(
            "ok (2xx)",
            format!("{}{}", format::count(m.success), share(m.success)),
            GOOD,
            w,
        )
        .row(
            "failed",
            format!("{}{}", format::count(failed), share(failed)),
            quiet(failed, BAD),
            w,
        )
        .row(
            "  http errors",
            format::count(m.failed),
            quiet(m.failed, WARN),
            w,
        )
        .row(
            "  timeouts",
            format::count(m.timeouts),
            quiet(m.timeouts, BAD),
            w,
        )
        .row(
            "  conn errors",
            format::count(m.errors),
            quiet(m.errors, BAD),
            w,
        )
}

/// Every percentile, each with a log-scale bar so p50 and p99.99 both show
fn latency_card(d: &Dashboard, w: usize) -> Card {
    let m = &d.metrics;
    let min = (m.min().as_micros() as f64).max(1.0);
    let max = (m.max().as_micros() as f64).max(min * 1.01);
    let value_width = 10;
    let bar_width = w.saturating_sub(8 + value_width + 2);
    let mut card = Card::new("latency");
    let row = |card: Card, name: &str, v: Duration, color: Color| {
        let t = ((v.as_micros() as f64).max(min) / min).ln() / (max / min).ln();
        card.line(Line::from(vec![
            label(format!("{name:<8}")),
            value(format!("{:>value_width$}", format::latency(v)), color),
            Span::raw("  "),
            Span::styled(bar(t, bar_width), Style::new().fg(color)),
        ]))
    };
    card = row(card, "min", m.min(), GOOD);
    let n = PERCENTILES.len();
    for (i, &(name, q)) in PERCENTILES.iter().enumerate() {
        card = row(card, name, m.percentile(q), heat(i, n));
    }
    card = row(card, "max", m.max(), BAD);
    card.row(
        "mean ± sd",
        format!(
            "{} ± {}",
            format::latency(m.mean()),
            format::latency(m.std_dev())
        ),
        Color::Reset,
        w,
    )
}

fn throughput_card(d: &Dashboard, w: usize) -> Card {
    let m = &d.metrics;
    let elapsed = d.elapsed();
    let samples = d.timeline.samples();
    // Leave out the ramp-up and drain seconds at either end
    let steady: Vec<f64> = samples
        .iter()
        .skip(1)
        .take(samples.len().saturating_sub(2))
        .map(|s| s.rps)
        .collect();
    let peak = samples.iter().map(|s| s.rps).fold(0.0, f64::max);
    let slowest = steady.iter().copied().fold(f64::MAX, f64::min);
    let spread = if steady.len() >= 2 {
        let mean = steady.iter().sum::<f64>() / steady.len() as f64;
        let var = steady.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / steady.len() as f64;
        format!("±{:.0}%", var.sqrt() / mean.max(f64::EPSILON) * 100.0)
    } else {
        "—".into()
    };
    let avg_body = if m.total > 0 {
        m.bytes as f64 / m.total as f64
    } else {
        0.0
    };
    let rate = |v: f64| format!("{} req/s", format::compact(v));
    Card::new("throughput")
        .row("average", rate(m.rps(elapsed)), ACCENT, w)
        .row("peak second", rate(peak), Color::Reset, w)
        .row(
            "slowest second",
            if steady.is_empty() {
                "—".into()
            } else {
                rate(slowest)
            },
            Color::Reset,
            w,
        )
        .row("spread", spread, Color::Reset, w)
        .row("received", format::bytes(m.bytes as f64), Color::Reset, w)
        .row(
            "transfer",
            format!("{}/s", format::bytes(m.throughput(elapsed))),
            Color::Reset,
            w,
        )
        .row("avg body", format::bytes(avg_body), Color::Reset, w)
        .row("elapsed", format::clock(elapsed), Color::Reset, w)
        .line(sparkline(samples.iter().map(|s| s.rps), w))
}

/// Values as a one-line bar sparkline, squeezed to `width` cells
fn sparkline(values: impl ExactSizeIterator<Item = f64>, width: usize) -> Line<'static> {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let values: Vec<f64> = values.collect();
    if values.is_empty() || width == 0 {
        return Line::raw("");
    }
    let group = values.len().div_ceil(width);
    let points: Vec<f64> = values
        .chunks(group)
        .map(|c| c.iter().sum::<f64>() / c.len() as f64)
        .collect();
    let peak = points.iter().copied().fold(0.0, f64::max).max(f64::EPSILON);
    let text: String = points
        .iter()
        .map(|v| LEVELS[((v / peak) * 7.0).round() as usize])
        .collect();
    Line::styled(text, Style::new().fg(ACCENT))
}

/// Failures grouped by cause, most frequent first
fn errors_card(d: &Dashboard, w: usize) -> Card {
    let m = &d.metrics;
    let failed = m.total - m.success;
    let mut card = Card::new("errors");
    if failed == 0 {
        let text = if m.total == 0 {
            "no responses yet"
        } else {
            "none"
        };
        return card.line(Line::from(value(
            text,
            if m.total == 0 { LABEL } else { GOOD },
        )));
    }
    for (cause, n) in d.top_failure_causes().into_iter().take(6) {
        let count = format!(
            "{}  {:>5.1}%",
            format::count(n),
            n as f64 / m.total as f64 * 100.0
        );
        let room = w.saturating_sub(count.len() + 2);
        card = card.row(&truncate(cause, room), count, BAD, w);
    }
    card
}

fn status_card(d: &Dashboard, w: usize) -> Card {
    let mut card = Card::new("status codes");
    for line in status_rows(d, w) {
        card = card.line(line);
    }
    card
}

fn test_card(d: &Dashboard, w: usize) -> Card {
    let args = &d.args;
    let on_off = |on: bool| if on { "on" } else { "off" }.to_string();
    let plan = match d.plan {
        Plan::Count(n) => format!("{} requests", format::count(n)),
        Plan::Duration(t) => format!("for {}", format::span(t)),
    };
    let concurrency = if d.concurrency == args.concurrency as usize {
        d.concurrency.to_string()
    } else {
        format!("{} (from {})", d.concurrency, args.concurrency)
    };
    let body = args
        .body
        .as_ref()
        .map_or("none".into(), |b| format::bytes(b.len() as f64));
    Card::new("test")
        .line(Line::from(vec![
            value(format!("{} ", args.method), Color::Magenta),
            Span::raw(truncate(&args.url, w.saturating_sub(args.method.len() + 1))),
        ]))
        .row("run", plan, Color::Reset, w)
        .row("concurrency", concurrency, Color::Reset, w)
        .row("timeout", format!("{}s", args.timeout), Color::Reset, w)
        .row("headers", args.headers.len().to_string(), Color::Reset, w)
        .row("body", body, Color::Reset, w)
        .row(
            "keep-alive",
            on_off(!args.disable_keepalive),
            Color::Reset,
            w,
        )
        .row(
            "redirects",
            on_off(!args.disable_redirects),
            Color::Reset,
            w,
        )
        .row(
            "proxy",
            truncate(args.proxy.as_deref().unwrap_or("none"), w / 2),
            Color::Reset,
            w,
        )
        .row(
            "user agent",
            truncate(&args.user_agent, w / 2),
            Color::Reset,
            w,
        )
}

fn network_card(d: &Dashboard, w: usize) -> Card {
    let m = &d.metrics;
    Card::new("network")
        .row(
            "dns lookup",
            format::latency(m.avg_dns_lookup()),
            Color::Reset,
            w,
        )
        .row(
            "cache hits",
            format!("{:.1}%", m.cache_hit_rate()),
            Color::Reset,
            w,
        )
        .row(
            "from",
            truncate(&gethostname().to_string_lossy(), w / 2),
            Color::Reset,
            w,
        )
        .row("cores", num_of_cores().to_string(), Color::Reset, w)
}

/// Latency histogram on a log x-axis and a square-root y-axis (so the tail
/// shows), with p50, p90 and p99 marked
fn render_distribution(d: &Dashboard, buf: &mut Buffer, area: Rect) {
    const LEVELS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    let hist = d.metrics.latency();
    // Bottom row for the latency labels, top row for the markers
    if hist.count() == 0 || area.height < 4 || area.width <= AXIS + 4 {
        return;
    }
    let plot = Rect::new(
        area.x + AXIS,
        area.y + 1,
        area.width - AXIS,
        area.height - 2,
    );
    let lo = (d.metrics.min().as_micros() as f64).max(1.0);
    let hi = (d.metrics.max().as_micros() as f64).max(lo * 1.01);
    let span = (hi / lo).ln();
    let x_of = |us: f64| -> usize {
        (((us.max(lo) / lo).ln() / span) * (plot.width - 1) as f64).round() as usize
    };
    let mut counts = vec![0u64; plot.width as usize];
    for (v, n) in hist.buckets() {
        counts[x_of(v as f64).min(plot.width as usize - 1)] += n;
    }
    let peak = counts.iter().copied().max().unwrap_or(0).max(1) as f64;
    let steps = plot.height as usize * 8;

    let p = |q| d.metrics.percentile(q).as_micros() as f64;
    let (p50, p90, p99) = (p(50.0), p(90.0), p(99.0));
    for (x, &n) in counts.iter().enumerate() {
        if n == 0 {
            continue;
        }
        let level = (((n as f64 / peak).sqrt() * steps as f64).round() as usize).max(1);
        // Past p99 is the tail: tint it so it reads as such
        let at = lo * (span * x as f64 / (plot.width - 1) as f64).exp();
        let color = if at > p99 { WARN } else { Color::Indexed(250) };
        for row in 0..plot.height as usize {
            let from_bottom = plot.height as usize - 1 - row;
            let fill = level.saturating_sub(from_bottom * 8).min(8);
            if fill > 0 {
                if let Some(cell) = buf.cell_mut((plot.x + x as u16, plot.y + row as u16)) {
                    cell.set_char(LEVELS[fill - 1]).set_fg(color);
                }
            }
        }
    }

    // Percentile markers: a dotted rule down the empty space, label on top
    for (name, us, color) in [
        ("p50", p50, ACCENT),
        ("p90", p90, Color::Reset),
        ("p99", p99, WARN),
    ] {
        let x = plot.x + x_of(us) as u16;
        for row in 0..plot.height {
            if let Some(cell) = buf.cell_mut((x, plot.y + row)) {
                if cell.symbol() == " " {
                    cell.set_char('┊').set_fg(color);
                }
            }
        }
        let text = format!("{name} {}", format::latency_short(us as u64));
        let start = x.min(area.right().saturating_sub(text.len() as u16));
        buf.set_string(start, area.y, text, Style::new().fg(color).bold());
    }

    // Axes: peak count on the left, latency labels every ~12 cells below
    draw_axis(buf, plot_axis(area, plot), &format::compact(peak), "0");
    let mut next_free = plot.x;
    let bottom = area.bottom() - 1;
    for x in (0..plot.width).step_by(12) {
        let us = lo * (span * x as f64 / (plot.width - 1) as f64).exp();
        let text = format::latency_short(us as u64);
        let at = plot.x + x;
        if at >= next_free && at + text.len() as u16 <= area.right() {
            buf.set_string(at, bottom, &text, Style::new().fg(LABEL));
            next_free = at + text.len() as u16 + 2;
        }
    }
}

/// The strip left of `plot`, where `draw_axis` puts its rule and labels
fn plot_axis(area: Rect, plot: Rect) -> Rect {
    Rect::new(area.x, plot.y, AXIS + 1, plot.height)
}

// ─── Requests ────────────────────────────────────────────────────────────────

fn render_requests_tab(d: &Dashboard, f: &mut Frame, area: Rect) {
    let log = d.visible_log();
    if d.inspecting && !log.is_empty() {
        return render_inspector(d, f, area, &log);
    }
    let [title, filters, _, table] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);

    let kept = d.kept();
    let mut right = vec![if d.filter.is_active() {
        label(format!(
            "{} of {} match  ",
            format::count(log.len() as u64),
            format::count(kept as u64)
        ))
    } else {
        label(format!("last {} kept  ", format::count(kept as u64)))
    }];
    if d.scroll > 0 && !d.inspecting {
        right.push(Span::styled(
            format!(" {} newer · g for live ", d.scroll),
            Style::new().fg(Color::Black).bg(WARN),
        ));
    } else if d.finished.is_none() {
        right.push(Span::styled(
            " live ",
            Style::new().fg(Color::Black).bg(GOOD),
        ));
    }
    section(f, title, "requests", Some(Line::from(right)));
    render_filter_bar(d, f, filters);

    if log.is_empty() {
        let msg = if d.filter.is_active() {
            "nothing matches these filters · c to clear"
        } else {
            "no responses yet"
        };
        return placeholder(f, table, msg);
    }

    let height = table.height.saturating_sub(1) as usize;
    // Scroll just enough to keep the selected row on screen
    let top = (d.scroll + 1).saturating_sub(height);
    let rows = log.iter().enumerate().skip(top).take(height).map(|(i, e)| {
        let row = request_row(e);
        if i == d.scroll {
            row.style(Style::new().bg(Color::Indexed(237)).bold())
        } else {
            row
        }
    });
    let header = Row::new([
        "", "#", "at", "status", "latency", "size", "cache", "response",
    ])
    .style(Style::new().fg(LABEL).bold());
    f.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(1),
                Constraint::Length(9),
                Constraint::Length(8),
                Constraint::Length(13),
                Constraint::Length(9),
                Constraint::Length(10),
                Constraint::Length(7),
                Constraint::Fill(1),
            ],
        )
        .header(header)
        .column_spacing(1),
        table,
    );
}

/// "STATUS f  all 2xx 3xx …   LATENCY l  any ≥p50 …   SEARCH /  query"
fn render_filter_bar(d: &Dashboard, f: &mut Frame, area: Rect) {
    let selected = Style::new().fg(Color::Black).bg(ACCENT).bold();
    let idle = Style::new().fg(LABEL);
    let group = |spans: &mut Vec<Span<'static>>, name: &str, key: &str| {
        spans.push(Span::styled(
            format!("{name} "),
            Style::new().fg(LABEL).bold(),
        ));
        spans.push(Span::styled(
            format!("{key} "),
            Style::new().fg(ACCENT).bold(),
        ));
    };

    let mut spans = Vec::new();
    group(&mut spans, "STATUS", "f");
    for status in filter::Status::ALL {
        let style = if status == d.filter.status {
            selected
        } else {
            idle
        };
        spans.push(Span::styled(format!(" {} ", status.label()), style));
    }
    spans.push(Span::raw("    "));
    group(&mut spans, "LATENCY", "l");
    for slow in filter::Slow::ALL {
        let style = if slow == d.filter.slow {
            selected
        } else {
            idle
        };
        spans.push(Span::styled(format!(" {} ", slow.label()), style));
    }
    if d.filter.slow != filter::Slow::Any {
        spans.push(label(format!(
            " {}",
            format::latency(Duration::from_micros(d.slow_threshold_us))
        )));
    }
    spans.push(Span::raw("    "));
    group(&mut spans, "SEARCH", "/");
    if d.filter.editing {
        spans.push(Span::styled(
            format!(" {}▏", d.filter.query),
            Style::new().fg(Color::Reset).bg(RULE),
        ));
    } else if d.filter.query.is_empty() {
        spans.push(label(" status or body text"));
    } else {
        spans.push(Span::styled(format!(" {} ", d.filter.query), selected));
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// One request in full: its stats and the request sent on the left, the
/// response's headers and body on the right
fn render_inspector(d: &Dashboard, f: &mut Frame, area: Rect, log: &[&LogEntry]) {
    let index = d.scroll.min(log.len().saturating_sub(1));
    let Some(entry) = log.get(index) else {
        return placeholder(f, area, "no request selected");
    };
    let [title, _, body] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
    ])
    .areas(area);

    let position = Line::from(vec![
        value(format::count((index + 1) as u64), Color::Reset),
        label(format!(" of {} ", format::count(log.len() as u64))),
        label(if index == 0 { "· newest" } else { "" }),
        label(if d.finished.is_none() {
            "  · list paused while inspecting"
        } else {
            ""
        }),
    ]);
    section(
        f,
        title,
        &format!("request #{}", format::count(entry.seq)),
        Some(position),
    );

    let [left, _, right] = Layout::horizontal([
        Constraint::Length(46.min(body.width / 2)),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .areas(body);
    f.render_widget(
        Paragraph::new(inspector_stats(d, entry, left.width as usize)),
        left,
    );
    render_response(d, f, right, &entry.stat);
}

fn inspector_stats(d: &Dashboard, entry: &LogEntry, w: usize) -> Vec<Line<'static>> {
    let stat = &entry.stat;
    let m = &d.metrics;
    let dash = || "—".to_string();
    let mut lines = vec![heading("outcome")];

    let (status, color) = match (stat.status_code, stat.error) {
        (Some(code), _) => (
            format!(
                "{} {}",
                code.as_u16(),
                code.canonical_reason().unwrap_or("")
            ),
            status_color(code.as_u16()),
        ),
        (None, error) => (error.map_or("ERROR", |e| e.label()).to_string(), BAD),
    };
    lines.push(kv("status", status.trim_end().to_string(), color, w));
    if let Some(message) = &stat.error_message {
        lines.push(Line::from(Span::styled(
            truncate(message, w),
            Style::new().fg(BAD),
        )));
    }

    lines.push(Line::raw(""));
    lines.push(heading("timing"));
    let rank = m.rank(stat.duration) * 100.0;
    lines.push(kv("total", format::latency(stat.duration), Color::Reset, w));
    lines.push(kv(
        "  vs the run",
        format!("slower than {rank:.0}%"),
        match rank {
            r if r >= 99.0 => BAD,
            r if r >= 90.0 => WARN,
            _ => LABEL,
        },
        w,
    ));
    match stat.ttfb {
        Some(ttfb) => {
            lines.push(kv("  first byte", format::latency(ttfb), Color::Reset, w));
            lines.push(kv(
                "  body",
                format::latency(stat.duration.saturating_sub(ttfb)),
                Color::Reset,
                w,
            ));
        }
        None => lines.push(kv("  first byte", dash(), LABEL, w)),
    }
    let started = entry.at.saturating_sub(stat.duration);
    lines.push(kv("started", format::clock_ms(started), Color::Reset, w));
    lines.push(kv("finished", format::clock_ms(entry.at), Color::Reset, w));
    lines.push(kv(
        "dns",
        stat.dns_times.map_or_else(
            || "not sampled".into(),
            |(lookup, _)| format::latency(lookup),
        ),
        if stat.dns_times.is_some() {
            Color::Reset
        } else {
            LABEL
        },
        w,
    ));

    lines.push(Line::raw(""));
    lines.push(heading("response"));
    let detail = stat.detail.as_deref();
    let header = |name: &str| {
        detail
            .and_then(|d| d.headers.get(name))
            .and_then(|v| v.to_str().ok())
            .map(str::to_string)
    };
    lines.push(kv(
        "size",
        format::bytes(stat.body_bytes as f64),
        Color::Reset,
        w,
    ));
    lines.push(kv(
        "type",
        truncate(&header("content-type").unwrap_or_else(dash), w / 2),
        Color::Reset,
        w,
    ));
    lines.push(kv(
        "encoding",
        header("content-encoding").unwrap_or_else(dash),
        Color::Reset,
        w,
    ));
    lines.push(kv(
        "cache",
        stat.cache_status
            .as_ref()
            .map_or_else(dash, |c| format!("{c:?}").to_lowercase()),
        Color::Reset,
        w,
    ));
    if let Some(detail) = detail {
        lines.push(kv(
            "protocol",
            format!("{:?}", detail.version),
            Color::Reset,
            w,
        ));
        lines.push(kv(
            "server",
            detail.remote_addr.map_or_else(dash, |a| a.to_string()),
            Color::Reset,
            w,
        ));
        if detail.final_url.trim_end_matches('/') != d.args.url.trim_end_matches('/') {
            lines.push(kv("redirected", dash(), WARN, w));
            lines.push(Line::raw(truncate(&detail.final_url, w)));
        }
    }

    // The request is the same for every run; show what went on the wire
    lines.push(Line::raw(""));
    lines.push(heading("request sent"));
    let args = &d.args;
    lines.push(Line::from(vec![
        value(format!("{} ", args.method), Color::Magenta),
        Span::raw(truncate(&args.url, w.saturating_sub(args.method.len() + 1))),
    ]));
    // Host carries the port unless it's the scheme's default
    let host = reqwest::Url::parse(&args.url)
        .ok()
        .and_then(|u| {
            let host = u.host_str()?.to_string();
            Some(match u.port() {
                Some(port) => format!("{host}:{port}"),
                None => host,
            })
        })
        .unwrap_or_default();
    let custom: Vec<(String, String)> = args
        .headers
        .iter()
        .filter_map(|h| h.split_once(':'))
        .map(|(name, value)| (name.trim().to_lowercase(), value.trim().to_string()))
        .collect();
    let mut sent = vec![
        ("host".to_string(), host),
        ("user-agent".to_string(), args.user_agent.clone()),
    ];
    // The client adds `accept: */*` unless one was given
    if !custom.iter().any(|(name, _)| name == "accept") {
        sent.push(("accept".to_string(), "*/*".to_string()));
    }
    sent.extend(custom);
    for (name, v) in sent {
        lines.push(Line::from(vec![
            label(format!("{name}: ")),
            Span::raw(truncate(&v, w.saturating_sub(name.len() + 2))),
        ]));
    }
    match &args.body {
        Some(body) => {
            lines.push(Line::from(label(format!(
                "body, {}:",
                format::bytes(body.len() as f64)
            ))));
            lines.push(Line::raw(truncate(&body.replace('\n', " "), w)));
        }
        None => lines.push(Line::from(label("no body"))),
    }
    lines
}

/// Status line, headers and body, scrollable
fn render_response(d: &Dashboard, f: &mut Frame, area: Rect, stat: &ResponseStats) {
    let [title, text] = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).areas(area);
    let Some(detail) = stat.detail.as_deref() else {
        section(f, title, "response", None);
        let why = if stat.status_code.is_none() {
            "no response: the request failed before one arrived"
        } else {
            "not kept in full: above 1,000 requests a second pepe keeps an \
             evenly spread sample of complete responses (marked ● in the \
             list; failures sampled separately), dropping the oldest past \
             32 MiB. Press [ or ] for the nearest newer or older one kept."
        };
        f.render_widget(
            Paragraph::new(Line::from(label(why).italic())).wrap(Wrap { trim: true }),
            text,
        );
        return;
    };
    let hint = if d.detail_scroll > 0 {
        format!("line {} · PgUp/PgDn", d.detail_scroll + 1)
    } else {
        "PgUp/PgDn to scroll".to_string()
    };
    section(f, title, "response", Some(Line::from(label(hint))));

    let mut lines = Vec::new();
    if let Some(code) = stat.status_code {
        lines.push(Line::from(vec![
            label(format!("{:?} ", detail.version)),
            value(
                format!(
                    "{} {}",
                    code.as_u16(),
                    code.canonical_reason().unwrap_or("")
                ),
                status_color(code.as_u16()),
            ),
        ]));
    }
    for (name, v) in &detail.headers {
        lines.push(Line::from(vec![
            Span::styled(format!("{name}: "), Style::new().fg(ACCENT)),
            Span::raw(String::from_utf8_lossy(v.as_bytes()).into_owned()),
        ]));
    }
    lines.push(Line::raw(""));
    lines.extend(body_lines(detail, stat.body_bytes));
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((d.detail_scroll, 0)),
        text,
    );
}

/// The body as text: pretty-printed when it's JSON, a note when binary
fn body_lines(detail: &crate::response::Detail, total: u64) -> Vec<Line<'static>> {
    let body = &detail.body[..];
    if body.is_empty() {
        return vec![Line::from(label("(empty body)"))];
    }
    let mut lines = Vec::new();
    let json = (!detail.truncated)
        .then(|| serde_json::from_slice::<serde_json::Value>(body).ok())
        .flatten()
        .and_then(|v| serde_json::to_string_pretty(&v).ok());
    let text = match (json, std::str::from_utf8(body)) {
        (Some(pretty), _) => pretty,
        (None, Ok(text)) => text.to_string(),
        // Cut mid-character at the capture limit: fine; otherwise binary
        (None, Err(e)) if e.error_len().is_none() => {
            String::from_utf8_lossy(&body[..e.valid_up_to()]).into_owned()
        }
        (None, Err(_)) => {
            return vec![Line::from(label(format!(
                "binary body, {} (not shown)",
                format::bytes(total as f64)
            )))]
        }
    };
    for line in text.lines() {
        lines.push(Line::raw(line.replace('\t', "    ")));
    }
    if detail.truncated {
        lines.push(Line::raw(""));
        lines.push(Line::from(label(format!(
            "… first {} of {} shown",
            format::bytes(detail.body.len() as f64),
            format::bytes(total as f64)
        ))));
    }
    lines
}

fn request_row(e: &LogEntry) -> Row<'static> {
    let cache = e
        .stat
        .cache_status
        .as_ref()
        .map(|c| format!("{c:?}").to_lowercase())
        .unwrap_or_default();
    // ● marks requests whose full response was kept for the inspector
    let full = if e.stat.detail.is_some() { "●" } else { "" };
    Row::new(vec![
        Line::from(Span::styled(full, Style::new().fg(ACCENT))),
        Line::from(label(format::count(e.seq))),
        Line::from(label(format::clock(e.at))),
        Line::from(status_span(&e.stat)),
        Line::from(Span::raw(format::latency(e.stat.duration))).alignment(Alignment::Right),
        Line::from(label(format::bytes(e.stat.body_bytes as f64))).alignment(Alignment::Right),
        Line::from(label(cache)),
        Line::raw(outcome_text(&e.stat)),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Cli;
    use clap::Parser;
    use ratatui::{backend::TestBackend, Terminal};
    use reqwest::StatusCode;

    #[test]
    fn bars_have_eighth_cell_resolution() {
        assert_eq!(bar(0.0, 10), "");
        assert_eq!(bar(1.0, 4), "████");
        assert_eq!(bar(0.5, 3), "█▌");
        assert_eq!(bar(2.0, 2), "██", "clamped");
    }

    #[test]
    fn summary_drops_what_does_not_fit() {
        let parts = [
            "1,000 requests".to_string(),
            "5s".into(),
            "200 req/s".into(),
        ];
        assert_eq!(fit_parts(&parts, 100), "1,000 requests · 5s · 200 req/s");
        assert_eq!(fit_parts(&parts, 22), "1,000 requests · 5s");
        assert_eq!(fit_parts(&parts, 5), "");
    }

    #[test]
    fn inspector_says_the_list_is_paused_while_live() {
        let args = Cli::parse_from(["pepe", "-c", "4", "http://example.com/"]);
        let mut d = Dashboard::new(args, Plan::Count(100));
        record_some(&mut d, 5);
        d.tab = Tab::Requests;
        d.inspecting = true;
        let mut terminal = Terminal::new(TestBackend::new(170, 40)).unwrap();
        terminal.draw(|f| render(&d, f)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(screen.contains("list paused while inspecting"));
        assert!(screen.contains("REQUEST #5"));
        assert!(screen.contains("first byte"));
    }

    #[test]
    fn json_bodies_are_pretty_printed() {
        let detail = crate::response::Detail {
            version: reqwest::Version::HTTP_11,
            headers: Default::default(),
            body: bytes::Bytes::from_static(b"{\"a\":[1,2]}"),
            truncated: false,
            remote_addr: None,
            final_url: String::new(),
        };
        let text: Vec<String> = body_lines(&detail, 11)
            .iter()
            .map(|l| l.to_string())
            .collect();
        assert_eq!(text, ["{", "  \"a\": [", "    1,", "    2", "  ]", "}"]);

        let binary = crate::response::Detail {
            body: bytes::Bytes::from_static(&[0xff, 0xfe, 0x00, 0x41]),
            ..detail
        };
        assert!(body_lines(&binary, 4)[0]
            .to_string()
            .starts_with("binary body"));
    }

    #[test]
    fn truncates_with_ellipsis() {
        assert_eq!(truncate("https://example.com", 50), "https://example.com");
        assert_eq!(truncate("abcdef", 4), "abc…");
    }

    fn record_some(d: &mut Dashboard, n: u64) {
        for i in 0..n {
            d.record(ResponseStats {
                duration: Duration::from_micros(200 + i * 997),
                status_code: StatusCode::from_u16(if i % 7 == 0 { 503 } else { 200 }).ok(),
                body_bytes: 2048,
                preview: Some(bytes::Bytes::from_static(b"{\"ok\":true}")),
                ttfb: Some(Duration::from_micros(150 + i * 900)),
                // Every other request captured in full, as the budget would
                detail: (i % 2 == 0).then(|| {
                    let mut headers = reqwest::header::HeaderMap::new();
                    headers.insert("content-type", "application/json".parse().unwrap());
                    std::sync::Arc::new(crate::response::Detail {
                        version: reqwest::Version::HTTP_11,
                        headers,
                        body: bytes::Bytes::from_static(b"{\"ok\":true,\"items\":[1,2,3]}"),
                        truncated: i % 4 == 0,
                        remote_addr: "127.0.0.1:8000".parse().ok(),
                        final_url: "http://example.com/next".into(),
                    })
                }),
                ..Default::default()
            });
        }
    }

    /// Every tab renders at many sizes, empty, live and finished, without panicking
    #[test]
    fn renders_every_tab_at_many_sizes() {
        let args = Cli::parse_from(["pepe", "-c", "4", "http://example.com/"]);
        let mut d = Dashboard::new(args, Plan::Count(100));
        for stage in 0..3 {
            match stage {
                1 => {
                    record_some(&mut d, 300);
                    d.timeline.advance(Duration::from_secs(3));
                }
                2 => {
                    // More samples than columns, then finished
                    for s in 0..700 {
                        record_some(&mut d, 3);
                        d.timeline.advance(Duration::from_secs(4 + s));
                    }
                    d.finished = Some(Duration::from_secs(704));
                    d.interrupted = true;
                    let samples: Vec<_> = d.timeline.samples().iter().copied().collect();
                    d.verdict = Some(crate::insights::verdict(&d.metrics, &samples, true));
                }
                _ => {}
            }
            for (w, h) in [
                (40, 10),
                (60, 18),
                (80, 24),
                (109, 30),
                (120, 40),
                (250, 70),
            ] {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                for tab in Tab::ALL {
                    d.tab = tab;
                    d.show_help = h >= 30;
                    for frame in [0, 3, 39] {
                        d.frame = frame;
                        terminal.draw(|f| render(&d, f)).unwrap();
                    }
                }
                // The inspector, on a captured request and on one that isn't
                d.tab = Tab::Requests;
                d.inspecting = true;
                for (selected, scroll) in [(0, 0), (1, 5), (2, 500)] {
                    d.scroll = selected;
                    d.detail_scroll = scroll;
                    terminal.draw(|f| render(&d, f)).unwrap();
                }
                d.inspecting = false;
                d.scroll = 0;
            }
        }
    }
}
