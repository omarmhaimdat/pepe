//! Pepe the chili pepper: a small pixel-art sprite drawn with half blocks
//! (two pixels per cell). Its face and the effects next to it follow how the
//! run is going.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
};

/// Sprite is 16×14 pixels, so 16×7 cells; effects use 4 more columns
const SPRITE_WIDTH: usize = 16;
const EFFECTS_WIDTH: usize = 4;
pub const WIDTH: u16 = (SPRITE_WIDTH + EFFECTS_WIDTH) as u16;
/// Seven rows of sprite; the header adds the speech line under it
pub const HEIGHT: u16 = 7;

/// K outline, R body, r shade, H highlight, G leaf, g leaf shade. The face is
/// painted over the body by `face_pixel`.
const SPRITE: [&str; 14] = [
    "..........gg....",
    ".........gg.....",
    ".....KgGGGGgK...",
    "....KRgGGGGgRK..",
    "...KRHRRRRRRRRK.",
    "...KRRRRRRRRRRrK",
    "..KRRRRRRRRRRRrK",
    "..KRRRRRRRRRRRrK",
    "..KRRRRRRRRRRrK.",
    "...KRRRRRRRRrK..",
    "...KRRRRRRRrK...",
    "..KRRRRRRrrK....",
    ".KRRRrrrKK......",
    "KrrKKK..........",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    /// No results yet
    Waiting,
    /// Running and healthy
    Happy,
    /// Some errors, or latency well above normal
    Sweating,
    /// Many errors
    OnFire,
    Sleeping,
    /// Finished healthy
    Proud,
    /// Finished degraded or failing
    Worried,
    /// Stopped by the user
    Dizzy,
}

