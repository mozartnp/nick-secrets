//! The rules of the agents of a ticket, with no screen (in the mold of tickets.rs): the stages,
//! the name given to `claude --name`, opening and ending the record of an agent, the lock that
//! proves its nick is alive, and the cleanup when the nick opens (ABOUT.md, "Rust no projeto:
//! agentes na casca `nick` (NS-33)").
//!
//! While an agent runs, its nick holds an exclusive lock (`flock`, through `File::lock`) on
//! `<state dir>/agents/<session_id>.lock`; the kernel releases it when the process dies, even
//! with `kill -9`. The lock is taken before the `active` row is inserted, so another nick never
//! sees an active row without the lock of a live agent. When the nick opens, an active agent
//! whose lock is free has lost its owner and becomes `ended`; one whose lock another live nick
//! holds stays `active`. The errors are the messages for the status bar, in PT-BR.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

pub use crate::db::Agent;

use crate::db::Db;
use crate::projects::db_error_message;

/// The directory of the locks, inside the state directory.
pub const AGENTS_DIR_NAME: &str = "agents";

/// The stage of an agent: the same values as the `$TYPE` of auto_scrum.sh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Po,
    TechLeader,
    Development,
    Review,
}

impl Stage {
    /// Every stage, in the order of the "Abrir agente" dialog.
    pub const ALL: [Stage; 4] = [
        Stage::Po,
        Stage::TechLeader,
        Stage::Development,
        Stage::Review,
    ];

    /// The key stored in `agents.stage`.
    pub fn key(self) -> &'static str {
        match self {
            Stage::Po => "po",
            Stage::TechLeader => "tech_leader",
            Stage::Development => "development",
            Stage::Review => "review",
        }
    }

    /// What the screen and the agent name show.
    pub fn label(self) -> &'static str {
        match self {
            Stage::Po => "PO",
            Stage::TechLeader => "Tech Leader",
            Stage::Development => "Dev",
            Stage::Review => "Review",
        }
    }

    /// The stage of a stored key.
    pub fn from_key(key: &str) -> Option<Stage> {
        Stage::ALL.into_iter().find(|stage| stage.key() == key)
    }
}

/// The state of an agent: the only writer of `agents.state` (the column has no CHECK, so the
/// F5 can add states without rebuilding the table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Active,
    Ended,
}

impl AgentState {
    /// The key stored in `agents.state`.
    pub fn key(self) -> &'static str {
        match self {
            AgentState::Active => "active",
            AgentState::Ended => "ended",
        }
    }
}

/// The `claude --name` of an agent: `<project> · <Stage> · <ticket ID>`.
pub fn agent_name(project: &str, stage: Stage, ticket_key: &str) -> String {
    format!("{project} · {} · {ticket_key}", stage.label())
}

/// The directory of the locks under the state directory.
pub fn agents_dir(state_dir: &Path) -> PathBuf {
    state_dir.join(AGENTS_DIR_NAME)
}

/// The lock file of the agent with `session_id`.
pub fn lock_path(agents_dir: &Path, session_id: &str) -> PathBuf {
    agents_dir.join(format!("{session_id}.lock"))
}

/// An agent being run by this nick: its row (`active`) and the lock it holds.
#[derive(Debug)]
pub struct OpenAgent {
    pub agent: Agent,
    lock: File,
    lock_path: PathBuf,
}

/// Opens the record of an agent: creates `agents_dir` (0700), takes the lock of `session_id`
/// and only then inserts the `active` row. On any failure nothing is left: no row and no lock.
pub fn open(
    db: &Db,
    agents_dir: &Path,
    ticket_id: i64,
    stage: Stage,
    session_id: &str,
    name: &str,
) -> Result<OpenAgent, String> {
    open_with(agents_dir, session_id, &mut || {
        db.insert_agent(ticket_id, stage.key(), session_id, name)
    })
}

