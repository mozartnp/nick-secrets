//! nick-panel: throwaway prototype (NS-29, epic NS-28) of a terminal panel that runs several
//! real claude sessions side by side, each inside the ai-jail with the nick's permission rules
//! (through launch_session.sh and lib/). Not the base of the future shell.
//!
//! Event driven: one thread reads the terminal input, one waits for signals, each session has
//! its reader and waiter threads, and all of them feed one channel. The main loop blocks on it
//! and redraws only after something changed, at most once per frame (~60/s).

use std::io::{self, Write};
use std::process::ExitCode;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    Event,
};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Rect;
use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

use nick_panel::app::{Action, App};
use nick_panel::keys::TermModes;
use nick_panel::session::{self, LaunchConfig, Session, SessionEvent};
use nick_panel::ui::{self, Areas, SessionView};

/// The fixed, fictitious sessions (labels in the epic's Project > Ticket > Agent format). All
/// of them run the same launcher.
const SESSION_LABELS: [&str; 2] = [
    "Projeto Demo > NS-0001 > Tech Leader",
    "Projeto Demo > a1b2c3 > Dev",
];

/// The shortest time between two redraws.
const FRAME: Duration = Duration::from_millis(16);

enum PanelEvent {
    Input(Event),
    Session(SessionEvent),
    Signal,
}

impl From<SessionEvent> for PanelEvent {
    fn from(event: SessionEvent) -> Self {
        PanelEvent::Session(event)
    }
}

type Tui = Terminal<CrosstermBackend<io::Stdout>>;

/// Puts the terminal back (raw mode, alternate screen, mouse, bracketed paste, cursor).
/// Errors are ignored: after a SIGHUP the terminal may be gone.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        crossterm::cursor::Show
    );
}

fn main() -> ExitCode {
    let default_hook = std::panic::take_hook();
    // Any thread's panic ends the panel: the terminal is restored and the sessions ended right
    // here, and then the process exits with 101 (the code of a main-thread panic). Without the
    // exit, a panic in a secondary thread (a session reader, say) would leave the main loop
    // drawing over the restored terminal (review NS-29, problem 3).
    std::panic::set_hook(Box::new(move |info| {
        restore_terminal();
        session::terminate_registered();
        default_hook(info);
        std::process::exit(101);
    }));
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            restore_terminal();
            eprintln!("nick-panel: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), session::Error> {
    let config = LaunchConfig::for_panel()?;
    if !config.launcher.is_file() {
        return Err(format!("launcher não encontrado: {}", config.launcher.display()).into());
    }
    let (tx, rx) = mpsc::channel::<PanelEvent>();
    let mut signals = Signals::new([SIGTERM, SIGHUP, SIGINT])?;
    let signal_tx = tx.clone();
    thread::spawn(move || {
        if signals.forever().next().is_some() {
            let _ = signal_tx.send(PanelEvent::Signal);
        }
    });

    enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableMouseCapture,
        EnableBracketedPaste
    )?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;

    let input_tx = tx.clone();
    thread::spawn(move || {
        while let Ok(event) = event::read() {
            if input_tx.send(PanelEvent::Input(event)).is_err() {
                break;
            }
        }
    });

    let size = terminal.size()?;
    let areas = ui::layout(Rect::new(0, 0, size.width, size.height));
    let (rows, cols) = pty_size(&areas);
    let mut sessions: Vec<Session> = Vec::new();
    for (id, label) in SESSION_LABELS.iter().enumerate() {
        match Session::spawn(id, label, &config, rows, cols, tx.clone()) {
            Ok(session) => sessions.push(session),
            Err(error) => {
                session::terminate_all(&mut sessions);
                restore_terminal();
                return Err(error);
            }
        }
    }
    drop(tx);

    // Test hook (README, tests/panic_hook.rs): proves the panic hook restores the terminal and
    // ends the sessions, for a panic in the main thread or ("thread") in a secondary one.
    match std::env::var("NICK_PANEL_TEST_PANIC").as_deref() {
        Ok("thread") => {
            thread::spawn(|| {
                thread::sleep(Duration::from_millis(1500));
                panic!("NICK_PANEL_TEST_PANIC: panic de teste numa thread secundária");
            });
        }
        Ok(_) => {
            thread::sleep(Duration::from_millis(1500));
            panic!("NICK_PANEL_TEST_PANIC: panic de teste");
        }
        Err(_) => {}
    }

    let result = event_loop(&mut terminal, &mut sessions, &rx, areas);
    session::terminate_all(&mut sessions);
    restore_terminal();
    result
}

