//! Pepe the chili pepper, drawn as pixel art with half blocks. Its face and
//! the effects around it follow how the run is going.

use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};

/// Art is 14×12 pixels, drawn two pixels per cell; effects use 4 more columns
const ART_WIDTH: usize = 14;
const EFFECTS_WIDTH: usize = 4;
pub const WIDTH: u16 = (ART_WIDTH + EFFECTS_WIDTH) as u16;
/// Rows of art (six) plus one for the speech line
pub const HEIGHT: u16 = 7;

/// g stem, G calyx, r body, R shade, h highlight; E eyes and M mouth are
/// filled in per mood
const ART: [&str; 12] = [
    "...........gg.",
    "..........g...",
    "........GGGG..",
    "......rrrrrrR.",
    ".....rhhrrrrR.",
    "....rEErrEErR.",
    "...rrrrrrrrR..",
    "..rrrMMMMrrR..",
    "..rrrMMMMrR...",
    "..rrrrrrRR....",
    "...rrrRR......",
    "....rR........",
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
            Mood::Happy => "all green!",
            Mood::Sweating => "feeling the heat",
            Mood::OnFire => "it's on fire!",
            Mood::Sleeping => "zzz… paused",
            Mood::Proud => "nailed it!",
            Mood::Worried => "that was rough",
            Mood::Dizzy => "stopped",
        }
    }
}

#[derive(Clone, Copy)]
enum Eyes {
    Open,
    Wide,
    Closed,
}

#[derive(Clone, Copy)]
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
        Mood::Proud => (Eyes::Closed, Mouth::Smile),
        Mood::Worried => (Eyes::Open, Mouth::Frown),
        Mood::Dizzy => (Eyes::Wide, Mouth::Open),
    }
}

/// Color of the pixel at (x, y), or None where the art is transparent
fn pixel(mood: Mood, eyes: Eyes, mouth: Mouth, x: usize, y: usize) -> Option<Color> {
    let asleep = mood == Mood::Sleeping;
    let body = Color::Indexed(if asleep { 131 } else { 160 });
    let shade = Color::Indexed(if asleep { 88 } else { 124 });
    let dark = Color::Indexed(if asleep { 52 } else { 16 });
    match ART[y].as_bytes()[x] {
        b'g' => Some(Color::Indexed(70)),
        b'G' => Some(Color::Indexed(28)),
        b'r' => Some(body),
        b'R' => Some(shade),
        b'h' => Some(Color::Indexed(if asleep { 174 } else { 210 })),
        // Each eye is two pixels: white, then pupil
        b'E' => Some(match eyes {
            Eyes::Closed => dark,
            Eyes::Wide => Color::Indexed(231),
            Eyes::Open if matches!(x, 5 | 9) => Color::Indexed(231),
            Eyes::Open => Color::Indexed(16),
        }),
        // Mouth: a 4×2 area; each shape darkens some of it
        b'M' => {
            let (mx, my) = (x.saturating_sub(5), y - 7);
            let dark_here = match mouth {
                // ◡: corners on top, middle below
                Mouth::Smile => (my == 0) == (mx == 0 || mx == 3),
                Mouth::Flat => my == 0,
                Mouth::Open => (1..=2).contains(&mx),
                // ◠: middle on top, corners below
                Mouth::Frown => (my == 0) == (1..=2).contains(&mx),
            };
            Some(if dark_here { dark } else { body })
        }
        _ => None,
    }
}

/// The mascot: six rows of art with effects to the right. `frame` drives the
/// animation.
pub fn lines(mood: Mood, frame: u64) -> Vec<Line<'static>> {
    // Blink for one frame every four seconds while awake and happy
    let blink = matches!(mood, Mood::Happy | Mood::Waiting) && frame % 40 == 39;
    let (eyes, mouth) = face(mood, blink);
    let tick = (frame / 3) % 2 == 0;

    let effect = |row: usize| -> (&'static str, Color) {
        let flame = Color::Indexed(if tick { 214 } else { 202 });
        let sweat = Color::Indexed(117);
        let sparkle = Color::Indexed(228);
        match (mood, row) {
            (Mood::OnFire, 0) => (if tick { " ) (" } else { "( ) " }, flame),
            (Mood::OnFire, 1) => (if tick { "(  )" } else { " )( " }, flame),
            (Mood::Sweating | Mood::Worried, 2) if tick => ("  '", sweat),
            (Mood::Sweating | Mood::Worried, 3) if !tick => ("  '", sweat),
            (Mood::Sleeping, 0) => (if tick { "  z" } else { "   Z" }, sweat),
            (Mood::Sleeping, 1) => (if tick { " z" } else { "  z" }, sweat),
            (Mood::Proud, 0) => (" *", sparkle),
            (Mood::Proud, 2) => ("  +", sparkle),
            (Mood::Dizzy, 1) => (if tick { " @" } else { " ~" }, sparkle),
            _ => ("", Color::Reset),
        }
    };

    let mut out: Vec<Line> = (0..ART.len() / 2)
        .map(|row| {
            let mut spans: Vec<Span> = (0..ART_WIDTH)
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
        .collect();
    out.push(Line::raw(""));
    out
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
    fn art_is_rectangular() {
        assert!(ART.iter().all(|row| row.len() == ART_WIDTH));
        assert_eq!(ART.len() % 2, 0);
    }

    #[test]
    fn fits_its_box_in_every_mood_and_frame() {
        for mood in MOODS {
            assert!(mood.says().chars().count() <= WIDTH as usize);
            for frame in 0..80 {
                let lines = lines(mood, frame);
                assert_eq!(lines.len(), HEIGHT as usize);
                for line in lines {
                    assert!(line.width() <= WIDTH as usize, "{mood:?} {line:?}");
                }
            }
        }
    }

    #[test]
    fn faces_differ_by_mood() {
        let face_of = |mood| {
            let (eyes, mouth) = face(mood, false);
            (5..=10)
                .flat_map(|x| (5..=8).map(move |y| (x, y)))
                .map(|(x, y)| pixel(mood, eyes, mouth, x, y))
                .collect::<Vec<_>>()
        };
        assert_ne!(face_of(Mood::Happy), face_of(Mood::Worried));
        assert_ne!(face_of(Mood::Happy), face_of(Mood::Proud));
    }
}
