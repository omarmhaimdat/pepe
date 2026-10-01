//! What the setup, plan and ramp screens are drawn with: cards, key chips,
//! the bar on the selected line

use ratatui::{
    layout::Rect,
    style::{Color, Style, Stylize},
    text::{Line, Span},
    widgets::{Block, BorderType, Paragraph},
    Frame,
};

use super::view::{label, ACCENT, LABEL, RULE};

/// Background of the selected line
pub(super) const SELECTED: Color = Color::Indexed(237);
/// Background of the field being typed in
pub(super) const FIELD: Color = Color::Indexed(236);
/// What's off, or not sent
pub(super) const FAINT: Color = Color::Indexed(242);
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
