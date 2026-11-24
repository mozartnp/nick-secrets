//! The marquee of the selected node on the tree (NS-55): a row whose text does not fit in the
//! column ends in `…` when not selected (`ellipsize`), and scrolls one column at a time when
//! selected (`window`): still at the start, then to the end, still at the end, and back to the
//! start at once, in a cycle. Pure: no I/O, no database, no screen, no `Instant::now()`. The
//! position is a function of the time since the node was selected (`offset`, `next_change`),
//! the state that follows the selection is `Marquee`, whose clock comes by parameter as in
//! mascot.rs, and the cut goes grapheme by grapheme with the display width of ratatui (no
//! dependency of its own). ABOUT.md, "Rust no projeto: letreiro do nó selecionado na árvore
//! (NS-55)".

use std::time::{Duration, Instant};

use ratatui::style::Style;
use ratatui::text::Span;

use crate::tree::NodeId;

/// How long the marquee stays at the start, takes for each column, and stays at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Timing {
    pub hold_start: Duration,
    pub step: Duration,
    pub hold_end: Duration,
}

/// The timing of production (D5).
pub const PRODUCTION: Timing = Timing {
    hold_start: Duration::from_millis(1500),
    step: Duration::from_millis(150),
    hold_end: Duration::from_millis(1500),
};

/// The short fixed timing of the test hook NICK_TEST_MARQUEE=fast (D9).
pub const FAST: Timing = Timing {
    hold_start: Duration::from_millis(60),
    step: Duration::from_millis(20),
    hold_end: Duration::from_millis(60),
};

/// The column of the text where the window starts `elapsed` after the start, for a text
/// `overflow` columns wider than the room (D5): 0 while still at the start, one more each
/// `step`, then `overflow` (the end shown) until the cycle starts again. Always 0 without
/// overflow.
pub fn offset(timing: &Timing, elapsed: Duration, overflow: usize) -> usize {
    let Some(cycle) = Cycle::new(timing, elapsed, overflow) else {
        return 0;
    };
    if cycle.at < timing.hold_start {
        0
    } else if cycle.at < cycle.moved {
        cycle.columns(cycle.at - timing.hold_start)
    } else {
        overflow
    }
}

/// The time since the start when the offset changes next (the value, not the phase): None
/// without overflow, when it never changes.
pub fn next_change(timing: &Timing, elapsed: Duration, overflow: usize) -> Option<Duration> {
    let cycle = Cycle::new(timing, elapsed, overflow)?;
    let start = elapsed - cycle.at;
    let next = if cycle.at < cycle.moved {
        // Still at the start, or moving: the next column (the first one from the start).
        let column = cycle.columns(cycle.at.saturating_sub(timing.hold_start)) + 1;
        timing.hold_start + timing.step * u32::try_from(column).unwrap_or(u32::MAX)
    } else {
        // At the end: back to the start when the cycle ends.
        cycle.length
    };
    Some(start + next)
}

/// Where `elapsed` falls in the cycle of a text `overflow` columns too wide (D5).
struct Cycle {
    /// The time since the start of this cycle.
    at: Duration,
    /// When the last column is reached, from the start of a cycle.
    moved: Duration,
    /// The length of a cycle.
    length: Duration,
    step: Duration,
}

impl Cycle {
    /// None without overflow (nothing moves) or with a step of zero.
    fn new(timing: &Timing, elapsed: Duration, overflow: usize) -> Option<Cycle> {
        if overflow == 0 || timing.step.is_zero() {
            return None;
        }
        let moving = timing.step * u32::try_from(overflow).unwrap_or(u32::MAX);
        let moved = timing.hold_start + moving;
        let length = moved + timing.hold_end;
        let at = elapsed.as_nanos() % length.as_nanos();
        Some(Cycle {
            // Less than `length`, a `Duration`: it fits in a u64 of nanoseconds.
            at: Duration::from_nanos(u64::try_from(at).unwrap_or(u64::MAX)),
            moved,
            length,
            step: timing.step,
        })
    }

    /// The whole steps in `moving` (the time since the end of the hold at the start).
    fn columns(&self, moving: Duration) -> usize {
        usize::try_from(moving.as_nanos() / self.step.as_nanos()).unwrap_or(usize::MAX)
    }
}

