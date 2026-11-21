//! The database of the nick (SQLite, in the state directory; see paths.rs): opening it,
//! applying the pending migrations and the queries. The schema version is
//! `PRAGMA user_version`, and each pending migration runs in a transaction together with the
//! new version, so a failing one leaves nothing behind. A database newer than this nick is
//! refused without being written.

pub mod migrations;

use std::fmt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, ErrorCode, OptionalExtension, params};

use crate::agents::AgentState;

/// How long a statement waits for another nick that is writing to the same database before
/// giving up with "database is locked" (the only transient error of SQLite).
const BUSY_TIMEOUT_MS: u32 = 5000;

/// The key of the reserved shortcut in `settings` (NS-34); its value is `Shortcut::to_db`.
pub const SETTING_RESERVED_SHORTCUT: &str = "reserved_shortcut";

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
    /// The current stage, the raw key of the column (migration 5; tickets.rs: `TicketStage`).
    pub stage: String,
    /// The epic it is in (migration 7, NS-40); None: right under the project.
    pub epic_id: Option<i64>,
}

/// The columns of a ticket, in the order of `ticket_from_row`.
const TICKET_COLUMNS: &str =
    "id, project_id, key, is_provisional, title, created_at, hidden_at, stage, epic_id";

fn ticket_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Ticket> {
    Ok(Ticket {
        id: row.get(0)?,
        project_id: row.get(1)?,
        key: row.get(2)?,
        is_provisional: row.get(3)?,
        title: row.get(4)?,
        created_at: row.get(5)?,
        hidden_at: row.get(6)?,
        stage: row.get(7)?,
        epic_id: row.get(8)?,
    })
}

/// An epic of a project, as stored (migration 7, NS-40): the key and the title of a ticket, with
/// no stage. Hiding it hides its whole branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Epic {
    pub id: i64,
    pub project_id: i64,
    /// The Jira ID, or the provisional short UUID.
    pub key: String,
    pub is_provisional: bool,
    pub title: String,
    pub created_at: String,
    /// When it was hidden (its whole branch); None while visible.
    pub hidden_at: Option<String>,
}

/// The columns of an epic, in the order of `epic_from_row`.
const EPIC_COLUMNS: &str = "id, project_id, key, is_provisional, title, created_at, hidden_at";

fn epic_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Epic> {
    Ok(Epic {
        id: row.get(0)?,
        project_id: row.get(1)?,
        key: row.get(2)?,
        is_provisional: row.get(3)?,
        title: row.get(4)?,
        created_at: row.get(5)?,
        hidden_at: row.get(6)?,
    })
}

/// Whose key it is in a project (`Db::find_key`): the key is unique across the tickets and the
/// epics (both use `.nick/docs/<key>/`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyKind {
    Ticket,
    Epic,
}

/// What `Db::hide_epic` did: hidden, gone, refused because the branch (the epic or a ticket of
/// it) has this many `active` agents, of any nick, or `Changed`: both tries lost the race with
/// another nick and no active agent is left to count (review of the NS-40, P3.3: never "1 ativo"
/// when there is none).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpicHide {
    Done,
    Gone,
    Active(i64),
    Changed,
}

/// The owner of an agent (migration 7, NS-40): a ticket or an epic, never both, never none (the
/// CHECK of the table). Read from `agents.ticket_id`/`agents.epic_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Owner {
    Ticket(i64),
    Epic(i64),
}

impl Owner {
    /// The values of the columns `(ticket_id, epic_id)`.
    fn columns(self) -> (Option<i64>, Option<i64>) {
        match self {
            Owner::Ticket(id) => (Some(id), None),
            Owner::Epic(id) => (None, Some(id)),
        }
    }
}

/// A change of the stage of a ticket, as stored (migration 5): the raw keys of the stages.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageChangeRow {
    pub from_stage: String,
    pub to_stage: String,
    /// RFC 3339, in UTC.
    pub changed_at: String,
}

/// What `Db::change_ticket_stage` did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageChange {
    /// The stage changed and the history got its row.
    Changed,
    /// The ticket is no longer in the database: nothing was written.
    Gone,
    /// The ticket is no longer at the expected stage (another nick changed it): nothing was
    /// written.
    Moved,
}

/// What `Db::resume_agent`, `archive_agent` and `unarchive_agent` did (NS-39, the mold of
/// `StageChange`): `Done`, or why nothing was written — the agent is gone, or is in the state
/// named (another nick changed it, or it was never in the expected one).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentChange {
    Done,
    Gone,
    Active,
    Stopped,
    Archived,
}

/// An agent of a ticket or of an epic, as stored (migrations 3, 6 and 7). `stage` and `state` are
/// the raw keys of the columns (agents.rs: `Stage`, `AgentState`; the state is only written from
/// the enum).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub id: i64,
    /// Its ticket or its epic (NS-40).
    pub owner: Owner,
    pub stage: String,
    /// The `claude --session-id`.
    pub session_id: String,
    /// The `claude --name`.
    pub name: String,
    pub opened_at: String,
    pub state: String,
    /// The key of its ticket or epic when it was opened (migration 6); None when opened before
    /// the NS-39.
    pub opened_key: Option<String>,
}

/// The columns of an agent, in the order of `agent_from_row`.
const AGENT_COLUMNS: &str =
    "id, ticket_id, epic_id, stage, session_id, name, opened_at, state, opened_key";

