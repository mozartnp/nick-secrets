//! Gives the binary the commit it was built from (NS-52), as NICK_GIT_COMMIT: the short hash of
//! HEAD, or `desconhecido` without git, without a repository or without a commit (a tarball of
//! the AUR, a copy with an empty `git init`). It never fails the build. It runs again when the
//! commit changes: HEAD, the ref HEAD points at and the packed-refs, found by git itself so a
//! worktree works too. No "dirty" mark: it would rebuild at every change of the index.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The stdout of `git <args>` at `dir`, trimmed, when git ran and exited with 0.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (!text.is_empty()).then_some(text)
}

/// The paths whose change means another commit: HEAD of the worktree, the ref it points at (or,
/// while that ref does not exist yet, its directory) and the packed-refs of the repository.
fn watched(dir: &Path) -> Vec<PathBuf> {
    let absolute = |path: String| dir.join(path);
    let Some(git_dir) = git(dir, &["rev-parse", "--git-dir"]).map(absolute) else {
        return Vec::new();
    };
    let common = git(dir, &["rev-parse", "--git-common-dir"])
        .map(absolute)
        .unwrap_or_else(|| git_dir.clone());
    let mut paths = vec![git_dir.join("HEAD"), common.join("packed-refs")];
    if let Some(reference) = git(dir, &["symbolic-ref", "-q", "HEAD"]) {
        let file = common.join(reference);
        match file.parent() {
            Some(parent) if !file.exists() => paths.push(parent.to_path_buf()),
            _ => paths.push(file),
        }
    }
    paths.into_iter().filter(|path| path.exists()).collect()
}

fn main() {
    let dir = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap_or_default());
    let commit = git(&dir, &["rev-parse", "--short", "HEAD"])
        .filter(|hash| hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .unwrap_or_else(|| "desconhecido".to_string());
    println!("cargo:rustc-env=NICK_GIT_COMMIT={commit}");
    println!("cargo:rerun-if-changed=build.rs");
    for path in watched(&dir) {
        println!("cargo:rerun-if-changed={}", path.display());
    }
}
