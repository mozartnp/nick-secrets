//! Paths of the nick: the state directory where the database lives (XDG Base Directory, with
//! the environment received by parameter: only main.rs reads it) and the helpers that turn
//! what the operator typed into a path and what the system returned into printable text.

use std::ffi::OsStr;
use std::fmt;
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Component, Path, PathBuf};

/// The file name of the database inside the state directory.
pub const DB_FILE_NAME: &str = "nick.db";

/// Why the state directory could not be resolved.
#[derive(Debug, PartialEq, Eq)]
pub enum StateDirError {
    /// Neither a usable XDG_STATE_HOME nor a HOME.
    NoHome,
}

impl fmt::Display for StateDirError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateDirError::NoHome => write!(
                f,
                "Erro: não sei onde guardar o banco do nick (HOME e XDG_STATE_HOME não definidos)."
            ),
        }
    }
}

/// The state directory of the nick: `$XDG_STATE_HOME/nick`, or `$HOME/.local/state/nick` when
/// XDG_STATE_HOME is unset, empty or relative (the XDG spec says a relative one is invalid
/// and must be ignored). Without a non-empty HOME to fall back to, it is an error.
pub fn state_dir(xdg: Option<&OsStr>, home: Option<&OsStr>) -> Result<PathBuf, StateDirError> {
    if let Some(xdg) = xdg.filter(|xdg| Path::new(xdg).is_absolute()) {
        return Ok(Path::new(xdg).join("nick"));
    }
    match home.filter(|home| !home.is_empty()) {
        Some(home) => Ok(Path::new(home).join(".local/state/nick")),
        None => Err(StateDirError::NoHome),
    }
}

/// Creates the state directory (and its missing parents) with mode 0700: the database may
/// hold paths of client projects. An existing directory is left as it is.
pub fn create_state_dir(dir: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// Removes `.` and `..` from an absolute path without touching the disk (the program it names
/// may be missing, and the message then shows this path). Used for the paths fixed at build
/// time: bin/nick-init (init.rs) and bin/nick-agent (launch.rs).
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

/// Replaces every control character with `?`, so a path typed by the operator or a message
/// from the system can't move the cursor nor change the terminal (the same rule as lib/).
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() { '?' } else { c })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn os(text: &str) -> Option<&OsStr> {
        Some(OsStr::new(text))
    }

    #[test]
    fn an_absolute_xdg_state_home_wins() {
        assert_eq!(
            state_dir(os("/x/state"), os("/home/u")),
            Ok(PathBuf::from("/x/state/nick"))
        );
    }

    #[test]
    fn an_absolute_xdg_state_home_works_without_home() {
        assert_eq!(
            state_dir(os("/x/state"), None),
            Ok(PathBuf::from("/x/state/nick"))
        );
    }

    #[test]
    fn without_xdg_state_home_it_falls_back_to_home() {
        assert_eq!(
            state_dir(None, os("/home/u")),
            Ok(PathBuf::from("/home/u/.local/state/nick"))
        );
    }

    #[test]
    fn an_empty_xdg_state_home_falls_back_to_home() {
        assert_eq!(
            state_dir(os(""), os("/home/u")),
            Ok(PathBuf::from("/home/u/.local/state/nick"))
        );
    }

    #[test]
    fn a_relative_xdg_state_home_is_ignored() {
        assert_eq!(
            state_dir(os("rel/state"), os("/home/u")),
            Ok(PathBuf::from("/home/u/.local/state/nick"))
        );
    }

    #[test]
    fn without_home_nor_xdg_state_home_it_is_an_error() {
        assert_eq!(state_dir(None, None), Err(StateDirError::NoHome));
        assert_eq!(state_dir(os(""), os("")), Err(StateDirError::NoHome));
        assert_eq!(state_dir(os("rel"), None), Err(StateDirError::NoHome));
        assert_eq!(
            StateDirError::NoHome.to_string(),
            "Erro: não sei onde guardar o banco do nick (HOME e XDG_STATE_HOME não definidos)."
        );
    }

    #[test]
    fn the_state_dir_is_created_with_mode_0700() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("a/b/nick");
        create_state_dir(&dir).unwrap();
        assert!(dir.is_dir());
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
        // Again: an existing directory is fine and keeps its mode.
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o750)).unwrap();
        create_state_dir(&dir).unwrap();
        let mode = std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o750);
    }

    #[test]
    fn creating_the_state_dir_over_a_file_fails() {
        let tmp = tempfile::tempdir().unwrap();
        let file = tmp.path().join("nick");
        std::fs::write(&file, "").unwrap();
        assert!(create_state_dir(&file).is_err());
    }

    #[test]
    fn normalize_removes_dot_and_dot_dot_without_touching_the_disk() {
        assert_eq!(
            normalize(Path::new("/a/b/../c/./d/../../bin/x")),
            PathBuf::from("/a/bin/x")
        );
        assert_eq!(
            normalize(Path::new("/nao/existe/../y")),
            PathBuf::from("/nao/y")
        );
    }

    #[test]
    fn control_characters_become_question_marks() {
        assert_eq!(printable("a\x1b[2Jb\nc\td\x7fé"), "a?[2Jb?c?d?é");
        assert_eq!(printable("/tmp/projeto"), "/tmp/projeto");
    }
}
