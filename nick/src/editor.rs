//! The long texts of the stage forms in `$EDITOR` (NS-35), as `read_via_editor` of
//! auto_scrum.sh does: the TUI is suspended (terminal::run_suspended, the path of the init, with
//! the nick immune to Ctrl+C and Ctrl+\ meanwhile), the editor opens on a temporary file, and its
//! content comes back without the final line breaks (`$(cat ...)`). Running it goes through the
//! `EditorRunner` trait, so the TUI state (app.rs) is tested with a fake one.
//!
//! The editor is `$EDITOR`, or nano when it is not set or empty (`${EDITOR:-nano}`), run as one
//! program, never split at the spaces (as `"$editor" "$tmpfile"`: `EDITOR="code -w"` fails in
//! both). It is run by bash (`exec "$0" "$1"`), the only program the shell starts with
//! `Command` (tests/agent.rs checks the sources), which also looks it up in the PATH as the
//! script does.

use std::ffi::{OsStr, OsString};
use std::io::{self, Write};
use std::process::Command;

use crate::terminal::{self, IgnoreTerminalSignals, InputGate, TerminalGuard};

/// The editor of `${EDITOR:-nano}`: `editor` (the value of EDITOR) when set and not empty, nano
/// otherwise.
pub fn editor_of(editor: Option<&OsStr>) -> OsString {
    match editor.filter(|value| !value.is_empty()) {
        Some(value) => value.to_os_string(),
        None => OsString::from("nano"),
    }
}

/// How an edit ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorOutcome {
    /// The file was saved with this text (the final line breaks dropped).
    Text(String),
    /// The file was left empty (size 0): `read_via_editor` asks again.
    Empty,
    /// The editor could not be run (or the text could not be read): why.
    Failed { editor: String, error: String },
}

/// Opens the editor on `text` for the field `label`.
pub trait EditorRunner {
    fn edit(&mut self, label: &str, text: &str) -> EditorOutcome;
}

/// The real runner: on the normal terminal, with the input thread paused.
pub struct TerminalEditorRunner<'a> {
    guard: &'a mut TerminalGuard,
    gate: &'a InputGate,
    editor: OsString,
    /// A failure to enter the TUI again, for the loop to end with it.
    pub error: Option<io::Error>,
}

impl<'a> TerminalEditorRunner<'a> {
    /// `editor` is the value of EDITOR (main.rs reads the environment).
    pub fn new(
        guard: &'a mut TerminalGuard,
        gate: &'a InputGate,
        editor: Option<&OsStr>,
    ) -> TerminalEditorRunner<'a> {
        TerminalEditorRunner {
            guard,
            gate,
            editor: editor_of(editor),
            error: None,
        }
    }
}

impl EditorRunner for TerminalEditorRunner<'_> {
    fn edit(&mut self, label: &str, text: &str) -> EditorOutcome {
        let editor = self.editor.clone();
        match terminal::run_suspended(self.guard, self.gate, || run_editor(&editor, label, text)) {
            Ok(outcome) => outcome,
            Err(err) => {
                let outcome = failed(&editor, err.to_string());
                self.error = Some(err);
                outcome
            }
        }
    }
}

fn failed(editor: &OsStr, error: String) -> EditorOutcome {
    EditorOutcome::Failed {
        editor: editor.to_string_lossy().into_owned(),
        error,
    }
}

