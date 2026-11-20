//! The state of the TUI and its transitions by key, with no terminal I/O: `handle_key` takes a
//! key and returns an `Action`, which the loop (main.rs) carries out. The only action that runs
//! bin/nick-init is `Action::RunInit`, and only the "Criar configuração" button of the
//! missing-config alert produces it (plan D7 of NS-31). The navigation is Modules > Projects >
//! Tickets; the rules of the tickets are in tickets.rs, and none of them touches a file of the
//! project. From a ticket, `g` opens an agent (NS-33) by the flow of auto_scrum.sh (NS-35): the
//! check of the launcher (`Action::CheckAgent`), the menu of the stage types its flags offer, the
//! form of the type (stages.rs; the long text in `$EDITOR`, `Action::EditDescription`), the
//! prompt built by bash (`Action::RenderPrompt`), its preview with the `(s/N)` confirmation, and
//! `Action::OpenAgent`, which the loop hands to `open_agent` with the real `AgentLauncher`
//! (launch.rs). Several agents
//! run at once (NS-34): the list stays on the left and the shown agent on the right, and the
//! reserved shortcut (`keys::Shortcut`, kept in the database) toggles the focus between them;
//! with the focus on the terminal every other key goes to the program (`Action::Send`). The
//! screen texts are in PT-BR, and drawing is ui.rs's job.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

use crate::agents::{self, OpenAgent};
use crate::db::{Db, SETTING_RESERVED_SHORTCUT};
use crate::editor::{EditorOutcome, EditorRunner};
use crate::init::{InitOutcome, InitRunner};
use crate::keys::{self, Shortcut, TermModes};
use crate::launch::{self, AgentLauncher, Precheck, RenderRequest, Rendered};
use crate::modules::ModuleKind;
use crate::paths::printable;
use crate::projects::{self, Availability, Check, Project};
use crate::stages::{self, Input, StageType};
use crate::tickets::{self, Ticket};

/// The list on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Modules,
    Projects,
    /// The tickets of the open project (`App::project`).
    Tickets,
    /// "Agentes ativos": every agent this nick runs, of every project (NS-34).
    ActiveAgents,
    /// "Configurações": the reserved shortcut (NS-34).
    Settings,
}

/// Where the keys go while an agent runs (NS-34): the list on the left or the terminal of the
/// shown agent on the right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    List,
    Terminal,
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
    /// "Abrir agente": the type of the stage for the ticket of the project at `project`, among
    /// `types` (the menu of `choose_type` with the flags of the check), `index` selected.
    ChooseType {
        project: PathBuf,
        ticket: Ticket,
        precheck: Precheck,
        types: Vec<StageType>,
        index: usize,
    },
    /// The form of `kind`: one value per field (stages.rs), `focus` the field being answered.
    StageForm {
        project: PathBuf,
        ticket: Ticket,
        precheck: Precheck,
        kind: StageType,
        values: Vec<String>,
        focus: usize,
    },
    /// The prompt bash built, before the launch: the `(s/N)` confirmation, with the prompt
    /// scrolled `scroll` lines down.
    ConfirmPrompt {
        project: PathBuf,
        ticket: Ticket,
        precheck: Precheck,
        kind: StageType,
        rendered: Rendered,
        scroll: usize,
    },
    /// Why no agent was opened (the launcher is missing, or its check failed).
    AgentError { message: String },
    /// The `(s/N)` confirmation of ending the agent `id` from "Agentes ativos".
    ConfirmEndAgent { id: i64, name: String },
    /// The `(s/N)` confirmation of quitting with agents running (they are all ended).
    ConfirmQuit,
    /// The capture of a new reserved shortcut: `first` is the key pressed once, which must be
    /// pressed again to be saved.
    CaptureShortcut { first: Option<Shortcut> },
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
    /// Check the launcher before the menu of the types (`g` on a ticket); the loop calls
    /// `check_agent`.
    CheckAgent {
        project: PathBuf,
        ticket: Ticket,
    },
    /// Open the editor on the focused field of the form; the loop calls `edit_description`.
    EditDescription,
    /// Build the prompt of the form (its answers passed the rules); the loop calls
    /// `render_prompt`.
    RenderPrompt,
    /// Open an agent of `kind` for `ticket`, with the cwd at `project` (the path in the
    /// database) and the prompt of `log` (confirmed on the preview); the loop calls
    /// `open_agent`.
    OpenAgent {
        project: PathBuf,
        ticket: Ticket,
        kind: StageType,
        log: PathBuf,
        precheck: Precheck,
    },
    /// Write these bytes to the agent's terminal.
    Send(Vec<u8>),
    /// Scroll the agent's history one page up (into the history) or down.
    ScrollPageUp,
    ScrollPageDown,
    /// Scroll the agent's history by lines (positive: up, into the history).
    ScrollLines(i32),
    /// End the agent with this id (confirmed from "Agentes ativos"): the loop terminates its
    /// session and calls `agent_ended_by_operator`.
    EndAgent(i64),
    /// Quit, ending every agent (confirmed): the loop terminates every session and calls
    /// `end_all_records`.
    QuitEndingAgents,
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

