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
use crate::tickets::TicketStage;

/// How long a statement waits for another nick that is writing to the same database before
/// giving up with "database is locked" (the only transient error of SQLite).
const BUSY_TIMEOUT_MS: u32 = 5000;

/// The key of the reserved shortcut in `settings` (NS-34); its value is `Shortcut::to_db`.
pub const SETTING_RESERVED_SHORTCUT: &str = "reserved_shortcut";

/// The keys of the filters of the tree remembered between openings (NS-54, D6): `"1"` when on;
/// no row is the default (off), as the migration 4 says. The `v` is not remembered.
pub const SETTING_TREE_SHOW_HIDDEN: &str = "tree_show_hidden";
pub const SETTING_TREE_HIDE_DONE: &str = "tree_hide_done";

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
    /// When it was done (migration 10, NS-54); None while open. A done epic has every ticket
    /// done: `Db::complete_epic`, and the writers of a ticket open it again.
    pub done_at: Option<String>,
}

/// The columns of an epic, in the order of `epic_from_row`.
const EPIC_COLUMNS: &str =
    "id, project_id, key, is_provisional, title, created_at, hidden_at, done_at";

fn epic_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Epic> {
    Ok(Epic {
        id: row.get(0)?,
        project_id: row.get(1)?,
        key: row.get(2)?,
        is_provisional: row.get(3)?,
        title: row.get(4)?,
        created_at: row.get(5)?,
        hidden_at: row.get(6)?,
        done_at: row.get(7)?,
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

/// What `Db::complete_epic` did (NS-54, the mold of `EpicHide`): done, or why nothing was written
/// — the epic is gone, another nick completed it before, it has no ticket, or it has `pending`
/// tickets not done (the hidden ones included: "every ticket"), `hidden` of them hidden (the tree
/// does not show those without the `f`, so the message says so).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpicCompletion {
    Done,
    Gone,
    AlreadyDone,
    Empty,
    Pending { pending: i64, hidden: i64 },
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

/// A change of the stage of a ticket, as stored (migrations 5 and 9): the raw keys of the stages
/// and of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageChangeRow {
    pub from_stage: String,
    pub to_stage: String,
    /// Who changed it: `operator` or `agent_opening` (`tickets::StageSource`, NS-45).
    pub source: String,
    /// Why the ticket was rejected (migration 11, NS-61): only in a change to `rejected`.
    pub reason: Option<String>,
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

/// How `Db::resume_agent` left the session of the resume (NS-50).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionSwap {
    /// The latest session is the effective one: nothing to change.
    Same,
    /// The latest session is now the session of the resume.
    Swapped,
    /// Another agent already resumes by the latest session (the UNIQUE of the migration 8): the
    /// session was kept.
    Taken,
}

/// An agent of a ticket or of an epic, as stored (migrations 3, 6, 7 and 8). `stage` and `state` are
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
    /// The session the conversation went on in, found at a resume (migration 8, NS-50); None
    /// while it is `session_id`.
    pub resume_session_id: Option<String>,
}

impl Agent {
    /// The session a resume uses (NS-50): `resume_session_id`, or `session_id` without one. The
    /// lock is always the one of `session_id`.
    pub fn effective_session(&self) -> &str {
        self.resume_session_id
            .as_deref()
            .unwrap_or(&self.session_id)
    }
}

/// The columns of an agent, in the order of `agent_from_row`.
const AGENT_COLUMNS: &str = "id, ticket_id, epic_id, stage, session_id, name, opened_at, state, \
                             opened_key, resume_session_id";

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
        resume_session_id: row.get(9)?,
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

/// The keys of the closed stages (`TicketStage::CLOSED`, NS-61, D2), for the `NOT IN (?, ?)` of the
/// queries of the epic: a rejected ticket counts as a done one. Destructured, so a new closed
/// stage breaks the build here, and not the rule in silence.
fn closed_keys() -> [&'static str; 2] {
    TicketStage::CLOSED.map(TicketStage::key)
}

/// Opens the done epic `epic_id` again when it has a ticket not closed (NS-54, D3; NS-61: not done
/// nor rejected), and gives its key; None without an epic, with an open one, or with every ticket
/// closed (the rule holds). Called only by the writers of a ticket, after the write and in its
/// transaction, which already holds the database for writing: no other nick completes the epic in
/// between.
fn reopen_if_pending(conn: &Connection, epic_id: Option<i64>) -> rusqlite::Result<Option<String>> {
    let Some(epic_id) = epic_id else {
        return Ok(None);
    };
    let [done, rejected] = closed_keys();
    conn.query_row(
        "UPDATE epics SET done_at = NULL \
         WHERE id = ?1 AND done_at IS NOT NULL \
         AND EXISTS (SELECT 1 FROM tickets WHERE epic_id = epics.id AND stage NOT IN (?2, ?3)) \
         RETURNING key",
        params![epic_id, done, rejected],
        |row| row.get(0),
    )
    .optional()
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
        let pending = &migrations[current as usize..];
        if pending.is_empty() {
            return Ok(Db { conn });
        }
        // The migrations run with the foreign keys off (NS-45, plan D2; the 12 steps of "Making
        // Other Kinds Of Table Schema Changes" of SQLite): rebuilding a parent table drops it, and
        // with them on the DROP deletes the rows of its children (ON DELETE CASCADE). The pragma
        // is ignored inside a transaction, so it is set around the loop, and back on also when a
        // migration fails. The `foreign_key_check` of each one takes the place of the check.
        conn.pragma_update(None, "foreign_keys", false)
            .map_err(open_error)?;
        let migrated = Db::migrate(&mut conn, path, current, pending);
        let on = conn.pragma_update(None, "foreign_keys", true);
        migrated?;
        on.map_err(open_error)?;
        Ok(Db { conn })
    }

    /// Runs the `pending` migrations, from the version `current` on, each with its version in one
    /// transaction (all or nothing), refusing one that leaves a broken reference.
    fn migrate(
        conn: &mut Connection,
        path: &Path,
        current: u32,
        pending: &[&str],
    ) -> Result<(), OpenError> {
        for (version, sql) in (current + 1..).zip(pending) {
            let migrate_error = |message: String| OpenError::Migrate {
                path: path.to_path_buf(),
                version,
                message,
            };
            let sql_error = |err: rusqlite::Error| migrate_error(err.to_string());
            let tx = conn.transaction().map_err(sql_error)?;
            tx.execute_batch(sql).map_err(sql_error)?;
            if let Some((table, parent)) = tx
                .query_row("PRAGMA foreign_key_check", [], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(2)?))
                })
                .optional()
                .map_err(sql_error)?
            {
                return Err(migrate_error(format!(
                    "referência quebrada na tabela {table} (para {parent})"
                )));
            }
            tx.pragma_update(None, "user_version", version)
                .map_err(sql_error)?;
            tx.commit().map_err(sql_error)?;
        }
        Ok(())
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
    /// trigger (`is_trigger_abort`). A done epic is opened again (`insert_ticket_reopening`, the
    /// same transaction); this one does not say so.
    pub fn insert_ticket(
        &self,
        project_id: i64,
        key: &str,
        is_provisional: bool,
        title: &str,
        epic_id: Option<i64>,
    ) -> rusqlite::Result<Ticket> {
        self.insert_ticket_reopening(project_id, key, is_provisional, title, epic_id)
            .map(|(ticket, _)| ticket)
    }

    /// `insert_ticket`, with the key of the epic it opened again (NS-54, D3): a new ticket is
    /// born `open`, so it always opens a done epic, in the same transaction as the INSERT. The
    /// one INSERT of a ticket (with `set_ticket_epic` and `change_ticket_stage`, the only writers
    /// of a ticket that could break the rule of a done epic).
    pub fn insert_ticket_reopening(
        &self,
        project_id: i64,
        key: &str,
        is_provisional: bool,
        title: &str,
        epic_id: Option<i64>,
    ) -> rusqlite::Result<(Ticket, Option<String>)> {
        // The Db has &self, so the transaction is unchecked; it rolls back when dropped.
        let tx = self.conn.unchecked_transaction()?;
        let ticket = tx.query_row(
            &format!(
                "INSERT INTO tickets (project_id, key, is_provisional, title, created_at, \
                 epic_id) \
                 VALUES (?1, ?2, ?3, ?4, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'), ?5) \
                 RETURNING {TICKET_COLUMNS}"
            ),
            params![project_id, key, is_provisional, title, epic_id],
            ticket_from_row,
        )?;
        let reopened = reopen_if_pending(&tx, ticket.epic_id)?;
        tx.commit()?;
        Ok((ticket, reopened))
    }

    /// Moves the ticket into the epic `epic_id`, or out of any epic (None): only the database
    /// changes, no file (NS-40). Only into a visible epic of the project of the ticket (the project
    /// here in the SQL, and by a trigger behind it; visible, so a ticket, maybe with an active
    /// agent, never goes into a branch another nick just hid: review of the NS-40, P3.4). Out of
    /// an epic is always allowed. `false` when the ticket is gone, or the epic is gone, hidden or
    /// of another project (nothing changed). The only writer: `epics::move_ticket`. With the key of
    /// the epic it opened again (NS-54, D3): a ticket not done into a done epic opens it, in the
    /// same transaction; a done one keeps it done, and so does the last ticket taken out of it.
    pub fn set_ticket_epic(
        &self,
        ticket_id: i64,
        epic_id: Option<i64>,
    ) -> rusqlite::Result<(bool, Option<String>)> {
        // The Db has &self, so the transaction is unchecked; it rolls back when dropped.
        let tx = self.conn.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE tickets SET epic_id = ?2 WHERE id = ?1 AND (?2 IS NULL OR EXISTS \
             (SELECT 1 FROM epics WHERE id = ?2 AND project_id = tickets.project_id \
             AND hidden_at IS NULL))",
            params![ticket_id, epic_id],
        )?;
        if changed == 0 {
            return Ok((false, None));
        }
        let reopened = reopen_if_pending(&tx, epic_id)?;
        tx.commit()?;
        Ok((true, reopened))
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

    /// Completes the epic (NS-54, D2): one UPDATE conditioned on the epic being open, having
    /// tickets and every one of them closed (done or rejected, NS-61), the hidden ones included
    /// (the rule: a done epic has every ticket closed). A hidden epic, or one with an active agent,
    /// may be completed. With 0 rows, the reason is read in the same transaction, which the UPDATE
    /// already holds for writing (the mold of `hide_epic`, with no second try: nothing changes in
    /// between).
    pub fn complete_epic(&self, id: i64) -> rusqlite::Result<EpicCompletion> {
        let [done, rejected] = closed_keys();
        // The Db has &self, so the transaction is unchecked; it rolls back when dropped.
        let tx = self.conn.unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE epics SET done_at = strftime('%Y-%m-%dT%H:%M:%SZ', 'now') \
             WHERE id = ?1 AND done_at IS NULL \
             AND EXISTS (SELECT 1 FROM tickets WHERE epic_id = ?1) \
             AND NOT EXISTS (SELECT 1 FROM tickets WHERE epic_id = ?1 \
             AND stage NOT IN (?2, ?3))",
            params![id, done, rejected],
        )?;
        if changed > 0 {
            tx.commit()?;
            return Ok(EpicCompletion::Done);
        }
        let row: Option<(bool, i64, i64, i64)> = tx
            .query_row(
                "SELECT done_at IS NOT NULL, \
                 (SELECT count(*) FROM tickets WHERE epic_id = ?1), \
                 (SELECT count(*) FROM tickets WHERE epic_id = ?1 AND stage NOT IN (?2, ?3)), \
                 (SELECT count(*) FROM tickets WHERE epic_id = ?1 AND stage NOT IN (?2, ?3) \
                  AND hidden_at IS NOT NULL) \
                 FROM epics WHERE id = ?1",
                params![id, done, rejected],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()?;
        Ok(match row {
            None => EpicCompletion::Gone,
            Some((true, ..)) => EpicCompletion::AlreadyDone,
            Some((false, 0, ..)) => EpicCompletion::Empty,
            Some((false, _, pending, hidden)) => EpicCompletion::Pending { pending, hidden },
        })
    }

    /// Opens a done epic again (NS-54, D2). `false` when it is gone or already open (nothing
    /// changed; `epics::reopen` reads it again to say which).
    pub fn reopen_epic(&self, id: i64) -> rusqlite::Result<bool> {
        let changed = self.conn.execute(
            "UPDATE epics SET done_at = NULL WHERE id = ?1 AND done_at IS NOT NULL",
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
        self.change_agent_state_with(id, from, to, None)
    }

    /// `change_agent_state` that, with `session`, also sets `resume_session_id` to it in the
    /// same UPDATE (NS-50). A session another agent has fails with the UNIQUE constraint.
    fn change_agent_state_with(
        &self,
        id: i64,
        from: &[AgentState],
        to: AgentState,
        session: Option<&str>,
    ) -> rusqlite::Result<AgentChange> {
        let keys: Vec<&str> = from.iter().map(|state| state.key()).collect();
        let placeholders: Vec<String> = (4..4 + keys.len()).map(|n| format!("?{n}")).collect();
        let to = to.key();
        let mut values: Vec<&dyn rusqlite::ToSql> = vec![&id, &to, &session];
        values.extend(keys.iter().map(|key| key as &dyn rusqlite::ToSql));
        let changed = self.conn.execute(
            &format!(
                "UPDATE agents SET state = ?2, \
                 resume_session_id = coalesce(?3, resume_session_id) \
                 WHERE id = ?1 AND state IN ({})",
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
    /// `active` row whose nick died (the caller holds its lock: agents.rs). `latest` is the
    /// session its conversation went on in (NS-50, `--has-conversation`): when it differs from
    /// the effective one, the same UPDATE stores it as `resume_session_id` (`Swapped`). When
    /// another agent already has it (the UNIQUE), only the state changes (`Taken`). Only the
    /// holder of the lock writes `resume_session_id`, so the effective session read first does
    /// not change before the UPDATE.
    pub fn resume_agent(
        &self,
        id: i64,
        latest: &str,
    ) -> rusqlite::Result<(AgentChange, SessionSwap)> {
        let from = [AgentState::Stopped, AgentState::Active];
        let effective: Option<String> = self
            .conn
            .query_row(
                "SELECT coalesce(resume_session_id, session_id) FROM agents WHERE id = ?1",
                params![id],
                |row| row.get(0),
            )
            .optional()?;
        if effective.is_none_or(|effective| effective == latest) {
            let change = self.change_agent_state(id, &from, AgentState::Active)?;
            return Ok((change, SessionSwap::Same));
        }
        match self.change_agent_state_with(id, &from, AgentState::Active, Some(latest)) {
            Ok(AgentChange::Done) => Ok((AgentChange::Done, SessionSwap::Swapped)),
            Ok(change) => Ok((change, SessionSwap::Same)),
            Err(err) if is_unique_violation(&err) => {
                let change = self.change_agent_state(id, &from, AgentState::Active)?;
                Ok((change, SessionSwap::Taken))
            }
            Err(err) => Err(err),
        }
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

    /// Changes the stage of the ticket from `from` to `to` and writes the row of the history with
    /// its `source` and `reason` (NS-61: the reason of a rejection, None for any other change; the
    /// CHECK of the migration 11 ties them), in one transaction: both or neither. Only when the
    /// ticket is still at `from`; otherwise nothing is written, and the result says whether the
    /// ticket is gone or was moved (by another nick). The only writer of the stage, called only by
    /// `tickets::change_stage` (the operator) and `tickets::advance_on_agent_opening` (NS-45). With
    /// the key of the epic it opened again (NS-54, D3): a ticket that leaves a closed stage for an
    /// open one (NS-61: not done nor rejected) opens its done epic, read in the SQL (the `Ticket`
    /// in memory may be old), in the same transaction.
    pub fn change_ticket_stage(
        &self,
        id: i64,
        from: &str,
        to: &str,
        source: &str,
        reason: Option<&str>,
    ) -> rusqlite::Result<(StageChange, Option<String>)> {
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
            let change = if exists {
                StageChange::Moved
            } else {
                StageChange::Gone
            };
            return Ok((change, None));
        }
        tx.execute(
            "INSERT INTO ticket_stage_changes (ticket_id, from_stage, to_stage, source, reason, \
             changed_at) VALUES (?1, ?2, ?3, ?4, ?5, strftime('%Y-%m-%dT%H:%M:%SZ', 'now'))",
            params![id, from, to, source, reason],
        )?;
        let epic: Option<i64> = tx.query_row(
            "SELECT epic_id FROM tickets WHERE id = ?1",
            params![id],
            |row| row.get(0),
        )?;
        let reopened = reopen_if_pending(&tx, epic)?;
        tx.commit()?;
        Ok((StageChange::Changed, reopened))
    }

    /// The history of the stage of the ticket, oldest first.
    pub fn list_stage_changes(&self, ticket_id: i64) -> rusqlite::Result<Vec<StageChangeRow>> {
        let mut stmt = self.conn.prepare(
            "SELECT from_stage, to_stage, source, reason, changed_at FROM ticket_stage_changes \
             WHERE ticket_id = ?1 ORDER BY id",
        )?;
        stmt.query_map(params![ticket_id], |row| {
            Ok(StageChangeRow {
                from_stage: row.get(0)?,
                to_stage: row.get(1)?,
                source: row.get(2)?,
                reason: row.get(3)?,
                changed_at: row.get(4)?,
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
        MIGRATION_8, MIGRATION_9, MIGRATION_10, MIGRATION_11, MIGRATIONS,
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
    fn a_new_database_gets_version_11_with_auto_scrum_and_no_project() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let db = Db::open(&path).unwrap();
        assert!(path.is_file());
        assert_eq!(user_version(db.connection()), 11);
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
        assert_eq!(user_version(db.connection()), 11);
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
            conn.execute_batch("CREATE TABLE future (x); PRAGMA user_version = 12;")
                .unwrap();
        }
        let before = std::fs::read(&path).unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let err = Db::open(&path).err().expect("a newer database is refused");
        assert_eq!(
            err.to_string(),
            format!(
                "Erro: o banco {} está na versão 12, mais nova que a deste nick (11). \
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

    fn foreign_keys(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .unwrap()
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .unwrap()
    }

    /// The rebuild of a parent table (the 12 steps of SQLite) in a migration: with the foreign keys
    /// on, the DROP would delete the rows of its children (ON DELETE CASCADE). NS-45, plan D2.
    #[test]
    fn a_migration_that_rebuilds_a_parent_table_keeps_its_children() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        {
            let db = Db::open_with(&path, &MIGRATIONS[..2]).unwrap();
            let project = db.insert_project("auto_scrum", "p", "/p").unwrap();
            db.connection()
                .execute(
                    "INSERT INTO tickets (project_id, key, is_provisional, title, created_at) \
                     VALUES (?1, 'NS-1', 0, 't', '2026-01-01T00:00:00Z')",
                    params![project.id],
                )
                .unwrap();
        }
        let rebuild = "\
CREATE TABLE projects_new (
    id         INTEGER PRIMARY KEY,
    module_id  INTEGER NOT NULL REFERENCES modules(id),
    name       TEXT NOT NULL,
    path       TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE (module_id, path)
);
INSERT INTO projects_new SELECT id, module_id, name, path, created_at FROM projects;
DROP TABLE projects;
ALTER TABLE projects_new RENAME TO projects;
";
        let db = Db::open_with(&path, &[MIGRATION_1, MIGRATION_2, rebuild]).unwrap();
        assert_eq!(user_version(db.connection()), 3);
        assert_eq!(count(db.connection(), "projects"), 1);
        assert_eq!(count(db.connection(), "tickets"), 1, "the ticket stays");
        assert_eq!(foreign_keys(db.connection()), 1, "on again after migrating");
    }

    /// With the foreign keys off in the migrations, a broken reference is caught by the
    /// `foreign_key_check` before the commit: the migration is undone (NS-45, plan D2).
    #[test]
    fn a_migration_that_leaves_a_broken_reference_is_refused_and_undone() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        Db::open_with(&path, &MIGRATIONS[..2]).unwrap();
        let broken = "\
CREATE TABLE half (x);
INSERT INTO tickets (project_id, key, is_provisional, title, created_at)
    VALUES (99, 'NS-1', 0, 't', '2026-01-01T00:00:00Z');
";
        let err = Db::open_with(&path, &[MIGRATION_1, MIGRATION_2, broken])
            .err()
            .expect("it fails");
        assert!(
            matches!(err, OpenError::Migrate { version: 3, .. }),
            "{err:?}"
        );
        assert_eq!(
            err.to_string(),
            format!(
                "Erro: não consegui atualizar o banco do nick {} para a versão 3: referência \
                 quebrada na tabela tickets (para projects).",
                path.display()
            )
        );
        let conn = Connection::open(&path).unwrap();
        assert_eq!(user_version(&conn), 2);
        assert_eq!(tables(&conn), ["modules", "projects", "tickets"]);
        assert_eq!(count(&conn, "tickets"), 0);
    }

    /// The foreign keys are on after migrating an old database, after opening one with no
    /// pending migration, and on the next opening after a failing migration (NS-45, plan D2).
    #[test]
    fn foreign_keys_are_on_after_migrating_and_after_a_failing_migration() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        Db::open_with(&path, &MIGRATIONS[..2]).unwrap();
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(db.connection()), MIGRATIONS.len() as u32);
        assert_eq!(foreign_keys(db.connection()), 1, "after migrating");
        drop(db);
        assert_eq!(
            foreign_keys(Db::open(&path).unwrap().connection()),
            1,
            "none pending"
        );

        let other = tmp.path().join("other.db");
        let broken = [
            MIGRATION_1,
            "CREATE TABLE half (x); INSERT INTO nope VALUES (1);",
        ];
        assert!(Db::open_with(&other, &broken).is_err());
        let db = Db::open(&other).unwrap();
        assert_eq!(foreign_keys(db.connection()), 1, "the next opening");
        let orphan = db.connection().execute(
            "INSERT INTO projects (module_id, name, path, created_at) VALUES (99, 'x', '/x', 'now')",
            [],
        );
        assert!(orphan.is_err(), "a project of a missing module is refused");
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
            11,
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
        assert_eq!(
            MIGRATION_8,
            "\
ALTER TABLE agents ADD COLUMN resume_session_id TEXT;  -- the session of the resume; NULL: session_id
CREATE UNIQUE INDEX agents_resume_session_id ON agents(resume_session_id);
"
        );
        assert_eq!(MIGRATIONS[7], MIGRATION_8);
        assert_eq!(
            MIGRATION_9,
            "\
DROP TRIGGER epics_key_not_a_ticket_insert;
DROP TRIGGER epics_key_not_a_ticket_update;
DROP TRIGGER tickets_key_not_an_epic_insert;
DROP TRIGGER tickets_key_not_an_epic_update;
DROP TRIGGER tickets_epic_of_the_project_insert;
DROP TRIGGER tickets_epic_of_the_project_update;
CREATE TABLE tickets_new (
    id             INTEGER PRIMARY KEY,
    project_id     INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key            TEXT NOT NULL,     -- the Jira ID, or the provisional short UUID
    is_provisional INTEGER NOT NULL CHECK (is_provisional IN (0, 1)),
    title          TEXT NOT NULL CHECK (trim(title) <> ''),
    created_at     TEXT NOT NULL,     -- RFC 3339, in UTC
    hidden_at      TEXT,              -- RFC 3339, in UTC; NULL while visible
    stage          TEXT NOT NULL DEFAULT 'open'
                   CHECK (stage IN ('open', 'po', 'tech_leader', 'development', 'review', 'done')),
    epic_id        INTEGER REFERENCES epics(id),  -- NULL: no epic
    UNIQUE (project_id, key)
);
INSERT INTO tickets_new (id, project_id, key, is_provisional, title, created_at, hidden_at,
                         stage, epic_id)
    SELECT id, project_id, key, is_provisional, title, created_at, hidden_at, stage, epic_id
    FROM tickets;
CREATE TABLE ticket_stage_changes_new (
    id         INTEGER PRIMARY KEY,
    ticket_id  INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    from_stage TEXT NOT NULL
               CHECK (from_stage IN ('open', 'po', 'tech_leader', 'development', 'review', 'done')),
    to_stage   TEXT NOT NULL
               CHECK (to_stage IN ('open', 'po', 'tech_leader', 'development', 'review', 'done')),
    source     TEXT NOT NULL CHECK (source IN ('operator', 'agent_opening')),
    changed_at TEXT NOT NULL,   -- RFC 3339, in UTC
    CHECK (from_stage <> to_stage)
);
INSERT INTO ticket_stage_changes_new (id, ticket_id, from_stage, to_stage, source, changed_at)
    SELECT id, ticket_id, from_stage, to_stage, 'operator', changed_at FROM ticket_stage_changes;
DROP TABLE ticket_stage_changes;
DROP TABLE tickets;
ALTER TABLE tickets_new RENAME TO tickets;
ALTER TABLE ticket_stage_changes_new RENAME TO ticket_stage_changes;
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
        assert_eq!(MIGRATIONS[8], MIGRATION_9);
        assert_eq!(
            MIGRATION_10,
            "\
ALTER TABLE epics ADD COLUMN done_at TEXT;  -- RFC 3339, in UTC; NULL while open (NS-54)
"
        );
        assert_eq!(MIGRATIONS[9], MIGRATION_10);
        assert_eq!(MIGRATION_11, PINNED_MIGRATION_11);
        assert_eq!(MIGRATIONS[10], MIGRATION_11);
    }

    #[test]
    fn a_database_one_version_newer_is_refused_too() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch("PRAGMA user_version = 12;")
            .unwrap();
        let err = Db::open(&path).err().expect("a newer database is refused");
        assert_eq!(
            err.to_string(),
            format!(
                "Erro: o banco {} está na versão 12, mais nova que a deste nick (11). \
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

    /// A row of the history by hand, with the source `operator` (the column of the migration 9).
    fn raw_change(conn: &Connection, ticket: i64, from: &str, to: &str) -> rusqlite::Result<usize> {
        raw_sourced_change(conn, ticket, from, to, Some("operator"))
    }

    fn raw_sourced_change(
        conn: &Connection,
        ticket: i64,
        from: &str,
        to: &str,
        source: Option<&str>,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO ticket_stage_changes (ticket_id, from_stage, to_stage, source, \
             changed_at) VALUES (?1, ?2, ?3, ?4, 'now')",
            params![ticket, from, to, source],
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
            db.change_ticket_stage(ticket, "open", "review", "operator", None)
                .unwrap()
                .0,
            StageChange::Changed
        );
        assert_eq!(
            db.change_ticket_stage(ticket, "review", "development", "agent_opening", None)
                .unwrap()
                .0,
            StageChange::Changed,
            "backwards too"
        );
        assert_eq!(stage_of(&db, ticket), "development");
        assert_eq!(stage_of(&db, other.id), "open", "only that ticket");
        let changes = db.list_stage_changes(ticket).unwrap();
        assert_eq!(
            changes
                .iter()
                .map(|c| (
                    c.from_stage.as_str(),
                    c.to_stage.as_str(),
                    c.source.as_str()
                ))
                .collect::<Vec<_>>(),
            [
                ("open", "review", "operator"),
                ("review", "development", "agent_opening")
            ]
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
        // Another nick moved it to review: this one still believes it is in open.
        db.change_ticket_stage(ticket, "open", "review", "operator", None)
            .unwrap();
        assert_eq!(
            db.change_ticket_stage(ticket, "open", "done", "operator", None)
                .unwrap()
                .0,
            StageChange::Moved
        );
        assert_eq!(stage_of(&db, ticket), "review");
        assert_eq!(db.list_stage_changes(ticket).unwrap().len(), 1);
        db.delete_project(project).unwrap();
        assert_eq!(
            db.change_ticket_stage(ticket, "review", "done", "agent_opening", None)
                .unwrap()
                .0,
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
        let err = db
            .change_ticket_stage(ticket, "open", "review", "operator", None)
            .unwrap_err();
        assert!(err.to_string().contains("no history"), "{err}");
        assert_eq!(stage_of(&db, ticket), "open");
        assert_eq!(stage_changes(db.connection()), []);
        // The connection is usable afterwards (no transaction left open).
        db.connection()
            .execute_batch("DROP TRIGGER no_history;")
            .unwrap();
        assert_eq!(
            db.change_ticket_stage(ticket, "open", "review", "operator", None)
                .unwrap()
                .0,
            StageChange::Changed
        );
        // A source the CHECK refuses stores nothing either: the stage stays.
        let err = db
            .change_ticket_stage(ticket, "review", "done", "cron", None)
            .unwrap_err();
        assert!(err.to_string().contains("CHECK"), "{err}");
        assert_eq!(stage_of(&db, ticket), "review");
        assert_eq!(stage_changes(db.connection()).len(), 1);
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
        assert_eq!(db.resume_agent(a.id, "s-1").unwrap().0, AgentChange::Done);
        assert_eq!(state_of(&db, a.id), "active");
        // An active row with no owner (its nick died): resumed too.
        assert_eq!(db.resume_agent(a.id, "s-1").unwrap().0, AgentChange::Done);
        assert_eq!(state_of(&db, a.id), "active");
        db.stop_agent(a.id).unwrap();
        db.archive_agent(a.id).unwrap();
        assert_eq!(
            db.resume_agent(a.id, "s-1").unwrap().0,
            AgentChange::Archived
        );
        assert_eq!(
            state_of(&db, a.id),
            "archived",
            "an archived one is not resumed"
        );
        db.delete_project(project).unwrap();
        assert_eq!(db.resume_agent(a.id, "s-1").unwrap().0, AgentChange::Gone);
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
        assert_eq!(db.resume_agent(a.id, "s-1").unwrap().0, AgentChange::Gone);
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
        // Only up to the migration 7: the test of the migration 8 is its own (NS-50).
        let db = Db::open_with(&path, &MIGRATIONS[..7]).unwrap();
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
        assert!(f.db.set_ticket_epic(f.loose.id, Some(f.e2.id)).unwrap().0);
        assert_eq!(
            f.db.get_ticket(f.loose.id).unwrap().unwrap().epic_id,
            Some(f.e2.id)
        );
        assert!(f.db.set_ticket_epic(f.in_e1.id, None).unwrap().0);
        assert_eq!(f.db.get_ticket(f.in_e1.id).unwrap().unwrap().epic_id, None);
        // An epic of another project: not in the SQL (false), and the trigger behind it.
        let other = f.db.insert_project("auto_scrum", "q", "/q").unwrap().id;
        let foreign = f.db.insert_epic(other, "X", false, "x").unwrap();
        assert!(
            !f.db
                .set_ticket_epic(f.in_e1.id, Some(foreign.id))
                .unwrap()
                .0
        );
        assert_eq!(f.db.get_ticket(f.in_e1.id).unwrap().unwrap().epic_id, None);
        assert!(
            !f.db.set_ticket_epic(f.in_e1.id, Some(999)).unwrap().0,
            "missing epic"
        );
        assert!(
            !f.db.set_ticket_epic(999, None).unwrap().0,
            "missing ticket"
        );
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
        assert!(!f.db.set_ticket_epic(f.loose.id, Some(f.e1.id)).unwrap().0);
        assert_eq!(f.db.get_ticket(f.loose.id).unwrap().unwrap().epic_id, None);
        assert!(
            f.db.set_ticket_epic(f.in_e1.id, None).unwrap().0,
            "out of the hidden epic"
        );
        assert!(
            f.db.set_ticket_epic(f.loose.id, Some(f.e2.id)).unwrap().0,
            "a visible one"
        );
    }

    // ---- migration 8: the session of the resume (NS-50) ----

    /// The `resume_session_id` of the agent `id`, raw.
    fn resume_session_of(db: &Db, id: i64) -> Option<String> {
        db.connection()
            .query_row(
                "SELECT resume_session_id FROM agents WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn a_version_7_database_with_agents_migrates_to_8_with_no_session_of_resume() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let (before, ids) = {
            let old = Db::open_with(&path, &MIGRATIONS[..7]).unwrap();
            assert_eq!(user_version(old.connection()), 7);
            let conn = old.connection();
            let p = old.insert_project("auto_scrum", "p", "/p").unwrap();
            let t = old_insert_ticket(conn, p.id, "NS-1", false, "um");
            let a = v6_insert_agent(conn, t, "po", "s-1", "stopped", Some("NS-1"));
            let b = v6_insert_agent(conn, t, "development", "s-2", "active", None);
            let c = v6_insert_agent(conn, t, "review", "s-3", "archived", Some("NS-1"));
            (raw_agents(conn), [a, b, c])
        };
        // Only up to the migration 8: the test of the migration 9 is its own (NS-45).
        let db = Db::open_with(&path, &MIGRATIONS[..8]).unwrap();
        assert_eq!(user_version(db.connection()), 8);
        assert_eq!(raw_agents(db.connection()), before, "every row is kept");
        for id in ids {
            assert_eq!(resume_session_of(&db, id), None);
            let agent = db.get_agent(id).unwrap().unwrap();
            assert_eq!(agent.resume_session_id, None);
            assert_eq!(agent.effective_session(), agent.session_id);
        }
        assert_eq!(checks(db.connection()), (0, "ok".to_string()));
    }

    #[test]
    fn the_session_of_resume_is_unique_but_many_may_have_none() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        let b = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-2", "n", "NS-1")
            .unwrap();
        db.insert_agent(Owner::Ticket(ticket), "po", "s-3", "n", "NS-1")
            .unwrap();
        let set = |id: i64, session: &str| {
            db.connection().execute(
                "UPDATE agents SET resume_session_id = ?2 WHERE id = ?1",
                params![id, session],
            )
        };
        set(a.id, "s-9").unwrap();
        let dup = set(b.id, "s-9").unwrap_err();
        assert!(is_unique_violation(&dup), "{dup:?}");
        assert_eq!(resume_session_of(&db, b.id), None, "three rows, two NULL");
        set(b.id, "s-8").unwrap();
    }

    #[test]
    fn resume_agent_moves_the_session_of_resume_to_the_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        db.stop_agent(a.id).unwrap();
        // The same session as the effective one: nothing to swap, the column stays NULL.
        assert_eq!(
            db.resume_agent(a.id, "s-1").unwrap(),
            (AgentChange::Done, SessionSwap::Same)
        );
        assert_eq!(resume_session_of(&db, a.id), None);
        db.stop_agent(a.id).unwrap();
        // Another one: swapped, in the same UPDATE as the state.
        assert_eq!(
            db.resume_agent(a.id, "s-b").unwrap(),
            (AgentChange::Done, SessionSwap::Swapped)
        );
        assert_eq!(state_of(&db, a.id), "active");
        let agent = db.get_agent(a.id).unwrap().unwrap();
        assert_eq!(agent.resume_session_id.as_deref(), Some("s-b"));
        assert_eq!(agent.effective_session(), "s-b");
        assert_eq!(
            agent.session_id, "s-1",
            "the session of the opening never changes"
        );
        db.stop_agent(a.id).unwrap();
        // The effective one is now s-b: the same again is no swap.
        assert_eq!(
            db.resume_agent(a.id, "s-b").unwrap(),
            (AgentChange::Done, SessionSwap::Same)
        );
        db.stop_agent(a.id).unwrap();
        assert_eq!(
            db.resume_agent(a.id, "s-c").unwrap(),
            (AgentChange::Done, SessionSwap::Swapped)
        );
        assert_eq!(resume_session_of(&db, a.id).as_deref(), Some("s-c"));
    }

    #[test]
    fn resume_agent_does_not_swap_an_archived_or_gone_one() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        db.stop_agent(a.id).unwrap();
        db.archive_agent(a.id).unwrap();
        assert_eq!(
            db.resume_agent(a.id, "s-b").unwrap().0,
            AgentChange::Archived
        );
        assert_eq!(state_of(&db, a.id), "archived");
        assert_eq!(resume_session_of(&db, a.id), None, "nothing written");
        db.connection()
            .execute("UPDATE agents SET state = 'future' WHERE id = ?1", [a.id])
            .unwrap();
        assert_eq!(db.resume_agent(a.id, "s-b").unwrap().0, AgentChange::Gone);
        assert_eq!(
            resume_session_of(&db, a.id),
            None,
            "an unknown state: nothing written"
        );
        db.delete_project(project).unwrap();
        assert_eq!(db.resume_agent(a.id, "s-b").unwrap().0, AgentChange::Gone);
    }

    /// D4 of the NS-50: another agent already resumes by the latest session (the UNIQUE): the
    /// resume goes on by the effective one, and says so.
    #[test]
    fn resume_agent_keeps_the_session_when_another_agent_has_the_latest() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        let a = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        let b = db
            .insert_agent(Owner::Ticket(ticket), "po", "s-2", "n", "NS-1")
            .unwrap();
        db.stop_agent(b.id).unwrap();
        assert_eq!(
            db.resume_agent(b.id, "s-9").unwrap(),
            (AgentChange::Done, SessionSwap::Swapped)
        );
        db.stop_agent(a.id).unwrap();
        assert_eq!(
            db.resume_agent(a.id, "s-9").unwrap(),
            (AgentChange::Done, SessionSwap::Taken)
        );
        assert_eq!(state_of(&db, a.id), "active", "resumed anyway");
        assert_eq!(resume_session_of(&db, a.id), None, "by the session it had");
        assert_eq!(resume_session_of(&db, b.id).as_deref(), Some("s-9"));
    }

    // ---- migration 9: the stage "Aberto" and the source of the history (NS-45) ----

    /// Every row of a query, every column as a raw value.
    fn dump(conn: &Connection, sql: &str) -> Vec<Vec<rusqlite::types::Value>> {
        let mut stmt = conn.prepare(sql).unwrap();
        let columns = stmt.column_count();
        stmt.query_map([], |row| (0..columns).map(|i| row.get(i)).collect())
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    /// The name and the text of every trigger.
    fn triggers(conn: &Connection) -> Vec<Vec<rusqlite::types::Value>> {
        dump(
            conn,
            "SELECT name, tbl_name, sql FROM sqlite_master WHERE type = 'trigger' ORDER BY name",
        )
    }

    /// Every row of every table but the history, whose columns change (by `id`).
    fn every_row(conn: &Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
        [
            "modules", "projects", "epics", "tickets", "agents", "settings",
        ]
        .map(|table| dump(conn, &format!("SELECT * FROM {table} ORDER BY rowid")))
        .into()
    }

    const OLD_HISTORY: &str = "SELECT id, ticket_id, from_stage, to_stage, changed_at FROM ticket_stage_changes ORDER BY id";

    /// A version 8 database with a bit of everything: two projects, epics (hidden and
    /// provisional), tickets in and out of an epic (hidden, provisional, in every stage of before,
    /// with a gap in the ids), agents of a ticket and of an epic in every state (one resumed by
    /// another session), the history and a setting. Returns the ids of the projects.
    fn populated_v8(path: &Path) -> (i64, i64) {
        let old = Db::open_with(path, &MIGRATIONS[..8]).unwrap();
        assert_eq!(user_version(old.connection()), 8);
        let conn = old.connection();
        let a = old.insert_project("auto_scrum", "a", "/a").unwrap().id;
        let b = old.insert_project("auto_scrum", "b", "/b").unwrap().id;
        let epic = |project: i64, key: &str, provisional: bool, hidden: bool| -> i64 {
            conn.query_row(
                "INSERT INTO epics (project_id, key, is_provisional, title, created_at, \
                 hidden_at) VALUES (?1, ?2, ?3, 'épico', '2026-01-01T00:00:00Z', \
                 CASE WHEN ?4 THEN '2026-01-02T00:00:00Z' END) RETURNING id",
                params![project, key, provisional, hidden],
                |row| row.get(0),
            )
            .unwrap()
        };
        let ea = epic(a, "EP-1", false, false);
        let eb = epic(a, "0a1b2c3d", true, true);
        let ec = epic(b, "EP-1", false, false);
        let ticket = |project: i64,
                      key: &str,
                      provisional: bool,
                      hidden: bool,
                      stage: &str,
                      epic: Option<i64>|
         -> i64 {
            conn.query_row(
                "INSERT INTO tickets (project_id, key, is_provisional, title, created_at, \
                 hidden_at, stage, epic_id) VALUES (?1, ?2, ?3, 'título', \
                 '2026-01-03T00:00:00Z', CASE WHEN ?4 THEN '2026-01-04T00:00:00Z' END, ?5, ?6) \
                 RETURNING id",
                params![project, key, provisional, hidden, stage, epic],
                |row| row.get(0),
            )
            .unwrap()
        };
        let t1 = ticket(a, "NS-1", false, false, "po", Some(ea));
        let gone = ticket(a, "NS-0", false, false, "po", None);
        conn.execute("DELETE FROM tickets WHERE id = ?1", [gone])
            .unwrap();
        let t2 = ticket(a, "9f8e7d6c", true, true, "done", None);
        let t3 = ticket(a, "NS-3", false, false, "development", Some(eb));
        let t4 = ticket(b, "NS-1", false, false, "review", Some(ec));
        let t5 = ticket(b, "NS-5", false, true, "tech_leader", None);
        let agent = |ticket: Option<i64>,
                     epic: Option<i64>,
                     stage: &str,
                     session: &str,
                     state: &str,
                     resume: Option<&str>| {
            conn.execute(
                "INSERT INTO agents (ticket_id, epic_id, stage, session_id, name, opened_at, \
                 state, opened_key, resume_session_id) VALUES (?1, ?2, ?3, ?4, 'n', \
                 '2026-01-05T00:00:00Z', ?5, 'NS-1', ?6)",
                params![ticket, epic, stage, session, state, resume],
            )
            .unwrap();
        };
        agent(Some(t1), None, "po", "s-1", "active", None);
        agent(
            Some(t1),
            None,
            "tech_leader",
            "s-2",
            "stopped",
            Some("s-2b"),
        );
        agent(Some(t3), None, "development", "s-3", "archived", None);
        agent(Some(t4), None, "review", "s-4", "stopped", None);
        agent(None, Some(ea), "po", "s-5", "active", None);
        agent(None, Some(ec), "tech_leader", "s-6", "archived", None);
        let change = |ticket: i64, from: &str, to: &str, at: &str| {
            conn.execute(
                "INSERT INTO ticket_stage_changes (ticket_id, from_stage, to_stage, changed_at) \
                 VALUES (?1, ?2, ?3, ?4)",
                params![ticket, from, to, at],
            )
            .unwrap();
        };
        change(t2, "po", "review", "2026-01-06T00:00:00Z");
        change(t2, "review", "done", "2026-01-07T00:00:00Z");
        change(t3, "po", "development", "2026-01-08T00:00:00Z");
        change(t4, "po", "review", "2026-01-09T00:00:00Z");
        change(t5, "po", "tech_leader", "2026-01-10T00:00:00Z");
        old.set_setting(SETTING_RESERVED_SHORTCUT, "f7").unwrap();
        (a, b)
    }

    #[test]
    fn a_version_8_database_populated_migrates_to_9_keeping_every_row_and_link() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let (a, b) = populated_v8(&path);
        let (rows, history, before_triggers) = {
            let conn = Connection::open(&path).unwrap();
            (every_row(&conn), dump(&conn, OLD_HISTORY), triggers(&conn))
        };
        assert_eq!(rows[3].len(), 5, "five tickets");
        assert_eq!(rows[4].len(), 6, "six agents");
        assert_eq!(history.len(), 5);
        assert_eq!(before_triggers.len(), 6);

        // Only up to the migration 9: the epics gain a column in the 10, which has its own test
        // (NS-54).
        let db = Db::open_with(&path, &MIGRATIONS[..9]).unwrap();
        let conn = db.connection();
        assert_eq!(user_version(conn), 9);
        assert_eq!(
            every_row(conn),
            rows,
            "every row, field by field, with its id"
        );
        assert_eq!(dump(conn, OLD_HISTORY), history, "the history, with its id");
        let sources: Vec<String> = conn
            .prepare("SELECT source FROM ticket_stage_changes ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            sources, ["operator"; 5],
            "only the operator changed it before"
        );
        assert_eq!(checks(conn), (0, "ok".to_string()), "no broken link");
        assert_eq!(triggers(conn), before_triggers, "the same six triggers");
        assert_eq!(foreign_keys(conn), 1);

        // The cascade from the project still works, and takes only what is its own.
        db.delete_project(a).unwrap();
        let left = |table: &str| -> Vec<i64> {
            conn.prepare(&format!(
                "SELECT DISTINCT project_id FROM {table} ORDER BY project_id"
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
        };
        assert_eq!(left("tickets"), [b]);
        assert_eq!(left("epics"), [b]);
        let sessions: Vec<String> = conn
            .prepare("SELECT session_id FROM agents ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(sessions, ["s-4", "s-6"]);
        assert_eq!(count(conn, "ticket_stage_changes"), 2);
        assert_eq!(checks(conn), (0, "ok".to_string()));
    }

    #[test]
    fn the_triggers_of_the_migration_7_still_refuse_after_the_migration_9() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let (a, b) = populated_v8(&path);
        let db = Db::open(&path).unwrap();
        let conn = db.connection();
        // The key of an epic in a ticket, and the key of a ticket in an epic.
        let err = raw_ticket(conn, a, "EP-1", 0, "t").unwrap_err();
        assert!(is_trigger_abort(&err, KEY_OF_AN_EPIC), "{err:?}");
        let err = raw_epic(conn, a, "NS-1").unwrap_err();
        assert!(is_trigger_abort(&err, KEY_OF_A_TICKET), "{err:?}");
        // An epic of another project.
        let epic_of_b: i64 = conn
            .query_row("SELECT id FROM epics WHERE project_id = ?1", [b], |row| {
                row.get(0)
            })
            .unwrap();
        let err = conn
            .execute(
                "UPDATE tickets SET epic_id = ?2 WHERE project_id = ?1",
                params![a, epic_of_b],
            )
            .unwrap_err();
        assert!(is_trigger_abort(&err, EPIC_OF_ANOTHER_PROJECT), "{err:?}");
    }

    #[test]
    fn a_new_ticket_is_born_open() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, project, ticket) = with_ticket(&tmp);
        assert_eq!(stage_of(&db, ticket), "open");
        assert_eq!(db.list_tickets(project, true).unwrap()[0].stage, "open");
    }

    #[test]
    fn the_open_stage_and_the_source_of_the_history_are_checked() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        let conn = db.connection();
        for stage in ["po", "open"] {
            conn.execute(
                "UPDATE tickets SET stage = ?2 WHERE id = ?1",
                params![ticket, stage],
            )
            .unwrap();
        }
        for stage in ["Open", "aberto", "Aberto", ""] {
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
            raw_change(conn, ticket, "open", "open").is_err(),
            "same stage"
        );
        for source in [Some("cron"), Some("Operator"), Some(""), None] {
            assert!(
                raw_sourced_change(conn, ticket, "open", "po", source).is_err(),
                "source {source:?}"
            );
        }
        assert_eq!(stage_changes(conn), []);
        raw_sourced_change(conn, ticket, "open", "po", Some("operator")).unwrap();
        raw_sourced_change(conn, ticket, "po", "open", Some("agent_opening")).unwrap();
        assert_eq!(stage_changes(conn).len(), 2);
    }

    // ---- migration 10: the state of an epic, open or done (NS-54) ----

    /// Every row of every table, the epics without the column of the migration 10 (by `id`).
    fn every_row_of_v9(conn: &Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
        [
            "SELECT * FROM modules ORDER BY rowid",
            "SELECT * FROM projects ORDER BY rowid",
            "SELECT id, project_id, key, is_provisional, title, created_at, hidden_at \
             FROM epics ORDER BY rowid",
            "SELECT * FROM tickets ORDER BY rowid",
            "SELECT * FROM agents ORDER BY rowid",
            "SELECT * FROM settings ORDER BY rowid",
            "SELECT * FROM ticket_stage_changes ORDER BY rowid",
        ]
        .map(|sql| dump(conn, sql))
        .into()
    }

    #[test]
    fn a_version_9_database_populated_migrates_to_10_with_every_epic_open() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        populated_v8(&path);
        // The same populated database, at the version 9: an epic whose every ticket is done
        // included, which stays open too (nothing is inferred).
        {
            let v9 = Db::open_with(&path, &MIGRATIONS[..9]).unwrap();
            assert_eq!(user_version(v9.connection()), 9);
            let conn = v9.connection();
            let p: i64 = conn
                .query_row("SELECT id FROM projects ORDER BY id LIMIT 1", [], |row| {
                    row.get(0)
                })
                .unwrap();
            let all_done = conn
                .query_row(
                    "INSERT INTO epics (project_id, key, is_provisional, title, created_at) \
                     VALUES (?1, 'EP-9', 0, 'pronto', '2026-01-11T00:00:00Z') RETURNING id",
                    [p],
                    |row| row.get::<_, i64>(0),
                )
                .unwrap();
            conn.execute(
                "INSERT INTO tickets (project_id, key, is_provisional, title, created_at, stage, \
                 epic_id) VALUES (?1, 'NS-9', 0, 't', '2026-01-12T00:00:00Z', 'done', ?2)",
                params![p, all_done],
            )
            .unwrap();
        }
        let (rows, before_triggers) = {
            let conn = Connection::open(&path).unwrap();
            (every_row_of_v9(&conn), triggers(&conn))
        };
        assert_eq!(rows[2].len(), 4, "four epics");
        assert_eq!(rows[3].len(), 6, "six tickets");
        assert_eq!(rows[4].len(), 6, "six agents");
        assert_eq!(rows[6].len(), 5, "five changes of the stage");
        assert_eq!(before_triggers.len(), 6);

        // Only up to the migration 10: the history gains a column in the 11, which has its own
        // test (NS-61).
        let db = Db::open_with(&path, &MIGRATIONS[..10]).unwrap();
        let conn = db.connection();
        assert_eq!(user_version(conn), 10);
        assert_eq!(
            every_row_of_v9(conn),
            rows,
            "every row, field by field, with its id"
        );
        let done: Vec<Option<String>> = conn
            .prepare("SELECT done_at FROM epics ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(done, [None, None, None, None], "every epic open");
        for epic in conn
            .prepare("SELECT project_id FROM epics")
            .unwrap()
            .query_map([], |row| row.get::<_, i64>(0))
            .unwrap()
            .map(Result::unwrap)
            .collect::<Vec<_>>()
        {
            assert!(
                db.list_epics(epic, true)
                    .unwrap()
                    .iter()
                    .all(|e| e.done_at.is_none())
            );
        }
        assert_eq!(checks(conn), (0, "ok".to_string()), "no broken link");
        assert_eq!(
            triggers(conn),
            before_triggers,
            "the triggers of the migration 9"
        );
        assert_eq!(foreign_keys(conn), 1);
    }

    #[test]
    fn an_epic_is_born_open_and_reads_its_done_at() {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let p = db.insert_project("auto_scrum", "p", "/p").unwrap().id;
        let epic = db.insert_epic(p, "E-1", false, "e").unwrap();
        assert_eq!(epic.done_at, None);
        db.connection()
            .execute(
                "UPDATE epics SET done_at = '2026-11-22T10:00:00Z' WHERE id = ?1",
                [epic.id],
            )
            .unwrap();
        let read = db.get_epic(epic.id).unwrap().unwrap();
        assert_eq!(read.done_at.as_deref(), Some("2026-11-22T10:00:00Z"));
        assert_eq!(db.list_epics(p, true).unwrap(), [read]);
    }

    // ---- completing and reopening an epic (NS-54, D2) ----

    /// Sets the stage of the ticket by hand (the tests of the epic only need where it is).
    fn put_stage(db: &Db, ticket: i64, stage: &str) {
        db.connection()
            .execute(
                "UPDATE tickets SET stage = ?2 WHERE id = ?1",
                params![ticket, stage],
            )
            .unwrap();
    }

    fn done_at_of(db: &Db, epic: i64) -> Option<String> {
        db.get_epic(epic).unwrap().unwrap().done_at
    }

    #[test]
    fn complete_epic_with_every_ticket_done_stores_when() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        put_stage(&f.db, f.in_e1.id, "done");
        // A done ticket without epic, or of another epic, does not matter.
        assert_eq!(f.db.complete_epic(f.e1.id).unwrap(), EpicCompletion::Done);
        assert_rfc3339_utc(done_at_of(&f.db, f.e1.id).as_deref().unwrap());
        assert_eq!(done_at_of(&f.db, f.e2.id), None, "only that epic");
        assert_eq!(
            f.db.complete_epic(f.e1.id).unwrap(),
            EpicCompletion::AlreadyDone,
            "another nick completed it before"
        );
    }

    #[test]
    fn complete_epic_is_refused_with_tickets_not_done_the_hidden_ones_counted() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        assert_eq!(
            f.db.complete_epic(f.e1.id).unwrap(),
            EpicCompletion::Pending {
                pending: 1,
                hidden: 0
            }
        );
        let hidden =
            f.db.insert_ticket(f.project, "NS-3", false, "t", Some(f.e1.id))
                .unwrap();
        f.db.set_ticket_hidden(hidden.id, true).unwrap();
        put_stage(&f.db, hidden.id, "review");
        assert_eq!(
            f.db.complete_epic(f.e1.id).unwrap(),
            EpicCompletion::Pending {
                pending: 2,
                hidden: 1
            }
        );
        put_stage(&f.db, f.in_e1.id, "done");
        assert_eq!(
            f.db.complete_epic(f.e1.id).unwrap(),
            EpicCompletion::Pending {
                pending: 1,
                hidden: 1
            },
            "the hidden one, not done, still counts, and is told apart"
        );
        assert_eq!(done_at_of(&f.db, f.e1.id), None, "nothing written");
        put_stage(&f.db, hidden.id, "done");
        assert_eq!(f.db.complete_epic(f.e1.id).unwrap(), EpicCompletion::Done);
    }

    #[test]
    fn complete_epic_is_refused_for_an_empty_or_gone_epic() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        // e2 has no ticket (the done one without epic is not its).
        put_stage(&f.db, f.loose.id, "done");
        assert_eq!(f.db.complete_epic(f.e2.id).unwrap(), EpicCompletion::Empty);
        assert_eq!(done_at_of(&f.db, f.e2.id), None);
        assert_eq!(f.db.complete_epic(999).unwrap(), EpicCompletion::Gone);
        f.db.delete_project(f.project).unwrap();
        assert_eq!(f.db.complete_epic(f.e1.id).unwrap(), EpicCompletion::Gone);
    }

    #[test]
    fn complete_epic_of_a_hidden_epic_or_with_an_active_agent_is_allowed() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        let t =
            f.db.insert_ticket(f.project, "NS-3", false, "t", Some(f.e2.id))
                .unwrap();
        put_stage(&f.db, t.id, "done");
        f.db.insert_agent(Owner::Epic(f.e2.id), "po", "s-1", "n", "E")
            .unwrap();
        f.db.insert_agent(Owner::Ticket(t.id), "po", "s-2", "n", "T")
            .unwrap();
        f.db.connection()
            .execute("UPDATE epics SET hidden_at = 'x' WHERE id = ?1", [f.e2.id])
            .unwrap();
        assert_eq!(f.db.complete_epic(f.e2.id).unwrap(), EpicCompletion::Done);
        let epic = f.db.get_epic(f.e2.id).unwrap().unwrap();
        assert_eq!(epic.hidden_at.as_deref(), Some("x"), "still hidden");
    }

    #[test]
    fn reopen_epic_opens_a_done_one_only() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        assert!(!f.db.reopen_epic(f.e1.id).unwrap(), "already open");
        put_stage(&f.db, f.in_e1.id, "done");
        f.db.complete_epic(f.e1.id).unwrap();
        assert!(f.db.reopen_epic(f.e1.id).unwrap());
        assert_eq!(done_at_of(&f.db, f.e1.id), None);
        assert!(!f.db.reopen_epic(f.e1.id).unwrap(), "open again");
        assert!(!f.db.reopen_epic(999).unwrap(), "gone");
    }

    // ---- the automatic reopening, in the writers of a ticket (NS-54, D3) ----

    /// `with_epics` with the epic e1 done (its ticket in_e1 done): the start of every case.
    fn with_done_epic(tmp: &tempfile::TempDir) -> Epics {
        let f = with_epics(tmp);
        put_stage(&f.db, f.in_e1.id, "done");
        assert_eq!(f.db.complete_epic(f.e1.id).unwrap(), EpicCompletion::Done);
        f
    }

    #[test]
    fn a_new_ticket_in_a_done_epic_reopens_it_and_says_which() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_done_epic(&tmp);
        let (ticket, reopened) =
            f.db.insert_ticket_reopening(f.project, "NS-3", false, "t", Some(f.e1.id))
                .unwrap();
        assert_eq!(reopened.as_deref(), Some("EPIC-1"));
        assert_eq!(ticket.epic_id, Some(f.e1.id));
        assert_eq!(done_at_of(&f.db, f.e1.id), None);
        // In an open epic, or without one: nothing to reopen.
        let (_, reopened) =
            f.db.insert_ticket_reopening(f.project, "NS-4", false, "t", Some(f.e1.id))
                .unwrap();
        assert_eq!(reopened, None, "already open");
        let (_, reopened) =
            f.db.insert_ticket_reopening(f.project, "NS-5", false, "t", None)
                .unwrap();
        assert_eq!(reopened, None, "no epic");
        // The insert_ticket of before reopens too (the same transaction), and gives the ticket.
        put_stage(&f.db, f.in_e1.id, "done");
        f.db.connection()
            .execute("UPDATE epics SET done_at = 'x' WHERE id = ?1", [f.e1.id])
            .unwrap();
        let t =
            f.db.insert_ticket(f.project, "NS-6", false, "t", Some(f.e1.id))
                .unwrap();
        assert_eq!(t.key, "NS-6");
        assert_eq!(done_at_of(&f.db, f.e1.id), None);
    }

    #[test]
    fn moving_a_ticket_not_done_into_a_done_epic_reopens_it() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_done_epic(&tmp);
        assert_eq!(
            f.db.set_ticket_epic(f.loose.id, Some(f.e1.id)).unwrap(),
            (true, Some("EPIC-1".to_string()))
        );
        assert_eq!(done_at_of(&f.db, f.e1.id), None);
        assert_eq!(
            f.db.get_ticket(f.loose.id).unwrap().unwrap().epic_id,
            Some(f.e1.id)
        );
    }

    #[test]
    fn moving_a_done_ticket_into_a_done_epic_does_not_reopen_it() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_done_epic(&tmp);
        put_stage(&f.db, f.loose.id, "done");
        assert_eq!(
            f.db.set_ticket_epic(f.loose.id, Some(f.e1.id)).unwrap(),
            (true, None)
        );
        assert!(done_at_of(&f.db, f.e1.id).is_some(), "the rule still holds");
    }

    #[test]
    fn taking_the_last_ticket_out_of_a_done_epic_keeps_it_done() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_done_epic(&tmp);
        assert_eq!(
            f.db.set_ticket_epic(f.in_e1.id, None).unwrap(),
            (true, None)
        );
        assert!(done_at_of(&f.db, f.e1.id).is_some(), "done and empty");
        // A move refused reopens nothing either.
        put_stage(&f.db, f.loose.id, "review");
        assert_eq!(
            f.db.set_ticket_epic(999, Some(f.e1.id)).unwrap(),
            (false, None)
        );
        assert!(done_at_of(&f.db, f.e1.id).is_some());
    }

    #[test]
    fn a_ticket_leaving_done_reopens_its_done_epic() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_done_epic(&tmp);
        assert_eq!(
            f.db.change_ticket_stage(f.in_e1.id, "done", "development", "operator", None)
                .unwrap(),
            (StageChange::Changed, Some("EPIC-1".to_string()))
        );
        assert_eq!(done_at_of(&f.db, f.e1.id), None);
        assert_eq!(f.db.list_stage_changes(f.in_e1.id).unwrap().len(), 1);
        // The epic read in the SQL, not from a ticket in memory: moved to the done epic e1
        // meanwhile, a ticket of no epic before reopens it.
        put_stage(&f.db, f.in_e1.id, "done");
        f.db.complete_epic(f.e1.id).unwrap();
        put_stage(&f.db, f.loose.id, "done");
        f.db.set_ticket_epic(f.loose.id, Some(f.e1.id)).unwrap();
        assert!(done_at_of(&f.db, f.e1.id).is_some());
        assert_eq!(
            f.db.change_ticket_stage(f.loose.id, "done", "review", "agent_opening", None)
                .unwrap(),
            (StageChange::Changed, Some("EPIC-1".to_string()))
        );
    }

    #[test]
    fn a_stage_of_a_ticket_without_epic_or_of_an_open_epic_reopens_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        assert_eq!(
            f.db.change_ticket_stage(f.loose.id, "open", "review", "operator", None)
                .unwrap(),
            (StageChange::Changed, None)
        );
        assert_eq!(
            f.db.change_ticket_stage(f.in_e1.id, "open", "done", "operator", None)
                .unwrap(),
            (StageChange::Changed, None)
        );
        assert_eq!(
            f.db.change_ticket_stage(f.in_e1.id, "done", "po", "operator", None)
                .unwrap(),
            (StageChange::Changed, None),
            "an open epic stays open"
        );
        assert_eq!(done_at_of(&f.db, f.e1.id), None);
        // Gone and moved reopen nothing.
        let f = with_done_epic(&tempfile::tempdir().unwrap());
        assert_eq!(
            f.db.change_ticket_stage(f.in_e1.id, "po", "review", "operator", None)
                .unwrap(),
            (StageChange::Moved, None)
        );
        assert!(done_at_of(&f.db, f.e1.id).is_some());
    }

    /// The write of the ticket and the reopening go together or not at all: an epic that can't
    /// be reopened (a test trigger) leaves the ticket as it was, in each of the three writers.
    #[test]
    fn the_write_of_the_ticket_and_the_reopening_are_all_or_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_done_epic(&tmp);
        f.db.connection()
            .execute_batch(
                "CREATE TRIGGER no_reopen BEFORE UPDATE OF done_at ON epics \
                 BEGIN SELECT RAISE(ABORT, 'no reopen'); END;",
            )
            .unwrap();
        let before = f.db.list_tickets(f.project, true).unwrap();
        let err =
            f.db.insert_ticket_reopening(f.project, "NS-3", false, "t", Some(f.e1.id))
                .unwrap_err();
        assert!(err.to_string().contains("no reopen"), "{err}");
        let err = f.db.set_ticket_epic(f.loose.id, Some(f.e1.id)).unwrap_err();
        assert!(err.to_string().contains("no reopen"), "{err}");
        let err =
            f.db.change_ticket_stage(f.in_e1.id, "done", "po", "operator", None)
                .unwrap_err();
        assert!(err.to_string().contains("no reopen"), "{err}");
        assert_eq!(
            f.db.list_tickets(f.project, true).unwrap(),
            before,
            "no ticket new, moved nor changed"
        );
        assert_eq!(f.db.list_stage_changes(f.in_e1.id).unwrap(), []);
        assert!(done_at_of(&f.db, f.e1.id).is_some());
        // The connection is usable afterwards (no transaction left open).
        f.db.connection()
            .execute_batch("DROP TRIGGER no_reopen;")
            .unwrap();
        assert_eq!(
            f.db.set_ticket_epic(f.loose.id, Some(f.e1.id)).unwrap(),
            (true, Some("EPIC-1".to_string()))
        );
    }

    // ---- migration 11: the stage "Rejeitado" and the reason of the rejection (NS-61) ----

    /// The text of the migration 11, pinned (a published migration is never edited).
    const PINNED_MIGRATION_11: &str = "\
DROP TRIGGER epics_key_not_a_ticket_insert;
DROP TRIGGER epics_key_not_a_ticket_update;
DROP TRIGGER tickets_key_not_an_epic_insert;
DROP TRIGGER tickets_key_not_an_epic_update;
DROP TRIGGER tickets_epic_of_the_project_insert;
DROP TRIGGER tickets_epic_of_the_project_update;
CREATE TABLE tickets_new (
    id             INTEGER PRIMARY KEY,
    project_id     INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    key            TEXT NOT NULL,     -- the Jira ID, or the provisional short UUID
    is_provisional INTEGER NOT NULL CHECK (is_provisional IN (0, 1)),
    title          TEXT NOT NULL CHECK (trim(title) <> ''),
    created_at     TEXT NOT NULL,     -- RFC 3339, in UTC
    hidden_at      TEXT,              -- RFC 3339, in UTC; NULL while visible
    stage          TEXT NOT NULL DEFAULT 'open'
                   CHECK (stage IN ('open', 'po', 'tech_leader', 'development', 'review', 'done',
                                    'rejected')),
    epic_id        INTEGER REFERENCES epics(id),  -- NULL: no epic
    UNIQUE (project_id, key)
);
INSERT INTO tickets_new (id, project_id, key, is_provisional, title, created_at, hidden_at,
                         stage, epic_id)
    SELECT id, project_id, key, is_provisional, title, created_at, hidden_at, stage, epic_id
    FROM tickets;
