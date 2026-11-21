//! The cat Nick (NS-51): the operator's cat, in pixel art, at the bottom of the column on the
//! left. Pure: no I/O, no database, no screen. Her art is data (`PALETTE`, `REST`, `BLINK`),
//! each cell of the terminal shows two pixels of it stacked (`cells`), and when she blinks and
//! meows is a state machine (`Mascot`) whose clock (an `Instant` by parameter) and dice (`Dice`)
//! come from outside, so the tests drive both. ABOUT.md, "Rust no projeto: a gata Nick no painel
//! (NS-51)".

use std::collections::hash_map::RandomState;
use std::hash::BuildHasher;
use std::time::{Duration, Instant};

use ratatui::style::Color;

/// The rows of the terminal she takes (two pixels each).
pub const MASCOT_ROWS: u16 = 7;
/// The columns of the terminal she takes (one pixel each).
pub const MASCOT_COLS: u16 = 17;
/// The fewest rows left for the tree (or the settings) above her: with less room inside the
/// column she is not drawn. A fixed rule, so opening or closing a node never shows nor hides her.
pub const TREE_MIN_ROWS: u16 = 8;
/// The columns of the "Miau!" bubble, on her right after one column of space.
pub const BUBBLE_COLS: u16 = 17;
/// The rows of the bubble (its top, the text and its bottom), on her last rows.
pub const BUBBLE_ROWS: u16 = 3;
/// The text of the bubble.
pub const MEOW_TEXT: &str = "Miau!";

/// The interval drawn between two blinks, and how long one lasts.
pub const BLINK_EVERY: (Duration, Duration) = (Duration::from_secs(6), Duration::from_secs(20));
pub const BLINK_FOR: Duration = Duration::from_millis(150);
/// The interval drawn between two meows, and how long the bubble stays.
pub const MEOW_EVERY: (Duration, Duration) =
    (Duration::from_secs(3 * 60), Duration::from_secs(10 * 60));
pub const MEOW_FOR: Duration = Duration::from_secs(3);

/// The short fixed intervals of `Mascot::fast` (the test hook NICK_TEST_MASCOT=fast).
const FAST_BLINK_EVERY: Duration = Duration::from_millis(40);
const FAST_BLINK_FOR: Duration = Duration::from_millis(20);
const FAST_MEOW_EVERY: Duration = Duration::from_millis(120);
const FAST_MEOW_FOR: Duration = Duration::from_millis(60);

/// The colors of the art, by letter (the approved palette); `.` is transparent.
pub const PALETTE: [(char, (u8, u8, u8)); 7] = [
    ('D', (52, 36, 30)),    // dark brown: ears and mask
    ('B', (110, 78, 62)),   // mid brown: inside the ears
    ('C', (196, 170, 140)), // cream: forehead and body
    ('W', (242, 240, 234)), // white: the V of the muzzle, chest and paws
    ('E', (110, 170, 225)), // blue of the eyes
    ('K', (20, 20, 24)),    // pupil
    ('P', (232, 160, 168)), // nose
];

/// Still (the approved variant B, the paws on the edge): one pixel a character.
pub const REST: [&str; 14] = [
    ".D.............D.",
    ".DD...........DD.",
    ".DBD.........DBD.",
    ".DBBD.......DBBD.",
    ".DBBDCCCCCCCDBBD.",
    "DDDDCCCCWCCCCDDDD",
    "DDDDDCCWWWCCDDDDD",
    "DDEEDDDWWWDDDEEDD",
    "DDEKDDDWWWDDDKEDD",
    ".DDDDDWWWWWDDDDD.",
    "..DDDWWWPWWWDDD..",
    "...DWWWWWWWWWD...",
    "...WWWWWWWWWWW...",
    "...WWW.....WWW...",
];

/// Blinking: `REST` with the eyes (pixel rows 7 and 8) turned into a dark line.
pub const BLINK: [&str; 14] = [
    ".D.............D.",
    ".DD...........DD.",
    ".DBD.........DBD.",
    ".DBBD.......DBBD.",
    ".DBBDCCCCCCCDBBD.",
    "DDDDCCCCWCCCCDDDD",
    "DDDDDCCWWWCCDDDDD",
    "DDDDDDDWWWDDDDDDD",
    "DDKKDDDWWWDDDKKDD",
    ".DDDDDWWWWWDDDDD.",
    "..DDDWWWPWWWDDD..",
    "...DWWWWWWWWWD...",
    "...WWWWWWWWWWW...",
    "...WWW.....WWW...",
];

