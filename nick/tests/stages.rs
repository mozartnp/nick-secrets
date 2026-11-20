//! The differential of the stages (NS-35): for the 8 types, the answers built the way the App
//! builds them (stages.rs, from raw values with spaces around them) and sent by the real
//! `launch::run_render` give the same log as `main()` of auto_scrum.sh with the same answers,
//! byte for byte, and the real `AgentSession::spawn` with that log gives the argv of claude of
//! the script, but for `--session-id <uuid> --name <name>`. It covers the mapping field →
//! argument that the bash differential (bin/tests/test_nick_agent.sh) does not see. The fake
//! claude of bin/tests/fixtures/bin records the argv; nothing real is launched, and no log goes
//! to auto_scrum/logs/ (NICK_PROMPT_LOGS_DIR).

use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use nick::agent_session::{AgentEvent, AgentSession};
use nick::launch::{self, Launch, RenderRequest};
use nick::stages::{self, Input, Rule, StageType};

const SESSION_ID: &str = "0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a60";
const NAME: &str = "proj · Etapa · NS-1";
/// What the fake editor of the script writes, and the text the App keeps from the editor (the
/// file without the final line breaks, as `$(cat ...)` and editor.rs take it).
const EDITOR_TEXT: &str = "  Descrição longa de teste\ncom duas linhas.  \n\n";
const EDITOR_VALUE: &str = "  Descrição longa de teste\ncom duas linhas.  ";

fn repo(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(rel)
}

/// The raw answers of `kind`, as typed (spaces and tabs around them, which the read of the script
/// and the trim of the App take away, and a no-break space, which both keep), one per field; the
/// editor ones as the editor gives them.
fn raw_values(kind: StageType) -> Vec<String> {
    kind.fields()
        .iter()
        .map(|field| match (field.input, field.rule) {
            (Input::Editor, _) => EDITOR_VALUE.to_string(),
            (_, Rule::JiraId) => " NS-1\t".to_string(),
            (_, Rule::Plan) => match kind {
                StageType::Review => "  outro/plano.md ".to_string(),
                _ => String::new(),
            },
            (_, Rule::Optional) => match kind {
                StageType::PoJira => "\t extra do jira ".to_string(),
                _ => "   ".to_string(),
            },
            // A no-break space at the start: read keeps it (it is not in the IFS), and so does
            // the trim of the App; str::trim would take it.
            (_, Rule::Required) => format!(" \u{a0}resposta de {} \t", field.flag),
        })
        .collect()
}

/// Runs `main()` of auto_scrum.sh in `project` with `kind` chosen on its menu (Jira and Sentry
/// on), the line answers of `values` on the stdin, the fake editor, and `s`; its log in
/// `logs` and the argv of claude in `argv`.
fn run_script(
    project: &Path,
    kind: StageType,
    values: &[String],
    root: &Path,
) -> (PathBuf, String) {
    let logs = root.join(format!("script_logs_{}", kind.key()));
    let argv = root.join(format!("script_{}.argv", kind.key()));
    let editor = root.join("script_editor");
    fs::write(
        &editor,
        format!("#!/usr/bin/env bash\nprintf '%s' '{EDITOR_TEXT}' > \"$1\"\n"),
    )
    .unwrap();
    fs::set_permissions(&editor, fs::Permissions::from_mode(0o755)).unwrap();
    let number = StageType::menu(true, true)
        .iter()
        .position(|k| *k == kind)
        .unwrap()
        + 1;
    let mut stdin = format!("{number}\n");
    for (field, value) in kind.fields().iter().zip(values) {
        if field.input == Input::Line {
            stdin.push_str(value);
            stdin.push('\n');
        }
    }
    stdin.push_str("s\n");
    let mut child = Command::new("bash")
        .arg("-c")
        .arg(format!(
            "source '{}'; check_requirements() {{ :; }}; LOGS_DIR='{}'; main",
            repo("auto_scrum/auto_scrum.sh").display(),
            logs.display()
        ))
        .current_dir(project)
        .env("EDITOR", &editor)
        .env("FAKE_CLAUDE_LOG", &argv)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}: the script failed: {}",
        kind.key(),
        String::from_utf8_lossy(&output.stderr)
    );
    let log = fs::read_dir(&logs)
        .unwrap()
        .next()
        .expect("the log of the script")
        .unwrap()
        .path();
    (log, fs::read_to_string(&argv).unwrap())
}

