//! Finding and running bin/nick-agent (NS-33), the only way the shell opens an agent: the
//! launcher loads the project config and launches claude through lib/ (the jail and the rules
//! of the nick), and the shell never builds that command itself. The path is under the root of
//! the files of the nick (resources.rs, NS-52: found by where the binary is, as bin/nick-init)
//! and, unlike the init, no variable swaps it: one would let something else run in place of the
//! path that guarantees the jail and the rules. Before the PTY, the shell runs
//! `bash <launcher> --check` (outside the PTY, so an error of the config or of the jail reaches
//! the TUI) and learns from its three
//! lines whether the agent runs in the jail and which stage types the project offers (Jira,
//! Sentry). Then `bash <launcher> --render <type> <answers>` (NS-35) builds the prompt by bash,
//! into its log, and the shell only reads that log for the preview: no Rust builds a prompt, reads
//! a template or reads the config of the project. The launch goes through the `AgentLauncher`
//! trait, so the TUI state (app.rs) is tested with a fake one that only records the calls.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;

use crate::agent_session::{AgentEvent, AgentSession};
use crate::agents::Stage;
use crate::paths::normalize;
use crate::stages::StageType;

/// When `--check` exits with 0 but its stdout is not exactly the three lines `jail=`, `jira=`
/// and `sentry=`: nothing is ever presumed.
pub const UNEXPECTED_CHECK_MESSAGE: &str =
    "O bin/nick-agent respondeu algo inesperado na verificação.";

/// When `--render` exits with 0 but its stdout does not start with `log=<absolute path>`.
pub const UNEXPECTED_RENDER_MESSAGE: &str =
    "O bin/nick-agent respondeu algo inesperado na montagem do prompt.";

/// When `--has-conversation` exits with 0 but its stdout is not exactly the line
/// `conversation=true` or `conversation=false` (NS-39).
pub const UNEXPECTED_CONVERSATION_MESSAGE: &str =
    "O bin/nick-agent respondeu algo inesperado na verificação da conversa.";

/// How the PTY of an agent starts (NS-39): a new agent, with the prompt `--render` built, or the
/// resume of a stopped one in its own conversation, with no prompt. The launcher decides the
/// rest of the argv of claude (the auto mode by the type or the stage, the notices).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Launch<'a> {
    /// `--type <kind> --prompt-file <prompt_file>` (NS-35).
    New {
        kind: StageType,
        prompt_file: &'a Path,
    },
    /// `--stage <stage> [--opened-id <opened_key>] --ticket-id <key>` (or `--epic-id <key>`, an
    /// agent of an epic: NS-40): the stage of the agent (its type is not stored), the key it was
    /// opened with, when known (`agents::opened_key`), and the key of its ticket or epic now; when
    /// they differ, the launcher tells the agent the new folder.
    Resume {
        stage: Stage,
        opened_key: Option<&'a str>,
        holder_key: HolderKey<'a>,
    },
}

/// The key now of the ticket or of the epic of a resumed agent (NS-40).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HolderKey<'a> {
    Ticket(&'a str),
    Epic(&'a str),
}

impl Launch<'_> {
    /// The arguments of the launcher for the agent `session_id` named `name`.
    pub fn args(&self, session_id: &str, name: &str) -> Vec<std::ffi::OsString> {
        let mut args: Vec<std::ffi::OsString> = Vec::new();
        let mut push = |values: &[&OsStr]| args.extend(values.iter().map(|v| v.to_os_string()));
        match *self {
            Launch::New { kind, prompt_file } => {
                push(&["--session-id".as_ref(), session_id.as_ref()]);
                push(&["--name".as_ref(), name.as_ref()]);
                push(&["--type".as_ref(), kind.key().as_ref()]);
                push(&["--prompt-file".as_ref(), prompt_file.as_os_str()]);
            }
            Launch::Resume {
                stage,
                opened_key,
                holder_key,
            } => {
                push(&["--resume".as_ref(), session_id.as_ref()]);
                push(&["--name".as_ref(), name.as_ref()]);
                push(&["--stage".as_ref(), stage.key().as_ref()]);
                if let Some(key) = opened_key {
                    push(&["--opened-id".as_ref(), key.as_ref()]);
                }
                match holder_key {
                    HolderKey::Ticket(key) => push(&["--ticket-id".as_ref(), key.as_ref()]),
                    HolderKey::Epic(key) => push(&["--epic-id".as_ref(), key.as_ref()]),
                }
            }
        }
        args
    }
}

