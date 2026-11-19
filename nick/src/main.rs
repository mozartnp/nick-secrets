//! Entry point of `nick`: the command line, the state directory and the database (before the
//! TUI: an error there is printed on the normal terminal), the cleanup of the agents left by a
//! dead nick, the panic hook, and the loop of the TUI. Only this file reads the environment
//! (HOME, XDG_STATE_HOME, NICK_INIT_BIN and the test hook NICK_TEST_PANIC).
//!
//! The loop is driven by a channel (finding of NS-29): an input thread reads the terminal
//! (paused by its gate while bin/nick-init runs), a signal thread turns SIGTERM and SIGHUP into
//! events, and the agent's threads send its output, its copies (OSC 52) and its end. The main
//! thread blocks on the channel and redraws only after something changed, at most once a frame.

use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use nick::agent_session::{AgentSession, SessionEvent};
use nick::agents;
use nick::app::{Action, AgentTerm, App, Screen};
use nick::db::Db;
use nick::init::{self, TerminalInitRunner};
use nick::keys::TermModes;
use nick::launch::{self, TerminalAgentLauncher};
use nick::paths::{self, DB_FILE_NAME};
use nick::terminal::{self, InputGate, TerminalGuard};
use nick::ui;
use ratatui::layout::Rect;
use signal_hook::consts::{SIGHUP, SIGTERM};
use signal_hook::iterator::Signals;

/// The shortest time between two redraws.
const FRAME: Duration = Duration::from_millis(16);
/// How long the input thread waits for an event before looking at its gate again.
const INPUT_POLL: Duration = Duration::from_millis(100);

/// What reaches the main loop.
enum LoopEvent {
    Input(Event),
    Agent(SessionEvent),
    /// SIGTERM or SIGHUP (closing the window).
    Signal(i32),
}

