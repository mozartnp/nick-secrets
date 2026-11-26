//! The terminal of an agent (NS-33): a PTY running `bash <bin/nick-agent> --session-id <uuid>
//! --name <name> --type <type> --prompt-file <log>` (the launcher runs claude with the prompt
//! of the log, inside the jail when the project asks for it, through lib/; NS-35), a vt100 parser that emulates its screen, a reader thread that drains the PTY
//! all the time (an unread PTY fills up and blocks the child on write) and a waiter thread that
//! reaps the child. Written from the findings of the panel prototype (NS-29,
//! prototype/panel/src/session.rs), whose cases are the source of the tests.
//!
//! The only command ever started here is bash with the launcher: the API takes no other
//! command, so there is no second launch path (tests/agent.rs checks the sources for that).

use std::io::{self, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::launch::Launch;

/// Lines of scrollback kept by the parser.
pub const SCROLLBACK_LINES: usize = 10_000;
/// How long the group gets after SIGTERM before SIGKILL.
pub const TERM_GRACE: Duration = Duration::from_secs(2);
/// How long the waiter waits for the reader to drain the last output after the child is
/// reaped (a background process may keep the PTY open forever).
const DRAIN_GRACE: Duration = Duration::from_millis(300);
/// The smallest screen vt100 0.16 survives: below it, a wide character (an emoji or CJK, 2
/// columns) panics in the parser (finding of NS-29).
pub const MIN_ROWS: u16 = 2;
pub const MIN_COLS: u16 = 2;

/// The size of the screen (parser and PTY) for `rows`x`cols`: at least
/// [`MIN_ROWS`]x[`MIN_COLS`]. On a too small window, the screen is cut at the edge.
pub fn screen_size(rows: u16, cols: u16) -> (u16, u16) {
    (rows.max(MIN_ROWS), cols.max(MIN_COLS))
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// What the agent threads tell the main loop, for one session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// New output was parsed: redraw.
    Output,
    /// The child asked the terminal to copy (OSC 52): the bytes to write to the outer terminal,
    /// which does the copy.
    Clipboard(Vec<u8>),
    /// The child was reaped, with its exit code (portable-pty reports 1 for a signal).
    Exited(u32),
}

/// An event of the session of the agent `agent` (the `id` of its row): several sessions share
/// the channel of the loop (NS-34). `generation` is the one of the session that sent it (NS-57,
/// D8): an agent ended and resumed has a new session with the same id, and an event of the old
/// one still in the channel must not be taken for the new one's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AgentEvent {
    pub agent: i64,
    pub generation: u64,
    pub event: SessionEvent,
}

/// The generation of the next session (NS-57, D8): unique in the process, never reused.
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// The vt100 callbacks: keeps the copy requests (OSC 52) the parser sees, for the reader to hand
/// them to the loop. vt100 only calls `copy_to_clipboard` for a valid selector and base64 data,
/// so what is rebuilt here carries no other escape sequence. Reads of the clipboard
/// (`ESC]52;c;?`) are never forwarded: the program never gets the user's clipboard through the
/// nick. Which copies arrive at all is up to the jail (the `restrito` level filters them).
#[derive(Default)]
pub struct Clipboard {
    copies: Vec<Vec<u8>>,
}

impl Clipboard {
    /// The copy requests seen since the last call, as OSC 52 sequences ended by BEL.
    pub fn take_copies(&mut self) -> Vec<Vec<u8>> {
        std::mem::take(&mut self.copies)
    }
}

impl vt100::Callbacks for Clipboard {
    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8], data: &[u8]) {
        let mut seq = b"\x1b]52;".to_vec();
        seq.extend_from_slice(ty);
        seq.push(b';');
        seq.extend_from_slice(data);
        seq.push(0x07);
        self.copies.push(seq);
    }
}

type AgentParser = vt100::Parser<Clipboard>;

fn new_parser(rows: u16, cols: u16) -> AgentParser {
    vt100::Parser::new_with_callbacks(rows, cols, SCROLLBACK_LINES, Clipboard::default())
}

/// The process groups of the agents not reaped yet, for the panic hook (which has no access to
/// the sessions themselves) to end them too.
static GROUPS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The terminal of a running agent.
pub struct AgentSession {
    parser: Arc<Mutex<AgentParser>>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pid: Option<u32>,
    exit: Arc<Mutex<Option<u32>>>,
    waiter: Option<JoinHandle<()>>,
    /// Unique to this session (NS-57, D8): its events carry it.
    generation: u64,
}

