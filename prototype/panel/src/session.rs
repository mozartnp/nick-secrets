//! One session of the panel: a PTY running `bash launch_session.sh` (which runs claude inside
//! the ai-jail, through lib/), a vt100 parser that emulates its terminal, a reader thread that
//! drains the PTY all the time (also while the session is not on screen: an unread PTY fills
//! up and blocks the child on write) and a waiter thread that reaps the child.
//!
//! The only command ever launched is `bash <absolute path of launch_session.sh>`: the panel
//! never builds the ai-jail argv nor runs claude itself (tests/pty_launch.rs checks the
//! sources for that).

use std::ffi::OsString;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use nix::sys::signal::{Signal, killpg};
use nix::unistd::Pid;
use portable_pty::{CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::app::SessionState;

/// Lines of scrollback kept by each session's parser.
pub const SCROLLBACK_LINES: usize = 10_000;
/// How long the sessions get after SIGTERM before SIGKILL.
pub const TERM_GRACE: Duration = Duration::from_secs(2);
/// The smallest session screen vt100 0.16 survives. Below it, a wide character (an emoji or
/// CJK, 2 columns) panics in the parser: at 1 column, `cols - width` overflows; at 1 row, the
/// wrap subtracts the scroll from the row (found probing every size from 1x1 to 3x4).
pub const MIN_ROWS: u16 = 2;
pub const MIN_COLS: u16 = 2;

/// The size a session's screen (parser and PTY) gets for `rows`x`cols`: at least
/// [`MIN_ROWS`]x[`MIN_COLS`]. On a too small window, the screen is cut at the edge.
pub fn screen_size(rows: u16, cols: u16) -> (u16, u16) {
    (rows.max(MIN_ROWS), cols.max(MIN_COLS))
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;

/// The launcher, found from the crate's own directory (CARGO_MANIFEST_DIR, fixed at build
/// time): the prototype runs from its checkout with `cargo run`, so the path holds wherever the
/// binary is and whatever the cwd is (the cwd is the project being worked on).
pub fn launcher_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("launch_session.sh")
}

/// How a session is launched.
#[derive(Clone, Debug)]
pub struct LaunchConfig {
    /// The launcher script, run as `bash <launcher>`.
    pub launcher: PathBuf,
    /// The session's cwd: the panel's own, the project being worked on.
    pub cwd: PathBuf,
    /// Extra environment on top of the panel's (the tests point PATH at the fakes).
    pub env: Vec<(OsString, OsString)>,
}

impl LaunchConfig {
    /// The panel's config: the launcher next to the crate, the panel's cwd, its environment.
    pub fn for_panel() -> io::Result<Self> {
        Ok(LaunchConfig {
            launcher: launcher_path(),
            cwd: std::env::current_dir()?,
            env: Vec::new(),
        })
    }
}

/// What the session threads tell the main loop, with the session's index.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// New output was parsed: redraw.
    Output(usize),
    /// The child was reaped.
    Exited(usize),
    /// The child asked the terminal to copy to the clipboard (OSC 52): the bytes to write to
    /// the outer terminal, which does the copy.
    Clipboard(usize, Vec<u8>),
}

/// The vt100 callbacks of a session: keeps the copy requests (OSC 52) the parser sees, for the
/// reader to hand them to the panel. vt100 only calls `copy_to_clipboard` for a valid selector
/// and base64 data, so what is rebuilt here carries no other escape sequence. Reads of the
/// clipboard (`ESC]52;c;?`) are not forwarded: the program never gets the user's clipboard
/// through the panel. Which copies arrive at all is up to the jail (the `restrito` level
/// filters OSC 52 before it reaches the panel).
#[derive(Default)]
pub struct Clipboard {
    copies: Vec<Vec<u8>>,
}

impl Clipboard {
    /// The copy requests seen since the last call, as OSC 52 sequences (BEL-terminated).
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

type SessionParser = vt100::Parser<Clipboard>;

fn new_parser(rows: u16, cols: u16) -> SessionParser {
    vt100::Parser::new_with_callbacks(rows, cols, SCROLLBACK_LINES, Clipboard::default())
}

/// A session's state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Running,
    /// The exit code (portable-pty reports 1 for a child killed by a signal).
    Exited(u32),
}

