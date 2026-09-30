//! Three-row block digits for the headline numbers

use ratatui::{
    style::Style,
    text::{Line, Span},
};

pub const HEIGHT: usize = 3;

fn glyph(c: char) -> Option<[&'static str; HEIGHT]> {
    Some(match c {
        '0' => ["█▀█", "█ █", "▀▀▀"],
        '1' => ["▀█ ", " █ ", "▀▀▀"],
        '2' => ["▀▀█", "█▀▀", "▀▀▀"],
        '3' => ["▀▀█", " ▀█", "▀▀▀"],
        '4' => ["█ █", "▀▀█", "  ▀"],
        '5' => ["█▀▀", "▀▀█", "▀▀▀"],
        '6' => ["█▀▀", "█▀█", "▀▀▀"],
        '7' => ["▀▀█", "  █", "  ▀"],
        '8' => ["█▀█", "█▀█", "▀▀▀"],
        '9' => ["█▀█", "▀▀█", "▀▀▀"],
        '.' => [" ", " ", "▀"],
        '—' | '-' => ["   ", "▀▀▀", "   "],
        _ => return None,
    })
}

/// Cells `text` takes when drawn big; characters without a glyph are skipped
pub fn width(text: &str) -> usize {
    text.chars()
        .filter_map(glyph)
        .map(|g| g[0].chars().count() + 1)
        .sum::<usize>()
        .saturating_sub(1)
}

/// `text` drawn big, followed by `unit` in normal size on the bottom row
pub fn lines(text: &str, unit: &str, style: Style, unit_style: Style) -> [Line<'static>; HEIGHT] {
    let mut rows: [String; HEIGHT] = Default::default();
    for (i, g) in text.chars().filter_map(glyph).enumerate() {
        for (row, part) in rows.iter_mut().zip(g) {
            if i > 0 {
                row.push(' ');
            }
            row.push_str(part);
        }
    }
    let [top, mid, bottom] = rows;
    [
        Line::from(Span::styled(top, style)),
        Line::from(Span::styled(mid, style)),
        Line::from(vec![
            Span::styled(bottom, style),
            Span::styled(format!(" {unit}"), unit_style),
        ]),
    ]
}

/// Split "53.38ms" into ("53.38", "ms") and "1.6k" into ("1.6", "k")
pub fn split_unit(value: &str) -> (&str, &str) {
    let end = value
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(value.len());
    (&value[..end], &value[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn digits_line_up() {
        let [a, b, c] = lines("12.5", "ms", Style::new(), Style::new());
        assert_eq!(a.width(), b.width());
        assert_eq!(width("12.5"), a.width());
        assert_eq!(c.width(), a.width() + 3);
        assert_eq!(width(""), 0);
    }

    #[test]
    fn splits_units() {
        assert_eq!(split_unit("53.38ms"), ("53.38", "ms"));
        assert_eq!(split_unit("1.6k"), ("1.6", "k"));
        assert_eq!(split_unit("850µs"), ("850", "µs"));
        assert_eq!(split_unit("100.00%"), ("100.00", "%"));
        assert_eq!(split_unit("—"), ("", "—"));
    }
}
