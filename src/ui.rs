use std::collections::VecDeque;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use gethostname::gethostname;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Bar, BarChart, BarGroup, Block, Borders, List, ListItem, Paragraph},
    Frame, Terminal,
};
use tokio::sync::mpsc::error::TryRecvError;

use crate::load::{LoadHandle, Plan};
use crate::metrics::Metrics;
use crate::response::ResponseStats;
use crate::utils::num_of_cores;
use crate::Cli;

const LOGO: &str = r#"██████╗ ███████╗██████╗ ███████╗
██╔══██╗██╔════╝██╔══██╗██╔════╝
██████╔╝█████╗  ██████╔╝█████╗
██╔═══╝ ██╔══╝  ██╔═══╝ ██╔══╝
██║     ███████╗██║     ███████╗
╚═╝     ╚══════╝╚═╝     ╚══════╝"#;

/// Requests kept for the "Recent Requests" / "Partial Responses" panels
const RECENT_REQUESTS: usize = 100;
const LATENCY_PERCENTILES: [u8; 9] = [0, 10, 25, 50, 75, 90, 95, 99, 100];
/// Status codes always shown in the chart, even at zero
const DEFAULT_STATUS_CODES: [u16; 5] = [200, 400, 404, 500, 503];

/// What the user asked for when leaving the dashboard
pub enum Outcome {
    Quit,
    Restart,
}

pub struct Dashboard {
    args: Cli,
    plan: Plan,
    metrics: Metrics,
    recent: VecDeque<ResponseStats>,
    sent: u64,
    started: Instant,
    /// Set once every request has finished (or the run was stopped)
    finished: Option<Duration>,
    interrupted: bool,
}

/// Width of each bar so `bars` bars (plus 1-cell gaps) fit in `area_width`
fn bar_width(area_width: u16, bars: usize) -> u16 {
    let inner = area_width.saturating_sub(2) as usize; // borders
    let per_bar = inner / bars.max(1);
    per_bar.saturating_sub(1).max(1) as u16
}

/// Progress in percent (0..=100)
fn progress_percent(plan: Plan, completed: u64, elapsed: Duration, finished: bool) -> u16 {
    if finished {
        return 100;
    }
    let ratio = match plan {
        Plan::Count(0) => 1.0,
        Plan::Count(n) => completed as f64 / n as f64,
        Plan::Duration(d) if d.is_zero() => 1.0,
        Plan::Duration(d) => elapsed.as_secs_f64() / d.as_secs_f64(),
    };
    // In-flight requests at the end of a duration run finish after the
    // deadline: hold at 99% until they do
    (ratio * 100.0).clamp(0.0, 99.0) as u16
}

fn format_ms(d: Duration) -> String {
    format!("{:.2}ms", d.as_secs_f64() * 1000.0)
}

fn format_clock(d: Duration) -> String {
    format!(
        "{:02}h:{:02}m:{:02}s:{:03}ms",
        d.as_secs() / 3600,
        d.as_secs() % 3600 / 60,
        d.as_secs() % 60,
        d.subsec_millis()
    )
}

impl Dashboard {
    pub fn new(args: Cli, plan: Plan) -> Self {
        Self {
            args,
            plan,
            metrics: Metrics::default(),
            recent: VecDeque::with_capacity(RECENT_REQUESTS),
            sent: 0,
            started: Instant::now(),
            finished: None,
            interrupted: false,
        }
    }

    fn elapsed(&self) -> Duration {
        self.finished.unwrap_or_else(|| self.started.elapsed())
    }

    fn record(&mut self, stat: ResponseStats) {
        self.metrics.record(&stat);
        if self.recent.len() == RECENT_REQUESTS {
            self.recent.pop_front();
        }
        self.recent.push_back(stat);
    }

