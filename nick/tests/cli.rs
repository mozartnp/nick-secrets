//! Integration test of the command line: `nick --version` is the only accepted argument, and
//! any other one is refused with the usage, in the pattern of bin/nick-init. None of these
//! cases opens the TUI nor touches the state directory.

use std::process::Command;

fn nick(args: &[&str]) -> std::process::Output {
    let dir = tempfile::tempdir().unwrap();
    Command::new(env!("CARGO_BIN_EXE_nick"))
        .args(args)
        .env("HOME", dir.path().join("home"))
        .env("XDG_STATE_HOME", dir.path().join("state"))
        .output()
        .unwrap()
}

#[test]
fn version_prints_the_package_version() {
    let out = nick(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        String::from_utf8_lossy(&out.stdout),
        format!("nick {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(out.stderr.is_empty());
}

#[test]
fn an_unknown_argument_is_refused_with_the_usage() {
    let out = nick(&["--help"]);
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Erro: argumento desconhecido: '--help'\nUso: nick\n"
    );
}

#[test]
fn version_with_another_argument_is_refused() {
    let out = nick(&["--version", "x"]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "Erro: argumento desconhecido: 'x'\nUso: nick\n"
    );
}
