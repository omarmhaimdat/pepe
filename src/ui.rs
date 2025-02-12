use std::{collections::HashMap, num::NonZeroUsize};

use crossterm::{
    event::{self, Event, KeyCode},
    terminal::enable_raw_mode,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    widgets::{BarChart, BarGroup, Block, Borders, List, Paragraph},
    Frame, Terminal,
};
use reqwest::StatusCode;
use std::thread::available_parallelism;
use tokio::sync::mpsc;

use crate::{Args, CacheCategory, ResponseStats, Sent};

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use ratatui::widgets::ListItem;

const LOGO: &str = r#"
    ██████╗ ███████╗██████╗ ███████╗
    ██╔══██╗██╔════╝██╔══██╗██╔════╝
    ██████╔╝█████╗  ██████╔╝█████╗  
    ██╔═══╝ ██╔══╝  ██╔═══╝ ██╔══╝  
    ██║     ███████╗██║     ███████╗
    ╚═╝     ╚══════╝╚═╝     ╚══════╝"#;

#[derive(Default)]
struct Stats {
    count: usize,
    success: usize,
    failed: usize,
    timeouts: usize,
    sent: usize,
    min: u64,
    max: u64,
    avg: u64,
    std_dev: u64,
    rps: u64,
    data: u64,
    total_dns_lookup: Vec<u128>,
    total_dns_resolution: Vec<u128>,
    avg_dns_lookup: u128,
    avg_dns_resolution: u128,
    cache_categories: HashMap<CacheCategory, usize>,
}
pub struct Dashboard {
    // Add storage fields to hold the strings and data
    label_storage: Vec<String>,
    bar_chart_data: Vec<(String, u64)>,
    histogram: Vec<(String, u64)>,
    requests: Vec<ResponseStats>,
    args: Args,
    status_codes: HashMap<StatusCode, usize>,
    stats: Stats,
    elapsed: std::time::Instant,
    final_duration: Option<std::time::Duration>,
    data_transfer: f64,
}

impl Dashboard {
    fn update_stats(&mut self, stat: ResponseStats) {
        // Update histogram
        self.histogram.push((
            format!("{:?}", stat.status_code),
            stat.duration.as_millis() as u64,
        ));

        // Update requests
        if self.requests.len() == 100 {
            self.requests.remove(0);
        }
        self.requests.push(stat.clone());

        // Update cache categories
        if let Some(ref cache_status) = stat.cache_status {
            *self
                .stats
                .cache_categories
                .entry(CacheCategory::from_cache_status(&cache_status))
                .or_insert(0) += 1;
        }

        let dns_times = stat.dns_times;
        if let Some(dns_times) = dns_times {
            let (dns_lookup_time, dns_resolution_time) = dns_times;
            self.stats
                .total_dns_lookup
                .push(dns_lookup_time.as_millis());
            self.stats
                .total_dns_resolution
                .push(dns_resolution_time.as_millis());
            self.stats.avg_dns_lookup = self.stats.total_dns_lookup.iter().sum::<u128>()
                / self.stats.total_dns_lookup.len() as u128;
            self.stats.avg_dns_resolution = self.stats.total_dns_resolution.iter().sum::<u128>()
                / self.stats.total_dns_resolution.len() as u128;
        }

        if self.stats.min == 0 || stat.duration.as_millis() < self.stats.min.into() {
            self.stats.min = stat.duration.as_millis() as u64;
        }

        if stat.duration.as_millis() > self.stats.max.into() {
            self.stats.max = stat.duration.as_millis() as u64;
        }

        self.stats.rps = if self.elapsed.elapsed().as_secs() > 0 {
            (self.stats.count as f64 / self.elapsed.elapsed().as_secs() as f64) as u64
        } else {
            0
        };

        self.stats.data = (self.data_transfer / self.elapsed.elapsed().as_secs() as f64) as u64;

        // If status code is None, it means the request timed out
        if stat.status_code.is_none() {
            self.stats.timeouts += 1;
            self.stats.count += 1;
            self.requests.push(stat);
            return;
        }

        let status_code = stat.status_code.unwrap();

        // Update status codes
        *self.status_codes.entry(status_code).or_insert(0) += 1;
        self.data_transfer += stat.content_length.unwrap_or(0) as f64;

        // Update stats
        self.stats.count += 1;
        if status_code.is_success() {
            self.stats.success += 1;
        } else {
            self.stats.failed += 1;
        }
    }

