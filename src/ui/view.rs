//! Drawing. Everything here reads `Dashboard` state and never changes it.

use std::time::Duration;

use gethostname::gethostname;
use ratatui::{
    layout::{Alignment, Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    symbols,
    text::{Line, Span},
    widgets::{
        Axis, Bar, BarChart, BarGroup, Block, BorderType, Chart, Clear, Dataset, GraphType,
        LineGauge, Paragraph, Row, Table, Tabs,
    },
    Frame,
};

use super::{format, progress_percent, Dashboard, LogEntry, Tab};
use crate::load::Plan;
use crate::response::ResponseStats;
use crate::utils::num_of_cores;

const ACCENT: Color = Color::Cyan;
const MUTED: Color = Color::DarkGray;
const GOOD: Color = Color::Green;
const WARN: Color = Color::Yellow;
const BAD: Color = Color::Red;

/// Smallest terminal the layout is designed for
const MIN_WIDTH: u16 = 60;
const MIN_HEIGHT: u16 = 16;
/// Seconds of history shown on the time-series charts
const CHART_WINDOW: f64 = 120.0;
const PERCENTILES: [(&str, f64); 7] = [
    ("p50", 50.0),
    ("p75", 75.0),
    ("p90", 90.0),
    ("p95", 95.0),
    ("p99", 99.0),
    ("p99.9", 99.9),
    ("max", 100.0),
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

    let [top, tabs, progress, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    render_top_bar(d, f, top);
    render_tabs(d, f, tabs);
    render_progress(d, f, progress);
    match d.tab {
        Tab::Overview => render_overview(d, f, body),
        Tab::Latency => render_latency_tab(d, f, body),
        Tab::Requests => render_requests_tab(d, f, body),
    }
    render_footer(d, f, footer);

    if d.show_help {
        render_help(f, area);
    }
}

fn panel(title: impl Into<Line<'static>>) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(MUTED))
        .title(title.into().style(Style::new().bold()))
}

/// Color for a status code class
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
        (Some(code), _) => Span::styled(
            code.as_u16().to_string(),
            Style::new().fg(status_color(code.as_u16())).bold(),
        ),
        (None, error) => Span::styled(
            error.map(|e| e.label()).unwrap_or("ERROR"),
            Style::new().fg(BAD).bold(),
        ),
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

// ─── Chrome ──────────────────────────────────────────────────────────────────

fn render_top_bar(d: &Dashboard, f: &mut Frame, area: Rect) {
    let (badge, badge_color) = if d.interrupted {
        ("■ STOPPED", BAD)
    } else if d.finished.is_some() {
        ("✔ DONE", Color::Blue)
    } else if d.paused {
        ("‖ PAUSED", WARN)
    } else {
        ("● RUNNING", GOOD)
    };
    let right = Line::from(vec![
        Span::styled(
            format!(" {badge} "),
            Style::new().fg(Color::Black).bg(badge_color).bold(),
        ),
        Span::raw("  "),
        Span::styled("×", Style::new().fg(MUTED)),
        Span::styled(format!("{} ", d.concurrency), Style::new().bold()),
        Span::styled(" ", Style::new()),
        Span::styled(format::clock(d.elapsed()), Style::new().bold()),
        Span::raw(" "),
    ]);

    let brand = " pepe ";
    let method = format!(" {} ", d.args.method);
    let url_room = (area.width as usize)
        .saturating_sub(right.width() + brand.chars().count() + method.len() + 3);
    let left = Line::from(vec![
        Span::styled(brand, Style::new().fg(Color::Black).bg(ACCENT).bold()),
        Span::raw(" "),
        Span::styled(method, Style::new().fg(Color::Magenta).bold()),
        Span::raw(truncate(&d.args.url, url_room)),
    ]);

    f.render_widget(Paragraph::new(left), area);
    f.render_widget(Paragraph::new(right).alignment(Alignment::Right), area);
}

fn render_tabs(d: &Dashboard, f: &mut Frame, area: Rect) {
    let titles = Tab::ALL
        .iter()
        .enumerate()
        .map(|(i, t)| Line::from(format!("{} {}", i + 1, t.title())));
    f.render_widget(
        Tabs::new(titles)
            .select(d.tab.index())
            .style(Style::new().fg(MUTED))
            .highlight_style(
                Style::new()
                    .fg(ACCENT)
                    .bold()
                    .add_modifier(Modifier::UNDERLINED),
            )
            .divider(Span::styled("│", Style::new().fg(MUTED))),
        area,
    );
}

fn render_progress(d: &Dashboard, f: &mut Frame, area: Rect) {
    let m = &d.metrics;
    let elapsed = d.elapsed();
    let finished = d.finished.is_some();
    let percent = progress_percent(d.plan, m.total, elapsed, finished);

    let detail = match d.plan {
        Plan::Count(n) => {
            let mut s = format!("{} / {} req", format::count(m.total), format::count(n));
            let rate = m.rps(elapsed);
            if !finished && rate > 0.0 {
                let eta = n.saturating_sub(m.total) as f64 / rate;
                s += &format!(
                    "  ·  ETA {}",
                    format::span(Duration::from_secs_f64(eta.ceil()))
                );
            }
            s
        }
        Plan::Duration(total) => format!(
            "{} / {}",
            format::span(elapsed.min(total)),
            format::span(total)
        ),
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
    let label = Line::from(vec![
        Span::styled(format!("{percent:>3}% "), Style::new().fg(color).bold()),
        Span::styled(format!("{detail}  "), Style::new().fg(MUTED)),
    ]);
    f.render_widget(
        LineGauge::default()
            .ratio(percent as f64 / 100.0)
            .label(label)
            .line_set(symbols::line::THICK)
            .filled_style(Style::new().fg(color))
            .unfilled_style(Style::new().fg(MUTED)),
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
        spans.push(Span::styled(format!("{action} "), Style::new().fg(MUTED)));
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
    if d.tab == Tab::Requests {
        hints.push(("↑↓", "scroll"));
        hints.push(("e", if d.errors_only { "all" } else { "errors" }));
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
    let rows: [(&str, &str); 12] = [
        ("space / p", "pause or resume sending"),
        ("+ / -", "raise or lower concurrency by ~10%"),
        ("s / i", "stop the run, keep the results"),
        ("r", "restart with the same settings"),
        ("tab / ← →", "switch view"),
        ("1 2 3", "overview, latency, requests"),
        ("↑ ↓ / j k", "scroll the request log"),
        ("PgUp PgDn", "scroll faster"),
        ("g / G", "newest / oldest request"),
        ("e", "show only failed requests"),
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
    let about = format!(
        "  pepe {} · {}/{} · {} cores · {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        num_of_cores(),
        gethostname().to_string_lossy()
    );
    lines.push(Line::raw(""));
    lines.push(Line::styled(about, Style::new().fg(MUTED)));

    let popup = center(area, 58, lines.len() as u16 + 2);
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).block(panel(" Keys ").border_style(Style::new().fg(ACCENT))),
        popup,
    );
}

// ─── Overview ────────────────────────────────────────────────────────────────

fn render_overview(d: &Dashboard, f: &mut Frame, area: Rect) {
    // Drop the bottom row on short terminals rather than squash everything
    let bottom_height = if area.height >= 24 { 10 } else { 0 };
    let [kpis, charts, bottom] = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(6),
        Constraint::Length(bottom_height),
    ])
    .areas(area);

    render_kpis(d, f, kpis);

    let [left, right] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(charts);
    render_throughput_chart(d, f, left);
    render_latency_chart(d, f, right);

    if bottom_height > 0 {
        let [codes, pcts, tail] = Layout::horizontal([
            Constraint::Percentage(30),
            Constraint::Percentage(30),
            Constraint::Percentage(40),
        ])
        .areas(bottom);
        render_status_codes(d, f, codes);
        render_percentile_bars(d, f, pcts);
        render_live_tail(d, f, tail);
    }
}

fn render_kpis(d: &Dashboard, f: &mut Frame, area: Rect) {
    let m = &d.metrics;
    let elapsed = d.elapsed();
    let in_flight = d.sent.saturating_sub(m.total);
    let current_rps = match d.timeline.last() {
        Some(s) if d.finished.is_none() => s.rps,
        _ => m.rps(elapsed),
    };
    let success_rate = 100.0 - m.error_rate();
    let failures = m.total - m.success;

    let tiles = [
        (
            "Requests",
            format::count(m.total),
            ACCENT,
            format!("{} in flight", format::count(in_flight)),
        ),
        (
            "Req/s",
            format::compact(current_rps),
            Color::Magenta,
            format!("avg {}", format::compact(m.rps(elapsed))),
        ),
        (
            "Success",
            if m.total == 0 {
                "—".into()
            } else {
                format!("{success_rate:.2}%")
            },
            match success_rate {
                _ if m.total == 0 => MUTED,
                r if r >= 99.0 => GOOD,
                r if r >= 90.0 => WARN,
                _ => BAD,
            },
            format!("{} failed", format::count(failures)),
        ),
        (
            "p50",
            format::latency(m.percentile(50.0)),
            GOOD,
            format!("mean {}", format::latency(m.mean())),
        ),
        (
            "p99",
            format::latency(m.percentile(99.0)),
            WARN,
            format!("max {}", format::latency(m.max())),
        ),
        (
            "Transfer",
            format!("{}/s", format::bytes(m.throughput(elapsed))),
            Color::Blue,
            format!("{} total", format::bytes(m.bytes as f64)),
        ),
    ];

    let areas = Layout::horizontal([Constraint::Fill(1); 6]).split(area);
    for ((title, value, color, sub), area) in tiles.into_iter().zip(areas.iter()) {
        let lines = vec![
            Line::styled(value, Style::new().fg(color).bold()),
            Line::styled(sub, Style::new().fg(MUTED)),
        ];
        f.render_widget(
            Paragraph::new(lines)
                .block(panel(format!(" {title} ")).title_style(Style::new().fg(MUTED))),
            *area,
        );
    }
}

/// X range for the charts: the last `CHART_WINDOW` seconds, at least 10s wide
fn chart_window(d: &Dashboard) -> [f64; 2] {
    let end = d.timeline.last().map_or(0.0, |s| s.at).max(10.0);
    [(end - CHART_WINDOW).max(0.0), end]
}

fn time_axis(bounds: [f64; 2]) -> Axis<'static> {
    let label = |secs: f64| format::span(Duration::from_secs_f64(secs.max(0.0)));
    Axis::default()
        .bounds(bounds)
        .style(Style::new().fg(MUTED))
        .labels([label(bounds[0]), label(bounds[1])])
}

fn value_axis(max: f64, fmt: impl Fn(f64) -> String) -> (Axis<'static>, f64) {
    // Headroom so the line never touches the top border
    let top = (max * 1.15).max(1.0);
    let axis = Axis::default()
        .bounds([0.0, top])
        .style(Style::new().fg(MUTED))
        .labels([fmt(0.0), fmt(top / 2.0), fmt(top)]);
    (axis, top)
}

fn legend(title: &str, series: &[(&str, Color)]) -> Line<'static> {
    let mut spans = vec![Span::styled(format!(" {title} "), Style::new().bold())];
    for (name, color) in series {
        spans.push(Span::styled("━ ", Style::new().fg(*color)));
        spans.push(Span::styled(format!("{name} "), Style::new().fg(MUTED)));
    }
    Line::from(spans)
}