/// Runs `editor` on a temporary file with `text`, on the terminal as it is (inherited), immune
/// to Ctrl+C and Ctrl+\ (`IgnoreTerminalSignals`), and reads the file back. The temporary file is
/// always removed (`NamedTempFile`). No "Pressione Enter" on the way back: the editor is
/// interactive, and the script does not wait either.
pub fn run_editor(editor: &OsStr, label: &str, text: &str) -> EditorOutcome {
    let _immune = IgnoreTerminalSignals::new();
    println!(
        "{label} — abrindo {} (salve e feche o editor para continuar).",
        editor.to_string_lossy()
    );
    let _ = io::stdout().flush();
    let mut file = match tempfile::NamedTempFile::new() {
        Ok(file) => file,
        Err(err) => return failed(editor, err.to_string()),
    };
    if let Err(err) = file.write_all(text.as_bytes()).and_then(|()| file.flush()) {
        return failed(editor, err.to_string());
    }
    // `command -v` first: an editor that is not there is told apart from one that exits with an
    // error. The editor is a single word ("$0"), as in the script.
    let status = Command::new("bash")
        .arg("-c")
        .arg("command -v -- \"$0\" > /dev/null || exit 127; exec \"$0\" \"$1\"")
        .arg(editor)
        .arg(file.path())
        .status();
    match status {
        Err(err) => return failed(editor, err.to_string()),
        Ok(status) => match status.code() {
            Some(0) => {}
            Some(127) => return failed(editor, "comando não encontrado".into()),
            Some(126) => return failed(editor, "não dá para executá-lo".into()),
            Some(code) => return failed(editor, format!("ele saiu com o código {code}")),
            None => return failed(editor, "ele foi interrompido".into()),
        },
    }
    // Read by path: an editor may replace the file (write to a new one and rename it).
    let bytes = match std::fs::read(file.path()) {
        Ok(bytes) => bytes,
        Err(err) => return failed(editor, err.to_string()),
    };
    if bytes.is_empty() {
        return EditorOutcome::Empty;
    }
    match String::from_utf8(bytes) {
        Ok(text) => EditorOutcome::Text(text.trim_end_matches('\n').to_string()),
        Err(_) => failed(editor, "o texto não é UTF-8".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn the_editor_is_editor_or_nano() {
        assert_eq!(editor_of(None), "nano");
        assert_eq!(editor_of(Some(OsStr::new(""))), "nano");
        assert_eq!(editor_of(Some(OsStr::new("vim"))), "vim");
        // Never split: "code -w" is one program, as in the script.
        assert_eq!(editor_of(Some(OsStr::new("code -w"))), "code -w");
    }

    /// A fake editor: a script that runs `body` with the file in $1.
    fn fake(dir: &std::path::Path, name: &str, body: &str) -> OsString {
        let path = dir.join(name);
        fs::write(&path, format!("#!/usr/bin/env bash\n{body}\n")).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
        path.into_os_string()
    }

    #[test]
    fn the_text_comes_back_without_the_final_line_breaks() {
        let tmp = tempfile::tempdir().unwrap();
        let editor = fake(
            tmp.path(),
            "ed",
            "printf '  linha 1\\nlinha 2  \\n\\n\\n' > \"$1\"",
        );
        assert_eq!(
            run_editor(&editor, "Descrição", ""),
            EditorOutcome::Text("  linha 1\nlinha 2  ".into())
        );
    }

    #[test]
    fn an_empty_file_is_empty_and_the_current_text_is_offered() {
        let tmp = tempfile::tempdir().unwrap();
        let editor = fake(tmp.path(), "ed", ": > \"$1\"");
        assert_eq!(run_editor(&editor, "D", "antes"), EditorOutcome::Empty);
        // The current text is in the file the editor opens.
        let seen = tmp.path().join("seen");
        let editor = fake(
            tmp.path(),
            "ed2",
            &format!("cp \"$1\" '{}'; echo depois > \"$1\"", seen.display()),
        );
        assert_eq!(
            run_editor(&editor, "D", "texto atual"),
            EditorOutcome::Text("depois".into())
        );
        assert_eq!(fs::read_to_string(&seen).unwrap(), "texto atual");
    }

    #[test]
    fn the_temporary_file_is_removed() {
        let tmp = tempfile::tempdir().unwrap();
        let path_log = tmp.path().join("path");
        let editor = fake(
            tmp.path(),
            "ed",
            &format!("echo \"$1\" > '{}'; echo x > \"$1\"", path_log.display()),
        );
        run_editor(&editor, "D", "");
        let used = fs::read_to_string(&path_log).unwrap();
        assert!(!std::path::Path::new(used.trim()).exists(), "{used}");
    }

    #[test]
    fn an_editor_that_does_not_open_fails_with_why() {
        let tmp = tempfile::tempdir().unwrap();
        let missing = tmp.path().join("nao-existe").into_os_string();
        assert_eq!(
            run_editor(&missing, "D", ""),
            EditorOutcome::Failed {
                editor: missing.to_string_lossy().into_owned(),
                error: "comando não encontrado".into()
            }
        );
        // "code -w" is looked up as one program, and is not there.
        assert!(matches!(
            run_editor(OsStr::new("code -w"), "D", ""),
            EditorOutcome::Failed { error, .. } if error == "comando não encontrado"
        ));
        let exits = fake(tmp.path(), "ed", "exit 3");
        assert!(matches!(
            run_editor(&exits, "D", ""),
            EditorOutcome::Failed { error, .. } if error == "ele saiu com o código 3"
        ));
        let latin1 = fake(tmp.path(), "ed2", "printf '\\xe9\\n' > \"$1\"");
        assert!(matches!(
            run_editor(&latin1, "D", ""),
            EditorOutcome::Failed { error, .. } if error == "o texto não é UTF-8"
        ));
    }
}
