//! What the screens are drawn with: the mode's band, cards with a tag for a
//! title and a hard shadow, tags, key chips, the bar on the selected line.
//! The look is the site's: square edges, ink and cream, the pepper's red.

use std::cell::RefCell;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Flex, Layout, Position, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
    Frame,
};

use super::view::{label, value, ACCENT, LABEL, RULE, WARN};
use crate::store::{self, Store};
use crate::Cli;

/// Background of the selected line
pub(super) const SELECTED: Color = Color::Indexed(237);
/// Background of the field being typed in
pub(super) const FIELD: Color = Color::Indexed(236);
/// What's off, or not sent: quieter than a label, still 4.8:1 on a dark
/// background
pub(super) const FAINT: Color = Color::Indexed(245);
/// What's written on a band or a badge: the pepper's ink, in every theme
pub(super) const INK: Color = Color::Indexed(16);
/// A card's hard shadow: the accent on a dark ground, the ink on a light one
pub(super) const SHADOW: Color = Color::Indexed(88);

/// A mode's band, the same colour in every theme, as on the site. The
/// palette has one for each mode (a ping's lime, a ramp's gold, the setup's
/// red: `theme::pepe::BANDS`); the screens take theirs one by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Band {
    /// A load test: ember
    Run,
}

impl Band {
    pub(super) fn color(self) -> Color {
        Color::Indexed(match self {
            Band::Run => 208,
        })
    }
}

/// A tag: a word in a block of the text's colour, the site's labels and
/// selections. Reverse video, so it reads in every theme, and with no
/// colour at all.
pub(super) fn tag_style() -> Style {
    Style::new()
        .fg(Color::Reset)
        .bg(Color::Reset)
        .add_modifier(Modifier::REVERSED | Modifier::BOLD)
}

/// ` WORD `, as a tag
pub(super) fn tag(text: &str) -> Span<'static> {
    Span::styled(format!(" {} ", text.to_uppercase()), tag_style())
}

/// A badge: ` WORD ` in ink on `color`, a verdict's level
pub(super) fn badge(text: &str, color: Color) -> Span<'static> {
    Span::styled(
        format!(" {} ", text.to_uppercase()),
        Style::new().fg(INK).bg(color).bold(),
    )
}

/// The red square at a selected tag's corner: a quarter cell just under its
/// right-hand end, drawn only where nothing else is
pub(super) fn corner(buf: &mut Buffer, x: u16, y: u16) {
    if let Some(cell) = buf.cell_mut((x, y)) {
        if cell.symbol() == " " {
            cell.set_char('▘').set_fg(ACCENT);
        }
    }
}

/// The mode's band across the top of `area`, and the checker under it, on
/// the mascot's grid: two rows. `left` and `right` are written in ink; the
/// band gives the screen its mode at a glance, as on the site.
pub(super) fn band(
    f: &mut Frame,
    area: Rect,
    band: Band,
    left: Vec<Span<'static>>,
    right: Vec<Span<'static>>,
) {
    let color = band.color();
    let [top, checker] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(area);
    let ink = |spans: Vec<Span<'static>>| -> Line<'static> {
        Line::from(
            spans
                .into_iter()
                .map(|s| {
                    let bold = s.style.add_modifier.contains(Modifier::BOLD);
                    let style = Style::new().fg(INK).bg(color);
                    Span::styled(s.content, if bold { style.bold() } else { style })
                })
                .collect::<Vec<_>>(),
        )
    };
    let mut lead = vec![
        Span::styled(" ", Style::new().bg(color)),
        Span::styled(" pepe ", Style::new().fg(color).bg(INK).bold()),
        Span::styled(" ", Style::new().bg(color)),
    ];
    lead.extend(ink(left).spans);
    f.render_widget(
        Paragraph::new(Line::from(lead)).style(Style::new().bg(color)),
        top,
    );
    let mut right = ink(right);
    right.spans.push(Span::styled(" ", Style::new().bg(color)));
    f.render_widget(
        Paragraph::new(right).alignment(ratatui::layout::Alignment::Right),
        top,
    );
    // Two cells a square, the band's colour and the ground's in turn: the
    // site's strip between sections
    let buf = f.buffer_mut();
    for x in checker.left()..checker.right() {
        if let Some(cell) = buf.cell_mut((x, checker.y)) {
            let on = ((x - checker.x) / 2) % 2 == 0;
            cell.set_char('▀');
            if on {
                cell.set_fg(color).set_bg(Color::Reset);
            } else {
                cell.set_fg(Color::Reset).set_bg(color);
            }
        }
    }
}

