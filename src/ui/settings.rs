//! The settings page: the theme, and the defaults a run starts from. F2
//! opens it over any screen, and every change is written to the settings
//! file as it's made (see `crate::settings`). The theme changes what's on
//! screen at once; the defaults are read when pepe starts.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
    Frame,
};

use super::kit::{chips, fitting, marker, FAINT, FIELD, SELECTED};
use super::theme::{self, Theme};
use super::view::{label, truncate, ACCENT, BAD, GOOD, LABEL, WARN};
use crate::config::RampConfig;
use crate::settings::Settings;
use crate::utils::{default_user_agent, num_of_cores};

/// The line every screen's `?` help has for this page
pub(super) const HELP: (&str, &str) = ("F2", "settings: theme, defaults");

/// The widest the page is drawn, and the column its values start in
const WIDTH: u16 = 76;
const LABELS: usize = 24;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Item {
    Theme,
    Requests,
    Duration,
    Concurrency,
    Timeout,
    Threads,
    Rate,
    Warmup,
    UserAgent,
    Proxy,
    KeepAlive,
    Compression,
    Redirects,
    VerifyTls,
    RampFrom,
    RampTo,
    RampStep,
    RampEvery,
    AllowHost,
    MaxRequests,
    MaxRate,
    MaxConcurrency,
    UpdateCheck,
}

use Item::*;

const SECTIONS: [(&str, &[Item]); 5] = [
    ("Look", &[Theme]),
    (
        "Every run",
        &[
            Requests,
            Duration,
            Concurrency,
            Timeout,
            Threads,
            Rate,
            Warmup,
            UserAgent,
            Proxy,
            KeepAlive,
            Compression,
            Redirects,
            VerifyTls,
        ],
    ),
    ("Ramp", &[RampFrom, RampTo, RampStep, RampEvery]),
    (
        "Guardrails",
        &[AllowHost, MaxRequests, MaxRate, MaxConcurrency],
    ),
    ("pepe", &[UpdateCheck]),
];

fn items() -> impl Iterator<Item = Item> {
    SECTIONS.iter().flat_map(|(_, items)| items.iter().copied())
}

/// A whole number of at least one
fn count<T: std::str::FromStr + PartialOrd + From<u8>>(
    text: &str,
    what: &str,
) -> Result<T, String> {
    match text.parse::<T>() {
        Ok(n) if n >= T::from(1) => Ok(n),
        _ => Err(format!("{text:?} isn't {what}: a whole number, 1 or more")),
    }
}

fn per_second(text: &str) -> Result<f64, String> {
    match text.parse::<f64>() {
        Ok(rate) if rate.is_finite() && rate > 0.0 => Ok(rate),
        _ => Err(format!("{text:?} isn't a rate: requests a second, above 0")),
    }
}

fn time(text: &str) -> Result<String, String> {
    match crate::Cli::parse_duration(text) {
        Ok(ms) if ms > 0 => Ok(text.to_string()),
        _ => Err(format!("{text:?} isn't a time: 30s, 5m or 2h")),
    }
}

/// An on/off item's field in the settings, and the value that means off
type Switch = fn(&mut Settings) -> (&mut Option<bool>, bool);