/// The launcher: bin/nick-agent under `root` (resources.rs, NS-52: the prefix of an installation
/// or the checkout), normalized (without `..`).
pub fn launcher_path(root: &Path) -> PathBuf {
    normalize(&root.join("bin/nick-agent"))
}

/// What a successful `--check` said.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Precheck {
    /// Whether the agent runs inside the jail (`jail=true`).
    pub jail: bool,
    /// Whether the project uses Jira (`jira=true`): the types with Jira are offered.
    pub jira: bool,
    /// Whether the project uses Sentry (`sentry=true`): the type with Sentry is offered.
    pub sentry: bool,
    /// What the check wrote on stderr (the warnings of the config), when anything.
    pub warning: Option<String>,
}

/// The error of a run of the launcher outside the PTY that did not exit with 0 (`code` is None
/// when killed by a signal): its stderr (the message of the lib, or its usage error), or, when it
/// said nothing, a message with the code and `what` it was doing ("verificação", "montagem do
/// prompt").
fn launcher_failure(code: Option<i32>, stderr: String, what: &str) -> String {
    if !stderr.trim().is_empty() {
        return stderr;
    }
    match code {
        Some(code) => format!("O bin/nick-agent falhou na {what} (código {code})."),
        None => format!("O bin/nick-agent foi interrompido na {what}."),
    }
}

/// The value of the line `<name>=true` or `<name>=false`, exactly.
fn flag_line(line: &[u8], name: &str) -> Option<bool> {
    let value = line.strip_prefix(name.as_bytes())?.strip_prefix(b"=")?;
    match value {
        b"true" => Some(true),
        b"false" => Some(false),
        _ => None,
    }
}

/// Reads the result of `--check`: the exit code (None when killed by a signal), its stdout and
/// its stderr. Exit 0 with exactly the three lines `jail=`, `jira=` and `sentry=`, in this
/// order, each `true` or `false` (with or without the final line break), is a success, with the
/// stderr as a warning; anything else on stdout is an error, so nothing is ever presumed. A
/// failure is the stderr of the launcher (the message of the lib), or a message with the code
/// when it said nothing.
pub fn parse_check(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> Result<Precheck, String> {
    let stderr = String::from_utf8_lossy(stderr).trim_end().to_string();
    if code != Some(0) {
        return Err(launcher_failure(code, stderr, "verificação"));
    }
    let stdout = stdout.strip_suffix(b"\n").unwrap_or(stdout);
    let lines: Vec<&[u8]> = stdout.split(|b| *b == b'\n').collect();
    let flags = match lines.as_slice() {
        [jail, jira, sentry] => (
            flag_line(jail, "jail"),
            flag_line(jira, "jira"),
            flag_line(sentry, "sentry"),
        ),
        _ => (None, None, None),
    };
    let (Some(jail), Some(jira), Some(sentry)) = flags else {
        return Err(UNEXPECTED_CHECK_MESSAGE.to_string());
    };
    let warning = (!stderr.trim().is_empty()).then_some(stderr);
    Ok(Precheck {
        jail,
        jira,
        sentry,
        warning,
    })
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

/// Whether `value` is a session id as claude writes it: a lowercase UUID (8-4-4-4-12 hex digits).
fn is_session_id(value: &[u8]) -> bool {
    value.len() == 36
        && value.iter().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => *b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(b),
        })
}