impl AgentSession {
    /// Opens a PTY of `rows`x`cols` (at least 2x2) and runs `bash <launcher> <arguments of
    /// launch>` in it (`Launch::args`: a new agent, `--session-id <session_id> --name <name>
    /// --type <kind> --prompt-file <prompt_file>`, or a resume, NS-39, `--resume <session_id>
    /// --name <name> --stage <stage> [--opened-id <key>] --ticket-id <key>`), with the cwd at
    /// `project`, the environment of the nick plus TERM=xterm-256color and COLORTERM=truecolor.
    /// Events go to `tx`, as `AgentEvent`s of `agent_id` (the `id` of the row of the agent,
    /// stored before the spawn). The launcher decides the rest of the argv of claude by the type
    /// or the stage (the auto mode of the Dev stage): the shell never passes it.
    #[allow(clippy::too_many_arguments)]
    pub fn spawn<E: From<AgentEvent> + Send + 'static>(
        launcher: &Path,
        project: &Path,
        agent_id: i64,
        session_id: &str,
        name: &str,
        launch: Launch,
        (rows, cols): (u16, u16),
        tx: Sender<E>,
    ) -> Result<AgentSession, Error> {
        AgentSession::spawn_with_env(
            launcher,
            project,
            agent_id,
            session_id,
            name,
            launch,
            (rows, cols),
            &[],
            tx,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn spawn_with_env<E: From<AgentEvent> + Send + 'static>(
        launcher: &Path,
        project: &Path,
        agent_id: i64,
        session_id: &str,
        name: &str,
        launch: Launch,
        (rows, cols): (u16, u16),
        env: &[(&str, &str)],
        tx: Sender<E>,
    ) -> Result<AgentSession, Error> {
        let (rows, cols) = screen_size(rows, cols);
        let pair = native_pty_system().openpty(pty_size(rows, cols))?;
        let mut command = CommandBuilder::new("bash");
        command.arg(launcher);
        command.args(launch.args(session_id, name));
        command.cwd(project);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        for (key, value) in env {
            command.env(key, value);
        }
        let mut child = pair.slave.spawn_command(command)?;
        // Only the child keeps the slave open, so the reader sees EOF when it is gone.
        drop(pair.slave);
        let pid = child.process_id();
        if let Some(pid) = pid {
            lock(&GROUPS).push(pid as i32);
        }

        let parser = Arc::new(Mutex::new(new_parser(rows, cols)));
        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        let reader_parser = Arc::clone(&parser);
        let reader_tx = tx.clone();
        let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
        let event = move |event| {
            AgentEvent {
                agent: agent_id,
                generation,
                event,
            }
            .into()
        };
        let (drained_tx, drained_rx) = mpsc::channel::<()>();
        thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let copies = {
                            let mut parser = lock(&reader_parser);
                            parser.process(&buf[..n]);
                            parser.callbacks_mut().take_copies()
                        };
                        // The loop may be gone: keep draining anyway.
                        for copy in copies {
                            let _ = reader_tx.send(event(SessionEvent::Clipboard(copy)));
                        }
                        let _ = reader_tx.send(event(SessionEvent::Output));
                    }
                }
            }
            let _ = drained_tx.send(());
        });

        let exit = Arc::new(Mutex::new(None));
        let waiter_exit = Arc::clone(&exit);
        let waiter = thread::spawn(move || {
            let code = child.wait().map_or(1, |status| status.exit_code());
            if let Some(pid) = pid {
                lock(&GROUPS).retain(|g| *g != pid as i32);
            }
            // The last output first, so the screen is complete when the loop hears of the end.
            let _ = drained_rx.recv_timeout(DRAIN_GRACE);
            *lock(&waiter_exit) = Some(code);
            let _ = tx.send(event(SessionEvent::Exited(code)));
        });

        Ok(AgentSession {
            parser,
            master: pair.master,
            writer,
            pid,
            exit,
            waiter: Some(waiter),
            generation,
        })
    }

    /// The generation of this session (NS-57, D8): the one its events carry.
    pub fn generation(&self) -> u64 {
        self.generation
    }

    /// The pid of the child, which is also its process group (the PTY child is a session
    /// leader; the launcher and ai-jail exec, so it ends up being claude's pid).
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// The exit code once the child was reaped; None while it runs.
    pub fn exit_code(&self) -> Option<u32> {
        *lock(&self.exit)
    }

    /// Writes input to the child.
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    /// Resizes the PTY (the child gets SIGWINCH) and the parser, to at least 2x2.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), Error> {
        let (rows, cols) = screen_size(rows, cols);
        lock(&self.parser).screen_mut().set_size(rows, cols);
        self.master.resize(pty_size(rows, cols))?;
        Ok(())
    }

    /// Runs `f` with the screen as the parser has it now.
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        f(lock(&self.parser).screen())
    }

    /// Scrolls the history by `lines` (positive: up, into the history); clamped by vt100.
    pub fn scroll(&self, lines: i32) {
        let mut parser = lock(&self.parser);
        let screen = parser.screen_mut();
        let current = i64::try_from(screen.scrollback()).unwrap_or(i64::MAX);
        let wanted = current.saturating_add(i64::from(lines)).max(0);
        screen.set_scrollback(usize::try_from(wanted).unwrap_or(usize::MAX));
    }

    /// Back to the live screen (no scrollback offset).
    pub fn scroll_to_bottom(&self) {
        lock(&self.parser).screen_mut().set_scrollback(0);
    }

    /// Ends the agent: `terminate_all` of this session alone.
    pub fn terminate(&mut self) {
        terminate_all(std::slice::from_mut(self));
    }

    /// Waits for the waiter thread (the child reaped and the end announced).
    pub fn wait(&mut self) {
        if let Some(waiter) = self.waiter.take() {
            let _ = waiter.join();
        }
    }
}