impl Item {
    fn label(self) -> &'static str {
        match self {
            Theme => "Theme",
            Requests => "Requests",
            Duration => "Duration",
            Concurrency => "Concurrency",
            Timeout => "Timeout, seconds",
            Threads => "Threads",
            Rate => "Rate, a second",
            Warmup => "Warm-up",
            UserAgent => "User agent",
            Proxy => "Proxy",
            KeepAlive => "Keep-alive",
            Compression => "Compression",
            Redirects => "Follow redirects",
            VerifyTls => "Verify TLS",
            RampFrom => "From",
            RampTo => "To",
            RampStep => "Step",
            RampEvery => "Each step held",
            AllowHost => "Allowed hosts",
            MaxRequests => "Most requests",
            MaxRate => "Highest rate",
            MaxConcurrency => "Most connections",
            UpdateCheck => "Look for new releases",
        }
    }

    /// What the selected line says underneath: the flag it stands in for
    fn about(self) -> &'static str {
        match self {
            Theme => "the colours every screen is drawn in, from now on",
            Requests => "-n · how many requests a run sends",
            Duration => "-z · run for this long instead of a count: 30s, 5m",
            Concurrency => "-c · connections open at once",
            Timeout => "-t · how long a request may take",
            Threads => "--threads · a number, or auto to add them as needed",
            Rate => "--rate · requests started a second; unset, as fast as answered",
            Warmup => "--warmup · sent first and left out of the numbers: 5s",
            UserAgent => "-u · the User-Agent header",
            Proxy => "-p · send through this proxy",
            KeepAlive => "off: --disable-keepalive, a connection per request",
            Compression => "off: --disable-compression",
            Redirects => "off: --disable-redirects",
            VerifyTls => "off: -k, accept certificates that don't check out",
            RampFrom => "pepe ramp --from · the first step's concurrency",
            RampTo => "pepe ramp --to · the last step's concurrency",
            RampStep => "pepe ramp --step · added at each step",
            RampEvery => "pepe ramp --every · 10s, 1m",
            AllowHost => "--allow-host · only these may be loaded: .example.com, localhost",
            MaxRequests => "--max-requests · a run asking for more is refused",
            MaxRate => "--max-rate · and every run has to name a rate",
            MaxConcurrency => "--max-concurrency · -c and a ramp's top step",
            UpdateCheck => "off: never ask GitHub whether there is a newer pepe",
        }
    }

    /// Stepped through with ← →, where the others are typed
    fn choice(self) -> bool {
        self.switch().is_some() || self == Theme
    }

    /// For an on/off item: whether it's off, as the settings stand
    fn switch(self) -> Option<Switch> {
        // Each is on until its field holds the second value
        Some(match self {
            KeepAlive => |s| (&mut s.defaults.keep_alive, false),
            Compression => |s| (&mut s.defaults.compression, false),
            Redirects => |s| (&mut s.defaults.redirects, false),
            VerifyTls => |s| (&mut s.defaults.insecure, true),
            UpdateCheck => |s| (&mut s.update_check, false),
            _ => return None,
        })
    }

    /// What the settings hold for it; None when they leave it to pepe
    fn value(self, settings: &Settings) -> Option<String> {
        let d = &settings.defaults;
        let ramp = d.ramp.as_ref();
        if let Some(switch) = self.switch() {
            let mut held = settings.clone();
            let (field, off) = switch(&mut held);
            return (*field == Some(off)).then(|| "off".to_string());
        }
        match self {
            Theme => settings.theme.clone(),
            Requests => d.requests.map(|n| n.to_string()),
            Duration => d.duration.clone(),
            Concurrency => d.concurrency.map(|n| n.to_string()),
            Timeout => d.timeout.map(|n| n.to_string()),
            Threads => d.threads.map(|t| t.to_string()),
            Rate => d.rate.map(|r| r.to_string()),
            Warmup => d.warmup.clone(),
            UserAgent => d.user_agent.clone(),
            Proxy => d.proxy.clone(),
            RampFrom => ramp.and_then(|r| r.from).map(|n| n.to_string()),
            RampTo => ramp.and_then(|r| r.to).map(|n| n.to_string()),
            RampStep => ramp.and_then(|r| r.step).map(|n| n.to_string()),
            RampEvery => ramp.and_then(|r| r.every.clone()),
            AllowHost => d.allow_host.as_ref().map(|hosts| hosts.join(", ")),
            MaxRequests => d.max_requests.map(|n| n.to_string()),
            MaxRate => d.max_rate.map(|r| r.to_string()),
            MaxConcurrency => d.max_concurrency.map(|n| n.to_string()),
            _ => None,
        }
    }

    /// What pepe does when the settings leave it unsaid
    fn usual(self) -> String {
        match self {
            Theme => "auto".into(),
            Requests => "100".into(),
            Concurrency => format!("{} (this machine's cores)", num_of_cores()),
            Timeout => "20".into(),
            UserAgent => default_user_agent(),
            KeepAlive | Compression | Redirects | VerifyTls | UpdateCheck => "on".into(),
            RampFrom | RampStep => "10".into(),
            RampTo => "100".into(),
            RampEvery => "10s".into(),
            AllowHost => "any".into(),
            Threads => "1".into(),
            Rate => "as fast as answered".into(),
            Duration | Warmup | Proxy => "none".into(),
            MaxRequests | MaxRate | MaxConcurrency => "no cap".into(),
        }
    }

    /// Hold what was typed, or say what's wrong with it. Nothing typed
    /// leaves it to pepe again.
    fn set(self, settings: &mut Settings, text: &str) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            self.clear(settings);
            return Ok(());
        }
        let d = &mut settings.defaults;
        match self {
            Requests => d.requests = Some(count(text, "a number of requests")?),
            Duration => d.duration = Some(time(text)?),
            Concurrency => d.concurrency = Some(count(text, "a number of connections")?),
            Timeout => d.timeout = Some(count(text, "a number of seconds")?),
            Threads => d.threads = Some(text.parse()?),
            Rate => d.rate = Some(per_second(text)?),
            Warmup => d.warmup = Some(time(text)?),
            UserAgent => d.user_agent = Some(text.to_string()),
            Proxy => d.proxy = Some(text.to_string()),
            RampFrom | RampTo | RampStep | RampEvery => {
                let ramp = d.ramp.get_or_insert_with(RampConfig::default);
                match self {
                    RampFrom => ramp.from = Some(count(text, "a concurrency")?),
                    RampTo => ramp.to = Some(count(text, "a concurrency")?),
                    RampStep => ramp.step = Some(count(text, "a step")?),
                    _ => ramp.every = Some(time(text)?),
                }
            }
            AllowHost => {
                d.allow_host = Some(
                    text.split([',', ' '])
                        .filter(|host| !host.is_empty())
                        .map(str::to_string)
                        .collect(),
                )
            }
            MaxRequests => d.max_requests = Some(count(text, "a number of requests")?),
            MaxRate => d.max_rate = Some(per_second(text)?),
            MaxConcurrency => d.max_concurrency = Some(count(text, "a number of connections")?),
            Theme => {
                let theme = Theme::parse(text).ok_or_else(|| format!("no theme named {text:?}"))?;
                settings.theme = Some(theme.name().to_string());
            }
            KeepAlive | Compression | Redirects | VerifyTls | UpdateCheck => {}
        }
        // A ramp table with nothing in it isn't written
        if d_ramp_is_empty(settings) {
            settings.defaults.ramp = None;
        }
        Ok(())
    }

    /// Leave it to pepe again
    fn clear(self, settings: &mut Settings) {
        if let Some(switch) = self.switch() {
            *switch(settings).0 = None;
            return;
        }
        let d = &mut settings.defaults;
        match self {
            Theme => settings.theme = None,
            Requests => d.requests = None,
            Duration => d.duration = None,
            Concurrency => d.concurrency = None,
            Timeout => d.timeout = None,
            Threads => d.threads = None,
            Rate => d.rate = None,
            Warmup => d.warmup = None,
            UserAgent => d.user_agent = None,
            Proxy => d.proxy = None,
            AllowHost => d.allow_host = None,
            MaxRequests => d.max_requests = None,
            MaxRate => d.max_rate = None,
            MaxConcurrency => d.max_concurrency = None,
            RampFrom | RampTo | RampStep | RampEvery => {
                if let Some(ramp) = &mut d.ramp {
                    match self {
                        RampFrom => ramp.from = None,
                        RampTo => ramp.to = None,
                        RampStep => ramp.step = None,
                        _ => ramp.every = None,
                    }
                }
                if d_ramp_is_empty(settings) {
                    settings.defaults.ramp = None;
                }
            }
            _ => {}
        }
    }

    /// The next choice, `by` steps along and around
    fn step(self, settings: &mut Settings, by: i64) {
        if let Some(switch) = self.switch() {
            let (field, off) = switch(settings);
            *field = if *field == Some(off) { None } else { Some(off) };
            return;
        }
        if self == Theme {
            // "auto", then every theme
            let at = settings
                .theme
                .as_deref()
                .and_then(Theme::parse)
                .and_then(|t| Theme::ALL.iter().position(|a| *a == t))
                .map_or(0, |i| i as i64 + 1);
            let next = (at + by).rem_euclid(Theme::ALL.len() as i64 + 1);
            settings.theme = (next > 0).then(|| Theme::ALL[next as usize - 1].name().to_string());
        }
    }
}

