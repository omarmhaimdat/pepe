//! Pepe the chili pepper: pixel art drawn with half blocks (two pixels per
//! cell). He comes in two sizes: a 16×16 sprite for every screen, and a
//! 26×26 one for the dashboard header when the terminal is tall enough. His
//! face, arms and the effects around him follow how the run is going.

use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
};

use super::theme::{self, Theme};

/// The small sprite is 16×16 pixels, so 16×8 cells; the box keeps 4 more
/// columns so the speech line under him fits
const SMALL_WIDTH: usize = 16;
pub const WIDTH: u16 = SMALL_WIDTH as u16 + 4;
/// Eight rows of sprite; screens add the speech line under it
pub const HEIGHT: u16 = 8;
/// The big sprite: 26×26 pixels, 26×13 cells
pub const BIG_WIDTH: u16 = 26;
pub const BIG_HEIGHT: u16 = 13;

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

    /// Index of this mood's picture in `SMALL` and `BIG`
    fn index(self, blink: bool) -> usize {
        match self {
            Mood::Waiting if blink => 9,
            Mood::Happy if blink => 8,
            Mood::Waiting => 0,
            Mood::Happy => 1,
            Mood::Sweating => 2,
            Mood::OnFire => 3,
            Mood::Sleeping => 4,
            Mood::Proud => 5,
            Mood::Worried => 6,
            Mood::Dizzy => 7,
        }
    }
}

