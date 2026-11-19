//! The rules of the projects of a module, with no screen: turning what the operator typed into
//! a canonical path and checking it (plan D5), registering, removing (only the database row:
//! the files of the project are never touched) and the availability, computed from the disk
//! every time and never stored.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

pub use crate::db::Project;

// Also used by the unit tests of tickets.rs (one inclusion only: clippy's duplicate_mod).
#[cfg(test)]
#[path = "../tests/common/snapshot.rs"]
pub(crate) mod snapshot;
use crate::db::{Db, is_unique_violation};
use crate::modules::ModuleKind;

/// The config of a project, relative to its directory: what bin/nick-init creates.
pub const CONFIG_FILE: &str = ".nick/nick.conf";

/// The config file of the project at `dir`.
pub fn config_file(dir: &Path) -> PathBuf {
    dir.join(CONFIG_FILE)
}

/// Whether a registered project can be used right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// The path no longer exists, or is not a directory.
    DirMissing,
    /// The directory exists, without `.nick/nick.conf`.
    ConfigMissing,
}

/// The availability of the project at `path`, from the disk. Never fails: whatever the system
/// says, the project is only marked.
pub fn availability(path: &Path) -> Availability {
    if !path.is_dir() {
        Availability::DirMissing
    } else if !config_file(path).is_file() {
        Availability::ConfigMissing
    } else {
        Availability::Available
    }
}

/// The mark of an unavailable project, shown on its row and in the refusal to open its
/// tickets; None when it is available.
pub fn unavailable_text(availability: Availability) -> Option<&'static str> {
    match availability {
        Availability::Available => None,
        Availability::DirMissing => Some("[indisponível: pasta não encontrada]"),
        Availability::ConfigMissing => Some("[indisponível: sem .nick/nick.conf]"),
    }
}

/// What a valid path still needs before it is registered.
#[derive(Debug, PartialEq, Eq)]
pub enum Check {
    /// Everything is fine: register the canonical path.
    Ready(PathBuf),
    /// The directory has no `.nick/nick.conf`: offer to create it (plan D6), store nothing.
    NeedsConfig(PathBuf),
}

/// The message of an unexpected database error, for the status bar.
pub fn db_error_message(err: &rusqlite::Error) -> String {
    format!("Erro no banco do nick: {err}.")
}

/// Steps 1 to 6 of plan D5: trims `input`, expands `~` with `home`, resolves a relative path
/// against `cwd` (the cwd of the nick process, the only place where it matters), canonicalizes
/// it and checks that it is a directory, that it can be stored as text and that the module
/// doesn't have it yet. Then step 7: whether it has the config. The error is the message for
/// the status bar. Nothing is written.
pub fn check(
    db: &Db,
    module: ModuleKind,
    input: &str,
    home: Option<&Path>,
    cwd: Option<&Path>,
) -> Result<Check, String> {
    let typed = input.trim();
    if typed.is_empty() {
        return Err("Informe o caminho da pasta do projeto.".into());
    }
    let path = absolute(typed, home, cwd)?;
    let canonical = std::fs::canonicalize(&path).map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            format!("A pasta {typed} não existe.")
        } else {
            format!("Não consegui ler a pasta {typed}: {err}.")
        }
    })?;
    if !canonical.is_dir() {
        return Err(format!("{typed} não é uma pasta."));
    }
    let text = path_text(&canonical)?;
    match db.project_exists(module.key(), text) {
        Ok(true) => return Err(duplicate_message(&canonical)),
        Ok(false) => {}
        Err(err) => return Err(db_error_message(&err)),
    }
    if config_file(&canonical).is_file() {
        Ok(Check::Ready(canonical))
    } else {
        Ok(Check::NeedsConfig(canonical))
    }
}

