//! The rules of the agents of a ticket, with no screen (in the mold of tickets.rs): the stages,
//! the name given to `claude --name`, opening and ending the record of an agent, the lock that
//! proves its nick is alive, and the cleanup when the nick opens (ABOUT.md, "Rust no projeto:
//! agentes na casca `nick` (NS-33)").
//!
//! While an agent runs, its nick holds an exclusive lock (`flock`, through `File::lock`) on
//! `<state dir>/agents/<session_id>.lock`; the kernel releases it when the process dies, even
//! with `kill -9`. The lock is taken before the `active` row is inserted, so another nick never
//! sees an active row without the lock of a live agent. When the nick opens, an active agent
//! whose lock is free has lost its owner and becomes `stopped`; one whose lock another live nick
//! holds stays `active`. A stopped agent is resumed in the same conversation (NS-39: `resume`,
//! the lock taken again before the row goes back to `active`), archived and unarchived. The
//! errors are the messages for the status bar, in PT-BR.

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::os::unix::fs::DirBuilderExt;
use std::path::{Path, PathBuf};

pub use crate::db::{Agent, Owner};

use crate::db::{AgentChange, Db, Epic, Ticket};
use crate::projects::db_error_message;
use crate::stages::EPIC_STAGES;
use crate::tickets::jira_id_valid;

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

/// The state of an agent: the only source of the values of `agents.state` (the column has no
/// CHECK, so states come without rebuilding the table). Since the NS-39: `Active` (a nick runs
/// it), `Stopped` (its claude ended or its nick closed; the conversation can be resumed: the
/// `ended` of before) and `Archived` (out of the tree unless the filter `f` is on).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentState {
    Active,
    Stopped,
    Archived,
}

impl AgentState {
    /// Every state.
    pub const ALL: [AgentState; 3] = [
        AgentState::Active,
        AgentState::Stopped,
        AgentState::Archived,
    ];

    /// The key stored in `agents.state`.
    pub fn key(self) -> &'static str {
        match self {
            AgentState::Active => "active",
            AgentState::Stopped => "stopped",
            AgentState::Archived => "archived",
        }
    }

    /// The state of a stored key.
    pub fn from_key(key: &str) -> Option<AgentState> {
        AgentState::ALL.into_iter().find(|state| state.key() == key)
    }
}

/// Who holds the agents of a flow of "Abrir agente" or of a resume (NS-40): a ticket or an epic,
/// with what the flow needs of it (its owner, its project, its key and whether it is hidden). The
/// one way of opening and resuming an agent: there is no separate path for the epics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holder {
    pub owner: Owner,
    pub project_id: i64,
    pub key: String,
    /// What the dialogs show beside the key (a field the plan did not have: the dialogs of
    /// "Abrir agente" show `Ticket <ID> — <título>`, NS-35).
    pub title: String,
    pub hidden: bool,
}

impl From<&Ticket> for Holder {
    fn from(ticket: &Ticket) -> Holder {
        Holder {
            owner: Owner::Ticket(ticket.id),
            project_id: ticket.project_id,
            key: ticket.key.clone(),
            title: ticket.title.clone(),
            hidden: ticket.hidden_at.is_some(),
        }
    }
}

impl From<&Epic> for Holder {
    fn from(epic: &Epic) -> Holder {
        Holder {
            owner: Owner::Epic(epic.id),
            project_id: epic.project_id,
            key: epic.key.clone(),
            title: epic.title.clone(),
            hidden: epic.hidden_at.is_some(),
        }
    }
}