/// Process groups of the live sessions, for the panic hook (which has no access to the
/// sessions themselves) to end them too.
static GROUPS: Mutex<Vec<i32>> = Mutex::new(Vec::new());

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub struct Session {
    pub label: String,
    parser: Arc<Mutex<SessionParser>>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    pid: Option<u32>,
    status: Arc<Mutex<Status>>,
    waiter: Option<JoinHandle<()>>,
}

impl Session {
    /// Opens a PTY of `rows`x`cols` and runs `bash <launcher>` in it, with the cwd and the extra
    /// environment of `config`, plus TERM=xterm-256color and COLORTERM=truecolor. Events go to
    /// `tx` tagged with `id`.
    pub fn spawn<E: From<SessionEvent> + Send + 'static>(
        id: usize,
        label: &str,
        config: &LaunchConfig,
        rows: u16,
        cols: u16,
        tx: Sender<E>,
    ) -> Result<Session, Error> {
        let (rows, cols) = screen_size(rows, cols);
        let pair = native_pty_system().openpty(pty_size(rows, cols))?;
        let mut command = CommandBuilder::new("bash");
        command.arg(&config.launcher);
        command.cwd(&config.cwd);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        for (key, value) in &config.env {
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
                        // The panel may be gone (tests): keep draining anyway.
                        for copy in copies {
                            let _ = reader_tx.send(SessionEvent::Clipboard(id, copy).into());
                        }
                        let _ = reader_tx.send(SessionEvent::Output(id).into());
                    }
                }
            }
        });

        let status = Arc::new(Mutex::new(Status::Running));
        let waiter_status = Arc::clone(&status);
        let waiter = thread::spawn(move || {
            let code = child.wait().map_or(1, |s| s.exit_code());
            *lock(&waiter_status) = Status::Exited(code);
            if let Some(pid) = pid {
                lock(&GROUPS).retain(|g| *g != pid as i32);
            }
            let _ = tx.send(SessionEvent::Exited(id).into());
        });

        Ok(Session {
            label: label.to_string(),
            parser,
            master: pair.master,
            writer,
            pid,
            status,
            waiter: Some(waiter),
        })
    }

    /// The child's pid, which is also its process group (the PTY child is a session leader;
    /// the launcher and ai-jail exec, so it ends up being claude's pid).
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    pub fn status(&self) -> Status {
        *lock(&self.status)
    }

    /// Writes input to the child.
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.writer.write_all(bytes)?;
        self.writer.flush()
    }

    /// Resizes the PTY (the child gets SIGWINCH) and the parser.
    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<(), Error> {
        let (rows, cols) = screen_size(rows, cols);
        lock(&self.parser).screen_mut().set_size(rows, cols);
        self.master.resize(pty_size(rows, cols))?;
        Ok(())
    }

    /// Runs `f` with the session's screen (as the parser has it now).
    pub fn with_screen<R>(&self, f: impl FnOnce(&vt100::Screen) -> R) -> R {
        f(lock(&self.parser).screen())
    }

    /// Scrolls the scrollback by `lines` (positive: up, into the history); clamped by vt100.
    pub fn scroll(&self, lines: i32) {
        let mut parser = lock(&self.parser);
        let screen = parser.screen_mut();
        let current = screen.scrollback() as i64;
        screen.set_scrollback((current + i64::from(lines)).max(0) as usize);
    }

    /// Back to the live screen (no scrollback offset).
    pub fn scroll_to_bottom(&self) {
        lock(&self.parser).screen_mut().set_scrollback(0);
    }

    /// Ends this session's process group (see [`terminate_all`]).
    pub fn terminate(&mut self) {
        terminate_all(std::slice::from_mut(self));
    }
}

