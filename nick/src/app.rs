//! The state of the TUI and its transitions by key, with no terminal I/O: `handle_key` takes a
//! key and returns an `Action`, which the loop (main.rs) carries out. The only action that runs
//! bin/nick-init is `Action::RunInit`, and only the "Criar configuração" button of the
//! missing-config alert produces it (plan D7 of NS-31). The navigation is Modules > Projects >
//! Tickets; the rules of the tickets are in tickets.rs, and none of them touches a file of the
//! project. From a ticket, `g` and a stage open an agent (NS-33): `Action::OpenAgent`, which
//! the loop hands to `open_agent` with the real `AgentLauncher` (launch.rs); on the agent
//! screen every key goes to the program (`Action::Send`). The screen texts are in PT-BR, and
//! drawing is ui.rs's job.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

use crate::agents::{self, OpenAgent, Stage};
use crate::db::Db;
use crate::init::{InitOutcome, InitRunner};
use crate::keys::{self, TermModes};
use crate::launch::{self, AgentLauncher};
use crate::modules::ModuleKind;
use crate::projects::{self, Availability, Check, Project};
use crate::tickets::{self, Ticket};

/// The list on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Modules,
    Projects,
    /// The tickets of the open project (`App::project`).
    Tickets,
    /// The terminal of the running agent (`App::agent`), over the whole screen.
    Agent,
}

/// The buttons of the missing-config alert.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    CreateConfig,
    Cancel,
}

/// The fields of the new ticket dialog.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TicketField {
    Title,
    JiraId,
}

/// A dialog over the list. While one is open, `q` does not quit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    /// The text field of a new project's path.
    AddPath { input: String },
    /// The directory has no `.nick/nick.conf` (plan D6). `note` is why the last attempt did
    /// not register it.
    NeedsConfig {
        dir: PathBuf,
        focus: Button,
        note: Option<String>,
    },
    /// The `(s/N)` confirmation of a removal, with how many tickets go with the project.
    ConfirmRemove { project: Project, tickets: i64 },
    /// A new ticket: the title (required) and the Jira ID (optional; empty generates a
    /// provisional one).
    NewTicket {
        title: String,
        jira_id: String,
        focus: TicketField,
    },
    /// The title of a ticket (its ID is not editable).
    EditTitle { ticket: Ticket, input: String },
    /// The `(s/N)` confirmation of hiding a ticket.
    ConfirmHide { ticket: Ticket },
    /// "Abrir agente": the stage of an agent for the ticket of the project at `project`
    /// (`index` in `Stage::ALL`).
    ChooseStage {
        project: PathBuf,
        ticket: Ticket,
        index: usize,
    },
    /// Why no agent was opened (the launcher is missing, or its check failed).
    AgentError { message: String },
}

/// What the loop must do after a key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    /// Run the init with the cwd at this directory (only from the button).
    RunInit {
        project: PathBuf,
    },
    /// Open an agent of `stage` for `ticket`, with the cwd at `project` (the path in the
    /// database); the loop calls `open_agent`.
    OpenAgent {
        project: PathBuf,
        ticket: Ticket,
        stage: Stage,
    },
    /// Write these bytes to the agent's terminal.
    Send(Vec<u8>),
    /// Scroll the agent's history one page up (into the history) or down.
    ScrollPageUp,
    ScrollPageDown,
    /// Scroll the agent's history by lines (positive: up, into the history).
    ScrollLines(i32),
}

/// Lines scrolled by one step of the mouse wheel, when the wheel scrolls the history.
pub const WHEEL_LINES: i32 = 3;

/// What the agent's program asked of its terminal, read from its screen by the loop before each
/// input event: how keys, pastes and the mouse are encoded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct AgentTerm {
    pub modes: TermModes,
    pub bracketed_paste: bool,
    pub mouse_mode: MouseProtocolMode,
    pub mouse_encoding: MouseProtocolEncoding,
}

/// The agent being run: its record (the active row and the lock) and what the screen shows.
#[derive(Debug)]
pub struct RunningAgent {
    record: OpenAgent,
    /// The `claude --name`.
    pub name: String,
    /// Whether it runs in the jail (the `jail=` of the check).
    pub jail: bool,
    /// What the check wrote on stderr, shown when the agent ends.
    warning: Option<String>,
}

/// A project of the list with its availability, computed when the list is refreshed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    pub project: Project,
    pub availability: Availability,
}

/// The state of the TUI.
pub struct App {
    db: Db,
    home: Option<PathBuf>,
    cwd: Option<PathBuf>,
    init: PathBuf,
    pub(crate) screen: Screen,
    pub(crate) module_index: usize,
    pub(crate) project_index: usize,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) status: Option<String>,
    pub(crate) rows: Vec<Row>,
    /// The project whose tickets are on the Tickets screen.
    pub(crate) project: Option<Project>,
    pub(crate) ticket_index: usize,
    pub(crate) ticket_rows: Vec<Ticket>,
    /// The filter of the Tickets screen: the hidden ones are listed too.
    pub(crate) show_hidden: bool,
    /// bin/nick-agent (launch.rs).
    pub(crate) launcher: PathBuf,
    /// Where the locks of the agents live (agents.rs); None until `with_agents`.
    pub(crate) agents_dir: Option<PathBuf>,
    /// The agent on the Agent screen.
    pub(crate) agent: Option<RunningAgent>,
    pub(crate) agent_term: AgentTerm,
}

impl App {
    /// A new TUI on the Modules screen. `home` expands `~` and `cwd` resolves a relative path
    /// typed by the operator; `init` is the bin/nick-init to run from the button.
    pub fn new(db: Db, home: Option<PathBuf>, cwd: Option<PathBuf>, init: PathBuf) -> App {
        App {
            db,
            home,
            cwd,
            init,
            screen: Screen::Modules,
            module_index: 0,
            project_index: 0,
            dialog: None,
            status: None,
            rows: Vec::new(),
            project: None,
            ticket_index: 0,
            ticket_rows: Vec::new(),
            show_hidden: false,
            launcher: launch::launcher_path(),
            agents_dir: None,
            agent: None,
            agent_term: AgentTerm::default(),
        }
    }

    /// The agents: `launcher` is the bin/nick-agent to run, and `agents_dir` the directory of
    /// their locks (in the state directory).
    pub fn with_agents(mut self, launcher: PathBuf, agents_dir: PathBuf) -> App {
        self.launcher = launcher;
        self.agents_dir = Some(agents_dir);
        self
    }

    /// The screen on now.
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The running agent, if any.
    pub fn agent(&self) -> Option<&RunningAgent> {
        self.agent.as_ref()
    }

    /// Sets what the agent's program asked of its terminal (the loop reads it from the screen).
    pub fn set_agent_term(&mut self, term: AgentTerm) {
        self.agent_term = term;
    }

    /// A message for the status bar (e.g. an error of the cleanup when the nick opens).
    pub fn set_status(&mut self, message: String) {
        self.status = Some(message);
    }

    /// The module of the Projects screen (the one selected on the Modules screen).
    pub fn module(&self) -> ModuleKind {
        ModuleKind::ALL[self.module_index]
    }

    /// The init the button runs.
    pub fn init_path(&self) -> &Path {
        &self.init
    }

    /// Handles a key and runs the init when the key asks for it. Returns true to quit.
    pub fn on_key(&mut self, key: KeyEvent, runner: &mut dyn InitRunner) -> bool {
        match self.handle_key(key) {
            Action::Quit => true,
            Action::RunInit { project } => {
                let outcome = runner.run(&self.init, &project);
                self.after_init(outcome);
                false
            }
            // The agents are carried out by the loop (main.rs).
            Action::None
            | Action::OpenAgent { .. }
            | Action::Send(_)
            | Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ScrollLines(_) => false,
        }
    }

