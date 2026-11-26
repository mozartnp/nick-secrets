//! Drawing the TUI with ratatui, with no business rule: everything comes from the `App` state
//! (and, with an agent running, from the vt100 screen of the shown agent's terminal, drawn
//! with tui-term). The texts are in PT-BR; every text that comes from the operator or the
//! system goes through `paths::printable` (control characters become `?`). Where each part
//! goes is computed once, by `layout`, which the loop also uses for the size of each PTY and
//! for the mouse (NS-34): two copies of the count would drift apart.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::agents::{self, Agent, AgentState, Holder, Owner};
use crate::app::{
    App, Button, Dialog, EpicDetail, NewItem, Row, RunningAgent, Screen, TicketDetail, TicketField,
};
use crate::db::{Epic, StageChangeRow};
use crate::docs;
use crate::launch::{Precheck, Rendered};
use crate::marquee;
use crate::mascot::{
    self, BUBBLE_COLS, BUBBLE_ROWS, MASCOT_COLS, MASCOT_ROWS, MEOW_TEXT, TREE_MIN_ROWS,
};
use crate::paths::{count_of, printable};
use crate::projects;
use crate::stages::{self, Input, Rule};
use crate::tickets::{self, Ticket, TicketStage};
use crate::tree::{NodeId, Visible};
use crate::version;

/// The widest the list gets beside an agent (NS-34); never more than half the window.
pub const LIST_WIDTH: u16 = 40;

/// The mark of the selected row of the tree. The `List` keeps its width on every row while a
/// row is selected (always, on the tree), so it is out of the room of each row (NS-55, D2).
pub const HIGHLIGHT_SYMBOL: &str = "> ";

/// The color of the border of the focused panel beside an agent (NS-37).
pub const FOCUS_COLOR: Color = Color::Cyan;

/// The blue of PO and Tech Leader (NS-45): of the 16 colors, and not the cyan of `FOCUS_COLOR`.
pub const STAGE_BLUE: Color = Color::LightBlue;

/// The orange of Dev and Review (NS-45): the 208 of the 256 palette (approximated with 16 colors).
pub const STAGE_ORANGE: Color = Color::Indexed(208);

/// The gray of a rejected ticket (NS-61, D9): the whole label on the tree, and the mark on the
/// panel. The 8 of the 16 colors (`Idx(8)` in the vt100).
pub const STAGE_GRAY: Color = Color::DarkGray;

/// The color of the ID and of the title of an epic on the tree (NS-45).
pub const EPIC_COLOR: Color = Color::Magenta;

/// The color of each stage of a ticket (NS-45), on the tree and on the panel.
pub fn stage_color(stage: TicketStage) -> Color {
    match stage {
        TicketStage::Open => Color::White,
        TicketStage::Po | TicketStage::TechLeader => STAGE_BLUE,
        TicketStage::Development | TicketStage::Review => STAGE_ORANGE,
        TicketStage::Done => Color::Green,
        TicketStage::Rejected => STAGE_GRAY,
    }
}

/// The block of a panel beside an agent (NS-37): thick and in `FOCUS_COLOR` with the focus,
/// plain and dim without it. Both take one cell, so toggling the focus resizes nothing; the
/// thickness alone tells them apart where the color is weak (16 colors, a light theme).
fn pane_block(focused: bool) -> Block<'static> {
    if focused {
        Block::bordered()
            .border_type(BorderType::Thick)
            .border_style(Style::new().fg(FOCUS_COLOR))
    } else {
        Block::bordered()
            .border_type(BorderType::Plain)
            .border_style(Style::new().add_modifier(Modifier::DIM))
    }
}

/// The color of the border of a dialog of an irreversible action (NS-46).
pub const IRREVERSIBLE_COLOR: Color = Color::Red;

/// The block of an open dialog (NS-46): it has the focus, so `pane_block(true)`; in
/// `IRREVERSIBLE_COLOR` when its action has no way back (`Dialog::irreversible`) or when it
/// changes the jail of an agent (`Dialog::touches_jail`, NS-57: two rules give the red).
fn dialog_block(dialog: &Dialog) -> Block<'static> {
    let block = pane_block(true);
    if dialog.irreversible() || dialog.touches_jail() {
        block.border_style(Style::new().fg(IRREVERSIBLE_COLOR))
    } else {
        block
    }
}

/// Who gets the keys, and so the focus border (NS-46): the open dialog, else the terminal of
/// the shown agent (`App::on_terminal`), else the list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Focused {
    Dialog,
    Terminal,
    List,
}

/// Who has the focus border (`Focused`). The dialog comes first even if one ever opens with the
/// focus on the terminal: only one border is highlighted.
fn focused(app: &App) -> Focused {
    if app.dialog.is_some() {
        Focused::Dialog
    } else if app.on_terminal() {
        Focused::Terminal
    } else {
        Focused::List
    }
}

/// The block of the column on the left (the tree, or the settings): always `pane_block` since
/// the NS-38 (always side by side), focused when it gets the keys (`focused`: not with a dialog
/// open nor with the focus on the terminal, NS-46).
fn list_block(app: &App) -> Block<'static> {
    pane_block(focused(app) == Focused::List)
}

/// Where the cat Nick goes inside the column on the left (`inner`, inside its border; NS-51):
/// its last `MASCOT_ROWS` rows, against the left. None when the inside has fewer than
/// `MASCOT_ROWS + TREE_MIN_ROWS` rows or `MASCOT_COLS` columns: a fixed rule, so opening or
/// closing a node never shows nor hides her, and she is never drawn cut.
pub fn mascot_area(inner: Rect) -> Option<Rect> {
    (inner.height >= MASCOT_ROWS + TREE_MIN_ROWS && inner.width >= MASCOT_COLS).then(|| Rect {
        x: inner.x,
        y: inner.bottom() - MASCOT_ROWS,
        width: MASCOT_COLS,
        height: MASCOT_ROWS,
    })
}

/// What is left of `inner` for the tree (or the settings): all of it without the cat, the rows
/// above her with her.
pub fn content_area(inner: Rect) -> Rect {
    match mascot_area(inner) {
        Some(cat) => Rect {
            height: cat.y - inner.y,
            ..inner
        },
        None => inner,
    }
}

/// Where the "Miau!" bubble goes: on the right of the cat after one column of space, on her
/// last `BUBBLE_ROWS` rows. None without the cat or without the `MASCOT_COLS + 1 + BUBBLE_COLS`
/// columns: a cut bubble would not be read.
pub fn bubble_area(inner: Rect) -> Option<Rect> {
    let cat = mascot_area(inner)?;
    (inner.width >= MASCOT_COLS + 1 + BUBBLE_COLS).then(|| Rect {
        x: cat.right() + 1,
        y: cat.bottom() - BUBBLE_ROWS,
        width: BUBBLE_COLS,
        height: BUBBLE_ROWS,
    })
}

/// The column on the left (the tree or the settings): its block with `title`, the cat at its
/// bottom when she fits (`mascot_area`), and the area left for the content. The cat is not in
/// `Areas`: she changes nothing the loop and the mouse use.
fn left_column(frame: &mut Frame, app: &App, area: Rect, title: impl Into<Line<'static>>) -> Rect {
    let block = list_block(app).title(title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    draw_mascot(frame, app, inner);
    content_area(inner)
}

/// The cat inside `inner`, when she fits: each cell two pixels of her pose (`mascot::cells`),
/// in truecolor or in the 256 palette (`App::truecolor`); a transparent cell is not touched.
/// With the bubble open and room for it, the bubble.
fn draw_mascot(frame: &mut Frame, app: &App, inner: Rect) {
    let Some(cat) = mascot_area(inner) else {
        return;
    };
    let shown = app.mascot().frame();
    let palette = app.palette();
    let color = |letter: Option<char>| letter.and_then(|c| palette.color(c));
    let buffer = frame.buffer_mut();
    for (y, row) in (cat.y..).zip(mascot::cells(shown.pose)) {
        for (x, art) in (cat.x..).zip(row) {
            let Some(symbol) = art.symbol else {
                continue;
            };
            let cell = &mut buffer[(x, y)];
            cell.set_char(symbol);
            if symbol == '▀' {
                if let Some(fg) = color(art.top) {
                    cell.set_fg(fg);
                }
                if let Some(bg) = color(art.bottom) {
                    cell.set_bg(bg);
                }
            } else if let Some(fg) = color(art.bottom) {
                cell.set_fg(fg);
            }
        }
    }
    if shown.meowing
        && let Some(bubble) = bubble_area(inner)
    {
        let inside = usize::from(BUBBLE_COLS - 2);
        let edge = "─".repeat(inside);
        let lines = vec![
            Line::from(format!("╭{edge}╮")),
            Line::from(format!("│{MEOW_TEXT:<inside$}│")),
            Line::from(format!("╰{edge}╯")),
        ];
        frame.render_widget(Paragraph::new(lines), bubble);
    }
}

/// Where each part of the screen goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Areas {
    /// Everything above the status: where the dialogs are centered.
    pub body: Rect,
    /// The list of the current screen (the whole body without an agent running).
    pub list: Rect,
    /// The panel of the shown agent, with its border (NS-37): inside it, its header, then its
    /// terminal (empty without agents).
    pub panel: Rect,
    pub header: Rect,
    pub terminal: Rect,
    pub status: Rect,
    pub help: Rect,
}

/// The layout of the screen `area`: the status and the help at the bottom, full width, taking
/// the rows they need, word wrapped (up to a third of the screen each), so a long one is not
/// cut; above them, always side by side since the NS-38 (with or without an agent): the tree
/// (or the settings) on the left (`LIST_WIDTH`, at most half the width) and the panel on the
/// right, the shown agent's or the selected node's.
pub fn layout(app: &App, area: Rect) -> Areas {
    layout_for(app, area, app.shown_agent())
}

/// The area of the terminal of `agent` on the screen `area`, as if it were the shown one: the
/// size of its PTY (at least 2x2: agent_session.rs). Each agent's header has its own height,
/// so each terminal has its own size.
pub fn agent_area(app: &App, area: Rect, agent: &RunningAgent) -> Rect {
    layout_for(app, area, Some(agent)).terminal
}

/// The panel an agent about to open gets on the screen `area` (review NS-34, P2): the same
/// count as `layout`, in the state right after the opening (one more agent, the focus on its
/// terminal, no status message). `terminal_in_panel` gives its terminal once its name is known:
/// the size of its PTY at the spawn (NS-57, D11: whatever the jail).
pub fn new_agent_panel(app: &App, area: Rect) -> Rect {
    let running = app.agents().len() + 1;
    let help = help_text_for(app, help_state(app), true, None, None);
    let help_rows = help_rows(app, area.width, running, &help);
    split(area, 1, help_rows).panel
}

/// The terminal of the agent `name` in `panel`: inside the border of the panel (NS-37, one cell
/// on each side, whatever the focus), below its header (`header_rows`). The one count of the
/// size of each PTY: the drawing, the mouse and the spawn take it from here. NS-57, D11: the
/// same in the jail and out of it, so releasing or jailing an agent keeps its size.
pub fn terminal_in_panel(panel: Rect, name: &str) -> Rect {
    let inner = Block::bordered().inner(panel);
    let rows = header_rows(name, inner);
    Rect {
        y: inner.y + rows,
        height: inner.height - rows,
        ..inner
    }
}

fn layout_for(app: &App, area: Rect, agent: Option<&RunningAgent>) -> Areas {
    let status_rows = status_lines(app, area.width).len();
    let help_rows = help_rows(app, area.width, app.agents().len(), &help_text(app));
    let mut areas = split(area, status_rows, help_rows);
    if let Some(agent) = agent.filter(|_| !areas.panel.is_empty()) {
        areas.terminal = terminal_in_panel(areas.panel, &agent.name);
        areas.header = header_in_panel(areas.panel, areas.terminal);
    } else {
        areas.terminal = areas.panel;
    }
    areas
}

/// The header of the panel: inside its border, from the top down to `terminal`.
fn header_in_panel(panel: Rect, terminal: Rect) -> Rect {
    let inner = Block::bordered().inner(panel);
    Rect {
        height: terminal.y - inner.y,
        ..inner
    }
}

/// The status and the help (`status_rows`, `help_rows`, up to a third of the screen each) at
/// the bottom of `area`, and above them the column on the left beside the panel: always side
/// by side since the NS-38 (decision 1), with or without an agent.
fn split(area: Rect, status_rows: usize, help_rows: usize) -> Areas {
    let rows = |lines: usize| {
        u16::try_from(lines)
            .unwrap_or(u16::MAX)
            .min((area.height / 3).max(1))
    };
    let [body, status, help] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(rows(status_rows)),
        Constraint::Length(rows(help_rows)),
    ])
    .areas(area);
    let [list, panel] = Layout::horizontal([
        Constraint::Length(LIST_WIDTH.min(area.width / 2)),
        Constraint::Min(0),
    ])
    .areas(body);
    Areas {
        body,
        list,
        panel,
        status,
        help,
        ..Areas::default()
    }
}

/// The status message word wrapped at `width` (one empty row without a message).
fn status_lines(app: &App, width: u16) -> Vec<String> {
    app.status.as_deref().map_or_else(
        || vec![String::new()],
        |message| wrap_words(&printable(message), width.max(1)),
    )
}

fn help_lines(app: &App, width: u16) -> Vec<String> {
    wrap_words(&help_text(app), width.max(1))
}

/// Draws the whole screen without an agent's terminal (see `draw_with_terminal`).
pub fn draw(frame: &mut Frame, app: &App) {
    draw_with_terminal(frame, app, None);
}

/// Draws the whole screen (`layout`): the tree (or the settings), the panel of the shown
/// agent with `terminal` (its vt100 screen) when an agent runs, the status line and the help
/// line, and the open dialog centered over the body.
pub fn draw_with_terminal(frame: &mut Frame, app: &App, terminal: Option<&vt100::Screen>) {
    let areas = layout(app, frame.area());
    let (body, status, help) = (areas.list, areas.status, areas.help);
    match app.screen {
        Screen::Settings => draw_settings(frame, app, body),
        Screen::Tree => draw_tree(frame, app, body),
    }
    if let Some(agent) = app.shown_agent() {
        draw_agent(
            frame,
            areas.panel,
            &agent.name,
            agent.jail,
            focused(app) == Focused::Terminal,
            terminal,
        );
    } else {
        draw_node_panel(frame, app, areas.panel);
    }
    let status_lines = status_lines(app, frame.area().width);
    let help_lines = help_lines(app, frame.area().width);
    if app.status.is_some() {
        let lines: Vec<Line> = status_lines.into_iter().map(Line::from).collect();
        frame.render_widget(Paragraph::new(lines), status);
    }
    let help_lines: Vec<Line> = help_lines.into_iter().map(Line::from).collect();
    frame.render_widget(
        Paragraph::new(help_lines).style(Style::new().add_modifier(Modifier::DIM)),
        help,
    );
    match &app.dialog {
        // The preview of the prompt takes almost the whole screen (NS-35).
        Some(
            dialog @ Dialog::ConfirmPrompt {
                precheck,
                rendered,
                scroll,
                ..
            },
        ) => draw_prompt(
            frame,
            prompt_area(frame.area(), areas),
            dialog,
            precheck,
            rendered,
            *scroll,
        ),
        Some(dialog) => draw_dialog(frame, app, dialog, areas.body),
        None => {}
    }
}

/// `text` broken into lines of at most `width` columns, at the spaces; a word longer than the
/// width is split. Exact, unlike `wrapped_rows`, so the status gets the rows it uses.
fn wrap_words(text: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split(' ') {
        let used = Span::raw(line.as_str()).width();
        let size = Span::raw(word).width();
        if !line.is_empty() && used + 1 + size <= width {
            line.push(' ');
            line.push_str(word);
            continue;
        }
        if !line.is_empty() {
            lines.push(std::mem::take(&mut line));
        }
        for c in word.chars() {
            if Span::raw(line.as_str()).width() + Span::raw(c.to_string()).width() > width {
                lines.push(std::mem::take(&mut line));
            }
            line.push(c);
        }
    }
    lines.push(line);
    lines
}

/// Where the help is (NS-38): the kind of the selected node of the tree, or the settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Place {
    Module,
    Project,
    /// The node of an epic (NS-40).
    Epic,
    Ticket,
    /// A live agent of a project without the jail (NS-57: no `l`).
    LiveAgent,
    /// A live agent of a project with the jail (NS-57): in it (`l liberar jail`) or released by
    /// the operator (`l prender na jail`).
    LiveAgentInJail,
    LiveAgentReleased,
    /// An agent not live (NS-39): stopped (resumable), archived, or active in another nick.
    StoppedAgent,
    /// A stopped agent whose project the last check said has the jail on (NS-57: `Row.jail`,
    /// `l liberar jail`).
    StoppedAgentInJail,
    ArchivedAgent,
    ElsewhereAgent,
    Settings,
}

/// The place of the help, with the filters of the tree (the hidden tickets, the done ones since
/// the NS-54, only the active agents), in the order of the help: `f`, `d`, `v`.
type HelpState = (Place, bool, bool, bool);

/// The state of the help of `app` now.
fn help_state(app: &App) -> HelpState {
    let place = if app.screen == Screen::Settings {
        Place::Settings
    } else {
        match app.selected() {
            NodeId::Module(_) => Place::Module,
            NodeId::Project(_) => Place::Project,
            NodeId::Epic(_) => Place::Epic,
            NodeId::Ticket(_) => Place::Ticket,
            // NS-57: by `App::jail_toggle`, the rule of the key itself, so the help and the key
            // always agree.
            NodeId::Agent(id) if app.agent(id).is_some() => match app.jail_toggle(id) {
                None => Place::LiveAgent,
                Some(true) => Place::LiveAgentInJail,
                Some(false) => Place::LiveAgentReleased,
            },
            NodeId::Agent(id) => {
                match app
                    .agent_record(id)
                    .and_then(|agent| AgentState::from_key(&agent.state))
                {
                    Some(AgentState::Stopped) if app.jail_toggle(id).is_some() => {
                        Place::StoppedAgentInJail
                    }
                    Some(AgentState::Stopped) => Place::StoppedAgent,
                    Some(AgentState::Archived) => Place::ArchivedAgent,
                    _ => Place::ElsewhereAgent,
                }
            }
        }
    };
    let (show_hidden, only_active) = app.filters();
    (place, show_hidden, app.hide_done(), only_active)
}

/// The keys of the selected node, in the help line. With the focus on the terminal, only the
/// way back (every other key is the program's). With agents running, `v` says how many there
/// are and how many are marked.
fn help_text(app: &App) -> String {
    let count = (!app.agents().is_empty()).then(|| agents_count(app));
    help_text_for(
        app,
        help_state(app),
        app.on_terminal(),
        app.dialog.as_ref(),
        count,
    )
}

/// The keys of an open dialog.
fn dialog_keys(dialog: &Dialog) -> &'static str {
    match dialog {
        Dialog::AddPath { .. } => "Enter confirmar · Esc cancelar",
        Dialog::NeedsConfig { .. } => "←/→/Tab escolher · Enter acionar · Esc cancelar",
        Dialog::ConfirmRemove { .. } => "s remover · qualquer outra tecla cancela",
        Dialog::NewTicket { .. } => "Tab trocar de campo · Enter confirmar · Esc cancelar",
        Dialog::EditTitle { .. } => "Enter confirmar · Esc cancelar",
        Dialog::ConfirmHide { .. } => "s ocultar · qualquer outra tecla cancela",
        Dialog::EditEpicTitle { .. } => "Enter confirmar · Esc cancelar",
        Dialog::ConfirmHideEpic { .. } => "s ocultar · qualquer outra tecla cancela",
        Dialog::EditEpicId { .. } => "Enter confirmar · Esc cancelar",
        Dialog::ConfirmEditEpicId { .. } => "s trocar · qualquer outra tecla cancela",
        Dialog::MoveTicket { .. } => "↑/↓ escolher · Enter mover · Esc cancelar",
        Dialog::ChooseType { .. } => "↑/↓ escolher · Enter abrir · Esc cancelar",
        Dialog::StageForm { .. } => {
            "Tab/↑/↓ trocar de campo · Enter confirmar (no campo do editor: abrir o editor) · \
             Esc cancelar"
        }
        Dialog::ConfirmPrompt { .. } => PROMPT_KEYS,
        Dialog::AgentError { .. } => "Enter/Esc fechar",
        Dialog::ConfirmEndAgent { .. } => "s encerrar · qualquer outra tecla cancela",
        Dialog::ConfirmJailToggle { release: true, .. } => {
            "s liberar · qualquer outra tecla cancela"
        }
        Dialog::ConfirmJailToggle { release: false, .. } => {
            "s prender · qualquer outra tecla cancela"
        }
        Dialog::ConversationLost { .. } => {
            "a arquivar · n abrir novo agente nesta etapa · Esc cancelar"
        }
        Dialog::ConfirmArchive { .. } => "s arquivar · qualquer outra tecla cancela",
        Dialog::ConfirmQuit => "s encerrar e sair · qualquer outra tecla cancela",
        Dialog::CaptureShortcut { .. } => "Esc cancelar",
        Dialog::ChooseTicketStage { .. } => "↑/↓ escolher · Enter escolher · Esc cancelar",
        Dialog::RejectReason { .. } => "Enter confirmar · Esc cancelar",
        Dialog::ConfirmStageChange {
            to: TicketStage::Rejected,
            ..
        } => "s rejeitar · qualquer outra tecla cancela",
        Dialog::ConfirmStageChange { .. } => "s mudar · qualquer outra tecla cancela",
        Dialog::EditId { .. } => "Enter confirmar · Esc cancelar",
        Dialog::ConfirmEditId { .. } => "s trocar · qualquer outra tecla cancela",
    }
}

/// `help_text` in a given state: the place with the filters, the focus on the terminal, the
/// open dialog, and the count of the agents (`None` without agents running). On the tree: the
/// keys of the node, then the two filters (`v` always, as the view of before, which worked
/// without agents too), then `q sair`.
fn help_text_for(
    app: &App,
    (place, show_hidden, hide_done, only_active): HelpState,
    on_terminal: bool,
    dialog: Option<&Dialog>,
    count: Option<String>,
) -> String {
    let shortcut = app.shortcut().label();
    if on_terminal {
        // The Alt+Enter, always (NS-49): the new line of the `claude` on any terminal; the
        // Shift+Enter only works with the kitty keyboard protocol, and isn't shown.
        return format!("{shortcut} voltar à lista · Alt+Enter nova linha");
    }
    if let Some(dialog) = dialog {
        return dialog_keys(dialog).to_string();
    }
    let keys = match place {
        Place::Settings => return "t trocar · p voltar ao padrão · Esc voltar · q sair".into(),
        Place::Module => {
            "↑/↓ mover · →/Enter abrir · ← fechar · a adicionar projeto · c configurações".into()
        }
        Place::Project => "↑/↓ mover · →/Enter abrir · ← fechar/subir · a novo ticket · \
                           n novo épico · r remover"
            .into(),
        Place::Epic => "↑/↓ mover · →/Enter abrir · ← fechar/subir · a novo ticket · \
                        e editar título · o ocultar/restaurar · c concluir/reabrir · \
                        g abrir agente · i editar ID"
            .into(),
        // Shorter since the NS-40 (`m etapa · i ID`), to make room for `p` with the same rows.
        Place::Ticket => "↑/↓ mover · →/Enter abrir · ← fechar/subir · e editar título · \
                          o ocultar/restaurar · g abrir agente · m etapa · i ID · p épico"
            .into(),
        Place::LiveAgent => {
            format!("↑/↓ mover · Enter ir ao terminal · ← subir · x encerrar · {shortcut} alternar")
        }
        Place::LiveAgentInJail => format!(
            "↑/↓ mover · Enter ir ao terminal · ← subir · x encerrar · l liberar jail · \
             {shortcut} alternar"
        ),
        Place::LiveAgentReleased => format!(
            "↑/↓ mover · Enter ir ao terminal · ← subir · x encerrar · l prender na jail · \
             {shortcut} alternar"
        ),
        Place::StoppedAgent => "↑/↓ mover · r retomar · o arquivar · ← subir".to_string(),
        Place::StoppedAgentInJail => {
            "↑/↓ mover · r retomar · o arquivar · l liberar jail · ← subir".to_string()
        }
        Place::ArchivedAgent => "↑/↓ mover · o desarquivar · ← subir".to_string(),
        Place::ElsewhereAgent => "↑/↓ mover · ← subir".to_string(),
    };
    let hidden = if show_hidden {
        "f esconder ocultos"
    } else {
        "f mostrar ocultos"
    };
    // NS-54: the filter of the done tickets and epics, off by default.
    let done = if hide_done {
        "d mostrar concluídos"
    } else {
        "d esconder concluídos"
    };
    let active = if only_active {
        "v mostrar tudo"
    } else {
        "v agentes ativos"
    };
    let count = count.map_or(String::new(), |count| format!(" ({count})"));
    format!("{keys} · {hidden} · {done} · {active}{count} · q sair")
}

/// The places of the tree, each with every combination of the three filters in `HELP_STATES`.
const TREE_PLACES: [Place; 11] = [
    Place::Module,
    Place::Project,
    Place::Epic,
    Place::Ticket,
    Place::LiveAgent,
    Place::LiveAgentInJail,
    Place::LiveAgentReleased,
    Place::StoppedAgent,
    Place::StoppedAgentInJail,
    Place::ArchivedAgent,
    Place::ElsewhereAgent,
];

/// Every state of the help without a dialog: each kind of node with the 8 combinations of the
/// three filters (NS-54: 8 × 8; NS-57: 11 kinds), and the settings. `help_rows` reserves rows
/// for the longest of them.
const HELP_STATES: [HelpState; 89] = help_states();

const fn help_states() -> [HelpState; 89] {
    let mut states = [(Place::Settings, false, false, false); 89];
    let mut at = 0;
    while at < 88 {
        let filters = at % 8;
        states[at] = (
            TREE_PLACES[at / 8],
            filters & 1 != 0,
            filters & 2 != 0,
            filters & 4 != 0,
        );
        at += 1;
    }
    states
}

/// The rows reserved for the help with `running` agents (review NS-34, P1): never fewer than
/// the longest help of the tree (`HELP_STATES`), with the longest count (`N, N com saída
/// nova`), whatever the node, the filters, the focus, the dialog or the marks. So moving on
/// the tree, toggling the focus or a new mark does not resize every terminal (each resize
/// redraws every claude and marks the hidden ones). The shorter help of the moment is drawn in
/// the same rows. Without agents, the rows of the help itself.
fn help_rows(app: &App, width: u16, running: usize, help: &str) -> usize {
    let rows = wrap_words(help, width.max(1)).len();
    if running == 0 {
        return rows;
    }
    let count = format!("{running}, {running} com saída nova");
    HELP_STATES
        .into_iter()
        .map(|state| {
            let text = help_text_for(app, state, false, None, Some(count.clone()));
            wrap_words(&text, width.max(1)).len()
        })
        .fold(rows, usize::max)
}

/// `3` or `3, 1 com saída nova`: the running agents and the marked ones.
fn agents_count(app: &App) -> String {
    let total = app.agents().len();
    match app.agents().iter().filter(|agent| agent.unseen).count() {
        0 => total.to_string(),
        unseen => format!("{total}, {unseen} com saída nova"),
    }
}

/// The warning of an agent out of the jail (the text of `jail_print_status` off), in red and
/// bold in its header.
const OFF_JAIL: &str = "FORA DA JAIL: o agente roda direto no seu shell";

/// The header of an agent, word wrapped at `width` (decision of the user in the NS-34: the
/// warning out of the jail is never cut): its name and whether it runs in the jail; out of it,
/// the warning in red and bold.
fn header_lines(name: &str, jail: bool, width: u16) -> Vec<Line<'static>> {
    let start = format!("{} · ", printable(name));
    let warning = if jail { "na jail" } else { OFF_JAIL };
    let text = format!("{start}{warning}{}", if jail { "" } else { "." });
    // The characters of the warning, in `text`, to paint red.
    let red = start.chars().count()..start.chars().count() + warning.chars().count();
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    wrap_words(&text, width)
        .into_iter()
        .map(|line| {
            // Each wrapped line is the next piece of the text, after the spaces it broke at.
            let len = line.chars().count();
            while at < chars.len() && chars[at] == ' ' && !line.starts_with(' ') {
                at += 1;
            }
            let mut spans: Vec<Span<'static>> = Vec::new();
            for (i, c) in (at..at + len).zip(line.chars()) {
                let style = if !jail && red.contains(&i) {
                    Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                match spans.last_mut() {
                    Some(span) if span.style == style => span.content.to_mut().push(c),
                    _ => spans.push(Span::styled(c.to_string(), style)),
                }
            }
            at += len;
            Line::from(spans)
        })
        .collect()
}

/// The rows of the header of an agent in `panel`: the lines it wraps into, up to a third of
/// the panel (at least one). NS-57, D11 (decision of the user): the most of its version in the
/// jail and of its version out of it (in practice, the one out of it), so the height does not
/// depend on the jail; in the jail, the rows to spare stay blank (and may serve more
/// information some day).
fn header_rows(name: &str, panel: Rect) -> u16 {
    let lines = [true, false]
        .map(|jail| header_lines(name, jail, panel.width).len())
        .into_iter()
        .max()
        .unwrap_or(1);
    u16::try_from(lines)
        .unwrap_or(u16::MAX)
        .min((panel.height / 3).max(1))
        .min(panel.height)
}

/// The panel of an agent in `area`: its border (`pane_block`, thick and cyan when `focused`),
/// and inside it its header (wrapped, `header_rows`) and its terminal below it, the same split
/// as `layout` (`terminal_in_panel`).
pub fn draw_agent(
    frame: &mut Frame,
    area: Rect,
    name: &str,
    jail: bool,
    focused: bool,
    terminal: Option<&vt100::Screen>,
) {
    frame.render_widget(pane_block(focused), area);
    let below = terminal_in_panel(area, name);
    let header = header_in_panel(area, below);
    let lines: Vec<Line> = header_lines(name, jail, header.width)
        .into_iter()
        .take(usize::from(header.height))
        .collect();
    frame.render_widget(Paragraph::new(lines), header);
    if let Some(screen) = terminal {
        // The program's cursor, hidden while the history is scrolled (it is not on screen).
        let mut cursor = Cursor::default();
        if screen.scrollback() > 0 || screen.hide_cursor() {
            cursor.hide();
        }
        frame.render_widget(PseudoTerminal::new(screen).cursor(cursor), below);
    }
}

fn selected_style() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

/// "Configurações": the reserved shortcut, with `(padrão)` when the database has none, and the
/// version of this nick (NS-52).
fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let default = if app.shortcut_stored {
        ""
    } else {
        " (padrão)"
    };
    let lines = vec![
        Line::raw(format!(
            "Atalho para alternar entre a lista e o terminal: {}{default}",
            app.shortcut().label()
        )),
        // The version and the commit of this nick (NS-52).
        Line::raw(format!("Versão: {}", version::long())),
    ];
    let content = left_column(frame, app, area, " Configurações ");
    frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), content);
}

/// The mark of a stage on the tree: `[<label>]`, and `[✓ Concluído]` for the done ones (which
/// stay on the tree: only `o` hides a ticket), in the color of the stage (NS-45).
fn stage_mark(key: &str) -> Span<'static> {
    stage_span(key, true)
}

/// A stage in its color (`stage_color`, NS-45): `[<label>]` with `brackets` (the tree), the label
/// alone without (the "Etapa atual:" of the panel); Done is always `[✓ Concluído]`, and Rejected
/// `[✗ Rejeitado]` (NS-61, D9). An unknown key shows raw, with no color.
fn stage_span(key: &str, brackets: bool) -> Span<'static> {
    let Some(stage) = TicketStage::from_key(key) else {
        let raw = printable(key);
        return Span::raw(if brackets { format!("[{raw}]") } else { raw });
    };
    let text = match stage {
        TicketStage::Done => format!("[✓ {}]", stage.label()),
        TicketStage::Rejected => format!("[✗ {}]", stage.label()),
        _ if brackets => format!("[{}]", stage.label()),
        _ => stage.label().to_string(),
    };
    Span::styled(text, Style::new().fg(stage_color(stage)))
}

