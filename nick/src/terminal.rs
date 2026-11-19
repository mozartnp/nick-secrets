//! Entering and leaving the TUI (raw mode and the alternate screen), suspending it to run a
//! program on the normal terminal, the modes of the agent screen (mouse capture and bracketed
//! paste, only while an agent is on screen), the gate that pauses the input thread and the
//! panic hook.

use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Condvar, Mutex, PoisonError};

use crossterm::cursor::Show;
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// The terminal of the TUI.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Turns off the mouse capture and the bracketed paste (always: turning off what is not on is
/// harmless), leaves the alternate screen, shows the cursor and leaves raw mode, ignoring errors
/// (there is nothing better to do with them while leaving; after a SIGHUP the terminal may be
/// gone). Safe to call more than once.
pub fn restore() {
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

/// RAII guard of the TUI: entering puts the terminal in raw mode and on the alternate screen,
/// and dropping it restores both and shows the cursor, also when `run` returns an error.
pub struct TerminalGuard {
    terminal: Tui,
}

impl TerminalGuard {
    pub fn enter() -> io::Result<TerminalGuard> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        if let Err(err) = execute!(stdout, EnterAlternateScreen) {
            restore();
            return Err(err);
        }
        match Terminal::new(CrosstermBackend::new(stdout)) {
            Ok(terminal) => Ok(TerminalGuard { terminal }),
            Err(err) => {
                restore();
                Err(err)
            }
        }
    }

    pub fn terminal(&mut self) -> &mut Tui {
        &mut self.terminal
    }

    /// Runs `f` on the normal terminal: leaves the alternate screen and raw mode, shows the
    /// cursor, runs it, and then enters both again with a full redraw, whatever `f` did.
    pub fn suspend<T>(&mut self, f: impl FnOnce() -> T) -> io::Result<T> {
        restore();
        let result = f();
        enable_raw_mode()?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        self.terminal.clear()?;
        Ok(result)
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

    #[test]
    fn close_without_a_reader_returns_at_once() {
        let gate = InputGate::default();
        gate.close();
        gate.open();
        gate.enter();
        gate.leave();
    }
}