/// The levels of the 6x6x6 cube of the xterm-256 palette (indexes 16 to 231).
const CUBE_LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

/// The color of `letter` of the palette (None for `.` or an unknown letter).
pub fn rgb_of(letter: char) -> Option<(u8, u8, u8)> {
    PALETTE
        .iter()
        .find(|(key, _)| *key == letter)
        .map(|(_, rgb)| *rgb)
}

/// The color of `letter` on a terminal with truecolor (the exact RGB) or without it (the
/// nearest of the 256 palette); None for `.`.
pub fn color_of(letter: char, truecolor: bool) -> Option<Color> {
    rgb_of(letter).map(|rgb| resolve(rgb, truecolor))
}

/// `rgb` as is with truecolor, else the nearest color of the 256 palette.
fn resolve((r, g, b): (u8, u8, u8), truecolor: bool) -> Color {
    if truecolor {
        Color::Rgb(r, g, b)
    } else {
        Color::Indexed(nearest_256(r, g, b))
    }
}

/// The colors of the letters on one kind of terminal, resolved once (review NS-51, problem 1):
/// without truecolor `color_of` runs `nearest_256` over 240 candidates, and the drawing would
/// do it for every cell of every frame to reach the same 7 colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette([(char, Color); PALETTE.len()]);

impl Palette {
    /// The palette of a terminal with truecolor (the exact RGB) or without it (the 256 palette).
    pub fn new(truecolor: bool) -> Palette {
        Palette(PALETTE.map(|(letter, rgb)| (letter, resolve(rgb, truecolor))))
    }

    /// The color of `letter` (None for `.` or an unknown letter).
    pub fn color(&self, letter: char) -> Option<Color> {
        self.0
            .iter()
            .find(|(key, _)| *key == letter)
            .map(|(_, color)| *color)
    }
}

/// The nearest color of the xterm-256 palette to `r`, `g`, `b`, by the euclidean distance in
/// RGB, over the 6x6x6 cube (16-231) and the gray ramp (232-255, `8 + 10·i`). The first 16 are
/// left out: the theme of the terminal redefines them. On a tie, the lowest index.
pub fn nearest_256(r: u8, g: u8, b: u8) -> u8 {
    let distance = |(cr, cg, cb): (u8, u8, u8)| {
        let d = |a: u8, b: u8| (i32::from(a) - i32::from(b)).pow(2);
        d(r, cr) + d(g, cg) + d(b, cb)
    };
    let cube = (0..216u16).map(|i| {
        let level = |n: u16| CUBE_LEVELS[usize::from(n)];
        (16 + i, (level(i / 36), level(i / 6 % 6), level(i % 6)))
    });
    let grays = (0..24u16).map(|i| {
        let gray = u8::try_from(8 + 10 * i).unwrap_or(u8::MAX);
        (232 + i, (gray, gray, gray))
    });
    cube.chain(grays)
        .min_by_key(|(index, rgb)| (distance(*rgb), *index))
        .map_or(16, |(index, _)| u8::try_from(index).unwrap_or(u8::MAX))
}

/// The pose of the cat.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pose {
    /// Still (`REST`).
    Rest,
    /// Blinking (`BLINK`).
    Blink,
}

impl Pose {
    /// The pixels of the pose.
    pub fn pixels(self) -> &'static [&'static str; 14] {
        match self {
            Pose::Rest => &REST,
            Pose::Blink => &BLINK,
        }
    }
}

/// One cell of the terminal: two pixels stacked, still without color. `symbol` is `▀` with the
/// top pixel colored (the top in the foreground, the bottom, if any, in the background), `▄`
/// with only the bottom one (in the foreground), and None with neither (the cell is not touched).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellArt {
    pub symbol: Option<char>,
    pub top: Option<char>,
    pub bottom: Option<char>,
}

