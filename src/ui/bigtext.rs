//! Block digits for the headline numbers: three rows, or four in the large
//! 5×7 face the dashboard header uses when it has the room

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

/// Rows of the large face: 7 pixels, two to a cell
pub const LARGE_HEIGHT: usize = 4;

/// The large face, 5×7 pixels with two-pixel strokes and cut corners
fn large_glyph(c: char) -> Option<[&'static str; 7]> {
    Some(match c {
        '0' => [
            ".###.", "##.##", "##.##", "##.##", "##.##", "##.##", ".###.",
        ],
        '1' => [
            ".##..", "###..", ".##..", ".##..", ".##..", ".##..", "####.",
        ],
        '2' => [
            "####.", "...##", "...##", ".###.", "##...", "##...", "#####",
        ],
        '3' => [
            "####.", "...##", "...##", ".###.", "...##", "...##", "####.",
        ],
        '4' => [
            "##.##", "##.##", "##.##", "#####", "...##", "...##", "...##",
        ],
        '5' => [
            "#####", "##...", "####.", "...##", "...##", "...##", "####.",
        ],
        '6' => [
            ".###.", "##...", "####.", "##.##", "##.##", "##.##", ".###.",
        ],
        '7' => [
            "#####", "...##", "...##", "..##.", ".##..", ".##..", ".##..",
        ],
        '8' => [
            ".###.", "##.##", "##.##", ".###.", "##.##", "##.##", ".###.",
        ],
        '9' => [
            ".###.", "##.##", "##.##", ".####", "...##", "...##", ".###.",
        ],
        '.' => ["..", "..", "..", "..", "..", "##", "##"],
        '—' | '-' => [
            ".....", ".....", ".....", "#####", ".....", ".....", ".....",
        ],
        _ => return None,
    })
}

/// Cells `text` takes in the large face
pub fn large_width(text: &str) -> usize {
    text.chars()
        .filter_map(large_glyph)
        .map(|g| g[0].len() + 1)
        .sum::<usize>()
        .saturating_sub(1)
}

/// `text` in the large face, `LARGE_HEIGHT` rows, with `unit` in normal
/// size after the last row
pub fn large_lines(
    text: &str,
    unit: &str,
    style: Style,
    unit_style: Style,
) -> [Line<'static>; LARGE_HEIGHT] {
    let mut pixels: [String; 8] = Default::default();
    for (i, g) in text.chars().filter_map(large_glyph).enumerate() {
        for (row, part) in pixels.iter_mut().zip(g) {
            if i > 0 {
                row.push('.');
            }
            row.push_str(part);
        }
    }
    let width = pixels[0].len();
    pixels[7] = ".".repeat(width);
    let mut rows: [Line<'static>; LARGE_HEIGHT] = Default::default();
    for (r, row) in rows.iter_mut().enumerate() {
        let (top, bottom) = (pixels[r * 2].as_bytes(), pixels[r * 2 + 1].as_bytes());
        let cells: String = (0..width)
            .map(|x| match (top[x] == b'#', bottom[x] == b'#') {
                (true, true) => '█',
                (true, false) => '▀',
                (false, true) => '▄',
                (false, false) => ' ',
            })
            .collect();
        let mut spans = vec![Span::styled(cells, style)];
        // The unit sits on the baseline, the row holding the digits' feet
        if r == LARGE_HEIGHT - 1 && !unit.is_empty() {
            spans.push(Span::styled(format!(" {unit}"), unit_style));
        }
        *row = Line::from(spans);
    }
    rows
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
    fn large_digits_line_up() {
        let rows = large_lines("207.4", "ms", Style::new(), Style::new());
        assert!(rows[..3].iter().all(|r| r.width() == large_width("207.4")));
        assert_eq!(rows[3].width(), large_width("207.4") + 3);
        assert_eq!(large_width("207.4"), 4 * 5 + 2 + 4);
        assert_eq!(large_width(""), 0);
        // Seven pixels: the last row holds only the bottom of the digits
        assert_eq!(rows[3].spans[0].content, "▀▀▀▀▀  ▀▀▀   ▀▀   ▀▀    ▀▀");
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