/// A card: the panel surface in a square frame, its title a tag set into
/// the top border, and with `shadow` the site's hard shadow, half a cell
/// right and below, drawn just outside `area` where it fits. The frame sits
/// in the panel's padding, so what's drawn inside at `view::inset(area, 2,
/// 1)` doesn't move.
pub(super) fn card(f: &mut Frame, area: Rect, title: Option<&str>, focused: bool, shadow: bool) {
    super::view::panel(f, area);
    if area.width < 4 || area.height < 2 {
        return;
    }
    f.render_widget(
        Block::bordered()
            .border_type(BorderType::Plain)
            .border_style(Style::new().fg(if focused { ACCENT } else { RULE })),
        area,
    );
    if let Some(title) = title {
        let room = area.width.saturating_sub(4);
        let text = format!(" {} ", title.to_uppercase());
        let text: String = text.chars().take(room as usize).collect();
        f.render_widget(
            Paragraph::new(Span::styled(text, tag_style())),
            Rect {
                x: area.x + 2,
                y: area.y,
                width: room,
                height: 1,
            },
        );
    }
    if shadow {
        let buf = f.buffer_mut();
        let bounds = buf.area;
        let mut put = |x: u16, y: u16, c: char| {
            if bounds.contains(Position::new(x, y)) {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_char(c).set_fg(SHADOW).set_bg(Color::Reset);
                }
            }
        };
        let (right, bottom) = (area.right(), area.bottom());
        put(right, area.y, '▗');
        for y in area.y + 1..bottom {
            put(right, y, '▐');
        }
        put(area.x, bottom, '▝');
        for x in area.x + 1..right {
            put(x, bottom, '▀');
        }
        put(right, bottom, '▘');
    }
}

/// A square card with its title in the top border, and `right` at the
/// other end of it
pub(super) fn panel(
    title: Line<'static>,
    right: Option<Line<'static>>,
    border: Color,
) -> Block<'static> {
    let mut block = Block::bordered()
        .border_type(BorderType::Plain)
        .border_style(Style::new().fg(border))
        .title_top(title);
    if let Some(right) = right {
        block = block.title_top(right.right_aligned());
    }
    block
}

/// A card's title, a tag: in the text's colour when the card has the
/// keys, quieter when it hasn't
pub(super) fn caption(text: &str, focused: bool) -> Line<'static> {
    if focused {
        return Line::from(tag(text));
    }
    Line::from(Span::styled(
        format!(" {} ", text.to_uppercase()),
        tag_style().fg(LABEL),
    ))
}

/// The bar at the left of the selected line, lit in the pane with the keys
pub(super) fn marker(active: bool, focused: bool) -> Span<'static> {
    match (active, focused) {
        (true, true) => Span::styled("▌", Style::new().fg(ACCENT)),
        (true, false) => Span::styled("▌", Style::new().fg(FAINT)),
        _ => Span::raw(" "),
    }
}

/// Keys and what they do, the keys as small caps on a chip
pub(super) fn chips(pairs: &[(&'static str, &'static str)]) -> Line<'static> {
    let mut spans = Vec::with_capacity(pairs.len() * 2);
    for (key, action) in pairs {
        spans.push(Span::styled(format!(" {key} "), tag_style()));
        spans.push(label(format!(" {action}  ")));
    }
    Line::from(spans)
}

/// What a chip takes on the line: ` key ` then ` action  `
fn chip_width(key: &str, action: &str) -> usize {
    key.chars().count() + action.chars().count() + 5
}

/// The chips that fit `width`: the screen's own keys in order while they
/// fit, and always the last two — `?` keys and the way out — at the end
pub(super) fn fitting(
    pairs: &[(&'static str, &'static str)],
    width: u16,
) -> Vec<(&'static str, &'static str)> {
    let width = width as usize;
    let keep = pairs.len().min(2);
    let (rest, tail) = pairs.split_at(pairs.len() - keep);
    let mut used: usize = tail.iter().map(|(k, a)| chip_width(k, a)).sum();
    let mut shown: Vec<(&'static str, &'static str)> = Vec::with_capacity(pairs.len());
    for pair in rest {
        if used + chip_width(pair.0, pair.1) > width {
            break;
        }
        used += chip_width(pair.0, pair.1);
        shown.push(*pair);
    }
    shown.extend_from_slice(tail);
    shown
}

/// A footer narrower than this keeps its whole width for the screen's keys
const ROOM_FOR_SETTINGS: u16 = 56;
/// And narrower than this has no room for the webhook page's key
const ROOM_FOR_WEBHOOKS: u16 = 96;
/// Nor, on a run screen, for the export page's
const ROOM_FOR_EXPORT: u16 = 120;

