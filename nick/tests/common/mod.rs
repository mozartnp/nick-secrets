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

use nick::mascot::MASCOT_COLS;
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
pub const UP: &str = "\x1b[A";
pub const RIGHT: &str = "\x1b[C";
pub const TAB: &str = "\t";
/// The columns inside the border of the column on the left (the tree, NS-38: `LIST_WIDTH`).
const TREE_COLS: usize = 38;
/// The half blocks the cat Nick is drawn with, at the bottom of the column on the left (NS-51):
/// taken out of `tree_rows` and `flat_screen`, so her rows read as empty ones.
const MASCOT_BLOCKS: [char; 2] = ['▀', '▄'];

/// The keyboard mode of the kitty protocol on (flag 1) and off (NS-49).
pub const KITTY_ON: &str = "\x1b[>1u";
pub const KITTY_OFF: &str = "\x1b[<1u";
/// The query of the kitty keyboard protocol that the nick sends when it opens (NS-49).
pub const KITTY_QUERY: &str = "\x1b[?u";

/// Asserts that `seq` appears in `raw` in this order, from the byte `from` on, each one after
/// the end of the one before; returns where the last one ends.
pub fn assert_in_order(raw: &str, from: usize, seq: &[&str]) -> usize {
    let mut at = from;
    for needle in seq {
        let found = raw
            .get(at..)
            .and_then(|rest| rest.find(needle))
            .unwrap_or_else(|| panic!("{needle:?} not after the byte {at} in {seq:?}: {raw:?}"));
        at += found + needle.len();
    }
    at
}

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
        db.insert_agent(
            nick::agents::Owner::Ticket(ticket.id),
            "po",
            session_id,
            "x",
            "NS-1",
        )
        .unwrap();
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

    /// Stores an epic of the registered project at `dir`, through the library (NS-40), and puts
    /// the tickets `inside` in it.
    pub fn epic(&self, dir: &Path, key: &str, title: &str, inside: &[&str]) {
        let db = nick::db::Db::open(&self.db_path()).unwrap();
        let id = self.project_id(&db, dir);
        let epic = nick::epics::create(&db, id, title, key).unwrap();
        for ticket in db.list_tickets(id, true).unwrap() {
            if inside.contains(&ticket.key.as_str()) {
                nick::epics::move_ticket(&db, &ticket, Some(&epic)).unwrap();
            }
        }
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
        Nick::spawn(cwd, &self.env_with(extra))
    }

    /// `spawn` on a PTY that answers like a terminal with the kitty keyboard protocol (the
    /// Konsole, NS-49): the nick turns the keyboard mode on.
    pub fn spawn_kitty(&self, cwd: &Path, extra: &[(&str, &str)]) -> Nick {
        Nick::spawn_as(cwd, &self.env_with(extra), Keyboard::Kitty)
    }

    /// The default environment plus `extra` (a key of `extra` replaces the default).
    fn env_with(&self, extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut env = self.env();
        for (key, value) in extra {
            env.retain(|(k, _)| k != key);
            env.push(((*key).into(), (*value).into()));
        }
        env
    }
}