fn waiting(f: &mut Frame, area: Rect, block: Block<'static>, message: &str) {
    let inner = block.inner(area);
    f.render_widget(block, area);
    f.render_widget(
        Paragraph::new(Span::styled(
            message.to_string(),
            Style::new().fg(MUTED).italic(),
        ))
        .alignment(Alignment::Center),
        center(inner, inner.width, 1),
    );
}

/// Points from the timeline inside the chart window
fn series(
    d: &Dashboard,
    window: [f64; 2],
    pick: impl Fn(&crate::timeline::Sample) -> f64,
) -> Vec<(f64, f64)> {
    d.timeline
        .samples()
        .iter()
        .filter(|s| s.at >= window[0])
        .map(|s| (s.at, pick(s)))
        .collect()
}

fn render_throughput_chart(d: &Dashboard, f: &mut Frame, area: Rect) {
    let block = panel(legend(
        "Throughput",
        &[("req/s", ACCENT), ("errors/s", BAD)],
    ));
    if d.timeline.samples().is_empty() {
        return waiting(f, area, block, "collecting the first second…");
    }
    let window = chart_window(d);
    let rps = series(d, window, |s| s.rps);
    let errors = series(d, window, |s| s.errors);
    let peak = rps.iter().map(|p| p.1).fold(0.0, f64::max);
    let (y_axis, _) = value_axis(peak, format::compact);
    let mut datasets = vec![line_dataset(&rps, ACCENT)];
    if errors.iter().any(|p| p.1 > 0.0) {
        datasets.push(line_dataset(&errors, BAD));
    }
    f.render_widget(
        Chart::new(datasets)
            .block(block)
            .x_axis(time_axis(window))
            .y_axis(y_axis)
            .legend_position(None),
        area,
    );
}

