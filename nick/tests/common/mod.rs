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
/// The first column of the panel of an agent at 100 columns (the list takes 40: ui.rs,
/// `LIST_WIDTH`).
pub const PANEL_COL: u16 = 40;
/// The sides of the border of a panel beside an agent (NS-37): thick with the focus, plain
/// without it.
const PANEL_SIDES: [&str; 2] = ["┃", "│"];

/// How long a case waits for a text on the screen or for the exit.
pub const TIMEOUT: Duration = Duration::from_secs(15);

pub const ENTER: &str = "\r";
pub const ESC: &str = "\x1b";
pub const LEFT: &str = "\x1b[D";
pub const DOWN: &str = "\x1b[B";
pub const TAB: &str = "\t";

/// The fake bin/nick-init (tests/fixtures/bin/nick-init).
pub fn fake_init() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/nick-init")
}

/// The fake claude and ai-jail of the bin/nick-agent tests (bin/tests/fixtures/bin), the same
/// ones the bash suite of the launcher uses.
pub fn fake_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../bin/tests/fixtures/bin")
}

/// A PATH with the fakes first, then the PATH of whoever runs the tests.
pub fn fake_path() -> String {
    format!(
        "{}:{}",
        fake_bin().display(),
        std::env::var("PATH").unwrap_or_default()
    )
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

    /// A project directory with a complete `.nick/nick.conf` (every key empty, so no warning
    /// of a missing key) plus `lines`.
    pub fn project(&self, rel: &str, lines: &[&str]) -> PathBuf {
        let dir = self.dir(rel, false);
        fs::create_dir_all(dir.join(".nick")).unwrap();
        let keys = std::process::Command::new("bash")
            .arg("-c")
            .arg(
                "source lib/config/config.sh; source lib/config/keys.sh; \
                 config_register_nick_keys; printf '%s\\n' \"${CONFIG_KEYS[@]}\"",
            )
            .current_dir(Path::new(env!("CARGO_MANIFEST_DIR")).join(".."))
            .output()
            .unwrap();
        let mut conf = String::new();
        for key in String::from_utf8(keys.stdout).unwrap().lines() {
            if !lines.iter().any(|l| l.starts_with(&format!("{key}="))) {
                conf.push_str(&format!("{key}=\"\"\n"));
            }
        }
        for line in lines {
            conf.push_str(line);
            conf.push('\n');
        }
        fs::write(dir.join(".nick/nick.conf"), conf).unwrap();
        dir
    }

    /// The directory of the agents' locks the binary will use.
    pub fn agents_dir(&self) -> PathBuf {
        self.state().join("nick/agents")
    }

    /// Every agent in the database, as (session_id, stage, name, state), in order.
    pub fn agents(&self) -> Vec<(String, String, String, String)> {
        if !self.db_path().exists() {
            return Vec::new();
        }
        let db = nick::db::Db::open(&self.db_path()).unwrap();
        let mut stmt = db
            .connection()
            .prepare("SELECT session_id, stage, name, state FROM agents ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    /// The value of the setting `key` in the database (NS-34), None without a row.
    pub fn setting(&self, key: &str) -> Option<String> {
        if !self.db_path().exists() {
            return None;
        }
        nick::db::Db::open(&self.db_path())
            .unwrap()
            .get_setting(key)
            .unwrap()
    }

    /// Stores an `active` agent of the ticket `key` of the project at `dir`, through the library
    /// (as a nick that died would leave it).
    pub fn active_agent(&self, dir: &Path, key: &str, session_id: &str) {
        let db = nick::db::Db::open(&self.db_path()).unwrap();
        let id = self.project_id(&db, dir);
        let ticket = db
            .list_tickets(id, true)
            .unwrap()
            .into_iter()
            .find(|t| t.key == key)
            .unwrap();
        db.insert_agent(ticket.id, "po", session_id, "x").unwrap();
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

    /// The id of the registered project at `dir`.
    fn project_id(&self, db: &nick::db::Db, dir: &Path) -> i64 {
        let canonical = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
        db.list_projects("auto_scrum")
            .unwrap()
            .into_iter()
            .find(|p| p.path == canonical)
            .unwrap_or_else(|| panic!("{} is not registered", dir.display()))
            .id
    }

    /// Stores a ticket of the registered project at `dir`, through the library.
    pub fn ticket(&self, dir: &Path, key: &str, title: &str) {
        let db = nick::db::Db::open(&self.db_path()).unwrap();
        let id = self.project_id(&db, dir);
        nick::tickets::create(&db, id, title, key).unwrap();
    }

    /// Hides the ticket `key` of the registered project at `dir`, through the library.
    pub fn hide_ticket(&self, dir: &Path, key: &str) {
        let db = nick::db::Db::open(&self.db_path()).unwrap();
        let id = self.project_id(&db, dir);
        let ticket = db
            .list_tickets(id, true)
            .unwrap()
            .into_iter()
            .find(|t| t.key == key)
            .unwrap_or_else(|| panic!("{key} is not a ticket of {}", dir.display()));
        nick::tickets::hide(&db, &ticket).unwrap();
    }

    /// Every ticket in the database (any project, hidden included), as (key, title, hidden),
    /// in the order they were created; of the project at `dir` only, when given.
    pub fn tickets(&self, dir: Option<&Path>) -> Vec<(String, String, bool)> {
        let db = nick::db::Db::open(&self.db_path()).unwrap();
        let only = dir.map(|dir| self.project_id(&db, dir));
        let mut stmt = db
            .connection()
            .prepare(
                "SELECT key, title, hidden_at IS NOT NULL FROM tickets \
                 WHERE ?1 IS NULL OR project_id = ?1 ORDER BY id",
            )
            .unwrap();
        stmt.query_map([only], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
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
    /// The size of the PTY now (rows, columns): `resize` changes it.
    size: (u16, u16),
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
            size: (ROWS, COLS),
        }
    }

    /// Everything the nick wrote so far (stdout and stderr: both go to the PTY).
    pub fn raw(&self) -> String {
        String::from_utf8_lossy(&self.output.lock().unwrap()).into_owned()
    }

    fn parser(&self) -> vt100::Parser {
        let mut parser = vt100::Parser::new(self.size.0, self.size.1, 0);
        parser.process(&self.output.lock().unwrap());
        parser
    }

    /// The text on the screen now.
    pub fn screen(&self) -> String {
        self.parser().screen().contents()
    }

    /// Appends the screen now, under `label`, to the file named by `NICK_TEST_SCREENS`, when it
    /// is set (NS-36: the screens of the validation of a ticket); otherwise does nothing.
    pub fn keep_screen(&self, label: &str) {
        let Some(path) = std::env::var_os("NICK_TEST_SCREENS") else {
            return;
        };
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "===== {label} =====\n{}\n", self.screen()).unwrap();
    }

    /// The screen as one line: the dialog borders (and the thick one of the focused list,
    /// NS-37) taken out and the non-empty rows joined by a space, so a text wrapped inside a
    /// dialog can be checked whole.
    pub fn flat_screen(&self) -> String {
        self.screen()
            .lines()
            .map(|l| l.replace(['│', '┃'], "").trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The panel of the shown agent beside the list (NS-34: the columns from `PANEL_COL` on, at
    /// 100 columns) as one line: its non-empty rows trimmed and joined by a space, so a wrapped
    /// header reads whole. Only the inside of its border (NS-37) is read: the rows with a side
    /// of the border on `PANEL_COL` (not the top nor the bottom), without the first and the last
    /// column. So a wrapped header does not read `seu ┃ ┃shell.`, and a rule or a box the
    /// program draws at the ends of a row stays whole.
    pub fn panel(&self) -> String {
        let parser = self.parser();
        let screen = parser.screen();
        let (_, cols) = screen.size();
        let start = PANEL_COL.min(cols);
        let inside = (start + 1).min(cols);
        screen
            .rows(inside, cols.saturating_sub(start).saturating_sub(2))
            .enumerate()
            .filter(|(row, _)| {
                let side = u16::try_from(*row)
                    .ok()
                    .and_then(|row| screen.cell(row, start))
                    .map(|cell| cell.contents());
                side.is_some_and(|side| PANEL_SIDES.contains(&side))
            })
            .map(|(_, row)| row.trim().to_string())
            .filter(|row| !row.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// The symbol and the color of the text of the cell at `row`, `col` of the screen, from
    /// the same cell in a single parse (empty and the default when out of it): the border of the
    /// focused panel is `FOCUS_COLOR` (NS-37). By the cell, and not by the text of `screen`,
    /// so a wide character before `col` does not shift it.
    pub fn cell(&self, row: u16, col: u16) -> (String, vt100::Color) {
        self.parser()
            .screen()
            .cell(row, col)
            .map_or((String::new(), vt100::Color::Default), |cell| {
                (cell.contents().to_string(), cell.fgcolor())
            })
    }

    /// Waits until `needle` is in the panel of the shown agent (`panel`).
    pub fn wait_for_panel(&self, needle: &str) {
        self.wait_until(&format!("{needle:?} in the panel"), || {
            self.panel().contains(needle)
        });
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

    /// Resizes the PTY (the nick gets SIGWINCH), and reads the screen at the new size.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .unwrap();
        self.size = (rows, cols);
    }

    /// The pid of the nick.
    pub fn pid(&self) -> i32 {
        self.child.process_id().expect("the nick has a pid") as i32
    }

    /// Waits until `cond` holds; panics with the screen after the timeout.
    pub fn wait_until(&self, what: &str, mut cond: impl FnMut() -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            if cond() {
                return;
            }
            std::thread::sleep(Duration::from_millis(30));
        }
        panic!("{what} never happened. Screen:\n{}", self.screen());
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
