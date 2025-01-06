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

use crate::{ResponseStats, Stats};

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

pub struct Dashboard {
    // Add storage fields to hold the strings and data
    label_storage: Vec<String>,
    bar_chart_data: Vec<(String, u64)>,
    histogram: Vec<(String, u64)>,
    requests: Vec<ResponseStats>,
    total_requests: usize,
    url: String,
    method: String,
    concurrency: usize,
    status_codes: HashMap<StatusCode, usize>,
    stats: Stats,
    elapsed: f64,
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

        // Update status codes
        *self.status_codes.entry(stat.status_code).or_insert(0) += 1;

        // Update stats
        self.stats.count += 1;
        if stat.status_code.is_success() {
            self.stats.success += 1;
        } else {
            self.stats.failed += 1;
        }

        // Update elapsed time
        self.elapsed = stat.elapsed.as_secs_f64();
        self.total_requests = stat.total_requests;
        self.url = stat.url;
        self.method = format!("{:?}", stat.method);
        self.concurrency = stat.concurrency;
    }
    pub fn new() -> Self {
        Self {
            bar_chart_data: Vec::new(),
            histogram: Vec::new(),
            requests: Vec::with_capacity(100),
            status_codes: HashMap::new(),
            stats: Stats::default(),
            elapsed: 0.0,
            total_requests: 0,
            url: String::new(),
            method: String::new(),
            concurrency: 0,
            label_storage: Vec::with_capacity(10),
        }
    }

    pub fn run(
        &mut self,
        rx: &mut mpsc::Receiver<ResponseStats>,
    ) -> Result<KeyCode, Box<dyn std::error::Error>> {
        enable_raw_mode()?;
        let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;

        loop {
            // Update stats
            while let Ok(stat) = rx.try_recv() {
                self.update_stats(stat);
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
                        return Ok(key.code);
                    }
                }
            }
        }
    }

    fn calculate_stats(&self, latencies: &[u64]) -> Vec<(&str, u64, Style)> {
        if latencies.is_empty() {
            return vec![("No Data", 0, Style::default())];
        }

        // sort the latencies
        let mut latencies = latencies.to_vec();
        latencies.sort_unstable();

        let sum: u64 = latencies.iter().sum();
        let len = latencies.len() as u64;
        let avg = sum / len;
        let min = *latencies.first().unwrap();
        let max = *latencies.last().unwrap();

        // Calculate standard deviation
        let variance = latencies
            .iter()
            .map(|&x| {
                let diff = x as i64 - avg as i64;
                (diff * diff) as u64
            })
            .sum::<u64>()
            / len;
        let std_dev = (variance as f64).sqrt() as u64;

        vec![
            ("Min", min, Style::default().fg(Color::Green)),
            ("Avg", avg, Style::default().fg(Color::White)),
            ("Max", max, Style::default().fg(Color::Red)),
            ("StdDev", std_dev, Style::default().fg(Color::Yellow)),
            (
                "RPS",
                if self.elapsed > 0.0 {
                    (self.stats.count as f64 / self.elapsed as f64) as u64
                } else {
                    0
                },
                Style::default().fg(Color::Cyan),
            ),
        ]
    }

    fn format_request_item(&self, stat: &ResponseStats) -> ListItem {
        let style = if stat.status_code.is_success() {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };

        ListItem::new(Line::from(vec![
            Span::styled(format!("[{}]", stat.status_code), style),
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
                format!("{:?}", stat.method),
                Style::default().fg(Color::Magenta),
            ),
            Span::raw(" "),
            Span::styled(stat.url.clone(), Style::default().fg(Color::White)),
        ]))
    }

    fn render_latency_distribution<'a>(
        &'a mut self, // Changed to &mut self to modify label_storage
        latencies: &[u64],
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
                    let label = format!("P{:02}: {:.2}ms", p, ms);
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

        BarChart::default()
            .data(&data)
            .bar_width(7)
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
                Constraint::Percentage(20),
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
        ];

        f.render_widget(
            Paragraph::new(LOGO)
                .style(Style::default().fg(Color::Cyan))
                .block(Block::default().borders(Borders::ALL)),
            chunks[0],
        );

        f.render_widget(
            Paragraph::new(version_info)
                .block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Info")
                        .title_style(Style::default().fg(Color::White)),
                )
                .style(Style::default().fg(Color::Cyan)),
            chunks[1],
        );

        // Parameters section
        let params = vec![
            Line::from(vec![
                Span::styled("URL: ", Style::default().fg(Color::Yellow)),
                Span::raw(&self.url),
            ]),
            Line::from(vec![
                Span::styled("Method: ", Style::default().fg(Color::Yellow)),
                Span::raw(&self.method),
            ]),
            Line::from(vec![
                Span::styled("Concurrency: ", Style::default().fg(Color::Yellow)),
                Span::raw(self.concurrency.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Total Requests: ", Style::default().fg(Color::Yellow)),
                Span::raw(self.total_requests.to_string()),
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
            chunks[2],
        );
    }

    fn render_progress(&self, f: &mut Frame, area: Rect) {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(20), Constraint::Percentage(80)])
            .split(area);

        let percent = (self.stats.count * 100) / self.total_requests.max(1);

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

        // Duration widget remains unchanged
        let formatted_duration = format!(
            "{:02}:{:02}:{:02}",
            self.elapsed as u64 / 3600,
            self.elapsed as u64 % 3600 / 60,
            self.elapsed as u64 % 60
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
            Span::styled("Success: ", Style::default().fg(Color::Green)),
            Span::raw(self.stats.success.to_string()),
            Span::raw(" | "),
            Span::styled("Failed: ", Style::default().fg(Color::Red)),
            Span::raw(self.stats.failed.to_string()),
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
                Constraint::Percentage(50),
                Constraint::Percentage(25),
                Constraint::Percentage(25),
            ])
            .split(area);

        // Render latency distribution
        let latencies: Vec<u64> = self.histogram.iter().map(|(_, latency)| *latency).collect();
        let latency_chart = self.render_latency_distribution(&latencies);
        f.render_widget(latency_chart, chunks[0]);

        // Render stats
        let stats = self.calculate_stats(&latencies);
        let stats = stats
            .iter()
            .map(|(label, value, style)| {
                Line::from(vec![
                    Span::styled(
                        format!("{:<8}", *label),
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(": "),
                    Span::styled(
                        format!("{:>8}", value.to_string()),
                        style.clone().add_modifier(Modifier::BOLD),
                    ),
                ])
            })
            .collect::<Vec<_>>();

        let stats_paragraph = Paragraph::new(stats)
            .block(Block::default().title("Stats").borders(Borders::ALL))
            .wrap(ratatui::widgets::Wrap { trim: false });

        f.render_widget(stats_paragraph, chunks[1]);

        // Render status codes distribution
        let status_chart = self.render_status_codes();
        f.render_widget(status_chart, chunks[2]);
    }

    fn render_status_codes(&self) -> BarChart {
        let data: Vec<(String, u64)> = self
            .status_codes
            .iter()
            .map(|(code, count)| (format!("{:?}", code), *count as u64))
            .collect();

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
            .bar_width(7)
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
                ListItem::new(Line::from(vec![
                    Span::styled(
                        format!("[{}]", req.status_code),
                        if req.status_code.is_success() {
                            Style::default().fg(Color::Green)
                        } else {
                            Style::default().fg(Color::Red)
                        },
                    ),
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
            List::new(partial_response_items)
                .block(
                    Block::default()
                        .title("Partial Responses")
                        .borders(Borders::ALL),
                )
                .highlight_style(Style::default().add_modifier(Modifier::BOLD)),
            chunks[1],
        );
    }

    fn render_layout(&mut self, f: &mut Frame) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(8),  // Header
                Constraint::Length(3),  // Progress
                Constraint::Length(5),  // Stats
                Constraint::Length(15), // Charts
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