/// `AAAA-MM-DD HH:MM UTC` from the RFC 3339 of the database, by string slicing (no date
/// crate); anything else shows raw.
fn short_date(date: &str) -> String {
    let b = date.as_bytes();
    let shaped = b.len() == 20
        && date.is_ascii()
        && (b[4], b[7], b[10], b[13], b[16], b[19]) == (b'-', b'-', b'T', b':', b':', b'Z');
    if shaped {
        format!("{} {} UTC", &date[..10], &date[11..16])
    } else {
        printable(date)
    }
}

/// The label of a project on the tree: its name and, unavailable, the mark (the path goes to
/// the panel, to fit in 40 columns); dimmed when unavailable.
fn project_spans(row: &Row) -> (Vec<Span<'static>>, Style) {
    let mut spans = vec![Span::raw(printable(&row.project.name))];
    let mut style = Style::new();
    if let Some(mark) = projects::unavailable_text(row.availability) {
        spans.push(Span::raw("  "));
        spans.push(Span::raw(mark));
        style = style.add_modifier(Modifier::DIM);
    }
    (spans, style)
}

/// The label of a ticket: `<ID>  [<etapa>]  <título>  [provisório]  [oculto]` (the stage right
/// after the ID: the end of the row is cut at 40 columns, NS-36); dimmed when hidden; a rejected
/// one in `STAGE_GRAY`, the whole label (NS-61, D9: the style of the row, so the `…` and the
/// window of the marquee keep it).
fn ticket_spans(ticket: &Ticket) -> (Vec<Span<'static>>, Style) {
    let mut spans = vec![
        Span::raw(printable(&ticket.key)),
        Span::raw("  "),
        stage_mark(&ticket.stage),
        Span::raw("  "),
        Span::raw(printable(&ticket.title)),
    ];
    if ticket.is_provisional {
        spans.push(Span::raw("  [provisório]"));
    }
    let mut style = Style::new();
    if ticket.hidden_at.is_some() {
        spans.push(Span::raw("  [oculto]"));
        style = style.add_modifier(Modifier::DIM);
    }
    if tickets::is_rejected(ticket) {
        style = style.fg(STAGE_GRAY);
    }
    (spans, style)
}

/// The label of an epic (NS-40): `<ID>  [épico]  [aberto|concluído]  <título>  [provisório]
/// [oculto]`, the `[épico]` in bold right after the ID, so the 34 columns of depth 2 keep it; the
/// state (NS-54) right after it, whole in the 30 columns the label has at depth 2 with an ID of up
/// to 8 characters (decisions of the user in the validation: after the title the cut took it
/// away, and an open epic says `[aberto]`); the ID and the title in
/// `EPIC_COLOR` (NS-45), the marks with no color; dimmed when hidden, as a ticket.
fn epic_spans(epic: &Epic) -> (Vec<Span<'static>>, Style) {
    let purple = Style::new().fg(EPIC_COLOR);
    let mut spans = vec![
        Span::styled(printable(&epic.key), purple),
        Span::raw("  "),
        Span::styled("[épico]", Style::new().add_modifier(Modifier::BOLD)),
    ];
    // NS-54: the state, open or done, with no new color.
    spans.push(Span::raw(if epic.done_at.is_some() {
        "  [concluído]"
    } else {
        "  [aberto]"
    }));
    spans.push(Span::raw("  "));
    spans.push(Span::styled(printable(&epic.title), purple));
    if epic.is_provisional {
        spans.push(Span::raw("  [provisório]"));
    }
    let mut style = Style::new();
    if epic.hidden_at.is_some() {
        spans.push(Span::raw("  [oculto]"));
        style = style.add_modifier(Modifier::DIM);
    }
    (spans, style)
}

/// The state of an agent record: `● vivo` (this nick runs it), `ativo em outro nick` (an active
/// row without a session here), `arquivado`, `parado` (NS-39, the `encerrado` of before), or
/// `estado desconhecido` (a state this nick does not know: review of the NS-39, point 6).
fn agent_state(app: &App, agent: &Agent) -> &'static str {
    if app.agent(agent.id).is_some() {
        return "● vivo";
    }
    match AgentState::from_key(&agent.state) {
        Some(AgentState::Active) => "ativo em outro nick",
        Some(AgentState::Archived) => "arquivado",
        Some(AgentState::Stopped) => "parado",
        None => "estado desconhecido",
    }
}

/// The label of an agent: `<Etapa>  <estado>`, `[FORA DA JAIL]` in red (live out of the jail),
/// then the date it was opened. A marked agent (always live) shows `● saída nova` in the place
/// of `● vivo`: at depth 3 the tree has 28 columns, and the mark must never be cut (review of the
/// NS-38, P1); the warning of the jail, whole in the header of the panel, comes after it. The
/// state and the marks before the date, so the 40 columns keep them (as the stage after the ID
/// of a ticket); not live, dimmed.
fn agent_spans(app: &App, agent: &Agent) -> (Vec<Span<'static>>, Style) {
    let live = app.agent(agent.id);
    let state = match live {
        Some(live) if live.unseen => "● saída nova",
        // Marked as a hidden ticket is (`[oculto]`), and shown only with the filter `f` (NS-39).
        None if agent.state == AgentState::Archived.key() => "[arquivado]",
        _ => agent_state(app, agent),
    };
    let mut spans = vec![
        Span::raw(printable(&tickets::stage_label(&agent.stage))),
        Span::raw("  "),
        Span::raw(state),
    ];
    let mut style = Style::new();
    match live {
        Some(live) if !live.jail => {
            spans.push(Span::raw("  "));
            spans.push(Span::styled("[FORA DA JAIL]", Style::new().fg(Color::Red)));
        }
        Some(_) => {}
        None => style = style.add_modifier(Modifier::DIM),
    }
    let date = short_date(&agent.opened_at);
    spans.push(Span::raw(format!(
        "  {}",
        date.strip_suffix(" UTC").unwrap_or(&date)
    )));
    (spans, style)
}

/// The tree (NS-38): two spaces a level, `▾` open, `▸` closed, two spaces on a leaf, and the
/// label of the node; `● saída nova` at the end of a closed node with a marked agent below.
/// The title says the filters on (` (com ocultos) (só ativos)`: both fit in the 40 columns).
fn draw_tree(frame: &mut Frame, app: &App, area: Rect) {
    let (show_hidden, only_active) = app.filters();
    let mut title = " Navegação".to_string();
    if show_hidden {
        title.push_str(" (com ocultos)");
    }
    if only_active {
        title.push_str(" (só ativos)");
    }
    title.push(' ');
    let content = left_column(frame, app, area, title);
    let items: Vec<ListItem> = app
        .visible()
        .iter()
        .map(|row| {
            let (prefix, mut text, style) = tree_row(app, row);
            // NS-55: what does not fit scrolls on the selected row and ends in `…` on the
            // others; the prefix stays (D1).
            let room = tree_available(content, &prefix);
            let overflow = marquee::width(&text).saturating_sub(room);
            if overflow > 0 {
                text = if row.id == app.selected() {
                    marquee::window(&text, app.marquee_offset(row.id, overflow), room)
                } else {
                    marquee::ellipsize(&text, room)
                };
            }
            let mut spans = vec![Span::raw(prefix)];
            spans.extend(text);
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();
    let selected = app
        .visible()
        .iter()
        .position(|row| row.id == app.selected());
    let list = List::new(items)
        .highlight_symbol(HIGHLIGHT_SYMBOL)
        .highlight_style(selected_style());
    let mut state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list, content, &mut state);
}

/// The columns left for the text of a row of the tree with `prefix` in `content` (the area of
/// the tree inside the column, `left_column`): its width without the highlight symbol and the
/// prefix (NS-55, D2).
fn tree_available(content: Rect, prefix: &str) -> usize {
    usize::from(content.width)
        .saturating_sub(Span::raw(HIGHLIGHT_SYMBOL).width())
        .saturating_sub(Span::raw(prefix).width())
}

/// How many columns of the text of the selected row of the tree do not fit on a screen of
/// `area` (NS-55): what its marquee scrolls, for the loop to give the App
/// (`App::tick_marquee`) before each draw. The same count the drawing makes (`layout`, the
/// block and the cat of the column, `tree_row`, `tree_available`); 0 on the settings and
/// without the selected row on the tree.
pub fn marquee_overflow(app: &App, area: Rect) -> usize {
    if app.screen != Screen::Tree {
        return 0;
    }
    let content = content_area(list_block(app).inner(layout(app, area).list));
    app.visible()
        .iter()
        .find(|row| row.id == app.selected())
        .map_or(0, |row| {
            let (prefix, text, _) = tree_row(app, row);
            marquee::width(&text).saturating_sub(tree_available(content, &prefix))
        })
}

/// One row of the tree: its prefix (the indent of its depth and the arrow, `▾ `/`▸ `/`  `), its
/// text (the label of the node and, on a closed node with a marked agent below it, the mark
/// `  ● saída nova`) and the style of the whole row. The drawing and the count of what does not
/// fit (NS-55) both take the row from here.
fn tree_row(app: &App, row: &Visible) -> (String, Vec<Span<'static>>, Style) {
    let (mut text, style) = node_spans(app, row);
    let arrow = match (row.expandable, row.open) {
        (true, true) => "▾ ",
        (true, false) => "▸ ",
        (false, _) => "  ",
    };
    let prefix = format!("{}{arrow}", "  ".repeat(usize::from(row.depth)));
    if row.unseen_below {
        text.push(Span::raw("  ● saída nova"));
    }
    (prefix, text, style)
}

/// The label of a visible row and its style.
fn node_spans(app: &App, row: &Visible) -> (Vec<Span<'static>>, Style) {
    let none = || (vec![Span::raw("?")], Style::new());
    match row.id {
        NodeId::Module(module) => (vec![Span::raw(module.name())], Style::new()),
        NodeId::Project(id) => app.project_row(id).map_or_else(none, project_spans),
        NodeId::Epic(id) => app.epic(id).map_or_else(none, epic_spans),
        NodeId::Ticket(id) => app.ticket(id).map_or_else(none, ticket_spans),
        NodeId::Agent(id) => app
            .agent_record(id)
            .map_or_else(none, |agent| agent_spans(app, agent)),
    }
}

/// The panel on the right of a node that is not a live agent (NS-38): the module, a project,
/// a ticket, or an agent not live; `Nenhum agente ativo.` under the filter without agents. The
/// focus is never on it (`pane_block(false)`).
fn draw_node_panel(frame: &mut Frame, app: &App, area: Rect) {
    let block = pane_block(false);
    let paragraph = |frame: &mut Frame, title: String, lines: Vec<Line<'static>>| {
        frame.render_widget(
            Paragraph::new(lines)
                .block(block.clone().title(title))
                .wrap(Wrap { trim: false }),
            area,
        );
    };
    let (_, only_active) = app.filters();
    match app.selected() {
        NodeId::Module(module) if only_active && app.agents().is_empty() => paragraph(
            frame,
            format!(" {} ", module.name()),
            vec![Line::from("Nenhum agente ativo.")],
        ),
        NodeId::Module(module) => {
            let rows = app.project_rows();
            let lines = if rows.is_empty() {
                vec![Line::from(
                    "Nenhum projeto cadastrado. Pressione a para adicionar.",
                )]
            } else {
                let unavailable = rows
                    .iter()
                    .filter(|row| projects::unavailable_text(row.availability).is_some())
                    .count();
                let mut projects = format!("Projetos: {}", rows.len());
                if unavailable > 0 {
                    projects.push_str(&format!(" ({unavailable} indisponíveis)"));
                }
                vec![
                    Line::from(projects),
                    Line::from(format!("Agentes vivos neste nick: {}", app.agents().len())),
                ]
            };
            paragraph(frame, format!(" {} ", module.name()), lines);
        }
        NodeId::Project(id) => {
            let Some(row) = app.project_row(id) else {
                frame.render_widget(block, area);
                return;
            };
            let tickets = app.project_tickets(id);
            let hidden = tickets.iter().filter(|t| t.hidden_at.is_some()).count();
            let situation = projects::unavailable_text(row.availability).unwrap_or("disponível");
            let mut count = format!("Tickets: {}", tickets.len());
            if hidden > 0 {
                count.push_str(&format!(" ({hidden} ocultos)"));
            }
            let mut lines = vec![
                Line::from(format!(
                    "Caminho: {}",
                    printable(&row.project.path.to_string_lossy())
                )),
                Line::from(format!("Situação: {situation}")),
                Line::from(count),
            ];
            if tickets.is_empty() {
                lines.push(Line::from(""));
                lines.push(Line::from(
                    "Nenhum ticket neste projeto. Pressione a para criar.",
                ));
            }
            paragraph(
                frame,
                format!(" Projeto {} ", printable(&row.project.name)),
                lines,
            );
        }
        NodeId::Epic(_) => match app.epic_detail.as_ref() {
            Some(detail) => paragraph(
                frame,
                format!(" Épico {} ", printable(&detail.epic.key)),
                epic_lines(detail),
            ),
            None => frame.render_widget(block, area),
        },
        NodeId::Ticket(_) => match app.ticket_detail.as_ref() {
            Some(detail) => {
                let epic = detail.ticket.epic_id.and_then(|id| app.epic(id));
                draw_ticket_detail(frame, detail, epic, block, area);
            }
            None => frame.render_widget(block, area),
        },
        NodeId::Agent(id) => {
            let Some(agent) = app.agent_record(id) else {
                frame.render_widget(block, area);
                return;
            };
            let state = agent_state(app, agent);
            let mut lines = vec![
                Line::from(format!("Nome: {}", printable(&agent.name))),
                Line::from(format!(
                    "Etapa: {}",
                    printable(&tickets::stage_label(&agent.stage))
                )),
                Line::from(match AgentState::from_key(&agent.state) {
                    None if app.agent(agent.id).is_none() => {
                        format!("Estado: desconhecido ({})", printable(&agent.state))
                    }
                    _ => format!("Estado: {state}"),
                }),
                Line::from(format!("Aberto em: {}", short_date(&agent.opened_at))),
                // NS-50: the session a resume uses; the one of the opening (the key of its lock)
                // only when the resume follows another session.
                Line::from(format!("Sessão: {}", printable(agent.effective_session()))),
            ];
            if agent.resume_session_id.is_some() {
                lines.push(Line::from(format!(
                    "Sessão na abertura: {}",
                    printable(&agent.session_id)
                )));
            }
            // The ID it was opened with, when it is not the one of its ticket now (NS-39: the
            // resume tells it the new folder), or unknown (before the NS-39, a name without it).
            let now = match agent.owner {
                Owner::Ticket(id) => app.ticket(id).map(|ticket| ticket.key.as_str()),
                Owner::Epic(id) => app.epic(id).map(|epic| epic.key.as_str()),
            };
            match agents::opened_key(agent) {
                None => lines.push(Line::from("ID na abertura: desconhecido")),
                Some(key) if Some(key.as_str()) != now => {
                    lines.push(Line::from(format!("ID na abertura: {}", printable(&key))));
                }
                Some(_) => {}
            }
            lines.push(Line::from(""));
            lines.push(Line::from(match AgentState::from_key(&agent.state) {
                Some(AgentState::Active) => "Ativo em outro nick: feche-o lá para retomar aqui.",
                Some(AgentState::Archived) => "o desarquivar",
                Some(AgentState::Stopped) => "r retomar esta conversa · o arquivar",
                None => "Este nick não conhece o estado deste agente: nada a fazer aqui.",
            }));
            paragraph(frame, " Agente ".to_string(), lines);
        }
    }
}

/// The lines of the panel of an epic (NS-40): the epic and its marks, its folder and its plan, how
/// many tickets it has, and that its agents are only of PO and Tech Leader. No stage, no history
/// and no `m`: an epic has no stage.
fn epic_lines(detail: &EpicDetail) -> Vec<Line<'static>> {
    let epic = &detail.epic;
    let mut head = vec![Span::raw(format!(
        "Épico {} — {}",
        printable(&epic.key),
        printable(&epic.title)
    ))];
    if epic.is_provisional {
        head.push(Span::raw("  [provisório]"));
    }
    if epic.done_at.is_some() {
        head.push(Span::raw("  [concluído]"));
    }
    if epic.hidden_at.is_some() {
        head.push(Span::raw("  [oculto]"));
    }
    // NS-54: open or done, with when.
    let state = match &epic.done_at {
        Some(at) => format!("Estado: concluído em {}", short_date(at)),
        None => "Estado: aberto".to_string(),
    };
    let exists = if detail.artifacts.plan {
        "(existe)"
    } else {
        "(não existe)"
    };
    let mut tickets = format!("Tickets: {}", detail.tickets);
    if detail.hidden_tickets > 0 {
        tickets.push_str(&format!(
            " ({})",
            count_of(detail.hidden_tickets, "oculto", "ocultos")
        ));
    }
    let mut lines = vec![
        Line::from(head),
        Line::from(format!("Pasta: {}", printable(&docs::dir_text(&epic.key)))),
    ];
    // NS-53: its Jira link, when it has one (printable: the base comes from a file another team
    // may edit).
    if let Some(link) = &detail.jira_link {
        lines.push(Line::from(format!("Jira: {}", printable(link))));
    }
    lines.extend([
        Line::from(format!(
            "Plano {exists}:  {}",
            printable(&docs::plan_path(&epic.key))
        )),
        Line::from(tickets),
        Line::from(state),
        Line::from(""),
        Line::from("Agentes do épico: só PO e Tech Leader."),
    ]);
    lines
}

/// The panel of a ticket (the ticket screen of the NS-36, without its agents, which are on the
/// tree since the NS-38): the ticket, its stage and its artifacts on top; then the history of
/// the stage with the rest of the height (the newest when it does not fit, with how many older
/// ones are left out).
/// Since the NS-40 it says the epic of the ticket, when it has one, and since the NS-53 its Jira
/// link, when it has one.
fn draw_ticket_detail(
    frame: &mut Frame,
    detail: &TicketDetail,
    epic: Option<&Epic>,
    block: Block<'static>,
    area: Rect,
) {
    let ticket = &detail.ticket;
    let block = block.title(format!(" Ticket {} ", printable(&ticket.key)));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let mut head = vec![Span::raw(format!(
        "{}  {}",
        printable(&ticket.key),
        printable(&ticket.title)
    ))];
    if ticket.is_provisional {
        head.push(Span::raw("  [provisório]"));
    }
    if ticket.hidden_at.is_some() {
        head.push(Span::raw("  [oculto]"));
    }
    let stage = stage_span(&ticket.stage, false);
    let exists = |there: bool| if there { "(existe)" } else { "(não existe)" };
    let mut top = vec![Line::from(head)];
    if let Some(epic) = epic {
        top.push(Line::from(format!(
            "Épico: {} — {}",
            printable(&epic.key),
            printable(&epic.title)
        )));
    }
    // NS-53: its Jira link, when it has one (printable: the base comes from a file another team
    // may edit).
    if let Some(link) = &detail.jira_link {
        top.push(Line::from(format!("Jira: {}", printable(link))));
    }
    top.extend([
        Line::from(vec![Span::raw("Etapa atual: "), stage]),
        // The state before the path (review of the NS-36, P3.1): the end of a line is cut.
        Line::from(format!(
            "Plano {}:  {}",
            exists(detail.artifacts.plan),
            printable(&docs::plan_path(&ticket.key))
        )),
        Line::from(format!(
            "Review {}:  {}",
            exists(detail.artifacts.review),
            printable(&docs::review_path(&ticket.key))
        )),
        Line::from(""),
    ]);
    let top_rows = u16::try_from(top.len())
        .unwrap_or(u16::MAX)
        .min(inner.height);
    frame.render_widget(
        Paragraph::new(top),
        Rect {
            height: top_rows,
            ..inner
        },
    );
    let history_area = Rect {
        y: inner.y + top_rows,
        height: inner.height - top_rows,
        ..inner
    };
    let mut history = vec![Line::from("Histórico de etapas")];
    if detail.changes.is_empty() {
        // With no change, the stage it started in is the current one (NS-45).
        history.push(Line::from(format!(
            "Nenhuma mudança: o ticket começou em {}.",
            printable(&tickets::stage_label(&ticket.stage))
        )));
    } else {
        let slots = usize::from(history_area.height.saturating_sub(1));
        let changes = &detail.changes[..];
        // NS-61, D10: the cut counts rows (a rejection takes two, with its reason), and keeps the
        // newest changes whole, with a row for the count of the older ones on top.
        let rows = |change: &StageChangeRow| 1 + usize::from(change.reason.is_some());
        let mut shown = changes;
        if changes.iter().map(rows).sum::<usize>() > slots {
            let room = slots.saturating_sub(1);
            let mut used = 0;
            let keep = changes
                .iter()
                .rev()
                .take_while(|change| {
                    used += rows(change);
                    used <= room
                })
                .count();
            let older = changes.len() - keep;
            history.push(Line::from(format!("(+{older} mudanças mais antigas)")));
            shown = &changes[older..];
        }
        for change in shown {
            // The source at the end (NS-45): the date and the stages are not cut at 40 columns.
            history.push(Line::from(format!(
                "  {}  {} → {}  ({})",
                short_date(&change.changed_at),
                printable(&tickets::stage_label(&change.from_stage)),
                printable(&tickets::stage_label(&change.to_stage)),
                printable(&tickets::source_label(&change.source))
            )));
            // NS-61, D10: the reason of a rejection on its own line, right below (the cut at 40
            // columns takes the end of the reason, not the date nor the stages).
            if let Some(reason) = &change.reason {
                history.push(Line::from(format!("    Motivo: {}", printable(reason))));
            }
        }
    }
    frame.render_widget(Paragraph::new(history), history_area);
}

/// How many rows `lines` take wrapped at `width` (an estimate that never falls short by much:
/// each line counts its characters, plus one row of slack for word breaks).
fn wrapped_rows(lines: &[Line], width: u16) -> u16 {
    let width = usize::from(width.max(1));
    lines
        .iter()
        .map(|line| {
            let chars = line.width();
            if chars == 0 {
                1
            } else {
                chars.div_ceil(width) + usize::from(chars > width)
            }
        })
        .sum::<usize>()
        .try_into()
        .unwrap_or(u16::MAX)
}

/// The width of a dialog box inside `area`: as wide as the area allows, up to 90.
fn dialog_width(area: Rect) -> u16 {
    area.width.saturating_sub(4).min(90)
}

/// The end of `typed` that fits in `width` columns with the cursor after it: the whole text
/// when it fits, or "…" and its last characters (the typing point is what matters).
fn input_tail(typed: &str, width: u16) -> String {
    let room = usize::from(width.saturating_sub(1));
    let count = typed.chars().count();
    if count <= room {
        return typed.to_string();
    }
    let keep = room.saturating_sub(1);
    let tail: String = typed.chars().skip(count - keep).collect();
    format!("…{tail}")
}

/// A centered box inside `area` that fits `lines`, `dialog_width` wide.
fn dialog_area(area: Rect, lines: &[Line]) -> Rect {
    let width = dialog_width(area);
    let height = (wrapped_rows(lines, width.saturating_sub(2)) + 2).min(area.height);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}

/// The removal question, with what goes with the project: its agents live in this nick, which
/// are ended (NS-38), its tickets (plan D7 of NS-32) and its agent records, the ended ones too
/// (NS-38). Since the NS-40 its epics too, in a sentence of their own, only when it has any.
/// Without any of them it is the text of the NS-31, byte for byte.
fn removal_question(
    name: &str,
    path: &str,
    tickets: i64,
    agents: i64,
    live: i64,
    epics: i64,
) -> String {
    let ended = match live {
        0 => String::new(),
        1 => "O agente vivo dele neste nick será encerrado. ".to_string(),
        n => format!("Os {n} agentes vivos dele neste nick serão encerrados. "),
    };
    let tickets = match tickets {
        0 => String::new(),
        1 => "O ticket dele será apagado do banco. ".to_string(),
        n => format!("Os {n} tickets dele (inclusive os ocultos) serão apagados do banco. "),
    };
    let epics = match epics {
        0 => String::new(),
        1 => "O épico dele será apagado do banco. ".to_string(),
        n => format!("Os {n} épicos dele (inclusive os ocultos) serão apagados do banco. "),
    };
    let records = match agents {
        0 => String::new(),
        1 => "O agente registrado dele será apagado do banco. ".to_string(),
        n => format!(
            "Os {n} agentes registrados dele (inclusive os parados e os arquivados) serão apagados \
             do banco. "
        ),
    };
    let goes = format!("{ended}{tickets}{epics}{records}");
    format!(
        "Remover o projeto {name} ({path}) do nick? Os arquivos do projeto não são tocados. \
         {goes}(s/N)"
    )
}

/// The question of quitting with agents running: how many, and of which projects (no repeats,
/// in the order they were opened).
fn quit_question(app: &App) -> String {
    let mut projects: Vec<String> = Vec::new();
    for agent in app.agents() {
        let name = printable(&agent.project);
        if !projects.contains(&name) {
            projects.push(name);
        }
    }
    let projects = projects.join(", ");
    match app.agents().len() {
        1 => format!("Há 1 agente ativo ({projects}). Encerrar e sair? (s/N)"),
        n => format!("Há {n} agentes ativos ({projects}). Encerrar todos e sair? (s/N)"),
    }
}

/// The first line of "Abrir agente": `Ticket <ID> — <título>` or, of an epic (NS-40), `Épico <ID>
/// — <título>`.
fn holder_line(holder: &Holder) -> String {
    let noun = match holder.owner {
        Owner::Ticket(_) => "Ticket",
        Owner::Epic(_) => "Épico",
    };
    format!(
        "{noun} {} — {}",
        printable(&holder.key),
        printable(&holder.title)
    )
}

/// The lines of a dialog with a one-line text field: the `header`, a blank line, the `label` and
/// `typed` (the field already cut to the box), with the cursor after it.
fn text_field_lines(
    header: String,
    label: String,
    typed: String,
) -> (Vec<Line<'static>>, Option<(u16, usize)>) {
    let cursor = typed.chars().count();
    (
        vec![
            Line::from(header),
            Line::from(""),
            Line::from(label),
            Line::from(typed),
        ],
        Some((3, cursor)),
    )
}

/// The lines of the dialog of the title of a ticket or of an epic (review of the NS-40, P2.1: one
/// drawing for both).
fn title_lines(key: &str, typed: String) -> (Vec<Line<'static>>, Option<(u16, usize)>) {
    text_field_lines(format!("ID: {}", printable(key)), "Título:".into(), typed)
}

/// The lines of the dialog of the new ID of a ticket or of an epic (`noun`).
fn id_lines(key: &str, noun: &str, typed: String) -> (Vec<Line<'static>>, Option<(u16, usize)>) {
    text_field_lines(
        format!("ID atual: {}", printable(key)),
        format!("Novo ID do {noun} (ex: SC-123):"),
        typed,
    )
}

/// The lines of the dialog of the reason of a rejection (NS-61, D4).
fn reason_lines(ticket: &Ticket, typed: String) -> (Vec<Line<'static>>, Option<(u16, usize)>) {
    text_field_lines(
        format!(
            "Ticket {} — {}",
            printable(&ticket.key),
            printable(&ticket.title)
        ),
        format!(
            "Motivo da rejeição (até {} caracteres):",
            tickets::REJECT_REASON_MAX
        ),
        typed,
    )
}

/// The confirmation of rejecting `ticket` (NS-61, D4): the stage it leaves and the `reason`, then,
/// with `live` agents of it in this nick, the warning of which ones the rejection ends.
fn rejection_lines(
    ticket: &Ticket,
    reason: Option<&str>,
    live: &[(i64, String)],
) -> Vec<Line<'static>> {
    let mut lines = vec![Line::from(format!(
        "Rejeitar o ticket {} (de {})? Motivo: {} (s/N)",
        printable(&ticket.key),
        printable(&tickets::stage_label(&ticket.stage)),
        printable(reason.unwrap_or_default())
    ))];
    let warning = match live {
        [] => None,
        [(_, name)] => Some(format!(
            "Aviso: o agente {} será encerrado.",
            printable(name)
        )),
        _ => Some(format!(
            "Aviso: {} agentes vivos deste ticket serão encerrados: {}.",
            live.len(),
            live.iter()
                .map(|(_, name)| printable(name))
                .collect::<Vec<_>>()
                .join(", ")
        )),
    };
    if let Some(warning) = warning {
        lines.extend([Line::from(""), Line::from(warning)]);
    }
    lines
}

/// The question of the new ID of a ticket or of an epic (`whose`: `o ticket`, `o épico`, what the
/// Jira does not follow), and whether its folder will be renamed.
fn confirm_id_lines(key: &str, whose: &str, new: &str, moves: bool) -> Vec<Line<'static>> {
    let folder = if moves {
        format!(
            " A pasta {} será renomeada para {}.",
            printable(&docs::dir_text(key)),
            printable(&docs::dir_text(new))
        )
    } else {
        String::new()
    };
    vec![Line::from(format!(
        "Trocar o ID {} por {}?{folder} A branch do git, {whose} no Jira e os agentes já \
         registrados não mudam. (s/N)",
        printable(key),
        printable(new)
    ))]
}