/// `open` with the insert received by parameter (the tests check the lock at that moment).
pub fn open_with(
    agents_dir: &Path,
    session_id: &str,
    insert: &mut dyn FnMut() -> rusqlite::Result<Agent>,
) -> Result<OpenAgent, String> {
    let lock_path = lock_path(agents_dir, session_id);
    let lock_error = |err: io::Error| {
        format!(
            "Não consegui criar a trava do agente em {}: {err}.",
            lock_path.display()
        )
    };
    create_agents_dir(agents_dir).map_err(lock_error)?;
    let lock = open_lock_file(&lock_path).map_err(lock_error)?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            // Another nick runs an agent with this session id: not ours to remove.
            return Err(format!(
                "A trava {} já está em uso por outro nick.",
                lock_path.display()
            ));
        }
        Err(TryLockError::Error(err)) => {
            let _ = std::fs::remove_file(&lock_path);
            return Err(lock_error(err));
        }
    }
    match insert() {
        Ok(agent) => Ok(OpenAgent {
            agent,
            lock,
            lock_path,
        }),
        Err(err) => {
            release(lock, &lock_path);
            Err(db_error_message(&err))
        }
    }
}

/// Releases a lock this nick holds and removes its file (a failure to remove it is ignored).
/// The file goes first, while still locked, so another nick never takes a lock on a file that
/// is about to disappear.
fn release(lock: File, path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = lock.unlock();
}

/// Ends the record: the row becomes `ended`, the lock is released and its file removed (a
/// failure to remove it is ignored). The lock goes even when the database fails.
pub fn end(db: &Db, open: OpenAgent) -> Result<(), String> {
    let ended = db.end_agent(open.agent.id);
    release(open.lock, &open.lock_path);
    ended.map(|_| ()).map_err(|err| db_error_message(&err))
}

/// The cleanup when the nick opens: every `active` agent whose lock is free (its nick died)
/// becomes `ended`, and its lock file is removed; one whose lock another live nick holds stays
/// `active`; any other I/O error leaves it as it is (what can't be proven dead is not ended).
/// Returns how many were ended; the error is a database one, for the status bar.
pub fn cleanup(db: &Db, agents_dir: &Path) -> Result<usize, String> {
    let active = db
        .list_active_agents()
        .map_err(|err| db_error_message(&err))?;
    if active.is_empty() {
        return Ok(0);
    }
    if create_agents_dir(agents_dir).is_err() {
        // Without the directory, no lock can be tried: nothing is proven dead.
        return Ok(0);
    }
    let mut ended = 0;
    for agent in active {
        let path = lock_path(agents_dir, &agent.session_id);
        let Ok(lock) = open_lock_file(&path) else {
            continue;
        };
        match lock.try_lock() {
            Ok(()) => {
                let result = db.end_agent(agent.id);
                release(lock, &path);
                result.map_err(|err| db_error_message(&err))?;
                ended += 1;
            }
            // Another live nick holds it (WouldBlock), or the lock can't be tried: it stays.
            Err(TryLockError::WouldBlock | TryLockError::Error(_)) => {}
        }
    }
    Ok(ended)
}

/// Creates the directory of the locks (and its parents) with mode 0700.
fn create_agents_dir(dir: &Path) -> io::Result<()> {
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
}

