//! Helpers of the integration tests: the real `nick` binary in a PTY (30x100), with a clean
//! environment (only PATH, TERM and what the case passes: never the operator's HOME nor
//! state directory), its output collected by a reader thread and read back as a screen
//! through `vt100`.

#![allow(dead_code)]

use std::fs;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

pub mod snapshot;

pub use snapshot::snapshot;

pub const ROWS: u16 = 30;
pub const COLS: u16 = 100;

/// How long a case waits for a text on the screen or for the exit.
pub const TIMEOUT: Duration = Duration::from_secs(15);

pub const ENTER: &str = "\r";
pub const ESC: &str = "\x1b";
pub const LEFT: &str = "\x1b[D";
pub const DOWN: &str = "\x1b[B";

/// The fake bin/nick-init (tests/fixtures/bin/nick-init).
pub fn fake_init() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/nick-init")
}

/// The temporary world of a case: HOME, the XDG state directory and the marker of the fake
/// init, under one canonical root that is removed at the end.
pub struct World {
    _tmp: tempfile::TempDir,
    pub root: PathBuf,
}

impl World {
    pub fn new() -> World {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        fs::create_dir_all(root.join("home")).unwrap();
        World { _tmp: tmp, root }
    }

    pub fn home(&self) -> PathBuf {
        self.root.join("home")
    }

    pub fn state(&self) -> PathBuf {
        self.root.join("state")
    }

    /// The database with XDG_STATE_HOME set (the default of `spawn`).
    pub fn db_path(&self) -> PathBuf {
        self.state().join("nick/nick.db")
    }

    pub fn marker(&self) -> PathBuf {
        self.root.join("init-marker")
    }

    /// A directory under the root, with `.nick/nick.conf` when `config`.
    pub fn dir(&self, rel: &str, config: bool) -> PathBuf {
        let dir = self.root.join(rel);
        fs::create_dir_all(&dir).unwrap();
        if config {
            fs::create_dir_all(dir.join(".nick")).unwrap();
            fs::write(dir.join(".nick/nick.conf"), "STACK_DESCRIPTION=\"\"\n").unwrap();
        }
        dir
    }

    /// Registers `dir` in the database the binary will use, through the library.
    pub fn register(&self, dir: &Path) {
        nick::paths::create_state_dir(self.db_path().parent().unwrap()).unwrap();
        let db = nick::db::Db::open(&self.db_path()).unwrap();
        let canonical = fs::canonicalize(dir).unwrap();
        nick::projects::register(&db, nick::modules::ModuleKind::AutoScrum, &canonical).unwrap();
    }

    /// The registered paths, read from the database.
    pub fn stored(&self) -> Vec<PathBuf> {
        if !self.db_path().exists() {
            return Vec::new();
        }
        nick::db::Db::open(&self.db_path())
            .unwrap()
            .list_projects("auto_scrum")
            .unwrap()
            .into_iter()
            .map(|p| p.path)
            .collect()
    }

    /// The default environment of a case: HOME, XDG_STATE_HOME, the fake init and its marker.
    pub fn env(&self) -> Vec<(String, String)> {
        vec![
            ("HOME".into(), self.home().display().to_string()),
            ("XDG_STATE_HOME".into(), self.state().display().to_string()),
            ("NICK_INIT_BIN".into(), fake_init().display().to_string()),
            (
                "FAKE_INIT_MARKER".into(),
                self.marker().display().to_string(),
            ),
        ]
    }

    /// Runs the nick with the default environment, plus `extra`, at `cwd`.
    pub fn spawn(&self, cwd: &Path, extra: &[(&str, &str)]) -> Nick {
        let mut env = self.env();
        for (key, value) in extra {
            env.retain(|(k, _)| k != key);
            env.push(((*key).into(), (*value).into()));
        }
        Nick::spawn(cwd, &env)
    }
}

/// The nick running in a PTY.
pub struct Nick {
    child: Box<dyn Child + Send + Sync>,
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    output: Arc<Mutex<Vec<u8>>>,
    code: Option<u32>,
}

impl Nick {
    /// Starts the binary at `cwd` with only PATH, TERM and `env` in the environment.
    pub fn spawn(cwd: &Path, env: &[(String, String)]) -> Nick {
        let pair = native_pty_system()
            .openpty(PtySize {
                rows: ROWS,
                cols: COLS,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_nick"));
        command.env_clear();
        command.cwd(cwd);
        command.env("PATH", std::env::var_os("PATH").unwrap_or_default());
        command.env("TERM", "xterm-256color");
        for (key, value) in env {
            command.env(key, value);
        }
        let child = pair.slave.spawn_command(command).unwrap();
        drop(pair.slave);

        let output = Arc::new(Mutex::new(Vec::new()));
        let writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let mut reader = pair.master.try_clone_reader().unwrap();
        let sink = Arc::clone(&output);
        let answer = Arc::clone(&writer);
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            let mut answered = 0;
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut out = sink.lock().unwrap();
                out.extend_from_slice(&buf[..n]);
                // Like a terminal, answer each cursor position request (ESC[6n): ratatui's
                // Terminal::clear asks for it when the TUI comes back from the init.
                let asked = out.windows(4).filter(|w| w == b"\x1b[6n").count();
                while answered < asked {
                    let mut writer = answer.lock().unwrap();
                    let _ = writer.write_all(b"\x1b[1;1R");
                    let _ = writer.flush();
                    answered += 1;
                }
            }
        });
        Nick {
            child,
            master: pair.master,
            writer,
            output,
            code: None,
        }
    }

    /// Everything the nick wrote so far (stdout and stderr: both go to the PTY).
    pub fn raw(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    fn parser(&self) -> vt100::Parser {
        let mut parser = vt100::Parser::new(ROWS, COLS, 0);
        parser.process(&self.output.lock().unwrap());
        parser
    }

    /// The text on the screen now.
    pub fn screen(&self) -> String {
        self.parser().screen().contents()
    }

    /// True while the alternate screen (the TUI) is on.
    pub fn on_alternate_screen(&self) -> bool {
        self.parser().screen().alternate_screen()
    }

    /// Types `keys`.
    pub fn send(&mut self, keys: &str) {
        let mut writer = self.writer.lock().unwrap();
        writer.write_all(keys.as_bytes()).unwrap();
        writer.flush().unwrap();
    }

    /// Waits until `needle` is on the screen; panics with the screen after the timeout.
    pub fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if self.screen().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("{needle:?} never showed up. Screen:\n{}", self.screen());
    }

    /// Waits until `needle` is in the raw output.
    pub fn wait_for_raw(&self, needle: &str) {
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if self.raw().contains(needle) {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("{needle:?} never in the output:\n{:?}", self.raw());
    }

    /// Waits for the exit and returns the code (None if it never ended: then it is killed).
    pub fn wait_exit(&mut self) -> Option<u32> {
        if self.code.is_some() {
            return self.code;
        }
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                // Let the reader drain what is left.
                std::thread::sleep(Duration::from_millis(200));
                self.code = Some(status.exit_code());
                return self.code;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        None
    }

    /// The size of the PTY (keeps `master` used).
    pub fn size(&self) -> PtySize {
        self.master.get_size().unwrap()
    }
}

impl Drop for Nick {
    fn drop(&mut self) {
        if self.code.is_none() && self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}
