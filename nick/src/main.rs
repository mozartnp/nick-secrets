//! Entry point of `nick`: the command line, the state directory and the database (before the
//! TUI: an error there is printed on the normal terminal), the cleanup of the agents left by a
//! dead nick, the panic hook, and the loop of the TUI. Only this file reads the environment
//! (HOME, XDG_STATE_HOME, NICK_INIT_BIN, EDITOR and the test hook NICK_TEST_PANIC).
//!
//! The loop is driven by a channel (finding of NS-29): an input thread reads the terminal
//! (paused by its gate while bin/nick-init runs), a signal thread turns SIGTERM and SIGHUP into
//! events, and the threads of each agent send its output, its copies (OSC 52) and its end,
//! with the id of the agent (NS-34: several run at once). The main thread blocks on the channel
//! and redraws only after something changed, at most once a frame.

use std::collections::HashMap;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event};
use nick::agent_session::{self, AgentEvent, AgentSession, SessionEvent};
use nick::agents;
use nick::app::{Action, AgentTerm, App, Focus};
use nick::db::Db;
use nick::editor::TerminalEditorRunner;
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
    Agent(AgentEvent),
    /// SIGTERM or SIGHUP (closing the window).
    Signal(i32),
}

impl From<AgentEvent> for LoopEvent {
    fn from(event: AgentEvent) -> LoopEvent {
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
    // The reserved shortcut of the database (NS-34): an invalid one, or an error, falls back
    // to the default with a message. The status has a single line: both messages are joined.
    let shortcut = app.load_shortcut();
    let messages: Vec<String> = cleanup.err().into_iter().chain(shortcut).collect();
    if !messages.is_empty() {
        app.set_status(messages.join(" "));
    }

    // The editor of the long texts of the stage forms (NS-35): ${EDITOR:-nano}.
    let editor = std::env::var_os("EDITOR");
    terminal::install_panic_hook();
    match run(app, editor) {
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
fn run(app: App, editor: Option<OsString>) -> io::Result<Option<i32>> {
    let mut guard = TerminalGuard::enter()?;
    let (tx, rx) = mpsc::channel::<LoopEvent>();
    let gate = Arc::new(InputGate::default());
    spawn_input(tx.clone(), Arc::clone(&gate));
    spawn_signals(tx.clone())?;
    test_hook()?;
    let mut shell = Shell {
        app,
        sessions: Vec::new(),
        sizes: HashMap::new(),
        tx,
        shown: None,
        agent_modes: false,
        exit_error: None,
        editor,
    };
    let result = shell.event_loop(&mut guard, &gate, &rx);
    // The error of the database when the agents were ended on the way out goes to the normal
    // terminal, after it is restored (a row may be left active; its lock is gone, so the cleanup
    // of the next opening ends it).
    drop(guard);
    if let Some(error) = shell.exit_error {
        eprintln!("{error}");
    }
    result
}

/// The state of the loop: the TUI and the terminal of each running agent.
struct Shell {
    app: App,
    /// The terminal of each running agent, by the id of its row, in the order they opened.
    sessions: Vec<(i64, AgentSession)>,
    /// The size (rows, columns) last given to each session.
    sizes: HashMap<i64, (u16, u16)>,
    tx: Sender<LoopEvent>,
    /// The shown agent the loop knows of (to see when it changes).
    shown: Option<i64>,
    /// Whether the mouse capture and the bracketed paste are on (with the focus on a terminal).
    agent_modes: bool,
    /// The error of the database when every agent was ended on the way out (`end_all`).
    exit_error: Option<String>,
    /// The value of EDITOR (the editor of the long texts of the stage forms).
    editor: Option<OsString>,
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
                LoopEvent::Agent(AgentEvent {
                    agent,
                    event: SessionEvent::Output,
                }) => {
                    // The shown one redraws; a hidden one only when its mark changes (N agents
                    // at work would redraw for nothing), and only a hidden one not marked yet
                    // reads its text at all.
                    if Some(agent) == self.app.shown() {
                        dirty = true;
                    } else if self.app.wants_hidden_text(agent)
                        && let Some(session) = self.session(agent)
                    {
                        let text = session.with_screen(|screen| screen.contents());
                        if self.app.hidden_output(agent, &text) {
                            dirty = true;
                            self.after_change(guard)?;
                        }
                    }
                }
                // A copy (OSC 52) of the shown agent goes to the outer terminal, which does it;
                // written between two frames, never inside one. A hidden agent must not change
                // the clipboard without the operator noticing: its copy is dropped.
                LoopEvent::Agent(AgentEvent {
                    agent,
                    event: SessionEvent::Clipboard(bytes),
                }) => {
                    if Some(agent) == self.app.shown() {
                        let mut stdout = io::stdout();
                        stdout.write_all(&bytes)?;
                        stdout.flush()?;
                    }
                }
                LoopEvent::Agent(AgentEvent {
                    agent,
                    event: SessionEvent::Exited(code),
                }) => {
                    self.agent_exited(guard, gate, agent, code)?;
                    self.after_change(guard)?;
                    dirty = true;
                }
                LoopEvent::Input(Event::Resize(..)) => {
                    self.after_change(guard)?;
                    dirty = true;
                }
                LoopEvent::Input(input) => {
                    self.read_agent_term();
                    // The window of the preview of the prompt depends on the screen: the App
                    // gets it from the same count the drawing uses.
                    let area = self.frame_area(guard)?;
                    if let Some((rows, max)) = ui::prompt_window(&self.app, area) {
                        self.app.set_prompt_window(rows, max);
                    }
                    let is_mouse = matches!(input, Event::Mouse(_));
                    let action = match input {
                        Event::Key(key) => self.app.handle_key(key),
                        Event::Mouse(mouse) => {
                            let area = ui::layout(&self.app, self.frame_area(guard)?).terminal;
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
                    if !is_mouse {
                        self.after_change(guard)?;
                    }
                }
            }
        }
    }

    /// A launcher for the runs outside the PTY (`--check` and `--render`): the size of a PTY is
    /// never asked.
    fn offline_launcher(&self) -> TerminalAgentLauncher<LoopEvent> {
        TerminalAgentLauncher::new(self.tx.clone(), Box::new(|_: &str, _: bool| (0, 0)))
    }

    /// The session of the agent `id`.
    fn session(&self, id: i64) -> Option<&AgentSession> {
        self.sessions
            .iter()
            .find(|(agent, _)| *agent == id)
            .map(|(_, session)| session)
    }

    /// The session of the shown agent.
    fn shown_session(&mut self) -> Option<&mut AgentSession> {
        let shown = self.app.shown()?;
        self.sessions
            .iter_mut()
            .find(|(agent, _)| *agent == shown)
            .map(|(_, session)| session)
    }

    fn frame_area(&self, guard: &mut TerminalGuard) -> io::Result<Rect> {
        Ok(guard.terminal().size()?.into())
    }

    /// After anything that may have changed the state of the app: when the shown agent
    /// changed, the text of the one that stopped being shown is kept (the rule of the mark);
    /// the modes of the outer terminal follow the focus; and every session whose area changed
    /// (a resize, the status or the help taking another number of rows, the list getting
    /// split) gets its new size, from the same `layout` the drawing uses.
    fn after_change(&mut self, guard: &mut TerminalGuard) -> io::Result<()> {
        let shown = self.app.shown();
        if shown != self.shown {
            if let Some(previous) = self.shown
                && let Some(session) = self.session(previous)
            {
                let text = session.with_screen(|screen| screen.contents());
                self.app.left_shown(previous, text);
            }
            self.shown = shown;
        }
        let modes = self.app.focus() == Focus::Terminal && shown.is_some();
        if modes != self.agent_modes {
            if modes {
                terminal::enter_agent_modes()?;
            } else {
                terminal::leave_agent_modes()?;
            }
            self.agent_modes = modes;
        }
        let frame = self.frame_area(guard)?;
        for (id, session) in &mut self.sessions {
            let Some(agent) = self.app.agent(*id) else {
                continue;
            };
            let area = ui::agent_area(&self.app, frame, agent);
            let size = (area.height, area.width);
            if self.sizes.get(id) != Some(&size) {
                let _ = session.resize(size.0, size.1);
                self.sizes.insert(*id, size);
            }
        }
        Ok(())
    }

    fn draw(&mut self, guard: &mut TerminalGuard) -> io::Result<()> {
        let app = &self.app;
        let session = app.shown().and_then(|id| self.session(id));
        guard.terminal().draw(|frame| match session {
            Some(session) => {
                session.with_screen(|screen| ui::draw_with_terminal(frame, app, Some(screen)));
            }
            None => ui::draw(frame, app),
        })?;
        Ok(())
    }

    /// Tells the app what the shown agent's program asked of its terminal, before an input
    /// event.
    fn read_agent_term(&mut self) {
        if let Some(session) = self.app.shown().and_then(|id| self.session(id)) {
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
            Action::CheckAgent { project, ticket } => {
                let mut launcher = self.offline_launcher();
                self.app.check_agent(&project, &ticket, &mut launcher);
            }
            Action::EditDescription => {
                let mut runner = TerminalEditorRunner::new(guard, gate, self.editor.as_deref());
                self.app.edit_description(&mut runner);
                if let Some(err) = runner.error {
                    return Err(err);
                }
            }
            Action::RenderPrompt => {
                let mut launcher = self.offline_launcher();
                self.app.render_prompt(&mut launcher);
            }
            Action::OpenAgent {
                project,
                ticket,
                kind,
                log,
                precheck,
            } => {
                // The PTY is born with its size: the panel the new agent will have (the same
                // count as `ui::layout`), less its header, known at the spawn.
                let panel = ui::new_agent_panel(&self.app, self.frame_area(guard)?);
                let size_of = move |name: &str, jail: bool| {
                    let area = ui::terminal_in_panel(panel, name, jail);
                    (area.height, area.width)
                };
                let mut launcher = TerminalAgentLauncher::new(self.tx.clone(), Box::new(size_of))
                    .with_jail(precheck.jail);
                self.app
                    .open_agent(&project, &ticket, kind, &log, precheck, &mut launcher);
                if let (Some(session), Some(id)) = (launcher.session.take(), self.app.shown()) {
                    let size = session.with_screen(|screen| screen.size());
                    self.sizes.insert(id, size);
                    self.sessions.push((id, session));
                    agent_test_hook(self.sessions.len());
                }
            }
            Action::Send(bytes) => {
                if let Some(session) = self.shown_session() {
                    // Any key sent goes back to the live screen.
                    session.scroll_to_bottom();
                    // A program that already ended has no reader: the input is dropped.
                    let _ = session.write(&bytes);
                }
            }
            Action::ScrollPageUp => {
                let page = page(guard)?;
                if let Some(session) = self.shown_session() {
                    session.scroll(page);
                }
            }
            Action::ScrollPageDown => {
                let page = page(guard)?;
                if let Some(session) = self.shown_session() {
                    session.scroll(-page);
                }
            }
            Action::ScrollLines(lines) => {
                if let Some(session) = self.shown_session() {
                    session.scroll(lines);
                }
            }
            Action::EndAgent(id) => {
                // Synchronous: up to TERM_GRACE when the group ignores SIGTERM (accepted). The
                // Exited its waiter sends afterwards finds no session and is ignored.
                if let Some(index) = self.sessions.iter().position(|(agent, _)| *agent == id) {
                    let (_, mut session) = self.sessions.remove(index);
                    self.sizes.remove(&id);
                    session.terminate();
                }
                self.app.agent_ended_by_operator(id);
            }
            Action::QuitEndingAgents => {
                self.end_all();
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The program of the agent `id` ended (the PTY closed and the child was reaped): its
    /// record ends and it leaves the running ones, with a full redraw. An agent the operator
    /// already ended has no session any more: its end is ignored.
    fn agent_exited(
        &mut self,
        guard: &mut TerminalGuard,
        gate: &InputGate,
        id: i64,
        code: u32,
    ) -> io::Result<()> {
        let Some(index) = self.sessions.iter().position(|(agent, _)| *agent == id) else {
            return Ok(());
        };
        let (_, mut session) = self.sessions.remove(index);
        self.sizes.remove(&id);
        session.wait();
        self.app.agent_ended(id, code);
        // The clear asks the terminal for the cursor position: the input thread is paused so
        // the answer is not read as a key.
        gate.close();
        let cleared = guard.terminal().clear();
        gate.open();
        cleared
    }

    /// Ends every agent: all the process groups at once (SIGTERM, SIGKILL after a single
    /// grace of 2 s, wait), then every record (`ended`, the locks released).
    fn end_all(&mut self) {
        let mut sessions: Vec<AgentSession> = self
            .sessions
            .drain(..)
            .map(|(_, session)| session)
            .collect();
        agent_session::terminate_all(&mut sessions);
        self.exit_error = self.app.end_all_records().err();
    }

    /// SIGTERM/SIGHUP: every agent is ended (`end_all`), and the terminal is restored by the
    /// guard on the way out.
    fn end_by_signal(&mut self) {
        self.end_all();
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
/// after an agent opens, and =second_agent after the second one (`running` agents), to prove
/// the panic hook ends the process groups of the agents.
fn agent_test_hook(running: usize) {
    let wanted = match std::env::var("NICK_TEST_PANIC").as_deref() {
        Ok("agent") => 1,
        Ok("second_agent") => 2,
        _ => return,
    };
    if running == wanted {
        thread::spawn(|| {
            thread::sleep(Duration::from_millis(500));
            panic!("NICK_TEST_PANIC: panic de teste com um agente aberto");
        });
    }
}