/// Steps 2 and 3 of plan D5: `~` alone or `~/...` expands with `home` (`~user` is not
/// supported: it is a plain relative name), and a relative path is joined to `cwd`.
fn absolute(typed: &str, home: Option<&Path>, cwd: Option<&Path>) -> Result<PathBuf, String> {
    let path = match (typed, home) {
        ("~", Some(home)) => home.to_path_buf(),
        (_, Some(home)) if typed.starts_with("~/") => home.join(&typed[2..]),
        _ => PathBuf::from(typed),
    };
    if path.is_absolute() {
        return Ok(path);
    }
    cwd.map(|cwd| cwd.join(&path)).ok_or_else(|| {
        format!(
            "Não consegui descobrir a pasta atual para resolver {typed}. Use um caminho absoluto."
        )
    })
}

/// The path as text, the way it is stored (a path that is not UTF-8 is refused: rare, and it
/// keeps the schema simple).
fn path_text(path: &Path) -> Result<&str, String> {
    path.to_str().ok_or_else(|| {
        "Erro: o caminho tem caracteres que o nick não consegue guardar.".to_string()
    })
}

fn duplicate_message(path: &Path) -> String {
    format!("Esse projeto já está cadastrado: {}.", path.display())
}

/// Step 8 of plan D5: stores the project (name = last component of the canonical path) and
/// returns it. The error is the message for the status bar (a duplicate, caught by the UNIQUE
/// constraint too, has the same message as in `check`).
pub fn register(db: &Db, module: ModuleKind, canonical: &Path) -> Result<Project, String> {
    let text = path_text(canonical)?;
    let name = canonical
        .file_name()
        .map_or(text, |name| name.to_str().unwrap_or(text));
    db.insert_project(module.key(), name, text).map_err(|err| {
        if is_unique_violation(&err) {
            duplicate_message(canonical)
        } else {
            db_error_message(&err)
        }
    })
}

/// The status message after a registration.
pub fn registered_message(project: &Project) -> String {
    format!(
        "Projeto cadastrado: {} ({}).",
        project.name,
        project.path.display()
    )
}

/// Removes the project from the nick: only its database row. Returns the status message.
pub fn remove(db: &Db, project: &Project) -> Result<String, String> {
    db.delete_project(project.id)
        .map_err(|err| db_error_message(&err))?;
    Ok(format!(
        "Projeto removido do nick: {}. Nenhum arquivo foi apagado.",
        project.name
    ))
}

#[cfg(test)]
mod tests {
    use super::snapshot::snapshot;
    use super::*;
    use std::ffi::OsStr;
    use std::fs;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;

    const MODULE: ModuleKind = ModuleKind::AutoScrum;

    struct Fixture {
        _tmp: tempfile::TempDir,
        /// Canonical root of the temporary tree.
        root: PathBuf,
        db: Db,
    }

    impl Fixture {
        fn new() -> Fixture {
            let tmp = tempfile::tempdir().unwrap();
            let root = fs::canonicalize(tmp.path()).unwrap();
            let db = Db::open(&root.join("nick.db")).unwrap();
            Fixture {
                _tmp: tmp,
                root,
                db,
            }
        }

        /// Creates the directory `rel` (with the config when `config`) and returns its path.
        fn project(&self, rel: &str, config: bool) -> PathBuf {
            let dir = self.root.join(rel);
            fs::create_dir_all(&dir).unwrap();
            if config {
                fs::create_dir_all(dir.join(".nick")).unwrap();
                fs::write(config_file(&dir), "STACK_DESCRIPTION=\"\"\n").unwrap();
            }
            dir
        }

        fn check(&self, input: &str) -> Result<Check, String> {
            check(
                &self.db,
                MODULE,
                input,
                Some(&self.root.join("home")),
                Some(&self.root.join("cwd")),
            )
        }

        fn paths(&self) -> Vec<PathBuf> {
            self.db
                .list_projects(MODULE.key())
                .unwrap()
                .into_iter()
                .map(|p| p.path)
                .collect()
        }
    }

