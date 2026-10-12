//! What the setup, plan and ramp screens are drawn with: cards, key chips,
//! the bar on the selected line

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Flex, Layout, Rect},
    style::{Color, Modifier, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Clear, Paragraph},
    Frame,
};

use super::view::{label, ACCENT, LABEL, RULE};

/// Background of the selected line
pub(super) const SELECTED: Color = Color::Indexed(237);
/// Background of the field being typed in
pub(super) const FIELD: Color = Color::Indexed(236);
/// What's off, or not sent: quieter than a label, still 4.8:1 on a dark
/// background
pub(super) const FAINT: Color = Color::Indexed(245);
/// A rounded card with its title in the top border, and `right` at the
/// other end of it
pub(super) fn panel(
    title: Line<'static>,
    right: Option<Line<'static>>,
    border: Color,
) -> Block<'static> {
    let mut block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(border))
        .title_top(title);
    if let Some(right) = right {
        block = block.title_top(right.right_aligned());
    }
    block
}

/// A card's title, lit when the card has the keys
pub(super) fn caption(text: &str, focused: bool) -> Line<'static> {
    Line::from(Span::styled(
        format!(" {} ", text.to_uppercase()),
        Style::new().fg(if focused { ACCENT } else { LABEL }).bold(),
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
        spans.push(Span::styled(
            format!(" {key} "),
            Style::new().bg(SELECTED).fg(ACCENT).bold(),
        ));
        spans.push(label(format!(" {action}  ")));
    }
    Line::from(spans)
}

/// Key chips that fit `width`: the screen's own keys in order while they
/// fit, and always the last two — `?` keys and the way out — at the end
pub(super) fn chips_fit(pairs: &[(&'static str, &'static str)], width: u16) -> Line<'static> {
    let cost = |(key, action): &(&str, &str)| key.chars().count() + action.chars().count() + 5;
    let width = width as usize;
    let keep = pairs.len().min(2);
    let (rest, tail) = pairs.split_at(pairs.len() - keep);
    let mut used: usize = tail.iter().map(cost).sum();
    let mut shown: Vec<(&'static str, &'static str)> = Vec::with_capacity(pairs.len());
    for pair in rest {
        if used + cost(pair) > width {
            break;
        }
        used += cost(pair);
        shown.push(*pair);
    }
    shown.extend_from_slice(tail);
    chips(&shown)
}

/// A row of views or modes, the selected one filled with the accent. Every
/// screen draws its tabs this way; a number goes in the title only where
/// the number keys pick the tab.
pub(super) fn tabs(titles: &[String], selected: usize) -> Line<'static> {
    let mut spans = Vec::with_capacity(titles.len() * 2);
    for (i, title) in titles.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(if i == selected {
            Span::styled(
                format!(" {title} "),
                Style::new().fg(Color::Black).bg(ACCENT).bold(),
            )
        } else {
            Span::styled(format!(" {title} "), Style::new().fg(LABEL))
        });
    }
    Line::from(spans)
}

/// The `?` overlay: every key on this screen with what it does, then notes
/// on anything that needs explaining. `?` or esc closes it, on every screen.
pub(super) fn help(f: &mut Frame, area: Rect, rows: &[(&str, &str)], notes: &[String]) {
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
                .border_type(BorderType::Rounded)
                .border_style(Style::new().fg(ACCENT))
                .title(Span::styled(" keys ", Style::new().fg(ACCENT).bold())),
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