/// The footer drawn at `area`: the key chips that fit, each clickable as
/// its key, and at the right-hand end the key that opens the settings.
/// Every screen's footer goes through here.
pub(super) fn footer(f: &mut Frame, area: Rect, pairs: &[(&'static str, &'static str)]) {
    // ` F2 ` and ` settings`, with a space between it and the keys; and
    // before it ` F3 ` and ` webhooks` when there's room
    let mut keys = area;
    let export = super::export::is_here();
    for (key, action, code, room) in [
        ("F2", "settings", 2, ROOM_FOR_SETTINGS),
        ("F3", "webhooks", 3, ROOM_FOR_WEBHOOKS),
        ("F4", "export", 4, ROOM_FOR_EXPORT),
    ] {
        if area.width < room || (code == 4 && !export) {
            break;
        }
        let width = (key.len() + action.len() + 3) as u16;
        let at = Rect {
            x: keys.right() - width,
            width,
            ..keys
        };
        hit(at, Hit::Key(KeyEvent::from(KeyCode::F(code))));
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(format!(" {key} "), tag_style()),
                label(format!(" {action}")),
            ])),
            at,
        );
        keys = Rect {
            width: keys.width - width - 1,
            ..keys
        };
    }
    let shown = fitting(pairs, keys.width);
    let mut x = keys.x;
    for (key, action) in &shown {
        let width = chip_width(key, action) as u16;
        if let Some(pressed) = key_for_chip(key) {
            hit(
                Rect {
                    x,
                    y: keys.y,
                    width,
                    height: 1,
                },
                Hit::Key(pressed),
            );
        }
        x = x.saturating_add(width);
    }
    f.render_widget(Paragraph::new(chips(&shown)), keys);
}

/// The key a chip stands for, when it stands for one: "enter", "esc",
/// "ctrl-s", "F1", a letter. "↑↓" and "j/k" stand for none.
pub(super) fn key_for_chip(label: &str) -> Option<KeyEvent> {
    let code = match label {
        "enter" => KeyCode::Enter,
        "esc" => KeyCode::Esc,
        "tab" => KeyCode::Tab,
        "space" => KeyCode::Char(' '),
        "F1" => KeyCode::F(1),
        "F4" => KeyCode::F(4),
        _ => {
            if let Some(rest) = label.strip_prefix("ctrl-") {
                let c = one_char(rest)?;
                return Some(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
            }
            KeyCode::Char(one_char(label)?)
        }
    };
    Some(KeyEvent::from(code))
}

fn one_char(text: &str) -> Option<char> {
    let mut chars = text.chars();
    let c = chars.next()?;
    chars.next().is_none().then_some(c)
}

/// The tab row drawn at `area`, each tab clickable, the picked one with
/// the red corner under its end
pub(super) fn tab_bar(f: &mut Frame, area: Rect, titles: &[String], selected: usize) {
    tabs_at(area, titles);
    f.render_widget(Paragraph::new(tabs(titles, selected)), area);
    let end: u16 = titles
        .iter()
        .take(selected + 1)
        .map(|t| t.chars().count() as u16 + 2)
        .sum::<u16>()
        + selected as u16;
    if area.x + end < area.right() {
        corner(f.buffer_mut(), area.x + end, area.y + 1);
    }
}

/// Where `tabs` puts each title on a line drawn at `area`, so a click
/// finds its tab; for a tab row drawn as part of a bigger paragraph
pub(super) fn tabs_at(area: Rect, titles: &[String]) {
    let mut x = area.x;
    for (i, title) in titles.iter().enumerate() {
        if i > 0 {
            x = x.saturating_add(1);
        }
        let width = title.chars().count() as u16 + 2;
        hit(
            Rect {
                x,
                y: area.y,
                width,
                height: 1,
            },
            Hit::Tab(i),
        );
        x = x.saturating_add(width);
    }
}

// ─── The mouse ───────────────────────────────────────────────────────────

/// What a click lands on. A screen registers regions while it draws a
/// frame (`theme::draw` forgets the last frame's) and looks a click up
/// afterwards, so what's clickable is exactly what's on screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Hit {
    /// Acts as this key, pressed
    Key(KeyEvent),
    /// The nth tab of the tab row
    Tab(usize),
    /// The nth entry of a list; `list` tells one screen's lists apart
    Row { list: u8, index: usize },
}

thread_local! {
    static HITS: RefCell<Vec<(Rect, Hit)>> = const { RefCell::new(Vec::new()) };
}

/// Forget the last frame's regions
pub(super) fn hits_clear() {
    HITS.with(|hits| hits.borrow_mut().clear());
}

/// What a click on `area` does
pub(super) fn hit(area: Rect, what: Hit) {
    if area.width > 0 && area.height > 0 {
        HITS.with(|hits| hits.borrow_mut().push((area, what)));
    }
}