/// The keyboard of the terminal the PTY plays (NS-49).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Keyboard {
    /// Without the kitty keyboard protocol: only the DA1 is answered, so crossterm sees no
    /// support at once (without the answer, every opening would wait 2 s).
    Legacy,
    /// With the kitty keyboard protocol: the `ESC[?u` is answered with `ESC[?0u`, before the
    /// DA1, like the Konsole.
    Kitty,
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
    /// Starts the binary at `cwd` with only PATH, TERM, `NICK_TEST_MASCOT=still` (the cat Nick
    /// drawn still, NS-51: no blink nor "Miau!" in the middle of a reading of the screen) and
    /// `env` in the environment (a `NICK_TEST_MASCOT` of `env` replaces it), on a terminal
    /// without the kitty keyboard protocol.
    pub fn spawn(cwd: &Path, env: &[(String, String)]) -> Nick {
        Nick::spawn_as(cwd, env, Keyboard::Legacy)
    }

    /// `spawn` on a terminal with the `keyboard`.
    pub fn spawn_as(cwd: &Path, env: &[(String, String)], keyboard: Keyboard) -> Nick {
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
        command.env("NICK_TEST_MASCOT", "still");
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
            let mut answered_flags = 0;
            let mut answered_da1 = 0;
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut out = sink.lock().unwrap();
                out.extend_from_slice(&buf[..n]);
                // Like a terminal, answer each `query` in the output once with `reply`.
                let answer_each = |query: &[u8], reply: &[u8], answered: &mut usize| {
                    let asked = out.windows(query.len()).filter(|w| *w == query).count();
                    while *answered < asked {
                        let mut writer = answer.lock().unwrap();
                        let _ = writer.write_all(reply);
                        let _ = writer.flush();
                        *answered += 1;
                    }
                };
                // The cursor position request (ESC[6n): ratatui's Terminal::clear asks for it
                // when the TUI comes back from the init.
                answer_each(b"\x1b[6n", b"\x1b[1;1R", &mut answered);
                // The query of the kitty keyboard protocol (ESC[?u, NS-49): answered only by a
                // kitty terminal, and before the DA1 of the same query.
                if keyboard == Keyboard::Kitty {
                    answer_each(KITTY_QUERY.as_bytes(), b"\x1b[?0u", &mut answered_flags);
                }
                // The primary device attributes (DA1, ESC[c): always, like any terminal.
                answer_each(b"\x1b[c", b"\x1b[?62c", &mut answered_da1);
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
        keep(label, self.parser().screen());
    }

    /// Waits until `cond` holds on the screen (`what` in the panic), and keeps that same screen
    /// (`keep`, so a screen that changes fast, as the cat Nick blinking, is kept as it was seen).
    pub fn keep_when(&self, label: &str, what: &str, cond: impl Fn(&vt100::Screen) -> bool) {
        let deadline = Instant::now() + TIMEOUT;
        while Instant::now() < deadline {
            let parser = self.parser();
            if cond(parser.screen()) {
                keep(label, parser.screen());
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("{what} never happened. Screen:\n{}", self.screen());
    }

    /// The rows of the screen with a half block of the cat Nick (NS-51) in her columns of the
    /// column on the left (1 to 17), from the top.
    pub fn cat_rows(&self) -> Vec<u16> {
        cat_rows(self.parser().screen())
    }

    /// The screen as one line: the dialog borders (and the thick one of the focused list,
    /// NS-37) and the half blocks of the cat Nick (NS-51) taken out and the non-empty rows
    /// joined by a space, so a text wrapped inside a dialog can be checked whole.
    pub fn flat_screen(&self) -> String {
        self.screen()
            .lines()
            .map(|l| {
                l.replace(['│', '┃'], "")
                    .replace(MASCOT_BLOCKS, " ")
                    .trim()
                    .to_string()
            })
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

    /// The color of the cell `offset` characters after the start of the first `needle` on the
    /// screen (NS-45: the colors of the stages and of the epic, by `cell`).
    pub fn color_at(&self, needle: &str, offset: u16) -> vt100::Color {
        let screen = self.screen();
        let (row, col) = screen
            .lines()
            .enumerate()
            .find_map(|(row, line)| {
                line.find(needle)
                    .map(|byte| (row, line[..byte].chars().count()))
            })
            .unwrap_or_else(|| panic!("{needle:?} not on the screen:\n{screen}"));
        let row = u16::try_from(row).unwrap();
        let col = u16::try_from(col).unwrap() + offset;
        self.cell(row, col).1
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

    /// Waits until `needle` is on the screen read as one line (`flat_screen`): a wrapped help
    /// line reads whole.
    pub fn wait_for_flat(&self, needle: &str) {
        self.wait_until(&format!("{needle:?} on the screen"), || {
            self.flat_screen().contains(needle)
        });
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

    /// The rows of the tree (NS-38): the columns inside the border of the column on the left,
    /// from the first row under the top of the border; the half blocks of the cat Nick (NS-51)
    /// taken out, so her rows are empty ones (the end of the tree for `select`).
    pub fn tree_rows(&self) -> Vec<String> {
        self.screen()
            .lines()
            .skip(1)
            .map(|line| {
                line.chars()
                    .skip(1)
                    .take(TREE_COLS)
                    .map(|c| if MASCOT_BLOCKS.contains(&c) { ' ' } else { c })
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }

    /// Waits until `needle` is in the column on the left read as one line (`tree_rows`, the
    /// non-empty ones joined by a space): a text wrapped in the 40 columns reads whole.
    pub fn wait_for_tree(&self, needle: &str) {
        self.wait_until(&format!("{needle:?} on the left"), || {
            self.tree_rows()
                .iter()
                .map(|row| row.trim())
                .filter(|row| !row.is_empty())
                .collect::<Vec<_>>()
                .join(" ")
                .contains(needle)
        });
    }

    /// The row of the tree with the selection (`> ` on it), and its index in `tree_rows`.
    pub fn selected_row(&self) -> Option<(usize, String)> {
        self.tree_rows()
            .into_iter()
            .enumerate()
            .find(|(_, row)| row.starts_with("> "))
    }

    /// Moves the selection of the tree with ↓ (then ↑) until its row contains `label`. Each key
    /// waits for the selection to move; the ends of the tree are its first row and the row
    /// before an empty one, so no key is sent past them.
    pub fn select(&mut self, label: &str) {
        self.wait_until("a selection on the tree", || self.selected_row().is_some());
        for key in [DOWN, UP] {
            loop {
                let (at, row) = self.selected_row().expect("a selection");
                if row.contains(label) {
                    return;
                }
                let rows = self.tree_rows();
                let last = rows.get(at + 1).is_none_or(|next| next.is_empty());
                if (key == DOWN && last) || (key == UP && at == 0) {
                    break;
                }
                self.send(key);
                self.wait_until(&format!("the selection to leave {row:?}"), || {
                    self.selected_row().is_some_and(|(_, now)| now != row)
                });
            }
        }
        panic!("{label:?} is not on the tree:\n{}", self.screen());
    }

    /// From anywhere on the tree: the module open, the project `name` selected and open (its
    /// tickets on the tree).
    pub fn open_project(&mut self, name: &str) {
        self.wait_for(" Navegação ");
        self.open_node("auto_scrum");
        self.open_node(name);
    }

    /// Selects the node `label` and opens it (`→`) when it is closed (`▸`).
    pub fn open_node(&mut self, label: &str) {
        self.select(label);
        let (_, row) = self.selected_row().expect("a selection");
        if row.contains(&format!("▸ {label}")) {
            self.send(RIGHT);
            self.wait_until(&format!("{label} open"), || {
                self.selected_row()
                    .is_some_and(|(_, row)| row.contains(&format!("▾ {label}")))
            });
        }
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

/// The rows of `screen` with a half block of the cat Nick in her columns (NS-51).
pub fn cat_rows(screen: &vt100::Screen) -> Vec<u16> {
    let (rows, _) = screen.size();
    (0..rows)
        .filter(|row| {
            (1..=MASCOT_COLS).any(|col| {
                screen.cell(*row, col).is_some_and(|cell| {
                    MASCOT_BLOCKS
                        .iter()
                        .any(|block| cell.contents() == block.to_string())
                })
            })
        })
        .collect()
}

/// Keeps `screen` under `label` for the validation of a ticket: its text appended to the file
/// named by `NICK_TEST_SCREENS` (NS-36) and, with its colors (ANSI: `cat` it on a terminal), in
/// `<label>.ansi` in the directory named by `NICK_TEST_SCREENS_ANSI` (NS-51), when they are
/// set; otherwise does nothing.
fn keep(label: &str, screen: &vt100::Screen) {
    if let Some(path) = std::env::var_os("NICK_TEST_SCREENS") {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(file, "===== {label} =====\n{}\n", screen.contents()).unwrap();
    }
    if let Some(dir) = std::env::var_os("NICK_TEST_SCREENS_ANSI") {
        let mut bytes = screen.contents_formatted();
        bytes.extend_from_slice(b"\x1b[0m\r\n");
        fs::write(Path::new(&dir).join(format!("{label}.ansi")), bytes).unwrap();
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