    /// The transition of a key.
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        // On the agent screen every key is the program's, Ctrl+C included: the terminal only
        // closes when claude ends (the reserved shortcut is the F3b's).
        if self.screen == Screen::Agent {
            return self.key_agent(key);
        }
        // Ctrl+C quits from anywhere: raw mode delivers it as a key, and the operator can't
        // be stuck.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return Action::Quit;
        }
        let action = match self.dialog.take() {
            Some(Dialog::AddPath { input }) => self.key_add_path(key, input),
            Some(Dialog::NeedsConfig { dir, focus, note }) => {
                self.key_needs_config(key, dir, focus, note)
            }
            Some(Dialog::ConfirmRemove { project, .. }) => self.key_confirm_remove(key, project),
            Some(Dialog::NewTicket {
                title,
                jira_id,
                focus,
            }) => self.key_new_ticket(key, title, jira_id, focus),
            Some(Dialog::EditTitle { ticket, input }) => self.key_edit_title(key, ticket, input),
            Some(Dialog::ConfirmHide { ticket }) => self.key_confirm_hide(key, ticket),
            Some(Dialog::ChooseStage {
                project,
                ticket,
                index,
            }) => self.key_choose_stage(key, project, ticket, index),
            Some(Dialog::AgentError { message }) => self.key_agent_error(key, message),
            None => match self.screen {
                Screen::Modules => self.key_modules(key),
                Screen::Projects => self.key_projects(key),
                Screen::Tickets => self.key_tickets(key),
                Screen::Agent => Action::None,
            },
        };
        // The availability is computed again after every action on the list (plan D9): not
        // while a dialog stays open (typing a path is not an action, and each refresh reads
        // the disk), but when it closes, whatever closed it. The tickets are read again the
        // same way.
        if self.dialog.is_none() {
            match self.screen {
                Screen::Projects => self.refresh(),
                Screen::Tickets => self.refresh_tickets(),
                Screen::Modules | Screen::Agent => {}
            }
        }
        action
    }

    fn key_modules(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.module_index = self.module_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.module_index = (self.module_index + 1).min(ModuleKind::ALL.len() - 1);
            }
            KeyCode::Enter | KeyCode::Right => {
                self.screen = Screen::Projects;
                self.project_index = 0;
                self.status = None;
            }
            _ if is_quit(key) => return Action::Quit,
            _ => {}
        }
        Action::None
    }

    fn key_projects(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.project_index = self.project_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.project_index =
                    (self.project_index + 1).min(self.rows.len().saturating_sub(1));
            }
            KeyCode::Char('a') => {
                self.dialog = Some(Dialog::AddPath {
                    input: String::new(),
                });
            }
            KeyCode::Char('r') => {
                if let Some(row) = self.rows.get(self.project_index) {
                    match self.db.count_tickets(row.project.id) {
                        Ok(tickets) => {
                            self.dialog = Some(Dialog::ConfirmRemove {
                                project: row.project.clone(),
                                tickets,
                            });
                        }
                        Err(err) => self.status = Some(projects::db_error_message(&err)),
                    }
                }
            }
            KeyCode::Enter | KeyCode::Right => self.open_tickets(),
            KeyCode::Esc | KeyCode::Left => {
                self.screen = Screen::Modules;
                self.status = None;
            }
            _ if is_quit(key) => return Action::Quit,
            _ => {}
        }
        Action::None
    }

    /// Opens the Tickets screen of the selected project, only if it is available now.
    fn open_tickets(&mut self) {
        let Some(row) = self.rows.get(self.project_index) else {
            return;
        };
        let availability = projects::availability(&row.project.path);
        if let Some(mark) = projects::unavailable_text(availability) {
            self.status = Some(format!(
                "Projeto indisponível: {mark}. Os tickets só abrem com a pasta e o \
                 .nick/nick.conf no lugar."
            ));
            return;
        }
        self.project = Some(row.project.clone());
        self.screen = Screen::Tickets;
        self.ticket_index = 0;
        self.ticket_rows.clear();
        self.show_hidden = false;
        self.status = None;
    }

    fn key_tickets(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.ticket_index = self.ticket_index.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.ticket_index =
                    (self.ticket_index + 1).min(self.ticket_rows.len().saturating_sub(1));
            }
            KeyCode::Char('a') => {
                self.dialog = Some(Dialog::NewTicket {
                    title: String::new(),
                    jira_id: String::new(),
                    focus: TicketField::Title,
                });
            }
            KeyCode::Char('e') => {
                if let Some(ticket) = self.ticket_rows.get(self.ticket_index) {
                    self.dialog = Some(Dialog::EditTitle {
                        input: ticket.title.clone(),
                        ticket: ticket.clone(),
                    });
                }
            }
            KeyCode::Char('o') => {
                if let Some(ticket) = self.ticket_rows.get(self.ticket_index).cloned() {
                    if ticket.hidden_at.is_some() {
                        // Restoring hides nothing and is undone with `o`: no confirmation.
                        self.status = Some(match tickets::restore(&self.db, &ticket) {
                            Ok(message) | Err(message) => message,
                        });
                    } else {
                        self.dialog = Some(Dialog::ConfirmHide { ticket });
                    }
                }
            }
            KeyCode::Char('f') => self.show_hidden = !self.show_hidden,
            KeyCode::Char('g') => self.choose_stage(),
            KeyCode::Esc | KeyCode::Left => {
                self.screen = Screen::Projects;
                self.project = None;
                self.ticket_rows.clear();
                self.show_hidden = false;
                self.status = None;
            }
            _ if is_quit(key) => return Action::Quit,
            _ => {}
        }
        Action::None
    }

    fn key_new_ticket(
        &mut self,
        key: KeyEvent,
        mut title: String,
        mut jira_id: String,
        mut focus: TicketField,
    ) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.status = None;
                return Action::None;
            }
            KeyCode::Enter => {
                let Some(project_id) = self.project.as_ref().map(|p| p.id) else {
                    return Action::None;
                };
                match tickets::create(&self.db, project_id, &title, &jira_id) {
                    Ok(ticket) => {
                        self.status = Some(tickets::created_message(&ticket));
                        // Select the new ticket: the refresh after the dialog closes reads the
                        // list again and keeps the selection by id.
                        self.ticket_rows.push(ticket);
                        self.ticket_index = self.ticket_rows.len() - 1;
                        return Action::None;
                    }
                    Err(message) => self.status = Some(message),
                }
            }
            KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                focus = match focus {
                    TicketField::Title => TicketField::JiraId,
                    TicketField::JiraId => TicketField::Title,
                };
            }
            _ => {
                let field = match focus {
                    TicketField::Title => &mut title,
                    TicketField::JiraId => &mut jira_id,
                };
                edit_text(field, key);
            }
        }
        self.dialog = Some(Dialog::NewTicket {
            title,
            jira_id,
            focus,
        });
        Action::None
    }

    fn key_edit_title(&mut self, key: KeyEvent, ticket: Ticket, mut input: String) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.status = None;
                return Action::None;
            }
            KeyCode::Enter => match tickets::rename(&self.db, &ticket, &input) {
                Ok(message) => {
                    self.status = Some(message);
                    return Action::None;
                }
                Err(message) => self.status = Some(message),
            },
            _ => {
                edit_text(&mut input, key);
            }
        }
        self.dialog = Some(Dialog::EditTitle { ticket, input });
        Action::None
    }

    fn key_confirm_hide(&mut self, key: KeyEvent, ticket: Ticket) -> Action {
        // Only s/S confirm, as in the removal; any other key cancels.
        if matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S')) {
            self.status = Some(match tickets::hide(&self.db, &ticket) {
                Ok(message) | Err(message) => message,
            });
        }
        Action::None
    }

    /// `g` on a ticket: the stage dialog, only for a visible ticket of a project available now
    /// (read from the disk at the key, like the Enter on a project).
    fn choose_stage(&mut self) {
        let Some(ticket) = self.ticket_rows.get(self.ticket_index).cloned() else {
            return;
        };
        if ticket.hidden_at.is_some() {
            self.status = Some(format!(
                "O ticket {} está oculto. Restaure-o (o) para abrir um agente.",
                ticket.key
            ));
            return;
        }
        let Some(project) = self.project.as_ref() else {
            return;
        };
        if let Some(mark) = projects::unavailable_text(projects::availability(&project.path)) {
            self.status = Some(format!(
                "Projeto indisponível: {mark}. O agente só abre com a pasta e o \
                 .nick/nick.conf no lugar."
            ));
            return;
        }
        self.dialog = Some(Dialog::ChooseStage {
            project: project.path.clone(),
            ticket,
            index: 0,
        });
    }

    fn key_choose_stage(
        &mut self,
        key: KeyEvent,
        project: PathBuf,
        ticket: Ticket,
        mut index: usize,
    ) -> Action {
        match key.code {
            KeyCode::Esc => return Action::None,
            KeyCode::Enter => {
                return Action::OpenAgent {
                    project,
                    ticket,
                    stage: Stage::ALL[index],
                };
            }
            KeyCode::Up | KeyCode::Char('k') => index = index.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => index = (index + 1).min(Stage::ALL.len() - 1),
            _ => {}
        }
        self.dialog = Some(Dialog::ChooseStage {
            project,
            ticket,
            index,
        });
        Action::None
    }

    fn key_agent_error(&mut self, key: KeyEvent, message: String) -> Action {
        if !matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            self.dialog = Some(Dialog::AgentError { message });
        }
        Action::None
    }

    /// Opens an agent of `stage` for `ticket` with the cwd at `project` (plan D9): checks that
    /// the launcher exists, runs its `--check` (a failure becomes the alert, and nothing is
    /// stored nor launched), then generates the session ID and the name, takes the lock and
    /// stores the `active` row (agents.rs), and only then starts the PTY. A failing start ends
    /// the row and releases the lock.
    pub fn open_agent(
        &mut self,
        project: &Path,
        ticket: &Ticket,
        stage: Stage,
        launcher: &mut dyn AgentLauncher,
    ) {
        if !self.launcher.is_file() {
            self.dialog = Some(Dialog::AgentError {
                message: format!(
                    "Não encontrei o bin/nick-agent em {}. Nada foi aberto.",
                    self.launcher.display()
                ),
            });
            return;
        }
        let check = match launcher.check(&self.launcher, project) {
            Ok(check) => check,
            Err(message) => {
                // "Nada foi aberto." first: the error of the jail is long, and a short terminal
                // cuts the end of the alert.
                self.dialog = Some(Dialog::AgentError {
                    message: format!("Nada foi aberto.\n\n{message}"),
                });
                return;
            }
        };
        let project_name = self.project.as_ref().map_or("", |p| p.name.as_str());
        let name = agents::agent_name(project_name, stage, &ticket.key);
        let session_id = match self.db.random_session_id() {
            Ok(id) => id,
            Err(err) => {
                self.status = Some(projects::db_error_message(&err));
                return;
            }
        };
        let Some(agents_dir) = self.agents_dir.as_deref() else {
            self.status = Some("Não consegui abrir o agente: sem a pasta das travas.".into());
            return;
        };
        let record = match agents::open(&self.db, agents_dir, ticket.id, stage, &session_id, &name)
        {
            Ok(record) => record,
            Err(message) => {
                self.status = Some(message);
                return;
            }
        };
        if let Err(err) = launcher.spawn(&self.launcher, project, &session_id, &name) {
            let mut message = format!("Não consegui abrir o agente: {err}.");
            if let Err(db) = agents::end(&self.db, record) {
                message = format!("{message} {db}");
            }
            self.status = Some(message);
            return;
        }
        self.agent = Some(RunningAgent {
            record,
            name,
            jail: check.jail,
            warning: check.warning,
        });
        self.agent_term = AgentTerm::default();
        self.screen = Screen::Agent;
        self.status = None;
    }

    /// The agent's program ended with `code`: the row becomes `ended` and the lock goes
    /// (agents.rs), and the Tickets screen comes back with the same ticket selected and the
    /// message of the end (plus the warning of the check, when there was one).
    pub fn agent_ended(&mut self, code: u32) {
        let Some(agent) = self.agent.take() else {
            return;
        };
        let mut message = if code == 0 {
            format!("O agente {} terminou.", agent.name)
        } else {
            format!("O agente {} terminou com o código {code}.", agent.name)
        };
        if let Some(warning) = &agent.warning {
            let joined: Vec<&str> = warning.split_whitespace().collect();
            message = format!("{message} Aviso da verificação: {}", joined.join(" "));
        }
        if let Err(err) = agents::end(&self.db, agent.record) {
            message = format!("{message} {err}");
        }
        self.screen = Screen::Tickets;
        self.agent_term = AgentTerm::default();
        self.status = Some(message);
        self.refresh_tickets();
    }

    /// Ends the record of the running agent without going back to a screen (the nick is
    /// leaving: a signal). Returns the error of the database, if any.
    pub fn end_agent_record(&mut self) -> Result<(), String> {
        match self.agent.take() {
            Some(agent) => agents::end(&self.db, agent.record),
            None => Ok(()),
        }
    }

    /// A key on the agent screen: Shift+PageUp/PageDown scroll the history; everything else
    /// is encoded as an xterm would and sent to the program.
    fn key_agent(&mut self, key: KeyEvent) -> Action {
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            match key.code {
                KeyCode::PageUp => return Action::ScrollPageUp,
                KeyCode::PageDown => return Action::ScrollPageDown,
                _ => {}
            }
        }
        keys::encode_key(&key, self.agent_term.modes).map_or(Action::None, Action::Send)
    }

    /// A paste: on the agent screen it goes to the program (bracketed and cleaned when the
    /// program asked for it); elsewhere nothing (bracketed paste is only on there).
    pub fn handle_paste(&mut self, text: &str) -> Action {
        if self.screen != Screen::Agent {
            return Action::None;
        }
        Action::Send(keys::encode_paste(text, self.agent_term.bracketed_paste))
    }

    /// A mouse event over `area` (the agent's terminal on the screen): to the program when it
    /// asked for the mouse; otherwise the wheel scrolls the history and the rest is ignored.
    pub fn handle_mouse(&mut self, event: &MouseEvent, area: Rect) -> Action {
        if self.screen != Screen::Agent {
            return Action::None;
        }
        let term = self.agent_term;
        if term.mouse_mode != MouseProtocolMode::None {
            return keys::encode_mouse(event, term.mouse_mode, term.mouse_encoding, area)
                .map_or(Action::None, Action::Send);
        }
        match event.kind {
            MouseEventKind::ScrollUp => Action::ScrollLines(WHEEL_LINES),
            MouseEventKind::ScrollDown => Action::ScrollLines(-WHEEL_LINES),
            _ => Action::None,
        }
    }

    /// Reads the tickets of the open project again (with the hidden ones when the filter is
    /// on), keeping the selected ticket selected when it is still listed.
    fn refresh_tickets(&mut self) {
        let Some(project_id) = self.project.as_ref().map(|p| p.id) else {
            return;
        };
        let selected = self.ticket_rows.get(self.ticket_index).map(|t| t.id);
        match self.db.list_tickets(project_id, self.show_hidden) {
            Ok(list) => self.ticket_rows = list,
            Err(err) => {
                self.ticket_rows.clear();
                self.status = Some(projects::db_error_message(&err));
            }
        }
        if let Some(index) =
            selected.and_then(|id| self.ticket_rows.iter().position(|t| t.id == id))
        {
            self.ticket_index = index;
        }
        self.ticket_index = self
            .ticket_index
            .min(self.ticket_rows.len().saturating_sub(1));
    }

    fn key_add_path(&mut self, key: KeyEvent, mut input: String) -> Action {
        match key.code {
            KeyCode::Esc => {
                // The operator gave up: a refusal of this field means nothing any more.
                self.status = None;
                return Action::None;
            }
            KeyCode::Enter => {
                let checked = projects::check(
                    &self.db,
                    self.module(),
                    &input,
                    self.home.as_deref(),
                    self.cwd.as_deref(),
                );
                match checked {
                    Ok(Check::Ready(dir)) => {
                        match projects::register(&self.db, self.module(), &dir) {
                            Ok(project) => self.registered(&project),
                            Err(message) => {
                                self.status = Some(message);
                                self.dialog = Some(Dialog::AddPath { input });
                            }
                        }
                    }
                    Ok(Check::NeedsConfig(dir)) => {
                        self.status = None;
                        self.dialog = Some(Dialog::NeedsConfig {
                            dir,
                            focus: Button::Cancel,
                            note: None,
                        });
                    }
                    Err(message) => {
                        self.status = Some(message);
                        self.dialog = Some(Dialog::AddPath { input });
                    }
                }
                return Action::None;
            }
            _ => {
                edit_text(&mut input, key);
            }
        }
        self.dialog = Some(Dialog::AddPath { input });
        Action::None
    }

    fn key_needs_config(
        &mut self,
        key: KeyEvent,
        dir: PathBuf,
        mut focus: Button,
        mut note: Option<String>,
    ) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.status = None;
                return Action::None;
            }
            KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                focus = match focus {
                    Button::CreateConfig => Button::Cancel,
                    Button::Cancel => Button::CreateConfig,
                };
            }
            KeyCode::Enter => match focus {
                Button::Cancel => {
                    self.status = None;
                    return Action::None;
                }
                Button::CreateConfig if !self.init.is_file() => {
                    note = Some(format!(
                        "Não encontrei o bin/nick-init em {}. Nada foi feito.",
                        self.init.display()
                    ));
                }
                Button::CreateConfig => {
                    self.dialog = Some(Dialog::NeedsConfig {
                        dir: dir.clone(),
                        focus,
                        note: None,
                    });
                    return Action::RunInit { project: dir };
                }
            },
            _ => {}
        }
        self.dialog = Some(Dialog::NeedsConfig { dir, focus, note });
        Action::None
    }

    fn key_confirm_remove(&mut self, key: KeyEvent, project: Project) -> Action {
        // Only s/S confirm (the "sim" of the auto_scrum confirmations); any other key cancels.
        if matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S')) {
            self.status = Some(match projects::remove(&self.db, &project) {
                Ok(message) | Err(message) => message,
            });
        }
        Action::None
    }

    /// After a registration: the status message and the new project selected.
    fn registered(&mut self, project: &Project) {
        self.status = Some(projects::registered_message(project));
        self.refresh();
        if let Some(index) = self.rows.iter().position(|r| r.project.id == project.id) {
            self.project_index = index;
        }
    }

    /// Reads the projects of the module again and computes their availability.
    fn refresh(&mut self) {
        match self.db.list_projects(self.module().key()) {
            Ok(list) => {
                self.rows = list
                    .into_iter()
                    .map(|project| Row {
                        availability: projects::availability(&project.path),
                        project,
                    })
                    .collect();
            }
            Err(err) => {
                self.rows.clear();
                self.status = Some(projects::db_error_message(&err));
            }
        }
        self.project_index = self.project_index.min(self.rows.len().saturating_sub(1));
    }

    /// Back from the init (plan D6): checks the config again and registers the project only if
    /// it exists now; otherwise the alert says why.
    pub fn after_init(&mut self, outcome: InitOutcome) {
        let Some(Dialog::NeedsConfig { dir, focus, .. }) = self.dialog.take() else {
            return;
        };
        let note = match outcome {
            InitOutcome::SpawnFailed(err) => {
                format!("Não consegui rodar o nick-init: {err}. Nada foi cadastrado.")
            }
            _ if projects::config_file(&dir).is_file() => {
                match projects::register(&self.db, self.module(), &dir) {
                    Ok(project) => {
                        self.registered(&project);
                        return;
                    }
                    Err(message) => message,
                }
            }
            InitOutcome::Exited(0) => "O nick-init terminou sem criar a configuração \
                 (sobrescrita recusada?). Nada foi cadastrado."
                .to_string(),
            InitOutcome::Exited(code) => {
                format!("O nick-init falhou (código {code}). Nada foi cadastrado.")
            }
            InitOutcome::Signaled => "O nick-init foi interrompido. Nada foi cadastrado.".into(),
        };
        self.dialog = Some(Dialog::NeedsConfig {
            dir,
            focus,
            note: Some(note),
        });
    }
}

