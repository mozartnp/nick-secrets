//! The programs the nick needs from the PATH (NS-52): claude (the agents), envsubst (the
//! prompts, by bash) and ai-jail with bwrap (the jail). Checked once, when the nick opens, with
//! the PATH received by parameter (only main.rs reads the environment). A missing one never
//! stops the nick: the status bar says what is missing and what stops working. Without claude no
//! agent opens (app.rs refuses before the launcher). Without the jail the nick does not turn it
//! off: a project with `JAIL_ENABLED="true"` keeps the error of `jail_check`.

use std::ffi::OsStr;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

/// Whether `name` is a regular executable file in some directory of `path` (empty entries are
/// skipped; no PATH finds nothing).
pub fn find_in_path(name: &str, path: Option<&OsStr>) -> bool {
    let Some(path) = path else {
        return false;
    };
    std::env::split_paths(path)
        .filter(|dir| !dir.as_os_str().is_empty())
        .any(|dir| is_executable(&dir.join(name)))
}

/// A regular file (a link is followed) with some execute bit.
fn is_executable(file: &Path) -> bool {
    std::fs::metadata(file)
        .is_ok_and(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
}

/// Which of the programs the nick needs are in the PATH.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Deps {
    pub claude: bool,
    pub envsubst: bool,
    pub ai_jail: bool,
    pub bwrap: bool,
}

impl Default for Deps {
    /// Everything present (the App of the tests).
    fn default() -> Deps {
        Deps {
            claude: true,
            envsubst: true,
            ai_jail: true,
            bwrap: true,
        }
    }
}

impl Deps {
    /// Looks each program up in `path`.
    pub fn check(path: Option<&OsStr>) -> Deps {
        Deps {
            claude: find_in_path("claude", path),
            envsubst: find_in_path("envsubst", path),
            ai_jail: find_in_path("ai-jail", path),
            bwrap: find_in_path("bwrap", path),
        }
    }

    /// The warning of the status bar when something is missing (None when nothing is).
    pub fn warning(&self) -> Option<String> {
        let missing: Vec<&str> = [
            (self.claude, "claude"),
            (self.envsubst, "envsubst"),
            (self.ai_jail, "ai-jail"),
            (self.bwrap, "bwrap"),
        ]
        .into_iter()
        .filter(|(found, _)| !found)
        .map(|(_, name)| name)
        .collect();
        if missing.is_empty() {
            return None;
        }
        let mut text = format!("Faltam no PATH: {}.", missing.join(", "));
        if !self.claude {
            text.push_str(" Sem o claude, o nick não abre agentes.");
        }
        if !self.envsubst {
            text.push_str(" Sem o envsubst (pacote gettext), o prompt não é montado.");
        }
        if !self.ai_jail || !self.bwrap {
            text.push_str(" Sem a jail, só abrem agentes os projetos com JAIL_ENABLED=\"false\".");
        }
        text.push_str(" Veja o INSTALL.md.");
        Some(text)
    }
}

