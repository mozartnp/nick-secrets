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

/// The directory of the prompt logs of the project `name` (its `projects.name`) with the id
/// `id`, in the state directory (NS-52): `<state>/logs/<name>-<id>`. Readable, and two projects
/// with the same folder name do not share it. The name is one path component: `/` and the
/// control characters (`\0` among them) become `_`, and an empty one, `.` or `..` (the root is
/// registered as `/`) becomes `projeto`.
pub fn prompt_logs_dir(state_dir: &Path, name: &str, id: i64) -> PathBuf {
    let clean: String = match name {
        "" | "." | ".." | "/" => "projeto".to_string(),
        _ => name
            .chars()
            .map(|c| if c == '/' || c.is_control() { '_' } else { c })
            .collect(),
    };
    state_dir.join("logs").join(format!("{clean}-{id}"))
}

/// Removes `.` and `..` from an absolute path without touching the disk (the program it names
/// may be missing, and the message then shows this path). Used for the root of the files of the
/// nick (resources.rs, NS-52) and the paths under it: bin/nick-init (init.rs) and bin/nick-agent
/// (launch.rs).
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

/// `1 <one>` or `<n> <many>` (review of the NS-40, P3.2: `1 ticket`, `0 tickets`), the one plural
/// of the counts of the screen and of the messages (moved from ui.rs in the review of the NS-54,
/// P2). Here, beside `printable`: the text shared by the modules of the screen.
pub fn count_of<N>(n: N, one: &str, many: &str) -> String
where
    N: PartialEq + From<u8> + std::fmt::Display,
{
    if n == N::from(1) {
        format!("1 {one}")
    } else {
        format!("{n} {many}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn count_of_says_one_or_many() {
        assert_eq!(count_of(1usize, "ticket", "tickets"), "1 ticket");
        assert_eq!(count_of(0usize, "ticket", "tickets"), "0 tickets");
        assert_eq!(
            count_of(3i64, "agente ativo", "agentes ativos"),
            "3 agentes ativos"
        );
        assert_eq!(count_of(1i64, "oculto", "ocultos"), "1 oculto");
    }

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
    fn the_prompt_logs_of_a_project_are_its_name_and_id_in_the_state_dir() {
        let state = Path::new("/x/state/nick");
        assert_eq!(
            prompt_logs_dir(state, "meu app", 3),
            PathBuf::from("/x/state/nick/logs/meu app-3")
        );
        assert_eq!(
            prompt_logs_dir(state, "meu-app", 12),
            PathBuf::from("/x/state/nick/logs/meu-app-12")
        );
    }

    #[test]
    fn the_name_of_the_prompt_logs_is_sanitized() {
        let state = Path::new("/s");
        let name = |name: &str, id: i64| {
            prompt_logs_dir(state, name, id)
                .strip_prefix("/s/logs")
                .unwrap()
                .to_string_lossy()
                .into_owned()
        };
        // projects::register stores "/" as the name of the root.
        assert_eq!(name("/", 1), "projeto-1");
        assert_eq!(name("", 2), "projeto-2");
        assert_eq!(name(".", 3), "projeto-3");
        assert_eq!(name("..", 4), "projeto-4");
        assert_eq!(name("a/b", 5), "a_b-5");
        assert_eq!(name("a\0b\x1b[2J\nc", 6), "a_b_[2J_c-6");
        assert_eq!(name("ação", 7), "ação-7");
        // Never leaves logs/: one component, never `..`.
        for raw in ["/", "..", "../..", "a/../../b", "\0"] {
            let dir = prompt_logs_dir(state, raw, 9);
            assert_eq!(dir.parent(), Some(Path::new("/s/logs")), "{raw:?}");
        }
    }

    #[test]
    fn control_characters_become_question_marks() {
        assert_eq!(printable("a\x1b[2Jb\nc\td\x7fé"), "a?[2Jb?c?d?é");
        assert_eq!(printable("/tmp/projeto"), "/tmp/projeto");
    }
}