fn d_ramp_is_empty(settings: &Settings) -> bool {
    settings.defaults.ramp.as_ref() == Some(&RampConfig::default())
}

/// What a key did to the page
#[derive(Debug, Default, PartialEq)]
struct Did {
    /// The settings are no longer what's in the file
    changed: bool,
    close: bool,
}

struct Page {
    settings: Settings,
    /// Where they're written; None when there's nowhere
    path: Option<PathBuf>,
    at: usize,
    /// What's being typed, on the selected line
    editing: Option<String>,
    /// What happened last, and whether it went wrong
    message: Option<(String, bool)>,
}

impl Page {
    fn open(settings: Settings, path: Option<PathBuf>) -> Self {
        Self {
            settings,
            path,
            at: 0,
            editing: None,
            message: None,
        }
    }

    fn item(&self) -> Item {
        items().nth(self.at).unwrap_or(Theme)
    }

    fn move_by(&mut self, by: i64) {
        let last = items().count() as i64 - 1;
        self.at = (self.at as i64 + by).clamp(0, last) as usize;
    }

    fn key(&mut self, key: KeyEvent) -> Did {
        // AltGr arrives as ctrl+alt on Windows, and types a character
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        let item = self.item();
        let changed = Did {
            changed: true,
            close: false,
        };
        if let Some(text) = &mut self.editing {
            match key.code {
                KeyCode::Esc => self.editing = None,
                KeyCode::Enter => {
                    let before = self.settings.clone();
                    match item.set(&mut self.settings, text) {
                        Ok(()) => {
                            self.editing = None;
                            self.message = None;
                            if self.settings != before {
                                return changed;
                            }
                        }
                        Err(e) => self.message = Some((e, true)),
                    }
                }
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Char('u') if ctrl => text.clear(),
                KeyCode::Char(c) if !ctrl => text.push(c),
                _ => {}
            }
            return Did::default();
        }
        self.message = None;
        let before = self.settings.clone();
        match key.code {
            KeyCode::Esc | KeyCode::F(2) | KeyCode::Char('q') => {
                return Did {
                    changed: false,
                    close: true,
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_by(-1),
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => self.move_by(1),
            KeyCode::BackTab => self.move_by(-1),
            KeyCode::PageUp => self.move_by(-8),
            KeyCode::PageDown => self.move_by(8),
            KeyCode::Home => self.at = 0,
            KeyCode::End => self.move_by(i64::MAX / 2),
            KeyCode::Left | KeyCode::Char('h') if item.choice() => {
                item.step(&mut self.settings, -1)
            }
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') | KeyCode::Enter
                if item.choice() =>
            {
                item.step(&mut self.settings, 1)
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
                self.editing = Some(item.value(&self.settings).unwrap_or_default())
            }
            KeyCode::Delete | KeyCode::Backspace | KeyCode::Char('d') => {
                item.clear(&mut self.settings)
            }
            _ => {}
        }
        if self.settings != before {
            return changed;
        }
        Did::default()
    }

    /// Text pasted in goes into the line being typed
    fn paste(&mut self, pasted: &str) {
        if let Some(text) = &mut self.editing {
            text.extend(pasted.trim().chars().filter(|c| !c.is_control()));
        }
    }

    // ─── Drawing ─────────────────────────────────────────────────────────────

    /// A line of the list, `width` cells wide
    fn row(&self, item: Item, selected: bool, width: usize) -> Line<'static> {
        let room = width.saturating_sub(LABELS + 2);
        let held = item.value(&self.settings);
        let mut spans = vec![
            marker(selected, true),
            Span::styled(
                format!(" {:<LABELS$}", item.label()),
                Style::new().fg(if selected { Color::Reset } else { LABEL }),
            ),
        ];
        match (&self.editing, selected) {
            (Some(text), true) => {
                // The end of what's typed stays in view
                let shown: String = {
                    let chars: Vec<char> = text.chars().collect();
                    let from = chars.len().saturating_sub(room.saturating_sub(2));
                    chars[from..].iter().collect()
                };
                let pad = room.saturating_sub(shown.chars().count() + 1);
                spans.push(Span::styled(shown, Style::new().bg(FIELD)));
                spans.push(Span::styled(" ", Style::new().bg(ACCENT)));
                spans.push(Span::styled(" ".repeat(pad), Style::new().bg(FIELD)));
            }
            _ => {
                let (text, style) = match held {
                    Some(text) => (text, Style::new().bold()),
                    None => (item.usual(), Style::new().fg(FAINT)),
                };
                let arrows = selected && item.choice();
                let text = truncate(&text, room.saturating_sub(if arrows { 4 } else { 0 }));
                if arrows {
                    spans.push(Span::styled("‹ ", Style::new().fg(ACCENT)));
                    spans.push(Span::styled(text, style));
                    spans.push(Span::styled(" ›", Style::new().fg(ACCENT)));
                } else {
                    spans.push(Span::styled(text, style));
                }
                if item == Theme {
                    spans.push(Span::raw("  "));
                    spans.extend(swatch());
                }
            }
        }
        let mut line = Line::from(spans);
        if selected {
            // The bar runs the width of the list
            let used = line.width();
            line.spans
                .push(Span::raw(" ".repeat(width.saturating_sub(used))));
            line = line.style(Style::new().bg(SELECTED));
        }
        line
    }

    /// Every line of the list, and which of them is the selected one
    fn lines(&self, width: usize) -> (Vec<Line<'static>>, usize) {
        let mut lines = Vec::new();
        let mut selected = 0;
        let mut index = 0;
        for (section, items) in SECTIONS {
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.push(Line::from(Span::styled(
                format!("  {}", section.to_uppercase()),
                Style::new().fg(LABEL).bold(),
            )));
            for item in items {
                if index == self.at {
                    selected = lines.len();
                }
                lines.push(self.row(*item, index == self.at, width));
                index += 1;
            }
        }
        (lines, selected)
    }

    /// What the selected line is, and what stands in its way
    fn about(&self) -> Line<'static> {
        let item = self.item();
        if item == Theme {
            if std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()) {
                return Line::from(Span::styled(
                    "  NO_COLOR is set: colour stays off until it isn't",
                    Style::new().fg(WARN),
                ));
            }
            if let Ok(named) = std::env::var("PEPE_THEME") {
                return Line::from(Span::styled(
                    format!("  PEPE_THEME={named} is set, and wins when pepe starts"),
                    Style::new().fg(WARN),
                ));
            }
        }
        Line::from(label(format!("  {}", item.about())))
    }

    /// Draws the page, and says where
    fn render(&self, f: &mut Frame) -> Rect {
        let area = f.area();
        let width = WIDTH.min(area.width);
        let inner_width = width.saturating_sub(2) as usize;
        let (lines, selected) = self.lines(inner_width);
        // The list, then what the line is, what happened, and the keys
        let height = (lines.len() as u16 + 6).min(area.height);
        let [popup] = Layout::horizontal([Constraint::Length(width)])
            .flex(Flex::Center)
            .areas(area);
        let [popup] = Layout::vertical([Constraint::Length(height)])
            .flex(Flex::Center)
            .areas(popup);
        f.render_widget(Clear, popup);
        let place = match &self.path {
            Some(path) => format!(
                " {} ",
                truncate(&tilde(path), inner_width.saturating_sub(14))
            ),
            None => " not saved: no home directory ".to_string(),
        };
        let block = Block::bordered()
            .border_type(BorderType::Rounded)
            .border_style(Style::new().fg(ACCENT))
            .title(Span::styled(" settings ", Style::new().fg(ACCENT).bold()))
            .title_top(Line::from(label(place)).right_aligned());
        let inner = block.inner(popup);
        f.render_widget(block, popup);
        let [list, _, about, message, keys] = Layout::vertical([
            Constraint::Min(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .areas(inner);

        // The selected line stays in view, the rest scrolls around it
        let room = list.height as usize;
        let top = (selected + 2)
            .saturating_sub(room)
            .min(lines.len().saturating_sub(room));
        let shown: Vec<Line> = lines.into_iter().skip(top).take(room).collect();
        f.render_widget(Paragraph::new(shown), list);

        f.render_widget(Paragraph::new(self.about()), about);
        let note = match &self.message {
            Some((text, wrong)) => Line::from(Span::styled(
                format!("  {}", truncate(text, inner_width.saturating_sub(2))),
                Style::new().fg(if *wrong { BAD } else { GOOD }),
            )),
            None if self.item() == Theme => Line::raw(""),
            None => Line::from(Span::styled(
                "  read when pepe starts · flags and a pepe.toml win",
                Style::new().fg(FAINT),
            )),
        };
        f.render_widget(Paragraph::new(note), message);
        let pairs: &[(&str, &str)] = if self.editing.is_some() {
            &[("enter", "keep"), ("esc", "leave as it was")]
        } else if self.item().choice() {
            &[
                ("↑↓", "move"),
                ("← →", "change"),
                ("del", "usual"),
                ("esc", "close"),
            ]
        } else {
            &[
                ("↑↓", "move"),
                ("enter", "type"),
                ("del", "usual"),
                ("esc", "close"),
            ]
        };
        f.render_widget(Paragraph::new(chips(&fitting(pairs, inner.width))), keys);
        popup
    }
}

/// The theme at a glance: the heatmap's ramp, then good, slow and failed
fn swatch() -> Vec<Span<'static>> {
    let mut spans: Vec<Span> = [237u8, 239, 241, 243, 245, 247, 250, 253, 255]
        .into_iter()
        .map(|heat| Span::styled("█", Style::new().fg(Color::Indexed(heat))))
        .collect();
    for color in [GOOD, WARN, BAD] {
        spans.push(Span::styled(" ●", Style::new().fg(color)));
    }
    spans
}

/// The path with the home directory as `~`
fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"));
    match home.and_then(|home| path.strip_prefix(home).ok().map(Path::to_path_buf)) {
        Some(rest) => format!("~/{}", rest.display()),
        None => path.display().to_string(),
    }
}

