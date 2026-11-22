//! Finding and running bin/nick-init (NS-30), the only source of the project config: the
//! shell runs it as it is, never copies its logic. The path is under the root of the files of
//! the nick (resources.rs, NS-52: the prefix of an installation or the checkout) and can be
//! replaced by NICK_INIT_BIN. Running it goes through the `InitRunner` trait, so the TUI state
//! (app.rs) is tested with a fake one that only records the calls.

use std::ffi::OsStr;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths::{normalize, printable};
use crate::terminal::{self, IgnoreTerminalSignals, InputGate, TerminalGuard};

/// How a run of the init ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitOutcome {
    /// It ended with this exit code.
    Exited(i32),
    /// It was killed by a signal.
    Signaled,
    /// It could not be started (e.g. no bash); the system error.
    SpawnFailed(String),
}

/// Runs the init of `init` with the cwd at `project`.
pub trait InitRunner {
    fn run(&mut self, init: &Path, project: &Path) -> InitOutcome;
}

/// The real runner: pauses the input thread (its gate, so the init has the stdin to itself),
/// suspends the TUI (terminal.rs), runs `bash <init>` with the cwd at the project and the
/// terminal inherited, and waits for an Enter before the TUI comes back, whatever the result,
/// so the operator can read the output of the init. A failure to enter the TUI again is kept in
/// `error`, for the loop to end with it.
pub struct TerminalInitRunner<'a> {
    guard: &'a mut TerminalGuard,
    gate: &'a InputGate,
    pub error: Option<io::Error>,
}

impl<'a> TerminalInitRunner<'a> {
    pub fn new(guard: &'a mut TerminalGuard, gate: &'a InputGate) -> TerminalInitRunner<'a> {
        TerminalInitRunner {
            guard,
            gate,
            error: None,
        }
    }
}

impl InitRunner for TerminalInitRunner<'_> {
    fn run(&mut self, init: &Path, project: &Path) -> InitOutcome {
        let result =
            terminal::run_suspended(self.guard, self.gate, || run_in_terminal(init, project));
        match result {
            Ok(outcome) => outcome,
            Err(err) => {
                let message = err.to_string();
                self.error = Some(err);
                InitOutcome::SpawnFailed(message)
            }
        }
    }
}

/// Runs the init on the normal terminal and waits for the Enter, with the nick immune to the
/// Ctrl+C (and Ctrl+\\) of that time: out of raw mode, the terminal sends SIGINT/SIGQUIT to the
/// whole foreground group, the nick included, and the default action would kill it without
/// restoring the TUI (review NS-31, P1). Like system(3) does, the parent ignores them while
/// the child runs; the child still gets them, since a handler (unlike SIG_IGN) goes back to
/// the default on exec.
fn run_in_terminal(init: &Path, project: &Path) -> InitOutcome {
    let _immune = IgnoreTerminalSignals::new();
    println!(
        "Rodando {} em {}...",
        printable(&init.to_string_lossy()),
        printable(&project.to_string_lossy())
    );
    let outcome = match Command::new("bash").arg(init).current_dir(project).status() {
        Ok(status) => match status.code() {
            Some(code) => InitOutcome::Exited(code),
            None => InitOutcome::Signaled,
        },
        Err(err) => InitOutcome::SpawnFailed(err.to_string()),
    };
    print!("Pressione Enter para voltar ao nick.");
    let _ = io::stdout().flush();
    // Any line, or the end of the input, goes back.
    let _ = io::stdin().lock().read_line(&mut String::new());
    outcome
}

/// The init to run: NICK_INIT_BIN when it is set and not empty (made absolute against the cwd
/// of the nick, since the init runs with the cwd at the project), or bin/nick-init under `root`
/// (resources.rs: the prefix of an installation or the checkout), normalized (without `..`).
pub fn init_path(override_path: Option<&OsStr>, root: &Path) -> PathBuf {
    match override_path.filter(|path| !path.is_empty()) {
        Some(path) => std::path::absolute(path).unwrap_or_else(|_| PathBuf::from(path)),
        None => normalize(&root.join("bin/nick-init")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::resources::Resources;
    use std::path::Component;

    /// `init_path` on the checkout (the root of the tests).
    fn init_path(override_path: Option<&OsStr>) -> PathBuf {
        super::init_path(override_path, Resources::checkout().root())
    }

    /// Moving the crate without fixing the path breaks the button: this pins it to
    /// <repository root>/bin/nick-init, and the file must exist.
    #[test]
    fn the_default_init_is_bin_nick_init_of_the_repository() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let path = init_path(None);
        assert_eq!(path, root.join("bin/nick-init"));
        assert!(path.is_file(), "{} exists", path.display());
        assert!(
            !path.components().any(|c| c == Component::ParentDir),
            "normalized: {}",
            path.display()
        );
    }

    #[test]
    fn an_empty_override_is_ignored() {
        assert_eq!(init_path(Some(OsStr::new(""))), init_path(None));
    }

    #[test]
    fn an_absolute_override_wins_as_it_is() {
        assert_eq!(
            init_path(Some(OsStr::new("/x/fake-init"))),
            PathBuf::from("/x/fake-init")
        );
    }

    #[test]
    fn a_relative_override_is_made_absolute_against_the_cwd() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            init_path(Some(OsStr::new("tests/fake"))),
            cwd.join("tests/fake")
        );
    }
}