fn agent_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Agent> {
    let ticket: Option<i64> = row.get(1)?;
    let epic: Option<i64> = row.get(2)?;
    // The CHECK of the table keeps exactly one; a row without either would be a broken database,
    // reported as an error and not as a panic.
    let owner = match (ticket, epic) {
        (Some(id), None) => Owner::Ticket(id),
        (None, Some(id)) => Owner::Epic(id),
        _ => {
            return Err(rusqlite::Error::InvalidColumnType(
                1,
                "ticket_id/epic_id".into(),
                rusqlite::types::Type::Null,
            ));
        }
    };
    Ok(Agent {
        id: row.get(0)?,
        owner,
        stage: row.get(3)?,
        session_id: row.get(4)?,
        name: row.get(5)?,
        opened_at: row.get(6)?,
        state: row.get(7)?,
        opened_key: row.get(8)?,
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

/// The markers of the `RAISE(ABORT, ...)` of the triggers of the migration 7 (NS-40): a key that
/// is a ticket's, a key that is an epic's (the key is unique across both tables of a project), and
/// an epic of another project for a ticket.
pub const KEY_OF_A_TICKET: &str = "nick: key of a ticket";
pub const KEY_OF_AN_EPIC: &str = "nick: key of an epic";
pub const EPIC_OF_ANOTHER_PROJECT: &str = "nick: epic of another project";

/// True when the error is the abort of a trigger of the nick with `marker` (one of the constants
/// above), the mold of `is_unique_violation`: SQLite reports a `RAISE(ABORT, <message>)` as a
/// `SQLITE_CONSTRAINT_TRIGGER` with the message.
pub fn is_trigger_abort(err: &rusqlite::Error, marker: &str) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, Some(message))
            if e.code == ErrorCode::ConstraintViolation
                && e.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_TRIGGER
                && message == marker
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

    /// Deletes the project row (its epics and its tickets go with it, and their agents: ON DELETE
    /// CASCADE). Nothing else: the files of the project are never touched.
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

    /// The ticket `id`, hidden or not; None when it is no longer in the database.
    pub fn get_ticket(&self, id: i64) -> rusqlite::Result<Option<Ticket>> {
        self.conn
            .query_row(
                &format!("SELECT {TICKET_COLUMNS} FROM tickets WHERE id = ?1"),
                params![id],
                ticket_from_row,
            )
            .optional()
    }

    /// Whether the project has a ticket or an epic with `key` (NS-40: the key is unique across
    /// both): None if not, Some((whose, hidden)) if it has.
    pub fn find_key(
        &self,
        project_id: i64,
        key: &str,
    ) -> rusqlite::Result<Option<(KeyKind, bool)>> {
        self.conn
            .query_row(
                "SELECT 0, hidden_at IS NOT NULL FROM tickets WHERE project_id = ?1 AND key = ?2 \
                 UNION ALL \
                 SELECT 1, hidden_at IS NOT NULL FROM epics WHERE project_id = ?1 AND key = ?2 \
                 LIMIT 1",
                params![project_id, key],
                |row| {
                    let epic: bool = row.get(0)?;
                    let kind = if epic { KeyKind::Epic } else { KeyKind::Ticket };
                    Ok((kind, row.get(1)?))
                },
            )
            .optional()
    }

    /// Stores a ticket of the project, in the epic `epic_id` (NS-40; None: right under the
    /// project), and returns it. A key the project already has fails with the UNIQUE constraint
    /// (see `is_unique_violation`), and the key of an epic, or an epic of another project, with a
    /// trigger (`is_trigger_abort`).
    pub fn insert_ticket(
        &self,
        project_id: i64,
        key: &str,
        is_provisional: bool,
        title: &str,
        epic_id: Option<i64>,
    ) -> rusqlite::Result<Ticket> {
        self.conn.query_row(
            &format!(
                "INSERT INTO tickets (project_id, key, is_provisional, title, created_at, \
                 epic_id) \
                 VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?5) \
                 RETURNING {TICKET_COLUMNS}"
            ),
            params![project_id, key, is_provisional, title, epic_id],
            ticket_from_row,
        )
    }

    /// Moves the ticket into the epic `epic_id`, or out of any epic (None): only the database
    /// changes, no file (NS-40). Only into a visible epic of the project of the ticket (the project
    /// here in the SQL, and by a trigger behind it; visible, so a ticket, maybe with an active
    /// agent, never goes into a branch another nick just hid: review of the NS-40, P3.4). Out of
    /// an epic is always allowed. `false` when the ticket is gone, or the epic is gone, hidden or
    /// of another project (nothing changed). The only writer: `epics::move_ticket`.
    pub fn set_ticket_epic(&self, ticket_id: i64, epic_id: Option<i64>) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "UPDATE tickets SET epic_id = ?2 WHERE id = ?1 AND (?2 IS NULL OR EXISTS \
             (SELECT 1 FROM epics WHERE id = ?2 AND project_id = tickets.project_id \
             AND hidden_at IS NULL))",
            params![ticket_id, epic_id],
        )?;
        Ok(changed > 0)
    }

    /// The epics of the project, in the order they were created; the hidden ones only when
    /// `include_hidden`.
    pub fn list_epics(&self, project_id: i64, include_hidden: bool) -> rusqlite::Result<Vec<Epic>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {EPIC_COLUMNS} FROM epics \
             WHERE project_id = ?1 AND (?2 OR hidden_at IS NULL) ORDER BY id"
        ))?;
        stmt.query_map(params![project_id, include_hidden], epic_from_row)?
            .collect()
    }

    /// The epic `id`, hidden or not; None when it is no longer in the database.
    pub fn get_epic(&self, id: i64) -> rusqlite::Result<Option<Epic>> {
        self.conn
            .query_row(
                &format!("SELECT {EPIC_COLUMNS} FROM epics WHERE id = ?1"),
                params![id],
                epic_from_row,
            )
            .optional()
    }

    /// Stores an epic of the project and returns it. A key the project already has fails with
    /// the UNIQUE constraint (another epic) or with a trigger (a ticket).
    pub fn insert_epic(
        &self,
        project_id: i64,
        key: &str,
        is_provisional: bool,
        title: &str,
    ) -> rusqlite::Result<Epic> {
        self.conn.query_row(
            &format!(
                "INSERT INTO epics (project_id, key, is_provisional, title, created_at) \
                 VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) \
                 RETURNING {EPIC_COLUMNS}"
            ),
            params![project_id, key, is_provisional, title],
            epic_from_row,
        )
    }

    /// Changes only the title of the epic. `false` when it is no longer in the database.
    pub fn update_epic_title(&self, id: i64, title: &str) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "UPDATE epics SET title = ?2 WHERE id = ?1",
            params![id, title],
        )?;
        Ok(changed > 0)
    }

    /// Changes the key of the epic from `old` to `new` (the mold of `update_ticket_key`:
    /// `is_provisional` becomes 0). `false` when the epic is gone or its key is no longer `old`.
    pub fn update_epic_key(&self, id: i64, old: &str, new: &str) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "UPDATE epics SET key = ?3, is_provisional = 0 WHERE id = ?1 AND key = ?2",
            params![id, old, new],
        )?;
        Ok(changed > 0)
    }

    /// How many epics the project has, the hidden ones included.
    pub fn count_epics(&self, project_id: i64) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT count(*) FROM epics WHERE project_id = ?1",
            params![project_id],
            |row| row.get(0),
        )
    }

    /// Hides the epic and so its whole branch (decision 8 of NS-40), leaving the `hidden_at` of
    /// its tickets as they are: one UPDATE conditioned on no `active` agent in the branch (of the
    /// epic or of a ticket of it, of any nick), so nothing starts between counting and hiding.
    /// With 0 rows, the row is read again and the reason counted (the mold of
    /// `change_agent_state`); a branch whose agents ended between the two statements is tried
    /// once more.
    pub fn hide_epic(&self, id: i64) -> rusqlite::Result<EpicHide> {
        for _ in 0..2 {
            let changed = self.conn.execute(
                "UPDATE epics SET hidden_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') \
                 WHERE id = ?1 AND NOT EXISTS (SELECT 1 FROM agents WHERE state = ?2 AND \
                 (epic_id = ?1 OR ticket_id IN (SELECT id FROM tickets WHERE epic_id = ?1)))",
                params![id, AgentState::Active.key()],
            )?;
            if changed > 0 {
                return Ok(EpicHide::Done);
            }
            if self.get_epic(id)?.is_none() {
                return Ok(EpicHide::Gone);
            }
            let active = self.count_active_agents_of_epic_branch(id)?;
            if active > 0 {
                return Ok(EpicHide::Active(active));
            }
        }
        Ok(match self.count_active_agents_of_epic_branch(id)? {
            0 => EpicHide::Changed,
            active => EpicHide::Active(active),
        })
    }

    /// Shows the epic (and its branch) again. `false` when it is no longer in the database.
    pub fn restore_epic(&self, id: i64) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "UPDATE epics SET hidden_at = NULL WHERE id = ?1",
            params![id],
        )?;
        Ok(changed > 0)
    }

    /// How many `active` agents the branch of the epic has: of the epic itself and of its tickets
    /// (of this nick or of another one).
    pub fn count_active_agents_of_epic_branch(&self, epic_id: i64) -> rusqlite::Result<i64> {
        self.conn.query_row(
            "SELECT count(*) FROM agents WHERE state = ?2 AND \
             (epic_id = ?1 OR ticket_id IN (SELECT id FROM tickets WHERE epic_id = ?1))",
            params![epic_id, AgentState::Active.key()],
            |row| row.get(0),
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

    /// A session ID for `claude --session-id`: a lowercase UUID v4 from SQLite's randomblob
    /// (no crate for it, as `random_short_id`), with the version nibble fixed at 4 and the
    /// variant one at 8, 9, a or b. Unique only by the UNIQUE constraint of the insert.
    pub fn random_session_id(&self) -> rusqlite::Result<String> {
        self.conn.query_row(
            "SELECT lower(hex(randomblob(4))) || '-' || lower(hex(randomblob(2))) || '-4' \
             || substr(lower(hex(randomblob(2))), 2) || '-' \
             || substr('89ab', 1 + (random() & 3), 1) \
             || substr(lower(hex(randomblob(2))), 2) || '-' || lower(hex(randomblob(6)))",
            [],
            |row| row.get(0),
        )
    }

    /// Stores an agent of `owner` (a ticket or an epic, NS-40), `active`, opened now with the key
    /// of its owner `opened_key`, and returns it. A session ID already stored fails with the UNIQUE
    /// constraint (see `is_unique_violation`); an epic agent of Dev or Review, with the CHECK.
    pub fn insert_agent(
        &self,
        owner: Owner,
        stage: &str,
        session_id: &str,
        name: &str,
        opened_key: &str,
    ) -> rusqlite::Result<Agent> {
        let (ticket_id, epic_id) = owner.columns();
        self.conn.query_row(
            &format!(
                "INSERT INTO agents \
                 (ticket_id, epic_id, stage, session_id, name, opened_at, state, opened_key) \
                 VALUES (?1, ?7, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?5, ?6) \
                 RETURNING {AGENT_COLUMNS}"
            ),
            params![
                ticket_id,
                stage,
                session_id,
                name,
                AgentState::Active.key(),
                opened_key,
                epic_id
            ],
            agent_from_row,
        )
    }

    /// Marks the agent as `stopped` (only the state changes; the `end_agent` of before the
    /// NS-39). `false` when the agent is no longer in the database.
    pub fn stop_agent(&self, id: i64) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "UPDATE agents SET state = ?2 WHERE id = ?1",
            params![id, AgentState::Stopped.key()],
        )?;
        Ok(changed > 0)
    }

    /// The agent `id`, of any state; None when it is no longer in the database.
    pub fn get_agent(&self, id: i64) -> rusqlite::Result<Option<Agent>> {
        self.conn
            .query_row(
                &format!("SELECT {AGENT_COLUMNS} FROM agents WHERE id = ?1"),
                params![id],
                agent_from_row,
            )
            .optional()
    }

    /// Moves the agent `id` to `to` only when it is in one of `from` (an UPDATE conditioned on
    /// the state, the mold of `change_ticket_stage`); otherwise nothing is written, and the
    /// state read again says why. A state this nick does not know counts as gone: nothing is
    /// ever done on it.
    fn change_agent_state(
        &self,
        id: i64,
        from: &[AgentState],
        to: AgentState,
    ) -> rusqlite::Result<AgentChange> {
        let keys: Vec<&str> = from.iter().map(|state| state.key()).collect();
        let placeholders: Vec<String> = (3..3 + keys.len()).map(|n| format!("?{n}")).collect();
        let to = to.key();
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![&id, &to];
        values.extend(keys.iter().map(|key| key as &dyn rusqlite::ToSql));
        let changed = self.conn.execute(
            &format!(
                "UPDATE agents SET state = ?2 WHERE id = ?1 AND state IN ({})",
                placeholders.join(", ")
            ),
            values.as_slice(),
        )?;
        if changed > 0 {
            return Ok(AgentChange::Done);
        }
        let state: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM agents WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match state.as_deref().and_then(AgentState::from_key) {
            None => AgentChange::Gone,
            Some(AgentState::Active) => AgentChange::Active,
            Some(AgentState::Stopped) => AgentChange::Stopped,
            Some(AgentState::Archived) => AgentChange::Archived,
        })
    }

    /// The agent `id` becomes `active` again (a resume, NS-39): only from `stopped`, or from an
    /// `active` row whose nick died (the caller holds its lock: agents.rs).
    pub fn resume_agent(&self, id: i64) -> rusqlite::Result<AgentChange> {
        self.change_agent_state(
            id,
            &[AgentState::Stopped, AgentState::Active],
            AgentState::Active,
        )
    }

    /// Archives the agent `id`: only a `stopped` one.
    pub fn archive_agent(&self, id: i64) -> rusqlite::Result<AgentChange> {
        self.change_agent_state(id, &[AgentState::Stopped], AgentState::Archived)
    }

    /// Takes the archived agent `id` back to `stopped`.
    pub fn unarchive_agent(&self, id: i64) -> rusqlite::Result<AgentChange> {
        self.change_agent_state(id, &[AgentState::Archived], AgentState::Stopped)
    }

    /// The value of the setting `key`, or `None` when it has no row (the default).
    pub fn get_setting(&self, key: &str) -> rusqlite::Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()
    }

    /// Stores `value` as the setting `key`, replacing the one stored before.
    pub fn set_setting(&self, key: &str, value: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Deletes the setting `key`: back to the default of the code.
    pub fn delete_setting(&self, key: &str) -> rusqlite::Result<()> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// Changes the stage of the ticket from `from` to `to` and writes the row of the history,
    /// in one transaction: both or neither. Only when the ticket is still at `from`; otherwise
    /// nothing is written, and the result says whether the ticket is gone or was moved (by
    /// another nick). The only writer of the stage is `tickets::change_stage`.
    pub fn change_ticket_stage(
        &self,
        id: i64,
        from: &str,
        to: &str,
    ) -> rusqlite::Result<StageChange> {
        // The Db has &self, so the transaction is unchecked; it rolls back when dropped.
        let tx = self.conn.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE tickets SET stage = ?3 WHERE id = ?1 AND stage = ?2",
            params![id, from, to],
        )?;
        if changed == 0 {
            let exists: bool = tx.query_row(
                "SELECT EXISTS (SELECT 1 FROM tickets WHERE id = ?1)",
                params![id],
                |row| row.get(0),
            )?;
            return Ok(if exists {
                StageChange::Moved
            } else {
                StageChange::Gone
            });
        }
        tx.execute(
            "INSERT INTO ticket_stage_changes (ticket_id, from_stage, to_stage, changed_at) \
             VALUES (?1, ?2, ?3, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
            params![id, from, to],
        )?;
        tx.commit()?;
        Ok(StageChange::Changed)
    }

    /// The history of the stage of the ticket, oldest first.
    pub fn list_stage_changes(&self, ticket_id: i64) -> rusqlite::Result<Vec<StageChangeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT from_stage, to_stage, changed_at FROM ticket_stage_changes \
             WHERE ticket_id = ?1 ORDER BY id",
        )?;
        stmt.query_map(params![ticket_id], |row| {
            Ok(StageChangeRow {
                from_stage: row.get(0)?,
                to_stage: row.get(1)?,
                changed_at: row.get(2)?,
            })
        })?
        .collect()
    }

    /// Every agent of the ticket, of every stage and state, in the order they were opened.
    pub fn list_ticket_agents(&self, ticket_id: i64) -> rusqlite::Result<Vec<Agent>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE ticket_id = ?1 ORDER BY id"
        ))?;
        stmt.query_map(params![ticket_id], agent_from_row)?
            .collect()
    }

    /// How many `active` agents the ticket or the epic has (of this nick or of another one); of
    /// an epic, only its own (the ones of its tickets: `count_active_agents_of_epic_branch`).
    pub fn count_active_agents(&self, owner: Owner) -> rusqlite::Result<i64> {
        let (ticket_id, epic_id) = owner.columns();
        self.conn.query_row(
            "SELECT count(*) FROM agents WHERE (ticket_id = ?1 OR epic_id = ?3) AND state = ?2",
            params![ticket_id, AgentState::Active.key(), epic_id],
            |row| row.get(0),
        )
    }

    /// Changes the key of the ticket from `old` to `new`; the key typed by the operator is never
    /// provisional, so `is_provisional` becomes 0. `false` when the ticket is gone or its key is
    /// no longer `old` (nothing changed). A key the project already has fails with the UNIQUE
    /// constraint (see `is_unique_violation`).
    pub fn update_ticket_key(&self, id: i64, old: &str, new: &str) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "UPDATE tickets SET key = ?3, is_provisional = 0 WHERE id = ?1 AND key = ?2",
            params![id, old, new],
        )?;
        Ok(changed > 0)
    }

    /// The agents of every ticket and every epic (NS-40) of the project, of every stage and
    /// state, in the order they were opened: what goes with it when it is removed, and the agents
    /// of an open branch of the tree (NS-38).
    pub fn list_project_agents(&self, project_id: i64) -> rusqlite::Result<Vec<Agent>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents \
             WHERE ticket_id IN (SELECT id FROM tickets WHERE project_id = ?1) \
             OR epic_id IN (SELECT id FROM epics WHERE project_id = ?1) ORDER BY id"
        ))?;
        stmt.query_map(params![project_id], agent_from_row)?
            .collect()
    }

    /// Every `active` agent, of any project, in the order they were opened.
    pub fn list_active_agents(&self) -> rusqlite::Result<Vec<Agent>> {
        let mut stmt = self.conn.prepare(&format!(
            "SELECT {AGENT_COLUMNS} FROM agents WHERE state = ?1 ORDER BY id"
        ))?;
        stmt.query_map(params![AgentState::Active.key()], agent_from_row)?
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::migrations::{
        MIGRATION_1, MIGRATION_2, MIGRATION_3, MIGRATION_4, MIGRATION_5, MIGRATION_6, MIGRATION_7,
        MIGRATIONS,
    };
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
    fn a_new_database_gets_version_7_with_auto_scrum_and_no_project() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let db = Db::open(&path).unwrap();
        assert!(path.is_file());
        assert_eq!(user_version(db.connection()), 7);
        assert_eq!(
            modules(db.connection()),
            [("auto_scrum".to_string(), "auto_scrum".to_string())]
        );
        assert_eq!(db.list_projects("auto_scrum").unwrap(), []);
    }

    #[test]
    fn the_schema_has_only_modules_projects_epics_tickets_agents_settings_and_stage_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        assert_eq!(
            tables(db.connection()),
            [
                "agents",
                "epics",
                "modules",
                "projects",
                "settings",
                "ticket_stage_changes",
                "tickets"
            ]
        );
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
        assert_eq!(user_version(db.connection()), 7);
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
            conn.execute_batch("CREATE TABLE future (x); PRAGMA user_version = 8;")
                .unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let err = Db::open(&path).err().expect("a newer database is refused");
        assert_eq!(
            err.to_string(),
            format!(
                "Erro: o banco {} está na versão 8, mais nova que a deste nick (7). \
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
            7,
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
        assert_eq!(
            MIGRATION_3,
            "\
CREATE TABLE agents (
    id         INTEGER PRIMARY KEY,
    ticket_id  INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    stage      TEXT NOT NULL
               CHECK (stage IN ('po', 'tech_leader', 'development', 'review')),
    session_id TEXT NOT NULL UNIQUE,   -- the claude --session-id, chosen by the nick
    name       TEXT NOT NULL,          -- the claude --name: 'project · Stage · KEY'
    opened_at  TEXT NOT NULL,          -- RFC 3339, in UTC
    state      TEXT NOT NULL           -- 'active' or 'ended' (the F5 adds more)
);
"
        );
        assert_eq!(MIGRATIONS[2], MIGRATION_3);
        assert_eq!(
            MIGRATION_4,
            "\
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,  -- stable identifier in the code: 'reserved_shortcut'
    value TEXT NOT NULL      -- the setting, in the format of its key (keys.rs: Shortcut::to_db)
);
"
        );
        assert_eq!(MIGRATIONS[3], MIGRATION_4);
        assert_eq!(
            MIGRATION_5,
            "\
ALTER TABLE tickets ADD COLUMN stage TEXT NOT NULL DEFAULT 'po'
    CHECK (stage IN ('po', 'tech_leader', 'development', 'review', 'done'));
CREATE TABLE ticket_stage_changes (
    id         INTEGER PRIMARY KEY,
    ticket_id  INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    from_stage TEXT NOT NULL
               CHECK (from_stage IN ('po', 'tech_leader', 'development', 'review', 'done')),
    to_stage   TEXT NOT NULL
               CHECK (to_stage IN ('po', 'tech_leader', 'development', 'review', 'done')),
    changed_at TEXT NOT NULL,   -- RFC 3339, in UTC
    CHECK (from_stage <> to_stage)
);
"
        );
        assert_eq!(MIGRATIONS[4], MIGRATION_5);
        assert_eq!(
            MIGRATION_6,
            "\
ALTER TABLE agents ADD COLUMN opened_key TEXT;  -- the ticket key at the opening; NULL before NS-39
UPDATE agents SET state = 'stopped' WHERE state = 'ended';
"
        );
        assert_eq!(MIGRATIONS[5], MIGRATION_6);
        assert_eq!(
            MIGRATION_7,
            "\
CREATE TABLE epics (
    id             INTEGER PRIMARY KEY,
    project_id     INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key            TEXT NOT NULL,     -- the Jira ID, or the provisional short UUID
    is_provisional INTEGER NOT NULL CHECK (is_provisional IN (0, 1)),
    title          TEXT NOT NULL CHECK (trim(title) <> ''),
    created_at     TEXT NOT NULL,     -- RFC 3339, in UTC
    hidden_at      TEXT,              -- RFC 3339, in UTC; NULL while visible (the whole branch)
    UNIQUE (project_id, key)
);
ALTER TABLE tickets ADD COLUMN epic_id INTEGER REFERENCES epics(id);  -- NULL: no epic
CREATE TABLE agents_new (
    id         INTEGER PRIMARY KEY,
    ticket_id  INTEGER REFERENCES tickets(id) ON DELETE CASCADE,
    epic_id    INTEGER REFERENCES epics(id) ON DELETE CASCADE,
    stage      TEXT NOT NULL
               CHECK (stage IN ('po', 'tech_leader', 'development', 'review')),
    session_id TEXT NOT NULL UNIQUE,
    name       TEXT NOT NULL,
    opened_at  TEXT NOT NULL,
    state      TEXT NOT NULL,          -- no CHECK (NS-33, NS-39): the AgentState enum
    opened_key TEXT,
    CHECK ((ticket_id IS NULL) <> (epic_id IS NULL)),
    CHECK (epic_id IS NULL OR stage IN ('po', 'tech_leader'))
);
INSERT INTO agents_new (id, ticket_id, epic_id, stage, session_id, name, opened_at, state,
                        opened_key)
    SELECT id, ticket_id, NULL, stage, session_id, name, opened_at, state, opened_key FROM agents;