/// The typing of a one-line text field: Backspace erases the last character, and a character
/// without Ctrl/Alt is appended. Returns true when the key changed the text.
fn edit_text(text: &mut String, key: KeyEvent) -> bool {
    match key.code {
        KeyCode::Backspace => text.pop().is_some(),
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            text.push(c);
            true
        }
        _ => false,
    }
}

/// `q` or `Q` exactly (Shift is how `Q` is typed): the "Sair (q)" rule of the lists.
fn is_quit(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('q') | KeyCode::Char('Q'))
        && (key.modifiers - KeyModifiers::SHIFT).is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// A fake runner: records each call and, before returning the scripted outcome, creates
    /// the config when `creates` is set (what the real init does).
    struct FakeRunner {
        calls: Vec<(PathBuf, PathBuf)>,
        outcome: InitOutcome,
        creates: bool,
    }

    impl FakeRunner {
        fn new(outcome: InitOutcome, creates: bool) -> FakeRunner {
            FakeRunner {
                calls: Vec::new(),
                outcome,
                creates,
            }
        }
    }

    impl InitRunner for FakeRunner {
        fn run(&mut self, init: &Path, project: &Path) -> InitOutcome {
            self.calls.push((init.to_path_buf(), project.to_path_buf()));
            if self.creates {
                fs::create_dir_all(project.join(".nick")).unwrap();
                fs::write(projects::config_file(project), "").unwrap();
            }
            self.outcome.clone()
        }
    }

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: PathBuf,
        app: App,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(tmp.path()).unwrap();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let init = root.join("bin/nick-init");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(&init, "#!/usr/bin/env bash\n").unwrap();
        fs::create_dir_all(root.join("cwd")).unwrap();
        let app = App::new(db, Some(root.join("home")), Some(root.join("cwd")), init);
        Fixture {
            _tmp: tmp,
            root,
            app,
        }
    }

    impl Fixture {
        fn dir(&self, rel: &str, config: bool) -> PathBuf {
            let dir = self.root.join(rel);
            fs::create_dir_all(&dir).unwrap();
            if config {
                fs::create_dir_all(dir.join(".nick")).unwrap();
                fs::write(projects::config_file(&dir), "").unwrap();
            }
            dir
        }

        fn key(&mut self, code: KeyCode) -> Action {
            self.app.handle_key(KeyEvent::new(code, KeyModifiers::NONE))
        }

        fn ch(&mut self, c: char) -> Action {
            self.key(KeyCode::Char(c))
        }

        fn type_text(&mut self, text: &str) {
            for c in text.chars() {
                assert_eq!(self.ch(c), Action::None);
            }
        }

        fn ctrl_c(&mut self) -> Action {
            self.app
                .handle_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
        }

        /// Opens the Projects screen and types `path` in the add field, then Enter.
        fn add(&mut self, path: &str) -> Action {
            if self.app.screen == Screen::Modules {
                self.key(KeyCode::Enter);
            }
            self.ch('a');
            self.type_text(path);
            self.key(KeyCode::Enter)
        }

        fn names(&self) -> Vec<String> {
            self.app
                .rows
                .iter()
                .map(|r| r.project.name.clone())
                .collect()
        }

        fn stored(&self) -> Vec<PathBuf> {
            self.app
                .db
                .list_projects("auto_scrum")
                .unwrap()
                .into_iter()
                .map(|p| p.path)
                .collect()
        }

        fn status(&self) -> Option<&str> {
            self.app.status.as_deref()
        }

        fn note(&self) -> Option<&str> {
            match &self.app.dialog {
                Some(Dialog::NeedsConfig { note, .. }) => note.as_deref(),
                other => panic!("not in the alert: {other:?}"),
            }
        }
    }

    // ---- navigation and quitting ----

    #[test]
    fn it_starts_on_the_modules_screen_with_auto_scrum() {
        let f = fixture();
        assert_eq!(f.app.screen, Screen::Modules);
        assert_eq!(f.app.module(), ModuleKind::AutoScrum);
        assert_eq!(f.app.dialog, None);
    }

    #[test]
    fn enter_and_right_open_the_projects_and_esc_and_left_go_back() {
        let mut f = fixture();
        for open in [KeyCode::Enter, KeyCode::Right] {
            for back in [KeyCode::Esc, KeyCode::Left] {
                assert_eq!(f.key(open), Action::None);
                assert_eq!(f.app.screen, Screen::Projects);
                assert_eq!(f.key(back), Action::None);
                assert_eq!(f.app.screen, Screen::Modules);
            }
        }
    }

    #[test]
    fn up_down_k_j_move_within_the_list() {
        let mut f = fixture();
        f.dir("a", true);
        f.dir("b", true);
        f.dir("c", true);
        f.add(f.root.join("a").to_str().unwrap());
        f.add(f.root.join("b").to_str().unwrap());
        f.add(f.root.join("c").to_str().unwrap());
        f.app.project_index = 0;
        f.key(KeyCode::Down);
        f.ch('j');
        assert_eq!(f.app.project_index, 2);
        f.key(KeyCode::Down);
        assert_eq!(f.app.project_index, 2, "stops at the last");
        f.key(KeyCode::Up);
        f.ch('k');
        f.ch('k');
        assert_eq!(f.app.project_index, 0, "stops at the first");
        // One module: moving on Modules stays on it.
        f.key(KeyCode::Esc);
        f.key(KeyCode::Down);
        f.ch('j');
        assert_eq!(f.app.module_index, 0);
    }

    #[test]
    fn q_and_capital_q_quit_on_both_lists() {
        for c in ['q', 'Q'] {
            let mut f = fixture();
            assert_eq!(f.ch(c), Action::Quit);
            let mut f = fixture();
            f.key(KeyCode::Enter);
            assert_eq!(f.ch(c), Action::Quit);
            let mut f = fixture();
            let shifted = KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::SHIFT);
            assert_eq!(f.app.handle_key(shifted), Action::Quit);
        }
    }

    #[test]
    fn q_does_not_quit_in_the_text_field_and_is_typed() {
        let mut f = fixture();
        f.key(KeyCode::Enter);
        f.ch('a');
        assert_eq!(f.ch('q'), Action::None);
        assert_eq!(f.ch('Q'), Action::None);
        assert_eq!(f.app.dialog, Some(Dialog::AddPath { input: "qQ".into() }));
    }

    #[test]
    fn q_does_not_quit_in_the_alert() {
        let mut f = fixture();
        let bare = f.dir("bare", false);
        f.add(bare.to_str().unwrap());
        assert_eq!(f.ch('q'), Action::None);
        assert_eq!(f.ch('Q'), Action::None);
        assert!(matches!(f.app.dialog, Some(Dialog::NeedsConfig { .. })));
    }

    #[test]
    fn q_does_not_quit_in_the_removal_confirmation_it_cancels() {
        let mut f = fixture();
        let p = f.dir("p", true);
        f.add(p.to_str().unwrap());
        f.ch('r');
        assert_eq!(f.ch('q'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.stored(), [p]);
    }

    #[test]
    fn ctrl_c_quits_on_every_screen_and_dialog() {
        let mut f = fixture();
        assert_eq!(f.ctrl_c(), Action::Quit);
        f.key(KeyCode::Enter);
        assert_eq!(f.ctrl_c(), Action::Quit);
        f.ch('a');
        assert_eq!(f.ctrl_c(), Action::Quit);
        let bare = f.dir("bare", false);
        f.type_text(bare.to_str().unwrap());
        f.key(KeyCode::Enter);
        assert!(matches!(f.app.dialog, Some(Dialog::NeedsConfig { .. })));
        assert_eq!(f.ctrl_c(), Action::Quit);
        f.key(KeyCode::Esc);
        let p = f.dir("p", true);
        f.add(p.to_str().unwrap());
        f.ch('r');
        assert!(matches!(f.app.dialog, Some(Dialog::ConfirmRemove { .. })));
        assert_eq!(f.ctrl_c(), Action::Quit);
    }

    // ---- adding ----

    #[test]
    fn adding_a_valid_directory_registers_it_and_selects_it() {
        let mut f = fixture();
        let a = f.dir("cwd/alfa", true);
        let b = f.dir("beta", true);
        assert_eq!(f.add(b.to_str().unwrap()), Action::None);
        assert_eq!(
            f.status(),
            Some(format!("Projeto cadastrado: beta ({}).", b.display()).as_str())
        );
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.add("alfa"), Action::None, "relative to the cwd");
        assert_eq!(f.names(), ["alfa", "beta"]);
        assert_eq!(f.app.project_index, 0, "the new one is selected");
        assert_eq!(f.stored(), [a, b]);
    }

    #[test]
    fn backspace_edits_and_esc_cancels_the_text_field() {
        let mut f = fixture();
        f.key(KeyCode::Enter);
        f.ch('a');
        f.type_text("abc");
        f.key(KeyCode::Enter);
        assert_eq!(f.status(), Some("A pasta abc não existe."));
        f.key(KeyCode::Backspace);
        assert_eq!(f.app.dialog, Some(Dialog::AddPath { input: "ab".into() }));
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.app.screen, Screen::Projects);
        assert_eq!(f.stored(), Vec::<PathBuf>::new());
        assert_eq!(f.status(), None, "the refusal goes away with the field");
    }

    #[test]
    fn each_refusal_shows_its_message_and_keeps_the_field_open() {
        let mut f = fixture();
        let file = f.root.join("file.txt");
        fs::write(&file, "").unwrap();
        let p = f.dir("p", true);
        f.add(p.to_str().unwrap());
        let file_text = file.to_str().unwrap().to_string();
        let cases = [
            ("", "Informe o caminho da pasta do projeto.".to_string()),
            ("nada", "A pasta nada não existe.".to_string()),
            (file_text.as_str(), format!("{file_text} não é uma pasta.")),
            (
                p.to_str().unwrap(),
                format!("Esse projeto já está cadastrado: {}.", p.display()),
            ),
        ];
        for (input, message) in cases {
            f.add(input);
            assert_eq!(f.status(), Some(message.as_str()), "input {input:?}");
            assert_eq!(
                f.app.dialog,
                Some(Dialog::AddPath {
                    input: input.into()
                }),
                "the field stays open with the text"
            );
            f.key(KeyCode::Esc);
        }
        assert_eq!(f.stored(), [p]);
    }

    // ---- the missing-config alert ----

    #[test]
    fn a_directory_without_config_opens_the_alert_on_cancel_and_stores_nothing() {
        let mut f = fixture();
        let bare = f.dir("bare", false);
        let mut runner = FakeRunner::new(InitOutcome::Exited(0), true);
        f.key(KeyCode::Enter);
        f.ch('a');
        f.type_text(bare.to_str().unwrap());
        assert!(!f.app.on_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut runner
        ));
        assert_eq!(
            f.app.dialog,
            Some(Dialog::NeedsConfig {
                dir: bare.clone(),
                focus: Button::Cancel,
                note: None,
            })
        );
        assert!(runner.calls.is_empty(), "the init did not run");
        assert_eq!(f.stored(), Vec::<PathBuf>::new());
        assert!(!projects::config_file(&bare).exists());
    }

    #[test]
    fn esc_and_cancel_close_the_alert_without_run_init() {
        let mut f = fixture();
        let bare = f.dir("bare", false);
        f.add(bare.to_str().unwrap());
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.dialog, None);
        f.add(bare.to_str().unwrap());
        assert_eq!(
            f.key(KeyCode::Enter),
            Action::None,
            "focus starts on Cancelar"
        );
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.stored(), Vec::<PathBuf>::new());
    }

    #[test]
    fn an_old_refusal_does_not_stay_under_the_alert_nor_after_it() {
        let mut f = fixture();
        let bare = f.dir("bare", false);
        f.add("nada");
        assert_eq!(f.status(), Some("A pasta nada não existe."));
        for _ in 0.."nada".len() {
            f.key(KeyCode::Backspace);
        }
        f.type_text(bare.to_str().unwrap());
        f.key(KeyCode::Enter);
        assert!(matches!(f.app.dialog, Some(Dialog::NeedsConfig { .. })));
        assert_eq!(f.status(), None, "the alert opens without the old refusal");
        // Esc and the focused "Cancelar" leave nothing on the status bar either.
        f.app.status = Some("x".into());
        f.key(KeyCode::Esc);
        assert_eq!(f.status(), None);
        f.add(bare.to_str().unwrap());
        f.app.status = Some("x".into());
        f.key(KeyCode::Enter);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), None);
    }

    #[test]
    fn left_right_tab_toggle_the_focus() {
        let mut f = fixture();
        let bare = f.dir("bare", false);
        f.add(bare.to_str().unwrap());
        let focus = |f: &Fixture| match &f.app.dialog {
            Some(Dialog::NeedsConfig { focus, .. }) => *focus,
            other => panic!("{other:?}"),
        };
        for (key, expected) in [
            (KeyCode::Left, Button::CreateConfig),
            (KeyCode::Right, Button::Cancel),
            (KeyCode::Tab, Button::CreateConfig),
            (KeyCode::Tab, Button::Cancel),
            (KeyCode::BackTab, Button::CreateConfig),
        ] {
            assert_eq!(f.key(key), Action::None);
            assert_eq!(focus(&f), expected, "after {key:?}");
        }
    }

    #[test]
    fn only_the_create_button_returns_run_init() {
        let mut f = fixture();
        let bare = f.dir("bare", false);
        f.add(bare.to_str().unwrap());
        // Every other key of the alert returns Action::None.
        for key in [
            KeyCode::Char('s'),
            KeyCode::Char('c'),
            KeyCode::Char(' '),
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Backspace,
        ] {
            assert_eq!(f.key(key), Action::None, "{key:?}");
        }
        f.key(KeyCode::Left);
        assert_eq!(
            f.key(KeyCode::Enter),
            Action::RunInit {
                project: bare.clone()
            }
        );
    }

    #[test]
    fn a_missing_init_shows_the_path_and_never_calls_the_runner() {
        let mut f = fixture();
        let missing = f.root.join("nao/existe/nick-init");
        f.app.init = missing.clone();
        let bare = f.dir("bare", false);
        f.add(bare.to_str().unwrap());
        f.key(KeyCode::Left);
        let mut runner = FakeRunner::new(InitOutcome::Exited(0), true);
        assert!(!f.app.on_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut runner
        ));
        assert!(runner.calls.is_empty());
        assert_eq!(
            f.note(),
            Some(
                format!(
                    "Não encontrei o bin/nick-init em {}. Nada foi feito.",
                    missing.display()
                )
                .as_str()
            )
        );
        // A directory in place of the init is not a file either.
        f.app.init = f.root.clone();
        f.app.on_key(
            KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &mut runner,
        );
        assert!(runner.calls.is_empty());
        // The TUI goes on: Esc closes the alert, q quits.
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.ch('q'), Action::Quit);
    }

    /// Runs the button with a fake runner and returns it.
    fn press_create(f: &mut Fixture, dir: &Path, runner: &mut FakeRunner) {
        f.add(dir.to_str().unwrap());
        f.key(KeyCode::Left);
        f.app
            .on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE), runner);
    }

    #[test]
    fn the_init_that_creates_the_config_registers_the_project() {
        let mut f = fixture();
        let bare = f.dir("bare", false);
        let mut runner = FakeRunner::new(InitOutcome::Exited(0), true);
        press_create(&mut f, &bare, &mut runner);
        assert_eq!(runner.calls, [(f.app.init.clone(), bare.clone())]);
        assert_eq!(f.app.dialog, None);
        assert_eq!(
            f.status(),
            Some(format!("Projeto cadastrado: bare ({}).", bare.display()).as_str())
        );
        assert_eq!(f.stored(), [bare]);
        assert_eq!(f.app.rows[0].availability, Availability::Available);
    }

    #[test]
    fn each_init_failure_registers_nothing_and_says_why() {
        let cases = [
            (
                InitOutcome::Exited(0),
                "O nick-init terminou sem criar a configuração (sobrescrita recusada?). \
                 Nada foi cadastrado.",
            ),
            (
                InitOutcome::Exited(1),
                "O nick-init falhou (código 1). Nada foi cadastrado.",
            ),
            (
                InitOutcome::Exited(42),
                "O nick-init falhou (código 42). Nada foi cadastrado.",
            ),
            (
                InitOutcome::Signaled,
                "O nick-init foi interrompido. Nada foi cadastrado.",
            ),
            (
                InitOutcome::SpawnFailed("No such file or directory (os error 2)".into()),
                "Não consegui rodar o nick-init: No such file or directory (os error 2). \
                 Nada foi cadastrado.",
            ),
        ];
        for (outcome, message) in cases {
            let mut f = fixture();
            let bare = f.dir("bare", false);
            let mut runner = FakeRunner::new(outcome.clone(), false);
            press_create(&mut f, &bare, &mut runner);
            assert_eq!(runner.calls.len(), 1);
            assert_eq!(f.note(), Some(message), "{outcome:?}");
            assert_eq!(f.stored(), Vec::<PathBuf>::new(), "{outcome:?}");
            // The alert stays, and Esc closes it.
            assert_eq!(f.key(KeyCode::Esc), Action::None);
            assert_eq!(f.app.dialog, None);
        }
    }

    #[test]
    fn a_failing_init_that_still_created_the_config_registers() {
        // The file is what counts (plan D6: "seja qual for o resultado", it is checked again).
        let mut f = fixture();
        let bare = f.dir("bare", false);
        let mut runner = FakeRunner::new(InitOutcome::Exited(1), true);
        press_create(&mut f, &bare, &mut runner);
        assert_eq!(f.stored(), [bare]);
        assert_eq!(f.app.dialog, None);
    }

    // ---- removing ----

    fn with_one_project() -> (Fixture, PathBuf) {
        let mut f = fixture();
        let p = f.dir("proj", true);
        f.add(p.to_str().unwrap());
        (f, p)
    }

    #[test]
    fn r_asks_for_confirmation_with_the_project() {
        let (mut f, p) = with_one_project();
        assert_eq!(f.ch('r'), Action::None);
        match &f.app.dialog {
            Some(Dialog::ConfirmRemove { project, .. }) => assert_eq!(project.path, p),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn s_and_capital_s_remove() {
        for c in ['s', 'S'] {
            let (mut f, _) = with_one_project();
            f.ch('r');
            assert_eq!(f.ch(c), Action::None);
            assert_eq!(f.app.dialog, None);
            assert_eq!(f.stored(), Vec::<PathBuf>::new());
            assert_eq!(f.names(), Vec::<String>::new());
            assert_eq!(
                f.status(),
                Some("Projeto removido do nick: proj. Nenhum arquivo foi apagado.")
            );
        }
    }

    #[test]
    fn n_enter_and_any_other_key_cancel_the_removal() {
        for key in [
            KeyCode::Char('n'),
            KeyCode::Char('N'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Char('x'),
            KeyCode::Char('y'),
        ] {
            let (mut f, p) = with_one_project();
            f.ch('r');
            assert_eq!(f.key(key), Action::None, "{key:?}");
            assert_eq!(f.app.dialog, None, "{key:?}");
            assert_eq!(f.stored(), std::slice::from_ref(&p), "{key:?}");
        }
    }

    #[test]
    fn r_on_an_empty_list_does_nothing() {
        let mut f = fixture();
        f.key(KeyCode::Enter);
        assert_eq!(f.ch('r'), Action::None);
        assert_eq!(f.app.dialog, None);
    }

    #[test]
    fn removing_the_last_row_keeps_the_selection_in_range() {
        let mut f = fixture();
        let a = f.dir("a", true);
        let b = f.dir("b", true);
        f.add(a.to_str().unwrap());
        f.add(b.to_str().unwrap());
        assert_eq!(f.app.project_index, 1);
        f.ch('r');
        f.ch('s');
        assert_eq!(f.app.project_index, 0);
        assert_eq!(f.names(), ["a"]);
    }

    // ---- availability ----

    #[test]
    fn availability_is_computed_again_after_each_key() {
        let (mut f, p) = with_one_project();
        assert_eq!(f.app.rows[0].availability, Availability::Available);
        fs::remove_file(projects::config_file(&p)).unwrap();
        f.key(KeyCode::Down);
        assert_eq!(f.app.rows[0].availability, Availability::ConfigMissing);
        fs::remove_dir_all(&p).unwrap();
        f.key(KeyCode::Esc);
        f.key(KeyCode::Enter);
        assert_eq!(f.app.rows[0].availability, Availability::DirMissing);
    }

    /// Typing in a dialog is not an action on the list (plan D9): no query and no disk check
    /// per key (review NS-31, P3; a stuck network mount would freeze every key). Closing it is.
    #[test]
    fn typing_in_a_dialog_does_not_refresh_the_list() {
        let (mut f, p) = with_one_project();
        fs::remove_file(projects::config_file(&p)).unwrap();
        f.ch('a');
        f.type_text("abc");
        f.key(KeyCode::Backspace);
        assert_eq!(
            f.app.rows[0].availability,
            Availability::Available,
            "not recomputed while typing"
        );
        f.key(KeyCode::Esc);
        assert_eq!(f.app.rows[0].availability, Availability::ConfigMissing);
        // The same in the removal confirmation: the answer closes it and refreshes.
        fs::remove_dir_all(&p).unwrap();
        f.ch('r');
        assert_eq!(f.app.rows[0].availability, Availability::ConfigMissing);
        f.ch('n');
        assert_eq!(f.app.rows[0].availability, Availability::DirMissing);
    }

    // ---- tickets (NS-32) ----

    /// Registers `rel` (with config) and opens its tickets.
    fn with_tickets(rel: &str) -> (Fixture, PathBuf) {
        let mut f = fixture();
        let p = f.dir(rel, true);
        f.add(p.to_str().unwrap());
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.app.screen, Screen::Tickets, "{:?}", f.status());
        (f, p)
    }

    impl Fixture {
        fn keys(&self) -> Vec<String> {
            self.app.ticket_rows.iter().map(|t| t.key.clone()).collect()
        }

        fn all_tickets(&self) -> Vec<crate::tickets::Ticket> {
            let id = self.app.project.as_ref().expect("a project is open").id;
            self.app.db.list_tickets(id, true).unwrap()
        }

        /// Creates a ticket through the dialog: `a`, the title, Tab, the ID, Enter.
        fn new_ticket(&mut self, title: &str, jira: &str) -> Action {
            self.ch('a');
            self.type_text(title);
            self.key(KeyCode::Tab);
            self.type_text(jira);
            self.key(KeyCode::Enter)
        }
    }

    #[test]
    fn enter_and_right_open_the_tickets_of_an_available_project() {
        for open in [KeyCode::Enter, KeyCode::Right] {
            let mut f = fixture();
            let p = f.dir("proj", true);
            f.add(p.to_str().unwrap());
            f.app.status = Some("x".into());
            assert_eq!(f.key(open), Action::None);
            assert_eq!(f.app.screen, Screen::Tickets);
            assert_eq!(f.app.project.as_ref().unwrap().path, p);
            assert_eq!(f.app.ticket_rows, []);
            assert!(!f.app.show_hidden);
            assert_eq!(f.status(), None);
        }
    }

    #[test]
    fn an_unavailable_project_does_not_open_and_says_why() {
        for dir_missing in [true, false] {
            let (mut f, p) = with_one_project();
            let mark = if dir_missing {
                fs::remove_dir_all(&p).unwrap();
                "[indisponível: pasta não encontrada]"
            } else {
                fs::remove_file(projects::config_file(&p)).unwrap();
                "[indisponível: sem .nick/nick.conf]"
            };
            for open in [KeyCode::Enter, KeyCode::Right] {
                assert_eq!(f.key(open), Action::None);
                assert_eq!(f.app.screen, Screen::Projects, "{mark}");
                assert_eq!(f.app.project, None);
                assert_eq!(
                    f.status(),
                    Some(
                        format!(
                            "Projeto indisponível: {mark}. Os tickets só abrem com a pasta \
                             e o .nick/nick.conf no lugar."
                        )
                        .as_str()
                    )
                );
            }
        }
    }

    #[test]
    fn enter_on_an_empty_projects_list_does_nothing() {
        let mut f = fixture();
        f.key(KeyCode::Enter);
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.app.screen, Screen::Projects);
    }

    #[test]
    fn esc_and_left_go_back_to_the_projects_with_it_selected() {
        for back in [KeyCode::Esc, KeyCode::Left] {
            let mut f = fixture();
            let a = f.dir("a", true);
            let b = f.dir("b", true);
            f.add(a.to_str().unwrap());
            f.add(b.to_str().unwrap());
            assert_eq!(f.app.project_index, 1);
            f.key(KeyCode::Enter);
            f.ch('f');
            assert!(f.app.show_hidden);
            assert_eq!(f.key(back), Action::None);
            assert_eq!(f.app.screen, Screen::Projects);
            assert_eq!(f.app.project_index, 1, "b is still selected");
            assert!(!f.app.show_hidden, "the filter is off again");
            assert_eq!(f.app.project, None);
        }
    }

    #[test]
    fn a_opens_the_new_ticket_dialog_on_the_title() {
        let (mut f, _) = with_tickets("proj");
        assert_eq!(f.ch('a'), Action::None);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::NewTicket {
                title: String::new(),
                jira_id: String::new(),
                focus: TicketField::Title,
            })
        );
    }

    #[test]
    fn tab_shift_tab_up_down_switch_the_field_and_typing_goes_to_it() {
        let (mut f, _) = with_tickets("proj");
        f.ch('a');
        f.type_text("Tq");
        f.key(KeyCode::Tab);
        f.type_text("NS-1");
        f.key(KeyCode::BackTab);
        f.type_text("x");
        f.key(KeyCode::Down);
        f.key(KeyCode::Backspace);
        f.key(KeyCode::Up);
        let alt = KeyEvent::new(KeyCode::Char('z'), KeyModifiers::ALT);
        f.app.handle_key(alt);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::NewTicket {
                title: "Tqx".into(),
                jira_id: "NS-".into(),
                focus: TicketField::Title,
            })
        );
    }

    #[test]
    fn enter_creates_from_either_field_and_selects_the_new_ticket() {
        let (mut f, _) = with_tickets("proj");
        assert_eq!(f.new_ticket("Primeiro", "NS-1"), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), Some("Ticket criado: NS-1 — Primeiro."));
        // Enter on the title field also confirms.
        f.ch('a');
        f.type_text("Segundo");
        f.key(KeyCode::Enter);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.keys().len(), 2);
        assert_eq!(f.app.ticket_index, 1, "the new one is selected");
        let second = &f.app.ticket_rows[1];
        assert!(second.is_provisional);
        assert_eq!(
            f.status(),
            Some(format!("Ticket criado com o ID provisório {}: Segundo.", second.key).as_str())
        );
    }

    #[test]
    fn esc_cancels_the_new_ticket_and_stores_nothing() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("fica", "NS-1");
        f.ch('a');
        f.type_text("abandonado");
        f.key(KeyCode::Tab);
        f.type_text("NS-2");
        f.app.status = Some("x".into());
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), None);
        assert_eq!(f.all_tickets().len(), 1);
    }

    #[test]
    fn refusals_keep_the_dialog_with_what_was_typed() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("a", "NS-1");
        let cases = [
            ("   ", "NS-2", "Informe o título do ticket."),
            ("b", "a/b", crate::tickets::INVALID_ID_MESSAGE),
            (
                "b",
                "NS-1",
                "Já existe um ticket com o ID NS-1 neste projeto.",
            ),
        ];
        for (title, jira, message) in cases {
            f.new_ticket(title, jira);
            assert_eq!(f.status(), Some(message), "{title:?} {jira:?}");
            assert_eq!(
                f.app.dialog,
                Some(Dialog::NewTicket {
                    title: title.into(),
                    jira_id: jira.into(),
                    focus: TicketField::JiraId,
                })
            );
            f.key(KeyCode::Esc);
        }
        assert_eq!(f.all_tickets().len(), 1);
    }

    #[test]
    fn e_edits_the_title_prefilled_and_validates_it() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("Velho", "NS-1");
        let ticket = f.app.ticket_rows[0].clone();
        assert_eq!(f.ch('e'), Action::None);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::EditTitle {
                ticket: ticket.clone(),
                input: "Velho".into(),
            })
        );
        for _ in 0.."Velho".len() {
            f.key(KeyCode::Backspace);
        }
        f.type_text(" ");
        f.key(KeyCode::Enter);
        assert_eq!(f.status(), Some("Informe o título do ticket."));
        assert_eq!(
            f.app.dialog,
            Some(Dialog::EditTitle {
                ticket: ticket.clone(),
                input: " ".into(),
            })
        );
        f.type_text("Novo q");
        f.key(KeyCode::Enter);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), Some("Título atualizado: NS-1."));
        assert_eq!(f.app.ticket_rows[0].title, "Novo q");
        assert_eq!(f.app.ticket_rows[0].key, "NS-1");
        // Esc cancels without saving.
        f.ch('e');
        f.type_text("zzz");
        f.key(KeyCode::Esc);
        assert_eq!(f.app.ticket_rows[0].title, "Novo q");
    }

    #[test]
    fn e_and_o_on_an_empty_list_do_nothing() {
        let (mut f, _) = with_tickets("proj");
        for c in ['e', 'o'] {
            assert_eq!(f.ch(c), Action::None);
            assert_eq!(f.app.dialog, None);
        }
    }

    #[test]
    fn o_asks_and_only_s_hides() {
        for (key, hidden) in [
            (KeyCode::Char('s'), true),
            (KeyCode::Char('S'), true),
            (KeyCode::Char('n'), false),
            (KeyCode::Enter, false),
            (KeyCode::Esc, false),
            (KeyCode::Char('q'), false),
        ] {
            let (mut f, _) = with_tickets("proj");
            f.new_ticket("t", "NS-1");
            let ticket = f.app.ticket_rows[0].clone();
            assert_eq!(f.ch('o'), Action::None);
            assert_eq!(f.app.dialog, Some(Dialog::ConfirmHide { ticket }));
            assert_eq!(f.key(key), Action::None, "{key:?}");
            assert_eq!(f.app.dialog, None);
            assert_eq!(f.all_tickets()[0].hidden_at.is_some(), hidden, "{key:?}");
            assert_eq!(f.keys().is_empty(), hidden, "{key:?}");
            if hidden {
                assert_eq!(
                    f.status(),
                    Some(
                        "Ticket ocultado: NS-1. Ele continua no banco e pode ser restaurado \
                         (f mostra os ocultos)."
                    )
                );
            }
        }
    }

    #[test]
    fn f_shows_the_hidden_ones_and_o_restores_without_asking() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("a", "NS-1");
        f.new_ticket("b", "NS-2");
        f.app.ticket_index = 0;
        f.ch('o');
        f.ch('s');
        assert_eq!(f.keys(), ["NS-2"]);
        assert_eq!(f.ch('f'), Action::None);
        assert!(f.app.show_hidden);
        assert_eq!(f.keys(), ["NS-1", "NS-2"]);
        f.app.ticket_index = 0;
        assert_eq!(f.ch('o'), Action::None);
        assert_eq!(f.app.dialog, None, "no confirmation to restore");
        assert_eq!(f.status(), Some("Ticket restaurado: NS-1."));
        assert!(f.all_tickets().iter().all(|t| t.hidden_at.is_none()));
        assert_eq!(f.ch('f'), Action::None);
        assert!(!f.app.show_hidden);
        assert_eq!(f.keys(), ["NS-1", "NS-2"]);
    }

    #[test]
    fn up_down_move_within_the_tickets() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("a", "A");
        f.new_ticket("b", "B");
        assert_eq!(f.app.ticket_index, 1);
        f.key(KeyCode::Down);
        assert_eq!(f.app.ticket_index, 1, "stops at the last");
        f.key(KeyCode::Up);
        f.ch('k');
        assert_eq!(f.app.ticket_index, 0, "stops at the first");
        f.ch('j');
        assert_eq!(f.app.ticket_index, 1);
    }

    #[test]
    fn hiding_the_last_ticket_keeps_the_selection_in_range() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("a", "A");
        f.new_ticket("b", "B");
        f.ch('o');
        f.ch('s');
        assert_eq!(f.app.ticket_index, 0);
        assert_eq!(f.keys(), ["A"]);
    }

    #[test]
    fn q_quits_the_tickets_list_but_not_its_dialogs() {
        for c in ['q', 'Q'] {
            let (mut f, _) = with_tickets("proj");
            assert_eq!(f.ch(c), Action::Quit);
        }
        let (mut f, _) = with_tickets("proj");
        f.ch('a');
        assert_eq!(f.ch('q'), Action::None);
        f.key(KeyCode::Tab);
        assert_eq!(f.ch('Q'), Action::None);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::NewTicket {
                title: "q".into(),
                jira_id: "Q".into(),
                focus: TicketField::JiraId,
            })
        );
        f.key(KeyCode::Enter);
        f.ch('e');
        assert_eq!(f.ch('q'), Action::None);
        assert!(matches!(f.app.dialog, Some(Dialog::EditTitle { .. })));
        f.key(KeyCode::Esc);
        f.ch('o');
        assert_eq!(f.ch('q'), Action::None, "it cancels the confirmation");
        assert_eq!(f.app.dialog, None);
    }

    #[test]
    fn ctrl_c_quits_from_the_tickets_and_every_dialog() {
        let (mut f, _) = with_tickets("proj");
        assert_eq!(f.ctrl_c(), Action::Quit);
        f.ch('a');
        assert_eq!(f.ctrl_c(), Action::Quit);
        f.type_text("t");
        f.key(KeyCode::Enter);
        f.ch('e');
        assert_eq!(f.ctrl_c(), Action::Quit);
        f.key(KeyCode::Esc);
        f.ch('o');
        assert_eq!(f.ctrl_c(), Action::Quit);
    }

    #[test]
    fn the_tickets_of_a_project_do_not_show_in_another() {
        let mut f = fixture();
        let a = f.dir("a", true);
        let b = f.dir("b", true);
        f.add(a.to_str().unwrap());
        f.add(b.to_str().unwrap());
        f.app.project_index = 0;
        f.key(KeyCode::Enter);
        f.new_ticket("de a", "NS-1");
        f.key(KeyCode::Esc);
        f.key(KeyCode::Down);
        f.key(KeyCode::Enter);
        assert_eq!(f.app.project.as_ref().unwrap().path, b);
        assert_eq!(f.keys(), Vec::<String>::new());
        f.new_ticket("de b", "NS-1");
        assert_eq!(
            f.status(),
            Some("Ticket criado: NS-1 — de b."),
            "same ID is fine"
        );
        assert_eq!(f.app.ticket_rows[0].title, "de b");
    }

    #[test]
    fn the_removal_confirmation_counts_the_tickets_and_removes_them() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("a", "NS-1");
        f.new_ticket("b", "NS-2");
        f.ch('o');
        f.ch('s');
        let id = f.app.project.as_ref().unwrap().id;
        f.key(KeyCode::Esc);
        f.ch('r');
        match &f.app.dialog {
            Some(Dialog::ConfirmRemove { tickets, .. }) => assert_eq!(*tickets, 2),
            other => panic!("{other:?}"),
        }
        f.ch('s');
        assert_eq!(f.stored(), Vec::<PathBuf>::new());
        assert_eq!(f.app.db.count_tickets(id).unwrap(), 0);
        // Without tickets, the count is 0.
        let (mut f, _) = with_one_project();
        f.ch('r');
        match &f.app.dialog {
            Some(Dialog::ConfirmRemove { tickets, .. }) => assert_eq!(*tickets, 0),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn key_releases_are_ignored() {
        let mut f = fixture();
        let mut release = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        release.kind = KeyEventKind::Release;
        assert_eq!(f.app.handle_key(release), Action::None);
    }

    // ---- agents (NS-33) ----

    use crate::agents::{self, Stage};
    use crate::launch::{AgentLauncher, Precheck};

    /// A fake launcher: records the calls and returns the scripted results.
    struct FakeLauncher {
        checks: Vec<(PathBuf, PathBuf)>,
        spawns: Vec<(PathBuf, PathBuf, String, String)>,
        check: Result<Precheck, String>,
        spawn: Result<(), String>,
    }

    impl FakeLauncher {
        fn new(jail: bool) -> FakeLauncher {
            FakeLauncher {
                checks: Vec::new(),
                spawns: Vec::new(),
                check: Ok(Precheck {
                    jail,
                    warning: None,
                }),
                spawn: Ok(()),
            }
        }
    }

    impl AgentLauncher for FakeLauncher {
        fn check(&mut self, launcher: &Path, project: &Path) -> Result<Precheck, String> {
            self.checks
                .push((launcher.to_path_buf(), project.to_path_buf()));
            self.check.clone()
        }

        fn spawn(
            &mut self,
            launcher: &Path,
            project: &Path,
            session_id: &str,
            name: &str,
        ) -> Result<(), String> {
            self.spawns.push((
                launcher.to_path_buf(),
                project.to_path_buf(),
                session_id.to_string(),
                name.to_string(),
            ));
            self.spawn.clone()
        }
    }

    /// A project `proj` with the tickets NS-1 and NS-2 open, and the agents wired: the
    /// launcher at <root>/bin/nick-agent (a file) and the locks under <root>/state/agents.
    fn with_agents() -> (Fixture, PathBuf) {
        let (mut f, p) = with_tickets("proj");
        f.new_ticket("Primeiro", "NS-1");
        f.new_ticket("Segundo", "NS-2");
        let launcher = f.root.join("bin/nick-agent");
        fs::write(&launcher, "#!/usr/bin/env bash\n").unwrap();
        f.app.launcher = launcher;
        f.app.agents_dir = Some(f.root.join("state/agents"));
        f.key(KeyCode::Up);
        assert_eq!(f.keys(), ["NS-1", "NS-2"]);
        (f, p)
    }

    impl Fixture {
        fn agents_dir(&self) -> PathBuf {
            self.root.join("state/agents")
        }

        /// (session_id, stage, name, state) of every agent row.
        fn agent_rows(&self) -> Vec<(String, String, String, String)> {
            let mut stmt = self
                .app
                .db
                .connection()
                .prepare("SELECT session_id, stage, name, state FROM agents ORDER BY id")
                .unwrap();
            stmt.query_map([], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
            })
            .unwrap()
            .map(Result::unwrap)
            .collect()
        }

        fn stage_index(&self) -> usize {
            match &self.app.dialog {
                Some(Dialog::ChooseStage { index, .. }) => *index,
                other => panic!("not in the stage dialog: {other:?}"),
            }
        }

        /// `g`, `Down` x `downs`, `Enter`: the OpenAgent action.
        fn choose(&mut self, downs: usize) -> Action {
            assert_eq!(self.ch('g'), Action::None);
            for _ in 0..downs {
                self.key(KeyCode::Down);
            }
            self.key(KeyCode::Enter)
        }

        /// Chooses the stage and carries out the action with `launcher`.
        fn open_agent(&mut self, downs: usize, launcher: &mut FakeLauncher) {
            let Action::OpenAgent {
                project,
                ticket,
                stage,
            } = self.choose(downs)
            else {
                panic!("no OpenAgent");
            };
            self.app.open_agent(&project, &ticket, stage, launcher);
        }

        fn alert(&self) -> String {
            match &self.app.dialog {
                Some(Dialog::AgentError { message }) => message.clone(),
                other => panic!("not in the agent alert: {other:?}"),
            }
        }

        fn mods(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Action {
            self.app.handle_key(KeyEvent::new(code, modifiers))
        }
    }

    #[test]
    fn g_on_a_visible_ticket_opens_the_stage_dialog_and_the_arrows_move() {
        let (mut f, _) = with_agents();
        assert_eq!(f.ch('g'), Action::None);
        assert_eq!(f.stage_index(), 0);
        for (code, expected) in [
            (KeyCode::Down, 1),
            (KeyCode::Char('j'), 2),
            (KeyCode::Down, 3),
            (KeyCode::Down, 3),
            (KeyCode::Up, 2),
            (KeyCode::Char('k'), 1),
            (KeyCode::Up, 0),
            (KeyCode::Up, 0),
        ] {
            f.key(code);
            assert_eq!(f.stage_index(), expected, "{code:?}");
        }
    }

    #[test]
    fn esc_cancels_the_stage_dialog_and_q_does_not_quit_it() {
        let (mut f, _) = with_agents();
        f.ch('g');
        assert_eq!(f.ch('q'), Action::None);
        assert_eq!(f.stage_index(), 0, "q stays in the dialog");
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.app.screen, Screen::Tickets);
        f.ch('g');
        assert_eq!(f.ctrl_c(), Action::Quit, "Ctrl+C quits, as in every dialog");
        assert_eq!(f.agent_rows(), []);
    }

    #[test]
    fn enter_returns_open_agent_with_the_project_path_the_ticket_and_the_stage() {
        for (downs, stage) in Stage::ALL.into_iter().enumerate() {
            let (mut f, p) = with_agents();
            f.key(KeyCode::Down);
            let ticket = f.app.ticket_rows[1].clone();
            assert_eq!(
                f.choose(downs),
                Action::OpenAgent {
                    project: p,
                    ticket,
                    stage
                }
            );
            assert_eq!(f.app.dialog, None);
            assert_eq!(f.agent_rows(), [], "the action itself stores nothing");
        }
    }

    #[test]
    fn g_on_an_empty_list_does_nothing() {
        let (mut f, _) = with_tickets("vazio");
        assert_eq!(f.ch('g'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), None);
    }

    #[test]
    fn g_on_a_hidden_ticket_says_to_restore_it() {
        let (mut f, _) = with_agents();
        f.ch('o');
        f.ch('s');
        f.ch('f');
        assert_eq!(f.app.ticket_rows[0].key, "NS-1");
        assert!(f.app.ticket_rows[0].hidden_at.is_some());
        f.key(KeyCode::Up);
        assert_eq!(f.ch('g'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(
            f.status(),
            Some("O ticket NS-1 está oculto. Restaure-o (o) para abrir um agente.")
        );
    }

    #[test]
    fn g_on_an_unavailable_project_says_why() {
        for dir_missing in [true, false] {
            let (mut f, p) = with_agents();
            let mark = if dir_missing {
                fs::remove_dir_all(&p).unwrap();
                "[indisponível: pasta não encontrada]"
            } else {
                fs::remove_file(projects::config_file(&p)).unwrap();
                "[indisponível: sem .nick/nick.conf]"
            };
            assert_eq!(f.ch('g'), Action::None);
            assert_eq!(f.app.dialog, None);
            assert_eq!(
                f.status(),
                Some(
                    format!(
                        "Projeto indisponível: {mark}. O agente só abre com a pasta e o \
                         .nick/nick.conf no lugar."
                    )
                    .as_str()
                )
            );
        }
    }

    #[test]
    fn opening_runs_the_check_then_the_launch_and_stores_an_active_agent() {
        let (mut f, p) = with_agents();
        let mut launcher = FakeLauncher::new(false);
        f.open_agent(2, &mut launcher);
        let bin = f.root.join("bin/nick-agent");
        assert_eq!(launcher.checks, [(bin.clone(), p.clone())]);
        assert_eq!(launcher.spawns.len(), 1);
        let (spawned, project, session_id, name) = launcher.spawns[0].clone();
        assert_eq!((spawned, project), (bin, p));
        assert_eq!(name, "proj · Dev · NS-1");
        assert_eq!(
            f.agent_rows(),
            [(
                session_id.clone(),
                "development".to_string(),
                name.clone(),
                "active".to_string()
            )]
        );
        assert_eq!(f.app.screen, Screen::Agent);
        assert_eq!(f.app.dialog, None);
        let agent = f.app.agent.as_ref().expect("the running agent");
        assert_eq!(agent.name, name);
        assert!(!agent.jail);
        let lock = agents::lock_path(&f.agents_dir(), &session_id);
        let other = fs::File::open(&lock).unwrap();
        assert!(other.try_lock().is_err(), "the lock is held while it runs");
    }

    #[test]
    fn a_failing_check_shows_the_alert_and_opens_nothing() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(true);
        launcher.check = Err("Erro: a jail (ai-jail) não funciona nesta máquina.\nDetalhe".into());
        f.open_agent(0, &mut launcher);
        assert_eq!(
            f.alert(),
            "Nada foi aberto.\n\nErro: a jail (ai-jail) não funciona nesta máquina.\nDetalhe"
        );
        assert_eq!(launcher.checks.len(), 1);
        assert_eq!(launcher.spawns, []);
        assert_eq!(f.agent_rows(), []);
        assert_eq!(f.app.screen, Screen::Tickets);
        // q does not quit the alert; Esc and Enter close it.
        assert_eq!(f.ch('q'), Action::None);
        assert!(matches!(f.app.dialog, Some(Dialog::AgentError { .. })));
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.app.dialog, None);
    }

    #[test]
    fn a_missing_launcher_shows_its_path_and_runs_nothing() {
        let (mut f, _) = with_agents();
        let bin = f.root.join("bin/nick-agent");
        fs::remove_file(&bin).unwrap();
        let mut launcher = FakeLauncher::new(false);
        f.open_agent(0, &mut launcher);
        assert_eq!(
            f.alert(),
            format!(
                "Não encontrei o bin/nick-agent em {}. Nada foi aberto.",
                bin.display()
            )
        );
        assert_eq!(launcher.checks, []);
        assert_eq!(launcher.spawns, []);
        assert_eq!(f.agent_rows(), []);
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.dialog, None);
    }

    #[test]
    fn a_failing_spawn_ends_the_row_and_releases_the_lock() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(false);
        launcher.spawn = Err("sem PTY".into());
        f.open_agent(1, &mut launcher);
        assert_eq!(f.status(), Some("Não consegui abrir o agente: sem PTY."));
        assert_eq!(f.app.screen, Screen::Tickets);
        assert!(f.app.agent.is_none());
        let rows = f.agent_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].3, "ended");
        assert!(!agents::lock_path(&f.agents_dir(), &rows[0].0).exists());
    }

    fn running() -> (Fixture, FakeLauncher) {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(true);
        f.key(KeyCode::Down);
        f.open_agent(1, &mut launcher);
        assert_eq!(f.app.screen, Screen::Agent);
        (f, launcher)
    }

    #[test]
    fn on_the_agent_screen_every_key_goes_to_the_program() {
        let (mut f, _) = running();
        let send = |b: &[u8]| Action::Send(b.to_vec());
        assert_eq!(f.ch('q'), send(b"q"));
        assert_eq!(f.key(KeyCode::Esc), send(b"\x1b"));
        assert_eq!(f.ctrl_c(), send(b"\x03"), "Ctrl+C does not quit the nick");
        assert_eq!(
            f.mods(KeyCode::BackTab, KeyModifiers::SHIFT),
            send(b"\x1b[Z")
        );
        assert_eq!(f.key(KeyCode::Enter), send(b"\r"));
        assert_eq!(f.ch('á'), send("á".as_bytes()));
        assert_eq!(
            f.mods(KeyCode::Char('4'), KeyModifiers::CONTROL),
            send(b"\x1c")
        );
        assert_eq!(f.key(KeyCode::Up), send(b"\x1b[A"));
        f.app.agent_term.modes.application_cursor = true;
        assert_eq!(f.key(KeyCode::Up), send(b"\x1bOA"));
        assert_eq!(f.app.screen, Screen::Agent);
        assert_eq!(f.app.dialog, None);
    }

    #[test]
    fn shift_page_up_and_down_scroll_the_history() {
        let (mut f, _) = running();
        assert_eq!(
            f.mods(KeyCode::PageUp, KeyModifiers::SHIFT),
            Action::ScrollPageUp
        );
        assert_eq!(
            f.mods(KeyCode::PageDown, KeyModifiers::SHIFT),
            Action::ScrollPageDown
        );
        assert_eq!(f.key(KeyCode::PageUp), Action::Send(b"\x1b[5~".to_vec()));
    }

    #[test]
    fn a_paste_goes_to_the_program_through_encode_paste() {
        let (mut f, _) = running();
        assert_eq!(
            f.app.handle_paste("x\x1b[201~rm"),
            Action::Send(b"x\x1b[201~rm".to_vec())
        );
        f.app.agent_term.bracketed_paste = true;
        assert_eq!(
            f.app.handle_paste("x\x1b[201~rm"),
            Action::Send(crate::keys::encode_paste("x\x1b[201~rm", true))
        );
    }

    #[test]
    fn the_wheel_scrolls_unless_the_program_asked_for_the_mouse() {
        use crossterm::event::{MouseEvent, MouseEventKind};
        use ratatui::layout::Rect;
        let (mut f, _) = running();
        let area = Rect::new(0, 1, 80, 20);
        let wheel = |kind| MouseEvent {
            kind,
            column: 4,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            f.app.handle_mouse(&wheel(MouseEventKind::ScrollUp), area),
            Action::ScrollLines(3)
        );
        assert_eq!(
            f.app.handle_mouse(&wheel(MouseEventKind::ScrollDown), area),
            Action::ScrollLines(-3)
        );
        f.app.agent_term.mouse_mode = vt100::MouseProtocolMode::PressRelease;
        f.app.agent_term.mouse_encoding = vt100::MouseProtocolEncoding::Sgr;
        assert_eq!(
            f.app.handle_mouse(&wheel(MouseEventKind::ScrollUp), area),
            Action::Send(b"\x1b[<64;5;5M".to_vec())
        );
    }

    #[test]
    fn the_end_of_the_agent_goes_back_to_the_ticket_with_the_message() {
        for (code, message) in [
            (0, "O agente proj · Tech Leader · NS-2 terminou."),
            (
                3,
                "O agente proj · Tech Leader · NS-2 terminou com o código 3.",
            ),
        ] {
            let (mut f, launcher) = running();
            let session_id = launcher.spawns[0].2.clone();
            f.app.agent_ended(code);
            assert_eq!(f.app.screen, Screen::Tickets);
            assert_eq!(f.app.ticket_rows[f.app.ticket_index].key, "NS-2");
            assert_eq!(f.status(), Some(message));
            assert!(f.app.agent.is_none());
            assert_eq!(f.agent_rows()[0].3, "ended");
            assert!(!agents::lock_path(&f.agents_dir(), &session_id).exists());
            // Back on the list, q quits again.
            assert_eq!(f.ch('q'), Action::Quit);
        }
    }

    #[test]
    fn the_warning_of_the_check_shows_when_the_agent_ends() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(false);
        launcher.check = Ok(Precheck {
            jail: false,
            warning: Some("Aviso: falta chave.\n  X=\"\"".into()),
        });
        f.open_agent(0, &mut launcher);
        assert_eq!(f.app.screen, Screen::Agent);
        f.app.agent_ended(0);
        assert_eq!(
            f.status(),
            Some(
                "O agente proj · PO · NS-1 terminou. Aviso da verificação: Aviso: falta chave. X=\"\""
            )
        );
    }
}
