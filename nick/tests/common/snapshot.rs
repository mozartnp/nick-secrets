//! A recursive snapshot of a directory, shared by the unit tests (src/projects.rs includes
//! this file with #[path]) and the integration tests (tests/common): the proof that the nick
//! touched no file of a project compares two of them.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// One entry: its type, size, content (files and link targets) and modification time.
#[derive(Debug, PartialEq, Eq)]
pub struct Entry {
    pub kind: &'static str,
    pub size: u64,
    pub content: Vec<u8>,
    pub modified: SystemTime,
}

/// Every entry under `dir` (not `dir` itself), by relative path, without following links.
pub fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Entry> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Entry>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            let meta = fs::symlink_metadata(&path).unwrap();
            let (kind, content) = if meta.is_symlink() {
                let target = fs::read_link(&path).unwrap();
                ("link", target.into_os_string().into_encoded_bytes())
            } else if meta.is_dir() {
                ("dir", Vec::new())
            } else {
                ("file", fs::read(&path).unwrap())
            };
            out.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                Entry {
                    kind,
                    size: meta.len(),
                    content,
                    modified: meta.modified().unwrap(),
                },
            );
            if kind == "dir" {
                walk(root, &path, out);
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(dir, dir, &mut out);
    out
}