DROP TABLE agents;
ALTER TABLE agents_new RENAME TO agents;
-- the key is unique across the tickets and the epics of a project (both use .nick/docs/<key>/)
CREATE TRIGGER epics_key_not_a_ticket_insert BEFORE INSERT ON epics
    WHEN EXISTS (SELECT 1 FROM tickets WHERE project_id = NEW.project_id AND key = NEW.key)
    BEGIN SELECT RAISE(ABORT, 'nick: key of a ticket'); END;
CREATE TRIGGER epics_key_not_a_ticket_update BEFORE UPDATE OF key ON epics
    WHEN EXISTS (SELECT 1 FROM tickets WHERE project_id = NEW.project_id AND key = NEW.key)
    BEGIN SELECT RAISE(ABORT, 'nick: key of a ticket'); END;
CREATE TRIGGER tickets_key_not_an_epic_insert BEFORE INSERT ON tickets
    WHEN EXISTS (SELECT 1 FROM epics WHERE project_id = NEW.project_id AND key = NEW.key)
    BEGIN SELECT RAISE(ABORT, 'nick: key of an epic'); END;
CREATE TRIGGER tickets_key_not_an_epic_update BEFORE UPDATE OF key ON tickets
    WHEN EXISTS (SELECT 1 FROM epics WHERE project_id = NEW.project_id AND key = NEW.key)
    BEGIN SELECT RAISE(ABORT, 'nick: key of an epic'); END;
-- a ticket only goes to an epic of its own project
CREATE TRIGGER tickets_epic_of_the_project_insert BEFORE INSERT ON tickets
    WHEN NEW.epic_id IS NOT NULL AND NOT EXISTS
         (SELECT 1 FROM epics WHERE id = NEW.epic_id AND project_id = NEW.project_id)
    BEGIN SELECT RAISE(ABORT, 'nick: epic of another project'); END;
CREATE TRIGGER tickets_epic_of_the_project_update BEFORE UPDATE OF epic_id ON tickets
    WHEN NEW.epic_id IS NOT NULL AND NOT EXISTS
         (SELECT 1 FROM epics WHERE id = NEW.epic_id AND project_id = NEW.project_id)
    BEGIN SELECT RAISE(ABORT, 'nick: epic of another project'); END;