fn draw_dialog(frame: &mut Frame, app: &App, dialog: &Dialog, area: Rect) {
    // One-line fields, scrolled to the end (no wrap), so the cursor always shows.
    let inner_width = dialog_width(area).saturating_sub(2);
    let field = |input: &str| input_tail(&printable(input), inner_width);
    // The cursor is (row inside the box, column), after the text of the focused field.
    let (title, lines, cursor) = match dialog {
        Dialog::AddPath { input } => {
            let typed = field(input);
            let cursor = typed.chars().count();
            (
                " Adicionar projeto ",
                vec![
                    Line::from("Caminho da pasta do projeto:"),
                    Line::from(typed),
                ],
                Some((1, cursor)),
            )
        }
        Dialog::NewTicket {
            item,
            title,
            jira_id,
            focus,
            ..
        } => {
            let label = |text: &'static str, mine: TicketField| {
                if *focus == mine {
                    Line::styled(text, Style::new().add_modifier(Modifier::BOLD))
                } else {
                    Line::from(text)
                }
            };
            let (title, jira_id) = (field(title), field(jira_id));
            let cursor = match focus {
                TicketField::Title => (1, title.chars().count()),
                TicketField::JiraId => (4, jira_id.chars().count()),
            };
            (
                match item {
                    NewItem::Ticket { .. } => " Novo ticket ",
                    NewItem::Epic => " Novo épico ",
                },
                vec![
                    label("Título:", TicketField::Title),
                    Line::from(title),
                    Line::from(""),
                    label(
                        "ID do Jira (opcional; vazio gera um ID provisório):",
                        TicketField::JiraId,
                    ),
                    Line::from(jira_id),
                ],
                Some(cursor),
            )
        }
        Dialog::EditTitle { ticket, input } => {
            let (lines, cursor) = title_lines(&ticket.key, field(input));
            (" Editar título ", lines, cursor)
        }
        Dialog::ConfirmHide { ticket } => (
            " Ocultar ticket ",
            vec![Line::from(format!(
                "Ocultar o ticket {} — {}? Ele continua no banco e pode ser restaurado. (s/N)",
                printable(&ticket.key),
                printable(&ticket.title)
            ))],
            None,
        ),
        Dialog::EditEpicTitle { epic, input } => {
            let (lines, cursor) = title_lines(&epic.key, field(input));
            (" Editar título do épico ", lines, cursor)
        }
        Dialog::ConfirmHideEpic { epic, tickets } => (
            " Ocultar épico ",
            vec![Line::from(format!(
                "Ocultar o épico {}? O ramo inteiro ({}) sai da árvore e volta com f. (s/N)",
                printable(&epic.key),
                count_of(*tickets, "ticket", "tickets")
            ))],
            None,
        ),
        Dialog::EditEpicId { epic, input } => {
            let (lines, cursor) = id_lines(&epic.key, "épico", field(input));
            (" Editar ID do épico ", lines, cursor)
        }
        Dialog::ConfirmEditEpicId { epic, new, moves } => (
            " Editar ID do épico ",
            confirm_id_lines(&epic.key, "o épico", new, *moves),
            None,
        ),
        Dialog::MoveTicket {
            ticket,
            choices,
            index,
        } => {
            let mut lines = vec![
                Line::from(format!(
                    "Ticket {} — {}",
                    printable(&ticket.key),
                    printable(&ticket.title)
                )),
                Line::from(""),
                Line::from("Para qual épico?"),
            ];
            for (i, choice) in choices.iter().enumerate() {
                let current = if choice.as_ref().map(|e| e.id) == ticket.epic_id {
                    " (atual)"
                } else {
                    ""
                };
                let label = match choice {
                    None => format!("Sem épico{current}"),
                    Some(epic) => format!(
                        "{} — {}{current}",
                        printable(&epic.key),
                        printable(&epic.title)
                    ),
                };
                lines.push(if i == *index {
                    Line::from(vec![
                        Span::raw("> "),
                        Span::styled(label, Style::new().add_modifier(Modifier::REVERSED)),
                    ])
                } else {
                    Line::from(format!("  {label}"))
                });
            }
            (" Mover ticket ", lines, None)
        }
        Dialog::NeedsConfig { dir, focus, note } => {
            let button = |label: &'static str, button: Button| {
                let style = if *focus == button {
                    Style::new().add_modifier(Modifier::REVERSED)
                } else {
                    Style::new()
                };
                Span::styled(label, style)
            };
            let mut lines = vec![
                Line::from("Esta pasta não tem a configuração do nick."),
                Line::from(""),
                Line::from(printable(&projects::config_file(dir).to_string_lossy())),
                Line::from(""),
            ];
            if let Some(note) = note {
                lines.push(Line::from(printable(note)));
                lines.push(Line::from(""));
            }
            lines.push(Line::from(vec![
                button("[ Criar configuração ]", Button::CreateConfig),
                Span::raw("   "),
                button("[ Cancelar ]", Button::Cancel),
            ]));
            (" Configuração do nick ", lines, None)
        }
        Dialog::ChooseType {
            holder,
            types,
            index,
            ..
        } => {
            let mut lines = vec![
                Line::from(holder_line(holder)),
                Line::from(""),
                Line::from("Qual tipo de conversa você quer iniciar?"),
            ];
            for (i, kind) in types.iter().enumerate() {
                lines.push(if i == *index {
                    Line::from(vec![
                        Span::raw("> "),
                        Span::styled(kind.label(), Style::new().add_modifier(Modifier::REVERSED)),
                    ])
                } else {
                    Line::from(format!("  {}", kind.label()))
                });
            }
            (" Abrir agente ", lines, None)
        }
        Dialog::StageForm {
            holder,
            kind,
            values,
            focus,
            plan_missing,
            ..
        } => {
            let mut lines = vec![
                Line::from(holder_line(holder)),
                Line::from(format!("Tipo: {}", kind.label())),
            ];
            let fields = kind.fields();
            // The plan shows its default with the ID typed now.
            let jira_id = fields
                .iter()
                .position(|f| f.rule == Rule::JiraId)
                .map_or("", |i| values[i].as_str());
            if *plan_missing {
                // Wrapped here, so each row is a line and the cursor row stays right (NS-36).
                let warning = format!(
                    "Aviso: não existe {}. Informe outro caminho no campo do plano, ou siga \
                     sem ele.",
                    docs::plan_path(stages::trim_read(jira_id))
                );
                lines.push(Line::from(""));
                for row in wrap_words(&printable(&warning), inner_width.max(1)) {
                    lines.push(Line::styled(row, Style::new().fg(Color::Yellow)));
                }
            }
            let mut cursor = None;
            for (i, question) in fields.iter().enumerate() {
                lines.push(Line::from(""));
                let label = format!("{}:", stages::field_label(question, jira_id));
                lines.push(if i == *focus {
                    Line::styled(printable(&label), Style::new().add_modifier(Modifier::BOLD))
                } else {
                    Line::from(printable(&label))
                });
                match question.input {
                    Input::Line => {
                        let typed = field(&values[i]);
                        if i == *focus {
                            let row = u16::try_from(lines.len()).unwrap_or(u16::MAX);
                            cursor = Some((row, typed.chars().count()));
                        }
                        lines.push(Line::from(typed));
                    }
                    Input::Editor => lines.push(Line::styled(
                        editor_summary(&values[i]),
                        Style::new().add_modifier(Modifier::DIM),
                    )),
                }
            }
            (" Abrir agente ", lines, cursor)
        }
        // Drawn by `draw_prompt`, over the whole screen.
        Dialog::ConfirmPrompt { .. } => return,
        Dialog::AgentError { message } => (
            " Agente não aberto ",
            message.lines().map(|l| Line::from(printable(l))).collect(),
            None,
        ),
        Dialog::ConfirmEndAgent { name, .. } => (
            " Encerrar agente ",
            vec![Line::from(format!(
                "Encerrar o agente {}? (s/N)",
                printable(name)
            ))],
            None,
        ),
        Dialog::ConfirmJailToggle {
            name,
            release: true,
            ..
        } => (
            " Liberar da jail ",
            vec![Line::from(format!(
                "Liberar o agente {} da jail? Ele reinicia e fica SEM a proteção da jail até \
                 fechar. (s/N)",
                printable(name)
            ))],
            None,
        ),
        Dialog::ConfirmJailToggle {
            name,
            release: false,
            ..
        } => (
            " Prender na jail ",
            vec![Line::from(format!(
                "Prender o agente {} na jail de novo? Ele reinicia e volta para a jail do \
                 projeto. (s/N)",
                printable(name)
            ))],
            None,
        ),
        Dialog::ConversationLost { agent, .. } => (
            " Conversa não encontrada ",
            vec![Line::from(format!(
                "A conversa do agente {} não existe mais no claude (o histórico foi apagado ou \
                 o projeto mudou de pasta). Nada foi aberto.",
                printable(&agent.name)
            ))],
            None,
        ),
        Dialog::ConfirmArchive { agent } => (
            " Arquivar agente ",
            vec![Line::from(format!(
                "Arquivar o agente {}? Ele sai da árvore e volta com f. (s/N)",
                printable(&agent.name)
            ))],
            None,
        ),
        Dialog::ConfirmQuit => (" Sair do nick ", vec![Line::from(quit_question(app))], None),
        Dialog::CaptureShortcut { first } => (
            " Trocar atalho ",
            vec![Line::from(match first {
                None => "Aperte a tecla do novo atalho. Esc cancela.".to_string(),
                Some(shortcut) => {
                    format!("Aperte {} de novo para confirmar.", shortcut.label())
                }
            })],
            None,
        ),
        Dialog::ChooseTicketStage { ticket, index } => {
            let mut lines = vec![
                Line::from(format!(
                    "Ticket {} — {}",
                    printable(&ticket.key),
                    printable(&ticket.title)
                )),
                Line::from(""),
                Line::from("Para qual etapa?"),
            ];
            for (i, stage) in TicketStage::ALL.iter().enumerate() {
                let current = if stage.key() == ticket.stage {
                    " (atual)"
                } else {
                    ""
                };
                let label = format!("{}{current}", stage.label());
                lines.push(if i == *index {
                    Line::from(vec![
                        Span::raw("> "),
                        Span::styled(label, Style::new().add_modifier(Modifier::REVERSED)),
                    ])
                } else {
                    Line::from(format!("  {label}"))
                });
            }
            (" Mudar etapa ", lines, None)
        }
        Dialog::RejectReason { ticket, input } => {
            let (lines, cursor) = reason_lines(ticket, field(input));
            (" Rejeitar ticket ", lines, cursor)
        }
        Dialog::ConfirmStageChange {
            ticket,
            to: TicketStage::Rejected,
            reason,
            live,
        } => (
            " Rejeitar ticket ",
            rejection_lines(ticket, reason.as_deref(), live),
            None,
        ),
        Dialog::ConfirmStageChange { ticket, to, .. } => (
            " Mudar etapa ",
            vec![Line::from(format!(
                "Mudar a etapa do ticket {} de {} para {}? (s/N)",
                printable(&ticket.key),
                printable(&tickets::stage_label(&ticket.stage)),
                to.label()
            ))],
            None,
        ),
        Dialog::EditId { ticket, input } => {
            let (lines, cursor) = id_lines(&ticket.key, "ticket", field(input));
            (" Editar ID ", lines, cursor)
        }
        Dialog::ConfirmEditId { ticket, new, moves } => (
            " Editar ID ",
            confirm_id_lines(&ticket.key, "o ticket", new, *moves),
            None,
        ),
        Dialog::ConfirmRemove {
            project,
            tickets,
            agents,
            live,
            epics,
        } => (
            " Remover projeto ",
            vec![Line::from(removal_question(
                &printable(&project.name),
                &printable(&project.path.to_string_lossy()),
                *tickets,
                *agents,
                *live,
                *epics,
            ))],
            None,
        ),
    };
    let popup = dialog_area(area, &lines);
    if popup.width < 3 || popup.height < 3 {
        return;
    }
    frame.render_widget(Clear, popup);
    let block = dialog_block(dialog).title(title);
    let inner = block.inner(popup);
    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false }),
        popup,
    );
    if let Some((row, column)) = cursor {
        // The cursor after the typed text of the field, as long as it fits in the box.
        let column = u16::try_from(column).unwrap_or(u16::MAX);
        if column < inner.width && inner.height > row {
            frame.set_cursor_position(Position::new(inner.x + column, inner.y + row));
        }
    }
}

/// An editor field in the form: `(vazio — Enter abre o editor)`, or its first line and how
/// many lines it has.
fn editor_summary(text: &str) -> String {
    if text.is_empty() {
        return "(vazio — Enter abre o editor)".to_string();
    }
    let count = text.lines().count().max(1);
    let first = printable(text.lines().next().unwrap_or(""));
    let lines = if count == 1 {
        "1 linha".to_string()
    } else {
        format!("{count} linhas")
    };
    format!("{first} ({lines}; Enter edita)")
}

/// The keys of the preview of the prompt.
const PROMPT_KEYS: &str = "↑/↓/PgUp/PgDn/Home/End rolar · s enviar · n/Esc cancelar";

/// `line` broken into rows of at most `width` columns, at the last space that fits when there is
/// one (the spaces are kept, unlike `wrap_words`: the indentation of the prompt shows). Linear:
/// each row looks at most at the characters that fit in it, measured on slices of `line` (review
/// of the NS-35: a long description reaches 128 KiB).
fn wrap_keeping_spaces(line: &str, width: u16) -> Vec<String> {
    let width = usize::from(width.max(1));
    if line.is_empty() {
        return vec![String::new()];
    }
    let mut rows = Vec::new();
    let mut start = 0;
    while start < line.len() {
        let rest = &line[start..];
        let mut used = 0;
        let mut end = start;
        // Where the row ends when it breaks at the last space (after it), a space that is not
        // the first character of the row.
        let mut after_space = None;
        for (at, c) in rest.char_indices() {
            let size = Span::raw(&rest[at..at + c.len_utf8()]).width();
            // A character wider than the row still goes, alone.
            if used + size > width && end > start {
                break;
            }
            used += size;
            end = start + at + c.len_utf8();
            if c == ' ' && at > 0 {
                after_space = Some(end);
            }
        }
        if end < line.len()
            && let Some(cut) = after_space
        {
            end = cut;
        }
        rows.push(line[start..end].to_string());
        start = end;
    }
    rows
}

/// `line` with each tab turned into the spaces up to the next column multiple of 8, as a
/// terminal shows it: in the preview only (`printable` would show a `?`), the prompt itself keeps
/// the tab.
fn expand_tabs(line: &str) -> String {
    if !line.contains('\t') {
        return line.to_string();
    }
    let mut out = String::with_capacity(line.len() + 8);
    let mut column = 0;
    for c in line.chars() {
        if c == '\t' {
            let spaces = 8 - column % 8;
            out.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            out.push(c);
            column += Span::raw(c.encode_utf8(&mut [0; 4]) as &str).width();
        }
    }
    out
}

/// Where the preview of the prompt goes on a screen of `area`: everything above the help line
/// (the keys of the preview are there, as in the other dialogs), the status row included.
fn prompt_area(area: Rect, areas: Areas) -> Rect {
    Rect {
        height: areas.help.y.saturating_sub(area.y),
        ..area
    }
}

/// Where the parts of the preview of the prompt go in `area` (`prompt_area`).
struct PromptLayout {
    /// The bordered box, with a margin of one cell.
    outer: Rect,
    /// On top: the jail status (and `FORA DA JAIL` out of it) and the warnings.
    top: Vec<Line<'static>>,
    top_area: Rect,
    /// The prompt, word wrapped: every row, of which the window shows `body.height`.
    prompt: Vec<String>,
    body: Rect,
    bottom_area: Rect,
}

/// The rows of the top of the preview: out of the jail, the warning in red and bold (as in the
/// header of an agent); the status lines of `jail_print_status`; and the warnings of the render.
fn prompt_top(precheck: &Precheck, rendered: &Rendered, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if !precheck.jail {
        let red = Style::new().fg(Color::Red).add_modifier(Modifier::BOLD);
        for row in wrap_words(&format!("{OFF_JAIL}."), width) {
            lines.push(Line::styled(row, red));
        }
    }
    let texts = rendered.status.lines().chain(
        rendered
            .warning
            .as_deref()
            .map_or("", |warning| warning)
            .lines(),
    );
    for text in texts {
        for row in wrap_words(&printable(text), width) {
            lines.push(Line::from(row));
        }
    }
    lines
}

/// The bottom of the preview: where the window is, the question of auto_scrum.sh and where the
/// log is (the keys are on the help line).
fn prompt_bottom(rendered: &Rendered, scroll: usize, shown: usize, total: usize) -> Vec<String> {
    let last = (scroll + shown).min(total);
    vec![
        format!(
            "linhas {}–{last} de {total}",
            (scroll + 1).min(total.max(1))
        ),
        "Confirma o envio para o claude? (s/N)".to_string(),
        format!("Salvo em {}", printable(&rendered.log.to_string_lossy())),
    ]
}

fn prompt_layout(area: Rect, precheck: &Precheck, rendered: &Rendered) -> PromptLayout {
    let outer = Rect {
        x: area.x + 1.min(area.width / 2),
        y: area.y + 1.min(area.height / 2),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    };
    let inner = Block::bordered().inner(outer);
    let width = inner.width.max(1);
    let third = (inner.height / 3).max(1);
    let top = prompt_top(precheck, rendered, width);
    let top_rows = u16::try_from(top.len()).unwrap_or(u16::MAX).min(third);
    // The bottom: its 3 lines, wrapped at most to a third.
    let bottom_rows: u16 = prompt_bottom(rendered, 0, 0, 0)
        .iter()
        .map(|line| u16::try_from(wrap_words(line, width).len()).unwrap_or(u16::MAX))
        .sum::<u16>()
        .min(third);
    // A blank row under the top separates it from the prompt.
    let gap = u16::from(top_rows > 0);
    let body_rows = inner.height.saturating_sub(top_rows + gap + bottom_rows);
    let prompt = rendered
        .prompt
        .lines()
        .flat_map(|line| wrap_keeping_spaces(&printable(&expand_tabs(line)), width))
        .collect();
    PromptLayout {
        outer,
        top,
        top_area: Rect {
            height: top_rows,
            ..inner
        },
        prompt,
        body: Rect {
            y: inner.y + top_rows + gap,
            height: body_rows,
            ..inner
        },
        bottom_area: Rect {
            y: inner.y + top_rows + gap + body_rows,
            height: bottom_rows,
            ..inner
        },
    }
}

/// The window of the preview of the prompt open in `app`, on a screen of `area`: the rows of the
/// prompt shown at a time (a page) and the largest scroll (its end), for the loop to give the App
/// (`App::set_prompt_window`) before each key; None without the preview.
pub fn prompt_window(app: &App, area: Rect) -> Option<(usize, usize)> {
    let (precheck, rendered) = app.prompt_preview()?;
    let layout = prompt_layout(prompt_area(area, layout(app, area)), precheck, rendered);
    let rows = usize::from(layout.body.height);
    Some((rows, layout.prompt.len().saturating_sub(rows)))
}

