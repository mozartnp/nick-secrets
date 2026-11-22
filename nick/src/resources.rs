//! Where the nick finds its files (NS-52): bin/nick-init, bin/nick-agent and, through them, lib/
//! and auto_scrum/. Decided by where the binary itself is (`std::env::current_exe`, read by
//! main.rs and passed here), never by a variable:
//! - installed (`make install`): `<dir of the binary>/../share/nick/.nick-install` exists, and
//!   the root is that `share/nick/`. Never falls back to the checkout: the binary and a checkout
//!   may be of different versions, and the Rust of one with the bash of the other would break
//!   the protocol of `--check`/`--render` without a word. A missing file gives its own message,
//!   with the path in the prefix;
//! - checkout (`cargo run`, `cargo test`, the old `cargo install --path nick`): the root is the
//!   checkout the binary was built from (`CARGO_MANIFEST_DIR/..`), normalized.

use std::path::{Path, PathBuf};

use crate::paths::normalize;

/// The mark (and manifest) `make install` writes in `$PREFIX/share/nick/`.
pub const INSTALL_MARKER: &str = ".nick-install";

/// The root of the checkout this binary was built from, before normalization.
const CHECKOUT_ROOT: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/..");

/// Where the files of the nick are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resources {
    /// `$PREFIX/share/nick` of an installation.
    Installed(PathBuf),
    /// The checkout the binary was built from.
    Checkout(PathBuf),
}

/// The files for the binary at `exe` (`None`: `current_exe` failed, the checkout).
pub fn locate(exe: Option<&Path>) -> Resources {
    let share = exe
        .and_then(Path::parent)
        .map(|bin| normalize(&bin.join("../share/nick")));
    match share {
        Some(share) if share.join(INSTALL_MARKER).exists() => Resources::Installed(share),
        _ => Resources::checkout(),
    }
}

impl Resources {
    /// The checkout this binary was built from, normalized (without `..`).
    pub fn checkout() -> Resources {
        Resources::Checkout(normalize(Path::new(CHECKOUT_ROOT)))
    }

    /// The root the files hang from (bin/, lib/, auto_scrum/).
    pub fn root(&self) -> &Path {
        match self {
            Resources::Installed(root) | Resources::Checkout(root) => root,
        }
    }

    /// bin/nick-init under the root (init.rs).
    pub fn init(&self) -> PathBuf {
        crate::init::init_path(None, self.root())
    }

    /// bin/nick-agent under the root (launch.rs).
    pub fn launcher(&self) -> PathBuf {
        crate::launch::launcher_path(self.root())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Component;

    fn repository() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .to_path_buf()
    }

    fn no_dot_dot(path: &Path) -> bool {
        !path.components().any(|c| c == Component::ParentDir)
    }

    /// A prefix with `bin/nick` and, when `marker`, `share/nick/.nick-install`.
    fn prefix(marker: bool) -> (tempfile::TempDir, PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("bin/nick"), "").unwrap();
        fs::create_dir_all(root.join("share/nick")).unwrap();
        if marker {
            fs::write(root.join("share/nick").join(INSTALL_MARKER), "# nick\n").unwrap();
        }
        (tmp, root)
    }

    #[test]
    fn a_binary_with_the_marker_beside_it_is_installed() {
        let (_tmp, root) = prefix(true);
        let found = locate(Some(&root.join("bin/nick")));
        assert_eq!(found, Resources::Installed(root.join("share/nick")));
        assert!(no_dot_dot(found.root()), "{found:?}");
        assert_eq!(found.launcher(), root.join("share/nick/bin/nick-agent"));
        assert_eq!(found.init(), root.join("share/nick/bin/nick-init"));
    }

    #[test]
    fn without_the_marker_it_is_the_checkout() {
        let (_tmp, root) = prefix(false);
        let found = locate(Some(&root.join("bin/nick")));
        assert_eq!(found, Resources::Checkout(repository()));
        assert_eq!(found, Resources::checkout());
        assert!(no_dot_dot(found.root()), "{found:?}");
        assert_eq!(found.launcher(), repository().join("bin/nick-agent"));
        assert_eq!(found.init(), repository().join("bin/nick-init"));
    }

    #[test]
    fn without_the_path_of_the_binary_it_is_the_checkout() {
        assert_eq!(locate(None), Resources::Checkout(repository()));
    }

    /// The binary of `cargo test` has no `share/nick/.nick-install` beside its target: the tests
    /// run on the checkout.
    #[test]
    fn the_binary_of_the_tests_is_on_the_checkout() {
        let exe = std::env::current_exe().unwrap();
        assert_eq!(locate(Some(&exe)), Resources::checkout());
    }

    /// No fallback: with the marker, a missing bin/nick-agent in the prefix stays in the prefix
    /// (the message of the missing file shows that path), even though the checkout has one.
    #[test]
    fn installed_without_its_files_never_falls_back_to_the_checkout() {
        let (_tmp, root) = prefix(true);
        let found = locate(Some(&root.join("bin/nick")));
        assert_eq!(found, Resources::Installed(root.join("share/nick")));
        assert!(!found.launcher().exists());
        assert!(found.launcher().starts_with(&root));
        assert!(repository().join("bin/nick-agent").is_file());
    }

    #[test]
    fn a_binary_reached_through_dot_dot_gives_a_normalized_root() {
        let (_tmp, root) = prefix(true);
        let exe = root.join("bin/../bin/./nick");
        let found = locate(Some(&exe));
        assert_eq!(found, Resources::Installed(root.join("share/nick")));
        assert!(no_dot_dot(&found.launcher()));
    }
}
