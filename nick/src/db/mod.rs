//! The database of the nick (SQLite, in the state directory; see paths.rs): opening it,
//! applying the pending migrations and the queries. The schema version is
//! `PRAGMA user_version`, and each pending migration runs in a transaction together with the
//! new version, so a failing one leaves nothing behind. A database newer than this nick is
//! refused without being written.

pub mod migrations;

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode, OptionalExtension, params};

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

/// A ticket of a project, as stored (migration 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticket {
    pub id: i64,
    pub project_id: i64,
    /// The Jira ID, or the provisional short UUID.
    pub key: String,
    pub is_provisional: bool,
    pub title: String,
    pub created_at: String,
    /// When it was hidden; None while visible.
    pub hidden_at: Option<String>,
}

/// The columns of a ticket, in the order of `ticket_from_row`.
const TICKET_COLUMNS: &str = "id, project_id, key, is_provisional, title, created_at, hidden_at";

fn ticket_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Ticket> {
    Ok(Ticket {
        id: row.get(0)?,
        project_id: row.get(1)?,
        key: row.get(2)?,
        is_provisional: row.get(3)?,
        title: row.get(4)?,
        created_at: row.get(5)?,
        hidden_at: row.get(6)?,
    })
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

    /// Deletes the project row (its tickets go with it: ON DELETE CASCADE). Nothing else: the
    /// files of the project are never touched.
    pub fn delete_project(&self, id: i64) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", params![id])?;
        Ok(())
    }

    /// The tickets of the project, in the order they were created; the hidden ones only when
    /// `include_hidden`.
    pub fn list_tickets(
        &self,
        project_id: i64,
        include_hidden: bool,
    ) -> rusqlite::Result<Vec<Ticket>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {TICKET_COLUMNS} FROM tickets \
             WHERE project_id = ?1 AND (?2 OR hidden_at IS NULL) ORDER BY id"
        ))?;
        stmt.query_map(params![project_id, include_hidden], ticket_from_row)?
            .collect()
    }

    /// Whether the project has a ticket with `key`: None if not, Some(hidden) if it has.
    pub fn find_ticket_key(&self, project_id: i64, key: &str) -> rusqlite::Result<Option<bool>> {
        self.conn
            .query_row(
                "SELECT hidden_at IS NOT NULL FROM tickets WHERE project_id = ?1 AND key = ?2",
                params![project_id, key],
                |row| row.get(0),
            )
            .optional()
    }

    /// Stores a ticket of the project and returns it. A key the project already has fails
    /// with the UNIQUE constraint (see `is_unique_violation`).
    pub fn insert_ticket(
        &self,
        project_id: i64,
        key: &str,
        is_provisional: bool,
        title: &str,
    ) -> rusqlite::Result<Ticket> {
        self.conn.query_row(
            &format!(
                "INSERT INTO tickets (project_id, key, is_provisional, title, created_at) \
                 VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
                 RETURNING {TICKET_COLUMNS}"
            ),
            params![project_id, key, is_provisional, title],
            ticket_from_row,
        )
    }

    /// A provisional short ID: 8 lowercase hex characters from SQLite's randomblob (plan D2;
    /// no crate for it). Unique only by the UNIQUE constraint of the insert.
    pub fn random_short_id(&self) -> rusqlite::Result<String> {
        self.conn
            .query_row("SELECT lower(hex(randomblob(4)))", [], |row| row.get(0))
    }

    /// Changes only the title of the ticket.
    /// `false` when the ticket is no longer in the database (nothing changed).
    pub fn update_ticket_title(&self, id: i64, title: &str) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "UPDATE tickets SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(changed > 0)
    }

    /// Hides the ticket (hidden_at = now) or shows it again (hidden_at = NULL). `false` when
    /// the ticket is no longer in the database (nothing changed).
    pub fn set_ticket_hidden(&self, id: i64, hidden: bool) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "UPDATE tickets SET hidden_at = \
             CASE WHEN ?2 THEN strftime('%Y-%m-%dT%H:%M:%SZ', 'now') END WHERE id = ?1",
            params![id, hidden],
        )?;
        Ok(changed > 0)
    }

    /// How many tickets the project has, the hidden ones included.
    pub fn count_tickets(&self, project_id: i64) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT count(*) FROM tickets WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::migrations::{MIGRATION_1, MIGRATION_2, MIGRATIONS};
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
    fn a_new_database_gets_version_2_with_auto_scrum_and_no_project() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let db = Db::open(&path).unwrap();
        assert!(path.is_file());
        assert_eq!(user_version(db.connection()), 2);
        assert_eq!(
            modules(db.connection()),
            [("auto_scrum".to_string(), "auto_scrum".to_string())]
        );
        assert_eq!(db.list_projects("auto_scrum").unwrap(), []);
    }

    #[test]
    fn the_schema_has_only_modules_projects_and_tickets() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        assert_eq!(tables(db.connection()), ["modules", "projects", "tickets"]);
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
        assert_eq!(user_version(db.connection()), 2);
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
                "Erro: o banco {} está na versão 7, mais nova que a deste nick (2). \
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
        // The 3rd migration fails in its 2nd statement: its 1st statement must be undone.
        let broken = [
            MIGRATION_1,
            MIGRATION_2,
            "CREATE TABLE half (x); INSERT INTO nope VALUES (1);",
        ];
        let err = Db::open_with(&path, &broken).err().expect("it fails");
        assert!(
            err.to_string().starts_with(&format!(
                "Erro: não consegui atualizar o banco do nick {} para a versão 3: ",
                path.display()
            )),
            "{err}"
        );
        let conn = Connection::open(&path).unwrap();
        assert_eq!(
            user_version(&conn),
            2,
            "migrations 1 and 2 stay, 3 does not"
        );
        assert_eq!(tables(&conn), ["modules", "projects", "tickets"]);
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
            2,
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
        assert_eq!(
            MIGRATION_2,
            "\
CREATE TABLE tickets (
    id             INTEGER PRIMARY KEY,
    project_id     INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key            TEXT NOT NULL,     -- the Jira ID, or the provisional short UUID
    is_provisional INTEGER NOT NULL CHECK (is_provisional IN (0, 1)),
    title          TEXT NOT NULL CHECK (trim(title) <> ''),
    created_at     TEXT NOT NULL,     -- RFC 3339, in UTC
    hidden_at      TEXT,              -- RFC 3339, in UTC; NULL while visible
    UNIQUE (project_id, key)
);
"
        );
        assert_eq!(MIGRATIONS[1], MIGRATION_2);
    }

    // ---- migration 2: tickets ----

    /// Inserts a ticket row by hand (the schema tests check the constraints, not the queries).
    fn raw_ticket(
        conn: &Connection,
        project_id: i64,
        key: &str,
        provisional: i64,
        title: &str,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO tickets (project_id, key, is_provisional, title, created_at) \
             VALUES (?1, ?2, ?3, ?4, 'now')",
            params![project_id, key, provisional, title],
        )
    }

    fn ticket_keys(conn: &Connection) -> Vec<(i64, String)> {
        let mut stmt = conn
            .prepare("SELECT project_id, key FROM tickets ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn a_version_1_database_with_projects_migrates_to_2_and_keeps_them() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let (a, b) = {
            let old = Db::open_with(&path, &[MIGRATION_1]).unwrap();
            assert_eq!(user_version(old.connection()), 1);
            assert_eq!(tables(old.connection()), ["modules", "projects"]);
            (
                old.insert_project("auto_scrum", "a", "/a").unwrap(),
                old.insert_project("auto_scrum", "b", "/b").unwrap(),
            )
        };
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(db.connection()), 2);
        assert_eq!(tables(db.connection()), ["modules", "projects", "tickets"]);
        assert_eq!(db.list_projects("auto_scrum").unwrap(), [a.clone(), b]);
        raw_ticket(db.connection(), a.id, "NS-1", 0, "t").unwrap();
    }

    #[test]
    fn a_ticket_of_a_missing_project_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        assert!(raw_ticket(db.connection(), 99, "NS-1", 0, "t").is_err());
        assert_eq!(ticket_keys(db.connection()), []);
    }

    #[test]
    fn the_key_is_unique_per_project_only_and_case_sensitive() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        let conn = db.connection();
        raw_ticket(conn, a.id, "NS-1", 0, "t").unwrap();
        let dup = raw_ticket(conn, a.id, "NS-1", 1, "outro").unwrap_err();
        assert!(is_unique_violation(&dup), "{dup:?}");
        raw_ticket(conn, b.id, "NS-1", 0, "t").unwrap();
        raw_ticket(conn, a.id, "ns-1", 0, "t").unwrap();
        assert_eq!(
            ticket_keys(conn),
            [
                (a.id, "NS-1".to_string()),
                (b.id, "NS-1".to_string()),
                (a.id, "ns-1".to_string())
            ]
        );
    }

    #[test]
    fn the_title_and_is_provisional_are_checked() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let conn = db.connection();
        assert!(raw_ticket(conn, a.id, "K1", 0, "").is_err(), "empty title");
        assert!(
            raw_ticket(conn, a.id, "K2", 0, "   ").is_err(),
            "blank title"
        );
        assert!(
            raw_ticket(conn, a.id, "K3", 2, "t").is_err(),
            "provisional 2"
        );
        assert!(
            raw_ticket(conn, a.id, "K4", -1, "t").is_err(),
            "provisional -1"
        );
        assert_eq!(ticket_keys(conn), []);
        raw_ticket(conn, a.id, "K5", 1, "t").unwrap();
        raw_ticket(conn, a.id, "K6", 0, "t").unwrap();
        assert_eq!(ticket_keys(conn).len(), 2);
    }

    #[test]
    fn deleting_a_project_deletes_its_tickets_and_no_other() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        let conn = db.connection();
        raw_ticket(conn, a.id, "A-1", 0, "t").unwrap();
        raw_ticket(conn, a.id, "A-2", 1, "t").unwrap();
        conn.execute("UPDATE tickets SET hidden_at = 'x' WHERE key = 'A-2'", [])
            .unwrap();
        raw_ticket(conn, b.id, "B-1", 0, "t").unwrap();
        db.delete_project(a.id).unwrap();
        assert_eq!(ticket_keys(conn), [(b.id, "B-1".to_string())]);
    }

    // ---- the queries of the tickets ----

    fn assert_rfc3339_utc(date: &str) {
        let bytes = date.as_bytes();
        assert_eq!(bytes.len(), 20, "{date}");
        assert_eq!(
            (
                bytes[4], bytes[7], bytes[10], bytes[13], bytes[16], bytes[19]
            ),
            (b'-', b'-', b'T', b':', b':', b'Z'),
            "{date}"
        );
    }

    fn keys(tickets: &[Ticket]) -> Vec<&str> {
        tickets.iter().map(|t| t.key.as_str()).collect()
    }

    #[test]
    fn insert_ticket_returns_the_stored_row() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let p = db.insert_project("auto_scrum", "p", "/p").unwrap();
        let t = db.insert_ticket(p.id, "NS-1", false, "Primeiro").unwrap();
        assert_eq!(t.project_id, p.id);
        assert_eq!(t.key, "NS-1");
        assert!(!t.is_provisional);
        assert_eq!(t.title, "Primeiro");
        assert_eq!(t.hidden_at, None);
        assert_rfc3339_utc(&t.created_at);
        let u = db.insert_ticket(p.id, "3f9a0c7e", true, "Segundo").unwrap();
        assert!(u.is_provisional);
        assert_eq!(db.list_tickets(p.id, false).unwrap(), [t, u]);
        let dup = db.insert_ticket(p.id, "NS-1", false, "x").unwrap_err();
        assert!(is_unique_violation(&dup), "{dup:?}");
    }

    #[test]
    fn list_tickets_is_by_creation_of_that_project_and_hides_the_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        db.insert_ticket(a.id, "Z-9", false, "z").unwrap();
        db.insert_ticket(b.id, "B-1", false, "b").unwrap();
        let hidden = db.insert_ticket(a.id, "A-1", false, "a").unwrap();
        db.insert_ticket(a.id, "M-5", false, "m").unwrap();
        db.set_ticket_hidden(hidden.id, true).unwrap();
        assert_eq!(keys(&db.list_tickets(a.id, false).unwrap()), ["Z-9", "M-5"]);
        assert_eq!(
            keys(&db.list_tickets(a.id, true).unwrap()),
            ["Z-9", "A-1", "M-5"]
        );
        assert_eq!(keys(&db.list_tickets(b.id, true).unwrap()), ["B-1"]);
        assert_eq!(db.list_tickets(999, true).unwrap(), []);
    }

    #[test]
    fn find_ticket_key_says_if_it_exists_and_if_it_is_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        db.insert_ticket(a.id, "V-1", false, "v").unwrap();
        let h = db.insert_ticket(a.id, "H-1", false, "h").unwrap();
        db.set_ticket_hidden(h.id, true).unwrap();
        assert_eq!(db.find_ticket_key(a.id, "V-1").unwrap(), Some(false));
        assert_eq!(db.find_ticket_key(a.id, "H-1").unwrap(), Some(true));
        assert_eq!(db.find_ticket_key(a.id, "v-1").unwrap(), None);
        assert_eq!(db.find_ticket_key(b.id, "V-1").unwrap(), None);
    }

    #[test]
    fn update_ticket_title_changes_only_the_title() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let p = db.insert_project("auto_scrum", "p", "/p").unwrap();
        let t = db.insert_ticket(p.id, "NS-1", false, "Velho").unwrap();
        let other = db.insert_ticket(p.id, "NS-2", false, "Outro").unwrap();
        db.update_ticket_title(t.id, "Novo").unwrap();
        let expected = Ticket {
            title: "Novo".into(),
            ..t
        };
        assert_eq!(db.list_tickets(p.id, false).unwrap(), [expected, other]);
    }

    #[test]
    fn set_ticket_hidden_stores_and_clears_the_date() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let p = db.insert_project("auto_scrum", "p", "/p").unwrap();
        let t = db.insert_ticket(p.id, "NS-1", false, "t").unwrap();
        db.set_ticket_hidden(t.id, true).unwrap();
        let hidden = db.list_tickets(p.id, true).unwrap().remove(0);
        assert_rfc3339_utc(hidden.hidden_at.as_deref().expect("hidden_at is set"));
        db.set_ticket_hidden(t.id, false).unwrap();
        assert_eq!(db.list_tickets(p.id, false).unwrap(), [t]);
    }

    #[test]
    fn the_ticket_updates_say_whether_the_row_still_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let p = db.insert_project("auto_scrum", "p", "/p").unwrap();
        let t = db.insert_ticket(p.id, "NS-1", false, "t").unwrap();
        assert!(db.update_ticket_title(t.id, "Novo").unwrap());
        assert!(db.set_ticket_hidden(t.id, true).unwrap());
        assert!(db.set_ticket_hidden(t.id, false).unwrap());
        db.delete_project(p.id).unwrap();
        assert!(!db.update_ticket_title(t.id, "Outro").unwrap());
        assert!(!db.set_ticket_hidden(t.id, true).unwrap());
        assert!(!db.set_ticket_hidden(t.id, false).unwrap());
    }

    #[test]
    fn count_tickets_counts_the_hidden_ones_of_that_project() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        assert_eq!(db.count_tickets(a.id).unwrap(), 0);
        db.insert_ticket(a.id, "A-1", false, "t").unwrap();
        let h = db.insert_ticket(a.id, "A-2", false, "t").unwrap();
        db.set_ticket_hidden(h.id, true).unwrap();
        db.insert_ticket(b.id, "B-1", false, "t").unwrap();
        assert_eq!(db.count_tickets(a.id).unwrap(), 2);
        assert_eq!(db.count_tickets(b.id).unwrap(), 1);
    }

    #[test]
    fn random_short_id_is_8_lowercase_hex_and_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let first = db.random_short_id().unwrap();
        let second = db.random_short_id().unwrap();
        for id in [&first, &second] {
            assert_eq!(id.len(), 8, "{id}");
            assert!(
                id.chars().all(|c| matches!(c, '0'..='9' | 'a'..='f')),
                "{id}"
            );
        }
        assert_ne!(first, second);
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
