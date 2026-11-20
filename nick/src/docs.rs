//! The docs dir of a ticket in a project, `<project>/.nick/docs/<ID>/` (NS-36), with no screen:
//! its path, whether its artifacts (`plan.md`, `review.md`) exist right now, read from the disk
//! at each call (nothing goes to the database, and their content is never read), and the rename
//! of the dir when the ID of the ticket is edited (`tickets::edit_id`). The rename is the only
//! write of the nick inside a project (a structural test checks it). It uses
//! `renameat2(RENAME_NOREPLACE)`: `rename(2)` silently replaces an empty directory at the
//! destination, and the edition must never overwrite nor merge folders. The errors are the
//! messages for the status bar, in PT-BR.

use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use nix::errno::Errno;
use nix::fcntl::{AT_FDCWD, RenameFlags, renameat2};

/// The dir of the nick in a project (bin/nick-init creates it).
pub const NICK_DIR_NAME: &str = ".nick";
/// The dir of the docs of the tickets, inside `NICK_DIR_NAME`.
pub const DOCS_DIR_NAME: &str = "docs";
/// The plan of a ticket (the Tech Leader writes it; Dev and Review read it).
pub const PLAN_FILE_NAME: &str = "plan.md";
/// The review of a ticket (the Review writes it).
pub const REVIEW_FILE_NAME: &str = "review.md";

/// `<project>/.nick/docs/<key>`.
pub fn docs_dir(project: &Path, key: &str) -> PathBuf {
    project.join(NICK_DIR_NAME).join(DOCS_DIR_NAME).join(key)
}

/// `.nick/docs/<key>/`, relative to the project: how the messages name the dir.
pub fn dir_text(key: &str) -> String {
    format!("{NICK_DIR_NAME}/{DOCS_DIR_NAME}/{key}/")
}

/// `.nick/docs/<key>/plan.md`, relative to the project.
pub fn plan_path(key: &str) -> String {
    format!("{}{PLAN_FILE_NAME}", dir_text(key))
}

/// `.nick/docs/<key>/review.md`, relative to the project.
pub fn review_path(key: &str) -> String {
    format!("{}{REVIEW_FILE_NAME}", dir_text(key))
}

/// Which artifacts of a ticket exist: only a regular file counts (a folder named `plan.md`
/// does not).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Artifacts {
    pub plan: bool,
    pub review: bool,
}

/// The artifacts of the ticket `key`, read from the disk now (one `stat` each; no cache, no
/// content read, nothing written).
pub fn artifacts(project: &Path, key: &str) -> Artifacts {
    let dir = docs_dir(project, key);
    Artifacts {
        plan: dir.join(PLAN_FILE_NAME).is_file(),
        review: dir.join(REVIEW_FILE_NAME).is_file(),
    }
}

/// Whether `.nick/docs/<key>` exists now, of any kind (what `rename` would move).
pub fn has_dir(project: &Path, key: &str) -> bool {
    std::fs::symlink_metadata(docs_dir(project, key)).is_ok()
}

/// What `rename` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Renamed {
    /// `.nick/docs/<old>` became `.nick/docs/<new>`.
    Moved,
    /// There was no `.nick/docs/<old>` (nor, maybe, `.nick/` or `.nick/docs/`): nothing done,
    /// nothing created.
    NoFolder,
}

/// The refusal of a destination that already exists (folder, even empty, file or link).
fn exists_message(new: &str) -> String {
    format!(
        "Já existe a pasta {} no projeto. Nada foi mudado.",
        dir_text(new)
    )
}

/// Renames `.nick/docs/<old>` to `.nick/docs/<new>` in `project`, never replacing anything:
/// without the old folder nothing is done (`NoFolder`); a destination that exists, of any
/// kind, is refused; the rename itself is `renameat2(RENAME_NOREPLACE)`, so a destination
/// that appears after the check is refused too (EEXIST). The error is the message of the
/// refusal; nothing changed.
pub fn rename(project: &Path, old: &str, new: &str) -> Result<Renamed, String> {
    rename_with(project, old, new, &mut || {})
}

/// `rename` with `between` run after the checks and before the rename (the tests make a
/// destination appear there, the window only `RENAME_NOREPLACE` covers).
fn rename_with(
    project: &Path,
    old: &str,
    new: &str,
    between: &mut dyn FnMut(),
) -> Result<Renamed, String> {
    let from = docs_dir(project, old);
    let to = docs_dir(project, new);
    let failed = |err: &dyn std::fmt::Display| {
        format!(
            "Não consegui renomear {} para {}: {err}. Nada foi mudado.",
            dir_text(old),
            dir_text(new)
        )
    };
    match std::fs::symlink_metadata(&from) {
        Ok(_) => {}
        Err(err) if matches!(err.kind(), ErrorKind::NotFound | ErrorKind::NotADirectory) => {
            return Ok(Renamed::NoFolder);
        }
        Err(err) => return Err(failed(&err)),
    }
    match std::fs::symlink_metadata(&to) {
        Ok(_) => return Err(exists_message(new)),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(failed(&err)),
    }
    between();
    match renameat2(
        AT_FDCWD,
        &from,
        AT_FDCWD,
        &to,
        RenameFlags::RENAME_NOREPLACE,
    ) {
        Ok(()) => Ok(Renamed::Moved),
        Err(Errno::EEXIST) => Err(exists_message(new)),
        Err(err) => Err(failed(&err.desc())),
    }
}