/// The PTY size for the terminal's inner area (never zero).
fn pty_size(areas: &Areas) -> (u16, u16) {
    (areas.inner.height.max(1), areas.inner.width.max(1))
}

fn draw(terminal: &mut Tui, app: &App, sessions: &[Session]) -> io::Result<()> {
    terminal.draw(|frame| {
        let views: Vec<SessionView> = sessions
            .iter()
            .map(|s| SessionView {
                label: &s.label,
                status: s.status(),
            })
            .collect();
        sessions[app.selected].with_screen(|screen| ui::draw(frame, app, &views, Some(screen)));
    })?;
    Ok(())
}

/// Carries out an action on the selected session. Returns true to quit.
fn apply(action: Action, session: &mut Session, areas: &Areas) -> bool {
    let page = i32::from(areas.inner.height.saturating_sub(1).max(1));
    match action {
        Action::None => {}
        Action::Send(bytes) => {
            session.scroll_to_bottom();
            // A session that already ended has no reader: the input is dropped.
            let _ = session.write(&bytes);
        }
        Action::ScrollPageUp => session.scroll(page),
        Action::ScrollPageDown => session.scroll(-page),
        Action::ScrollLines(lines) => session.scroll(lines),
        Action::Quit => return true,
    }
    false
}

fn event_loop(
    terminal: &mut Tui,
    sessions: &mut [Session],
    rx: &mpsc::Receiver<PanelEvent>,
    mut areas: Areas,
) -> Result<(), session::Error> {
    let mut app = App::new(sessions.len());
    let mut dirty = true;
    let mut last_draw = Instant::now() - FRAME;
    loop {
        let event = if dirty {
            let wait = (last_draw + FRAME).saturating_duration_since(Instant::now());
            if wait.is_zero() {
                draw(terminal, &app, sessions)?;
                last_draw = Instant::now();
                dirty = false;
                continue;
            }
            match rx.recv_timeout(wait) {
                Ok(event) => event,
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return Ok(()),
            }
        } else {
            match rx.recv() {
                Ok(event) => event,
                Err(_) => return Ok(()),
            }
        };

        let selected = app.selected;
        let (modes, mouse_mode, mouse_encoding, bracketed) = sessions[selected].with_screen(|s| {
            (
                TermModes {
                    application_cursor: s.application_cursor(),
                },
                s.mouse_protocol_mode(),
                s.mouse_protocol_encoding(),
                s.bracketed_paste(),
            )
        });
        let action = match event {
            // Output of a hidden session changes nothing on screen.
            PanelEvent::Session(SessionEvent::Output(id)) => {
                dirty |= id == selected;
                continue;
            }
            PanelEvent::Session(SessionEvent::Exited(_)) => {
                dirty = true;
                continue;
            }
            // A copy (OSC 52) goes to the outer terminal, which does it; written between two
            // frames, so it never lands inside one.
            PanelEvent::Session(SessionEvent::Clipboard(_, bytes)) => {
                let backend = terminal.backend_mut();
                backend.write_all(&bytes)?;
                backend.flush()?;
                continue;
            }
            PanelEvent::Signal => return Ok(()),
            PanelEvent::Input(Event::Key(key)) => {
                dirty = true;
                app.handle_key(&key, sessions, modes)
            }
            PanelEvent::Input(Event::Mouse(mouse)) => {
                // With the mouse captured, every move is an event: redraw only on effect.
                let action = app.handle_mouse(&mouse, mouse_mode, mouse_encoding, areas.inner);
                dirty |= action != Action::None;
                action
            }
            PanelEvent::Input(Event::Paste(text)) => app.handle_paste(&text, bracketed),
            PanelEvent::Input(Event::Resize(width, height)) => {
                areas = ui::layout(Rect::new(0, 0, width, height));
                let (rows, cols) = pty_size(&areas);
                // Every session shares the same rectangle, so a hidden one is already the
                // right size when it comes back on screen.
                for session in sessions.iter_mut() {
                    let _ = session.resize(rows, cols);
                }
                dirty = true;
                Action::None
            }
            PanelEvent::Input(_) => Action::None,
        };
        if apply(action, &mut sessions[app.selected], &areas) {
            return Ok(());
        }
    }
}
