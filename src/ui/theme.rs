//! Which palette the screens are drawn in. Everything is drawn with the dark
//! palette; once a frame is drawn, `adjust` turns it into the theme's own
//! colours, or adjusts it for a light terminal or for `NO_COLOR`, so no
//! drawing code has to know.
//!
//! - **pepe**, the default where the terminal has true colour: pepe's own
//!   palette, warm darks with one ember accent; the heatmap glows from
//!   embers to flame; panels sit on a surface a shade lighter than the ground
//! - **nord**, **dracula**, **gruvbox**, **tokyo-night**, **catppuccin**,
//!   and **paper** for a light background: the same roles in other true
//!   colour palettes, each worked out from a dozen colours (see `Base`)
//! - **terminal** (or **dark**): the 256-colour grays and cyan the screens
//!   name, on the terminal's own background
//! - **light**: the same roles, chosen to read on a light background — the
//!   gray ramp turns around, so the heatmap's busiest band is the darkest
//! - **none** (`NO_COLOR` set): no colour at all; what had a background is
//!   drawn in reverse video, the heatmap in shades ░▒▓█, and Pepe stays home
//!
//! `PEPE_THEME` names one; otherwise the theme saved in the settings (F2 on
//! any screen); otherwise `COLORFGBG`, which many terminals set, says whether
//! the background is light, and `COLORTERM` whether the terminal can show
//! pepe's own colours.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU8, Ordering};

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
    Nord,
    Dracula,
    Gruvbox,
    TokyoNight,
    Catppuccin,
    /// True colour on a light ground
    Paper,
    /// The terminal's background and the 256-colour palette
    Dark,
    Light,
    None,
}

impl Theme {
    /// Every theme, in the order the settings page steps through them
    pub const ALL: [Theme; 10] = [
        Theme::Pepe,
        Theme::Nord,
        Theme::Dracula,
        Theme::Gruvbox,
        Theme::TokyoNight,
        Theme::Catppuccin,
        Theme::Paper,
        Theme::Dark,
        Theme::Light,
        Theme::None,
    ];

    /// The name `PEPE_THEME` and the settings file know it by
    pub fn name(self) -> &'static str {
        match self {
            Theme::Pepe => "pepe",
            Theme::Nord => "nord",
            Theme::Dracula => "dracula",
            Theme::Gruvbox => "gruvbox",
            Theme::TokyoNight => "tokyo-night",
            Theme::Catppuccin => "catppuccin",
            Theme::Paper => "paper",
            Theme::Dark => "terminal",
            Theme::Light => "light",
            Theme::None => "none",
        }
    }

    pub fn parse(name: &str) -> Option<Theme> {
        let name = name.trim().to_ascii_lowercase().replace(['_', ' '], "-");
        Some(match name.as_str() {
            "pepe" | "scoville" => Theme::Pepe,
            "nord" => Theme::Nord,
            "dracula" => Theme::Dracula,
            "gruvbox" => Theme::Gruvbox,
            "tokyo-night" | "tokyonight" | "tokyo" => Theme::TokyoNight,
            "catppuccin" | "mocha" => Theme::Catppuccin,
            "paper" => Theme::Paper,
            "dark" | "terminal" => Theme::Dark,
            "light" => Theme::Light,
            "none" | "no-color" => Theme::None,
            _ => return None,
        })
    }

    /// The theme's own colours, where it has them: the true colour themes
    pub fn palette(self) -> Option<&'static Palette> {
        Some(match self {
            Theme::Pepe => &PEPE,
            Theme::Nord => &NORD,
            Theme::Dracula => &DRACULA,
            Theme::Gruvbox => &GRUVBOX,
            Theme::TokyoNight => &TOKYO_NIGHT,
            Theme::Catppuccin => &CATPPUCCIN,
            Theme::Paper => &PAPER,
            Theme::Dark | Theme::Light | Theme::None => return None,
        })
    }

    /// Whether the theme paints its own ground, with panels a shade above it
    pub fn true_color(self) -> bool {
        self.palette().is_some()
    }

    fn index(self) -> u8 {
        Theme::ALL.iter().position(|t| *t == self).unwrap_or(0) as u8
    }
}

/// No theme decided yet
const UNDECIDED: u8 = u8::MAX;
static THEME: AtomicU8 = AtomicU8::new(UNDECIDED);

thread_local! {
    /// Where this frame drew pixel art, which keeps its own colours
    static KEPT: RefCell<Vec<Rect>> = const { RefCell::new(Vec::new()) };
}

