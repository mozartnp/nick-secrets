//! The state of the TUI and its transitions by key, with no terminal I/O: `handle_key` takes a key
//! and returns an `Action`, which the loop (main.rs) carries out. The only action that runs
//! bin/nick-init is `Action::RunInit`, and only the "Criar configuração" button of the
//! missing-config alert produces it (plan D7 of NS-31). The navigation is a tree (NS-38, the model
//! in tree.rs): Module > Project > Ticket > Agent, the keys of the selected node and its panel on
//! the right; "Configurações" is the only other screen. The rules of the tickets are in tickets.rs,
//! and none of them touches a file of the project. From a ticket, `g` opens an agent (NS-33) by the
//! flow of auto_scrum.sh (NS-35): the check of the launcher (`Action::CheckAgent`), the menu of the
//! stage types its flags offer, the form of the type (stages.rs; the long text in `$EDITOR`,
//! `Action::EditDescription`), the prompt built by bash (`Action::RenderPrompt`), its preview with
//! the `(s/N)` confirmation, and `Action::OpenAgent`, which the loop hands to `open_agent` with the
//! real `AgentLauncher` (launch.rs). Several agents run at once (NS-34): the tree stays on the
//! left, and the agent selected on it is shown on the right (NS-38); the reserved shortcut
//! (`keys::Shortcut`, kept in the database) toggles the focus between them; with the focus on the
//! terminal every other key goes to the program (`Action::Send`). A stopped agent (NS-39) is
//! resumed in its own conversation by `r` on its node (`Action::CheckResume`: the check, whether
//! claude still keeps the conversation, then `resume_agent`), and archived or unarchived by `o`.
//! The screen texts are in PT-BR, and drawing is ui.rs's job.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

use crate::agents::{self, Agent, AgentState, Holder, OpenAgent, Owner};
use crate::db::{Db, Epic, SETTING_RESERVED_SHORTCUT, SessionSwap, StageChangeRow};
use crate::docs::{self, Artifacts};
use crate::editor::{EditorOutcome, EditorRunner};
use crate::epics;
use crate::init::{InitOutcome, InitRunner};
use crate::keys::{self, Shortcut, TermModes};
use crate::launch::{self, AgentLauncher, HolderKey, Launch, Precheck, RenderRequest, Rendered};
use crate::mascot::{Mascot, Palette, SystemDice};
use crate::modules::ModuleKind;
use crate::paths::printable;
use crate::projects::{self, Availability, Check, Project};
use crate::stages::{self, Input, Rule, StageType};
use crate::tickets::{self, Ticket, TicketStage};
use crate::tree::{LiveAgent, NodeId, Tree, TreeData, Visible};

/// What the column on the left shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// "Configurações": the reserved shortcut (NS-34), opened by `c` on the module node; the
    /// right panel keeps showing the selected node.
    Settings,
    /// The tree Module > Project > Ticket > Agent (NS-38), with the panel of the selected node
    /// on the right.
    Tree,
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

/// What the dialog "Novo ticket" creates (NS-40): a ticket, in an epic or not (`a` on the node of
/// the project or of the epic), or an epic (`n` on the node of the project). The same two fields
/// and the same typing for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NewItem {
    Ticket { epic_id: Option<i64> },
    Epic,
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
    /// The `(s/N)` confirmation of a removal, with how many tickets and agent records go with
    /// the project, and how many of its agents this nick runs (they are ended first, NS-38).
    /// Since the NS-40 also how many epics go with it.
    ConfirmRemove {
        project: Project,
        tickets: i64,
        agents: i64,
        live: i64,
        epics: i64,
    },
    /// A new ticket of the project `project_id` (the node where `a` opened it, review of the
    /// NS-38, P5): the title (required) and the Jira ID (optional; empty generates a
    /// provisional one). Since the NS-40 `item` says whether it is a ticket (and in which epic)
    /// or an epic.
    NewTicket {
        project_id: i64,
        item: NewItem,
        title: String,
        jira_id: String,
        focus: TicketField,
    },
    /// The title of a ticket (its ID is not editable).
    EditTitle { ticket: Ticket, input: String },
    /// The `(s/N)` confirmation of hiding a ticket.
    ConfirmHide { ticket: Ticket },
    /// The title of an epic (NS-40).
    EditEpicTitle { epic: Epic, input: String },
    /// The `(s/N)` confirmation of hiding an epic and its branch of `tickets` tickets (NS-40).
    ConfirmHideEpic { epic: Epic, tickets: usize },
    /// "Editar ID" of an epic (NS-40), starting with the current one.
    EditEpicId { epic: Epic, input: String },
    /// The `(s/N)` confirmation of the new ID of an epic; `moves` when its folder exists now.
    ConfirmEditEpicId {
        epic: Epic,
        new: String,
        moves: bool,
    },
    /// "Mover ticket" (`p`, NS-40): `Sem épico` (None) and the visible epics of its project,
    /// `index` selected.
    MoveTicket {
        ticket: Ticket,
        choices: Vec<Option<Epic>>,
        index: usize,
    },
    /// "Abrir agente": the type of the stage for the ticket or the epic (`holder`, NS-40) of the
    /// project at `project`, among `types` (the menu of `choose_type` with the flags of the check;
    /// of an epic, `StageType::epic_menu`), `index` selected.
    ChooseType {
        project: PathBuf,
        holder: Holder,
        precheck: Precheck,
        types: Vec<StageType>,
        index: usize,
    },
    /// The form of `kind`: one value per field (stages.rs), `focus` the field being answered.
    StageForm {
        project: PathBuf,
        holder: Holder,
        precheck: Precheck,
        kind: StageType,
        values: Vec<String>,
        focus: usize,
        /// Dev and Review (NS-36): the plan field is empty, the ID is valid and
        /// `.nick/docs/<ID>/plan.md` does not exist, computed at the opening and at each key
        /// (the ui reads no disk). Only a warning: it blocks nothing.
        plan_missing: bool,
    },
    /// The prompt bash built, before the launch: the `(s/N)` confirmation, with the prompt
    /// scrolled `scroll` lines down.
    ConfirmPrompt {
        project: PathBuf,
        holder: Holder,
        precheck: Precheck,
        kind: StageType,
        rendered: Rendered,
        scroll: usize,
    },
    /// Why no agent was opened (the launcher is missing, or its check failed).
    AgentError { message: String },
    /// The `(s/N)` confirmation of ending the agent `id` (`x` on its node, NS-38).
    ConfirmEndAgent { id: i64, name: String },
    /// The conversation of `agent` is no longer in claude (NS-39): nothing was opened. `a`
    /// archives it (the dialog is the question), `n` opens the menu of a new agent of its stage
    /// for its ticket or epic (`holder`) with the `precheck` of the resume, Esc closes it.
    ConversationLost {
        project: PathBuf,
        agent: Agent,
        holder: Holder,
        precheck: Precheck,
    },
    /// The `(s/N)` confirmation of archiving a stopped agent (`o` on its node, NS-39).
    ConfirmArchive { agent: Agent },
    /// The `(s/N)` confirmation of quitting with agents running (they are all ended).
    ConfirmQuit,
    /// The capture of a new reserved shortcut: `first` is the key pressed once, which must be
    /// pressed again to be saved.
    CaptureShortcut { first: Option<Shortcut> },
    /// "Mudar etapa" (NS-36): the stages of `TicketStage::ALL`, `index` selected.
    ChooseTicketStage { ticket: Ticket, index: usize },
    /// The `(s/N)` confirmation of changing the stage of the ticket to `to`.
    ConfirmStageChange { ticket: Ticket, to: TicketStage },
    /// "Editar ID" (NS-36): the new ID, starting with the current one.
    EditId { ticket: Ticket, input: String },
    /// The `(s/N)` confirmation of the new ID; `moves` when `.nick/docs/<old>/` exists now.
    ConfirmEditId {
        ticket: Ticket,
        new: String,
        moves: bool,
    },
}

impl Dialog {
    /// Whether the dialog has a text field, which gets the keys as they were typed: the others
    /// get them through `keys::shortcut_key`, where a capital letter is the small one (NS-43).
    /// No `_`: a new dialog must say whether it takes text.
    pub fn takes_text(&self) -> bool {
        match self {
            Dialog::AddPath { .. }
            | Dialog::NewTicket { .. }
            | Dialog::EditTitle { .. }
            | Dialog::EditEpicTitle { .. }
            | Dialog::EditEpicId { .. }
            | Dialog::EditId { .. }
            | Dialog::StageForm { .. } => true,
            Dialog::NeedsConfig { .. }
            | Dialog::ConfirmRemove { .. }
            | Dialog::ConfirmHide { .. }
            | Dialog::ConfirmHideEpic { .. }
            | Dialog::ConfirmEditEpicId { .. }
            | Dialog::MoveTicket { .. }
            | Dialog::ChooseType { .. }
            | Dialog::ConfirmPrompt { .. }
            | Dialog::AgentError { .. }
            | Dialog::ConfirmEndAgent { .. }
            | Dialog::ConversationLost { .. }
            | Dialog::ConfirmArchive { .. }
            | Dialog::ConfirmQuit
            | Dialog::ChooseTicketStage { .. }
            | Dialog::ConfirmStageChange { .. }
            | Dialog::ConfirmEditId { .. } => false,
            // Taken by `App::handle_key` before the shortcuts: it never gets here.
            Dialog::CaptureShortcut { .. } => false,
        }
    }

    /// Whether the action the dialog confirms has no way back (NS-46): its border is red
    /// (`ui::IRREVERSIBLE_COLOR`). Exhaustive, without `_`: a new dialog must decide.
    pub fn irreversible(&self) -> bool {
        match self {
            // Deletes the project from the database with its tickets, epics, agents and history.
            Dialog::ConfirmRemove { .. } => true,
            // Hiding, archiving, editing an ID, ending an agent (resumable, NS-39) and quitting
            // have a way back; the others are forms, choices and alerts.
            Dialog::AddPath { .. }
            | Dialog::NeedsConfig { .. }
            | Dialog::NewTicket { .. }
            | Dialog::EditTitle { .. }
            | Dialog::ConfirmHide { .. }
            | Dialog::EditEpicTitle { .. }
            | Dialog::ConfirmHideEpic { .. }
            | Dialog::EditEpicId { .. }
            | Dialog::ConfirmEditEpicId { .. }
            | Dialog::MoveTicket { .. }
            | Dialog::ChooseType { .. }
            | Dialog::StageForm { .. }
            | Dialog::ConfirmPrompt { .. }
            | Dialog::AgentError { .. }
            | Dialog::ConfirmEndAgent { .. }
            | Dialog::ConversationLost { .. }
            | Dialog::ConfirmArchive { .. }
            | Dialog::ConfirmQuit
            | Dialog::CaptureShortcut { .. }
            | Dialog::ChooseTicketStage { .. }
            | Dialog::ConfirmStageChange { .. }
            | Dialog::EditId { .. }
            | Dialog::ConfirmEditId { .. } => false,
        }
    }
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
    /// Check the launcher before the menu of the types (`g` on a ticket or an epic); the loop
    /// calls `check_agent`.
    CheckAgent {
        project: PathBuf,
        holder: Holder,
    },
    /// Open the editor on the focused field of the form; the loop calls `edit_description`.
    EditDescription,
    /// Build the prompt of the form (its answers passed the rules); the loop calls
    /// `render_prompt`.
    RenderPrompt,
    /// Open an agent of `kind` for the ticket or the epic `holder`, with the cwd at `project` (the
    /// path in the database) and the prompt of `log` (confirmed on the preview); the loop calls
    /// `open_agent`.
    OpenAgent {
        project: PathBuf,
        holder: Holder,
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
    /// End the agent with this id (confirmed on its node): the loop terminates its
    /// session and calls `agent_ended_by_operator`.
    EndAgent(i64),
    /// Quit, ending every agent (confirmed): the loop terminates every session and calls
    /// `end_all_records`.
    QuitEndingAgents,
    /// Remove `project`, whose live `agents` this nick runs (confirmed, NS-38): the loop
    /// terminates their sessions and calls `remove_project_ending_agents`.
    RemoveProject {
        project: Project,
        agents: Vec<i64>,
    },
    /// Resume the stopped `agent`, with the cwd at `project` (`r` on its node, NS-39): the loop
    /// calls `check_resume`.
    CheckResume {
        project: PathBuf,
        agent: Agent,
    },
}

/// The status when the terminal of an agent drops a Ctrl+Z (NS-44). Short on purpose: it fits
/// one row of the status bar, so it doesn't change the height of the panel (a second row would
/// resize every PTY, NS-34).
const CTRL_Z_IGNORED: &str = "O Ctrl+Z não tem efeito dentro do nick.";

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
    /// Where it is in the tree (NS-38): its ticket or epic (NS-40) and their project.
    pub owner: Owner,
    pub project_id: i64,
    /// Whether it runs in the jail (the `jail=` of the check). Only in memory: every running
    /// agent is of this process (NS-34, not a column).
    pub jail: bool,
    /// What the check wrote on stderr, shown when the agent ends.
    warning: Option<String>,
    /// The text of its screen when it stopped being shown (`hidden_output`).
    seen_text: String,
    /// Marked: its text changed while hidden ("● saída nova").
    pub unseen: bool,
}

/// The selected ticket (NS-36; the panel on the right since the NS-38), read again after every
/// action: the artifacts come from the disk each time, with no cache. Its agents are on the
/// tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketDetail {
    pub ticket: Ticket,
    /// The history of its stage, oldest first.
    pub changes: Vec<StageChangeRow>,
    pub artifacts: Artifacts,
}

/// The selected epic (NS-40), read again after every action, as the `TicketDetail`: the artifacts
/// come from the disk each time; the count of its tickets from the tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EpicDetail {
    pub epic: Epic,
    pub artifacts: Artifacts,
    /// Its tickets, the hidden ones included, and how many of them are hidden.
    pub tickets: usize,
    pub hidden_tickets: usize,
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
    pub(crate) dialog: Option<Dialog>,
    pub(crate) status: Option<String>,
    /// The projects of the module, with their availability.
    pub(crate) rows: Vec<Row>,
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
    /// The reserved shortcut in use, and whether the database has a row for it (no row: the
    /// default).
    pub(crate) shortcut: Shortcut,
    pub(crate) shortcut_stored: bool,
    /// The window of the preview of the prompt: the rows shown at a time (a page) and the
    /// largest scroll (its end). The loop computes it from the screen (`ui::prompt_window`) and
    /// sets it before each key, so the App does not know the drawing.
    pub(crate) prompt_window: (usize, usize),
    /// The detail of the selected ticket (NS-36; the panel on the right since the NS-38).
    pub(crate) ticket_detail: Option<TicketDetail>,
    /// The tree (NS-38): the open branches and the filters.
    pub(crate) tree: Tree,
    /// The selected node.
    pub(crate) selected: NodeId,
    /// The tickets of every project of the module (the hidden ones too), by project id.
    pub(crate) tickets: HashMap<i64, Vec<Ticket>>,
    /// The epics of every project of the module (the hidden ones too), by project id (NS-40).
    pub(crate) epics: HashMap<i64, Vec<Epic>>,
    /// The detail of the selected epic (NS-40).
    pub(crate) epic_detail: Option<EpicDetail>,
    /// The agents (of every stage and state) of the tickets and epics of the open projects and of
    /// the projects with a live agent, by owner.
    pub(crate) owner_agents: HashMap<Owner, Vec<Agent>>,
    /// The visible rows of the tree, computed after every change of the tree.
    pub(crate) visible: Vec<Visible>,
    /// The last agent shown (decision 2 of NS-38): where the shortcut goes from another node.
    pub(crate) last_shown: Option<i64>,
    /// The cat Nick at the bottom of the column on the left (NS-51), and whether the terminal
    /// shows her in truecolor (else in the 256 palette). Only `tick_mascot` moves her.
    mascot: Mascot,
    truecolor: bool,
    /// The colors of the cat for `truecolor`, resolved once (review NS-51, problem 1).
    palette: Palette,
}

impl App {
    /// A new TUI on the tree, with the module selected and every branch closed. `home` expands `~`
    /// and `cwd` resolves a relative path typed by the operator; `init` is the bin/nick-init to run
    /// from the button.
    pub fn new(db: Db, home: Option<PathBuf>, cwd: Option<PathBuf>, init: PathBuf) -> App {
        let mut app = App {
            db,
            home,
            cwd,
            init,
            screen: Screen::Tree,
            dialog: None,
            status: None,
            rows: Vec::new(),
            launcher: launch::launcher_path(),
            agents_dir: None,
            agents: Vec::new(),
            shown: None,
            focus: Focus::List,
            agent_term: AgentTerm::default(),
            shortcut: Shortcut::DEFAULT,
            shortcut_stored: false,
            prompt_window: (0, 0),
            ticket_detail: None,
            tree: Tree::default(),
            selected: NodeId::Module(ModuleKind::ALL[0]),
            tickets: HashMap::new(),
            epics: HashMap::new(),
            epic_detail: None,
            owner_agents: HashMap::new(),
            visible: Vec::new(),
            last_shown: None,
            mascot: Mascot::new(Instant::now(), SystemDice::new()),
            truecolor: false,
            palette: Palette::new(false),
        };
        app.refresh_tree();
        app
    }

    /// The cat Nick (NS-51).
    pub fn mascot(&self) -> &Mascot {
        &self.mascot
    }

    /// Replaces the cat (the loop, by the test hook NICK_TEST_MASCOT).
    pub fn set_mascot(&mut self, mascot: Mascot) {
        self.mascot = mascot;
    }

    /// Moves the clock of the cat to `now`. True when she shows another frame (a redraw). It
    /// touches nothing else: not the focus, the selection, the dialog, the status nor the agents.
    pub fn tick_mascot(&mut self, now: Instant) -> bool {
        self.mascot.tick(now)
    }

    /// When the cat changes next (None: never): where the idle loop wakes.
    pub fn mascot_wake(&self) -> Option<Instant> {
        self.mascot.next_wake()
    }

    /// Whether the terminal has truecolor (COLORTERM, read by the loop).
    pub fn truecolor(&self) -> bool {
        self.truecolor
    }

    /// Sets whether the terminal has truecolor, and resolves the colors of the cat for it.
    pub fn set_truecolor(&mut self, truecolor: bool) {
        self.truecolor = truecolor;
        self.palette = Palette::new(truecolor);
    }

    /// The colors of the cat on this terminal.
    pub fn palette(&self) -> &Palette {
        &self.palette
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

    /// The module whose projects are on the tree (the only one today: `ModuleKind::ALL`).
    pub fn module(&self) -> ModuleKind {
        ModuleKind::ALL[0]
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
            | Action::EndAgent(_)
            | Action::RemoveProject { .. }
            | Action::CheckResume { .. } => false,
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
        // With the focus on the terminal every other key is the program's, Ctrl+C included
        // (`key_agent` drops only the Ctrl+Z, NS-44).
        if self.focus == Focus::Terminal {
            return self.key_agent(key);
        }
        // Ctrl+C quits from anywhere: raw mode delivers it as a key, and the operator can't
        // be stuck. With agents running, it asks first.
        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            return self.quit();
        }
        // The shortcuts don't tell a capital letter from the small one (NS-43); a text field
        // gets the key as it was typed.
        let key = if self.dialog.as_ref().is_some_and(Dialog::takes_text) {
            key
        } else {
            keys::shortcut_key(key)
        };
        let action = match self.dialog.take() {
            Some(Dialog::AddPath { input }) => self.key_add_path(key, input),
            Some(Dialog::NeedsConfig { dir, focus, note }) => {
                self.key_needs_config(key, dir, focus, note)
            }
            Some(Dialog::ConfirmRemove { project, .. }) => self.key_confirm_remove(key, project),
            Some(Dialog::NewTicket {
                project_id,
                item,
                title,
                jira_id,
                focus,
            }) => self.key_new_ticket(key, project_id, item, title, jira_id, focus),
            Some(Dialog::EditTitle { ticket, input }) => self.key_edit_title(key, ticket, input),
            Some(Dialog::ConfirmHide { ticket }) => self.key_confirm_hide(key, ticket),
            Some(Dialog::EditEpicTitle { epic, input }) => {
                self.key_edit_epic_title(key, epic, input)
            }
            Some(Dialog::ConfirmHideEpic { epic, .. }) => {
                // Only s/S confirm; the conditioned UPDATE runs again (decision 8 of NS-40).
                if is_yes(key) {
                    self.status = Some(match epics::hide(&self.db, &epic) {
                        Ok(message) | Err(message) => message,
                    });
                }
                Action::None
            }
            Some(Dialog::EditEpicId { epic, input }) => self.key_edit_epic_id(key, epic, input),
            Some(Dialog::ConfirmEditEpicId { epic, new, .. }) => {
                self.confirm_edit_id(key, &epic, epic.project_id, &new, epics::edit_id)
            }
            Some(Dialog::MoveTicket {
                ticket,
                choices,
                index,
            }) => self.key_move_ticket(key, ticket, choices, index),
            Some(Dialog::ChooseType {
                project,
                holder,
                precheck,
                types,
                index,
            }) => self.key_choose_type(key, project, holder, precheck, types, index),
            Some(Dialog::StageForm {
                project,
                holder,
                precheck,
                kind,
                values,
                focus,
                ..
            }) => self.key_stage_form(key, project, holder, precheck, kind, values, focus),
            Some(Dialog::ConfirmPrompt {
                project,
                holder,
                precheck,
                kind,
                rendered,
                scroll,
            }) => self.key_confirm_prompt(key, project, holder, precheck, kind, rendered, scroll),
            Some(Dialog::AgentError { message }) => self.key_agent_error(key, message),
            Some(Dialog::ConfirmEndAgent { id, .. }) => confirmed(key, Action::EndAgent(id)),
            Some(Dialog::ConversationLost {
                project,
                agent,
                holder,
                precheck,
            }) => self.key_conversation_lost(key, project, agent, holder, precheck),
            Some(Dialog::ConfirmArchive { agent }) => {
                // Only s/S confirm; any other key cancels.
                if is_yes(key) {
                    self.status = Some(match agents::archive(&self.db, &agent) {
                        Ok(message) | Err(message) => message,
                    });
                }
                Action::None
            }
            Some(Dialog::ConfirmQuit) => confirmed(key, Action::QuitEndingAgents),
            // Taken before the dialogs (above).
            Some(Dialog::CaptureShortcut { .. }) => Action::None,
            Some(Dialog::ChooseTicketStage { ticket, index }) => {
                self.key_choose_ticket_stage(key, ticket, index)
            }
            Some(Dialog::ConfirmStageChange { ticket, to }) => {
                // Only s/S confirm; any other key cancels. The only call of change_stage: the
                // nick never changes the stage alone (plan D9 of NS-36).
                if is_yes(key) {
                    self.status = Some(match tickets::change_stage(&self.db, &ticket, to) {
                        Ok(message) | Err(message) => message,
                    });
                }
                Action::None
            }
            Some(Dialog::EditId { ticket, input }) => self.key_edit_id(key, ticket, input),
            Some(Dialog::ConfirmEditId { ticket, new, .. }) => {
                self.key_confirm_edit_id(key, ticket, new)
            }
            None => match self.screen {
                Screen::Settings => self.key_settings(key),
                Screen::Tree => self.key_tree(key),
            },
        };
        // The tree is read again after every action on it (plan D9 of NS-31): not while a
        // dialog stays open (typing a path is not an action, and each refresh reads the disk),
        // but when it closes, whatever closed it.
        if self.dialog.is_none() && self.screen == Screen::Tree {
            self.refresh_tree();
        }
        action
    }