/// An agent being run: its record (the active row and the lock) and what the screen shows.
#[derive(Debug)]
pub struct RunningAgent {
    record: OpenAgent,
    /// The `id` of its row: its events and its session in the loop carry it.
    pub id: i64,
    /// The `claude --name`.
    pub name: String,
    /// The name of its project.
    pub project: String,
    /// Whether it runs in the jail (the `jail=` of the check). Only in memory: every agent of
    /// the view is of this process (NS-34, not a column).
    pub jail: bool,
    /// What the check wrote on stderr, shown when the agent ends.
    warning: Option<String>,
    /// The text of its screen when it stopped being shown (`hidden_output`).
    seen_text: String,
    /// Marked: its text changed while hidden ("● saída nova").
    pub unseen: bool,
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
    /// The agents running, in the order they were opened.
    pub(crate) agents: Vec<RunningAgent>,
    /// The id of the agent shown on the right, if any.
    pub(crate) shown: Option<i64>,
    pub(crate) focus: Focus,
    /// What the shown agent's program asked of its terminal.
    pub(crate) agent_term: AgentTerm,
    /// The selection of "Agentes ativos".
    pub(crate) agent_index: usize,
    /// The screen "Agentes ativos" goes back to.
    pub(crate) back: Screen,
    /// The reserved shortcut in use, and whether the database has a row for it (no row: the
    /// default).
    pub(crate) shortcut: Shortcut,
    pub(crate) shortcut_stored: bool,
    /// The window of the preview of the prompt: the rows shown at a time (a page) and the
    /// largest scroll (its end). The loop computes it from the screen (`ui::prompt_window`) and
    /// sets it before each key, so the App does not know the drawing.
    pub(crate) prompt_window: (usize, usize),
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
            agents: Vec::new(),
            shown: None,
            focus: Focus::List,
            agent_term: AgentTerm::default(),
            agent_index: 0,
            back: Screen::Modules,
            shortcut: Shortcut::DEFAULT,
            shortcut_stored: false,
            prompt_window: (0, 0),
        }
    }

    /// The window of the preview of the prompt (rows shown, largest scroll), set by the loop
    /// before each key while the preview is open.
    pub fn set_prompt_window(&mut self, rows: usize, max: usize) {
        self.prompt_window = (rows, max);
    }

    /// The preview of the prompt, when it is open: the check and the prompt bash built.
    pub fn prompt_preview(&self) -> Option<(&Precheck, &Rendered)> {
        match &self.dialog {
            Some(Dialog::ConfirmPrompt {
                precheck, rendered, ..
            }) => Some((precheck, rendered)),
            _ => None,
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

    /// The agents running, in the order they were opened.
    pub fn agents(&self) -> &[RunningAgent] {
        &self.agents
    }

    /// The running agent `id`, if any.
    pub fn agent(&self, id: i64) -> Option<&RunningAgent> {
        self.agents.iter().find(|agent| agent.id == id)
    }

    /// The id of the agent shown on the right.
    pub fn shown(&self) -> Option<i64> {
        self.shown
    }

    /// The agent shown on the right.
    pub fn shown_agent(&self) -> Option<&RunningAgent> {
        self.shown.and_then(|id| self.agent(id))
    }

    /// Where the keys go.
    pub fn focus(&self) -> Focus {
        self.focus
    }

    /// The reserved shortcut in use.
    pub fn shortcut(&self) -> Shortcut {
        self.shortcut
    }

    /// Reads the reserved shortcut of the database (when the nick opens). A value that is not
    /// a valid shortcut today falls back to the default; the message for the status bar says
    /// so, or is the error of the database.
    pub fn load_shortcut(&mut self) -> Option<String> {
        match self.db.get_setting(SETTING_RESERVED_SHORTCUT) {
            Err(err) => Some(projects::db_error_message(&err)),
            Ok(None) => None,
            Ok(Some(value)) => {
                let valid = Shortcut::from_db(&value)
                    .filter(|shortcut| keys::shortcut_refusal(shortcut).is_none());
                match valid {
                    Some(shortcut) => {
                        self.shortcut = shortcut;
                        self.shortcut_stored = true;
                        None
                    }
                    None => Some(format!(
                        "O atalho gravado no banco ({}) não é válido. Usando o padrão {}.",
                        printable(&value),
                        Shortcut::DEFAULT.label()
                    )),
                }
            }
        }
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
            Action::Quit | Action::QuitEndingAgents => true,
            Action::RunInit { project } => {
                let outcome = runner.run(&self.init, &project);
                self.after_init(outcome);
                false
            }
            // The agents are carried out by the loop (main.rs).
            Action::None
            | Action::CheckAgent { .. }
            | Action::EditDescription
            | Action::RenderPrompt
            | Action::OpenAgent { .. }
            | Action::Send(_)
            | Action::ScrollPageUp
            | Action::ScrollPageDown
            | Action::ScrollLines(_)
            | Action::EndAgent(_) => false,
        }
    }

    /// The transition of a key.
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
        }
        // The capture of a new shortcut takes every key, the current shortcut and Ctrl+C
        // included (Ctrl+C is refused there, and does not quit).
        if let Some(Dialog::CaptureShortcut { first }) = self.dialog {
            self.dialog = None;
            return self.key_capture(key, first);
        }
        // The reserved shortcut toggles the focus before anything else and never reaches the
        // program; only a press counts (holding it must not blink the focus).
        if self.shortcut.matches(&key) {
            if key.kind == KeyEventKind::Press {
                self.toggle_focus();
            }
            return Action::None;
        }
        // With the focus on the terminal every other key is the program's, Ctrl+C included.
        if self.focus == Focus::Terminal {
            return self.key_agent(key);
        }
        // Ctrl+C quits from anywhere: raw mode delivers it as a key, and the operator can't
        // be stuck. With agents running, it asks first.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return self.quit();
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
            Some(Dialog::ChooseType {
                project,
                ticket,
                precheck,
                types,
                index,
            }) => self.key_choose_type(key, project, ticket, precheck, types, index),
            Some(Dialog::StageForm {
                project,
                ticket,
                precheck,
                kind,
                values,
                focus,
            }) => self.key_stage_form(key, project, ticket, precheck, kind, values, focus),
            Some(Dialog::ConfirmPrompt {
                project,
                ticket,
                precheck,
                kind,
                rendered,
                scroll,
            }) => self.key_confirm_prompt(key, project, ticket, precheck, kind, rendered, scroll),
            Some(Dialog::AgentError { message }) => self.key_agent_error(key, message),
            Some(Dialog::ConfirmEndAgent { id, .. }) => confirmed(key, Action::EndAgent(id)),
            Some(Dialog::ConfirmQuit) => confirmed(key, Action::QuitEndingAgents),
            // Taken before the dialogs (above).
            Some(Dialog::CaptureShortcut { .. }) => Action::None,
            None => match self.screen {
                Screen::Modules => self.key_modules(key),
                Screen::Projects => self.key_projects(key),
                Screen::Tickets => self.key_tickets(key),
                Screen::ActiveAgents => self.key_active_agents(key),
                Screen::Settings => self.key_settings(key),
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
                Screen::Modules | Screen::ActiveAgents | Screen::Settings => {}
            }
        }
        action
    }

    /// The reserved shortcut: from the terminal back to the list; from the list to the terminal
    /// of the shown agent, but not with a dialog open (it is a pending question) nor without
    /// an agent shown.
    fn toggle_focus(&mut self) {
        match self.focus {
            Focus::Terminal => self.focus = Focus::List,
            Focus::List if self.dialog.is_none() && self.shown.is_some() => {
                self.focus = Focus::Terminal;
            }
            Focus::List => {}
        }
    }

    /// `q` on a list or Ctrl+C with the focus on it: quits right away without agents (as
    /// before the NS-34), or asks to end them all.
    fn quit(&mut self) -> Action {
        if self.agents.is_empty() {
            return Action::Quit;
        }
        self.dialog = Some(Dialog::ConfirmQuit);
        Action::None
    }

    /// `v` on a list: the view "Agentes ativos", with the shown agent selected; `Esc` goes back
    /// to `screen`.
    fn open_active_agents(&mut self) {
        self.back = self.screen;
        self.screen = Screen::ActiveAgents;
        self.agent_index = self
            .shown
            .and_then(|id| self.agents.iter().position(|agent| agent.id == id))
            .unwrap_or(0);
        self.status = None;
    }

    fn key_active_agents(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.agent_index = self.agent_index.saturating_sub(1);
                self.show_selected();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.agent_index = (self.agent_index + 1).min(self.agents.len().saturating_sub(1));
                self.show_selected();
            }
            KeyCode::Enter | KeyCode::Right => {
                if self.agents.get(self.agent_index).is_some() {
                    self.show_selected();
                    self.focus = Focus::Terminal;
                }
            }
            KeyCode::Char('x') => {
                if let Some(agent) = self.agents.get(self.agent_index) {
                    self.dialog = Some(Dialog::ConfirmEndAgent {
                        id: agent.id,
                        name: agent.name.clone(),
                    });
                }
            }
            KeyCode::Esc | KeyCode::Left => {
                self.screen = self.back;
                self.status = None;
            }
            _ if is_quit(key) => return self.quit(),
            _ => {}
        }
        Action::None
    }

    /// Shows the agent selected on "Agentes ativos".
    fn show_selected(&mut self) {
        if let Some(id) = self.agents.get(self.agent_index).map(|agent| agent.id) {
            self.show(id);
        }
    }

    /// Shows the agent `id` on the right: being shown counts as seen, so its mark goes. The
    /// loop keeps the text of the one that stopped being shown (`left_shown`).
    fn show(&mut self, id: i64) {
        if self.shown != Some(id) {
            self.agent_term = AgentTerm::default();
        }
        self.shown = Some(id);
        if let Some(agent) = self.agents.iter_mut().find(|agent| agent.id == id) {
            agent.unseen = false;
        }
    }

    /// The agent `id` stopped being shown, with `text` on its screen (vt100 `contents`).
    pub fn left_shown(&mut self, id: i64, text: String) {
        if let Some(agent) = self.agents.iter_mut().find(|agent| agent.id == id) {
            agent.seen_text = text;
        }
    }

    /// Whether an output of the agent `id` needs its text for `hidden_output`: only a hidden
    /// agent not marked yet (a marked one costs nothing until it is seen).
    pub fn wants_hidden_text(&self, id: i64) -> bool {
        self.shown != Some(id) && self.agent(id).is_some_and(|agent| !agent.unseen)
    }

    /// The rule of the "saída nova" mark (NS-34): an output of the hidden agent `id` whose
    /// visible `text` differs from the text it had when it stopped being shown marks it. A
    /// redraw of the same text (colors, cursor) does not. Returns true when the mark changed.
    pub fn hidden_output(&mut self, id: i64, text: &str) -> bool {
        if !self.wants_hidden_text(id) {
            return false;
        }
        let Some(agent) = self.agents.iter_mut().find(|agent| agent.id == id) else {
            return false;
        };
        if agent.seen_text == text {
            return false;
        }
        agent.unseen = true;
        true
    }

    fn key_settings(&mut self, key: KeyEvent) -> Action {
        match key.code {
            KeyCode::Char('t') => {
                self.status = None;
                self.dialog = Some(Dialog::CaptureShortcut { first: None });
            }
            KeyCode::Char('p') => match self.db.delete_setting(SETTING_RESERVED_SHORTCUT) {
                Ok(()) => {
                    self.shortcut = Shortcut::DEFAULT;
                    self.shortcut_stored = false;
                    self.status = Some(format!(
                        "Atalho de volta ao padrão: {}.",
                        Shortcut::DEFAULT.label()
                    ));
                }
                Err(err) => self.status = Some(projects::db_error_message(&err)),
            },
            KeyCode::Esc | KeyCode::Left => {
                self.screen = Screen::Modules;
                self.status = None;
            }
            _ if is_quit(key) => return self.quit(),
            _ => {}
        }
        Action::None
    }

    /// A key in the capture of a new shortcut: Esc cancels; a refused key says why and starts
    /// over; a valid key once becomes `first`, and the same key again (in the canonical form)
    /// is saved. The second press proves that the terminal delivers that key to the nick.
    fn key_capture(&mut self, key: KeyEvent, first: Option<Shortcut>) -> Action {
        if key.kind == KeyEventKind::Repeat {
            self.dialog = Some(Dialog::CaptureShortcut { first });
            return Action::None;
        }
        if key.code == KeyCode::Esc {
            self.status = None;
            return Action::None;
        }
        match keys::shortcut_of_key(&key) {
            Err(message) => {
                self.status = Some(message);
                self.dialog = Some(Dialog::CaptureShortcut { first: None });
            }
            Ok(shortcut) if first == Some(shortcut) => {
                match self
                    .db
                    .set_setting(SETTING_RESERVED_SHORTCUT, &shortcut.to_db())
                {
                    Ok(()) => {
                        self.shortcut = shortcut;
                        self.shortcut_stored = true;
                        self.status = Some(format!("Atalho salvo: {}.", shortcut.label()));
                    }
                    Err(err) => self.status = Some(projects::db_error_message(&err)),
                }
            }
            Ok(shortcut) => {
                self.status = None;
                self.dialog = Some(Dialog::CaptureShortcut {
                    first: Some(shortcut),
                });
            }
        }
        Action::None
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
            KeyCode::Char('c') => {
                self.screen = Screen::Settings;
                self.status = None;
            }
            KeyCode::Char('v') => self.open_active_agents(),
            _ if is_quit(key) => return self.quit(),
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
            KeyCode::Char('v') => self.open_active_agents(),
            KeyCode::Esc | KeyCode::Left => {
                self.screen = Screen::Modules;
                self.status = None;
            }
            _ if is_quit(key) => return self.quit(),
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
            KeyCode::Char('g') => return self.choose_stage(),
            KeyCode::Char('v') => self.open_active_agents(),
            KeyCode::Esc | KeyCode::Left => {
                self.screen = Screen::Projects;
                self.project = None;
                self.ticket_rows.clear();
                self.show_hidden = false;
                self.status = None;
            }
            _ if is_quit(key) => return self.quit(),
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
        if is_yes(key) {
            self.status = Some(match tickets::hide(&self.db, &ticket) {
                Ok(message) | Err(message) => message,
            });
        }
        Action::None
    }

    /// `g` on a ticket: the check of the launcher (`Action::CheckAgent`), then the menu, only for
    /// a visible ticket of a project available now (read from the disk at the key, like the Enter
    /// on a project).
    fn choose_stage(&mut self) -> Action {
        let Some(ticket) = self.ticket_rows.get(self.ticket_index).cloned() else {
            return Action::None;
        };
        if ticket.hidden_at.is_some() {
            self.status = Some(format!(
                "O ticket {} está oculto. Restaure-o (o) para abrir um agente.",
                ticket.key
            ));
            return Action::None;
        }
        let Some(project) = self.project.as_ref() else {
            return Action::None;
        };
        if let Some(mark) = projects::unavailable_text(projects::availability(&project.path)) {
            self.status = Some(format!(
                "Projeto indisponível: {mark}. O agente só abre com a pasta e o \
                 .nick/nick.conf no lugar."
            ));
            return Action::None;
        }
        Action::CheckAgent {
            project: project.path.clone(),
            ticket,
        }
    }

    /// The check before the menu (plan D7 of NS-35): the launcher must be a file, and its
    /// `--check` must pass (a failure becomes the alert, with `Nada foi aberto.` on top, and
    /// nothing is stored nor launched). Then the menu of the types the flags of the project offer
    /// (`StageType::menu`).
    pub fn check_agent(
        &mut self,
        project: &Path,
        ticket: &Ticket,
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
        match launcher.check(&self.launcher, project) {
            Ok(precheck) => {
                self.dialog = Some(Dialog::ChooseType {
                    project: project.to_path_buf(),
                    ticket: ticket.clone(),
                    types: StageType::menu(precheck.jira, precheck.sentry),
                    precheck,
                    index: 0,
                });
            }
            Err(message) => self.agent_failed(&message),
        }
    }

    /// The alert of an agent not opened (a failing `--check` or `--render`), with `Nada foi
    /// aberto.` first: the error of the jail is long, and a short terminal cuts the end of the
    /// alert.
    fn agent_failed(&mut self, message: &str) {
        self.dialog = Some(Dialog::AgentError {
            message: format!("Nada foi aberto.\n\n{message}"),
        });
    }

    fn key_choose_type(
        &mut self,
        key: KeyEvent,
        project: PathBuf,
        ticket: Ticket,
        precheck: Precheck,
        types: Vec<StageType>,
        mut index: usize,
    ) -> Action {
        match key.code {
            KeyCode::Esc => return Action::None,
            KeyCode::Enter => {
                if let Some(kind) = types.get(index).copied() {
                    self.dialog = Some(Dialog::StageForm {
                        values: kind.initial_values(&ticket.key),
                        project,
                        ticket,
                        precheck,
                        kind,
                        focus: 0,
                    });
                    return Action::None;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => index = index.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                index = (index + 1).min(types.len().saturating_sub(1));
            }
            _ => {}
        }
        self.dialog = Some(Dialog::ChooseType {
            project,
            ticket,
            precheck,
            types,
            index,
        });
        Action::None
    }

    #[allow(clippy::too_many_arguments)]
    fn key_stage_form(
        &mut self,
        key: KeyEvent,
        project: PathBuf,
        ticket: Ticket,
        precheck: Precheck,
        kind: StageType,
        mut values: Vec<String>,
        mut focus: usize,
    ) -> Action {
        let fields = kind.fields();
        let count = fields.len();
        let mut action = Action::None;
        match key.code {
            KeyCode::Esc => {
                // Nothing was written: the log only exists after --render.
                self.status = None;
                return Action::None;
            }
            KeyCode::Tab | KeyCode::Down => focus = (focus + 1) % count,
            KeyCode::BackTab | KeyCode::Up => focus = (focus + count - 1) % count,
            KeyCode::Enter if fields[focus].input == Input::Editor => {
                action = Action::EditDescription;
            }
            KeyCode::Enter => match stages::render_args(kind, &values) {
                Ok(_) => {
                    self.status = None;
                    action = Action::RenderPrompt;
                }
                Err(refusal) => {
                    self.status = Some(refusal.message);
                    focus = refusal.field;
                }
            },
            // An editor field is not typed into: Enter opens the editor.
            _ if fields[focus].input == Input::Line => {
                edit_text(&mut values[focus], key);
            }
            _ => {}
        }
        self.dialog = Some(Dialog::StageForm {
            project,
            ticket,
            precheck,
            kind,
            values,
            focus,
        });
        action
    }

    /// `Action::EditDescription`: the editor on the focused field of the form, with its current
    /// text (plan D8 of NS-35). A text fills the field and the focus goes to the next one; an
    /// empty file (or one with only line breaks) leaves it empty, with the message of
    /// `read_via_editor`, and the operator decides (Enter opens it again, Esc gives up: unlike the
    /// script, no loop); an editor that does not open says why.
    pub fn edit_description(&mut self, runner: &mut dyn EditorRunner) {
        let Some(Dialog::StageForm {
            kind,
            values,
            focus,
            ..
        }) = self.dialog.as_mut()
        else {
            return;
        };
        let fields = kind.fields();
        let Some(field) = fields.get(*focus).filter(|f| f.input == Input::Editor) else {
            return;
        };
        let label = stages::field_label(field, "");
        match runner.edit(&label, &values[*focus]) {
            EditorOutcome::Text(text) if !text.is_empty() => {
                values[*focus] = text;
                *focus = (*focus + 1) % fields.len();
                self.status = None;
            }
            EditorOutcome::Text(_) | EditorOutcome::Empty => {
                values[*focus].clear();
                self.status = Some("Esse campo é obrigatório, tente de novo.".into());
            }
            EditorOutcome::Failed { editor, error } => {
                self.status = Some(format!("Não consegui abrir o editor {editor}: {error}."));
            }
        }
    }

    /// `Action::RenderPrompt`: `bin/nick-agent --render` with the answers of the form (the
    /// prompt is built by bash, into its log). A failure becomes the alert, with `Nada foi
    /// aberto.` on top; a success opens the preview.
    pub fn render_prompt(&mut self, launcher: &mut dyn AgentLauncher) {
        let Some(Dialog::StageForm {
            project,
            ticket,
            precheck,
            kind,
            values,
            ..
        }) = self.dialog.clone()
        else {
            return;
        };
        let answers = match stages::render_args(kind, &values) {
            Ok(answers) => answers,
            Err(refusal) => {
                self.status = Some(refusal.message);
                return;
            }
        };
        let request = RenderRequest { kind, answers };
        match launcher.render(&self.launcher, &project, &request) {
            Ok(rendered) => {
                self.dialog = Some(Dialog::ConfirmPrompt {
                    project,
                    ticket,
                    precheck,
                    kind,
                    rendered,
                    scroll: 0,
                });
            }
            Err(message) => self.agent_failed(&message),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn key_confirm_prompt(
        &mut self,
        key: KeyEvent,
        project: PathBuf,
        ticket: Ticket,
        precheck: Precheck,
        kind: StageType,
        rendered: Rendered,
        mut scroll: usize,
    ) -> Action {
        if is_yes(key) {
            return Action::OpenAgent {
                project,
                ticket,
                kind,
                log: rendered.log,
                precheck,
            };
        }
        if matches!(
            key.code,
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N')
        ) {
            // The text of auto_scrum.sh: the log stays where --render wrote it.
            self.status = Some(format!(
                "Cancelado. O prompt gerado continua salvo em: {}",
                rendered.log.display()
            ));
            return Action::None;
        }
        let (rows, max) = self.prompt_window;
        let page = rows.max(1);
        scroll = match key.code {
            KeyCode::Up | KeyCode::Char('k') => scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => scroll + 1,
            KeyCode::PageUp => scroll.saturating_sub(page),
            KeyCode::PageDown => scroll + page,
            KeyCode::Home => 0,
            KeyCode::End => max,
            _ => scroll,
        }
        .min(max);
        self.dialog = Some(Dialog::ConfirmPrompt {
            project,
            ticket,
            precheck,
            kind,
            rendered,
            scroll,
        });
        Action::None
    }

    fn key_agent_error(&mut self, key: KeyEvent, message: String) -> Action {
        if !matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
            self.dialog = Some(Dialog::AgentError { message });
        }
        Action::None
    }

    /// Opens an agent of `kind` for `ticket` with the cwd at `project`, with the prompt of `log`
    /// (confirmed on the preview), and the jail and the warning of `precheck` (plan D9 of NS-33,
    /// D7 of NS-35): the check is not run again (the launch repeats the checks in bash). It
    /// generates the session ID and the name (the stage of the type and the ID of the ticket),
    /// takes the lock and stores the `active` row (agents.rs), and only then starts the PTY. A
    /// failing start ends the row and releases the lock. The agent joins the running ones, is
    /// shown and gets the focus; the list stays on the same ticket.
    pub fn open_agent(
        &mut self,
        project: &Path,
        ticket: &Ticket,
        kind: StageType,
        log: &Path,
        check: Precheck,
        launcher: &mut dyn AgentLauncher,
    ) {
        let stage = kind.stage();
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
        let id = record.agent.id;
        if let Err(err) = launcher.spawn(&self.launcher, project, id, &session_id, &name, kind, log)
        {
            let mut message = format!("Não consegui abrir o agente: {err}.");
            if let Err(db) = agents::end(&self.db, record) {
                message = format!("{message} {db}");
            }
            self.status = Some(message);
            return;
        }
        let project = project_name.to_string();
        self.agents.push(RunningAgent {
            record,
            id,
            name,
            project,
            jail: check.jail,
            warning: check.warning,
            seen_text: String::new(),
            unseen: false,
        });
        self.show(id);
        self.focus = Focus::Terminal;
        self.status = None;
    }

    /// Takes the agent `id` out of the running ones. When it was the shown one, the agent
    /// opened most recently is shown (or none) and the focus goes to the list; otherwise the
    /// focus and the shown agent stay.
    fn take_agent(&mut self, id: i64) -> Option<RunningAgent> {
        let index = self.agents.iter().position(|agent| agent.id == id)?;
        let agent = self.agents.remove(index);
        if self.shown == Some(id) {
            self.shown = None;
            self.focus = Focus::List;
            self.agent_term = AgentTerm::default();
            if let Some(last) = self.agents.last().map(|agent| agent.id) {
                self.show(last);
            }
        }
        if self.agents.is_empty() {
            self.focus = Focus::List;
        }
        self.agent_index = self
            .shown
            .and_then(|shown| self.agents.iter().position(|agent| agent.id == shown))
            .unwrap_or(0);
        Some(agent)
    }

    /// The program of the agent `id` ended with `code`: the row becomes `ended` and the lock
    /// goes (agents.rs), it leaves the running ones (`take_agent`), and the status says so
    /// (plus the warning of the check, when there was one). The screen on the left stays, with
    /// the tickets read again.
    pub fn agent_ended(&mut self, id: i64, code: u32) {
        let Some(agent) = self.take_agent(id) else {
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
        self.status = Some(message);
        self.refresh_tickets();
    }

    /// The operator ended the agent `id` (`Action::EndAgent`, its session already terminated
    /// by the loop): as `agent_ended`, with the message of an end by the operator.
    pub fn agent_ended_by_operator(&mut self, id: i64) {
        let Some(agent) = self.take_agent(id) else {
            return;
        };
        let mut message = format!("O agente {} foi encerrado.", agent.name);
        if let Err(err) = agents::end(&self.db, agent.record) {
            message = format!("{message} {err}");
        }
        self.status = Some(message);
        self.refresh_tickets();
    }

    /// Ends the records of every running agent (the nick is leaving, its sessions already
    /// terminated): each row becomes `ended` and each lock goes, one by one, even after an
    /// error, so no lock is left held. Returns the first error of the database, if any.
    pub fn end_all_records(&mut self) -> Result<(), String> {
        self.shown = None;
        self.focus = Focus::List;
        let mut first = Ok(());
        for agent in std::mem::take(&mut self.agents) {
            let ended = agents::end(&self.db, agent.record);
            if first.is_ok() {
                first = ended;
            }
        }
        first
    }

    /// A key with the focus on the terminal: Shift+PageUp/PageDown scroll the history;
    /// everything else is encoded as an xterm would and sent to the program.
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

    /// Whether the input goes to the shown agent's program: the focus is on its terminal.
    fn on_terminal(&self) -> bool {
        self.focus == Focus::Terminal && self.shown.is_some()
    }

    /// A paste: with the focus on the terminal it goes to the program (bracketed and cleaned
    /// when the program asked for it); on the list nothing (bracketed paste is only on there).
    pub fn handle_paste(&mut self, text: &str) -> Action {
        if !self.on_terminal() {
            return Action::None;
        }
        Action::Send(keys::encode_paste(text, self.agent_term.bracketed_paste))
    }

    /// A mouse event over `area` (the agent's terminal on the screen), with the focus on the
    /// terminal: to the program when it asked for the mouse; otherwise the wheel scrolls the
    /// history and the rest is ignored. On the list the mouse is the outer terminal's (it is
    /// not captured there).
    pub fn handle_mouse(&mut self, event: &MouseEvent, area: Rect) -> Action {
        if !self.on_terminal() {
            return Action::None;
        }
        let term = self.agent_term;
        if term.mouse_mode != MouseProtocolMode::None {
            return keys::encode_mouse(event, term.mouse_mode, term.mouse_encoding, area)
                .map_or(Action::None, Action::Send);
        }
        // The wheel over the list (beside the terminal since the NS-34) scrolls nothing.
        if !area.contains(ratatui::layout::Position::new(event.column, event.row)) {
            return Action::None;
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
        if is_yes(key) {
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

/// `s` or `S`: the "sim" of the `(s/N)` confirmations (of auto_scrum too); any other key is no.
fn is_yes(key: KeyEvent) -> bool {
    matches!(key.code, KeyCode::Char('s') | KeyCode::Char('S'))
}

/// The answer of an `(s/N)` confirmation: only s/S give `action`; any other key cancels.
fn confirmed(key: KeyEvent, action: Action) -> Action {
    if is_yes(key) { action } else { Action::None }
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

    // ---- agents (NS-33; the flow of the stages, NS-35) ----

    use crate::agents::{self, Stage};
    use crate::editor::{EditorOutcome, EditorRunner};
    use crate::launch::{AgentLauncher, Precheck, RenderRequest, Rendered};
    use crate::stages::StageType;

    /// A fake launcher: records the calls and returns the scripted results. `ids` are the
    /// agent ids of the spawns (NS-34), and `kinds` the type and the prompt file of each spawn
    /// (NS-35), apart so `spawns` keeps the shape of the NS-33.
    struct FakeLauncher {
        checks: Vec<(PathBuf, PathBuf)>,
        renders: Vec<(PathBuf, PathBuf, RenderRequest)>,
        spawns: Vec<(PathBuf, PathBuf, String, String)>,
        ids: Vec<i64>,
        kinds: Vec<(StageType, PathBuf)>,
        check: Result<Precheck, String>,
        render: Result<Rendered, String>,
        spawn: Result<(), String>,
    }

    /// The log of the fake render.
    const LOG: &str = "/logs/po_discussion_20261119_120000.md";

    /// A rendered prompt of `rows` lines (`linha 1`, `linha 2`, ...).
    fn rendered(rows: usize) -> Rendered {
        Rendered {
            log: PathBuf::from(LOG),
            prompt: (1..=rows).map(|i| format!("linha {i}\n")).collect(),
            status: "Jail (ai-jail): desligada — o agente roda direto no seu shell.".into(),
            warning: None,
        }
    }

    impl FakeLauncher {
        fn new(jail: bool) -> FakeLauncher {
            FakeLauncher {
                checks: Vec::new(),
                renders: Vec::new(),
                spawns: Vec::new(),
                ids: Vec::new(),
                kinds: Vec::new(),
                check: Ok(Precheck {
                    jail,
                    jira: false,
                    sentry: false,
                    warning: None,
                }),
                render: Ok(rendered(3)),
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

        fn render(
            &mut self,
            launcher: &Path,
            project: &Path,
            request: &RenderRequest,
        ) -> Result<Rendered, String> {
            self.renders.push((
                launcher.to_path_buf(),
                project.to_path_buf(),
                request.clone(),
            ));
            self.render.clone()
        }

        fn spawn(
            &mut self,
            launcher: &Path,
            project: &Path,
            agent_id: i64,
            session_id: &str,
            name: &str,
            kind: StageType,
            prompt_file: &Path,
        ) -> Result<(), String> {
            self.ids.push(agent_id);
            self.kinds.push((kind, prompt_file.to_path_buf()));
            self.spawns.push((
                launcher.to_path_buf(),
                project.to_path_buf(),
                session_id.to_string(),
                name.to_string(),
            ));
            self.spawn.clone()
        }
    }

    /// A fake editor: records (label, text) of each call and returns the next outcome.
    struct FakeEditor {
        calls: Vec<(String, String)>,
        outcomes: Vec<EditorOutcome>,
    }

    impl FakeEditor {
        fn new(outcomes: Vec<EditorOutcome>) -> FakeEditor {
            FakeEditor {
                calls: Vec::new(),
                outcomes,
            }
        }
    }

    impl EditorRunner for FakeEditor {
        fn edit(&mut self, label: &str, text: &str) -> EditorOutcome {
            self.calls.push((label.to_string(), text.to_string()));
            self.outcomes.remove(0)
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

    /// The type each stage of the NS-33/NS-34 tests opens with now: PO by its discussion (one
    /// line), the others by their own type.
    fn kind_of(stage: Stage) -> StageType {
        match stage {
            Stage::Po => StageType::PoDiscussion,
            Stage::TechLeader => StageType::TechLeader,
            Stage::Development => StageType::Development,
            Stage::Review => StageType::Review,
        }
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

        /// `g` and the check it asks for (`Action::CheckAgent`), carried out with `launcher`.
        fn g(&mut self, launcher: &mut FakeLauncher) {
            match self.ch('g') {
                Action::CheckAgent { project, ticket } => {
                    self.app.check_agent(&project, &ticket, launcher);
                }
                other => panic!("no CheckAgent: {other:?}"),
            }
        }

        /// The types of the menu and the selected one.
        fn menu(&self) -> (Vec<StageType>, usize) {
            match &self.app.dialog {
                Some(Dialog::ChooseType { types, index, .. }) => (types.clone(), *index),
                other => panic!("not in the menu: {other:?}"),
            }
        }

        /// The type, the values and the focus of the form.
        fn form(&self) -> (StageType, Vec<String>, usize) {
            match &self.app.dialog {
                Some(Dialog::StageForm {
                    kind,
                    values,
                    focus,
                    ..
                }) => (*kind, values.clone(), *focus),
                other => panic!("not in the form: {other:?}"),
            }
        }

        /// The scroll of the preview.
        fn scroll(&self) -> usize {
            match &self.app.dialog {
                Some(Dialog::ConfirmPrompt { scroll, .. }) => *scroll,
                other => panic!("not in the preview: {other:?}"),
            }
        }

        /// From the menu, ↓ until `kind` and Enter: its form.
        fn pick(&mut self, kind: StageType) {
            let (types, _) = self.menu();
            let at = types.iter().position(|t| *t == kind).expect("in the menu");
            for _ in 0..at {
                self.key(KeyCode::Down);
            }
            assert_eq!(self.key(KeyCode::Enter), Action::None);
            assert_eq!(self.form().0, kind);
        }

        /// Fills the empty required fields of the form (a line: `Resposta`; the editor:
        /// `Descrição longa`), going back to the first field, and Enter there: the action.
        fn fill_and_enter(&mut self) -> Action {
            let (kind, values, _) = self.form();
            for (i, field) in kind.fields().iter().enumerate() {
                while self.form().2 != i {
                    self.key(KeyCode::Tab);
                }
                if !values[i].is_empty() || field.rule == crate::stages::Rule::Optional {
                    continue;
                }
                match field.input {
                    Input::Line if field.rule == crate::stages::Rule::Plan => {}
                    Input::Line => self.type_text("Resposta"),
                    Input::Editor => {
                        assert_eq!(self.key(KeyCode::Enter), Action::EditDescription);
                        let mut editor =
                            FakeEditor::new(vec![EditorOutcome::Text("Descrição longa".into())]);
                        self.app.edit_description(&mut editor);
                    }
                }
            }
            while self.form().2 != 0 {
                self.key(KeyCode::Tab);
            }
            self.key(KeyCode::Enter)
        }

        /// The whole flow of the stage `downs` (in `Stage::ALL`, as in the NS-33): `g`, the
        /// check, the type of the stage (`kind_of`), the form, the render, `s` on the preview
        /// and the launch with `launcher`.
        fn open_agent(&mut self, downs: usize, launcher: &mut FakeLauncher) {
            self.open_kind(kind_of(Stage::ALL[downs]), launcher);
        }

        /// `open_agent` of a type.
        fn open_kind(&mut self, kind: StageType, launcher: &mut FakeLauncher) {
            self.g(launcher);
            self.pick(kind);
            assert_eq!(self.fill_and_enter(), Action::RenderPrompt);
            self.app.render_prompt(launcher);
            let Action::OpenAgent {
                project,
                ticket,
                kind,
                log,
                precheck,
            } = self.ch('s')
            else {
                panic!("no OpenAgent: {:?}", self.app.dialog);
            };
            self.app
                .open_agent(&project, &ticket, kind, &log, precheck, launcher);
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
    fn g_runs_the_check_before_the_menu() {
        let (mut f, p) = with_agents();
        let ticket = f.app.ticket_rows[0].clone();
        assert_eq!(
            f.ch('g'),
            Action::CheckAgent {
                project: p.clone(),
                ticket: ticket.clone()
            }
        );
        assert_eq!(f.app.dialog, None, "no menu before the check");
        let mut launcher = FakeLauncher::new(false);
        f.app.check_agent(&p, &ticket, &mut launcher);
        assert_eq!(
            launcher.checks,
            [(f.root.join("bin/nick-agent"), p.clone())]
        );
        assert_eq!(f.menu().1, 0);
        assert_eq!(f.agent_rows(), [], "nothing stored");
    }

    #[test]
    fn the_menu_follows_the_flags_of_the_check_and_the_arrows_move() {
        for (jira, sentry) in [(false, false), (true, false), (false, true), (true, true)] {
            let (mut f, _) = with_agents();
            let mut launcher = FakeLauncher::new(false);
            launcher.check = Ok(Precheck {
                jail: false,
                jira,
                sentry,
                warning: None,
            });
            f.g(&mut launcher);
            assert_eq!(f.menu().0, StageType::menu(jira, sentry), "{jira} {sentry}");
        }
        let (mut f, _) = with_agents();
        f.g(&mut FakeLauncher::new(false));
        for (code, expected) in [
            (KeyCode::Down, 1),
            (KeyCode::Char('j'), 2),
            (KeyCode::Down, 3),
            (KeyCode::Down, 4),
            (KeyCode::Down, 4),
            (KeyCode::Up, 3),
            (KeyCode::Char('k'), 2),
            (KeyCode::Up, 1),
            (KeyCode::Up, 0),
            (KeyCode::Up, 0),
        ] {
            f.key(code);
            assert_eq!(f.menu().1, expected, "{code:?}");
        }
    }

    #[test]
    fn esc_cancels_the_menu_and_q_does_not_quit_it() {
        let (mut f, _) = with_agents();
        f.g(&mut FakeLauncher::new(false));
        assert_eq!(f.ch('q'), Action::None);
        assert_eq!(f.menu().1, 0, "q stays in the menu");
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.app.screen, Screen::Tickets);
        f.g(&mut FakeLauncher::new(false));
        assert_eq!(f.ctrl_c(), Action::Quit, "Ctrl+C quits, as in every dialog");
        assert_eq!(f.agent_rows(), []);
    }

    #[test]
    fn the_id_comes_from_the_ticket_and_is_editable() {
        let (mut f, _) = with_agents();
        f.key(KeyCode::Down);
        f.g(&mut FakeLauncher::new(false));
        f.pick(StageType::Development);
        assert_eq!(
            f.form(),
            (
                StageType::Development,
                vec!["NS-2".into(), String::new()],
                0
            )
        );
        f.key(KeyCode::Backspace);
        f.ch('9');
        assert_eq!(f.form().1[0], "NS-9");
        // q is typed in the form: it does not quit.
        assert_eq!(f.ch('q'), Action::None);
        assert_eq!(f.form().1[0], "NS-9q");
    }

    #[test]
    fn tab_and_the_arrows_move_between_the_fields() {
        let (mut f, _) = with_agents();
        f.g(&mut FakeLauncher::new(false));
        f.pick(StageType::TechLeader);
        for (code, expected) in [
            (KeyCode::Tab, 1),
            (KeyCode::Down, 2),
            (KeyCode::Tab, 0),
            (KeyCode::BackTab, 2),
            (KeyCode::Up, 1),
        ] {
            f.key(code);
            assert_eq!(f.form().2, expected, "{code:?}");
        }
    }

    #[test]
    fn each_refusal_goes_to_the_bar_with_the_focus_on_its_field() {
        let (mut f, _) = with_agents();
        f.g(&mut FakeLauncher::new(false));
        f.pick(StageType::TechLeader);
        // The ID is filled; the title is not.
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(
            f.status(),
            Some("Esse campo é obrigatório: Título da tarefa.")
        );
        assert_eq!(f.form().2, 1);
        // Only spaces count as empty (the trim of read).
        f.type_text("   ");
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.form().2, 1);
        f.type_text("Título");
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(
            f.status(),
            Some("Esse campo é obrigatório: Descrição / contexto.")
        );
        assert_eq!(f.form().2, 2, "the focus goes to the editor field");
        // An invalid ID: the message of tickets.rs, and the focus on the ID.
        f.key(KeyCode::Tab);
        f.type_text("/x");
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.status(), Some(tickets::INVALID_ID_MESSAGE));
        assert_eq!(f.form().2, 0);
        // An empty ID is a required one.
        for _ in 0.."NS-1/x".len() {
            f.key(KeyCode::Backspace);
        }
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(
            f.status(),
            Some("Esse campo é obrigatório: ID do Jira (ex: SC-123).")
        );
        // Esc closes without writing anything.
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), None);
    }

    #[test]
    fn the_editor_field_takes_no_typing_and_enter_opens_the_editor() {
        let (mut f, _) = with_agents();
        f.g(&mut FakeLauncher::new(false));
        f.pick(StageType::Po);
        f.key(KeyCode::Tab);
        f.ch('x');
        assert_eq!(
            f.form().1,
            ["", ""],
            "typing on the editor field does nothing"
        );
        assert_eq!(f.key(KeyCode::Enter), Action::EditDescription);
        assert_eq!(f.form().2, 1, "the form stays for the editor");
    }

    #[test]
    fn a_text_of_the_editor_fills_the_field_and_moves_the_focus() {
        let (mut f, _) = with_agents();
        f.g(&mut FakeLauncher::new(false));
        f.pick(StageType::TechLeader);
        f.key(KeyCode::Tab);
        f.key(KeyCode::Tab);
        let mut editor = FakeEditor::new(vec![EditorOutcome::Text("  linha 1\nlinha 2".into())]);
        f.app.edit_description(&mut editor);
        assert_eq!(
            editor.calls,
            [("Descrição / contexto".to_string(), String::new())]
        );
        let (_, values, focus) = f.form();
        assert_eq!(
            values[2], "  linha 1\nlinha 2",
            "kept as the editor left it"
        );
        assert_eq!(focus, 0, "the next field (wrapping to the first)");
        assert_eq!(f.status(), None);
    }

    #[test]
    fn an_empty_editor_leaves_the_field_empty_with_the_message_of_the_script() {
        for outcome in [EditorOutcome::Empty, EditorOutcome::Text(String::new())] {
            let (mut f, _) = with_agents();
            f.g(&mut FakeLauncher::new(false));
            f.pick(StageType::Po);
            f.key(KeyCode::Tab);
            let mut editor = FakeEditor::new(vec![outcome.clone()]);
            f.app.edit_description(&mut editor);
            assert_eq!(f.form().1[1], "");
            assert_eq!(f.form().2, 1, "the focus stays: Enter opens it again");
            assert_eq!(f.status(), Some("Esse campo é obrigatório, tente de novo."));
        }
    }

    #[test]
    fn reopening_the_editor_offers_the_current_text_and_an_empty_one_clears_it() {
        let (mut f, _) = with_agents();
        f.g(&mut FakeLauncher::new(false));
        f.pick(StageType::Po);
        f.key(KeyCode::Tab);
        let mut editor = FakeEditor::new(vec![
            EditorOutcome::Text("primeira".into()),
            EditorOutcome::Text("segunda".into()),
            EditorOutcome::Empty,
        ]);
        f.app.edit_description(&mut editor);
        f.key(KeyCode::Tab);
        f.app.edit_description(&mut editor);
        f.key(KeyCode::Tab);
        f.app.edit_description(&mut editor);
        assert_eq!(
            editor.calls,
            [
                ("Descrição do pedido".to_string(), String::new()),
                ("Descrição do pedido".to_string(), "primeira".to_string()),
                ("Descrição do pedido".to_string(), "segunda".to_string())
            ]
        );
        assert_eq!(f.form().1[1], "");
    }

    #[test]
    fn an_editor_that_does_not_open_says_why_and_the_form_stays() {
        let (mut f, _) = with_agents();
        f.g(&mut FakeLauncher::new(false));
        f.pick(StageType::Po);
        f.key(KeyCode::Tab);
        let mut editor = FakeEditor::new(vec![EditorOutcome::Failed {
            editor: "vim".into(),
            error: "comando não encontrado".into(),
        }]);
        f.app.edit_description(&mut editor);
        assert_eq!(
            f.status(),
            Some("Não consegui abrir o editor vim: comando não encontrado.")
        );
        assert_eq!(
            f.form(),
            (StageType::Po, vec![String::new(), String::new()], 1)
        );
    }

    #[test]
    fn the_render_gets_the_answers_of_each_type() {
        let flags = Precheck {
            jail: false,
            jira: true,
            sentry: true,
            warning: None,
        };
        for kind in StageType::ALL {
            let (mut f, p) = with_agents();
            let mut launcher = FakeLauncher::new(false);
            launcher.check = Ok(flags.clone());
            f.g(&mut launcher);
            f.pick(kind);
            assert_eq!(f.fill_and_enter(), Action::RenderPrompt, "{kind:?}");
            f.app.render_prompt(&mut launcher);
            let expected: Vec<(&str, String)> = kind
                .fields()
                .iter()
                .filter_map(|field| match (field.rule, field.input) {
                    (crate::stages::Rule::Optional | crate::stages::Rule::Plan, _) => None,
                    (crate::stages::Rule::JiraId, _) => Some((field.flag, "NS-1".to_string())),
                    (_, Input::Editor) => Some((field.flag, "Descrição longa".to_string())),
                    (_, Input::Line) => Some((field.flag, "Resposta".to_string())),
                })
                .collect();
            assert_eq!(
                launcher.renders,
                [(
                    f.root.join("bin/nick-agent"),
                    p.clone(),
                    RenderRequest {
                        kind,
                        answers: expected
                    }
                )],
                "{kind:?}"
            );
            assert_eq!(f.scroll(), 0, "{kind:?}: the preview opens at the top");
        }
    }

    #[test]
    fn the_answers_are_trimmed_and_the_optional_ones_go_when_filled() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(false);
        f.g(&mut launcher);
        f.pick(StageType::Review);
        f.key(KeyCode::Tab);
        f.type_text("  outro.md\t");
        f.key(KeyCode::Tab);
        assert_eq!(f.key(KeyCode::Enter), Action::RenderPrompt);
        f.app.render_prompt(&mut launcher);
        assert_eq!(
            launcher.renders[0].2.answers,
            [
                ("--jira-id", "NS-1".to_string()),
                ("--plan-path", "outro.md".to_string())
            ]
        );
    }

    #[test]
    fn a_failing_render_shows_the_alert_and_opens_nothing() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(false);
        launcher.render = Err("Erro: o tipo 'po_jira' não está disponível neste projeto.".into());
        f.g(&mut launcher);
        f.pick(StageType::PoDiscussion);
        assert_eq!(f.fill_and_enter(), Action::RenderPrompt);
        f.app.render_prompt(&mut launcher);
        assert_eq!(
            f.alert(),
            "Nada foi aberto.\n\nErro: o tipo 'po_jira' não está disponível neste projeto."
        );
        assert_eq!(launcher.spawns, []);
        assert_eq!(f.agent_rows(), []);
    }

    #[test]
    fn s_on_the_preview_launches_with_the_type_and_the_log_without_a_second_check() {
        let (mut f, p) = with_agents();
        let mut launcher = FakeLauncher::new(false);
        f.g(&mut launcher);
        f.pick(StageType::Development);
        assert_eq!(f.fill_and_enter(), Action::RenderPrompt);
        f.app.render_prompt(&mut launcher);
        let action = f.mods(KeyCode::Char('S'), KeyModifiers::SHIFT);
        let Action::OpenAgent {
            project,
            ticket,
            kind,
            log,
            precheck,
        } = action.clone()
        else {
            panic!("{action:?}");
        };
        assert_eq!(
            (project.clone(), kind, log.clone()),
            (p, StageType::Development, PathBuf::from(LOG))
        );
        f.app
            .open_agent(&project, &ticket, kind, &log, precheck, &mut launcher);
        assert_eq!(launcher.checks.len(), 1, "no second check");
        assert_eq!(
            launcher.kinds,
            [(StageType::Development, PathBuf::from(LOG))]
        );
        assert_eq!(f.agent_rows()[0].1, "development");
        assert_eq!(f.agent_rows()[0].2, "proj · Dev · NS-1");
    }

    #[test]
    fn the_name_keeps_the_id_of_the_ticket_when_the_id_typed_is_another() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(false);
        f.g(&mut launcher);
        f.pick(StageType::Review);
        f.key(KeyCode::Backspace);
        f.ch('7');
        assert_eq!(f.key(KeyCode::Enter), Action::RenderPrompt);
        f.app.render_prompt(&mut launcher);
        assert_eq!(
            launcher.renders[0].2.answers[0],
            ("--jira-id", "NS-7".to_string())
        );
        let Action::OpenAgent {
            project,
            ticket,
            kind,
            log,
            precheck,
        } = f.ch('s')
        else {
            panic!("no OpenAgent");
        };
        f.app
            .open_agent(&project, &ticket, kind, &log, precheck, &mut launcher);
        assert_eq!(f.agent_rows()[0].2, "proj · Review · NS-1");
    }

    #[test]
    fn n_and_esc_cancel_the_preview_with_the_log_and_launch_nothing() {
        for code in [KeyCode::Char('n'), KeyCode::Char('N'), KeyCode::Esc] {
            let (mut f, _) = with_agents();
            let mut launcher = FakeLauncher::new(false);
            f.g(&mut launcher);
            f.pick(StageType::PoDiscussion);
            f.fill_and_enter();
            f.app.render_prompt(&mut launcher);
            // Other keys are ignored: the preview stays.
            assert_eq!(f.ch('x'), Action::None);
            assert_eq!(f.ch('q'), Action::None);
            assert_eq!(f.scroll(), 0);
            assert_eq!(f.key(code), Action::None, "{code:?}");
            assert_eq!(f.app.dialog, None);
            assert_eq!(
                f.status(),
                Some(format!("Cancelado. O prompt gerado continua salvo em: {LOG}").as_str())
            );
            assert_eq!(launcher.spawns, []);
            assert_eq!(f.agent_rows(), []);
        }
    }

    #[test]
    fn the_scroll_of_the_preview_stays_inside_the_prompt() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(false);
        launcher.render = Ok(rendered(100));
        f.g(&mut launcher);
        f.pick(StageType::PoDiscussion);
        f.fill_and_enter();
        f.app.render_prompt(&mut launcher);
        // The window the loop computes (ui::prompt_window) before each key.
        let (rows, max) = (10, 90);
        f.app.set_prompt_window(rows, max);
        assert_eq!(
            f.app
                .prompt_preview()
                .map(|(_, r)| r.prompt.lines().count()),
            Some(100)
        );
        assert_eq!(f.key(KeyCode::Up), Action::None);
        assert_eq!(f.scroll(), 0, "not above the top");
        f.key(KeyCode::Down);
        f.ch('j');
        assert_eq!(f.scroll(), 2);
        f.key(KeyCode::PageDown);
        assert_eq!(f.scroll(), 2 + rows);
        f.key(KeyCode::End);
        assert_eq!(f.scroll(), max);
        f.key(KeyCode::Down);
        f.key(KeyCode::PageDown);
        assert_eq!(f.scroll(), max, "not below the end");
        f.key(KeyCode::PageUp);
        assert_eq!(f.scroll(), max - rows);
        f.key(KeyCode::Home);
        assert_eq!(f.scroll(), 0);
        // A prompt shorter than the window does not scroll.
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(false);
        f.g(&mut launcher);
        f.pick(StageType::PoDiscussion);
        f.fill_and_enter();
        f.app.render_prompt(&mut launcher);
        f.key(KeyCode::End);
        f.key(KeyCode::Down);
        assert_eq!(f.scroll(), 0);
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
        // The agent is shown with the focus on its terminal, and the list stays on the ticket.
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.app.screen, Screen::Tickets);
        assert_eq!(f.app.dialog, None);
        let agent = f.app.shown_agent().expect("the running agent");
        assert_eq!(agent.name, name);
        assert!(!agent.jail);
        assert_eq!(
            launcher.ids,
            [agent.id],
            "the spawn carries the id of the row"
        );
        let lock = agents::lock_path(&f.agents_dir(), &session_id);
        let other = fs::File::open(&lock).unwrap();
        assert!(other.try_lock().is_err(), "the lock is held while it runs");
    }

    #[test]
    fn a_failing_check_shows_the_alert_and_opens_nothing() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(true);
        launcher.check = Err("Erro: a jail (ai-jail) não funciona nesta máquina.\nDetalhe".into());
        f.g(&mut launcher);
        assert_eq!(
            f.alert(),
            "Nada foi aberto.\n\nErro: a jail (ai-jail) não funciona nesta máquina.\nDetalhe"
        );
        assert_eq!(launcher.checks.len(), 1);
        assert_eq!(launcher.renders, []);
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
        f.g(&mut launcher);
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
        assert!(f.app.agents.is_empty());
        assert_eq!(f.app.shown, None);
        assert_eq!(f.app.focus, Focus::List);
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
        assert_eq!(f.app.focus, Focus::Terminal);
        (f, launcher)
    }

    /// With the focus on the terminal, every key but the reserved shortcut goes to the program
    /// (the shortcut: `the_shortcut_toggles_the_focus_and_never_reaches_the_program`).
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
            f.mods(KeyCode::Char(']'), KeyModifiers::CONTROL),
            send(b"\x1d")
        );
        assert_eq!(f.key(KeyCode::Up), send(b"\x1b[A"));
        f.app.agent_term.modes.application_cursor = true;
        assert_eq!(f.key(KeyCode::Up), send(b"\x1bOA"));
        assert_eq!(f.app.focus, Focus::Terminal);
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
            f.app.agent_ended(launcher.ids[0], code);
            assert_eq!(f.app.screen, Screen::Tickets);
            assert_eq!(f.app.ticket_rows[f.app.ticket_index].key, "NS-2");
            assert_eq!(f.status(), Some(message));
            assert!(f.app.agents.is_empty());
            assert_eq!(f.app.focus, Focus::List);
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
            jira: false,
            sentry: false,
            warning: Some("Aviso: falta chave.\n  X=\"\"".into()),
        });
        f.open_agent(0, &mut launcher);
        assert_eq!(f.app.focus, Focus::Terminal);
        f.app.agent_ended(launcher.ids[0], 0);
        assert_eq!(
            f.status(),
            Some(
                "O agente proj · PO · NS-1 terminou. Aviso da verificação: Aviso: falta chave. X=\"\""
            )
        );
    }

    // ---- several agents, the focus and the shortcut (NS-34) ----

    impl Fixture {
        /// The reserved shortcut as crossterm reports Ctrl+\ in raw mode (Ctrl+4).
        fn shortcut(&mut self) -> Action {
            self.mods(KeyCode::Char('4'), KeyModifiers::CONTROL)
        }

        fn kind(&mut self, code: KeyCode, modifiers: KeyModifiers, kind: KeyEventKind) -> Action {
            let mut key = KeyEvent::new(code, modifiers);
            key.kind = kind;
            self.app.handle_key(key)
        }

        fn names_running(&self) -> Vec<String> {
            self.app.agents.iter().map(|a| a.name.clone()).collect()
        }

        fn locked(&self, session_id: &str) -> bool {
            let path = agents::lock_path(&self.agents_dir(), session_id);
            fs::File::open(path).is_ok_and(|file| file.try_lock().is_err())
        }
    }

    /// Two agents of different tickets and projects: `proj · PO · NS-1`, then (Esc back to the
    /// projects, a new project `beta` with the ticket B-1) `beta · Dev · B-1`, shown.
    fn two_running() -> (Fixture, FakeLauncher) {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(true);
        f.open_agent(0, &mut launcher);
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::List);
        f.key(KeyCode::Esc);
        assert_eq!(f.app.screen, Screen::Projects);
        let beta = f.dir("beta", true);
        f.add(beta.to_str().unwrap());
        f.key(KeyCode::Enter);
        assert_eq!(f.app.screen, Screen::Tickets);
        f.new_ticket("Beta", "B-1");
        launcher.check = Ok(Precheck {
            jail: false,
            jira: false,
            sentry: false,
            warning: None,
        });
        f.open_agent(2, &mut launcher);
        assert_eq!(f.names_running(), ["proj · PO · NS-1", "beta · Dev · B-1"]);
        (f, launcher)
    }

    #[test]
    fn two_agents_of_different_projects_run_at_once_and_the_last_is_shown() {
        let (f, launcher) = two_running();
        assert_eq!(f.app.agents.len(), 2);
        assert_eq!(f.app.shown, Some(launcher.ids[1]));
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(
            f.app.screen,
            Screen::Tickets,
            "the list stays on the tickets"
        );
        assert_eq!(f.app.ticket_rows[f.app.ticket_index].key, "B-1");
        assert_eq!(launcher.spawns[0].1, f.root.join("proj"));
        assert_eq!(launcher.spawns[1].1, f.root.join("beta"));
        let rows = f.agent_rows();
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|r| r.3 == "active"));
        assert!(
            f.locked(&rows[0].0) && f.locked(&rows[1].0),
            "two locks held"
        );
        assert!(f.app.agents[0].jail && !f.app.agents[1].jail);
        assert_eq!(f.app.agents[0].project, "proj");
        assert_eq!(f.app.agents[1].project, "beta");
    }

    #[test]
    fn the_shortcut_toggles_the_focus_and_never_reaches_the_program() {
        let (mut f, _) = running();
        // Ctrl+4 (raw mode) and Ctrl+\ (kitty) are the same key.
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(
            f.mods(KeyCode::Char('\\'), KeyModifiers::CONTROL),
            Action::None
        );
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.app.agents.len(), 1, "the agent goes on");
        // On the terminal, Ctrl+C, Esc, q and Ctrl+] are the program's.
        let send = |b: &[u8]| Action::Send(b.to_vec());
        assert_eq!(f.ctrl_c(), send(b"\x03"));
        assert_eq!(f.key(KeyCode::Esc), send(b"\x1b"));
        assert_eq!(f.ch('q'), send(b"q"));
        assert_eq!(
            f.mods(KeyCode::Char(']'), KeyModifiers::CONTROL),
            send(b"\x1d")
        );
    }

    #[test]
    fn another_shortcut_takes_the_place_of_ctrl_backslash() {
        let (mut f, _) = running();
        f.app.shortcut = Shortcut::from_db("ctrl+]").unwrap();
        assert_eq!(
            f.shortcut(),
            Action::Send(b"\x1c".to_vec()),
            "Ctrl+\\ is the program's"
        );
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(
            f.mods(KeyCode::Char('5'), KeyModifiers::CONTROL),
            Action::None
        );
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(
            f.mods(KeyCode::Char(']'), KeyModifiers::CONTROL),
            Action::None
        );
        assert_eq!(f.app.focus, Focus::Terminal);
    }

    #[test]
    fn with_the_focus_on_the_list_the_keys_are_the_list_s() {
        let (mut f, _) = running();
        f.shortcut();
        assert_eq!(f.app.ticket_index, 1);
        assert_eq!(f.key(KeyCode::Up), Action::None);
        assert_eq!(f.app.ticket_index, 0, "the arrows move the tickets");
        assert_eq!(f.ch('a'), Action::None);
        assert!(matches!(f.app.dialog, Some(Dialog::NewTicket { .. })));
        f.key(KeyCode::Esc);
        // Nothing goes to the program: a paste and the mouse neither.
        assert_eq!(f.app.handle_paste("x"), Action::None);
        let wheel = MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column: 50,
            row: 5,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(
            f.app.handle_mouse(&wheel, Rect::new(40, 1, 60, 20)),
            Action::None
        );
        // The shortcut takes the focus back to the terminal of the shown agent.
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.app.handle_paste("x"), Action::Send(b"x".to_vec()));
    }

    #[test]
    fn without_an_agent_the_shortcut_does_nothing() {
        let (mut f, _) = with_agents();
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(f.app.screen, Screen::Tickets);
        assert_eq!(f.ch('q'), Action::Quit, "q still quits the list");
    }

    #[test]
    fn with_a_dialog_open_the_shortcut_is_ignored() {
        let (mut f, _) = running();
        f.shortcut();
        f.g(&mut FakeLauncher::new(false));
        assert!(matches!(f.app.dialog, Some(Dialog::ChooseType { .. })));
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(
            f.app.focus,
            Focus::List,
            "a pending question stays in front"
        );
        assert!(matches!(f.app.dialog, Some(Dialog::ChooseType { .. })));
    }

    #[test]
    fn a_repeat_of_the_shortcut_does_not_toggle_and_a_release_is_ignored() {
        let (mut f, _) = running();
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(
            f.kind(KeyCode::Char('4'), ctrl, KeyEventKind::Repeat),
            Action::None
        );
        assert_eq!(
            f.app.focus,
            Focus::Terminal,
            "holding it does not blink the focus"
        );
        assert_eq!(
            f.kind(KeyCode::Char('4'), ctrl, KeyEventKind::Release),
            Action::None
        );
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(
            f.kind(KeyCode::Char('4'), ctrl, KeyEventKind::Press),
            Action::None
        );
        assert_eq!(f.app.focus, Focus::List);
        // Another key held on the terminal is still the program's.
        f.shortcut();
        assert_eq!(
            f.kind(KeyCode::Char('a'), KeyModifiers::NONE, KeyEventKind::Repeat),
            Action::Send(b"a".to_vec())
        );
    }

    #[test]
    fn the_end_of_the_shown_agent_shows_the_most_recent_one_on_the_list() {
        let (mut f, launcher) = two_running();
        let (first, second) = (launcher.ids[0], launcher.ids[1]);
        f.app.agent_ended(second, 0);
        assert_eq!(f.status(), Some("O agente beta · Dev · B-1 terminou."));
        assert_eq!(f.app.shown, Some(first));
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(f.app.screen, Screen::Tickets);
        assert_eq!(f.agent_rows()[1].3, "ended");
        assert!(!agents::lock_path(&f.agents_dir(), &launcher.spawns[1].2).exists());
        assert_eq!(f.agent_rows()[0].3, "active");
        // The last one: nothing shown.
        f.app.agent_ended(first, 0);
        assert_eq!(f.app.shown, None);
        assert_eq!(f.app.focus, Focus::List);
        assert!(f.app.agents.is_empty());
        // An end that is not of a running agent changes nothing.
        f.app.status = None;
        f.app.agent_ended(first, 0);
        assert_eq!(f.status(), None);
    }

    #[test]
    fn the_end_of_a_hidden_agent_keeps_the_focus_and_the_shown_one() {
        let (mut f, launcher) = two_running();
        let (first, second) = (launcher.ids[0], launcher.ids[1]);
        f.app.agent_ended(first, 3);
        assert_eq!(
            f.status(),
            Some("O agente proj · PO · NS-1 terminou com o código 3.")
        );
        assert_eq!(f.app.shown, Some(second));
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.names_running(), ["beta · Dev · B-1"]);
    }

    #[test]
    fn the_mark_of_new_output_compares_the_visible_text() {
        let (mut f, launcher) = two_running();
        let (first, second) = (launcher.ids[0], launcher.ids[1]);
        // The loop kept the text of the first when it stopped being shown.
        f.app.left_shown(first, "tela antiga".into());
        assert!(f.app.wants_hidden_text(first));
        assert!(
            !f.app.hidden_output(first, "tela antiga"),
            "the same text: no mark"
        );
        assert!(!f.app.agents[0].unseen);
        assert!(f.app.hidden_output(first, "tela nova"), "the mark changed");
        assert!(f.app.agents[0].unseen);
        assert!(
            !f.app.wants_hidden_text(first),
            "marked: nothing to compare"
        );
        assert!(!f.app.hidden_output(first, "outra"), "true once only");
        // The shown one is never marked.
        assert!(!f.app.wants_hidden_text(second));
        assert!(!f.app.hidden_output(second, "qualquer coisa"));
        assert!(!f.app.agents[1].unseen);
        // Being shown clears it.
        f.shortcut();
        f.ch('v');
        f.key(KeyCode::Up);
        assert_eq!(f.app.shown, Some(first));
        assert!(!f.app.agents[0].unseen);
    }

    // ---- "Agentes ativos", ending and quitting (NS-34) ----

    #[test]
    fn v_opens_the_active_agents_from_every_list_and_esc_goes_back() {
        let (mut f, _) = two_running();
        f.shortcut();
        for back in [KeyCode::Esc, KeyCode::Left] {
            // From the tickets, with the same ticket selected.
            assert_eq!(f.app.screen, Screen::Tickets);
            let ticket = f.app.ticket_index;
            assert_eq!(f.ch('v'), Action::None);
            assert_eq!(f.app.screen, Screen::ActiveAgents);
            assert_eq!(f.key(back), Action::None);
            assert_eq!(f.app.screen, Screen::Tickets);
            assert_eq!(f.app.ticket_index, ticket);
        }
        f.key(KeyCode::Esc);
        assert_eq!(f.app.screen, Screen::Projects);
        let project = f.app.project_index;
        f.ch('v');
        assert_eq!(f.app.screen, Screen::ActiveAgents);
        f.key(KeyCode::Esc);
        assert_eq!(
            (f.app.screen, f.app.project_index),
            (Screen::Projects, project)
        );
        f.key(KeyCode::Esc);
        f.ch('v');
        assert_eq!(f.app.screen, Screen::ActiveAgents);
        f.key(KeyCode::Left);
        assert_eq!(f.app.screen, Screen::Modules);
    }

    #[test]
    fn v_works_without_agents_and_is_typed_in_a_text_field() {
        let mut f = fixture();
        assert_eq!(f.ch('v'), Action::None);
        assert_eq!(f.app.screen, Screen::ActiveAgents);
        assert!(f.app.agents.is_empty());
        f.key(KeyCode::Esc);
        f.key(KeyCode::Enter);
        f.ch('a');
        f.ch('v');
        assert_eq!(f.app.dialog, Some(Dialog::AddPath { input: "v".into() }));
        assert_eq!(f.app.screen, Screen::Projects);
    }

    #[test]
    fn moving_on_the_view_shows_the_selected_agent_and_enter_focuses_it() {
        let (mut f, launcher) = two_running();
        let (first, second) = (launcher.ids[0], launcher.ids[1]);
        f.shortcut();
        f.ch('v');
        assert_eq!(f.app.agent_index, 1, "it starts on the shown one");
        f.key(KeyCode::Up);
        assert_eq!(f.app.shown, Some(first));
        assert_eq!(f.app.focus, Focus::List);
        f.ch('k');
        assert_eq!(f.app.shown, Some(first), "stops at the first");
        f.ch('j');
        assert_eq!(f.app.shown, Some(second));
        f.key(KeyCode::Down);
        assert_eq!(f.app.shown, Some(second), "stops at the last");
        f.key(KeyCode::Up);
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!((f.app.shown, f.app.focus), (Some(first), Focus::Terminal));
        f.shortcut();
        f.key(KeyCode::Esc);
        f.ch('v');
        assert_eq!(f.app.agent_index, 0, "the shown one again");
        f.key(KeyCode::Down);
        assert_eq!(f.key(KeyCode::Right), Action::None);
        assert_eq!((f.app.shown, f.app.focus), (Some(second), Focus::Terminal));
    }

    #[test]
    fn x_asks_and_only_s_ends_the_agent() {
        for (key, ends) in [
            (KeyCode::Char('s'), true),
            (KeyCode::Char('S'), true),
            (KeyCode::Char('n'), false),
            (KeyCode::Enter, false),
            (KeyCode::Esc, false),
            (KeyCode::Char('q'), false),
        ] {
            let (mut f, launcher) = two_running();
            let first = launcher.ids[0];
            f.shortcut();
            f.ch('v');
            f.key(KeyCode::Up);
            assert_eq!(f.ch('x'), Action::None);
            assert_eq!(
                f.app.dialog,
                Some(Dialog::ConfirmEndAgent {
                    id: first,
                    name: "proj · PO · NS-1".into()
                })
            );
            let expected = if ends {
                Action::EndAgent(first)
            } else {
                Action::None
            };
            assert_eq!(f.key(key), expected, "{key:?}");
            assert_eq!(f.app.dialog, None);
            assert_eq!(f.app.agents.len(), 2, "the loop ends it: {key:?}");
        }
    }

    #[test]
    fn an_agent_ended_by_the_operator_leaves_with_its_record() {
        let (mut f, launcher) = two_running();
        let (first, second) = (launcher.ids[0], launcher.ids[1]);
        f.shortcut();
        f.ch('v');
        f.app.agent_ended_by_operator(second);
        assert_eq!(f.status(), Some("O agente beta · Dev · B-1 foi encerrado."));
        assert_eq!(f.names_running(), ["proj · PO · NS-1"]);
        assert_eq!(f.app.shown, Some(first));
        assert_eq!(f.app.agent_index, 0);
        assert_eq!(f.app.focus, Focus::List);
        let rows = f.agent_rows();
        assert_eq!(
            (rows[0].3.as_str(), rows[1].3.as_str()),
            ("active", "ended")
        );
        assert!(!agents::lock_path(&f.agents_dir(), &rows[1].0).exists());
        assert!(f.locked(&rows[0].0), "the other one goes on");
    }

    #[test]
    fn quitting_with_agents_asks_first_and_without_them_does_not() {
        let mut f = fixture();
        assert_eq!(f.ch('q'), Action::Quit);
        assert_eq!(f.ctrl_c(), Action::Quit);
        for quit in ['q', 'Q', 'c'] {
            let (mut f, _) = two_running();
            f.shortcut();
            let action = if quit == 'c' { f.ctrl_c() } else { f.ch(quit) };
            assert_eq!(action, Action::None, "{quit}");
            assert_eq!(f.app.dialog, Some(Dialog::ConfirmQuit));
            assert_eq!(f.ch('s'), Action::QuitEndingAgents);
        }
        for key in [
            KeyCode::Char('n'),
            KeyCode::Enter,
            KeyCode::Char('x'),
            KeyCode::Esc,
        ] {
            let (mut f, _) = two_running();
            f.shortcut();
            f.ch('q');
            assert_eq!(f.key(key), Action::None, "{key:?}");
            assert_eq!(f.app.dialog, None);
            assert_eq!(f.app.agents.len(), 2);
            assert!(f.agent_rows().iter().all(|r| r.3 == "active"));
        }
        // From the view too.
        let (mut f, _) = two_running();
        f.shortcut();
        f.ch('v');
        f.ch('q');
        assert_eq!(f.app.dialog, Some(Dialog::ConfirmQuit));
    }

    #[test]
    fn end_all_records_goes_on_after_an_error() {
        let (mut f, launcher) = two_running();
        let first = launcher.ids[0];
        f.app
            .db
            .connection()
            .execute_batch(&format!(
                "CREATE TEMP TRIGGER fail BEFORE UPDATE ON agents WHEN old.id = {first} \
                 BEGIN SELECT RAISE(ABORT, 'falha de teste'); END;"
            ))
            .unwrap();
        let err = f.app.end_all_records().unwrap_err();
        assert!(err.contains("falha de teste"), "{err}");
        let rows = f.agent_rows();
        assert_eq!(
            (rows[0].3.as_str(), rows[1].3.as_str()),
            ("active", "ended")
        );
        for row in &rows {
            assert!(
                !agents::lock_path(&f.agents_dir(), &row.0).exists(),
                "{row:?}"
            );
        }
        assert!(f.app.agents.is_empty());
        assert_eq!((f.app.shown, f.app.focus), (None, Focus::List));
    }

    // ---- the settings and the capture of the shortcut (NS-34) ----

    impl Fixture {
        fn stored_shortcut(&self) -> Option<String> {
            self.app.db.get_setting(SETTING_RESERVED_SHORTCUT).unwrap()
        }

        fn first(&self) -> Option<Shortcut> {
            match &self.app.dialog {
                Some(Dialog::CaptureShortcut { first }) => *first,
                other => panic!("not in the capture: {other:?}"),
            }
        }
    }

    #[test]
    fn c_opens_the_settings_only_on_the_modules() {
        let mut f = fixture();
        assert_eq!(f.ch('c'), Action::None);
        assert_eq!(f.app.screen, Screen::Settings);
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.screen, Screen::Modules);
        f.ch('c');
        f.key(KeyCode::Left);
        assert_eq!(f.app.screen, Screen::Modules);
        f.key(KeyCode::Enter);
        f.ch('c');
        assert_eq!(f.app.screen, Screen::Projects, "not on the projects");
        let mut f = fixture();
        f.ch('c');
        assert_eq!(f.ch('q'), Action::Quit, "q quits the settings");
    }

    #[test]
    fn the_new_shortcut_is_saved_only_when_pressed_twice() {
        let mut f = fixture();
        f.ch('c');
        assert!(!f.app.shortcut_stored);
        assert_eq!(f.ch('t'), Action::None);
        assert_eq!(f.first(), None);
        let ctrl_q = |f: &mut Fixture| f.mods(KeyCode::Char('q'), KeyModifiers::CONTROL);
        let ctrl_bracket = |f: &mut Fixture| f.mods(KeyCode::Char('5'), KeyModifiers::CONTROL);
        ctrl_q(&mut f);
        assert_eq!(f.first(), Shortcut::from_db("ctrl+q"));
        assert_eq!(f.stored_shortcut(), None, "once is not saved");
        // Another valid key becomes the new first.
        ctrl_bracket(&mut f);
        assert_eq!(f.first(), Shortcut::from_db("ctrl+]"));
        assert_eq!(f.stored_shortcut(), None);
        // The same key again (Ctrl+5 and Ctrl+] are one), saved.
        assert_eq!(
            f.mods(KeyCode::Char(']'), KeyModifiers::CONTROL),
            Action::None
        );
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.stored_shortcut().as_deref(), Some("ctrl+]"));
        assert_eq!(f.status(), Some("Atalho salvo: Ctrl+]."));
        assert_eq!(f.app.shortcut, Shortcut::from_db("ctrl+]").unwrap());
        assert!(f.app.shortcut_stored);
    }

    #[test]
    fn a_refused_key_says_why_and_starts_the_capture_over() {
        let mut f = fixture();
        f.ch('c');
        f.ch('t');
        f.mods(KeyCode::Char('q'), KeyModifiers::CONTROL);
        assert!(f.first().is_some());
        for (code, modifiers, message) in [
            (
                KeyCode::Enter,
                KeyModifiers::NONE,
                "O claude precisa dessa tecla (Enter). Escolha outra.",
            ),
            (
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
                "O claude precisa dessa tecla (Ctrl+C). Escolha outra.",
            ),
            (
                KeyCode::Char('a'),
                KeyModifiers::NONE,
                "Use uma tecla com Ctrl ou Alt (ou uma tecla de função, F1–F12).",
            ),
            (
                KeyCode::PageUp,
                KeyModifiers::SHIFT,
                "O nick usa essa tecla para rolar o histórico. Escolha outra.",
            ),
        ] {
            assert_eq!(
                f.mods(code, modifiers),
                Action::None,
                "Ctrl+C does not quit"
            );
            assert_eq!(f.status(), Some(message));
            assert_eq!(f.first(), None, "the capture stays open, from the start");
        }
        assert_eq!(f.stored_shortcut(), None);
        // q is refused too, and does not quit.
        assert_eq!(f.ch('q'), Action::None);
        assert_eq!(f.first(), None);
    }

    #[test]
    fn esc_cancels_the_capture_without_saving() {
        let mut f = fixture();
        f.ch('c');
        f.ch('t');
        f.mods(KeyCode::Char('q'), KeyModifiers::CONTROL);
        f.app.status = Some("x".into());
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.app.screen, Screen::Settings);
        assert_eq!(f.status(), None);
        assert_eq!(f.stored_shortcut(), None);
        assert_eq!(f.app.shortcut, Shortcut::DEFAULT);
    }

    #[test]
    fn the_current_shortcut_can_be_chosen_again_in_the_capture() {
        let (mut f, _) = running();
        f.shortcut();
        f.key(KeyCode::Esc);
        f.key(KeyCode::Esc);
        assert_eq!(f.app.screen, Screen::Modules);
        f.ch('c');
        f.ch('t');
        // The shortcut is captured, not taken as the toggle.
        f.shortcut();
        assert_eq!(f.first(), Some(Shortcut::DEFAULT));
        assert_eq!(f.app.focus, Focus::List);
        f.shortcut();
        assert_eq!(f.stored_shortcut().as_deref(), Some("ctrl+\\"));
    }

    #[test]
    fn a_saved_shortcut_works_right_away_and_p_goes_back_to_the_default() {
        let (mut f, _) = running();
        f.shortcut();
        f.key(KeyCode::Esc);
        f.key(KeyCode::Esc);
        f.ch('c');
        f.ch('t');
        f.mods(KeyCode::F(7), KeyModifiers::NONE);
        f.mods(KeyCode::F(7), KeyModifiers::NONE);
        assert_eq!(f.stored_shortcut().as_deref(), Some("f7"));
        // The new one toggles, the old one is the program's.
        assert_eq!(f.mods(KeyCode::F(7), KeyModifiers::NONE), Action::None);
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.shortcut(), Action::Send(b"\x1c".to_vec()));
        assert_eq!(f.mods(KeyCode::F(7), KeyModifiers::NONE), Action::None);
        assert_eq!(f.app.focus, Focus::List);
        // p: the row is deleted, and Ctrl+\ is the shortcut again.
        assert_eq!(f.ch('p'), Action::None);
        assert_eq!(f.stored_shortcut(), None);
        assert_eq!(f.status(), Some("Atalho de volta ao padrão: Ctrl+\\."));
        assert!(!f.app.shortcut_stored);
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::Terminal);
        // p on the default says the same.
        f.shortcut();
        f.app.status = None;
        f.ch('p');
        assert_eq!(f.status(), Some("Atalho de volta ao padrão: Ctrl+\\."));
    }

    #[test]
    fn the_shortcut_of_the_database_is_used_when_the_nick_opens() {
        let mut f = fixture();
        assert_eq!(f.app.load_shortcut(), None, "no row: the default");
        assert_eq!(f.app.shortcut, Shortcut::DEFAULT);
        assert!(!f.app.shortcut_stored);
        f.app
            .db
            .set_setting(SETTING_RESERVED_SHORTCUT, "ctrl+]")
            .unwrap();
        assert_eq!(f.app.load_shortcut(), None);
        assert_eq!(f.app.shortcut, Shortcut::from_db("ctrl+]").unwrap());
        assert!(f.app.shortcut_stored);
    }

    #[test]
    fn an_invalid_shortcut_of_the_database_falls_back_to_the_default() {
        // Not a shortcut at all, and one that would be refused today.
        for value in ["banana\x1b[2J", "ctrl+c"] {
            let mut f = fixture();
            f.app
                .db
                .set_setting(SETTING_RESERVED_SHORTCUT, value)
                .unwrap();
            assert_eq!(
                f.app.load_shortcut(),
                Some(format!(
                    "O atalho gravado no banco ({}) não é válido. Usando o padrão Ctrl+\\.",
                    printable(value)
                ))
            );
            assert_eq!(f.app.shortcut, Shortcut::DEFAULT);
        }
    }

    /// Review NS-34, N2: the wheel scrolls the history only over the terminal, not over the
    /// list beside it.
    #[test]
    fn the_wheel_over_the_list_scrolls_nothing() {
        let (mut f, _) = running();
        let area = Rect::new(40, 1, 60, 20);
        let wheel = |column, row| MouseEvent {
            kind: MouseEventKind::ScrollUp,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        assert_eq!(f.app.handle_mouse(&wheel(10, 5), area), Action::None);
        assert_eq!(f.app.handle_mouse(&wheel(50, 0), area), Action::None);
        assert_eq!(
            f.app.handle_mouse(&wheel(50, 5), area),
            Action::ScrollLines(WHEEL_LINES)
        );
    }
}