/// Reads the result of `--has-conversation` (NS-39), like `parse_check`. Exit 0 with exactly the
/// line `conversation=false` is `None` (the conversation is lost); exit 0 with exactly the lines
/// `conversation=true` and `session=<lowercase UUID>` (NS-50: the last session of the chain of
/// `continued-in`, the session given when there is none) is that session. With or without the
/// final line break, whatever the stderr. Anything else on stdout is an error, so neither a
/// conversation nor a session is ever presumed; a failure is the stderr of the launcher, or a
/// message with the code.
pub fn parse_has_conversation(
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<Option<String>, String> {
    let stderr = String::from_utf8_lossy(stderr).trim_end().to_string();
    if code != Some(0) {
        return Err(launcher_failure(code, stderr, "verificação da conversa"));
    }
    let unexpected = || UNEXPECTED_CONVERSATION_MESSAGE.to_string();
    let stdout = stdout.strip_suffix(b"\n").unwrap_or(stdout);
    let lines: Vec<&[u8]> = stdout.split(|b| *b == b'\n').collect();
    match lines.as_slice() {
        [line] if flag_line(line, "conversation") == Some(false) => Ok(None),
        [line, session] if flag_line(line, "conversation") == Some(true) => {
            let session = session
                .strip_prefix(b"session=")
                .filter(|session| is_session_id(session))
                .ok_or_else(unexpected)?;
            Ok(Some(String::from_utf8_lossy(session).into_owned()))
        }
        _ => Err(unexpected()),
    }
}

/// Runs `bash <launcher> --has-conversation <session_id>` with the cwd at `project`, the stdin
/// closed and the output captured (outside the PTY), and reads it with `parse_has_conversation`:
/// whether claude still keeps the conversation of the agent, before a resume (NS-39), and the
/// session it went on in (NS-50).
pub fn run_has_conversation(
    launcher: &Path,
    project: &Path,
    session_id: &str,
) -> Result<Option<String>, String> {
    run_has_conversation_with_env(launcher, project, session_id, &[])
}

/// `run_has_conversation` with `env` added to the environment of the launcher (the tests point
/// CLAUDE_CONFIG_DIR at a temporary dir).
pub(crate) fn run_has_conversation_with_env(
    launcher: &Path,
    project: &Path,
    session_id: &str,
    env: &[(&str, &str)],
) -> Result<Option<String>, String> {
    let mut command = Command::new("bash");
    command
        .arg(launcher)
        .arg("--has-conversation")
        .arg(session_id)
        .current_dir(project)
        .stdin(Stdio::null());
    for (key, value) in env {
        command.env(key, value);
    }
    let output = command
        .output()
        .map_err(|err| format!("Não consegui rodar o bin/nick-agent: {err}."))?;
    parse_has_conversation(output.status.code(), &output.stdout, &output.stderr)
}

/// The answers of a form, for `--render`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderRequest {
    pub kind: StageType,
    /// The `--render` arguments and their values, in order (stages::render_args).
    pub answers: Vec<(&'static str, String)>,
    /// Where the log of the prompt goes (NS-52: `paths::prompt_logs_dir`), given to the launcher
    /// as NICK_PROMPT_LOGS_DIR over any value in the environment; None leaves the environment as
    /// it is (the tests that point it at a temporary dir themselves).
    pub logs_dir: Option<PathBuf>,
}

/// What a successful `--render` gave: the log with the prompt, read for the preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rendered {
    /// The log of the prompt (auto_scrum/logs/<type>_<date>.md), absolute.
    pub log: PathBuf,
    /// Its content: the prompt, as bash built it.
    pub prompt: String,
    /// The jail status (`jail_print_status`), the lines after `log=`.
    pub status: String,
    /// What the render wrote on stderr (the warnings of the config, the plan in the old place).
    pub warning: Option<String>,
}

/// The log and the status of a `--render`, before the log is read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderOutput {
    pub log: PathBuf,
    pub status: String,
    pub warning: Option<String>,
}

/// Reads the result of `--render`, like `parse_check`: a failure is the stderr of the launcher
/// (its usage error or the message of the lib), or a message with the code; exit 0 must start
/// with the line `log=<absolute path>`, and the rest of the stdout is the jail status.
pub fn parse_render(
    code: Option<i32>,
    stdout: &[u8],
    stderr: &[u8],
) -> Result<RenderOutput, String> {
    let stderr = String::from_utf8_lossy(stderr).trim_end().to_string();
    if code != Some(0) {
        return Err(launcher_failure(code, stderr, "montagem do prompt"));
    }
    let (first, rest) = match stdout.iter().position(|b| *b == b'\n') {
        Some(at) => (&stdout[..at], &stdout[at + 1..]),
        None => (stdout, &b""[..]),
    };
    let log = first
        .strip_prefix(b"log=")
        .map(|path| PathBuf::from(OsStr::from_bytes(path)))
        .filter(|path| path.is_absolute())
        .ok_or_else(|| UNEXPECTED_RENDER_MESSAGE.to_string())?;
    let status = String::from_utf8_lossy(rest).trim_end().to_string();
    let warning = (!stderr.trim().is_empty()).then_some(stderr);
    Ok(RenderOutput {
        log,
        status,
        warning,
    })
}

/// Runs `bash <launcher> --render <type> <answers>` with the cwd at `project`, the stdin closed
/// and the output captured (outside the PTY), reads it with `parse_render` and then reads the
/// log for the preview (reading the log is not building the prompt). A log that cannot be read,
/// or is not UTF-8, is an error with its path.
pub fn run_render(
    launcher: &Path,
    project: &Path,
    request: &RenderRequest,
) -> Result<Rendered, String> {
    run_render_with_env(launcher, project, request, &[])
}