impl Holder {
    /// What the messages call it: `ticket` or `épico`.
    pub fn noun(&self) -> &'static str {
        match self.owner {
            Owner::Ticket(_) => "ticket",
            Owner::Epic(_) => "épico",
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

/// An agent being run by this nick: its row (`active`) and the lock it holds (opened by `open`,
/// or resumed by `resume`).
#[derive(Debug)]
pub struct OpenAgent {
    pub agent: Agent,
    lock: File,
    lock_path: PathBuf,
}

/// The refusal of opening an agent of another stage in an epic (NS-40).
pub const EPIC_STAGE_MESSAGE: &str = "O épico só tem agentes de PO e de Tech Leader.";

/// Opens the record of an agent of `owner` (a ticket, or an epic: NS-40): creates `agents_dir`
/// (0700), takes the lock of `session_id` and only then inserts the `active` row, with the key of
/// the owner it is opened with (`opened_key`, NS-39). An epic only has agents of
/// `stages::EPIC_STAGES`: another stage is refused before the lock. On any failure nothing is
/// left: no row and no lock.
pub fn open(
    db: &Db,
    agents_dir: &Path,
    owner: Owner,
    stage: Stage,
    session_id: &str,
    name: &str,
    opened_key: &str,
) -> Result<OpenAgent, String> {
    if matches!(owner, Owner::Epic(_)) && !EPIC_STAGES.contains(&stage) {
        return Err(EPIC_STAGE_MESSAGE.into());
    }
    open_with(agents_dir, session_id, &mut || {
        db.insert_agent(owner, stage.key(), session_id, name, opened_key)
    })
}

/// Why `take_lock` did not take the lock.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockError {
    /// Another live nick holds it (`WouldBlock`): not ours to remove.
    Held,
    /// The directory or the file could not be made, or the lock tried: the message.
    Failed(String),
}

/// Takes the lock of `session_id` (NS-39, the first half of `open_with`): creates `agents_dir`
/// (0700) and the lock file, and locks it. On a failure to lock, the file is removed (but not
/// when another nick holds it).
pub fn take_lock(agents_dir: &Path, session_id: &str) -> Result<(File, PathBuf), LockError> {
    let lock_path = lock_path(agents_dir, session_id);
    let lock_error = |err: io::Error| {
        LockError::Failed(format!(
            "Não consegui criar a trava do agente em {}: {err}.",
            lock_path.display()
        ))
    };
    create_agents_dir(agents_dir).map_err(lock_error)?;
    let lock = open_lock_file(&lock_path).map_err(lock_error)?;
    match lock.try_lock() {
        Ok(()) => Ok((lock, lock_path)),
        Err(TryLockError::WouldBlock) => Err(LockError::Held),
        Err(TryLockError::Error(err)) => {
            let _ = std::fs::remove_file(&lock_path);
            Err(lock_error(err))
        }
    }
}

/// `open` with the insert received by parameter (the tests check the lock at that moment).
pub fn open_with(
    agents_dir: &Path,
    session_id: &str,
    insert: &mut dyn FnMut() -> rusqlite::Result<Agent>,
) -> Result<OpenAgent, String> {
    let (lock, lock_path) = match take_lock(agents_dir, session_id) {
        Ok(taken) => taken,
        // Another nick runs an agent with this session id: not ours to remove.
        Err(LockError::Held) => {
            return Err(format!(
                "A trava {} já está em uso por outro nick.",
                lock_path(agents_dir, session_id).display()
            ));
        }
        Err(LockError::Failed(message)) => return Err(message),
    };
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

/// Resumes the record of a stopped agent (NS-39), the mold of `open`: takes its lock first (held
/// by another nick: refused, the row untouched), and only then the row goes back to `active`
/// (`Db::resume_agent`: only from `stopped`, or from an `active` row whose nick died, since its
/// lock was free). An agent gone or archived meanwhile is refused, and the lock goes. Returns the
/// row read again, with the lock.
pub fn resume(db: &Db, agents_dir: &Path, agent: &Agent) -> Result<OpenAgent, String> {
    resume_with(db, agents_dir, agent, &mut || db.resume_agent(agent.id))
}

/// `resume` with the update received by parameter (the tests check the lock at that moment).
pub fn resume_with(
    db: &Db,
    agents_dir: &Path,
    agent: &Agent,
    update: &mut dyn FnMut() -> rusqlite::Result<AgentChange>,
) -> Result<OpenAgent, String> {
    let (lock, lock_path) = match take_lock(agents_dir, &agent.session_id) {
        Ok(taken) => taken,
        Err(LockError::Held) => return Err(elsewhere_message(agent)),
        Err(LockError::Failed(message)) => return Err(message),
    };
    let refusal = match update() {
        Ok(AgentChange::Done) => match db.get_agent(agent.id) {
            Ok(Some(row)) => {
                return Ok(OpenAgent {
                    agent: row,
                    lock,
                    lock_path,
                });
            }
            // The row is `active` now: back to `stopped` before the lock goes, or it would look
            // alive in another nick until the next opening (review of the NS-39, point 1).
            Ok(None) => {
                let _ = db.stop_agent(agent.id);
                gone_message(agent)
            }
            Err(err) => {
                let _ = db.stop_agent(agent.id);
                db_error_message(&err)
            }
        },
        Ok(AgentChange::Archived) => format!(
            "O agente {} está arquivado. Desarquive (o) antes de retomar.",
            agent.name
        ),
        // Gone (the update takes a stopped and an active one: nothing else is left).
        Ok(_) => gone_message(agent),
        Err(err) => db_error_message(&err),
    };
    release(lock, &lock_path);
    Err(refusal)
}

/// The refusal of resuming an agent whose lock another live nick holds (decision 9 of NS-39).
pub fn elsewhere_message(agent: &Agent) -> String {
    format!(
        "O agente {} está ativo em outro nick e não pode ser retomado aqui.",
        agent.name
    )
}

/// The agent is no longer in the database.
fn gone_message(agent: &Agent) -> String {
    format!("O agente {} não existe mais no banco.", agent.name)
}

/// Whether another live nick holds the lock of `session_id` now: the lock is tried and released
/// at once (its file removed). Only to refuse early, before the `--check`, an `active` agent
/// without a session here; the guarantee is `resume`'s, which takes the lock for good. A lock
/// that can't be tried is not proven held (`resume` says why).
pub fn held_elsewhere(agents_dir: &Path, session_id: &str) -> bool {
    match take_lock(agents_dir, session_id) {
        Ok((lock, path)) => {
            release(lock, &path);
            false
        }
        Err(LockError::Held) => true,
        Err(LockError::Failed(_)) => false,
    }
}

/// Archives a stopped agent (NS-39): it leaves the tree, back with the filter `f`. An active one
/// (of this nick or another) is refused.
pub fn archive(db: &Db, agent: &Agent) -> Result<String, String> {
    match db.archive_agent(agent.id) {
        Ok(AgentChange::Done) => Ok(format!("O agente {} foi arquivado.", agent.name)),
        Ok(AgentChange::Active) => Err("Um agente ativo não pode ser arquivado.".into()),
        Ok(AgentChange::Archived) => Err(format!("O agente {} já está arquivado.", agent.name)),
        Ok(AgentChange::Gone | AgentChange::Stopped) => Err(gone_message(agent)),
        Err(err) => Err(db_error_message(&err)),
    }
}

/// Takes an archived agent back to stopped (NS-39).
pub fn unarchive(db: &Db, agent: &Agent) -> Result<String, String> {
    match db.unarchive_agent(agent.id) {
        Ok(AgentChange::Done) => Ok(format!("O agente {} voltou para parado.", agent.name)),
        Ok(AgentChange::Active) => Err(format!("O agente {} já está ativo.", agent.name)),
        Ok(AgentChange::Stopped) => Err(format!("O agente {} já está parado.", agent.name)),
        Ok(AgentChange::Gone | AgentChange::Archived) => Err(gone_message(agent)),
        Err(err) => Err(db_error_message(&err)),
    }
}

/// The ticket key the agent was opened with (decision 5 of NS-39): the `opened_key` column, or,
/// for an agent opened before the NS-39 (NULL), the last piece of its name after ` · ` (the
/// `agent_name` always ends with the key, which has no space: `jira_id_valid`), only when it is
/// a valid ID. None when it can't be known: then the resume says nothing of the ID.
pub fn opened_key(agent: &Agent) -> Option<String> {
    if let Some(key) = &agent.opened_key {
        return Some(key.clone());
    }
    let (_, last) = agent.name.rsplit_once(" · ")?;
    jira_id_valid(last).then(|| last.to_string())
}

/// Releases a lock this nick holds and removes its file (a failure to remove it is ignored).
/// The file goes first, while still locked, so another nick never takes a lock on a file that
/// is about to disappear.
fn release(lock: File, path: &Path) {
    let _ = std::fs::remove_file(path);
    let _ = lock.unlock();
}

/// Ends the record: the row becomes `stopped` (resumable, NS-39), the lock is released and its
/// file removed (a failure to remove it is ignored). The lock goes even when the database fails.
pub fn end(db: &Db, open: OpenAgent) -> Result<(), String> {
    let ended = db.stop_agent(open.agent.id);
    release(open.lock, &open.lock_path);
    ended.map(|_| ()).map_err(|err| db_error_message(&err))
}

/// The cleanup when the nick opens: every `active` agent whose lock is free (its nick died)
/// becomes `stopped`, and its lock file is removed; one whose lock another live nick holds stays
/// `active`; any other I/O error leaves it as it is (what can't be proven dead is not ended).
/// Returns how many were stopped; the error is a database one, for the status bar.
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
                let result = db.stop_agent(agent.id);
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
        let t = db.insert_ticket(p.id, "NS-1", false, "t", None).unwrap();
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
        assert_eq!(AgentState::Stopped.key(), "stopped");
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
            f.db.insert_agent(Owner::Ticket(f.ticket), "po", "s-1", "n", "NS-1")
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
            Owner::Ticket(f.ticket),
            Stage::Review,
            "s-1",
            "proj · Review · NS-1",
            "NS-1",
        )
        .unwrap();
        assert_eq!(open.agent.stage, "review");
        assert_eq!(open.agent.name, "proj · Review · NS-1");
        assert_eq!(open.agent.owner, Owner::Ticket(f.ticket));
        assert!(lock_path(&f.dir, "s-1").is_file());
    }

    #[test]
    fn the_agents_dir_is_created_with_mode_0700() {
        let f = fixture();
        assert!(!f.dir.exists());
        let _open = open(
            &f.db,
            &f.dir,
            Owner::Ticket(f.ticket),
            Stage::Po,
            "s-1",
            "n",
            "NS-1",
        )
        .unwrap();
        let mode = std::fs::metadata(&f.dir).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o700);
    }

    #[test]
    fn a_failing_insert_leaves_no_lock() {
        let f = fixture();
        // A session id already stored: the UNIQUE constraint refuses it.
        f.db.insert_agent(Owner::Ticket(f.ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        f.db.stop_agent(1).unwrap();
        let err = open(
            &f.db,
            &f.dir,
            Owner::Ticket(f.ticket),
            Stage::Po,
            "s-1",
            "n",
            "NS-1",
        )
        .unwrap_err();
        assert!(err.starts_with("Erro no banco do nick: "), "{err}");
        assert!(!lock_path(&f.dir, "s-1").exists(), "no lock file left");
        assert_eq!(states(&f.db), pairs(&[("s-1", "stopped")]));
    }

    #[test]
    fn a_lock_held_by_another_nick_refuses_to_open() {
        let f = fixture();
        create_agents_dir(&f.dir).unwrap();
        let other = open_lock_file(&lock_path(&f.dir, "s-1")).unwrap();
        other.lock().unwrap();
        let err = open(
            &f.db,
            &f.dir,
            Owner::Ticket(f.ticket),
            Stage::Po,
            "s-1",
            "n",
            "NS-1",
        )
        .unwrap_err();
        assert!(!err.is_empty());
        assert_eq!(states(&f.db), Vec::<(String, String)>::new(), "no row");
    }

    #[test]
    fn ending_marks_stopped_releases_and_removes_the_lock() {
        let f = fixture();
        let open = open(
            &f.db,
            &f.dir,
            Owner::Ticket(f.ticket),
            Stage::Po,
            "s-1",
            "n",
            "NS-1",
        )
        .unwrap();
        let lock = lock_path(&f.dir, "s-1");
        end(&f.db, open).unwrap();
        assert_eq!(states(&f.db), pairs(&[("s-1", "stopped")]));
        assert!(!lock.exists(), "the lock file is gone");
        // Released: a new open of the same path takes it at once.
        let again = open_lock_file(&lock).unwrap();
        again.try_lock().unwrap();
    }

    #[test]
    fn ending_a_row_that_is_gone_still_releases_the_lock() {
        let f = fixture();
        let open = open(
            &f.db,
            &f.dir,
            Owner::Ticket(f.ticket),
            Stage::Po,
            "s-1",
            "n",
            "NS-1",
        )
        .unwrap();
        f.db.connection().execute("DELETE FROM agents", []).unwrap();
        end(&f.db, open).unwrap();
        assert!(!lock_path(&f.dir, "s-1").exists());
    }

    #[test]
    fn cleanup_ends_an_active_row_without_a_lock_file() {
        let f = fixture();
        f.db.insert_agent(Owner::Ticket(f.ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        assert_eq!(cleanup(&f.db, &f.dir), Ok(1));
        assert_eq!(states(&f.db), pairs(&[("s-1", "stopped")]));
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
        f.db.insert_agent(Owner::Ticket(f.ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        create_agents_dir(&f.dir).unwrap();
        // A file left by a nick that died (kill -9): nobody holds it.
        drop(open_lock_file(&lock_path(&f.dir, "s-1")).unwrap());
        assert_eq!(cleanup(&f.db, &f.dir), Ok(1));
        assert_eq!(states(&f.db), pairs(&[("s-1", "stopped")]));
        assert!(!lock_path(&f.dir, "s-1").exists());
    }

    #[test]
    fn cleanup_keeps_an_agent_whose_lock_another_nick_holds() {
        let f = fixture();
        let held = open(
            &f.db,
            &f.dir,
            Owner::Ticket(f.ticket),
            Stage::Po,
            "s-1",
            "n",
            "NS-1",
        )
        .unwrap();
        f.db.insert_agent(Owner::Ticket(f.ticket), "review", "s-2", "n", "NS-1")
            .unwrap();
        assert_eq!(cleanup(&f.db, &f.dir), Ok(1));
        assert_eq!(
            states(&f.db),
            pairs(&[("s-1", "active"), ("s-2", "stopped")])
        );
        assert!(lock_path(&f.dir, "s-1").is_file(), "the live lock stays");
        end(&f.db, held).unwrap();
    }

    #[test]
    fn cleanup_does_not_touch_ended_rows() {
        let f = fixture();
        let a =
            f.db.insert_agent(Owner::Ticket(f.ticket), "po", "s-1", "n", "NS-1")
                .unwrap();
        f.db.stop_agent(a.id).unwrap();
        create_agents_dir(&f.dir).unwrap();
        // A stray file of an ended agent is not this cleanup's business.
        drop(open_lock_file(&lock_path(&f.dir, "s-1")).unwrap());
        assert_eq!(cleanup(&f.db, &f.dir), Ok(0));
        assert_eq!(states(&f.db), pairs(&[("s-1", "stopped")]));
        assert!(lock_path(&f.dir, "s-1").exists());
    }

    #[test]
    fn cleanup_keeps_an_agent_whose_lock_cant_be_opened() {
        let f = fixture();
        f.db.insert_agent(Owner::Ticket(f.ticket), "po", "s-1", "n", "NS-1")
            .unwrap();
        create_agents_dir(&f.dir).unwrap();
        // A directory in place of the lock file: an I/O error, not a proof of death.
        std::fs::create_dir(lock_path(&f.dir, "s-1")).unwrap();
        assert_eq!(cleanup(&f.db, &f.dir), Ok(0));
        assert_eq!(states(&f.db), pairs(&[("s-1", "active")]));
    }

    // ---- NS-39: the states, the resume, the archive and the key at the opening ----

    #[test]
    fn the_states_are_active_stopped_and_archived() {
        let keys: Vec<_> = AgentState::ALL.iter().map(|s| s.key()).collect();
        assert_eq!(keys, ["active", "stopped", "archived"]);
        for state in AgentState::ALL {
            assert_eq!(AgentState::from_key(state.key()), Some(state));
        }
        assert_eq!(
            AgentState::from_key("ended"),
            None,
            "the state of before the NS-39"
        );
        assert_eq!(AgentState::from_key("Active"), None);
    }

    /// The production code of `rel` (src/...), without the `mod tests` and the comments.
    fn production_code(rel: &str) -> String {
        let text =
            std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap();
        let code = text.split("#[cfg(test)]").next().unwrap();
        code.lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The literals of a state in `code`: `'active'`, `"stopped"`...
    fn state_literals(code: &str) -> Vec<String> {
        let mut found = Vec::new();
        for state in ["active", "ended", "stopped", "archived"] {
            for quote in ['\'', '"'] {
                let literal = format!("{quote}{state}{quote}");
                if code.contains(&literal) {
                    found.push(literal);
                }
            }
        }
        found
    }

    /// The states come only from the enum (its `key()`): no literal in the production code of
    /// db/mod.rs nor of agents.rs but the one of `AgentState::key` (the migrations are pinned
    /// apart, db/migrations.rs).
    #[test]
    fn no_state_literal_outside_the_enum() {
        assert_eq!(
            state_literals(&production_code("src/db/mod.rs")),
            Vec::<String>::new()
        );
        let agents = production_code("src/agents.rs");
        let (before, rest) = agents.split_once("impl AgentState {").unwrap();
        let (key_fn, after) = rest.split_once("pub fn from_key").unwrap();
        assert_eq!(
            state_literals(&format!("{before}{after}")),
            Vec::<String>::new()
        );
        // Positive controls: the filter finds the literals of the enum and a quoted one.
        assert_eq!(
            state_literals(key_fn),
            ["\"active\"", "\"stopped\"", "\"archived\""]
        );
        assert_eq!(
            state_literals("UPDATE agents SET state = 'ended'"),
            ["'ended'"]
        );
    }

    /// A lock held by another nick: another open of the file, locked (a flock on two opens of
    /// the same file conflicts even in the same process).
    fn held_by_another(f: &Fixture, session: &str) -> File {
        create_agents_dir(&f.dir).unwrap();
        let other = open_lock_file(&lock_path(&f.dir, session)).unwrap();
        other.lock().unwrap();
        other
    }

    fn stopped(f: &Fixture, session: &str) -> Agent {
        let agent =
            f.db.insert_agent(
                Owner::Ticket(f.ticket),
                "po",
                session,
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        f.db.stop_agent(agent.id).unwrap();
        f.db.get_agent(agent.id).unwrap().unwrap()
    }

    #[test]
    fn resuming_takes_the_lock_before_the_update() {
        let f = fixture();
        let agent = stopped(&f, "s-1");
        let lock = lock_path(&f.dir, "s-1");
        let mut held_at_update = None;
        let open = resume_with(&f.db, &f.dir, &agent, &mut || {
            held_at_update = Some(!lock_is_free(&lock));
            f.db.resume_agent(agent.id)
        })
        .unwrap();
        assert_eq!(held_at_update, Some(true), "the lock is held at the update");
        assert_eq!(open.agent.state, "active", "the row read again");
        assert_eq!(open.agent.id, agent.id);
        assert_eq!(states(&f.db), pairs(&[("s-1", "active")]));
        assert!(!lock_is_free(&lock), "still held after resuming");
        end(&f.db, open).unwrap();
        assert_eq!(states(&f.db), pairs(&[("s-1", "stopped")]));
    }

    #[test]
    fn a_lock_held_by_another_nick_refuses_to_resume_and_changes_nothing() {
        let f = fixture();
        let agent =
            f.db.insert_agent(
                Owner::Ticket(f.ticket),
                "po",
                "s-1",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        let _other = held_by_another(&f, "s-1");
        let err = resume(&f.db, &f.dir, &agent).unwrap_err();
        assert_eq!(
            err,
            "O agente proj · PO · NS-1 está ativo em outro nick e não pode ser retomado aqui."
        );
        assert_eq!(states(&f.db), pairs(&[("s-1", "active")]));
        assert!(lock_path(&f.dir, "s-1").exists(), "not ours to remove");
    }

    #[test]
    fn an_active_agent_whose_lock_is_free_is_resumed() {
        let f = fixture();
        let agent =
            f.db.insert_agent(
                Owner::Ticket(f.ticket),
                "po",
                "s-1",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        let open = resume(&f.db, &f.dir, &agent).unwrap();
        assert_eq!(open.agent.state, "active");
        assert!(!lock_is_free(&lock_path(&f.dir, "s-1")));
    }

    #[test]
    fn a_gone_or_archived_agent_is_not_resumed_and_the_lock_goes() {
        let f = fixture();
        let agent = stopped(&f, "s-1");
        f.db.archive_agent(agent.id).unwrap();
        let err = resume(&f.db, &f.dir, &agent).unwrap_err();
        assert_eq!(
            err,
            "O agente proj · PO · NS-1 está arquivado. Desarquive (o) antes de retomar."
        );
        assert!(!lock_path(&f.dir, "s-1").exists(), "the lock went");
        assert_eq!(states(&f.db), pairs(&[("s-1", "archived")]));
        f.db.connection().execute("DELETE FROM agents", []).unwrap();
        let err = resume(&f.db, &f.dir, &agent).unwrap_err();
        assert_eq!(err, "O agente proj · PO · NS-1 não existe mais no banco.");
        assert!(!lock_path(&f.dir, "s-1").exists(), "the lock went");
    }

    #[test]
    fn held_elsewhere_says_whether_another_nick_holds_the_lock() {
        let f = fixture();
        assert!(!held_elsewhere(&f.dir, "s-1"), "no lock at all");
        assert!(!lock_path(&f.dir, "s-1").exists(), "nothing left behind");
        let other = held_by_another(&f, "s-1");
        assert!(held_elsewhere(&f.dir, "s-1"));
        drop(other);
        assert!(
            !held_elsewhere(&f.dir, "s-1"),
            "released when its nick dies"
        );
    }

    #[test]
    fn archiving_and_unarchiving_say_what_happened() {
        let f = fixture();
        let agent = stopped(&f, "s-1");
        assert_eq!(
            archive(&f.db, &agent),
            Ok("O agente proj · PO · NS-1 foi arquivado.".to_string())
        );
        assert_eq!(states(&f.db), pairs(&[("s-1", "archived")]));
        assert_eq!(
            unarchive(&f.db, &agent),
            Ok("O agente proj · PO · NS-1 voltou para parado.".to_string())
        );
        assert_eq!(states(&f.db), pairs(&[("s-1", "stopped")]));
        let active =
            f.db.insert_agent(
                Owner::Ticket(f.ticket),
                "po",
                "s-2",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        assert_eq!(
            archive(&f.db, &active),
            Err("Um agente ativo não pode ser arquivado.".to_string())
        );
        f.db.connection().execute("DELETE FROM agents", []).unwrap();
        let gone = Err("O agente proj · PO · NS-1 não existe mais no banco.".to_string());
        assert_eq!(archive(&f.db, &agent), gone);
        assert_eq!(unarchive(&f.db, &agent), gone);
    }

    fn named(name: &str, key: Option<&str>) -> Agent {
        Agent {
            id: 1,
            owner: Owner::Ticket(1),
            stage: "po".into(),
            session_id: "s".into(),
            name: name.into(),
            opened_at: "2026-11-20T10:00:00Z".into(),
            state: "stopped".into(),
            opened_key: key.map(str::to_string),
        }
    }

    #[test]
    fn the_key_at_the_opening_is_the_column_or_the_end_of_the_name() {
        assert_eq!(
            opened_key(&named("proj · PO · NS-1", Some("NS-7"))),
            Some("NS-7".to_string()),
            "the column wins"
        );
        assert_eq!(
            opened_key(&named("proj · Tech Leader · 3f9a0c7e", None)),
            Some("3f9a0c7e".to_string()),
            "before the NS-39: the end of the name"
        );
        assert_eq!(
            opened_key(&named("meu · projeto · Dev · SC-12", None)),
            Some("SC-12".to_string())
        );
        assert_eq!(opened_key(&named("sem separador", None)), None);
        assert_eq!(
            opened_key(&named("proj · PO · ../x", None)),
            None,
            "not a valid ID"
        );
        assert_eq!(opened_key(&named("proj · PO · ", None)), None);
    }

    /// Review of the NS-39, point 1: the row became `active`, but reading it again failed; the
    /// resume is refused, and the row goes back to `stopped` before the lock goes (an `active`
    /// row without its lock would look alive in another nick until the next opening).
    #[test]
    fn a_resume_whose_row_cant_be_read_again_stops_it_and_releases_the_lock() {
        let f = fixture();
        let agent = stopped(&f, "s-1");
        let err = resume_with(&f.db, &f.dir, &agent, &mut || {
            let change = f.db.resume_agent(agent.id);
            // The SELECT of the row fails after the UPDATE (a column it reads is gone).
            f.db.connection()
                .execute_batch("ALTER TABLE agents RENAME COLUMN opened_key TO gone")
                .unwrap();
            change
        })
        .unwrap_err();
        assert!(err.starts_with("Erro no banco do nick: "), "{err}");
        assert_eq!(states(&f.db), pairs(&[("s-1", "stopped")]));
        assert!(!lock_path(&f.dir, "s-1").exists(), "the lock went");
    }

    /// Review of the NS-39, point 3: unarchiving an active agent says it is active (and not
    /// "can't be archived").
    #[test]
    fn unarchiving_an_active_agent_says_it_is_active() {
        let f = fixture();
        let active =
            f.db.insert_agent(
                Owner::Ticket(f.ticket),
                "po",
                "s-1",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        assert_eq!(
            unarchive(&f.db, &active),
            Err("O agente proj · PO · NS-1 já está ativo.".to_string())
        );
        assert_eq!(states(&f.db), pairs(&[("s-1", "active")]));
    }

    // ---- the agents of an epic (NS-40) ----

    #[test]
    fn an_epic_has_agents_of_po_and_tech_leader_only() {
        let f = fixture();
        let project = f.db.get_ticket(f.ticket).unwrap().unwrap().project_id;
        let epic = f.db.insert_epic(project, "EPIC-1", false, "e").unwrap();
        for (i, stage) in [Stage::Po, Stage::TechLeader].into_iter().enumerate() {
            let session = format!("ok-{i}");
            let name = agent_name("proj", stage, "EPIC-1");
            let open = open(
                &f.db,
                &f.dir,
                Owner::Epic(epic.id),
                stage,
                &session,
                &name,
                "EPIC-1",
            )
            .unwrap();
            assert_eq!(open.agent.owner, Owner::Epic(epic.id));
            assert_eq!(open.agent.opened_key.as_deref(), Some("EPIC-1"));
            assert!(lock_path(&f.dir, &session).is_file());
            end(&f.db, open).unwrap();
        }
        assert_eq!(
            agent_name("proj", Stage::TechLeader, "EPIC-1"),
            "proj · Tech Leader · EPIC-1"
        );
        for stage in [Stage::Development, Stage::Review] {
            let err = open(
                &f.db,
                &f.dir,
                Owner::Epic(epic.id),
                stage,
                "no",
                "n",
                "EPIC-1",
            )
            .unwrap_err();
            assert_eq!(err, EPIC_STAGE_MESSAGE);
            assert_eq!(err, "O épico só tem agentes de PO e de Tech Leader.");
            assert!(!lock_path(&f.dir, "no").exists(), "the lock is not touched");
        }
        assert_eq!(f.db.list_project_agents(project).unwrap().len(), 2);
        // A ticket takes every stage, as before.
        let dev = open(
            &f.db,
            &f.dir,
            Owner::Ticket(f.ticket),
            Stage::Development,
            "d",
            "n",
            "NS-1",
        );
        assert!(dev.is_ok());
    }

    #[test]
    fn a_holder_is_built_from_a_ticket_or_an_epic() {
        let f = fixture();
        let mut ticket = f.db.get_ticket(f.ticket).unwrap().unwrap();
        let epic =
            f.db.insert_epic(ticket.project_id, "E", false, "um épico")
                .unwrap();
        let from_ticket = Holder::from(&ticket);
        assert_eq!(from_ticket.owner, Owner::Ticket(ticket.id));
        assert_eq!(
            (from_ticket.key.as_str(), from_ticket.title.as_str()),
            ("NS-1", "t")
        );
        assert_eq!(from_ticket.project_id, ticket.project_id);
        assert!(!from_ticket.hidden);
        assert_eq!(from_ticket.noun(), "ticket");
        ticket.hidden_at = Some("x".into());
        assert!(Holder::from(&ticket).hidden);
        let from_epic = Holder::from(&epic);
        assert_eq!(from_epic.owner, Owner::Epic(epic.id));
        assert_eq!(
            (from_epic.key.as_str(), from_epic.title.as_str()),
            ("E", "um épico")
        );
        assert_eq!(from_epic.noun(), "épico");
    }
}