    #[test]
    fn an_empty_input_asks_for_the_path() {
        let f = Fixture::new();
        assert_eq!(
            f.check("   "),
            Err("Informe o caminho da pasta do projeto.".into())
        );
    }

    #[test]
    fn tilde_alone_and_tilde_slash_expand_with_home() {
        let f = Fixture::new();
        let home = f.project("home", true);
        let x = f.project("home/x", true);
        assert_eq!(f.check("~"), Ok(Check::Ready(home)));
        assert_eq!(f.check("  ~/x  "), Ok(Check::Ready(x)));
    }

    #[test]
    fn tilde_user_is_a_plain_relative_name() {
        let f = Fixture::new();
        let other = f.project("cwd/~other", true);
        assert_eq!(f.check("~other"), Ok(Check::Ready(other)));
    }

    #[test]
    fn a_relative_path_resolves_against_the_cwd_and_dotdot_is_removed() {
        let f = Fixture::new();
        f.project("cwd", false);
        let p = f.project("p", true);
        let q = f.project("cwd/q", true);
        assert_eq!(f.check("q"), Ok(Check::Ready(q.clone())));
        assert_eq!(f.check("../p"), Ok(Check::Ready(p)));
        assert_eq!(f.check("./q/../q/."), Ok(Check::Ready(q)));
    }

    #[test]
    fn a_relative_path_without_a_cwd_is_refused() {
        let f = Fixture::new();
        assert_eq!(
            check(&f.db, MODULE, "q", None, None),
            Err(
                "Não consegui descobrir a pasta atual para resolver q. Use um caminho absoluto."
                    .into()
            )
        );
    }

    #[test]
    fn a_symlink_registers_its_target_and_counts_as_the_same_directory() {
        let f = Fixture::new();
        let real = f.project("real", true);
        symlink(&real, f.root.join("link")).unwrap();
        let link = f.root.join("link");
        let Ok(Check::Ready(canonical)) = f.check(link.to_str().unwrap()) else {
            panic!("the link is valid");
        };
        assert_eq!(canonical, real);
        register(&f.db, MODULE, &canonical).unwrap();
        assert_eq!(
            f.check(real.to_str().unwrap()),
            Err(format!(
                "Esse projeto já está cadastrado: {}.",
                real.display()
            ))
        );
        assert_eq!(
            f.check(link.to_str().unwrap()),
            Err(format!(
                "Esse projeto já está cadastrado: {}.",
                real.display()
            ))
        );
        assert_eq!(f.paths(), [real]);
    }

    #[test]
    fn a_missing_directory_is_refused_with_the_typed_path() {
        let f = Fixture::new();
        assert_eq!(f.check("~/nada"), Err("A pasta ~/nada não existe.".into()));
    }

    #[test]
    fn an_unreadable_parent_is_reported_with_the_system_error() {
        let f = Fixture::new();
        let file = f.root.join("file");
        fs::write(&file, "").unwrap();
        let typed = format!("{}/sub", file.display());
        let err = f.check(&typed).unwrap_err();
        assert!(
            err.starts_with(&format!("Não consegui ler a pasta {typed}: ")),
            "{err}"
        );
        assert!(err.ends_with('.'), "{err}");
    }

    #[test]
    fn a_regular_file_is_refused() {
        let f = Fixture::new();
        let file = f.root.join("file.txt");
        fs::write(&file, "").unwrap();
        let typed = file.to_str().unwrap();
        assert_eq!(f.check(typed), Err(format!("{typed} não é uma pasta.")));
    }

    #[test]
    fn a_path_that_is_not_utf8_is_refused() {
        let f = Fixture::new();
        let weird = f.root.join(OsStr::from_bytes(b"bad\xffname"));
        fs::create_dir(&weird).unwrap();
        fs::create_dir(weird.join(".nick")).unwrap();
        fs::write(config_file(&weird), "").unwrap();
        symlink(&weird, f.root.join("link")).unwrap();
        let msg = "Erro: o caminho tem caracteres que o nick não consegue guardar.";
        assert_eq!(
            f.check(f.root.join("link").to_str().unwrap()),
            Err(msg.into())
        );
        assert_eq!(register(&f.db, MODULE, &weird), Err(msg.into()));
        assert_eq!(f.paths(), Vec::<PathBuf>::new());
    }