/// The marquee of the selected node: which node scrolls, since when, its overflow and its
/// offset at the last `tick`. Only the `tick` moves it, with the clock by parameter.
#[derive(Debug, Clone)]
pub struct Marquee {
    timing: Timing,
    still: bool,
    node: Option<NodeId>,
    since: Instant,
    now: Instant,
    overflow: usize,
    offset: usize,
}

impl Marquee {
    fn with(now: Instant, timing: Timing, still: bool) -> Marquee {
        Marquee {
            timing,
            still,
            node: None,
            since: now,
            now,
            overflow: 0,
            offset: 0,
        }
    }

    /// The marquee of production, from `now`.
    pub fn new(now: Instant) -> Marquee {
        Marquee::with(now, PRODUCTION, false)
    }

    /// Never leaves the offset 0 and never wakes the loop: the test hook NICK_TEST_MARQUEE=still.
    pub fn still(now: Instant) -> Marquee {
        Marquee::with(now, PRODUCTION, true)
    }

    /// The short fixed timing (`FAST`): the test hook NICK_TEST_MARQUEE=fast.
    pub fn fast(now: Instant) -> Marquee {
        Marquee::with(now, FAST, false)
    }

    /// The timing of this marquee.
    pub fn timing(&self) -> Timing {
        self.timing
    }

    /// Follows the selected `node` and its `overflow` at `now` (D6). True when the offset
    /// changed (a redraw).
    pub fn tick(&mut self, now: Instant, node: NodeId, overflow: usize) -> bool {
        if self.node != Some(node) {
            // Another node (going back to one seen before too): from the start, from `now`.
            self.node = Some(node);
            self.since = now;
        }
        self.now = now;
        self.overflow = overflow;
        let offset = if self.still {
            0
        } else {
            offset(&self.timing, self.elapsed(), overflow)
        };
        let changed = offset != self.offset;
        self.offset = offset;
        changed
    }

    /// The time from the start of the node that scrolls to the last `tick`.
    fn elapsed(&self) -> Duration {
        self.now.saturating_duration_since(self.since)
    }

    /// The next instant the offset changes (None: never).
    pub fn next_wake(&self) -> Option<Instant> {
        if self.still || self.node.is_none() {
            return None;
        }
        next_change(&self.timing, self.elapsed(), self.overflow).map(|at| self.since + at)
    }

    /// The offset to draw for `node` with `overflow`: 0 for a node other than the one that
    /// scrolls, never more than `overflow`.
    pub fn offset_for(&self, node: NodeId, overflow: usize) -> usize {
        if self.node == Some(node) {
            self.offset.min(overflow)
        } else {
            0
        }
    }
}

