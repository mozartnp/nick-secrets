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

/// Every migration, in order. The schema version of a fully migrated database is its length.
pub const MIGRATIONS: &[&str] = &[MIGRATION_1, MIGRATION_2];