fn pty_size(rows: u16, cols: u16) -> PtySize {
    PtySize {
        rows,
        cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Whether any process of group `pgid` is still alive (a not yet reaped zombie counts).
fn group_alive(pgid: i32) -> bool {
    killpg(Pid::from_raw(pgid), None).is_ok()
}

/// SIGTERM to every group, up to [`TERM_GRACE`] for them to go, then SIGKILL to what is left.
fn end_groups(groups: &[i32]) {
    for &group in groups {
        let _ = killpg(Pid::from_raw(group), Signal::SIGTERM);
    }
    let deadline = Instant::now() + TERM_GRACE;
    while Instant::now() < deadline && groups.iter().any(|&g| group_alive(g)) {
        thread::sleep(Duration::from_millis(20));
    }
    for &group in groups {
        if group_alive(group) {
            let _ = killpg(Pid::from_raw(group), Signal::SIGKILL);
        }
    }
}

/// Ends the agents of `sessions`: SIGTERM to the process group of each child (not only the
/// child: whatever it started in its group goes too), SIGKILL after [`TERM_GRACE`] to what is
/// left, all in a single `end_groups` (one grace for all of them, not one per agent), then
/// waits for each child to be reaped. Only a group not reaped yet is signaled: once the child
/// is reaped, its pid may go to an unrelated group leader (finding of NS-29).
pub fn terminate_all(sessions: &mut [AgentSession]) {
    let registered = lock(&GROUPS).clone();
    let groups: Vec<i32> = sessions
        .iter()
        .filter_map(|session| session.pid)
        .map(|pid| pid as i32)
        .filter(|group| registered.contains(group))
        .collect();
    end_groups(&groups);
    for session in sessions {
        session.wait();
    }
}

/// The index in `sessions` (the sessions of the loop, by the id of the row of each agent) of the
/// session `event` came from, or None when it is from no session there: an agent already ended,
/// or (NS-57, D8) an older session of an agent resumed since then, with the same id and another
/// generation.
pub fn event_session(sessions: &[(i64, AgentSession)], event: &AgentEvent) -> Option<usize> {
    sessions.iter().position(|(agent, session)| {
        *agent == event.agent && session.generation == event.generation
    })
}

/// Ends the groups of every agent not reaped yet, from anywhere (the panic hook): the same
/// signals as [`AgentSession::terminate`], without the session.
pub fn terminate_registered() {
    let groups = lock(&GROUPS).clone();
    end_groups(&groups);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::Stage;
    use crate::launch::HolderKey;
    use crate::stages::StageType;
    use std::fs;
    use std::path::PathBuf;

    fn copies(input: &[u8]) -> Vec<Vec<u8>> {
        let mut parser = new_parser(24, 80);
        parser.process(input);
        parser.callbacks_mut().take_copies()
    }

    #[test]
    fn a_copy_request_is_kept_to_forward_to_the_outer_terminal() {
        assert_eq!(
            copies(b"antes\x1b]52;c;b2zDoQ==\x07depois"),
            vec![b"\x1b]52;c;b2zDoQ==\x07".to_vec()]
        );
        // Ended by ST too, forwarded ended by BEL (terminals take both).
        assert_eq!(
            copies(b"\x1b]52;pc;aGk=\x1b\\"),
            vec![b"\x1b]52;pc;aGk=\x07".to_vec()]
        );
        // Taken once: the next take is empty.
        let mut parser = new_parser(24, 80);
        parser.process(b"\x1b]52;c;aGk=\x07");
        assert_eq!(parser.callbacks_mut().take_copies().len(), 1);
        assert!(parser.callbacks_mut().take_copies().is_empty());
    }

    #[test]
    fn only_copies_are_forwarded() {
        // A read of the clipboard, and anything vt100 does not take as a copy (a bad selector,
        // data that isn't base64, another OSC).
        assert!(copies(b"\x1b]52;c;?\x07").is_empty());
        assert!(copies(b"\x1b]52;x;aGk=\x07").is_empty());
        assert!(copies(b"\x1b]52;c;a b!\x07").is_empty());
        assert!(copies(b"\x1b]2;titulo\x07").is_empty());
        // An ESC inside ends the OSC there: only the clean part is forwarded.
        assert_eq!(
            copies(b"\x1b]52;c;aGk=\x1b[31m\x07"),
            vec![b"\x1b]52;c;aGk=\x07".to_vec()]
        );
    }

    #[test]
    fn the_screen_is_never_smaller_than_2x2() {
        // Control: vt100 panics with a wide character at 1 column or 1 row.
        let texts = [
            "⚠️ a ⚠️ b 中文",
            "中中中中中中\r\n中a中a中",
            "\x1b[999C中中\r\n\x1b[2;1H中",
        ];
        let feed = |parser: &mut AgentParser| {
            for _ in 0..3 {
                for text in texts {
                    parser.process(text.as_bytes());
                }
            }
        };
        let hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        for (rows, cols) in [(1, 1), (2, 1), (1, 2)] {
            let raw = std::panic::catch_unwind(|| feed(&mut new_parser(rows, cols)));
            assert!(raw.is_err(), "control: vt100 panics at {rows}x{cols}");
        }
        std::panic::set_hook(hook);
        assert_eq!(screen_size(0, 0), (MIN_ROWS, MIN_COLS));
        assert_eq!(screen_size(1, 1), (MIN_ROWS, MIN_COLS));
        assert_eq!(screen_size(1, 80), (MIN_ROWS, 80));
        assert_eq!(screen_size(29, 100), (29, 100));
        for (rows, cols) in [(0, 0), (1, 1), (2, 1), (1, 2), (24, 1), (1, 80)] {
            let (rows, cols) = screen_size(rows, cols);
            feed(&mut new_parser(rows, cols));
        }
    }

    // ---- a real PTY, with the real bin/nick-agent and the fakes of bin/tests/fixtures/bin ----

    /// bin/nick-agent and the fakes of the checkout.
    fn repo(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel)
    }

    /// A project with the jail off (no ai-jail) and an empty HOME, under a temporary root.
    struct Case {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        /// The directory of the project under the root.
        project: &'static str,
    }

    impl Case {
        fn new() -> Case {
            Case::named("proj")
        }

        /// A case whose project directory is named `project`.
        fn named(project: &'static str) -> Case {
            let tmp = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(tmp.path()).unwrap();
            fs::create_dir_all(root.join(project).join(".nick")).unwrap();
            fs::create_dir_all(root.join("home")).unwrap();
            fs::write(
                root.join(project).join(".nick/nick.conf"),
                "JAIL_ENABLED=\"false\"\n",
            )
            .unwrap();
            // The prompt of the launch (out of the project), as --render would leave it.
            fs::write(root.join("prompt.md"), PROMPT_TEXT).unwrap();
            Case {
                _tmp: tmp,
                root,
                project,
            }
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.root.join(rel)
        }

        fn spawn(
            &self,
            mode: &str,
            extra: &[(&str, &str)],
            size: (u16, u16),
        ) -> (AgentSession, mpsc::Receiver<AgentEvent>) {
            let (tx, rx) = mpsc::channel();
            (self.spawn_on(AGENT_ID, mode, extra, size, tx), rx)
        }

        /// `spawn` as the agent `agent_id`, with the events sent to `tx` (several sessions may
        /// share one channel, as in the loop of the nick).
        fn spawn_on(
            &self,
            agent_id: i64,
            mode: &str,
            extra: &[(&str, &str)],
            size: (u16, u16),
            tx: Sender<AgentEvent>,
        ) -> AgentSession {
            let prompt = self.path("prompt.md");
            let launch = Launch::New {
                kind: StageType::PoDiscussion,
                prompt_file: &prompt,
            };
            self.spawn_launch(agent_id, mode, extra, size, tx, launch)
        }

        /// `spawn_on` with the launch of the agent (NS-39: a new one or a resume).
        fn spawn_launch(
            &self,
            agent_id: i64,
            mode: &str,
            extra: &[(&str, &str)],
            size: (u16, u16),
            tx: Sender<AgentEvent>,
            launch: Launch,
        ) -> AgentSession {
            let path = format!(
                "{}:{}",
                repo("bin/tests/fixtures/bin").display(),
                std::env::var("PATH").unwrap_or_default()
            );
            let home = self.path("home").display().to_string();
            let log = self.path("claude.argv").display().to_string();
            let envlog = self.path("claude.env").display().to_string();
            let pids = self.path("pids").display().to_string();
            let mut env = vec![
                ("PATH", path.as_str()),
                ("HOME", home.as_str()),
                ("FAKE_CLAUDE_MODE", mode),
                ("FAKE_CLAUDE_LOG", log.as_str()),
                ("FAKE_CLAUDE_ENV", envlog.as_str()),
                ("FAKE_CLAUDE_PIDS", pids.as_str()),
            ];
            env.extend_from_slice(extra);
            AgentSession::spawn_with_env(
                &repo("bin/nick-agent"),
                &self.path(self.project),
                agent_id,
                "0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a60",
                "proj · Dev · NS-1",
                launch,
                size,
                &env,
                tx,
            )
            .expect("spawn the agent")
        }

        fn pids(&self) -> Vec<i32> {
            fs::read_to_string(self.path("pids"))
                .unwrap()
                .split_whitespace()
                .map(|p| p.parse().unwrap())
                .collect()
        }
    }

    /// The id of the agent of `Case::spawn`.
    const AGENT_ID: i64 = 42;

    /// The prompt of the launches: two lines and line breaks at the end, which the launcher
    /// drops ($(cat ...)).
    const PROMPT_TEXT: &str = "Prompt de teste\ncom duas linhas.\n\n";

    /// The events of `rx` so far, all of them from `AGENT_ID`.
    fn session_events(rx: &mpsc::Receiver<AgentEvent>) -> Vec<SessionEvent> {
        rx.try_iter()
            .map(|e| {
                assert_eq!(e.agent, AGENT_ID, "{e:?}");
                e.event
            })
            .collect()
    }

    fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            thread::sleep(Duration::from_millis(20));
        }
        cond()
    }

    fn contents(session: &AgentSession) -> String {
        session.with_screen(|screen| screen.contents())
    }

    fn wait_for_text(session: &AgentSession, text: &str) {
        let found = wait_until(Duration::from_secs(10), || contents(session).contains(text));
        assert!(found, "{text:?} never showed up:\n{}", contents(session));
    }

    fn wait_exit(session: &AgentSession) -> u32 {
        assert!(
            wait_until(Duration::from_secs(10), || session.exit_code().is_some()),
            "the agent never ended:\n{}",
            contents(session)
        );
        session.exit_code().unwrap()
    }

    fn alive(pid: i32) -> bool {
        nix::sys::signal::kill(Pid::from_raw(pid), None).is_ok()
    }

    #[test]
    fn the_launcher_runs_claude_with_the_session_id_and_the_name_in_the_project() {
        let case = Case::new();
        let (mut session, rx) = case.spawn("", &[], (24, 80));
        assert_eq!(wait_exit(&session), 0);
        session.wait();
        wait_for_text(&session, "OK");
        let argv = fs::read_to_string(case.path("claude.argv")).expect("the fake claude ran");
        assert!(argv.starts_with("--settings\n{"), "{argv}");
        // The session id and the name, then the prompt of the file as the last argument.
        assert!(
            argv.ends_with(
                "--session-id\n0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a60\n--name\nproj · Dev · NS-1\n\
                 Prompt de teste\ncom duas linhas.\n"
            ),
            "{argv}"
        );
        let env = fs::read_to_string(case.path("claude.env")).unwrap();
        assert!(env.lines().any(|l| l == "TERM=xterm-256color"), "{env}");
        assert!(env.lines().any(|l| l == "COLORTERM=truecolor"), "{env}");
        let pwd = format!("PWD={}", case.path("proj").display());
        assert!(env.lines().any(|l| l == pwd), "{env}");
        let events = session_events(&rx);
        assert!(events.contains(&SessionEvent::Output), "{events:?}");
        assert_eq!(events.last(), Some(&SessionEvent::Exited(0)), "{events:?}");
    }

    #[test]
    fn the_output_reaches_the_parser_and_the_exit_code_is_kept() {
        let case = Case::new();
        let (session, _rx) = case.spawn("exit", &[("FAKE_CLAUDE_EXIT", "3")], (24, 80));
        assert_eq!(wait_exit(&session), 3);
        wait_for_text(&session, "BYE");
        wait_for_text(&session, "Jail (ai-jail): desligada");
    }

    #[test]
    fn writes_reach_the_child() {
        let case = Case::new();
        let (mut session, _rx) = case.spawn("echo", &[("FAKE_CLAUDE_READ_BYTES", "6")], (24, 80));
        wait_for_text(&session, "READY");
        session.write(b"\x03ola\r").unwrap();
        session.write("ç".as_bytes()).unwrap();
        wait_for_text(&session, "GOT 03 6f 6c 61 0d c3");
        wait_exit(&session);
    }

    #[test]
    fn a_flood_ends_without_anyone_reading_the_screen() {
        // Only the reader thread drains the PTY while the child writes ~1 MB: without it, the
        // child would block on write and never end.
        let case = Case::new();
        let (session, _rx) = case.spawn("flood", &[], (24, 80));
        assert_eq!(wait_exit(&session), 0);
        wait_for_text(&session, "FIM-DO-FLOOD");
    }

    #[test]
    fn resize_reaches_the_child_and_the_parser() {
        let case = Case::new();
        let (mut session, _rx) = case.spawn("winsize", &[], (24, 80));
        wait_for_text(&session, "SIZE 24 80");
        session.resize(30, 100).unwrap();
        assert_eq!(session.with_screen(|s| s.size()), (30, 100));
        wait_for_text(&session, "SIZE 30 100");
        session.resize(1, 1).unwrap();
        assert_eq!(session.with_screen(|s| s.size()), (MIN_ROWS, MIN_COLS));
        session.terminate();
    }

    #[test]
    fn a_tiny_size_at_spawn_is_raised_to_2x2() {
        let case = Case::new();
        let (mut session, _rx) = case.spawn("winsize", &[], (0, 1));
        // The text wraps on a 2-column screen: the size is read from the PTY and the parser.
        let pty = session.master.get_size().unwrap();
        assert_eq!((pty.rows, pty.cols), (MIN_ROWS, MIN_COLS));
        assert_eq!(session.with_screen(|s| s.size()), (MIN_ROWS, MIN_COLS));
        // The end of "SIZE 2 2" is what fits on the last row.
        wait_for_text(&session, " 2");
        session.terminate();
    }

    fn assert_terminate_ends_the_group(extra: &[(&str, &str)]) {
        let case = Case::new();
        let (mut session, _rx) = case.spawn("sleep", extra, (24, 80));
        wait_for_text(&session, "READY");
        let pids = case.pids();
        assert_eq!(pids.len(), 2);
        assert_eq!(
            session.pid(),
            Some(pids[0] as u32),
            "the launcher execs claude"
        );
        assert!(pids.iter().all(|p| alive(*p)), "control: both alive before");
        session.terminate();
        assert!(session.exit_code().is_some(), "reaped");
        for pid in pids {
            assert!(
                wait_until(Duration::from_secs(3), || !alive(pid)),
                "pid {pid} survived terminate()"
            );
        }
    }

    #[test]
    fn terminate_ends_the_whole_process_group_with_sigterm() {
        let started = Instant::now();
        assert_terminate_ends_the_group(&[]);
        assert!(started.elapsed() < TERM_GRACE, "no SIGKILL was needed");
    }

    #[test]
    fn terminate_kills_a_group_that_ignores_sigterm() {
        let started = Instant::now();
        assert_terminate_ends_the_group(&[("FAKE_CLAUDE_IGNORE_TERM", "1")]);
        assert!(
            started.elapsed() >= TERM_GRACE,
            "the SIGKILL came after the grace"
        );
    }

    #[test]
    fn terminate_leaves_the_group_of_a_reaped_child_alone() {
        // Once the child is reaped, its pid may go to an unrelated group: terminate must not
        // signal it any more. The leftover here is the fake's own background sleep (it ignores
        // SIGHUP), which signaling the group would kill.
        let case = Case::new();
        let (mut session, _rx) = case.spawn("sleep", &[], (24, 80));
        wait_for_text(&session, "READY");
        let pids = case.pids();
        let (leader, leftover) = (pids[0], pids[1]);
        nix::sys::signal::kill(Pid::from_raw(leader), Signal::SIGKILL).unwrap();
        wait_exit(&session);
        assert!(alive(leftover), "control: the leftover outlives the leader");
        session.terminate();
        let survived = alive(leftover);
        let _ = nix::sys::signal::kill(Pid::from_raw(leftover), Signal::SIGKILL);
        assert!(survived, "terminate signaled the group of a reaped child");
    }

    #[test]
    fn a_copy_comes_out_as_an_event() {
        let case = Case::new();
        let (mut session, rx) = case.spawn("osc52", &[], (24, 80));
        wait_exit(&session);
        session.wait();
        wait_for_text(&session, "COPIADO");
        let events = session_events(&rx);
        assert!(
            events.contains(&SessionEvent::Clipboard(b"\x1b]52;c;b2zDoQ==\x07".to_vec())),
            "{events:?}"
        );
    }

    #[test]
    fn scrolling_moves_through_the_history_and_back() {
        let case = Case::new();
        let (session, _rx) = case.spawn("flood", &[], (24, 80));
        wait_exit(&session);
        wait_for_text(&session, "FIM-DO-FLOOD");
        session.scroll(5);
        assert_eq!(session.with_screen(|s| s.scrollback()), 5);
        session.scroll(-2);
        assert_eq!(session.with_screen(|s| s.scrollback()), 3);
        session.scroll(-100);
        assert_eq!(session.with_screen(|s| s.scrollback()), 0);
        session.scroll(4);
        session.scroll_to_bottom();
        assert_eq!(session.with_screen(|s| s.scrollback()), 0);
    }

    // ---- several sessions at once (NS-34) ----

    #[test]
    fn two_sessions_tell_their_events_apart_and_a_flood_does_not_block_the_other() {
        let (a, b) = (Case::new(), Case::new());
        let (tx, rx) = mpsc::channel();
        let mut echo = a.spawn_on(
            7,
            "echo",
            &[("FAKE_CLAUDE_READ_BYTES", "2")],
            (24, 80),
            tx.clone(),
        );
        // Nobody reads the channel nor the screen of the flood while it runs.
        let flood = b.spawn_on(9, "flood", &[], (24, 80), tx);
        wait_for_text(&echo, "READY");
        echo.write(b"ab").unwrap();
        wait_for_text(&echo, "GOT 61 62");
        assert_eq!(wait_exit(&flood), 0);
        wait_for_text(&flood, "FIM-DO-FLOOD");
        assert_eq!(wait_exit(&echo), 0);
        echo.wait();
        assert!(!contents(&echo).contains("xxx"), "{}", contents(&echo));
        assert!(!contents(&echo).contains("FIM-DO-FLOOD"));
        assert!(!contents(&flood).contains("READY"));
        let events: Vec<AgentEvent> = rx.try_iter().collect();
        for id in [7, 9] {
            let mine: Vec<&SessionEvent> = events
                .iter()
                .filter(|e| e.agent == id)
                .map(|e| &e.event)
                .collect();
            assert!(mine.contains(&&SessionEvent::Output), "{id}: {mine:?}");
            assert_eq!(mine.last(), Some(&&SessionEvent::Exited(0)), "{id}");
        }
        assert!(events.iter().all(|e| [7, 9].contains(&e.agent)));
    }

    #[test]
    fn terminate_all_ends_every_group_with_a_single_grace() {
        let (a, b) = (Case::new(), Case::new());
        let ignore = [("FAKE_CLAUDE_IGNORE_TERM", "1")];
        let (first, _rx1) = a.spawn("sleep", &ignore, (24, 80));
        let (second, _rx2) = b.spawn("sleep", &ignore, (24, 80));
        wait_for_text(&first, "READY");
        wait_for_text(&second, "READY");
        let pids: Vec<i32> = a.pids().into_iter().chain(b.pids()).collect();
        assert_eq!(pids.len(), 4);
        assert!(pids.iter().all(|p| alive(*p)), "control: all alive");
        let started = Instant::now();
        let mut sessions = [first, second];
        terminate_all(&mut sessions);
        let took = started.elapsed();
        assert!(took >= TERM_GRACE, "the SIGKILL came after the grace");
        assert!(took < TERM_GRACE * 2, "one grace for both: {took:?}");
        assert!(sessions.iter().all(|s| s.exit_code().is_some()), "reaped");
        for pid in pids {
            assert!(
                wait_until(Duration::from_secs(3), || !alive(pid)),
                "pid {pid} survived terminate_all()"
            );
        }
    }

    #[test]
    fn terminate_all_leaves_the_group_of_a_reaped_child_alone() {
        let (a, b) = (Case::new(), Case::new());
        let (reaped, _rx1) = a.spawn("sleep", &[], (24, 80));
        let (running, _rx2) = b.spawn("sleep", &[], (24, 80));
        wait_for_text(&reaped, "READY");
        wait_for_text(&running, "READY");
        let (leader, leftover) = (a.pids()[0], a.pids()[1]);
        nix::sys::signal::kill(Pid::from_raw(leader), Signal::SIGKILL).unwrap();
        wait_exit(&reaped);
        let others = b.pids();
        let mut sessions = [reaped, running];
        terminate_all(&mut sessions);
        let survived = alive(leftover);
        let _ = nix::sys::signal::kill(Pid::from_raw(leftover), Signal::SIGKILL);
        assert!(
            survived,
            "terminate_all signaled the group of a reaped child"
        );
        for pid in others {
            assert!(
                wait_until(Duration::from_secs(3), || !alive(pid)),
                "pid {pid} survived"
            );
        }
    }

    /// NS-57, D8: the session of an agent is ended and another one, with the same id, starts
    /// before the loop reads the end of the first (the `Exited` its waiter sends is already in
    /// the channel when `terminate` returns). That late event is of no session of the loop: taken
    /// for the new one, it would end the agent just resumed.
    #[test]
    fn a_late_event_of_an_ended_session_is_not_the_new_one_s_with_the_same_id() {
        let (a, b) = (Case::new(), Case::new());
        let (tx, rx) = mpsc::channel();
        let mut old = a.spawn_on(7, "sleep", &[], (24, 80), tx.clone());
        wait_for_text(&old, "READY");
        old.terminate();
        let late: Vec<AgentEvent> = rx.try_iter().collect();
        assert!(
            late.iter()
                .any(|e| e.agent == 7 && matches!(e.event, SessionEvent::Exited(_))),
            "control: the end of the old session is in the channel: {late:?}"
        );
        let new = b.spawn_on(7, "sleep", &[], (24, 80), tx);
        wait_for_text(&new, "READY");
        let mut sessions = vec![(7, new)];
        for event in &late {
            assert_eq!(
                event_session(&sessions, event),
                None,
                "an event of the ended session: {event:?}"
            );
        }
        let own: Vec<AgentEvent> = rx.try_iter().collect();
        assert!(!own.is_empty(), "control: the new session sent its output");
        for event in &own {
            assert_eq!(event_session(&sessions, event), Some(0), "{event:?}");
        }
        assert_eq!(sessions[0].1.exit_code(), None, "the new agent still runs");
        sessions[0].1.terminate();
    }

    /// Review NS-34, P3: the fake claude runs in a project whose name is not a variable name
    /// (FAKE_CLAUDE_MODE_<name> only applies to the valid ones), and the mode by project works.
    #[test]
    fn the_fake_claude_runs_in_a_project_whose_name_is_not_a_variable() {
        let case = Case::named("meu-proj.v2");
        let (session, _rx) = case.spawn("exit", &[("FAKE_CLAUDE_EXIT", "0")], (24, 80));
        assert_eq!(wait_exit(&session), 0);
        wait_for_text(&session, "BYE");
        // Control: in a project with a valid name, its own mode wins.
        let case = Case::named("alfa");
        let (session, _rx) = case.spawn("exit", &[("FAKE_CLAUDE_MODE_alfa", "")], (24, 80));
        assert_eq!(wait_exit(&session), 0);
        wait_for_text(&session, "BYE");
        let case = Case::named("alfa");
        let (session, _rx) = case.spawn("exit", &[("FAKE_CLAUDE_MODE_alfa", "osc52")], (24, 80));
        assert_eq!(wait_exit(&session), 0);
        wait_for_text(&session, "COPIADO");
    }

    /// A resume (NS-39): the real launcher gets `--resume <id> --name <name> --stage <stage>
    /// [--opened-id <key>] --ticket-id <key>`, and claude gets `--resume` with no prompt.
    #[test]
    fn a_resume_runs_claude_with_resume_and_no_prompt() {
        let case = Case::new();
        let (tx, rx) = mpsc::channel();
        let launch = Launch::Resume {
            stage: Stage::Development,
            opened_key: Some("NS-1"),
            holder_key: HolderKey::Ticket("NS-9"),
            jail_off: false,
        };
        let mut session = case.spawn_launch(AGENT_ID, "", &[], (24, 80), tx, launch);
        assert_eq!(wait_exit(&session), 0);
        session.wait();
        let argv = fs::read_to_string(case.path("claude.argv")).expect("the fake claude ran");
        assert!(argv.starts_with("--settings\n{"), "{argv}");
        assert!(
            argv.ends_with(
                "--resume\n0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a60\n--name\nproj · Dev · NS-1\n"
            ),
            "no prompt after the name: {argv}"
        );
        // The auto mode of the Dev stage (the launcher's; no Rust names its flag).
        assert!(argv.contains("\nauto\n"), "the Dev stage: {argv}");
        assert!(
            argv.contains(".nick/docs/NS-9/"),
            "the notice of the new ID: {argv}"
        );
        assert!(!argv.contains("--session-id"), "{argv}");
        let env = fs::read_to_string(case.path("claude.env")).unwrap();
        let pwd = format!("PWD={}", case.path("proj").display());
        assert!(env.lines().any(|l| l == pwd), "{env}");
        assert_eq!(session_events(&rx).last(), Some(&SessionEvent::Exited(0)));
    }

    /// Without the key at the opening, no `--opened-id` and no notice of the ID.
    #[test]
    fn a_resume_without_the_key_at_the_opening_has_no_notice() {
        let case = Case::new();
        let (tx, _rx) = mpsc::channel();
        let launch = Launch::Resume {
            stage: Stage::Po,
            opened_key: None,
            holder_key: HolderKey::Ticket("NS-9"),
            jail_off: false,
        };
        let mut session = case.spawn_launch(AGENT_ID, "", &[], (24, 80), tx, launch);
        assert_eq!(wait_exit(&session), 0);
        session.wait();
        let argv = fs::read_to_string(case.path("claude.argv")).unwrap();
        // NS-41: the launcher always sends one --append-system-prompt (the instruction about the
        // permission rules), but with no notice of the ID: no folder of a ticket in it.
        assert!(argv.contains("--append-system-prompt\n"), "{argv}");
        assert!(!argv.contains(".nick/docs/"), "{argv}");
        assert!(
            !argv.contains("\nauto\n"),
            "no auto mode out of the Dev stage: {argv}"
        );
    }

    /// NS-40: the resume of an agent of an epic, by the real launcher: `--epic-id`, and the notice
    /// of the new ID of an epic.
    #[test]
    fn a_resume_of_an_epic_agent_tells_the_new_folder_of_the_epic() {
        let case = Case::new();
        let (tx, _rx) = mpsc::channel();
        let launch = Launch::Resume {
            stage: Stage::TechLeader,
            opened_key: Some("EPIC-1"),
            holder_key: HolderKey::Epic("EPIC-2"),
            jail_off: false,
        };
        let mut session = case.spawn_launch(AGENT_ID, "", &[], (24, 80), tx, launch);
        assert_eq!(wait_exit(&session), 0);
        session.wait();
        let argv = fs::read_to_string(case.path("claude.argv")).unwrap();
        assert!(
            argv.contains("A pasta do épico agora é .nick/docs/EPIC-2/"),
            "{argv}"
        );
        assert!(!argv.contains("deste ticket"), "{argv}");
        assert!(
            argv.ends_with("--name\nproj · Dev · NS-1\n"),
            "no prompt: {argv}"
        );
    }

    /// NS-57: a resume out of the jail reaches the real launcher with `--no-jail`. The project has
    /// the jail on (the fake ai-jail of the fixtures, in front of the PATH): the check of the jail
    /// runs, and then claude runs directly, without the notice of the jail. The control, without
    /// `jail_off`, goes through the fake ai-jail with the notice.
    #[test]
    fn a_resume_out_of_the_jail_passes_no_jail_to_the_launcher() {
        for jail_off in [true, false] {
            let case = Case::new();
            fs::write(case.path("proj/.nick/nick.conf"), "JAIL_ENABLED=\"true\"\n").unwrap();
            let jail_log = case.path("jail.argv").display().to_string();
            let (tx, _rx) = mpsc::channel();
            let launch = Launch::Resume {
                stage: Stage::Po,
                opened_key: None,
                holder_key: HolderKey::Ticket("NS-9"),
                jail_off,
            };
            let extra = [("FAKE_JAIL_LOG", jail_log.as_str())];
            let mut session = case.spawn_launch(AGENT_ID, "", &extra, (24, 80), tx, launch);
            assert_eq!(wait_exit(&session), 0, "{jail_off}: {}", contents(&session));
            session.wait();
            let argv = fs::read_to_string(case.path("claude.argv")).expect("the fake claude ran");
            if jail_off {
                wait_for_text(&session, "Jail (ai-jail): desligada");
                assert!(!case.path("jail.argv").exists(), "no launch by the ai-jail");
                assert!(!argv.contains("ai-jail"), "no notice of the jail: {argv}");
            } else {
                wait_for_text(&session, "Jail (ai-jail): ativa");
                assert!(case.path("jail.argv").exists(), "control: by the ai-jail");
                assert!(
                    argv.contains("ai-jail"),
                    "control: the notice of the jail: {argv}"
                );
            }
            assert!(
                argv.ends_with(
                    "--resume\n0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a60\n--name\nproj · Dev · NS-1\n"
                ),
                "{argv}"
            );
        }
    }
}
