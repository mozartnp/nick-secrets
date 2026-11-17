//! Integration test: real sessions in real PTYs, opened by the same constructor the panel uses
//! (`Session::spawn` with the real `launch_session.sh`), with the fake ai-jail and claude of
//! tests/fixtures/bin (the same ones test_launch_session.sh uses) first in the PATH. Nothing
//! real is launched, and nothing is written inside the repository.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nick_panel::session::{self, LaunchConfig, Session, SessionEvent, Status};

static NEXT_DIR: AtomicUsize = AtomicUsize::new(0);

/// A fresh temporary directory for one test, removed on drop.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let n = NEXT_DIR.fetch_add(1, Ordering::SeqCst);
        let dir =
            std::env::temp_dir().join(format!("nick-panel-test-{}-{n}-{name}", std::process::id()));
        fs::create_dir_all(dir.join("cwd")).unwrap();
        fs::create_dir_all(dir.join("home")).unwrap();
        TempDir(dir)
    }

    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixtures_bin() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin")
}

/// The panel's own launch config, pointed at the fakes and at the test's directories: the
/// fakes first in the PATH, an empty HOME (the jail's toolchain mounts come from it) and no
/// PLAYWRIGHT_BROWSERS_PATH nor NICK_PANEL_* from whoever runs the tests.
fn config(dir: &TempDir, mode: &str, extra: &[(&str, &str)]) -> LaunchConfig {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![fixtures_bin()];
    paths.extend(std::env::split_paths(&path));
    let mut env: Vec<(OsString, OsString)> = vec![
        ("PATH".into(), std::env::join_paths(paths).unwrap()),
        ("HOME".into(), dir.path("home").into()),
        ("PLAYWRIGHT_BROWSERS_PATH".into(), "".into()),
        ("NICK_PANEL_CLIPBOARD".into(), "".into()),
        ("NICK_PANEL_ALLOW_HOSTS".into(), "".into()),
        ("NICK_PANEL_FORWARD_PORTS".into(), "".into()),
        ("FAKE_JAIL_LOG".into(), dir.path("jail.argv").into()),
        ("FAKE_CLAUDE_LOG".into(), dir.path("claude.argv").into()),
        ("FAKE_CLAUDE_ENV".into(), dir.path("claude.env").into()),
        ("FAKE_CLAUDE_MODE".into(), mode.into()),
    ];
    env.extend(
        extra
            .iter()
            .map(|(k, v)| (OsString::from(k), OsString::from(v))),
    );
    let mut config = LaunchConfig::for_panel().unwrap();
    config.cwd = dir.path("cwd");
    config.env = env;
    config
}

fn spawn(config: &LaunchConfig, rows: u16, cols: u16) -> (Session, mpsc::Receiver<SessionEvent>) {
    let (tx, rx) = mpsc::channel();
    let session = Session::spawn(0, "Projeto Demo > NS-0001 > Dev", config, rows, cols, tx)
        .expect("spawn the session");
    (session, rx)
}

/// Polls `cond` until it holds or `timeout` passes.
fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if cond() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    cond()
}

fn contents(session: &Session) -> String {
    session.with_screen(|screen| screen.contents())
}

fn wait_for_text(session: &Session, text: &str) {
    let found = wait_until(Duration::from_secs(10), || contents(session).contains(text));
    assert!(
        found,
        "{text:?} never showed up; screen:\n{}",
        contents(session)
    );
}

fn wait_exit(session: &Session) -> Status {
    assert!(
        wait_until(Duration::from_secs(10), || session.status()
            != Status::Running),
        "the session never ended; screen:\n{}",
        contents(session)
    );
    session.status()
}

fn alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

