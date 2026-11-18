//! The database of the nick (SQLite, in the state directory; see paths.rs): opening it,
//! applying the pending migrations and the queries. The schema version is
//! `PRAGMA user_version`, and each pending migration runs in a transaction together with the
//! new version, so a failing one leaves nothing behind. A database newer than this nick is
//! refused without being written.

pub mod migrations;

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode, params};

/// How long a statement waits for another nick that is writing to the same database before
/// giving up with "database is locked" (the only transient error of SQLite).
const BUSY_TIMEOUT_MS: u32 = 5000;

/// Why the database could not be opened. The messages are what the operator reads on the
/// stderr before the TUI opens.
#[derive(Debug)]
pub enum OpenError {
    /// The file could not be opened or read (e.g. it is a directory, or not a database).
    Open { path: PathBuf, message: String },
    /// The database was migrated by a newer nick.
    TooNew {
        path: PathBuf,
        found: u32,
        known: u32,
    },
    /// A pending migration failed (and was rolled back).
    Migrate {
        path: PathBuf,
        version: u32,
        message: String,
    },
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::Open { path, message } => write!(
                f,
                "Erro: não consegui abrir o banco do nick {}: {message}.",
                path.display()
            ),
            OpenError::TooNew { path, found, known } => write!(
                f,
                "Erro: o banco {} está na versão {found}, mais nova que a deste nick ({known}). \
                 Atualize o nick.",
                path.display()
            ),
            OpenError::Migrate {
                path,
                version,
                message,
            } => write!(
                f,
                "Erro: não consegui atualizar o banco do nick {} para a versão {version}: \
                 {message}.",
                path.display()
            ),
        }
    }
}

/// A registered project, as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub id: i64,
    pub name: String,
    pub path: PathBuf,
    pub created_at: String,
}

/// True when the error is the UNIQUE constraint (a project already registered in the module).
pub fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _)
            if e.code == ErrorCode::ConstraintViolation
                && e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
    )
}

fn project_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        path: PathBuf::from(row.get::<_, String>(2)?),
        created_at: row.get(3)?,
    })
}

/// An open, migrated database.
pub struct Db {
    conn: Connection,
}

impl Db {
    /// Opens (creating it if needed) and migrates the database at `path`.
    pub fn open(path: &Path) -> Result<Db, OpenError> {
        Db::open_with(path, migrations::MIGRATIONS)
    }

    /// Like `open`, with the list of migrations received by parameter (the tests inject a
    /// failing one).
    pub fn open_with(path: &Path, migrations: &[&str]) -> Result<Db, OpenError> {
        let open_error = |err: rusqlite::Error| OpenError::Open {
            path: path.to_path_buf(),
            message: err.to_string(),
        };
        let mut conn = Connection::open(path).map_err(open_error)?;
        conn.busy_timeout(std::time::Duration::from_millis(BUSY_TIMEOUT_MS.into()))
            .map_err(open_error)?;
        conn.pragma_update(None, "foreign_keys", true)
            .map_err(open_error)?;
        let current: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(open_error)?;
        let known = u32::try_from(migrations.len()).expect("fewer than 2^32 migrations");
        if current > known {
            return Err(OpenError::TooNew {
                path: path.to_path_buf(),
                found: current,
                known,
            });
        }
        for (version, sql) in (current + 1..).zip(&migrations[current as usize..]) {
            let migrate_error = |err: rusqlite::Error| OpenError::Migrate {
                path: path.to_path_buf(),
                version,
                message: err.to_string(),
            };
            // The migration and its version in one transaction: all or nothing.
            let tx = conn.transaction().map_err(migrate_error)?;
            tx.execute_batch(sql).map_err(migrate_error)?;
            tx.pragma_update(None, "user_version", version)
                .map_err(migrate_error)?;
            tx.commit().map_err(migrate_error)?;
        }
        Ok(Db { conn })
    }

    /// The connection, for the tests.
    pub fn connection(&self) -> &Connection {
        &self.conn
    }