/// `run_render` with `env` added to the environment of the launcher (the tests point
/// NICK_PROMPT_LOGS_DIR at a temporary dir, so no log goes to auto_scrum/logs/).
pub(crate) fn run_render_with_env(
    launcher: &Path,
    project: &Path,
    request: &RenderRequest,
    env: &[(&str, &str)],
) -> Result<Rendered, String> {
    let mut command = Command::new("bash");
    command
        .arg(launcher)
        .arg("--render")
        .arg(request.kind.key())
        .current_dir(project)
        .stdin(Stdio::null());
    for (flag, value) in &request.answers {
        command.arg(flag).arg(value);
    }
    for (key, value) in env {
        command.env(key, value);
    }
    // After the environment: the dir of the request always wins (NS-52).
    if let Some(dir) = &request.logs_dir {
        command.env("NICK_PROMPT_LOGS_DIR", dir);
    }
    let output = command
        .output()
        .map_err(|err| format!("Não consegui rodar o bin/nick-agent: {err}."))?;
    let out = parse_render(output.status.code(), &output.stdout, &output.stderr)?;
    let prompt = std::fs::read_to_string(&out.log).map_err(|err| {
        format!(
            "Não consegui ler o prompt gerado em {}: {err}.",
            out.log.display()
        )
    })?;
    Ok(Rendered {
        log: out.log,
        prompt,
        status: out.status,
        warning: out.warning,
    })
}

/// Opens an agent: the check before the form, the render of the prompt and the launch in the
/// PTY.
pub trait AgentLauncher {
    /// `bash <launcher> --check` with the cwd at `project`.
    fn check(&mut self, launcher: &Path, project: &Path) -> Result<Precheck, String>;
    /// `bash <launcher> --render <type> <answers>` with the cwd at `project`.
    fn render(
        &mut self,
        launcher: &Path,
        project: &Path,
        request: &RenderRequest,
    ) -> Result<Rendered, String>;
    /// `bash <launcher> --has-conversation <session_id>` with the cwd at `project` (NS-39):
    /// `None` when the conversation is lost, or the last session of its chain (NS-50).
    fn has_conversation(
        &mut self,
        launcher: &Path,
        project: &Path,
        session_id: &str,
    ) -> Result<Option<String>, String>;
    /// Starts `bash <launcher> <arguments of launch>` in a PTY with the cwd at `project`, for
    /// the agent whose row has the id `agent_id` (its events carry it): a new one (`--session-id
    /// <session_id> --name <name> --type <kind> --prompt-file <prompt_file>`) or a resume
    /// (NS-39, `Launch::args`). The error is the system's.
    fn spawn(
        &mut self,
        launcher: &Path,
        project: &Path,
        agent_id: i64,
        session_id: &str,
        name: &str,
        launch: Launch,
    ) -> Result<(), String>;
}

/// The size (rows, columns) of the PTY of an agent, from its name and whether it runs in the
/// jail (its header, which wraps, takes rows from its terminal: ui.rs).
pub type SizeOf = Box<dyn Fn(&str, bool) -> (u16, u16)>;

/// The real launcher: the check by `run_check`, the render by `run_render`, and the launch in an
/// `AgentSession` whose events go to `tx`, of the size `size_of` gives for its name and the jail
/// (review NS-34, P2: the PTY is born with its final size): the jail of the last check, or the one
/// `with_jail` gives (the launch does not run the check again: NS-35). The loop takes the session
/// from `session`.
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

    /// The jail of the agent, known from its check (the size of its header depends on it).
    pub fn with_jail(mut self, jail: bool) -> TerminalAgentLauncher<E> {
        self.jail = jail;
        self
    }
}