/// `count` rows of a list drawn from the top of `area`, a line each, the
/// first of them entry `first`
pub(super) fn hit_rows(area: Rect, list: u8, first: usize, count: usize) {
    for i in 0..count.min(area.height as usize) {
        hit(
            Rect {
                y: area.y + i as u16,
                height: 1,
                ..area
            },
            Hit::Row {
                list,
                index: first + i,
            },
        );
    }
}

/// What was drawn under this cell, the last drawn winning (an overlay
/// over a list)
pub(super) fn hit_at(column: u16, row: u16) -> Option<Hit> {
    HITS.with(|hits| {
        hits.borrow()
            .iter()
            .rev()
            .find(|(area, _)| area.contains(Position::new(column, row)))
            .map(|(_, hit)| *hit)
    })
}

#[cfg(test)]
pub(super) fn hits() -> Vec<(Rect, Hit)> {
    HITS.with(|hits| hits.borrow().clone())
}

/// What a mouse event amounts to: the wheel is the arrow keys, as the
/// terminal made it before the mouse was captured, and a left click is
/// what was drawn under it. Moves, drags and releases are nothing.
pub(super) fn click(event: &MouseEvent) -> Option<Hit> {
    match event.kind {
        MouseEventKind::ScrollUp => Some(Hit::Key(KeyEvent::from(KeyCode::Up))),
        MouseEventKind::ScrollDown => Some(Hit::Key(KeyEvent::from(KeyCode::Down))),
        MouseEventKind::Down(MouseButton::Left) => hit_at(event.column, event.row),
        _ => None,
    }
}

/// A button going down, which closes an overlay wherever it lands
pub(super) fn pressed(event: &MouseEvent) -> bool {
    matches!(event.kind, MouseEventKind::Down(_))
}

/// Two clicks on the same thing this close together are a double click
const DOUBLE_CLICK: Duration = Duration::from_millis(500);

/// Tells the second click on a row from the first
#[derive(Debug, Default)]
pub(super) struct Clicks {
    last: Option<(Hit, Instant)>,
}

impl Clicks {
    /// Note a click on `hit`; true when it's the second on the same thing
    /// within the double-click time
    pub(super) fn second(&mut self, hit: Hit, now: Instant) -> bool {
        let again = matches!(self.last, Some((was, when))
            if was == hit && now.duration_since(when) <= DOUBLE_CLICK);
        self.last = if again { None } else { Some((hit, now)) };
        again
    }
}

/// A row of views or modes in small capitals, the selected one a tag. Every
/// screen draws its tabs this way; a number goes in the title only where
/// the number keys pick the tab.
pub(super) fn tabs(titles: &[String], selected: usize) -> Line<'static> {
    let mut spans = Vec::with_capacity(titles.len() * 2);
    for (i, title) in titles.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(if i == selected {
            tag(title)
        } else {
            Span::styled(
                format!(" {} ", title.to_uppercase()),
                Style::new().fg(LABEL),
            )
        });
    }
    Line::from(spans)
}

/// The `?` overlay: every key on this screen with what it does, then notes
/// on anything that needs explaining. `?` or esc closes it, on every screen.
pub(super) fn help(f: &mut Frame, area: Rect, rows: &[(&str, &str)], notes: &[String]) {
    // The settings page opens from every screen: its key goes above the
    // last two, which close the help and leave
    let mut rows = rows.to_vec();
    rows.insert(rows.len().saturating_sub(2), super::settings::HELP);
    rows.insert(rows.len().saturating_sub(2), super::hooks::HELP);
    if super::export::is_here() {
        rows.insert(rows.len().saturating_sub(2), super::export::HELP);
    }
    let mut lines: Vec<Line> = rows
        .iter()
        .map(|(key, action)| {
            Line::from(vec![
                Span::styled(format!("  {key:<12}"), Style::new().fg(ACCENT).bold()),
                Span::raw(action.to_string()),
            ])
        })
        .collect();
    if !notes.is_empty() {
        lines.push(Line::raw(""));
        for note in notes {
            lines.push(Line::from(label(format!("  {note}"))));
        }
    }
    let width = 60.min(area.width);
    let height = (lines.len() as u16 + 2).min(area.height);
    let [popup] = Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .areas(area);
    let [popup] = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .areas(popup);
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines).block(
            Block::bordered()
                .border_type(BorderType::Plain)
                .border_style(Style::new().fg(ACCENT))
                .title(tag("keys")),
        ),
        popup,
    );
}

