//! The modules of the nick. Each module defines its own hierarchy when it is designed; in
//! this slice there is one, auto_scrum, which shows its projects.
//! To add a module: a variant in `ModuleKind` (and in `ALL`), a migration that inserts it in
//! the `modules` table, and a file in src/modules/. A plain enum with `match`, and not a
//! trait object: with a single module, a trait would be an abstraction with no real reuse.

pub mod auto_scrum;

/// A module of the nick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModuleKind {
    AutoScrum,
}

impl ModuleKind {
    /// Every module, in the order of the Modules screen.
    pub const ALL: &'static [ModuleKind] = &[ModuleKind::AutoScrum];

    /// The stable key of the module, the one stored in `modules.key`.
    pub fn key(self) -> &'static str {
        match self {
            ModuleKind::AutoScrum => auto_scrum::KEY,
        }
    }

    /// The name shown on the screen.
    pub fn name(self) -> &'static str {
        match self {
            ModuleKind::AutoScrum => auto_scrum::NAME,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    /// Every module of the enum exists in the database after the migrations, with the same
    /// name, and the database has no module the enum doesn't know.
    #[test]
    fn the_enum_and_the_modules_table_agree() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let mut stmt = db
            .connection()
            .prepare("SELECT key, name FROM modules ORDER BY id")
            .unwrap();
        let rows: Vec<(String, String)> = stmt
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let expected: Vec<(String, String)> = ModuleKind::ALL
            .iter()
            .map(|m| (m.key().to_string(), m.name().to_string()))
            .collect();
        assert_eq!(rows, expected);
    }
}