    #[test]
    fn without_the_config_it_needs_config_and_writes_nothing() {
        let f = Fixture::new();
        let dir = f.project("bare", false);
        let before = snapshot(&dir);
        assert_eq!(
            f.check(dir.to_str().unwrap()),
            Ok(Check::NeedsConfig(dir.clone()))
        );
        assert_eq!(snapshot(&dir), before);
        assert_eq!(f.paths(), Vec::<PathBuf>::new());
    }

    #[test]
    fn a_config_that_is_a_directory_does_not_count() {
        let f = Fixture::new();
        let dir = f.project("odd", false);
        fs::create_dir_all(config_file(&dir)).unwrap();
        assert_eq!(f.check(dir.to_str().unwrap()), Ok(Check::NeedsConfig(dir)));
    }

    #[test]
    fn a_valid_directory_is_registered_absolute_with_its_basename() {
        let f = Fixture::new();
        let dir = f.project("cwd/meu projeto", true);
        let Ok(Check::Ready(canonical)) = f.check("meu projeto") else {
            panic!("valid");
        };
        let project = register(&f.db, MODULE, &canonical).unwrap();
        assert_eq!(project.name, "meu projeto");
        assert_eq!(project.path, dir);
        assert!(project.path.is_absolute());
        assert_eq!(
            registered_message(&project),
            format!("Projeto cadastrado: meu projeto ({}).", dir.display())
        );
        assert_eq!(f.paths(), std::slice::from_ref(&dir));
        // Registering it again (the UNIQUE constraint) gives the duplicate message.
        assert_eq!(
            register(&f.db, MODULE, &dir),
            Err(format!(
                "Esse projeto já está cadastrado: {}.",
                dir.display()
            ))
        );
    }

    #[test]
    fn removing_deletes_only_the_row_and_touches_no_file() {
        let f = Fixture::new();
        let dir = f.project("keep", true);
        fs::create_dir_all(dir.join("src/deep")).unwrap();
        fs::write(dir.join("src/deep/main.rs"), "fn main() {}\n").unwrap();
        fs::write(dir.join("README.md"), "# keep\n").unwrap();
        symlink("README.md", dir.join("link")).unwrap();
        let other = f.project("other", true);
        let project = register(&f.db, MODULE, &dir).unwrap();
        register(&f.db, MODULE, &other).unwrap();
        let before = snapshot(&dir);
        assert_eq!(before.len(), 7, "{before:?}");

        assert_eq!(
            remove(&f.db, &project),
            Ok("Projeto removido do nick: keep. Nenhum arquivo foi apagado.".into())
        );
        assert_eq!(snapshot(&dir), before);
        assert_eq!(f.paths(), [other]);
    }

    #[test]
    fn availability_follows_the_disk_and_never_panics() {
        let f = Fixture::new();
        let ok = f.project("ok", true);
        assert_eq!(availability(&ok), Availability::Available);

        let gone = f.project("gone", true);
        fs::remove_dir_all(&gone).unwrap();
        assert_eq!(availability(&gone), Availability::DirMissing);

        let file = f.project("file", true);
        fs::remove_dir_all(&file).unwrap();
        fs::write(&file, "").unwrap();
        assert_eq!(availability(&file), Availability::DirMissing);

        let noconf = f.project("noconf", true);
        fs::remove_file(config_file(&noconf)).unwrap();
        assert_eq!(availability(&noconf), Availability::ConfigMissing);

        assert_eq!(
            availability(Path::new("/proc/self/fd/0/../x")),
            Availability::DirMissing
        );
    }
}
