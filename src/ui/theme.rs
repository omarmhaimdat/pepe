//! Which palette the screens are drawn in. Everything is drawn with the dark
//! palette; once a frame is drawn, `adjust` turns it into pepe's own colours,
//! or adjusts it for a light terminal or for `NO_COLOR`, so no drawing code
//! has to know.
//!
//! - **pepe**, the default where the terminal has true colour: pepe's own
//!   palette, warm darks with one ember accent; the heatmap glows from
//!   embers to flame; panels sit on a surface a shade lighter than the ground
//! - **terminal** (or **dark**): the 256-colour grays and cyan the screens
//!   name, on the terminal's own background
//! - **light**: the same roles, chosen to read on a light background — the
//!   gray ramp turns around, so the heatmap's busiest band is the darkest
//! - **none** (`NO_COLOR` set): no colour at all; what had a background is
//!   drawn in reverse video, the heatmap in shades ░▒▓█, and Pepe stays home
//!
//! `PEPE_THEME=pepe|terminal|light|none` picks one; otherwise `COLORFGBG`,
//! which many terminals set, says whether the background is light, and
//! `COLORTERM` whether the terminal can show pepe's own colours.

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
    /// pepe's own palette, in true colour
    Pepe,
    /// The terminal's background and the 256-colour palette
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
            std::env::var("COLORTERM").ok().as_deref(),
        )
    })
}

/// Whether colour is off: the scrollback report and the mascot ask
pub fn no_color() -> bool {
    current() == Theme::None
}

fn detect(
    no_color: bool,
    asked: Option<&str>,
    colorfgbg: Option<&str>,
    colorterm: Option<&str>,
) -> Theme {
    if no_color {
        return Theme::None;
    }
    match asked.map(|a| a.trim().to_ascii_lowercase()).as_deref() {
        Some("pepe") | Some("scoville") => return Theme::Pepe,
        Some("light") => return Theme::Light,
        Some("dark") | Some("terminal") => return Theme::Dark,
        Some("none") | Some("no-color") => return Theme::None,
        _ => {}
    }
    // "15;0" is light text on black; "0;15" (or "0;default;15") dark on white
    let background = colorfgbg
        .and_then(|v| v.rsplit(';').next())
        .and_then(|b| b.trim().parse::<u8>().ok());
    let true_color = colorterm
        .map(|c| c.trim().to_ascii_lowercase())
        .is_some_and(|c| c == "truecolor" || c == "24bit");
    match background {
        Some(7) | Some(9..=15) => Theme::Light,
        _ if true_color => Theme::Pepe,
        _ => Theme::Dark,
    }
}

/// Background of a panel. Drawn as a 256-colour index so every theme can
/// say what it is: a surface a shade above the ground in pepe's palette,
/// nothing at all elsewhere, where panels keep only their spacing.
pub const PANEL: Color = Color::Indexed(235);

/// pepe's palette
pub mod pepe {
    use ratatui::style::Color;

    pub const GROUND: Color = Color::Rgb(0x14, 0x0f, 0x0d);
    pub const SURFACE: Color = Color::Rgb(0x1d, 0x16, 0x13);
    pub const TEXT: Color = Color::Rgb(0xf3, 0xe8, 0xde);
    pub const EMBER: Color = Color::Rgb(0xff, 0x8a, 0x3d);
    pub const LABEL: Color = Color::Rgb(0x9c, 0x8a, 0x7e);
    pub const GOOD: Color = Color::Rgb(0x9c, 0xcf, 0x6a);
    pub const WARN: Color = Color::Rgb(0xf0, 0xcf, 0x5a);
    pub const BAD: Color = Color::Rgb(0xff, 0x5a, 0x6e);
    /// Few requests → many: embers to flame
    pub const HEAT: [Color; 9] = [
        Color::Rgb(0x2e, 0x1d, 0x17),
        Color::Rgb(0x46, 0x24, 0x1a),
        Color::Rgb(0x65, 0x2c, 0x19),
        Color::Rgb(0x8b, 0x36, 0x17),
        Color::Rgb(0xb4, 0x45, 0x16),
        Color::Rgb(0xda, 0x5d, 0x1b),
        Color::Rgb(0xff, 0x8a, 0x3d),
        Color::Rgb(0xff, 0xc2, 0x7a),
        Color::Rgb(0xff, 0xe3, 0xb8),
    ];
}