/// Why an agent does not open without claude.
pub const NO_CLAUDE_MESSAGE: &str = "Não dá para abrir agentes: o claude não foi encontrado no \
     PATH. Instale o Claude Code e abra o nick de novo.";

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn exec(dir: &Path, name: &str, mode: u32) -> PathBuf {
        let file = dir.join(name);
        fs::write(&file, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(mode)).unwrap();
        file
    }

    #[test]
    fn an_executable_in_a_dir_of_the_path_is_found() {
        let tmp = tempfile::tempdir().unwrap();
        let (a, b) = (tmp.path().join("a"), tmp.path().join("b"));
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        exec(&b, "claude", 0o755);
        let path = format!("{}:{}", a.display(), b.display());
        assert!(find_in_path("claude", Some(OsStr::new(&path))));
        assert!(!find_in_path("bwrap", Some(OsStr::new(&path))));
        // Only the execute bit of the owner is enough.
        exec(&a, "envsubst", 0o700);
        assert!(find_in_path("envsubst", Some(OsStr::new(&path))));
    }

    #[test]
    fn a_file_without_the_execute_bit_or_a_dir_does_not_count() {
        let tmp = tempfile::tempdir().unwrap();
        exec(tmp.path(), "claude", 0o644);
        fs::create_dir_all(tmp.path().join("bwrap")).unwrap();
        let path = tmp.path().display().to_string();
        assert!(!find_in_path("claude", Some(OsStr::new(&path))));
        assert!(!find_in_path("bwrap", Some(OsStr::new(&path))));
    }

    #[test]
    fn a_link_to_an_executable_counts() {
        let tmp = tempfile::tempdir().unwrap();
        let real = exec(tmp.path(), "real", 0o755);
        let bin = tmp.path().join("bin");
        fs::create_dir_all(&bin).unwrap();
        std::os::unix::fs::symlink(&real, bin.join("ai-jail")).unwrap();
        let path = bin.display().to_string();
        assert!(find_in_path("ai-jail", Some(OsStr::new(&path))));
    }

    #[test]
    fn no_path_an_empty_one_or_empty_entries_find_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        // An empty entry would be the cwd: a claude there must not count.
        exec(tmp.path(), "claude", 0o755);
        let cwd = std::env::current_dir().unwrap();
        assert!(
            !cwd.join("claude").exists(),
            "no claude in the cwd of the tests"
        );
        assert!(!find_in_path("claude", None));
        assert!(!find_in_path("claude", Some(OsStr::new(""))));
        assert!(!find_in_path("claude", Some(OsStr::new("::"))));
        let path = format!(":{}:", tmp.path().display());
        assert!(find_in_path("claude", Some(OsStr::new(&path))));
    }

    #[test]
    fn check_looks_every_program_up() {
        let tmp = tempfile::tempdir().unwrap();
        exec(tmp.path(), "claude", 0o755);
        exec(tmp.path(), "bwrap", 0o755);
        let path = tmp.path().display().to_string();
        assert_eq!(
            Deps::check(Some(OsStr::new(&path))),
            Deps {
                claude: true,
                envsubst: false,
                ai_jail: false,
                bwrap: true,
            }
        );
    }

    fn deps(claude: bool, envsubst: bool, ai_jail: bool, bwrap: bool) -> Deps {
        Deps {
            claude,
            envsubst,
            ai_jail,
            bwrap,
        }
    }

    #[test]
    fn nothing_missing_means_no_warning() {
        assert_eq!(Deps::default().warning(), None);
        assert_eq!(deps(true, true, true, true).warning(), None);
    }

    #[test]
    fn the_warning_lists_what_is_missing_and_what_stops_working() {
        assert_eq!(
            deps(false, true, true, true).warning().as_deref(),
            Some(
                "Faltam no PATH: claude. Sem o claude, o nick não abre agentes. \
                 Veja o INSTALL.md."
            )
        );
        assert_eq!(
            deps(true, false, true, true).warning().as_deref(),
            Some(
                "Faltam no PATH: envsubst. Sem o envsubst (pacote gettext), o prompt não é \
                 montado. Veja o INSTALL.md."
            )
        );
        assert_eq!(
            deps(true, true, false, true).warning().as_deref(),
            Some(
                "Faltam no PATH: ai-jail. Sem a jail, só abrem agentes os projetos com \
                 JAIL_ENABLED=\"false\". Veja o INSTALL.md."
            )
        );
        assert_eq!(
            deps(true, true, false, false).warning().as_deref(),
            Some(
                "Faltam no PATH: ai-jail, bwrap. Sem a jail, só abrem agentes os projetos com \
                 JAIL_ENABLED=\"false\". Veja o INSTALL.md."
            )
        );
        assert_eq!(
            deps(false, false, false, false).warning().as_deref(),
            Some(
                "Faltam no PATH: claude, envsubst, ai-jail, bwrap. Sem o claude, o nick não \
                 abre agentes. Sem o envsubst (pacote gettext), o prompt não é montado. Sem a \
                 jail, só abrem agentes os projetos com JAIL_ENABLED=\"false\". Veja o \
                 INSTALL.md."
            )
        );
    }

    #[test]
    fn the_refusal_without_claude_is_literal() {
        assert_eq!(
            NO_CLAUDE_MESSAGE,
            "Não dá para abrir agentes: o claude não foi encontrado no PATH. Instale o Claude \
             Code e abra o nick de novo."
        );
    }
}
