//! Entering and leaving the TUI (raw mode and the alternate screen), suspending it to run a
//! program on the normal terminal, and the panic hook. No mouse capture nor bracketed paste in
//! this slice: nothing uses them, and each mode more is one more to restore.

use std::io::{self, Stdout, Write};

use crossterm::cursor::Show;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// The terminal of the TUI.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Leaves the alternate screen, shows the cursor and leaves raw mode, ignoring errors (there
/// is nothing better to do with them while leaving). Safe to call more than once.
pub fn restore() {
    let mut stdout = io::stdout();
    let _ = execute!(stdout, LeaveAlternateScreen, Show);
    let _ = stdout.flush();
    let _ = disable_raw_mode();
}

/// Installs the panic hook: whatever thread panics, the terminal is restored first, then the
/// default hook prints the message (readable, on the normal screen) and the process exits with
/// 101. Without the exit, a panic in a secondary thread would leave the main loop running on
/// a restored terminal (finding of NS-29).
pub fn install_panic_hook() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        default_hook(info);
        std::process::exit(101);
    }));
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
