//! Integration test of the panic hook: the real nick-panel binary in a PTY, with the fake
//! ai-jail and claude of tests/fixtures/bin first in the PATH, and a panic forced by the test
//! hook NICK_PANEL_TEST_PANIC. Whatever thread panics, the panel restores the terminal, ends
//! every session and exits with 101 (a panic in a secondary thread used to leave the main loop
//! running on a restored terminal, review NS-29, problem 3).

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nick-panel-panic-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("home")).unwrap();
    dir
}

fn alive(pid: i32) -> bool {
    nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid), None).is_ok()
}

/// Runs the panel with NICK_PANEL_TEST_PANIC=`trigger` and the fake claude in "sleep" mode.
/// Returns the exit code (None if it never ended), the terminal output and the session pids.
fn run_panel(trigger: &str) -> (Option<u32>, String, Vec<i32>) {
    let dir = temp_dir(trigger);
    let pids_file = dir.join("pids");
    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin");
    let path = std::env::var_os("PATH").unwrap_or_default();
    let mut paths = vec![fixtures];
    paths.extend(std::env::split_paths(&path));

    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_nick-panel"));
    command.cwd(&dir);
    command.env("PATH", std::env::join_paths(paths).unwrap());
    command.env("HOME", dir.join("home"));
    command.env("TERM", "xterm-256color");
    command.env("PLAYWRIGHT_BROWSERS_PATH", "");
    command.env("NICK_PANEL_CLIPBOARD", "");
    command.env("NICK_PANEL_ALLOW_HOSTS", "");
    command.env("NICK_PANEL_FORWARD_PORTS", "");
    command.env("FAKE_CLAUDE_MODE", "sleep");
    command.env("FAKE_CLAUDE_PIDS", &pids_file);
    command.env("NICK_PANEL_TEST_PANIC", trigger);
    let mut child = pair.slave.spawn_command(command).unwrap();
    drop(pair.slave);

    let output = Arc::new(Mutex::new(Vec::new()));
    let mut reader = pair.master.try_clone_reader().unwrap();
    let sink = Arc::clone(&output);
    std::thread::spawn(move || {
        let mut buf = [0u8; 8192];
        while let Ok(n) = reader.read(&mut buf) {
            if n == 0 {
                break;
            }
            sink.lock().unwrap().extend_from_slice(&buf[..n]);
        }
    });

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut code = None;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            code = Some(status.exit_code());
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    if code.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    std::thread::sleep(Duration::from_millis(200));
    let pids: Vec<i32> = fs::read_to_string(&pids_file)
        .unwrap_or_default()
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect();
    let out = String::from_utf8_lossy(&output.lock().unwrap()).into_owned();
    let _ = fs::remove_dir_all(&dir);
    (code, out, pids)
}

fn assert_clean_panic(trigger: &str) {
    let (code, out, pids) = run_panel(trigger);
    // The end of the output is what tells: the panic message and what came after it.
    let out: String = {
        let chars: Vec<char> = out.chars().collect();
        chars[chars.len().saturating_sub(800)..].iter().collect()
    };
    // Each session leaves a pid pair (fake claude, its background sleep) before the panic.
    assert_eq!(pids.len(), 4, "both sessions started: {pids:?}");
    let left: Vec<i32> = pids.iter().copied().filter(|&p| alive(p)).collect();
    for &pid in &left {
        let _ = nix::sys::signal::kill(
            nix::unistd::Pid::from_raw(pid),
            nix::sys::signal::Signal::SIGKILL,
        );
    }
    assert_eq!(
        code,
        Some(101),
        "the panel exits with 101; output:\n{out:?}"
    );
    assert!(left.is_empty(), "session processes survived: {left:?}");
    assert!(
        out.contains("\x1b[?1049l"),
        "the alternate screen was left: {out:?}"
    );
    assert!(
        out.contains("NICK_PANEL_TEST_PANIC"),
        "the panic message: {out:?}"
    );
}

#[test]
fn a_panic_in_the_main_thread_restores_and_ends_everything() {
    assert_clean_panic("1");
}

#[test]
fn a_panic_in_a_secondary_thread_restores_and_ends_everything() {
    assert_clean_panic("thread");
}
