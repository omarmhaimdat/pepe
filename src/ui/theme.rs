//! Which palette the screens are drawn in. Everything is drawn with the dark
//! palette; once a frame is drawn, `apply` adjusts it for a light terminal
//! or for `NO_COLOR`, so no drawing code has to know.
//!
//! - **dark**, the default: the 256-colour grays and cyan the screens name
//! - **light**: the same roles, chosen to read on a light background — the
//!   gray ramp turns around, so the heatmap's busiest band is the darkest
//! - **none** (`NO_COLOR` set): no colour at all; what had a background is
//!   drawn in reverse video, the heatmap in shades ░▒▓█, and Pepe stays home
//!
//! `PEPE_THEME=light|dark` picks one; otherwise `COLORFGBG`, which many
//! terminals set, says whether the background is light.

use std::cell::RefCell;
use std::sync::OnceLock;

use ratatui::{
    buffer::Buffer,
    layout::{Position, Rect},
    style::{Color, Modifier},
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Theme {
    Dark,
    Light,
    None,
}

static THEME: OnceLock<Theme> = OnceLock::new();

thread_local! {
    /// Where this frame drew pixel art, which keeps its own colours
    static KEPT: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
}

/// The theme for this run, decided once from the environment
pub fn current() -> Theme {
    *THEME.get_or_init(|| {
        detect(
            std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty()),
            std::env::var("PEPE_THEME").ok().as_deref(),
            std::env::var("COLORFGBG").ok().as_deref(),
        )
    })
}

/// Whether colour is off: the scrollback report and the mascot ask
pub fn no_color() -> bool {
    current() == Theme::None
}

fn detect(no_color: bool, asked: Option<&str>, colorfgbg: Option<&str>) -> Theme {
    if no_color {
        return Theme::None;
    }
    match asked.map(|a| a.trim().to_ascii_lowercase()).as_deref() {
        Some("light") => return Theme::Light,
        Some("dark") => return Theme::Dark,
        Some("none") | Some("no-color") => return Theme::None,
        _ => {}
    }
    // "15;0" is light text on black; "0;15" (or "0;default;15") dark on white
    let background = colorfgbg
        .and_then(|v| v.rsplit(';').next())
        .and_then(|b| b.trim().parse::<u8>().ok());
    match background {
        Some(7) | Some(9..=15) => Theme::Light,
        _ => Theme::Dark,
    }
}

/// Leave `area` as drawn: pixel art whose colours are the picture
pub fn keep(area: Rect) {
    KEPT.with(|k| k.borrow_mut().push(area));
}

/// Draw with `render`, then adjust the frame for the theme
pub fn draw(f: &mut Frame, render: impl FnOnce(&mut Frame)) {
    KEPT.with(|k| k.borrow_mut().clear());
    render(f);
    let kept = KEPT.with(|k| std::mem::take(&mut *k.borrow_mut()));
    adjust(f.buffer_mut(), current(), &kept);
}

/// The dark palette's 256-colour indexes, for a light background
fn light(n: u8) -> u8 {
    match n {
        81 => 24,   // accent
        150 => 22,  // JSON strings
        176 => 96,  // true / false / null
        180 => 58,  // HTML attributes
        186 => 100, // spark
        215 => 94,  // JSON numbers
        52 => 224,  // the fill under failing throughput
        // Grays: the ramp turns around, so more stays more visible
        236 => 253, // the field being typed in
        237 => 254, // the selected line, key chips, the faintest heat
        238 => 252, // the fill under throughput
        239 => 250, // rules, axes
        241 => 247,
        242 => 245,
        243 => 245,
        244 => 242, // JSON punctuation
        245 => 243, // faint, and a heat step
        246 => 241,
        248 => 241, // labels
        247 => 240,
        250 => 238, // the p90 line
        253 => 236,
        255 => 234, // the busiest heat
        other => other,
    }
}

/// Heat level 0–8 of a heatmap colour, as drawn in the dark palette
fn heat_level(color: Option<Color>) -> Option<usize> {
    const HEAT: [u8; 9] = [237, 239, 241, 243, 245, 247, 250, 253, 255];
    match color {
        Some(Color::Indexed(n)) => HEAT.iter().position(|&h| h == n),
        _ => None,
    }
}

fn adjust(buf: &mut Buffer, theme: Theme, kept: &[Rect]) {
    if theme == Theme::Dark {
        return;
    }
    let area = buf.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            if kept.iter().any(|r| r.contains(Position { x, y })) {
                continue;
            }
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            match theme {
                Theme::Light => {
                    if let Color::Indexed(n) = cell.fg {
                        cell.fg = Color::Indexed(light(n));
                    }
                    if let Color::Indexed(n) = cell.bg {
                        cell.bg = Color::Indexed(light(n));
                    }
                }
                Theme::None => {
                    let block = matches!(cell.symbol(), "▀" | "▄" | "█");
                    let heat = heat_level(Some(cell.fg))
                        .into_iter()
                        .chain(heat_level(Some(cell.bg)))
                        .max();
                    // The dim fill under a chart: a light shade, so the
                    // bright edge on top still reads as the line
                    let fill = matches!(cell.fg, Color::Indexed(238) | Color::Indexed(52));
                    if fill && cell.symbol() != " " {
                        cell.set_char('░');
                    } else if let (true, Some(level)) = (block, heat) {
                        // The heatmap in shades instead of grays
                        cell.set_char(match level {
                            0..=2 => '░',
                            3..=5 => '▒',
                            6..=7 => '▓',
                            _ => '█',
                        });
                    } else if cell.bg != Color::Reset {
                        // A selected line, a chip, a badge: reverse video
                        cell.modifier.insert(Modifier::REVERSED);
                    }
                    cell.fg = Color::Reset;
                    cell.bg = Color::Reset;
                }
                Theme::Dark => {}
            }
        }
    }
}

