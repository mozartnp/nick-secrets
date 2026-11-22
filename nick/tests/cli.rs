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
fn version_prints_the_package_version_and_the_commit() {
    let out = nick(&["--version"]);
    assert_eq!(out.status.code(), Some(0));
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert_eq!(stdout, format!("nick {}\n", nick::version::long()));
    assert!(
        stdout.starts_with(&format!("nick {} (commit ", env!("CARGO_PKG_VERSION"))),
        "{stdout}"
    );
    assert!(out.stderr.is_empty());
    // Built from a git checkout with a commit: the hash is the one of HEAD (NS-52).
    let head = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output();
    if let Ok(head) = head
        && head.status.success()
    {
        let hash = String::from_utf8_lossy(&head.stdout).trim().to_string();
        assert!(
            stdout.ends_with(&format!("(commit {hash})\n")),
            "{stdout} vs {hash}"
        );
    }
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

/// A database migrated by a newer nick (its `user_version` above the last migration this one
/// knows) is refused with the message and the code 1, and its bytes are left as they were: the
/// policy of migrations of the NS-31, through the binary (NS-52: two nicks, one database).
#[test]
fn a_database_newer_than_this_nick_is_refused_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let db_path = state.join("nick/nick.db");
    // The last version this nick knows: the one of a database it creates.
    nick::paths::create_state_dir(db_path.parent().unwrap()).unwrap();
    drop(nick::db::Db::open(&db_path).unwrap());
    let known: i64 = rusqlite::Connection::open(&db_path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert!(known > 0);
    let newer = known + 1;
    rusqlite::Connection::open(&db_path)
        .unwrap()
        .execute_batch(&format!("PRAGMA user_version = {newer};"))
        .unwrap();
    let before = std::fs::read(&db_path).unwrap();

    let out = Command::new(env!("CARGO_BIN_EXE_nick"))
        .env_clear()
        .env("HOME", dir.path().join("home"))
        .env("XDG_STATE_HOME", &state)
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(out.stdout.is_empty(), "the TUI never opened");
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        format!(
            "Erro: o banco {} está na versão {newer}, mais nova que a deste nick ({known}). \
             Atualize o nick.\n",
            db_path.display()
        )
    );
    assert!(
        std::fs::read(&db_path).unwrap() == before,
        "the database was not written"
    );
}