/// `argv` without the 4 lines `--session-id <uuid> --name <name>`.
fn without_session(argv: &str) -> String {
    argv.replacen(
        &format!("--session-id\n{SESSION_ID}\n--name\n{NAME}\n"),
        "",
        1,
    )
}

#[test]
fn the_app_and_the_script_build_the_same_prompt_and_argv_for_every_type() {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let project = root.join("proj");
    fs::create_dir_all(project.join(".nick")).unwrap();
    fs::write(
        project.join(".nick/nick.conf"),
        [
            "STACK_DESCRIPTION=\"Bash e Rust\"",
            "JIRA_ENABLED=\"true\"",
            "SENTRY_ENABLED=\"true\"",
            "PRODUCTION_ENABLED=\"true\"",
            "PERMISSION_ENABLED=\"true\"",
            "TDD_ENABLED=\"true\"",
            "JAIL_ENABLED=\"false\"",
            "",
        ]
        .join("\n"),
    )
    .unwrap();
    let agent_argv = root.join("agent.argv");
    let path = format!(
        "{}:{}",
        repo("bin/tests/fixtures/bin").display(),
        std::env::var("PATH").unwrap_or_default()
    );
    // SAFETY: the only test of this binary, before any thread of its own: the launcher (run by
    // run_render and by the PTY of AgentSession) inherits the fake claude in the PATH, its log
    // and the dir of the logs of the prompts.
    unsafe {
        std::env::set_var("PATH", &path);
        std::env::set_var("FAKE_CLAUDE_LOG", &agent_argv);
        std::env::remove_var("FAKE_CLAUDE_MODE");
        std::env::set_var("NICK_PROMPT_LOGS_DIR", root.join("agent_logs"));
    }
    for kind in StageType::ALL {
        let values = raw_values(kind);
        let answers = stages::render_args(kind, &values).expect("the answers pass");
        let rendered = launch::run_render(
            &launch::launcher_path(),
            &project,
            &RenderRequest { kind, answers },
        )
        .unwrap_or_else(|err| panic!("{}: {err}", kind.key()));
        let (script_log, script_argv) = run_script(&project, kind, &values, &root);
        assert_eq!(
            fs::read(&rendered.log).unwrap(),
            fs::read(&script_log).unwrap(),
            "{}: the log of the shell is the log of the script, byte for byte",
            kind.key()
        );
        // A guard of the sample: the answers reached the prompt, trimmed (and the editor text
        // as it is).
        if kind == StageType::PoJira {
            assert!(
                rendered.prompt.contains("extra do jira"),
                "{}",
                rendered.prompt
            );
            assert!(!rendered.prompt.contains("\t extra do jira"));
        }
        if kind == StageType::Po {
            assert!(
                rendered.prompt.contains(EDITOR_VALUE),
                "{}",
                rendered.prompt
            );
            assert!(rendered.prompt.contains("\u{a0}resposta de --title"));
        }
        let _ = fs::remove_file(&agent_argv);
        let (tx, rx) = mpsc::channel::<AgentEvent>();
        let mut session = AgentSession::spawn(
            &launch::launcher_path(),
            &project,
            1,
            SESSION_ID,
            NAME,
            Launch::New {
                kind,
                prompt_file: &rendered.log,
            },
            (24, 80),
            tx,
        )
        .unwrap();
        let deadline = Instant::now() + Duration::from_secs(15);
        while session.exit_code().is_none() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert_eq!(session.exit_code(), Some(0), "{}", kind.key());
        session.wait();
        drop(rx);
        let agent = fs::read_to_string(&agent_argv).unwrap();
        assert!(
            agent.contains(&format!("--session-id\n{SESSION_ID}\n--name\n{NAME}\n")),
            "{}: {agent}",
            kind.key()
        );
        assert_eq!(
            without_session(&agent),
            script_argv,
            "{}: the argv of claude is the one of the script, but for --session-id/--name",
            kind.key()
        );
        let auto = "--permission-mode\nauto\n";
        assert_eq!(
            agent.contains(auto),
            kind == StageType::Development,
            "{}: auto mode only in the Development stage",
            kind.key()
        );
    }
    assert_eq!(
        fs::read_dir(root.join("agent_logs")).unwrap().count(),
        StageType::ALL.len(),
        "one log per type, in the dir of the test"
    );
}