/// The inverse of a `rename` that moved the folder: `.nick/docs/<new>` back to
/// `.nick/docs/<old>`, with `RENAME_NOREPLACE` too (a folder that took the old name meanwhile
/// is never replaced). The error is the one of the system.
pub fn undo_rename(project: &Path, old: &str, new: &str) -> Result<(), String> {
    renameat2(
        AT_FDCWD,
        &docs_dir(project, new),
        AT_FDCWD,
        &docs_dir(project, old),
        RenameFlags::RENAME_NOREPLACE,
    )
    .map_err(|err| err.desc().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::projects::snapshot::{Entry, snapshot};
    use std::collections::BTreeMap;
    use std::fs;
    use std::os::unix::fs::symlink;
    use std::path::{Path, PathBuf};

    fn project() -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap().join("p");
        fs::create_dir_all(root.join(".nick")).unwrap();
        fs::write(root.join(".nick/nick.conf"), "STACK_DESCRIPTION=\"\"\n").unwrap();
        fs::write(root.join("README.md"), "# p\n").unwrap();
        (tmp, root)
    }

    /// `.nick/docs/<key>/` with a plan, a review and a nested file.
    fn with_docs(root: &Path, key: &str) {
        let dir = root.join(".nick/docs").join(key);
        fs::create_dir_all(dir.join("prints")).unwrap();
        fs::write(dir.join("plan.md"), "# plano\n").unwrap();
        fs::write(dir.join("review.md"), "# review\n").unwrap();
        fs::write(dir.join("prints/tela.txt"), "tela\n").unwrap();
    }

    /// `snapshot` with the entries under `.nick/docs/<old>` moved to `.nick/docs/<new>`.
    fn moved(
        mut before: BTreeMap<PathBuf, Entry>,
        old: &str,
        new: &str,
    ) -> BTreeMap<PathBuf, Entry> {
        let from = Path::new(".nick/docs").join(old);
        let to = Path::new(".nick/docs").join(new);
        let keys: Vec<PathBuf> = before
            .keys()
            .filter(|k| k.starts_with(&from))
            .cloned()
            .collect();
        for key in keys {
            let entry = before.remove(&key).unwrap();
            before.insert(to.join(key.strip_prefix(&from).unwrap()), entry);
        }
        before
    }

    /// `snapshot` without the modification time of `.nick/docs`, the parent of the renamed dir
    /// (renaming an entry changes its parent's mtime; nothing else).
    fn but_the_docs_mtime(mut snap: BTreeMap<PathBuf, Entry>) -> BTreeMap<PathBuf, Entry> {
        if let Some(entry) = snap.get_mut(Path::new(".nick/docs")) {
            entry.modified = std::time::UNIX_EPOCH;
        }
        snap
    }

    #[test]
    fn the_paths_of_the_docs_of_a_ticket() {
        assert_eq!(
            docs_dir(Path::new("/w/p"), "NS-1"),
            PathBuf::from("/w/p/.nick/docs/NS-1")
        );
        assert_eq!(plan_path("NS-1"), ".nick/docs/NS-1/plan.md");
        assert_eq!(review_path("NS-1"), ".nick/docs/NS-1/review.md");
        assert_eq!(dir_text("NS-1"), ".nick/docs/NS-1/");
        assert_eq!((NICK_DIR_NAME, DOCS_DIR_NAME), (".nick", "docs"));
    }

    #[test]
    fn artifacts_are_read_from_the_disk_at_each_call_without_writing() {
        let (_tmp, root) = project();
        let none = Artifacts {
            plan: false,
            review: false,
        };
        assert_eq!(artifacts(&root, "NS-1"), none, "no .nick/docs at all");
        let dir = root.join(".nick/docs/NS-1");
        fs::create_dir_all(&dir).unwrap();
        assert_eq!(artifacts(&root, "NS-1"), none);
        fs::write(dir.join("plan.md"), "x").unwrap();
        assert_eq!(
            artifacts(&root, "NS-1"),
            Artifacts {
                plan: true,
                review: false
            }
        );
        fs::write(dir.join("review.md"), "x").unwrap();
        assert_eq!(
            artifacts(&root, "NS-1"),
            Artifacts {
                plan: true,
                review: true
            }
        );
        fs::remove_file(dir.join("plan.md")).unwrap();
        assert_eq!(
            artifacts(&root, "NS-1"),
            Artifacts {
                plan: false,
                review: true
            }
        );
        // A folder named plan.md is not a plan.
        fs::create_dir(dir.join("plan.md")).unwrap();
        assert!(!artifacts(&root, "NS-1").plan);
        let before = snapshot(&root);
        artifacts(&root, "NS-1");
        artifacts(&root, "NS-2");
        assert_eq!(snapshot(&root), before, "reading writes nothing");
    }

    #[test]
    fn without_the_folder_rename_does_nothing_and_creates_nothing() {
        let (_tmp, root) = project();
        let before = snapshot(&root);
        assert_eq!(rename(&root, "abcd1234", "NS-9"), Ok(Renamed::NoFolder));
        assert_eq!(snapshot(&root), before);
        assert!(!root.join(".nick/docs").exists(), "not even .nick/docs");
        fs::create_dir_all(root.join(".nick/docs/NS-1")).unwrap();
        let before = snapshot(&root);
        assert_eq!(rename(&root, "abcd1234", "NS-9"), Ok(Renamed::NoFolder));
        assert_eq!(snapshot(&root), before);
        // Without .nick/ at all (it became a file, or never existed): no folder either.
        fs::remove_dir_all(root.join(".nick")).unwrap();
        fs::write(root.join(".nick"), "").unwrap();
        assert_eq!(rename(&root, "abcd1234", "NS-9"), Ok(Renamed::NoFolder));
    }

    #[test]
    fn with_the_folder_rename_moves_it_whole() {
        let (_tmp, root) = project();
        with_docs(&root, "abcd1234");
        with_docs(&root, "NS-1");
        let before = snapshot(&root);
        assert_eq!(rename(&root, "abcd1234", "NS-9"), Ok(Renamed::Moved));
        assert!(!root.join(".nick/docs/abcd1234").exists());
        assert_eq!(
            but_the_docs_mtime(snapshot(&root)),
            but_the_docs_mtime(moved(before, "abcd1234", "NS-9")),
            "only the name changed"
        );
    }

    /// The test that catches a `std::fs::rename`: rename(2) replaces an empty directory.
    #[test]
    fn an_empty_folder_at_the_destination_is_refused_and_both_stay() {
        let (_tmp, root) = project();
        with_docs(&root, "abcd1234");
        fs::create_dir_all(root.join(".nick/docs/NS-9")).unwrap();
        let before = snapshot(&root);
        assert_eq!(
            rename(&root, "abcd1234", "NS-9"),
            Err("Já existe a pasta .nick/docs/NS-9/ no projeto. Nada foi mudado.".into())
        );
        assert_eq!(snapshot(&root), before);
    }

    /// The guarantee of `RENAME_NOREPLACE`: an empty folder that appears at the destination
    /// after the check (another nick, the operator) is not silently replaced, as `rename(2)`
    /// would do. This is the test that catches a `std::fs::rename` in place of `renameat2`.
    #[test]
    fn an_empty_folder_that_appears_after_the_check_is_refused_too() {
        let (_tmp, root) = project();
        with_docs(&root, "abcd1234");
        let target = root.join(".nick/docs/NS-9");
        let mut appear = || fs::create_dir(&target).unwrap();
        assert_eq!(
            rename_with(&root, "abcd1234", "NS-9", &mut appear),
            Err("Já existe a pasta .nick/docs/NS-9/ no projeto. Nada foi mudado.".into())
        );
        assert!(
            root.join(".nick/docs/abcd1234/plan.md").is_file(),
            "the old one stays"
        );
        assert_eq!(
            fs::read_dir(&target).unwrap().count(),
            0,
            "the new one stays empty"
        );
    }

    #[test]
    fn a_file_or_a_link_at_the_destination_is_refused_too() {
        for kind in ["file", "link", "dangling"] {
            let (_tmp, root) = project();
            with_docs(&root, "abcd1234");
            let target = root.join(".nick/docs/NS-9");
            match kind {
                "file" => fs::write(&target, "x").unwrap(),
                "link" => symlink(root.join("README.md"), &target).unwrap(),
                _ => symlink(root.join("nada"), &target).unwrap(),
            }
            let before = snapshot(&root);
            assert_eq!(
                rename(&root, "abcd1234", "NS-9"),
                Err("Já existe a pasta .nick/docs/NS-9/ no projeto. Nada foi mudado.".into()),
                "{kind}"
            );
            assert_eq!(snapshot(&root), before, "{kind}");
        }
    }

    #[test]
    fn undo_rename_moves_it_back_without_replacing_anything() {
        let (_tmp, root) = project();
        with_docs(&root, "abcd1234");
        let before = snapshot(&root);
        assert_eq!(rename(&root, "abcd1234", "NS-9"), Ok(Renamed::Moved));
        assert_eq!(undo_rename(&root, "abcd1234", "NS-9"), Ok(()));
        assert_eq!(
            but_the_docs_mtime(snapshot(&root)),
            but_the_docs_mtime(before)
        );
        // An empty folder took the old name meanwhile: undo refuses, both stay.
        assert_eq!(rename(&root, "abcd1234", "NS-9"), Ok(Renamed::Moved));
        fs::create_dir(root.join(".nick/docs/abcd1234")).unwrap();
        let before = snapshot(&root);
        assert!(undo_rename(&root, "abcd1234", "NS-9").is_err());
        assert_eq!(snapshot(&root), before);
    }
}
