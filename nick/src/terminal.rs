//! Entering and leaving the TUI (raw mode and the alternate screen), suspending it to run a
//! program on the normal terminal (bin/nick-init, and `$EDITOR` since the NS-35:
//! `run_suspended`, immune to Ctrl+C and Ctrl+\\ meanwhile), the modes of the agent screen
//! (mouse capture and bracketed paste, only while an agent is on screen), the gate that pauses
//! the input thread and the panic hook. Also the keyboard mode of the kitty protocol (flag 1,
//! disambiguate, NS-49): turned on when the terminal supports it, on the whole time the TUI is
//! (the list included), off while suspended and when leaving, so the Shift+Enter arrives.

use std::ffi::OsStr;
use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use signal_hook::consts::{SIGINT, SIGQUIT};

/// The terminal of the TUI.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// How long the idle loop (nothing to draw) waits for an event: with no deadline (None)
/// without a `wake`, else until the `wake` of the cat Nick (NS-51), zero when it passed.
pub fn idle_wait(wake: Option<Instant>, now: Instant) -> Option<Duration> {
    wake.map(|at| at.saturating_duration_since(now))
}

/// Whether the terminal has truecolor, by the convention of `COLORTERM` (`truecolor` or
/// `24bit`, any case); any other value, or none, is the 256 palette (NS-51).
pub fn is_truecolor(colorterm: Option<&OsStr>) -> bool {
    colorterm.and_then(OsStr::to_str).is_some_and(|value| {
        value.eq_ignore_ascii_case("truecolor") || value.eq_ignore_ascii_case("24bit")
    })
}

/// Turns off the keyboard mode first, only if it is on (`pop_keyboard_enhancement`), then the
/// mouse capture and the bracketed paste (always: turning off what is not on is harmless),
/// leaves the alternate screen, shows the cursor and leaves raw mode, ignoring errors (there is
/// nothing better to do with them while leaving; after a SIGHUP the terminal may be gone). Safe
/// to call more than once. The single exit point of the drop of the guard (`q`, error, signal),
/// of the panic hook and of `suspend` (init and editor).
pub fn restore() {
    pop_keyboard_enhancement();
    let mut stdout = io::stdout();
    let _ = execute!(
        stdout,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        Show
    );
    let _ = stdout.flush();
    let _ = disable_raw_mode();
}

/// The modes of the agent screen: the mouse (the wheel scrolls the history, or goes to the
/// program) and the bracketed paste (a paste arrives whole, for `keys::encode_paste`).
pub fn enter_agent_modes() -> io::Result<()> {
    execute!(io::stdout(), EnableMouseCapture, EnableBracketedPaste)
}

/// Back from the agent screen: both modes off.
pub fn leave_agent_modes() -> io::Result<()> {
    execute!(io::stdout(), DisableMouseCapture, DisableBracketedPaste)
}

/// Whether the keyboard mode of the kitty protocol is on (NS-49). Global, like `PANICKING`,
/// because the panic hook has no access to the guard.
static KEYBOARD_ENHANCED: AtomicBool = AtomicBool::new(false);

/// Turns on the keyboard mode of the kitty protocol, only the flag 1 (`ESC[>1u`, disambiguate):
/// the Shift+Enter arrives as `ESC[13;2u`, and crossterm parses it and the other keys into the
/// same `KeyEvent`s of the legacy mode. Only when the terminal supports it
/// (`TerminalGuard::enter`). Errors are ignored: without it the keys are the legacy ones.
pub fn push_keyboard_enhancement() {
    let mut stdout = io::stdout();
    push_keyboard_enhancement_to(&mut stdout);
}

/// Turns off the keyboard mode (`ESC[<1u`), only if `push_keyboard_enhancement` turned it on:
/// a terminal without support never gets it, and a second call writes nothing.
pub fn pop_keyboard_enhancement() {
    let mut stdout = io::stdout();
    pop_keyboard_enhancement_to(&mut stdout);
}