    /// The reserved shortcut: from the terminal back to the list; from the list to the terminal
    /// of the shown agent, but not with a dialog open (it is a pending question) nor without
    /// an agent shown.
    /// On the tree (decision 2 of NS-38): a live agent selected gets the focus; from another
    /// node, the last agent shown, when still live, is selected (its branches opened), shown and
    /// focused; without a live agent, nothing.
    fn toggle_focus(&mut self) {
        match self.focus {
            Focus::Terminal => self.focus = Focus::List,
            Focus::List if self.dialog.is_some() => {}
            Focus::List => {
                let target = match self.selected {
                    NodeId::Agent(id) if self.agent(id).is_some() => Some(id),
                    _ => self.last_shown.filter(|id| self.agent(*id).is_some()),
                };
                if let Some(id) = target {
                    if self.selected != NodeId::Agent(id) {
                        self.reveal_and_select(NodeId::Agent(id));
                        self.refresh_tree();
                    }
                    self.show(id);
                    self.focus = Focus::Terminal;
                }
            }
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

    /// The live agents, as the tree sees them.
    fn live(&self) -> Vec<LiveAgent> {
        self.agents
            .iter()
            .map(|agent| LiveAgent {
                id: agent.id,
                owner: agent.owner,
                project_id: agent.project_id,
                epic_id: self.epic_of_owner(agent.owner),
                unseen: agent.unseen,
            })
            .collect()
    }

    fn tree_data<'a>(&'a self, live: &'a [LiveAgent]) -> TreeData<'a> {
        TreeData {
            module: self.module(),
            projects: &self.rows,
            tickets: &self.tickets,
            epics: &self.epics,
            agents: &self.owner_agents,
            live,
        }
    }

    /// The visible rows of the tree, in order.
    pub fn visible(&self) -> &[Visible] {
        &self.visible
    }

    /// The selected node of the tree.
    pub fn selected(&self) -> NodeId {
        self.selected
    }

    /// The project `id` of the module, with its availability.
    pub fn project_row(&self, id: i64) -> Option<&Row> {
        self.rows.iter().find(|row| row.project.id == id)
    }

    /// The projects of the module, with their availability.
    pub fn project_rows(&self) -> &[Row] {
        &self.rows
    }

    /// The tickets of the project `id` (the hidden ones too).
    pub fn project_tickets(&self, id: i64) -> &[Ticket] {
        self.tickets.get(&id).map_or(&[][..], Vec::as_slice)
    }

    /// The ticket `id`, of any project.
    pub fn ticket(&self, id: i64) -> Option<&Ticket> {
        self.tickets
            .values()
            .flatten()
            .find(|ticket| ticket.id == id)
    }

    /// The epics of the project `id` (the hidden ones too).
    pub fn project_epics(&self, id: i64) -> &[Epic] {
        self.epics.get(&id).map_or(&[][..], Vec::as_slice)
    }

    /// The epic `id`, of any project.
    pub fn epic(&self, id: i64) -> Option<&Epic> {
        self.epics.values().flatten().find(|epic| epic.id == id)
    }

    /// The epic of the branch of `owner` (NS-40): the epic itself, or the epic of the ticket.
    pub fn epic_of_owner(&self, owner: Owner) -> Option<i64> {
        match owner {
            Owner::Epic(id) => Some(id),
            Owner::Ticket(id) => self.ticket(id).and_then(|ticket| ticket.epic_id),
        }
    }

    /// The tickets of the epic `id` (the hidden ones too).
    pub fn epic_tickets(&self, id: i64) -> Vec<&Ticket> {
        self.tickets
            .values()
            .flatten()
            .filter(|ticket| ticket.epic_id == Some(id))
            .collect()
    }

    /// The project of the ticket or epic `owner`.
    fn project_id_of_owner(&self, owner: Owner) -> Option<i64> {
        match owner {
            Owner::Ticket(id) => self.ticket(id).map(|ticket| ticket.project_id),
            Owner::Epic(id) => self.epic(id).map(|epic| epic.project_id),
        }
    }

    /// The record of the agent `id` (of every state), when its owner's agents were read.
    pub fn agent_record(&self, id: i64) -> Option<&Agent> {
        self.owner_agents
            .values()
            .flatten()
            .find(|agent| agent.id == id)
    }

    /// The filters of the tree: the hidden tickets (`f`) and only the active agents (`v`).
    pub fn filters(&self) -> (bool, bool) {
        (self.tree.show_hidden, self.tree.only_active)
    }

    /// The project of the selected node (a project, or the project of a ticket or an agent).
    pub fn selected_project(&self) -> Option<&Project> {
        let project = match self.selected {
            NodeId::Module(_) => return None,
            NodeId::Project(id) => id,
            NodeId::Epic(id) => self.epic(id)?.project_id,
            NodeId::Ticket(id) => self.ticket(id)?.project_id,
            NodeId::Agent(id) => {
                let owner = match self.agent(id) {
                    Some(agent) => agent.owner,
                    None => self.agent_record(id)?.owner,
                };
                self.project_id_of_owner(owner)?
            }
        };
        self.project_row(project).map(|row| &row.project)
    }

    /// The project of `ticket`, from the list of the module.
    fn project_of(&self, ticket: &Ticket) -> Option<&Project> {
        self.project_row(ticket.project_id).map(|row| &row.project)
    }

    /// The project of the ticket or the epic `holder` (NS-40).
    fn project_of_holder(&self, holder: &Holder) -> Option<&Project> {
        self.project_row(holder.project_id).map(|row| &row.project)
    }

    /// Computes the visible rows again (after the tree or its data changed).
    pub(crate) fn relayout(&mut self) {
        let live = self.live();
        self.visible = self.tree.visible(&self.tree_data(&live));
    }

    /// Opens every ancestor of `id` and selects it (a new node, or the agent of the shortcut).
    pub(crate) fn reveal_and_select(&mut self, id: NodeId) {
        let live = self.live();
        let mut tree = std::mem::take(&mut self.tree);
        tree.reveal(id, &self.tree_data(&live));
        self.tree = tree;
        self.select(id);
    }

    /// Selects `id`. "Shown" follows the selection (decision 5 of NS-38): a live agent of this
    /// nick is shown on the right; any other node leaves every agent hidden.
    pub(crate) fn select(&mut self, id: NodeId) {
        self.selected = id;
        match id {
            NodeId::Agent(agent) if self.agent(agent).is_some() => self.show(agent),
            _ => {
                if self.shown.take().is_some() {
                    self.agent_term = AgentTerm::default();
                }
                if self.focus == Focus::Terminal {
                    self.focus = Focus::List;
                }
            }
        }
    }

    /// Reads the tree again: the projects with their availability (from the disk, as the
    /// Projects screen did), the tickets of every project and the agents of the tickets of the
    /// open projects and of the projects with a live agent (from the database), and the detail
    /// of the selected ticket. An open project that became unavailable is closed (decision 11),
    /// unless a live agent of this nick is below it (decision 7: it stays reachable).
    /// The selection stays on its node, or goes to the next sibling, the previous one or the
    /// parent when its node is gone (decision 9).
    pub(crate) fn refresh_tree(&mut self) {
        self.refresh();
        let live_projects: HashSet<i64> = self.agents.iter().map(|a| a.project_id).collect();
        // One with a live agent stays open: the agent stays reachable (decision 7).
        let unavailable: Vec<i64> = self
            .rows
            .iter()
            .filter(|row| row.availability != Availability::Available)
            .map(|row| row.project.id)
            .filter(|id| !live_projects.contains(id))
            .collect();
        for id in unavailable {
            self.tree.close(NodeId::Project(id));
        }
        let mut tickets = HashMap::new();
        let mut epics = HashMap::new();
        let mut agents = HashMap::new();
        let mut error = None;
        for row in &self.rows {
            let id = row.project.id;
            // One query of epics per project, as the tickets (P2 of the review of the NS-38).
            match self.db.list_epics(id, true) {
                Ok(list) => {
                    epics.insert(id, list);
                }
                Err(err) => error = Some(err),
            }
            match self.db.list_tickets(id, true) {
                Ok(list) => {
                    // One query per open project (or with a live agent), not one per ticket:
                    // this runs after every key (review of the NS-38, P2). Ordered by id, so
                    // each ticket and epic keeps its agents in the order they were opened.
                    if self.tree.is_open(NodeId::Project(id)) || live_projects.contains(&id) {
                        match self.db.list_project_agents(id) {
                            Ok(found) => {
                                for ticket in &list {
                                    agents.insert(Owner::Ticket(ticket.id), Vec::new());
                                }
                                for epic in epics.get(&id).map_or(&[][..], Vec::as_slice) {
                                    agents.insert(Owner::Epic(epic.id), Vec::new());
                                }
                                for agent in found {
                                    agents
                                        .entry(agent.owner)
                                        .or_insert_with(Vec::new)
                                        .push(agent);
                                }
                            }
                            Err(err) => error = Some(err),
                        }
                    }
                    tickets.insert(id, list);
                }
                Err(err) => error = Some(err),
            }
        }
        if let Some(err) = error {
            self.status = Some(projects::db_error_message(&err));
        }
        // The selected ticket gone from the database (another nick removed it): say so.
        if let NodeId::Ticket(id) = self.selected
            && let Some(old) = self.ticket(id)
            && !tickets.values().flatten().any(|t: &Ticket| t.id == id)
        {
            self.status = Some(tickets::gone_message(old));
        }
        // The selected epic gone from the database: say so too (NS-40).
        if let NodeId::Epic(id) = self.selected
            && let Some(old) = self.epic(id)
            && !epics.values().flatten().any(|e: &Epic| e.id == id)
        {
            self.status = Some(epics::gone_message(old));
        }
        self.tickets = tickets;
        self.epics = epics;
        self.owner_agents = agents;
        let before = std::mem::take(&mut self.visible);
        self.relayout();
        let selected = Tree::reselect(&before, &self.visible, self.selected);
        if selected != self.selected {
            self.select(selected);
        }
        self.read_ticket_detail();
        self.read_epic_detail();
    }

    /// The detail of the selected epic (NS-40): the row from the database, its artifacts from the
    /// disk, and the count of its tickets from the tree; None on another node.
    fn read_epic_detail(&mut self) {
        let NodeId::Epic(id) = self.selected else {
            self.epic_detail = None;
            return;
        };
        let Some(path) = self
            .epic(id)
            .and_then(|epic| self.project_row(epic.project_id))
            .map(|row| row.project.path.clone())
        else {
            self.epic_detail = None;
            return;
        };
        match self.db.get_epic(id) {
            Ok(Some(epic)) => {
                let tickets = self.epic_tickets(id);
                let hidden_tickets = tickets.iter().filter(|t| t.hidden_at.is_some()).count();
                self.epic_detail = Some(EpicDetail {
                    artifacts: docs::artifacts(&path, &epic.key),
                    tickets: tickets.len(),
                    hidden_tickets,
                    epic,
                });
            }
            Ok(None) => self.epic_detail = None,
            Err(err) => self.status = Some(projects::db_error_message(&err)),
        }
    }

    /// The detail of the selected ticket (its history from the database, its artifacts from
    /// the disk); None on another node. Only reads: nothing here writes the stage.
    fn read_ticket_detail(&mut self) {
        let NodeId::Ticket(id) = self.selected else {
            self.ticket_detail = None;
            return;
        };
        let Some(path) = self
            .ticket(id)
            .and_then(|ticket| self.project_of(ticket))
            .map(|project| project.path.clone())
        else {
            self.ticket_detail = None;
            return;
        };
        let read = || -> rusqlite::Result<Option<TicketDetail>> {
            let Some(ticket) = self.db.get_ticket(id)? else {
                return Ok(None);
            };
            Ok(Some(TicketDetail {
                changes: self.db.list_stage_changes(id)?,
                artifacts: docs::artifacts(&path, &ticket.key),
                ticket,
            }))
        };
        match read() {
            Ok(detail) => self.ticket_detail = detail,
            Err(err) => self.status = Some(projects::db_error_message(&err)),
        }
    }

    /// The index of the selected row.
    fn selected_row(&self) -> Option<usize> {
        self.visible.iter().position(|row| row.id == self.selected)
    }

    fn key_tree(&mut self, key: KeyEvent) -> Action {
        let at = self.selected_row();
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => {
                if let Some(at) = at.filter(|at| *at > 0) {
                    self.select(self.visible[at - 1].id);
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if let Some(row) = at.and_then(|at| self.visible.get(at + 1)) {
                    self.select(row.id);
                }
            }
            KeyCode::Enter | KeyCode::Right => self.tree_enter(),
            KeyCode::Esc | KeyCode::Left => self.tree_back(),
            KeyCode::Char('f') => self.tree.show_hidden = !self.tree.show_hidden,
            KeyCode::Char('v') => {
                self.tree.only_active = !self.tree.only_active;
                self.status = None;
            }
            _ if is_quit(key) => return self.quit(),
            _ => return self.key_node(key),
        }
        // No relayout here: the refresh after the key computes the rows, and `reselect` needs
        // the ones of before the key (a filter may take the selected node away).
        Action::None
    }

    /// `→`/`Enter`: a live agent gets the focus on its terminal; another agent says why not; an
    /// unavailable project says why it does not open (read from the disk now); a closed node
    /// opens, and an open one goes down to its first child.
    fn tree_enter(&mut self) {
        match self.selected {
            NodeId::Agent(id) => {
                if self.agent(id).is_some() {
                    self.show(id);
                    self.focus = Focus::Terminal;
                    self.status = None;
                } else if let Some(agent) = self.agent_record(id) {
                    // Which key, by the state (NS-39); an active one of another nick: as before.
                    self.status = Some(match AgentState::from_key(&agent.state) {
                        Some(AgentState::Stopped) => format!(
                            "O agente {} está parado. Pressione r para retomar.",
                            agent.name
                        ),
                        Some(AgentState::Archived) => format!(
                            "O agente {} está arquivado. Pressione o para desarquivar.",
                            agent.name
                        ),
                        _ => format!("O agente {} não está rodando neste nick.", agent.name),
                    });
                }
                return;
            }
            NodeId::Project(id)
                if !self.tree.only_active && !self.agents.iter().any(|a| a.project_id == id) =>
            {
                let Some(row) = self.project_row(id) else {
                    return;
                };
                if let Some(mark) =
                    projects::unavailable_text(projects::availability(&row.project.path))
                {
                    self.status = Some(format!(
                        "Projeto indisponível: {mark}. Os tickets só abrem com a pasta e o \
                         .nick/nick.conf no lugar."
                    ));
                    return;
                }
                if !self.tree.is_open(self.selected) {
                    // Available now, maybe not at the last refresh.
                    self.tree.open(self.selected);
                    self.status = None;
                    return;
                }
            }
            _ => {}
        }
        let Some(at) = self.selected_row() else {
            return;
        };
        let row = self.visible[at];
        if !row.expandable {
            return;
        }
        if !row.open {
            self.tree.open(row.id);
            self.status = None;
        } else if let Some(child) = self.visible.get(at + 1).filter(|c| c.depth > row.depth) {
            self.select(child.id);
        }
    }

    /// `←`/`Esc`: an open node closes (not under the filter `v`, which shows every branch
    /// open); a closed one or a leaf goes up to its parent.
    fn tree_back(&mut self) {
        self.status = None;
        if let Some(row) = self.selected_row().map(|at| self.visible[at])
            && row.open
            && !self.tree.only_active
        {
            self.tree.close(row.id);
            return;
        }
        let live = self.live();
        if let Some(parent) = self.tree.parent(self.selected, &self.tree_data(&live)) {
            self.select(parent);
        }
    }

    /// The keys of the selected node (NS-38, the table of the plan): the same dialogs and
    /// messages as the screens of before.
    fn key_node(&mut self, key: KeyEvent) -> Action {
        match (self.selected, key.code) {
            (NodeId::Module(_), KeyCode::Char('a')) => {
                self.dialog = Some(Dialog::AddPath {
                    input: String::new(),
                });
            }
            (NodeId::Module(_), KeyCode::Char('c')) => {
                self.screen = Screen::Settings;
                self.status = None;
            }
            (NodeId::Project(project_id), KeyCode::Char('a')) => {
                self.new_item(project_id, NewItem::Ticket { epic_id: None });
            }
            (NodeId::Project(project_id), KeyCode::Char('n')) => {
                self.new_item(project_id, NewItem::Epic);
            }
            (NodeId::Epic(id), code) => {
                let Some(epic) = self.epic(id).cloned() else {
                    return Action::None;
                };
                return self.key_epic_node(code, epic);
            }
            (NodeId::Project(id), KeyCode::Char('r')) => {
                if let Some(project) = self.project_row(id).map(|row| row.project.clone()) {
                    self.ask_removal(project);
                }
            }
            (NodeId::Ticket(id), code) => {
                let Some(ticket) = self.ticket(id).cloned() else {
                    return Action::None;
                };
                return self.key_ticket_node(code, ticket);
            }
            (NodeId::Agent(id), KeyCode::Char('x')) => {
                if let Some(agent) = self.agent(id) {
                    self.dialog = Some(Dialog::ConfirmEndAgent {
                        id,
                        name: agent.name.clone(),
                    });
                }
            }
            (NodeId::Agent(id), KeyCode::Char('r')) if self.agent(id).is_none() => {
                return self.ask_resume(id);
            }
            (NodeId::Agent(id), KeyCode::Char('o')) => self.archive_key(id),
            _ => {}
        }
        Action::None
    }

    /// `r` on the node of an agent not live (NS-39): refused when archived, when another live
    /// nick holds its lock (early, before the `--check`: the guarantee is `agents::resume`'s), or
    /// when its project is unavailable now (the refusal of `g`); otherwise the loop runs the
    /// check of the resume (`Action::CheckResume`). A hidden ticket does not refuse it: the
    /// resume goes on a conversation that already exists (decision of the user in the NS-39).
    fn ask_resume(&mut self, id: i64) -> Action {
        let Some(cached) = self.agent_record(id).cloned() else {
            return Action::None;
        };
        // The row of now, and not the one the tree read (another nick may have resumed it
        // meanwhile: review of the NS-39, point 2).
        let agent = match self.db.get_agent(id) {
            Ok(Some(agent)) => agent,
            Ok(None) => {
                self.status = Some(format!(
                    "O agente {} não existe mais no banco.",
                    cached.name
                ));
                return Action::None;
            }
            Err(err) => {
                self.status = Some(projects::db_error_message(&err));
                return Action::None;
            }
        };
        let Some(state) = AgentState::from_key(&agent.state) else {
            self.status = Some(unknown_state_message(&agent));
            return Action::None;
        };
        if state == AgentState::Archived {
            self.status = Some(format!(
                "O agente {} está arquivado. Desarquive (o) antes de retomar.",
                agent.name
            ));
            return Action::None;
        }
        if state == AgentState::Active
            && self
                .agents_dir
                .as_deref()
                .is_some_and(|dir| agents::held_elsewhere(dir, &agent.session_id))
        {
            self.status = Some(agents::elsewhere_message(&agent));
            return Action::None;
        }
        let Some(project) = self
            .project_id_of_owner(agent.owner)
            .and_then(|id| self.project_row(id))
            .map(|row| row.project.clone())
        else {
            return Action::None;
        };
        if let Some(mark) = projects::unavailable_text(projects::availability(&project.path)) {
            self.status = Some(format!(
                "Projeto indisponível: {mark}. O agente só abre com a pasta e o \
                 .nick/nick.conf no lugar."
            ));
            return Action::None;
        }
        self.status = None;
        Action::CheckResume {
            project: project.path,
            agent,
        }
    }

    /// `o` on the node of an agent (NS-39): a stopped one asks `(s/N)` and is archived; an
    /// archived one goes back to stopped at once (as restoring a ticket); a live one, or an
    /// active one of another nick, is refused.
    fn archive_key(&mut self, id: i64) {
        let Some(agent) = self.agent_record(id).cloned() else {
            return;
        };
        match AgentState::from_key(&agent.state) {
            Some(AgentState::Stopped) if self.agent(id).is_none() => {
                self.status = None;
                self.dialog = Some(Dialog::ConfirmArchive { agent });
            }
            Some(AgentState::Archived) => {
                self.status = Some(match agents::unarchive(&self.db, &agent) {
                    Ok(message) | Err(message) => message,
                });
            }
            None => self.status = Some(unknown_state_message(&agent)),
            Some(_) => self.status = Some("Um agente ativo não pode ser arquivado.".into()),
        }
    }

    /// The keys of a ticket node: `e` the title, `o` hide (asking) or restore, `g` an agent,
    /// `m` the stage, `i` the ID.
    fn key_ticket_node(&mut self, code: KeyCode, ticket: Ticket) -> Action {
        match code {
            KeyCode::Char('e') => {
                self.dialog = Some(Dialog::EditTitle {
                    input: ticket.title.clone(),
                    ticket,
                });
            }
            KeyCode::Char('o') => {
                if ticket.hidden_at.is_some() {
                    // Restoring hides nothing and is undone with `o`: no confirmation.
                    self.status = Some(match tickets::restore(&self.db, &ticket) {
                        Ok(message) | Err(message) => message,
                    });
                } else {
                    self.dialog = Some(Dialog::ConfirmHide { ticket });
                }
            }
            KeyCode::Char('g') => return self.choose_stage(Some((&ticket).into())),
            KeyCode::Char('p') => self.ask_move(ticket),
            KeyCode::Char('m') => {
                let index = TicketStage::ALL
                    .iter()
                    .position(|stage| stage.key() == ticket.stage)
                    .unwrap_or(0);
                self.status = None;
                self.dialog = Some(Dialog::ChooseTicketStage { ticket, index });
            }
            KeyCode::Char('i') => {
                self.status = self.edit_id_refusal(ticket.project_id);
                if self.status.is_none() {
                    self.dialog = Some(Dialog::EditId {
                        input: ticket.key.clone(),
                        ticket,
                    });
                }
            }
            _ => {}
        }
        Action::None
    }

    /// Opens the dialog "Novo ticket" of the project for `item` (a ticket, in an epic or not, or
    /// an epic: NS-40).
    fn new_item(&mut self, project_id: i64, item: NewItem) {
        self.dialog = Some(Dialog::NewTicket {
            project_id,
            item,
            title: String::new(),
            jira_id: String::new(),
            focus: TicketField::Title,
        });
    }

    /// The keys of the node of an epic (NS-40): `a` a ticket in it (not in a hidden one), `e` the
    /// title, `o` hide its branch (asking, and refused with an active agent in it) or restore it,
    /// `g` an agent of PO or Tech Leader, `i` the ID. No `m`, `p` nor `r`: an epic has no stage.
    fn key_epic_node(&mut self, code: KeyCode, epic: Epic) -> Action {
        match code {
            KeyCode::Char('a') => {
                if epic.hidden_at.is_some() {
                    self.status = Some(format!(
                        "O épico {} está oculto. Restaure-o (o) para criar um ticket.",
                        epic.key
                    ));
                } else {
                    self.status = None;
                    self.new_item(
                        epic.project_id,
                        NewItem::Ticket {
                            epic_id: Some(epic.id),
                        },
                    );
                }
            }
            KeyCode::Char('e') => {
                self.dialog = Some(Dialog::EditEpicTitle {
                    input: epic.title.clone(),
                    epic,
                });
            }
            KeyCode::Char('o') => {
                if epic.hidden_at.is_some() {
                    // Restoring hides nothing and is undone with `o`: no confirmation.
                    self.status = Some(match epics::restore(&self.db, &epic) {
                        Ok(message) | Err(message) => message,
                    });
                    return Action::None;
                }
                // The refusal with the count right at the key, with no dialog; the confirmation
                // runs the conditioned UPDATE again.
                match self.db.count_active_agents_of_epic_branch(epic.id) {
                    Ok(0) => {
                        self.status = None;
                        let tickets = self.epic_tickets(epic.id).len();
                        self.dialog = Some(Dialog::ConfirmHideEpic { epic, tickets });
                    }
                    Ok(active) => self.status = Some(epics::active_message(&epic, active)),
                    Err(err) => self.status = Some(projects::db_error_message(&err)),
                }
            }
            KeyCode::Char('g') => return self.choose_stage(Some((&epic).into())),
            KeyCode::Char('i') => {
                self.status = self.edit_id_refusal(epic.project_id);
                if self.status.is_none() {
                    self.dialog = Some(Dialog::EditEpicId {
                        input: epic.key.clone(),
                        epic,
                    });
                }
            }
            _ => {}
        }
        Action::None
    }

    /// `p` on a ticket (NS-40): the list `Sem épico` and the visible epics of its project, the
    /// current place selected; without any epic to offer, why.
    fn ask_move(&mut self, ticket: Ticket) {
        let visible: Vec<Epic> = self
            .project_epics(ticket.project_id)
            .iter()
            .filter(|epic| epic.hidden_at.is_none())
            .cloned()
            .collect();
        if visible.is_empty() && ticket.epic_id.is_none() {
            self.status = Some("O projeto não tem épicos. Crie um com n no nó do projeto.".into());
            return;
        }
        let mut choices = vec![None];
        choices.extend(visible.into_iter().map(Some));
        let index = choices
            .iter()
            .position(|choice| choice.as_ref().map(|e| e.id) == ticket.epic_id)
            .unwrap_or(0);
        self.status = None;
        self.dialog = Some(Dialog::MoveTicket {
            ticket,
            choices,
            index,
        });
    }

    /// The list of "Mover ticket": Enter moves to the selected place (no `(s/N)`: moving is undone
    /// the same way and touches no file); the current place is refused with the list open.
    fn key_move_ticket(
        &mut self,
        key: KeyEvent,
        ticket: Ticket,
        choices: Vec<Option<Epic>>,
        mut index: usize,
    ) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.status = None;
                return Action::None;
            }
            KeyCode::Up | KeyCode::Char('k') => index = index.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                index = (index + 1).min(choices.len().saturating_sub(1));
            }
            KeyCode::Enter => {
                let to = choices.get(index).cloned().flatten();
                match epics::move_ticket(&self.db, &ticket, to.as_ref()) {
                    Ok(message) => {
                        self.status = Some(message);
                        // The ticket stays selected, in its new place.
                        if let Some(epic) = &to {
                            self.tree.open(NodeId::Epic(epic.id));
                        }
                        return Action::None;
                    }
                    Err(message) => self.status = Some(message),
                }
            }
            _ => {}
        }
        self.dialog = Some(Dialog::MoveTicket {
            ticket,
            choices,
            index,
        });
        Action::None
    }

    fn key_edit_epic_title(&mut self, key: KeyEvent, epic: Epic, input: String) -> Action {
        self.key_text_field(
            key,
            epic,
            input,
            |app, epic, input| {
                app.status = Some(epics::rename(&app.db, epic, input)?);
                Ok(None)
            },
            |epic, input| Dialog::EditEpicTitle { epic, input },
        )
    }

    /// The field of the new ID of an epic: the rules of `key_edit_id`, the same handler.
    fn key_edit_epic_id(&mut self, key: KeyEvent, epic: Epic, input: String) -> Action {
        self.key_text_field(
            key,
            epic,
            input,
            |app, epic, input| {
                let new = epics::check_new_id(&app.db, epic, input)?;
                app.status = None;
                Ok(Some(Dialog::ConfirmEditEpicId {
                    moves: app.has_docs(epic.project_id, &epic.key),
                    epic: epic.clone(),
                    new,
                }))
            },
            |epic, input| Dialog::EditEpicId { epic, input },
        )
    }

    /// The one-line field of the dialogs of a title and of an ID, of a ticket or of an epic
    /// (review of the NS-40, P2.1: one handler for both): Esc closes it, Enter runs `enter` (Ok:
    /// the dialog that follows, or none; Err: the refusal on the bar with the field open), and
    /// any other key types in it (`reopen` puts the dialog back with the text).
    fn key_text_field<T>(
        &mut self,
        key: KeyEvent,
        item: T,
        mut input: String,
        enter: fn(&mut App, &T, &str) -> Result<Option<Dialog>, String>,
        reopen: fn(T, String) -> Dialog,
    ) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.status = None;
                return Action::None;
            }
            KeyCode::Enter => match enter(self, &item, &input) {
                Ok(next) => {
                    self.dialog = next;
                    return Action::None;
                }
                Err(message) => self.status = Some(message),
            },
            _ => {
                edit_text(&mut input, key);
            }
        }
        self.dialog = Some(reopen(item, input));
        Action::None
    }

    /// Only s/S apply the new ID of a ticket or of an epic (`edit` runs every check again: the
    /// state may have changed); any other key cancels, renaming nothing.
    fn confirm_edit_id<T>(
        &mut self,
        key: KeyEvent,
        item: &T,
        project_id: i64,
        new: &str,
        edit: fn(&Db, &Path, &T, &str) -> Result<String, String>,
    ) -> Action {
        if is_yes(key)
            && let Some(row) = self.project_row(project_id)
        {
            let path = row.project.path.clone();
            self.status = Some(match edit(&self.db, &path, item, new) {
                Ok(message) | Err(message) => message,
            });
        }
        Action::None
    }

    /// The refusal of "Editar ID" (`i`) of a ticket or an epic when its project is unavailable
    /// now (read from the disk at the key); None when the ID can be edited.
    fn edit_id_refusal(&self, project_id: i64) -> Option<String> {
        let row = self.project_row(project_id)?;
        let mark = projects::unavailable_text(projects::availability(&row.project.path))?;
        Some(format!(
            "Projeto indisponível: {mark}. O ID só é editado com a pasta e o .nick/nick.conf no \
             lugar."
        ))
    }

    /// Whether `.nick/docs/<key>/` exists now in the project `project_id`.
    fn has_docs(&self, project_id: i64, key: &str) -> bool {
        self.project_row(project_id)
            .is_some_and(|row| docs::has_dir(&row.project.path, key))
    }

    /// The refusal of a new agent of `holder` (`g`, or `n` on a lost conversation) when it is
    /// hidden, or is a ticket of a hidden epic (decision 9 of NS-40); None when it can have one.
    fn hidden_refusal(&self, holder: &Holder) -> Option<String> {
        if holder.hidden {
            return Some(format!(
                "O {} {} está oculto. Restaure-o (o) para abrir um agente.",
                holder.noun(),
                holder.key
            ));
        }
        let Owner::Ticket(id) = holder.owner else {
            return None;
        };
        let epic = self.ticket(id)?.epic_id.and_then(|epic| self.epic(epic))?;
        epic.hidden_at
            .is_some()
            .then(|| epics::hidden_message(epic))
    }

    /// Shows the agent `id` on the right: being shown counts as seen, so its mark goes. The
    /// loop keeps the text of the one that stopped being shown (`left_shown`).
    fn show(&mut self, id: i64) {
        if self.shown != Some(id) {
            self.agent_term = AgentTerm::default();
        }
        self.shown = Some(id);
        self.last_shown = Some(id);
        if let Some(agent) = self.agents.iter_mut().find(|agent| agent.id == id)
            && agent.unseen
        {
            agent.unseen = false;
            self.relayout();
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
        // The mark goes up to the closed ancestors on the tree right away (NS-38).
        self.relayout();
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
                self.screen = Screen::Tree;
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

    fn key_new_ticket(
        &mut self,
        key: KeyEvent,
        project_id: i64,
        item: NewItem,
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
                // The project where `a` opened the dialog, not the selection now (a refresh by
                // the end of an agent may have moved it). One gone from the database fails in
                // `create`, which says why, with the dialog open.
                let created = match item {
                    NewItem::Ticket { epic_id } => tickets::create_in(
                        &self.db, project_id, epic_id, &title, &jira_id,
                    )
                    .map(|ticket| {
                        (
                            tickets::created_message(&ticket),
                            NodeId::Ticket(ticket.id),
                            epic_id,
                        )
                    }),
                    NewItem::Epic => epics::create(&self.db, project_id, &title, &jira_id)
                        .map(|epic| (epics::created_message(&epic), NodeId::Epic(epic.id), None)),
                };
                match created {
                    Ok((message, node, epic_id)) => {
                        self.status = Some(message);
                        // Select the new node, its branch open: the refresh after the dialog
                        // closes reads it.
                        self.tree.open(NodeId::Module(self.module()));
                        self.tree.open(NodeId::Project(project_id));
                        if let Some(epic) = epic_id {
                            self.tree.open(NodeId::Epic(epic));
                        }
                        self.selected = node;
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
            project_id,
            item,
            title,
            jira_id,
            focus,
        });
        Action::None
    }

    fn key_edit_title(&mut self, key: KeyEvent, ticket: Ticket, input: String) -> Action {
        self.key_text_field(
            key,
            ticket,
            input,
            |app, ticket, input| {
                app.status = Some(tickets::rename(&app.db, ticket, input)?);
                Ok(None)
            },
            |ticket, input| Dialog::EditTitle { ticket, input },
        )
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

    /// `g` on the node of a ticket or of an epic (NS-38, NS-40): the check of the launcher
    /// (`Action::CheckAgent`), then the menu, only for a visible ticket or epic (and not a ticket
    /// of a hidden epic) of a project available now (read from the disk at the key, like the
    /// Enter on a project). An epic is read again first: another nick may have hidden it.
    fn choose_stage(&mut self, holder: Option<Holder>) -> Action {
        let Some(mut holder) = holder else {
            return Action::None;
        };
        if let Owner::Epic(id) = holder.owner {
            match self.db.get_epic(id) {
                Ok(Some(epic)) => holder = (&epic).into(),
                Ok(None) => {
                    if let Some(epic) = self.epic(id) {
                        self.status = Some(epics::gone_message(epic));
                    }
                    return Action::None;
                }
                Err(err) => {
                    self.status = Some(projects::db_error_message(&err));
                    return Action::None;
                }
            }
        }
        if let Some(refusal) = self.hidden_refusal(&holder) {
            self.status = Some(refusal);
            return Action::None;
        }
        let Some(project) = self.project_of_holder(&holder) else {
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
            holder,
        }
    }

    /// The check before the menu (plan D7 of NS-35): the launcher must be a file, and its
    /// `--check` must pass (a failure becomes the alert, with `Nada foi aberto.` on top, and
    /// nothing is stored nor launched). Then the menu of the types the flags of the project offer
    /// (`StageType::menu`; of an epic, only PO and Tech Leader: `StageType::epic_menu`, NS-40).
    pub fn check_agent(
        &mut self,
        project: &Path,
        holder: &Holder,
        launcher: &mut dyn AgentLauncher,
    ) {
        if !self.launcher_found() {
            return;
        }
        match launcher.check(&self.launcher, project) {
            Ok(precheck) => {
                let types = match holder.owner {
                    Owner::Ticket(_) => StageType::menu(precheck.jira, precheck.sentry),
                    Owner::Epic(_) => StageType::epic_menu(precheck.jira, precheck.sentry),
                };
                self.dialog = Some(Dialog::ChooseType {
                    project: project.to_path_buf(),
                    holder: holder.clone(),
                    types,
                    precheck,
                    index: 0,
                });
            }
            Err(message) => self.agent_failed(&message),
        }
    }

    /// Whether the launcher is a file; when not, the alert with its path (the check of `g` and of
    /// the resume, NS-39).
    fn launcher_found(&mut self) -> bool {
        if self.launcher.is_file() {
            return true;
        }
        self.dialog = Some(Dialog::AgentError {
            message: format!(
                "Não encontrei o bin/nick-agent em {}. Nada foi aberto.",
                self.launcher.display()
            ),
        });
        false
    }

    /// `Action::CheckResume` (NS-39): the `--check` (a failure is the alert "Agente não aberto",
    /// as for `g`), then whether claude still keeps the conversation (`--has-conversation`, out of
    /// the PTY): a missing one opens the warning of the lost conversation and nothing is opened;
    /// a found one is resumed (`resume_agent`), with the jail and the warning of this check, and
    /// the session the conversation went on in (NS-50). The check asks by the effective session
    /// of the agent (`Agent::effective_session`).
    pub fn check_resume(
        &mut self,
        project: &Path,
        agent: &Agent,
        launcher: &mut dyn AgentLauncher,
    ) {
        if !self.launcher_found() {
            return;
        }
        let precheck = match launcher.check(&self.launcher, project) {
            Ok(precheck) => precheck,
            Err(message) => return self.agent_failed(&message),
        };
        match launcher.has_conversation(&self.launcher, project, agent.effective_session()) {
            Err(message) => self.agent_failed(&message),
            Ok(Some(latest)) => self.resume_agent(project, agent, &latest, precheck, launcher),
            Ok(None) => {
                if let Some(holder) = self.holder_of_agent(agent) {
                    self.dialog = Some(Dialog::ConversationLost {
                        project: project.to_path_buf(),
                        agent: agent.clone(),
                        holder,
                        precheck,
                    });
                }
            }
        }
        // This runs from the loop, not from `handle_key`: a resume refused (the row changed
        // after the key) reads the tree again, so the node shows the state the bar talks about
        // (review of the NS-39, point 4). A resumed one was read by `start_running`.
        if self.agent(agent.id).is_none() && self.dialog.is_none() {
            self.refresh_tree();
        }
    }

    /// The ticket or the epic of `agent` (NS-40: by its owner), read from the database now; when
    /// it is gone (or the database fails), the status bar says why and there is none.
    fn holder_of_agent(&mut self, agent: &Agent) -> Option<Holder> {
        let read = match agent.owner {
            Owner::Ticket(id) => self.db.get_ticket(id).map(|t| t.as_ref().map(Holder::from)),
            Owner::Epic(id) => self.db.get_epic(id).map(|e| e.as_ref().map(Holder::from)),
        };
        match read {
            Ok(Some(holder)) => Some(holder),
            Ok(None) => {
                self.status = Some(format!("O agente {} não existe mais no banco.", agent.name));
                None
            }
            Err(err) => {
                self.status = Some(projects::db_error_message(&err));
                None
            }
        }
    }

    /// Resumes `agent` in its own conversation (NS-39), with the cwd at `project` and the jail
    /// and the warning of `check` (not run again: the launch repeats the checks in bash, with the
    /// config of now). Its lock is taken and its row goes back to `active` (`agents::resume`),
    /// then the PTY starts with `Launch::Resume`: its stage, the ticket key it was opened with
    /// (`agents::opened_key`) and the key of its ticket now, so the launcher tells it when the
    /// folder of the ticket changed. Its name is the one stored. A failing start stops it again
    /// and releases the lock, as `open_agent`; a started one runs as an opened one
    /// (`start_running`). NS-50: `latest` is the session the conversation went on in; the resume
    /// stores it (the lock stays the one of the session of the opening) and the PTY resumes the
    /// effective session of the row read again. The bar says when it changed, or when another
    /// agent has it already.
    pub fn resume_agent(
        &mut self,
        project: &Path,
        agent: &Agent,
        latest: &str,
        check: Precheck,
        launcher: &mut dyn AgentLauncher,
    ) {
        let Some(stage) = agents::Stage::from_key(&agent.stage) else {
            self.status = Some(format!(
                "O agente {} tem uma etapa que este nick não conhece ({}).",
                agent.name,
                printable(&agent.stage)
            ));
            return;
        };
        let Some(holder) = self.holder_of_agent(agent) else {
            return;
        };
        let Some(agents_dir) = self.agents_dir.as_deref() else {
            self.status = Some("Não consegui abrir o agente: sem a pasta das travas.".into());
            return;
        };
        let (record, swap) = match agents::resume(&self.db, agents_dir, agent, latest) {
            Ok(resumed) => resumed,
            Err(message) => {
                self.status = Some(message);
                return;
            }
        };
        let opened = agents::opened_key(&record.agent);
        let launch = Launch::Resume {
            stage,
            opened_key: opened.as_deref(),
            holder_key: match holder.owner {
                Owner::Ticket(_) => HolderKey::Ticket(&holder.key),
                Owner::Epic(_) => HolderKey::Epic(&holder.key),
            },
        };
        let id = record.agent.id;
        let name = record.agent.name.clone();
        let session_id = record.agent.effective_session().to_string();
        if let Err(err) = launcher.spawn(&self.launcher, project, id, &session_id, &name, launch) {
            let mut message = format!("Não consegui abrir o agente: {err}.");
            if let Err(db) = agents::end(&self.db, record) {
                message = format!("{message} {db}");
            }
            self.status = Some(message);
            return;
        }
        self.start_running(record, name.clone(), &holder, check);
        let mut message = format!("O agente {name} foi retomado.");
        match swap {
            SessionSwap::Same => {}
            SessionSwap::Swapped => message.push_str(&format!(
                " A conversa tinha continuado em outra sessão do claude ({}), e o nick passou a \
                 usá-la.",
                printable(latest)
            )),
            SessionSwap::Taken => message.push_str(&format!(
                " A conversa continuou na sessão {} do claude, mas outro agente deste nick já usa \
                 essa sessão: a retomada seguiu pela sessão guardada.",
                printable(latest)
            )),
        }
        if opened.as_deref().is_some_and(|key| key != holder.key) {
            message.push_str(&format!(
                " Ele foi avisado de que a pasta agora é {}.",
                docs::dir_text(&holder.key)
            ));
        }
        self.status = Some(message);
    }

    /// The keys of the warning of a lost conversation (NS-39): `a` archives (no second
    /// question), `n` opens the menu "Abrir agente" of the types of the stage of the agent that
    /// the flags offer, with the precheck of the resume (no second `--check`; a hidden ticket or
    /// epic is refused as by `g`), Esc closes writing nothing; any other key keeps it.
    fn key_conversation_lost(
        &mut self,
        key: KeyEvent,
        project: PathBuf,
        agent: Agent,
        holder: Holder,
        precheck: Precheck,
    ) -> Action {
        match key.code {
            KeyCode::Esc => {}
            KeyCode::Char('a') => {
                self.status = Some(match agents::archive(&self.db, &agent) {
                    Ok(message) | Err(message) => message,
                });
            }
            KeyCode::Char('n') if let Some(refusal) = self.hidden_refusal(&holder) => {
                self.status = Some(refusal);
            }
            KeyCode::Char('n') => {
                let types: Vec<StageType> = StageType::menu(precheck.jira, precheck.sentry)
                    .into_iter()
                    .filter(|kind| kind.stage().key() == agent.stage)
                    .collect();
                if types.is_empty() {
                    self.status = Some(format!(
                        "Nenhum tipo de agente da etapa {} está disponível neste projeto.",
                        printable(&tickets::stage_label(&agent.stage))
                    ));
                } else {
                    self.status = None;
                    self.dialog = Some(Dialog::ChooseType {
                        project,
                        holder,
                        precheck,
                        types,
                        index: 0,
                    });
                }
            }
            _ => {
                self.dialog = Some(Dialog::ConversationLost {
                    project,
                    agent,
                    holder,
                    precheck,
                });
            }
        }
        Action::None
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
        holder: Holder,
        precheck: Precheck,
        types: Vec<StageType>,
        mut index: usize,
    ) -> Action {
        match key.code {
            KeyCode::Esc => return Action::None,
            KeyCode::Enter => {
                if let Some(kind) = types.get(index).copied() {
                    let values = kind.initial_values(&holder.key);
                    self.dialog = Some(Dialog::StageForm {
                        plan_missing: plan_missing(&project, kind, &values),
                        values,
                        project,
                        holder,
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
            holder,
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
        holder: Holder,
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
            plan_missing: plan_missing(&project, kind, &values),
            project,
            holder,
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
            holder,
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
                    holder,
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
        holder: Holder,
        precheck: Precheck,
        kind: StageType,
        rendered: Rendered,
        mut scroll: usize,
    ) -> Action {
        if is_yes(key) {
            return Action::OpenAgent {
                project,
                holder,
                kind,
                log: rendered.log,
                precheck,
            };
        }
        if matches!(key.code, KeyCode::Esc | KeyCode::Char('n')) {
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
            holder,
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
    /// failing start ends the row and releases the lock. The agent joins the running ones, its
    /// node is selected (its branches open), and it is shown with the focus on its terminal. An
    /// agent of a ticket, once started, advances the stage of the ticket to its own when it is a
    /// later one (`tickets::advance_on_agent_opening`, NS-45), with the message on the bar.
    pub fn open_agent(
        &mut self,
        project: &Path,
        holder: &Holder,
        kind: StageType,
        log: &Path,
        check: Precheck,
        launcher: &mut dyn AgentLauncher,
    ) {
        let stage = kind.stage();
        let project_name = self
            .project_of_holder(holder)
            .map_or("", |p| p.name.as_str());
        let name = agents::agent_name(project_name, stage, &holder.key);
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
        let record = match agents::open(
            &self.db,
            agents_dir,
            holder.owner,
            stage,
            &session_id,
            &name,
            &holder.key,
        ) {
            Ok(record) => record,
            Err(message) => {
                self.status = Some(message);
                return;
            }
        };
        let id = record.agent.id;
        let launch = Launch::New {
            kind,
            prompt_file: log,
        };
        if let Err(err) = launcher.spawn(&self.launcher, project, id, &session_id, &name, launch) {
            let mut message = format!("Não consegui abrir o agente: {err}.");
            if let Err(db) = agents::end(&self.db, record) {
                message = format!("{message} {db}");
            }
            self.status = Some(message);
            return;
        }
        // A new agent of a later stage moves its ticket to it (NS-45, plan D4), before the
        // `start_running`, which reads the tree again and clears the bar. A failing advance ends
        // no agent: it already runs, and only the bar says so.
        let advance = match holder.owner {
            Owner::Ticket(ticket_id) => {
                tickets::advance_on_agent_opening(&self.db, ticket_id, stage)
            }
            Owner::Epic(_) => Ok(None),
        };
        self.start_running(record, name, holder, check);
        self.status = match advance {
            Ok(message) => message,
            Err(message) => Some(message),
        };
    }

    /// An agent whose PTY started (opened, or resumed: NS-39) joins the running ones, with the
    /// jail and the warning of its `check`; its node is selected (its branches open: reading
    /// only; the stage changes only in `open_agent`, NS-45), and it is shown with the focus on its
    /// terminal.
    fn start_running(&mut self, record: OpenAgent, name: String, holder: &Holder, check: Precheck) {
        let id = record.agent.id;
        let project = self
            .project_of_holder(holder)
            .map_or(String::new(), |p| p.name.clone());
        self.agents.push(RunningAgent {
            record,
            id,
            name,
            project,
            owner: holder.owner,
            project_id: holder.project_id,
            jail: check.jail,
            warning: check.warning,
            seen_text: String::new(),
            unseen: false,
        });
        self.status = None;
        self.reveal_and_select(NodeId::Agent(id));
        self.refresh_tree();
        self.show(id);
        self.focus = Focus::Terminal;
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
        }
        // The selection stays on its node, now an ended agent (decision 10 of NS-38); the
        // shortcut goes to the live agent opened most recently.
        if self.last_shown == Some(id) {
            self.last_shown = self.agents.last().map(|agent| agent.id);
        }
        if self.agents.is_empty() {
            self.focus = Focus::List;
        }
        Some(agent)
    }

    /// The program of the agent `id` ended with `code`: the row becomes `stopped` and the lock
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
        self.refresh_tree();
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
        self.refresh_tree();
    }

    /// Ends the records of every running agent (the nick is leaving, its sessions already
    /// terminated): each row becomes `stopped` and each lock goes, one by one, even after an
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

    /// A key with the focus on the terminal: the Ctrl+Z is dropped, with a status
    /// (`keys::is_suspend_key`, NS-44); Shift+PageUp/PageDown scroll the history; everything
    /// else is encoded as an xterm would and sent to the program.
    fn key_agent(&mut self, key: KeyEvent) -> Action {
        if keys::is_suspend_key(&key) {
            self.status = Some(CTRL_Z_IGNORED.into());
            return Action::None;
        }
        if key.modifiers.contains(KeyModifiers::SHIFT) {
            match key.code {
                KeyCode::PageUp => return Action::ScrollPageUp,
                KeyCode::PageDown => return Action::ScrollPageDown,
                _ => {}
            }
        }
        keys::encode_key(&key, self.agent_term.modes).map_or(Action::None, Action::Send)
    }

    /// Whether the input goes to the shown agent's program: the focus is on its terminal. The
    /// one rule of the focus: the help and the borders of the panels (NS-37) follow it too.
    pub fn on_terminal(&self) -> bool {
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

    fn key_choose_ticket_stage(
        &mut self,
        key: KeyEvent,
        ticket: Ticket,
        mut index: usize,
    ) -> Action {
        match key.code {
            KeyCode::Esc => {
                self.status = None;
                return Action::None;
            }
            KeyCode::Up | KeyCode::Char('k') => index = index.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                index = (index + 1).min(TicketStage::ALL.len() - 1);
            }
            KeyCode::Enter => {
                let to = TicketStage::ALL[index];
                if to.key() == ticket.stage {
                    self.status = Some(tickets::same_stage_message(&ticket, to));
                } else {
                    self.status = None;
                    self.dialog = Some(Dialog::ConfirmStageChange { ticket, to });
                    return Action::None;
                }
            }
            _ => {}
        }
        self.dialog = Some(Dialog::ChooseTicketStage { ticket, index });
        Action::None
    }

    /// The field of the new ID: Enter runs the checks 1 to 5 of plan D5 of NS-36, renaming
    /// nothing (a refusal stays on the bar with the field open, as in `EditTitle`); then the
    /// confirmation, which says whether the folder will be renamed.
    fn key_edit_id(&mut self, key: KeyEvent, ticket: Ticket, input: String) -> Action {
        self.key_text_field(
            key,
            ticket,
            input,
            |app, ticket, input| {
                let new = tickets::check_new_id(&app.db, ticket, input)?;
                app.status = None;
                Ok(Some(Dialog::ConfirmEditId {
                    moves: app.has_docs(ticket.project_id, &ticket.key),
                    ticket: ticket.clone(),
                    new,
                }))
            },
            |ticket, input| Dialog::EditId { ticket, input },
        )
    }

    /// Only s/S apply the new ID (`tickets::edit_id` runs every check again: the state may have
    /// changed); any other key cancels, renaming nothing.
    fn key_confirm_edit_id(&mut self, key: KeyEvent, ticket: Ticket, new: String) -> Action {
        self.confirm_edit_id(key, &ticket, ticket.project_id, &new, tickets::edit_id)
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

    /// `r` on a project: the confirmation with what goes with it (its tickets, its agent
    /// records and its agents live in this nick), or the refusal when an agent of it is active
    /// in another nick, which this one can't end (the safe side, as "Editar ID" of NS-36).
    fn ask_removal(&mut self, project: Project) {
        let counted = self.db.count_tickets(project.id).and_then(|tickets| {
            Ok((
                tickets,
                self.db.list_project_agents(project.id)?,
                self.db.count_epics(project.id)?,
            ))
        });
        let (tickets, agents, epics) = match counted {
            Ok(counted) => counted,
            Err(err) => {
                self.status = Some(projects::db_error_message(&err));
                return;
            }
        };
        if let Some(refusal) = self.removal_refusal(&project, &agents) {
            self.status = Some(refusal);
            return;
        }
        let live = agents.iter().filter(|a| self.agent(a.id).is_some()).count();
        self.dialog = Some(Dialog::ConfirmRemove {
            project,
            tickets,
            agents: i64::try_from(agents.len()).unwrap_or(i64::MAX),
            live: i64::try_from(live).unwrap_or(i64::MAX),
            epics,
        });
    }

    /// The refusal of removing `project` when one of its `agents` is active in another nick.
    fn removal_refusal(&self, project: &Project, agents: &[Agent]) -> Option<String> {
        agents
            .iter()
            .any(|a| a.state == agents::AgentState::Active.key() && self.agent(a.id).is_none())
            .then(|| {
                format!(
                    "O projeto {} tem agente ativo em outro nick. Encerre os agentes dele lá antes \
                     de remover o projeto.",
                    project.name
                )
            })
    }

    fn key_confirm_remove(&mut self, key: KeyEvent, project: Project) -> Action {
        // Only s/S confirm (the "sim" of the auto_scrum confirmations); any other key cancels.
        if !is_yes(key) {
            return Action::None;
        }
        // Checked again: another nick may have opened an agent of it meanwhile.
        match self.db.list_project_agents(project.id) {
            Ok(agents) => {
                if let Some(refusal) = self.removal_refusal(&project, &agents) {
                    self.status = Some(refusal);
                    return Action::None;
                }
            }
            Err(err) => {
                self.status = Some(projects::db_error_message(&err));
                return Action::None;
            }
        }
        let live: Vec<i64> = self
            .agents
            .iter()
            .filter(|agent| agent.project_id == project.id)
            .map(|agent| agent.id)
            .collect();
        if !live.is_empty() {
            return Action::RemoveProject {
                project,
                agents: live,
            };
        }
        self.status = Some(match projects::remove(&self.db, &project) {
            Ok(message) | Err(message) => message,
        });
        Action::None
    }

    /// `Action::RemoveProject` (the sessions of `ended` already terminated by the loop): their
    /// records end and their locks go, as when the operator ends one (`x`), then the project is
    /// removed (its tickets and agents leave the database with it, by the cascade).
    pub fn remove_project_ending_agents(&mut self, project: &Project, ended: &[i64]) {
        let mut names = Vec::new();
        let mut error = None;
        for id in ended {
            if let Some(agent) = self.take_agent(*id) {
                names.push(agent.name.clone());
                if let Err(err) = agents::end(&self.db, agent.record) {
                    error.get_or_insert(err);
                }
            }
        }
        let mut message = match projects::remove(&self.db, project) {
            Ok(message) | Err(message) => message,
        };
        match names.as_slice() {
            [] => {}
            [name] => message = format!("{message} O agente {name} foi encerrado."),
            _ => {
                message = format!(
                    "{message} Os {} agentes vivos dele foram encerrados.",
                    names.len()
                );
            }
        }
        if let Some(err) = error {
            message = format!("{message} {err}");
        }
        self.status = Some(message);
        self.refresh_tree();
    }

    /// After a registration: the status message and the new project selected.
    fn registered(&mut self, project: &Project) {
        self.status = Some(projects::registered_message(project));
        self.tree.open(NodeId::Module(self.module()));
        self.selected = NodeId::Project(project.id);
        self.refresh_tree();
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

/// Whether the form of `kind` warns that the plan is missing (plan D7 of NS-36): only the plan
/// field of Dev and Review, empty, with a valid ID typed, and `.nick/docs/<ID>/plan.md` not
/// there now (one `stat`). A path typed in the plan field is not checked (the script does not
/// check it either); an empty or invalid ID is the validation's job.
fn plan_missing(project: &Path, kind: StageType, values: &[String]) -> bool {
    let fields = kind.fields();
    let value = |rule: Rule| {
        fields
            .iter()
            .position(|field| field.rule == rule)
            .and_then(|i| values.get(i))
            .map(|v| stages::trim_read(v))
    };
    let (Some(plan), Some(id)) = (value(Rule::Plan), value(Rule::JiraId)) else {
        return false;
    };
    plan.is_empty() && tickets::jira_id_valid(id) && !docs::artifacts(project, id).plan
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

/// The refusal of acting on an agent in a state this nick does not know (the column has no
/// CHECK; review of the NS-39, point 6): nothing is done on it.
fn unknown_state_message(agent: &Agent) -> String {
    format!(
        "O agente {} está num estado que este nick não conhece ({}).",
        agent.name,
        printable(&agent.state)
    )
}

/// `s`: the "sim" of the `(s/N)` confirmations (of auto_scrum too); any other key is no. The key
/// comes through `keys::shortcut_key`, so `S` (Caps Lock or Shift) is `s` (NS-43).
fn is_yes(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('s')
}

/// The answer of an `(s/N)` confirmation: only s/S give `action`; any other key cancels.
fn confirmed(key: KeyEvent, action: Action) -> Action {
    if is_yes(key) { action } else { Action::None }
}

/// `q` without modifiers: the "Sair (q)" rule of the lists. The key comes through
/// `keys::shortcut_key`, so `Q` (Caps Lock or Shift) is `q`, and Ctrl+Q and Alt+Q don't quit.
fn is_quit(key: KeyEvent) -> bool {
    key.code == KeyCode::Char('q') && key.modifiers.is_empty()
}

#[cfg(test)]
pub(crate) mod tests {
    /// The ticket of an agent owned by a ticket (the tests of before the NS-40 have only those).
    fn owner_ticket(owner: Owner) -> i64 {
        match owner {
            Owner::Ticket(id) => id,
            Owner::Epic(id) => panic!("the agent is of the epic {id}"),
        }
    }

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

        /// Selects the module on the tree and types `path` in the add field, then Enter.
        fn add(&mut self, path: &str) -> Action {
            self.app
                .select(crate::tree::NodeId::Module(ModuleKind::AutoScrum));
            self.app.relayout();
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
        assert_eq!(
            f.app.selected,
            crate::tree::NodeId::Project(f.pid("cwd/alfa")),
            "the new one is selected"
        );
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
        assert_eq!(f.app.screen, Screen::Tree);
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
        let node = |f: &Fixture, rel| crate::tree::NodeId::Project(f.pid(rel));
        assert_eq!(f.app.selected, node(&f, "b"));
        f.ch('r');
        f.ch('s');
        assert_eq!(f.app.selected, node(&f, "a"));
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

    /// Registers `rel` (with config) and opens its node on the tree (selected).
    fn with_tickets(rel: &str) -> (Fixture, PathBuf) {
        let mut f = fixture();
        let p = f.dir(rel, true);
        f.add(p.to_str().unwrap());
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        let id = f.app.selected_project().expect("the project selected").id;
        assert!(
            f.app.tree.is_open(crate::tree::NodeId::Project(id)),
            "{:?}",
            f.status()
        );
        (f, p)
    }

    impl Fixture {
        /// The tickets of the project of the selected node on the tree, in order.
        fn visible_tickets(&self) -> Vec<&crate::tickets::Ticket> {
            let project = self.app.selected_project().map(|p| p.id);
            self.app
                .visible()
                .iter()
                .filter_map(|row| match row.id {
                    crate::tree::NodeId::Ticket(id) => self.app.ticket(id),
                    _ => None,
                })
                .filter(|t| Some(t.project_id) == project)
                .collect()
        }

        /// The keys of the tickets of the project of the selected node, as the tree lists them.
        fn keys(&self) -> Vec<String> {
            self.visible_tickets()
                .iter()
                .map(|t| t.key.clone())
                .collect()
        }

        /// The key of the selected ticket.
        fn selected_key(&self) -> Option<String> {
            match self.app.selected {
                crate::tree::NodeId::Ticket(id) => self.app.ticket(id).map(|t| t.key.clone()),
                _ => None,
            }
        }

        /// Selects the project of the selected node (where `a` creates a ticket).
        fn on_project(&mut self) {
            if let Some(id) = self.app.selected_project().map(|p| p.id) {
                self.app.select(crate::tree::NodeId::Project(id));
            }
        }

        /// From an agent node, selects its ticket (where `g`, `m`, `i` act).
        fn on_ticket(&mut self) {
            if let crate::tree::NodeId::Agent(id) = self.app.selected {
                let owner = match self.app.agent(id) {
                    Some(agent) => agent.owner,
                    None => self.app.agent_record(id).expect("read").owner,
                };
                self.app.select(owner.into());
            }
        }

        /// Selects the ticket `key` of the project of the selected node.
        fn select_key(&mut self, key: &str) {
            let id = self
                .visible_tickets()
                .iter()
                .find(|t| t.key == key)
                .unwrap_or_else(|| panic!("{key} not listed"))
                .id;
            self.app.select(crate::tree::NodeId::Ticket(id));
        }

        fn all_tickets(&self) -> Vec<crate::tickets::Ticket> {
            let id = self.app.selected_project().expect("a project selected").id;
            self.app.db.list_tickets(id, true).unwrap()
        }

        /// Creates a ticket through the dialog, on the node of the project of the selected
        /// node: `a`, the title, Tab, the ID, Enter.
        fn new_ticket(&mut self, title: &str, jira: &str) -> Action {
            self.on_project();
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
            let id = f.pid("proj");
            assert!(f.app.tree.is_open(crate::tree::NodeId::Project(id)));
            assert_eq!(f.app.selected_project().unwrap().path, p);
            assert_eq!(f.keys(), Vec::<String>::new());
            assert!(!f.app.tree.show_hidden);
            assert_eq!(f.status(), None);
        }
    }

    #[test]
    fn enter_on_an_empty_projects_list_does_nothing() {
        let mut f = fixture();
        f.key(KeyCode::Enter);
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.app.screen, Screen::Tree);
        assert_eq!(f.rows(), [MODULE], "a module without projects is a leaf");
    }

    #[test]
    fn esc_and_left_go_back_to_the_projects_with_it_selected() {
        for back in [KeyCode::Esc, KeyCode::Left] {
            let mut f = fixture();
            let a = f.dir("a", true);
            let b = f.dir("b", true);
            f.add(a.to_str().unwrap());
            f.add(b.to_str().unwrap());
            let node = crate::tree::NodeId::Project(f.pid("b"));
            assert_eq!(f.app.selected, node);
            f.key(KeyCode::Enter);
            f.ch('f');
            assert!(f.app.tree.show_hidden);
            assert_eq!(f.key(back), Action::None);
            assert_eq!(f.app.selected, node, "b is still selected");
            assert!(!f.app.tree.is_open(node), "and closed");
            // Decision 8 of NS-38: the filters are global, they stay on.
            assert!(f.app.tree.show_hidden, "the filter stays on");
        }
    }

    #[test]
    fn a_opens_the_new_ticket_dialog_on_the_title() {
        let (mut f, _) = with_tickets("proj");
        assert_eq!(f.ch('a'), Action::None);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::NewTicket {
                project_id: f.pid("proj"),
                item: NewItem::Ticket { epic_id: None },
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
                project_id: f.pid("proj"),
                item: NewItem::Ticket { epic_id: None },
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
        f.on_project();
        f.ch('a');
        f.type_text("Segundo");
        f.key(KeyCode::Enter);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.keys().len(), 2);
        let second = f.visible_tickets()[1].clone();
        assert_eq!(
            f.selected_key().as_deref(),
            Some(second.key.as_str()),
            "the new one is selected"
        );
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
                    project_id: f.pid("proj"),
                    item: NewItem::Ticket { epic_id: None },
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
        let ticket = f.visible_tickets()[0].clone();
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
        assert_eq!(f.visible_tickets()[0].title, "Novo q");
        assert_eq!(f.visible_tickets()[0].key, "NS-1");
        // Esc cancels without saving.
        f.ch('e');
        f.type_text("zzz");
        f.key(KeyCode::Esc);
        assert_eq!(f.visible_tickets()[0].title, "Novo q");
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
            let ticket = f.visible_tickets()[0].clone();
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
        f.select_key("NS-1");
        f.ch('o');
        f.ch('s');
        assert_eq!(f.keys(), ["NS-2"]);
        assert_eq!(f.ch('f'), Action::None);
        assert!(f.app.tree.show_hidden);
        assert_eq!(f.keys(), ["NS-1", "NS-2"]);
        f.select_key("NS-1");
        assert_eq!(f.ch('o'), Action::None);
        assert_eq!(f.app.dialog, None, "no confirmation to restore");
        assert_eq!(f.status(), Some("Ticket restaurado: NS-1."));
        assert!(f.all_tickets().iter().all(|t| t.hidden_at.is_none()));
        assert_eq!(f.ch('f'), Action::None);
        assert!(!f.app.tree.show_hidden);
        assert_eq!(f.keys(), ["NS-1", "NS-2"]);
    }

    #[test]
    fn up_down_move_within_the_tickets() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("a", "A");
        f.new_ticket("b", "B");
        let key = |f: &Fixture| f.selected_key();
        assert_eq!(key(&f).as_deref(), Some("B"));
        f.key(KeyCode::Down);
        assert_eq!(key(&f).as_deref(), Some("B"), "stops at the last row");
        f.key(KeyCode::Up);
        assert_eq!(key(&f).as_deref(), Some("A"));
        // On the tree, up from the first ticket is its project (NS-38).
        f.ch('k');
        assert_eq!(f.app.selected, crate::tree::NodeId::Project(f.pid("proj")));
        f.ch('j');
        f.ch('j');
        assert_eq!(key(&f).as_deref(), Some("B"));
    }

    #[test]
    fn hiding_the_last_ticket_keeps_the_selection_in_range() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("a", "A");
        f.new_ticket("b", "B");
        f.ch('o');
        f.ch('s');
        assert_eq!(f.selected_key().as_deref(), Some("A"));
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
                project_id: f.pid("proj"),
                item: NewItem::Ticket { epic_id: None },
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
        f.app.select(crate::tree::NodeId::Project(f.pid("a")));
        f.key(KeyCode::Enter);
        f.new_ticket("de a", "NS-1");
        f.app.select(crate::tree::NodeId::Project(f.pid("b")));
        f.key(KeyCode::Enter);
        assert_eq!(f.app.selected_project().unwrap().path, b);
        assert_eq!(f.keys(), Vec::<String>::new());
        f.new_ticket("de b", "NS-1");
        assert_eq!(
            f.status(),
            Some("Ticket criado: NS-1 — de b."),
            "same ID is fine"
        );
        assert_eq!(f.visible_tickets()[0].title, "de b");
    }

    #[test]
    fn the_removal_confirmation_counts_the_tickets_and_removes_them() {
        let (mut f, _) = with_tickets("proj");
        f.new_ticket("a", "NS-1");
        f.new_ticket("b", "NS-2");
        f.ch('o');
        f.ch('s');
        let id = f.app.selected_project().unwrap().id;
        f.app.select(crate::tree::NodeId::Project(id));
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
        /// The resumes (NS-39): the stage, the key at the opening and the key of now.
        resumes: Vec<(Stage, Option<String>, String)>,
        /// Whether each resume named an epic (`--epic-id`, NS-40) and not a ticket.
        resume_epics: Vec<bool>,
        /// The sessions whose conversation was asked for (NS-39).
        conversations: Vec<String>,
        check: Result<Precheck, String>,
        render: Result<Rendered, String>,
        conversation: Result<bool, String>,
        /// The session the conversation went on in (NS-50), when found; None: the session asked.
        latest: Option<String>,
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
                resumes: Vec::new(),
                resume_epics: Vec::new(),
                conversations: Vec::new(),
                check: Ok(Precheck {
                    jail,
                    jira: false,
                    sentry: false,
                    warning: None,
                }),
                render: Ok(rendered(3)),
                conversation: Ok(true),
                latest: None,
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

        fn has_conversation(
            &mut self,
            _: &Path,
            _: &Path,
            session_id: &str,
        ) -> Result<Option<String>, String> {
            self.conversations.push(session_id.to_string());
            let found = self.conversation.clone()?;
            Ok(found.then(|| {
                self.latest
                    .clone()
                    .unwrap_or_else(|| session_id.to_string())
            }))
        }

        fn spawn(
            &mut self,
            launcher: &Path,
            project: &Path,
            agent_id: i64,
            session_id: &str,
            name: &str,
            launch: Launch,
        ) -> Result<(), String> {
            self.ids.push(agent_id);
            match launch {
                Launch::New { kind, prompt_file } => {
                    self.kinds.push((kind, prompt_file.to_path_buf()));
                }
                Launch::Resume {
                    stage,
                    opened_key,
                    holder_key:
                        holder @ (HolderKey::Ticket(ticket_key) | HolderKey::Epic(ticket_key)),
                } => {
                    self.resume_epics.push(matches!(holder, HolderKey::Epic(_)));
                    self.resumes.push((
                        stage,
                        opened_key.map(str::to_string),
                        ticket_key.to_string(),
                    ));
                }
            }
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
        /// On the tree, from an agent node: `g` on its ticket.
        fn g(&mut self, launcher: &mut FakeLauncher) {
            self.on_ticket();
            match self.ch('g') {
                Action::CheckAgent {
                    project,
                    holder: ticket,
                } => {
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
                holder: ticket,
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
        let ticket = f.visible_tickets()[0].clone();
        assert_eq!(
            f.ch('g'),
            Action::CheckAgent {
                project: p.clone(),
                holder: (&ticket).into()
            }
        );
        assert_eq!(f.app.dialog, None, "no menu before the check");
        let mut launcher = FakeLauncher::new(false);
        f.app.check_agent(&p, &(&ticket).into(), &mut launcher);
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
        assert_eq!(f.app.screen, Screen::Tree);
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
            holder: ticket,
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
            holder: ticket,
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
        assert_eq!(f.visible_tickets()[0].key, "NS-1");
        assert!(f.visible_tickets()[0].hidden_at.is_some());
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
        // The agent is shown with the focus on its terminal, its node selected on the tree
        // (NS-38; before, the list stayed on the ticket).
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.app.screen, Screen::Tree);
        assert_eq!(f.app.dialog, None);
        let agent = f.app.shown_agent().expect("the running agent");
        assert_eq!(f.app.selected, crate::tree::NodeId::Agent(agent.id));
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
        assert_eq!(f.app.screen, Screen::Tree);
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
        assert_eq!(f.app.screen, Screen::Tree);
        assert_eq!(
            f.selected_key().as_deref(),
            Some("NS-1"),
            "still on the ticket"
        );
        assert!(f.app.agents.is_empty());
        assert_eq!(f.app.shown, None);
        assert_eq!(f.app.focus, Focus::List);
        let rows = f.agent_rows();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].3, "stopped");
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

    /// NS-51: the cat of a new App is the one of production (still, no bubble), drawn in the
    /// 256 palette until the loop says the terminal has truecolor.
    #[test]
    fn a_new_app_has_the_cat_still_and_without_truecolor() {
        let mut f = fixture();
        assert_eq!(
            f.app.mascot().frame(),
            crate::mascot::Frame {
                pose: crate::mascot::Pose::Rest,
                meowing: false
            }
        );
        assert!(f.app.mascot_wake().is_some(), "she wakes some day");
        assert!(!f.app.truecolor());
        assert_eq!(*f.app.palette(), crate::mascot::Palette::new(false));
        f.app.set_truecolor(true);
        assert!(f.app.truecolor());
        assert_eq!(*f.app.palette(), crate::mascot::Palette::new(true));
        let t0 = std::time::Instant::now();
        f.app.set_mascot(crate::mascot::Mascot::still(t0));
        assert_eq!(f.app.mascot_wake(), None);
        assert!(!f.app.tick_mascot(t0 + std::time::Duration::from_secs(3600)));
    }

    /// NS-51, criterion 5: the blink and the meow touch nothing but the cat: not the focus, the
    /// selection, the dialog (nor the text typed in it), the status nor the agents.
    #[test]
    fn ticking_the_cat_changes_nothing_but_the_cat() {
        use crate::mascot::Mascot;
        use std::time::{Duration, Instant};
        // Every change of the fast cat in one second, checked against `state`.
        fn tick_a_second(f: &mut Fixture) {
            let t0 = Instant::now();
            f.app.set_mascot(Mascot::fast(t0));
            let mut changed = 0;
            let mut now = t0;
            while now < t0 + Duration::from_secs(1) {
                now = f.app.mascot_wake().expect("the fast cat wakes");
                changed += usize::from(f.app.tick_mascot(now));
            }
            assert!(changed > 10, "the cat changed only {changed} times");
        }
        fn state(f: &Fixture) -> String {
            format!(
                "{:?} {:?} {:?} {:?} {:?} {:?} {:?}",
                f.app.focus(),
                f.app.selected(),
                f.app.dialog,
                f.app.status,
                f.app.shown(),
                f.app.agents.len(),
                f.app.screen,
            )
        }

        // An agent running, the focus on its terminal.
        let (mut f, _) = running();
        f.app.set_status("uma mensagem".into());
        let before = state(&f);
        tick_a_second(&mut f);
        assert_eq!(state(&f), before);
        assert_eq!(f.app.focus(), Focus::Terminal);

        // A text field open with something typed.
        let mut f = fixture();
        f.app
            .select(crate::tree::NodeId::Module(ModuleKind::AutoScrum));
        f.ch('a');
        f.type_text("/um/cami");
        let before = state(&f);
        tick_a_second(&mut f);
        assert_eq!(state(&f), before);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::AddPath {
                input: "/um/cami".into()
            })
        );
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

    /// NS-44: the Ctrl+Z would suspend the `claude` with no shell to resume it, so the
    /// terminal drops it (no 0x1a to the PTY) and the status says so.
    #[test]
    fn on_the_terminal_ctrl_z_is_dropped_and_the_status_says_so() {
        let (mut f, _) = running();
        let shown = f.app.shown;
        let ctrl = KeyModifiers::CONTROL;
        let shift = KeyModifiers::SHIFT;
        let events = [
            KeyEvent::new(KeyCode::Char('z'), ctrl),
            KeyEvent::new(KeyCode::Char('Z'), ctrl),
            KeyEvent::new(KeyCode::Char('Z'), ctrl | shift),
            KeyEvent::new(KeyCode::Char('z'), ctrl | shift),
            KeyEvent::new_with_kind(KeyCode::Char('z'), ctrl, KeyEventKind::Repeat),
        ];
        for event in events {
            f.app.status = None;
            assert_eq!(f.app.handle_key(event), Action::None, "{event:?}");
            assert_eq!(
                f.status(),
                Some("O Ctrl+Z não tem efeito dentro do nick."),
                "{event:?}"
            );
            assert_eq!(f.app.focus, Focus::Terminal);
            assert_eq!(f.app.dialog, None);
            assert_eq!(f.app.shown, shown);
        }
    }

    #[test]
    fn on_the_terminal_the_other_ctrl_keys_and_alt_ctrl_z_still_go_to_the_program() {
        let (mut f, _) = running();
        let ctrl = KeyModifiers::CONTROL;
        f.app.status = None;
        for c in 'a'..='y' {
            assert_eq!(
                f.mods(KeyCode::Char(c), ctrl),
                Action::Send(vec![(c as u8) & 0x1f]),
                "Ctrl+{c}"
            );
        }
        f.app.shortcut = Shortcut::from_db("ctrl+]").unwrap();
        assert_eq!(
            f.mods(KeyCode::Char('\\'), ctrl),
            Action::Send(b"\x1c".to_vec())
        );
        assert_eq!(
            f.mods(KeyCode::Char('z'), ctrl | KeyModifiers::ALT),
            Action::Send(b"\x1b\x1a".to_vec())
        );
        assert_eq!(f.status(), None);
        assert_eq!(f.app.focus, Focus::Terminal);
    }

    #[test]
    fn on_the_list_ctrl_z_does_nothing_and_shows_no_message() {
        let (mut f, _) = running();
        f.shortcut();
        assert_eq!(f.app.focus, Focus::List);
        let status = f.status().map(str::to_owned);
        let selected = f.app.selected;
        let agents = f.app.agents.len();
        for c in ['z', 'Z'] {
            assert_eq!(
                f.mods(KeyCode::Char(c), KeyModifiers::CONTROL),
                Action::None
            );
            assert_eq!(f.status(), status.as_deref());
            assert_eq!(f.app.focus, Focus::List);
            assert_eq!(f.app.selected, selected);
            assert_eq!(f.app.dialog, None);
            assert_eq!(f.app.agents.len(), agents);
        }
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
            // Decision 10 of NS-38: the selection stays on the node of the agent, now ended.
            assert_eq!(f.app.screen, Screen::Tree);
            assert_eq!(f.app.selected, crate::tree::NodeId::Agent(launcher.ids[0]));
            f.on_ticket();
            assert_eq!(f.selected_key().as_deref(), Some("NS-2"));
            assert_eq!(f.status(), Some(message));
            assert!(f.app.agents.is_empty());
            assert_eq!(f.app.focus, Focus::List);
            assert_eq!(f.agent_rows()[0].3, "stopped");
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
        let beta = f.dir("beta", true);
        f.add(beta.to_str().unwrap());
        f.key(KeyCode::Enter);
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
        assert_eq!(f.app.screen, Screen::Tree);
        assert_eq!(
            f.app.selected,
            crate::tree::NodeId::Agent(launcher.ids[1]),
            "the node of the last one is selected (NS-38)"
        );
        assert_eq!(f.app.agents[1].owner, Owner::Ticket(f.tnode_id("B-1")));
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
        assert_eq!(f.key(KeyCode::Up), Action::None);
        assert_eq!(
            f.selected_key().as_deref(),
            Some("NS-2"),
            "the arrows move on the tree"
        );
        f.on_project();
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
        // The shortcut takes the focus back to the terminal of the last shown agent.
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.app.handle_paste("x"), Action::Send(b"x".to_vec()));
    }

    #[test]
    fn without_an_agent_the_shortcut_does_nothing() {
        let (mut f, _) = with_agents();
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(f.app.screen, Screen::Tree);
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
        // Being shown clears it (selected on the tree, NS-38).
        f.shortcut();
        f.ch('v');
        f.select(crate::tree::NodeId::Agent(first));
        assert_eq!(f.app.shown, Some(first));
        assert!(!f.app.agents[0].unseen);
    }

    // ---- "Agentes ativos", ending and quitting (NS-34) ----

    #[test]
    fn v_works_without_agents_and_is_typed_in_a_text_field() {
        let mut f = fixture();
        assert_eq!(f.ch('v'), Action::None);
        assert!(f.app.tree.only_active, "the filter of the tree (NS-38)");
        assert_eq!(f.rows(), [MODULE]);
        assert!(f.app.agents.is_empty());
        f.ch('v');
        assert!(!f.app.tree.only_active);
        f.ch('a');
        f.ch('v');
        assert_eq!(f.app.dialog, Some(Dialog::AddPath { input: "v".into() }));
        assert!(!f.app.tree.only_active);
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
            f.select(crate::tree::NodeId::Agent(first));
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
        f.app.agent_ended_by_operator(second);
        assert_eq!(f.status(), Some("O agente beta · Dev · B-1 foi encerrado."));
        assert_eq!(f.names_running(), ["proj · PO · NS-1"]);
        // Decision 10 of NS-38: the selection stays on the node; the shortcut goes to the other.
        assert_eq!(f.app.shown, None);
        assert_eq!(f.app.selected, crate::tree::NodeId::Agent(second));
        assert_eq!(f.app.last_shown, Some(first));
        assert_eq!(f.app.focus, Focus::List);
        let rows = f.agent_rows();
        assert_eq!(
            (rows[0].3.as_str(), rows[1].3.as_str()),
            ("active", "stopped")
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
        // With the filter of the active agents too.
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
            ("active", "stopped")
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
        assert_eq!(f.app.screen, Screen::Tree, "back to the tree (NS-38)");
        assert_eq!(f.app.selected, MODULE);
        f.ch('c');
        f.key(KeyCode::Left);
        assert_eq!(f.app.screen, Screen::Tree);
        let (mut f, _) = with_one_project();
        f.ch('c');
        assert_eq!(f.app.screen, Screen::Tree, "not on a project");
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
        f.select(MODULE);
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
        f.select(MODULE);
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

    // ---- the ticket screen, its stage and editing its ID (NS-36) ----

    use crate::docs::Artifacts;
    use crate::tickets::TicketStage;

    impl Fixture {
        fn detail(&self) -> &TicketDetail {
            self.app
                .ticket_detail
                .as_ref()
                .expect("on the ticket screen")
        }

        /// The stored stage of the ticket `key` of the open project.
        fn stage_of(&self, key: &str) -> String {
            self.all_tickets()
                .into_iter()
                .find(|t| t.key == key)
                .unwrap()
                .stage
        }

        fn history_of(&self, key: &str) -> Vec<(String, String)> {
            self.sourced_history_of(key)
                .into_iter()
                .map(|(from, to, _)| (from, to))
                .collect()
        }

        /// From the ticket screen: `m`, ↓/↑ until `to`, Enter (the confirmation).
        fn choose_ticket_stage(&mut self, to: TicketStage) {
            assert_eq!(self.ch('m'), Action::None);
            let Some(Dialog::ChooseTicketStage { index, .. }) = self.app.dialog else {
                panic!("not in the stage dialog: {:?}", self.app.dialog);
            };
            let at = TicketStage::ALL.iter().position(|s| *s == to).unwrap();
            for _ in index..at {
                self.key(KeyCode::Down);
            }
            for _ in at..index {
                self.key(KeyCode::Up);
            }
            assert_eq!(self.key(KeyCode::Enter), Action::None);
        }

        /// Whether the open form shows the warning of the missing plan.
        fn plan_missing(&self) -> bool {
            match &self.app.dialog {
                Some(Dialog::StageForm { plan_missing, .. }) => *plan_missing,
                other => panic!("not in the form: {other:?}"),
            }
        }
    }

    /// Replaces `enter_and_right_open_the_ticket_and_esc_and_left_go_back_with_the_same_selection`
    /// (NS-38: no ticket screen; the selected ticket shows on the right).
    #[test]
    fn selecting_a_ticket_reads_its_detail_and_another_node_drops_it() {
        let (mut f, _) = with_agents();
        assert_eq!(f.detail().ticket.key, "NS-1");
        f.key(KeyCode::Down);
        assert_eq!(f.detail().ticket.key, "NS-2");
        f.key(KeyCode::Left);
        assert_eq!(f.app.ticket_detail, None, "the project selected");
        // A hidden one, with the filter on, too.
        f.select_key("NS-1");
        f.ch('o');
        f.ch('s');
        f.ch('f');
        f.select_key("NS-1");
        f.key(KeyCode::Char('z'));
        assert_eq!(f.detail().ticket.key, "NS-1");
        assert!(f.detail().ticket.hidden_at.is_some());
    }

    #[test]
    fn the_ticket_screen_shows_the_history_the_agents_and_the_artifacts_read_now() {
        let (mut f, p) = with_agents();
        let mut launcher = FakeLauncher::new(true);
        f.open_agent(0, &mut launcher);
        f.shortcut();
        f.open_agent(2, &mut launcher);
        f.shortcut();
        f.app.agent_ended(launcher.ids[1], 0);
        f.on_ticket();
        f.key(KeyCode::Char('z'));
        let detail = f.detail();
        assert_eq!(detail.ticket.key, "NS-1");
        // The openings of PO and of Dev advanced the stage (NS-45).
        assert_eq!(
            detail
                .changes
                .iter()
                .map(|c| (c.to_stage.as_str(), c.source.as_str()))
                .collect::<Vec<_>>(),
            [("po", "agent_opening"), ("development", "agent_opening")]
        );
        // The agents of the ticket are on the tree now (NS-38), every stage and state.
        let id = detail.ticket.id;
        assert_eq!(
            f.app.owner_agents[&Owner::Ticket(id)]
                .iter()
                .map(|a| (a.name.as_str(), a.state.as_str()))
                .collect::<Vec<_>>(),
            [
                ("proj · PO · NS-1", "active"),
                ("proj · Dev · NS-1", "stopped")
            ]
        );
        assert!(f.rows().contains(&NodeId::Agent(launcher.ids[1])));
        assert_eq!(f.detail().artifacts, Artifacts::default());
        // Created with the ticket selected: the next refresh reads it.
        let docs = p.join(".nick/docs/NS-1");
        fs::create_dir_all(&docs).unwrap();
        fs::write(docs.join("plan.md"), "# plano\n").unwrap();
        f.key(KeyCode::Char('z'));
        assert_eq!(
            f.detail().artifacts,
            Artifacts {
                plan: true,
                review: false
            }
        );
        f.choose_ticket_stage(TicketStage::Review);
        f.ch('s');
        assert_eq!(f.detail().changes.len(), 3);
        assert_eq!(f.detail().changes[2].source, "operator");
        assert_eq!(f.detail().ticket.stage, "review");
    }

    #[test]
    fn a_ticket_gone_from_the_database_goes_back_to_the_list() {
        let (mut f, _) = with_agents();
        let id = f.detail().ticket.id;
        f.app
            .db
            .connection()
            .execute("DELETE FROM tickets WHERE id = ?1", [id])
            .unwrap();
        f.key(KeyCode::Char('z'));
        assert_eq!(f.app.screen, Screen::Tree);
        assert_eq!(f.status(), Some("O ticket NS-1 não existe mais no banco."));
        assert_eq!(f.keys(), ["NS-2"]);
        assert_eq!(f.detail().ticket.key, "NS-2", "the next one selected");
    }

    #[test]
    fn enter_on_a_live_agent_shows_it_whatever_the_stage_and_on_an_ended_one_says_so() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(true);
        f.open_agent(0, &mut launcher);
        f.shortcut();
        f.open_agent(1, &mut launcher);
        f.shortcut();
        f.app.agent_ended(launcher.ids[1], 0);
        f.on_ticket();
        // The ticket goes to Dev; its live PO agent still opens.
        f.choose_ticket_stage(TicketStage::Development);
        f.ch('s');
        assert_eq!(f.stage_of("NS-1"), "development");
        f.select(NodeId::Agent(launcher.ids[0]));
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.app.shown, Some(launcher.ids[0]));
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.app.selected, NodeId::Agent(launcher.ids[0]));
        f.shortcut();
        f.key(KeyCode::Down);
        assert_eq!(f.app.selected, NodeId::Agent(launcher.ids[1]));
        assert_eq!(f.key(KeyCode::Right), Action::None);
        assert_eq!(f.app.focus, Focus::List);
        // NS-39 (decision 3): a stopped one says which key resumes it.
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · NS-1 está parado. Pressione r para retomar.")
        );
        // An active one of another nick does not open either.
        f.app
            .db
            .insert_agent(
                Owner::Ticket(f.tnode_id("NS-1")),
                "review",
                "outro",
                "proj · Review · NS-1",
                "NS-1",
            )
            .unwrap();
        // A first key reads it (the refresh comes after the key), the next one reaches it.
        f.key(KeyCode::Char('z'));
        f.key(KeyCode::Down);
        let other = f.app.owner_agents[&Owner::Ticket(f.tnode_id("NS-1"))][2].id;
        assert_eq!(f.app.selected, NodeId::Agent(other));
        f.key(KeyCode::Enter);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(
            f.status(),
            Some("O agente proj · Review · NS-1 não está rodando neste nick.")
        );
    }

    #[test]
    fn m_lists_the_six_stages_and_only_s_changes_the_stage() {
        let (mut f, _) = with_agents();
        f.key(KeyCode::Enter);
        assert_eq!(f.ch('m'), Action::None);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::ChooseTicketStage {
                ticket: f.detail().ticket.clone(),
                index: 0
            })
        );
        // Enter on the current one refuses, with the dialog open.
        f.key(KeyCode::Enter);
        assert_eq!(f.status(), Some("O ticket NS-1 já está em Aberto."));
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::ChooseTicketStage { .. })
        ));
        for _ in 0..9 {
            f.key(KeyCode::Down);
        }
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::ChooseTicketStage { index: 5, .. })
        ));
        f.key(KeyCode::Enter);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::ConfirmStageChange {
                ticket: f.detail().ticket.clone(),
                to: TicketStage::Done
            })
        );
        // Any other key cancels, nothing stored.
        assert_eq!(f.ch('n'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.stage_of("NS-1"), "open");
        f.choose_ticket_stage(TicketStage::Done);
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.stage_of("NS-1"), "open");
        // s stores it, with its history; S too, and backwards.
        f.choose_ticket_stage(TicketStage::Done);
        f.ch('s');
        assert_eq!(
            f.status(),
            Some("Etapa do ticket NS-1: Aberto → Concluído.")
        );
        assert_eq!(f.stage_of("NS-1"), "done");
        f.choose_ticket_stage(TicketStage::Review);
        f.ch('S');
        assert_eq!(f.stage_of("NS-1"), "review");
        assert_eq!(
            f.history_of("NS-1"),
            [
                ("open".to_string(), "done".to_string()),
                ("done".to_string(), "review".to_string())
            ]
        );
        // The dialog starts on the current stage; Esc cancels it.
        f.ch('m');
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::ChooseTicketStage { index: 4, .. })
        ));
        f.key(KeyCode::Esc);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.app.screen, Screen::Tree);
        // Done stays on the list.
        f.choose_ticket_stage(TicketStage::Done);
        f.ch('s');
        f.key(KeyCode::Esc);
        assert_eq!(f.keys(), ["NS-1", "NS-2"]);
    }

    impl Fixture {
        /// The history of the ticket `key` with the source of each change (NS-45).
        fn sourced_history_of(&self, key: &str) -> Vec<(String, String, String)> {
            let id = self
                .all_tickets()
                .into_iter()
                .find(|t| t.key == key)
                .unwrap()
                .id;
            self.app
                .db
                .list_stage_changes(id)
                .unwrap()
                .into_iter()
                .map(|c| (c.from_stage, c.to_stage, c.source))
                .collect()
        }

        /// The stage of the ticket `key` as the tree has it (read again by `refresh_tree`).
        fn tree_stage_of(&self, key: &str) -> String {
            self.app
                .tickets
                .values()
                .flatten()
                .find(|t| t.key == key)
                .unwrap_or_else(|| panic!("no ticket {key}"))
                .stage
                .clone()
        }

        /// From any node of the ticket `key`: the operator moves it to `to` (`m`, `s`).
        fn operator_moves(&mut self, key: &str, to: TicketStage) {
            self.select(self.tnode(key));
            self.choose_ticket_stage(to);
            self.ch('s');
            assert_eq!(self.stage_of(key), to.key());
        }
    }

    fn triple(from: &str, to: &str, source: &str) -> (String, String, String) {
        (from.into(), to.into(), source.into())
    }

    /// Replaces `the_stage_never_changes_alone_and_done_closes_no_agent` (NS-45, plan D4): a new
    /// agent of a later stage moves the ticket to it, with the source `agent_opening`, the message
    /// on the bar and the tree read with the new stage; and moving to Done still ends and hides no
    /// agent.
    #[test]
    fn opening_a_new_agent_of_a_later_stage_advances_the_ticket() {
        let (mut f, _) = with_agents();
        assert_eq!(f.stage_of("NS-1"), "open", "born in Aberto");
        let mut launcher = FakeLauncher::new(true);
        f.open_kind(StageType::PoDiscussion, &mut launcher);
        assert_eq!(f.stage_of("NS-1"), "po");
        assert_eq!(f.tree_stage_of("NS-1"), "po", "the tree has the new stage");
        assert_eq!(
            f.sourced_history_of("NS-1"),
            [triple("open", "po", "agent_opening")]
        );
        assert_eq!(
            f.status(),
            Some("Etapa do ticket NS-1: Aberto → PO (abertura de agente).")
        );
        assert_eq!(f.app.focus, Focus::Terminal, "the agent shown, as always");
        assert_eq!(f.app.shown(), Some(launcher.ids[0]));
        // Skipping a stage: PO → Dev.
        f.shortcut();
        f.open_kind(StageType::Development, &mut launcher);
        assert_eq!(f.stage_of("NS-1"), "development");
        assert_eq!(f.tree_stage_of("NS-1"), "development");
        assert_eq!(
            f.status(),
            Some("Etapa do ticket NS-1: PO → Dev (abertura de agente).")
        );
        assert_eq!(
            f.sourced_history_of("NS-1"),
            [
                triple("open", "po", "agent_opening"),
                triple("po", "development", "agent_opening")
            ]
        );
        assert_eq!(f.stage_of("NS-2"), "open", "only that ticket");
        // Every type of a stage advances by its stage (PO by Jira too).
        f.shortcut();
        f.select(f.tnode("NS-2"));
        if let Ok(check) = launcher.check.as_mut() {
            check.jira = true;
        }
        f.open_kind(StageType::PoJira, &mut launcher);
        assert_eq!(f.stage_of("NS-2"), "po");
        // Done does not end nor hide the live agent of the ticket.
        f.shortcut();
        let before = f.names_running();
        f.operator_moves("NS-1", TicketStage::Done);
        assert_eq!(f.names_running(), before);
        assert_eq!(f.agent_rows()[1].3, "active");
    }

    /// NS-45, plan D4: nothing but a new agent of a later stage changes the stage, one case per
    /// item of the ticket.
    #[test]
    fn the_stage_does_not_change_in_the_other_cases() {
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(true);
        f.operator_moves("NS-1", TicketStage::Development);
        let history = f.sourced_history_of("NS-1");
        assert_eq!(history, [triple("open", "development", "operator")]);
        // A new agent of an earlier stage, then of the same one.
        f.open_kind(StageType::PoDiscussion, &mut launcher);
        assert_eq!(f.status(), None, "no message of the stage");
        f.shortcut();
        f.open_kind(StageType::Development, &mut launcher);
        assert_eq!(f.stage_of("NS-1"), "development");
        // Switching the focus to an existing agent: the shortcut, and Enter on a live one.
        f.shortcut();
        f.shortcut();
        assert_eq!(f.app.focus, Focus::Terminal);
        f.shortcut();
        f.select(NodeId::Agent(launcher.ids[0]));
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.app.shown(), Some(launcher.ids[0]));
        assert_eq!(f.stage_of("NS-1"), "development");
        // The end of an agent, its end by the operator and the way out.
        f.app.agent_ended(launcher.ids[0], 0);
        f.app.agent_ended_by_operator(launcher.ids[1]);
        f.select(f.tnode("NS-1"));
        f.open_kind(StageType::Review, &mut launcher);
        f.operator_moves("NS-1", TicketStage::Done);
        let history = f.sourced_history_of("NS-1");
        assert_eq!(
            history,
            [
                triple("open", "development", "operator"),
                triple("development", "review", "agent_opening"),
                triple("review", "done", "operator")
            ]
        );
        f.app.end_all_records().unwrap();
        assert_eq!(f.sourced_history_of("NS-1"), history);
        // A ticket in Concluído: a new agent of any stage leaves it there.
        f.select(f.tnode("NS-1"));
        f.open_kind(StageType::Review, &mut launcher);
        f.shortcut();
        f.open_kind(StageType::PoDiscussion, &mut launcher);
        assert_eq!(f.stage_of("NS-1"), "done");
        assert_eq!(f.sourced_history_of("NS-1"), history);
        // A failing spawn: the agent did not open, the stage stays.
        f.shortcut();
        f.select(f.tnode("NS-2"));
        launcher.spawn = Err("sem PTY".into());
        f.open_kind(StageType::Review, &mut launcher);
        assert_eq!(f.status(), Some("Não consegui abrir o agente: sem PTY."));
        assert_eq!(f.stage_of("NS-2"), "open");
        // A failing `agents::open` (the folder of the locks can't be made).
        launcher.spawn = Ok(());
        let file = f.root.join("not-a-dir");
        fs::write(&file, "").unwrap();
        f.app.agents_dir = Some(file.join("agents"));
        let spawns = launcher.spawns.len();
        f.open_kind(StageType::Review, &mut launcher);
        assert_eq!(launcher.spawns.len(), spawns, "no spawn");
        assert_eq!(f.stage_of("NS-2"), "open");
        assert_eq!(f.sourced_history_of("NS-2"), []);
    }

    /// NS-45: the resume of a stopped agent (NS-39) does not change the stage.
    #[test]
    fn resuming_an_agent_does_not_change_the_stage() {
        let (mut f, mut launcher, id) = stopped_agent();
        assert_eq!(f.stage_of("NS-2"), "tech_leader", "its opening advanced it");
        f.operator_moves("NS-2", TicketStage::Open);
        let history = f.sourced_history_of("NS-2");
        f.select(Fixture::anode(id));
        assert!(f.r(&mut launcher));
        assert_eq!(f.state_of(id), "active");
        assert_eq!(f.stage_of("NS-2"), "open");
        assert_eq!(f.sourced_history_of("NS-2"), history);
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · NS-2 foi retomado.")
        );
    }

    /// NS-45: an agent of an epic changes the stage of no ticket (the epic has none).
    #[test]
    fn an_agent_of_an_epic_changes_no_stage() {
        let (mut f, _) = with_epic();
        let mut launcher = FakeLauncher::new(true);
        f.open_kind(StageType::TechLeader, &mut launcher);
        assert_eq!(launcher.ids.len(), 1, "it opened");
        assert_eq!(f.status(), None);
        for key in ["NS-1", "NS-2"] {
            let t = f.app.db.get_ticket(f.tnode_id(key)).unwrap().unwrap();
            assert_eq!(t.stage, "open", "{key}");
            assert_eq!(f.app.db.list_stage_changes(t.id).unwrap(), [], "{key}");
        }
    }

    /// NS-45: the manual change (NS-36) goes to Aberto from any stage, by the operator.
    #[test]
    fn the_operator_moves_the_ticket_to_open_from_any_stage() {
        let (mut f, _) = with_agents();
        for stage in TicketStage::ALL.into_iter().skip(1) {
            f.operator_moves("NS-1", stage);
            f.choose_ticket_stage(TicketStage::Open);
            f.ch('s');
            assert_eq!(
                f.status(),
                Some(format!("Etapa do ticket NS-1: {} → Aberto.", stage.label()).as_str())
            );
            assert_eq!(f.stage_of("NS-1"), "open");
        }
        let history = f.sourced_history_of("NS-1");
        assert_eq!(history.len(), 10);
        assert!(history.iter().all(|(_, _, source)| source == "operator"));
    }

    /// Adapted from `g_and_v_work_from_the_ticket_screen` (NS-38: `v` is the filter of the
    /// tree, `the_filter_v_shows_the_branches_of_the_live_agents_and_the_mark_goes_up`).
    #[test]
    fn g_works_from_the_ticket_node_with_its_refusals() {
        let (mut f, p) = with_agents();
        let ticket = f.detail().ticket.clone();
        assert_eq!(
            f.ch('g'),
            Action::CheckAgent {
                project: p.clone(),
                holder: (&ticket).into()
            }
        );
        let mut launcher = FakeLauncher::new(true);
        f.open_kind(StageType::PoDiscussion, &mut launcher);
        assert_eq!(f.app.screen, Screen::Tree);
        assert_eq!(
            f.app.owner_agents[&Owner::Ticket(f.tnode_id("NS-1"))].len(),
            1,
            "the new agent is on the tree"
        );
        f.shortcut();
        // The refusals of the g of the list: a hidden ticket.
        f.select(f.tnode("NS-2"));
        f.ch('o');
        f.ch('s');
        f.ch('f');
        f.select(f.tnode("NS-2"));
        assert_eq!(f.detail().ticket.key, "NS-2");
        assert_eq!(f.ch('g'), Action::None);
        assert_eq!(
            f.status(),
            Some("O ticket NS-2 está oculto. Restaure-o (o) para abrir um agente.")
        );
    }

    #[test]
    fn q_quits_the_ticket_screen_but_not_its_dialogs() {
        let (mut f, _) = with_agents();
        f.key(KeyCode::Enter);
        f.ch('i');
        assert_eq!(f.ch('q'), Action::None, "typed in the ID");
        f.key(KeyCode::Esc);
        f.ch('m');
        assert_eq!(f.ch('q'), Action::None);
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::ChooseTicketStage { .. })
        ));
        f.key(KeyCode::Esc);
        assert_eq!(f.ch('q'), Action::Quit);
    }

    #[test]
    fn i_refusals_stay_on_the_bar_with_the_dialog_open() {
        let (mut f, p) = with_agents();
        f.key(KeyCode::Enter);
        let ticket = f.detail().ticket.clone();
        assert_eq!(f.ch('i'), Action::None);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::EditId {
                ticket: ticket.clone(),
                input: "NS-1".into()
            })
        );
        for (typed, message) in [
            ("", "O ID não mudou."),
            ("\u{8}\u{8}\u{8}\u{8}", "Informe o novo ID do ticket."),
            ("a b", tickets::INVALID_ID_MESSAGE),
            (
                "\u{8}\u{8}\u{8}NS-2",
                "Já existe um ticket com o ID NS-2 neste projeto.",
            ),
        ] {
            for c in typed.chars() {
                if c == '\u{8}' {
                    f.key(KeyCode::Backspace);
                } else {
                    f.ch(c);
                }
            }
            f.key(KeyCode::Enter);
            assert_eq!(f.status(), Some(message), "{typed:?}");
            assert!(matches!(f.app.dialog, Some(Dialog::EditId { .. })));
        }
        f.key(KeyCode::Esc);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), None);
        // An active agent (of this nick or not) is refused at Enter too.
        f.app
            .db
            .insert_agent(
                Owner::Ticket(ticket.id),
                "po",
                "outro",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        f.ch('i');
        f.ch('9');
        f.key(KeyCode::Enter);
        assert_eq!(
            f.status(),
            Some(
                "O ticket NS-1 tem agente ativo. Encerre os agentes dele antes de editar o ID: o \
                 prompt deles cita a pasta .nick/docs/NS-1/."
            )
        );
        f.key(KeyCode::Esc);
        // An unavailable project is refused when the dialog would open.
        fs::remove_file(p.join(".nick/nick.conf")).unwrap();
        f.ch('i');
        assert_eq!(f.app.dialog, None);
        assert_eq!(
            f.status(),
            Some(
                "Projeto indisponível: [indisponível: sem .nick/nick.conf]. O ID só é editado \
                 com a pasta e o .nick/nick.conf no lugar."
            )
        );
        let keys: Vec<String> = f.all_tickets().into_iter().map(|t| t.key).collect();
        assert_eq!(keys, ["NS-1", "NS-2"]);
    }

    #[test]
    fn i_confirms_with_and_without_the_folder_and_only_s_applies() {
        let (mut f, p) = with_agents();
        f.key(KeyCode::Enter);
        let ticket = f.detail().ticket.clone();
        f.ch('i');
        f.ch('0');
        f.key(KeyCode::Enter);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::ConfirmEditId {
                ticket: ticket.clone(),
                new: "NS-10".into(),
                moves: false
            })
        );
        assert_eq!(f.ch('n'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.keys(), ["NS-1", "NS-2"]);
        // With the folder: it says so, and another key cancels without renaming.
        let docs = p.join(".nick/docs/NS-1");
        fs::create_dir_all(&docs).unwrap();
        fs::write(docs.join("plan.md"), "# plano\n").unwrap();
        f.ch('i');
        f.ch('0');
        f.key(KeyCode::Enter);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::ConfirmEditId {
                ticket: ticket.clone(),
                new: "NS-10".into(),
                moves: true
            })
        );
        f.key(KeyCode::Enter);
        assert!(docs.join("plan.md").is_file());
        f.ch('i');
        f.ch('0');
        f.key(KeyCode::Enter);
        f.ch('s');
        assert_eq!(
            f.status(),
            Some("ID atualizado: NS-1 → NS-10. A pasta .nick/docs/NS-1/ virou .nick/docs/NS-10/.")
        );
        assert!(!docs.exists());
        assert!(p.join(".nick/docs/NS-10/plan.md").is_file());
        assert_eq!(f.app.screen, Screen::Tree);
        assert_eq!(f.detail().ticket.key, "NS-10");
        assert!(f.detail().artifacts.plan);
        assert_eq!(f.keys(), ["NS-10", "NS-2"]);
    }

    #[test]
    fn the_form_of_dev_and_review_warns_of_a_missing_plan_without_blocking() {
        for kind in [StageType::Development, StageType::Review] {
            let (mut f, p) = with_agents();
            let mut launcher = FakeLauncher::new(true);
            f.g(&mut launcher);
            f.pick(kind);
            assert!(f.plan_missing(), "no plan: warned at the opening");
            // Enter still renders.
            assert_eq!(f.key(KeyCode::Enter), Action::RenderPrompt);
            f.key(KeyCode::Esc);
            let docs = p.join(".nick/docs/NS-1");
            fs::create_dir_all(&docs).unwrap();
            fs::write(docs.join("plan.md"), "# plano\n").unwrap();
            f.g(&mut launcher);
            f.pick(kind);
            assert!(!f.plan_missing(), "with the plan");
            // The warning follows the ID typed.
            f.key(KeyCode::Backspace);
            assert!(f.plan_missing(), "NS- has no plan");
            f.ch('1');
            assert!(!f.plan_missing());
            f.ch('x');
            assert!(f.plan_missing());
            // An invalid or empty ID: no warning (the validation refuses it).
            f.ch(' ');
            f.ch('b');
            assert!(!f.plan_missing(), "invalid ID");
            for _ in 0.."NS-1x b".len() {
                f.key(KeyCode::Backspace);
            }
            assert_eq!(f.form().1[0], "");
            assert!(!f.plan_missing(), "empty ID");
            f.type_text("NS-2");
            assert!(f.plan_missing());
            // A path typed in the plan field: no warning, no check.
            f.key(KeyCode::Tab);
            f.type_text("outro/plano.md");
            assert!(!f.plan_missing(), "a path typed");
            assert_eq!(f.key(KeyCode::Enter), Action::RenderPrompt);
            f.key(KeyCode::Esc);
            fs::remove_dir_all(p.join(".nick/docs")).unwrap();
        }
        // The other types never warn.
        let (mut f, _) = with_agents();
        let mut launcher = FakeLauncher::new(true);
        f.g(&mut launcher);
        f.pick(StageType::TechLeader);
        assert!(!f.plan_missing());
    }

    // ---- the tree (NS-38) ----

    use crate::tree::NodeId;

    const MODULE: NodeId = NodeId::Module(ModuleKind::AutoScrum);

    impl Fixture {
        /// The visible nodes of the tree, in order.
        fn rows(&self) -> Vec<NodeId> {
            self.app.visible().iter().map(|row| row.id).collect()
        }

        /// Selects `id` with its branches open, as the helpers of navigation of the tests do.
        fn select(&mut self, id: NodeId) {
            self.app.reveal_and_select(id);
            self.app.refresh_tree();
            assert_eq!(self.app.selected, id, "{id:?} not on the tree");
        }

        /// The id of the project at `rel`.
        fn pid(&self, rel: &str) -> i64 {
            let path = self.root.join(rel);
            self.app
                .rows
                .iter()
                .find(|row| row.project.path == path)
                .expect("registered")
                .project
                .id
        }

        /// The node of the ticket `key` (of any project; the first one).
        fn tnode(&self, key: &str) -> NodeId {
            let ticket = self
                .app
                .tickets
                .values()
                .flatten()
                .find(|t| t.key == key)
                .unwrap_or_else(|| panic!("no ticket {key}"));
            NodeId::Ticket(ticket.id)
        }
    }

    #[test]
    fn the_tree_starts_on_the_module_closed() {
        let mut f = fixture();
        assert_eq!(f.app.screen, Screen::Tree);
        assert_eq!(f.app.selected, MODULE);
        assert_eq!(f.rows(), [MODULE]);
        assert_eq!(f.app.dialog, None);
        // Registered before the nick opened: still closed, the module selected.
        let p = f.dir("p", true);
        f.add(p.to_str().unwrap());
        let db = Db::open(&f.root.join("nick.db")).unwrap();
        let app = App::new(db, None, None, f.root.join("bin/nick-init"));
        assert_eq!(app.selected, MODULE);
        assert_eq!(app.visible().len(), 1);
        assert!(app.visible()[0].expandable && !app.visible()[0].open);
    }

    /// The tree of a module with the projects `a` (tickets A-1, A-2) and `b` (B-1), every
    /// branch closed and the module selected.
    fn two_projects() -> Fixture {
        let (mut f, _) = with_tickets("a");
        f.new_ticket("um", "A-1");
        f.new_ticket("dois", "A-2");
        let b = f.dir("b", true);
        f.add(b.to_str().unwrap());
        f.key(KeyCode::Enter);
        f.new_ticket("três", "B-1");
        f.app.tree = Tree::default();
        f.app.selected = MODULE;
        f.app.refresh_tree();
        assert_eq!(f.rows(), [MODULE]);
        f
    }

    #[test]
    fn right_and_enter_open_a_closed_node_and_go_down_on_an_open_one() {
        for open in [KeyCode::Right, KeyCode::Enter] {
            let mut f = two_projects();
            let (a, b) = (NodeId::Project(f.pid("a")), NodeId::Project(f.pid("b")));
            f.app.status = Some("x".into());
            assert_eq!(f.key(open), Action::None);
            assert_eq!(f.rows(), [MODULE, a, b], "{open:?}");
            assert_eq!(f.app.selected, MODULE, "opening does not move");
            assert_eq!(f.status(), None);
            f.key(open);
            assert_eq!(f.app.selected, a, "down to the first child");
            f.key(open);
            let (a1, a2) = (f.tnode("A-1"), f.tnode("A-2"));
            assert_eq!(f.rows(), [MODULE, a, a1, a2, b]);
            f.key(open);
            assert_eq!(f.app.selected, a1);
            // A ticket without agents is a leaf: nothing happens.
            f.key(open);
            assert_eq!(f.app.selected, a1);
            assert_eq!(f.rows(), [MODULE, a, a1, a2, b]);
        }
    }

    #[test]
    fn up_down_k_j_move_on_the_visible_rows_and_stop_at_the_ends() {
        let mut f = two_projects();
        f.key(KeyCode::Enter);
        let (a, b) = (NodeId::Project(f.pid("a")), NodeId::Project(f.pid("b")));
        f.key(KeyCode::Down);
        assert_eq!(f.app.selected, a);
        f.ch('j');
        assert_eq!(f.app.selected, b);
        f.key(KeyCode::Down);
        assert_eq!(f.app.selected, b, "stops at the last");
        f.ch('k');
        f.key(KeyCode::Up);
        assert_eq!(f.app.selected, MODULE);
        f.key(KeyCode::Up);
        assert_eq!(f.app.selected, MODULE, "stops at the first");
    }

    #[test]
    fn left_and_esc_close_an_open_node_and_go_up_from_a_closed_one() {
        for back in [KeyCode::Left, KeyCode::Esc] {
            let mut f = two_projects();
            let a = NodeId::Project(f.pid("a"));
            let a1 = f.tnode("A-1");
            f.select(a1);
            f.app.status = Some("x".into());
            assert_eq!(f.key(back), Action::None);
            assert_eq!(f.app.selected, a, "a leaf goes up");
            assert_eq!(f.status(), None);
            f.key(back);
            assert_eq!(f.app.selected, a, "an open node closes");
            assert!(!f.rows().contains(&a1));
            f.key(back);
            assert_eq!(f.app.selected, MODULE);
            f.key(back);
            assert_eq!(f.rows(), [MODULE]);
            assert_eq!(f.key(back), Action::None, "the root stays");
            assert_eq!(f.app.selected, MODULE);
        }
    }

    #[test]
    fn an_unavailable_project_does_not_open_and_says_why_on_the_tree() {
        for dir_missing in [true, false] {
            let (mut f, p) = with_one_project();
            let node = NodeId::Project(f.pid("proj"));
            assert_eq!(f.app.selected, node);
            let mark = if dir_missing {
                fs::remove_dir_all(&p).unwrap();
                "[indisponível: pasta não encontrada]"
            } else {
                fs::remove_file(projects::config_file(&p)).unwrap();
                "[indisponível: sem .nick/nick.conf]"
            };
            for open in [KeyCode::Enter, KeyCode::Right] {
                assert_eq!(f.key(open), Action::None);
                assert_eq!(f.rows(), [MODULE, node], "{mark}");
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
    fn an_open_project_that_becomes_unavailable_is_closed() {
        let (mut f, p) = with_tickets("proj");
        f.new_ticket("t", "NS-1");
        let node = NodeId::Project(f.pid("proj"));
        assert!(f.rows().contains(&f.tnode("NS-1")));
        fs::remove_file(projects::config_file(&p)).unwrap();
        f.key(KeyCode::Down);
        assert_eq!(f.rows(), [MODULE, node]);
        assert_eq!(f.app.selected, node, "the selection goes up to the project");
        fs::write(projects::config_file(&p), "").unwrap();
        f.key(KeyCode::Down);
        assert_eq!(f.rows(), [MODULE, node], "it stays closed");
    }

    #[test]
    fn the_selection_survives_a_refresh_by_identity() {
        let mut f = two_projects();
        let b1 = f.tnode("B-1");
        f.select(b1);
        // Another nick creates a ticket in `a` (above B-1): B-1 stays selected.
        f.select(f.tnode("A-1"));
        f.select(b1);
        let a = f.pid("a");
        crate::tickets::create(&f.app.db, a, "de fora", "A-0").unwrap();
        f.key(KeyCode::Char('z'));
        assert_eq!(f.app.selected, b1);
        // The selected ticket hidden: the next sibling, else the previous, else the parent.
        // The list of `a` is A-1, A-2, A-0 (in the order they were created).
        f.select(f.tnode("A-1"));
        f.ch('o');
        f.ch('s');
        assert_eq!(f.app.selected, f.tnode("A-2"), "the next one");
        f.select(f.tnode("A-0"));
        f.ch('o');
        f.ch('s');
        assert_eq!(f.app.selected, f.tnode("A-2"), "the last: the previous one");
        f.ch('o');
        f.ch('s');
        assert_eq!(f.app.selected, NodeId::Project(a), "none left: the parent");
    }

    impl Fixture {
        fn tnode_id(&self, key: &str) -> i64 {
            match self.tnode(key) {
                NodeId::Ticket(id) => id,
                _ => unreachable!(),
            }
        }
    }

    #[test]
    fn typing_in_a_dialog_does_not_refresh_the_tree() {
        let (mut f, p) = with_one_project();
        f.select(MODULE);
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
        f.app.select(NodeId::Project(f.pid("proj")));
        f.ch('r');
        assert_eq!(f.app.rows[0].availability, Availability::ConfigMissing);
        f.ch('n');
        assert_eq!(f.app.rows[0].availability, Availability::DirMissing);
    }

    #[test]
    fn the_keys_of_a_node_do_nothing_on_another() {
        let mut f = two_projects();
        // On the module: no ticket key, no removal; `x` only on a live agent.
        for c in ['e', 'o', 'g', 'm', 'i', 'r', 'x'] {
            assert_eq!(f.ch(c), Action::None, "{c}");
            assert_eq!(f.app.dialog, None, "{c}");
        }
        // On a project: no `c`, no ticket key.
        f.select(NodeId::Project(f.pid("a")));
        for c in ['c', 'e', 'o', 'g', 'm', 'i', 'x'] {
            assert_eq!(f.ch(c), Action::None, "{c}");
            assert_eq!(f.app.dialog, None, "{c}");
            assert_eq!(f.app.screen, Screen::Tree);
        }
        // On a ticket: no `a`, `r` nor `c`.
        f.select(f.tnode("A-1"));
        for c in ['a', 'r', 'c', 'x'] {
            assert_eq!(f.ch(c), Action::None, "{c}");
            assert_eq!(f.app.dialog, None, "{c}");
            assert_eq!(f.app.screen, Screen::Tree);
        }
    }

    // ---- focus and the shown agent on the tree (NS-38) ----

    impl Fixture {
        /// The node of the running agent `id`.
        fn anode(id: i64) -> NodeId {
            NodeId::Agent(id)
        }
    }

    #[test]
    fn open_agent_selects_the_new_agent_with_the_focus_on_the_terminal() {
        let (f, launcher) = running();
        let id = launcher.ids[0];
        assert_eq!(f.app.selected, Fixture::anode(id));
        assert_eq!(f.app.shown, Some(id));
        assert_eq!(f.app.last_shown, Some(id));
        assert_eq!(f.app.focus, Focus::Terminal);
        assert!(f.rows().contains(&Fixture::anode(id)), "its branch is open");
        assert_eq!(f.app.agents[0].owner, Owner::Ticket(f.tnode_id("NS-2")));
        assert_eq!(f.app.agents[0].project_id, f.pid("proj"));
    }

    #[test]
    fn moving_to_a_live_agent_shows_it_and_to_another_node_hides_it() {
        let (mut f, launcher) = two_running();
        let (first, second) = (launcher.ids[0], launcher.ids[1]);
        f.shortcut();
        assert_eq!(f.app.focus, Focus::List);
        // Up from the second agent: its ticket; every agent hidden.
        f.key(KeyCode::Up);
        assert_eq!(f.app.selected, f.tnode("B-1"));
        assert_eq!(f.app.shown, None);
        assert!(f.app.wants_hidden_text(second), "it can be marked now");
        assert_eq!(f.app.last_shown, Some(second));
        assert!(f.app.hidden_output(first, "algo novo"));
        // To the first agent: shown, its mark gone, the focus still on the tree.
        f.select(Fixture::anode(first));
        assert_eq!(f.app.shown, Some(first));
        assert!(!f.app.agents[0].unseen);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(f.app.last_shown, Some(first));
    }

    #[test]
    fn enter_on_a_live_agent_focuses_it_and_on_another_says_why() {
        let (mut f, launcher) = running();
        let id = launcher.ids[0];
        f.shortcut();
        assert_eq!(f.app.focus, Focus::List);
        f.app.status = Some("x".into());
        for open in [KeyCode::Enter, KeyCode::Right] {
            assert_eq!(f.key(open), Action::None);
            assert_eq!(f.app.focus, Focus::Terminal);
            assert_eq!(f.status(), None);
            f.shortcut();
        }
        f.app.agent_ended(id, 0);
        f.app.status = None;
        assert_eq!(f.app.selected, Fixture::anode(id));
        f.key(KeyCode::Enter);
        assert_eq!(f.app.focus, Focus::List);
        // NS-39 (decision 3): a stopped one says which key resumes it.
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · NS-2 está parado. Pressione r para retomar.")
        );
    }

    #[test]
    fn the_shortcut_from_another_node_goes_to_the_last_shown_and_back() {
        let (mut f, launcher) = two_running();
        let (first, second) = (launcher.ids[0], launcher.ids[1]);
        f.shortcut();
        // The first agent shown last, then the module selected and every branch closed.
        f.select(Fixture::anode(first));
        f.select(MODULE);
        f.key(KeyCode::Esc);
        assert_eq!(f.rows(), [MODULE]);
        assert_eq!(f.app.shown, None);
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.selected, Fixture::anode(first), "the last shown");
        assert!(
            f.rows().contains(&Fixture::anode(first)),
            "its branches open"
        );
        assert_eq!(f.app.shown, Some(first));
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(f.app.selected, Fixture::anode(first), "the same agent");
        // On a live agent the shortcut takes that one.
        f.select(Fixture::anode(second));
        f.shortcut();
        assert_eq!(f.app.shown, Some(second));
        assert_eq!(f.app.focus, Focus::Terminal);
    }

    #[test]
    fn the_shortcut_does_nothing_without_a_live_agent_nor_with_a_dialog() {
        let (mut f, launcher) = running();
        let id = launcher.ids[0];
        f.shortcut();
        f.select(f.tnode("NS-1"));
        f.ch('m');
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::ChooseTicketStage { .. })
        ));
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(f.app.selected, f.tnode("NS-1"));
        f.key(KeyCode::Esc);
        f.app.agent_ended(id, 0);
        assert_eq!(f.app.last_shown, None);
        assert_eq!(f.shortcut(), Action::None);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(f.app.selected, f.tnode("NS-1"));
    }

    /// Replaces `the_end_of_the_shown_agent_shows_the_most_recent_one_on_the_list` (decision
    /// 10 of NS-38): the selection stays on the node, now an ended agent.
    #[test]
    fn the_end_of_the_shown_agent_keeps_the_selection_on_its_node() {
        let (mut f, launcher) = two_running();
        let (first, second) = (launcher.ids[0], launcher.ids[1]);
        f.app.agent_ended(second, 0);
        assert_eq!(f.status(), Some("O agente beta · Dev · B-1 terminou."));
        assert_eq!(f.app.selected, Fixture::anode(second));
        assert!(
            f.rows().contains(&Fixture::anode(second)),
            "an ended agent now"
        );
        assert_eq!(f.app.shown, None);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(
            f.app.last_shown,
            Some(first),
            "the live one opened most recently"
        );
        assert_eq!(f.agent_rows()[1].3, "stopped");
        assert!(!agents::lock_path(&f.agents_dir(), &launcher.spawns[1].2).exists());
        assert_eq!(f.agent_rows()[0].3, "active");
        // The last one: nothing to go to.
        f.app.agent_ended(first, 0);
        assert_eq!(f.app.shown, None);
        assert_eq!(f.app.last_shown, None);
        assert!(f.app.agents.is_empty());
        // An end that is not of a running agent changes nothing.
        f.app.status = None;
        f.app.agent_ended(first, 0);
        assert_eq!(f.status(), None);
    }

    #[test]
    fn x_on_a_live_agent_asks_and_only_s_ends_it() {
        let (mut f, launcher) = running();
        let id = launcher.ids[0];
        f.shortcut();
        assert_eq!(f.ch('x'), Action::None);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::ConfirmEndAgent {
                id,
                name: "proj · Tech Leader · NS-2".into()
            })
        );
        assert_eq!(f.ch('n'), Action::None);
        assert_eq!(f.app.dialog, None);
        f.ch('x');
        assert_eq!(f.ch('s'), Action::EndAgent(id));
        f.app.agent_ended_by_operator(id);
        assert_eq!(f.app.selected, Fixture::anode(id));
        f.app.status = None;
        assert_eq!(f.ch('x'), Action::None, "an ended agent is not asked");
        assert_eq!(f.app.dialog, None);
    }

    #[test]
    fn the_filter_v_shows_the_branches_of_the_live_agents_and_the_mark_goes_up() {
        let (mut f, launcher) = two_running();
        let (first, second) = (launcher.ids[0], launcher.ids[1]);
        let (proj, beta) = (
            NodeId::Project(f.pid("proj")),
            NodeId::Project(f.pid("beta")),
        );
        f.shortcut();
        f.select(MODULE);
        f.key(KeyCode::Right);
        // Every branch closed, but the module open: the projects.
        f.app.tree.close(proj);
        f.app.tree.close(beta);
        f.app.refresh_tree();
        // The mark goes up at once: no key, no refresh (the loop only redraws).
        assert!(f.app.hidden_output(first, "algo novo"));
        let row = |f: &Fixture, id| *f.app.visible().iter().find(|r| r.id == id).unwrap();
        assert!(
            row(&f, proj).unseen_below,
            "the mark goes up to the closed project"
        );
        assert!(!row(&f, beta).unseen_below);
        assert!(!row(&f, MODULE).unseen_below, "the module is open");
        f.app.status = Some("x".into());
        assert_eq!(f.ch('v'), Action::None);
        assert_eq!(f.status(), None);
        assert_eq!(
            f.rows(),
            [
                MODULE,
                beta,
                f.tnode("B-1"),
                Fixture::anode(second),
                proj,
                f.tnode("NS-1"),
                Fixture::anode(first),
            ],
            "only the live branches, open (the projects by name)"
        );
        // Shown, the mark goes.
        f.select(Fixture::anode(first));
        assert!(!f.app.agents[0].unseen);
        f.ch('v');
        assert!(!f.app.tree.only_active);
        assert_eq!(f.app.selected, Fixture::anode(first));
        // Without live agents the filter shows only the module.
        f.app.agent_ended(first, 0);
        f.app.agent_ended(second, 0);
        f.ch('v');
        assert_eq!(f.rows(), [MODULE]);
        assert_eq!(f.app.selected, MODULE);
    }

    #[test]
    fn the_filters_are_global_and_q_quits_on_every_node() {
        let mut f = two_projects();
        f.ch('f');
        f.ch('v');
        assert_eq!(f.app.filters(), (true, true));
        f.ch('f');
        f.ch('v');
        assert_eq!(f.app.filters(), (false, false));
        for node in [MODULE, NodeId::Project(f.pid("a")), f.tnode("A-1")] {
            f.select(node);
            assert_eq!(f.ch('q'), Action::Quit, "{node:?}");
            assert_eq!(f.ctrl_c(), Action::Quit, "{node:?}");
        }
    }

    /// Decisions 7 and 11 of NS-38 together: an unavailable project with a live agent of this
    /// nick stays open and opens, so the agent stays reachable on the tree and by the shortcut;
    /// without it, the refusal of before.
    #[test]
    fn an_unavailable_project_with_a_live_agent_stays_reachable() {
        let (mut f, launcher) = running();
        let id = launcher.ids[0];
        let proj = NodeId::Project(f.pid("proj"));
        f.shortcut();
        fs::remove_file(projects::config_file(&f.root.join("proj"))).unwrap();
        f.key(KeyCode::Char('z'));
        assert_eq!(f.app.selected, NodeId::Agent(id), "it stays selected");
        f.select(proj);
        f.key(KeyCode::Left);
        assert!(!f.app.tree.is_open(proj));
        f.app.status = None;
        f.key(KeyCode::Enter);
        assert!(f.app.tree.is_open(proj), "it opens");
        assert_eq!(f.status(), None);
        f.select(MODULE);
        f.shortcut();
        assert_eq!(f.app.selected, NodeId::Agent(id));
        assert_eq!(f.app.focus, Focus::Terminal);
        // Once the agent ends, it is closed and refused, as any unavailable project.
        f.app.agent_ended(id, 0);
        assert!(!f.app.tree.is_open(proj));
        f.select(proj);
        f.key(KeyCode::Enter);
        assert!(f.status().unwrap().starts_with("Projeto indisponível: "));
    }

    /// A filter that takes the selected node away: the next sibling still there, by the rows
    /// before the key (decision 9 of NS-38).
    #[test]
    fn a_filter_that_takes_the_selection_away_goes_to_the_next_sibling() {
        let (mut f, _) = running();
        f.shortcut();
        f.select(f.tnode("NS-1"));
        f.ch('v');
        assert_eq!(f.app.selected, f.tnode("NS-2"), "NS-1 has no live agent");
        f.ch('v');
        f.select(f.tnode("NS-1"));
        f.ch('o');
        f.ch('s');
        f.ch('f');
        f.select(f.tnode("NS-1"));
        f.ch('f');
        assert_eq!(f.app.selected, f.tnode("NS-2"), "hidden again");
    }

    // ---- removing a project with its agents (NS-38, decision of the user) ----

    /// The confirmation of the removal of `proj` (its node selected): the counts it shows.
    fn removal(f: &mut Fixture) -> (i64, i64, i64) {
        f.app.select(NodeId::Project(f.pid("proj")));
        assert_eq!(f.ch('r'), Action::None);
        match &f.app.dialog {
            Some(Dialog::ConfirmRemove {
                tickets,
                agents,
                live,
                ..
            }) => (*tickets, *agents, *live),
            other => panic!("not in the removal: {other:?} {:?}", f.status()),
        }
    }

    #[test]
    fn the_removal_counts_the_agents_and_ends_the_live_ones_through_the_loop() {
        let (mut f, launcher) = running();
        let id = launcher.ids[0];
        f.shortcut();
        let project = f.app.project_row(f.pid("proj")).unwrap().project.clone();
        assert_eq!(removal(&mut f), (2, 1, 1));
        // n cancels: nothing ends.
        f.ch('n');
        assert_eq!(f.app.agents.len(), 1);
        removal(&mut f);
        assert_eq!(
            f.ch('s'),
            Action::RemoveProject {
                project: project.clone(),
                agents: vec![id],
            }
        );
        assert_eq!(f.stored().len(), 1, "the loop ends the agents first");
        // The loop terminated the session: the record ends, the lock goes, then the removal.
        f.app.remove_project_ending_agents(&project, &[id]);
        assert_eq!(
            f.status(),
            Some(
                "Projeto removido do nick: proj. Nenhum arquivo foi apagado. O agente proj · \
                 Tech Leader · NS-2 foi encerrado."
            )
        );
        assert!(f.app.agents.is_empty());
        assert_eq!(f.app.shown, None);
        assert_eq!(f.app.focus, Focus::List);
        assert_eq!(f.app.last_shown, None);
        assert!(!agents::lock_path(&f.agents_dir(), &launcher.spawns[0].2).exists());
        assert_eq!(f.stored(), Vec::<PathBuf>::new());
        assert_eq!(f.agent_rows(), [], "its agents left the database with it");
        assert_eq!(f.app.selected, MODULE);
    }

    #[test]
    fn the_removal_without_a_live_agent_counts_the_ended_ones_and_removes_at_once() {
        let (mut f, launcher) = running();
        f.app.agent_ended(launcher.ids[0], 0);
        let mut other = FakeLauncher::new(true);
        f.select(f.tnode("NS-1"));
        f.open_agent(0, &mut other);
        f.app.agent_ended(other.ids[0], 0);
        assert_eq!(removal(&mut f), (2, 2, 0));
        assert_eq!(f.ch('s'), Action::None);
        assert_eq!(
            f.status(),
            Some("Projeto removido do nick: proj. Nenhum arquivo foi apagado.")
        );
        assert_eq!(f.agent_rows(), []);
    }

    /// An agent active in another nick: this one can't end it, so the removal is refused (the
    /// safe side, as "Editar ID" of NS-36), when it would open and again on the `s`.
    #[test]
    fn the_removal_is_refused_with_an_agent_active_in_another_nick() {
        let refusal = "O projeto proj tem agente ativo em outro nick. Encerre os agentes dele lá \
                       antes de remover o projeto.";
        let (mut f, _) = with_agents();
        let ticket = f.tnode_id("NS-1");
        f.app
            .db
            .insert_agent(
                Owner::Ticket(ticket),
                "po",
                "outro",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        f.app.select(NodeId::Project(f.pid("proj")));
        assert_eq!(f.ch('r'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), Some(refusal));
        // Opened before the other nick opened its agent: refused on the s.
        let (mut f, _) = with_agents();
        removal(&mut f);
        let ticket = f.tnode_id("NS-1");
        f.app
            .db
            .insert_agent(
                Owner::Ticket(ticket),
                "po",
                "outro",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        assert_eq!(f.ch('s'), Action::None);
        assert_eq!(f.status(), Some(refusal));
        assert_eq!(f.stored().len(), 1, "nothing removed");
    }

    /// Review of the NS-38, P5: the new ticket goes to the project where `a` opened the dialog,
    /// even if the selection moved meanwhile (a refresh by the end of an agent can move it);
    /// a project gone from the database says why, and stores nothing.
    #[test]
    fn the_new_ticket_goes_to_the_project_where_the_dialog_opened() {
        let mut f = two_projects();
        let (a, b) = (f.pid("a"), f.pid("b"));
        f.app.select(NodeId::Project(a));
        f.ch('a');
        f.type_text("do a");
        f.app.select(NodeId::Project(b));
        f.key(KeyCode::Enter);
        let of = |f: &Fixture, id| f.app.db.list_tickets(id, true).unwrap();
        assert!(
            of(&f, a).iter().any(|t| t.title == "do a"),
            "{:?}",
            f.status()
        );
        assert!(!of(&f, b).iter().any(|t| t.title == "do a"));
        // The project removed by another nick while the dialog is open.
        f.app.select(NodeId::Project(b));
        f.ch('a');
        f.type_text("perdido");
        f.app.db.delete_project(b).unwrap();
        f.key(KeyCode::Enter);
        assert!(
            f.status()
                .is_some_and(|s| s.starts_with("Erro no banco do nick")),
            "{:?}",
            f.status()
        );
        assert!(
            matches!(f.app.dialog, Some(Dialog::NewTicket { .. })),
            "it stays open"
        );
    }

    // ---- NS-39: resuming and archiving the agents ----

    /// The Tech Leader agent of NS-2 (`running`), ended: stopped, its node selected.
    fn stopped_agent() -> (Fixture, FakeLauncher, i64) {
        let (mut f, launcher) = running();
        let id = launcher.ids[0];
        f.app.agent_ended(id, 0);
        assert_eq!(f.app.selected, Fixture::anode(id));
        f.app.status = None;
        (f, launcher, id)
    }

    const TL_NAME: &str = "proj · Tech Leader · NS-2";

    impl Fixture {
        /// `r` on the selected node and, when it asks for it (`Action::CheckResume`), the check
        /// of the resume carried out with `launcher`, as the loop does.
        fn r(&mut self, launcher: &mut FakeLauncher) -> bool {
            match self.ch('r') {
                Action::CheckResume { project, agent } => {
                    self.app.check_resume(&project, &agent, launcher);
                    true
                }
                Action::None => false,
                other => panic!("unexpected {other:?}"),
            }
        }

        /// The state of the agent `id` in the database.
        fn state_of(&self, id: i64) -> String {
            self.app
                .db
                .get_agent(id)
                .unwrap()
                .expect("in the database")
                .state
        }

        fn session_of(&self, id: i64) -> String {
            self.app.db.get_agent(id).unwrap().unwrap().session_id
        }

        /// An agent row of the ticket `key`, inserted as another nick would (active), its node
        /// selected (the ticket opened).
        fn foreign_agent(&mut self, key: &str, session: &str) -> i64 {
            let ticket = self.tnode_id(key);
            let agent = self
                .app
                .db
                .insert_agent(
                    Owner::Ticket(ticket),
                    "review",
                    session,
                    "proj · Review · NS-1",
                    key,
                )
                .unwrap();
            self.app.tree.open(NodeId::Ticket(ticket));
            self.app.refresh_tree();
            self.select(Fixture::anode(agent.id));
            agent.id
        }
    }

    #[test]
    fn r_on_a_stopped_agent_checks_finds_the_conversation_and_resumes_it() {
        let (mut f, mut launcher, id) = stopped_agent();
        let p = f.app.selected_project().unwrap().path.clone();
        let agent = f.app.agent_record(id).unwrap().clone();
        assert_eq!(
            f.ch('r'),
            Action::CheckResume {
                project: p.clone(),
                agent: agent.clone()
            }
        );
        assert_eq!(
            launcher.checks.len(),
            1,
            "no check before the loop carries it out"
        );
        f.app.check_resume(&p, &agent, &mut launcher);
        assert_eq!(launcher.checks.len(), 2, "the check of the resume");
        assert_eq!(
            launcher.conversations,
            std::slice::from_ref(&agent.session_id)
        );
        assert_eq!(
            launcher.resumes,
            [(
                Stage::TechLeader,
                Some("NS-2".to_string()),
                "NS-2".to_string()
            )]
        );
        assert_eq!(launcher.ids, [id, id], "the same row, the same session");
        assert_eq!(launcher.spawns[1].2, agent.session_id);
        assert_eq!(launcher.spawns[1].3, TL_NAME, "the name stored");
        assert_eq!(f.state_of(id), "active");
        assert!(f.app.agent(id).is_some(), "running again");
        assert_eq!(f.app.selected, Fixture::anode(id));
        assert_eq!(f.app.shown(), Some(id));
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · NS-2 foi retomado.")
        );
        assert!(
            f.agents_dir()
                .join(format!("{}.lock", agent.session_id))
                .exists()
        );
        // Its end stops it again, as any agent.
        f.app.agent_ended(id, 0);
        assert_eq!(f.state_of(id), "stopped");
    }

    // ---- NS-50: the resume follows the session the conversation went on in ----

    /// The B session of the tests of the NS-50: where claude went on with the conversation.
    const SESSION_B: &str = "0f8e4c2a-9b1d-4e6f-a3c5-7d9e1b2f4a61";

    fn resume_session_of(f: &Fixture, id: i64) -> Option<String> {
        f.app.db.get_agent(id).unwrap().unwrap().resume_session_id
    }

    #[test]
    fn a_resume_follows_the_latest_session_and_stores_it() {
        let (mut f, mut launcher, id) = stopped_agent();
        let opening = f.session_of(id);
        launcher.latest = Some(SESSION_B.to_string());
        assert!(f.r(&mut launcher));
        assert_eq!(
            launcher.conversations,
            std::slice::from_ref(&opening),
            "the effective session is asked: the one of the opening"
        );
        assert_eq!(launcher.spawns[1].2, SESSION_B, "--resume <B>");
        assert_eq!(resume_session_of(&f, id).as_deref(), Some(SESSION_B));
        assert_eq!(
            f.session_of(id),
            opening,
            "the session of the opening stays"
        );
        assert_eq!(f.state_of(id), "active");
        assert_eq!(
            f.status(),
            Some(
                format!(
                    "O agente {TL_NAME} foi retomado. A conversa tinha continuado em outra sessão \
                     do claude ({SESSION_B}), e o nick passou a usá-la."
                )
                .as_str()
            )
        );
        assert!(
            f.agents_dir().join(format!("{opening}.lock")).exists(),
            "the lock of the opening"
        );
        assert!(!f.agents_dir().join(format!("{SESSION_B}.lock")).exists());
        // The next resume starts from B, and the bar is the one of before, byte by byte.
        f.app.agent_ended(id, 0);
        f.app.status = None;
        launcher.latest = None;
        assert!(f.r(&mut launcher));
        assert_eq!(launcher.conversations[1], SESSION_B);
        assert_eq!(launcher.spawns[2].2, SESSION_B);
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · NS-2 foi retomado.")
        );
        assert_eq!(resume_session_of(&f, id).as_deref(), Some(SESSION_B));
    }

    #[test]
    fn a_latest_session_another_agent_has_is_not_taken_and_the_bar_says_so() {
        let (mut f, mut launcher, id) = stopped_agent();
        let opening = f.session_of(id);
        let ticket = owner_ticket(f.app.agent_record(id).unwrap().owner);
        let other = f
            .app
            .db
            .insert_agent(Owner::Ticket(ticket), "po", "outra", "outro", "NS-2")
            .unwrap();
        f.app.db.resume_agent(other.id, SESSION_B).unwrap();
        launcher.latest = Some(SESSION_B.to_string());
        assert!(f.r(&mut launcher));
        assert_eq!(
            launcher.spawns[1].2, opening,
            "--resume <the session stored>"
        );
        assert_eq!(resume_session_of(&f, id), None);
        assert_eq!(f.state_of(id), "active");
        assert_eq!(
            f.status(),
            Some(
                format!(
                    "O agente {TL_NAME} foi retomado. A conversa continuou na sessão {SESSION_B} \
                     do claude, mas outro agente deste nick já usa essa sessão: a retomada \
                     seguiu pela sessão guardada."
                )
                .as_str()
            )
        );
    }

    #[test]
    fn the_swap_comes_before_the_warning_of_the_folder() {
        let (mut f, mut launcher, id) = stopped_agent();
        let ticket = owner_ticket(f.app.agent_record(id).unwrap().owner);
        let t = f.app.db.get_ticket(ticket).unwrap().unwrap();
        let p = f.app.selected_project().unwrap().path.clone();
        tickets::edit_id(&f.app.db, &p, &t, "NS-9").unwrap();
        f.app.refresh_tree();
        launcher.latest = Some(SESSION_B.to_string());
        assert!(f.r(&mut launcher));
        assert_eq!(
            f.status(),
            Some(
                format!(
                    "O agente {TL_NAME} foi retomado. A conversa tinha continuado em outra sessão \
                     do claude ({SESSION_B}), e o nick passou a usá-la. Ele foi avisado de que a \
                     pasta agora é .nick/docs/NS-9/."
                )
                .as_str()
            )
        );
    }

    /// The ids of the bar go through `printable` (a control character from the launcher would
    /// reach the terminal).
    #[test]
    fn the_session_in_the_bar_is_printable() {
        let (mut f, mut launcher, id) = stopped_agent();
        launcher.latest = Some("b\x1b[2J".to_string());
        assert!(f.r(&mut launcher));
        assert!(!f.status().unwrap().contains('\x1b'), "{:?}", f.status());
        assert_eq!(f.state_of(id), "active");
    }

    #[test]
    fn a_failing_check_or_conversation_check_shows_the_alert_and_changes_nothing() {
        let (mut f, mut launcher, id) = stopped_agent();
        launcher.check = Err("Erro: a jail falhou.".into());
        assert!(f.r(&mut launcher));
        assert_eq!(f.alert(), "Nada foi aberto.\n\nErro: a jail falhou.");
        assert!(
            launcher.conversations.is_empty(),
            "no conversation check after a failure"
        );
        assert_eq!(launcher.resumes, []);
        assert_eq!(f.state_of(id), "stopped");
        f.key(KeyCode::Esc);
        launcher.check = FakeLauncher::new(true).check;
        launcher.conversation = Err("Erro: x".into());
        assert!(f.r(&mut launcher));
        assert_eq!(f.alert(), "Nada foi aberto.\n\nErro: x");
        assert_eq!(launcher.resumes, []);
        assert_eq!(f.state_of(id), "stopped");
    }

    #[test]
    fn a_missing_launcher_on_resume_shows_its_path() {
        let (mut f, mut launcher, id) = stopped_agent();
        fs::remove_file(f.root.join("bin/nick-agent")).unwrap();
        assert!(f.r(&mut launcher));
        assert!(
            f.alert().starts_with("Não encontrei o bin/nick-agent em "),
            "{}",
            f.alert()
        );
        assert_eq!(launcher.checks.len(), 1);
        assert_eq!(f.state_of(id), "stopped");
    }

    fn lost(f: &Fixture) -> (Agent, Precheck) {
        match &f.app.dialog {
            Some(Dialog::ConversationLost {
                agent, precheck, ..
            }) => (agent.clone(), precheck.clone()),
            other => panic!("not in the lost conversation: {other:?}"),
        }
    }

    #[test]
    fn a_lost_conversation_opens_the_warning_and_esc_writes_nothing() {
        let (mut f, mut launcher, id) = stopped_agent();
        launcher.conversation = Ok(false);
        assert!(f.r(&mut launcher));
        assert_eq!(lost(&f).0.id, id);
        assert_eq!(launcher.resumes, [], "no PTY");
        assert_eq!(launcher.ids, [id], "only the first opening");
        assert_eq!(f.state_of(id), "stopped");
        // The reserved shortcut is ignored with the dialog open (NS-34).
        f.shortcut();
        assert_eq!(f.app.focus, Focus::List);
        assert!(f.app.dialog.is_some());
        // Another key keeps it; Esc closes it writing nothing.
        f.ch('x');
        assert!(f.app.dialog.is_some());
        assert_eq!(f.key(KeyCode::Esc), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.state_of(id), "stopped");
        assert_eq!(f.app.selected, Fixture::anode(id));
    }

    #[test]
    fn a_on_the_lost_conversation_archives_without_asking_again() {
        let (mut f, mut launcher, id) = stopped_agent();
        let ticket = owner_ticket(f.app.agent_record(id).unwrap().owner);
        launcher.conversation = Ok(false);
        f.r(&mut launcher);
        assert_eq!(f.ch('a'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.state_of(id), "archived");
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · NS-2 foi arquivado.")
        );
        assert!(!f.rows().contains(&Fixture::anode(id)), "out of the tree");
        assert_eq!(
            f.app.selected,
            NodeId::Ticket(ticket),
            "the selection goes up"
        );
    }

    #[test]
    fn n_on_the_lost_conversation_opens_the_menu_of_its_stage_without_a_second_check() {
        let (mut f, mut launcher, id) = stopped_agent();
        launcher.conversation = Ok(false);
        launcher.check = Ok(Precheck {
            jail: true,
            jira: true,
            sentry: true,
            warning: None,
        });
        f.r(&mut launcher);
        assert_eq!(f.ch('n'), Action::None);
        assert_eq!(
            f.menu(),
            (vec![StageType::TechLeader, StageType::TechLeaderJira], 0)
        );
        assert_eq!(launcher.checks.len(), 2, "the check of the resume only");
        // From there, the flow of `g`: the form, the preview and a new agent.
        f.pick(StageType::TechLeader);
        assert_eq!(f.fill_and_enter(), Action::RenderPrompt);
        f.app.render_prompt(&mut launcher);
        let Action::OpenAgent {
            project,
            holder: ticket,
            kind,
            log,
            precheck,
        } = f.ch('s')
        else {
            panic!("no OpenAgent");
        };
        assert_eq!(ticket.key, "NS-2");
        f.app
            .open_agent(&project, &ticket, kind, &log, precheck, &mut launcher);
        assert_eq!(launcher.checks.len(), 2, "no other check");
        assert_eq!(f.app.agents().len(), 1);
        assert_ne!(f.app.agents()[0].id, id, "a new agent");
        assert_eq!(f.state_of(id), "stopped", "the old one stays");
    }

    #[test]
    fn the_menu_of_the_lost_conversation_follows_the_stage_and_the_flags() {
        let cases = [
            (
                0,
                true,
                true,
                vec![
                    StageType::Po,
                    StageType::PoDiscussion,
                    StageType::PoJira,
                    StageType::PoSentry,
                ],
            ),
            (
                0,
                false,
                false,
                vec![StageType::Po, StageType::PoDiscussion],
            ),
            (
                0,
                false,
                true,
                vec![StageType::Po, StageType::PoDiscussion, StageType::PoSentry],
            ),
            (2, true, true, vec![StageType::Development]),
            (3, true, true, vec![StageType::Review]),
        ];
        for (downs, jira, sentry, expected) in cases {
            let (mut f, _) = with_agents();
            let mut launcher = FakeLauncher::new(false);
            f.open_agent(downs, &mut launcher);
            let id = launcher.ids[0];
            f.app.agent_ended(id, 0);
            launcher.conversation = Ok(false);
            launcher.check = Ok(Precheck {
                jail: false,
                jira,
                sentry,
                warning: None,
            });
            f.r(&mut launcher);
            f.ch('n');
            assert_eq!(
                f.menu().0,
                expected,
                "stage {downs}, jira {jira}, sentry {sentry}"
            );
        }
    }

    #[test]
    fn n_on_the_lost_conversation_of_a_hidden_ticket_says_to_restore_it() {
        let (mut f, mut launcher, id) = stopped_agent();
        let ticket = owner_ticket(f.app.agent_record(id).unwrap().owner);
        let t = f.app.db.get_ticket(ticket).unwrap().unwrap();
        tickets::hide(&f.app.db, &t).unwrap();
        f.ch('f');
        f.select(Fixture::anode(id));
        launcher.conversation = Ok(false);
        assert!(
            f.r(&mut launcher),
            "r works on the agent of a hidden ticket"
        );
        assert_eq!(f.ch('n'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(
            f.status(),
            Some("O ticket NS-2 está oculto. Restaure-o (o) para abrir um agente.")
        );
    }

    #[test]
    fn the_agent_of_a_hidden_ticket_is_resumed() {
        let (mut f, mut launcher, id) = stopped_agent();
        let ticket = owner_ticket(f.app.agent_record(id).unwrap().owner);
        let t = f.app.db.get_ticket(ticket).unwrap().unwrap();
        tickets::hide(&f.app.db, &t).unwrap();
        f.ch('f');
        f.select(Fixture::anode(id));
        assert!(f.r(&mut launcher));
        assert_eq!(f.state_of(id), "active");
        assert_eq!(f.app.focus, Focus::Terminal);
    }

    #[test]
    fn an_agent_active_in_another_nick_is_refused_without_the_check() {
        let (mut f, _) = with_agents();
        let id = f.foreign_agent("NS-1", "outro-1");
        let lock = f.agents_dir().join("outro-1.lock");
        let mut child = agents::test_support::hold_lock(&lock);
        let mut launcher = FakeLauncher::new(false);
        assert!(!f.r(&mut launcher), "refused at the key");
        assert_eq!(
            f.status(),
            Some(
                "O agente proj · Review · NS-1 está ativo em outro nick e não pode ser retomado aqui."
            )
        );
        assert!(launcher.checks.is_empty(), "no check");
        assert_eq!(f.state_of(id), "active");
        // `o` refuses it too.
        f.ch('o');
        assert_eq!(f.status(), Some("Um agente ativo não pode ser arquivado."));
        assert_eq!(f.state_of(id), "active");
        child.kill().unwrap();
        child.wait().unwrap();
    }

    #[test]
    fn an_active_agent_whose_lock_is_free_is_resumed() {
        let (mut f, _) = with_agents();
        let id = f.foreign_agent("NS-1", "outro-1");
        let mut launcher = FakeLauncher::new(false);
        assert!(f.r(&mut launcher));
        assert_eq!(
            launcher.resumes,
            [(Stage::Review, Some("NS-1".to_string()), "NS-1".to_string())]
        );
        assert_eq!(f.state_of(id), "active");
        assert!(f.app.agent(id).is_some());
        assert_eq!(f.app.focus, Focus::Terminal);
    }

    #[test]
    fn a_resume_after_the_id_changed_tells_the_agent_the_new_folder() {
        let (mut f, mut launcher, id) = stopped_agent();
        let ticket = owner_ticket(f.app.agent_record(id).unwrap().owner);
        let t = f.app.db.get_ticket(ticket).unwrap().unwrap();
        let p = f.app.selected_project().unwrap().path.clone();
        tickets::edit_id(&f.app.db, &p, &t, "NS-9").unwrap();
        f.app.refresh_tree();
        assert!(f.r(&mut launcher));
        assert_eq!(
            launcher.resumes,
            [(
                Stage::TechLeader,
                Some("NS-2".to_string()),
                "NS-9".to_string()
            )]
        );
        assert_eq!(launcher.spawns[1].3, TL_NAME, "the name does not change");
        assert_eq!(
            f.status(),
            Some(
                "O agente proj · Tech Leader · NS-2 foi retomado. Ele foi avisado de que a \
                 pasta agora é .nick/docs/NS-9/."
            )
        );
    }

    #[test]
    fn an_agent_of_before_the_ns39_takes_its_key_from_the_name() {
        let (mut f, mut launcher, id) = stopped_agent();
        f.app
            .db
            .connection()
            .execute("UPDATE agents SET opened_key = NULL WHERE id = ?1", [id])
            .unwrap();
        f.app.refresh_tree();
        assert!(f.r(&mut launcher));
        assert_eq!(
            launcher.resumes,
            [(
                Stage::TechLeader,
                Some("NS-2".to_string()),
                "NS-2".to_string()
            )]
        );
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · NS-2 foi retomado.")
        );
    }

    #[test]
    fn a_failing_spawn_on_resume_stops_the_agent_again_and_releases_the_lock() {
        let (mut f, mut launcher, id) = stopped_agent();
        launcher.spawn = Err("sem pty".into());
        assert!(f.r(&mut launcher));
        assert_eq!(f.status(), Some("Não consegui abrir o agente: sem pty."));
        assert_eq!(f.state_of(id), "stopped");
        assert!(f.app.agent(id).is_none());
        let session = f.session_of(id);
        assert!(!f.agents_dir().join(format!("{session}.lock")).exists());
    }

    #[test]
    fn r_on_an_archived_agent_and_on_an_unavailable_project_says_why() {
        let (mut f, mut launcher, id) = stopped_agent();
        f.ch('o');
        f.ch('s');
        assert_eq!(f.state_of(id), "archived");
        f.ch('f');
        f.select(Fixture::anode(id));
        assert!(!f.r(&mut launcher));
        assert_eq!(
            f.status(),
            Some(
                "O agente proj · Tech Leader · NS-2 está arquivado. Desarquive (o) antes de retomar."
            )
        );
        f.ch('o');
        assert_eq!(f.state_of(id), "stopped");
        let p = f.app.selected_project().unwrap().path.clone();
        fs::remove_file(projects::config_file(&p)).unwrap();
        assert!(!f.r(&mut launcher));
        assert_eq!(
            f.status(),
            Some(
                "Projeto indisponível: [indisponível: sem .nick/nick.conf]. O agente só abre com \
                 a pasta e o .nick/nick.conf no lugar."
            )
        );
        assert_eq!(launcher.checks.len(), 1, "no check");
    }

    #[test]
    fn o_archives_a_stopped_agent_asking_and_the_selection_goes_to_the_sibling() {
        let (mut f, mut launcher, first) = stopped_agent();
        // A second agent of the same ticket, ended too.
        f.on_ticket();
        f.open_kind(StageType::TechLeader, &mut launcher);
        let second = launcher.ids[1];
        f.app.agent_ended(second, 0);
        f.select(Fixture::anode(first));
        assert_eq!(f.ch('o'), Action::None);
        assert_eq!(
            f.app.dialog,
            Some(Dialog::ConfirmArchive {
                agent: f.app.agent_record(first).unwrap().clone()
            })
        );
        f.ch('n');
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.state_of(first), "stopped", "any other key cancels");
        f.ch('o');
        f.ch('s');
        assert_eq!(f.state_of(first), "archived");
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · NS-2 foi arquivado.")
        );
        assert_eq!(f.app.selected, Fixture::anode(second), "the next sibling");
        assert!(!f.rows().contains(&Fixture::anode(first)));
        // With `f`, the archived one is back on the tree; `o` unarchives it without asking.
        f.ch('f');
        assert!(f.rows().contains(&Fixture::anode(first)));
        f.select(Fixture::anode(first));
        assert_eq!(f.ch('o'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.state_of(first), "stopped");
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · NS-2 voltou para parado.")
        );
    }

    #[test]
    fn o_on_a_live_agent_is_refused() {
        let (mut f, _launcher) = running();
        let id = f.app.agents()[0].id;
        f.shortcut();
        assert_eq!(f.app.selected, Fixture::anode(id));
        assert_eq!(f.ch('o'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), Some("Um agente ativo não pode ser arquivado."));
        assert_eq!(f.state_of(id), "active");
    }

    #[test]
    fn enter_on_a_stopped_or_an_archived_agent_says_which_key() {
        let (mut f, _launcher, id) = stopped_agent();
        for open in [KeyCode::Enter, KeyCode::Right] {
            f.key(open);
            assert_eq!(
                f.status(),
                Some("O agente proj · Tech Leader · NS-2 está parado. Pressione r para retomar.")
            );
        }
        f.ch('o');
        f.ch('s');
        f.ch('f');
        f.select(Fixture::anode(id));
        f.key(KeyCode::Enter);
        assert_eq!(
            f.status(),
            Some(
                "O agente proj · Tech Leader · NS-2 está arquivado. Pressione o para desarquivar."
            )
        );
        assert_eq!(f.app.focus, Focus::List);
    }

    /// Opening the nick resumes nothing: the stopped agents stay stopped, and nothing reaches a
    /// launcher (the App runs none by itself: only `Action::CheckResume` does, from `r`).
    #[test]
    fn opening_the_app_with_stopped_agents_resumes_none() {
        let (f, _launcher, id) = stopped_agent();
        let agents_dir = f.agents_dir();
        // The nick closes (its App goes) and opens again on the same database.
        let Fixture { _tmp, root, app } = f;
        drop(app);
        let db = Db::open(&root.join("nick.db")).unwrap();
        assert_eq!(agents::cleanup(&db, &agents_dir), Ok(0));
        let mut app = App::new(db, None, None, root.join("bin/nick-init"))
            .with_agents(root.join("bin/nick-agent"), agents_dir);
        assert!(app.agents().is_empty());
        // Every node of the tree, opened and walked: no action asks for a launch.
        for _ in 0..12 {
            for code in [KeyCode::Enter, KeyCode::Down] {
                let action = app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
                assert_eq!(action, Action::None);
            }
        }
        assert!(app.agents().is_empty());
        assert_eq!(app.db.get_agent(id).unwrap().unwrap().state, "stopped");
    }

    /// Review of the NS-39, point 2: the node still says stopped (the tree was read before),
    /// but another nick resumed it meanwhile: `r` reads the row again and refuses before the
    /// check.
    #[test]
    fn r_reads_the_row_again_and_refuses_one_another_nick_resumed_meanwhile() {
        let (mut f, mut launcher, id) = stopped_agent();
        assert_eq!(
            f.app.agent_record(id).unwrap().state,
            "stopped",
            "the tree of before"
        );
        f.app
            .db
            .connection()
            .execute("UPDATE agents SET state = 'active' WHERE id = ?1", [id])
            .unwrap();
        let session = f.session_of(id);
        let mut child =
            agents::test_support::hold_lock(&f.agents_dir().join(format!("{session}.lock")));
        assert!(!f.r(&mut launcher), "refused at the key");
        assert_eq!(
            f.status(),
            Some(
                "O agente proj · Tech Leader · NS-2 está ativo em outro nick e não pode ser \
                 retomado aqui."
            )
        );
        assert_eq!(launcher.checks.len(), 1, "no check of the resume");
        assert_eq!(f.state_of(id), "active");
        child.kill().unwrap();
        child.wait().unwrap();
    }

    /// Review of the NS-39, point 4: a resume refused in the loop (the row changed after the
    /// key) reads the tree again, so the node shows the state the bar talks about.
    #[test]
    fn a_resume_refused_in_the_loop_reads_the_tree_again() {
        let (mut f, mut launcher, id) = stopped_agent();
        let p = f.app.selected_project().unwrap().path.clone();
        let agent = f.app.agent_record(id).unwrap().clone();
        f.app.db.archive_agent(id).unwrap();
        f.app.check_resume(&p, &agent, &mut launcher);
        assert_eq!(
            f.status(),
            Some(
                "O agente proj · Tech Leader · NS-2 está arquivado. Desarquive (o) antes de retomar."
            )
        );
        assert_eq!(
            f.app.agent_record(id).unwrap().state,
            "archived",
            "read again"
        );
        assert!(
            !f.rows().contains(&Fixture::anode(id)),
            "out of the tree without f"
        );
    }

    /// Review of the NS-39, point 6: a state this nick does not know is not offered as
    /// stopped: `r` and `o` say so, and nothing is run.
    #[test]
    fn an_agent_in_an_unknown_state_is_neither_resumed_nor_archived() {
        let (mut f, mut launcher, id) = stopped_agent();
        f.app
            .db
            .connection()
            .execute("UPDATE agents SET state = 'futuro' WHERE id = ?1", [id])
            .unwrap();
        f.app.refresh_tree();
        let message = "O agente proj · Tech Leader · NS-2 está num estado que este nick não conhece (futuro).";
        assert!(!f.r(&mut launcher));
        assert_eq!(f.status(), Some(message));
        assert_eq!(launcher.checks.len(), 1, "no check");
        assert_eq!(f.ch('o'), Action::None);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), Some(message));
        assert_eq!(f.state_of(id), "futuro");
    }

    // ---- the epics (NS-40) ----

    impl Fixture {
        /// Creates an epic by the dialog on the node of the project of the selection: `n`, the
        /// title, Tab, the ID, Enter.
        fn new_epic(&mut self, title: &str, jira: &str) -> Action {
            self.on_project();
            self.ch('n');
            self.type_text(title);
            self.key(KeyCode::Tab);
            self.type_text(jira);
            self.key(KeyCode::Enter)
        }

        /// The epic `key` of the selected project.
        fn epic_of(&self, key: &str) -> Epic {
            let project = self.app.selected_project().expect("a project").id;
            self.app
                .project_epics(project)
                .iter()
                .find(|e| e.key == key)
                .cloned()
                .unwrap_or_else(|| panic!("no epic {key}"))
        }

        fn epic_node(&self, key: &str) -> NodeId {
            NodeId::Epic(self.epic_of(key).id)
        }

        fn ticket_of(&self, key: &str) -> Ticket {
            self.app
                .tickets
                .values()
                .flatten()
                .find(|t| t.key == key)
                .cloned()
                .unwrap_or_else(|| panic!("no ticket {key}"))
        }
    }

    /// `proj` with the tickets NS-1 and NS-2, the agents on (`with_agents`) and the epic EPIC-1
    /// selected.
    fn with_epic() -> (Fixture, PathBuf) {
        let (mut f, p) = with_agents();
        assert_eq!(f.new_epic("Plano longo", "EPIC-1"), Action::None);
        (f, p)
    }

    #[test]
    fn n_on_the_project_creates_an_epic_and_selects_it() {
        let (mut f, _) = with_tickets("proj");
        f.ch('n');
        assert_eq!(
            f.app.dialog,
            Some(Dialog::NewTicket {
                project_id: f.pid("proj"),
                item: NewItem::Epic,
                title: String::new(),
                jira_id: String::new(),
                focus: TicketField::Title,
            })
        );
        // Esc writes nothing.
        f.key(KeyCode::Esc);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.app.project_epics(f.pid("proj")), []);
        // The refusals keep the dialog open.
        f.ch('n');
        f.key(KeyCode::Enter);
        assert_eq!(f.status(), Some("Informe o título do épico."));
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::NewTicket {
                item: NewItem::Epic,
                ..
            })
        ));
        f.key(KeyCode::Esc);
        f.new_ticket("t", "NS-1");
        assert_eq!(f.new_epic("e", "NS-1"), Action::None);
        assert_eq!(
            f.status(),
            Some("Já existe um ticket com o ID NS-1 neste projeto.")
        );
        assert!(f.app.dialog.is_some());
        f.key(KeyCode::Esc);
        assert_eq!(f.new_epic("Migrar", "EPIC-1"), Action::None);
        assert_eq!(f.status(), Some("Épico criado: EPIC-1 — Migrar."));
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.app.selected, f.epic_node("EPIC-1"));
        // The epics before the tickets without epic.
        let at = |id: NodeId| f.rows().iter().position(|r| *r == id).unwrap();
        assert!(at(f.epic_node("EPIC-1")) < at(f.tnode("NS-1")));
        // A provisional one.
        f.new_epic("Sem ID", "");
        let message = f.status().unwrap().to_string();
        assert!(
            message.starts_with("Épico criado com o ID provisório "),
            "{message}"
        );
    }

    #[test]
    fn a_on_the_epic_creates_a_ticket_in_it_and_selects_it_with_the_branch_open() {
        let (mut f, _) = with_epic();
        let epic = f.epic_of("EPIC-1");
        f.ch('a');
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::NewTicket { item: NewItem::Ticket { epic_id: Some(id) }, .. }) if id == epic.id
        ));
        f.type_text("Dentro");
        f.key(KeyCode::Tab);
        f.type_text("NS-5");
        assert_eq!(f.key(KeyCode::Enter), Action::None);
        assert_eq!(f.status(), Some("Ticket criado: NS-5 — Dentro."));
        let ticket = f.ticket_of("NS-5");
        assert_eq!(ticket.epic_id, Some(epic.id));
        assert_eq!(f.app.selected, NodeId::Ticket(ticket.id));
        assert!(f.app.tree.is_open(NodeId::Epic(epic.id)));
        let depth = f
            .app
            .visible()
            .iter()
            .find(|r| r.id == f.app.selected)
            .unwrap()
            .depth;
        assert_eq!(depth, 3, "under the epic");
        // In a hidden epic, refused.
        f.app.select(NodeId::Epic(epic.id));
        epics::hide(&f.app.db, &epic).unwrap();
        f.ch('f');
        f.app.refresh_tree();
        f.app.select(NodeId::Epic(epic.id));
        f.ch('a');
        assert_eq!(f.app.dialog, None);
        assert_eq!(
            f.status(),
            Some("O épico EPIC-1 está oculto. Restaure-o (o) para criar um ticket.")
        );
    }

    #[test]
    fn e_and_i_on_the_epic_with_their_refusals() {
        let (mut f, p) = with_epic();
        let node = f.epic_node("EPIC-1");
        f.ch('e');
        assert!(matches!(f.app.dialog, Some(Dialog::EditEpicTitle { .. })));
        for _ in 0.."Plano longo".len() {
            f.key(KeyCode::Backspace);
        }
        f.key(KeyCode::Enter);
        assert_eq!(f.status(), Some("Informe o título do épico."));
        f.type_text("Novo");
        f.key(KeyCode::Enter);
        assert_eq!(f.status(), Some("Título atualizado: EPIC-1."));
        assert_eq!(f.epic_of("EPIC-1").title, "Novo");
        // `i`: the ID of a ticket is refused; then a new one renames the folder.
        f.app.select(node);
        f.ch('i');
        let erase = |f: &mut Fixture| {
            for _ in 0..10 {
                f.key(KeyCode::Backspace);
            }
        };
        erase(&mut f);
        f.type_text("NS-1");
        f.key(KeyCode::Enter);
        assert_eq!(
            f.status(),
            Some("Já existe um ticket com o ID NS-1 neste projeto.")
        );
        assert!(matches!(f.app.dialog, Some(Dialog::EditEpicId { .. })));
        erase(&mut f);
        f.type_text("EPIC-2");
        fs::create_dir_all(p.join(".nick/docs/EPIC-1")).unwrap();
        f.key(KeyCode::Enter);
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::ConfirmEditEpicId { moves: true, .. })
        ));
        f.ch('s');
        assert_eq!(
            f.status(),
            Some(
                "ID atualizado: EPIC-1 → EPIC-2. A pasta .nick/docs/EPIC-1/ virou .nick/docs/EPIC-2/."
            )
        );
        assert!(p.join(".nick/docs/EPIC-2").is_dir());
        // A ticket with the ID of the epic (the other way).
        f.app.select(f.tnode("NS-1"));
        f.ch('i');
        erase(&mut f);
        f.type_text("EPIC-2");
        f.key(KeyCode::Enter);
        assert_eq!(
            f.status(),
            Some("Já existe um épico com o ID EPIC-2 neste projeto.")
        );
        f.key(KeyCode::Esc);
        // The project unavailable: `i` refused at the key.
        f.app.select(f.epic_node("EPIC-2"));
        fs::remove_file(p.join(".nick/nick.conf")).unwrap();
        f.ch('i');
        assert_eq!(f.app.dialog, None);
        assert!(
            f.status().unwrap().starts_with("Projeto indisponível: "),
            "{:?}",
            f.status()
        );
    }

    #[test]
    fn i_on_the_epic_is_refused_with_an_active_agent_of_the_epic() {
        let (mut f, _) = with_epic();
        let epic = f.epic_of("EPIC-1");
        let dir = f.agents_dir();
        let _agent = agents::open(
            &f.app.db,
            &dir,
            Owner::Epic(epic.id),
            Stage::Po,
            "s-other",
            "n",
            "EPIC-1",
        )
        .unwrap();
        f.app.select(NodeId::Epic(epic.id));
        f.ch('i');
        f.key(KeyCode::Backspace);
        f.type_text("9");
        f.key(KeyCode::Enter);
        assert_eq!(
            f.status(),
            Some(
                "O épico EPIC-1 tem agente ativo. Encerre os agentes dele antes de editar o ID: o \
                 prompt deles cita a pasta .nick/docs/EPIC-1/."
            )
        );
    }

    #[test]
    fn o_on_the_epic_asks_hides_the_branch_and_restores_it() {
        let (mut f, _) = with_epic();
        let epic = f.epic_of("EPIC-1");
        f.ch('a');
        f.type_text("Dentro");
        f.key(KeyCode::Tab);
        f.type_text("NS-5");
        f.key(KeyCode::Enter);
        f.app.select(NodeId::Epic(epic.id));
        f.ch('o');
        assert_eq!(
            f.app.dialog,
            Some(Dialog::ConfirmHideEpic {
                epic: f.app.db.get_epic(epic.id).unwrap().unwrap(),
                tickets: 1
            })
        );
        f.ch('n');
        assert!(
            f.app
                .db
                .get_epic(epic.id)
                .unwrap()
                .unwrap()
                .hidden_at
                .is_none(),
            "only s"
        );
        f.ch('o');
        f.ch('s');
        assert_eq!(
            f.status(),
            Some(
                "Épico ocultado: EPIC-1. O ramo inteiro continua no banco e pode ser restaurado \
                 (f mostra os ocultos)."
            )
        );
        assert!(!f.rows().contains(&NodeId::Epic(epic.id)));
        assert!(
            !f.rows().contains(&f.tnode("NS-5")),
            "its tickets go with it"
        );
        assert_eq!(f.app.selected, f.tnode("NS-1"), "the next sibling");
        assert!(
            f.ticket_of("NS-5").hidden_at.is_none(),
            "the ticket is not hidden"
        );
        // `f` shows it, and `o` restores it at once.
        f.ch('f');
        f.app.select(NodeId::Epic(epic.id));
        f.ch('o');
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), Some("Épico restaurado: EPIC-1."));
        f.ch('f');
        assert!(f.rows().contains(&NodeId::Epic(epic.id)));
    }

    #[test]
    fn o_on_the_epic_with_active_agents_is_refused_with_the_count_and_no_dialog() {
        let (mut f, _) = with_epic();
        let epic = f.epic_of("EPIC-1");
        let ticket = f.ticket_of("NS-1");
        epics::move_ticket(&f.app.db, &ticket, Some(&epic)).unwrap();
        let dir = f.agents_dir();
        let _a = agents::open(
            &f.app.db,
            &dir,
            Owner::Ticket(ticket.id),
            Stage::Po,
            "s-a",
            "n",
            "NS-1",
        )
        .unwrap();
        let _b = agents::open(
            &f.app.db,
            &dir,
            Owner::Epic(epic.id),
            Stage::Po,
            "s-b",
            "n",
            "EPIC-1",
        )
        .unwrap();
        f.app.refresh_tree();
        f.app.select(NodeId::Epic(epic.id));
        f.ch('o');
        assert_eq!(f.app.dialog, None);
        assert_eq!(
            f.status(),
            Some(
                "O épico EPIC-1 tem 2 agentes ativos (no épico ou nos tickets dele). Encerre-os \
                 antes de ocultar."
            )
        );
    }

    #[test]
    fn g_on_the_epic_offers_only_po_and_tech_leader_and_opens_an_epic_agent() {
        let (mut f, p) = with_epic();
        let epic = f.epic_of("EPIC-1");
        for (jira, sentry, count) in [(false, false, 3), (true, true, 6)] {
            let mut launcher = FakeLauncher::new(true);
            if let Ok(check) = launcher.check.as_mut() {
                check.jira = jira;
                check.sentry = sentry;
            }
            f.app.select(NodeId::Epic(epic.id));
            f.g(&mut launcher);
            let (types, _) = f.menu();
            assert_eq!(types, StageType::epic_menu(jira, sentry));
            assert_eq!(types.len(), count);
            assert!(
                types
                    .iter()
                    .all(|t| matches!(t.stage(), Stage::Po | Stage::TechLeader))
            );
            f.key(KeyCode::Esc);
        }
        let mut launcher = FakeLauncher::new(true);
        f.app.select(NodeId::Epic(epic.id));
        f.g(&mut launcher);
        f.pick(StageType::TechLeader);
        assert_eq!(
            f.form().1[0],
            "EPIC-1",
            "the form of the TL with the ID of the epic"
        );
        assert_eq!(f.fill_and_enter(), Action::RenderPrompt);
        assert_eq!(launcher.renders.len(), 0);
        f.app.render_prompt(&mut launcher);
        assert_eq!(
            launcher.renders[0].2.answers[0],
            ("--jira-id", "EPIC-1".to_string())
        );
        let Action::OpenAgent {
            project,
            holder,
            kind,
            log,
            precheck,
        } = f.ch('s')
        else {
            panic!("no OpenAgent");
        };
        assert_eq!(project, p);
        assert_eq!(holder, Holder::from(&epic));
        f.app
            .open_agent(&project, &holder, kind, &log, precheck, &mut launcher);
        let id = launcher.ids[0];
        let row = f.app.db.get_agent(id).unwrap().unwrap();
        assert_eq!(row.owner, Owner::Epic(epic.id));
        assert_eq!(row.name, "proj · Tech Leader · EPIC-1");
        assert_eq!(row.opened_key.as_deref(), Some("EPIC-1"));
        assert_eq!(launcher.spawns[0].3, "proj · Tech Leader · EPIC-1");
        assert_eq!(f.app.selected, NodeId::Agent(id));
        assert_eq!(f.app.focus, Focus::Terminal);
        let at = |id: NodeId| f.app.visible().iter().find(|r| r.id == id).unwrap().depth;
        assert_eq!(at(NodeId::Agent(id)), 3, "under the epic");
        assert_eq!(f.app.agents[0].owner, Owner::Epic(epic.id));
    }

    #[test]
    fn g_on_a_hidden_epic_and_on_a_ticket_of_one_is_refused() {
        let (mut f, _) = with_epic();
        let epic = f.epic_of("EPIC-1");
        let ticket = f.ticket_of("NS-1");
        epics::move_ticket(&f.app.db, &ticket, Some(&epic)).unwrap();
        epics::hide(&f.app.db, &epic).unwrap();
        f.ch('f');
        f.app.refresh_tree();
        let message = "O épico EPIC-1 está oculto. Restaure-o (o) para abrir um agente.";
        f.app.select(NodeId::Epic(epic.id));
        assert_eq!(f.ch('g'), Action::None);
        assert_eq!(f.status(), Some(message));
        f.app.select(NodeId::Ticket(ticket.id));
        assert_eq!(f.ch('g'), Action::None);
        assert_eq!(f.status(), Some(message));
        // Another nick hid the epic after this one read the tree: `g` reads it again.
        let (mut f, _) = with_epic();
        let epic = f.epic_of("EPIC-1");
        epics::hide(&f.app.db, &epic).unwrap();
        assert_eq!(f.ch('g'), Action::None);
        assert_eq!(f.status(), Some(message));
    }

    #[test]
    fn p_on_a_ticket_moves_it_with_the_list_and_touches_no_file() {
        let (mut f, p) = with_epic();
        f.new_epic("Outro", "EPIC-2");
        f.new_epic("Escondido", "EPIC-3");
        let hidden = f.epic_of("EPIC-3");
        epics::hide(&f.app.db, &hidden).unwrap();
        f.app.refresh_tree();
        fs::create_dir_all(p.join(".nick/docs/NS-1")).unwrap();
        fs::create_dir_all(p.join(".nick/docs/EPIC-1")).unwrap();
        let before = crate::projects::snapshot::snapshot(&p);
        f.app.select(f.tnode("NS-1"));
        f.ch('p');
        let Some(Dialog::MoveTicket { choices, index, .. }) = f.app.dialog.clone() else {
            panic!("no list: {:?}", f.app.dialog);
        };
        let keys: Vec<Option<String>> = choices
            .iter()
            .map(|c| c.as_ref().map(|e| e.key.clone()))
            .collect();
        assert_eq!(
            keys,
            [None, Some("EPIC-1".into()), Some("EPIC-2".into())],
            "Sem épico first, then the visible epics"
        );
        assert_eq!(index, 0, "the current place");
        // Enter on the current place: refused, the list stays.
        f.key(KeyCode::Enter);
        assert_eq!(f.status(), Some("O ticket NS-1 já está aí."));
        assert!(f.app.dialog.is_some());
        f.key(KeyCode::Down);
        f.key(KeyCode::Enter);
        assert_eq!(f.app.dialog, None);
        assert_eq!(f.status(), Some("Ticket NS-1 movido para o épico EPIC-1."));
        assert_eq!(f.ticket_of("NS-1").epic_id, Some(f.epic_of("EPIC-1").id));
        assert_eq!(f.app.selected, f.tnode("NS-1"), "it stays selected");
        // Now in EPIC-1: the list starts there; out of the epic.
        f.ch('p');
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::MoveTicket { index: 1, .. })
        ));
        f.key(KeyCode::Up);
        f.key(KeyCode::Enter);
        assert_eq!(f.status(), Some("Ticket NS-1 tirado do épico EPIC-1."));
        assert_eq!(
            crate::projects::snapshot::snapshot(&p),
            before,
            "no file touched"
        );
    }

    #[test]
    fn p_without_epics_says_how_to_create_one() {
        let (mut f, _) = with_agents();
        f.app.select(f.tnode("NS-1"));
        f.ch('p');
        assert_eq!(f.app.dialog, None);
        assert_eq!(
            f.status(),
            Some("O projeto não tem épicos. Crie um com n no nó do projeto.")
        );
    }

    /// An agent of the epic EPIC-1 (Tech Leader), opened and ended: stopped, its node selected.
    fn stopped_epic_agent() -> (Fixture, FakeLauncher, i64) {
        let (mut f, _) = with_epic();
        let mut launcher = FakeLauncher::new(true);
        f.open_kind(StageType::TechLeader, &mut launcher);
        let id = launcher.ids[0];
        f.app.agent_ended(id, 0);
        assert_eq!(f.app.selected, NodeId::Agent(id));
        f.app.status = None;
        (f, launcher, id)
    }

    #[test]
    fn an_epic_agent_is_stopped_resumed_with_its_epic_and_archived() {
        let (mut f, mut launcher, id) = stopped_epic_agent();
        assert_eq!(f.state_of(id), "stopped");
        assert!(f.r(&mut launcher));
        assert_eq!(f.state_of(id), "active");
        assert_eq!(
            launcher.resumes,
            [(
                Stage::TechLeader,
                Some("EPIC-1".to_string()),
                "EPIC-1".to_string()
            )]
        );
        assert_eq!(launcher.resume_epics, [true], "--epic-id, not --ticket-id");
        assert_eq!(
            f.status(),
            Some("O agente proj · Tech Leader · EPIC-1 foi retomado.")
        );
        assert_eq!(f.app.focus, Focus::Terminal);
        // `x` ends it again, and `o` archives it.
        f.shortcut();
        assert_eq!(f.ch('x'), Action::None);
        assert_eq!(f.ch('s'), Action::EndAgent(id));
        f.app.agent_ended_by_operator(id);
        assert_eq!(f.state_of(id), "stopped");
        f.ch('o');
        f.ch('s');
        assert_eq!(f.state_of(id), "archived");
    }

    #[test]
    fn the_lost_conversation_of_an_epic_agent_offers_only_the_types_of_its_stage() {
        let (mut f, mut launcher, _) = stopped_epic_agent();
        launcher.conversation = Ok(false);
        if let Ok(check) = launcher.check.as_mut() {
            check.jira = true;
        }
        f.r(&mut launcher);
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::ConversationLost { .. })
        ));
        f.ch('n');
        let (types, _) = f.menu();
        assert_eq!(types, [StageType::TechLeader, StageType::TechLeaderJira]);
    }

    #[test]
    fn a_resume_after_the_id_of_the_epic_changed_tells_the_new_folder() {
        let (mut f, mut launcher, id) = stopped_epic_agent();
        let epic = f.app.db.list_epics(f.pid("proj"), true).unwrap()[0].clone();
        let p = f.root.join("proj");
        epics::edit_id(&f.app.db, &p, &epic, "EPIC-9").unwrap();
        f.app.refresh_tree();
        f.app.select(NodeId::Agent(id));
        assert!(f.r(&mut launcher));
        assert_eq!(
            launcher.resumes,
            [(
                Stage::TechLeader,
                Some("EPIC-1".to_string()),
                "EPIC-9".to_string()
            )]
        );
        assert_eq!(launcher.resume_epics, [true]);
        assert_eq!(
            f.status(),
            Some(
                "O agente proj · Tech Leader · EPIC-1 foi retomado. Ele foi avisado de que a pasta \
                 agora é .nick/docs/EPIC-9/."
            )
        );
    }

    #[test]
    fn the_removal_of_a_project_counts_its_epics_and_their_agents() {
        let (mut f, _) = with_epic();
        let (mut launcher, epic) = (FakeLauncher::new(true), f.epic_of("EPIC-1"));
        f.open_kind(StageType::PoDiscussion, &mut launcher);
        let id = launcher.ids[0];
        f.app.agent_ended(id, 0);
        f.app.select(NodeId::Project(epic.project_id));
        f.ch('r');
        assert!(
            matches!(
                f.app.dialog,
                Some(Dialog::ConfirmRemove {
                    epics: 1,
                    agents: 1,
                    tickets: 2,
                    ..
                })
            ),
            "{:?}",
            f.app.dialog
        );
        // Without epics: 0, and the text of before (ui.rs).
        let (mut f, _) = with_agents();
        f.on_project();
        f.ch('r');
        assert!(matches!(
            f.app.dialog,
            Some(Dialog::ConfirmRemove { epics: 0, .. })
        ));
    }

    // ---- the capital letters on the shortcuts (NS-43) ----

    /// One dialog of each variant (`Dialog::takes_text`), with the values of a fixture with a
    /// project, a ticket, an epic and a stopped agent.
    pub(crate) fn one_of_each_dialog() -> Vec<Dialog> {
        let (mut f, _, id) = stopped_agent();
        assert_eq!(f.new_epic("Plano longo", "EPIC-1"), Action::None);
        let project = f.app.rows[0].project.clone();
        let path = project.path.clone();
        let ticket = f.ticket_of("NS-1");
        let epic = f.epic_of("EPIC-1");
        let agent = f.app.agent_record(id).expect("the stopped agent").clone();
        let holder: Holder = (&ticket).into();
        let precheck = Precheck {
            jail: false,
            jira: false,
            sentry: false,
            warning: None,
        };
        let kind = StageType::PoDiscussion;
        vec![
            Dialog::AddPath {
                input: String::new(),
            },
            Dialog::NeedsConfig {
                dir: path.clone(),
                focus: Button::CreateConfig,
                note: None,
            },
            Dialog::ConfirmRemove {
                project,
                tickets: 2,
                agents: 1,
                live: 0,
                epics: 1,
            },
            Dialog::NewTicket {
                project_id: ticket.project_id,
                item: NewItem::Epic,
                title: String::new(),
                jira_id: String::new(),
                focus: TicketField::Title,
            },
            Dialog::EditTitle {
                ticket: ticket.clone(),
                input: String::new(),
            },
            Dialog::ConfirmHide {
                ticket: ticket.clone(),
            },
            Dialog::EditEpicTitle {
                epic: epic.clone(),
                input: String::new(),
            },
            Dialog::ConfirmHideEpic {
                epic: epic.clone(),
                tickets: 0,
            },
            Dialog::EditEpicId {
                epic: epic.clone(),
                input: String::new(),
            },
            Dialog::ConfirmEditEpicId {
                epic: epic.clone(),
                new: "EPIC-2".into(),
                moves: false,
            },
            Dialog::MoveTicket {
                ticket: ticket.clone(),
                choices: vec![None, Some(epic)],
                index: 0,
            },
            Dialog::ChooseType {
                project: path.clone(),
                holder: holder.clone(),
                precheck: precheck.clone(),
                types: vec![kind],
                index: 0,
            },
            Dialog::StageForm {
                project: path.clone(),
                holder: holder.clone(),
                precheck: precheck.clone(),
                kind,
                values: kind.initial_values(&holder.key),
                focus: 0,
                plan_missing: false,
            },
            Dialog::ConfirmPrompt {
                project: path.clone(),
                holder: holder.clone(),
                precheck: precheck.clone(),
                kind,
                rendered: rendered(1),
                scroll: 0,
            },
            Dialog::AgentError {
                message: "Nada foi aberto.".into(),
            },
            Dialog::ConfirmEndAgent {
                id,
                name: agent.name.clone(),
            },
            Dialog::ConversationLost {
                project: path,
                agent: agent.clone(),
                holder,
                precheck,
            },
            Dialog::ConfirmArchive { agent },
            Dialog::ConfirmQuit,
            Dialog::CaptureShortcut { first: None },
            Dialog::ChooseTicketStage {
                ticket: ticket.clone(),
                index: 0,
            },
            Dialog::ConfirmStageChange {
                ticket: ticket.clone(),
                to: TicketStage::Done,
            },
            Dialog::EditId {
                ticket: ticket.clone(),
                input: String::new(),
            },
            Dialog::ConfirmEditId {
                ticket,
                new: "NS-10".into(),
                moves: false,
            },
        ]
    }

    /// The name of the variant of a dialog (`AddPath`, `ConfirmQuit`, ...).
    pub(crate) fn variant(dialog: &Dialog) -> String {
        format!("{dialog:?}")
            .split([' ', '{', '('])
            .next()
            .unwrap_or_default()
            .to_string()
    }

    #[test]
    fn only_the_seven_dialogs_with_a_text_field_take_text() {
        let dialogs = one_of_each_dialog();
        let text: Vec<String> = dialogs
            .iter()
            .filter(|dialog| dialog.takes_text())
            .map(variant)
            .collect();
        assert_eq!(
            text,
            [
                "AddPath",
                "NewTicket",
                "EditTitle",
                "EditEpicTitle",
                "EditEpicId",
                "StageForm",
                "EditId"
            ]
        );
        // Every variant is in the list (the 24 of the enum), each once.
        let mut names: Vec<String> = dialogs.iter().map(variant).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 24);
        assert_eq!(dialogs.len(), 24);
    }

    #[test]
    fn only_the_removal_of_a_project_is_irreversible() {
        let dialogs = one_of_each_dialog();
        let irreversible: Vec<String> = dialogs
            .iter()
            .filter(|dialog| dialog.irreversible())
            .map(variant)
            .collect();
        assert_eq!(irreversible, ["ConfirmRemove"]);
        // Every variant is in the list (the 24 of the enum), each once.
        let mut names: Vec<String> = dialogs.iter().map(variant).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 24);
        assert_eq!(dialogs.len(), 24);
    }

    /// A letter as the terminal sends it: Caps Lock gives the capital without modifiers, Shift
    /// the capital with `SHIFT`.
    fn letter(c: char, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), modifiers)
    }

    impl Fixture {
        /// `text` as the terminal sends it with Shift: each capital letter with `SHIFT`.
        fn type_shifted(&mut self, text: &str) {
            for c in text.chars() {
                let modifiers = if c.is_uppercase() {
                    KeyModifiers::SHIFT
                } else {
                    KeyModifiers::NONE
                };
                assert_eq!(self.mods(KeyCode::Char(c), modifiers), Action::None, "{c}");
            }
        }

        /// `text` without what differs between two fixtures built the same way: the temporary
        /// folder, the timestamps and the session ids.
        fn scrub(&self, text: String) -> String {
            let mut volatile = Vec::new();
            let conn = self.app.db.connection();
            for sql in [
                "SELECT created_at FROM projects",
                "SELECT created_at FROM tickets",
                "SELECT hidden_at FROM tickets",
                "SELECT created_at FROM epics",
                "SELECT hidden_at FROM epics",
                "SELECT session_id FROM agents",
                "SELECT opened_at FROM agents",
            ] {
                let mut stmt = conn.prepare(sql).unwrap();
                let values = stmt
                    .query_map([], |row| row.get::<_, Option<String>>(0))
                    .unwrap()
                    .map(Result::unwrap);
                volatile.extend(values.flatten());
            }
            let mut text = text.replace(self.root.to_str().unwrap(), "<root>");
            for value in volatile {
                text = text.replace(&value, "<v>");
            }
            text
        }

        /// What the keys of the panel change, as text (`scrub`): the dialog, the bar, the
        /// screen, the selection, the focus and the filters of the tree.
        fn state(&self) -> String {
            let app = &self.app;
            self.scrub(format!(
                "{:?} | {:?} | {:?} | {:?} | {:?} | hidden {} | active {}",
                app.dialog,
                app.status,
                app.screen,
                app.selected,
                app.focus,
                app.tree.show_hidden,
                app.tree.only_active
            ))
        }

        /// The action of `key` and the `state` after it.
        fn outcome(&mut self, key: KeyEvent) -> String {
            let action = self.app.handle_key(key);
            format!("{} | {}", self.scrub(format!("{action:?}")), self.state())
        }

        /// The text field being typed in.
        fn field(&self) -> String {
            match &self.app.dialog {
                Some(
                    Dialog::AddPath { input }
                    | Dialog::EditTitle { input, .. }
                    | Dialog::EditEpicTitle { input, .. }
                    | Dialog::EditEpicId { input, .. }
                    | Dialog::EditId { input, .. },
                ) => input.clone(),
                Some(Dialog::NewTicket {
                    title,
                    jira_id,
                    focus,
                    ..
                }) => match focus {
                    TicketField::Title => title.clone(),
                    TicketField::JiraId => jira_id.clone(),
                },
                Some(Dialog::StageForm { values, focus, .. }) => values[*focus].clone(),
                other => panic!("not in a text field: {other:?}"),
            }
        }
    }

    /// On fixtures built by `setup` (the focus on the list), the capital of `small` with Caps
    /// Lock (no modifier) and with Shift does what `small` does, and `small` does something (the
    /// action or the state changes): two keys that do nothing don't pass. Returns the outcome.
    fn assert_capital_does_the_same(setup: fn() -> Fixture, small: char) -> String {
        let capital = small.to_ascii_uppercase();
        let mut f = setup();
        assert_eq!(f.app.focus, Focus::List, "the keys are the list's");
        let before = format!("{:?} | {}", Action::None, f.state());
        let expected = f.outcome(letter(small, KeyModifiers::NONE));
        assert_ne!(expected, before, "{small} does nothing here");
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
            let mut f = setup();
            assert_eq!(
                f.outcome(letter(capital, modifiers)),
                expected,
                "{capital} with {modifiers:?}"
            );
        }
        expected
    }

    /// An agent running (`running`), the focus back on the list and its node selected.
    fn live_agent() -> Fixture {
        let (mut f, launcher) = running();
        f.shortcut();
        f.select(Fixture::anode(launcher.ids[0]));
        f
    }

    #[test]
    fn capital_letters_do_on_the_tree_what_the_small_ones_do() {
        let module: fn() -> Fixture = fixture;
        // NS-1 selected, between the project and NS-2; no agent running.
        let global: fn() -> Fixture = || with_agents().0;
        let project: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.on_project();
            f
        };
        let epic: fn() -> Fixture = || with_epic().0;
        let ticket: fn() -> Fixture = || {
            let (mut f, _) = with_epic();
            f.select_key("NS-1");
            f
        };
        let stopped: fn() -> Fixture = || stopped_agent().0;
        for (setup, letters) in [
            (global, "jkfvq"),
            (module, "ac"),
            (project, "anr"),
            (epic, "aeogi"),
            (ticket, "eogpmi"),
            (live_agent as fn() -> Fixture, "x"),
            (stopped, "ro"),
        ] {
            for small in letters.chars() {
                assert_capital_does_the_same(setup, small);
            }
        }
        // Some of the outcomes, so the comparison is not of two wrong ones.
        let mut f = global();
        assert_eq!(f.ch('Q'), Action::Quit);
        let mut f = module();
        f.ch('A');
        assert_eq!(
            f.app.dialog,
            Some(Dialog::AddPath {
                input: String::new()
            })
        );
        let mut f = global();
        f.mods(KeyCode::Char('J'), KeyModifiers::SHIFT);
        assert_eq!(f.selected_key().as_deref(), Some("NS-2"));
    }

    #[test]
    fn capital_letters_do_on_the_lists_of_choice_and_the_settings_what_the_small_ones_do() {
        // Each list with its second item selected, so j and k both move.
        let menu: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.g(&mut FakeLauncher::new(false));
            f.key(KeyCode::Down);
            f
        };
        let stage: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.ch('m');
            f.key(KeyCode::Down);
            assert!(matches!(
                f.app.dialog,
                Some(Dialog::ChooseTicketStage { index: 1, .. })
            ));
            f
        };
        let mover: fn() -> Fixture = || {
            let (mut f, _) = with_epic();
            assert_eq!(f.new_epic("Outro plano", "EPIC-2"), Action::None);
            f.select_key("NS-1");
            f.ch('p');
            f.key(KeyCode::Down);
            assert!(matches!(
                f.app.dialog,
                Some(Dialog::MoveTicket { index: 1, .. })
            ));
            f
        };
        let preview: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            let mut launcher = FakeLauncher::new(false);
            launcher.render = Ok(rendered(100));
            f.g(&mut launcher);
            f.pick(StageType::PoDiscussion);
            f.fill_and_enter();
            f.app.render_prompt(&mut launcher);
            f.app.set_prompt_window(10, 90);
            f.key(KeyCode::Down);
            assert_eq!(f.scroll(), 1);
            f
        };
        let settings: fn() -> Fixture = || {
            let mut f = fixture();
            f.ch('c');
            assert_eq!(f.app.screen, Screen::Settings);
            f
        };
        let lost: fn() -> Fixture = || {
            let (mut f, mut launcher, _) = stopped_agent();
            launcher.conversation = Ok(false);
            assert!(f.r(&mut launcher));
            assert!(matches!(
                f.app.dialog,
                Some(Dialog::ConversationLost { .. })
            ));
            f
        };
        for (setup, letters) in [
            (menu, "jk"),
            (stage, "jk"),
            (mover, "jk"),
            (preview, "jksn"),
            (settings, "tp"),
            (lost, "an"),
        ] {
            for small in letters.chars() {
                assert_capital_does_the_same(setup, small);
            }
        }
        let mut f = menu();
        f.ch('J');
        assert_eq!(f.menu().1, 2);
        let mut f = preview();
        f.ch('K');
        assert_eq!(f.scroll(), 0);
        assert!(matches!(f.ch('S'), Action::OpenAgent { .. }));
    }

    #[test]
    fn capital_s_confirms_and_capital_n_cancels_every_confirmation() {
        let hide: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.ch('o');
            f
        };
        let hide_epic: fn() -> Fixture = || {
            let (mut f, _) = with_epic();
            f.ch('o');
            f
        };
        let remove: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.on_project();
            f.ch('r');
            f
        };
        let end_agent: fn() -> Fixture = || {
            let mut f = live_agent();
            f.ch('x');
            f
        };
        let archive: fn() -> Fixture = || {
            let mut f = stopped_agent().0;
            f.ch('o');
            f
        };
        let stage_change: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.choose_ticket_stage(TicketStage::Done);
            f
        };
        let edit_id: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.ch('i');
            f.ch('0');
            f.key(KeyCode::Enter);
            f
        };
        let edit_epic_id: fn() -> Fixture = || {
            let (mut f, _) = with_epic();
            f.ch('i');
            f.ch('0');
            f.key(KeyCode::Enter);
            f
        };
        let quit: fn() -> Fixture = || {
            let mut f = live_agent();
            f.ch('q');
            f
        };
        for (setup, dialog) in [
            (hide, "ConfirmHide"),
            (hide_epic, "ConfirmHideEpic"),
            (remove, "ConfirmRemove"),
            (end_agent, "ConfirmEndAgent"),
            (archive, "ConfirmArchive"),
            (stage_change, "ConfirmStageChange"),
            (edit_id, "ConfirmEditId"),
            (edit_epic_id, "ConfirmEditEpicId"),
            (quit, "ConfirmQuit"),
        ] {
            let f = setup();
            assert_eq!(f.app.dialog.as_ref().map(variant).as_deref(), Some(dialog));
            let yes = assert_capital_does_the_same(setup, 's');
            let no = assert_capital_does_the_same(setup, 'n');
            assert_ne!(yes, no, "{dialog}: s confirms, n cancels");
        }
    }

    /// The text of the tests of the text fields, with the letters of the shortcuts of the
    /// confirmations (`S`, `N`), of quitting (`Q`) and of the lists (`J`, `K`).
    const SHOUT: &str = "TÍTULO EM MAIÚSCULAS QSNJK";

    #[test]
    fn capital_letters_are_typed_as_they_are_in_the_text_fields() {
        let add_path: fn() -> Fixture = || {
            let mut f = fixture();
            f.ch('a');
            f
        };
        let title: fn() -> Fixture = || {
            let (mut f, _) = with_tickets("proj");
            f.ch('a');
            f
        };
        let jira_id: fn() -> Fixture = || {
            let (mut f, _) = with_tickets("proj");
            f.ch('a');
            f.key(KeyCode::Tab);
            f
        };
        let edit_title: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.ch('e');
            f
        };
        let epic_title: fn() -> Fixture = || {
            let (mut f, _) = with_epic();
            f.ch('e');
            f
        };
        let epic_id: fn() -> Fixture = || {
            let (mut f, _) = with_epic();
            f.ch('i');
            f
        };
        let edit_id: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.ch('i');
            f
        };
        let form_line: fn() -> Fixture = || {
            let (mut f, _) = with_agents();
            f.g(&mut FakeLauncher::new(false));
            f.pick(StageType::PoDiscussion);
            let line = StageType::PoDiscussion
                .fields()
                .iter()
                .position(|field| field.input == Input::Line)
                .expect("a line field");
            while f.form().2 != line {
                f.key(KeyCode::Tab);
            }
            f
        };
        let typings: [fn(&mut Fixture, &str); 2] = [Fixture::type_text, Fixture::type_shifted];
        for (setup, dialog) in [
            (add_path, "AddPath"),
            (title, "NewTicket"),
            (jira_id, "NewTicket"),
            (edit_title, "EditTitle"),
            (epic_title, "EditEpicTitle"),
            (epic_id, "EditEpicId"),
            (edit_id, "EditId"),
            (form_line, "StageForm"),
        ] {
            for typing in typings {
                let mut f = setup();
                let before = f.field();
                // Each key returns Action::None: no Q quits.
                typing(&mut f, SHOUT);
                assert_eq!(f.field(), format!("{before}{SHOUT}"), "{dialog}");
                assert_eq!(
                    f.app.dialog.as_ref().map(variant).as_deref(),
                    Some(dialog),
                    "no S confirms nor N cancels"
                );
            }
        }
        // The title and the ID typed in capitals are stored so.
        for typing in typings {
            let (mut f, _) = with_tickets("proj");
            f.ch('a');
            typing(&mut f, SHOUT);
            f.key(KeyCode::Tab);
            typing(&mut f, "NS-99");
            assert_eq!(f.key(KeyCode::Enter), Action::None);
            assert_eq!(f.app.dialog, None);
            let stored: Vec<(String, String)> = f
                .all_tickets()
                .into_iter()
                .map(|t| (t.key, t.title))
                .collect();
            assert_eq!(stored, [("NS-99".to_string(), SHOUT.to_string())]);
        }
    }

    #[test]
    fn ctrl_or_alt_with_a_capital_s_does_not_confirm_and_with_a_small_s_still_does() {
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        let alt_shift = KeyModifiers::ALT | KeyModifiers::SHIFT;
        for (key, hides) in [
            (letter('S', ctrl_shift), false),
            (letter('S', alt_shift), false),
            (letter('s', KeyModifiers::CONTROL), true),
            (letter('s', KeyModifiers::ALT), true),
        ] {
            let (mut f, _) = with_agents();
            f.ch('o');
            assert!(matches!(f.app.dialog, Some(Dialog::ConfirmHide { .. })));
            assert_eq!(f.app.handle_key(key), Action::None, "{key:?}");
            assert_eq!(f.app.dialog, None, "any key closes it");
            let hidden = f
                .all_tickets()
                .iter()
                .any(|t| t.key == "NS-1" && t.hidden_at.is_some());
            assert_eq!(hidden, hides, "{key:?}");
        }
    }

    #[test]
    fn on_the_terminal_the_capital_letters_go_to_the_program_as_they_are() {
        let (mut f, _) = running();
        assert_eq!(
            f.mods(KeyCode::Char('J'), KeyModifiers::SHIFT),
            Action::Send(b"J".to_vec())
        );
        assert_eq!(f.ch('Q'), Action::Send(b"Q".to_vec()));
        assert_eq!(f.ch('S'), Action::Send(b"S".to_vec()));
        assert_eq!(f.app.focus, Focus::Terminal);
        assert_eq!(f.app.dialog, None);
    }

    #[test]
    fn the_capture_of_the_shortcut_gets_the_capital_letters_as_they_are() {
        let mut f = fixture();
        f.ch('c');
        f.ch('t');
        // A letter alone is refused, as before, with Caps Lock and with Shift.
        for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
            assert_eq!(f.mods(KeyCode::Char('J'), modifiers), Action::None);
            assert_eq!(
                f.status(),
                Some("Use uma tecla com Ctrl ou Alt (ou uma tecla de função, F1–F12).")
            );
            assert_eq!(f.first(), None);
        }
        // With Alt and Caps Lock the capital is Alt+Shift+J, not Alt+J.
        f.mods(KeyCode::Char('J'), KeyModifiers::ALT);
        assert_eq!(f.first(), Shortcut::from_db("alt+shift+j"));
        f.mods(KeyCode::Char('j'), KeyModifiers::ALT | KeyModifiers::SHIFT);
        assert_eq!(f.stored_shortcut().as_deref(), Some("alt+shift+j"));
    }
}