// ─── The page over whichever screen is up ────────────────────────────────────

thread_local! {
    /// The screens are drawn and take their keys on one thread
    static PAGE: RefCell<Option<Page>> = const { RefCell::new(None) };
}

/// Every screen hands its keys here first. True when the key was the
/// page's: F2 opens it, and while it's open every key but ctrl-c is its
/// own.
pub(super) fn key(key: KeyEvent) -> bool {
    let ctrl =
        key.modifiers.contains(KeyModifiers::CONTROL) && !key.modifiers.contains(KeyModifiers::ALT);
    if ctrl && key.code == KeyCode::Char('c') {
        return false;
    }
    // The export page (F4, opened by a run screen) and the webhook page
    // (F3) take keys the same way, while this one is shut
    if !PAGE.with(|page| page.borrow().is_some())
        && (super::export::key(key) || super::hooks::key(key))
    {
        return true;
    }
    PAGE.with(|page| {
        let mut page = page.borrow_mut();
        let Some(open) = page.as_mut() else {
            let opens = key.code == KeyCode::F(2);
            if opens {
                *page = Some(Page::open(crate::settings::get(), crate::settings::path()));
            }
            return opens;
        };
        let did = open.key(key);
        if did.changed {
            theme::choose(open.settings.theme.as_deref().and_then(Theme::parse));
            open.message = Some(match crate::settings::save(&open.settings) {
                Ok(path) => (format!("saved {}", tilde(&path)), false),
                Err(e) => (e, true),
            });
        }
        if did.close {
            *page = None;
        }
        true
    })
}