/// The cells of `pose`, by row and column of the terminal.
pub fn cells(pose: Pose) -> [[CellArt; MASCOT_COLS as usize]; MASCOT_ROWS as usize] {
    let pixels = pose.pixels();
    let mut out = [[CellArt::default(); MASCOT_COLS as usize]; MASCOT_ROWS as usize];
    for (row, cells) in out.iter_mut().enumerate() {
        let top = pixels[2 * row].chars();
        let bottom = pixels[2 * row + 1].chars();
        for (cell, (t, b)) in cells.iter_mut().zip(top.zip(bottom)) {
            let colored = |c: char| rgb_of(c).map(|_| c);
            let (top, bottom) = (colored(t), colored(b));
            *cell = CellArt {
                symbol: match (top, bottom) {
                    (Some(_), _) => Some('▀'),
                    (None, Some(_)) => Some('▄'),
                    (None, None) => None,
                },
                top,
                bottom,
            };
        }
    }
    out
}

/// What is shown now: the pose and whether the "Miau!" bubble is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame {
    pub pose: Pose,
    pub meowing: bool,
}

/// The dice of the intervals: a duration between `min` and `max`, both included.
pub trait Dice {
    fn pick(&mut self, min: Duration, max: Duration) -> Duration;
}

/// The dice of production: a xorshift64* seeded by the `RandomState` of the std (random for
/// each process). No crate: the cat needs no cryptographic quality.
pub struct SystemDice {
    state: u64,
}

impl SystemDice {
    pub fn new() -> SystemDice {
        // The state of a xorshift must never be zero.
        let seed = RandomState::new().hash_one(0x6e69_636bu64) | 1;
        SystemDice { state: seed }
    }

    fn next(&mut self) -> u64 {
        let mut x = self.state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.state = x;
        x.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
}

impl Default for SystemDice {
    fn default() -> SystemDice {
        SystemDice::new()
    }
}

impl Dice for SystemDice {
    fn pick(&mut self, min: Duration, max: Duration) -> Duration {
        let span = u64::try_from(max.saturating_sub(min).as_nanos()).unwrap_or(u64::MAX);
        let offset = match span.checked_add(1) {
            Some(range) => self.next() % range,
            None => self.next(),
        };
        min + Duration::from_nanos(offset)
    }
}

/// Always the shortest of the interval: the fixed intervals of `Mascot::fast`.
struct Fixed;

impl Dice for Fixed {
    fn pick(&mut self, min: Duration, _max: Duration) -> Duration {
        min
    }
}

/// One event of the cat (the blink or the meow): on or off, and when it changes next (None:
/// never, `Mascot::still`).
struct Event {
    every: (Duration, Duration),
    lasts: Duration,
    on: bool,
    at: Option<Instant>,
}

impl Event {
    fn new(
        now: Instant,
        every: (Duration, Duration),
        lasts: Duration,
        dice: &mut dyn Dice,
    ) -> Event {
        Event {
            every,
            lasts,
            on: false,
            at: Some(now + dice.pick(every.0, every.1)),
        }
    }

    /// At most one change: the end of the event whose time came (the next interval drawn from
    /// `now`), or its start. A late `now` recovers nothing: the next time is always after it.
    fn tick(&mut self, now: Instant, dice: &mut dyn Dice) -> bool {
        match self.at {
            Some(at) if now >= at => {
                self.on = !self.on;
                let wait = if self.on {
                    self.lasts
                } else {
                    dice.pick(self.every.0, self.every.1)
                };
                self.at = Some(now + wait);
                true
            }
            _ => false,
        }
    }
}

/// The state machine of the cat: when she blinks (`BLINK_EVERY`, `BLINK_FOR`) and meows
/// (`MEOW_EVERY`, `MEOW_FOR`), each on its own (she may blink with the bubble open). The next
/// interval is drawn when the event before it ends (and, at the creation, from that instant):
/// the blink first, then the meow.
pub struct Mascot {
    blink: Event,
    meow: Event,
    dice: Box<dyn Dice>,
}

impl Mascot {
    /// The cat of production, from `now`, with `dice`.
    pub fn new(now: Instant, dice: impl Dice + 'static) -> Mascot {
        let mut dice: Box<dyn Dice> = Box::new(dice);
        let blink = Event::new(now, BLINK_EVERY, BLINK_FOR, dice.as_mut());
        let meow = Event::new(now, MEOW_EVERY, MEOW_FOR, dice.as_mut());
        Mascot { blink, meow, dice }
    }