impl SessionState for Session {
    fn is_running(&self) -> bool {
        self.status() == Status::Running
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
    for &g in groups {
        let _ = killpg(Pid::from_raw(g), Signal::SIGTERM);
    }
    let deadline = Instant::now() + TERM_GRACE;
    while Instant::now() < deadline && groups.iter().any(|&g| group_alive(g)) {
        thread::sleep(Duration::from_millis(20));
    }
    for &g in groups {
        if group_alive(g) {
            let _ = killpg(Pid::from_raw(g), Signal::SIGKILL);
        }
    }
}

/// Ends every session: SIGTERM to each child's process group (not only the child: whatever it
/// started in its group goes too), SIGKILL after [`TERM_GRACE`] to the groups still alive,
/// then waits for each child to be reaped.
///
/// Only the groups still registered (children not reaped yet) are signaled, as in
/// [`terminate_registered`]: once a child is reaped, its pid may go to an unrelated group
/// leader (a job of the user's shell), which must not get SIGTERM/SIGKILL. The cost: what a
/// session that already ended left in its group is not ended by the panel.
pub fn terminate_all(sessions: &mut [Session]) {
    let registered = lock(&GROUPS).clone();
    let groups: Vec<i32> = sessions
        .iter()
        .filter_map(|s| s.pid.map(|p| p as i32))
        .filter(|g| registered.contains(g))
        .collect();
    end_groups(&groups);
    for session in sessions.iter_mut() {
        if let Some(waiter) = session.waiter.take() {
            let _ = waiter.join();
        }
    }
}

/// Ends the groups of every live session from anywhere (the panic hook): the same signals as
/// [`terminate_all`], without the sessions.
pub fn terminate_registered() {
    let groups = lock(&GROUPS).clone();
    end_groups(&groups);
}

#[cfg(test)]
mod tests {
    use super::*;

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
        // ST-terminated too, forwarded as BEL-terminated (both are accepted by terminals).
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
    fn a_tiny_screen_still_takes_wide_characters() {
        // A wide character panics vt100 at 1 column or 1 row (controls). A window under 33
        // columns, or just a few rows tall, gives the session that 1 column or row.
        let texts = [
            "⚠️ a ⚠️ b 中文",
            "中中中中中中\r\n中a中a中",
            "\x1b[999C中中\r\n\x1b[2;1H中",
        ];
        let feed = |parser: &mut SessionParser| {
            for _ in 0..3 {
                for text in texts {
                    parser.process(text.as_bytes());
                }
            }
        };
        for (rows, cols) in [(1, 1), (2, 1), (1, 2)] {
            let raw = std::panic::catch_unwind(|| feed(&mut new_parser(rows, cols)));
            assert!(raw.is_err(), "control: vt100 panics at {rows}x{cols}");
        }
        assert_eq!(screen_size(0, 0), (MIN_ROWS, MIN_COLS));
        assert_eq!(screen_size(1, 1), (MIN_ROWS, MIN_COLS));
        assert_eq!(screen_size(30, 100), (30, 100));
        for (rows, cols) in [(0, 0), (1, 1), (2, 1), (1, 2), (24, 1), (1, 80)] {
            let (rows, cols) = screen_size(rows, cols);
            feed(&mut new_parser(rows, cols));
        }
    }

    #[test]
    fn only_copies_are_forwarded() {
        // A read of the clipboard (the program would get the user's clipboard back) and
        // anything vt100 does not accept as a copy (bad selector, data that isn't base64).
        assert!(copies(b"\x1b]52;c;?\x07").is_empty());
        assert!(copies(b"\x1b]52;x;aGk=\x07").is_empty());
        assert!(copies(b"\x1b]52;c;a b!\x07").is_empty());
        assert!(copies(b"\x1b]2;titulo\x07").is_empty());
        // An ESC inside ends the OSC there: what is forwarded is only the clean part, never the
        // escape sequence that came after it.
        assert_eq!(
            copies(b"\x1b]52;c;aGk=\x1b[31m\x07"),
            vec![b"\x1b]52;c;aGk=\x07".to_vec()]
        );
    }
}