// Pixels: K outline, R body, r shade, H highlight, G leaf, g leaf shade,
// S stem, W white, P pupil, M mouth and lids, B blush, T tongue; Y and O
// flame and spark, C sweat and sleep. Generated from the design canvas.
const SMALL: [[&str; 16]; 10] = [
    // waiting
    [
        "...........KKK..",
        "..........KSSK..",
        ".........KSKK...",
        "......KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRK.",
        "...KRHRRRRRRRrK.",
        "...KRPWRRRPWRrK.",
        "..KRHPPRRRPPRrK.",
        "..KRRRRMMRRRrrK.",
        "KKKRRRRRRRRRrKKK",
        "KRRKRRRRRRRrK.RR",
        ".KKKRRRRRrrK..KK",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
    // happy
    [
        "...........KKK..",
        "..........KSSK..",
        ".........KSKK...",
        "......KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRK.",
        "...KRHRRRRRRRrK.",
        "...KRPWRRRPWRrK.",
        "..KRHPPRRRPPRrK.",
        "..KRBRMRRMRRBrK.",
        "KKKRRRRMMRRRrKKK",
        "KRRKRRRRRRRrK.RR",
        ".KKKRRRRRrrK..KK",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
    // sweating
    [
        "...........KKK..",
        "..........KSSK..",
        ".........KSKK...",
        "......KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRK.",
        "...KRHRRRRRRRrKC",
        "...KRWWRRRWWRrKC",
        "..KRHWPRRRWPRrK.",
        ".KRRRRMRMRRRrrRK",
        ".KRRRRRMRMRRrKRK",
        "...KRRRRRRRrK...",
        "...KRRRRRrrK....",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
    // on_fire
    [
        "...........KKY..",
        "....Y.....KSSOY.",
        "...YO....KSKK...",
        "...OO.KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRKY",
        "...KRHRRRRRRRrKK",
        "...KRWWRRRWWRrKK",
        "..KRHWPRRRWPRrK.",
        "..KRRRRMMRRRrrK.",
        "KKKRRRRTTRRRrK..",
        "KRRKRRRRRRRrK...",
        ".KKKRRRRRrrK....",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
    // sleeping
    [
        "...........KKCCC",
        "..........KSSKC.",
        ".........KSKK...",
        "......KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRK.",
        "...KRHRRRRRRRrK.",
        "...KRHRRRRRRRrK.",
        "..KRHMMRRRMMRrK.",
        "..KRBRRRRRRRBrK.",
        "KKKRRRMMMMRRrKKK",
        "KRRKRRRRRRRrK.RR",
        ".KKKRRRRRrrK..KK",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
    // proud
    [
        "...........KKK..",
        "..........KSSKY.",
        ".........KSKKYY.",
        "..Y...KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRK.",
        "K..KRHRRRRRRRrKK",
        "KK.KRMMRRRMMRrKK",
        ".KRRMRRMRMRRMrRK",
        "..KRBRMMMMRRBrK.",
        "..KRRRRTTRRRrK..",
        "...KRRRRRRRrK...",
        "...KRRRRRrrK....",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
    // worried
    [
        "...........KKK..",
        "..........KSSK..",
        ".........KSKK...",
        "......KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRK.",
        "...KRHRRRRRRRrK.",
        "...KRPWRRRPWRrK.",
        "..KRHPPRRRPPRrK.",
        ".KRRRRRMMRRRrrRK",
        ".KRRRRMRRMRRrKRK",
        "...KRRRRRRRrK...",
        "...KRRRRRrrK....",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
    // dizzy
    [
        "...........KKK..",
        "..........KSSK..",
        "...Y.....KSKK.Y.",
        "......KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRK.",
        "...KRHRRRRRRRrK.",
        "...KRMRRRRMRRrK.",
        "..KRHRMRRRRMRrK.",
        "..KRRRMRMRRRrrK.",
        "KKKRRRRMRMRRrKKK",
        "KRRKRRRRRRRrK.RR",
        ".KKKRRRRRrrK..KK",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
    // happy_blink
    [
        "...........KKK..",
        "..........KSSK..",
        ".........KSKK...",
        "......KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRK.",
        "...KRHRRRRRRRrK.",
        "...KRHRRRRRRRrK.",
        "..KRHMMRRRMMRrK.",
        "..KRBRMRRMRRBrK.",
        "KKKRRRRMMRRRrKKK",
        "KRRKRRRRRRRrK.RR",
        ".KKKRRRRRrrK..KK",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
    // waiting_blink
    [
        "...........KKK..",
        "..........KSSK..",
        ".........KSKK...",
        "......KKKSKKKK..",
        ".....KGGgGGgGGK.",
        "....KRgGGgGGgRK.",
        "...KRHRRRRRRRrK.",
        "...KRHRRRRRRRrK.",
        "..KRHMMRRRMMRrK.",
        "..KRRRRMMRRRrrK.",
        "KKKRRRRRRRRRrKKK",
        "KRRKRRRRRRRrK.RR",
        ".KKKRRRRRrrK..KK",
        "..KRRRRrrKK.....",
        ".KRrrrrKK.......",
        ".KrKKKK.........",
    ],
];

const BIG: [[&str; 26]; 10] = [
    // waiting
    [
        "...............KKK........",
        "..............KSSSK.......",
        ".............KSSKK........",
        "............KSSK..........",
        "........KKKKKSGKKKK.......",
        ".......KGGGgGGGGgGGKK.....",
        "......KGgGGGGgGGGGGgGK....",
        ".....KRGgRRRGGRRRRgGRRK...",
        "....KRHRgRRRRgRRRRRgRrK...",
        "....KRHRRRRRRRRRRRRRRrK...",
        "...KRHRRPWRRRRRPWRRRRrK...",
        "...KRHRRPPRRRRRPPRRRrrK...",
        "...KRRRRPPRRRRRPPRRRrK....",
        "...KRRRRRRRRRRRRRRRRrK....",
        "..KKRRRRRRRMMRRRRRRrrKK...",
        ".KRRRRRRRRRMMRRRRRRrKrRK..",
        "..KKKRRRRRRRRRRRRRrrKKK...",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
    // happy
    [
        "...............KKK........",
        "..............KSSSK.......",
        ".............KSSKK........",
        "............KSSK..........",
        "........KKKKKSGKKKK.......",
        ".......KGGGgGGGGgGGKK.....",
        "......KGgGGGGgGGGGGgGK....",
        ".....KRGgRRRGGRRRRgGRRK...",
        "....KRHRgRRRRgRRRRRgRrK...",
        "....KRHRRRRRRRRRRRRRRrK...",
        "...KRHRRPWRRRRRPWRRRRrK...",
        "...KRHRRPPRRRRRPPRRRrrK...",
        "...KRRRRPPRRRRRPPRRRrK....",
        "...KRRBBRRRRRRRRRBBRrK....",
        "..KKRRRRRRMRRMRRRRRrrKK...",
        ".KRRRRRRRRRMMRRRRRRrKrRK..",
        "..KKKRRRRRRRRRRRRRrrKKK...",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
    // sweating
    [
        "...............KKK........",
        "..............KSSSK.......",
        ".............KSSKK........",
        "............KSSK..........",
        "........KKKKKSGKKKK.......",
        ".......KGGGgGGGGgGGKK.....",
        "......KGgGGGGgGGGGGgGK....",
        ".....KRGgRRRGGRRRRgGRRK...",
        "....KRHRgRRRRgRRRRRgRrK.C.",
        "....KRHRRRRRRRRRRRRRRrKCC.",
        "...KRHRRWWRRRRRWWRRRRrKCC.",
        "...KRHRRWPRRRRRWPRRRrrK...",
        "..KKRRRRWWRRRRRWWRRRrKK...",
        ".KRRRRRRRRRRRRRRRRRRrRRK..",
        ".KRRRRRRRRMRMRRRRRRrrrRK..",
        "..KKRRRRRRRMRMRRRRRrKKK...",
        "....KRRRRRRRRRRRRRrrK.....",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
    // on_fire
    [
        "...............KKK....Y...",
        ".....Y........KSSSK..YY...",
        ".....YY..Y...KSSKK...YOY..",
        "....YOY..O..KSSK....YOOY..",
        "....YOOYKKKKKSGKKKK..OO...",
        ".....OOKGGGgGGGGgGGKK.....",
        "......KGgGGGGgGGGGGgGK....",
        ".....KRGgRRRGGRRRRgGRRKYK.",
        "....KRHRgRRRRgRRRRRgRrYYK.",
        "....KRHRRRRRRRRRRRRRRrKRK.",
        "...KRHRRWWRRRRRWWRRRRrKRK.",
        "...KRHRRWPRRRRRWPRRRrrRK..",
        "...KRRRRWWRRRRRWWRRRrKK...",
        "...KRRRRRRRRRRRRRRRRrK....",
        "..KKRRRRRRRMMRRRRRRrrK....",
        ".KRRRRRRRRMTTMRRRRRrK.....",
        "..KKKRRRRRRMMRRRRRrrK.....",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
    // sleeping
    [
        "...............KKK..CCCC..",
        "..............KSSSK...C...",
        ".............KSSKK...C....",
        "............KSSK....CCCCCC",
        "........KKKKKSGKKKK.....CC",
        ".......KGGGgGGGGgGGKK.....",
        "......KGgGGGGgGGGGGgGK....",
        ".....KRGgRRRGGRRRRgGRRK...",
        "....KRHRgRRRRgRRRRRgRrK...",
        "....KRHRRRRRRRRRRRRRRrK...",
        "...KRHRRRRRRRRRRRRRRRrK...",
        "...KRHRRRRRRRRRRRRRRrrK...",
        "...KRRRRMMRRRRRMMRRRrK....",
        "...KRRBBRRRRRRRRRBBRrK....",
        "..KKRRRRRRRRRRRRRRRrrKK...",
        ".KRRRRRRRRMMMMRRRRRrKrRK..",
        "..KKKRRRRRRRRRRRRRrrKKK...",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
    // proud
    [
        "...............KKK........",
        "..............KSSSK.......",
        ".............KSSKK........",
        "............KSSK.......Y..",
        "...Y....KKKKKSGKKKK...YYY.",
        ".......KGGGgGGGGgGGKK..Y..",
        ".K....KGgGGGGgGGGGGgGK..K.",
        "KRK..KRGgRRRGGRRRRgGRRKKRK",
        "KRK.KRHRgRRRRgRRRRRgRrKKRK",
        ".KRKKRHRRRRRRRRRRRRRRrKRK.",
        "..KRRHRRMMRRRRRMMRRRRrRK..",
        "...KRHRMRRMRRRMRRMRRrrK...",
        "...KRRRRRRRRRRRRRRRRrK....",
        "...KRRBBRRRRRRRRRBBRrK....",
        "...KRRRRRRMMMMRRRRRrrK....",
        "...KRRRRRRRTTRRRRRRrK.....",
        "....KRRRRRRRRRRRRRrrK.....",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
    // worried
    [
        "...............KKK........",
        "..............KSSSK.......",
        ".............KSSKK........",
        "............KSSK..........",
        "........KKKKKSGKKKK.......",
        ".......KGGGgGGGGgGGKK.....",
        "......KGgGGGGgGGGGGgGK....",
        ".....KRGgRRRGGRRRRgGRRK...",
        "....KRHRgRRRRgRRRRRgRrK...",
        "....KRHRRRRRRRRRRRRRRrK...",
        "...KRHRRPWRRRRRPWRRRRrK...",
        "...KRHRRPPRRRRRPPRRRrrK...",
        "..KKRRRRPPRRRRRPPRRRrKK...",
        ".KRRRRRRRRRRRRRRRRRRrRRK..",
        ".KRRRRRRRRRMMRRRRRRrrrRK..",
        "..KKRRRRRRMRRMRRRRRrKKK...",
        "....KRRRRRRRRRRRRRrrK.....",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
    // dizzy
    [
        "...............KKK........",
        "......Y.Y.....KSSSK.......",
        ".............KSSKK.Y......",
        "............KSSK..........",
        "........KKKKKSGKKKK.......",
        ".......KGGGgGGGGgGGKK.....",
        "......KGgGGGGgGGGGGgGK....",
        ".....KRGgRRRGGRRRRgGRRK...",
        "....KRHRgRRRRgRRRRRgRrK...",
        "....KRHRRRRRRRRRRRRRRrK...",
        "...KRHRMRMRRRRMRMRRRRrK...",
        "...KRHRRMRRRRRRMRRRRrrK...",
        "...KRRRMRMRRRRMRMRRRrK....",
        "...KRRRRRRRRRRRRRRRRrK....",
        "..KKRRRRRRMRMRRRRRRrrKK...",
        ".KRRRRRRRRRMRMRRRRRrKrRK..",
        "..KKKRRRRRRRRRRRRRrrKKK...",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
    // happy_blink
    [
        "...............KKK........",
        "..............KSSSK.......",
        ".............KSSKK........",
        "............KSSK..........",
        "........KKKKKSGKKKK.......",
        ".......KGGGgGGGGgGGKK.....",
        "......KGgGGGGgGGGGGgGK....",
        ".....KRGgRRRGGRRRRgGRRK...",
        "....KRHRgRRRRgRRRRRgRrK...",
        "....KRHRRRRRRRRRRRRRRrK...",
        "...KRHRRRRRRRRRRRRRRRrK...",
        "...KRHRRRRRRRRRRRRRRrrK...",
        "...KRRRRMMRRRRRMMRRRrK....",
        "...KRRBBRRRRRRRRRBBRrK....",
        "..KKRRRRRRMRRMRRRRRrrKK...",
        ".KRRRRRRRRRMMRRRRRRrKrRK..",
        "..KKKRRRRRRRRRRRRRrrKKK...",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
    // waiting_blink
    [
        "...............KKK........",
        "..............KSSSK.......",
        ".............KSSKK........",
        "............KSSK..........",
        "........KKKKKSGKKKK.......",
        ".......KGGGgGGGGgGGKK.....",
        "......KGgGGGGgGGGGGgGK....",
        ".....KRGgRRRGGRRRRgGRRK...",
        "....KRHRgRRRRgRRRRRgRrK...",
        "....KRHRRRRRRRRRRRRRRrK...",
        "...KRHRRRRRRRRRRRRRRRrK...",
        "...KRHRRRRRRRRRRRRRRrrK...",
        "...KRRRRMMRRRRRMMRRRrK....",
        "...KRRRRRRRRRRRRRRRRrK....",
        "..KKRRRRRRRMMRRRRRRrrKK...",
        ".KRRRRRRRRRMMRRRRRRrKrRK..",
        "..KKKRRRRRRRRRRRRRrrKKK...",
        "....KRRRRRRRRRRRRrrK......",
        ".....KRRRRRRRRRRrrK.......",
        ".....KRRRRRRRRRrrK........",
        "....KRRRRRRRRRrrK.........",
        "...KRRRRRRRRrrK...........",
        "..KRRRRRRrrKK.............",
        ".KRrrrrrKK................",
        ".KrrKKKK..................",
        "..KK......................",
    ],
];

/// A pixel's colour: true colour in the pepe theme, the 256-colour palette
/// everywhere else
fn color(pixel: u8, true_color: bool) -> Option<Color> {
    let (rgb, indexed) = match pixel {
        b'K' => ((0x2c, 0x0a, 0x0b), 52),
        b'R' => ((0xef, 0x33, 0x24), 196),
        b'r' => ((0xb8, 0x1c, 0x1e), 160),
        b'H' => ((0xff, 0xa0, 0x82), 217),
        b'G' => ((0x7a, 0xc8, 0x46), 76),
        b'g' | b'S' => ((0x2e, 0x78, 0x32), 28),
        b'W' => ((0xff, 0xff, 0xff), 231),
        b'P' | b'M' => ((0x24, 0x08, 0x0a), 16),
        b'B' | b'T' => ((0xff, 0x80, 0x9a), 211),
        b'Y' => ((0xff, 0xd2, 0x50), 221),
        b'O' => ((0xff, 0x78, 0x28), 208),
        b'C' => ((0x8c, 0xd2, 0xff), 117),
        _ => return None,
    };
    Some(if true_color {
        Color::Rgb(rgb.0, rgb.1, rgb.2)
    } else {
        Color::Indexed(indexed)
    })
}

/// The picture for `mood` at `frame`, as rows of pixels. The effects move on
/// two frames: flames flicker, sweat drips, the zzz drift up.
fn pixels(rows: &[&str], mood: Mood, frame: u64) -> Vec<Vec<u8>> {
    let mut grid: Vec<Vec<u8>> = rows.iter().map(|r| r.as_bytes().to_vec()).collect();
    let tick = (frame / 3) % 2 == 1;
    if !tick {
        return grid;
    }
    match mood {
        Mood::OnFire => {
            for px in grid.iter_mut().flatten() {
                *px = match *px {
                    b'Y' => b'O',
                    b'O' => b'Y',
                    other => other,
                };
            }
        }
        Mood::Sweating | Mood::Sleeping => {
            // The drop falls a pixel, the zzz rise one
            let down = mood == Mood::Sweating;
            let from = grid.clone();
            for row in grid.iter_mut() {
                for px in row.iter_mut().filter(|p| **p == b'C') {
                    *px = b'.';
                }
            }
            for (y, row) in from.iter().enumerate() {
                for (x, &px) in row.iter().enumerate() {
                    let to = if down { y + 1 } else { y.wrapping_sub(1) };
                    if px == b'C' && to < grid.len() && grid[to][x] == b'.' {
                        grid[to][x] = b'C';
                    }
                }
            }
        }
        _ => {}
    }
    grid
}

/// Pixel rows drawn two to a cell with half blocks
fn draw(grid: &[Vec<u8>], width: usize, true_color: bool) -> Vec<Line<'static>> {
    (0..grid.len().div_ceil(2))
        .map(|row| {
            let spans: Vec<Span> = (0..width)
                .map(|x| {
                    let at = |y: usize| grid.get(y).and_then(|r| r.get(x)).copied();
                    let top = at(row * 2).and_then(|p| color(p, true_color));
                    let bottom = at(row * 2 + 1).and_then(|p| color(p, true_color));
                    match (top, bottom) {
                        (Some(t), Some(b)) => Span::styled("▀", Style::new().fg(t).bg(b)),
                        (Some(t), None) => Span::styled("▀", Style::new().fg(t)),
                        (None, Some(b)) => Span::styled("▄", Style::new().fg(b)),
                        (None, None) => Span::raw(" "),
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

fn sprite(rows: &[&str], width: usize, mood: Mood, frame: u64) -> Vec<Line<'static>> {
    let true_color = theme::current() == Theme::Pepe;
    draw(&pixels(rows, mood, frame), width, true_color)
}

/// Blink for one frame every four seconds while awake
fn blinks(mood: Mood, frame: u64) -> bool {
    matches!(mood, Mood::Happy | Mood::Waiting) && frame % 40 == 39
}

/// The small mascot, `HEIGHT` lines of `WIDTH` cells at most. `frame` drives
/// the animation.
pub fn lines(mood: Mood, frame: u64) -> Vec<Line<'static>> {
    // Pepe is a picture in colour: with colour off he stays home
    if theme::no_color() {
        return vec![Line::raw(""); HEIGHT as usize];
    }
    sprite(
        &SMALL[mood.index(blinks(mood, frame))],
        SMALL_WIDTH,
        mood,
        frame,
    )
}

/// The big mascot, `BIG_HEIGHT` lines of `BIG_WIDTH` cells
pub fn big_lines(mood: Mood, frame: u64) -> Vec<Line<'static>> {
    if theme::no_color() {
        return vec![Line::raw(""); BIG_HEIGHT as usize];
    }
    sprite(
        &BIG[mood.index(blinks(mood, frame))],
        BIG_WIDTH as usize,
        mood,
        frame,
    )
}

/// Keep the sprite's colours as drawn when the theme adjusts the frame: they
/// are the picture, not the palette. `area` is where the lines were drawn.
pub fn keep(area: Rect) {
    keep_rows(area, HEIGHT);
}

/// `keep` for a sprite `rows` cells tall
pub fn keep_rows(area: Rect, rows: u16) {
    theme::keep(Rect {
        height: area.height.min(rows),
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
    fn sprites_are_rectangular_and_fit() {
        for picture in SMALL {
            assert!(picture.iter().all(|r| r.len() == SMALL_WIDTH));
            assert_eq!(picture.len(), HEIGHT as usize * 2);
        }
        for picture in BIG {
            assert!(picture.iter().all(|r| r.len() == BIG_WIDTH as usize));
            assert_eq!(picture.len(), BIG_HEIGHT as usize * 2);
        }
    }

    #[test]
    fn every_pixel_has_a_colour() {
        for row in SMALL.iter().flatten().chain(BIG.iter().flatten()) {
            for px in row.bytes().filter(|&p| p != b'.') {
                assert!(color(px, true).is_some(), "{}", px as char);
            }
        }
    }

    #[test]
    fn fits_its_box_in_every_mood_and_frame() {
        for mood in MOODS {
            assert!(mood.says().chars().count() <= WIDTH as usize);
            for frame in 0..80 {
                let small = sprite(
                    &SMALL[mood.index(blinks(mood, frame))],
                    SMALL_WIDTH,
                    mood,
                    frame,
                );
                assert_eq!(small.len(), HEIGHT as usize);
                assert!(small.iter().all(|l| l.width() <= WIDTH as usize));
                let big = sprite(
                    &BIG[mood.index(blinks(mood, frame))],
                    BIG_WIDTH as usize,
                    mood,
                    frame,
                );
                assert_eq!(big.len(), BIG_HEIGHT as usize);
                assert!(big.iter().all(|l| l.width() == BIG_WIDTH as usize));
            }
        }
    }

    #[test]
    fn moods_have_different_faces() {
        for (i, a) in SMALL.iter().enumerate() {
            for b in &SMALL[i + 1..] {
                assert_ne!(a, b);
            }
        }
        for (i, a) in BIG.iter().enumerate() {
            for b in &BIG[i + 1..] {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn effects_move_between_frames() {
        let fire = |frame| pixels(&BIG[Mood::OnFire.index(false)], Mood::OnFire, frame);
        assert_ne!(fire(0), fire(3));
        let sweat = |frame| pixels(&SMALL[Mood::Sweating.index(false)], Mood::Sweating, frame);
        assert_ne!(sweat(0), sweat(3));
        let calm = |frame| pixels(&SMALL[Mood::Happy.index(false)], Mood::Happy, frame);
        assert_eq!(calm(0), calm(3));
    }
}
