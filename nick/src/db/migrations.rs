//! The ordered list of schema migrations (ABOUT.md, "Rust no projeto: a casca `nick`
//! (NS-31)"). Migration N (the item at index N-1) takes the database from version N-1 to N;
//! the version is `PRAGMA user_version`. A published migration is never edited: a mistake is
//! fixed by a new migration at the end of the list. A test pins the text of each published
//! migration, so editing one makes it fail.

/// Migration 1: the modules of the nick (only auto_scrum in this slice) and the projects of
/// each module. A project belongs to a module because the navigation is "the projects of the
/// module"; another module that uses the same directory registers it again.
pub const MIGRATION_1: &str = "\
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
";

/// Migration 2 (NS-32): the tickets of each project, kept only in the database. `key` is the
/// Jira ID or, when none was given, a provisional short UUID (`is_provisional = 1`, so the F4
/// knows which one to replace). Hiding sets `hidden_at`; nothing is ever deleted, except with
/// the project (ON DELETE CASCADE). A hidden ticket still holds its key in the project.
pub const MIGRATION_2: &str = "\
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
";

/// Migration 3 (NS-33): the agents opened from each ticket. `stage` takes the same values as
/// the `$TYPE` of auto_scrum.sh; `session_id` is the `claude --session-id`, chosen by the nick
/// (a UUID v4, `Db::random_session_id`). `state` has no CHECK on purpose: the F5 adds states
/// ("stopped", "archived"), and SQLite can't alter a CHECK without rebuilding the table, so the
/// source of the states is the `AgentState` enum (agents.rs), the only writer of the column.
/// The agents go with their ticket (ON DELETE CASCADE), which goes with its project.
pub const MIGRATION_3: &str = "\
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
";

/// Migration 4 (NS-34): the settings of the nick, a key and its value. A missing row is the
/// default, which lives only in the code: going back to the default deletes the row, so a
/// future change of the default reaches whoever never changed the setting. The first key is
/// the reserved shortcut (`db::SETTING_RESERVED_SHORTCUT`, in the form of `Shortcut::to_db`).
pub const MIGRATION_4: &str = "\
CREATE TABLE settings (
    key   TEXT PRIMARY KEY,  -- stable identifier in the code: 'reserved_shortcut'
    value TEXT NOT NULL      -- the setting, in the format of its key (keys.rs: Shortcut::to_db)
);
";

/// Migration 5 (NS-36): the current stage of each ticket and the history of its changes. The
/// existing tickets get `po` by the DEFAULT, and a new one is born there too. Unlike
/// `agents.state`, the stage has a CHECK: the five stages are the flow of auto_scrum, not a set
/// the F5 will grow; a new stage comes with a migration that rebuilds the column. The history
/// keeps only the changes (the creation is the `created_at` of the ticket, so a ticket never
/// changed has an empty history, "it started in PO"), without who changed it (a single
/// operator), and goes with its ticket (ON DELETE CASCADE), which goes with its project. Only
/// the operator changes the stage (`tickets::change_stage`); the nick never does it alone.
pub const MIGRATION_5: &str = "\
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
";

/// Migration 6 (NS-39): the agents can be resumed and archived. The states become `active`,
/// `stopped` (the `ended` of before: the claude ended or the nick closed, and the conversation can
/// be resumed) and `archived`; the column still has no CHECK (see the migration 3), and the
/// `AgentState` enum stays the only source of the states in the code (this literal is the
/// exception: a published migration is pinned by a test and never changes). `opened_key` is the
/// ticket key the agent was opened with, so a resume can tell the agent the folder of a ticket
/// whose ID changed; NULL for the agents opened before the NS-39, whose key comes from the end of
/// their name (`agents::opened_key`).
pub const MIGRATION_6: &str = "\
ALTER TABLE agents ADD COLUMN opened_key TEXT;  -- the ticket key at the opening; NULL before NS-39
UPDATE agents SET state = 'stopped' WHERE state = 'ended';
";

/// Migration 7 (NS-40): the optional epics, a level between the project and the ticket that holds
/// long conversations (only of PO and Tech Leader), with no stage. An epic has the key and the
/// title of a ticket, and hiding it (`hidden_at`) hides its whole branch without touching the
/// `hidden_at` of its tickets. A ticket goes into an epic by `tickets.epic_id` (NULL: no epic),
/// which has no ON DELETE: an epic is never deleted alone, only with its project, and then its
/// tickets go too (ON DELETE CASCADE from the project).
///
/// The table `agents` is rebuilt, because SQLite can't alter a NOT NULL, a CHECK nor a foreign
/// key: the owner of an agent is a ticket or an epic, never both and never none, and an agent of
/// an epic is only of PO or Tech Leader (`stages::EPIC_STAGES`). The rebuild works with the
/// foreign keys on, because no table points to `agents`; the `id` of every row is kept (the locks
/// are by `session_id`, but the tree and the tests use the `id`). The column `state` still has no
/// CHECK (see the migration 3): the `AgentState` enum stays its only source.
///
/// The triggers: the key is unique across the tickets and the epics of a project, because both
/// use `.nick/docs/<key>/`, and a UNIQUE does not cross two tables (the code checks it first, with
/// the message in PT-BR; the triggers close the race between two nicks, and their error becomes
/// the same message: `db::is_trigger_abort`). A ticket only goes into an epic of its own project.
pub const MIGRATION_7: &str = "\
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
";

/// Every migration, in order. The schema version of a fully migrated database is its length.
pub const MIGRATIONS: &[&str] = &[
    MIGRATION_1,
    MIGRATION_2,
    MIGRATION_3,
    MIGRATION_4,
    MIGRATION_5,
    MIGRATION_6,
    MIGRATION_7,
];