    /// Pull everything the load generator produced since the last frame
    fn drain(&mut self, load: &mut LoadHandle) {
        loop {
            match load.rx.try_recv() {
                Ok(stat) => self.record(stat),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if self.finished.is_none() {
                        self.finished = Some(self.started.elapsed());
                    }
                    break;
                }
            }
        }
        self.sent = load.sent();
    }

    pub fn run(&mut self, load: &mut LoadHandle) -> Result<Outcome, Box<dyn std::error::Error>> {
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
        terminal.clear()?;

        loop {
            self.drain(load);
            terminal.draw(|f| self.render_layout(f))?;

            if !event::poll(Duration::from_millis(25))? {
                continue;
            }
            let Event::Key(key) = event::read()? else {
                continue;
            };
            // Windows reports key releases too; act on presses only
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc | KeyCode::Enter => return Ok(Outcome::Quit),
                KeyCode::Char('r') => return Ok(Outcome::Restart),
                KeyCode::Char('i') if self.finished.is_none() => {
                    // Stop sending; results so far stay on screen
                    load.stop();
                    self.interrupted = true;
                }
                _ => {}
            }
        }
    }

    fn render_layout(&self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),  // Header
                Constraint::Length(3),  // Progress
                Constraint::Length(3),  // Stats
                Constraint::Length(20), // Charts
                Constraint::Min(0),     // Request Log
            ])
            .split(f.area());

        self.render_header(f, chunks[0]);
        self.render_progress(f, chunks[1]);
        self.render_stats(f, chunks[2]);
        self.render_charts(f, chunks[3]);
        self.render_request_log(f, chunks[4]);
    }

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(15),
                Constraint::Percentage(15),
                Constraint::Percentage(50),
            ])
            .split(area);

        f.render_widget(
            Paragraph::new(LOGO)
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL)),
            chunks[0],
        );

        let key = |name: &'static str, key: &'static str| {
            Line::from(vec![
                Span::styled(name, Style::default().fg(Color::Yellow)),
                Span::raw(key),
            ])
        };
        let commands = vec![
            key("Quit: ", "q"),
            key("Restart: ", "r"),
            key("Stop: ", "i"),
        ];
        f.render_widget(
            Paragraph::new(commands).block(
                Block::default()
                    .title("Commands")
                    .borders(Borders::ALL)
                    .style(Style::default().fg(Color::Cyan))
                    .title_style(Style::default().fg(Color::White)),
            ),
            chunks[1],
        );

        let field = |name: &'static str, value: String| {
            Line::from(vec![
                Span::styled(name, Style::default().fg(Color::Yellow)),
                Span::raw(value),
            ])
        };
        let info = vec![
            field("Version: ", env!("CARGO_PKG_VERSION").to_string()),
            field("Author: ", env!("CARGO_PKG_AUTHORS").to_string()),
            field("OS: ", std::env::consts::OS.to_string()),
            field("Arch: ", std::env::consts::ARCH.to_string()),
            field("Cores: ", num_of_cores().to_string()),
            field("Hostname: ", gethostname().to_string_lossy().into_owned()),
        ];
        f.render_widget(
            Paragraph::new(info)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Info")
                        .title_style(Style::default().fg(Color::White)),
                )
                .style(Style::default().fg(Color::Cyan)),
            chunks[2],
        );

        let run_length = match self.plan {
            Plan::Count(n) => field("Total Requests: ", n.to_string()),
            Plan::Duration(d) => field("Duration: ", format!("{}s", d.as_secs())),
        };
        let params = vec![
            field("URL: ", self.args.url.clone()),
            field("Method: ", self.args.method.clone()),
            field("Concurrency: ", self.args.concurrency.to_string()),
            run_length,
            field("Timeout: ", format!("{}s", self.args.timeout)),
        ];
        f.render_widget(
            Paragraph::new(params).block(
                Block::default()
                    .title("Test Parameters")
                    .borders(Borders::ALL)
                    .style(Style::default().fg(Color::Cyan))
                    .title_style(Style::default().fg(Color::White)),
            ),
            chunks[3],
        );
    }

    fn render_progress(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
            .split(area);

        let finished = self.finished.is_some();
        let percent = progress_percent(self.plan, self.metrics.total, self.elapsed(), finished);
        let progress_color = match percent {
            0..=25 => Color::Red,
            26..=50 => Color::LightRed,
            51..=75 => Color::Yellow,
            76..=95 => Color::LightGreen,
            _ => Color::Green,
        };

        let total_blocks = chunks[1].width.saturating_sub(14) as usize;
        let filled_blocks = (percent as usize * total_blocks / 100).min(total_blocks);
        let progress_bar = "█".repeat(filled_blocks) + &"░".repeat(total_blocks - filled_blocks);

        let status = if self.interrupted {
            "stopped".to_string()
        } else if finished {
            "done".to_string()
        } else {
            format!("{percent}%")
        };
        let progress_line = Line::from(vec![
            Span::styled(
                format!("{status:>7} "),
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(progress_bar, Style::default().fg(progress_color)),
        ]);

        f.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format_clock(self.elapsed()),
                Style::default().fg(if finished { Color::Green } else { Color::White }),
            )))
            .block(Block::default().borders(Borders::ALL).title("⏳ Duration")),
            chunks[0],
        );
        f.render_widget(
            Paragraph::new(progress_line)
                .block(Block::default().borders(Borders::ALL).title("🚀 Progress")),
            chunks[1],
        );
    }

    fn render_stats(&self, f: &mut Frame, area: Rect) {
        let m = &self.metrics;
        let remaining = match self.plan {
            _ if self.finished.is_some() => "0".to_string(),
            Plan::Count(n) => n.saturating_sub(m.total).to_string(),
            Plan::Duration(d) => format!("{}s", d.saturating_sub(self.elapsed()).as_secs()),
        };
        let stats = [
            ("Total", m.total.to_string(), Color::Yellow),
            ("Remaining", remaining, Color::LightYellow),
            ("Sent", self.sent.to_string(), Color::Cyan),
            ("Success", m.success.to_string(), Color::Green),
            ("Failed", m.failed.to_string(), Color::LightRed),
            ("Errors", m.errors.to_string(), Color::Red),
            ("Timeouts", m.timeouts.to_string(), Color::Red),
        ];

        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Ratio(1, stats.len() as u32); stats.len()])
            .split(area);

        for (i, (label, value, color)) in stats.into_iter().enumerate() {
            self.render_stat_widget(f, chunks[i], label, value, color);
        }
    }

    fn render_charts(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(50),
            ])
            .split(area);

        f.render_widget(self.latency_chart(chunks[2].width), chunks[2]);
        f.render_widget(self.status_codes_chart(chunks[1].width), chunks[1]);

        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Ratio(1, 4); 4])
            .split(chunks[0]);
        let split = |area: Rect, n: u32| {
            Layout::default()
                .direction(Direction::Horizontal)
                .constraints(vec![Constraint::Ratio(1, n); n as usize])
                .split(area)
        };
        let (row0, row1, row2, row3) = (
            split(rows[0], 3),
            split(rows[1], 3),
            split(rows[2], 2),
            split(rows[3], 2),
        );

        let m = &self.metrics;
        let elapsed = self.elapsed();
        let error_rate = if m.total > 0 {
            (m.total - m.success) as f64 / m.total as f64 * 100.0
        } else {
            0.0
        };
        let widgets = [
            (row0[0], "Min", format_ms(m.min()), Color::Green),
            (row0[1], "Max", format_ms(m.max()), Color::Red),
            (row0[2], "Avg", format_ms(m.mean()), Color::Yellow),
            (row1[0], "Std Dev", format_ms(m.std_dev()), Color::Cyan),
            (
                row1[1],
                "Requests/Sec",
                format!("{:.1}", m.rps(elapsed)),
                Color::Magenta,
            ),
            (
                row1[2],
                "Cache Hit Rate",
                format!("{:.2}%", m.cache_hit_rate()),
                Color::Green,
            ),
            (
                row2[0],
                "Avg DNS Lookup",
                format_ms(m.avg_dns_lookup()),
                Color::LightMagenta,
            ),
            (
                row2[1],
                "Error Rate",
                format!("{error_rate:.2}%"),
                Color::LightRed,
            ),
            (
                row3[0],
                "Total data",
                format!(
                    "{:.2}kb | {:.2}mb",
                    m.bytes as f64 / 1024.0,
                    m.bytes as f64 / 1024.0 / 1024.0
                ),
                Color::LightYellow,
            ),
            (
                row3[1],
                "Data Transfer",
                format!("{:.2}kb/s", m.throughput(elapsed) / 1024.0),
                Color::Yellow,
            ),
        ];
        for (area, title, value, color) in widgets {
            self.render_stat_widget(f, area, title, value, color);
        }
    }

    fn render_stat_widget(
        &self,
        f: &mut Frame,
        area: Rect,
        title: &str,
        value: String,
        color: Color,
    ) {
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(value, Style::default().fg(color)))).block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(title.to_string()),
            ),
            area,
        );
    }

    fn latency_chart(&self, area_width: u16) -> BarChart<'static> {
        let bars: Vec<Bar> = LATENCY_PERCENTILES
            .iter()
            .map(|&p| {
                let latency = self.metrics.percentile(p as f64);
                Bar::default()
                    .label(Line::from(format!("P{p:02}")))
                    .value(latency.as_micros() as u64)
                    .text_value(format_ms(latency))
            })
            .collect();

        BarChart::default()
            .data(BarGroup::default().bars(&bars))
            .bar_width(bar_width(area_width, bars.len()))
            .bar_gap(1)
            .bar_style(Style::default().fg(Color::Cyan))
            .value_style(Style::default().fg(Color::Yellow))
            .label_style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title("Latency Distribution")
                    .borders(Borders::ALL),
            )
    }

    fn status_codes_chart(&self, area_width: u16) -> BarChart<'static> {
        let mut codes: Vec<(u16, u64)> = self
            .metrics
            .status_codes
            .iter()
            .map(|(code, count)| (*code, *count))
            .collect();
        for code in DEFAULT_STATUS_CODES {
            if !self.metrics.status_codes.contains_key(&code) {
                codes.push((code, 0));
            }
        }
        codes.sort_unstable();

        let bars: Vec<Bar> = codes
            .iter()
            .map(|&(code, count)| {
                let color = match code {
                    100..=199 => Color::Blue,
                    200..=299 => Color::Green,
                    300..=399 => Color::Magenta,
                    400..=499 => Color::Yellow,
                    500..=599 => Color::Red,
                    _ => Color::White,
                };
                Bar::default()
                    .label(Line::from(code.to_string()))
                    .value(count)
                    .style(Style::default().fg(color))
            })
            .collect();

        BarChart::default()
            .data(BarGroup::default().bars(&bars))
            .bar_width(bar_width(area_width, bars.len()))
            .bar_gap(1)
            .value_style(Style::default().fg(Color::Yellow))
            .label_style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title("Status Codes Distribution")
                    .borders(Borders::ALL),
            )
    }

    fn status_span(stat: &ResponseStats) -> Span<'static> {
        match (stat.status_code, stat.error) {
            (Some(code), _) => Span::styled(
                format!("[{code}]"),
                Style::default().fg(if code.is_success() {
                    Color::Green
                } else {
                    Color::Red
                }),
            ),
            (None, error) => Span::styled(
                format!("[{}]", error.map(|e| e.label()).unwrap_or("ERROR")),
                Style::default().fg(Color::Red),
            ),
        }
    }

    fn format_request_item(&self, stat: &ResponseStats) -> ListItem<'static> {
        ListItem::new(Line::from(vec![
            Self::status_span(stat),
            Span::raw(" "),
            Span::styled(
                self.args.method.clone(),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw(" "),
            Span::styled(format_ms(stat.duration), Style::default().fg(Color::Yellow)),
            Span::raw(" "),
            Span::styled(
                format!("{}b", stat.body_bytes),
                Style::default().fg(Color::Blue),
            ),
            Span::raw(" "),
            Span::styled(self.args.url.clone(), Style::default().fg(Color::White)),
        ]))
    }

    fn render_request_log(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        // Newest first, so the panel always shows the latest activity
        let items: Vec<ListItem> = self
            .recent
            .iter()
            .rev()
            .map(|req| self.format_request_item(req))
            .collect();
        f.render_widget(
            List::new(items).block(
                Block::default()
                    .title("Recent Requests")
                    .borders(Borders::ALL),
            ),
            chunks[0],
        );

        let partial_response_items: Vec<ListItem> = self
            .recent
            .iter()
            .rev()
            .filter_map(|req| {
                let partial = req.partial_response.as_ref()?;
                Some(ListItem::new(Line::from(vec![
                    Self::status_span(req),
                    Span::raw(" "),
                    Span::styled(partial.clone(), Style::default().fg(Color::White)),
                ])))
            })
            .collect();
        f.render_widget(
            List::new(partial_response_items).block(
                Block::default()
                    .title("Partial Responses")
                    .borders(Borders::ALL),
            ),
            chunks[1],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bar_width_never_underflows() {
        assert_eq!(bar_width(0, 9), 1);
        assert_eq!(bar_width(5, 9), 1);
        assert_eq!(bar_width(100, 0), 97);
        assert_eq!(bar_width(92, 9), 9);
    }

    #[test]
    fn progress_is_bounded_in_both_modes() {
        let secs = Duration::from_secs;
        // Count mode never exceeds 99% before the run reports finished
        assert_eq!(progress_percent(Plan::Count(10), 5, secs(1), false), 50);
        assert_eq!(progress_percent(Plan::Count(10), 50, secs(1), false), 99);
        assert_eq!(progress_percent(Plan::Count(10), 10, secs(1), true), 100);
        // Duration mode is time based, whatever the request count
        let d = Plan::Duration(secs(10));
        assert_eq!(progress_percent(d, 1_000_000, secs(5), false), 50);
        assert_eq!(progress_percent(d, 1_000_000, secs(60), false), 99);
        assert_eq!(progress_percent(Plan::Count(0), 0, secs(0), false), 99);
    }
}