"
        );
        assert_eq!(MIGRATIONS[6], MIGRATION_7);
    }

    #[test]
    fn a_database_one_version_newer_is_refused_too() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("PRAGMA user_version = 8;")
            .unwrap();
        let err = Db::open(&path).err().expect("a newer database is refused");
        assert_eq!(
            err.to_string(),
            format!(
                "Erro: o banco {} está na versão 8, mais nova que a deste nick (7). \
                 Atualize o nick.",
                path.display()
            )
        );
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
        let db = Db::open_with(&path, &[MIGRATION_1, MIGRATION_2]).unwrap();
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

    /// A ticket row of the schema before the migration 5 (NS-36): the columns of the
    /// migration 2, without `stage`. `Ticket` (and so `insert_ticket`/`list_tickets`) has the
    /// stage, so the tests of the older migrations write and read the tickets by hand.
    type OldTicket = (i64, i64, String, bool, String, String, Option<String>);

    /// Inserts a ticket in a database before the migration 5; returns its id.
    fn old_insert_ticket(
        conn: &Connection,
        project_id: i64,
        key: &str,
        provisional: bool,
        title: &str,
    ) -> i64 {
        conn.query_row(
            "INSERT INTO tickets (project_id, key, is_provisional, title, created_at) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now')) RETURNING id",
            params![project_id, key, provisional, title],
            |row| row.get(0),
        )
        .unwrap()
    }

    /// The tickets of the project (hidden included, by id), by the columns of the migration 2.
    fn old_tickets(conn: &Connection, project_id: i64) -> Vec<OldTicket> {
        let mut stmt = conn
            .prepare(
                "SELECT id, project_id, key, is_provisional, title, created_at, hidden_at \
                 FROM tickets WHERE project_id = ?1 ORDER BY id",
            )
            .unwrap();
        stmt.query_map(params![project_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
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
        let t = db
            .insert_ticket(p.id, "NS-1", false, "Primeiro", None)
            .unwrap();
        assert_eq!(t.project_id, p.id);
        assert_eq!(t.key, "NS-1");
        assert!(!t.is_provisional);
        assert_eq!(t.title, "Primeiro");
        assert_eq!(t.hidden_at, None);
        assert_rfc3339_utc(&t.created_at);
        let u = db
            .insert_ticket(p.id, "3f9a0c7e", true, "Segundo", None)
            .unwrap();
        assert!(u.is_provisional);
        assert_eq!(db.list_tickets(p.id, false).unwrap(), [t, u]);
        let dup = db
            .insert_ticket(p.id, "NS-1", false, "x", None)
            .unwrap_err();
        assert!(is_unique_violation(&dup), "{dup:?}");
    }

    #[test]
    fn list_tickets_is_by_creation_of_that_project_and_hides_the_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        db.insert_ticket(a.id, "Z-9", false, "z", None).unwrap();
        db.insert_ticket(b.id, "B-1", false, "b", None).unwrap();
        let hidden = db.insert_ticket(a.id, "A-1", false, "a", None).unwrap();
        db.insert_ticket(a.id, "M-5", false, "m", None).unwrap();
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
    fn find_key_says_if_a_ticket_key_exists_and_if_it_is_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        db.insert_ticket(a.id, "V-1", false, "v", None).unwrap();
        let h = db.insert_ticket(a.id, "H-1", false, "h", None).unwrap();
        db.set_ticket_hidden(h.id, true).unwrap();
        assert_eq!(
            db.find_key(a.id, "V-1").unwrap(),
            Some((KeyKind::Ticket, false))
        );
        assert_eq!(
            db.find_key(a.id, "H-1").unwrap(),
            Some((KeyKind::Ticket, true))
        );
        assert_eq!(db.find_key(a.id, "v-1").unwrap(), None);
        assert_eq!(db.find_key(b.id, "V-1").unwrap(), None);
    }

    #[test]
    fn update_ticket_title_changes_only_the_title() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let p = db.insert_project("auto_scrum", "p", "/p").unwrap();
        let t = db
            .insert_ticket(p.id, "NS-1", false, "Velho", None)
            .unwrap();
        let other = db
            .insert_ticket(p.id, "NS-2", false, "Outro", None)
            .unwrap();
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
        let t = db.insert_ticket(p.id, "NS-1", false, "t", None).unwrap();
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
        let t = db.insert_ticket(p.id, "NS-1", false, "t", None).unwrap();
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
        db.insert_ticket(a.id, "A-1", false, "t", None).unwrap();
        let h = db.insert_ticket(a.id, "A-2", false, "t", None).unwrap();
        db.set_ticket_hidden(h.id, true).unwrap();
        db.insert_ticket(b.id, "B-1", false, "t", None).unwrap();
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

    // ---- migration 3: agents ----

    /// A project with one ticket, for the agents tests: (db, project id, ticket id).
    fn with_ticket(tmp: &tempfile::TempDir) -> (Db, i64, i64) {
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let p = db.insert_project("auto_scrum", "p", "/p").unwrap();
        let t = db.insert_ticket(p.id, "NS-1", false, "t", None).unwrap();
        (db, p.id, t.id)
    }

    /// Inserts an agent row by hand (the schema tests check the constraints, not the queries).
    fn raw_agent(
        conn: &Connection,
        ticket_id: i64,
        stage: &str,
        session_id: &str,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO agents (ticket_id, stage, session_id, name, opened_at, state) \
             VALUES (?1, ?2, ?3, 'n', 'now', 'active')",
            params![ticket_id, stage, session_id],
        )
    }

    fn agent_sessions(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT session_id FROM agents ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn a_version_2_database_with_projects_and_tickets_migrates_to_3_and_keeps_them() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let (projects, tickets, ns1) = {
            let old = Db::open_with(&path, &[MIGRATION_1, MIGRATION_2]).unwrap();
            assert_eq!(user_version(old.connection()), 2);
            let conn = old.connection();
            let a = old.insert_project("auto_scrum", "a", "/a").unwrap();
            let b = old.insert_project("auto_scrum", "b", "/b").unwrap();
            let t1 = old_insert_ticket(conn, a.id, "NS-1", false, "um");
            let t2 = old_insert_ticket(conn, a.id, "3f9a0c7e", true, "dois");
            old.set_ticket_hidden(t2, true).unwrap();
            old_insert_ticket(conn, b.id, "NS-1", false, "três");
            (
                old.list_projects("auto_scrum").unwrap(),
                [old_tickets(conn, a.id), old_tickets(conn, b.id)],
                t1,
            )
        };
        let db = Db::open_with(&path, &[MIGRATION_1, MIGRATION_2, MIGRATION_3]).unwrap();
        assert_eq!(user_version(db.connection()), 3);
        assert_eq!(
            tables(db.connection()),
            ["agents", "modules", "projects", "tickets"]
        );
        assert_eq!(db.list_projects("auto_scrum").unwrap(), projects);
        assert_eq!(old_tickets(db.connection(), projects[0].id), tickets[0]);
        assert_eq!(old_tickets(db.connection(), projects[1].id), tickets[1]);
        assert_eq!(old_active_agents(db.connection()), []);
        raw_agent(db.connection(), ns1, "po", "s-1").unwrap();
    }

    #[test]
    fn the_stage_is_checked() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        let conn = db.connection();
        for (n, stage) in ["po", "tech_leader", "development", "review"]
            .iter()
            .enumerate()
        {
            raw_agent(conn, ticket, stage, &format!("ok-{n}")).unwrap();
        }
        for stage in ["", "PO", "dev", "Review", "po_jira"] {
            assert!(
                raw_agent(conn, ticket, stage, &format!("bad-{stage}")).is_err(),
                "stage {stage:?}"
            );
        }
        assert_eq!(agent_sessions(conn).len(), 4);
    }

    #[test]
    fn the_session_id_is_unique() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let other = db.insert_ticket(project, "NS-2", false, "t", None).unwrap();
        let conn = db.connection();
        raw_agent(conn, ticket, "po", "same").unwrap();
        let dup = raw_agent(conn, other.id, "review", "same").unwrap_err();
        assert!(is_unique_violation(&dup), "{dup:?}");
        let dup = db
            .insert_agent(Owner::Ticket(ticket), "development", "same", "n", "NS-1")
            .unwrap_err();
        assert!(is_unique_violation(&dup), "{dup:?}");
        assert_eq!(agent_sessions(conn), ["same"]);
    }

    #[test]
    fn an_agent_of_a_missing_ticket_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, _) = with_ticket(&tmp);
        assert!(raw_agent(db.connection(), 99, "po", "s").is_err());
        assert!(
            db.insert_agent(Owner::Ticket(99), "po", "s", "n", "NS-1")
                .is_err()
        );
        assert_eq!(agent_sessions(db.connection()), Vec::<String>::new());
    }

    #[test]
    fn deleting_a_project_deletes_its_tickets_and_agents_and_no_other() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        let ta = db.insert_ticket(a.id, "A-1", false, "t", None).unwrap();
        let tb = db.insert_ticket(b.id, "B-1", false, "t", None).unwrap();
        db.insert_agent(Owner::Ticket(ta.id), "po", "a-1", "n", "NS-1")
            .unwrap();
        db.insert_agent(Owner::Ticket(ta.id), "review", "a-2", "n", "NS-1")
            .unwrap();
        db.insert_agent(Owner::Ticket(tb.id), "development", "b-1", "n", "NS-1")
            .unwrap();
        db.delete_project(a.id).unwrap();
        assert_eq!(ticket_keys(db.connection()), [(b.id, "B-1".to_string())]);
        assert_eq!(agent_sessions(db.connection()), ["b-1"]);
    }

    #[test]
    fn random_session_id_is_a_lowercase_uuid_v4_and_changes() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let mut seen = std::collections::HashSet::new();
        for _ in 0..200 {
            let id = db.random_session_id().unwrap();
            let parts: Vec<&str> = id.split('-').collect();
            assert_eq!(
                parts.iter().map(|p| p.len()).collect::<Vec<_>>(),
                [8, 4, 4, 4, 12],
                "{id}"
            );
            assert!(
                id.chars()
                    .all(|c| c == '-' || matches!(c, '0'..='9' | 'a'..='f')),
                "{id}"
            );
            assert!(parts[2].starts_with('4'), "version 4: {id}");
            assert!(
                matches!(parts[3].as_bytes()[0], b'8' | b'9' | b'a' | b'b'),
                "variant: {id}"
            );
            seen.insert(id);
        }
        assert_eq!(seen.len(), 200, "it changes between calls");
    }

    #[test]
    fn insert_agent_returns_the_stored_row_active() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        let agent = db
            .insert_agent(
                Owner::Ticket(ticket),
                "tech_leader",
                "s-1",
                "p · Tech Leader · NS-1",
                "NS-1",
            )
            .unwrap();
        assert_eq!(agent.owner, Owner::Ticket(ticket));
        assert_eq!(agent.stage, "tech_leader");
        assert_eq!(agent.session_id, "s-1");
        assert_eq!(agent.name, "p · Tech Leader · NS-1");
        assert_eq!(agent.state, "active");
        assert_eq!(agent.opened_key.as_deref(), Some("NS-1"));
        assert_rfc3339_utc(&agent.opened_at);
        assert_eq!(db.list_active_agents().unwrap(), [agent]);
    }

    #[test]
    fn stop_agent_changes_only_the_state_and_says_whether_the_row_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let first = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n1", "NS-1")
            .unwrap();
        let second = db
            .insert_agent(Owner::Ticket(ticket), "review", "s-2", "n2", "NS-1")
            .unwrap();
        assert!(db.stop_agent(first.id).unwrap());
        assert_eq!(
            db.list_active_agents().unwrap(),
            std::slice::from_ref(&second)
        );
        let stored: (String, String, String, String, String) = db
            .connection()
            .query_row(
                "SELECT stage, session_id, name, opened_at, state FROM agents WHERE id = ?1",
                params![first.id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            stored,
            (
                first.stage,
                first.session_id,
                first.name,
                first.opened_at,
                "stopped".to_string()
            )
        );
        // Ending again is harmless; a row that is gone says so.
        assert!(db.stop_agent(first.id).unwrap());
        db.delete_project(project).unwrap();
        assert!(!db.stop_agent(second.id).unwrap());
    }

    #[test]
    fn list_active_agents_lists_only_the_active_ones_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let other = db.insert_ticket(project, "NS-2", false, "t", None).unwrap();
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        let b = db
            .insert_agent(Owner::Ticket(other.id), "review", "s-2", "n", "NS-1")
            .unwrap();
        let c = db
            .insert_agent(Owner::Ticket(ticket), "development", "s-3", "n", "NS-1")
            .unwrap();
        db.stop_agent(b.id).unwrap();
        assert_eq!(db.list_active_agents().unwrap(), [a, c]);
    }

    /// The agents of a project, of every ticket (the hidden ones too), stage and state, in the
    /// order they were opened; none of another project (the removal of a project, NS-38).
    #[test]
    fn list_project_agents_lists_every_agent_of_the_tickets_of_that_project() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let hidden = db.insert_ticket(project, "NS-2", false, "t", None).unwrap();
        db.set_ticket_hidden(hidden.id, true).unwrap();
        let other = db.insert_project("auto_scrum", "q", "/q").unwrap();
        let elsewhere = db
            .insert_ticket(other.id, "NS-1", false, "t", None)
            .unwrap();
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        let b = db
            .insert_agent(Owner::Ticket(hidden.id), "review", "s-2", "n", "NS-1")
            .unwrap();
        db.insert_agent(Owner::Ticket(elsewhere.id), "po", "s-3", "n", "NS-1")
            .unwrap();
        let c = db
            .insert_agent(Owner::Ticket(ticket), "development", "s-4", "n", "NS-1")
            .unwrap();
        db.stop_agent(b.id).unwrap();
        let b = db.list_ticket_agents(hidden.id).unwrap().remove(0);
        assert_eq!(db.list_project_agents(project).unwrap(), [a, b, c]);
        assert_eq!(db.list_project_agents(9999).unwrap(), []);
    }

    // ---- migration 4: settings (NS-34) ----

    #[test]
    fn a_version_3_database_with_projects_tickets_and_agents_migrates_to_4_and_keeps_them() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let (projects, tickets, agents) = {
            let old = Db::open_with(&path, &[MIGRATION_1, MIGRATION_2, MIGRATION_3]).unwrap();
            assert_eq!(user_version(old.connection()), 3);
            let conn = old.connection();
            let a = old.insert_project("auto_scrum", "a", "/a").unwrap();
            let b = old.insert_project("auto_scrum", "b", "/b").unwrap();
            let t1 = old_insert_ticket(conn, a.id, "NS-1", false, "um");
            let t2 = old_insert_ticket(conn, b.id, "3f9a0c7e", true, "dois");
            old.set_ticket_hidden(t2, true).unwrap();
            let g1 = old_insert_agent(conn, t1, "po", "s-1", "a · PO · NS-1");
            old_insert_agent(conn, t2, "review", "s-2", "b · Review · 3f9a0c7e");
            old_end_agent(conn, g1);
            (
                old.list_projects("auto_scrum").unwrap(),
                [old_tickets(conn, a.id), old_tickets(conn, b.id)],
                old_active_agents(conn),
            )
        };
        let db =
            Db::open_with(&path, &[MIGRATION_1, MIGRATION_2, MIGRATION_3, MIGRATION_4]).unwrap();
        assert_eq!(user_version(db.connection()), 4);
        assert_eq!(
            tables(db.connection()),
            ["agents", "modules", "projects", "settings", "tickets"]
        );
        assert_eq!(db.list_projects("auto_scrum").unwrap(), projects);
        assert_eq!(old_tickets(db.connection(), projects[0].id), tickets[0]);
        assert_eq!(old_tickets(db.connection(), projects[1].id), tickets[1]);
        assert_eq!(old_active_agents(db.connection()), agents);
        assert_eq!(agent_sessions(db.connection()), ["s-1", "s-2"]);
        assert_eq!(
            db.get_setting(SETTING_RESERVED_SHORTCUT).unwrap(),
            None,
            "the shortcut is the default"
        );
    }

    #[test]
    fn a_setting_is_stored_overwritten_and_deleted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let db = Db::open(&path).unwrap();
        assert_eq!(db.get_setting(SETTING_RESERVED_SHORTCUT).unwrap(), None);
        db.set_setting(SETTING_RESERVED_SHORTCUT, "ctrl+]").unwrap();
        assert_eq!(
            db.get_setting(SETTING_RESERVED_SHORTCUT)
                .unwrap()
                .as_deref(),
            Some("ctrl+]")
        );
        db.set_setting(SETTING_RESERVED_SHORTCUT, "f7").unwrap();
        assert_eq!(
            db.get_setting(SETTING_RESERVED_SHORTCUT)
                .unwrap()
                .as_deref(),
            Some("f7"),
            "overwritten"
        );
        let count: i64 = db
            .connection()
            .query_row("SELECT count(*) FROM settings", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1, "one row per key");
        assert_eq!(db.get_setting("other").unwrap(), None);
        // It is kept for the next opening.
        drop(db);
        let db = Db::open(&path).unwrap();
        assert_eq!(
            db.get_setting(SETTING_RESERVED_SHORTCUT)
                .unwrap()
                .as_deref(),
            Some("f7")
        );
        db.delete_setting(SETTING_RESERVED_SHORTCUT).unwrap();
        assert_eq!(db.get_setting(SETTING_RESERVED_SHORTCUT).unwrap(), None);
        db.delete_setting(SETTING_RESERVED_SHORTCUT).unwrap();
    }

    // ---- migration 5: the stage of the ticket and its history (NS-36) ----

    fn stage_changes(conn: &Connection) -> Vec<(i64, String, String)> {
        let mut stmt = conn
            .prepare("SELECT ticket_id, from_stage, to_stage FROM ticket_stage_changes ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn stage_of(db: &Db, ticket: i64) -> String {
        db.connection()
            .query_row("SELECT stage FROM tickets WHERE id = ?1", [ticket], |row| {
                row.get(0)
            })
            .unwrap()
    }

    fn raw_change(conn: &Connection, ticket: i64, from: &str, to: &str) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO ticket_stage_changes (ticket_id, from_stage, to_stage, changed_at) \
             VALUES (?1, ?2, ?3, 'now')",
            params![ticket, from, to],
        )
    }

    #[test]
    fn a_version_4_database_with_data_migrates_to_5_with_every_ticket_in_po() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let (projects, tickets, agents, ids) = {
            let old = Db::open_with(&path, &[MIGRATION_1, MIGRATION_2, MIGRATION_3, MIGRATION_4])
                .unwrap();
            assert_eq!(user_version(old.connection()), 4);
            let conn = old.connection();
            let a = old.insert_project("auto_scrum", "a", "/a").unwrap();
            let b = old.insert_project("auto_scrum", "b", "/b").unwrap();
            let t1 = old_insert_ticket(conn, a.id, "NS-1", false, "um");
            let t2 = old_insert_ticket(conn, a.id, "3f9a0c7e", true, "dois");
            old.set_ticket_hidden(t2, true).unwrap();
            let t3 = old_insert_ticket(conn, b.id, "NS-1", false, "três");
            let g = old_insert_agent(conn, t1, "po", "s-1", "a · PO · NS-1");
            old_insert_agent(conn, t3, "development", "s-2", "b · Dev · NS-1");
            old_end_agent(conn, g);
            old.set_setting(SETTING_RESERVED_SHORTCUT, "f7").unwrap();
            (
                old.list_projects("auto_scrum").unwrap(),
                [old_tickets(conn, a.id), old_tickets(conn, b.id)],
                old_active_agents(conn),
                [t1, t2, t3],
            )
        };
        let db = Db::open_with(
            &path,
            &[
                MIGRATION_1,
                MIGRATION_2,
                MIGRATION_3,
                MIGRATION_4,
                MIGRATION_5,
            ],
        )
        .unwrap();
        assert_eq!(user_version(db.connection()), 5);
        assert_eq!(db.list_projects("auto_scrum").unwrap(), projects);
        assert_eq!(old_tickets(db.connection(), projects[0].id), tickets[0]);
        assert_eq!(old_tickets(db.connection(), projects[1].id), tickets[1]);
        assert_eq!(old_active_agents(db.connection()), agents);
        assert_eq!(agent_sessions(db.connection()), ["s-1", "s-2"]);
        assert_eq!(
            db.get_setting(SETTING_RESERVED_SHORTCUT)
                .unwrap()
                .as_deref(),
            Some("f7")
        );
        for id in ids {
            assert_eq!(stage_of(&db, id), "po", "every ticket starts in PO");
        }
        // By raw SQL since the NS-40: `Ticket` has the `epic_id` of the migration 7.
        let all: Vec<String> = db
            .connection()
            .prepare("SELECT stage FROM tickets WHERE project_id = ?1 ORDER BY id")
            .unwrap()
            .query_map([projects[0].id], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(all, ["po", "po"]);
        assert_eq!(stage_changes(db.connection()), []);
        // A new ticket is born in PO too.
        let t = old_insert_ticket(db.connection(), projects[1].id, "NS-9", false, "novo");
        assert_eq!(stage_of(&db, t), "po");
    }

    #[test]
    fn the_stage_of_the_ticket_and_of_the_history_are_checked() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        let conn = db.connection();
        for stage in ["tech_leader", "development", "review", "done", "po"] {
            conn.execute(
                "UPDATE tickets SET stage = ?2 WHERE id = ?1",
                params![ticket, stage],
            )
            .unwrap();
        }
        for stage in ["", "PO", "dev", "Done", "concluido", "po_jira"] {
            assert!(
                conn.execute(
                    "UPDATE tickets SET stage = ?2 WHERE id = ?1",
                    params![ticket, stage],
                )
                .is_err(),
                "ticket stage {stage:?}"
            );
            assert!(
                raw_change(conn, ticket, stage, "po").is_err(),
                "from {stage:?}"
            );
            assert!(
                raw_change(conn, ticket, "po", stage).is_err(),
                "to {stage:?}"
            );
        }
        assert!(
            raw_change(conn, ticket, "review", "review").is_err(),
            "same stage"
        );
        assert!(
            raw_change(conn, 99, "po", "review").is_err(),
            "missing ticket"
        );
        assert_eq!(stage_of(&db, ticket), "po");
        assert_eq!(stage_changes(conn), []);
        raw_change(conn, ticket, "done", "po").unwrap();
        assert_eq!(stage_changes(conn).len(), 1);
    }

    #[test]
    fn the_history_goes_with_its_ticket_and_its_project_and_no_other() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        let ta1 = db.insert_ticket(a.id, "A-1", false, "t", None).unwrap();
        let ta2 = db.insert_ticket(a.id, "A-2", false, "t", None).unwrap();
        let tb = db.insert_ticket(b.id, "B-1", false, "t", None).unwrap();
        let conn = db.connection();
        raw_change(conn, ta1.id, "po", "review").unwrap();
        raw_change(conn, ta2.id, "po", "done").unwrap();
        raw_change(conn, tb.id, "po", "development").unwrap();
        conn.execute("DELETE FROM tickets WHERE id = ?1", [ta1.id])
            .unwrap();
        assert_eq!(
            stage_changes(conn),
            [
                (ta2.id, "po".to_string(), "done".to_string()),
                (tb.id, "po".to_string(), "development".to_string())
            ]
        );
        db.delete_project(a.id).unwrap();
        assert_eq!(
            stage_changes(conn),
            [(tb.id, "po".to_string(), "development".to_string())]
        );
    }

    #[test]
    fn change_ticket_stage_stores_the_stage_and_one_history_row() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let other = db.insert_ticket(project, "NS-2", false, "t", None).unwrap();
        assert_eq!(
            db.change_ticket_stage(ticket, "po", "review").unwrap(),
            StageChange::Changed
        );
        assert_eq!(
            db.change_ticket_stage(ticket, "review", "development")
                .unwrap(),
            StageChange::Changed,
            "backwards too"
        );
        assert_eq!(stage_of(&db, ticket), "development");
        assert_eq!(stage_of(&db, other.id), "po", "only that ticket");
        let changes = db.list_stage_changes(ticket).unwrap();
        assert_eq!(
            changes
                .iter()
                .map(|c| (c.from_stage.as_str(), c.to_stage.as_str()))
                .collect::<Vec<_>>(),
            [("po", "review"), ("review", "development")]
        );
        for change in &changes {
            assert_rfc3339_utc(&change.changed_at);
        }
        assert_eq!(db.list_stage_changes(other.id).unwrap(), []);
        let t = &db.list_tickets(project, true).unwrap()[0];
        assert_eq!(t.stage, "development");
    }

    #[test]
    fn change_ticket_stage_gone_and_moved_store_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        // Another nick moved it to review: this one still believes it is in po.
        db.change_ticket_stage(ticket, "po", "review").unwrap();
        assert_eq!(
            db.change_ticket_stage(ticket, "po", "done").unwrap(),
            StageChange::Moved
        );
        assert_eq!(stage_of(&db, ticket), "review");
        assert_eq!(db.list_stage_changes(ticket).unwrap().len(), 1);
        db.delete_project(project).unwrap();
        assert_eq!(
            db.change_ticket_stage(ticket, "review", "done").unwrap(),
            StageChange::Gone
        );
        assert_eq!(stage_changes(db.connection()), []);
    }

    /// The UPDATE and the INSERT go together or not at all: a history that can't be written
    /// (a test trigger) leaves the stage as it was.
    #[test]
    fn change_ticket_stage_is_all_or_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        db.connection()
            .execute_batch(
                "CREATE TRIGGER no_history BEFORE INSERT ON ticket_stage_changes \
                 BEGIN SELECT RAISE(ABORT, 'no history'); END;",
            )
            .unwrap();
        let err = db.change_ticket_stage(ticket, "po", "review").unwrap_err();
        assert!(err.to_string().contains("no history"), "{err}");
        assert_eq!(stage_of(&db, ticket), "po");
        assert_eq!(stage_changes(db.connection()), []);
        // The connection is usable afterwards (no transaction left open).
        db.connection()
            .execute_batch("DROP TRIGGER no_history;")
            .unwrap();
        assert_eq!(
            db.change_ticket_stage(ticket, "po", "review").unwrap(),
            StageChange::Changed
        );
    }

    #[test]
    fn list_ticket_agents_lists_every_state_in_order_and_only_of_that_ticket() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let other = db.insert_ticket(project, "NS-2", false, "t", None).unwrap();
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        db.insert_agent(Owner::Ticket(other.id), "review", "s-2", "n", "NS-1")
            .unwrap();
        let c = db
            .insert_agent(Owner::Ticket(ticket), "development", "s-3", "n", "NS-1")
            .unwrap();
        db.stop_agent(a.id).unwrap();
        let listed = db.list_ticket_agents(ticket).unwrap();
        assert_eq!(
            listed
                .iter()
                .map(|g| g.session_id.as_str())
                .collect::<Vec<_>>(),
            ["s-1", "s-3"]
        );
        assert_eq!(listed[0].state, "stopped");
        assert_eq!(listed[1], c);
        assert_eq!(db.list_ticket_agents(999).unwrap(), []);
    }

    #[test]
    fn count_active_agents_counts_only_the_active_ones_of_the_ticket() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let other = db.insert_ticket(project, "NS-2", false, "t", None).unwrap();
        assert_eq!(db.count_active_agents(Owner::Ticket(ticket)).unwrap(), 0);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        db.insert_agent(Owner::Ticket(ticket), "review", "s-2", "n", "NS-1")
            .unwrap();
        db.insert_agent(Owner::Ticket(other.id), "review", "s-3", "n", "NS-1")
            .unwrap();
        assert_eq!(db.count_active_agents(Owner::Ticket(ticket)).unwrap(), 2);
        db.stop_agent(a.id).unwrap();
        assert_eq!(db.count_active_agents(Owner::Ticket(ticket)).unwrap(), 1);
        assert_eq!(db.count_active_agents(Owner::Ticket(other.id)).unwrap(), 1);
    }

    #[test]
    fn get_ticket_reads_one_ticket_by_id_hidden_or_not() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let hidden = db
            .insert_ticket(project, "NS-2", true, "oculto", None)
            .unwrap();
        db.set_ticket_hidden(hidden.id, true).unwrap();
        let all = db.list_tickets(project, true).unwrap();
        assert_eq!(db.get_ticket(ticket).unwrap().as_ref(), Some(&all[0]));
        assert_eq!(db.get_ticket(hidden.id).unwrap().as_ref(), Some(&all[1]));
        assert_eq!(db.get_ticket(999).unwrap(), None);
        db.delete_project(project).unwrap();
        assert_eq!(
            db.get_ticket(ticket).unwrap(),
            None,
            "gone with its project"
        );
    }

    #[test]
    fn update_ticket_key_changes_the_key_and_clears_is_provisional() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        let t = db.insert_ticket(a.id, "3f9a0c7e", true, "t", None).unwrap();
        let other = db.insert_ticket(a.id, "NS-2", false, "o", None).unwrap();
        db.insert_ticket(b.id, "NS-7", false, "b", None).unwrap();
        assert!(
            db.update_ticket_key(t.id, "3f9a0c7e", "NS-7").unwrap(),
            "another project's key"
        );
        let stored = &db.list_tickets(a.id, true).unwrap()[0];
        assert_eq!(stored.key, "NS-7");
        assert!(!stored.is_provisional);
        assert_eq!(
            (stored.id, &stored.title, &stored.stage),
            (t.id, &t.title, &t.stage)
        );
        // The old key no longer matches: another nick changed it.
        assert!(!db.update_ticket_key(t.id, "3f9a0c7e", "NS-8").unwrap());
        let dup = db.update_ticket_key(t.id, "NS-7", "NS-2").unwrap_err();
        assert!(is_unique_violation(&dup), "{dup:?}");
        assert_eq!(db.list_tickets(a.id, true).unwrap()[1], other);
        // The provisional ID typed with another case is not provisional any more.
        let p = db.insert_ticket(a.id, "abcd1234", true, "p", None).unwrap();
        assert!(db.update_ticket_key(p.id, "abcd1234", "ABCD1234").unwrap());
        assert!(!db.list_tickets(a.id, true).unwrap()[2].is_provisional);
        db.delete_project(a.id).unwrap();
        assert!(!db.update_ticket_key(t.id, "NS-7", "NS-9").unwrap(), "gone");
    }

    /// An agent row of the schema before the migration 6 (NS-39): the columns of the
    /// migration 3, without `opened_key`. `Agent` (and so `insert_agent`/`list_active_agents`)
    /// has the key, so the tests of the older migrations write and read the agents by hand (the
    /// mold of `old_insert_ticket`, NS-36).
    type OldAgent = (i64, i64, String, String, String, String, String);

    /// Inserts an `active` agent in a database before the migration 6; returns its id.
    fn old_insert_agent(
        conn: &Connection,
        ticket: i64,
        stage: &str,
        session: &str,
        name: &str,
    ) -> i64 {
        conn.query_row(
            "INSERT INTO agents (ticket_id, stage, session_id, name, opened_at, state) \
             VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), 'active') \
             RETURNING id",
            params![ticket, stage, session, name],
            |row| row.get(0),
        )
        .unwrap()
    }

    /// Ends an agent of a database before the migration 6, as the nick of then did.
    fn old_end_agent(conn: &Connection, id: i64) {
        conn.execute("UPDATE agents SET state = 'ended' WHERE id = ?1", [id])
            .unwrap();
    }

    /// The `active` agents, by the columns of the migration 3, in the order they were opened.
    fn old_active_agents(conn: &Connection) -> Vec<OldAgent> {
        let mut stmt = conn
            .prepare(
                "SELECT id, ticket_id, stage, session_id, name, opened_at, state FROM agents \
                 WHERE state = 'active' ORDER BY id",
            )
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    // ---- migration 6: stopped, archived and the key at the opening (NS-39) ----

    /// (session_id, state, opened_key) of every agent, by id.
    fn agent_states(conn: &Connection) -> Vec<(String, String, Option<String>)> {
        let mut stmt = conn
            .prepare("SELECT session_id, state, opened_key FROM agents ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    #[test]
    fn a_version_5_database_with_agents_migrates_to_6_ended_becoming_stopped() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let tickets = {
            let old = Db::open_with(
                &path,
                &[
                    MIGRATION_1,
                    MIGRATION_2,
                    MIGRATION_3,
                    MIGRATION_4,
                    MIGRATION_5,
                ],
            )
            .unwrap();
            assert_eq!(user_version(old.connection()), 5);
            let conn = old.connection();
            let p = old.insert_project("auto_scrum", "a", "/a").unwrap();
            // By raw SQL since the NS-40: `Ticket` and `Agent` have the columns of the 7.
            let t = old_insert_ticket(conn, p.id, "NS-1", false, "um");
            let ended = old_insert_agent(conn, t, "po", "s-1", "a · PO · NS-1");
            old_insert_agent(conn, t, "development", "s-2", "a · Dev · NS-1");
            old_end_agent(conn, ended);
            old.set_setting(SETTING_RESERVED_SHORTCUT, "f7").unwrap();
            (p.id, old_tickets(conn, p.id))
        };
        // Only up to the migration 6: the test of the migration 7 is its own (NS-40).
        let db = Db::open_with(&path, &MIGRATIONS[..6]).unwrap();
        assert_eq!(user_version(db.connection()), 6);
        assert_eq!(
            agent_states(db.connection()),
            [
                ("s-1".to_string(), "stopped".to_string(), None),
                ("s-2".to_string(), "active".to_string(), None),
            ],
            "ended becomes stopped, active stays, and the key at the opening is unknown"
        );
        assert_eq!(old_tickets(db.connection(), tickets.0), tickets.1);
        assert_eq!(
            db.get_setting(SETTING_RESERVED_SHORTCUT)
                .unwrap()
                .as_deref(),
            Some("f7")
        );
        let agents = old_active_agents(db.connection());
        assert_eq!(agent_states(db.connection())[0].2, None);
        let name: String = db
            .connection()
            .query_row(
                "SELECT name FROM agents WHERE ticket_id = ?1 ORDER BY id",
                [tickets.1[0].0],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(name, "a · PO · NS-1");
        assert_eq!(agents.len(), 1, "s-2 stays active");
    }

    #[test]
    fn get_agent_reads_one_agent_by_id() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        assert_eq!(db.get_agent(a.id).unwrap(), Some(a.clone()));
        assert_eq!(db.get_agent(999).unwrap(), None);
        db.delete_project(project).unwrap();
        assert_eq!(db.get_agent(a.id).unwrap(), None, "gone with its project");
    }

    /// The state of the agent `id`, raw.
    fn state_of(db: &Db, id: i64) -> String {
        db.connection()
            .query_row("SELECT state FROM agents WHERE id = ?1", [id], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn resume_agent_makes_a_stopped_or_active_one_active_and_says_why_not() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        db.stop_agent(a.id).unwrap();
        assert_eq!(db.resume_agent(a.id).unwrap(), AgentChange::Done);
        assert_eq!(state_of(&db, a.id), "active");
        // An active row with no owner (its nick died): resumed too.
        assert_eq!(db.resume_agent(a.id).unwrap(), AgentChange::Done);
        assert_eq!(state_of(&db, a.id), "active");
        db.stop_agent(a.id).unwrap();
        db.archive_agent(a.id).unwrap();
        assert_eq!(db.resume_agent(a.id).unwrap(), AgentChange::Archived);
        assert_eq!(
            state_of(&db, a.id),
            "archived",
            "an archived one is not resumed"
        );
        db.delete_project(project).unwrap();
        assert_eq!(db.resume_agent(a.id).unwrap(), AgentChange::Gone);
    }

    #[test]
    fn archive_agent_archives_only_a_stopped_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        let other = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-2", "n", "NS-1")
            .unwrap();
        db.stop_agent(other.id).unwrap();
        assert_eq!(db.archive_agent(a.id).unwrap(), AgentChange::Active);
        assert_eq!(
            state_of(&db, a.id),
            "active",
            "an active one is not archived"
        );
        db.stop_agent(a.id).unwrap();
        assert_eq!(db.archive_agent(a.id).unwrap(), AgentChange::Done);
        assert_eq!(state_of(&db, a.id), "archived");
        assert_eq!(state_of(&db, other.id), "stopped", "only that agent");
        assert_eq!(db.archive_agent(a.id).unwrap(), AgentChange::Archived);
        db.delete_project(project).unwrap();
        assert_eq!(db.archive_agent(a.id).unwrap(), AgentChange::Gone);
    }

    #[test]
    fn unarchive_agent_takes_an_archived_one_back_to_stopped() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        assert_eq!(db.unarchive_agent(a.id).unwrap(), AgentChange::Active);
        db.stop_agent(a.id).unwrap();
        assert_eq!(db.unarchive_agent(a.id).unwrap(), AgentChange::Stopped);
        db.archive_agent(a.id).unwrap();
        assert_eq!(db.unarchive_agent(a.id).unwrap(), AgentChange::Done);
        assert_eq!(state_of(&db, a.id), "stopped");
        db.delete_project(project).unwrap();
        assert_eq!(db.unarchive_agent(a.id).unwrap(), AgentChange::Gone);
    }

    /// A state this nick does not know (the column has no CHECK) is never acted on.
    #[test]
    fn an_unknown_state_is_not_resumed_archived_nor_unarchived() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        db.connection()
            .execute("UPDATE agents SET state = 'future' WHERE id = ?1", [a.id])
            .unwrap();
        assert_eq!(db.resume_agent(a.id).unwrap(), AgentChange::Gone);
        assert_eq!(db.archive_agent(a.id).unwrap(), AgentChange::Gone);
        assert_eq!(db.unarchive_agent(a.id).unwrap(), AgentChange::Gone);
        assert_eq!(state_of(&db, a.id), "future");
    }

    // ---- migration 7: the epics, the owner of an agent and the triggers (NS-40) ----

    /// The migrations 1 to 6: a database of before the NS-40.
    const BEFORE_7: [&str; 6] = [
        MIGRATION_1,
        MIGRATION_2,
        MIGRATION_3,
        MIGRATION_4,
        MIGRATION_5,
        MIGRATION_6,
    ];

    /// Every column of every agent, by id (the raw row, of any version from the migration 6 on;
    /// `epic_id` only when the column exists).
    type RawAgent = (
        i64,
        Option<i64>,
        String,
        String,
        String,
        String,
        String,
        Option<String>,
    );

    fn raw_agents(conn: &Connection) -> Vec<RawAgent> {
        let mut stmt = conn
            .prepare(
                "SELECT id, ticket_id, stage, session_id, name, opened_at, state, opened_key \
                 FROM agents ORDER BY id",
            )
            .unwrap();
        stmt.query_map([], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get(3)?,
                row.get(4)?,
                row.get(5)?,
                row.get(6)?,
                row.get(7)?,
            ))
        })
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    /// Inserts an agent in a database of the migration 6 (with `opened_key`); returns its id.
    fn v6_insert_agent(
        conn: &Connection,
        ticket: i64,
        stage: &str,
        session: &str,
        state: &str,
        opened_key: Option<&str>,
    ) -> i64 {
        conn.query_row(
            "INSERT INTO agents (ticket_id, stage, session_id, name, opened_at, state, \
             opened_key) VALUES (?1, ?2, ?3, 'p · PO · NS-1', \
             strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?4, ?5) RETURNING id",
            params![ticket, stage, session, state, opened_key],
            |row| row.get(0),
        )
        .unwrap()
    }

    /// The rows `PRAGMA foreign_key_check` reports, and the answer of `PRAGMA integrity_check`.
    fn checks(conn: &Connection) -> (usize, String) {
        let mut stmt = conn.prepare("PRAGMA foreign_key_check").unwrap();
        let broken = stmt.query_map([], |_| Ok(())).unwrap().count();
        let integrity: String = conn
            .query_row("PRAGMA integrity_check", [], |row| row.get(0))
            .unwrap();
        (broken, integrity)
    }

    #[test]
    fn a_version_6_database_with_agents_migrates_to_7_keeping_every_row() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let (project, before, tickets) = {
            let old = Db::open_with(&path, &BEFORE_7).unwrap();
            assert_eq!(user_version(old.connection()), 6);
            let conn = old.connection();
            let p = old.insert_project("auto_scrum", "p", "/p").unwrap();
            let t1 = old_insert_ticket(conn, p.id, "NS-1", false, "um");
            let t2 = old_insert_ticket(conn, p.id, "abcd1234", true, "dois");
            // A gap in the ids: they must be kept, not renumbered.
            let gone = v6_insert_agent(conn, t1, "po", "s-0", "stopped", Some("NS-1"));
            v6_insert_agent(conn, t1, "po", "s-1", "active", Some("NS-1"));
            conn.execute("DELETE FROM agents WHERE id = ?1", [gone])
                .unwrap();
            v6_insert_agent(conn, t1, "development", "s-2", "stopped", None);
            v6_insert_agent(conn, t2, "review", "s-3", "archived", Some("abcd1234"));
            v6_insert_agent(conn, t2, "tech_leader", "s-4", "future", None);
            (p.id, raw_agents(conn), old_tickets(conn, p.id))
        };
        assert_eq!(before.len(), 4);
        assert_eq!(before[0].0, 2, "the first id after the gap");
        let db = Db::open(&path).unwrap();
        let conn = db.connection();
        assert_eq!(user_version(conn), 7);
        assert_eq!(
            raw_agents(conn),
            before,
            "every agent keeps its id, its fields, its state and its key at the opening"
        );
        let epic_ids: Vec<Option<i64>> = conn
            .prepare("SELECT epic_id FROM agents ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            epic_ids,
            [None, None, None, None],
            "every owner is a ticket"
        );
        assert_eq!(old_tickets(conn, project), tickets);
        let ticket_epics: Vec<Option<i64>> = conn
            .prepare("SELECT epic_id FROM tickets ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(ticket_epics, [None, None], "no ticket is in an epic");
        assert_eq!(checks(conn), (0, "ok".to_string()));
        // The UNIQUE of the session id still holds after the rebuild.
        assert!(raw_agent(conn, tickets[0].0, "po", "s-1").is_err());
        assert!(raw_agent(conn, tickets[0].0, "po", "s-9").is_ok());
        // And the cascade from the ticket.
        conn.execute("DELETE FROM tickets WHERE id = ?1", [tickets[1].0])
            .unwrap();
        assert_eq!(agent_sessions(conn), ["s-1", "s-2", "s-9"]);
    }

    /// Inserts an epic row by hand (the schema tests check the constraints, not the queries).
    fn raw_epic(conn: &Connection, project_id: i64, key: &str) -> rusqlite::Result<i64> {
        conn.query_row(
            "INSERT INTO epics (project_id, key, is_provisional, title, created_at) \
             VALUES (?1, ?2, 0, 'e', 'now') RETURNING id",
            params![project_id, key],
            |row| row.get(0),
        )
    }

    /// Inserts an agent with both owners given by hand (either may be NULL).
    fn raw_owned_agent(
        conn: &Connection,
        ticket_id: Option<i64>,
        epic_id: Option<i64>,
        stage: &str,
        session_id: &str,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO agents (ticket_id, epic_id, stage, session_id, name, opened_at, state) \
             VALUES (?1, ?2, ?3, ?4, 'n', 'now', 'active')",
            params![ticket_id, epic_id, stage, session_id],
        )
    }

    #[test]
    fn an_agent_has_exactly_one_owner() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let conn = db.connection();
        let epic = raw_epic(conn, project, "EPIC-1").unwrap();
        assert!(raw_owned_agent(conn, None, None, "po", "none").is_err());
        assert!(raw_owned_agent(conn, Some(ticket), Some(epic), "po", "both").is_err());
        assert!(raw_owned_agent(conn, Some(ticket), None, "po", "ticket").is_ok());
        assert!(raw_owned_agent(conn, None, Some(epic), "po", "epic").is_ok());
        assert!(raw_owned_agent(conn, None, Some(999), "po", "missing").is_err());
        assert_eq!(agent_sessions(conn), ["ticket", "epic"]);
    }

    #[test]
    fn an_agent_of_an_epic_is_only_of_po_or_tech_leader() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let conn = db.connection();
        let epic = raw_epic(conn, project, "EPIC-1").unwrap();
        for stage in ["development", "review"] {
            assert!(
                raw_owned_agent(conn, None, Some(epic), stage, stage).is_err(),
                "{stage} refused in an epic"
            );
            assert!(
                raw_owned_agent(conn, Some(ticket), None, stage, &format!("t-{stage}")).is_ok(),
                "{stage} accepted in a ticket"
            );
        }
        for stage in ["po", "tech_leader"] {
            assert!(raw_owned_agent(conn, None, Some(epic), stage, stage).is_ok());
        }
        assert!(raw_owned_agent(conn, None, Some(epic), "nope", "nope").is_err());
        assert_eq!(
            agent_sessions(conn),
            ["t-development", "t-review", "po", "tech_leader"]
        );
    }

    #[test]
    fn the_stages_of_an_epic_agent_are_the_ones_of_the_check() {
        // stages::EPIC_STAGES is the source of the menu and of agents::open; the CHECK of the
        // migration must say the same.
        let keys: Vec<String> = crate::stages::EPIC_STAGES
            .iter()
            .map(|stage| format!("'{}'", stage.key()))
            .collect();
        let check = format!("CHECK (epic_id IS NULL OR stage IN ({}))", keys.join(", "));
        assert!(MIGRATION_7.contains(&check), "{check}");
    }

    #[test]
    fn deleting_a_project_deletes_its_epics_tickets_and_the_agents_of_both_and_no_other() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let conn = db.connection();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap();
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap();
        let ea = raw_epic(conn, a.id, "EA").unwrap();
        let eb = raw_epic(conn, b.id, "EB").unwrap();
        // A ticket in the epic, and one without epic, in each project.
        let in_a: i64 = conn
            .query_row(
                "INSERT INTO tickets (project_id, key, is_provisional, title, created_at, \
                 epic_id) VALUES (?1, 'A-1', 0, 't', 'now', ?2) RETURNING id",
                params![a.id, ea],
                |row| row.get(0),
            )
            .unwrap();
        let out_a = raw_ticket(conn, a.id, "A-2", 0, "t");
        assert!(out_a.is_ok());
        let in_b: i64 = conn
            .query_row(
                "INSERT INTO tickets (project_id, key, is_provisional, title, created_at, \
                 epic_id) VALUES (?1, 'B-1', 0, 't', 'now', ?2) RETURNING id",
                params![b.id, eb],
                |row| row.get(0),
            )
            .unwrap();
        raw_owned_agent(conn, Some(in_a), None, "po", "a-ticket").unwrap();
        raw_owned_agent(conn, None, Some(ea), "po", "a-epic").unwrap();
        raw_owned_agent(conn, Some(in_b), None, "po", "b-ticket").unwrap();
        raw_owned_agent(conn, None, Some(eb), "tech_leader", "b-epic").unwrap();
        db.delete_project(a.id).unwrap();
        assert_eq!(ticket_keys(conn), [(b.id, "B-1".to_string())]);
        assert_eq!(agent_sessions(conn), ["b-ticket", "b-epic"]);
        let epics: Vec<String> = conn
            .prepare("SELECT key FROM epics ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(epics, ["EB"]);
        assert_eq!(checks(conn), (0, "ok".to_string()));
    }

    /// The error of a statement that the triggers of the migration 7 must refuse.
    fn trigger_error(result: rusqlite::Result<usize>) -> rusqlite::Error {
        result.expect_err("refused by a trigger")
    }

    #[test]
    fn the_key_is_unique_across_the_tickets_and_the_epics_of_a_project_both_ways() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let conn = db.connection();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap().id;
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap().id;
        raw_ticket(conn, a, "T-1", 0, "t").unwrap();
        raw_epic(conn, a, "E-1").unwrap();
        raw_ticket(conn, a, "T-2", 0, "t").unwrap();
        raw_epic(conn, a, "E-2").unwrap();
        // INSERT: an epic with the key of a ticket, and a ticket with the key of an epic.
        let err = raw_epic(conn, a, "T-1").expect_err("an epic with the key of a ticket");
        assert!(is_trigger_abort(&err, KEY_OF_A_TICKET), "{err:?}");
        assert!(!is_trigger_abort(&err, KEY_OF_AN_EPIC), "{err:?}");
        let err = trigger_error(raw_ticket(conn, a, "E-1", 0, "t"));
        assert!(is_trigger_abort(&err, KEY_OF_AN_EPIC), "{err:?}");
        assert!(!is_unique_violation(&err), "a trigger, not the UNIQUE");
        // UPDATE of the key, both ways.
        let err = trigger_error(conn.execute("UPDATE epics SET key = 'T-2' WHERE key = 'E-2'", []));
        assert!(is_trigger_abort(&err, KEY_OF_A_TICKET), "{err:?}");
        let err =
            trigger_error(conn.execute("UPDATE tickets SET key = 'E-2' WHERE key = 'T-2'", []));
        assert!(is_trigger_abort(&err, KEY_OF_AN_EPIC), "{err:?}");
        // Another update of the row (not of the key) is not refused.
        conn.execute("UPDATE tickets SET title = 'x' WHERE key = 'T-2'", [])
            .unwrap();
        conn.execute("UPDATE epics SET title = 'x' WHERE key = 'E-2'", [])
            .unwrap();
        // The same key in another project is accepted, both ways.
        raw_epic(conn, b, "T-1").unwrap();
        raw_ticket(conn, b, "E-1", 0, "t").unwrap();
        // The UNIQUE of each table is still the UNIQUE (not a trigger).
        let err = raw_epic(conn, a, "E-1").expect_err("the same epic key");
        assert!(is_unique_violation(&err), "{err:?}");
        assert!(!is_trigger_abort(&err, KEY_OF_A_TICKET), "{err:?}");
    }

    #[test]
    fn a_ticket_only_goes_to_an_epic_of_its_project() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let conn = db.connection();
        let a = db.insert_project("auto_scrum", "a", "/a").unwrap().id;
        let b = db.insert_project("auto_scrum", "b", "/b").unwrap().id;
        let ea = raw_epic(conn, a, "EA").unwrap();
        let eb = raw_epic(conn, b, "EB").unwrap();
        let insert = |epic: i64, key: &str| {
            conn.execute(
                "INSERT INTO tickets (project_id, key, is_provisional, title, created_at, \
                 epic_id) VALUES (?1, ?2, 0, 't', 'now', ?3)",
                params![a, key, epic],
            )
        };
        let err = trigger_error(insert(eb, "A-1"));
        assert!(is_trigger_abort(&err, EPIC_OF_ANOTHER_PROJECT), "{err:?}");
        insert(ea, "A-1").unwrap();
        let err =
            trigger_error(conn.execute("UPDATE tickets SET epic_id = ?1 WHERE key = 'A-1'", [eb]));
        assert!(is_trigger_abort(&err, EPIC_OF_ANOTHER_PROJECT), "{err:?}");
        // Out of the epic, and back into one of its project.
        conn.execute("UPDATE tickets SET epic_id = NULL WHERE key = 'A-1'", [])
            .unwrap();
        conn.execute("UPDATE tickets SET epic_id = ?1 WHERE key = 'A-1'", [ea])
            .unwrap();
        // A missing epic: the trigger refuses it first (the foreign key would too).
        assert!(insert(999, "A-2").is_err());
    }

    #[test]
    fn is_trigger_abort_needs_the_marker_of_the_nick() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let conn = db.connection();
        conn.execute_batch(
            "CREATE TRIGGER other BEFORE INSERT ON settings \
             BEGIN SELECT RAISE(ABORT, 'someone else'); END;",
        )
        .unwrap();
        let err =
            trigger_error(conn.execute("INSERT INTO settings (key, value) VALUES ('k', 'v')", []));
        for marker in [KEY_OF_A_TICKET, KEY_OF_AN_EPIC, EPIC_OF_ANOTHER_PROJECT] {
            assert!(!is_trigger_abort(&err, marker), "{marker}");
        }
        assert!(!is_trigger_abort(
            &rusqlite::Error::QueryReturnedNoRows,
            KEY_OF_A_TICKET
        ));
    }

    // ---- the epics and the owner of an agent (NS-40) ----

    /// A project with two epics (one hidden) and a ticket in the first one and one without epic.
    struct Epics {
        db: Db,
        project: i64,
        e1: Epic,
        e2: Epic,
        in_e1: Ticket,
        loose: Ticket,
    }

    fn with_epics(tmp: &tempfile::TempDir) -> Epics {
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let project = db.insert_project("auto_scrum", "p", "/p").unwrap().id;
        let e1 = db.insert_epic(project, "EPIC-1", false, "Um").unwrap();
        let e2 = db.insert_epic(project, "abcd1234", true, "Dois").unwrap();
        let in_e1 = db
            .insert_ticket(project, "NS-1", false, "t", Some(e1.id))
            .unwrap();
        let loose = db.insert_ticket(project, "NS-2", false, "t", None).unwrap();
        Epics {
            db,
            project,
            e1,
            e2,
            in_e1,
            loose,
        }
    }

    #[test]
    fn insert_epic_returns_the_stored_row_and_refuses_a_repeated_key() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        assert_eq!(f.e1.project_id, f.project);
        assert_eq!(f.e1.key, "EPIC-1");
        assert!(!f.e1.is_provisional);
        assert_eq!(f.e1.title, "Um");
        assert_rfc3339_utc(&f.e1.created_at);
        assert_eq!(f.e1.hidden_at, None);
        assert!(f.e2.is_provisional);
        let err =
            f.db.insert_epic(f.project, "EPIC-1", false, "x")
                .unwrap_err();
        assert!(is_unique_violation(&err), "{err:?}");
        let err = f.db.insert_epic(f.project, "NS-1", false, "x").unwrap_err();
        assert!(is_trigger_abort(&err, KEY_OF_A_TICKET), "{err:?}");
        assert!(
            f.db.insert_epic(f.project, "E", false, "  ").is_err(),
            "blank title"
        );
        assert!(
            f.db.insert_epic(999, "E", false, "x").is_err(),
            "missing project"
        );
    }

    #[test]
    fn list_epics_is_by_creation_of_that_project_and_hides_the_hidden() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        let other = f.db.insert_project("auto_scrum", "q", "/q").unwrap().id;
        f.db.insert_epic(other, "EPIC-1", false, "outro").unwrap();
        assert_eq!(
            f.db.list_epics(f.project, false).unwrap(),
            [f.e1.clone(), f.e2.clone()]
        );
        assert_eq!(f.db.hide_epic(f.e1.id).unwrap(), EpicHide::Done);
        let keys = |include_hidden| -> Vec<String> {
            f.db.list_epics(f.project, include_hidden)
                .unwrap()
                .into_iter()
                .map(|e| e.key)
                .collect()
        };
        assert_eq!(keys(false), ["abcd1234"]);
        assert_eq!(keys(true), ["EPIC-1", "abcd1234"]);
        assert_eq!(
            f.db.count_epics(f.project).unwrap(),
            2,
            "the hidden ones too"
        );
        assert_eq!(f.db.count_epics(other).unwrap(), 1);
    }

    #[test]
    fn get_epic_reads_one_epic_by_id_hidden_or_not() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        assert_eq!(f.db.get_epic(f.e1.id).unwrap(), Some(f.e1.clone()));
        f.db.hide_epic(f.e1.id).unwrap();
        assert!(f.db.get_epic(f.e1.id).unwrap().unwrap().hidden_at.is_some());
        assert_eq!(f.db.get_epic(999).unwrap(), None);
        f.db.delete_project(f.project).unwrap();
        assert_eq!(
            f.db.get_epic(f.e1.id).unwrap(),
            None,
            "gone with its project"
        );
    }

    #[test]
    fn update_epic_title_and_key_change_only_them_and_say_whether_the_row_exists() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        assert!(f.db.update_epic_title(f.e1.id, "Novo").unwrap());
        assert!(f.db.update_epic_key(f.e2.id, "abcd1234", "EPIC-2").unwrap());
        let e2 = f.db.get_epic(f.e2.id).unwrap().unwrap();
        assert_eq!(e2.key, "EPIC-2");
        assert!(!e2.is_provisional, "a typed key is never provisional");
        assert_eq!(e2.title, "Dois");
        assert_eq!(f.db.get_epic(f.e1.id).unwrap().unwrap().title, "Novo");
        assert!(
            !f.db.update_epic_key(f.e2.id, "abcd1234", "X").unwrap(),
            "moved"
        );
        let err = f.db.update_epic_key(f.e2.id, "EPIC-2", "NS-2").unwrap_err();
        assert!(is_trigger_abort(&err, KEY_OF_A_TICKET), "{err:?}");
        let err =
            f.db.update_epic_key(f.e2.id, "EPIC-2", "EPIC-1")
                .unwrap_err();
        assert!(is_unique_violation(&err), "{err:?}");
        f.db.delete_project(f.project).unwrap();
        assert!(!f.db.update_epic_title(f.e1.id, "x").unwrap(), "gone");
        assert!(
            !f.db.update_epic_key(f.e1.id, "EPIC-1", "Y").unwrap(),
            "gone"
        );
    }

    #[test]
    fn find_key_looks_in_the_tickets_and_in_the_epics() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        let find = |key: &str| f.db.find_key(f.project, key).unwrap();
        assert_eq!(find("NS-1"), Some((KeyKind::Ticket, false)));
        assert_eq!(find("EPIC-1"), Some((KeyKind::Epic, false)));
        assert_eq!(find("nope"), None);
        assert_eq!(find("epic-1"), None, "case sensitive");
        f.db.set_ticket_hidden(f.loose.id, true).unwrap();
        f.db.hide_epic(f.e2.id).unwrap();
        assert_eq!(find("NS-2"), Some((KeyKind::Ticket, true)));
        assert_eq!(find("abcd1234"), Some((KeyKind::Epic, true)));
        let other = f.db.insert_project("auto_scrum", "q", "/q").unwrap().id;
        assert_eq!(f.db.find_key(other, "EPIC-1").unwrap(), None, "per project");
    }

    #[test]
    fn a_ticket_keeps_its_epic_and_set_ticket_epic_moves_it() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        assert_eq!(f.in_e1.epic_id, Some(f.e1.id));
        assert_eq!(f.loose.epic_id, None);
        assert!(f.db.set_ticket_epic(f.loose.id, Some(f.e2.id)).unwrap());
        assert_eq!(
            f.db.get_ticket(f.loose.id).unwrap().unwrap().epic_id,
            Some(f.e2.id)
        );
        assert!(f.db.set_ticket_epic(f.in_e1.id, None).unwrap());
        assert_eq!(f.db.get_ticket(f.in_e1.id).unwrap().unwrap().epic_id, None);
        // An epic of another project: not in the SQL (false), and the trigger behind it.
        let other = f.db.insert_project("auto_scrum", "q", "/q").unwrap().id;
        let foreign = f.db.insert_epic(other, "X", false, "x").unwrap();
        assert!(!f.db.set_ticket_epic(f.in_e1.id, Some(foreign.id)).unwrap());
        assert_eq!(f.db.get_ticket(f.in_e1.id).unwrap().unwrap().epic_id, None);
        assert!(
            !f.db.set_ticket_epic(f.in_e1.id, Some(999)).unwrap(),
            "missing epic"
        );
        assert!(!f.db.set_ticket_epic(999, None).unwrap(), "missing ticket");
        // A ticket born in an epic of another project is refused by the trigger.
        let err =
            f.db.insert_ticket(f.project, "NS-9", false, "t", Some(foreign.id))
                .unwrap_err();
        assert!(is_trigger_abort(&err, EPIC_OF_ANOTHER_PROJECT), "{err:?}");
    }

    #[test]
    fn insert_agent_stores_the_owner_and_list_project_agents_has_the_epics_too() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        let t =
            f.db.insert_agent(Owner::Ticket(f.in_e1.id), "po", "s-1", "n", "NS-1")
                .unwrap();
        let e =
            f.db.insert_agent(Owner::Epic(f.e1.id), "tech_leader", "s-2", "n", "EPIC-1")
                .unwrap();
        assert_eq!(t.owner, Owner::Ticket(f.in_e1.id));
        assert_eq!(e.owner, Owner::Epic(f.e1.id));
        assert_eq!(e.opened_key.as_deref(), Some("EPIC-1"));
        assert!(
            f.db.insert_agent(Owner::Epic(f.e1.id), "development", "s-3", "n", "E")
                .is_err(),
            "the CHECK of the stage of an epic agent"
        );
        assert!(
            f.db.insert_agent(Owner::Epic(999), "po", "s-4", "n", "E")
                .is_err()
        );
        let other = f.db.insert_project("auto_scrum", "q", "/q").unwrap().id;
        let oe = f.db.insert_epic(other, "Q", false, "q").unwrap();
        f.db.insert_agent(Owner::Epic(oe.id), "po", "s-5", "n", "Q")
            .unwrap();
        assert_eq!(
            f.db.list_project_agents(f.project).unwrap(),
            [t.clone(), e.clone()]
        );
        assert_eq!(f.db.get_agent(e.id).unwrap(), Some(e.clone()));
        assert_eq!(f.db.list_ticket_agents(f.in_e1.id).unwrap(), [t]);
    }

    #[test]
    fn count_active_agents_counts_by_owner_and_by_the_branch_of_an_epic() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        let db = &f.db;
        let epic = db
            .insert_agent(Owner::Epic(f.e1.id), "po", "s-1", "n", "E")
            .unwrap();
        db.insert_agent(Owner::Ticket(f.in_e1.id), "po", "s-2", "n", "T")
            .unwrap();
        let stopped = db
            .insert_agent(Owner::Ticket(f.in_e1.id), "po", "s-3", "n", "T")
            .unwrap();
        db.stop_agent(stopped.id).unwrap();
        db.insert_agent(Owner::Ticket(f.loose.id), "po", "s-4", "n", "T")
            .unwrap();
        db.insert_agent(Owner::Epic(f.e2.id), "po", "s-5", "n", "T")
            .unwrap();
        assert_eq!(db.count_active_agents(Owner::Epic(f.e1.id)).unwrap(), 1);
        assert_eq!(
            db.count_active_agents(Owner::Ticket(f.in_e1.id)).unwrap(),
            1
        );
        assert_eq!(
            db.count_active_agents(Owner::Ticket(f.loose.id)).unwrap(),
            1
        );
        assert_eq!(db.count_active_agents_of_epic_branch(f.e1.id).unwrap(), 2);
        assert_eq!(db.count_active_agents_of_epic_branch(f.e2.id).unwrap(), 1);
        db.stop_agent(epic.id).unwrap();
        assert_eq!(db.count_active_agents(Owner::Epic(f.e1.id)).unwrap(), 0);
        assert_eq!(db.count_active_agents_of_epic_branch(f.e1.id).unwrap(), 1);
    }

    #[test]
    fn hide_epic_is_refused_with_an_active_agent_in_the_epic() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        let a =
            f.db.insert_agent(Owner::Epic(f.e1.id), "po", "s-1", "n", "E")
                .unwrap();
        assert_eq!(f.db.hide_epic(f.e1.id).unwrap(), EpicHide::Active(1));
        assert_eq!(f.db.get_epic(f.e1.id).unwrap().unwrap().hidden_at, None);
        f.db.stop_agent(a.id).unwrap();
        assert_eq!(f.db.hide_epic(f.e1.id).unwrap(), EpicHide::Done);
    }

    #[test]
    fn hide_epic_is_refused_with_active_agents_in_a_ticket_of_the_epic() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        for session in ["s-1", "s-2"] {
            f.db.insert_agent(Owner::Ticket(f.in_e1.id), "po", session, "n", "T")
                .unwrap();
        }
        // An active agent of a ticket out of the epic does not count.
        f.db.insert_agent(Owner::Ticket(f.loose.id), "po", "s-3", "n", "T")
            .unwrap();
        assert_eq!(f.db.hide_epic(f.e1.id).unwrap(), EpicHide::Active(2));
        assert_eq!(f.db.get_epic(f.e1.id).unwrap().unwrap().hidden_at, None);
        assert_eq!(f.db.hide_epic(f.e2.id).unwrap(), EpicHide::Done);
    }

    #[test]
    fn hide_epic_without_active_agents_keeps_the_tickets_and_restore_brings_it_back() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        let hidden_ticket =
            f.db.insert_ticket(f.project, "NS-3", false, "t", Some(f.e1.id))
                .unwrap();
        f.db.set_ticket_hidden(hidden_ticket.id, true).unwrap();
        let before: Vec<Ticket> = f.db.list_tickets(f.project, true).unwrap();
        let stopped =
            f.db.insert_agent(Owner::Epic(f.e1.id), "po", "s-1", "n", "E")
                .unwrap();
        f.db.stop_agent(stopped.id).unwrap();
        assert_eq!(f.db.hide_epic(f.e1.id).unwrap(), EpicHide::Done);
        let hidden = f.db.get_epic(f.e1.id).unwrap().unwrap();
        assert_rfc3339_utc(hidden.hidden_at.as_deref().unwrap());
        assert_eq!(
            f.db.list_tickets(f.project, true).unwrap(),
            before,
            "the hidden_at of the tickets does not change"
        );
        assert!(f.db.restore_epic(f.e1.id).unwrap());
        assert_eq!(f.db.get_epic(f.e1.id).unwrap().unwrap().hidden_at, None);
        assert_eq!(f.db.list_tickets(f.project, true).unwrap(), before);
        f.db.delete_project(f.project).unwrap();
        assert_eq!(f.db.hide_epic(f.e1.id).unwrap(), EpicHide::Gone);
        assert!(!f.db.restore_epic(f.e1.id).unwrap(), "gone");
    }

    /// Review of the NS-40, P3.3: when both tries lose the race and no active agent is left, the
    /// result says so (`Changed`), never "1 active". A test trigger plays the other nick: it makes
    /// every UPDATE of the epic change nothing.
    #[test]
    fn hide_epic_that_keeps_losing_the_race_with_no_active_agent_is_changed() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        f.db.connection()
            .execute_batch(
                "CREATE TRIGGER other_nick BEFORE UPDATE ON epics \
                 BEGIN SELECT RAISE(IGNORE); END;",
            )
            .unwrap();
        assert_eq!(f.db.hide_epic(f.e1.id).unwrap(), EpicHide::Changed);
        assert_eq!(f.db.get_epic(f.e1.id).unwrap().unwrap().hidden_at, None);
    }

    /// Review of the NS-40, P3.4: a ticket only goes into a visible epic; out of a hidden one is
    /// allowed.
    #[test]
    fn set_ticket_epic_refuses_a_hidden_epic_and_lets_a_ticket_out_of_one() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        f.db.hide_epic(f.e1.id).unwrap();
        assert!(!f.db.set_ticket_epic(f.loose.id, Some(f.e1.id)).unwrap());
        assert_eq!(f.db.get_ticket(f.loose.id).unwrap().unwrap().epic_id, None);
        assert!(
            f.db.set_ticket_epic(f.in_e1.id, None).unwrap(),
            "out of the hidden epic"
        );
        assert!(
            f.db.set_ticket_epic(f.loose.id, Some(f.e2.id)).unwrap(),
            "a visible one"
        );
    }
}
