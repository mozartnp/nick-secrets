//! Finding and running bin/nick-agent (NS-33), the only way the shell opens an agent: the
//! launcher loads the project config and launches claude through lib/ (the jail and the rules
//! of the nick), and the shell never builds that command itself. The path is fixed at compile
//! time (the checkout the binary was built from, as bin/nick-init) and, unlike the init, no
//! variable swaps it: one would let something else run in place of the path that guarantees
//! the jail and the rules. Before the PTY, the shell runs `bash <launcher> --check` (outside
//! the PTY, so an error of the config or of the jail reaches the TUI) and learns from its one
//! line whether the agent runs in the jail. The launch goes through the `AgentLauncher` trait,
//! so the TUI state (app.rs) is tested with a fake one that only records the calls.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use crate::agent_session::{AgentEvent, AgentSession};
use crate::paths::normalize;

/// bin/nick-agent of the checkout this binary was built from, before normalization.
const DEFAULT_LAUNCHER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../bin/nick-agent");

/// When `--check` exits with 0 but its stdout is not exactly one `jail=` line: the jail is
/// never presumed.
pub const UNEXPECTED_CHECK_MESSAGE: &str =
    "O bin/nick-agent respondeu algo inesperado na verificação.";

/// The launcher: bin/nick-agent of the checkout, normalized (without `..`).
pub fn launcher_path() -> PathBuf {
    normalize(Path::new(DEFAULT_LAUNCHER))
}

/// What a successful `--check` said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precheck {
    /// Whether the agent runs inside the jail (`jail=true`).
    pub jail: bool,
    /// What the check wrote on stderr (the warnings of the config), when anything.
    pub warning: Option<String>,
}