#[test]
fn the_session_runs_the_launcher_which_runs_claude_inside_the_jail() {
    let dir = TempDir::new("argv");
    let (session, rx) = spawn(&config(&dir, "", &[]), 24, 80);
    assert_eq!(wait_exit(&session), Status::Exited(0));
    let jail_argv = fs::read_to_string(dir.path("jail.argv")).expect("the fake ai-jail ran");
    let lines: Vec<&str> = jail_argv.lines().collect();
    let dash = lines
        .iter()
        .position(|l| *l == "--")
        .expect("a -- in the argv");
    assert_eq!(&lines[dash + 1..dash + 3], ["claude", "--settings"]);
    assert!(lines[..dash].contains(&"--no-inherit-env"), "{jail_argv}");
    let claude_argv = fs::read_to_string(dir.path("claude.argv")).unwrap();
    assert!(claude_argv.starts_with("--settings\n{"), "{claude_argv}");
    assert!(claude_argv.contains("--append-system-prompt"));
    // The terminal type the panel gives the child (inside the real jail: see the report).
    let env = fs::read_to_string(dir.path("claude.env")).unwrap();
    assert!(env.lines().any(|l| l == "TERM=xterm-256color"), "{env}");
    assert!(env.lines().any(|l| l == "COLORTERM=truecolor"), "{env}");
    assert!(
        env.lines()
            .any(|l| l == format!("PWD={}", dir.path("cwd").display())),
        "the session runs in the panel's cwd: {env}"
    );
    let events: Vec<SessionEvent> = rx.try_iter().collect();
    assert!(events.contains(&SessionEvent::Output(0)), "{events:?}");
    assert!(events.contains(&SessionEvent::Exited(0)), "{events:?}");
}

#[test]
fn the_output_reaches_the_screen_with_its_color() {
    let dir = TempDir::new("color");
    let (session, _rx) = spawn(&config(&dir, "", &[]), 24, 80);
    wait_exit(&session);
    wait_for_text(&session, "OK");
    wait_for_text(&session, "Jail (ai-jail): ativa");
    let red_ok = session.with_screen(|screen| {
        let (rows, cols) = screen.size();
        (0..rows).any(|r| {
            (0..cols.saturating_sub(1)).any(|c| {
                let a = screen.cell(r, c).unwrap();
                let b = screen.cell(r, c + 1).unwrap();
                a.contents() == "O"
                    && b.contents() == "K"
                    && a.fgcolor() == vt100::Color::Idx(1)
                    && b.fgcolor() == vt100::Color::Idx(1)
            })
        })
    });
    assert!(red_ok, "no red OK on the screen:\n{}", contents(&session));
}

#[test]
fn writes_reach_the_child() {
    let dir = TempDir::new("echo");
    let (mut session, _rx) = spawn(
        &config(&dir, "echo", &[("FAKE_CLAUDE_READ_BYTES", "6")]),
        24,
        80,
    );
    wait_for_text(&session, "READY");
    session.write(b"\x03ola\r").unwrap();
    session.write("ç".as_bytes()).unwrap();
    wait_for_text(&session, "GOT 03 6f 6c 61 0d c3");
    wait_exit(&session);
}

#[test]
fn resize_changes_the_parser_and_the_pty() {
    let dir = TempDir::new("winsize");
    let (mut session, _rx) = spawn(&config(&dir, "winsize", &[]), 24, 80);
    wait_for_text(&session, "SIZE 24 80");
    session.resize(30, 100).unwrap();
    assert_eq!(session.with_screen(|s| s.size()), (30, 100));
    wait_for_text(&session, "SIZE 30 100");
    session.terminate();
}

#[test]
fn terminate_ends_the_whole_process_group() {
    let dir = TempDir::new("sleep");
    let pids_file = dir.path("pids");
    let pids_env = pids_file.to_str().unwrap().to_string();
    let (mut session, _rx) = spawn(
        &config(&dir, "sleep", &[("FAKE_CLAUDE_PIDS", &pids_env)]),
        24,
        80,
    );
    wait_for_text(&session, "READY");
    let pids: Vec<i32> = fs::read_to_string(&pids_file)
        .unwrap()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    assert_eq!(pids.len(), 2);
    assert_eq!(
        session.pid(),
        Some(pids[0] as u32),
        "the launcher execs down to claude"
    );
    assert!(pids.iter().all(|p| alive(*p)), "control: both alive before");
    session.terminate();
    assert_ne!(session.status(), Status::Running);
    for pid in pids {
        assert!(
            wait_until(Duration::from_secs(3), || !alive(pid)),
            "pid {pid} survived terminate()"
        );
    }
}

#[test]
fn an_unwatched_session_keeps_being_read() {
    // Nobody looks at this session's screen while the child writes ~1 MB: only the reader
    // thread drains the PTY. If it didn't, the child would block on write and never end.
    let dir = TempDir::new("flood");
    let (session, _rx) = spawn(&config(&dir, "flood", &[]), 24, 80);
    assert_eq!(wait_exit(&session), Status::Exited(0));
    wait_for_text(&session, "FIM-DO-FLOOD");
}