/// Opens (creating it) the lock file at `path`.
fn open_lock_file(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    struct Fixture {
        _tmp: tempfile::TempDir,
        db: Db,
        dir: PathBuf,
        ticket: i64,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let p = db.insert_project("auto_scrum", "proj", "/proj").unwrap();
        let t = db.insert_ticket(p.id, "NS-1", false, "t").unwrap();
        let dir = agents_dir(tmp.path());
        Fixture {
            _tmp: tmp,
            db,
            dir,
            ticket: t.id,
        }
    }

    /// Whether another open of `path` can take the lock now (a flock on two opens of the same
    /// file conflicts even in the same process: this is how "another nick" is played here).
    fn lock_is_free(path: &Path) -> bool {
        let file = File::open(path).unwrap();
        match file.try_lock() {
            Ok(()) => true,
            Err(TryLockError::WouldBlock) => false,
            Err(TryLockError::Error(err)) => panic!("{err}"),
        }
    }

    fn states(db: &Db) -> Vec<(String, String)> {
        let mut stmt = db
            .connection()
            .prepare("SELECT session_id, state FROM agents ORDER BY id")
            .unwrap();
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
            .unwrap()
            .map(Result::unwrap)
            .collect()
    }

    fn pairs(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter()
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    #[test]
    fn the_stages_have_the_auto_scrum_keys_the_labels_and_the_dialog_order() {
        let keys: Vec<_> = Stage::ALL.iter().map(|s| s.key()).collect();
        assert_eq!(keys, ["po", "tech_leader", "development", "review"]);
        let labels: Vec<_> = Stage::ALL.iter().map(|s| s.label()).collect();
        assert_eq!(labels, ["PO", "Tech Leader", "Dev", "Review"]);
        for stage in Stage::ALL {
            assert_eq!(Stage::from_key(stage.key()), Some(stage));
        }
        assert_eq!(Stage::from_key("PO"), None);
        assert_eq!(Stage::from_key("po_jira"), None);
        assert_eq!(AgentState::Active.key(), "active");
        assert_eq!(AgentState::Ended.key(), "ended");
    }

    #[test]
    fn the_agent_name_is_project_stage_and_ticket() {
        assert_eq!(
            agent_name("proj", Stage::Development, "NS-32"),
            "proj · Dev · NS-32"
        );
        assert_eq!(
            agent_name("nick_secrets", Stage::TechLeader, "3f9a0c7e"),
            "nick_secrets · Tech Leader · 3f9a0c7e"
        );
    }

    #[test]
    fn opening_takes_the_lock_before_the_row_is_inserted() {
        let f = fixture();
        let lock = lock_path(&f.dir, "s-1");
        let mut held_at_insert = None;
        let open = open_with(&f.dir, "s-1", &mut || {
            held_at_insert = Some(!lock_is_free(&lock));
            f.db.insert_agent(f.ticket, "po", "s-1", "n")
        })
        .unwrap();
        assert_eq!(held_at_insert, Some(true), "the lock is held at the insert");
        assert_eq!(open.agent.state, "active");
        assert_eq!(states(&f.db), pairs(&[("s-1", "active")]));
        assert!(!lock_is_free(&lock), "still held after opening");
    }

    #[test]
    fn open_inserts_the_row_with_the_stage_and_the_name() {
        let f = fixture();
        let open = open(
            &f.db,
            &f.dir,
            f.ticket,
            Stage::Review,
            "s-1",
            "proj · Review · NS-1",
        )
        .unwrap();
        assert_eq!(open.agent.stage, "review");
        assert_eq!(open.agent.name, "proj · Review · NS-1");
        assert_eq!(open.agent.ticket_id, f.ticket);
        assert!(lock_path(&f.dir, "s-1").is_file());
    }

    #[test]
    fn the_agents_dir_is_created_with_mode_0700() {
        let f = fixture();
        assert!(!f.dir.exists());
        let _open = open(&f.db, &f.dir, f.ticket, Stage::Po, "s-1", "n").unwrap();
        let mode = std::fs::metadata(&f.dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn a_failing_insert_leaves_no_lock() {
        let f = fixture();
        // A session id already stored: the UNIQUE constraint refuses it.
        f.db.insert_agent(f.ticket, "po", "s-1", "n").unwrap();
        f.db.end_agent(1).unwrap();
        let err = open(&f.db, &f.dir, f.ticket, Stage::Po, "s-1", "n").unwrap_err();
        assert!(err.starts_with("Erro no banco do nick: "), "{err}");
        assert!(!lock_path(&f.dir, "s-1").exists(), "no lock file left");
        assert_eq!(states(&f.db), pairs(&[("s-1", "ended")]));
    }

    #[test]
    fn a_lock_held_by_another_nick_refuses_to_open() {
        let f = fixture();
        create_agents_dir(&f.dir).unwrap();
        let other = open_lock_file(&lock_path(&f.dir, "s-1")).unwrap();
        other.lock().unwrap();
        let err = open(&f.db, &f.dir, f.ticket, Stage::Po, "s-1", "n").unwrap_err();
        assert!(!err.is_empty());
        assert_eq!(states(&f.db), Vec::<(String, String)>::new(), "no row");
    }

    #[test]
    fn ending_marks_ended_releases_and_removes_the_lock() {
        let f = fixture();
        let open = open(&f.db, &f.dir, f.ticket, Stage::Po, "s-1", "n").unwrap();
        let lock = lock_path(&f.dir, "s-1");
        end(&f.db, open).unwrap();
        assert_eq!(states(&f.db), pairs(&[("s-1", "ended")]));
        assert!(!lock.exists(), "the lock file is gone");
        // Released: a new open of the same path takes it at once.
        let again = open_lock_file(&lock).unwrap();
        again.try_lock().unwrap();
    }

    #[test]
    fn ending_a_row_that_is_gone_still_releases_the_lock() {
        let f = fixture();
        let open = open(&f.db, &f.dir, f.ticket, Stage::Po, "s-1", "n").unwrap();
        f.db.connection().execute("DELETE FROM agents", []).unwrap();
        end(&f.db, open).unwrap();
        assert!(!lock_path(&f.dir, "s-1").exists());
    }

    #[test]
    fn cleanup_ends_an_active_row_without_a_lock_file() {
        let f = fixture();
        f.db.insert_agent(f.ticket, "po", "s-1", "n").unwrap();
        assert_eq!(cleanup(&f.db, &f.dir), Ok(1));
        assert_eq!(states(&f.db), pairs(&[("s-1", "ended")]));
        assert!(
            !lock_path(&f.dir, "s-1").exists(),
            "no lock file left behind"
        );
        let mode = std::fs::metadata(&f.dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700, "the dir is created with 0700");
    }

    #[test]
    fn cleanup_ends_an_active_row_whose_lock_is_free_and_removes_the_file() {
        let f = fixture();
        f.db.insert_agent(f.ticket, "po", "s-1", "n").unwrap();
        create_agents_dir(&f.dir).unwrap();
        // A file left by a nick that died (kill -9): nobody holds it.
        drop(open_lock_file(&lock_path(&f.dir, "s-1")).unwrap());
        assert_eq!(cleanup(&f.db, &f.dir), Ok(1));
        assert_eq!(states(&f.db), pairs(&[("s-1", "ended")]));
        assert!(!lock_path(&f.dir, "s-1").exists());
    }

    #[test]
    fn cleanup_keeps_an_agent_whose_lock_another_nick_holds() {
        let f = fixture();
        let held = open(&f.db, &f.dir, f.ticket, Stage::Po, "s-1", "n").unwrap();
        f.db.insert_agent(f.ticket, "review", "s-2", "n").unwrap();
        assert_eq!(cleanup(&f.db, &f.dir), Ok(1));
        assert_eq!(states(&f.db), pairs(&[("s-1", "active"), ("s-2", "ended")]));
        assert!(lock_path(&f.dir, "s-1").is_file(), "the live lock stays");
        end(&f.db, held).unwrap();
    }

    #[test]
    fn cleanup_does_not_touch_ended_rows() {
        let f = fixture();
        let a = f.db.insert_agent(f.ticket, "po", "s-1", "n").unwrap();
        f.db.end_agent(a.id).unwrap();
        create_agents_dir(&f.dir).unwrap();
        // A stray file of an ended agent is not this cleanup's business.
        drop(open_lock_file(&lock_path(&f.dir, "s-1")).unwrap());
        assert_eq!(cleanup(&f.db, &f.dir), Ok(0));
        assert_eq!(states(&f.db), pairs(&[("s-1", "ended")]));
        assert!(lock_path(&f.dir, "s-1").exists());
    }

    #[test]
    fn cleanup_keeps_an_agent_whose_lock_cant_be_opened() {
        let f = fixture();
        f.db.insert_agent(f.ticket, "po", "s-1", "n").unwrap();
        create_agents_dir(&f.dir).unwrap();
        // A directory in place of the lock file: an I/O error, not a proof of death.
        std::fs::create_dir(lock_path(&f.dir, "s-1")).unwrap();
        assert_eq!(cleanup(&f.db, &f.dir), Ok(0));
        assert_eq!(states(&f.db), pairs(&[("s-1", "active")]));
    }
}