/// `push_keyboard_enhancement` on `out` (testable without a terminal).
fn push_keyboard_enhancement_to(out: &mut impl Write) {
    KEYBOARD_ENHANCED.store(true, Ordering::SeqCst);
    let _ = execute!(
        out,
        PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
    );
}

/// `pop_keyboard_enhancement` on `out` (testable without a terminal).
fn pop_keyboard_enhancement_to(out: &mut impl Write) {
    if KEYBOARD_ENHANCED.swap(false, Ordering::SeqCst) {
        let _ = execute!(out, PopKeyboardEnhancementFlags);
    }
}

/// Set by the panic hook: from then on the main loop handles no event (it would draw over the
/// restored terminal, or write to the database, while the hook ends the agents).
static PANICKING: AtomicBool = AtomicBool::new(false);

/// Whether a panic is being handled (the process is about to exit with 101).
pub fn panicking() -> bool {
    PANICKING.load(Ordering::SeqCst)
}

/// Installs the panic hook: whatever thread panics, the terminal is restored first, the
/// process groups of the agents still running are ended (agent_session.rs), then the default
/// hook prints the message (readable, on the normal screen) and the process exits with 101.
/// Without the exit, a panic in a secondary thread would leave the main loop running on a
/// restored terminal (finding of NS-29); while the hook runs, the loop stops handling events
/// (`panicking`). The database is not touched: the cleanup of the next opening ends the agent
/// (agents.rs).
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        PANICKING.store(true, Ordering::SeqCst);
        restore();
        crate::agent_session::terminate_registered();
        default_hook(info);
        std::process::exit(101);
    }));
}

/// The gate of the input thread (main.rs): while it is closed, the thread does not read the
/// terminal, so bin/nick-init, running on the suspended terminal, has the stdin to itself.
/// `close` returns only once the thread stopped reading (it polls at most 100 ms at a time).
#[derive(Default)]
pub struct InputGate {
    state: Mutex<GateState>,
    changed: Condvar,
}

#[derive(Default)]
struct GateState {
    closed: bool,
    reading: bool,
}