/// The line under every screen's help: which pepe, where
pub(super) fn about_line() -> String {
    format!(
        "pepe {} · {}/{} · {} cores · {}",
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
        crate::utils::num_of_cores(),
        gethostname::gethostname().to_string_lossy()
    )
}

/// A pane's title and a rule to the edge, lit when the pane has the keys
pub(super) fn pane_title(
    f: &mut Frame,
    area: Rect,
    title: &str,
    right: Option<Line<'static>>,
    focused: bool,
) {
    let title = format!("{} ", title.to_uppercase());
    let right_width = right.as_ref().map_or(0, |r| r.width() + 1);
    let rule = (area.width as usize).saturating_sub(title.chars().count() + right_width);
    let mut spans = vec![
        Span::styled(
            title,
            Style::new().fg(if focused { ACCENT } else { LABEL }).bold(),
        ),
        Span::styled("─".repeat(rule), Style::new().fg(RULE)),
    ];
    if let Some(right) = right {
        spans.push(Span::raw(" "));
        spans.extend(right.spans);
    }
    f.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Text on lines of at most `width`, broken between words where it can be;
/// what doesn't fit `max_lines` is cut
pub(super) fn wrap(text: &str, width: usize, max_lines: usize) -> Vec<String> {
    let width = width.max(8);
    let mut lines: Vec<String> = Vec::new();
    let mut line: Vec<char> = Vec::new();
    for word in text.split_whitespace() {
        let mut word: Vec<char> = word.chars().collect();
        loop {
            let space = usize::from(!line.is_empty());
            if line.len() + space + word.len() <= width {
                if space == 1 {
                    line.push(' ');
                }
                line.append(&mut word);
                break;
            }
            // A word longer than a line fills this one and carries on
            let room = width.saturating_sub(line.len() + space);
            if word.len() > width && room > 0 {
                if space == 1 {
                    line.push(' ');
                }
                line.extend(word.drain(..room));
            }
            lines.push(line.drain(..).collect());
        }
    }
    if !line.is_empty() {
        lines.push(line.into_iter().collect());
    }
    if lines.len() > max_lines {
        lines.truncate(max_lines);
        if let Some(last) = lines.last_mut() {
            let kept: String = last.chars().take(width - 1).collect();
            *last = format!("{kept}…");
        }
    }
    lines
}

/// One line of text being edited
#[derive(Debug, Default, Clone, PartialEq)]
pub(super) struct TextInput {
    chars: Vec<char>,
    cursor: usize,
}

impl TextInput {
    pub(super) fn new(text: &str) -> Self {
        let chars: Vec<char> = text.chars().collect();
        TextInput {
            cursor: chars.len(),
            chars,
        }
    }

    pub(super) fn text(&self) -> String {
        self.chars.iter().collect()
    }

    pub(super) fn insert(&mut self, text: &str) {
        for c in text.chars().filter(|c| !c.is_control()) {
            self.chars.insert(self.cursor, c);
            self.cursor += 1;
        }
    }

    /// Edit or move; false when the key isn't one for the text
    pub(super) fn key(&mut self, key: KeyEvent) -> bool {
        // AltGr arrives as ctrl+alt on Windows, and types a character
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('u') if ctrl => {
                self.chars.drain(..self.cursor);
                self.cursor = 0;
            }
            KeyCode::Char('w') if ctrl => {
                let mut start = self.cursor;
                while start > 0 && self.chars[start - 1] == ' ' {
                    start -= 1;
                }
                while start > 0 && self.chars[start - 1] != ' ' {
                    start -= 1;
                }
                self.chars.drain(start..self.cursor);
                self.cursor = start;
            }
            KeyCode::Char('a') if ctrl => self.cursor = 0,
            KeyCode::Char('e') if ctrl => self.cursor = self.chars.len(),
            KeyCode::Char(c) if !ctrl => {
                self.chars.insert(self.cursor, c);
                self.cursor += 1;
            }
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.chars.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.chars.len() => {
                self.chars.remove(self.cursor);
            }
            KeyCode::Backspace | KeyCode::Delete => {}
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.chars.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.chars.len(),
            _ => return false,
        }
        true
    }

    /// The text on lines of `width`, the `max_lines` of them around the
    /// cursor
    pub(super) fn wrapped(&self, width: usize, max_lines: usize) -> Vec<Vec<Span<'static>>> {
        let width = width.max(2);
        // The cursor can sit one past the end
        let count = self.chars.len() / width + 1;
        let first = (self.cursor / width + 1).saturating_sub(max_lines.max(1));
        (first..count.min(first + max_lines.max(1)))
            .map(|line| {
                let start = line * width;
                let end = self.chars.len().min(start + width);
                let text = |range: std::ops::Range<usize>| -> String {
                    self.chars[range].iter().collect()
                };
                if (start..start + width).contains(&self.cursor) {
                    let under = if self.cursor < end {
                        text(self.cursor..self.cursor + 1)
                    } else {
                        " ".to_string()
                    };
                    vec![
                        Span::raw(text(start..self.cursor)),
                        Span::styled(under, Style::new().add_modifier(Modifier::REVERSED)),
                        Span::raw(text((self.cursor + 1).min(end)..end)),
                    ]
                } else {
                    vec![Span::raw(text(start..end))]
                }
            })
            .collect()
    }

    /// The part that fits `width` with the cursor in view, as spans
    pub(super) fn spans(&self, width: usize, hidden: bool) -> Vec<Span<'static>> {
        let width = width.max(2);
        let start = (self.cursor + 1).saturating_sub(width);
        let end = self.chars.len().min(start + width);
        let show = |range: std::ops::Range<usize>| -> String {
            if hidden {
                "•".repeat(range.len())
            } else {
                self.chars[range].iter().collect()
            }
        };
        let under = if self.cursor < end {
            show(self.cursor..self.cursor + 1)
        } else {
            " ".to_string()
        };
        vec![
            Span::raw(show(start..self.cursor)),
            Span::styled(under, Style::new().add_modifier(Modifier::REVERSED)),
            Span::raw(show((self.cursor + 1).min(end)..end)),
        ]
    }
}