fn no_color_asked() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

fn from_environment(saved: Option<&str>) -> Theme {
    detect(
        no_color_asked(),
        std::env::var("PEPE_THEME").ok().as_deref(),
        saved,
        std::env::var("COLORFGBG").ok().as_deref(),
        std::env::var("COLORTERM").ok().as_deref(),
    )
}

/// The theme for this run: decided from the environment and the settings
/// the first time it's asked for, and changed since only by `choose`
pub fn current() -> Theme {
    match THEME.load(Ordering::Relaxed) {
        UNDECIDED => {
            let theme = from_environment(crate::settings::get().theme.as_deref());
            THEME.store(theme.index(), Ordering::Relaxed);
            theme
        }
        index => Theme::ALL[index as usize % Theme::ALL.len()],
    }
}

/// Draw in `theme` from the next frame on, or in the one the terminal
/// suggests when it's `None`. The settings page calls this as a theme is
/// picked, so it wins over `PEPE_THEME` for the rest of the run; `NO_COLOR`
/// still holds.
pub fn choose(theme: Option<Theme>) {
    let theme = match theme {
        _ if no_color_asked() => Theme::None,
        Some(theme) => theme,
        None => detect(
            false,
            None,
            None,
            std::env::var("COLORFGBG").ok().as_deref(),
            std::env::var("COLORTERM").ok().as_deref(),
        ),
    };
    THEME.store(theme.index(), Ordering::Relaxed);
}

/// Whether colour is off: the scrollback report and the mascot ask
pub fn no_color() -> bool {
    current() == Theme::None
}