impl InputGate {
    /// Closes the gate and waits until the input thread is out of its read.
    pub fn close(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = true;
        while state.reading {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Opens the gate: the input thread reads again.
    pub fn open(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.closed = false;
        self.changed.notify_all();
    }

    /// The input thread, before each read: waits while the gate is closed, then marks it as
    /// reading.
    pub fn enter(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        while state.closed {
            state = self
                .changed
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state.reading = true;
    }

    /// The input thread, after each read.
    pub fn leave(&self) {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.reading = false;
        self.changed.notify_all();
    }
}

/// RAII guard of the TUI: entering puts the terminal in raw mode and on the alternate screen
/// (and in the keyboard mode, if supported), and dropping it restores all and shows the cursor,
/// also when `run` returns an error.
pub struct TerminalGuard {
    terminal: Tui,
    /// Whether the terminal supports the kitty keyboard protocol (asked once, in `enter`).
    keyboard_enhancement: bool,
}

impl TerminalGuard {
    /// Enters raw mode and the alternate screen, then asks the terminal once whether it
    /// supports the kitty keyboard protocol (`ESC[?u` + DA1, up to 2 s) and, if it does, turns
    /// the keyboard mode on. No answer or an error counts as no support, silently: nothing
    /// changes. Must run before the input thread exists (main.rs, `spawn_input`): the query
    /// reads the input and would compete with it for the bytes.
    pub fn enter() -> io::Result<TerminalGuard> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(err) = execute!(stdout, EnterAlternateScreen) {
            restore();
            return Err(err);
        }
        let keyboard_enhancement = supports_keyboard_enhancement().unwrap_or(false);
        if keyboard_enhancement {
            push_keyboard_enhancement();
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(TerminalGuard {
                terminal,
                keyboard_enhancement,
            }),
            Err(err) => {
                restore();
                Err(err)
            }
        }
    }

    pub fn terminal(&mut self) -> &mut Tui {
        &mut self.terminal
    }

    /// Runs `f` on the normal terminal: turns the keyboard mode off, leaves the alternate
    /// screen and raw mode, shows the cursor (`restore`), runs it, and then enters both again
    /// (and the keyboard mode, if supported, without asking again) with a full redraw,
    /// whatever `f` did.
    pub fn suspend<T>(&mut self, f: impl FnOnce() -> T) -> io::Result<T> {
        restore();
        let result = f();
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        if self.keyboard_enhancement {
            push_keyboard_enhancement();
        }
        self.terminal.clear()?;
        Ok(result)
    }
}

/// Runs `f` on the normal terminal with the input thread paused (NS-35, from the path of the
/// init): closes `gate` (and waits for the thread to leave its read, so the program has the
/// stdin to itself), suspends the TUI (`TerminalGuard::suspend`) and opens the gate again,
/// whatever `f` did. The error is the one of entering the TUI again.
pub fn run_suspended<T>(
    guard: &mut TerminalGuard,
    gate: &InputGate,
    f: impl FnOnce() -> T,
) -> io::Result<T> {
    gated(gate, || guard.suspend(f))
}

/// Runs `f` with `gate` closed (the input thread out of its read and held), and opens it again
/// after.
fn gated<T>(gate: &InputGate, f: impl FnOnce() -> T) -> T {
    gate.close();
    let result = f();
    gate.open();
    result
}

/// While alive, SIGINT and SIGQUIT only set a flag that nobody reads (so they do nothing to
/// the nick). Dropping it removes the handlers: back in raw mode, Ctrl+C is a key again. Out of
/// raw mode, the terminal sends Ctrl+C/Ctrl+\\ to the whole foreground group, the nick included,
/// and the default action would kill it without restoring the TUI (review NS-31, P1). Like
/// system(3) does, the parent ignores them while the child runs; the child still gets them,
/// since a handler (unlike SIG_IGN) goes back to the default on exec. Moved from init.rs in the
/// NS-35 (the editor uses it too).
pub(crate) struct IgnoreTerminalSignals {
    ids: Vec<signal_hook::SigId>,
}

impl IgnoreTerminalSignals {
    pub(crate) fn new() -> IgnoreTerminalSignals {
        let flag = Arc::new(AtomicBool::new(false));
        let ids = [SIGINT, SIGQUIT]
            .into_iter()
            .filter_map(|signal| signal_hook::flag::register(signal, Arc::clone(&flag)).ok())
            .collect();
        IgnoreTerminalSignals { ids }
    }
}

impl Drop for IgnoreTerminalSignals {
    fn drop(&mut self) {
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        restore();
        // ratatui shows the cursor again when the Terminal is dropped if it hid it; telling
        // it now keeps the restore sequence the last thing written.
        let _ = self.terminal.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    /// NS-51: the idle loop blocks with no deadline without a wake of the cat, and else waits
    /// until it (zero when it already passed).
    #[test]
    fn the_idle_wait_goes_until_the_wake_of_the_cat() {
        let now = std::time::Instant::now();
        assert_eq!(idle_wait(None, now), None);
        assert_eq!(
            idle_wait(Some(now + Duration::from_millis(150)), now),
            Some(Duration::from_millis(150))
        );
        assert_eq!(idle_wait(Some(now), now), Some(Duration::ZERO));
        assert_eq!(
            idle_wait(Some(now), now + Duration::from_secs(5)),
            Some(Duration::ZERO),
            "in the past"
        );
    }

    /// NS-51: truecolor by the convention of COLORTERM (`truecolor` or `24bit`, any case).
    #[test]
    fn truecolor_comes_from_colorterm() {
        use std::ffi::OsStr;
        for yes in ["truecolor", "24bit", "TrueColor", "24BIT"] {
            assert!(is_truecolor(Some(OsStr::new(yes))), "{yes}");
        }
        for no in ["", "256", "yes", "truecolour", " truecolor"] {
            assert!(!is_truecolor(Some(OsStr::new(no))), "{no:?}");
        }
        assert!(!is_truecolor(None));
    }

    #[test]
    fn close_waits_for_the_reader_and_holds_it_until_open() {
        let gate = Arc::new(InputGate::default());
        let left = Arc::new(AtomicBool::new(false));
        gate.enter();
        let (g, l) = (Arc::clone(&gate), Arc::clone(&left));
        let closer = std::thread::spawn(move || {
            g.close();
            l.load(Ordering::SeqCst)
        });
        std::thread::sleep(Duration::from_millis(100));
        left.store(true, Ordering::SeqCst);
        gate.leave();
        assert!(closer.join().unwrap(), "close returned only after leave");
        // Closed: the reader waits at the gate until it opens.
        let entered = Arc::new(AtomicBool::new(false));
        let (g, e) = (Arc::clone(&gate), Arc::clone(&entered));
        let reader = std::thread::spawn(move || {
            g.enter();
            e.store(true, Ordering::SeqCst);
            g.leave();
        });
        std::thread::sleep(Duration::from_millis(100));
        assert!(!entered.load(Ordering::SeqCst), "held while closed");
        gate.open();
        reader.join().unwrap();
        assert!(entered.load(Ordering::SeqCst));
    }

    /// The gate of `run_suspended` (`gated`), without a terminal: the suspension itself needs a
    /// real terminal, covered by the PTY tests of the init and of the editor.
    #[test]
    fn the_gate_is_closed_while_the_suspended_part_runs_and_open_after() {
        let gate = Arc::new(InputGate::default());
        let entered = Arc::new(AtomicBool::new(false));
        let (g, e) = (Arc::clone(&gate), Arc::clone(&entered));
        // `f` checks that a reader is held while it runs.
        let held = gated(&gate, || {
            let reader = std::thread::spawn(move || {
                g.enter();
                e.store(true, Ordering::SeqCst);
                g.leave();
            });
            std::thread::sleep(Duration::from_millis(100));
            let held = !entered.load(Ordering::SeqCst);
            (held, reader)
        });
        let (held, reader) = held;
        assert!(held, "the reader waits while the part runs");
        reader.join().unwrap();
        assert!(entered.load(Ordering::SeqCst), "the gate is open after");
    }

    /// Every case in one test: `KEYBOARD_ENHANCED` is global to the test process. It ends
    /// `false`.
    #[test]
    fn the_keyboard_mode_is_popped_only_when_pushed() {
        let written = |f: &dyn Fn(&mut Vec<u8>)| {
            let mut out = Vec::new();
            f(&mut out);
            String::from_utf8(out).unwrap()
        };
        let push = |out: &mut Vec<u8>| push_keyboard_enhancement_to(out);
        let pop = |out: &mut Vec<u8>| pop_keyboard_enhancement_to(out);
        assert_eq!(written(&pop), "", "no pop without a push");
        assert_eq!(written(&push), "\x1b[>1u");
        assert_eq!(written(&pop), "\x1b[<1u");
        assert_eq!(written(&pop), "", "a second pop writes nothing");
        let cycle = |out: &mut Vec<u8>| {
            push_keyboard_enhancement_to(out);
            pop_keyboard_enhancement_to(out);
            push_keyboard_enhancement_to(out);
            pop_keyboard_enhancement_to(out);
            pop_keyboard_enhancement_to(out);
        };
        assert_eq!(written(&cycle), "\x1b[>1u\x1b[<1u\x1b[>1u\x1b[<1u");
        assert!(!KEYBOARD_ENHANCED.load(Ordering::SeqCst));
    }

    #[test]
    fn close_without_a_reader_returns_at_once() {
        let gate = InputGate::default();
        gate.close();
        gate.open();
        gate.enter();
        gate.leave();
    }
}