    /// The projects of the module `module_key`, by name (case insensitive), then by path.
    pub fn list_projects(&self, module_key: &str) -> rusqlite::Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT p.id, p.name, p.path, p.created_at FROM projects p \
             JOIN modules m ON m.id = p.module_id WHERE m.key = ?1 \
             ORDER BY p.name COLLATE NOCASE, p.path",
        )?;
        stmt.query_map(params![module_key], project_from_row)?
            .collect()
    }

    /// True when the module already has a project at `path`.
    pub fn project_exists(&self, module_key: &str, path: &str) -> rusqlite::Result<bool> {
        self.conn.query_row(
            "SELECT EXISTS (SELECT 1 FROM projects p JOIN modules m ON m.id = p.module_id \
             WHERE m.key = ?1 AND p.path = ?2)",
            params![module_key, path],
            |row| row.get(0),
        )
    }

    /// Stores a project of the module and returns it. A duplicate fails with the UNIQUE
    /// constraint (see `is_unique_violation`).
    pub fn insert_project(
        &self,
        module_key: &str,
        name: &str,
        path: &str,
    ) -> rusqlite::Result<Project> {
        self.conn.query_row(
            "INSERT INTO projects (module_id, name, path, created_at) \
             SELECT id, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%SZ', 'now') FROM modules \
             WHERE key = ?1 \
             RETURNING id, name, path, created_at",
            params![module_key, name, path],
            project_from_row,
        )
    }

    /// Deletes the project row. Nothing else: the files of the project are never touched.
    pub fn delete_project(&self, id: i64) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::migrations::{MIGRATION_1, MIGRATIONS};
    use super::*;

    fn user_version(conn: &Connection) -> u32 {
        conn.query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap()
    }

    fn tables(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' \
                 AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn modules(conn: &Connection) -> Vec<(String, String)> {
        let mut stmt = conn
            .prepare("SELECT key, name FROM modules ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn a_new_database_gets_version_1_with_auto_scrum_and_no_project() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let db = Db::open(&path).unwrap();
        assert!(path.is_file());
        assert_eq!(user_version(db.connection()), 1);
        assert_eq!(
            modules(db.connection()),
            [("auto_scrum".to_string(), "auto_scrum".to_string())]
        );
        assert_eq!(db.list_projects("auto_scrum").unwrap(), []);
    }

    #[test]
    fn the_schema_has_only_modules_and_projects() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        assert_eq!(tables(db.connection()), ["modules", "projects"]);
    }

    #[test]
    fn opening_twice_duplicates_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        Db::open(&path)
            .unwrap()
            .insert_project("auto_scrum", "p", "/p")
            .unwrap();
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(db.connection()), 1);
        assert_eq!(modules(db.connection()).len(), 1);
        assert_eq!(db.list_projects("auto_scrum").unwrap().len(), 1);
    }

    #[test]
    fn foreign_keys_are_on() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let on: i64 = db
            .connection()
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap();
        assert_eq!(on, 1);
        let orphan = db.connection().execute(
            "INSERT INTO projects (module_id, name, path, created_at) VALUES (99, 'x', '/x', 'now')",
            [],
        );
        assert!(orphan.is_err(), "a project of a missing module is refused");
    }

    #[test]
    fn the_busy_timeout_is_set() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let ms: i64 = db
            .connection()
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(ms, 5000);
    }

    #[test]
    fn a_newer_database_is_refused_without_being_written() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TABLE future (x); PRAGMA user_version = 7;")
                .unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let err = Db::open(&path).err().expect("a newer database is refused");
        assert_eq!(
            err.to_string(),
            format!(
                "Erro: o banco {} está na versão 7, mais nova que a deste nick (1). \
                 Atualize o nick.",
                path.display()
            )
        );
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_eq!(std::fs::metadata(&path).unwrap().modified().unwrap(), mtime);
        let names: Vec<_> = std::fs::read_dir(tmp.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["nick.db"], "no journal nor wal left");
    }

    #[test]
    fn a_failing_migration_leaves_nothing_applied() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        // The 2nd migration fails in its 2nd statement: its 1st statement must be undone.
        let broken = [
            MIGRATION_1,
            "CREATE TABLE half (x); INSERT INTO nope VALUES (1);",
        ];
        let err = Db::open_with(&path, &broken).err().expect("it fails");
        assert!(
            err.to_string().starts_with(&format!(
                "Erro: não consegui atualizar o banco do nick {} para a versão 2: ",
                path.display()
            )),
            "{err}"
        );
        let conn = Connection::open(&path).unwrap();
        assert_eq!(user_version(&conn), 1, "migration 1 stays, 2 does not");
        assert_eq!(tables(&conn), ["modules", "projects"]);
    }

    #[test]
    fn a_failing_first_migration_leaves_an_empty_database() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let broken = ["CREATE TABLE half (x); INSERT INTO nope VALUES (1);"];
        assert!(Db::open_with(&path, &broken).is_err());
        let conn = Connection::open(&path).unwrap();
        assert_eq!(user_version(&conn), 0);
        assert_eq!(tables(&conn), Vec::<String>::new());
    }

    #[test]
    fn a_directory_in_place_of_the_file_is_an_open_error() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        std::fs::create_dir(&path).unwrap();
        let err = Db::open(&path).err().expect("it fails");
        assert!(matches!(err, OpenError::Open { .. }), "{err:?}");
        assert!(
            err.to_string().starts_with(&format!(
                "Erro: não consegui abrir o banco do nick {}: ",
                path.display()
            )),
            "{err}"
        );
    }

    /// A published migration is never edited (ABOUT.md): a mistake is fixed by a new one.
    /// This pins the text of migration 1; if this test fails, revert the edit and add a
    /// migration 2 instead.
    #[test]
    fn migration_1_is_pinned() {
        assert_eq!(
            MIGRATIONS.len(),
            1,
            "a new migration also pins its own text here"
        );
        assert_eq!(
            MIGRATION_1,
            "\
CREATE TABLE modules (
    id   INTEGER PRIMARY KEY,
    key  TEXT NOT NULL UNIQUE,      -- stable identifier in the code: 'auto_scrum'
    name TEXT NOT NULL              -- what the screen shows: 'auto_scrum'
);
CREATE TABLE projects (
    id         INTEGER PRIMARY KEY,
    module_id  INTEGER NOT NULL REFERENCES modules(id),
    name       TEXT NOT NULL,       -- basename of the path, at registration
    path       TEXT NOT NULL,       -- absolute and canonical (plan D5)
    created_at TEXT NOT NULL,       -- RFC 3339, in UTC
    UNIQUE (module_id, path)
);
INSERT INTO modules (key, name) VALUES ('auto_scrum', 'auto_scrum');
"
        );
        assert_eq!(MIGRATIONS[0], MIGRATION_1);
    }

    #[test]
    fn projects_are_listed_by_name_ignoring_case() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        db.insert_project("auto_scrum", "beta", "/b").unwrap();
        db.insert_project("auto_scrum", "Alfa", "/a").unwrap();
        db.insert_project("auto_scrum", "gama", "/g").unwrap();
        let names: Vec<_> = db
            .list_projects("auto_scrum")
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, ["Alfa", "beta", "gama"]);
    }

    #[test]
    fn insert_stores_an_rfc3339_utc_date_and_refuses_a_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let project = db.insert_project("auto_scrum", "p", "/x/p").unwrap();
        assert_eq!(project.name, "p");
        assert_eq!(project.path, PathBuf::from("/x/p"));
        let date = project.created_at.as_bytes();
        assert_eq!(date.len(), 20, "{}", project.created_at);
        assert_eq!(
            (date[4], date[7], date[10], date[19]),
            (b'-', b'-', b'T', b'Z')
        );
        assert!(db.project_exists("auto_scrum", "/x/p").unwrap());
        assert!(!db.project_exists("auto_scrum", "/x/q").unwrap());
        let dup = db.insert_project("auto_scrum", "p", "/x/p").unwrap_err();
        assert!(is_unique_violation(&dup), "{dup:?}");
        assert_eq!(db.list_projects("auto_scrum").unwrap(), [project]);
    }

    #[test]
    fn delete_removes_only_that_row() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        db.delete_project(a.id).unwrap();
        assert_eq!(db.list_projects("auto_scrum").unwrap(), [b]);
    }
}