#[test]
fn terminate_all_ends_every_session() {
    let dirs = [TempDir::new("all_a"), TempDir::new("all_b")];
    let (tx, _rx) = mpsc::channel::<SessionEvent>();
    let mut sessions: Vec<Session> = dirs
        .iter()
        .enumerate()
        .map(|(i, dir)| {
            Session::spawn(i, "x", &config(dir, "winsize", &[]), 24, 80, tx.clone()).unwrap()
        })
        .collect();
    for s in &sessions {
        wait_for_text(s, "SIZE");
    }
    let pids: Vec<i32> = sessions.iter().map(|s| s.pid().unwrap() as i32).collect();
    session::terminate_all(&mut sessions);
    for (s, pid) in sessions.iter().zip(pids) {
        assert_ne!(s.status(), Status::Running);
        assert!(!alive(pid));
    }
}

#[test]
fn terminate_all_leaves_the_group_of_a_reaped_session_alone() {
    // Once the waiter reaps a session's child, its pid may be reused by an unrelated group
    // leader (a job of the user's shell): terminate_all must not signal that group anymore
    // (review NS-29, problem 4). Here the leftover is the fake's own background sleep, which
    // the old behavior would have killed, so it shows which groups get signaled.
    let dir = TempDir::new("reaped");
    let pids_file = dir.path("pids");
    let pids_env = pids_file.to_str().unwrap().to_string();
    let (mut session, _rx) = spawn(
        &config(&dir, "sleep", &[("FAKE_CLAUDE_PIDS", &pids_env)]),
        24,
        80,
    );
    wait_for_text(&session, "READY");
    let pids: Vec<i32> = fs::read_to_string(&pids_file)
        .unwrap()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    let (leader, leftover) = (pids[0], pids[1]);
    nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(leader),
        nix::sys::signal::Signal::SIGKILL,
    )
    .unwrap();
    wait_exit(&session);
    assert!(alive(leftover), "control: the leftover outlives the leader");
    session.terminate();
    let survived = alive(leftover);
    let _ = nix::sys::signal::kill(
        nix::unistd::Pid::from_raw(leftover),
        nix::sys::signal::Signal::SIGKILL,
    );
    assert!(
        survived,
        "terminate_all signaled the group of a reaped session"
    );
}

#[test]
fn a_copy_from_the_session_comes_out_as_an_event() {
    // The OSC 52 that claude sends to copy is not something vt100 shows: the session hands it
    // to the panel, which writes it to the outer terminal (review NS-29, problem 2).
    let dir = TempDir::new("osc52");
    let (session, rx) = spawn(&config(&dir, "osc52", &[]), 24, 80);
    wait_exit(&session);
    wait_for_text(&session, "COPIADO");
    let events: Vec<SessionEvent> = rx.try_iter().collect();
    assert!(
        events.contains(&SessionEvent::Clipboard(
            0,
            b"\x1b]52;c;b2zDoQ==\x07".to_vec()
        )),
        "{events:?}"
    );
}

/// The sources of the panel, read at compile time.
const SOURCES: [(&str, &str); 6] = [
    ("lib.rs", include_str!("../src/lib.rs")),
    ("main.rs", include_str!("../src/main.rs")),
    ("app.rs", include_str!("../src/app.rs")),
    ("keys.rs", include_str!("../src/keys.rs")),
    ("session.rs", include_str!("../src/session.rs")),
    ("ui.rs", include_str!("../src/ui.rs")),
];

#[test]
fn the_only_command_launched_is_bash_with_the_launcher() {
    let mut builders = 0;
    for (name, source) in SOURCES {
        assert!(
            !source.contains("\"claude\""),
            "{name} names claude as a string"
        );
        assert!(
            !source.contains("\"ai-jail\""),
            "{name} names ai-jail as a string"
        );
        assert!(
            !source.contains("process::Command"),
            "{name} uses std::process::Command"
        );
        assert!(
            !source.contains("Command::new("),
            "{name} launches a command other than through CommandBuilder"
        );
        builders += source.matches("CommandBuilder::new(").count();
        if source.contains("CommandBuilder::new(") {
            assert!(source.contains("CommandBuilder::new(\"bash\")"), "{name}");
            assert!(source.contains("launch_session.sh"), "{name}");
        }
    }
    assert_eq!(
        builders, 1,
        "a single CommandBuilder::new(\"bash\") in src/"
    );
    // Positive control: the checks see a direct launch.
    let bad = "let c = CommandBuilder::new(\"claude\"); std::process::Command::new(\"ai-jail\");";
    assert!(bad.contains("\"claude\"") && bad.contains("\"ai-jail\""));
    assert!(bad.contains("process::Command") && bad.contains("Command::new("));
}