CREATE TABLE ticket_stage_changes_new (
    id         INTEGER PRIMARY KEY,
    ticket_id  INTEGER NOT NULL REFERENCES tickets(id) ON DELETE CASCADE,
    from_stage TEXT NOT NULL
               CHECK (from_stage IN ('open', 'po', 'tech_leader', 'development', 'review', 'done',
                                     'rejected')),
    to_stage   TEXT NOT NULL
               CHECK (to_stage IN ('open', 'po', 'tech_leader', 'development', 'review', 'done',
                                   'rejected')),
    source     TEXT NOT NULL CHECK (source IN ('operator', 'agent_opening')),
    reason     TEXT,            -- why it was rejected; only in a change to 'rejected'
    changed_at TEXT NOT NULL,   -- RFC 3339, in UTC
    CHECK (from_stage <> to_stage),
    CHECK ((to_stage = 'rejected') = (reason IS NOT NULL)),
    CHECK (reason IS NULL OR (trim(reason) <> '' AND length(reason) <= 120))
);
INSERT INTO ticket_stage_changes_new (id, ticket_id, from_stage, to_stage, source, reason,
                                      changed_at)
    SELECT id, ticket_id, from_stage, to_stage, source, NULL, changed_at
    FROM ticket_stage_changes;
DROP TABLE ticket_stage_changes;
DROP TABLE tickets;
ALTER TABLE tickets_new RENAME TO tickets;
ALTER TABLE ticket_stage_changes_new RENAME TO ticket_stage_changes;
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
";

    /// The history with the columns of before the migration 11.
    const HISTORY_OF_V10: &str = "SELECT id, ticket_id, from_stage, to_stage, source, changed_at \
                                  FROM ticket_stage_changes ORDER BY id";

    /// Every row of every table but the history, whose columns change (by `id`), at the version 10.
    fn every_row_of_v10(conn: &Connection) -> Vec<Vec<Vec<rusqlite::types::Value>>> {
        [
            "modules", "projects", "epics", "tickets", "agents", "settings",
        ]
        .map(|table| dump(conn, &format!("SELECT * FROM {table} ORDER BY rowid")))
        .into()
    }

    /// The names of the columns of `table`, in order.
    fn columns_of(conn: &Connection, table: &str) -> Vec<String> {
        conn.prepare(&format!(
            "SELECT name FROM pragma_table_info('{table}') ORDER BY cid"
        ))
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect()
    }

    /// `populated_v8` taken to the version 10, with more: a done epic (its two tickets done, one
    /// hidden), a ticket in `open` (the stage of the migration 9) in and out of an epic, and
    /// changes by the opening of an agent. Every stage of before has a ticket now. Returns the ids
    /// of the projects.
    fn populated_v10(path: &Path) -> (i64, i64) {
        let (a, b) = populated_v8(path);
        let v10 = Db::open_with(path, &MIGRATIONS[..10]).unwrap();
        assert_eq!(user_version(v10.connection()), 10);
        let conn = v10.connection();
        let done_epic = conn
            .query_row(
                "INSERT INTO epics (project_id, key, is_provisional, title, created_at, done_at) \
                 VALUES (?1, 'EP-9', 0, 'pronto', '2026-01-11T00:00:00Z', \
                 '2026-01-20T00:00:00Z') RETURNING id",
                [a],
                |row| row.get::<_, i64>(0),
            )
            .unwrap();
        let ticket = |key: &str, hidden: bool, stage: &str, epic: Option<i64>| -> i64 {
            conn.query_row(
                "INSERT INTO tickets (project_id, key, is_provisional, title, created_at, \
                 hidden_at, stage, epic_id) VALUES (?1, ?2, 0, 'título', '2026-01-12T00:00:00Z', \
                 CASE WHEN ?3 THEN '2026-01-13T00:00:00Z' END, ?4, ?5) RETURNING id",
                params![a, key, hidden, stage, epic],
                |row| row.get(0),
            )
            .unwrap()
        };
        let t9 = ticket("NS-9", false, "done", Some(done_epic));
        ticket("NS-10", true, "done", Some(done_epic));
        let epic_of_a: i64 = conn
            .query_row(
                "SELECT id FROM epics WHERE project_id = ?1 AND key = 'EP-1'",
                [a],
                |row| row.get(0),
            )
            .unwrap();
        let t11 = ticket("NS-11", false, "open", Some(epic_of_a));
        ticket("NS-12", false, "open", None);
        for (ticket, from, to, source) in [
            (t9, "open", "development", "agent_opening"),
            (t9, "development", "done", "operator"),
            (t11, "po", "open", "operator"),
        ] {
            raw_sourced_change(conn, ticket, from, to, Some(source)).unwrap();
        }
        v10.set_setting(SETTING_TREE_HIDE_DONE, "1").unwrap();
        (a, b)
    }

    #[test]
    fn a_version_10_database_populated_migrates_to_11_keeping_every_row_and_link() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        let (a, b) = populated_v10(&path);
        let (rows, history, before_triggers, ticket_columns) = {
            let conn = Connection::open(&path).unwrap();
            (
                every_row_of_v10(&conn),
                dump(&conn, HISTORY_OF_V10),
                triggers(&conn),
                columns_of(&conn, "tickets"),
            )
        };
        assert_eq!(rows[2].len(), 4, "four epics");
        assert_eq!(rows[3].len(), 9, "nine tickets");
        assert_eq!(rows[4].len(), 6, "six agents");
        assert_eq!(rows[5].len(), 2, "two settings");
        assert_eq!(history.len(), 8, "eight changes of the stage");
        assert_eq!(before_triggers.len(), 6);
        let stages: std::collections::BTreeSet<String> = rows[3]
            .iter()
            .map(|row| match &row[7] {
                rusqlite::types::Value::Text(stage) => stage.clone(),
                other => panic!("{other:?}"),
            })
            .collect();
        assert_eq!(
            stages.into_iter().collect::<Vec<_>>(),
            ["development", "done", "open", "po", "review", "tech_leader"],
            "every stage of before"
        );

        let db = Db::open(&path).unwrap();
        let conn = db.connection();
        assert_eq!(user_version(conn), 11);
        assert_eq!(
            every_row_of_v10(conn),
            rows,
            "every row, field by field, with its id"
        );
        assert_eq!(
            dump(conn, HISTORY_OF_V10),
            history,
            "the history, with its id"
        );
        assert_eq!(
            columns_of(conn, "tickets"),
            ticket_columns,
            "the same columns"
        );
        assert_eq!(
            columns_of(conn, "ticket_stage_changes"),
            [
                "id",
                "ticket_id",
                "from_stage",
                "to_stage",
                "source",
                "reason",
                "changed_at"
            ],
            "the reason after the source"
        );
        let reasons: Vec<Option<String>> = conn
            .prepare("SELECT reason FROM ticket_stage_changes ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(reasons, vec![None; 8], "no reason in the history of before");
        let ids: Vec<i64> = conn
            .prepare("SELECT id FROM tickets ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        let read: Vec<Option<String>> = ids
            .iter()
            .flat_map(|id| db.list_stage_changes(*id).unwrap())
            .map(|c| c.reason)
            .collect();
        assert_eq!(read, vec![None; 8], "the reader of the history reads it");
        assert_eq!(checks(conn), (0, "ok".to_string()), "no broken link");
        assert_eq!(
            triggers(conn),
            before_triggers,
            "the six triggers, with the text of the migration 9"
        );
        assert_eq!(foreign_keys(conn), 1);
        // The epics keep their state.
        let done: Vec<Option<String>> = conn
            .prepare("SELECT done_at FROM epics ORDER BY id")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        assert_eq!(
            done.iter().filter(|d| d.is_some()).count(),
            1,
            "the done epic"
        );

        // The triggers still refuse.
        let err = raw_ticket(conn, a, "EP-1", 0, "t").unwrap_err();
        assert!(is_trigger_abort(&err, KEY_OF_AN_EPIC), "{err:?}");
        let err = raw_epic(conn, a, "NS-1").unwrap_err();
        assert!(is_trigger_abort(&err, KEY_OF_A_TICKET), "{err:?}");
        let epic_of_b: i64 = conn
            .query_row("SELECT id FROM epics WHERE project_id = ?1", [b], |row| {
                row.get(0)
            })
            .unwrap();
        let err = conn
            .execute(
                "UPDATE tickets SET epic_id = ?2 WHERE project_id = ?1",
                params![a, epic_of_b],
            )
            .unwrap_err();
        assert!(is_trigger_abort(&err, EPIC_OF_ANOTHER_PROJECT), "{err:?}");

        // The cascade from the project still works, and takes only what is its own.
        db.delete_project(a).unwrap();
        let left = |table: &str| -> Vec<i64> {
            conn.prepare(&format!(
                "SELECT DISTINCT project_id FROM {table} ORDER BY project_id"
            ))
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect()
        };
        assert_eq!(left("tickets"), [b]);
        assert_eq!(left("epics"), [b]);
        assert_eq!(count(conn, "agents"), 2);
        assert_eq!(count(conn, "ticket_stage_changes"), 2, "the history of b");
        assert_eq!(checks(conn), (0, "ok".to_string()));
    }

    /// A row of the history by hand, with a reason (the column of the migration 11).
    fn raw_reasoned_change(
        conn: &Connection,
        ticket: i64,
        from: &str,
        to: &str,
        reason: Option<&str>,
    ) -> rusqlite::Result<usize> {
        conn.execute(
            "INSERT INTO ticket_stage_changes (ticket_id, from_stage, to_stage, source, reason, \
             changed_at) VALUES (?1, ?2, ?3, 'operator', ?4, 'now')",
            params![ticket, from, to, reason],
        )
    }

    #[test]
    fn the_rejected_stage_and_the_reason_are_checked() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        let conn = db.connection();
        // An invalid stage, in both tables; `rejected` is accepted.
        let set = |stage: &str| {
            conn.execute(
                "UPDATE tickets SET stage = ?2 WHERE id = ?1",
                params![ticket, stage],
            )
        };
        for bad in ["archived", "Rejected", "rejeitado", ""] {
            assert!(set(bad).is_err(), "ticket stage {bad:?}");
            assert!(raw_change(conn, ticket, bad, "po").is_err(), "from {bad:?}");
            assert!(
                raw_reasoned_change(conn, ticket, "po", bad, Some("x")).is_err(),
                "to {bad:?}"
            );
        }
        set("rejected").unwrap();
        assert_eq!(stage_of(&db, ticket), "rejected");
        set("open").unwrap();
        // A change to `rejected` needs a reason; a change to another stage has none.
        assert!(
            raw_reasoned_change(conn, ticket, "open", "rejected", None).is_err(),
            "rejected without a reason"
        );
        assert!(
            raw_reasoned_change(conn, ticket, "open", "po", Some("motivo")).is_err(),
            "a reason out of a rejection"
        );
        assert!(
            raw_reasoned_change(conn, ticket, "rejected", "po", Some("motivo")).is_err(),
            "a reason leaving rejected"
        );
        // The reason: not empty nor only spaces, up to 120 characters (accents count one). The
        // `trim` of SQLite takes only spaces, as in the CHECK of the title: the other blanks (a
        // tab) are refused by the code, which stores the reason trimmed (`tickets`).
        for bad in ["", " ", "     ", &"a".repeat(121), &"é".repeat(121)] {
            assert!(
                raw_reasoned_change(conn, ticket, "open", "rejected", Some(bad)).is_err(),
                "reason {bad:?}"
            );
        }
        assert_eq!(stage_changes(conn), [], "nothing stored");
        let longest = "é".repeat(120);
        raw_reasoned_change(conn, ticket, "open", "rejected", Some(&longest)).unwrap();
        raw_reasoned_change(conn, ticket, "rejected", "po", None).unwrap();
        raw_reasoned_change(conn, ticket, "po", "rejected", Some("a")).unwrap();
        assert!(
            raw_change(conn, ticket, "rejected", "rejected").is_err(),
            "same stage"
        );
        assert_eq!(stage_changes(conn).len(), 3);
    }

    #[test]
    fn change_ticket_stage_stores_the_reason_of_a_rejection_and_none_otherwise() {
        let tmp = tempfile::tempdir().unwrap();
        let (db, _, ticket) = with_ticket(&tmp);
        assert_eq!(
            db.change_ticket_stage(ticket, "open", "rejected", "operator", Some("não vale"))
                .unwrap()
                .0,
            StageChange::Changed
        );
        assert_eq!(
            db.change_ticket_stage(ticket, "rejected", "po", "operator", None)
                .unwrap()
                .0,
            StageChange::Changed
        );
        let read: Vec<(String, String, Option<String>)> = db
            .list_stage_changes(ticket)
            .unwrap()
            .into_iter()
            .map(|c| (c.from_stage, c.to_stage, c.reason))
            .collect();
        assert_eq!(
            read,
            [
                ("open".into(), "rejected".into(), Some("não vale".into())),
                ("rejected".into(), "po".into(), None),
            ]
        );
        // The CHECK refuses a rejection without a reason: the stage is not written either.
        let err = db
            .change_ticket_stage(ticket, "po", "rejected", "operator", None)
            .unwrap_err();
        assert!(err.to_string().contains("CHECK"), "{err}");
        assert_eq!(stage_of(&db, ticket), "po", "all or nothing");
        assert_eq!(db.list_stage_changes(ticket).unwrap().len(), 2);
    }

    // ---- a rejected ticket is closed for the epic (NS-61, D2 and D8) ----

    #[test]
    fn an_epic_with_its_tickets_rejected_or_done_and_rejected_is_completed() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        put_stage(&f.db, f.in_e1.id, "rejected");
        assert_eq!(
            f.db.complete_epic(f.e1.id).unwrap(),
            EpicCompletion::Done,
            "all rejected"
        );
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        put_stage(&f.db, f.in_e1.id, "done");
        let other =
            f.db.insert_ticket(f.project, "NS-3", false, "t", Some(f.e1.id))
                .unwrap();
        put_stage(&f.db, other.id, "rejected");
        assert_eq!(
            f.db.complete_epic(f.e1.id).unwrap(),
            EpicCompletion::Done,
            "mixed"
        );
    }

    #[test]
    fn a_hidden_rejected_ticket_is_not_pending() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_epics(&tmp);
        put_stage(&f.db, f.in_e1.id, "done");
        let hidden =
            f.db.insert_ticket(f.project, "NS-3", false, "t", Some(f.e1.id))
                .unwrap();
        f.db.set_ticket_hidden(hidden.id, true).unwrap();
        put_stage(&f.db, hidden.id, "review");
        assert_eq!(
            f.db.complete_epic(f.e1.id).unwrap(),
            EpicCompletion::Pending {
                pending: 1,
                hidden: 1
            }
        );
        put_stage(&f.db, hidden.id, "rejected");
        assert_eq!(f.db.complete_epic(f.e1.id).unwrap(), EpicCompletion::Done);
    }

    #[test]
    fn between_done_and_rejected_a_done_epic_stays_done_and_leaving_rejected_reopens_it() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_done_epic(&tmp);
        assert_eq!(
            f.db.change_ticket_stage(f.in_e1.id, "done", "rejected", "operator", Some("não"))
                .unwrap(),
            (StageChange::Changed, None),
            "Concluído → Rejeitado"
        );
        assert!(done_at_of(&f.db, f.e1.id).is_some());
        assert_eq!(
            f.db.change_ticket_stage(f.in_e1.id, "rejected", "done", "operator", None)
                .unwrap(),
            (StageChange::Changed, None),
            "Rejeitado → Concluído"
        );
        assert!(done_at_of(&f.db, f.e1.id).is_some());
        f.db.change_ticket_stage(f.in_e1.id, "done", "rejected", "operator", Some("não"))
            .unwrap();
        assert_eq!(
            f.db.change_ticket_stage(f.in_e1.id, "rejected", "development", "operator", None)
                .unwrap(),
            (StageChange::Changed, Some("EPIC-1".to_string())),
            "Rejeitado → Dev"
        );
        assert_eq!(done_at_of(&f.db, f.e1.id), None);
    }

    #[test]
    fn moving_a_rejected_ticket_into_a_done_epic_does_not_reopen_it() {
        let tmp = tempfile::tempdir().unwrap();
        let f = with_done_epic(&tmp);
        put_stage(&f.db, f.loose.id, "rejected");
        assert_eq!(
            f.db.set_ticket_epic(f.loose.id, Some(f.e1.id)).unwrap(),
            (true, None)
        );
        assert!(done_at_of(&f.db, f.e1.id).is_some(), "the rule still holds");
        // A new ticket (born open) still reopens it.
        let (_, reopened) =
            f.db.insert_ticket_reopening(f.project, "NS-3", false, "t", Some(f.e1.id))
                .unwrap();
        assert_eq!(reopened.as_deref(), Some("EPIC-1"));
    }

    #[test]
    fn a_new_ticket_is_still_born_open_after_the_migration_11() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("nick.db");
        populated_v10(&path);
        let db = Db::open(&path).unwrap();
        let p = db.insert_project("auto_scrum", "novo", "/novo").unwrap().id;
        let t = db.insert_ticket(p, "NS-1", false, "t", None).unwrap();
        assert_eq!(t.stage, "open");
        assert_eq!(stage_of(&db, t.id), "open");
    }
}