    /// Still: she never blinks nor meows (`next_wake` is None). The test hook
    /// NICK_TEST_MASCOT=still.
    pub fn still(now: Instant) -> Mascot {
        let mut mascot = Mascot::fast(now);
        mascot.blink.at = None;
        mascot.meow.at = None;
        mascot
    }

    /// Short fixed intervals, with no dice: the test hook NICK_TEST_MASCOT=fast.
    pub fn fast(now: Instant) -> Mascot {
        let mut dice: Box<dyn Dice> = Box::new(Fixed);
        let every = |d: Duration| (d, d);
        let blink = Event::new(now, every(FAST_BLINK_EVERY), FAST_BLINK_FOR, dice.as_mut());
        let meow = Event::new(now, every(FAST_MEOW_EVERY), FAST_MEOW_FOR, dice.as_mut());
        Mascot { blink, meow, dice }
    }

    /// Moves the clock to `now`: at most one change for each event. True when what is shown
    /// (the pose or the bubble) changed.
    pub fn tick(&mut self, now: Instant) -> bool {
        let blinked = self.blink.tick(now, self.dice.as_mut());
        let meowed = self.meow.tick(now, self.dice.as_mut());
        blinked || meowed
    }

    /// The next instant something changes (None: never).
    pub fn next_wake(&self) -> Option<Instant> {
        [self.blink.at, self.meow.at].into_iter().flatten().min()
    }