/// Reads the result of `--check`: the exit code (None when killed by a signal), its stdout and
/// its stderr. Exit 0 with exactly `jail=true` or `jail=false` (one line) is a success, with
/// the stderr as a warning; anything else on stdout is an error, so the jail is never presumed.
/// A failure is the stderr of the launcher (the message of the lib), or a message with the code
/// when it said nothing.
pub fn parse_check(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Result<Precheck, String> {
    let stderr = String::from_utf8_lossy(stderr).trim_end().to_string();
    if code != Some(0) {
        if !stderr.trim().is_empty() {
            return Err(stderr);
        }
        return Err(match code {
            Some(code) => format!("O bin/nick-agent falhou na verificação (código {code})."),
            None => "O bin/nick-agent foi interrompido na verificação.".to_string(),
        });
    }
    let stdout = stdout.strip_suffix(b"\n").unwrap_or(stdout);
    let jail = match stdout {
        b"jail=true" => true,
        b"jail=false" => false,
        _ => return Err(UNEXPECTED_CHECK_MESSAGE.to_string()),
    };
    let warning = (!stderr.trim().is_empty()).then_some(stderr);
    Ok(Precheck { jail, warning })
}

/// Runs `bash <launcher> --check` with the cwd at `project`, the stdin closed and the output
/// captured (outside the PTY), and reads it with `parse_check`. Synchronous: the jail check
/// runs `ai-jail ... -- true`, and the TUI waits for it.
pub fn run_check(launcher: &Path, project: &Path) -> Result<Precheck, String> {
    let output = Command::new("bash")
        .arg(launcher)
        .arg("--check")
        .current_dir(project)
        .stdin(Stdio::null())
        .output()
        .map_err(|err| format!("Não consegui rodar o bin/nick-agent: {err}."))?;
    parse_check(output.status.code(), &output.stdout, &output.stderr)
}

/// Opens an agent: the check before the PTY and the launch in it.
pub trait AgentLauncher {
    /// `bash <launcher> --check` with the cwd at `project`.
    fn check(&mut self, launcher: &Path, project: &Path) -> Result<Precheck, String>;
    /// Starts `bash <launcher> --session-id <session_id> --name <name>` in a PTY with the cwd
    /// at `project`, for the agent whose row has the id `agent_id` (its events carry it). The
    /// error is the system's.
    fn spawn(
        &mut self,
        launcher: &Path,
        project: &Path,
        agent_id: i64,
        session_id: &str,
        name: &str,
    ) -> Result<(), String>;
}

/// The size (rows, columns) of the PTY of an agent, from its name and whether it runs in the
/// jail (its header, which wraps, takes rows from its terminal: ui.rs).
pub type SizeOf = Box<dyn Fn(&str, bool) -> (u16, u16)>;

/// The real launcher: the check by `run_check`, and the launch in an `AgentSession` whose
/// events go to `tx`, of the size `size_of` gives for its name and the jail of the last check
/// (review NS-34, P2: the PTY is born with its final size). The loop takes the session from
/// `session`.
pub struct TerminalAgentLauncher<E> {
    tx: Sender<E>,
    size_of: SizeOf,
    jail: bool,
    pub session: Option<AgentSession>,
}

impl<E> TerminalAgentLauncher<E> {
    pub fn new(tx: Sender<E>, size_of: SizeOf) -> TerminalAgentLauncher<E> {
        TerminalAgentLauncher {
            tx,
            size_of,
            jail: false,
            session: None,
        }
    }
}

impl<E: From<AgentEvent> + Send + 'static> AgentLauncher for TerminalAgentLauncher<E> {
    fn check(&mut self, launcher: &Path, project: &Path) -> Result<Precheck, String> {
        let check = run_check(launcher, project)?;
        self.jail = check.jail;
        Ok(check)
    }

    fn spawn(
        &mut self,
        launcher: &Path,
        project: &Path,
        agent_id: i64,
        session_id: &str,
        name: &str,
    ) -> Result<(), String> {
        let session = AgentSession::spawn(
            launcher,
            project,
            agent_id,
            session_id,
            name,
            (self.size_of)(name, self.jail),
            self.tx.clone(),
        )
        .map_err(|err| err.to_string())?;
        self.session = Some(session);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Component;

    /// Moving the crate without fixing the path breaks the agents: this pins it to
    /// <repository root>/bin/nick-agent, and the file must exist.
    #[test]
    fn the_launcher_is_bin_nick_agent_of_the_repository() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let path = launcher_path();
        assert_eq!(path, root.join("bin/nick-agent"));
        assert!(path.is_file(), "{} exists", path.display());
        assert!(
            !path.components().any(|c| c == Component::ParentDir),
            "normalized: {}",
            path.display()
        );
    }

    #[test]
    fn jail_true_and_jail_false_are_read() {
        assert_eq!(
            parse_check(Some(0), b"jail=true\n", b""),
            Ok(Precheck {
                jail: true,
                warning: None
            })
        );
        assert_eq!(
            parse_check(Some(0), b"jail=false\n", b""),
            Ok(Precheck {
                jail: false,
                warning: None
            })
        );
    }

    #[test]
    fn any_other_stdout_with_exit_0_is_an_error() {
        for stdout in [
            &b""[..],
            b"\n",
            b"jail=TRUE\n",
            b"jail=1\n",
            b"jail=true\njail=true\n",
            b"Configura\xc3\xa7\xc3\xa3o carregada\njail=true\n",
            b"jail=true\nmais\n",
            b" jail=true\n",
            b"jail=truex\n",
        ] {
            assert_eq!(
                parse_check(Some(0), stdout, b""),
                Err(UNEXPECTED_CHECK_MESSAGE.to_string()),
                "{:?}",
                String::from_utf8_lossy(stdout)
            );
        }
        // Without the final line break, the line is still the line.
        assert!(parse_check(Some(0), b"jail=false", b"").is_ok());
    }

    #[test]
    fn the_stderr_of_a_success_is_a_warning() {
        assert_eq!(
            parse_check(
                Some(0),
                b"jail=false\n",
                b"Aviso: chave ausente\n  X=\"\"\n"
            ),
            Ok(Precheck {
                jail: false,
                warning: Some("Aviso: chave ausente\n  X=\"\"".into())
            })
        );
        assert_eq!(
            parse_check(Some(0), b"jail=true\n", b"  \n"),
            Ok(Precheck {
                jail: true,
                warning: None
            }),
            "blank stderr is no warning"
        );
    }

    #[test]
    fn a_failure_is_the_stderr_or_the_code() {
        assert_eq!(
            parse_check(Some(1), b"", b"Erro: a jail falhou.\nDetalhe\n"),
            Err("Erro: a jail falhou.\nDetalhe".into())
        );
        // Even with a jail= line: a non-zero exit never opens.
        assert_eq!(
            parse_check(Some(1), b"jail=false\n", b"Erro: x\n"),
            Err("Erro: x".into())
        );
        assert_eq!(
            parse_check(Some(2), b"", b""),
            Err("O bin/nick-agent falhou na verificação (código 2).".into())
        );
        assert_eq!(
            parse_check(None, b"", b""),
            Err("O bin/nick-agent foi interrompido na verificação.".into())
        );
    }

    #[test]
    fn run_check_runs_the_real_launcher_in_the_project() {
        let tmp = tempfile::tempdir().unwrap();
        let proj = fs::canonicalize(tmp.path()).unwrap();
        // No config: the error of the launcher, pointing at bin/nick-init.
        let err = run_check(&launcher_path(), &proj).unwrap_err();
        assert!(
            err.starts_with(&format!(
                "Erro: não há configuração do projeto em {}/.nick/nick.conf.",
                proj.display()
            )),
            "{err}"
        );
        // With the jail off (no ai-jail involved), jail=false and the warnings of the config.
        fs::create_dir_all(proj.join(".nick")).unwrap();
        fs::write(proj.join(".nick/nick.conf"), "JAIL_ENABLED=\"false\"\n").unwrap();
        let check = run_check(&launcher_path(), &proj).unwrap();
        assert!(!check.jail);
        assert!(
            check
                .warning
                .as_deref()
                .is_some_and(|w| w.contains("não tem todas as chaves")),
            "{check:?}"
        );
        assert!(
            !proj.join(".nick/.gitignore").exists(),
            "--check writes nothing in the project"
        );
    }

    #[test]
    fn run_check_of_a_missing_launcher_says_so() {
        let tmp = tempfile::tempdir().unwrap();
        let err = run_check(&tmp.path().join("nao-existe"), tmp.path()).unwrap_err();
        // The app checks that the launcher is a file before (the alert with its path); here,
        // bash's own error is the message.
        assert!(err.contains("nao-existe"), "{err}");
    }
}