impl<E: From<AgentEvent> + Send + 'static> AgentLauncher for TerminalAgentLauncher<E> {
    fn check(&mut self, launcher: &Path, project: &Path) -> Result<Precheck, String> {
        let check = run_check(launcher, project)?;
        self.jail = check.jail;
        Ok(check)
    }

    fn render(
        &mut self,
        launcher: &Path,
        project: &Path,
        request: &RenderRequest,
    ) -> Result<Rendered, String> {
        run_render(launcher, project, request)
    }

    fn has_conversation(
        &mut self,
        launcher: &Path,
        project: &Path,
        session_id: &str,
    ) -> Result<Option<String>, String> {
        run_has_conversation(launcher, project, session_id)
    }

    fn spawn(
        &mut self,
        launcher: &Path,
        project: &Path,
        agent_id: i64,
        session_id: &str,
        name: &str,
        launch: Launch,
    ) -> Result<(), String> {
        let session = AgentSession::spawn(
            launcher,
            project,
            agent_id,
            session_id,
            name,
            launch,
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
    use crate::agents::Stage;
    use crate::resources::Resources;
    use std::fs;
    use std::path::Component;

    /// `launcher_path` on the checkout (the root of the tests).
    fn launcher_path() -> PathBuf {
        super::launcher_path(Resources::checkout().root())
    }

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

    /// A precheck without a warning.
    fn pre(jail: bool, jira: bool, sentry: bool) -> Precheck {
        Precheck {
            jail,
            jira,
            sentry,
            warning: None,
        }
    }

    #[test]
    fn jail_true_and_jail_false_are_read() {
        assert_eq!(
            parse_check(Some(0), b"jail=true\njira=false\nsentry=false\n", b""),
            Ok(pre(true, false, false))
        );
        assert_eq!(
            parse_check(Some(0), b"jail=false\njira=false\nsentry=false\n", b""),
            Ok(pre(false, false, false))
        );
    }

    #[test]
    fn the_flags_of_jira_and_sentry_are_read() {
        for jail in [true, false] {
            for jira in [true, false] {
                for sentry in [true, false] {
                    let stdout = format!("jail={jail}\njira={jira}\nsentry={sentry}\n");
                    assert_eq!(
                        parse_check(Some(0), stdout.as_bytes(), b""),
                        Ok(pre(jail, jira, sentry)),
                        "{stdout:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn any_other_stdout_with_exit_0_is_an_error() {
        for stdout in [
            &b""[..],
            b"\n",
            // The one line of the NS-33, alone.
            b"jail=true\n",
            b"jail=false\n",
            b"jail=TRUE\njira=false\nsentry=false\n",
            b"jail=1\njira=false\nsentry=false\n",
            b"jail=true\njail=true\n",
            b"Configura\xc3\xa7\xc3\xa3o carregada\njail=true\njira=false\nsentry=false\n",
            b"jail=true\njira=false\nsentry=false\nmais\n",
            b" jail=true\njira=false\nsentry=false\n",
            b"jail=truex\njira=false\nsentry=false\n",
            // The order swapped, a line missing, a value not exact, a line too many.
            b"jira=false\njail=true\nsentry=false\n",
            b"jail=true\nsentry=false\njira=false\n",
            b"jail=true\njira=false\n",
            b"jail=true\nsentry=false\n",
            b"jail=true\njira=TRUE\nsentry=false\n",
            b"jail=true\njira=false\nsentry=sim\n",
            b"jail=true\njira=false\nsentry=false\n\n",
            b"jail=true\njira=false\nsentry=false\njail=true\n",
            b"jail=true\r\njira=false\r\nsentry=false\r\n",
        ] {
            assert_eq!(
                parse_check(Some(0), stdout, b""),
                Err(UNEXPECTED_CHECK_MESSAGE.to_string()),
                "{:?}",
                String::from_utf8_lossy(stdout)
            );
        }
        // Without the final line break, the lines are still the lines.
        assert!(parse_check(Some(0), b"jail=false\njira=true\nsentry=false", b"").is_ok());
    }

    #[test]
    fn the_stderr_of_a_success_is_a_warning() {
        assert_eq!(
            parse_check(
                Some(0),
                b"jail=false\njira=false\nsentry=false\n",
                b"Aviso: chave ausente\n  X=\"\"\n"
            ),
            Ok(Precheck {
                warning: Some("Aviso: chave ausente\n  X=\"\"".into()),
                ..pre(false, false, false)
            })
        );
        assert_eq!(
            parse_check(Some(0), b"jail=true\njira=false\nsentry=false\n", b"  \n"),
            Ok(pre(true, false, false)),
            "blank stderr is no warning"
        );
    }

    #[test]
    fn a_failure_is_the_stderr_or_the_code() {
        assert_eq!(
            parse_check(Some(1), b"", b"Erro: a jail falhou.\nDetalhe\n"),
            Err("Erro: a jail falhou.\nDetalhe".into())
        );
        // Even with the three lines: a non-zero exit never opens.
        assert_eq!(
            parse_check(
                Some(1),
                b"jail=false\njira=false\nsentry=false\n",
                b"Erro: x\n"
            ),
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
    fn a_render_gives_the_log_the_status_and_the_warning() {
        assert_eq!(
            parse_render(
                Some(0),
                b"log=/x/logs/po_20261119_120000.md\nJail (ai-jail): desligada.\n",
                b""
            ),
            Ok(RenderOutput {
                log: PathBuf::from("/x/logs/po_20261119_120000.md"),
                status: "Jail (ai-jail): desligada.".into(),
                warning: None
            })
        );
        assert_eq!(
            parse_render(Some(0), b"log=/x/l.md\na\nb\n", b"Aviso: plano antigo\n"),
            Ok(RenderOutput {
                log: PathBuf::from("/x/l.md"),
                status: "a\nb".into(),
                warning: Some("Aviso: plano antigo".into())
            })
        );
        assert_eq!(
            parse_render(Some(0), b"log=/x/l.md", b"").map(|o| o.status),
            Ok(String::new())
        );
    }

    #[test]
    fn a_render_without_an_absolute_log_or_that_failed_is_an_error() {
        for stdout in [
            &b""[..],
            b"log=\n",
            b"log=logs/x_1.md\nstatus\n",
            b"status\nlog=/x/l.md\n",
            b" log=/x/l.md\n",
            b"LOG=/x/l.md\n",
        ] {
            assert_eq!(
                parse_render(Some(0), stdout, b""),
                Err(UNEXPECTED_RENDER_MESSAGE.to_string()),
                "{:?}",
                String::from_utf8_lossy(stdout)
            );
        }
        assert_eq!(
            parse_render(
                Some(1),
                b"log=/x/l.md\n",
                b"Erro: o tipo 'x' nao existe.\nUso: ...\n"
            ),
            Err("Erro: o tipo 'x' nao existe.\nUso: ...".into())
        );
        assert_eq!(
            parse_render(Some(3), b"", b""),
            Err("O bin/nick-agent falhou na montagem do prompt (código 3).".into())
        );
        assert_eq!(
            parse_render(None, b"", b""),
            Err("O bin/nick-agent foi interrompido na montagem do prompt.".into())
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
        // With the jail off (no ai-jail involved), jail=false, the flags and the warnings of the
        // config.
        fs::create_dir_all(proj.join(".nick")).unwrap();
        fs::write(
            proj.join(".nick/nick.conf"),
            "JAIL_ENABLED=\"false\"\nJIRA_ENABLED=\"true\"\nSENTRY_ENABLED=\"TRUE\"\n",
        )
        .unwrap();
        let check = run_check(&launcher_path(), &proj).unwrap();
        assert!(!check.jail);
        assert!(check.jira, "jira=true with \"true\"");
        assert!(!check.sentry, "sentry=false with \"TRUE\" (not exact)");
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

    #[test]
    fn run_render_runs_the_real_launcher_and_reads_the_log() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let proj = root.join("proj");
        let logs = root.join("logs");
        fs::create_dir_all(proj.join(".nick")).unwrap();
        fs::write(proj.join(".nick/nick.conf"), "JAIL_ENABLED=\"false\"\n").unwrap();
        let logs_env = logs.display().to_string();
        let env = [("NICK_PROMPT_LOGS_DIR", logs_env.as_str())];
        let request = RenderRequest {
            kind: StageType::PoDiscussion,
            answers: vec![("--title", "Assunto de teste".into())],
            logs_dir: None,
        };
        let rendered = run_render_with_env(&launcher_path(), &proj, &request, &env).unwrap();
        assert_eq!(rendered.log.parent(), Some(logs.as_path()), "{rendered:?}");
        let name = rendered
            .log
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        assert!(
            name.starts_with("po_discussion_") && name.ends_with(".md"),
            "{name}"
        );
        assert_eq!(rendered.prompt, fs::read_to_string(&rendered.log).unwrap());
        assert!(rendered.prompt.contains("Assunto de teste"), "{rendered:?}");
        assert_eq!(
            rendered.status,
            "Jail (ai-jail): desligada — o agente roda direto no seu shell."
        );
        assert!(
            rendered
                .warning
                .as_deref()
                .is_some_and(|w| w.contains("não tem todas as chaves")),
            "{rendered:?}"
        );
        assert!(
            !proj.join(".nick/.gitignore").exists(),
            "--render writes nothing in the project"
        );
        // A refusal of the launcher: its usage error, and no log.
        let bad = RenderRequest {
            kind: StageType::Po,
            answers: vec![("--title", "x".into())],
            logs_dir: None,
        };
        let err = run_render_with_env(&launcher_path(), &proj, &bad, &env).unwrap_err();
        assert!(
            err.starts_with("Erro: falta a resposta --description do tipo 'po'."),
            "{err}"
        );
        assert_eq!(
            fs::read_dir(&logs).unwrap().count(),
            1,
            "only the first log"
        );
    }

    /// NS-52: the dir of the request wins over a NICK_PROMPT_LOGS_DIR of the environment (the
    /// nick installed must never write in its prefix, nor in auto_scrum/logs/).
    #[test]
    fn run_render_writes_the_log_in_the_dir_of_the_request() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let proj = root.join("proj");
        let wanted = root.join("state/nick/logs/proj-1");
        let other = root.join("other");
        fs::create_dir_all(proj.join(".nick")).unwrap();
        fs::write(proj.join(".nick/nick.conf"), "JAIL_ENABLED=\"false\"\n").unwrap();
        let other_env = other.display().to_string();
        let env = [("NICK_PROMPT_LOGS_DIR", other_env.as_str())];
        let request = RenderRequest {
            kind: StageType::PoDiscussion,
            answers: vec![("--title", "Assunto de teste".into())],
            logs_dir: Some(wanted.clone()),
        };
        let rendered = run_render_with_env(&launcher_path(), &proj, &request, &env).unwrap();
        assert_eq!(
            rendered.log.parent(),
            Some(wanted.as_path()),
            "{rendered:?}"
        );
        assert!(rendered.log.is_file());
        assert!(!other.exists(), "nothing in the dir of the environment");
    }

    #[test]
    fn a_log_that_cannot_be_read_is_an_error_with_its_path() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("fake-agent");
        // A launcher that answers a log that does not exist.
        fs::write(&fake, "echo log=/nao/existe/po_x.md\necho status\n").unwrap();
        let request = RenderRequest {
            kind: StageType::Po,
            answers: Vec::new(),
            logs_dir: None,
        };
        let err = run_render(&fake, tmp.path(), &request).unwrap_err();
        assert!(
            err.starts_with("Não consegui ler o prompt gerado em /nao/existe/po_x.md:"),
            "{err}"
        );
        // Not UTF-8.
        let log = tmp.path().join("log.md");
        fs::write(&log, b"\xff\xfe").unwrap();
        fs::write(&fake, format!("echo log={}\n", log.display())).unwrap();
        let err = run_render(&fake, tmp.path(), &request).unwrap_err();
        assert!(err.contains(&log.display().to_string()), "{err}");
    }

    // ---- NS-39: the check of the conversation before a resume ----

    #[test]
    fn the_conversation_check_is_false_or_true_with_the_session() {
        let session = "0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a60";
        let found = format!("conversation=true\nsession={session}\n");
        assert_eq!(
            parse_has_conversation(Some(0), found.as_bytes(), b""),
            Ok(Some(session.to_string()))
        );
        // Without the final line break too.
        assert_eq!(
            parse_has_conversation(Some(0), found.trim_end().as_bytes(), b""),
            Ok(Some(session.to_string()))
        );
        assert_eq!(
            parse_has_conversation(Some(0), b"conversation=false", b""),
            Ok(None)
        );
        assert_eq!(
            parse_has_conversation(Some(0), b"conversation=false\n", b""),
            Ok(None)
        );
        // A warning on stderr does not change a success (the locale, say).
        assert_eq!(
            parse_has_conversation(Some(0), found.as_bytes(), b"aviso"),
            Ok(Some(session.to_string()))
        );
    }

    /// NS-50: anything out of the new format is refused with the message of before.
    #[test]
    fn the_conversation_check_refuses_anything_else() {
        let session = "0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a60";
        let upper = session.to_uppercase();
        let refused = [
            String::new(),
            "conversation=true\n".to_string(),
            "conversation=true".to_string(),
            "conversation=TRUE\n".to_string(),
            "conversation=true\nconversation=true\n".to_string(),
            "conversation=true\nsession=\n".to_string(),
            format!("conversation=true\nsession={upper}\n"),
            format!("conversation=true\nsession={}\n", &session[..35]),
            format!("conversation=true\nsession={session}x\n"),
            format!("conversation=true\nsession=../{}\n", &session[3..]),
            format!("conversation=true\nsession= {session}\n"),
            format!("conversation=true\nsession={session}\nmais\n"),
            format!("conversation=true\nsession={session}\n\n"),
            format!("session={session}\nconversation=true\n"),
            format!("conversation=false\nsession={session}\n"),
            "jail=true\n".to_string(),
            " conversation=true\n".to_string(),
            "conversation=".to_string(),
        ];
        for stdout in refused {
            assert_eq!(
                parse_has_conversation(Some(0), stdout.as_bytes(), b""),
                Err(UNEXPECTED_CONVERSATION_MESSAGE.to_string()),
                "{stdout:?}"
            );
        }
        assert_eq!(
            UNEXPECTED_CONVERSATION_MESSAGE,
            "O bin/nick-agent respondeu algo inesperado na verificação da conversa."
        );
    }

    #[test]
    fn a_failing_conversation_check_is_the_stderr_or_the_code() {
        assert_eq!(
            parse_has_conversation(Some(1), b"", b"Erro: x\nUso: y\n"),
            Err("Erro: x\nUso: y".to_string())
        );
        assert_eq!(
            parse_has_conversation(Some(2), b"conversation=true\n", b""),
            Err("O bin/nick-agent falhou na verificação da conversa (código 2).".to_string())
        );
        assert_eq!(
            parse_has_conversation(None, b"", b""),
            Err("O bin/nick-agent foi interrompido na verificação da conversa.".to_string())
        );
    }

    #[test]
    fn run_has_conversation_runs_the_real_launcher_in_the_project() {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let proj = root.join("meu proj");
        fs::create_dir_all(&proj).unwrap();
        let config = root.join("cfg");
        let config_env = config.display().to_string();
        let env = [("CLAUDE_CONFIG_DIR", config_env.as_str())];
        let session = "0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a60";
        assert_eq!(
            run_has_conversation_with_env(&launcher_path(), &proj, session, &env),
            Ok(None),
            "no config is needed"
        );
        let dir = config.join("projects").join(
            proj.display()
                .to_string()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect::<String>(),
        );
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(format!("{session}.jsonl")), "{}\n").unwrap();
        assert_eq!(
            run_has_conversation_with_env(&launcher_path(), &proj, session, &env),
            Ok(Some(session.to_string()))
        );
        // NS-50: the chain session → b → c, in the real format of the record.
        let next = "0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a61";
        let last = "0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a62";
        let record = |from: &str, to: &str| {
            format!(
                "{{\"type\":\"continued-in\",\"timestamp\":\"2026-11-21T14:41:33.908Z\",\
                 \"sessionId\":\"{from}\",\"continuedInSessionId\":\"{to}\"}}\n"
            )
        };
        fs::write(dir.join(format!("{session}.jsonl")), record(session, next)).unwrap();
        fs::write(dir.join(format!("{next}.jsonl")), record(next, last)).unwrap();
        fs::write(dir.join(format!("{last}.jsonl")), "{}\n").unwrap();
        assert_eq!(
            run_has_conversation_with_env(&launcher_path(), &proj, session, &env),
            Ok(Some(last.to_string()))
        );
        assert_eq!(
            fs::read_dir(&proj).unwrap().count(),
            0,
            "nothing written in the project"
        );
        let err = run_has_conversation_with_env(&launcher_path(), &proj, "", &env).unwrap_err();
        assert!(err.starts_with("Erro: "), "{err}");
    }

    /// NS-40: the resume of an agent of an epic gives `--epic-id` in the place of `--ticket-id`;
    /// the one of a ticket is the argv of the NS-39.
    #[test]
    fn the_args_of_a_resume_name_the_ticket_or_the_epic() {
        let args = |holder_key| {
            Launch::Resume {
                stage: Stage::TechLeader,
                opened_key: Some("E-1"),
                holder_key,
            }
            .args("s-1", "p · Tech Leader · E-1")
            .into_iter()
            .map(|arg| arg.into_string().unwrap())
            .collect::<Vec<_>>()
        };
        let head = [
            "--resume",
            "s-1",
            "--name",
            "p · Tech Leader · E-1",
            "--stage",
            "tech_leader",
            "--opened-id",
            "E-1",
        ];
        let mut ticket = head.to_vec();
        ticket.extend(["--ticket-id", "NS-9"]);
        assert_eq!(args(HolderKey::Ticket("NS-9")), ticket);
        let mut epic = head.to_vec();
        epic.extend(["--epic-id", "E-2"]);
        assert_eq!(args(HolderKey::Epic("E-2")), epic);
    }
}