fn render_latency_chart(d: &Dashboard, f: &mut Frame, area: Rect) {
    let block = panel(legend("Latency", &[("p50", GOOD), ("p99", WARN)]));
    if d.timeline.samples().is_empty() {
        return waiting(f, area, block, "collecting the first second…");
    }
    let window = chart_window(d);
    let p50 = series(d, window, |s| s.p50_ms);
    let p99 = series(d, window, |s| s.p99_ms);
    let peak = p99.iter().map(|p| p.1).fold(0.0, f64::max);
    let (y_axis, _) = value_axis(peak, |ms| format::latency_short((ms * 1000.0) as u64));
    f.render_widget(
        Chart::new(vec![line_dataset(&p99, WARN), line_dataset(&p50, GOOD)])
            .block(block)
            .x_axis(time_axis(window))
            .y_axis(y_axis)
            .legend_position(None),
        area,
    );
}

fn line_dataset(data: &[(f64, f64)], color: Color) -> Dataset<'_> {
    Dataset::default()
        .marker(symbols::Marker::Braille)
        .graph_type(GraphType::Line)
        .style(Style::new().fg(color))
        .data(data)
}

fn render_status_codes(d: &Dashboard, f: &mut Frame, area: Rect) {
    let block = panel(" Responses ");
    let m = &d.metrics;
    if m.total == 0 {
        return waiting(f, area, block, "no responses yet");
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
        rows.push(("error".into(), BAD, m.errors));
    }

    let inner = block.inner(area);
    let label_width = 8;
    let count_width = rows
        .iter()
        .map(|r| format::count(r.2).len())
        .max()
        .unwrap_or(1);
    let bar_width = (inner.width as usize).saturating_sub(label_width + count_width + 8);
    let lines: Vec<Line> = rows
        .into_iter()
        .map(|(label, color, n)| {
            let share = n as f64 / m.total as f64;
            Line::from(vec![
                Span::styled(
                    format!("{label:<label_width$}"),
                    Style::new().fg(color).bold(),
                ),
                Span::styled(
                    format!("{:<bar_width$}", bar(share, bar_width)),
                    Style::new().fg(color),
                ),
                Span::raw(format!(" {:>count_width$}", format::count(n))),
                Span::styled(format!(" {:>5.1}%", share * 100.0), Style::new().fg(MUTED)),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_percentile_bars(d: &Dashboard, f: &mut Frame, area: Rect) {
    let block = panel(" Percentiles ");
    let m = &d.metrics;
    if m.total == 0 {
        return waiting(f, area, block, "no responses yet");
    }
    let inner = block.inner(area);
    let max = m.max().as_secs_f64().max(f64::EPSILON);
    let bar_width = (inner.width as usize).saturating_sub(6 + 10 + 1);
    let lines: Vec<Line> = PERCENTILES
        .iter()
        .enumerate()
        .map(|(i, &(name, q))| {
            let v = m.percentile(q);
            let color = heat(i, PERCENTILES.len());
            Line::from(vec![
                Span::styled(format!("{name:<6}"), Style::new().fg(MUTED)),
                Span::styled(
                    format!("{:<bar_width$}", bar(v.as_secs_f64() / max, bar_width)),
                    Style::new().fg(color),
                ),
                Span::styled(
                    format!(" {:>9}", format::latency(v)),
                    Style::new().fg(color).bold(),
                ),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(block), area);
}

fn render_live_tail(d: &Dashboard, f: &mut Frame, area: Rect) {
    let block = panel(" Latest ");
    if d.log.is_empty() {
        return waiting(f, area, block, "no responses yet");
    }
    let inner = block.inner(area);
    let lines: Vec<Line> = d
        .log
        .iter()
        .rev()
        .take(inner.height as usize)
        .map(|e| {
            let mut spans = vec![
                status_span(&e.stat),
                Span::raw(" "),
                Span::styled(
                    format!("{:>9}", format::latency(e.stat.duration)),
                    Style::new().fg(WARN),
                ),
                Span::styled(
                    format!(" {:>9} ", format::bytes(e.stat.body_bytes as f64)),
                    Style::new().fg(Color::Blue),
                ),
            ];
            if let Some(preview) = e.stat.preview_text() {
                spans.push(Span::styled(preview, Style::new().fg(MUTED)));
            }
            Line::from(spans)
        })
        .collect();
    f.render_widget(Paragraph::new(lines).block(block), area);
}

// ─── Latency ─────────────────────────────────────────────────────────────────

fn render_latency_tab(d: &Dashboard, f: &mut Frame, area: Rect) {
    let [left, right] =
        Layout::horizontal([Constraint::Fill(3), Constraint::Length(34)]).areas(area);
    let [dist, over_time] =
        Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(left);
    render_distribution(d, f, dist);
    render_latency_chart(d, f, over_time);
    render_summary(d, f, right);
}

/// Latency histogram re-binned onto a log scale between min and max
fn render_distribution(d: &Dashboard, f: &mut Frame, area: Rect) {
    let block = panel(" Distribution ");
    let hist = d.metrics.latency();
    if hist.count() == 0 {
        return waiting(f, area, block, "no responses yet");
    }

    const BAR: u16 = 6;
    let inner = block.inner(area);
    let bins = ((inner.width + 1) / (BAR + 1)).clamp(1, 40) as usize;
    let lo = (d.metrics.min().as_micros() as f64).max(1.0);
    let hi = (d.metrics.max().as_micros() as f64).max(lo + 1.0);
    let span = (hi / lo).ln();
    let mut counts = vec![0u64; bins];
    for (value, n) in hist.buckets() {
        let t = ((value as f64).max(lo) / lo).ln() / span;
        counts[((t * bins as f64) as usize).min(bins - 1)] += n;
    }

    let bars: Vec<Bar> = counts
        .iter()
        .enumerate()
        .map(|(i, &n)| {
            // Label each bin by its geometric midpoint
            let mid = lo * (span * (i as f64 + 0.5) / bins as f64).exp();
            Bar::default()
                .value(n)
                .text_value(if n == 0 {
                    String::new()
                } else {
                    format::compact(n as f64)
                })
                .label(Line::from(format::latency_short(mid as u64)))
                .style(Style::new().fg(heat(i, bins)))
        })
        .collect();
    f.render_widget(
        BarChart::default()
            .block(block)
            .data(BarGroup::default().bars(&bars))
            .bar_width(BAR)
            .bar_gap(1)
            .value_style(Style::new().fg(Color::Black).bold())
            .label_style(Style::new().fg(MUTED)),
        area,
    );
}

fn render_summary(d: &Dashboard, f: &mut Frame, area: Rect) {
    let m = &d.metrics;
    let lat = |v: Duration| format::latency(v);
    let mut rows: Vec<(&str, String, Color)> = vec![
        ("min", lat(m.min()), GOOD),
        ("mean", lat(m.mean()), Color::Reset),
        ("std dev", lat(m.std_dev()), Color::Reset),
    ];
    for (i, &(name, q)) in PERCENTILES.iter().enumerate() {
        rows.push((name, lat(m.percentile(q)), heat(i, PERCENTILES.len())));
    }
    rows.extend([
        ("", String::new(), Color::Reset),
        ("requests", format::count(m.total), ACCENT),
        (
            "errors",
            format!("{:.2}%", m.error_rate()),
            if m.error_rate() > 0.0 { BAD } else { GOOD },
        ),
        ("dns lookup", lat(m.avg_dns_lookup()), Color::Reset),
        (
            "cache hits",
            format!("{:.1}%", m.cache_hit_rate()),
            Color::Reset,
        ),
    ]);

    let rows = rows.into_iter().map(|(name, value, color)| {
        Row::new(vec![
            Line::styled(name, Style::new().fg(MUTED)),
            Line::styled(value, Style::new().fg(color).bold()).alignment(Alignment::Right),
        ])
    });
    f.render_widget(
        Table::new(rows, [Constraint::Length(12), Constraint::Fill(1)]).block(panel(" Summary ")),
        area,
    );
}

// ─── Requests ────────────────────────────────────────────────────────────────

fn render_requests_tab(d: &Dashboard, f: &mut Frame, area: Rect) {
    let log = d.visible_log();
    let mut title = vec![Span::styled(
        if d.errors_only {
            " Failed requests "
        } else {
            " Requests "
        },
        Style::new().bold(),
    )];
    if d.scroll > 0 {
        title.push(Span::styled(
            format!(" ↑{} · g to follow ", d.scroll),
            Style::new().fg(Color::Black).bg(WARN),
        ));
    } else if d.finished.is_none() {
        title.push(Span::styled(
            " live ",
            Style::new().fg(Color::Black).bg(GOOD),
        ));
    }
    title.push(Span::styled(
        format!(" last {} kept ", log.len()),
        Style::new().fg(MUTED),
    ));
    let block = panel(Line::from(title));

    if log.is_empty() {
        let msg = if d.errors_only {
            "no failed requests so far"
        } else {
            "no responses yet"
        };
        return waiting(f, area, block, msg);
    }

    let height = block.inner(area).height.saturating_sub(1) as usize;
    let rows = log
        .iter()
        .rev()
        .skip(d.scroll)
        .take(height)
        .map(request_row);
    let header = Row::new(["#", "at", "status", "latency", "size", "cache", "response"])
        .style(Style::new().fg(MUTED).bold());
    f.render_widget(
        Table::new(
            rows,
            [
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
        .column_spacing(1)
        .block(block),
        area,
    );
}

fn request_row(e: &LogEntry) -> Row<'static> {
    let cache = e
        .stat
        .cache_status
        .as_ref()
        .map(|c| format!("{c:?}").to_lowercase())
        .unwrap_or_default();
    Row::new(vec![
        Line::styled(format::count(e.seq), Style::new().fg(MUTED)),
        Line::styled(format::clock(e.at), Style::new().fg(MUTED)),
        Line::from(status_span(&e.stat)),
        Line::styled(format::latency(e.stat.duration), Style::new().fg(WARN))
            .alignment(Alignment::Right),
        Line::styled(
            format::bytes(e.stat.body_bytes as f64),
            Style::new().fg(Color::Blue),
        )
        .alignment(Alignment::Right),
        Line::styled(cache, Style::new().fg(MUTED)),
        Line::raw(e.stat.preview_text().unwrap_or_default()),
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
    fn truncates_with_ellipsis() {
        assert_eq!(truncate("https://example.com", 50), "https://example.com");
        assert_eq!(truncate("abcdef", 4), "abc…");
    }

    /// Every tab renders at common sizes, with and without data, without panicking
    #[test]
    fn renders_every_tab_at_many_sizes() {
        let args = Cli::parse_from(["pepe", "-c", "4", "http://example.com/"]);
        let mut d = Dashboard::new(args, Plan::Count(100));
        let mut empty = true;
        for _ in 0..2 {
            for (w, h) in [(40, 10), (60, 16), (80, 24), (120, 40), (250, 70)] {
                let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
                for tab in Tab::ALL {
                    d.tab = tab;
                    d.show_help = h >= 24;
                    terminal.draw(|f| render(&d, f)).unwrap();
                }
            }
            if empty {
                for i in 0..300u64 {
                    d.record(ResponseStats {
                        duration: Duration::from_micros(200 + i * 997),
                        status_code: StatusCode::from_u16(if i % 7 == 0 { 503 } else { 200 }).ok(),
                        body_bytes: 2048,
                        preview: Some(bytes::Bytes::from_static(b"{\"ok\":true}")),
                        ..Default::default()
                    });
                }
                d.timeline.advance(Duration::from_secs(3));
                empty = false;
            }
        }
    }
}