/// The display width of `spans`, grapheme by grapheme (the width the buffer draws).
pub fn width(spans: &[Span<'static>]) -> usize {
    spans
        .iter()
        .flat_map(graphemes)
        .map(|(symbol, _)| columns(symbol))
        .sum()
}

/// `spans` cut to fit in `width` columns: as they are when they fit, else the graphemes that fit
/// in `width - 1` columns and then `…` (unstyled: it takes the style of the row), each span with
/// its own style (D3). A wide grapheme that does not fit whole before the `…` is left out.
pub fn ellipsize(spans: &[Span<'static>], width: usize) -> Vec<Span<'static>> {
    if self::width(spans) <= width {
        return spans.to_vec();
    }
    let Some(room) = width.checked_sub(1) else {
        return Vec::new();
    };
    let mut out = Pieces::default();
    let mut used = 0;
    'spans: for span in spans {
        for (symbol, style) in graphemes(span) {
            let size = columns(symbol);
            if used + size > room {
                break 'spans;
            }
            used += size;
            out.push(symbol, style);
        }
        out.end_span();
    }
    out.end_span();
    let mut out = out.spans;
    out.push(Span::raw("…"));
    out
}

/// The window of `width` columns of `spans` from the column `offset` (D4), each span with its
/// own style and no `…`. A wide grapheme cut by an edge of the window becomes a space on its
/// half that shows, so the window always has `width` columns.
pub fn window(spans: &[Span<'static>], offset: usize, width: usize) -> Vec<Span<'static>> {
    let end = offset + width;
    let mut out = Pieces::default();
    let mut column = 0;
    // A grapheme of width 0 goes with the one before it (shown or not).
    let mut shown = offset == 0 && width > 0;
    for span in spans {
        for (symbol, style) in graphemes(span) {
            let size = columns(symbol);
            let (start, stop) = (column, column + size);
            column = stop;
            if size == 0 {
                if shown {
                    out.push(symbol, style);
                }
                continue;
            }
            shown = start >= offset && stop <= end;
            if shown {
                out.push(symbol, style);
            } else {
                // The part of a wide grapheme cut by an edge: spaces, so the window keeps its
                // width and the text does not jump.
                let cut = stop.min(end).saturating_sub(start.max(offset));
                out.push(&" ".repeat(cut), style);
            }
        }
        out.end_span();
    }
    out.end_span();
    out.spans
}

/// The display width of one grapheme, as ratatui draws it.
fn columns(symbol: &str) -> usize {
    Span::raw(symbol).width()
}

/// The spans of a cut, built grapheme by grapheme: one span for each span of the input that
/// shows something, with its style.
#[derive(Default)]
struct Pieces {
    spans: Vec<Span<'static>>,
    text: String,
    style: Style,
}

impl Pieces {
    fn push(&mut self, symbol: &str, style: Style) {
        self.text.push_str(symbol);
        self.style = style;
    }

    /// Closes the span of the input being read.
    fn end_span(&mut self) {
        if !self.text.is_empty() {
            let text = std::mem::take(&mut self.text);
            self.spans.push(Span::styled(text, self.style));
        }
    }
}

/// The graphemes of `span` with its own style (and not patched over another one).
fn graphemes<'a>(span: &'a Span<'static>) -> impl Iterator<Item = (&'a str, Style)> {
    span.styled_graphemes(Style::default())
        .map(|g| (g.symbol, g.style))
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier, Stylize};

    use super::*;

    const MS: Duration = Duration::from_millis(1);

    /// Round numbers: 100 ms still at the start, 10 ms a column, 50 ms still at the end.
    const ROUND: Timing = Timing {
        hold_start: Duration::from_millis(100),
        step: Duration::from_millis(10),
        hold_end: Duration::from_millis(50),
    };

    fn at(ms: u64) -> Duration {
        MS * u32::try_from(ms).unwrap()
    }

    #[test]
    fn the_timings_of_production_and_of_the_fast_hook() {
        assert_eq!(
            PRODUCTION,
            Timing {
                hold_start: at(1500),
                step: at(150),
                hold_end: at(1500),
            }
        );
        assert_eq!(
            FAST,
            Timing {
                hold_start: at(60),
                step: at(20),
                hold_end: at(60),
            }
        );
    }

    #[test]
    fn without_overflow_the_offset_is_always_0_and_never_changes() {
        for ms in [0, 50, 100, 110, 1000, 123_456] {
            assert_eq!(offset(&ROUND, at(ms), 0), 0, "at {ms} ms");
            assert_eq!(next_change(&ROUND, at(ms), 0), None, "at {ms} ms");
        }
    }

    #[test]
    fn the_offset_at_exact_instants_of_the_cycle() {
        // The cycle is 100 + 3·10 + 50 = 180 ms.
        let expected = [
            (0, 0),
            (99, 0),
            (100, 0),
            (109, 0),
            (110, 1),
            (119, 1),
            (120, 2),
            (129, 2),
            (130, 3),
            (179, 3),
            (180, 0),
            (190, 0),
            (289, 0),
            (290, 1),
            (310, 3),
            (359, 3),
            (360, 0),
        ];
        for (ms, want) in expected {
            assert_eq!(offset(&ROUND, at(ms), 3), want, "at {ms} ms");
        }
    }

    #[test]
    fn next_change_is_when_the_value_changes_and_not_the_phase() {
        let expected = [
            (0, 110),
            (99, 110),
            (100, 110),
            (105, 110),
            (110, 120),
            (115, 120),
            (120, 130),
            (130, 180),
            (179, 180),
            (180, 290),
            (290, 300),
        ];
        for (ms, want) in expected {
            assert_eq!(next_change(&ROUND, at(ms), 3), Some(at(want)), "at {ms} ms");
        }
        // With an overflow of 1 the only move is straight to the end.
        assert_eq!(offset(&ROUND, at(110), 1), 1);
        assert_eq!(next_change(&ROUND, at(0), 1), Some(at(110)));
        assert_eq!(next_change(&ROUND, at(110), 1), Some(at(160)));
    }

    #[test]
    fn many_cycles_later_the_offset_is_where_it_is_in_the_cycle() {
        let cycles = 1_000_003;
        let base = 180 * cycles;
        assert_eq!(offset(&ROUND, at(base), 3), 0);
        assert_eq!(offset(&ROUND, at(base + 125), 3), 2);
        assert_eq!(offset(&ROUND, at(base + 170), 3), 3);
        assert_eq!(next_change(&ROUND, at(base + 125), 3), Some(at(base + 130)));
        assert_eq!(next_change(&ROUND, at(base + 170), 3), Some(at(base + 180)));
    }

    fn ticket(id: i64) -> NodeId {
        NodeId::Ticket(id)
    }

    fn round(t0: Instant) -> Marquee {
        Marquee::with(t0, ROUND, false)
    }

    #[test]
    fn a_new_node_starts_still_from_the_tick_that_saw_it() {
        let t0 = Instant::now();
        let mut m = round(t0);
        // Selected 500 ms after the marquee was made: it starts from there, not from t0.
        let t1 = t0 + at(500);
        m.tick(t1, ticket(1), 3);
        assert_eq!(m.offset_for(ticket(1), 3), 0);
        assert!(!m.tick(t1 + at(109), ticket(1), 3));
        assert_eq!(m.offset_for(ticket(1), 3), 0);
        assert!(m.tick(t1 + at(110), ticket(1), 3), "the offset moved");
        assert_eq!(m.offset_for(ticket(1), 3), 1);
    }

    #[test]
    fn the_same_node_moves_by_d5_and_tick_is_true_only_on_a_change() {
        let t0 = Instant::now();
        let mut m = round(t0);
        m.tick(t0, ticket(1), 3);
        let mut seen = Vec::new();
        for ms in (0..=400).step_by(5) {
            if m.tick(t0 + at(ms), ticket(1), 3) {
                seen.push((ms, m.offset_for(ticket(1), 3)));
            }
        }
        assert_eq!(
            seen,
            [
                (110, 1),
                (120, 2),
                (130, 3),
                (180, 0),
                (290, 1),
                (300, 2),
                (310, 3),
                (360, 0),
            ]
        );
    }

    #[test]
    fn another_node_and_back_starts_again() {
        let t0 = Instant::now();
        let mut m = round(t0);
        m.tick(t0, ticket(1), 3);
        m.tick(t0 + at(125), ticket(1), 3);
        assert_eq!(m.offset_for(ticket(1), 3), 2);
        assert_eq!(m.offset_for(ticket(2), 3), 0, "another node draws from 0");
        // To another node: it starts there.
        assert!(m.tick(t0 + at(126), ticket(2), 5), "back to 0 is a change");
        assert_eq!(m.offset_for(ticket(2), 5), 0);
        assert_eq!(m.offset_for(ticket(1), 3), 0);
        m.tick(t0 + at(240), ticket(2), 5);
        assert_eq!(m.offset_for(ticket(2), 5), 1);
        // Back to the first one: from the start too.
        m.tick(t0 + at(241), ticket(1), 3);
        assert_eq!(m.offset_for(ticket(1), 3), 0);
        assert_eq!(m.next_wake(), Some(t0 + at(241 + 110)));
    }

    #[test]
    fn a_smaller_overflow_of_the_same_node_does_not_start_again_and_bounds_the_offset() {
        let t0 = Instant::now();
        let mut m = round(t0);
        m.tick(t0, ticket(1), 5);
        m.tick(t0 + at(145), ticket(1), 5);
        assert_eq!(m.offset_for(ticket(1), 5), 4);
        // Drawn with a smaller overflow before the next tick (a resize): never past it.
        assert_eq!(m.offset_for(ticket(1), 2), 2);
        // The tick with the new overflow keeps the start: 145 ms is the end of a 2 column text.
        m.tick(t0 + at(146), ticket(1), 2);
        assert_eq!(m.offset_for(ticket(1), 2), 2);
        assert_eq!(m.next_wake(), Some(t0 + at(170)), "the cycle of 170 ms");
        // Without overflow (a title edited shorter), 0 and no wake.
        m.tick(t0 + at(147), ticket(1), 0);
        assert_eq!(m.offset_for(ticket(1), 0), 0);
        assert_eq!(m.next_wake(), None);
        // Longer again: still the same start, not a new one.
        m.tick(t0 + at(148), ticket(1), 5);
        assert_eq!(m.offset_for(ticket(1), 5), 4);
    }

    #[test]
    fn next_wake_is_the_start_plus_next_change() {
        let t0 = Instant::now();
        let mut m = round(t0);
        assert_eq!(m.next_wake(), None, "no node yet");
        let t1 = t0 + at(7);
        m.tick(t1, ticket(1), 3);
        assert_eq!(m.next_wake(), Some(t1 + at(110)));
        m.tick(t1 + at(110), ticket(1), 3);
        assert_eq!(m.next_wake(), Some(t1 + at(120)));
        m.tick(t1 + at(130), ticket(1), 3);
        assert_eq!(m.next_wake(), Some(t1 + at(180)));
        m.tick(t1 + at(131), ticket(1), 0);
        assert_eq!(m.next_wake(), None, "without overflow it never wakes");
    }

    #[test]
    fn the_still_marquee_never_moves_nor_wakes() {
        let t0 = Instant::now();
        let mut m = Marquee::still(t0);
        m.tick(t0, ticket(1), 30);
        assert_eq!(m.next_wake(), None);
        for s in 0..20 {
            assert!(!m.tick(t0 + Duration::from_secs(s), ticket(1), 30));
            assert_eq!(m.offset_for(ticket(1), 30), 0);
        }
        assert_eq!(m.next_wake(), None);
    }

    #[test]
    fn the_production_and_the_fast_marquees_use_their_timings() {
        let t0 = Instant::now();
        let mut m = Marquee::new(t0);
        assert_eq!(m.timing(), PRODUCTION);
        m.tick(t0, ticket(1), 4);
        assert_eq!(m.next_wake(), Some(t0 + at(1650)));
        let mut m = Marquee::fast(t0);
        assert_eq!(m.timing(), FAST);
        m.tick(t0, ticket(1), 4);
        assert_eq!(m.next_wake(), Some(t0 + at(80)));
        m.tick(t0 + at(140), ticket(1), 4);
        assert_eq!(m.offset_for(ticket(1), 4), 4, "60 + 4·20 = 140 ms: the end");
    }

    // --- The cut (D3, D4). ---

    fn texts(spans: &[Span<'static>]) -> Vec<(String, Style)> {
        spans
            .iter()
            .map(|s| (s.content.to_string(), s.style))
            .collect()
    }

    fn plain(text: &'static str) -> Vec<Span<'static>> {
        vec![Span::raw(text)]
    }

    fn joined(spans: &[Span<'static>]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn what_fits_is_left_as_it_is() {
        let spans = vec![Span::raw("NS-1  "), Span::raw("[PO]").fg(Color::LightBlue)];
        assert_eq!(width(&spans), 10);
        assert_eq!(texts(&ellipsize(&spans, 10)), texts(&spans));
        assert_eq!(texts(&ellipsize(&spans, 30)), texts(&spans));
        assert_eq!(texts(&window(&spans, 0, 10)), texts(&spans));
    }

    #[test]
    fn ascii_that_does_not_fit() {
        let spans = plain("abcdefgh");
        assert_eq!(
            texts(&ellipsize(&spans, 5)),
            texts(&[Span::raw("abcd"), Span::raw("…")])
        );
        assert_eq!(joined(&window(&spans, 2, 5)), "cdefg");
        assert_eq!(joined(&window(&spans, 0, 5)), "abcde");
        assert_eq!(joined(&window(&spans, 3, 5)), "defgh");
    }

    #[test]
    fn an_accent_counts_one_column_and_is_never_split_from_its_letter() {
        let nfc = (plain("ação longa"), "çã");
        let combining = (
            plain("a\u{0303}c\u{0327}a\u{0303}o longa"),
            "c\u{0327}a\u{0303}",
        );
        for (spans, second_and_third) in [nfc, combining] {
            let text = joined(&spans);
            assert_eq!(width(&spans), 10, "{text:?}");
            let cut = joined(&ellipsize(&spans, 4));
            assert_eq!(width(&ellipsize(&spans, 4)), 4, "{text:?}");
            // Three letters, each with its accent, then the `…`.
            let three: String = text.chars().take(text.chars().count() - 7).collect();
            assert_eq!(cut, format!("{three}…"), "{text:?}");
            let window = window(&spans, 1, 2);
            assert_eq!(width(&window), 2, "{text:?}");
            assert_eq!(joined(&window), second_and_third, "{text:?}");
        }
        // A combining mark alone (a grapheme of width 0) goes with the one before it.
        let alone = vec![Span::raw("ab"), Span::raw("\u{0301}cdef")];
        assert_eq!(width(&alone), 6);
        assert_eq!(joined(&ellipsize(&alone, 4)), "ab\u{0301}c…");
        assert_eq!(joined(&window(&alone, 1, 2)), "b\u{0301}c");
        assert_eq!(joined(&window(&alone, 2, 2)), "cd");
    }

    #[test]
    fn a_wide_grapheme_that_does_not_fit_before_the_ellipsis_is_left_out() {
        let spans = plain("ab🐈cd");
        assert_eq!(width(&spans), 6);
        assert_eq!(joined(&ellipsize(&spans, 4)), "ab…");
        assert_eq!(joined(&ellipsize(&spans, 5)), "ab🐈…");
        let han = plain("漢字の長い題名");
        for w in 0..14 {
            assert!(width(&ellipsize(&han, w)) <= w, "width {w}");
        }
        assert_eq!(joined(&ellipsize(&han, 6)), "漢字…");
    }

    #[test]
    fn a_wide_grapheme_cut_by_an_edge_of_the_window_becomes_a_space() {
        let spans = plain("ab🐈cd漢e");
        assert_eq!(width(&spans), 9);
        // The cat on columns 2 and 3, the 漢 on 6 and 7.
        assert_eq!(joined(&window(&spans, 3, 4)), " cd ");
        assert_eq!(joined(&window(&spans, 1, 2)), "b ");
        assert_eq!(joined(&window(&spans, 2, 4)), "🐈cd");
        assert_eq!(joined(&window(&spans, 5, 4)), "d漢e");
        for offset in 0..=5 {
            for w in 0..=(9 - offset) {
                assert_eq!(width(&window(&spans, offset, w)), w, "{offset} {w}");
            }
        }
    }

    #[test]
    fn the_cut_falls_inside_a_span_and_the_others_keep_their_style() {
        let blue = Style::new().fg(Color::LightBlue);
        let red = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
        let spans = vec![
            Span::raw("NS-12  "),
            Span::styled("[Tech Leader]", blue),
            Span::raw("  "),
            Span::styled("[FORA DA JAIL]", red),
        ];
        assert_eq!(
            texts(&ellipsize(&spans, 12)),
            [
                ("NS-12  ".to_string(), Style::new()),
                ("[Tec".to_string(), blue),
                ("…".to_string(), Style::new()),
            ]
        );
        assert_eq!(
            texts(&window(&spans, 9, 16)),
            [
                ("ech Leader]".to_string(), blue),
                ("  ".to_string(), Style::new()),
                ("[FO".to_string(), red),
            ]
        );
    }

    #[test]
    fn widths_0_and_1_do_not_panic() {
        let spans = plain("abc🐈");
        assert_eq!(joined(&ellipsize(&spans, 0)), "");
        assert_eq!(texts(&ellipsize(&spans, 1)), texts(&[Span::raw("…")]));
        assert_eq!(joined(&window(&spans, 0, 0)), "");
        assert_eq!(joined(&window(&spans, 2, 1)), "c");
        assert_eq!(joined(&window(&spans, 3, 1)), " ");
        assert_eq!(joined(&window(&spans, 4, 1)), " ");
        assert!(ellipsize(&[], 0).is_empty());
        assert!(window(&[], 0, 0).is_empty());
    }
}
