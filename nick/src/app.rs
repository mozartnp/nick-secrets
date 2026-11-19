//! The state of the TUI and its transitions by key, with no terminal I/O: `handle_key` takes a
//! key and returns an `Action`, which the loop (main.rs, through `on_key`) carries out. The
//! only action that runs bin/nick-init is `Action::RunInit`, and only the "Criar configuração"
//! button of the missing-config alert produces it (plan D7). The navigation is Modules >
//! Projects > Tickets; the rules of the tickets are in tickets.rs, and none of them touches a
//! file of the project. The screen texts are in PT-BR, and drawing is ui.rs's job.

use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::db::Db;
use crate::init::{InitOutcome, InitRunner};
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
        }
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
            Action::None => false,
        }
    }

    /// The transition of a key.
    pub fn handle_key(&mut self, key: KeyEvent) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::None;
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
            None => match self.screen {
                Screen::Modules => self.key_modules(key),
                Screen::Projects => self.key_projects(key),
                Screen::Tickets => self.key_tickets(key),
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
                Screen::Modules => {}
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
}