    pub fn new(args: Args) -> Self {
        Self {
            bar_chart_data: Vec::new(),
            histogram: Vec::new(),
            requests: Vec::with_capacity(args.number as usize),
            status_codes: HashMap::new(),
            stats: Stats::default(),
            elapsed: std::time::Instant::now(),
            label_storage: Vec::with_capacity(10),
            data_transfer: 0.0,
            final_duration: None,
            args,
        }
    }

    pub fn run(
        &mut self,
        rx: &mut mpsc::Receiver<ResponseStats>,
        sent_rx: &mut mpsc::Receiver<Sent>,
    ) -> Result<KeyCode, Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

        terminal.clear()?;

        loop {
            // Update stats
            while let Ok(stat) = rx.try_recv() {
                self.update_stats(stat);
            }

            while let Ok(sent) = sent_rx.try_recv() {
                self.update_sent(sent);
            }

            terminal.draw(|f| {
                self.render_layout(f);
            })?;

            if event::poll(std::time::Duration::from_millis(250))? {
                if let Event::Key(key) = event::read()? {
                    if matches!(
                        key.code,
                        KeyCode::Char('q')
                            | KeyCode::Char('r')
                            | KeyCode::Esc
                            | KeyCode::Enter
                            | KeyCode::Char('i')
                    ) {
                        terminal.clear()?;
                        return Ok(key.code);
                    }
                }
            }
        }
    }

    fn update_sent(&mut self, sent: Sent) {
        self.stats.sent += sent.count;
    }

    fn calculate_stats(&mut self, latencies: &[u64]) {
        if latencies.is_empty() {
            return;
        }

        // sort the latencies
        let mut latencies = latencies.to_vec();
        latencies.sort_unstable();

        // Calculate standard deviation
        let variance = latencies
            .iter()
            .map(|&x| {
                let diff = x as i64 - self.stats.avg as i64;
                (diff * diff) as u64
            })
            .sum::<u64>()
            / self.stats.count as u64;
        let std_dev = (variance as f64).sqrt() as u64;
        self.stats.std_dev = std_dev;

        // Calculate average
        let avg = latencies.iter().sum::<u64>() / self.stats.count as u64;
        self.stats.avg = avg;
    }

    fn format_request_item(&self, stat: &ResponseStats) -> ListItem {
        if stat.status_code.is_none() {
            return ListItem::new(Line::from(vec![
                Span::styled("[TIMEOUT]", Style::default().fg(Color::Red)),
                Span::raw(" "),
                Span::styled(
                    format!("{:.2}ms", stat.duration.as_millis()),
                    Style::default().fg(Color::Yellow),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{}b", stat.content_length.unwrap_or(0)),
                    Style::default().fg(Color::Blue),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("{:?}", self.args.method),
                    Style::default().fg(Color::Magenta),
                ),
                Span::raw(" "),
                Span::styled(format!("{:?}", self.args.url), Style::default().fg(Color::White)),
            ]));
        }

        let status_code = stat.status_code.unwrap();

        let style = if status_code.is_success() {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };

        ListItem::new(Line::from(vec![
            Span::styled(format!("[{}]", status_code), style),
            Span::raw(" "),
            Span::styled(
                format!("{:.2}ms", stat.duration.as_millis()),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{}b", stat.content_length.unwrap_or(0)),
                Style::default().fg(Color::Blue),
            ),
            Span::raw(" "),
            Span::styled(
                format!("{:?}", self.args.method),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw(" "),
            Span::styled(format!("{:?}", self.args.url), Style::default().fg(Color::White)),
        ]))
    }

    fn render_latency_distribution<'a>(
        &'a mut self, // Changed to &mut self to modify label_storage
        latencies: &[u64],
        area_width: u16,
    ) -> BarChart<'a> {
        let percentiles = [0, 10, 25, 50, 75, 90, 95, 99, 100];
        let mut sorted_latencies = latencies.to_vec();
        sorted_latencies.sort_unstable();

        // Clear previous storage
        self.bar_chart_data = if !sorted_latencies.is_empty() {
            percentiles
                .iter()
                .map(|&p| {
                    let idx = if p == 0 {
                        0
                    } else if p == 100 {
                        sorted_latencies.len() - 1
                    } else {
                        let idx_float = (p as f64 / 100.0) * (sorted_latencies.len() - 1) as f64;
                        (idx_float.round() as usize).min(sorted_latencies.len() - 1)
                    };

                    let ms = sorted_latencies[idx] as f64 / 1000.0;
                    let label = format!("P{:02}: {:.2}s", p, ms);
                    self.label_storage.push(label);
                    (
                        self.label_storage.last().unwrap().clone(),
                        sorted_latencies[idx],
                    )
                })
                .collect::<Vec<_>>()
        } else {
            self.label_storage.push("No Data".to_string());
            vec![(self.label_storage[0].clone(), 0)]
        };

        let data: Vec<(&str, u64)> = self
            .bar_chart_data
            .iter()
            .map(|(s, u)| (s.as_str(), *u))
            .collect();

        // Calculate the width of each bar, if it's not possible to divide equally, use the maximum width
        // Make sure division lefts no remainder
        let each_bar_width = (area_width as usize / data.len()) - 1;

        BarChart::default()
            .data(&data)
            .bar_width(each_bar_width as u16)
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

    fn render_header(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(20),
                Constraint::Percentage(10),
                Constraint::Percentage(10),
                Constraint::Percentage(60),
            ])
            .split(area);

        let mut n_threads = 8;
        if let Some(default_n_threads) = NonZeroUsize::new(8) {
            n_threads = available_parallelism()
                .unwrap_or(NonZeroUsize::new(8).unwrap_or(default_n_threads))
                .get();
        }

        // Logo section
        f.render_widget(
            Paragraph::new(LOGO)
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL)),
            chunks[0],
        );

        // Commands section
        let commands: Vec<Line<'_>> = vec![
            Line::from(vec![
                Span::styled("Quit: ", Style::default().fg(Color::Yellow)),
                Span::raw("q"),
            ]),
            Line::from(vec![
                Span::styled("Restart: ", Style::default().fg(Color::Yellow)),
                Span::raw("r"),
            ]),
            Line::from(vec![
                Span::styled("Interrupt: ", Style::default().fg(Color::Yellow)),
                Span::raw("i"),
            ]),
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

        // Info section
        let version_info = vec![
            Line::from(vec![
                Span::styled("Version: ", Style::default().fg(Color::Yellow)),
                Span::raw(env!("CARGO_PKG_VERSION")),
            ]),
            Line::from(vec![
                Span::styled("Author: ", Style::default().fg(Color::Yellow)),
                Span::raw(env!("CARGO_PKG_AUTHORS")),
            ]),
            Line::from(vec![
                Span::styled("OS: ", Style::default().fg(Color::Yellow)),
                Span::raw(std::env::consts::OS),
            ]),
            Line::from(vec![
                Span::styled("Arch: ", Style::default().fg(Color::Yellow)),
                Span::raw(std::env::consts::ARCH),
            ]),
            Line::from(vec![
                Span::styled("Cores: ", Style::default().fg(Color::Yellow)),
                Span::raw(n_threads.to_string()),
            ]),
            Line::from(vec![
                Span::styled("PID: ", Style::default().fg(Color::Yellow)),
                Span::raw(std::process::id().to_string()),
            ]),
        ];

        f.render_widget(
            Paragraph::new(version_info)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Info")
                        .title_style(Style::default().fg(Color::White)),
                )
                .style(Style::default().fg(Color::Cyan)),
            chunks[2],
        );

        // Parameters section
        let params = vec![
            Line::from(vec![
                Span::styled("URL: ", Style::default().fg(Color::Yellow)),
                Span::raw(&self.args.url),
            ]),
            Line::from(vec![
                Span::styled("Method: ", Style::default().fg(Color::Yellow)),
                Span::raw(&self.args.method),
            ]),
            Line::from(vec![
                Span::styled("Concurrency: ", Style::default().fg(Color::Yellow)),
                Span::raw(self.args.concurrency.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Total Requests: ", Style::default().fg(Color::Yellow)),
                Span::raw(self.args.number.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Timeout: ", Style::default().fg(Color::Yellow)),
                Span::raw(self.args.timeout.to_string()),
            ]),
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

    fn render_progress(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
            .split(area);

        let percent = (self.stats.count * 100) / (self.args.number as usize).max(1);

        // Fixed elements: borders(2) + "Progress"(8) + "[]"(2) + "100%"(4) + safety margin(2)
        let fixed_elements = 18;
        let bar_width = chunks[1].width.saturating_sub(fixed_elements) as usize;
        let filled = (percent as usize * bar_width) / 100;
        let progress_bar = format!(
            "{} [{}{}] {}%",
            "Progress",
            "=".repeat(filled),
            " ".repeat(bar_width - filled),
            percent
        );
        if self.stats.count == (self.args.number as usize)  && self.final_duration.is_none() {
            self.final_duration = Some(std::time::Instant::now() - self.elapsed);
        }

        // Duration widget remains unchanged
        if let Some(duration) = self.final_duration {
            let formatted_duration = format!(
                "{:02}h:{:02}m:{:02}s:{:03}ms",
                duration.as_secs() as u64 / 3600,
                duration.as_secs() as u64 % 3600 / 60,
                duration.as_secs() as u64 % 60,
                duration.subsec_millis()
            );
            f.render_widget(
                Paragraph::new(vec![Line::from(vec![
                    Span::styled("Duration: ", Style::default().fg(Color::Yellow)),
                    Span::raw(formatted_duration),
                ])])
                .block(Block::default().borders(Borders::ALL)),
                chunks[0],
            );
            f.render_widget(
                Paragraph::new(progress_bar)
                    .style(Style::default().fg(Color::Green))
                    .block(Block::default().borders(Borders::ALL)),
                chunks[1],
            );
            return;
        }
        let formatted_duration = format!(
            "{:02}h:{:02}m:{:02}s:{:03}ms",
            // Compute seconds from self.elapsed, now - self.elapsed
            (std::time::Instant::now() - self.elapsed).as_secs() as u64 / 3600,
            (std::time::Instant::now() - self.elapsed).as_secs() as u64 % 3600 / 60,
            (std::time::Instant::now() - self.elapsed).as_secs() as u64 % 60,
            (std::time::Instant::now() - self.elapsed).subsec_millis()
        );
        f.render_widget(
            Paragraph::new(vec![Line::from(vec![
                Span::styled("Duration: ", Style::default().fg(Color::Yellow)),
                Span::raw(formatted_duration),
            ])])
            .block(Block::default().borders(Borders::ALL)),
            chunks[0],
        );

        f.render_widget(
            Paragraph::new(progress_bar)
                .style(Style::default().fg(Color::Green))
                .block(Block::default().borders(Borders::ALL)),
            chunks[1],
        );
    }

    fn render_stats(&self, f: &mut Frame, area: Rect) {
        let stats = vec![Line::from(vec![
            Span::styled("Total: ", Style::default().fg(Color::Yellow)),
            Span::raw(self.stats.count.to_string()),
            Span::raw(" | "),
            Span::styled("Remaining: ", Style::default().fg(Color::LightYellow)),
            Span::raw(((self.args.number as usize) - self.stats.count).to_string()),
            Span::raw(" | "),
            Span::styled("Success: ", Style::default().fg(Color::Green)),
            Span::raw(self.stats.success.to_string()),
            Span::raw(" | "),
            Span::styled("Failed: ", Style::default().fg(Color::LightRed)),
            Span::raw(self.stats.failed.to_string()),
            Span::raw(" | "),
            Span::styled("Timeouts: ", Style::default().fg(Color::Red)),
            Span::raw(self.stats.timeouts.to_string()),
            Span::raw(" | "),
            Span::styled("Sent: ", Style::default().fg(Color::Cyan)),
            Span::raw(self.stats.sent.to_string()),
        ])];

        f.render_widget(
            Paragraph::new(stats).block(Block::default().title("Statistics").borders(Borders::ALL)),
            area,
        );
    }

    fn render_charts(&mut self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(50),
            ])
            .split(area);

        // Render latency distribution
        let latencies: Vec<u64> = self.histogram.iter().map(|(_, latency)| *latency).collect();
        let latency_chart = self.render_latency_distribution(&latencies, chunks[2].width);
        f.render_widget(latency_chart, chunks[2]);

        // Render stats
        self.calculate_stats(&latencies);

        let statistics_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(chunks[0]);

        let min_max_avg_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ])
            .split(statistics_chunks[0]);

        let stats_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(33),
                Constraint::Percentage(33),
                Constraint::Percentage(34),
            ])
            .split(statistics_chunks[1]);

        let dns_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(statistics_chunks[2]);

        let data_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(statistics_chunks[3]);

        self.render_stat_widget(
            f,
            min_max_avg_chunks[0],
            "Min",
            format!("{:.2}ms", self.stats.min as f64),
            Color::Green,
        );
        self.render_stat_widget(
            f,
            min_max_avg_chunks[1],
            "Max",
            format!("{:.2}ms", self.stats.max as f64),
            Color::Red,
        );
        self.render_stat_widget(
            f,
            min_max_avg_chunks[2],
            "Avg",
            format!("{:.2}ms", self.stats.avg as f64),
            Color::Yellow,
        );
        self.render_stat_widget(
            f,
            stats_chunks[0],
            "Std Dev",
            format!("{:.2}ms", self.stats.std_dev as f64),
            Color::Cyan,
        );
        self.render_stat_widget(
            f,
            stats_chunks[1],
            "Request per Second",
            self.stats.rps.to_string(),
            Color::Magenta,
        );
        self.render_stat_widget(
            f,
            stats_chunks[2],
            "Cache Hit Rate",
            if self.stats.count > 0 {
                format!(
                    "{:.2}%",
                    (*self
                        .stats
                        .cache_categories
                        .get(&CacheCategory::Hit)
                        .unwrap_or(&0) as f64
                        / self.stats.count as f64)
                        * 100.0
                )
            } else {
                "0%".to_string()
            },
            Color::Green,
        );

        self.render_stat_widget(
            f,
            dns_chunks[0],
            "Avg DNS Lookup",
            format!("{:.2}ms", self.stats.avg_dns_lookup as f64),
            Color::LightMagenta,
        );
        self.render_stat_widget(
            f,
            dns_chunks[1],
            "Avg DNS Resolution",
            format!("{:.2}ms", self.stats.avg_dns_resolution as f64),
            Color::LightMagenta,
        );
        self.render_stat_widget(
            f,
            data_chunks[0],
            "Total data",
            format!(
                "{:.2}kb | {:.2}mb",
                self.data_transfer / 1024.0,
                self.data_transfer / 1024.0 / 1024.0
            ),
            Color::LightYellow,
        );

        self.render_stat_widget(
            f,
            data_chunks[1],
            "Data Transfer",
            format!("{:.2}kb/s", (self.stats.data as f64) / 1024.0),
            Color::Yellow,
        );

        // Render status codes distribution
        let status_chart = self.render_status_codes(chunks[1].width);
        f.render_widget(status_chart, chunks[1]);
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
            Paragraph::new(vec![Line::from(vec![Span::styled(
                value,
                Style::default().fg(color),
            )])])
            .block(Block::default().borders(Borders::ALL).title(title)),
            area,
        );
    }

    fn render_status_codes(&self, area_width: u16) -> BarChart {
        let mut data: Vec<(String, u64)> = self
            .status_codes
            .iter()
            .map(|(code, count)| (format!("{:?}", code), *count as u64))
            .collect();

        let list_of_default_status = vec![
            StatusCode::OK,
            StatusCode::BAD_REQUEST,
            StatusCode::NOT_FOUND,
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::SERVICE_UNAVAILABLE,
        ];

        // At least the ones in the default list should be present, if not, add them
        for status in list_of_default_status {
            if !self.status_codes.contains_key(&status) {
                data.push((format!("{:?}", status), 0));
            }
        }
        let each_bar_width = (area_width as usize / data.len()) - 1;

        BarChart::default()
            .data(
                BarGroup::default().bars(
                    data.iter()
                        .map(|(label, value)| {
                            ratatui::widgets::Bar::default()
                                .label(Line::from(label.clone()))
                                .value(*value)
                        })
                        .collect::<Vec<_>>()
                        .as_slice(),
                ),
            )
            .bar_width(each_bar_width as u16)
            .bar_gap(1)
            .bar_style(Style::default().fg(Color::Cyan))
            .value_style(Style::default().fg(Color::Yellow))
            .label_style(Style::default().fg(Color::White))
            .block(
                Block::default()
                    .title("Status Codes Distribution")
                    .borders(Borders::ALL),
            )
    }

    fn render_request_log(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area);

        let items: Vec<ListItem> = self
            .requests
            .iter()
            .map(|req| self.format_request_item(req))
            .collect();

        f.render_widget(
            List::new(items)
                .block(
                    Block::default()
                        .title("Recent Requests")
                        .borders(Borders::ALL),
                )
                .highlight_style(Style::default().add_modifier(Modifier::BOLD)),
            chunks[0],
        );

        let partial_response_items: Vec<ListItem> = self
            .requests
            .iter()
            .filter(|req| req.partial_response.is_some())
            .map(|req| {
                let status_text = if req.status_code.is_none() {
                    "[TIMEOUT]".to_string()
                } else {
                    let status_code = req.status_code.unwrap();
                    format!("[{}]", status_code)
                };

                let status_style =
                    if req.status_code.is_none() || !req.status_code.unwrap().is_success() {
                        Style::default().fg(Color::Red)
                    } else {
                        Style::default().fg(Color::Green)
                    };

                ListItem::new(Line::from(vec![
                    Span::styled(status_text, status_style),
                    Span::raw(" "),
                    Span::styled(
                        req.partial_response
                            .as_ref()
                            .unwrap_or(&String::new())
                            .clone(),
                        Style::default().fg(Color::White),
                    ),
                ]))
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

    fn render_layout(&mut self, f: &mut Frame) {
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
}