/// The report left in the shell, with each verdict glyph in its colour and
/// the headline's level word bold too. Plain when colour is off.
pub fn color_report(report: &str, color: bool) -> String {
    if !color {
        return report.to_string();
    }
    const GREEN: &str = "\x1b[32m";
    const YELLOW: &str = "\x1b[33m";
    const RED: &str = "\x1b[31m";
    const BOLD: &str = "\x1b[1m";
    const RESET: &str = "\x1b[0m";
    let mut out = String::with_capacity(report.len() + 64);
    for line in report.split_inclusive('\n') {
        let body = line.trim_start();
        let indent = &line[..line.len() - body.len()];
        let glyph = ['✔', '▲', '✖'].into_iter().find(|g| body.starts_with(*g));
        let Some(glyph) = glyph else {
            out.push_str(line);
            continue;
        };
        let color = match glyph {
            '✔' => GREEN,
            '▲' => YELLOW,
            _ => RED,
        };
        let rest = &body[glyph.len_utf8()..];
        if indent.is_empty() {
            // The headline: "✔ Healthy · …" — the glyph and the word
            let word_end = rest
                .trim_start()
                .find(' ')
                .map_or(rest.len(), |i| i + (rest.len() - rest.trim_start().len()));
            let (word, tail) = rest.split_at(word_end);
            out.push_str(&format!("{BOLD}{color}{glyph}{word}{RESET}{tail}"));
        } else {
            out.push_str(&format!("{indent}{color}{glyph}{RESET}{rest}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn theme_comes_from_the_environment() {
        assert_eq!(detect(false, None, None), Theme::Dark);
        assert_eq!(
            detect(true, Some("light"), None),
            Theme::None,
            "NO_COLOR wins"
        );
        assert_eq!(detect(false, Some("Light"), None), Theme::Light);
        assert_eq!(detect(false, Some("dark"), Some("0;15")), Theme::Dark);
        assert_eq!(detect(false, None, Some("0;15")), Theme::Light);
        assert_eq!(detect(false, None, Some("0;default;15")), Theme::Light);
        assert_eq!(detect(false, None, Some("15;0")), Theme::Dark);
        assert_eq!(detect(false, None, Some("nonsense")), Theme::Dark);
    }

    #[test]
    fn light_turns_the_gray_ramp_around_and_keeps_pixel_art() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 1));
        buf.set_string(
            0,
            0,
            "ab",
            Style::new().fg(Color::Indexed(81)).bg(Color::Indexed(237)),
        );
        buf.set_string(
            2,
            0,
            "cd",
            Style::new().fg(Color::Indexed(196)).bg(Color::Indexed(237)),
        );
        adjust(&mut buf, Theme::Light, &[Rect::new(2, 0, 2, 1)]);
        assert_eq!(buf[(0, 0)].fg, Color::Indexed(24));
        assert_eq!(buf[(0, 0)].bg, Color::Indexed(254));
        assert_eq!(buf[(2, 0)].bg, Color::Indexed(237), "kept as drawn");
        // Heat stays ordered: busier is darker on a light background
        let heat = [237u8, 239, 241, 243, 245, 247, 250, 253, 255].map(light);
        assert!(heat.windows(2).all(|w| w[0] > w[1]), "{heat:?}");
    }

    #[test]
    fn no_color_reverses_fills_and_shades_the_heatmap() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf.set_string(
            0,
            0,
            "x",
            Style::new().fg(Color::Indexed(81)).bg(Color::Indexed(237)),
        );
        buf.set_string(
            1,
            0,
            "▀",
            Style::new().fg(Color::Indexed(255)).bg(Color::Indexed(239)),
        );
        buf.set_string(2, 0, "y", Style::new().fg(Color::Green));
        adjust(&mut buf, Theme::None, &[]);
        assert!(buf[(0, 0)].modifier.contains(Modifier::REVERSED));
        assert_eq!(buf[(1, 0)].symbol(), "█");
        assert_eq!(buf[(2, 0)].fg, Color::Reset);
        assert!(!buf[(2, 0)].modifier.contains(Modifier::REVERSED));
    }

    #[test]
    fn report_glyphs_take_their_colour() {
        let report = "pepe · GET x · ×4\n✔ Healthy · 10 requests\n  ▲ Long tail\n  ✖ 3 timeouts\n";
        let colored = color_report(report, true);
        assert!(
            colored.contains("\x1b[1m\x1b[32m✔ Healthy\x1b[0m · 10 requests"),
            "{colored:?}"
        );
        assert!(
            colored.contains("  \x1b[33m▲\x1b[0m Long tail"),
            "{colored:?}"
        );
        assert!(
            colored.contains("  \x1b[31m✖\x1b[0m 3 timeouts"),
            "{colored:?}"
        );
        assert!(colored.starts_with("pepe · GET x · ×4\n"));
        assert_eq!(color_report(report, false), report);
    }
}