// ─── Saving a test ───────────────────────────────────────────────────────

/// What a key did in the save-as prompt
#[derive(Debug, PartialEq, Eq)]
pub(super) enum SaveKey {
    /// Enter: save under the name
    Save,
    /// Esc: close the prompt, nothing saved
    Cancel,
    /// Ctrl-C
    Quit,
    /// The name changed, or nothing
    Typed,
}

/// The prompt that keeps a test under a name, the same on every screen
/// that has one to keep: the setup form, a run, a ramp, the library's
/// copy. It takes the status line: the name being typed, then what enter
/// does or what's wrong with the name. A taken name asks once before it's
/// replaced; a test saved under its own name again goes straight through.
pub(super) struct SaveAs {
    pub(super) input: TextInput,
    /// "save as", "copy as"
    lead: &'static str,
    /// The name is taken and replacing it was said once; enter again does
    replace: bool,
    /// What to say after the name: (text, is an error)
    pub(super) message: Option<(String, bool)>,
}

impl SaveAs {
    pub(super) fn new(lead: &'static str, offered: &str) -> Self {
        SaveAs {
            input: TextInput::new(offered),
            lead,
            replace: false,
            message: None,
        }
    }

    pub(super) fn name(&self) -> String {
        self.input.text().trim().to_string()
    }