/// The preview of the prompt (NS-35), over the screen above the help line (cleared whole, so
/// nothing of the list behind shows), in a box with a margin of one cell: the jail status and the
/// warnings on top, the prompt in the middle (word wrapped, from the row `scroll`), and the
/// position, the `(s/N)` question and the log at the bottom.
fn draw_prompt(
    frame: &mut Frame,
    area: Rect,
    dialog: &Dialog,
    precheck: &Precheck,
    rendered: &Rendered,
    scroll: usize,
) {
    frame.render_widget(Clear, area);
    let layout = prompt_layout(area, precheck, rendered);
    if layout.outer.width < 3 || layout.outer.height < 3 {
        return;
    }
    frame.render_widget(dialog_block(dialog).title(" Prompt final "), layout.outer);
    frame.render_widget(Paragraph::new(layout.top.clone()), layout.top_area);
    let rows = usize::from(layout.body.height);
    let scroll = scroll.min(layout.prompt.len().saturating_sub(rows));
    let shown: Vec<Line> = layout
        .prompt
        .iter()
        .skip(scroll)
        .take(rows)
        .map(|row| Line::from(row.as_str()))
        .collect();
    let count = shown.len();
    frame.render_widget(Paragraph::new(shown), layout.body);
    let bottom: Vec<Line> = prompt_bottom(rendered, scroll, count, layout.prompt.len())
        .iter()
        .flat_map(|line| wrap_words(line, layout.bottom_area.width))
        .map(Line::from)
        .collect();
    frame.render_widget(Paragraph::new(bottom), layout.bottom_area);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Button, Dialog, Focus, NewItem, Row, Screen, TicketField};
    use crate::db::Db;
    use crate::projects::{Availability, Project};
    use crate::stages::StageType;
    use crate::tickets::Ticket;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::{Color, Modifier};
    use std::path::PathBuf;

    struct Fixture {
        _tmp: tempfile::TempDir,
        app: App,
    }

    fn fixture() -> Fixture {
        let tmp = tempfile::tempdir().unwrap();
        let db = Db::open(&tmp.path().join("nick.db")).unwrap();
        let app = App::new(db, None, None, PathBuf::from("/repo/bin/nick-init"));
        Fixture { _tmp: tmp, app }
    }

    fn project(id: i64, name: &str, path: &str) -> Project {
        Project {
            id,
            name: name.into(),
            path: PathBuf::from(path),
            created_at: "2026-11-17T00:00:00Z".into(),
        }
    }

    fn render(app: &App) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// The rows of the buffer as text, one line per row, trailing spaces removed.
    fn text(buffer: &Buffer) -> String {
        let area = buffer.area;
        (0..area.height)
            .map(|y| {
                let line: String = (0..area.width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect();
                line.trim_end().to_string()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The row index where `needle` appears, and its column.
    fn find(buffer: &Buffer, needle: &str) -> Option<(u16, u16)> {
        text(buffer).lines().enumerate().find_map(|(y, line)| {
            line.find(needle).map(|byte| {
                let x = line[..byte].chars().count();
                (y as u16, x as u16)
            })
        })
    }

    fn assert_has(buffer: &Buffer, needle: &str) {
        assert!(
            find(buffer, needle).is_some(),
            "{needle:?} not on the screen:\n{}",
            text(buffer)
        );
    }

    /// The last row with text (NS-54: the help may keep more than one empty row below it).
    fn last_line(buffer: &Buffer) -> String {
        text(buffer)
            .trim_end()
            .lines()
            .last()
            .unwrap_or("")
            .to_string()
    }

    #[test]
    fn the_status_message_is_shown_above_the_help() {
        let mut f = fixture();
        f.app.status = Some("A pasta x não existe.".into());
        let buffer = render(&f.app);
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        let help = help_lines(&f.app, 100).len();
        assert_eq!(
            lines[lines.len() - 1 - help].trim(),
            "A pasta x não existe."
        );
    }

    /// A status longer than the screen wraps (NS-32: decision of the user), above the help line:
    /// none of it is cut. A short one still takes one row.
    #[test]
    fn a_long_status_message_wraps_above_the_help() {
        let mut f = fixture();
        let message = "Projeto indisponível: [indisponível: sem .nick/nick.conf]. Os tickets só \
                       abrem com a pasta e o .nick/nick.conf no lugar.";
        assert!(message.chars().count() > 100);
        f.app.status = Some(message.into());
        let buffer = render(&f.app);
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        // The help of the module takes two rows at 100 columns (NS-38).
        let help = help_lines(&f.app, 100);
        assert_eq!(help.len(), 2);
        let n = lines.len() - help.len() + 1;
        assert_eq!(lines[n - 1].trim(), help[0]);
        assert_eq!(
            format!("{} {}", lines[n - 3].trim(), lines[n - 2].trim()),
            message,
            "two rows, word wrapped"
        );
        assert!(lines[n - 4].contains('┗'), "the tree ends right above");
        // A word longer than the width is broken, not lost.
        let long = format!("Esse projeto já está cadastrado: /{}.", "x".repeat(150));
        f.app.status = Some(long.clone());
        let buffer = render(&f.app);
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        let n = lines.len() - help.len() + 1;
        let joined: String = lines[n - 4..n - 1]
            .iter()
            .map(|l| l.trim())
            .collect::<Vec<_>>()
            .join("");
        assert_eq!(joined.replace(' ', ""), long.replace(' ', ""));
    }

    #[test]
    fn wrap_words_breaks_at_spaces_and_splits_long_words() {
        assert_eq!(wrap_words("abc def ghi", 7), ["abc def", "ghi"]);
        assert_eq!(wrap_words("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        assert_eq!(wrap_words("curto", 80), ["curto"]);
        assert_eq!(wrap_words("", 10), [""]);
        assert_eq!(wrap_words("é ção", 3), ["é", "ção"]);
    }

    #[test]
    fn the_text_field_shows_the_label_and_what_was_typed() {
        let mut f = fixture();
        f.app.dialog = Some(Dialog::AddPath {
            input: "~/proj q".into(),
        });
        let buffer = render(&f.app);
        assert_has(&buffer, "Caminho da pasta do projeto:");
        assert_has(&buffer, "~/proj q");
        assert_eq!(last_line(&buffer).trim(), "Enter confirmar · Esc cancelar");
    }

    /// A path longer than the box scrolls: its end stays visible on one line, after a "…",
    /// with the cursor right after it (review NS-31, P5).
    #[test]
    fn a_long_path_in_the_text_field_keeps_its_end_and_the_cursor_visible() {
        let mut f = fixture();
        let path = format!("/home/u/{}/fim-do-caminho", "x".repeat(97));
        assert_eq!(path.chars().count(), 120);
        f.app.dialog = Some(Dialog::AddPath {
            input: path.clone(),
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &f.app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let (label_y, label_x) = find(&buffer, "Caminho da pasta do projeto:").unwrap();
        let line = text(&buffer)
            .lines()
            .nth(usize::from(label_y) + 1)
            .unwrap()
            .to_string();
        let inside: String = line.chars().skip(usize::from(label_x)).collect();
        let inside = inside.split('┃').next().unwrap().trim_end().to_string();
        assert!(inside.starts_with('…'), "{inside:?}");
        assert!(inside.ends_with("/fim-do-caminho"), "{inside:?}");
        assert!(path.ends_with(&inside[inside.char_indices().nth(1).unwrap().0..]));
        // The next row is the bottom border: nothing wrapped.
        let below = text(&buffer)
            .lines()
            .nth(usize::from(label_y) + 2)
            .unwrap()
            .to_string();
        assert!(below.contains('┗'), "{below:?}");
        let width = u16::try_from(inside.chars().count()).unwrap();
        assert!(terminal.backend().cursor_visible(), "the cursor is shown");
        terminal
            .backend_mut()
            .assert_cursor_position((label_x + width, label_y + 1));
        // A short one is shown whole, without the "…".
        f.app.dialog = Some(Dialog::AddPath {
            input: "~/p".into(),
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &f.app)).unwrap();
        terminal
            .backend_mut()
            .assert_cursor_position((label_x + 3, label_y + 1));
        assert_has(terminal.backend().buffer(), "~/p");
        assert!(find(terminal.backend().buffer(), "…").is_none());
    }

    #[test]
    fn the_alert_shows_the_full_config_path_and_both_buttons() {
        let mut f = fixture();
        let dir = format!("/home/u/{}/projeto", "muito-longo".repeat(8));
        f.app.dialog = Some(Dialog::NeedsConfig {
            dir: PathBuf::from(&dir),
            focus: Button::Cancel,
            note: Some("O nick-init falhou (código 1). Nada foi cadastrado.".into()),
        });
        let buffer = render(&f.app);
        assert_has(&buffer, "Esta pasta não tem a configuração do nick.");
        // The path may wrap: compare the screen without line breaks nor borders.
        let flat: String = text(&buffer)
            .lines()
            .map(|l| l.replace(['│', '┃'], "").trim().to_string())
            .collect();
        assert!(
            flat.contains(&format!("{dir}/.nick/nick.conf")),
            "full path:\n{}",
            text(&buffer)
        );
        assert_has(
            &buffer,
            "O nick-init falhou (código 1). Nada foi cadastrado.",
        );
        assert_has(&buffer, "Criar configuração");
        assert_has(&buffer, "Cancelar");
        // The focused button is reversed, the other is not.
        let (y, x) = find(&buffer, "Cancelar").unwrap();
        assert!(buffer[(x, y)].modifier.contains(Modifier::REVERSED));
        let (y, x) = find(&buffer, "Criar configuração").unwrap();
        assert!(!buffer[(x, y)].modifier.contains(Modifier::REVERSED));
        assert_eq!(
            last_line(&buffer).trim(),
            "←/→/Tab escolher · Enter acionar · Esc cancelar"
        );
    }

    #[test]
    fn the_removal_confirmation_names_the_project_and_s_n() {
        let mut f = fixture();
        f.app.dialog = Some(Dialog::ConfirmRemove {
            project: project(1, "alfa", "/w/alfa"),
            tickets: 0,
            agents: 0,
            live: 0,
            epics: 0,
        });
        let buffer = render(&f.app);
        let flat = flat(&buffer);
        assert!(
            flat.contains(
                "Remover o projeto alfa (/w/alfa) do nick? Os arquivos do projeto não são \
                 tocados. (s/N)"
            ),
            "{flat}"
        );
        assert_eq!(
            last_line(&buffer).trim(),
            "s remover · qualquer outra tecla cancela"
        );
    }

    // ---- tickets (NS-32) ----

    fn ticket(id: i64, key: &str, title: &str, provisional: bool, hidden: bool) -> Ticket {
        Ticket {
            id,
            project_id: 1,
            key: key.into(),
            is_provisional: provisional,
            title: title.into(),
            created_at: "2026-11-18T00:00:00Z".into(),
            hidden_at: hidden.then(|| "2026-11-18T01:00:00Z".into()),
            stage: "po".into(),
            epic_id: None,
        }
    }

    /// The text of the dialog box, without borders, as one line.
    fn flat(buffer: &Buffer) -> String {
        text(buffer)
            .lines()
            .map(|l| l.replace(['│', '┃'], "").trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_new_ticket_dialog_has_both_fields_and_the_cursor_on_the_focused_one() {
        let mut f = fixture();
        for (focus, typed) in [
            (TicketField::Title, "Meu título"),
            (TicketField::JiraId, "NS-12"),
        ] {
            f.app.dialog = Some(Dialog::NewTicket {
                project_id: 1,
                item: NewItem::Ticket { epic_id: None },
                title: "Meu título".into(),
                jira_id: "NS-12".into(),
                focus,
            });
            let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
            terminal.draw(|frame| draw(frame, &f.app)).unwrap();
            let buffer = terminal.backend().buffer().clone();
            assert_has(&buffer, "Novo ticket");
            assert_has(&buffer, "Título:");
            assert_has(
                &buffer,
                "ID do Jira (opcional; vazio gera um ID provisório):",
            );
            let (y, x) = find(&buffer, typed).unwrap();
            let width = u16::try_from(typed.chars().count()).unwrap();
            assert!(terminal.backend().cursor_visible());
            terminal
                .backend_mut()
                .assert_cursor_position((x + width, y));
            assert_eq!(
                last_line(&buffer).trim(),
                "Tab trocar de campo · Enter confirmar · Esc cancelar"
            );
        }
    }

    #[test]
    fn the_edit_title_dialog_shows_the_fixed_id_and_the_cursor() {
        let mut f = fixture();
        f.app.dialog = Some(Dialog::EditTitle {
            ticket: ticket(1, "NS-1", "Velho", false, false),
            input: "Novo".into(),
        });
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| draw(frame, &f.app)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        assert_has(&buffer, "Editar título");
        assert_has(&buffer, "ID: NS-1");
        assert_has(&buffer, "Título:");
        let (y, x) = find(&buffer, "Novo").unwrap();
        terminal.backend_mut().assert_cursor_position((x + 4, y));
        assert_eq!(last_line(&buffer).trim(), "Enter confirmar · Esc cancelar");
    }

    #[test]
    fn the_hide_confirmation_names_the_ticket_and_s_n() {
        let mut f = fixture();
        f.app.dialog = Some(Dialog::ConfirmHide {
            ticket: ticket(1, "NS-1", "Corrigir", false, false),
        });
        let buffer = render(&f.app);
        assert!(
            flat(&buffer).contains(
                "Ocultar o ticket NS-1 — Corrigir? Ele continua no banco e pode ser \
                 restaurado. (s/N)"
            ),
            "{}",
            flat(&buffer)
        );
        assert_eq!(
            last_line(&buffer).trim(),
            "s ocultar · qualquer outra tecla cancela"
        );
    }

    #[test]
    fn the_removal_confirmation_counts_one_or_more_tickets() {
        let mut f = fixture();
        for (tickets, sentence) in [
            (1, "O ticket dele será apagado do banco."),
            (
                3,
                "Os 3 tickets dele (inclusive os ocultos) serão apagados do banco.",
            ),
        ] {
            f.app.dialog = Some(Dialog::ConfirmRemove {
                project: project(1, "alfa", "/w/alfa"),
                tickets,
                agents: 0,
                live: 0,
                epics: 0,
            });
            let buffer = render(&f.app);
            assert!(
                flat(&buffer).contains(&format!(
                    "Remover o projeto alfa (/w/alfa) do nick? Os arquivos do projeto não são \
                     tocados. {sentence} (s/N)"
                )),
                "{}",
                flat(&buffer)
            );
        }
    }

    /// NS-38 (decision of the user): the agents go with the project. The live ones of this nick
    /// are ended; every record (the ended ones too) leaves the database.
    #[test]
    fn the_removal_confirmation_says_which_agents_go_with_the_project() {
        let mut f = fixture();
        for (tickets, agents, live, sentence) in [
            (
                1,
                1,
                1,
                "O agente vivo dele neste nick será encerrado. O ticket dele será apagado do \
                 banco. O agente registrado dele será apagado do banco.",
            ),
            (
                3,
                4,
                2,
                "Os 2 agentes vivos dele neste nick serão encerrados. Os 3 tickets dele \
                 (inclusive os ocultos) serão apagados do banco. Os 4 agentes registrados dele \
                 (inclusive os parados e os arquivados) serão apagados do banco.",
            ),
            (
                2,
                3,
                0,
                "Os 2 tickets dele (inclusive os ocultos) serão apagados do banco. Os 3 agentes \
                 registrados dele (inclusive os parados e os arquivados) serão apagados do banco.",
            ),
        ] {
            f.app.dialog = Some(Dialog::ConfirmRemove {
                project: project(1, "alfa", "/w/alfa"),
                tickets,
                agents,
                live,
                epics: 0,
            });
            let buffer = render(&f.app);
            assert!(
                flat(&buffer).contains(&format!(
                    "Remover o projeto alfa (/w/alfa) do nick? Os arquivos do projeto não são \
                     tocados. {sentence} (s/N)"
                )),
                "{}",
                flat(&buffer)
            );
        }
    }

    #[test]
    fn control_characters_are_shown_as_question_marks() {
        let mut f = fixture();
        f.app.rows = vec![Row {
            project: project(1, "a\x1b[2Jb", "/w/a\x1b[2Jb"),
            availability: Availability::DirMissing,
            jira_url: None,
            jail: None,
        }];
        f.app.tree.open(MODULE);
        f.app.relayout();
        f.app.select(NodeId::Project(1));
        f.app.status = Some("A pasta x\ny não existe.".into());
        let buffer = render(&f.app);
        assert_has(&buffer, "a?[2Jb  [indisponível");
        assert_has(&buffer, "Projeto a?[2Jb");
        assert_has(&buffer, "Caminho: /w/a?[2Jb");
        assert_has(&buffer, "A pasta x?y não existe.");
        f.app.dialog = Some(Dialog::AddPath {
            input: "a\tb".into(),
        });
        assert_has(&render(&f.app), "a?b");
    }

    #[test]
    fn a_tiny_terminal_does_not_panic() {
        let mut f = fixture();
        f.app.dialog = Some(Dialog::NeedsConfig {
            dir: PathBuf::from("/x"),
            focus: Button::CreateConfig,
            note: None,
        });
        for (w, h) in [(1, 1), (5, 3), (20, 4)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|frame| draw(frame, &f.app)).unwrap();
        }
    }

    // ---- agents (NS-33) ----

    /// The panel of an agent over a 100x30 screen, with the focus. Its border (NS-37) takes
    /// the first and the last row and column: the header is on row 1, from column 1.
    fn render_agent(name: &str, jail: bool, screen: Option<&vt100::Screen>) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| draw_agent(frame, frame.area(), name, jail, true, screen))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The row `y` of `buffer` inside the border of the panel (NS-37): from column 1, without
    /// the right side, trailing spaces removed.
    fn inside_row(buffer: &Buffer, y: u16) -> String {
        let width = usize::from(buffer.area.width);
        let row = text(buffer)
            .lines()
            .nth(usize::from(y))
            .unwrap_or("")
            .to_string();
        let inner: String = row.chars().skip(1).take(width.saturating_sub(2)).collect();
        inner.trim_end().to_string()
    }

    #[test]
    fn the_agent_header_in_the_jail() {
        let buffer = render_agent("proj · Dev · NS-1", true, None);
        assert_eq!(inside_row(&buffer, 1), "proj · Dev · NS-1 · na jail");
        let (y, x) = find(&buffer, "na jail").unwrap();
        assert_ne!(buffer[(x, y)].fg, Color::Red);
        assert!(find(&buffer, "FORA DA JAIL").is_none());
    }

    #[test]
    fn the_agent_header_out_of_the_jail_is_red_and_bold() {
        let buffer = render_agent("proj · Dev · NS-1", false, None);
        assert_eq!(
            inside_row(&buffer, 1),
            "proj · Dev · NS-1 · FORA DA JAIL: o agente roda direto no seu shell."
        );
        let (y, x) = find(&buffer, "FORA DA JAIL").unwrap();
        let end = x + "FORA DA JAIL: o agente roda direto no seu shell"
            .chars()
            .count() as u16;
        for column in x..end {
            assert_eq!(buffer[(column, y)].fg, Color::Red, "column {column}");
            assert!(buffer[(column, y)].modifier.contains(Modifier::BOLD));
        }
    }

    #[test]
    fn the_agent_terminal_takes_the_rest_of_the_screen() {
        // The header takes one row when it fits, and the terminal the rest (in a panel of one
        // row, nothing).
        let name = "proj · PO · NS-1";
        assert_eq!(header_rows(name, Rect::new(0, 0, 100, 30)), 1);
        assert_eq!(header_rows(name, Rect::new(0, 0, 10, 1)), 1);
        // Inside the border (NS-37): 98x28, the header on its first row.
        let mut parser = vt100::Parser::new(27, 98, 0);
        parser.process(b"\x1b[31mlinha um\x1b[0m\r\nlinha dois\x1b[27;1Hfim");
        let buffer = render_agent("proj · PO · NS-1", true, Some(parser.screen()));
        assert!(inside_row(&buffer, 2).starts_with("linha um"));
        assert!(inside_row(&buffer, 3).starts_with("linha dois"));
        assert_eq!(
            buffer[(1, 2)].fg,
            Color::Indexed(1),
            "the colors of the program"
        );
        // No status line nor help: the last row of the program is the last one inside the
        // border, above its bottom.
        assert_eq!(find(&buffer, "fim"), Some((28, 1)), "{}", text(&buffer));
        assert_eq!(buffer[(0, 29)].symbol(), "┗");
    }

    fn precheck(jail: bool) -> Precheck {
        Precheck {
            jail,
            jira: false,
            sentry: false,
            jira_url: None,
            warning: None,
        }
    }

    #[test]
    fn the_menu_lists_the_types_with_the_selection() {
        let mut f = fixture();
        let t = ticket(1, "NS-1", "Corrigir o login", false, false);
        f.app.dialog = Some(Dialog::ChooseType {
            project: PathBuf::from("/x/meu-proj"),
            holder: (&t).into(),
            precheck: precheck(false),
            types: StageType::menu(true, false),
            index: 5,
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Abrir agente ");
        assert_has(&buffer, "Ticket NS-1 — Corrigir o login");
        assert_has(&buffer, "Qual tipo de conversa você quer iniciar?");
        let rows: Vec<u16> = StageType::menu(true, false)
            .iter()
            .map(|kind| {
                find(&buffer, kind.label())
                    .unwrap_or_else(|| panic!("{}", kind.label()))
                    .0
            })
            .collect();
        assert!(rows.windows(2).all(|w| w[0] < w[1]), "{rows:?}");
        let (dev, x) = find(&buffer, "> Desenvolvimento").unwrap();
        assert!(buffer[(x + 2, dev)].modifier.contains(Modifier::REVERSED));
        assert!(find(&buffer, "Sentry").is_none(), "no Sentry type off");
        assert_eq!(
            last_line(&buffer).trim(),
            "↑/↓ escolher · Enter abrir · Esc cancelar"
        );
    }

    /// The form of `kind` with `values`, `focus` on the field.
    fn form(kind: StageType, values: &[&str], focus: usize) -> Fixture {
        let mut f = fixture();
        let t = ticket(1, "NS-1", "Corrigir o login", false, false);
        f.app.dialog = Some(Dialog::StageForm {
            project: PathBuf::from("/x/meu-proj"),
            holder: (&t).into(),
            precheck: precheck(false),
            kind,
            values: values.iter().map(|v| v.to_string()).collect(),
            focus,
            plan_missing: false,
        });
        f
    }

    #[test]
    fn the_form_shows_each_question_with_the_editor_field_empty_or_filled() {
        let f = form(StageType::TechLeader, &["NS-1", "Título x", ""], 1);
        let buffer = render(&f.app);
        assert_has(&buffer, "Tipo: Tech Leader - Criar plano");
        let (id, _) = find(&buffer, "ID do Jira (ex: SC-123):").unwrap();
        let (title, _) = find(&buffer, "Título da tarefa:").unwrap();
        let (description, _) = find(&buffer, "Descrição / contexto:").unwrap();
        assert!(id < title && title < description);
        let (_, x) = find(&buffer, "Título da tarefa:").unwrap();
        assert!(
            buffer[(x, title)].modifier.contains(Modifier::BOLD),
            "focused"
        );
        assert_has(&buffer, "(vazio — Enter abre o editor)");
        let f = form(
            StageType::TechLeader,
            &["NS-1", "Título x", "Primeira linha\nsegunda\nterceira"],
            2,
        );
        let buffer = render(&f.app);
        assert_has(&buffer, "Primeira linha (3 linhas; Enter edita)");
        assert!(find(&buffer, "segunda").is_none(), "only the first line");
        let f = form(StageType::Po, &["", "uma"], 1);
        assert_has(&render(&f.app), "uma (1 linha; Enter edita)");
        assert!(
            last_line(&render(&f.app)).contains("Tab/↑/↓ trocar de campo"),
            "{}",
            last_line(&render(&f.app))
        );
    }

    #[test]
    fn the_label_of_the_plan_shows_the_current_id() {
        let f = form(StageType::Development, &["SC-42", ""], 0);
        let buffer = render(&f.app);
        assert_has(&buffer, "Arquivo do plano (opcional, Enter para usar");
        assert!(
            flat(&buffer).contains(".nick/docs/SC-42/plan.md):"),
            "{}",
            flat(&buffer)
        );
    }

    /// The preview of a prompt of `rows` lines, scrolled `scroll` down.
    fn preview(rows: usize, scroll: usize, jail: bool, warning: Option<&str>) -> Fixture {
        let mut f = fixture();
        let t = ticket(1, "NS-1", "Corrigir o login", false, false);
        f.app.dialog = Some(Dialog::ConfirmPrompt {
            project: PathBuf::from("/x/meu-proj"),
            holder: (&t).into(),
            precheck: precheck(jail),
            kind: StageType::PoDiscussion,
            rendered: Rendered {
                log: PathBuf::from("/repo/auto_scrum/logs/po_discussion_20261119_120000.md"),
                prompt: (1..=rows).map(|i| format!("linha {i}\n")).collect(),
                status: if jail {
                    "⚠️ Jail (ai-jail): ativa | modo de clipboard médio".into()
                } else {
                    "Jail (ai-jail): desligada — o agente roda direto no seu shell.".into()
                },
                warning: warning.map(str::to_string),
            },
            scroll,
        });
        f
    }

    #[test]
    fn the_preview_shows_the_status_the_prompt_and_the_question() {
        let f = preview(
            5,
            0,
            false,
            Some("Aviso: plano antigo em docs/plans/NS-1_x.md"),
        );
        let buffer = render(&f.app);
        assert_has(&buffer, " Prompt final ");
        let (row, x) = find(&buffer, "FORA DA JAIL: o agente roda direto no seu shell.").unwrap();
        let cell = &buffer[(x, row)];
        assert_eq!(cell.fg, Color::Red);
        assert!(cell.modifier.contains(Modifier::BOLD));
        assert_has(
            &buffer,
            "Jail (ai-jail): desligada — o agente roda direto no seu shell.",
        );
        assert_has(&buffer, "Aviso: plano antigo em docs/plans/NS-1_x.md");
        for i in 1..=5 {
            assert_has(&buffer, &format!("linha {i}"));
        }
        assert_has(&buffer, "linhas 1–5 de 5");
        assert_has(&buffer, "Confirma o envio para o claude? (s/N)");
        assert_has(
            &buffer,
            "Salvo em /repo/auto_scrum/logs/po_discussion_20261119_120000.md",
        );
        assert_has(&buffer, PROMPT_KEYS);
        // In the jail: no warning in red.
        let buffer = render(&preview(5, 0, true, None).app);
        assert!(find(&buffer, "FORA DA JAIL").is_none());
        assert_has(&buffer, "Jail (ai-jail): ativa | modo de clipboard médio");
    }

    #[test]
    fn the_preview_scrolls_and_says_where_it_is() {
        let area = Rect::new(0, 0, 100, 30);
        let (rows, max) = prompt_window(&preview(100, 0, true, None).app, area).unwrap();
        assert_eq!(max, 100 - rows);
        assert_eq!(
            prompt_window(&fixture().app, area),
            None,
            "no preview, no window"
        );
        let buffer = render(&preview(100, 10, true, None).app);
        assert_has(&buffer, &format!("linhas 11–{} de 100", 10 + rows));
        assert!(find(&buffer, "linha 10 ").is_none() && find(&buffer, "linha 10\n").is_none());
        assert_has(&buffer, "linha 11");
        assert!(find(&buffer, &format!("linha {}", 11 + rows)).is_none());
        // A scroll past the end is drawn at the end.
        let buffer = render(&preview(100, 500, true, None).app);
        assert_has(&buffer, "linha 100");
        assert_has(&buffer, &format!("linhas {}–100 de 100", 101 - rows));
    }

    #[test]
    fn a_long_line_of_the_prompt_wraps_keeping_its_indentation() {
        assert_eq!(
            wrap_keeping_spaces("  um dois tres", 8),
            ["  um ", "dois ", "tres"]
        );
        assert_eq!(wrap_keeping_spaces("", 8), [""]);
        assert_eq!(wrap_keeping_spaces("abcdefghij", 4), ["abcd", "efgh", "ij"]);
        // Wide characters count 2 columns; one wider than the row still goes, alone.
        assert_eq!(wrap_keeping_spaces("日本語", 4), ["日本", "語"]);
        assert_eq!(wrap_keeping_spaces("日x", 1), ["日", "x"]);
        // A long line (the limit of an argument is 128 KiB) wraps in linear time.
        let long = "palavra ".repeat(16 * 1024);
        let start = std::time::Instant::now();
        let rows = wrap_keeping_spaces(&long, 80);
        assert_eq!(rows.concat(), long);
        assert!(
            start.elapsed() < std::time::Duration::from_millis(500),
            "{:?}",
            start.elapsed()
        );
    }

    #[test]
    fn a_tab_of_the_prompt_shows_as_spaces_up_to_the_next_stop() {
        assert_eq!(expand_tabs("a\tb"), "a       b");
        assert_eq!(expand_tabs("\tx"), "        x");
        assert_eq!(expand_tabs("abcdefgh\ti"), "abcdefgh        i");
        assert_eq!(expand_tabs("sem tab"), "sem tab");
        let mut f = preview(1, 0, true, None);
        if let Some(Dialog::ConfirmPrompt { rendered, .. }) = &mut f.app.dialog {
            rendered.prompt = "fn main() {\n\tcodigo();\n}\n".into();
        }
        let buffer = render(&f.app);
        assert_has(&buffer, "        codigo();");
        assert!(find(&buffer, "?codigo").is_none(), "no ? for the tab");
    }

    #[test]
    fn the_preview_covers_the_screen_above_the_help_and_the_keys_show_once() {
        let f = preview(100, 0, true, None);
        let buffer = render(&f.app);
        let text = text(&buffer);
        assert_eq!(text.matches(PROMPT_KEYS).count(), 1, "{text}");
        assert_eq!(
            last_line(&buffer).trim(),
            PROMPT_KEYS,
            "the keys on the help line"
        );
        // Nothing of the list behind shows through: no corner of its frame beside the box.
        for (row, line) in text.lines().enumerate().take(29) {
            assert!(
                !line.starts_with('└') && !line.starts_with('┌'),
                "{row}: {line}"
            );
        }
    }

    #[test]
    fn the_preview_on_a_narrow_terminal_does_not_panic() {
        for (w, h) in [(20, 10), (8, 4), (3, 3), (1, 1)] {
            render_at(&preview(30, 3, false, Some("Aviso")).app, w, h, None);
        }
        render_at(
            &form(StageType::Development, &["SC-1", ""], 1).app,
            20,
            8,
            None,
        );
    }

    #[test]
    fn the_agent_alert_shows_each_line_of_the_message() {
        let mut f = fixture();
        f.app.dialog = Some(Dialog::AgentError {
            message: "Nada foi aberto.\n\nErro: a jail não funciona.\nDetalhe \x1b[2J".into(),
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Agente não aberto ");
        assert_has(&buffer, "Erro: a jail não funciona.");
        assert_has(&buffer, "Detalhe ?[2J");
        let (a, _) = find(&buffer, "Nada foi aberto.").unwrap();
        let (b, _) = find(&buffer, "Erro: a jail").unwrap();
        let (c, _) = find(&buffer, "Detalhe").unwrap();
        assert_eq!(
            (b, c),
            (a + 2, a + 3),
            "one row per line, the blank one included"
        );
        assert_eq!(last_line(&buffer).trim(), "Enter/Esc fechar");
    }

    // ---- several agents side by side (NS-34) ----

    use crate::keys::Shortcut;
    use crate::launch::{AgentLauncher, RenderRequest};
    use std::path::Path;

    /// Spawns nothing (the jail comes in the precheck given to `open_agent`).
    struct NoSpawn;

    impl AgentLauncher for NoSpawn {
        fn check(&mut self, _: &Path, _: &Path) -> Result<Precheck, String> {
            unreachable!("open_agent does not check again")
        }

        fn render(&mut self, _: &Path, _: &Path, _: &RenderRequest) -> Result<Rendered, String> {
            unreachable!("open_agent does not render")
        }

        fn has_conversation(
            &mut self,
            _: &Path,
            _: &Path,
            _: &str,
        ) -> Result<Option<String>, String> {
            unreachable!("open_agent does not check the conversation")
        }

        fn spawn(
            &mut self,
            _: &Path,
            _: &Path,
            _: i64,
            _: &str,
            _: &str,
            _: crate::launch::Launch,
        ) -> Result<(), String> {
            Ok(())
        }
    }

    /// The app with real agents (rows and locks) of the projects `names`, out of the jail when
    /// the name starts with `fora`, opened in that order; the last one is shown with the focus
    /// on its terminal.
    fn with_running(names: &[&str]) -> (Fixture, PathBuf) {
        let mut f = fixture();
        let root = std::fs::canonicalize(f._tmp.path()).unwrap();
        let launcher = root.join("nick-agent");
        std::fs::write(&launcher, "").unwrap();
        f.app = App::new(
            Db::open(&root.join("nick.db")).unwrap(),
            None,
            None,
            PathBuf::from("/repo/bin/nick-init"),
        )
        .with_agents(launcher, root.join("agents"));
        // A second connection to the same database, to store the projects and tickets.
        let db = Db::open(&root.join("nick.db")).unwrap();
        for name in names {
            let dir = root.join(name);
            std::fs::create_dir_all(dir.join(".nick")).unwrap();
            std::fs::write(dir.join(".nick/nick.conf"), "").unwrap();
            let p = db
                .insert_project("auto_scrum", name, dir.to_str().unwrap())
                .unwrap();
            let t = db.insert_ticket(p.id, "NS-1", false, "t", None).unwrap();
            let _ = p;
            f.app.refresh_tree();
            f.app.open_agent(
                &dir,
                &(&t).into(),
                StageType::Development,
                Path::new("/log.md"),
                precheck(!name.starts_with("fora")),
                &mut NoSpawn,
            );
        }
        (f, root)
    }

    fn render_at(app: &App, width: u16, height: u16, screen: Option<&vt100::Screen>) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| draw_with_terminal(frame, app, screen))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn with_an_agent_the_list_is_on_the_left_and_the_agent_on_the_right() {
        let (f, _) = with_running(&["proj"]);
        let area = Rect::new(0, 0, 100, 30);
        let areas = layout(&f.app, area);
        // The help keeps the rows the longest help needs (review NS-34, P1): three since the
        // NS-54 (the `c` of the epic and the `d` of the filters; two before).
        assert_eq!(areas.list, Rect::new(0, 0, 40, 26));
        // The panel keeps its border (NS-37): the header and the terminal are inside it.
        assert_eq!(areas.panel, Rect::new(40, 0, 60, 26));
        // NS-57, D11: the header of an agent in the jail has the rows of its version out of
        // it (two at 58 columns), the second one blank; one row each before.
        assert_eq!(areas.header, Rect::new(41, 1, 58, 2));
        assert_eq!(areas.terminal, Rect::new(41, 3, 58, 22));
        assert_eq!(areas.status, Rect::new(0, 26, 100, 1));
        assert_eq!(areas.help, Rect::new(0, 27, 100, 3));
        let agent = f.app.shown_agent().unwrap();
        assert_eq!(agent_area(&f.app, area, agent), areas.terminal);
        let mut parser = vt100::Parser::new(22, 58, 0);
        parser.process(b"primeira\x1b[22;1Hultima");
        let buffer = render_at(&f.app, 100, 30, Some(parser.screen()));
        assert_eq!(find(&buffer, "proj · Dev · NS-1 · na jail"), Some((1, 41)));
        assert_eq!(find(&buffer, "primeira"), Some((3, 41)));
        assert_eq!(
            find(&buffer, "ultima"),
            Some((24, 41)),
            "the last row of the program"
        );
        let (y, _) = find(&buffer, " Navegação ").unwrap();
        assert_eq!(y, 0);
        assert!(text(&buffer).lines().nth(25).unwrap().starts_with('└'));
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        assert_eq!(
            lines[27].trim(),
            "Ctrl+\\ voltar à lista · Alt+Enter nova linha",
            "in the first row"
        );
    }

    #[test]
    fn the_list_takes_at_most_half_of_a_narrow_window() {
        let (f, _) = with_running(&["proj"]);
        let areas = layout(&f.app, Rect::new(0, 0, 60, 30));
        assert_eq!(areas.list.width, 30);
        assert_eq!((areas.panel.x, areas.panel.width), (30, 30));
        for (width, height) in [(20, 10), (3, 3), (1, 1), (100, 4)] {
            let area = Rect::new(0, 0, width, height);
            let areas = layout(&f.app, area);
            let terminal = agent_area(&f.app, area, f.app.shown_agent().unwrap());
            assert_eq!(terminal, areas.terminal, "{width}x{height}");
            let (rows, cols) = crate::agent_session::screen_size(terminal.height, terminal.width);
            assert!(rows >= 2 && cols >= 2, "{width}x{height}");
            let parser = vt100::Parser::new(rows, cols, 0);
            render_at(&f.app, width, height, Some(parser.screen()));
        }
    }

    #[test]
    fn the_warning_out_of_the_jail_wraps_in_the_header_and_the_terminal_shrinks() {
        let (f, _) = with_running(&["proj", "fora"]);
        let area = Rect::new(0, 0, 100, 30);
        let areas = layout(&f.app, area);
        // Inside the border (NS-37) the header wraps at 58 columns.
        assert_eq!(
            areas.header,
            Rect::new(41, 1, 58, 2),
            "decision of the user"
        );
        // One row less since the NS-54 (the help keeps three rows at 100 columns).
        assert_eq!(areas.terminal, Rect::new(41, 3, 58, 22));
        // NS-57, D11: the one in the jail has the same size (its header keeps the rows of the
        // warning, blank); it had one row more before.
        let in_jail = &f.app.agents()[0];
        assert_eq!(agent_area(&f.app, area, in_jail), Rect::new(41, 3, 58, 22));
        let buffer = render_at(&f.app, 100, 30, None);
        let header: Vec<String> = text(&buffer)
            .lines()
            .skip(1)
            .take(2)
            .map(|l| {
                l.chars()
                    .skip(41)
                    .take(58)
                    .collect::<String>()
                    .trim()
                    .to_string()
            })
            .collect();
        assert_eq!(
            header.join(" "),
            "fora · Dev · NS-1 · FORA DA JAIL: o agente roda direto no seu shell."
        );
        for word in ["FORA DA JAIL", "shell"] {
            let (y, x) = find(&buffer, word).unwrap();
            assert_eq!(buffer[(x, y)].fg, Color::Red, "{word}");
            assert!(buffer[(x, y)].modifier.contains(Modifier::BOLD));
        }
        let (y, x) = find(&buffer, "fora · Dev").unwrap();
        assert_ne!(buffer[(x, y)].fg, Color::Red, "the name is not red");
        let end = find(&buffer, "shell.").unwrap();
        assert_ne!(
            buffer[(end.1 + 5, end.0)].fg,
            Color::Red,
            "nor the final dot"
        );
    }

    #[test]
    fn a_long_status_takes_rows_from_the_panel_too() {
        let (mut f, _) = with_running(&["proj"]);
        f.app.focus = Focus::List;
        f.app.status = Some("palavra ".repeat(30));
        let area = Rect::new(0, 0, 100, 30);
        let areas = layout(&f.app, area);
        assert_eq!(areas.status.height, 3);
        // 2 rows of the border (NS-37), 2 of the header (NS-57, D11: the rows of the warning out
        // of the jail, in the jail too; 1 before) and 3 of the status.
        assert_eq!(areas.terminal.height, 30 - 2 - 2 - 3 - areas.help.height);
        let agent = f.app.shown_agent().unwrap();
        assert_eq!(agent_area(&f.app, area, agent), areas.terminal);
        let buffer = render_at(&f.app, 100, 30, None);
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        let help_rows = usize::from(areas.help.height);
        assert!(
            lines[30 - help_rows - 1].starts_with("palavra"),
            "{lines:?}"
        );
    }

    /// NS-44: the message of the dropped Ctrl+Z fits the one row the status always takes, so
    /// it resizes no PTY (a second row would, NS-34).
    #[test]
    fn the_ctrl_z_message_fits_one_row_and_keeps_the_panel_size() {
        let (mut f, _) = with_running(&["proj"]);
        assert_eq!(f.app.focus, Focus::Terminal);
        let message = "O Ctrl+Z não tem efeito dentro do nick.";
        for width in [100, 60] {
            let area = Rect::new(0, 0, width, 30);
            f.app.status = None;
            let without = layout(&f.app, area);
            f.app.status = Some(message.into());
            let with = layout(&f.app, area);
            assert_eq!(without.status.height, 1, "{width}");
            assert_eq!(with.status.height, 1, "{width}");
            assert_eq!(with.terminal, without.terminal, "{width}");
            let agent = f.app.shown_agent().unwrap();
            assert_eq!(agent_area(&f.app, area, agent), with.terminal, "{width}");
            let buffer = render_at(&f.app, width, 30, None);
            let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
            assert_eq!(
                lines[usize::from(with.status.y)].trim(),
                message,
                "{width}: {lines:?}"
            );
        }
    }

    #[test]
    fn the_settings_show_the_shortcut_and_whether_it_is_the_default() {
        let mut f = fixture();
        f.app.screen = Screen::Settings;
        let buffer = render(&f.app);
        assert_has(&buffer, " Configurações ");
        // In the column on the left (40 columns), word wrapped (NS-38).
        let column = |buffer: &Buffer| {
            (1..6)
                .map(|y| list_row(buffer, y))
                .collect::<Vec<_>>()
                .join(" ")
        };
        assert!(
            column(&buffer)
                .contains("Atalho para alternar entre a lista e o terminal: Ctrl+\\ (padrão)"),
            "{}",
            text(&buffer)
        );
        assert_eq!(
            last_line(&buffer).trim(),
            "t trocar · p voltar ao padrão · Esc voltar · q sair"
        );
        f.app.shortcut = Shortcut::from_db("alt+x").unwrap();
        f.app.shortcut_stored = true;
        let buffer = render(&f.app);
        assert!(column(&buffer).contains("Atalho para alternar entre a lista e o terminal: Alt+X"));
        assert!(find(&buffer, "(padrão)").is_none());
    }

    #[test]
    fn the_settings_show_the_version_under_the_shortcut() {
        let mut f = fixture();
        f.app.screen = Screen::Settings;
        let buffer = render(&f.app);
        let rows: Vec<String> = (1..8).map(|y| list_row(&buffer, y)).collect();
        let shortcut = rows
            .iter()
            .position(|row| row.contains("Atalho para alternar"))
            .unwrap_or_else(|| panic!("{rows:?}"));
        let version = rows
            .iter()
            .position(|row| row.trim() == format!("Versão: {}", crate::version::long()))
            .unwrap_or_else(|| panic!("{rows:?}"));
        assert!(version > shortcut, "{rows:?}");
    }

    #[test]
    fn the_capture_asks_for_the_key_then_for_it_again() {
        let mut f = fixture();
        f.app.screen = Screen::Settings;
        f.app.dialog = Some(Dialog::CaptureShortcut { first: None });
        let buffer = render(&f.app);
        assert_has(&buffer, " Trocar atalho ");
        assert_has(&buffer, "Aperte a tecla do novo atalho. Esc cancela.");
        assert_eq!(last_line(&buffer).trim(), "Esc cancelar");
        f.app.dialog = Some(Dialog::CaptureShortcut {
            first: Shortcut::from_db("ctrl+]"),
        });
        assert_has(&render(&f.app), "Aperte Ctrl+] de novo para confirmar.");
    }

    /// NS-57, D4/D5: releasing and jailing again ask `(s/N)` in a red thick border, with no cyan
    /// cell; the text says the action, that the agent restarts and, to release, that it goes
    /// without the protection of the jail.
    #[test]
    fn the_jail_toggle_dialogs_are_red_and_say_what_happens() {
        let (mut f, _) = with_running(&["alfa"]);
        f.app.focus = Focus::List;
        for (release, text, keys) in [
            (
                true,
                "Liberar o agente alfa · Dev · NS-1 da jail? Ele reinicia e fica SEM a proteção \
                 da jail até fechar. (s/N)",
                "s liberar · qualquer outra tecla cancela",
            ),
            (
                false,
                "Prender o agente alfa · Dev · NS-1 na jail de novo? Ele reinicia e volta para a \
                 jail do projeto. (s/N)",
                "s prender · qualquer outra tecla cancela",
            ),
        ] {
            f.app.dialog = Some(Dialog::ConfirmJailToggle {
                id: 1,
                name: "alfa · Dev · NS-1".into(),
                release,
            });
            let buffer = render(&f.app);
            assert!(flat(&buffer).contains(text), "{release}: {}", flat(&buffer));
            assert_eq!(last_line(&buffer).trim(), keys, "{release}");
            let corners = all_of(&buffer, "┏");
            assert_eq!(corners.len(), 1, "{release}");
            let (x, y) = corners[0];
            assert_border_color(&buffer, (x, y), "┏", Some(IRREVERSIBLE_COLOR));
            assert_border_color(&buffer, (x, y + 1), "┃", Some(IRREVERSIBLE_COLOR));
            assert!(
                buffer.content.iter().all(|cell| cell.fg != FOCUS_COLOR),
                "{release}: no cyan cell"
            );
        }
        let mut title = |release| {
            f.app.dialog = Some(Dialog::ConfirmJailToggle {
                id: 1,
                name: "a".into(),
                release,
            });
            flat(&render(&f.app))
        };
        assert!(title(true).contains(" Liberar da jail "));
        assert!(title(false).contains(" Prender na jail "));
    }

    #[test]
    fn the_confirmations_of_ending_one_and_quitting() {
        let (mut f, _) = with_running(&["alfa", "beta", "alfa2"]);
        f.app.focus = Focus::List;
        f.app.agents[2].project = "alfa".into();
        f.app.dialog = Some(Dialog::ConfirmEndAgent {
            id: 1,
            name: "alfa · Dev · NS-1".into(),
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Encerrar agente ");
        assert!(flat(&buffer).contains("Encerrar o agente alfa · Dev · NS-1? (s/N)"));
        assert_eq!(
            last_line(&buffer).trim(),
            "s encerrar · qualquer outra tecla cancela"
        );
        f.app.dialog = Some(Dialog::ConfirmQuit);
        let buffer = render(&f.app);
        assert_has(&buffer, " Sair do nick ");
        assert!(
            flat(&buffer)
                .contains("Há 3 agentes ativos (alfa, beta). Encerrar todos e sair? (s/N)"),
            "the projects without repeats, in order: {}",
            flat(&buffer)
        );
        assert_eq!(
            last_line(&buffer).trim(),
            "s encerrar e sair · qualquer outra tecla cancela"
        );
        let (mut f, _) = with_running(&["solo"]);
        f.app.focus = Focus::List;
        f.app.dialog = Some(Dialog::ConfirmQuit);
        assert!(flat(&render(&f.app)).contains("Há 1 agente ativo (solo). Encerrar e sair? (s/N)"));
    }

    #[test]
    fn a_dialog_is_centered_over_the_whole_body() {
        let (mut f, _) = with_running(&["proj"]);
        f.app.focus = Focus::List;
        f.app.dialog = Some(Dialog::ConfirmQuit);
        let buffer = render(&f.app);
        let (_, x) = find(&buffer, "Sair do nick").unwrap();
        assert!(x < 40, "the box starts over the list: {x}");
        let (y, _) = find(&buffer, "Há 1 agente").unwrap();
        let row = text(&buffer)
            .lines()
            .nth(usize::from(y))
            .unwrap()
            .to_string();
        assert!(row.chars().count() > 70, "and spans the panel: {row:?}");
    }

    /// Review NS-34, P1: the terminal keeps its size when the focus toggles, the list changes,
    /// a mark comes or a dialog opens (each change of size redraws every claude and marks the
    /// hidden ones).
    #[test]
    fn the_terminal_keeps_its_size_whatever_the_focus_the_marks_and_the_dialog() {
        let (mut f, _) = with_running(&["proj", "fora"]);
        for (width, height) in [(100, 30), (50, 30), (140, 40)] {
            let area = Rect::new(0, 0, width, height);
            let mut sizes = Vec::new();
            // The tree with both filters and the settings (NS-38; before, LIST_SCREENS).
            for (screen, show_hidden) in [
                (Screen::Tree, false),
                (Screen::Tree, true),
                (Screen::Settings, false),
            ] {
                for focus in [Focus::Terminal, Focus::List] {
                    for unseen in [false, true] {
                        for dialog in [None, Some(Dialog::ConfirmQuit)] {
                            f.app.screen = screen;
                            f.app.tree.show_hidden = show_hidden;
                            f.app.focus = focus;
                            f.app.agents[0].unseen = unseen;
                            f.app.dialog = dialog.filter(|_| focus == Focus::List);
                            sizes.push(layout(&f.app, area).terminal);
                        }
                    }
                }
            }
            assert!(
                sizes.iter().all(|s| *s == sizes[0]),
                "{width}x{height}: {sizes:?}"
            );
        }
        // Without agents, the help takes only the rows it needs, as before the NS-34 (the help
        // of the module takes two at 100 columns, and two at 140 since the `d` of the NS-54; one
        // at 160).
        let f = fixture();
        assert_eq!(layout(&f.app, Rect::new(0, 0, 100, 30)).help.height, 2);
        assert_eq!(layout(&f.app, Rect::new(0, 0, 140, 30)).help.height, 2);
        assert_eq!(layout(&f.app, Rect::new(0, 0, 160, 30)).help.height, 1);
    }

    /// Review NS-34, P2: the PTY of a new agent is born with the size `layout` gives it once
    /// it is open.
    #[test]
    fn a_new_agent_is_born_with_the_size_of_its_panel() {
        for names in [&["proj"][..], &["proj", "fora"][..], &["fora", "proj"][..]] {
            let (mut f, root) = with_running(&names[..names.len() - 1]);
            let area = Rect::new(0, 0, 100, 30);
            f.app.focus = Focus::List;
            f.app.status = Some("palavra ".repeat(30));
            let panel = new_agent_panel(&f.app, area);
            let last = names[names.len() - 1];
            let jail = !last.starts_with("fora");
            let name = format!("{last} · Dev · NS-1");
            let expected = terminal_in_panel(panel, &name);
            // Open it as with_running does.
            let db = Db::open(&root.join("nick.db")).unwrap();
            let dir = root.join(last);
            std::fs::create_dir_all(&dir).unwrap();
            let p = db
                .insert_project("auto_scrum", last, dir.to_str().unwrap())
                .unwrap();
            let t = db.insert_ticket(p.id, "NS-1", false, "t", None).unwrap();
            let _ = p;
            f.app.refresh_tree();
            f.app.open_agent(
                &dir,
                &(&t).into(),
                StageType::Development,
                Path::new("/log.md"),
                precheck(jail),
                &mut NoSpawn,
            );
            let agent = f.app.shown_agent().unwrap();
            assert_eq!(agent.name, name);
            assert_eq!(agent_area(&f.app, area, agent), expected, "{names:?}");
        }
    }

    // ---- the stage on the list, the ticket screen and its dialogs (NS-36) ----

    use crate::db::StageChangeRow;
    use crate::tickets::TicketStage;

    fn staged(id: i64, key: &str, title: &str, stage: &str, hidden: bool) -> Ticket {
        Ticket {
            stage: stage.into(),
            ..ticket(id, key, title, false, hidden)
        }
    }

    fn change(from: &str, to: &str, at: &str) -> StageChangeRow {
        sourced(from, to, "operator", at)
    }

    fn sourced(from: &str, to: &str, source: &str, at: &str) -> StageChangeRow {
        StageChangeRow {
            from_stage: from.into(),
            to_stage: to.into(),
            source: source.into(),
            reason: None,
            changed_at: at.into(),
        }
    }

    #[test]
    fn the_dialogs_of_the_stage_and_of_the_id() {
        let mut f = fixture();
        let t = staged(1, "NS-12", "Título do ticket", "development", false);
        f.app.dialog = Some(Dialog::ChooseTicketStage {
            ticket: t.clone(),
            index: 4,
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Mudar etapa ");
        assert_has(&buffer, "Ticket NS-12 — Título do ticket");
        // Aberto first (NS-45), the six in the order of the flow.
        let (open, _) = find(&buffer, "  Aberto").unwrap();
        let (po, _) = find(&buffer, "  PO").unwrap();
        assert_eq!(po, open + 1);
        assert_has(&buffer, "  Dev (atual)");
        assert_has(&buffer, "> Review");
        assert_has(&buffer, "  Concluído");
        assert_eq!(
            last_line(&buffer).trim(),
            "↑/↓ escolher · Enter escolher · Esc cancelar"
        );
        f.app.dialog = Some(Dialog::ConfirmStageChange {
            ticket: t.clone(),
            to: TicketStage::Done,
            reason: None,
            live: Vec::new(),
        });
        let buffer = render(&f.app);
        assert!(
            flat(&buffer).contains("Mudar a etapa do ticket NS-12 de Dev para Concluído? (s/N)"),
            "{}",
            flat(&buffer)
        );
        assert_eq!(
            last_line(&buffer).trim(),
            "s mudar · qualquer outra tecla cancela"
        );
        f.app.dialog = Some(Dialog::EditId {
            ticket: t.clone(),
            input: "NS-1".into(),
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Editar ID ");
        assert_has(&buffer, "Novo ID do ticket (ex: SC-123):");
        assert_eq!(last_line(&buffer).trim(), "Enter confirmar · Esc cancelar");
        for (moves, expected) in [
            (
                false,
                "Trocar o ID NS-12 por NS-99? A branch do git, o ticket no Jira e os agentes \
                 já registrados não mudam. (s/N)",
            ),
            (
                true,
                "Trocar o ID NS-12 por NS-99? A pasta .nick/docs/NS-12/ será renomeada para \
                 .nick/docs/NS-99/. A branch do git, o ticket no Jira e os agentes já \
                 registrados não mudam. (s/N)",
            ),
        ] {
            f.app.dialog = Some(Dialog::ConfirmEditId {
                ticket: t.clone(),
                new: "NS-99".into(),
                moves,
            });
            let buffer = render(&f.app);
            assert!(flat(&buffer).contains(expected), "{}", flat(&buffer));
            assert_eq!(
                last_line(&buffer).trim(),
                "s trocar · qualquer outra tecla cancela"
            );
        }
    }

    #[test]
    fn the_form_shows_the_warning_of_the_missing_plan_in_yellow() {
        let mut f = form(StageType::Development, &[" NS-1 ", ""], 1);
        let buffer = render(&f.app);
        assert!(
            find(&buffer, "Aviso:").is_none(),
            "no warning when not missing"
        );
        if let Some(Dialog::StageForm { plan_missing, .. }) = f.app.dialog.as_mut() {
            *plan_missing = true;
        }
        let buffer = render(&f.app);
        let warning = "Aviso: não existe .nick/docs/NS-1/plan.md. Informe outro caminho no \
                       campo do plano, ou siga sem ele.";
        assert!(flat(&buffer).contains(warning), "{}", flat(&buffer));
        let (y, x) = find(&buffer, "Aviso:").unwrap();
        assert_eq!(buffer[(x, y)].fg, Color::Yellow);
        let (fields, _) = find(&buffer, "ID do Jira").unwrap();
        assert!(y < fields, "above the fields");
    }

    // ---- the focused panel (NS-37) ----

    /// `block` drawn alone over `width`x`height`.
    fn render_block(block: Block<'static>, width: u16, height: u16) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| frame.render_widget(block, frame.area()))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    /// The border at `pos` is `symbol`, thick and in `FOCUS_COLOR` (`thick`), or plain, dim and
    /// without a color: `assert_border_color` with `FOCUS_COLOR` when `thick` (NS-46).
    fn assert_border(buffer: &Buffer, pos: (u16, u16), thick: bool, symbol: &str) {
        assert_border_color(buffer, pos, symbol, thick.then_some(FOCUS_COLOR));
    }

    /// The border at (`x`, `y`) is `symbol`, thick and in `color` without DIM (`Some`), or plain,
    /// dim and without a color (`None`; NS-46).
    fn assert_border_color(
        buffer: &Buffer,
        (x, y): (u16, u16),
        symbol: &str,
        color: Option<Color>,
    ) {
        let cell = &buffer[(x, y)];
        assert_eq!(cell.symbol(), symbol, "({x}, {y})\n{}", text(buffer));
        if let Some(color) = color {
            assert_eq!(cell.fg, color, "({x}, {y})");
            assert!(!cell.modifier.contains(Modifier::DIM), "({x}, {y})");
        } else {
            assert_eq!(cell.fg, Color::Reset, "({x}, {y}) without a color");
            assert!(cell.modifier.contains(Modifier::DIM), "({x}, {y})");
        }
    }

    #[test]
    fn the_focused_block_is_thick_and_cyan_the_other_plain_and_dim() {
        assert_eq!(FOCUS_COLOR, Color::Cyan);
        let buffer = render_block(pane_block(true), 6, 4);
        assert_border(&buffer, (0, 0), true, "┏");
        assert_border(&buffer, (5, 0), true, "┓");
        assert_border(&buffer, (0, 3), true, "┗");
        assert_border(&buffer, (5, 3), true, "┛");
        assert_border(&buffer, (2, 0), true, "━");
        assert_border(&buffer, (0, 1), true, "┃");
        assert_border(&buffer, (5, 2), true, "┃");
        let buffer = render_block(pane_block(false), 6, 4);
        assert_border(&buffer, (0, 0), false, "┌");
        assert_border(&buffer, (5, 0), false, "┐");
        assert_border(&buffer, (0, 3), false, "└");
        assert_border(&buffer, (5, 3), false, "┘");
        assert_border(&buffer, (2, 3), false, "─");
        assert_border(&buffer, (0, 2), false, "│");
        assert_border(&buffer, (5, 1), false, "│");
        // Both take one cell: the inside is the same.
        let area = Rect::new(3, 2, 10, 8);
        assert_eq!(pane_block(true).inner(area), pane_block(false).inner(area));
        assert_eq!(pane_block(true).inner(area), Block::bordered().inner(area));
    }

    #[test]
    fn the_focused_panel_has_the_thick_cyan_border() {
        let (mut f, _) = with_running(&["proj"]);
        // The focus on the terminal: the panel of the agent thick and cyan, the list plain.
        let buffer = render_at(&f.app, 100, 30, None);
        assert_border(&buffer, (40, 0), true, "┏");
        assert_border(&buffer, (99, 0), true, "┓");
        assert_border(&buffer, (99, 10), true, "┃");
        // The bottom one row up since the NS-54 (the help keeps three rows).
        assert_border(&buffer, (40, 25), true, "┗");
        assert_border(&buffer, (0, 0), false, "┌");
        assert_border(&buffer, (39, 10), false, "│");
        // The focus on the list: the other way around.
        f.app.focus = Focus::List;
        let buffer = render_at(&f.app, 100, 30, None);
        assert_border(&buffer, (0, 0), true, "┏");
        assert_border(&buffer, (39, 10), true, "┃");
        assert_border(&buffer, (0, 25), true, "┗");
        assert_border(&buffer, (40, 0), false, "┌");
        assert_border(&buffer, (99, 10), false, "│");
        // The title of the tree stays on the border.
        assert_has(&buffer, " Navegação ");
    }

    #[test]
    fn every_list_screen_shows_where_the_focus_is() {
        let (mut f, _) = with_running(&["proj"]);
        // The tree with both filters and the settings (NS-38; before, LIST_SCREENS).
        for (screen, show_hidden) in [
            (Screen::Tree, false),
            (Screen::Tree, true),
            (Screen::Settings, false),
        ] {
            for focus in [Focus::List, Focus::Terminal] {
                f.app.screen = screen;
                f.app.tree.show_hidden = show_hidden;
                f.app.focus = focus;
                let on_list = focus == Focus::List;
                let buffer = render_at(&f.app, 100, 30, None);
                let corner = if on_list { "┏" } else { "┌" };
                let side = if on_list { "┃" } else { "│" };
                let panel = if on_list { "┌" } else { "┏" };
                assert_border(&buffer, (0, 0), on_list, corner);
                assert_border(&buffer, (39, 5), on_list, side);
                assert_border(&buffer, (40, 0), !on_list, panel);
            }
        }
    }

    /// The border takes a cell on each side of the panel (NS-37): the terminal (the size of the
    /// PTY) is inside it, whatever the size, without an underflow on a tiny panel.
    #[test]
    fn the_terminal_is_inside_the_border_of_the_panel() {
        let name = "proj · Dev · NS-1";
        let panel = Rect::new(40, 0, 60, 27);
        // NS-57, D11: below the two rows of the header (the warning out of the jail wraps at 58
        // columns), whatever the jail; (41, 2, 58, 24) in the jail before.
        assert_eq!(terminal_in_panel(panel, name), Rect::new(41, 3, 58, 23));
        for panel in [
            Rect::new(40, 0, 60, 27),
            Rect::new(5, 3, 20, 10),
            Rect::new(0, 0, 3, 3),
            Rect::new(0, 0, 2, 2),
            Rect::new(0, 0, 1, 1),
            Rect::new(0, 0, 0, 0),
            Rect::new(0, 0, 50, 1),
        ] {
            for jail in [true, false] {
                let inner = Block::bordered().inner(panel);
                let terminal = terminal_in_panel(panel, name);
                assert_eq!(terminal.union(inner), inner, "{panel:?} {jail}");
                assert_eq!((terminal.x, terminal.width), (inner.x, inner.width));
                assert_eq!(terminal.bottom(), inner.bottom(), "{panel:?} {jail}");
            }
        }
        // agent_area, the size of each PTY, comes from the same count.
        let (f, _) = with_running(&["proj", "fora"]);
        let area = Rect::new(0, 0, 100, 30);
        let panel = layout(&f.app, area).panel;
        for agent in f.app.agents() {
            assert_eq!(
                agent_area(&f.app, area, agent),
                terminal_in_panel(panel, &agent.name)
            );
        }
    }

    /// NS-57, D11: the header has the height of its version out of the jail, in the jail too
    /// (capped as before: a third of the panel, at least 1), so the terminal of an agent has the
    /// same size in the jail and out of it, for the same name and panel. At 40 columns inside the
    /// border, the warning wraps in 2 rows; at 20, in 4 (capped at 3 in a panel of 9 rows).
    #[test]
    fn the_size_of_the_terminal_does_not_depend_on_the_jail() {
        let name = "proj · Dev · NS-1";
        let mut wrapped = Vec::new();
        for width in [12, 22, 32, 42, 60, 80, 100, 140] {
            for height in [1, 3, 9, 27, 40] {
                let panel = Rect::new(40, 0, width, height);
                let terminal = terminal_in_panel(panel, name);
                let inner = Block::bordered().inner(panel);
                let off = header_lines(name, false, inner.width).len();
                let on = header_lines(name, true, inner.width).len();
                assert!(on <= off, "the warning is the longer: {panel:?}");
                let cap = usize::from((inner.height / 3).max(1).min(inner.height));
                assert_eq!(
                    usize::from(terminal.y - inner.y),
                    off.max(on).min(cap),
                    "the height out of the jail, capped, in the jail too: {panel:?}"
                );
                wrapped.push(off);
            }
        }
        for rows in [2, 3, 4] {
            assert!(wrapped.contains(&rows), "control: {rows} rows: {wrapped:?}");
        }
    }

    /// NS-57, D11: in the jail, the rows the header has to spare are blank, below the first one,
    /// with `na jail`; the terminal starts below them, where it starts out of the jail.
    #[test]
    fn the_header_in_the_jail_leaves_its_extra_rows_blank() {
        let name = "proj · Dev · NS-1";
        let draw = |jail: bool| {
            let mut terminal = Terminal::new(TestBackend::new(42, 30)).unwrap();
            let parser = vt100::Parser::new(30, 40, 0);
            terminal
                .draw(|frame| {
                    draw_agent(frame, frame.area(), name, jail, true, Some(parser.screen()))
                })
                .unwrap();
            terminal.backend().buffer().clone()
        };
        let (jailed, off) = (draw(true), draw(false));
        assert_eq!(inside_row(&jailed, 1), "proj · Dev · NS-1 · na jail");
        assert_eq!(inside_row(&jailed, 2), "", "a blank row in the jail");
        assert!(
            inside_row(&off, 2).contains("direto no seu shell."),
            "control"
        );
        assert_eq!(terminal_in_panel(Rect::new(0, 0, 42, 30), name).y, 3);
        // The cursor of the program, at (0, 0) of the terminal, on the same row in both.
        assert_eq!(jailed[(1, 3)].symbol(), "█");
        assert_eq!(off[(1, 3)].symbol(), "█");
    }

    /// The program's cursor at (0, 0) is drawn on the first cell of the terminal, inside the
    /// border (NS-37).
    #[test]
    fn the_cursor_of_the_program_is_inside_the_border() {
        let (f, _) = with_running(&["proj"]);
        let areas = layout(&f.app, Rect::new(0, 0, 100, 30));
        let parser = vt100::Parser::new(areas.terminal.height, areas.terminal.width, 0);
        let buffer = render_at(&f.app, 100, 30, Some(parser.screen()));
        assert_eq!(buffer[(areas.terminal.x, areas.terminal.y)].symbol(), "█");
        // NS-57, D11: below the two rows of the header (one in the jail before).
        assert_eq!((areas.terminal.x, areas.terminal.y), (41, 3));
        assert_eq!(buffer[(40, 2)].symbol(), "┃", "the border stays");
    }

    /// The mouse goes to the program with the coordinates of the terminal; over the border it
    /// is outside the terminal, so it is dropped (NS-37).
    #[test]
    fn the_mouse_over_the_border_goes_nowhere() {
        use crate::app::{Action, AgentTerm};
        use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let (mut f, _) = with_running(&["proj"]);
        let terminal = layout(&f.app, Rect::new(0, 0, 100, 30)).terminal;
        let event = |kind, column, row| MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        };
        let click = |column, row| event(MouseEventKind::Down(MouseButton::Left), column, row);
        let wheel = |column, row| event(MouseEventKind::ScrollUp, column, row);
        // Without the mouse asked by the program, the wheel scrolls only over the terminal (NS-57,
        // D11: from the row 3, below the two rows of the header; the row 2 before).
        assert_ne!(f.app.handle_mouse(&wheel(41, 3), terminal), Action::None);
        for (column, row) in [(40, 5), (99, 5), (60, 0), (60, 26)] {
            assert_eq!(
                f.app.handle_mouse(&wheel(column, row), terminal),
                Action::None
            );
        }
        f.app.agent_term = AgentTerm {
            mouse_mode: vt100::MouseProtocolMode::PressRelease,
            mouse_encoding: vt100::MouseProtocolEncoding::Sgr,
            ..AgentTerm::default()
        };
        assert_eq!(
            f.app.handle_mouse(&click(41, 3), terminal),
            Action::Send(b"\x1b[<0;1;1M".to_vec()),
            "the first cell below the header is the program's (1, 1)"
        );
        for (column, row) in [(40, 5), (99, 5), (60, 0), (60, 1), (60, 2), (60, 26)] {
            assert_eq!(
                f.app.handle_mouse(&click(column, row), terminal),
                Action::None,
                "({column}, {row})"
            );
        }
    }

    // ---- the focus border on the dialogs (NS-46) ----

    use crate::app::tests::{one_of_each_dialog, variant};

    /// The symbols of a thick border.
    const THICK: [&str; 6] = ["┏", "┓", "┗", "┛", "━", "┃"];

    /// Where `symbol` is on the screen, every time.
    fn all_of(buffer: &Buffer, symbol: &str) -> Vec<(u16, u16)> {
        let area = buffer.area;
        (0..area.height)
            .flat_map(|y| (0..area.width).map(move |x| (x, y)))
            .filter(|&pos| buffer[pos].symbol() == symbol)
            .collect()
    }

    /// The color of the border of `dialog` (NS-57: red also for the one that touches the jail).
    fn color_of(dialog: &Dialog) -> Color {
        if dialog.irreversible() || dialog.touches_jail() {
            IRREVERSIBLE_COLOR
        } else {
            FOCUS_COLOR
        }
    }

    #[test]
    fn the_dialog_block_is_the_focused_pane_block_cyan_or_red() {
        assert_eq!(IRREVERSIBLE_COLOR, Color::Red);
        let dialogs = one_of_each_dialog();
        let common = dialogs
            .iter()
            .find(|d| variant(d) == "ConfirmQuit")
            .unwrap();
        let removal = dialogs
            .iter()
            .find(|d| variant(d) == "ConfirmRemove")
            .unwrap();
        // A common dialog: the very block of the focused panel, not a copied style.
        assert_eq!(dialog_block(common), pane_block(true));
        for (dialog, color) in [(common, FOCUS_COLOR), (removal, IRREVERSIBLE_COLOR)] {
            let buffer = render_block(dialog_block(dialog), 6, 4);
            for (pos, symbol) in [
                ((0, 0), "┏"),
                ((5, 0), "┓"),
                ((0, 3), "┗"),
                ((5, 3), "┛"),
                ((2, 0), "━"),
                ((3, 3), "━"),
                ((0, 1), "┃"),
                ((5, 2), "┃"),
            ] {
                assert_border_color(&buffer, pos, symbol, Some(color));
            }
            // The red replaces the cyan: no cell of the border is left cyan under it.
            if color == IRREVERSIBLE_COLOR {
                assert!(buffer.content.iter().all(|cell| cell.fg != FOCUS_COLOR));
            }
            // The same size as any border.
            let area = Rect::new(3, 2, 10, 8);
            assert_eq!(
                dialog_block(dialog).inner(area),
                Block::bordered().inner(area)
            );
        }
    }

    #[test]
    fn every_dialog_has_the_focus_border_and_only_it() {
        let (mut running, _) = with_running(&["proj"]);
        running.app.focus = Focus::List;
        let (mut alone, _) = with_tree();
        for (f, agent) in [(&mut running, true), (&mut alone, false)] {
            for dialog in one_of_each_dialog() {
                let name = variant(&dialog);
                let color = color_of(&dialog);
                let prompt = matches!(dialog, Dialog::ConfirmPrompt { .. });
                f.app.dialog = Some(dialog);
                let buffer = render_at(&f.app, 100, 30, None);
                // One focus border on the screen: the dialog's.
                let corners = all_of(&buffer, "┏");
                assert_eq!(corners.len(), 1, "{name} {agent}\n{}", text(&buffer));
                let corner = corners[0];
                assert_border_color(&buffer, corner, "┏", Some(color));
                if prompt {
                    // It covers the screen: the box from (1, 1), and nothing thick outside it.
                    assert_eq!(corner, (1, 1), "{name}");
                    let end = all_of(&buffer, "┛");
                    assert_eq!(end.len(), 1, "{name}");
                    let (right, bottom) = end[0];
                    for symbol in THICK {
                        for (x, y) in all_of(&buffer, symbol) {
                            assert!(
                                (1..=right).contains(&x) && (1..=bottom).contains(&y),
                                "{name}: {symbol} at ({x}, {y})\n{}",
                                text(&buffer)
                            );
                        }
                    }
                    let (x, y) = corner;
                    assert_border_color(&buffer, (x, y + 1), "┃", Some(color));
                } else {
                    // The list and the panel on the right, behind it, without the focus.
                    assert_border(&buffer, (0, 0), false, "┌");
                    assert_border(&buffer, (40, 0), false, "┌");
                }
            }
        }
    }

    #[test]
    fn the_focus_border_goes_back_when_the_dialog_closes() {
        let (mut f, _) = with_running(&["proj"]);
        // The focus on the list.
        f.app.focus = Focus::List;
        f.app.dialog = Some(Dialog::ConfirmQuit);
        let buffer = render_at(&f.app, 100, 30, None);
        assert_border(&buffer, (0, 0), false, "┌");
        assert_border(&buffer, (40, 0), false, "┌");
        f.app.dialog = None;
        let buffer = render_at(&f.app, 100, 30, None);
        assert_border(&buffer, (0, 0), true, "┏");
        assert_border(&buffer, (40, 0), false, "┌");
        // The focus on the terminal (forced: the App opens no dialog there today): the dialog
        // comes first anyway, and the panel gets the border back when it closes.
        f.app.focus = Focus::Terminal;
        f.app.dialog = Some(Dialog::ConfirmQuit);
        let buffer = render_at(&f.app, 100, 30, None);
        assert_border(&buffer, (0, 0), false, "┌");
        assert_border(&buffer, (40, 0), false, "┌");
        assert_eq!(all_of(&buffer, "┏").len(), 1, "{}", text(&buffer));
        f.app.dialog = None;
        let buffer = render_at(&f.app, 100, 30, None);
        assert_border(&buffer, (0, 0), false, "┌");
        assert_border(&buffer, (40, 0), true, "┏");
    }

    #[test]
    fn the_removal_dialog_is_red_and_the_others_cyan() {
        let (mut f, _) = with_running(&["proj"]);
        f.app.focus = Focus::List;
        for dialog in one_of_each_dialog() {
            let name = variant(&dialog);
            let color = match name.as_str() {
                "ConfirmRemove" => Color::Red,
                "ConfirmHide" => Color::Cyan,
                _ => continue,
            };
            f.app.dialog = Some(dialog);
            let buffer = render_at(&f.app, 100, 30, None);
            let (y, x) = find(&buffer, "┏").expect(&name);
            assert_border_color(&buffer, (x, y), "┏", Some(color));
            assert_border_color(&buffer, (x, y + 1), "┃", Some(color));
        }
    }

    // ---- the tree (NS-38) ----

    use crate::modules::ModuleKind;
    use crate::tree::NodeId;

    const MODULE: NodeId = NodeId::Module(ModuleKind::AutoScrum);

    /// A database with the projects `alfa` (available) and `beta` (its folder missing). `alfa`
    /// has NS-1 "Um" (Tech Leader) with an ended Dev agent and a PO agent active in another
    /// nick, NS-2 "Dois" (done), NS-3 "Três" (hidden) and abcd1234 "Quatro" (provisional).
    /// The module, `alfa` and NS-1 open, the module selected.
    fn with_tree() -> (Fixture, PathBuf) {
        let mut f = fixture();
        let root = std::fs::canonicalize(f._tmp.path()).unwrap();
        let alfa = root.join("alfa");
        std::fs::create_dir_all(alfa.join(".nick")).unwrap();
        std::fs::write(alfa.join(".nick/nick.conf"), "").unwrap();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let a = db
            .insert_project("auto_scrum", "alfa", alfa.to_str().unwrap())
            .unwrap();
        db.insert_project("auto_scrum", "beta", root.join("beta").to_str().unwrap())
            .unwrap();
        let sql = |query: &str, id: i64| {
            db.connection().execute(query, [id]).unwrap();
        };
        let t1 = db.insert_ticket(a.id, "NS-1", false, "Um", None).unwrap();
        sql(
            "UPDATE tickets SET stage = 'tech_leader' WHERE id = ?1",
            t1.id,
        );
        let t2 = db.insert_ticket(a.id, "NS-2", false, "Dois", None).unwrap();
        sql("UPDATE tickets SET stage = 'done' WHERE id = ?1", t2.id);
        let t3 = db.insert_ticket(a.id, "NS-3", false, "Três", None).unwrap();
        sql(
            "UPDATE tickets SET hidden_at = '2026-11-20T00:00:00Z' WHERE id = ?1",
            t3.id,
        );
        db.insert_ticket(a.id, "abcd1234", true, "Quatro", None)
            .unwrap();
        let ended = db
            .insert_agent(
                Owner::Ticket(t1.id),
                "development",
                "s-1",
                "alfa · Dev · NS-1",
                "NS-1",
            )
            .unwrap();
        db.stop_agent(ended.id).unwrap();
        sql(
            "UPDATE agents SET opened_at = '2026-11-19T21:46:05Z' WHERE id = ?1",
            ended.id,
        );
        let other = db
            .insert_agent(
                Owner::Ticket(t1.id),
                "po",
                "s-2",
                "alfa · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        sql(
            "UPDATE agents SET opened_at = '2026-11-20T00:00:00Z' WHERE id = ?1",
            other.id,
        );
        f.app.refresh_tree();
        f.app.tree.open(MODULE);
        f.app.tree.open(NodeId::Project(a.id));
        f.app.tree.open(NodeId::Ticket(t1.id));
        f.app.refresh_tree();
        (f, root)
    }

    /// The node of the ticket `key`.
    fn tnode(app: &App, key: &str) -> NodeId {
        NodeId::Ticket(
            app.tickets
                .values()
                .flatten()
                .find(|t| t.key == key)
                .unwrap()
                .id,
        )
    }

    /// The cells of the list (the 40 columns on the left) of the row `y`, as text.
    fn list_row(buffer: &Buffer, y: u16) -> String {
        (1..LIST_WIDTH - 1)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect::<String>()
            .trim_end()
            .to_string()
    }

    #[test]
    fn the_tree_starts_closed_on_the_module_with_its_panel_and_help() {
        let (mut f, _) = with_tree();
        f.app.tree = crate::tree::Tree::default();
        f.app.refresh_tree();
        let buffer = render(&f.app);
        assert_has(&buffer, " Navegação ");
        assert_eq!(list_row(&buffer, 1), "> ▸ auto_scrum");
        // The panel of the module on the right.
        assert_has(&buffer, " auto_scrum ");
        assert_has(&buffer, "Projetos: 2 (1 indisponíveis)");
        assert_has(&buffer, "Agentes vivos neste nick: 0");
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · →/Enter abrir · ← fechar · a adicionar projeto · c configurações · \
             f mostrar ocultos · d esconder concluídos · v agentes ativos · q sair"
        );
        // Without a project: the text of before.
        let f = fixture();
        let buffer = render(&f.app);
        assert_eq!(list_row(&buffer, 1), ">   auto_scrum", "a leaf");
        assert_has(
            &buffer,
            "Nenhum projeto cadastrado. Pressione a para adicionar.",
        );
    }

    #[test]
    fn the_rows_are_indented_with_the_arrows_and_the_label_of_each_node() {
        let (f, _) = with_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        let rows: Vec<String> = (1..10).map(|y| list_row(&buffer, y)).collect();
        assert_eq!(
            rows,
            [
                "> ▾ auto_scrum",
                "    ▾ alfa",
                "      ▾ NS-1  [Tech Leader]  Um",
                "          Dev  parado  2026-11-19 21:46",
                "          PO  ativo em outro nick  2026-11-20 00:00",
                "        NS-2  [✓ Concluído]  Dois",
                "        abcd1234  [Aberto]  Quatro  [provisório]",
                "      beta  [indisponível: pasta não encontrada]",
                "",
            ]
            .map(|row| {
                // The end of a row is cut at the 38 columns inside the border (NS-34), and a
                // row not selected that does not fit ends in `…` since the NS-55.
                if row.chars().count() > 38 {
                    format!("{}…", row.chars().take(37).collect::<String>())
                } else {
                    row.to_string()
                }
            }),
            "{}",
            text(&buffer)
        );
    }

    #[test]
    fn the_styles_of_the_nodes() {
        let (mut f, _) = with_tree();
        f.app.tree.show_hidden = true;
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        let at = |needle| find(&buffer, needle).unwrap();
        let dim = |(y, x): (u16, u16)| buffer[(x, y)].modifier.contains(Modifier::DIM);
        assert!(!dim(at("alfa")));
        assert!(dim(at("beta  [indisponível")), "unavailable: dimmed");
        assert!(dim(at("NS-3  [Aberto]  Três  [oculto]")), "hidden: dimmed");
        assert!(!dim(at("NS-2")));
        assert!(dim(at("Dev  parado")), "not live: dimmed");
        assert!(dim(at("PO  ativo em outro nick")));
        let (y, x) = at("[✓ Concluído]");
        assert_eq!(buffer[(x + 1, y)].fg, Color::Green);
        let (y, x) = at("[Tech Leader]");
        assert_ne!(buffer[(x + 1, y)].fg, Color::Green);
        // The title says the filters.
        assert_has(&buffer, " Navegação (com ocultos) ");
        f.app.tree.only_active = true;
        f.app.refresh_tree();
        let buffer = render(&f.app);
        assert_has(&buffer, " Navegação (com ocultos) (só ativos) ");
        assert_has(&buffer, "Nenhum agente ativo.");
        f.app.tree.show_hidden = false;
        assert_has(&render(&f.app), " Navegação (só ativos) ");
    }

    #[test]
    fn control_characters_in_the_tree_and_in_the_panel_are_shown_as_question_marks() {
        let (mut f, root) = with_tree();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let id = match tnode(&f.app, "NS-2") {
            NodeId::Ticket(id) => id,
            _ => unreachable!(),
        };
        db.connection()
            .execute("UPDATE tickets SET title = 'a\x1b[2Jb' WHERE id = ?1", [id])
            .unwrap();
        f.app.refresh_tree();
        f.app.select(NodeId::Ticket(id));
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, "NS-2  [✓ Concluído]  a?[2Jb");
        assert_has(&buffer, "NS-2  a?[2Jb");
        assert!(!text(&buffer).contains('\x1b'));
    }

    #[test]
    fn the_panel_of_a_project_and_of_an_agent_not_live() {
        let (mut f, root) = with_tree();
        let alfa = f.app.rows[0].project.id;
        f.app.select(NodeId::Project(alfa));
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, " Projeto alfa ");
        assert_has(
            &buffer,
            &format!("Caminho: {}", root.join("alfa").display()),
        );
        assert_has(&buffer, "Situação: disponível");
        assert_has(&buffer, "Tickets: 4 (1 ocultos)");
        let beta = f.app.rows[1].project.id;
        f.app.select(NodeId::Project(beta));
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, " Projeto beta ");
        assert_has(&buffer, "Situação: [indisponível: pasta não encontrada]");
        assert_has(&buffer, "Tickets: 0");
        assert_has(
            &buffer,
            "Nenhum ticket neste projeto. Pressione a para criar.",
        );
        // A stopped agent and one active in another nick (NS-39: the actions of each state).
        let agents = f
            .app
            .owner_agents
            .values()
            .flatten()
            .cloned()
            .collect::<Vec<_>>();
        f.app.select(NodeId::Agent(agents[0].id));
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, " Agente ");
        assert_has(&buffer, "Nome: alfa · Dev · NS-1");
        assert_has(&buffer, "Etapa: Dev");
        assert_has(&buffer, "Estado: parado");
        assert_has(&buffer, "Aberto em: 2026-11-19 21:46 UTC");
        assert_has(&buffer, "Sessão: s-1");
        assert_has(&buffer, "r retomar esta conversa · o arquivar");
        assert!(
            !text(&buffer).contains("Retomar um agente"),
            "{}",
            text(&buffer)
        );
        assert!(
            !text(&buffer).contains("ID na abertura"),
            "the same ID: no line"
        );
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · r retomar · o arquivar · ← subir · f mostrar ocultos · \
             d esconder concluídos · v agentes \
             ativos · q sair"
        );
        f.app.select(NodeId::Agent(agents[1].id));
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, "Estado: ativo em outro nick");
        assert_has(
            &buffer,
            "Ativo em outro nick: feche-o lá para retomar aqui.",
        );
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · ← subir · f mostrar ocultos · d esconder concluídos · \
             v agentes ativos · q sair"
        );
    }

    /// The panel of a ticket: the ticket screen of the NS-36 without its agents (on the tree).
    #[test]
    fn the_panel_of_a_ticket_shows_the_stage_the_artifacts_and_the_history() {
        let (mut f, _) = with_tree();
        f.app.select(tnode(&f.app, "NS-1"));
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, " Ticket NS-1 ");
        assert_has(&buffer, "NS-1  Um");
        assert_has(&buffer, "Etapa atual: Tech Leader");
        assert_has(&buffer, "Plano (não existe):  .nick/docs/NS-1/plan.md");
        assert_has(
            &buffer,
            &format!("Review (não existe):  {}", docs::review_path("NS-1")),
        );
        assert_has(&buffer, "Histórico de etapas");
        // NS-45: with no change, the stage it started in is the current one.
        assert_has(&buffer, "Nenhuma mudança: o ticket começou em Tech Leader.");
        assert!(find(&buffer, "Agentes do ticket").is_none(), "on the tree");
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · →/Enter abrir · ← fechar/subir · e editar título · \
             o ocultar/restaurar · g abrir agente · m etapa · i ID · p épico · \
             f mostrar ocultos · d esconder concluídos · v agentes ativos · q sair"
        );
        // A history, oldest first, with the date as AAAA-MM-DD HH:MM UTC (raw when odd).
        let detail = f.app.ticket_detail.as_mut().unwrap();
        detail.changes = vec![
            change("po", "tech_leader", "2026-11-19T21:46:05Z"),
            change("tech_leader", "development", "ontem"),
        ];
        let buffer = render_at(&f.app, 140, 30, None);
        let (first, _) = find(
            &buffer,
            "2026-11-19 21:46 UTC  PO → Tech Leader  (operador)",
        )
        .unwrap();
        let (second, _) = find(&buffer, "ontem  Tech Leader → Dev  (operador)").unwrap();
        assert!(first < second);
        // Done, hidden and provisional, with the marks of the list.
        let mut t = staged(1, "abcd1234", "Fim", "done", true);
        t.is_provisional = true;
        f.app.ticket_detail.as_mut().unwrap().ticket = t;
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, "abcd1234  Fim  [provisório]  [oculto]");
        assert_has(&buffer, "Etapa atual: [✓ Concluído]");
        let (y, x) = find(&buffer, "Etapa atual: [✓ Concluído]").unwrap();
        assert_eq!(buffer[(x + 14, y)].fg, Color::Green);
    }

    #[test]
    fn a_long_history_in_the_panel_shows_the_newest_and_how_many_are_older() {
        let (mut f, _) = with_tree();
        f.app.select(tnode(&f.app, "NS-1"));
        f.app.refresh_tree();
        f.app.ticket_detail.as_mut().unwrap().changes = (0..40)
            .map(|i| {
                let (from, to) = if i % 2 == 0 {
                    ("po", "review")
                } else {
                    ("review", "po")
                };
                change(from, to, &format!("2026-11-19T10:{i:02}:00Z"))
            })
            .collect();
        let buffer = render(&f.app);
        assert_has(&buffer, "2026-11-19 10:39 UTC  Review → PO");
        assert!(
            find(&buffer, "2026-11-19 10:00 UTC").is_none(),
            "the oldest go"
        );
        let shown = text(&buffer).matches("2026-11-19 10:").count();
        assert_has(&buffer, &format!("(+{} mudanças mais antigas)", 40 - shown));
        let (cut, _) = find(&buffer, "mudanças mais antigas").unwrap();
        let (newest, _) = find(&buffer, "10:39").unwrap();
        assert!(cut < newest, "the count on top of the section");
        // With the whole height of the panel (no section of agents below it).
        assert!(shown >= 15, "{shown}");
    }

    /// Replaces `without_an_agent_the_border_of_the_list_is_as_before` (NS-37) and
    /// `without_an_agent_there_is_no_panel` (NS-33/34): decision 1 of NS-38, always side by
    /// side; without an agent the focus is on the tree.
    #[test]
    fn without_an_agent_the_tree_is_on_the_left_focused_and_the_panel_on_the_right() {
        let (mut f, _) = with_tree();
        for screen in [Screen::Tree, Screen::Settings] {
            f.app.screen = screen;
            let areas = layout(&f.app, Rect::new(0, 0, 100, 30));
            // Above the status and the help (the help of the module takes two rows here).
            let body = 30 - 1 - areas.help.height;
            assert_eq!(areas.list, Rect::new(0, 0, 40, body), "{screen:?}");
            assert_eq!(areas.panel, Rect::new(40, 0, 60, body), "{screen:?}");
            let buffer = render(&f.app);
            assert_border(&buffer, (0, 0), true, "┏");
            assert_border(&buffer, (39, 10), true, "┃");
            assert_border(&buffer, (40, 0), false, "┌");
            assert_border(&buffer, (99, 10), false, "│");
        }
        // Narrow: at most half the window.
        assert_eq!(layout(&f.app, Rect::new(0, 0, 60, 30)).list.width, 30);
    }

    #[test]
    fn the_live_agents_on_the_tree_and_the_mark_that_goes_up() {
        let (mut f, _) = with_running(&["proj", "fora"]);
        f.app.focus = Focus::List;
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, "Dev  ● vivo");
        assert_has(&buffer, "[FORA DA JAIL]");
        let (y, x) = find(&buffer, "[FORA DA JAIL]").unwrap();
        assert_eq!(buffer[(x + 1, y)].fg, Color::Red);
        let (y, x) = find(&buffer, "● vivo").unwrap();
        assert!(
            !buffer[(x, y)].modifier.contains(Modifier::DIM),
            "live: not dimmed"
        );
        // The first one marked: on its row; and on its closed ancestors.
        f.app.agents[0].unseen = true;
        f.app.relayout();
        assert_has(&render_at(&f.app, 140, 30, None), "Dev  ● saída nova");
        let proj = NodeId::Project(f.app.agents[0].project_id);
        f.app.tree.close(proj);
        f.app.relayout();
        let buffer = render_at(&f.app, 140, 30, None);
        let row = text(&buffer)
            .lines()
            .find(|line| line.contains("▸ proj"))
            .unwrap()
            .to_string();
        assert!(row.contains("▸ proj  ● saída nova"), "{row}");
        assert_eq!(
            help_text(&f.app).rsplit(" · ").nth(1).unwrap(),
            "v agentes ativos (2, 1 com saída nova)"
        );
    }

    #[test]
    fn the_help_of_each_node_with_the_filters_and_the_count() {
        let (mut f, _) = with_running(&["proj"]);
        f.app.focus = Focus::List;
        let live = f.app.selected;
        assert!(matches!(live, NodeId::Agent(_)));
        // NS-57, D9: the agent of `proj` runs in the jail, so the help has `l liberar jail`.
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · Enter ir ao terminal · ← subir · x encerrar · l liberar jail · \
             Ctrl+\\ alternar · f mostrar ocultos · d esconder concluídos · v agentes ativos (1) \
             · q sair"
        );
        f.app.tree.show_hidden = true;
        f.app.tree.only_active = true;
        assert!(
            help_text(&f.app).ends_with(
                "f esconder ocultos · d esconder concluídos · v mostrar tudo (1) · q sair"
            )
        );
        let project = NodeId::Project(f.app.agents[0].project_id);
        f.app.select(project);
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · →/Enter abrir · ← fechar/subir · a novo ticket · n novo épico · \
             r remover · f esconder ocultos · d esconder concluídos · v mostrar tudo (1) · q sair"
        );
        f.app.screen = Screen::Settings;
        assert_eq!(
            help_text(&f.app),
            "t trocar · p voltar ao padrão · Esc voltar · q sair"
        );
        // With the focus on the terminal: only the way back and the new line (NS-49).
        f.app.screen = Screen::Tree;
        f.app.select(live);
        f.app.focus = Focus::Terminal;
        assert_eq!(
            help_text(&f.app),
            "Ctrl+\\ voltar à lista · Alt+Enter nova linha"
        );
    }

    /// NS-49: with the focus on the terminal, the help shows the Alt+Enter, the new line that
    /// works on any terminal; with another shortcut only the shortcut changes.
    #[test]
    fn the_terminal_help_shows_alt_enter_for_a_new_line() {
        let (mut f, _) = with_running(&["proj"]);
        f.app.focus = Focus::Terminal;
        assert!(f.app.on_terminal());
        assert_eq!(
            help_text(&f.app),
            "Ctrl+\\ voltar à lista · Alt+Enter nova linha"
        );
        f.app.shortcut = Shortcut::from_db("ctrl+]").unwrap();
        assert_eq!(
            help_text(&f.app),
            "Ctrl+] voltar à lista · Alt+Enter nova linha"
        );
    }

    /// NS-49: the longer help of the terminal still fits the rows `help_rows` keeps, so the
    /// toggle of the focus resizes no PTY (NS-34).
    #[test]
    fn the_terminal_help_keeps_the_reserved_help_rows() {
        let (mut f, _) = with_running(&["proj"]);
        for width in [100, 60] {
            let area = Rect::new(0, 0, width, 30);
            f.app.focus = Focus::List;
            let list = layout(&f.app, area);
            f.app.focus = Focus::Terminal;
            assert!(f.app.on_terminal());
            let terminal = layout(&f.app, area);
            assert_eq!(list.help.height, terminal.help.height, "{width}");
            assert_eq!(list.terminal, terminal.terminal, "{width}");
        }
    }

    /// Every place of the help is in `HELP_STATES`, with both filters (the rows of the help
    /// with an agent running never change between them; review NS-34, P1).
    #[test]
    fn every_help_state_is_in_help_states() {
        for place in [
            Place::Module,
            Place::Project,
            Place::Epic,
            Place::Ticket,
            Place::LiveAgent,
            Place::LiveAgentInJail,
            Place::LiveAgentReleased,
            Place::StoppedAgent,
            Place::StoppedAgentInJail,
            Place::ArchivedAgent,
            Place::ElsewhereAgent,
            Place::Settings,
        ] {
            // Exhaustive: a new place does not compile here until it is listed above.
            match place {
                Place::Module
                | Place::Project
                | Place::Epic
                | Place::Ticket
                | Place::LiveAgent
                | Place::LiveAgentInJail
                | Place::LiveAgentReleased
                | Place::StoppedAgent
                | Place::StoppedAgentInJail
                | Place::ArchivedAgent
                | Place::ElsewhereAgent
                | Place::Settings => {}
            }
            let filters: &[(bool, bool)] = if place == Place::Settings {
                &[(false, false)]
            } else {
                &[(false, false), (true, false), (false, true), (true, true)]
            };
            for &(hidden, active) in filters {
                assert!(
                    HELP_STATES.contains(&(place, hidden, false, active)),
                    "{place:?} {hidden} {active}"
                );
            }
        }
    }

    /// NS-57, D9: `l` is in the help only where it works: a live agent of a project with the jail
    /// (to release it, or to jail it again), a stopped one whose project the last check said has
    /// the jail on.
    #[test]
    fn the_help_has_l_only_where_it_works() {
        let (mut f, root) = with_running(&["proj"]);
        f.app.focus = Focus::List;
        let keys = |f: &Fixture| help_text(&f.app).split(" · f ").next().unwrap().to_string();
        assert_eq!(
            keys(&f),
            "↑/↓ mover · Enter ir ao terminal · ← subir · x encerrar · l liberar jail · \
             Ctrl+\\ alternar"
        );
        f.app.agents[0].jail = false;
        assert_eq!(
            keys(&f),
            "↑/↓ mover · Enter ir ao terminal · ← subir · x encerrar · l prender na jail · \
             Ctrl+\\ alternar"
        );
        f.app.agents[0].project_jail = false;
        assert_eq!(
            keys(&f),
            "↑/↓ mover · Enter ir ao terminal · ← subir · x encerrar · Ctrl+\\ alternar",
            "the help of before"
        );
        // A stopped agent of the same project.
        let db = Db::open(&root.join("nick.db")).unwrap();
        let Owner::Ticket(ticket) = f.app.agents[0].owner else {
            panic!("a ticket agent")
        };
        let ended = db
            .insert_agent(
                Owner::Ticket(ticket),
                "po",
                "s-x",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        db.stop_agent(ended.id).unwrap();
        f.app.refresh_tree();
        f.app.select(NodeId::Agent(ended.id));
        for (jail, expected) in [
            (None, "↑/↓ mover · r retomar · o arquivar · ← subir"),
            (Some(false), "↑/↓ mover · r retomar · o arquivar · ← subir"),
            (
                Some(true),
                "↑/↓ mover · r retomar · o arquivar · l liberar jail · ← subir",
            ),
        ] {
            f.app.rows[0].jail = jail;
            assert_eq!(keys(&f), expected, "{jail:?}");
            assert_eq!(
                f.app.jail_toggle(ended.id).is_some(),
                expected.contains("l liberar"),
                "the help and the key agree: {jail:?}"
            );
        }
    }

    /// NS-57, D9: the helps with `l` are shorter than the longest one of before, so the rows the
    /// help reserves (and the size of every terminal) stay the same at any width.
    #[test]
    fn the_longest_help_is_still_the_one_of_before() {
        let f = fixture();
        let before = [
            Place::Module,
            Place::Project,
            Place::Epic,
            Place::Ticket,
            Place::LiveAgent,
            Place::StoppedAgent,
            Place::ArchivedAgent,
            Place::ElsewhereAgent,
            Place::Settings,
        ];
        let count = Some("9, 9 com saída nova".to_string());
        for width in 20..=200 {
            let rows = |all: bool| {
                HELP_STATES
                    .into_iter()
                    .filter(|state| all || before.contains(&state.0))
                    .map(|state| {
                        let text = help_text_for(&f.app, state, false, None, count.clone());
                        wrap_words(&text, width).len()
                    })
                    .max()
                    .unwrap()
            };
            assert_eq!(rows(true), rows(false), "at {width} columns");
        }
    }

    /// Moving the selection between the kinds of node and toggling the filters resizes no
    /// terminal (the plan of NS-38, step 11; review NS-34, P1).
    #[test]
    fn moving_on_the_tree_and_the_filters_resize_no_terminal() {
        let (mut f, root) = with_running(&["proj", "fora"]);
        // An ended agent of `proj`, beside the live one.
        let db = Db::open(&root.join("nick.db")).unwrap();
        let Owner::Ticket(ticket) = f.app.agents[0].owner else {
            panic!("a ticket agent")
        };
        let ended = db
            .insert_agent(
                Owner::Ticket(ticket),
                "po",
                "s-x",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        db.stop_agent(ended.id).unwrap();
        f.app.refresh_tree();
        let nodes = [
            MODULE,
            NodeId::Project(f.app.agents[0].project_id),
            NodeId::Ticket(ticket),
            NodeId::Agent(f.app.agents[0].id),
            NodeId::Agent(ended.id),
        ];
        for (width, height) in [(100, 30), (120, 40), (50, 30), (140, 40), (80, 24)] {
            let area = Rect::new(0, 0, width, height);
            let mut sizes: Vec<Vec<Rect>> = Vec::new();
            for screen in [Screen::Tree, Screen::Settings] {
                for node in nodes {
                    for (hidden, active, done) in [
                        (false, false, false),
                        (true, false, false),
                        (false, true, false),
                        (true, true, false),
                        // NS-54: the filter of the done ones.
                        (false, false, true),
                        (true, true, true),
                    ] {
                        for focus in [Focus::List, Focus::Terminal] {
                            for unseen in [false, true] {
                                f.app.screen = screen;
                                f.app.select(node);
                                f.app.tree.show_hidden = hidden;
                                f.app.tree.only_active = active;
                                f.app.tree.hide_done = done;
                                f.app.focus = focus;
                                f.app.agents[1].unseen = unseen;
                                f.app.relayout();
                                sizes.push(
                                    f.app
                                        .agents()
                                        .iter()
                                        .map(|agent| agent_area(&f.app, area, agent))
                                        .collect(),
                                );
                            }
                        }
                    }
                }
            }
            assert_eq!(
                sizes.iter().find(|s| **s != sizes[0]),
                None,
                "{width}x{height}: {:?}",
                sizes[0]
            );
        }
    }

    #[test]
    fn a_tiny_terminal_does_not_panic_on_the_tree() {
        let (mut f, _) = with_tree();
        for node in [MODULE, tnode(&f.app, "NS-1")] {
            f.app.select(node);
            f.app.refresh_tree();
            for (w, h) in [(1, 1), (2, 2), (3, 3), (10, 3), (20, 6), (40, 12)] {
                render_at(&f.app, w, h, None);
            }
        }
        let (f, _) = with_running(&["proj"]);
        for (w, h) in [(1, 1), (3, 3), (20, 6)] {
            render_at(&f.app, w, h, None);
        }
    }

    /// A stage the database should never have (the `CHECK` forbids it) shows raw on the tree,
    /// with no panic (NS-36).
    #[test]
    fn an_unknown_stage_shows_raw_on_the_tree() {
        let (spans, _) = ticket_spans(&staged(4, "NS-4", "Quatro", "archived", false));
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "NS-4  [archived]  Quatro");
    }

    /// Review of the NS-38, P1: the mark of new output shows whole on the row of the agent at
    /// the real width of the tree (28 columns at depth 3), whatever the stage and the jail. A
    /// marked agent is live: the mark takes the place of `● vivo`, before `[FORA DA JAIL]`.
    #[test]
    fn the_mark_of_a_live_agent_fits_in_the_tree_out_of_the_jail() {
        let (mut f, root) = with_running(&["fora"]);
        f.app.focus = Focus::List;
        let db = Db::open(&root.join("nick.db")).unwrap();
        let id = f.app.agents[0].id;
        db.connection()
            .execute(
                "UPDATE agents SET stage = 'tech_leader' WHERE id = ?1",
                [id],
            )
            .unwrap();
        f.app.refresh_tree();
        f.app.agents[0].unseen = true;
        f.app.relayout();
        let buffer = render(&f.app);
        let (y, _) = find(&buffer, "Tech Leader").expect("the row of the agent");
        let row = list_row(&buffer, y);
        assert!(row.contains("Tech Leader  ● saída nova"), "{row:?}");
        assert!(!row.contains("● vivo"), "the mark in its place: {row:?}");
        // Seen, `● vivo` and `[FORA DA JAIL]` again (the end cut at 40 columns).
        f.app.agents[0].unseen = false;
        let row = list_row(&render(&f.app), y);
        assert!(row.contains("Tech Leader  ● vivo  [FORA"), "{row:?}");
    }

    // ---- NS-39: the stopped and the archived agents ----

    /// The agents of NS-1 of `with_tree`: the stopped Dev and the PO active in another nick.
    fn ns1_agents(f: &Fixture) -> Vec<Agent> {
        let mut agents: Vec<Agent> = f.app.owner_agents.values().flatten().cloned().collect();
        agents.sort_by_key(|a| a.id);
        agents
    }

    #[test]
    fn an_archived_agent_is_dimmed_marked_and_only_with_the_filter() {
        let (mut f, root) = with_tree();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let dev = ns1_agents(&f)[0].id;
        db.archive_agent(dev).unwrap();
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        assert!(
            find(&buffer, "Dev  arquivado").is_none(),
            "out of the tree without f"
        );
        f.app.tree.show_hidden = true;
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        let (y, x) =
            find(&buffer, "Dev  [arquivado]  2026").unwrap_or_else(|| panic!("{}", text(&buffer)));
        assert!(buffer[(x, y)].modifier.contains(Modifier::DIM), "dimmed");
        // Its panel and its help: only the way back.
        f.app.select(NodeId::Agent(dev));
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, "Estado: arquivado");
        assert_has(&buffer, "o desarquivar");
        assert!(!text(&buffer).contains("r retomar"), "{}", text(&buffer));
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · o desarquivar · ← subir · f esconder ocultos · \
             d esconder concluídos · v agentes ativos · q sair"
        );
    }

    #[test]
    fn a_live_agent_and_a_stopped_one_are_told_apart() {
        let (mut f, root) = with_running(&["proj"]);
        let db = Db::open(&root.join("nick.db")).unwrap();
        let Owner::Ticket(ticket) = f.app.agents[0].owner else {
            panic!("a ticket agent")
        };
        let stopped = db
            .insert_agent(
                Owner::Ticket(ticket),
                "po",
                "s-x",
                "proj · PO · NS-1",
                "NS-1",
            )
            .unwrap();
        db.stop_agent(stopped.id).unwrap();
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        let at = |needle| find(&buffer, needle).unwrap_or_else(|| panic!("{needle}"));
        let dim = |(y, x): (u16, u16)| buffer[(x, y)].modifier.contains(Modifier::DIM);
        assert!(!dim(at("● vivo")), "live: not dimmed");
        assert!(dim(at("PO  parado")), "stopped: dimmed");
    }

    #[test]
    fn the_panel_says_the_id_at_the_opening_when_it_is_not_the_id_of_now() {
        let (mut f, root) = with_tree();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let dev = ns1_agents(&f)[0].id;
        db.connection()
            .execute("UPDATE agents SET opened_key = 'NS-0' WHERE id = ?1", [dev])
            .unwrap();
        f.app.refresh_tree();
        f.app.select(NodeId::Agent(dev));
        assert_has(&render_at(&f.app, 140, 30, None), "ID na abertura: NS-0");
        // Before the NS-39 (NULL): from the name, the same ID, no line.
        db.connection()
            .execute("UPDATE agents SET opened_key = NULL WHERE id = ?1", [dev])
            .unwrap();
        f.app.refresh_tree();
        assert!(!text(&render_at(&f.app, 140, 30, None)).contains("ID na abertura"));
        // A name that does not give it: unknown.
        db.connection()
            .execute("UPDATE agents SET name = 'sem id' WHERE id = ?1", [dev])
            .unwrap();
        f.app.refresh_tree();
        assert_has(
            &render_at(&f.app, 140, 30, None),
            "ID na abertura: desconhecido",
        );
    }

    /// NS-50: the panel shows the effective session and, when the resume follows another one, the
    /// session of the opening (the key of the lock) too.
    #[test]
    fn the_panel_shows_the_effective_session_and_the_one_of_the_opening() {
        let (mut f, root) = with_tree();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let dev = ns1_agents(&f)[0].id;
        f.app.select(NodeId::Agent(dev));
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, "Sessão: s-1");
        assert!(
            !text(&buffer).contains("Sessão na abertura"),
            "no swap: no line"
        );
        db.connection()
            .execute(
                "UPDATE agents SET resume_session_id = 's-b' WHERE id = ?1",
                [dev],
            )
            .unwrap();
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, "Sessão: s-b");
        assert_has(&buffer, "Sessão na abertura: s-1");
        assert!(!text(&buffer).contains("Sessão: s-1"), "{}", text(&buffer));
        assert_has(&buffer, "r retomar esta conversa · o arquivar");
        // Its control characters do not reach the terminal.
        db.connection()
            .execute(
                "UPDATE agents SET resume_session_id = 's\x1bb' WHERE id = ?1",
                [dev],
            )
            .unwrap();
        f.app.refresh_tree();
        assert_has(&render_at(&f.app, 140, 30, None), "Sessão: s?b");
    }

    #[test]
    fn the_dialogs_of_the_lost_conversation_and_of_archiving() {
        let (mut f, _) = with_tree();
        let agent = ns1_agents(&f)[0].clone();
        let Owner::Ticket(ticket_id) = agent.owner else {
            panic!("a ticket agent")
        };
        let ticket = f.app.ticket(ticket_id).unwrap().clone();
        f.app.dialog = Some(Dialog::ConversationLost {
            project: PathBuf::from("/alfa"),
            agent: agent.clone(),
            holder: (&ticket).into(),
            precheck: Precheck {
                jail: true,
                jira: false,
                sentry: false,
                jira_url: None,
                warning: None,
            },
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Conversa não encontrada ");
        assert!(
            flat(&buffer).contains(
                "A conversa do agente alfa · Dev · NS-1 não existe mais no claude (o histórico \
                 foi apagado ou o projeto mudou de pasta). Nada foi aberto."
            ),
            "{}",
            text(&buffer)
        );
        assert_eq!(
            last_line(&buffer).trim(),
            "a arquivar · n abrir novo agente nesta etapa · Esc cancelar"
        );
        f.app.dialog = Some(Dialog::ConfirmArchive { agent });
        let buffer = render(&f.app);
        assert_has(&buffer, " Arquivar agente ");
        assert!(
            flat(&buffer).contains(
                "Arquivar o agente alfa · Dev · NS-1? Ele sai da árvore e volta com f. (s/N)"
            ),
            "{}",
            text(&buffer)
        );
        assert_eq!(
            last_line(&buffer).trim(),
            "s arquivar · qualquer outra tecla cancela"
        );
    }

    /// Review of the NS-39, point 6: a state this nick does not know is not shown as stopped,
    /// and its panel and its help offer no action.
    #[test]
    fn an_unknown_state_is_shown_as_unknown_with_no_action() {
        let (mut f, root) = with_tree();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let dev = ns1_agents(&f)[0].id;
        db.connection()
            .execute("UPDATE agents SET state = 'futuro' WHERE id = ?1", [dev])
            .unwrap();
        f.app.refresh_tree();
        f.app.select(NodeId::Agent(dev));
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, "Dev  estado desconhecido");
        assert_has(&buffer, "Estado: desconhecido (futuro)");
        assert_has(
            &buffer,
            "Este nick não conhece o estado deste agente: nada a fazer aqui.",
        );
        assert!(!text(&buffer).contains("r retomar"), "{}", text(&buffer));
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · ← subir · f mostrar ocultos · d esconder concluídos · \
             v agentes ativos · q sair"
        );
    }

    // ---- the epics (NS-40) ----

    /// A project `proj` with the epic EPIC-1 (the ticket NS-1 in it) and the ticket NS-2 without
    /// epic, the epic selected with its branches open. The second connection stores the rows.
    fn with_epic_tree() -> (Fixture, Db, PathBuf) {
        let mut f = fixture();
        let root = std::fs::canonicalize(f._tmp.path()).unwrap();
        let dir = root.join("proj");
        std::fs::create_dir_all(dir.join(".nick")).unwrap();
        std::fs::write(dir.join(".nick/nick.conf"), "").unwrap();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let p = db
            .insert_project("auto_scrum", "proj", dir.to_str().unwrap())
            .unwrap();
        let e = db
            .insert_epic(p.id, "EPIC-1", false, "Plano longo")
            .unwrap();
        let t = db
            .insert_ticket(p.id, "NS-1", false, "Dentro", Some(e.id))
            .unwrap();
        db.insert_ticket(p.id, "NS-2", false, "Fora", None).unwrap();
        f.app.refresh_tree();
        f.app.reveal_and_select(NodeId::Ticket(t.id));
        f.app.refresh_tree();
        f.app.select(NodeId::Epic(e.id));
        f.app.refresh_tree();
        (f, db, dir)
    }

    fn spans_text(spans: &[Span]) -> String {
        spans.iter().map(|span| span.content.as_ref()).collect()
    }

    #[test]
    fn the_label_of_an_epic_has_the_mark_bold_and_dims_when_hidden() {
        let mut epic = Epic {
            id: 1,
            project_id: 1,
            key: "EPIC-1".into(),
            is_provisional: false,
            title: "Plano".into(),
            created_at: String::new(),
            hidden_at: None,
            done_at: None,
        };
        let (spans, style) = epic_spans(&epic);
        // NS-54: the state right after `[épico]` (`[aberto]`, decision of the user).
        assert_eq!(spans_text(&spans), "EPIC-1  [épico]  [aberto]  Plano");
        assert_eq!(spans[2].content, "[épico]");
        assert!(spans[2].style.add_modifier.contains(Modifier::BOLD));
        assert_eq!(spans[2].style.fg, None, "the mark only bold");
        // NS-45: the ID and the title in purple.
        assert_eq!(spans[0].style.fg, Some(EPIC_COLOR));
        assert_eq!(spans[5].style.fg, Some(EPIC_COLOR));
        assert!(!style.add_modifier.contains(Modifier::DIM));
        epic.is_provisional = true;
        epic.hidden_at = Some("x".into());
        let (spans, style) = epic_spans(&epic);
        assert_eq!(
            spans_text(&spans),
            "EPIC-1  [épico]  [aberto]  Plano  [provisório]  [oculto]"
        );
        assert!(style.add_modifier.contains(Modifier::DIM));
        assert_eq!(spans[6].style.fg, None, "[provisório] with no color");
        assert_eq!(spans[7].style.fg, None, "[oculto] with no color");
        epic.title = "\x1b[2J".into();
        assert_eq!(
            spans_text(&epic_spans(&epic).0),
            "EPIC-1  [épico]  [aberto]  ?[2J  [provisório]  [oculto]"
        );
    }

    #[test]
    fn the_tree_shows_the_epic_before_the_tickets_and_a_long_id_fits_with_its_mark() {
        let (f, db, _) = with_epic_tree();
        let buffer = render(&f.app);
        let (epic_row, _) = find(&buffer, "EPIC-1  [épico]").expect("the epic");
        let (inside, _) = find(&buffer, "NS-1  [Aberto]").expect("its ticket");
        let (outside, _) = find(&buffer, "NS-2  [Aberto]").expect("the loose ticket");
        assert!(epic_row < inside && inside < outside, "{}", text(&buffer));
        // The highlight symbol (2), three levels (6) and a leaf (2).
        assert!(
            list_row(&buffer, inside).starts_with("          NS-1"),
            "depth 3: {:?}",
            list_row(&buffer, inside)
        );
        // An ID of 8 characters and the mark fit in the 34 columns of depth 2.
        let p = f.app.project_rows()[0].project.id;
        db.insert_epic(p, "abcd1234", true, "um título bem longo que é cortado")
            .unwrap();
        let mut f = f;
        f.app.refresh_tree();
        let buffer = render(&f.app);
        let (y, _) = find(&buffer, "abcd1234").expect("the long one");
        assert!(
            list_row(&buffer, y).contains("abcd1234  [épico]"),
            "{:?}",
            list_row(&buffer, y)
        );
    }

    /// NS-53: the line `Jira: <link>` of the panel of a ticket, right after its head (or after the
    /// line of its epic) and before the stage; none without a link.
    #[test]
    fn the_panel_of_a_ticket_shows_its_jira_link() {
        let (mut f, _) = with_tree();
        f.app.select(tnode(&f.app, "NS-1"));
        f.app.refresh_tree();
        let without = text(&render_at(&f.app, 140, 30, None));
        assert!(!without.contains("Jira:"), "no link: no line\n{without}");
        f.app.ticket_detail.as_mut().unwrap().jira_link = Some("https://j/browse/NS-1".into());
        let buffer = render_at(&f.app, 140, 30, None);
        let (head, _) = find(&buffer, "NS-1  Um").unwrap();
        let (jira, _) = find(&buffer, "Jira: https://j/browse/NS-1").unwrap();
        let (stage, _) = find(&buffer, "Etapa atual:").unwrap();
        assert_eq!((head + 1, jira + 1), (jira, stage), "{}", text(&buffer));
        // Without the line, the rest of the panel is the one of before, a row up.
        let with = text(&buffer);
        let with: Vec<&str> = with.lines().filter(|l| !l.contains("Jira:")).collect();
        let before: Vec<&str> = without.lines().collect();
        assert_eq!(with[..stage as usize - 1], before[..stage as usize - 1]);
        // A control character of the base shows as "?".
        f.app.ticket_detail.as_mut().unwrap().jira_link = Some("https://j\x1b[2J/NS-1".into());
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(&buffer, "Jira: https://j?[2J/NS-1");
    }

    #[test]
    fn the_jira_link_of_a_ticket_in_an_epic_comes_after_the_epic() {
        let (mut f, db, _) = with_epic_tree();
        let t = db
            .list_tickets(f.app.project_rows()[0].project.id, true)
            .unwrap()
            .into_iter()
            .find(|t| t.key == "NS-1")
            .unwrap();
        f.app.select(NodeId::Ticket(t.id));
        f.app.refresh_tree();
        f.app.ticket_detail.as_mut().unwrap().jira_link = Some("https://j/browse/NS-1".into());
        let buffer = render(&f.app);
        let (epic, _) = find(&buffer, "Épico: EPIC-1 — Plano longo").unwrap();
        let (jira, _) = find(&buffer, "Jira: https://j/browse/NS-1").unwrap();
        let (stage, _) = find(&buffer, "Etapa atual:").unwrap();
        assert_eq!((epic + 1, jira + 1), (jira, stage), "{}", text(&buffer));
    }

    /// NS-53: the line `Jira: <link>` of the panel of an epic, right after `Pasta:`.
    #[test]
    fn the_panel_of_an_epic_shows_its_jira_link() {
        let (mut f, _, _) = with_epic_tree();
        let without = text(&render(&f.app));
        assert!(!without.contains("Jira:"), "no link: no line\n{without}");
        f.app.epic_detail.as_mut().unwrap().jira_link = Some("https://j/browse/EPIC-1".into());
        let buffer = render(&f.app);
        let (folder, _) = find(&buffer, "Pasta: .nick/docs/EPIC-1/").unwrap();
        let (jira, _) = find(&buffer, "Jira: https://j/browse/EPIC-1").unwrap();
        let (plan, _) = find(&buffer, "Plano (não existe)").unwrap();
        assert_eq!((folder + 1, jira + 1), (jira, plan), "{}", text(&buffer));
        f.app.epic_detail.as_mut().unwrap().jira_link = Some("https://j\x07/EPIC-1".into());
        let buffer = render(&f.app);
        assert_has(&buffer, "Jira: https://j?/EPIC-1");
    }

    #[test]
    fn the_panel_of_an_epic_has_no_stage_history_nor_m() {
        let (mut f, _, dir) = with_epic_tree();
        let buffer = render(&f.app);
        assert_has(&buffer, " Épico EPIC-1 ");
        assert_has(&buffer, "Épico EPIC-1 — Plano longo");
        assert_has(&buffer, "Pasta: .nick/docs/EPIC-1/");
        assert_has(&buffer, "Plano (não existe):  .nick/docs/EPIC-1/plan.md");
        assert_has(&buffer, "Tickets: 1");
        assert_has(&buffer, "Agentes do épico: só PO e Tech Leader.");
        for absent in ["Etapa", "Histórico", "m etapa", "mudar etapa"] {
            assert!(
                find(&buffer, absent).is_none(),
                "{absent}: {}",
                text(&buffer)
            );
        }
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · →/Enter abrir · ← fechar/subir · a novo ticket · e editar título · \
             o ocultar/restaurar · c concluir/reabrir · g abrir agente · i editar ID · \
             f mostrar ocultos · d esconder concluídos · v agentes ativos · q sair"
        );
        // The plan from the disk, and the hidden tickets counted.
        std::fs::create_dir_all(dir.join(".nick/docs/EPIC-1")).unwrap();
        std::fs::write(dir.join(".nick/docs/EPIC-1/plan.md"), "x").unwrap();
        f.app.refresh_tree();
        if let Some(detail) = f.app.epic_detail.as_mut() {
            detail.hidden_tickets = 1;
        }
        let buffer = render(&f.app);
        assert_has(&buffer, "Plano (existe):  .nick/docs/EPIC-1/plan.md");
        assert_has(&buffer, "Tickets: 1 (1 oculto)");
        // The plural (review of the NS-40, P3.2).
        if let Some(detail) = f.app.epic_detail.as_mut() {
            detail.tickets = 3;
            detail.hidden_tickets = 2;
        }
        assert_has(&render(&f.app), "Tickets: 3 (2 ocultos)");
    }

    #[test]
    fn the_panel_of_a_ticket_in_an_epic_says_the_epic() {
        let (mut f, _, _) = with_epic_tree();
        let inside = f
            .app
            .tickets
            .values()
            .flatten()
            .find(|t| t.key == "NS-1")
            .unwrap()
            .id;
        let outside = f
            .app
            .tickets
            .values()
            .flatten()
            .find(|t| t.key == "NS-2")
            .unwrap()
            .id;
        f.app.select(NodeId::Ticket(inside));
        f.app.refresh_tree();
        assert_has(&render(&f.app), "Épico: EPIC-1 — Plano longo");
        f.app.select(NodeId::Ticket(outside));
        f.app.refresh_tree();
        assert!(
            find(&render(&f.app), "Épico:").is_none(),
            "only with an epic"
        );
    }

    #[test]
    fn the_project_help_has_n_and_the_epic_place_is_in_the_help_states() {
        let (mut f, _, _) = with_epic_tree();
        let p = f.app.project_rows()[0].project.id;
        f.app.select(NodeId::Project(p));
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · →/Enter abrir · ← fechar/subir · a novo ticket · n novo épico · \
             r remover · f mostrar ocultos · d esconder concluídos · v agentes ativos · q sair"
        );
        // NS-57: 89 (the 3 places of the `l`); 65 before.
        assert_eq!(HELP_STATES.len(), 89);
        assert_eq!(
            HELP_STATES
                .iter()
                .filter(|(place, ..)| *place == Place::Epic)
                .count(),
            8
        );
    }

    #[test]
    fn the_dialogs_of_the_epic() {
        let (mut f, _, _) = with_epic_tree();
        let epic = f
            .app
            .epic(f.app.project_epics(f.app.project_rows()[0].project.id)[0].id)
            .unwrap()
            .clone();
        let p = epic.project_id;
        f.app.dialog = Some(Dialog::NewTicket {
            project_id: p,
            item: NewItem::Epic,
            title: String::new(),
            jira_id: String::new(),
            focus: TicketField::Title,
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Novo épico ");
        assert_has(
            &buffer,
            "ID do Jira (opcional; vazio gera um ID provisório):",
        );
        f.app.dialog = Some(Dialog::ConfirmHideEpic {
            epic: epic.clone(),
            tickets: 3,
        });
        assert!(flat(&render(&f.app)).contains(
            "Ocultar o épico EPIC-1? O ramo inteiro (3 tickets) sai da árvore e volta com f. (s/N)"
        ));
        for (tickets, text) in [(1, "(1 ticket)"), (0, "(0 tickets)")] {
            f.app.dialog = Some(Dialog::ConfirmHideEpic {
                epic: epic.clone(),
                tickets,
            });
            assert!(
                flat(&render(&f.app)).contains(&format!("O ramo inteiro {text} sai")),
                "{tickets}"
            );
        }
        f.app.dialog = Some(Dialog::EditEpicTitle {
            epic: epic.clone(),
            input: "Plano".into(),
        });
        assert_has(&render(&f.app), " Editar título do épico ");
        f.app.dialog = Some(Dialog::EditEpicId {
            epic: epic.clone(),
            input: "EPIC-1".into(),
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Editar ID do épico ");
        assert_has(&buffer, "Novo ID do épico (ex: SC-123):");
        f.app.dialog = Some(Dialog::ConfirmEditEpicId {
            epic: epic.clone(),
            new: "EPIC-2".into(),
            moves: true,
        });
        assert!(flat(&render(&f.app)).contains(
            "Trocar o ID EPIC-1 por EPIC-2? A pasta .nick/docs/EPIC-1/ será renomeada para \
             .nick/docs/EPIC-2/."
        ));
        let ticket = f
            .app
            .tickets
            .values()
            .flatten()
            .find(|t| t.key == "NS-1")
            .unwrap()
            .clone();
        f.app.dialog = Some(Dialog::MoveTicket {
            ticket,
            choices: vec![None, Some(epic.clone())],
            index: 0,
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Mover ticket ");
        assert_has(&buffer, "Ticket NS-1 — Dentro");
        assert_has(&buffer, "> Sem épico");
        assert_has(&buffer, "EPIC-1 — Plano longo (atual)");
        assert_eq!(
            last_line(&buffer),
            "↑/↓ escolher · Enter mover · Esc cancelar"
        );
        // "Abrir agente" of an epic names it.
        f.app.dialog = Some(Dialog::ChooseType {
            project: PathBuf::from("/proj"),
            holder: (&epic).into(),
            precheck: precheck(true),
            types: StageType::epic_menu(false, false),
            index: 0,
        });
        let buffer = render(&f.app);
        assert_has(&buffer, "Épico EPIC-1 — Plano longo");
        assert!(find(&buffer, "Desenvolvimento").is_none());
    }

    #[test]
    fn the_removal_confirmation_counts_the_epics_only_when_there_are_any() {
        let question = |epics| removal_question("alfa", "/w/alfa", 2, 0, 0, epics);
        assert_eq!(
            question(0),
            "Remover o projeto alfa (/w/alfa) do nick? Os arquivos do projeto não são tocados. \
             Os 2 tickets dele (inclusive os ocultos) serão apagados do banco. (s/N)"
        );
        assert_eq!(
            question(1),
            "Remover o projeto alfa (/w/alfa) do nick? Os arquivos do projeto não são tocados. \
             Os 2 tickets dele (inclusive os ocultos) serão apagados do banco. O épico dele será \
             apagado do banco. (s/N)"
        );
        assert_eq!(
            removal_question("alfa", "/w/alfa", 0, 0, 0, 3),
            "Remover o projeto alfa (/w/alfa) do nick? Os arquivos do projeto não são tocados. \
             Os 3 épicos dele (inclusive os ocultos) serão apagados do banco. (s/N)"
        );
    }

    /// The agent at depth 4 (a ticket of an epic) keeps its stage and its state whole in the 26
    /// columns left: `Tech Leader  ● saída nova` has 25; the date is cut first.
    #[test]
    fn an_agent_at_depth_4_keeps_its_mark_whole() {
        let (mut f, root) = with_running(&["fora"]);
        f.app.focus = Focus::List;
        let db = Db::open(&root.join("nick.db")).unwrap();
        let id = f.app.agents[0].id;
        let Owner::Ticket(ticket) = f.app.agents[0].owner else {
            panic!("a ticket agent")
        };
        let project = f.app.agents[0].project_id;
        let epic = db.insert_epic(project, "EPIC-1", false, "e").unwrap();
        db.set_ticket_epic(ticket, Some(epic.id)).unwrap();
        db.connection()
            .execute(
                "UPDATE agents SET stage = 'tech_leader' WHERE id = ?1",
                [id],
            )
            .unwrap();
        f.app.refresh_tree();
        f.app.reveal_and_select(NodeId::Agent(id));
        f.app.refresh_tree();
        f.app.agents[0].unseen = true;
        f.app.relayout();
        let depth = f
            .app
            .visible()
            .iter()
            .find(|r| r.id == NodeId::Agent(id))
            .unwrap()
            .depth;
        assert_eq!(depth, 4);
        let buffer = render(&f.app);
        let (y, _) = find(&buffer, "Tech Leader").expect("the row of the agent");
        let row = list_row(&buffer, y);
        assert!(row.contains("Tech Leader  ● saída nova"), "{row:?}");
    }

    // ----- NS-51: the cat Nick at the bottom of the column on the left -----

    use crate::mascot::tests::{APPROVED_BLINK, APPROVED_PALETTE, APPROVED_REST, showing};
    use crate::mascot::{MASCOT_COLS, MASCOT_ROWS, Pose};

    /// The index of the 256 palette of each letter (the table of D8 of the plan), independent
    /// of `nearest_256`.
    const APPROVED_INDEXED: [(char, u8); 7] = [
        ('D', 235),
        ('B', 240),
        ('C', 180),
        ('W', 255),
        ('E', 74),
        ('K', 233),
        ('P', 181),
    ];

    fn approved_color(letter: char, truecolor: bool) -> Option<Color> {
        if truecolor {
            APPROVED_PALETTE
                .iter()
                .find(|(key, _)| *key == letter)
                .map(|(_, (r, g, b))| Color::Rgb(*r, *g, *b))
        } else {
            APPROVED_INDEXED
                .iter()
                .find(|(key, _)| *key == letter)
                .map(|(_, index)| Color::Indexed(*index))
        }
    }

    /// The inside of the column on the left of `app` on a screen `width` x `height`.
    fn column_inner(app: &App, width: u16, height: u16) -> Rect {
        Block::bordered().inner(layout(app, Rect::new(0, 0, width, height)).list)
    }

    /// The fixture with `n` projects, the module open (more nodes than rows at 30 rows).
    fn with_projects(n: usize) -> Fixture {
        let mut f = fixture();
        let root = std::fs::canonicalize(f._tmp.path()).unwrap();
        let db = Db::open(&root.join("nick.db")).unwrap();
        for i in 0..n {
            let dir = root.join(format!("p{i:02}"));
            std::fs::create_dir_all(&dir).unwrap();
            db.insert_project("auto_scrum", &format!("p{i:02}"), dir.to_str().unwrap())
                .unwrap();
        }
        f.app.refresh_tree();
        f.app
            .tree
            .open(NodeId::Module(crate::modules::ModuleKind::AutoScrum));
        f.app.refresh_tree();
        f
    }

    /// The cells of `rect` with a half block (`▀`/`▄`).
    fn half_blocks(buffer: &Buffer, rect: Rect) -> usize {
        rect.positions()
            .filter(|p| ["▀", "▄"].contains(&buffer[(p.x, p.y)].symbol()))
            .count()
    }

    #[test]
    fn the_cat_needs_15_rows_and_17_columns_inside_the_column() {
        let inner = |width, height| Rect::new(1, 1, width, height);
        assert_eq!(mascot_area(inner(38, 14)), None);
        assert_eq!(content_area(inner(38, 14)), inner(38, 14));
        for height in [15, 16] {
            assert_eq!(
                mascot_area(inner(38, height)),
                Some(Rect::new(1, 1 + height - 7, MASCOT_COLS, MASCOT_ROWS)),
                "{height}"
            );
            assert_eq!(
                content_area(inner(38, height)),
                Rect::new(1, 1, 38, height - 7),
                "{height}"
            );
        }
        assert_eq!(mascot_area(inner(16, 20)), None);
        assert_eq!(content_area(inner(16, 20)), inner(16, 20));
        assert_eq!(mascot_area(inner(17, 20)), Some(Rect::new(1, 14, 17, 7)));
        // The bubble: 17 + 1 + 17 columns.
        assert_eq!(bubble_area(inner(34, 20)), None);
        assert_eq!(bubble_area(inner(35, 20)), Some(Rect::new(19, 18, 17, 3)));
        assert_eq!(bubble_area(inner(38, 14)), None, "no cat, no bubble");
    }

    /// Criterion 1: every cell of the cat is the one the literal copy of the approved art
    /// says (symbol, fg, bg), in both poses, with and without truecolor, on the tree and on the
    /// settings; the transparent ones are the background of the column, inside its border.
    #[test]
    fn the_cat_is_drawn_pixel_by_pixel_inside_the_border() {
        let mut f = fixture();
        for screen in [Screen::Tree, Screen::Settings] {
            for (pose, pixels) in [(Pose::Rest, APPROVED_REST), (Pose::Blink, APPROVED_BLINK)] {
                for truecolor in [true, false] {
                    f.app.screen = screen;
                    f.app.set_mascot(showing(pose, false));
                    f.app.set_truecolor(truecolor);
                    let buffer = render(&f.app);
                    let inner = column_inner(&f.app, 100, 30);
                    let cat = mascot_area(inner).expect("room for the cat");
                    assert_eq!((cat.x, cat.y + cat.height), (inner.x, inner.bottom()));
                    let case = format!("{screen:?} {pose:?} truecolor={truecolor}");
                    for row in 0..MASCOT_ROWS {
                        for col in 0..MASCOT_COLS {
                            let top = pixels[usize::from(2 * row)].chars().nth(col.into());
                            let bottom = pixels[usize::from(2 * row + 1)].chars().nth(col.into());
                            let (top, bottom) = (
                                top.and_then(|c| approved_color(c, truecolor)),
                                bottom.and_then(|c| approved_color(c, truecolor)),
                            );
                            let (symbol, fg, bg) = match (top, bottom) {
                                (Some(t), Some(b)) => ("▀", t, b),
                                (Some(t), None) => ("▀", t, Color::Reset),
                                (None, Some(b)) => ("▄", b, Color::Reset),
                                (None, None) => (" ", Color::Reset, Color::Reset),
                            };
                            let cell = &buffer[(cat.x + col, cat.y + row)];
                            assert_eq!(
                                (cell.symbol(), cell.fg, cell.bg),
                                (symbol, fg, bg),
                                "{case}: row {row}, column {col}"
                            );
                        }
                    }
                    // The border stays: the left side on her rows and the bottom under her.
                    let list = layout(&f.app, Rect::new(0, 0, 100, 30)).list;
                    for y in cat.y..cat.bottom() {
                        assert_eq!(buffer[(list.x, y)].symbol(), "┃", "{case}: {y}");
                    }
                    assert_eq!(buffer[(list.x, list.bottom() - 1)].symbol(), "┗", "{case}");
                    for x in cat.x..cat.right() {
                        assert_eq!(buffer[(x, list.bottom() - 1)].symbol(), "━", "{case}");
                    }
                }
            }
        }
    }

    /// Criterion 3: below 15 rows (or 17 columns) inside the column, no cat, and the tree takes
    /// the whole inside; at 15 the cat on the last 7 and the tree on the 8 above.
    #[test]
    fn the_cat_goes_away_when_the_column_is_too_short_or_narrow() {
        let f = with_projects(30);
        let height_for = |rows: u16| {
            (5..80)
                .find(|h| column_inner(&f.app, 100, *h).height == rows)
                .expect("a height with that inside")
        };
        for rows in [14, 15, 16] {
            let height = height_for(rows);
            let buffer = render_at(&f.app, 100, height, None);
            let inner = column_inner(&f.app, 100, height);
            let tree_rows = if rows >= 15 { rows - 7 } else { rows };
            // The tree, more nodes than rows, down to its last row.
            for y in inner.y..inner.y + tree_rows {
                assert!(
                    !list_row(&buffer, y).trim().is_empty(),
                    "{rows}: row {y} empty\n{}",
                    text(&buffer)
                );
            }
            if rows >= 15 {
                let cat = Rect::new(inner.x, inner.y + tree_rows, MASCOT_COLS, 7);
                assert!(half_blocks(&buffer, cat) > 50, "{rows}: the cat");
                assert_eq!(half_blocks(&buffer, inner), half_blocks(&buffer, cat));
                // Nothing of the tree under the tree's last row, but the cat.
                for y in cat.y..cat.bottom() {
                    let rest: String = (cat.right()..inner.right())
                        .map(|x| buffer[(x, y)].symbol().to_string())
                        .collect();
                    assert_eq!(rest.trim(), "", "{rows}: row {y}");
                }
            } else {
                assert_eq!(half_blocks(&buffer, inner), 0, "{rows}: no cat");
            }
        }
        // A narrow window: the column of 18 columns outside, 16 inside.
        let buffer = render_at(&f.app, 36, 30, None);
        let inner = column_inner(&f.app, 36, 30);
        assert_eq!(inner.width, 16);
        assert_eq!(half_blocks(&buffer, inner), 0, "narrow: no cat");
        let buffer = render_at(&f.app, 38, 30, None);
        let inner = column_inner(&f.app, 38, 30);
        assert_eq!(inner.width, 17);
        assert!(half_blocks(&buffer, inner) > 50, "17 columns: the cat");
    }

    /// Criterion 4: with the bubble open, its three rows byte by byte on her right, one column
    /// away; on a column too narrow for it, the cat without the bubble.
    #[test]
    fn the_bubble_is_on_the_right_of_the_cat_when_it_fits() {
        let mut f = fixture();
        for pose in [Pose::Rest, Pose::Blink] {
            f.app.set_mascot(showing(pose, true));
            let buffer = render(&f.app);
            let inner = column_inner(&f.app, 100, 30);
            let cat = mascot_area(inner).unwrap();
            let row = |y: u16, from: u16, width: u16| -> String {
                (from..from + width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect()
            };
            let x = cat.right() + 1;
            assert_eq!(row(cat.y + 4, x, 17), "╭───────────────╮", "{pose:?}");
            assert_eq!(row(cat.y + 5, x, 17), "│Miau!          │", "{pose:?}");
            assert_eq!(row(cat.y + 6, x, 17), "╰───────────────╯", "{pose:?}");
            assert_eq!(row(cat.y + 4, cat.right(), 1), " ", "one column of space");
            for y in cat.y..cat.y + 4 {
                assert_eq!(row(y, x, 17).trim(), "", "{pose:?}: above the bubble");
            }
        }
        // 66 columns: the column of 33 outside, 31 inside: no room for the bubble.
        let buffer = render_at(&f.app, 66, 30, None);
        let inner = column_inner(&f.app, 66, 30);
        assert!(inner.width < 35);
        assert!(half_blocks(&buffer, inner) > 50, "the cat is there");
        assert!(!text(&buffer).contains("Miau!"), "{}", text(&buffer));
        assert!(!text(&buffer).contains('╭'));
        // Without the bubble open, no bubble.
        f.app.set_mascot(showing(Pose::Rest, false));
        assert!(!text(&render(&f.app)).contains("Miau!"));
    }

    /// Criterion 5: the cat changes nothing of the layout: the areas of the loop (the PTY of each
    /// agent, the header, the mouse) are the same whatever the pose and the bubble.
    #[test]
    fn the_cat_changes_neither_the_layout_nor_the_size_of_the_terminals() {
        let (mut f, _) = with_running(&["proj", "fora"]);
        let area = Rect::new(0, 0, 100, 30);
        let mut seen = Vec::new();
        for pose in [Pose::Rest, Pose::Blink] {
            for meowing in [false, true] {
                f.app.set_mascot(showing(pose, meowing));
                let areas = layout(&f.app, area);
                let terminals: Vec<Rect> = f
                    .app
                    .agents()
                    .iter()
                    .map(|agent| agent_area(&f.app, area, agent))
                    .collect();
                render_at(&f.app, 100, 30, None);
                seen.push((areas, terminals));
            }
        }
        assert!(seen.iter().all(|s| *s == seen[0]), "{seen:?}");
    }

    // ---- the colors of the stages and of the epic (NS-45) ----

    #[test]
    fn the_colors_of_the_stages_and_of_the_epic() {
        assert_eq!(stage_color(TicketStage::Open), Color::White);
        assert_eq!(STAGE_BLUE, Color::LightBlue);
        assert_ne!(STAGE_BLUE, FOCUS_COLOR, "not the cyan of the focus (NS-37)");
        assert_eq!(STAGE_ORANGE, Color::Indexed(208));
        assert_eq!(EPIC_COLOR, Color::Magenta);
        assert_eq!(
            TicketStage::ALL.map(stage_color),
            [
                Color::White,
                STAGE_BLUE,
                STAGE_BLUE,
                STAGE_ORANGE,
                STAGE_ORANGE,
                Color::Green,
                STAGE_GRAY
            ]
        );
        for stage in TicketStage::ALL {
            let mark = stage_mark(stage.key());
            assert_eq!(mark.style.fg, Some(stage_color(stage)), "{stage:?}");
        }
        assert_eq!(stage_mark("open").content, "[Aberto]");
        assert_eq!(stage_mark("done").content, "[✓ Concluído]");
        // An unknown key shows raw, with no color.
        let raw = stage_mark("archived");
        assert_eq!((raw.content.as_ref(), raw.style.fg), ("[archived]", None));
        // On the panel (no brackets), raw and with no color too, as before the NS-45.
        let raw = stage_span("archived", false);
        assert_eq!((raw.content.as_ref(), raw.style.fg), ("archived", None));
    }

    /// The mark of each stage on the tree and the "Etapa atual:" of the panel, in its color; a
    /// hidden ticket keeps the color, dimmed.
    #[test]
    fn each_stage_shows_in_its_color_on_the_tree_and_on_the_panel() {
        let (mut f, root) = with_tree();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let NodeId::Ticket(id) = tnode(&f.app, "NS-1") else {
            unreachable!()
        };
        f.app.select(NodeId::Ticket(id));
        // The six of before (NS-61: Rejeitado is gray in the whole label, in
        // `a_rejected_ticket_is_gray_with_its_mark_on_the_tree_and_on_the_panel`).
        for stage in TicketStage::ALL
            .into_iter()
            .filter(|s| *s != TicketStage::Rejected)
        {
            db.connection()
                .execute(
                    "UPDATE tickets SET stage = ?2 WHERE id = ?1",
                    rusqlite::params![id, stage.key()],
                )
                .unwrap();
            f.app.refresh_tree();
            let buffer = render_at(&f.app, 140, 30, None);
            let mark = stage_mark(stage.key()).content.to_string();
            let (y, x) = find(&buffer, &format!("NS-1  {mark}")).unwrap();
            for i in 0..mark.chars().count() as u16 {
                assert_eq!(
                    buffer[(x + 6 + i, y)].fg,
                    stage_color(stage),
                    "{stage:?} tree"
                );
            }
            assert_eq!(buffer[(x, y)].fg, Color::Reset, "the ID with no color");
            let label = if stage == TicketStage::Done {
                mark.clone()
            } else {
                stage.label().to_string()
            };
            let (y, x) = find(&buffer, &format!("Etapa atual: {label}")).unwrap();
            assert_eq!(
                buffer[(x + 13, y)].fg,
                stage_color(stage),
                "{stage:?} panel"
            );
            assert_eq!(buffer[(x, y)].fg, Color::Reset);
        }
        // The hidden NS-3, in Aberto: white and dimmed.
        f.app.tree.show_hidden = true;
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        let (y, x) = find(&buffer, "NS-3  [Aberto]").unwrap();
        assert_eq!(buffer[(x + 6, y)].fg, Color::White);
        assert!(buffer[(x + 6, y)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn the_id_and_the_title_of_the_epic_are_purple_on_the_tree() {
        let (mut f, db, _) = with_epic_tree();
        let buffer = render(&f.app);
        let (y, x) = find(&buffer, "EPIC-1  [épico]  [aberto]  Pla").unwrap();
        assert_eq!(buffer[(x, y)].fg, EPIC_COLOR, "the ID");
        assert_eq!(buffer[(x + 27, y)].fg, EPIC_COLOR, "the title");
        assert_eq!(
            buffer[(x + 17, y)].fg,
            Color::Reset,
            "[aberto] with no color"
        );
        assert_eq!(buffer[(x + 8, y)].fg, Color::Reset, "[épico] with no color");
        assert!(buffer[(x + 8, y)].modifier.contains(Modifier::BOLD));
        // Hidden: still purple, dimmed.
        let epic = db
            .list_epics(f.app.project_rows()[0].project.id, true)
            .unwrap()[0]
            .clone();
        db.hide_epic(epic.id).unwrap();
        f.app.tree.show_hidden = true;
        f.app.refresh_tree();
        let buffer = render(&f.app);
        let (y, x) = find(&buffer, "EPIC-1  [épico]").unwrap();
        assert_eq!(buffer[(x, y)].fg, EPIC_COLOR);
        assert!(buffer[(x, y)].modifier.contains(Modifier::DIM));
    }

    #[test]
    fn the_history_shows_the_source_and_the_empty_one_the_current_stage() {
        let (mut f, _) = with_tree();
        f.app.select(tnode(&f.app, "NS-1"));
        f.app.refresh_tree();
        let detail = f.app.ticket_detail.as_mut().unwrap();
        detail.changes = vec![
            sourced("open", "po", "agent_opening", "2026-11-21T10:19:00Z"),
            sourced("po", "open", "operator", "2026-11-21T10:20:00Z"),
            sourced("open", "review", "cron", "2026-11-21T10:21:00Z"),
        ];
        let buffer = render_at(&f.app, 140, 30, None);
        assert_has(
            &buffer,
            "  2026-11-21 10:19 UTC  Aberto → PO  (abertura de agente)",
        );
        assert_has(&buffer, "  2026-11-21 10:20 UTC  PO → Aberto  (operador)");
        assert_has(&buffer, "  2026-11-21 10:21 UTC  Aberto → Review  (cron)");
        // Empty: the stage it started in is the current one, for a new and for an old ticket.
        for (stage, label) in [("open", "Aberto"), ("po", "PO")] {
            let detail = f.app.ticket_detail.as_mut().unwrap();
            detail.changes.clear();
            detail.ticket.stage = stage.into();
            assert_has(
                &render_at(&f.app, 140, 30, None),
                &format!("Nenhuma mudança: o ticket começou em {label}."),
            );
        }
    }

    #[test]
    fn the_dialog_of_the_stage_marks_aberto_as_the_current_one() {
        let mut f = fixture();
        f.app.dialog = Some(Dialog::ChooseTicketStage {
            ticket: staged(1, "NS-12", "Título", "open", false),
            index: 0,
        });
        let buffer = render(&f.app);
        assert_has(&buffer, "> Aberto (atual)");
        assert_has(&buffer, "  Concluído");
    }

    // ---- the done epic and the filter of the done ones (NS-54, D7) ----

    #[test]
    /// The mark right after `[épico]` (decision of the user in the validation of the NS-54): at
    /// the end of the row, after the title, the cut at 40 columns took it away.
    fn the_label_of_a_done_epic_has_its_mark_right_after_the_mark_of_the_epic() {
        let mut epic = Epic {
            id: 1,
            project_id: 1,
            key: "EPIC-1".into(),
            is_provisional: false,
            title: "Plano".into(),
            created_at: String::new(),
            hidden_at: None,
            done_at: Some("2026-11-22T10:00:00Z".into()),
        };
        let (spans, style) = epic_spans(&epic);
        assert_eq!(spans_text(&spans), "EPIC-1  [épico]  [concluído]  Plano");
        assert_eq!(spans[3].content, "  [concluído]");
        assert_eq!(spans[3].style.fg, None, "no new color");
        assert!(!style.add_modifier.contains(Modifier::DIM));
        assert_eq!(spans[0].style.fg, Some(EPIC_COLOR), "the purple stays");
        assert_eq!(spans[5].style.fg, Some(EPIC_COLOR), "on the title too");
        epic.is_provisional = true;
        epic.hidden_at = Some("x".into());
        assert_eq!(
            spans_text(&epic_spans(&epic).0),
            "EPIC-1  [épico]  [concluído]  Plano  [provisório]  [oculto]"
        );
        // At depth 2 the label has 30 columns (40, less the border, `> `, the indent and the
        // arrow): with an ID of 8 characters (a provisional one) the mark still fits whole.
        epic.key = "0a1b2c3d".into();
        epic.title = "Painel do nick em Rust".into();
        let text = spans_text(&epic_spans(&epic).0);
        assert_eq!(
            text.chars().take(30).collect::<String>(),
            "0a1b2c3d  [épico]  [concluído]",
            "{text}"
        );
        // An open epic: `[aberto]` in the same place, whole in the 30 columns too.
        epic.done_at = None;
        epic.is_provisional = false;
        epic.hidden_at = None;
        assert_eq!(
            spans_text(&epic_spans(&epic).0),
            "0a1b2c3d  [épico]  [aberto]  Painel do nick em Rust"
        );
    }

    #[test]
    fn the_panel_of_an_epic_says_its_state() {
        let (mut f, _, _) = with_epic_tree();
        let buffer = render(&f.app);
        assert_has(&buffer, "Estado: aberto");
        assert!(find(&buffer, "[concluído]").is_none());
        let detail = f.app.epic_detail.as_mut().unwrap();
        detail.epic.done_at = Some("2026-11-22T10:05:00Z".into());
        let buffer = render(&f.app);
        assert_has(&buffer, "Épico EPIC-1 — Plano longo  [concluído]");
        assert_has(&buffer, "Estado: concluído em 2026-11-22 10:05 UTC");
        let (tickets, _) = find(&buffer, "Tickets: 1").unwrap();
        let (state, _) = find(&buffer, "Estado: concluído").unwrap();
        assert_eq!(tickets + 1, state, "right after the tickets");
        // A date out of the format is shown as it is, printable (the mold of `short_date`).
        f.app.epic_detail.as_mut().unwrap().epic.done_at = Some("ontem\x07".into());
        assert_has(&render(&f.app), "Estado: concluído em ontem?");
    }

    #[test]
    fn the_help_of_the_epic_has_c_and_the_filters_have_d() {
        let (mut f, _, _) = with_epic_tree();
        assert_eq!(
            help_text(&f.app),
            "↑/↓ mover · →/Enter abrir · ← fechar/subir · a novo ticket · e editar título · \
             o ocultar/restaurar · c concluir/reabrir · g abrir agente · i editar ID · \
             f mostrar ocultos · d esconder concluídos · v agentes ativos · q sair"
        );
        f.app.tree.hide_done = true;
        assert!(
            help_text(&f.app)
                .ends_with("f mostrar ocultos · d mostrar concluídos · v agentes ativos · q sair"),
            "{}",
            help_text(&f.app)
        );
        f.app.tree.show_hidden = true;
        f.app.tree.only_active = true;
        assert!(
            help_text(&f.app)
                .ends_with("f esconder ocultos · d mostrar concluídos · v mostrar tudo · q sair")
        );
    }

    /// The 11 places of the tree (NS-57: 8 before) with the 8 combinations of the three filters,
    /// and the settings.
    #[test]
    fn help_states_has_every_combination_of_the_three_filters() {
        assert_eq!(HELP_STATES.len(), 89);
        let tree = [
            Place::Module,
            Place::Project,
            Place::Epic,
            Place::Ticket,
            Place::LiveAgent,
            Place::LiveAgentInJail,
            Place::LiveAgentReleased,
            Place::StoppedAgent,
            Place::StoppedAgentInJail,
            Place::ArchivedAgent,
            Place::ElsewhereAgent,
        ];
        for place in tree {
            for hidden in [false, true] {
                for done in [false, true] {
                    for active in [false, true] {
                        assert!(
                            HELP_STATES.contains(&(place, hidden, done, active)),
                            "{place:?} {hidden} {done} {active}"
                        );
                    }
                }
            }
        }
        assert!(HELP_STATES.contains(&(Place::Settings, false, false, false)));
        // 89 states, the 89 combinations above among them: no state twice.
    }

    // --- NS-55: the marquee of the selected node, and the `…` of the others. ---

    /// A tree with long rows of every kind: a project with a long name (depth 1), an epic with
    /// a long title (depth 2), the ticket NS-10 with accents (precomposed and combining), a cat
    /// and a wide character, the ticket NS-11 in Tech Leader with an ASCII-ish title and an agent
    /// not live (depth 3), and the short NS-12. Every branch open, the module selected.
    fn with_long() -> Fixture {
        let mut f = fixture();
        let root = std::fs::canonicalize(f._tmp.path()).unwrap();
        let dir = root.join("alfa");
        std::fs::create_dir_all(dir.join(".nick")).unwrap();
        std::fs::write(dir.join(".nick/nick.conf"), "").unwrap();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let p = db
            .insert_project(
                "auto_scrum",
                "alfa-projeto-com-um-nome-comprido-demais",
                dir.to_str().unwrap(),
            )
            .unwrap();
        db.insert_epic(p.id, "E-1", false, "Épico de título muito comprido")
            .unwrap();
        db.insert_ticket(
            p.id,
            "NS-10",
            false,
            "Título com ação, a\u{0303} e 🐈 漢 que não cabe",
            None,
        )
        .unwrap();
        let t11 = db
            .insert_ticket(
                p.id,
                "NS-11",
                false,
                "Outro titulo bem longo para ver as reticencias",
                None,
            )
            .unwrap();
        db.connection()
            .execute(
                "UPDATE tickets SET stage = 'tech_leader' WHERE id = ?1",
                [t11.id],
            )
            .unwrap();
        db.insert_ticket(p.id, "NS-12", false, "Curto", None)
            .unwrap();
        let agent = db
            .insert_agent(
                Owner::Ticket(t11.id),
                "tech_leader",
                "s-1",
                "alfa · Tech Leader · NS-11",
                "NS-11",
            )
            .unwrap();
        db.stop_agent(agent.id).unwrap();
        db.connection()
            .execute(
                "UPDATE agents SET opened_at = '2026-11-20T00:00:00Z' WHERE id = ?1",
                [agent.id],
            )
            .unwrap();
        f.app.refresh_tree();
        f.app.tree.open(MODULE);
        f.app.tree.open(NodeId::Project(p.id));
        f.app.tree.open(NodeId::Ticket(t11.id));
        f.app.refresh_tree();
        f.app.select(MODULE);
        f.app
            .set_marquee(crate::marquee::Marquee::fast(std::time::Instant::now()));
        f
    }

    /// The node of the row whose text starts with `start`.
    fn node_starting(app: &App, start: &str) -> NodeId {
        app.visible()
            .iter()
            .find(|row| marquee_text(app, row).starts_with(start))
            .unwrap_or_else(|| panic!("no row starts with {start:?}"))
            .id
    }

    /// The text of a row, after its prefix (what scrolls, D1).
    fn marquee_text(app: &App, row: &Visible) -> String {
        tree_row(app, row)
            .1
            .iter()
            .map(|s| s.content.to_string())
            .collect()
    }

    /// The screen row of `node` on the tree (the list starts right below the border).
    fn screen_row(app: &App, node: NodeId) -> u16 {
        let index = app.visible().iter().position(|row| row.id == node).unwrap();
        1 + u16::try_from(index).unwrap()
    }

    /// The cells of the row `y` from the column 1 to `last`, as the buffer has them: a wide
    /// grapheme then the blank cell it covers.
    fn row_cells(buffer: &Buffer, y: u16, last: u16) -> String {
        (1..=last)
            .map(|x| buffer[(x, y)].symbol().to_string())
            .collect()
    }

    /// `spans` as the buffer draws them in `cells` columns: each grapheme of width > 0 and a
    /// blank for each other column it takes, then blanks up to `cells`.
    fn as_cells(spans: &[Span<'static>], cells: usize) -> String {
        let mut out = String::new();
        let mut used = 0;
        for span in spans {
            for g in span.styled_graphemes(Style::default()) {
                let w = Span::raw(g.symbol).width();
                if w == 0 {
                    continue;
                }
                out.push_str(g.symbol);
                out.push_str(&" ".repeat(w - 1));
                used += w;
            }
        }
        out.push_str(&" ".repeat(cells.saturating_sub(used)));
        out
    }

    /// The row of `node` as `draw_tree` must draw it on a 40 column list (38 inside): the
    /// highlight symbol (or its blanks), the prefix, and its text `cut` to the room left.
    fn expected_row(
        app: &App,
        node: NodeId,
        cut: impl Fn(&[Span<'static>], usize) -> Vec<Span<'static>>,
    ) -> String {
        let row = app.visible().iter().find(|row| row.id == node).unwrap();
        let (prefix, text, _) = tree_row(app, row);
        let symbol = if node == app.selected() {
            HIGHLIGHT_SYMBOL.to_string()
        } else {
            " ".repeat(HIGHLIGHT_SYMBOL.len())
        };
        let room = 38 - HIGHLIGHT_SYMBOL.len() - prefix.chars().count();
        format!("{symbol}{prefix}{}", as_cells(&cut(&text, room), room))
    }

    /// Moves the marquee of `f` to the column `k` of the selected node (fast timing, from `t0`
    /// selected on the screen `area`) and draws it.
    fn draw_at_column(f: &mut Fixture, t0: std::time::Instant, k: usize, area: Rect) -> Buffer {
        use crate::marquee::FAST;
        let overflow = marquee_overflow(&f.app, area);
        let at = if k == 0 {
            t0
        } else {
            t0 + FAST.hold_start + FAST.step * u32::try_from(k).unwrap()
        };
        f.app.tick_marquee(at, overflow);
        render_at(&f.app, area.width, area.height, None)
    }

    #[test]
    fn a_long_row_not_selected_ends_in_an_ellipsis_before_the_border() {
        let f = with_long();
        let buffer = render(&f.app);
        let module = screen_row(&f.app, MODULE);
        let border = buffer[(LIST_WIDTH - 1, module)].symbol().to_string();
        for start in [
            "alfa-projeto",
            "E-1",
            "NS-10",
            "NS-11",
            "Tech Leader  parado",
        ] {
            let node = node_starting(&f.app, start);
            let y = screen_row(&f.app, node);
            assert_eq!(
                row_cells(&buffer, y, LIST_WIDTH - 2),
                expected_row(&f.app, node, marquee::ellipsize),
                "{start}:\n{}",
                text(&buffer)
            );
            assert_eq!(
                buffer[(LIST_WIDTH - 1, y)].symbol(),
                border,
                "{start}: the border in its place"
            );
        }
        // Byte by byte: the last column inside the border is the `…`, after 29 columns.
        let y = screen_row(&f.app, node_starting(&f.app, "NS-11"));
        assert_eq!(
            list_row(&buffer, y),
            "      ▾ NS-11  [Tech Leader]  Outro t…"
        );
        assert_eq!(buffer[(LIST_WIDTH - 2, y)].symbol(), "…");
        // Each part keeps its style: the color of the stage; the `…` has the style of the row.
        let (_, x) = find(&buffer, "[Tech Leader]").unwrap();
        assert_eq!(buffer[(x + 1, y)].fg, STAGE_BLUE);
        assert_eq!(buffer[(LIST_WIDTH - 2, y)].fg, Color::Reset);
        let agent = screen_row(&f.app, node_starting(&f.app, "Tech Leader  parado"));
        assert_eq!(
            list_row(&buffer, agent),
            "          Tech Leader  parado  2026-1…"
        );
        let dim = |x: u16, y: u16| buffer[(x, y)].modifier.contains(Modifier::DIM);
        assert!(
            dim(LIST_WIDTH - 2, agent),
            "the `…` of a row not live: dimmed"
        );
        assert!(dim(12, agent));
        let project = screen_row(&f.app, node_starting(&f.app, "alfa-projeto"));
        assert_eq!(
            list_row(&buffer, project),
            "    ▾ alfa-projeto-com-um-nome-compri…"
        );
    }

    #[test]
    fn the_selected_long_row_scrolls_without_an_ellipsis_and_its_prefix_stays() {
        use crate::marquee::FAST;
        let mut f = with_long();
        let area = Rect::new(0, 0, 100, 30);
        let node = node_starting(&f.app, "NS-11");
        f.app.select(node);
        let y = screen_row(&f.app, node);
        let text = marquee_text(&f.app, &f.app.visible()[usize::from(y - 1)]);
        assert_eq!(
            text,
            "NS-11  [Tech Leader]  Outro titulo bem longo para ver as reticencias"
        );
        // 68 columns, 30 of room after `> ` and `    ▾ `.
        assert_eq!(marquee_overflow(&f.app, area), 38);
        let t0 = std::time::Instant::now();
        let window = |k: usize| -> String { text.chars().skip(k).take(30).collect() };
        // At the start: still, with the start of the text and no `…`.
        let buffer = draw_at_column(&mut f, t0, 0, area);
        assert_eq!(
            row_cells(&buffer, y, 38),
            format!("> {}{}", "    ▾ ", window(0))
        );
        // Still while the hold lasts.
        f.app.tick_marquee(
            t0 + FAST.hold_start - std::time::Duration::from_millis(1),
            38,
        );
        let buffer = render_at(&f.app, 100, 30, None);
        assert_eq!(
            row_cells(&buffer, y, 38),
            format!("> {}{}", "    ▾ ", window(0))
        );
        for k in [1, 5, 20, 37, 38] {
            let buffer = draw_at_column(&mut f, t0, k, area);
            assert_eq!(
                row_cells(&buffer, y, 38),
                format!("> {}{}", "    ▾ ", window(k)),
                "column {k}"
            );
            assert!(!row_cells(&buffer, y, 38).contains('…'));
        }
        // At the end: the end of the text against the border.
        assert!(window(38).ends_with("reticencias"));
        // Back to the start at once after the hold at the end.
        let cycle = FAST.hold_start + FAST.step * 38 + FAST.hold_end;
        f.app.tick_marquee(t0 + cycle, 38);
        let buffer = render_at(&f.app, 100, 30, None);
        assert_eq!(
            row_cells(&buffer, y, 38),
            format!("> {}{}", "    ▾ ", window(0))
        );
    }

    #[test]
    fn a_short_row_is_drawn_as_before() {
        let mut f = with_long();
        let node = node_starting(&f.app, "NS-12");
        let y = screen_row(&f.app, node);
        let buffer = render(&f.app);
        assert_eq!(list_row(&buffer, y), "        NS-12  [Aberto]  Curto");
        f.app.select(node);
        let area = Rect::new(0, 0, 100, 30);
        assert_eq!(marquee_overflow(&f.app, area), 0);
        let t0 = std::time::Instant::now();
        for k in [0, 3, 50] {
            let buffer = draw_at_column(&mut f, t0, k, area);
            assert_eq!(list_row(&buffer, y), ">       NS-12  [Aberto]  Curto");
        }
        assert_eq!(
            f.app.next_wake(),
            f.app.mascot_wake(),
            "nothing to wake for"
        );
        // The module row, short too, as before.
        assert_eq!(list_row(&render(&f.app), 1), "  ▾ auto_scrum");
    }

    #[test]
    fn accents_and_wide_graphemes_never_move_the_border() {
        let mut f = with_long();
        let area = Rect::new(0, 0, 100, 30);
        let node = node_starting(&f.app, "NS-10");
        let y = screen_row(&f.app, node);
        let reference = render(&f.app);
        let edge = |b: &Buffer| -> Vec<String> {
            (LIST_WIDTH - 1..=LIST_WIDTH)
                .map(|x| b[(x, y)].symbol().to_string())
                .collect()
        };
        // Not selected: the `…`, and the border where it was.
        assert_eq!(
            row_cells(&reference, y, 38),
            expected_row(&f.app, node, marquee::ellipsize)
        );
        let border = edge(&reference);
        f.app.select(node);
        let overflow = marquee_overflow(&f.app, area);
        assert!(overflow > 0);
        let t0 = std::time::Instant::now();
        for k in 0..=overflow {
            let buffer = draw_at_column(&mut f, t0, k, area);
            assert_eq!(edge(&buffer), border, "column {k}");
            assert_eq!(
                row_cells(&buffer, y, 38),
                expected_row(&f.app, node, |text, room| marquee::window(text, k, room)),
                "column {k}"
            );
        }
    }

    #[test]
    fn marquee_overflow_is_what_the_drawing_cuts_at_every_width_with_and_without_the_cat() {
        use crate::marquee::FAST;
        for (width, height) in [
            (50, 30),
            (60, 30),
            (78, 30),
            (100, 30),
            (140, 30),
            (100, 16),
            (50, 16),
        ] {
            let mut f = with_long();
            let node = node_starting(&f.app, "NS-11");
            f.app.select(node);
            let y = screen_row(&f.app, node);
            let area = Rect::new(0, 0, width, height);
            let list = layout(&f.app, area).list;
            let inner = Block::bordered().inner(list);
            assert_eq!(
                mascot_area(inner).is_some(),
                height == 30,
                "{width}x{height}: the cat"
            );
            let overflow = marquee_overflow(&f.app, area);
            assert!(overflow > 0, "{width}x{height}");
            let last = inner.right() - 1;
            let t0 = std::time::Instant::now();
            let draw = |f: &mut Fixture, k: usize| {
                let at = t0 + FAST.hold_start + FAST.step * u32::try_from(k).unwrap();
                f.app.tick_marquee(at, overflow);
                render_at(&f.app, width, height, None)
            };
            f.app.tick_marquee(t0, overflow);
            // At the end (offset = overflow) the last letter is against the border; one
            // column before, the one before it.
            let buffer = draw(&mut f, overflow);
            assert_eq!(buffer[(last, y)].symbol(), "s", "{width}x{height}");
            let buffer = draw(&mut f, overflow - 1);
            assert_eq!(buffer[(last, y)].symbol(), "a", "{width}x{height}");
            // Not selected: the `…` in the last column inside.
            f.app.select(MODULE);
            let buffer = render_at(&f.app, width, height, None);
            assert_eq!(buffer[(last, y)].symbol(), "…", "{width}x{height}");
        }
    }

    #[test]
    fn marquee_overflow_is_0_on_the_settings_and_without_a_selected_row() {
        let mut f = with_long();
        let area = Rect::new(0, 0, 100, 30);
        f.app.select(node_starting(&f.app, "NS-11"));
        assert_eq!(marquee_overflow(&f.app, area), 38);
        f.app.screen = Screen::Settings;
        assert_eq!(marquee_overflow(&f.app, area), 0);
        f.app.screen = Screen::Tree;
        f.app.select(MODULE);
        assert_eq!(marquee_overflow(&f.app, area), 0, "a short row");
    }

    #[test]
    fn the_marquee_resizes_no_terminal() {
        let (mut f, root) = with_running(&["proj", "fora"]);
        let db = Db::open(&root.join("nick.db")).unwrap();
        db.connection()
            .execute(
                "UPDATE tickets SET title = 'Um título bem comprido que não cabe na árvore'",
                [],
            )
            .unwrap();
        f.app.refresh_tree();
        let ticket = node_starting(&f.app, "NS-1");
        f.app.select(ticket);
        f.app.focus = Focus::List;
        f.app
            .set_marquee(crate::marquee::Marquee::fast(std::time::Instant::now()));
        for (width, height) in [(100, 30), (50, 30), (140, 40)] {
            let area = Rect::new(0, 0, width, height);
            let before = (
                layout(&f.app, area),
                f.app
                    .agents()
                    .iter()
                    .map(|a| agent_area(&f.app, area, a))
                    .collect::<Vec<_>>(),
            );
            let overflow = marquee_overflow(&f.app, area);
            assert!(overflow > 0, "{width}x{height}");
            let t0 = std::time::Instant::now();
            for k in 0..=overflow + 3 {
                draw_at_column(&mut f, t0, k, area);
                let now = (
                    layout(&f.app, area),
                    f.app
                        .agents()
                        .iter()
                        .map(|a| agent_area(&f.app, area, a))
                        .collect::<Vec<_>>(),
                );
                assert_eq!(now, before, "{width}x{height}, column {k}");
            }
        }
    }

    // ---- NS-61: the stage Rejeitado ----

    /// A change to Rejeitado with its reason.
    fn rejection(from: &str, reason: &str, at: &str) -> StageChangeRow {
        StageChangeRow {
            reason: Some(reason.into()),
            ..change(from, "rejected", at)
        }
    }

    #[test]
    fn the_mark_and_the_color_of_rejeitado() {
        assert_eq!(STAGE_GRAY, Color::DarkGray);
        assert_eq!(stage_color(TicketStage::Rejected), STAGE_GRAY);
        for brackets in [true, false] {
            let span = stage_span("rejected", brackets);
            assert_eq!(span.content, "[✗ Rejeitado]", "always with the mark");
            assert_eq!(span.style.fg, Some(STAGE_GRAY));
        }
        assert_eq!(stage_mark("rejected").content, "[✗ Rejeitado]");
        // The six of before keep their marks.
        assert_eq!(stage_span("done", false).content, "[✓ Concluído]");
        assert_eq!(stage_span("review", false).content, "Review");
        assert_eq!(stage_mark("review").content, "[Review]");
    }

    #[test]
    fn the_label_of_a_rejected_ticket_is_gray_and_dimmed_when_hidden() {
        let (spans, style) = ticket_spans(&staged(4, "NS-4", "Quatro", "rejected", false));
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "NS-4  [✗ Rejeitado]  Quatro");
        assert_eq!(style.fg, Some(STAGE_GRAY), "the whole label");
        assert!(!style.add_modifier.contains(Modifier::DIM));
        let mut hidden = staged(4, "NS-4", "Quatro", "rejected", true);
        hidden.is_provisional = true;
        let (spans, style) = ticket_spans(&hidden);
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        assert_eq!(text, "NS-4  [✗ Rejeitado]  Quatro  [provisório]  [oculto]");
        assert_eq!(style.fg, Some(STAGE_GRAY));
        assert!(
            style.add_modifier.contains(Modifier::DIM),
            "gray and dimmed"
        );
        // Another stage: the label with no color, as before.
        let (_, style) = ticket_spans(&staged(4, "NS-4", "Quatro", "review", false));
        assert_eq!(style.fg, None);
    }

    #[test]
    fn a_rejected_ticket_is_gray_with_its_mark_on_the_tree_and_on_the_panel() {
        let (mut f, root) = with_tree();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let NodeId::Ticket(id) = tnode(&f.app, "NS-1") else {
            unreachable!()
        };
        db.connection()
            .execute("UPDATE tickets SET stage = 'rejected' WHERE id = ?1", [id])
            .unwrap();
        // NS-1 not selected: its row, whole, in gray.
        f.app.select(tnode(&f.app, "NS-2"));
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        let row = "NS-1  [✗ Rejeitado]  Um";
        let (y, x) = find(&buffer, row).unwrap();
        for i in 0..row.chars().count() as u16 {
            assert_eq!(buffer[(x + i, y)].fg, STAGE_GRAY, "column {i} of the row");
        }
        // Selected: still gray; the panel says the mark, gray.
        f.app.select(NodeId::Ticket(id));
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 140, 30, None);
        let (y, x) = find(&buffer, row).unwrap();
        assert_eq!(buffer[(x, y)].fg, STAGE_GRAY, "the ID");
        let (y, x) = find(&buffer, "Etapa atual: [✗ Rejeitado]").unwrap();
        assert_eq!(buffer[(x + 13, y)].fg, STAGE_GRAY);
        assert_eq!(buffer[(x, y)].fg, Color::Reset);
    }

    #[test]
    fn the_gray_of_a_rejected_ticket_holds_on_the_ellipsis_and_on_the_scrolling_row() {
        let (mut f, root) = with_tree();
        let db = Db::open(&root.join("nick.db")).unwrap();
        let NodeId::Ticket(id) = tnode(&f.app, "NS-1") else {
            unreachable!()
        };
        db.connection()
            .execute(
                "UPDATE tickets SET stage = 'rejected', title = ?2 WHERE id = ?1",
                rusqlite::params![id, "Um título bem longo que não cabe na coluna da árvore"],
            )
            .unwrap();
        // Not selected, at 40 columns: cut with the ellipsis, the ellipsis gray too.
        f.app.select(tnode(&f.app, "NS-2"));
        f.app.refresh_tree();
        let buffer = render_at(&f.app, 60, 30, None);
        let (y, x) = find(&buffer, "NS-1  [✗ Rejeitado]").unwrap();
        let end = (x..buffer.area.width)
            .find(|c| buffer[(*c, y)].symbol() == "…")
            .expect("the ellipsis");
        for c in x..=end {
            assert_eq!(buffer[(c, y)].fg, STAGE_GRAY, "column {c}");
        }
        // Selected, scrolled: the window keeps the gray.
        f.app.select(NodeId::Ticket(id));
        f.app.refresh_tree();
        let area = Rect::new(0, 0, 60, 30);
        let overflow = marquee_overflow(&f.app, area);
        assert!(overflow > 0, "it scrolls");
        let t0 = std::time::Instant::now();
        f.app.set_marquee(crate::marquee::Marquee::fast(t0));
        f.app.tick_marquee(t0, overflow);
        let buffer = draw_at_column(&mut f, t0, overflow, area);
        let (y, x) = find(&buffer, "da árvore").unwrap_or_else(|| panic!("{}", text(&buffer)));
        for c in x..x + 9 {
            assert_eq!(buffer[(c, y)].fg, STAGE_GRAY, "column {c}");
        }
    }

    #[test]
    fn the_history_shows_the_reason_of_a_rejection_on_the_line_below() {
        let (mut f, _) = with_tree();
        f.app.select(tnode(&f.app, "NS-1"));
        f.app.refresh_tree();
        let detail = f.app.ticket_detail.as_mut().unwrap();
        detail.changes = vec![
            rejection("tech_leader", "fora do escopo\u{7}", "2026-11-21T10:19:00Z"),
            change("rejected", "development", "2026-11-21T10:20:00Z"),
        ];
        let buffer = render_at(&f.app, 140, 30, None);
        let (first, x) = find(
            &buffer,
            "  2026-11-21 10:19 UTC  Tech Leader → Rejeitado  (operador)",
        )
        .unwrap();
        let (motivo, mx) = find(&buffer, "    Motivo: fora do escopo?").unwrap();
        assert_eq!((motivo, mx), (first + 1, x), "right below, indented");
        let (second, _) = find(
            &buffer,
            "  2026-11-21 10:20 UTC  Rejeitado → Dev  (operador)",
        )
        .unwrap();
        assert_eq!(second, first + 2);
        assert_eq!(
            text(&buffer).matches("Motivo:").count(),
            1,
            "only the rejection has a reason"
        );
    }

    #[test]
    fn a_long_history_with_reasons_counts_rows_and_fits_the_area() {
        let (mut f, _) = with_tree();
        f.app.select(tnode(&f.app, "NS-1"));
        f.app.refresh_tree();
        f.app.ticket_detail.as_mut().unwrap().changes = (0..40)
            .map(|i| {
                let at = format!("2026-11-19T10:{i:02}:00Z");
                if i % 2 == 0 {
                    rejection("po", &format!("motivo {i}"), &at)
                } else {
                    change("rejected", "po", &at)
                }
            })
            .collect();
        for height in [30, 20, 14] {
            let buffer = render_at(&f.app, 140, height, None);
            assert_has(&buffer, "2026-11-19 10:39 UTC  Rejeitado → PO");
            let shown = text(&buffer).matches("2026-11-19 10:").count();
            assert_has(&buffer, &format!("(+{} mudanças mais antigas)", 40 - shown));
            // Every change shown with its reason: no line of a reason without its change.
            let reasons = text(&buffer).matches("Motivo: motivo").count();
            let rejections = text(&buffer).matches("→ Rejeitado").count();
            assert_eq!(reasons, rejections, "height {height}");
            // Nothing past the bottom of the panel (above its border).
            let (bottom, _) = find(&buffer, "10:39").unwrap();
            let border = (bottom + 1..height)
                .find(|y| buffer[(41, *y)].symbol() == "─" || buffer[(41, *y)].symbol() == "━")
                .expect("the bottom border below the newest");
            assert!(border > bottom, "height {height}");
        }
    }

    #[test]
    fn the_dialogs_of_the_rejection() {
        let mut f = fixture();
        let t = staged(1, "NS-12", "Título do ticket", "development", false);
        f.app.dialog = Some(Dialog::ChooseTicketStage {
            ticket: t.clone(),
            index: 6,
        });
        let buffer = render(&f.app);
        assert_has(&buffer, "> Rejeitado");
        assert_has(&buffer, "  Concluído");
        f.app.dialog = Some(Dialog::ChooseTicketStage {
            ticket: staged(1, "NS-12", "Título do ticket", "rejected", false),
            index: 6,
        });
        assert_has(&render(&f.app), "> Rejeitado (atual)");
        f.app.dialog = Some(Dialog::RejectReason {
            ticket: t.clone(),
            input: "fora".into(),
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Rejeitar ticket ");
        assert_has(&buffer, "Ticket NS-12 — Título do ticket");
        assert_has(&buffer, "Motivo da rejeição (até 120 caracteres):");
        assert_has(&buffer, "fora");
        assert_eq!(last_line(&buffer).trim(), "Enter confirmar · Esc cancelar");
        f.app.dialog = Some(Dialog::ConfirmStageChange {
            ticket: t.clone(),
            to: TicketStage::Rejected,
            reason: Some("fora do escopo".into()),
            live: Vec::new(),
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Rejeitar ticket ");
        assert!(
            flat(&buffer)
                .contains("Rejeitar o ticket NS-12 (de Dev)? Motivo: fora do escopo (s/N)"),
            "{}",
            flat(&buffer)
        );
        assert!(!flat(&buffer).contains("Aviso:"));
        assert_eq!(
            last_line(&buffer).trim(),
            "s rejeitar · qualquer outra tecla cancela"
        );
        f.app.dialog = Some(Dialog::ConfirmStageChange {
            ticket: t.clone(),
            to: TicketStage::Rejected,
            reason: Some("fora do escopo".into()),
            live: vec![(7, "proj · Dev · NS-12".into())],
        });
        assert!(
            flat(&render(&f.app)).contains("Aviso: o agente proj · Dev · NS-12 será encerrado."),
            "{}",
            flat(&render(&f.app))
        );
        f.app.dialog = Some(Dialog::ConfirmStageChange {
            ticket: t,
            to: TicketStage::Rejected,
            reason: Some("fora do escopo".into()),
            live: vec![
                (7, "proj · Dev · NS-12".into()),
                (8, "proj · PO · NS-12".into()),
            ],
        });
        assert!(
            flat(&render(&f.app)).contains(
                "Aviso: 2 agentes vivos deste ticket serão encerrados: proj · Dev · NS-12, \
                 proj · PO · NS-12."
            ),
            "{}",
            flat(&render(&f.app))
        );
    }

    #[test]
    fn the_dialog_of_the_stages_fits_its_seven_rows_in_a_short_terminal() {
        let mut f = fixture();
        let t = staged(1, "NS-12", "Título do ticket", "development", false);
        f.app.dialog = Some(Dialog::ChooseTicketStage {
            ticket: t,
            index: 6,
        });
        // The lines of the dialog: the ticket, a blank, the question and the seven stages.
        for height in [24, 20, 16] {
            let buffer = render_at(&f.app, 80, height, None);
            for stage in TicketStage::ALL {
                assert!(
                    find(&buffer, stage.label()).is_some(),
                    "{} at {height} rows",
                    stage.label()
                );
            }
        }
    }
}