    /// What is shown now.
    pub fn frame(&self) -> Frame {
        Frame {
            pose: if self.blink.on {
                Pose::Blink
            } else {
                Pose::Rest
            },
            meowing: self.meow.on,
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;
    use std::rc::Rc;

    use super::*;

    const SECOND: Duration = Duration::from_secs(1);

    /// The approved art, copied by hand from .nick/docs/NS-51/nick_sprite.py (PAWS and BLINK,
    /// already resolved), independent of the constants: changing the art takes changing both.
    pub(crate) const APPROVED_REST: [&str; 14] = [
        ".D.............D.",
        ".DD...........DD.",
        ".DBD.........DBD.",
        ".DBBD.......DBBD.",
        ".DBBDCCCCCCCDBBD.",
        "DDDDCCCCWCCCCDDDD",
        "DDDDDCCWWWCCDDDDD",
        "DDEEDDDWWWDDDEEDD",
        "DDEKDDDWWWDDDKEDD",
        ".DDDDDWWWWWDDDDD.",
        "..DDDWWWPWWWDDD..",
        "...DWWWWWWWWWD...",
        "...WWWWWWWWWWW...",
        "...WWW.....WWW...",
    ];
    pub(crate) const APPROVED_BLINK: [&str; 14] = [
        ".D.............D.",
        ".DD...........DD.",
        ".DBD.........DBD.",
        ".DBBD.......DBBD.",
        ".DBBDCCCCCCCDBBD.",
        "DDDDCCCCWCCCCDDDD",
        "DDDDDCCWWWCCDDDDD",
        "DDDDDDDWWWDDDDDDD",
        "DDKKDDDWWWDDDKKDD",
        ".DDDDDWWWWWDDDDD.",
        "..DDDWWWPWWWDDD..",
        "...DWWWWWWWWWD...",
        "...WWWWWWWWWWW...",
        "...WWW.....WWW...",
    ];
    pub(crate) const APPROVED_PALETTE: [(char, (u8, u8, u8)); 7] = [
        ('D', (52, 36, 30)),
        ('B', (110, 78, 62)),
        ('C', (196, 170, 140)),
        ('W', (242, 240, 234)),
        ('E', (110, 170, 225)),
        ('K', (20, 20, 24)),
        ('P', (232, 160, 168)),
    ];

    /// A dice with a fixed script of durations, that records the limits it was called with.
    #[derive(Clone, Default)]
    struct Scripted {
        script: Rc<RefCell<VecDeque<Duration>>>,
        calls: Rc<RefCell<Vec<(Duration, Duration)>>>,
    }

    impl Scripted {
        fn new(script: &[u64]) -> Scripted {
            let dice = Scripted::default();
            dice.script
                .borrow_mut()
                .extend(script.iter().map(|s| Duration::from_secs(*s)));
            dice
        }
    }

    impl Dice for Scripted {
        fn pick(&mut self, min: Duration, max: Duration) -> Duration {
            self.calls.borrow_mut().push((min, max));
            self.script
                .borrow_mut()
                .pop_front()
                .expect("the script of the dice ran out")
        }
    }

    /// A cat showing `pose`, with the bubble open when `meowing` (for the tests of the drawing).
    pub(crate) fn showing(pose: Pose, meowing: bool) -> Mascot {
        let t0 = Instant::now();
        if pose == Pose::Rest && !meowing {
            return Mascot::still(t0);
        }
        let far = 1000;
        let blink = if pose == Pose::Blink { 1 } else { far };
        let meow = if meowing { 1 } else { far };
        let mut mascot = Mascot::new(t0, Scripted::new(&[blink, meow]));
        assert!(mascot.tick(t0 + SECOND));
        assert_eq!(mascot.frame(), Frame { pose, meowing });
        mascot
    }

    fn at(t0: Instant, ms: u64) -> Instant {
        t0 + Duration::from_millis(ms)
    }

    #[test]
    fn nearest_256_of_the_palette_and_of_the_ends() {
        let cases = [
            ((52, 36, 30), 235),
            ((110, 78, 62), 240),
            ((196, 170, 140), 180),
            ((242, 240, 234), 255),
            ((110, 170, 225), 74),
            ((20, 20, 24), 233),
            ((232, 160, 168), 181),
            ((0, 0, 0), 16),
            ((255, 255, 255), 231),
            ((128, 128, 128), 244),
        ];
        for ((r, g, b), index) in cases {
            assert_eq!(nearest_256(r, g, b), index, "({r}, {g}, {b})");
        }
    }

    #[test]
    fn the_color_of_a_letter_is_the_exact_rgb_with_truecolor_and_indexed_without() {
        for (letter, (r, g, b)) in APPROVED_PALETTE {
            assert_eq!(
                color_of(letter, true),
                Some(Color::Rgb(r, g, b)),
                "{letter}"
            );
            assert_eq!(
                color_of(letter, false),
                Some(Color::Indexed(nearest_256(r, g, b))),
                "{letter}"
            );
        }
        assert_eq!(color_of('.', true), None);
        assert_eq!(color_of('.', false), None);
    }

    /// Review NS-51, problem 1: the palette resolved once gives the same colors as `color_of`.
    #[test]
    fn the_palette_resolved_once_has_the_colors_of_color_of() {
        for truecolor in [true, false] {
            let palette = Palette::new(truecolor);
            for (letter, _) in APPROVED_PALETTE {
                assert_eq!(
                    palette.color(letter),
                    color_of(letter, truecolor),
                    "{letter} truecolor={truecolor}"
                );
            }
            assert_eq!(palette.color('.'), None);
        }
        assert_ne!(Palette::new(true), Palette::new(false));
    }

    #[test]
    fn the_art_is_pinned_to_the_approved_one() {
        assert_eq!(REST, APPROVED_REST, "REST is not the approved PAWS");
        assert_eq!(BLINK, APPROVED_BLINK, "BLINK is not the approved BLINK");
        assert_eq!(PALETTE, APPROVED_PALETTE, "PALETTE is not the approved one");
    }

    #[test]
    fn each_pose_has_14_rows_of_17_pixels_of_the_palette() {
        for pose in [Pose::Rest, Pose::Blink] {
            let pixels = pose.pixels();
            assert_eq!(pixels.len(), usize::from(2 * MASCOT_ROWS));
            for row in pixels {
                assert_eq!(row.chars().count(), usize::from(MASCOT_COLS), "{row}");
                assert!(
                    row.chars().all(|c| c == '.' || rgb_of(c).is_some()),
                    "{row}"
                );
            }
        }
    }

    #[test]
    fn two_pixels_make_one_cell() {
        let cells = cells(Pose::Rest);
        // Row 0 of the cells: ".D..." on top, ".DD.." below.
        assert_eq!(cells[0][0], CellArt::default(), "neither: not touched");
        assert_eq!(
            cells[0][1],
            CellArt {
                symbol: Some('▀'),
                top: Some('D'),
                bottom: Some('D')
            },
            "both"
        );
        assert_eq!(
            cells[0][2],
            CellArt {
                symbol: Some('▄'),
                top: None,
                bottom: Some('D')
            },
            "only the bottom one"
        );
        // Row 6 of the cells: the pixel rows 12 ("...WWWWWWWWWWW...") and 13
        // ("...WWW.....WWW..."): white on both at the column 3, only on top at the 7.
        assert_eq!(
            cells[6][3],
            CellArt {
                symbol: Some('▀'),
                top: Some('W'),
                bottom: Some('W')
            }
        );
        assert_eq!(
            cells[6][7],
            CellArt {
                symbol: Some('▀'),
                top: Some('W'),
                bottom: None
            },
            "only the top one"
        );
    }

    #[test]
    fn the_blink_changes_only_the_rows_of_the_eyes() {
        let (rest, blink) = (cells(Pose::Rest), cells(Pose::Blink));
        for row in 0..usize::from(MASCOT_ROWS) {
            // The pixel rows 7 and 8 fall in the cells 3 (6-7) and 4 (8-9).
            if row == 3 || row == 4 {
                assert_ne!(rest[row], blink[row], "row {row}");
            } else {
                assert_eq!(rest[row], blink[row], "row {row}");
            }
        }
    }

    #[test]
    fn a_new_cat_is_still_and_wakes_at_the_first_event_drawn() {
        let t0 = Instant::now();
        let dice = Scripted::new(&[10, 200]);
        let mascot = Mascot::new(t0, dice.clone());
        assert_eq!(
            mascot.frame(),
            Frame {
                pose: Pose::Rest,
                meowing: false
            }
        );
        assert_eq!(mascot.next_wake(), Some(t0 + 10 * SECOND));
        let dice = Scripted::new(&[10, 5]);
        let mascot = Mascot::new(t0, dice);
        assert_eq!(mascot.next_wake(), Some(t0 + 5 * SECOND));
    }

    #[test]
    fn she_blinks_at_its_time_for_blink_for_and_draws_the_next_from_the_end() {
        let t0 = Instant::now();
        let dice = Scripted::new(&[10, 200, 7]);
        let mut mascot = Mascot::new(t0, dice.clone());
        assert!(!mascot.tick(at(t0, 9_999)), "before its time");
        assert_eq!(mascot.frame().pose, Pose::Rest);
        assert_eq!(mascot.next_wake(), Some(t0 + 10 * SECOND));

        let start = t0 + 10 * SECOND;
        assert!(mascot.tick(start));
        assert_eq!(mascot.frame().pose, Pose::Blink);
        assert_eq!(mascot.next_wake(), Some(start + BLINK_FOR));
        assert!(!mascot.tick(start + BLINK_FOR - Duration::from_millis(1)));
        assert_eq!(mascot.frame().pose, Pose::Blink);

        let end = start + BLINK_FOR;
        assert!(mascot.tick(end));
        assert_eq!(mascot.frame().pose, Pose::Rest);
        assert_eq!(mascot.next_wake(), Some(end + 7 * SECOND));
        assert!(!mascot.frame().meowing);
    }

    #[test]
    fn she_meows_at_its_time_for_meow_for() {
        let t0 = Instant::now();
        // The blink far away (1000 s), the meow at 200 s, then 300 s.
        let dice = Scripted::new(&[1000, 200, 300]);
        let mut mascot = Mascot::new(t0, dice);
        let start = t0 + 200 * SECOND;
        assert!(!mascot.tick(start - Duration::from_millis(1)));
        assert!(mascot.tick(start));
        assert!(mascot.frame().meowing);
        assert_eq!(mascot.frame().pose, Pose::Rest);
        assert_eq!(mascot.next_wake(), Some(start + MEOW_FOR));
        let end = start + MEOW_FOR;
        assert!(mascot.tick(end));
        assert!(!mascot.frame().meowing);
        assert_eq!(mascot.next_wake(), Some(end + 300 * SECOND));
    }

    #[test]
    fn she_blinks_with_the_bubble_open() {
        let t0 = Instant::now();
        let dice = Scripted::new(&[201, 200, 10, 300]);
        let mut mascot = Mascot::new(t0, dice);
        assert!(mascot.tick(t0 + 200 * SECOND));
        assert!(mascot.tick(t0 + 201 * SECOND));
        assert_eq!(
            mascot.frame(),
            Frame {
                pose: Pose::Blink,
                meowing: true
            }
        );
    }

    #[test]
    fn a_late_tick_makes_at_most_one_change_for_each_event() {
        let t0 = Instant::now();
        let dice = Scripted::new(&[10, 200, 7, 300]);
        let mut mascot = Mascot::new(t0, dice.clone());
        let late = t0 + 3600 * SECOND;
        assert!(mascot.tick(late));
        // One blink and one meow start, both from `late`; nothing ends in the same tick.
        assert_eq!(
            mascot.frame(),
            Frame {
                pose: Pose::Blink,
                meowing: true
            }
        );
        assert_eq!(mascot.next_wake(), Some(late + BLINK_FOR));
        assert_eq!(
            dice.calls.borrow().len(),
            2,
            "no interval drawn to catch up"
        );
        // Another late tick: both end, each drawing its next interval from it.
        let later = late + 3600 * SECOND;
        assert!(mascot.tick(later));
        assert_eq!(
            mascot.frame(),
            Frame {
                pose: Pose::Rest,
                meowing: false
            }
        );
        assert_eq!(mascot.next_wake(), Some(later + 7 * SECOND));
    }

    #[test]
    fn the_dice_is_asked_with_the_limits_of_each_event() {
        let t0 = Instant::now();
        let dice = Scripted::new(&[10, 200, 7, 9, 300]);
        let mut mascot = Mascot::new(t0, dice.clone());
        // The blink at 10 s ends at 11 s (draws 7 s: the next at 18 s); at 200 s the blink and
        // the meow start, at 210 s both end, each drawing its next interval.
        mascot.tick(t0 + 10 * SECOND);
        mascot.tick(t0 + 11 * SECOND);
        mascot.tick(t0 + 200 * SECOND);
        mascot.tick(t0 + 210 * SECOND);
        assert_eq!(
            *dice.calls.borrow(),
            vec![
                BLINK_EVERY,
                MEOW_EVERY,
                BLINK_EVERY,
                BLINK_EVERY,
                MEOW_EVERY
            ]
        );
    }

    #[test]
    fn the_system_dice_stays_within_the_limits_and_varies() {
        let mut dice = SystemDice::new();
        let (min, max) = BLINK_EVERY;
        let picks: Vec<Duration> = (0..1000).map(|_| dice.pick(min, max)).collect();
        assert!(picks.iter().all(|p| *p >= min && *p <= max));
        assert!(
            picks.iter().any(|p| *p != picks[0]),
            "always the same value"
        );
        assert_eq!(
            dice.pick(SECOND, SECOND),
            SECOND,
            "an interval of one value"
        );
    }

    #[test]
    fn the_still_cat_never_changes() {
        let t0 = Instant::now();
        let mut mascot = Mascot::still(t0);
        assert_eq!(mascot.next_wake(), None);
        for s in [0, 1, 60, 3600, 86_400] {
            assert!(!mascot.tick(t0 + s * SECOND));
            assert_eq!(
                mascot.frame(),
                Frame {
                    pose: Pose::Rest,
                    meowing: false
                }
            );
        }
    }

    #[test]
    fn the_fast_cat_blinks_and_meows_on_short_fixed_intervals() {
        let t0 = Instant::now();
        let mut mascot = Mascot::fast(t0);
        assert_eq!(mascot.next_wake(), Some(t0 + FAST_BLINK_EVERY));
        assert!(mascot.tick(t0 + FAST_BLINK_EVERY));
        assert_eq!(mascot.frame().pose, Pose::Blink);
        let mut meowed = false;
        let mut now = t0 + FAST_BLINK_EVERY;
        while now < t0 + Duration::from_secs(1) {
            now = mascot.next_wake().expect("it keeps waking");
            mascot.tick(now);
            meowed |= mascot.frame().meowing;
        }
        assert!(meowed);
    }
}