    pub(super) fn key(&mut self, key: KeyEvent) -> SaveKey {
        // AltGr arrives as ctrl+alt on Windows, and types a character
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL)
            && !key.modifiers.contains(KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('c') if ctrl => return SaveKey::Quit,
            KeyCode::Esc => return SaveKey::Cancel,
            KeyCode::Enter => return SaveKey::Save,
            _ => {
                if self.input.key(key) {
                    self.replace = false;
                }
            }
        }
        self.message = None;
        SaveKey::Typed
    }

    pub(super) fn paste(&mut self, text: &str) {
        self.input.insert(text.trim());
        self.replace = false;
        self.message = None;
    }

    /// Enter: save `cli` under the name, in `store`. `saved_as` is the name
    /// the settings already have, which replaces without asking. The name
    /// saved, with `message` saying so, or none and `message` saying why
    /// the prompt stays open.
    pub(super) fn save(
        &mut self,
        store: &Store,
        saved_as: Option<&str>,
        cli: &Cli,
    ) -> Option<String> {
        let name = self.name();
        if let Err(e) = store::validate_name(&name) {
            self.message = Some((e, true));
            return None;
        }
        if store.has(&name) && !self.replace && saved_as != Some(name.as_str()) {
            self.replace = true;
            self.message = Some((
                format!("`{name}` is saved already · enter replaces it, esc keeps it"),
                true,
            ));
            return None;
        }
        match store.save(&name, cli) {
            Ok(path) => {
                self.message = Some((
                    format!("saved {} · `pepe --load {name}` runs it", path.display()),
                    false,
                ));
                Some(name)
            }
            Err(e) => {
                self.message = Some((e, true));
                None
            }
        }
    }

    /// The status line while the prompt is open
    pub(super) fn line(&self, store: &Store, width: u16) -> Line<'static> {
        let lead = format!(" {} ", self.lead);
        let hint = match &self.message {
            Some((text, _)) => format!("  {text}"),
            None => format!(
                "  enter saves to {} · esc cancels",
                store.tests_dir().display()
            ),
        };
        let room = (width as usize)
            .saturating_sub(lead.chars().count() + hint.chars().count())
            .max(12);
        let mut spans = vec![value(lead, ACCENT)];
        spans.extend(self.input.spans(room, false));
        spans.push(Span::styled(
            hint,
            Style::new().fg(if matches!(&self.message, Some((_, true))) {
                WARN
            } else {
                LABEL
            }),
        ));
        Line::from(spans)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn the_settings_key_has_the_right_hand_end_of_a_footer_with_room() {
        let keys = [("r", "restart"), ("?", "keys"), ("q", "quit")];
        let draw = |width: u16| {
            hits_clear();
            let mut terminal = Terminal::new(TestBackend::new(width, 1)).unwrap();
            terminal.draw(|f| footer(f, f.area(), &keys)).unwrap();
            let buffer = terminal.backend().buffer().clone();
            (0..width)
                .map(|x| buffer[(x, 0)].symbol().to_string())
                .collect::<String>()
        };
        let line = draw(80);
        assert!(line.ends_with(" F2  settings"), "{line:?}");
        assert!(line.starts_with(" r  restart"), "{line:?}");
        // A click on it is the key, as on any chip
        assert_eq!(hit_at(79, 0), Some(Hit::Key(KeyEvent::from(KeyCode::F(2)))));
        assert_eq!(
            hit_at(0, 0),
            Some(Hit::Key(KeyEvent::from(KeyCode::Char('r'))))
        );
        // Too narrow, the line is the screen's own keys
        let line = draw(40);
        assert!(!line.contains("F2") && line.contains("quit"), "{line:?}");
        assert_eq!(hit_at(39, 0), None);
    }

    #[test]
    fn the_prompt_saves_asks_before_replacing_and_refuses_bad_names() {
        let root = std::env::temp_dir().join(format!("pepe-saveas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let store = Store::at(&root);
        let cli = Cli::parse_from(["pepe", "-c", "3", "https://x.io/"]);

        let mut prompt = SaveAs::new("save as", "x.io");
        assert_eq!(
            prompt.key(KeyEvent::from(KeyCode::Backspace)),
            SaveKey::Typed
        );
        assert_eq!(prompt.name(), "x.i");
        prompt.paste(" tems ");
        assert_eq!(prompt.name(), "x.items");
        assert_eq!(prompt.key(KeyEvent::from(KeyCode::Enter)), SaveKey::Save);
        assert_eq!(prompt.key(KeyEvent::from(KeyCode::Esc)), SaveKey::Cancel);
        assert_eq!(
            prompt.key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            SaveKey::Quit
        );
        // Saved, and said
        assert_eq!(prompt.save(&store, None, &cli), Some("x.items".into()));
        assert!(matches!(&prompt.message, Some((m, false)) if m.contains("--load x.items")));
        assert_eq!(store.load("x.items").unwrap().1.concurrency, Some(3));
        // Taken by another: asked once, then replaced
        let mut again = SaveAs::new("save as", "x.items");
        assert_eq!(again.save(&store, None, &cli), None);
        assert!(matches!(&again.message, Some((m, true)) if m.contains("replaces")));
        assert_eq!(again.save(&store, None, &cli), Some("x.items".into()));
        // Its own name: straight through
        let mut own = SaveAs::new("save as", "x.items");
        assert_eq!(
            own.save(&store, Some("x.items"), &cli),
            Some("x.items".into())
        );
        // Editing after the warning asks again for the new name
        let mut edited = SaveAs::new("save as", "x.items");
        edited.save(&store, None, &cli);
        edited.key(KeyEvent::from(KeyCode::Char('2')));
        assert_eq!(edited.save(&store, None, &cli), Some("x.items2".into()));
        // A bad name stays in the prompt
        let mut bad = SaveAs::new("save as", "a b");
        assert_eq!(bad.save(&store, None, &cli), None);
        assert!(matches!(&bad.message, Some((m, true)) if m.contains("space")));
        // The line shows the name and the hint, and the message instead
        let line = SaveAs::new("copy as", "x.items")
            .line(&store, 80)
            .to_string();
        assert!(
            line.contains("copy as") && line.contains("x.items") && line.contains("enter saves"),
            "{line}"
        );
        assert!(bad.line(&store, 80).to_string().contains("space"));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn chips_stand_for_their_keys() {
        assert_eq!(key_for_chip("enter"), Some(KeyEvent::from(KeyCode::Enter)));
        assert_eq!(key_for_chip("esc"), Some(KeyEvent::from(KeyCode::Esc)));
        assert_eq!(key_for_chip("F1"), Some(KeyEvent::from(KeyCode::F(1))));
        assert_eq!(
            key_for_chip("space"),
            Some(KeyEvent::from(KeyCode::Char(' ')))
        );
        assert_eq!(key_for_chip("q"), Some(KeyEvent::from(KeyCode::Char('q'))));
        assert_eq!(key_for_chip("?"), Some(KeyEvent::from(KeyCode::Char('?'))));
        assert_eq!(
            key_for_chip("ctrl-s"),
            Some(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL))
        );
        assert_eq!(key_for_chip("↑↓"), None);
        assert_eq!(key_for_chip("↑↓ u/d"), None);
        assert_eq!(key_for_chip("j/k"), None);
        assert_eq!(key_for_chip(""), None);
    }

    #[test]
    fn the_footer_and_the_tabs_are_clickable_where_they_are_drawn() {
        hits_clear();
        let mut terminal = Terminal::new(TestBackend::new(60, 3)).unwrap();
        terminal
            .draw(|f| {
                tab_bar(
                    f,
                    Rect::new(0, 0, 60, 1),
                    &["1 Graph".into(), "2 Pings".into()],
                    0,
                );
                footer(
                    f,
                    Rect::new(0, 2, 60, 1),
                    &[
                        ("enter", "open"),
                        ("↑↓", "move"),
                        ("?", "keys"),
                        ("q", "quit"),
                    ],
                );
            })
            .unwrap();
        // " 1 Graph " is 9 wide, then a space, then " 2 Pings "
        assert_eq!(hit_at(0, 0), Some(Hit::Tab(0)));
        assert_eq!(hit_at(8, 0), Some(Hit::Tab(0)));
        assert_eq!(hit_at(9, 0), None);
        assert_eq!(hit_at(10, 0), Some(Hit::Tab(1)));
        // " enter " + " open  " is 14 wide
        assert_eq!(hit_at(0, 2), Some(Hit::Key(KeyEvent::from(KeyCode::Enter))));
        assert_eq!(
            hit_at(13, 2),
            Some(Hit::Key(KeyEvent::from(KeyCode::Enter)))
        );
        // "↑↓ move" stands for no key, so it isn't clickable
        assert_eq!(hit_at(14, 2), None);
        assert_eq!(
            hit_at(25, 2),
            Some(Hit::Key(KeyEvent::from(KeyCode::Char('?'))))
        );
        assert_eq!(hit_at(0, 1), None);
        // The last drawn wins, and a cleared frame has nothing
        hit(Rect::new(0, 0, 60, 3), Hit::Row { list: 0, index: 7 });
        assert_eq!(hit_at(0, 0), Some(Hit::Row { list: 0, index: 7 }));
        hits_clear();
        assert_eq!(hit_at(0, 0), None);
    }

    #[test]
    fn rows_count_from_the_first_shown_and_stop_at_the_area() {
        hits_clear();
        hit_rows(Rect::new(2, 5, 20, 3), 1, 10, 8);
        assert_eq!(hit_at(2, 5), Some(Hit::Row { list: 1, index: 10 }));
        assert_eq!(hit_at(21, 7), Some(Hit::Row { list: 1, index: 12 }));
        assert_eq!(hit_at(2, 8), None, "no further than the area");
        assert_eq!(hit_at(1, 5), None);
    }

    #[test]
    fn the_wheel_is_the_arrows_and_a_second_click_is_a_double_click() {
        let wheel = |kind| MouseEvent {
            kind,
            column: 0,
            row: 0,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            click(&wheel(MouseEventKind::ScrollUp)),
            Some(Hit::Key(KeyEvent::from(KeyCode::Up)))
        );
        assert_eq!(
            click(&wheel(MouseEventKind::ScrollDown)),
            Some(Hit::Key(KeyEvent::from(KeyCode::Down)))
        );
        assert_eq!(click(&wheel(MouseEventKind::Moved)), None);
        assert!(pressed(&wheel(MouseEventKind::Down(MouseButton::Right))));
        assert!(!pressed(&wheel(MouseEventKind::Up(MouseButton::Left))));

        let row = |index| Hit::Row { list: 0, index };
        let t0 = Instant::now();
        let mut clicks = Clicks::default();
        assert!(!clicks.second(row(1), t0));
        assert!(clicks.second(row(1), t0 + Duration::from_millis(200)));
        // Opened: the next click starts over
        assert!(!clicks.second(row(1), t0 + Duration::from_millis(300)));
        assert!(
            !clicks.second(row(2), t0 + Duration::from_millis(400)),
            "another row"
        );
        assert!(
            !clicks.second(row(2), t0 + Duration::from_secs(3)),
            "too late"
        );
    }
}