impl Mood {
    pub fn says(self) -> &'static str {
        match self {
            Mood::Waiting => "warming up…",
            Mood::Happy => "all good",
            Mood::Sweating => "feeling the heat",
            Mood::OnFire => "it's on fire!",
            Mood::Sleeping => "paused",
            Mood::Proud => "nailed it",
            Mood::Worried => "have a look",
            Mood::Dizzy => "stopped",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Eyes {
    Open,
    /// Happy squint: ^ ^
    Smiling,
    Closed,
    Wide,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mouth {
    Smile,
    Flat,
    Open,
    Frown,
}

fn face(mood: Mood, blink: bool) -> (Eyes, Mouth) {
    match mood {
        _ if blink => (Eyes::Closed, Mouth::Smile),
        Mood::Waiting => (Eyes::Open, Mouth::Flat),
        Mood::Happy => (Eyes::Open, Mouth::Smile),
        Mood::Sweating => (Eyes::Wide, Mouth::Flat),
        Mood::OnFire => (Eyes::Wide, Mouth::Open),
        Mood::Sleeping => (Eyes::Closed, Mouth::Flat),
        Mood::Proud => (Eyes::Smiling, Mouth::Smile),
        Mood::Worried => (Eyes::Open, Mouth::Frown),
        Mood::Dizzy => (Eyes::Wide, Mouth::Open),
    }
}

/// What the face paints at (x, y), if anything
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Paint {
    White,
    Pupil,
    Dark,
    Blush,
}

/// Left edge of each eye; eyes are 2×2 at rows 5-6
const EYES_X: [usize; 2] = [5, 10];

fn face_pixel(eyes: Eyes, mouth: Mouth, blush: bool, x: usize, y: usize) -> Option<Paint> {
    for ex in EYES_X {
        let (dx, dy) = (x as isize - ex as isize, y as isize - 5);
        let paint = match (eyes, dx, dy) {
            (Eyes::Open, 0..=1, 0) | (Eyes::Open, 0, 1) => Some(Paint::White),
            (Eyes::Open, 1, 1) => Some(Paint::Pupil),
            (Eyes::Wide, 0..=1, 0..=1) => Some(Paint::White),
            (Eyes::Closed, 0..=1, 1) => Some(Paint::Dark),
            (Eyes::Smiling, 0 | 2, 1) | (Eyes::Smiling, 1, 0) => Some(Paint::Dark),
            _ => None,
        };
        if paint.is_some() {
            return paint;
        }
    }
    if blush && y == 7 && (x == 4 || x == 12) {
        return Some(Paint::Blush);
    }
    // Mouth: a 4×2 area at x 7-10, rows 8-9
    let (mx, my) = (x as isize - 7, y as isize - 8);
    if !(0..=3).contains(&mx) || !(0..=1).contains(&my) {
        return None;
    }
    let corner = mx == 0 || mx == 3;
    let dark = match mouth {
        Mouth::Smile => (my == 0) == corner,
        Mouth::Frown => (my == 1) == corner,
        Mouth::Flat => my == 1,
        Mouth::Open => !corner,
    };
    dark.then_some(Paint::Dark)
}

fn pixel(mood: Mood, eyes: Eyes, mouth: Mouth, x: usize, y: usize) -> Option<Color> {
    let asleep = mood == Mood::Sleeping;
    let blush = matches!(mood, Mood::Happy | Mood::Proud);
    let c = |awake: u8, sleeping: u8| Some(Color::Indexed(if asleep { sleeping } else { awake }));
    let base = SPRITE[y].as_bytes()[x];
    if base == b'R' {
        if let Some(paint) = face_pixel(eyes, mouth, blush, x, y) {
            return match paint {
                Paint::White => c(231, 250),
                Paint::Pupil | Paint::Dark => c(16, 16),
                Paint::Blush => c(211, 174),
            };
        }
    }
    match base {
        b'K' => c(52, 236),
        b'R' => c(196, 131),
        b'r' => c(160, 95),
        b'H' => c(217, 181),
        b'G' => c(76, 65),
        b'g' => c(28, 22),
        _ => None,
    }
}

/// The mascot, `HEIGHT` lines of at most `WIDTH` cells. `frame` drives the
/// animation.
pub fn lines(mood: Mood, frame: u64) -> Vec<Line<'static>> {
    // Pepe is a picture in colour: with colour off he stays home
    if super::theme::no_color() {
        return vec![Line::raw(""); HEIGHT as usize];
    }
    sprite(mood, frame)
}

/// The picture itself, whatever the terminal's colour setting
fn sprite(mood: Mood, frame: u64) -> Vec<Line<'static>> {
    // Blink for one frame every four seconds while awake
    let blink = matches!(mood, Mood::Happy | Mood::Waiting) && frame % 40 == 39;
    let (eyes, mouth) = face(mood, blink);
    let tick = (frame / 3) % 2 == 0;

    let effect = |row: usize| -> (&'static str, Color) {
        let spark = Color::Indexed(186);
        let cool = Color::Indexed(117);
        let hot = Color::Indexed(if tick { 215 } else { 209 });
        match (mood, row) {
            (Mood::OnFire, 0) => (if tick { " ,'" } else { " ', " }, hot),
            (Mood::OnFire, 1) => (if tick { " (" } else { "  )" }, hot),
            (Mood::Sweating | Mood::Worried, 2) if tick => ("  ,", cool),
            (Mood::Sweating | Mood::Worried, 3) if !tick => ("  '", cool),
            (Mood::Sleeping, 0) => (if tick { "  z" } else { "   z" }, cool),
            (Mood::Sleeping, 1) => (if tick { " z" } else { "  z" }, cool),
            (Mood::Proud, 1) => ("  ✦", spark),
            (Mood::Proud, 3) => (" ·", spark),
            (Mood::Dizzy, 1) => (if tick { " @" } else { " ~" }, spark),
            _ => ("", Color::Reset),
        }
    };

    (0..SPRITE.len() / 2)
        .map(|row| {
            let mut spans: Vec<Span> = (0..SPRITE_WIDTH)
                .map(|x| {
                    let top = pixel(mood, eyes, mouth, x, row * 2);
                    let bottom = pixel(mood, eyes, mouth, x, row * 2 + 1);
                    match (top, bottom) {
                        (Some(t), Some(b)) => Span::styled("▀", Style::new().fg(t).bg(b)),
                        (Some(t), None) => Span::styled("▀", Style::new().fg(t)),
                        (None, Some(b)) => Span::styled("▄", Style::new().fg(b)),
                        (None, None) => Span::raw(" "),
                    }
                })
                .collect();
            let (text, color) = effect(row);
            spans.push(Span::styled(text, Style::new().fg(color)));
            Line::from(spans)
        })
        .collect()
}

/// Keep the sprite's colours as drawn when the theme adjusts the frame: they
/// are the picture, not the palette. `area` is where the lines were drawn.
pub fn keep(area: Rect) {
    super::theme::keep(Rect {
        height: area.height.min(HEIGHT),
        ..area
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOODS: [Mood; 8] = [
        Mood::Waiting,
        Mood::Happy,
        Mood::Sweating,
        Mood::OnFire,
        Mood::Sleeping,
        Mood::Proud,
        Mood::Worried,
        Mood::Dizzy,
    ];

    #[test]
    fn sprite_is_rectangular_and_fits() {
        assert!(SPRITE.iter().all(|row| row.len() == SPRITE_WIDTH));
        assert_eq!(SPRITE.len(), HEIGHT as usize * 2);
    }

    #[test]
    fn fits_its_box_in_every_mood_and_frame() {
        for mood in MOODS {
            assert!(mood.says().chars().count() <= WIDTH as usize);
            for frame in 0..80 {
                let lines = sprite(mood, frame);
                assert_eq!(lines.len(), HEIGHT as usize);
                for line in lines {
                    assert!(line.width() <= WIDTH as usize, "{mood:?} {line:?}");
                }
            }
        }
    }

    #[test]
    fn face_is_painted_on_the_body() {
        // Every pixel the face touches must be body, or it would be lost
        for mood in MOODS {
            let (eyes, mouth) = face(mood, false);
            for (y, row) in SPRITE.iter().enumerate() {
                for x in 0..SPRITE_WIDTH {
                    if face_pixel(eyes, mouth, true, x, y).is_some() {
                        assert_eq!(row.as_bytes()[x], b'R', "{mood:?} at {x},{y}");
                    }
                }
            }
        }
    }

    #[test]
    fn moods_have_different_faces() {
        let face_of = |mood| {
            let (eyes, mouth) = face(mood, false);
            (0..SPRITE.len())
                .flat_map(|y| (0..SPRITE_WIDTH).map(move |x| (x, y)))
                .map(|(x, y)| pixel(mood, eyes, mouth, x, y))
                .collect::<Vec<_>>()
        };
        assert_ne!(face_of(Mood::Happy), face_of(Mood::Worried));
        assert_ne!(face_of(Mood::Happy), face_of(Mood::Proud));
        assert_ne!(face_of(Mood::Proud), face_of(Mood::Sleeping));
    }
}