fn detect(
    no_color: bool,
    asked: Option<&str>,
    saved: Option<&str>,
    colorfgbg: Option<&str>,
    colorterm: Option<&str>,
) -> Theme {
    if no_color {
        return Theme::None;
    }
    // The environment first, then the settings; a name neither knows (a
    // theme from a newer pepe, say) leaves the terminal to decide
    if let Some(theme) = [asked, saved].into_iter().flatten().find_map(Theme::parse) {
        return theme;
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
/// say what it is: a surface a shade above the ground in a true colour
/// palette, nothing at all elsewhere, where panels keep only their spacing.
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

/// What each colour of the dark palette is in a true colour theme
pub struct Palette {
    pub ground: Color,
    /// Panels
    pub surface: Color,
    pub text: Color,
    /// What the screens draw in white: a step past the text
    pub bright: Color,
    pub accent: Color,
    /// Every other ramp step
    pub accent_dim: Color,
    pub label: Color,
    pub good: Color,
    pub warn: Color,
    pub bad: Color,
    pub blue: Color,
    pub magenta: Color,
    /// Sweat, sleep
    pub cool: Color,
    /// JSON strings
    pub string: Color,
    /// true / false / null
    pub keyword: Color,
    /// HTML attributes
    pub attr: Color,
    pub spark: Color,
    /// JSON numbers
    pub number: Color,
    /// The fill under failing throughput
    pub fail_fill: Color,
    /// The field being typed in, the selected line, key chips
    pub selected: Color,
    /// The fill under throughput
    pub fill: Color,
    /// Rules, axes
    pub rule: Color,
    /// The grays between a rule and a label, dimmest first
    pub dim: [Color; 2],
    /// JSON punctuation
    pub punct: Color,
    pub faint: Color,
    /// The p90 line
    pub soft: Color,
    /// Just under the text
    pub strong: Color,
    /// Few requests → many
    pub heat: [Color; 9],
}

static PEPE: Palette = {
    use pepe::*;
    Palette {
        ground: GROUND,
        surface: SURFACE,
        text: TEXT,
        bright: Color::Rgb(0xff, 0xf6, 0xee),
        accent: EMBER,
        accent_dim: Color::Rgb(0xb4, 0x45, 0x16),
        label: LABEL,
        good: GOOD,
        warn: WARN,
        bad: BAD,
        blue: Color::Rgb(0x8c, 0xb8, 0xff),
        magenta: Color::Rgb(0xff, 0xc2, 0x7a),
        cool: Color::Rgb(0x8c, 0xd2, 0xff),
        string: Color::Rgb(0xb8, 0xd9, 0x8a),
        keyword: Color::Rgb(0xe8, 0xa8, 0xc8),
        attr: Color::Rgb(0xe0, 0xb9, 0x8a),
        spark: Color::Rgb(0xff, 0xd2, 0x7a),
        number: Color::Rgb(0xff, 0xb0, 0x70),
        fail_fill: Color::Rgb(0x5a, 0x1a, 0x24),
        selected: Color::Rgb(0x2e, 0x24, 0x20),
        fill: Color::Rgb(0x4a, 0x2a, 0x1c),
        rule: Color::Rgb(0x3a, 0x2c, 0x25),
        dim: [Color::Rgb(0x5f, 0x50, 0x48), Color::Rgb(0x6f, 0x5e, 0x53)],
        punct: Color::Rgb(0x8a, 0x7a, 0x6e),
        faint: Color::Rgb(0x7f, 0x6d, 0x61),
        soft: Color::Rgb(0xb9, 0xa5, 0x97),
        strong: Color::Rgb(0xe0, 0xd3, 0xc7),
        heat: HEAT,
    }
};

type Rgb = (u8, u8, u8);

/// The dozen colours a theme is worked out from. Everything else in its
/// `Palette` is one of these or a mix of two, so a theme reads the same way
/// on a dark ground and on a light one.
struct Base {
    ground: Rgb,
    surface: Rgb,
    text: Rgb,
    label: Rgb,
    accent: Rgb,
    good: Rgb,
    warn: Rgb,
    bad: Rgb,
    blue: Rgb,
    magenta: Rgb,
    cyan: Rgb,
    orange: Rgb,
}

/// `percent` of the way from `a` to `b`
const fn mix(a: Rgb, b: Rgb, percent: u32) -> Rgb {
    const fn channel(a: u8, b: u8, percent: u32) -> u8 {
        ((a as u32 * (100 - percent) + b as u32 * percent + 50) / 100) as u8
    }
    (
        channel(a.0, b.0, percent),
        channel(a.1, b.1, percent),
        channel(a.2, b.2, percent),
    )
}

const fn rgb(c: Rgb) -> Color {
    Color::Rgb(c.0, c.1, c.2)
}

impl Base {
    const fn palette(&self) -> Palette {
        let (ground, text, accent) = (self.ground, self.text, self.accent);
        // Past the text, away from the ground: white on a dark ground,
        // black on a light one
        let light = ground.0 as u32 + ground.1 as u32 + ground.2 as u32
            > text.0 as u32 + text.1 as u32 + text.2 as u32;
        let end: Rgb = if light { (0, 0, 0) } else { (255, 255, 255) };
        let bright = mix(text, end, 50);
        // A gray halfway to the text reads fainter on a light ground than
        // on a dark one, so there the quiet grays sit further along
        let further = if light { 10 } else { 0 };
        Palette {
            ground: rgb(ground),
            surface: rgb(self.surface),
            text: rgb(text),
            bright: rgb(bright),
            accent: rgb(accent),
            accent_dim: rgb(mix(ground, accent, 65)),
            label: rgb(self.label),
            good: rgb(self.good),
            warn: rgb(self.warn),
            bad: rgb(self.bad),
            blue: rgb(self.blue),
            magenta: rgb(self.magenta),
            cool: rgb(self.cyan),
            string: rgb(mix(self.good, text, 25)),
            keyword: rgb(mix(self.magenta, text, 25)),
            attr: rgb(mix(self.orange, self.label, 35)),
            spark: rgb(mix(self.warn, text, 25)),
            number: rgb(self.orange),
            fail_fill: rgb(mix(ground, self.bad, 30)),
            selected: rgb(mix(ground, text, 12)),
            fill: rgb(mix(ground, accent, 25)),
            rule: rgb(mix(ground, text, 17)),
            dim: [
                rgb(mix(ground, text, 33 + further)),
                rgb(mix(ground, text, 40 + further)),
            ],
            punct: rgb(mix(ground, text, 52 + further)),
            faint: rgb(mix(ground, text, 47 + further)),
            soft: rgb(mix(ground, text, 70)),
            strong: rgb(mix(ground, text, 87)),
            heat: [
                rgb(mix(ground, accent, 12)),
                rgb(mix(ground, accent, 22)),
                rgb(mix(ground, accent, 34)),
                rgb(mix(ground, accent, 48)),
                rgb(mix(ground, accent, 64)),
                rgb(mix(ground, accent, 82)),
                rgb(accent),
                rgb(mix(accent, bright, 50)),
                rgb(mix(accent, bright, 85)),
            ],
        }
    }
}

static NORD: Palette = Base {
    ground: (0x2e, 0x34, 0x40),
    surface: (0x35, 0x3c, 0x4a),
    text: (0xd8, 0xde, 0xe9),
    label: (0x93, 0x9e, 0xb2),
    accent: (0x88, 0xc0, 0xd0),
    good: (0xa3, 0xbe, 0x8c),
    warn: (0xeb, 0xcb, 0x8b),
    bad: (0xbf, 0x61, 0x6a),
    blue: (0x81, 0xa1, 0xc1),
    magenta: (0xb4, 0x8e, 0xad),
    cyan: (0x8f, 0xbc, 0xbb),
    orange: (0xd0, 0x87, 0x70),
}
.palette();

static DRACULA: Palette = Base {
    ground: (0x28, 0x2a, 0x36),
    surface: (0x30, 0x33, 0x42),
    text: (0xf8, 0xf8, 0xf2),
    label: (0x92, 0x9c, 0xc6),
    accent: (0xff, 0x79, 0xc6),
    good: (0x50, 0xfa, 0x7b),
    warn: (0xf1, 0xfa, 0x8c),
    bad: (0xff, 0x55, 0x55),
    blue: (0x7d, 0xb5, 0xff),
    magenta: (0xbd, 0x93, 0xf9),
    cyan: (0x8b, 0xe9, 0xfd),
    orange: (0xff, 0xb8, 0x6c),
}
.palette();

static GRUVBOX: Palette = Base {
    ground: (0x28, 0x28, 0x28),
    surface: (0x32, 0x30, 0x2f),
    text: (0xeb, 0xdb, 0xb2),
    label: (0xa8, 0x99, 0x84),
    accent: (0xfe, 0x80, 0x19),
    good: (0xb8, 0xbb, 0x26),
    warn: (0xfa, 0xbd, 0x2f),
    bad: (0xfb, 0x49, 0x34),
    blue: (0x83, 0xa5, 0x98),
    magenta: (0xd3, 0x86, 0x9b),
    cyan: (0x8e, 0xc0, 0x7c),
    orange: (0xfe, 0x80, 0x19),
}
.palette();

static TOKYO_NIGHT: Palette = Base {
    ground: (0x1a, 0x1b, 0x26),
    surface: (0x21, 0x23, 0x32),
    text: (0xc0, 0xca, 0xf5),
    label: (0x80, 0x8a, 0xb8),
    accent: (0x7a, 0xa2, 0xf7),
    good: (0x9e, 0xce, 0x6a),
    warn: (0xe0, 0xaf, 0x68),
    bad: (0xf7, 0x76, 0x8e),
    blue: (0x7d, 0xcf, 0xff),
    magenta: (0xbb, 0x9a, 0xf7),
    cyan: (0x2a, 0xc3, 0xde),
    orange: (0xff, 0x9e, 0x64),
}
.palette();

static CATPPUCCIN: Palette = Base {
    ground: (0x1e, 0x1e, 0x2e),
    surface: (0x27, 0x27, 0x3a),
    text: (0xcd, 0xd6, 0xf4),
    label: (0x93, 0x99, 0xb2),
    accent: (0xcb, 0xa6, 0xf7),
    good: (0xa6, 0xe3, 0xa1),
    warn: (0xf9, 0xe2, 0xaf),
    bad: (0xf3, 0x8b, 0xa8),
    blue: (0x89, 0xb4, 0xfa),
    magenta: (0xf5, 0xc2, 0xe7),
    cyan: (0x94, 0xe2, 0xd5),
    orange: (0xfa, 0xb3, 0x87),
}
.palette();

static PAPER: Palette = Base {
    ground: (0xf7, 0xf3, 0xea),
    surface: (0xee, 0xe8, 0xdb),
    text: (0x2b, 0x26, 0x22),
    label: (0x6f, 0x65, 0x5c),
    accent: (0xc2, 0x41, 0x0c),
    good: (0x3f, 0x7d, 0x20),
    warn: (0x9a, 0x67, 0x00),
    bad: (0xc6, 0x28, 0x39),
    blue: (0x2f, 0x5f, 0xb3),
    magenta: (0x8a, 0x3e, 0xa8),
    cyan: (0x0f, 0x7b, 0x8a),
    orange: (0xb3, 0x52, 0x0e),
}
.palette();

impl Palette {
    /// A colour of the dark palette in this one. `heat` when the colour is
    /// a step of the heatmap rather than text or a rule, which share some
    /// grays.
    fn color(&self, color: Color, heat: bool, background: bool) -> Color {
        if heat {
            if let Some(level) = heat_level(Some(color)) {
                return self.heat[level];
            }
        }
        match color {
            Color::Reset if background => self.ground,
            Color::Reset => self.text,
            Color::Black => self.ground,
            Color::White => self.bright,
            Color::Gray => self.label,
            Color::DarkGray => self.dim[1],
            Color::Green | Color::LightGreen => self.good,
            Color::Yellow | Color::LightYellow => self.warn,
            Color::Red | Color::LightRed => self.bad,
            Color::Blue | Color::LightBlue => self.blue,
            Color::Magenta | Color::LightMagenta => self.magenta,
            Color::Cyan | Color::LightCyan => self.accent,
            Color::Indexed(n) => match n {
                81 => self.accent,
                67 => self.accent_dim,
                117 => self.cool,
                150 => self.string,
                176 => self.keyword,
                180 => self.attr,
                186 => self.spark,
                215 => self.number,
                52 => self.fail_fill,
                235 => self.surface,
                236 | 237 => self.selected,
                238 => self.fill,
                239 => self.rule,
                241 => self.dim[0],
                242 | 243 => self.dim[1],
                244 => self.punct,
                245 => self.faint,
                246..=248 => self.label,
                250 => self.soft,
                251..=253 => self.strong,
                254 | 255 => self.text,
                other => Color::Indexed(other),
            },
            other => other,
        }
    }
}

/// Leave `area` as drawn: pixel art whose colours are the picture
pub fn keep(area: Rect) {
    KEPT.with(|k| k.borrow_mut().push(area));
}

/// Draw with `render`, then adjust the frame for the theme
pub fn draw(f: &mut Frame, render: impl FnOnce(&mut Frame)) {
    KEPT.with(|k| k.borrow_mut().clear());
    super::kit::hits_clear();
    render(f);
    let mut kept = KEPT.with(|k| std::mem::take(&mut *k.borrow_mut()));
    // The settings page, over whichever screen asked for it: where it
    // covers a picture, it's the page that shows, in the theme's colours
    if let Some(page) = super::settings::render(f) {
        kept = kept.into_iter().flat_map(|k| around(k, page)).collect();
    }
    if let Some(page) = super::hooks::render(f) {
        kept = kept.into_iter().flat_map(|k| around(k, page)).collect();
    }
    if let Some(page) = super::export::render(f) {
        kept = kept.into_iter().flat_map(|k| around(k, page)).collect();
    }
    adjust(f.buffer_mut(), current(), &kept);
}

/// What's left of `area` around `hole`: the strips above and below it, and
/// those beside it
fn around(area: Rect, hole: Rect) -> Vec<Rect> {
    let cut = area.intersection(hole);
    if cut.is_empty() {
        return vec![area];
    }
    let strip = |x: u16, y: u16, right: u16, bottom: u16| Rect::new(x, y, right - x, bottom - y);
    [
        strip(area.x, area.y, area.right(), cut.y),
        strip(area.x, cut.bottom(), area.right(), area.bottom()),
        strip(area.x, cut.y, cut.x, cut.bottom()),
        strip(cut.right(), cut.y, area.right(), cut.bottom()),
    ]
    .into_iter()
    .filter(|r| !r.is_empty())
    .collect()
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
    let palette = theme.palette();
    let area = buf.area;
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let keep = kept.iter().any(|r| r.contains(Position { x, y }));
            let Some(cell) = buf.cell_mut((x, y)) else {
                continue;
            };
            if keep {
                // The picture keeps its colours; around it, the ground shows
                if let (Some(palette), Color::Reset) = (palette, cell.bg) {
                    cell.bg = palette.ground;
                }
                continue;
            }
            if let Some(palette) = palette {
                let symbol = cell.symbol();
                let block = matches!(symbol, "▀" | "▄" | "█");
                let braille = symbol
                    .chars()
                    .next()
                    .is_some_and(|c| ('\u{2800}'..='\u{28ff}').contains(&c));
                cell.fg = palette.color(cell.fg, block, false);
                cell.bg = palette.color(cell.bg, block || braille, true);
                continue;
            }
            // Panels are a true colour surface; elsewhere only their spacing
            if cell.bg == PANEL {
                cell.bg = Color::Reset;
            }
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
                _ => {}
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
        let detect = |no_color, asked, colorfgbg, colorterm| {
            detect(no_color, asked, None, colorfgbg, colorterm)
        };
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
        assert_eq!(
            detect(false, Some("Tokyo Night"), None, None),
            Theme::TokyoNight
        );
    }

    #[test]
    fn the_saved_theme_holds_where_the_environment_names_none() {
        let saved = |no_color, asked, saved| detect(no_color, asked, saved, Some("0;15"), None);
        assert_eq!(saved(false, None, Some("nord")), Theme::Nord);
        assert_eq!(saved(false, Some("dracula"), Some("nord")), Theme::Dracula);
        assert_eq!(saved(true, None, Some("nord")), Theme::None);
        // A name this pepe doesn't know leaves the terminal to decide
        assert_eq!(saved(false, None, Some("vaporwave")), Theme::Light);
        assert_eq!(saved(false, Some("vaporwave"), Some("nord")), Theme::Nord);
    }

    #[test]
    fn every_theme_reads_back_from_its_name() {
        for theme in Theme::ALL {
            assert_eq!(Theme::parse(theme.name()), Some(theme));
            assert_eq!(Theme::ALL[theme.index() as usize], theme);
        }
        assert_eq!(Theme::parse("solarized"), None);
    }

    /// Relative luminance, for the contrast between two colours
    fn luminance(color: Color) -> f64 {
        let Color::Rgb(r, g, b) = color else {
            panic!("{color:?} isn't true colour")
        };
        let channel = |c: u8| {
            let c = f64::from(c) / 255.0;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
    }

    fn contrast(a: Color, b: Color) -> f64 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    #[test]
    fn every_palette_reads_on_its_ground() {
        for theme in Theme::ALL {
            let Some(p) = theme.palette() else { continue };
            let name = theme.name();
            for (what, color, least) in [
                ("text", p.text, 7.0),
                ("label", p.label, 4.0),
                ("faint", p.faint, 3.0),
                ("accent", p.accent, 4.0),
                ("good", p.good, 3.5),
                ("warn", p.warn, 3.5),
                ("bad", p.bad, 3.0),
            ] {
                let on_ground = contrast(color, p.ground);
                assert!(on_ground >= least, "{name}: {what} is {on_ground:.1}:1");
            }
            // A tab's title is the ground on the accent
            assert!(contrast(p.ground, p.accent) >= 4.0, "{name}: tabs");
            // Busier is further from the ground, every step of the way
            let heat = p.heat.map(|c| contrast(c, p.ground));
            assert!(heat.windows(2).all(|w| w[0] < w[1]), "{name}: {heat:?}");
            assert_ne!(p.surface, p.ground, "{name}: panels");
        }
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
    fn a_picture_under_the_settings_page_gives_way_to_it() {
        let picture = Rect::new(2, 2, 10, 6);
        // Clear of it, the picture is kept whole
        assert_eq!(around(picture, Rect::new(20, 0, 5, 5)), [picture]);
        // Across a corner: what's above the page, and what's left of it
        let left = around(picture, Rect::new(8, 4, 30, 30));
        assert_eq!(left, [Rect::new(2, 2, 10, 2), Rect::new(2, 4, 6, 4)]);
        let cells = |rects: &[Rect]| rects.iter().map(|r| r.area()).sum::<u32>();
        assert_eq!(cells(&left), 60 - 16);
        // In the middle: a strip on every side, and none of them overlap
        let ring = around(picture, Rect::new(4, 4, 2, 2));
        assert_eq!((ring.len(), cells(&ring)), (4, 60 - 4));
        assert!(around(picture, Rect::new(0, 0, 50, 50)).is_empty());
    }

    #[test]
    fn another_palette_paints_the_same_roles() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf.set_string(0, 0, "a", Style::new().fg(Color::Indexed(81)));
        buf.set_string(1, 0, " ", Style::new().bg(PANEL));
        buf.set_string(2, 0, "▀", Style::new().fg(Color::Indexed(250)));
        adjust(&mut buf, Theme::Nord, &[]);
        assert_eq!(buf[(0, 0)].fg, Color::Rgb(0x88, 0xc0, 0xd0));
        assert_eq!(buf[(0, 0)].bg, Color::Rgb(0x2e, 0x34, 0x40));
        assert_eq!(buf[(1, 0)].bg, NORD.surface);
        assert_eq!(buf[(2, 0)].fg, NORD.heat[6]);
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