/// True when the page was open to take the pasted text
pub(super) fn paste(text: &str) -> bool {
    PAGE.with(|page| match page.borrow_mut().as_mut() {
        Some(open) => {
            open.paste(text);
            true
        }
        None => super::export::paste(text) || super::hooks::paste(text),
    })
}

/// Whether the page is up: the mouse is no screen's while it is
pub(super) fn is_open() -> bool {
    PAGE.with(|page| page.borrow().is_some()) || super::hooks::is_open() || super::export::is_open()
}

/// Draw the page over the frame, when it's open, and say where it is
pub(super) fn render(f: &mut Frame) -> Option<Rect> {
    PAGE.with(|page| page.borrow().as_ref().map(|open| open.render(f)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    fn press(page: &mut Page, code: KeyCode) -> Did {
        page.key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn type_in(page: &mut Page, text: &str) -> Did {
        for c in text.chars() {
            press(page, KeyCode::Char(c));
        }
        press(page, KeyCode::Enter)
    }

    fn go_to(page: &mut Page, item: Item) {
        page.at = items().position(|i| i == item).unwrap();
    }

    fn draw(page: &Page, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|f| {
                page.render(f);
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut text = String::new();
        for y in 0..height {
            for x in 0..width {
                text.push_str(buffer[(x, y)].symbol());
            }
            text.push('\n');
        }
        text
    }

    #[test]
    fn the_theme_steps_through_auto_and_every_theme() {
        let mut page = Page::open(Settings::default(), None);
        assert_eq!(page.item(), Theme);
        let mut seen = vec![];
        for _ in 0..Theme::ALL.len() {
            assert!(press(&mut page, KeyCode::Right).changed);
            seen.push(page.settings.theme.clone().unwrap());
        }
        let names: Vec<String> = Theme::ALL.iter().map(|t| t.name().to_string()).collect();
        assert_eq!(seen, names);
        // Around to auto, which is no theme named, and back the other way
        assert!(press(&mut page, KeyCode::Right).changed);
        assert_eq!(page.settings.theme, None);
        press(&mut page, KeyCode::Left);
        assert_eq!(page.settings.theme.as_deref(), Some("none"));
        assert!(press(&mut page, KeyCode::Delete).changed);
        assert_eq!(page.settings, Settings::default());
    }

    #[test]
    fn a_default_is_typed_checked_and_cleared() {
        let mut page = Page::open(Settings::default(), None);
        go_to(&mut page, Concurrency);
        assert_eq!(press(&mut page, KeyCode::Enter), Did::default());
        assert!(type_in(&mut page, "50").changed);
        assert_eq!(page.settings.defaults.concurrency, Some(50));
        assert!(page.editing.is_none());

        // What doesn't read is said, and the line stays open
        go_to(&mut page, Duration);
        press(&mut page, KeyCode::Enter);
        assert_eq!(type_in(&mut page, "soon"), Did::default());
        let (said, wrong) = page.message.clone().unwrap();
        assert!(wrong && said.contains("isn't a time"), "{said}");
        assert!(page.editing.is_some());
        press(&mut page, KeyCode::Esc);
        assert_eq!(page.settings.defaults.duration, None);

        // Keys that move the list are typed while a line is open
        go_to(&mut page, UserAgent);
        press(&mut page, KeyCode::Enter);
        assert!(type_in(&mut page, "jq/1").changed);
        assert_eq!(page.settings.defaults.user_agent.as_deref(), Some("jq/1"));

        // Typed over with nothing, or del: pepe's own again
        go_to(&mut page, Concurrency);
        press(&mut page, KeyCode::Enter);
        press(&mut page, KeyCode::Backspace);
        press(&mut page, KeyCode::Backspace);
        assert!(press(&mut page, KeyCode::Enter).changed);
        assert_eq!(page.settings.defaults.concurrency, None);
        go_to(&mut page, UserAgent);
        assert!(press(&mut page, KeyCode::Char('d')).changed);
        assert_eq!(page.settings, Settings::default());
    }

    #[test]
    fn switches_hold_only_what_differs_from_pepe() {
        let mut page = Page::open(Settings::default(), None);
        go_to(&mut page, VerifyTls);
        assert!(press(&mut page, KeyCode::Char(' ')).changed);
        assert_eq!(page.settings.defaults.insecure, Some(true));
        assert_eq!(VerifyTls.value(&page.settings).as_deref(), Some("off"));
        press(&mut page, KeyCode::Left);
        assert_eq!(page.settings.defaults.insecure, None);
        go_to(&mut page, KeepAlive);
        press(&mut page, KeyCode::Enter);
        assert_eq!(page.settings.defaults.keep_alive, Some(false));
        go_to(&mut page, UpdateCheck);
        press(&mut page, KeyCode::Right);
        assert_eq!(page.settings.update_check, Some(false));
    }

    #[test]
    fn guardrails_and_the_ramp_are_kept_as_a_pepe_toml_has_them() {
        let mut page = Page::open(Settings::default(), None);
        go_to(&mut page, AllowHost);
        press(&mut page, KeyCode::Enter);
        type_in(&mut page, ".staging.example.com, localhost");
        go_to(&mut page, MaxRate);
        press(&mut page, KeyCode::Enter);
        type_in(&mut page, "500");
        go_to(&mut page, RampTo);
        press(&mut page, KeyCode::Enter);
        type_in(&mut page, "200");
        let d = &page.settings.defaults;
        assert_eq!(
            d.allow_host.as_deref(),
            Some(&[".staging.example.com".to_string(), "localhost".to_string()][..])
        );
        assert_eq!(d.max_rate, Some(500.0));
        assert_eq!(d.ramp.as_ref().and_then(|r| r.to), Some(200));
        let text = page.settings.to_toml();
        assert_eq!(crate::settings::parse(&text).unwrap(), page.settings);
        // The last of the ramp cleared takes its table with it
        press(&mut page, KeyCode::Delete);
        assert_eq!(page.settings.defaults.ramp, None);

        go_to(&mut page, MaxConcurrency);
        press(&mut page, KeyCode::Enter);
        assert!(!type_in(&mut page, "0").changed);
        assert!(page.message.as_ref().is_some_and(|(_, wrong)| *wrong));
    }

    #[test]
    fn the_page_closes_on_esc_and_on_the_key_that_opened_it() {
        let mut page = Page::open(Settings::default(), None);
        assert!(press(&mut page, KeyCode::Esc).close);
        assert!(press(&mut page, KeyCode::F(2)).close);
        // But esc on a line being typed only leaves the line
        go_to(&mut page, Proxy);
        press(&mut page, KeyCode::Enter);
        assert!(!press(&mut page, KeyCode::Esc).close);
        assert!(!press(&mut page, KeyCode::Char('q')).close || page.editing.is_none());
    }

    #[test]
    fn it_draws_what_is_held_and_what_pepe_does_otherwise() {
        let mut page = Page::open(
            Settings {
                theme: Some("nord".into()),
                ..Default::default()
            },
            Some(PathBuf::from("/etc/pepe/settings.toml")),
        );
        let text = draw(&page, 100, 40);
        for part in [
            "settings",
            "/etc/pepe/settings.toml",
            "LOOK",
            "‹ nord ›",
            "EVERY RUN",
            "GUARDRAILS",
            "no cap",
            "Look for new releases",
        ] {
            assert!(text.contains(part), "{part:?} in\n{text}");
        }
        // On a short terminal the list scrolls to the selected line
        go_to(&mut page, UpdateCheck);
        let text = draw(&page, 80, 16);
        assert!(text.contains("Look for new releases"), "{text}");
        assert!(!text.contains("EVERY RUN"), "{text}");
        go_to(&mut page, Requests);
        page.editing = Some("2500".into());
        let text = draw(&page, 60, 12);
        assert!(text.contains("2500") && text.contains("keep"), "{text}");
    }
}