impl From<SessionEvent> for LoopEvent {
    fn from(event: SessionEvent) -> LoopEvent {
        LoopEvent::Agent(event)
    }
}

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if let Some(code) = handle_args(&args) {
        return code;
    }

    let home = std::env::var_os("HOME");
    let state_dir = match paths::state_dir(
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        home.as_deref(),
    ) {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = paths::create_state_dir(&state_dir) {
        eprintln!(
            "Erro: não consegui criar a pasta do banco do nick {}: {err}.",
            state_dir.display()
        );
        return ExitCode::FAILURE;
    }
    let db = match Db::open(&state_dir.join(DB_FILE_NAME)) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };
    // The agents whose nick died (even by kill -9) are ended before the TUI; one whose lock
    // another live nick holds stays active (agents.rs). An error here goes to the status bar.
    let agents_dir = agents::agents_dir(&state_dir);
    let cleanup = agents::cleanup(&db, &agents_dir);
    let mut app = App::new(
        db,
        home.filter(|home| !home.is_empty()).map(PathBuf::from),
        std::env::current_dir().ok(),
        init::init_path(std::env::var_os("NICK_INIT_BIN").as_deref()),
    )
    .with_agents(launch::launcher_path(), agents_dir);
    if let Err(message) = cleanup {
        app.set_status(message);
    }

    terminal::install_panic_hook();
    match run(app) {
        Ok(None) => ExitCode::SUCCESS,
        // Ended by a signal: 128 + the signal, like a shell reports it (143, 129).
        Ok(Some(signal)) => ExitCode::from(u8::try_from(128 + signal).unwrap_or(1)),
        Err(err) => {
            eprintln!("nick: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `--version` alone prints the version; any other argument is refused with the usage, in
/// the pattern of bin/nick-init. Returns the exit code when the program must end here.
fn handle_args(args: &[OsString]) -> Option<ExitCode> {
    match args {
        [] => None,
        [only] if only == "--version" => {
            println!("nick {}", env!("CARGO_PKG_VERSION"));
            Some(ExitCode::SUCCESS)
        }
        _ => {
            let unknown = args
                .iter()
                .find(|arg| *arg != "--version")
                .unwrap_or_else(|| &args[1]);
            eprintln!(
                "Erro: argumento desconhecido: '{}'",
                unknown.to_string_lossy()
            );
            eprintln!("Uso: nick");
            Some(ExitCode::FAILURE)
        }
    }
}

/// The input thread: reads the terminal while its gate is open, polling so a closed gate is
/// seen within `INPUT_POLL`. It ends when the loop is gone.
fn spawn_input(tx: Sender<LoopEvent>, gate: Arc<InputGate>) {
    thread::spawn(move || {
        loop {
            gate.enter();
            let event = match event::poll(INPUT_POLL) {
                Ok(true) => event::read().ok(),
                Ok(false) => None,
                Err(_) => {
                    gate.leave();
                    return;
                }
            };
            gate.leave();
            if let Some(event) = event
                && tx.send(LoopEvent::Input(event)).is_err()
            {
                return;
            }
        }
    });
}

/// The signal thread: SIGTERM and SIGHUP become events. Not SIGINT: in raw mode Ctrl+C is a
/// key, and while bin/nick-init runs the init's own handler is its only owner (init.rs).
fn spawn_signals(tx: Sender<LoopEvent>) -> io::Result<()> {
    let mut signals = Signals::new([SIGTERM, SIGHUP])?;
    thread::spawn(move || {
        for signal in signals.forever() {
            if tx.send(LoopEvent::Signal(signal)).is_err() {
                return;
            }
        }
    });
    Ok(())
}

/// The loop of the TUI. Returns the signal that ended it, if any.
fn run(app: App) -> io::Result<Option<i32>> {
    let mut guard = TerminalGuard::enter()?;
    let (tx, rx) = mpsc::channel::<LoopEvent>();
    let gate = Arc::new(InputGate::default());
    spawn_input(tx.clone(), Arc::clone(&gate));
    spawn_signals(tx.clone())?;
    test_hook()?;
    let mut shell = Shell {
        app,
        session: None,
        tx,
    };
    shell.event_loop(&mut guard, &gate, &rx)
}

/// The state of the loop: the TUI and the terminal of the running agent.
struct Shell {
    app: App,
    session: Option<AgentSession>,
    tx: Sender<LoopEvent>,
}

impl Shell {
    fn event_loop(
        &mut self,
        guard: &mut TerminalGuard,
        gate: &InputGate,
        rx: &Receiver<LoopEvent>,
    ) -> io::Result<Option<i32>> {
        let mut dirty = true;
        let mut last_draw = Instant::now() - FRAME;
        loop {
            let event = if dirty && !terminal::panicking() {
                let wait = (last_draw + FRAME).saturating_duration_since(Instant::now());
                if wait.is_zero() {
                    self.draw(guard)?;
                    last_draw = Instant::now();
                    dirty = false;
                    continue;
                }
                match rx.recv_timeout(wait) {
                    Ok(event) => event,
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => return Ok(None),
                }
            } else {
                match rx.recv() {
                    Ok(event) => event,
                    Err(_) => return Ok(None),
                }
            };
            // A panic in another thread: its hook restores the terminal, ends the agents and
            // exits; this thread must neither draw nor write anything meanwhile.
            while terminal::panicking() {
                thread::park();
            }
            match event {
                LoopEvent::Signal(signal) => {
                    self.end_by_signal();
                    return Ok(Some(signal));
                }
                LoopEvent::Agent(SessionEvent::Output) => dirty |= self.session.is_some(),
                // A copy (OSC 52) goes to the outer terminal, which does it; written between two
                // frames, never inside one.
                LoopEvent::Agent(SessionEvent::Clipboard(bytes)) => {
                    let mut stdout = io::stdout();
                    stdout.write_all(&bytes)?;
                    stdout.flush()?;
                }
                LoopEvent::Agent(SessionEvent::Exited(code)) => {
                    self.agent_exited(guard, gate, code)?;
                    dirty = true;
                }
                LoopEvent::Input(Event::Resize(width, height)) => {
                    if let Some(session) = &mut self.session {
                        let area = ui::agent_area(Rect::new(0, 0, width, height));
                        let _ = session.resize(area.height, area.width);
                    }
                    dirty = true;
                }
                LoopEvent::Input(input) => {
                    self.read_agent_term();
                    let is_mouse = matches!(input, Event::Mouse(_));
                    let action = match input {
                        Event::Key(key) => self.app.handle_key(key),
                        Event::Mouse(mouse) => {
                            let area = ui::agent_area(guard.terminal().size()?.into());
                            self.app.handle_mouse(&mouse, area)
                        }
                        Event::Paste(text) => self.app.handle_paste(&text),
                        _ => Action::None,
                    };
                    // With the mouse captured, every move is an event: redraw only on effect.
                    dirty |= !is_mouse || action != Action::None;
                    if self.carry_out(action, guard, gate)? {
                        return Ok(None);
                    }
                }
            }
        }
    }

    fn draw(&mut self, guard: &mut TerminalGuard) -> io::Result<()> {
        let app = &self.app;
        let session = self.session.as_ref();
        guard.terminal().draw(|frame| match session {
            Some(session) => {
                session.with_screen(|screen| ui::draw_with_terminal(frame, app, Some(screen)));
            }
            None => ui::draw(frame, app),
        })?;
        Ok(())
    }

    /// Tells the app what the agent's program asked of its terminal, before an input event.
    fn read_agent_term(&mut self) {
        if let Some(session) = &self.session {
            let term = session.with_screen(|screen| AgentTerm {
                modes: TermModes {
                    application_cursor: screen.application_cursor(),
                },
                bracketed_paste: screen.bracketed_paste(),
                mouse_mode: screen.mouse_protocol_mode(),
                mouse_encoding: screen.mouse_protocol_encoding(),
            });
            self.app.set_agent_term(term);
        }
    }

    /// Carries out an action. Returns true to quit.
    fn carry_out(
        &mut self,
        action: Action,
        guard: &mut TerminalGuard,
        gate: &InputGate,
    ) -> io::Result<bool> {
        // A page of the history: the terminal of the agent, less one row of context.
        let page = |guard: &mut TerminalGuard| {
            guard
                .terminal()
                .size()
                .map(|size| i32::from(size.height.saturating_sub(2).max(1)))
        };
        match action {
            Action::None => {}
            Action::Quit => return Ok(true),
            Action::RunInit { project } => {
                let init = self.app.init_path().to_path_buf();
                let mut runner = TerminalInitRunner::new(guard, gate);
                let outcome = init::InitRunner::run(&mut runner, &init, &project);
                if let Some(err) = runner.error {
                    return Err(err);
                }
                self.app.after_init(outcome);
            }
            Action::OpenAgent {
                project,
                ticket,
                stage,
            } => {
                let area = ui::agent_area(guard.terminal().size()?.into());
                let mut launcher =
                    TerminalAgentLauncher::new(self.tx.clone(), (area.height, area.width));
                self.app.open_agent(&project, &ticket, stage, &mut launcher);
                if self.app.screen() == Screen::Agent {
                    self.session = launcher.session.take();
                    terminal::enter_agent_modes()?;
                    agent_test_hook();
                }
            }
            Action::Send(bytes) => {
                if let Some(session) = &mut self.session {
                    // Any key sent goes back to the live screen.
                    session.scroll_to_bottom();
                    // A program that already ended has no reader: the input is dropped.
                    let _ = session.write(&bytes);
                }
            }
            Action::ScrollPageUp => {
                let page = page(guard)?;
                if let Some(session) = &self.session {
                    session.scroll(page);
                }
            }
            Action::ScrollPageDown => {
                let page = page(guard)?;
                if let Some(session) = &self.session {
                    session.scroll(-page);
                }
            }
            Action::ScrollLines(lines) => {
                if let Some(session) = &self.session {
                    session.scroll(lines);
                }
            }
        }
        Ok(false)
    }

    /// The agent's program ended (the PTY closed and the child was reaped): the record ends,
    /// the modes of the agent screen go off, and the Tickets screen comes back with a full
    /// redraw.
    fn agent_exited(
        &mut self,
        guard: &mut TerminalGuard,
        gate: &InputGate,
        code: u32,
    ) -> io::Result<()> {
        let Some(mut session) = self.session.take() else {
            return Ok(());
        };
        session.wait();
        self.app.agent_ended(code);
        terminal::leave_agent_modes()?;
        // The clear asks the terminal for the cursor position: the input thread is paused so
        // the answer is not read as a key.
        gate.close();
        let cleared = guard.terminal().clear();
        gate.open();
        cleared
    }

    /// SIGTERM/SIGHUP: the agent's process group is ended (SIGTERM, SIGKILL after 2 s, wait),
    /// its record ends, and the terminal is restored by the guard on the way out.
    fn end_by_signal(&mut self) {
        if let Some(mut session) = self.session.take() {
            session.terminate();
        }
        let _ = self.app.end_agent_record();
    }
}

/// Test hook (ABOUT.md, tests/tui.rs), only for the tests: NICK_TEST_PANIC=main panics in the
/// main thread, =thread panics in a secondary thread while the main one is blocked on the
/// channel, and =error makes `run` return an error, all after entering the TUI.
fn test_hook() -> io::Result<()> {
    match std::env::var("NICK_TEST_PANIC").as_deref() {
        Ok("main") => panic!("NICK_TEST_PANIC: panic de teste na thread principal"),
        Ok("thread") => {
            thread::spawn(|| {
                thread::sleep(Duration::from_millis(300));
                panic!("NICK_TEST_PANIC: panic de teste numa thread secundária");
            });
            Ok(())
        }
        Ok("error") => Err(io::Error::other("NICK_TEST_PANIC: erro de teste")),
        _ => Ok(()),
    }
}

/// Test hook (tests/agent.rs): NICK_TEST_PANIC=agent panics in a secondary thread shortly
/// after an agent opens, to prove the panic hook ends the agent's process group.
fn agent_test_hook() {
    if std::env::var("NICK_TEST_PANIC").as_deref() == Ok("agent") {
        thread::spawn(|| {
            thread::sleep(Duration::from_millis(500));
            panic!("NICK_TEST_PANIC: panic de teste com um agente aberto");
        });
    }
}