/// The dark palette's colours in pepe's. `heat` when the colour is a step
/// of the heatmap rather than text or a rule, which share some grays.
fn pepe_color(color: Color, heat: bool, background: bool) -> Color {
    use pepe::*;
    let rgb = |r, g, b| Color::Rgb(r, g, b);
    if heat {
        if let Some(level) = heat_level(Some(color)) {
            return HEAT[level];
        }
    }
    match color {
        Color::Reset if background => GROUND,
        Color::Reset => TEXT,
        Color::Black => GROUND,
        Color::White => rgb(0xff, 0xf6, 0xee),
        Color::Green | Color::LightGreen => GOOD,
        Color::Yellow | Color::LightYellow => WARN,
        Color::Red | Color::LightRed => BAD,
        Color::Blue | Color::LightBlue => rgb(0x8c, 0xb8, 0xff),
        Color::Magenta | Color::LightMagenta => rgb(0xff, 0xc2, 0x7a),
        Color::Cyan | Color::LightCyan => EMBER,
        Color::Indexed(n) => match n {
            81 => EMBER,                  // the accent
            67 => rgb(0xb4, 0x45, 0x16),  // every other ramp step
            117 => rgb(0x8c, 0xd2, 0xff), // cool: sweat, sleep
            150 => rgb(0xb8, 0xd9, 0x8a), // JSON strings
            176 => rgb(0xe8, 0xa8, 0xc8), // true / false / null
            180 => rgb(0xe0, 0xb9, 0x8a), // HTML attributes
            186 => rgb(0xff, 0xd2, 0x7a), // spark
            215 => rgb(0xff, 0xb0, 0x70), // JSON numbers
            52 => rgb(0x5a, 0x1a, 0x24),  // the fill under failing throughput
            235 => SURFACE,               // panels
            236 => rgb(0x2e, 0x24, 0x20), // the field being typed in
            237 => rgb(0x2e, 0x24, 0x20), // the selected line, key chips
            238 => rgb(0x4a, 0x2a, 0x1c), // the fill under throughput
            239 => rgb(0x3a, 0x2c, 0x25), // rules, axes
            241 => rgb(0x5f, 0x50, 0x48),
            242 | 243 => rgb(0x6f, 0x5e, 0x53),
            244 => rgb(0x8a, 0x7a, 0x6e), // JSON punctuation
            245 => rgb(0x7f, 0x6d, 0x61), // faint
            246..=248 => LABEL,
            250 => rgb(0xb9, 0xa5, 0x97), // the p90 line
            251..=253 => rgb(0xe0, 0xd3, 0xc7),
            254 | 255 => TEXT,
            other => Color::Indexed(other),
        },
        other => other,
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
    let area = buf.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let keep = kept.iter().any(|r| r.contains(Position { x, y }));
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            if keep {
                // The picture keeps its colours; around it, the ground shows
                if theme == Theme::Pepe && cell.bg == Color::Reset {
                    cell.bg = pepe::GROUND;
                }
                continue;
            }
            // Panels are a pepe-theme surface; elsewhere only their spacing
            if theme != Theme::Pepe && cell.bg == PANEL {
                cell.bg = Color::Reset;
            }
            match theme {
                Theme::Pepe => {
                    let symbol = cell.symbol();
                    let block = matches!(symbol, "▀" | "▄" | "█");
                    let braille = symbol
                        .chars()
                        .next()
                        .is_some_and(|c| ('\u{2800}'..='\u{28ff}').contains(&c));
                    cell.fg = pepe_color(cell.fg, block, false);
                    cell.bg = pepe_color(cell.bg, block || braille, true);
                }
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
        assert_eq!(detect(false, None, None, None), Theme::Dark);
        assert_eq!(
            detect(true, Some("light"), None, None),
            Theme::None,
            "NO_COLOR wins"
        );
        assert_eq!(detect(false, Some("Light"), None, None), Theme::Light);
        assert_eq!(detect(false, Some("dark"), Some("0;15"), None), Theme::Dark);
        assert_eq!(detect(false, None, Some("0;15"), None), Theme::Light);
        assert_eq!(
            detect(false, None, Some("0;default;15"), None),
            Theme::Light
        );
        assert_eq!(detect(false, None, Some("15;0"), None), Theme::Dark);
        assert_eq!(detect(false, None, Some("nonsense"), None), Theme::Dark);
        // True colour gets pepe's own palette, unless the ground is light
        assert_eq!(detect(false, None, None, Some("truecolor")), Theme::Pepe);
        assert_eq!(detect(false, None, None, Some("24bit")), Theme::Pepe);
        assert_eq!(
            detect(false, None, Some("0;15"), Some("truecolor")),
            Theme::Light
        );
        assert_eq!(
            detect(false, Some("terminal"), None, Some("truecolor")),
            Theme::Dark
        );
        assert_eq!(detect(false, Some("pepe"), None, None), Theme::Pepe);
    }

    #[test]
    fn pepe_paints_the_ground_and_lights_the_heatmap() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 5, 1));
        buf.set_string(0, 0, "a", Style::new().fg(Color::Indexed(81)));
        // Gray 239 is a rule as text, and a heat step as a half block
        buf.set_string(1, 0, "─", Style::new().fg(Color::Indexed(239)));
        buf.set_string(
            2,
            0,
            "▀",
            Style::new().fg(Color::Indexed(255)).bg(Color::Indexed(239)),
        );
        buf.set_string(3, 0, " ", Style::new().bg(PANEL));
        buf.set_string(4, 0, "▀", Style::new().fg(Color::Indexed(196)));
        adjust(&mut buf, Theme::Pepe, &[Rect::new(4, 0, 1, 1)]);
        assert_eq!(buf[(0, 0)].fg, pepe::EMBER);
        assert_eq!(buf[(0, 0)].bg, pepe::GROUND);
        assert_eq!(buf[(1, 0)].fg, Color::Rgb(0x3a, 0x2c, 0x25));
        assert_eq!(buf[(2, 0)].fg, pepe::HEAT[8]);
        assert_eq!(buf[(2, 0)].bg, pepe::HEAT[1]);
        assert_eq!(buf[(3, 0)].bg, pepe::SURFACE);
        assert_eq!(buf[(4, 0)].fg, Color::Indexed(196), "kept as drawn");
        assert_eq!(buf[(4, 0)].bg, pepe::GROUND, "on the ground");
    }

    #[test]
    fn panels_disappear_outside_the_pepe_theme() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        buf.set_string(0, 0, "x", Style::new().bg(PANEL));
        adjust(&mut buf, Theme::Dark, &[]);
        assert_eq!(buf[(0, 0)].bg, Color::Reset);
        let mut buf = Buffer::empty(Rect::new(0, 0, 1, 1));
        buf.set_string(0, 0, "x", Style::new().bg(PANEL));
        adjust(&mut buf, Theme::None, &[]);
        assert!(!buf[(0, 0)].modifier.contains(Modifier::REVERSED));
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
