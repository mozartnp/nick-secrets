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
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::agents::AgentState;
use crate::app::{App, Button, Dialog, Focus, RunningAgent, Screen, TicketField};
use crate::docs;
use crate::launch::{Precheck, Rendered};
use crate::modules::ModuleKind;
use crate::paths::printable;
use crate::projects;
use crate::stages::{self, Input, Rule};
use crate::tickets::{self, TicketStage};

/// The widest the list gets beside an agent (NS-34); never more than half the window.
pub const LIST_WIDTH: u16 = 40;

/// Where each part of the screen goes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Areas {
    /// Everything above the status: where the dialogs are centered.
    pub body: Rect,
    /// The list of the current screen (the whole body without an agent running).
    pub list: Rect,
    /// The panel of the shown agent: its header, then its terminal (empty without agents).
    pub panel: Rect,
    pub header: Rect,
    pub terminal: Rect,
    pub status: Rect,
    pub help: Rect,
}

/// The layout of the screen `area`: the status and the help at the bottom, full width, taking
/// the rows they need, word wrapped (up to a third of the screen each), so a long one is not
/// cut; above them, without an agent running, the list over the whole width (the screens of
/// before the NS-34); with one, the list on the left (`LIST_WIDTH`, at most half the width)
/// and the panel of the shown agent on the right.
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
/// terminal, no status message). `terminal_in_panel` gives its terminal once its name and
/// whether it runs in the jail are known: the size of its PTY at the spawn.
pub fn new_agent_panel(app: &App, area: Rect) -> Rect {
    let running = app.agents().len() + 1;
    let help = help_text_for(app, (app.screen, app.show_hidden), true, None, None);
    let help_rows = help_rows(app, area.width, running, &help);
    split(area, 1, help_rows, true).panel
}

/// The terminal of the agent `name` in `panel`, below its header (`header_rows`).
pub fn terminal_in_panel(panel: Rect, name: &str, jail: bool) -> Rect {
    let rows = header_rows(name, jail, panel);
    Rect {
        y: panel.y + rows,
        height: panel.height - rows,
        ..panel
    }
}

fn layout_for(app: &App, area: Rect, agent: Option<&RunningAgent>) -> Areas {
    let status_rows = status_lines(app, area.width).len();
    let help_rows = help_rows(app, area.width, app.agents().len(), &help_text(app));
    let mut areas = split(area, status_rows, help_rows, !app.agents().is_empty());
    if let Some(agent) = agent.filter(|_| !areas.panel.is_empty()) {
        areas.terminal = terminal_in_panel(areas.panel, &agent.name, agent.jail);
        areas.header = Rect {
            height: areas.terminal.y - areas.panel.y,
            ..areas.panel
        };
    } else {
        areas.terminal = areas.panel;
    }
    areas
}

/// The status and the help (`status_rows`, `help_rows`, up to a third of the screen each) at
/// the bottom of `area`, and above them the list, beside the panel of an agent when `beside`.
fn split(area: Rect, status_rows: usize, help_rows: usize, beside: bool) -> Areas {
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
    let mut areas = Areas {
        body,
        list: body,
        status,
        help,
        ..Areas::default()
    };
    if beside {
        let [list, panel] = Layout::horizontal([
            Constraint::Length(LIST_WIDTH.min(area.width / 2)),
            Constraint::Min(0),
        ])
        .areas(body);
        areas.list = list;
        areas.panel = panel;
    }
    areas
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

/// Draws the whole screen (`layout`): the list of the current screen, the panel of the shown
/// agent with `terminal` (its vt100 screen) when an agent runs, the status line and the help
/// line, and the open dialog centered over the body.
pub fn draw_with_terminal(frame: &mut Frame, app: &App, terminal: Option<&vt100::Screen>) {
    let areas = layout(app, frame.area());
    let (body, status, help) = (areas.list, areas.status, areas.help);
    match app.screen {
        Screen::Modules => draw_modules(frame, app, body),
        Screen::Projects => draw_projects(frame, app, body),
        Screen::Tickets => draw_tickets(frame, app, body),
        Screen::Ticket => draw_ticket(frame, app, body),
        Screen::ActiveAgents => draw_active_agents(frame, app, body),
        Screen::Settings => draw_settings(frame, app, body),
    }
    if let Some(agent) = app.shown_agent() {
        draw_agent(frame, areas.panel, &agent.name, agent.jail, terminal);
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
        Some(Dialog::ConfirmPrompt {
            precheck,
            rendered,
            scroll,
            ..
        }) => draw_prompt(
            frame,
            prompt_area(frame.area(), areas),
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

/// The keys of the current screen, in the help line. With the focus on the terminal, only the
/// way back to the list (every other key is the program's). With agents running, the lists
/// that open "Agentes ativos" say so, with how many there are and how many are marked.
fn help_text(app: &App) -> String {
    let on_terminal = app.focus() == Focus::Terminal && app.shown().is_some();
    let count = (!app.agents().is_empty()).then(|| agents_count(app));
    help_text_for(
        app,
        (app.screen, app.show_hidden),
        on_terminal,
        app.dialog.as_ref(),
        count,
    )
}

/// `help_text` in a given state: the screen (with the filter of the tickets), the focus on the
/// terminal, the open dialog, and the count of the agents (`None` without agents running).
fn help_text_for(
    app: &App,
    (screen, show_hidden): (Screen, bool),
    on_terminal: bool,
    dialog: Option<&Dialog>,
    count: Option<String>,
) -> String {
    let shortcut = app.shortcut().label();
    if on_terminal {
        return format!("{shortcut} voltar à lista");
    }
    let text = match (dialog, screen) {
        (Some(Dialog::AddPath { .. }), _) => "Enter confirmar · Esc cancelar",
        (Some(Dialog::NeedsConfig { .. }), _) => "←/→/Tab escolher · Enter acionar · Esc cancelar",
        (Some(Dialog::ConfirmRemove { .. }), _) => "s remover · qualquer outra tecla cancela",
        (Some(Dialog::NewTicket { .. }), _) => {
            "Tab trocar de campo · Enter confirmar · Esc cancelar"
        }
        (Some(Dialog::EditTitle { .. }), _) => "Enter confirmar · Esc cancelar",
        (Some(Dialog::ConfirmHide { .. }), _) => "s ocultar · qualquer outra tecla cancela",
        (Some(Dialog::ChooseType { .. }), _) => "↑/↓ escolher · Enter abrir · Esc cancelar",
        (Some(Dialog::StageForm { .. }), _) => {
            "Tab/↑/↓ trocar de campo · Enter confirmar (no campo do editor: abrir o editor) · \
             Esc cancelar"
        }
        (Some(Dialog::ConfirmPrompt { .. }), _) => PROMPT_KEYS,
        (Some(Dialog::AgentError { .. }), _) => "Enter/Esc fechar",
        (Some(Dialog::ConfirmEndAgent { .. }), _) => "s encerrar · qualquer outra tecla cancela",
        (Some(Dialog::ConfirmQuit), _) => "s encerrar e sair · qualquer outra tecla cancela",
        (Some(Dialog::CaptureShortcut { .. }), _) => "Esc cancelar",
        (Some(Dialog::ChooseTicketStage { .. }), _) => {
            "↑/↓ escolher · Enter escolher · Esc cancelar"
        }
        (Some(Dialog::ConfirmStageChange { .. }), _) => "s mudar · qualquer outra tecla cancela",
        (Some(Dialog::EditId { .. }), _) => "Enter confirmar · Esc cancelar",
        (Some(Dialog::ConfirmEditId { .. }), _) => "s trocar · qualquer outra tecla cancela",
        (None, Screen::Tickets) if show_hidden => {
            "↑/↓ mover · a novo · e editar título · o ocultar/restaurar · f esconder ocultos · \
             g abrir agente · Enter detalhes · Esc voltar · q sair"
        }
        (None, Screen::Tickets) => {
            "↑/↓ mover · a novo · e editar título · o ocultar/restaurar · f mostrar ocultos · \
             g abrir agente · Enter detalhes · Esc voltar · q sair"
        }
        (None, Screen::Ticket) => {
            "↑/↓ mover · Enter abrir agente · m mudar etapa · i editar ID · g novo agente · \
             Esc voltar · q sair"
        }
        (None, Screen::Modules) => "↑/↓ mover · Enter abrir · c configurações · q sair",
        (None, Screen::Projects) => "↑/↓ mover · a adicionar · r remover · Esc voltar · q sair",
        (None, Screen::ActiveAgents) => {
            return format!(
                "↑/↓ escolher · Enter abrir · x encerrar · {shortcut} alternar · Esc voltar · \
                 q sair"
            );
        }
        (None, Screen::Settings) => {
            return "t trocar · p voltar ao padrão · Esc voltar · q sair".to_string();
        }
    };
    let opens_the_view = dialog.is_none()
        && matches!(
            screen,
            Screen::Modules | Screen::Projects | Screen::Tickets | Screen::Ticket
        );
    match (text.strip_suffix("q sair"), count) {
        (Some(start), Some(count)) if opens_the_view => {
            format!("{start}v agentes ativos ({count}) · q sair")
        }
        _ => text.to_string(),
    }
}

/// Every list screen, with the filter of the tickets: the helps `help_rows` reserves rows for.
const LIST_SCREENS: [(Screen, bool); 7] = [
    (Screen::Modules, false),
    (Screen::Projects, false),
    (Screen::Tickets, false),
    (Screen::Tickets, true),
    (Screen::Ticket, false),
    (Screen::ActiveAgents, false),
    (Screen::Settings, false),
];

/// The rows reserved for the help with `running` agents (review NS-34, P1): never fewer than
/// the longest help of the lists, with the longest count (`N, N com saída nova`), whatever the
/// screen, the focus, the dialog or the marks. So toggling the focus, going to another list or
/// a new mark does not resize every terminal (each resize redraws every claude and marks the
/// hidden ones). The shorter help of the moment is drawn in the same rows. Without agents, the
/// rows of the help itself, as before the NS-34.
fn help_rows(app: &App, width: u16, running: usize, help: &str) -> usize {
    let rows = wrap_words(help, width.max(1)).len();
    if running == 0 {
        return rows;
    }
    let count = format!("{running}, {running} com saída nova");
    LIST_SCREENS
        .into_iter()
        .map(|screen| {
            let text = help_text_for(app, screen, false, None, Some(count.clone()));
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
/// the panel (at least one).
fn header_rows(name: &str, jail: bool, panel: Rect) -> u16 {
    let lines = header_lines(name, jail, panel.width).len();
    u16::try_from(lines)
        .unwrap_or(u16::MAX)
        .min((panel.height / 3).max(1))
        .min(panel.height)
}

/// The panel of an agent in `area`: its header (wrapped, `header_rows`) and its terminal below
/// it, the same split as `layout`.
pub fn draw_agent(
    frame: &mut Frame,
    area: Rect,
    name: &str,
    jail: bool,
    terminal: Option<&vt100::Screen>,
) {
    let rows = header_rows(name, jail, area);
    let header = Rect {
        height: rows,
        ..area
    };
    let lines: Vec<Line> = header_lines(name, jail, area.width)
        .into_iter()
        .take(usize::from(rows))
        .collect();
    frame.render_widget(Paragraph::new(lines), header);
    if let Some(screen) = terminal {
        // The program's cursor, hidden while the history is scrolled (it is not on screen).
        let mut cursor = Cursor::default();
        if screen.scrollback() > 0 || screen.hide_cursor() {
            cursor.hide();
        }
        let below = Rect {
            y: area.y + rows,
            height: area.height - rows,
            ..area
        };
        frame.render_widget(PseudoTerminal::new(screen).cursor(cursor), below);
    }
}

fn selected_style() -> Style {
    Style::new().add_modifier(Modifier::BOLD)
}

fn draw_modules(frame: &mut Frame, app: &App, area: Rect) {
    let items: Vec<ListItem> = ModuleKind::ALL
        .iter()
        .map(|module| ListItem::new(module.name()))
        .collect();
    let list = List::new(items)
        .block(Block::bordered().title(" Módulos "))
        .highlight_symbol("> ")
        .highlight_style(selected_style());
    let mut state = ListState::default().with_selected(Some(app.module_index));
    frame.render_stateful_widget(list, area, &mut state);
}

/// "Agentes ativos": every agent of this nick, in the order they were opened, with
/// `[FORA DA JAIL]` (red) out of the jail and `● saída nova` when marked.
fn draw_active_agents(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(" Agentes ativos ");
    if app.agents().is_empty() {
        frame.render_widget(
            Paragraph::new("Nenhum agente ativo.")
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .agents()
        .iter()
        .map(|agent| {
            let mut spans = vec![Span::raw(printable(&agent.name))];
            if !agent.jail {
                spans.push(Span::raw("  "));
                spans.push(Span::styled("[FORA DA JAIL]", Style::new().fg(Color::Red)));
            }
            if agent.unseen {
                spans.push(Span::raw("  ● saída nova"));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_symbol("> ")
        .highlight_style(selected_style());
    let mut state = ListState::default().with_selected(Some(app.agent_index));
    frame.render_stateful_widget(list, area, &mut state);
}

/// "Configurações": the reserved shortcut, with `(padrão)` when the database has none.
fn draw_settings(frame: &mut Frame, app: &App, area: Rect) {
    let default = if app.shortcut_stored {
        ""
    } else {
        " (padrão)"
    };
    let line = format!(
        "Atalho para alternar entre a lista e o terminal: {}{default}",
        app.shortcut().label()
    );
    frame.render_widget(
        Paragraph::new(line)
            .block(Block::bordered().title(" Configurações "))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_projects(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().title(format!(" {} > Projetos ", app.module().name()));
    if app.rows.is_empty() {
        frame.render_widget(
            Paragraph::new("Nenhum projeto cadastrado. Pressione a para adicionar.")
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .rows
        .iter()
        .map(|row| {
            let mut spans = vec![
                Span::raw(printable(&row.project.name)),
                Span::raw("  "),
                Span::raw(printable(&row.project.path.to_string_lossy())),
            ];
            let mut style = Style::new();
            if let Some(mark) = projects::unavailable_text(row.availability) {
                spans.push(Span::raw("  "));
                spans.push(Span::raw(mark));
                style = style.add_modifier(Modifier::DIM);
            }
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_symbol("> ")
        .highlight_style(selected_style());
    let mut state = ListState::default().with_selected(Some(app.project_index));
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_tickets(frame: &mut Frame, app: &App, area: Rect) {
    let project = app
        .project
        .as_ref()
        .map_or(String::new(), |p| printable(&p.name));
    let filter = if app.show_hidden {
        " (com ocultos)"
    } else {
        ""
    };
    let block = Block::bordered().title(format!(
        " {} > {project} > Tickets{filter} ",
        app.module().name()
    ));
    if app.ticket_rows.is_empty() {
        let empty = if app.show_hidden {
            "Nenhum ticket neste projeto, nem oculto. Pressione a para criar."
        } else {
            "Nenhum ticket neste projeto. Pressione a para criar."
        };
        frame.render_widget(
            Paragraph::new(empty)
                .block(block)
                .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let items: Vec<ListItem> = app
        .ticket_rows
        .iter()
        .map(|ticket| {
            // The stage right after the ID: beside an agent the list has 40 columns, and the
            // end of the row is cut (NS-36).
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
            ListItem::new(Line::from(spans)).style(style)
        })
        .collect();
    let list = List::new(items)
        .block(block)
        .highlight_symbol("> ")
        .highlight_style(selected_style());
    let mut state = ListState::default().with_selected(Some(app.ticket_index));
    frame.render_stateful_widget(list, area, &mut state);
}

/// The mark of a stage on the list and on the ticket screen: `[<label>]`, and `[✓ Concluído]` in
/// green for the done ones (which stay on the list: only `o` hides a ticket).
fn stage_mark(key: &str) -> Span<'static> {
    if key == TicketStage::Done.key() {
        Span::styled(
            format!("[✓ {}]", TicketStage::Done.label()),
            Style::new().fg(Color::Green),
        )
    } else {
        Span::raw(format!("[{}]", printable(&tickets::stage_label(key))))
    }
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

/// The ticket screen (NS-36): the ticket, its stage and its artifacts on top; then the history
/// of the stage (the newest when it does not fit, with how many older ones are left out) and
/// the agents of the ticket, a list with selection.
fn draw_ticket(frame: &mut Frame, app: &App, area: Rect) {
    let project = app
        .project
        .as_ref()
        .map_or(String::new(), |p| printable(&p.name));
    let Some(detail) = app.ticket_detail.as_ref() else {
        frame.render_widget(Block::bordered(), area);
        return;
    };
    let ticket = &detail.ticket;
    let block = Block::bordered().title(format!(
        " {} > {project} > {} ",
        app.module().name(),
        printable(&ticket.key)
    ));
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
    let stage = if ticket.stage == TicketStage::Done.key() {
        stage_mark(&ticket.stage)
    } else {
        Span::raw(printable(&tickets::stage_label(&ticket.stage)))
    };
    let exists = |there: bool| if there { "(existe)" } else { "(não existe)" };
    let top = vec![
        Line::from(head),
        Line::from(vec![Span::raw("Etapa atual: "), stage]),
        // The state before the path: beside an agent the screen has 40 columns, and the end of
        // the line is cut (review of the NS-36, P3.1; as the stage after the ID on the list).
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
    ];
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
    let rest = inner.height - top_rows;
    // Each section: its title, its rows (at least the "none" one); the history also a blank
    // row below. The agents keep at least half of what is left (or what they need).
    let as_rows = |n: usize| u16::try_from(n).unwrap_or(u16::MAX);
    let agents_need = 1 + as_rows(detail.agents.len().max(1));
    let history_need = 2 + as_rows(detail.changes.len().max(1));
    let agents_min = agents_need.min((rest / 2).max(2)).min(rest);
    let history_rows = history_need.min(rest - agents_min);
    let history_area = Rect {
        y: inner.y + top_rows,
        height: history_rows,
        ..inner
    };
    let agents_area = Rect {
        y: history_area.y + history_rows,
        height: rest - history_rows,
        ..inner
    };
    let mut history = vec![Line::from("Histórico de etapas")];
    if detail.changes.is_empty() {
        history.push(Line::from("Nenhuma mudança: o ticket começou em PO."));
    } else {
        let slots = usize::from(history_rows.saturating_sub(2));
        let mut shown = &detail.changes[..];
        if shown.len() > slots {
            let keep = slots.saturating_sub(1);
            let older = shown.len() - keep;
            history.push(Line::from(format!("(+{older} mudanças mais antigas)")));
            shown = &shown[older..];
        }
        for change in shown {
            history.push(Line::from(format!(
                "  {}  {} → {}",
                short_date(&change.changed_at),
                printable(&tickets::stage_label(&change.from_stage)),
                printable(&tickets::stage_label(&change.to_stage))
            )));
        }
    }
    frame.render_widget(Paragraph::new(history), history_area);
    if agents_area.height == 0 {
        return;
    }
    frame.render_widget(
        Paragraph::new("Agentes do ticket"),
        Rect {
            height: 1,
            ..agents_area
        },
    );
    let list_area = Rect {
        y: agents_area.y + 1,
        height: agents_area.height - 1,
        ..agents_area
    };
    if detail.agents.is_empty() {
        frame.render_widget(
            Paragraph::new("Nenhum agente aberto para este ticket."),
            list_area,
        );
        return;
    }
    let items: Vec<ListItem> = detail
        .agents
        .iter()
        .map(|agent| {
            let state = if app.agent(agent.id).is_some() {
                "● vivo"
            } else if agent.state == AgentState::Active.key() {
                "ativo em outro nick"
            } else {
                "encerrado"
            };
            ListItem::new(format!("{}  {state}", printable(&agent.name)))
        })
        .collect();
    let list = List::new(items)
        .highlight_symbol("> ")
        .highlight_style(selected_style());
    let mut state = ListState::default().with_selected(Some(app.ticket_agent_index));
    frame.render_stateful_widget(list, list_area, &mut state);
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

/// The removal question, with the tickets that go with the project (plan D7). Without
/// tickets it is the text of the NS-31, byte for byte.
fn removal_question(name: &str, path: &str, tickets: i64) -> String {
    let goes = match tickets {
        0 => String::new(),
        1 => "O ticket dele será apagado do banco. ".to_string(),
        n => format!("Os {n} tickets dele (inclusive os ocultos) serão apagados do banco. "),
    };
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
            title,
            jira_id,
            focus,
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
                " Novo ticket ",
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
            let typed = field(input);
            let cursor = typed.chars().count();
            (
                " Editar título ",
                vec![
                    Line::from(format!("ID: {}", printable(&ticket.key))),
                    Line::from(""),
                    Line::from("Título:"),
                    Line::from(typed),
                ],
                Some((3, cursor)),
            )
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
            ticket,
            types,
            index,
            ..
        } => {
            let mut lines = vec![
                Line::from(format!(
                    "Ticket {} — {}",
                    printable(&ticket.key),
                    printable(&ticket.title)
                )),
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
            ticket,
            kind,
            values,
            focus,
            plan_missing,
            ..
        } => {
            let mut lines = vec![
                Line::from(format!(
                    "Ticket {} — {}",
                    printable(&ticket.key),
                    printable(&ticket.title)
                )),
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
        Dialog::ConfirmStageChange { ticket, to } => (
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
            let typed = field(input);
            let cursor = typed.chars().count();
            (
                " Editar ID ",
                vec![
                    Line::from(format!("ID atual: {}", printable(&ticket.key))),
                    Line::from(""),
                    Line::from("Novo ID do ticket (ex: SC-123):"),
                    Line::from(typed),
                ],
                Some((3, cursor)),
            )
        }
        Dialog::ConfirmEditId { ticket, new, moves } => {
            let folder = if *moves {
                format!(
                    " A pasta {} será renomeada para {}.",
                    printable(&docs::dir_text(&ticket.key)),
                    printable(&docs::dir_text(new))
                )
            } else {
                String::new()
            };
            (
                " Editar ID ",
                vec![Line::from(format!(
                    "Trocar o ID {} por {}?{folder} A branch do git, o ticket no Jira e os \
                     agentes já registrados não mudam. (s/N)",
                    printable(&ticket.key),
                    printable(new)
                ))],
                None,
            )
        }
        Dialog::ConfirmRemove { project, tickets } => (
            " Remover projeto ",
            vec![Line::from(removal_question(
                &printable(&project.name),
                &printable(&project.path.to_string_lossy()),
                *tickets,
            ))],
            None,
        ),
    };
    let popup = dialog_area(area, &lines);
    if popup.width < 3 || popup.height < 3 {
        return;
    }
    frame.render_widget(Clear, popup);
    let block = Block::bordered().title(title);
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
    precheck: &Precheck,
    rendered: &Rendered,
    scroll: usize,
) {
    frame.render_widget(Clear, area);
    let layout = prompt_layout(area, precheck, rendered);
    if layout.outer.width < 3 || layout.outer.height < 3 {
        return;
    }
    frame.render_widget(Block::bordered().title(" Prompt final "), layout.outer);
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
    use crate::app::{Button, Dialog, Row, Screen, TicketField};
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

    fn last_line(buffer: &Buffer) -> String {
        text(buffer).lines().last().unwrap_or("").to_string()
    }

    #[test]
    fn the_modules_screen_lists_auto_scrum_with_its_help() {
        let f = fixture();
        let buffer = render(&f.app);
        assert_has(&buffer, "Módulos");
        assert_has(&buffer, "auto_scrum");
        assert_eq!(
            last_line(&buffer).trim(),
            "↑/↓ mover · Enter abrir · c configurações · q sair"
        );
    }

    #[test]
    fn the_projects_screen_marks_the_unavailable_ones_dimmed() {
        let mut f = fixture();
        f.app.screen = Screen::Projects;
        f.app.rows = vec![
            Row {
                project: project(1, "alfa", "/w/alfa"),
                availability: Availability::Available,
            },
            Row {
                project: project(2, "beta", "/w/beta"),
                availability: Availability::DirMissing,
            },
            Row {
                project: project(3, "gama", "/w/gama"),
                availability: Availability::ConfigMissing,
            },
        ];
        let buffer = render(&f.app);
        assert_has(&buffer, "auto_scrum > Projetos");
        assert_has(&buffer, "alfa  /w/alfa");
        assert_has(
            &buffer,
            "beta  /w/beta  [indisponível: pasta não encontrada]",
        );
        assert_has(
            &buffer,
            "gama  /w/gama  [indisponível: sem .nick/nick.conf]",
        );
        let (y, _) = find(&buffer, "alfa  /w/alfa").unwrap();
        assert!(
            !text(&buffer)
                .lines()
                .nth(y as usize)
                .unwrap()
                .contains("indisponível"),
            "the available one has no mark"
        );
        for name in ["beta", "gama"] {
            let (y, x) = find(&buffer, name).unwrap();
            assert!(
                buffer[(x, y)].modifier.contains(Modifier::DIM),
                "{name} is dimmed"
            );
        }
        let (y, x) = find(&buffer, "alfa").unwrap();
        assert!(!buffer[(x, y)].modifier.contains(Modifier::DIM));
        assert_eq!(
            last_line(&buffer).trim(),
            "↑/↓ mover · a adicionar · r remover · Esc voltar · q sair"
        );
    }

    #[test]
    fn an_empty_projects_list_says_how_to_add() {
        let mut f = fixture();
        f.app.screen = Screen::Projects;
        let buffer = render(&f.app);
        assert_has(
            &buffer,
            "Nenhum projeto cadastrado. Pressione a para adicionar.",
        );
    }

    #[test]
    fn the_status_message_is_shown_above_the_help() {
        let mut f = fixture();
        f.app.screen = Screen::Projects;
        f.app.status = Some("A pasta x não existe.".into());
        let buffer = render(&f.app);
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        assert_eq!(lines[lines.len() - 2].trim(), "A pasta x não existe.");
    }

    /// A status longer than the screen wraps (NS-32: decision of the user), above the help line:
    /// none of it is cut. A short one still takes one row.
    #[test]
    fn a_long_status_message_wraps_above_the_help() {
        let mut f = fixture();
        f.app.screen = Screen::Projects;
        let message = "Projeto indisponível: [indisponível: sem .nick/nick.conf]. Os tickets só \
                       abrem com a pasta e o .nick/nick.conf no lugar.";
        assert!(message.chars().count() > 100);
        f.app.status = Some(message.into());
        let buffer = render(&f.app);
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        let n = lines.len();
        assert_eq!(
            lines[n - 1].trim(),
            "↑/↓ mover · a adicionar · r remover · Esc voltar · q sair"
        );
        assert_eq!(
            format!("{} {}", lines[n - 3].trim(), lines[n - 2].trim()),
            message,
            "two rows, word wrapped"
        );
        assert!(lines[n - 4].contains('└'), "the list ends right above");
        // A word longer than the width is broken, not lost.
        let long = format!("Esse projeto já está cadastrado: /{}.", "x".repeat(150));
        f.app.status = Some(long.clone());
        let buffer = render(&f.app);
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        let n = lines.len();
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
        f.app.screen = Screen::Projects;
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
        f.app.screen = Screen::Projects;
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
        let inside = inside.split('│').next().unwrap().trim_end().to_string();
        assert!(inside.starts_with('…'), "{inside:?}");
        assert!(inside.ends_with("/fim-do-caminho"), "{inside:?}");
        assert!(path.ends_with(&inside[inside.char_indices().nth(1).unwrap().0..]));
        // The next row is the bottom border: nothing wrapped.
        let below = text(&buffer)
            .lines()
            .nth(usize::from(label_y) + 2)
            .unwrap()
            .to_string();
        assert!(below.contains('└'), "{below:?}");
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
        f.app.screen = Screen::Projects;
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
            .map(|l| l.replace('│', "").trim().to_string())
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
        f.app.screen = Screen::Projects;
        f.app.dialog = Some(Dialog::ConfirmRemove {
            project: project(1, "alfa", "/w/alfa"),
            tickets: 0,
        });
        let buffer = render(&f.app);
        let flat: String = text(&buffer)
            .lines()
            .map(|l| l.replace('│', "").trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
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
        }
    }

    fn on_tickets(f: &mut Fixture) {
        f.app.screen = Screen::Tickets;
        f.app.project = Some(project(1, "meu-proj", "/w/meu-proj"));
    }

    /// The text of the dialog box, without borders, as one line.
    fn flat(buffer: &Buffer) -> String {
        text(buffer)
            .lines()
            .map(|l| l.replace('│', "").trim().to_string())
            .filter(|l| !l.is_empty())
            .collect::<Vec<_>>()
            .join(" ")
    }

    #[test]
    fn the_tickets_screen_lists_id_and_title_with_the_provisional_mark() {
        let mut f = fixture();
        on_tickets(&mut f);
        f.app.ticket_rows = vec![
            ticket(1, "NS-1", "Corrigir o login", false, false),
            ticket(2, "3f9a0c7e", "Sem ID ainda", true, false),
        ];
        let buffer = render(&f.app);
        assert_has(&buffer, " auto_scrum > meu-proj > Tickets ");
        assert_has(&buffer, "NS-1  [PO]  Corrigir o login");
        assert_has(&buffer, "3f9a0c7e  [PO]  Sem ID ainda  [provisório]");
        let (y, _) = find(&buffer, "NS-1").unwrap();
        assert!(
            !text(&buffer)
                .lines()
                .nth(y as usize)
                .unwrap()
                .contains("[provisório]")
        );
        assert!(find(&buffer, "(com ocultos)").is_none());
        // With `g abrir agente` (NS-33) the help passes 100 columns and wraps, as with the filter.
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        let n = lines.len();
        assert_eq!(
            format!("{} {}", lines[n - 2].trim(), lines[n - 1].trim()),
            "↑/↓ mover · a novo · e editar título · o ocultar/restaurar · f mostrar ocultos · \
             g abrir agente · Enter detalhes · Esc voltar · q sair"
        );
    }

    #[test]
    fn with_the_filter_the_hidden_ones_are_dimmed_and_marked() {
        let mut f = fixture();
        on_tickets(&mut f);
        f.app.show_hidden = true;
        f.app.ticket_rows = vec![
            ticket(1, "NS-1", "Visível", false, false),
            ticket(2, "NS-2", "Escondido", false, true),
            ticket(3, "abcd1234", "Provisório escondido", true, true),
        ];
        let buffer = render(&f.app);
        assert_has(&buffer, " auto_scrum > meu-proj > Tickets (com ocultos) ");
        assert_has(&buffer, "NS-2  [PO]  Escondido  [oculto]");
        assert_has(
            &buffer,
            "abcd1234  [PO]  Provisório escondido  [provisório]  [oculto]",
        );
        for key in ["NS-2", "abcd1234"] {
            let (y, x) = find(&buffer, key).unwrap();
            assert!(buffer[(x, y)].modifier.contains(Modifier::DIM), "{key}");
        }
        let (y, x) = find(&buffer, "NS-1").unwrap();
        assert!(!buffer[(x, y)].modifier.contains(Modifier::DIM));
        // 101 columns: the help wraps (NS-32, decision of the user), nothing is cut.
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        let n = lines.len();
        assert_eq!(
            format!("{} {}", lines[n - 2].trim(), lines[n - 1].trim()),
            "↑/↓ mover · a novo · e editar título · o ocultar/restaurar · f esconder ocultos · \
             g abrir agente · Enter detalhes · Esc voltar · q sair"
        );
        assert_eq!(
            lines[n - 3].trim(),
            "",
            "the empty status row, as in the NS-31"
        );
        assert!(lines[n - 4].contains('└'), "the list ends right above");
    }

    #[test]
    fn an_empty_tickets_list_says_how_to_create_with_and_without_the_filter() {
        let mut f = fixture();
        on_tickets(&mut f);
        assert_has(
            &render(&f.app),
            "Nenhum ticket neste projeto. Pressione a para criar.",
        );
        f.app.show_hidden = true;
        assert_has(
            &render(&f.app),
            "Nenhum ticket neste projeto, nem oculto. Pressione a para criar.",
        );
    }

    #[test]
    fn the_new_ticket_dialog_has_both_fields_and_the_cursor_on_the_focused_one() {
        let mut f = fixture();
        on_tickets(&mut f);
        for (focus, typed) in [
            (TicketField::Title, "Meu título"),
            (TicketField::JiraId, "NS-12"),
        ] {
            f.app.dialog = Some(Dialog::NewTicket {
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
        on_tickets(&mut f);
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
        on_tickets(&mut f);
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
        f.app.screen = Screen::Projects;
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
    fn control_characters_in_tickets_are_shown_as_question_marks() {
        let mut f = fixture();
        on_tickets(&mut f);
        f.app.project = Some(project(1, "p\x1bq", "/w/p"));
        f.app.ticket_rows = vec![ticket(1, "NS-1", "a\x1b[2Jb", false, false)];
        let buffer = render(&f.app);
        assert_has(&buffer, "auto_scrum > p?q > Tickets");
        assert_has(&buffer, "NS-1  [PO]  a?[2Jb");
    }

    #[test]
    fn control_characters_are_shown_as_question_marks() {
        let mut f = fixture();
        f.app.screen = Screen::Projects;
        f.app.rows = vec![Row {
            project: project(1, "a\x1b[2Jb", "/w/a\x1b[2Jb"),
            availability: Availability::DirMissing,
        }];
        f.app.status = Some("A pasta x\ny não existe.".into());
        let buffer = render(&f.app);
        assert_has(&buffer, "a?[2Jb  /w/a?[2Jb");
        assert_has(&buffer, "A pasta x?y não existe.");
        f.app.dialog = Some(Dialog::AddPath {
            input: "a\tb".into(),
        });
        assert_has(&render(&f.app), "a?b");
    }

    #[test]
    fn a_tiny_terminal_does_not_panic() {
        let mut f = fixture();
        f.app.screen = Screen::Projects;
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

    fn render_agent(name: &str, jail: bool, screen: Option<&vt100::Screen>) -> Buffer {
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| draw_agent(frame, frame.area(), name, jail, screen))
            .unwrap();
        terminal.backend().buffer().clone()
    }

    #[test]
    fn the_agent_header_in_the_jail() {
        let buffer = render_agent("proj · Dev · NS-1", true, None);
        assert_eq!(
            text(&buffer).lines().next().unwrap(),
            "proj · Dev · NS-1 · na jail"
        );
        let (y, x) = find(&buffer, "na jail").unwrap();
        assert_ne!(buffer[(x, y)].fg, Color::Red);
        assert!(find(&buffer, "FORA DA JAIL").is_none());
    }

    #[test]
    fn the_agent_header_out_of_the_jail_is_red_and_bold() {
        let buffer = render_agent("proj · Dev · NS-1", false, None);
        assert_eq!(
            text(&buffer).lines().next().unwrap(),
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
        assert_eq!(header_rows(name, true, Rect::new(0, 0, 100, 30)), 1);
        assert_eq!(header_rows(name, true, Rect::new(0, 0, 10, 1)), 1);
        let mut parser = vt100::Parser::new(29, 100, 0);
        parser.process(b"\x1b[31mlinha um\x1b[0m\r\nlinha dois\x1b[29;1Hfim");
        let buffer = render_agent("proj · PO · NS-1", true, Some(parser.screen()));
        let body = text(&buffer);
        let body: Vec<&str> = body.lines().collect();
        assert!(body[1].starts_with("linha um"), "{body:?}");
        assert!(body[2].starts_with("linha dois"), "{body:?}");
        assert_eq!(
            buffer[(0, 1)].fg,
            Color::Indexed(1),
            "the colors of the program"
        );
        // No status line nor help: the last row of the program is the last row of the screen.
        assert_eq!(find(&buffer, "fim"), Some((29, 0)), "{body:?}");
    }

    #[test]
    fn without_an_agent_there_is_no_panel() {
        // The Agent screen is gone (NS-34): without an agent running, the list takes the body
        // and the panel is empty.
        let f = fixture();
        let areas = layout(&f.app, Rect::new(0, 0, 100, 30));
        assert_eq!(areas.list, areas.body);
        assert!(areas.panel.is_empty() && areas.terminal.is_empty());
    }

    fn precheck(jail: bool) -> Precheck {
        Precheck {
            jail,
            jira: false,
            sentry: false,
            warning: None,
        }
    }

    #[test]
    fn the_menu_lists_the_types_with_the_selection() {
        let mut f = fixture();
        on_tickets(&mut f);
        let t = ticket(1, "NS-1", "Corrigir o login", false, false);
        f.app.ticket_rows = vec![t.clone()];
        f.app.dialog = Some(Dialog::ChooseType {
            project: PathBuf::from("/x/meu-proj"),
            ticket: t,
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
        on_tickets(&mut f);
        let t = ticket(1, "NS-1", "Corrigir o login", false, false);
        f.app.ticket_rows = vec![t.clone()];
        f.app.dialog = Some(Dialog::StageForm {
            project: PathBuf::from("/x/meu-proj"),
            ticket: t,
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
        on_tickets(&mut f);
        let t = ticket(1, "NS-1", "Corrigir o login", false, false);
        f.app.dialog = Some(Dialog::ConfirmPrompt {
            project: PathBuf::from("/x/meu-proj"),
            ticket: t,
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
        on_tickets(&mut f);
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

        fn spawn(
            &mut self,
            _: &Path,
            _: &Path,
            _: i64,
            _: &str,
            _: &str,
            _: StageType,
            _: &Path,
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
            std::fs::create_dir_all(&dir).unwrap();
            let p = db
                .insert_project("auto_scrum", name, dir.to_str().unwrap())
                .unwrap();
            let t = db.insert_ticket(p.id, "NS-1", false, "t").unwrap();
            f.app.project = Some(p);
            f.app.screen = Screen::Tickets;
            f.app.open_agent(
                &dir,
                &t,
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
        // The help keeps the two rows the list of the tickets needs (review NS-34, P1).
        assert_eq!(areas.list, Rect::new(0, 0, 40, 27));
        assert_eq!(areas.header, Rect::new(40, 0, 60, 1));
        assert_eq!(areas.terminal, Rect::new(40, 1, 60, 26));
        assert_eq!(areas.status, Rect::new(0, 27, 100, 1));
        assert_eq!(areas.help, Rect::new(0, 28, 100, 2));
        let agent = f.app.shown_agent().unwrap();
        assert_eq!(agent_area(&f.app, area, agent), areas.terminal);
        let mut parser = vt100::Parser::new(26, 60, 0);
        parser.process(b"primeira\x1b[26;1Hultima");
        let buffer = render_at(&f.app, 100, 30, Some(parser.screen()));
        assert_eq!(find(&buffer, "proj · Dev · NS-1 · na jail"), Some((0, 40)));
        assert_eq!(find(&buffer, "primeira"), Some((1, 40)));
        assert_eq!(
            find(&buffer, "ultima"),
            Some((26, 40)),
            "the last row of the program"
        );
        let (y, _) = find(&buffer, "auto_scrum > proj > Tickets").unwrap();
        assert_eq!(y, 0);
        assert!(text(&buffer).lines().nth(26).unwrap().starts_with('└'));
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        assert_eq!(
            lines[28].trim(),
            "Ctrl+\\ voltar à lista",
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
        assert_eq!(
            areas.header,
            Rect::new(40, 0, 60, 2),
            "decision of the user"
        );
        assert_eq!(areas.terminal, Rect::new(40, 2, 60, 25));
        // Each agent has the size of its own header: the one in the jail, one row.
        let in_jail = &f.app.agents()[0];
        assert_eq!(agent_area(&f.app, area, in_jail), Rect::new(40, 1, 60, 26));
        let buffer = render_at(&f.app, 100, 30, None);
        let header: Vec<String> = text(&buffer)
            .lines()
            .take(2)
            .map(|l| l.chars().skip(40).collect::<String>().trim().to_string())
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
        assert_eq!(areas.terminal.height, 30 - 1 - 3 - areas.help.height);
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

    #[test]
    fn the_view_of_the_active_agents_marks_out_of_the_jail_and_new_output() {
        let (mut f, _) = with_running(&["proj", "fora"]);
        f.app.focus = Focus::List;
        f.app.screen = Screen::ActiveAgents;
        f.app.agents[0].unseen = true;
        let buffer = render(&f.app);
        assert_has(&buffer, " Agentes ativos ");
        assert_has(&buffer, "proj · Dev · NS-1  ● saída nova");
        assert_has(&buffer, "fora · Dev · NS-1  [FORA DA JAIL]");
        let (y, x) = find(&buffer, "[FORA DA JAIL]").unwrap();
        assert_eq!(buffer[(x, y)].fg, Color::Red);
        let (y, _) = find(&buffer, "proj · Dev").unwrap();
        assert!(
            !text(&buffer)
                .lines()
                .nth(usize::from(y))
                .unwrap()
                .contains("[FORA")
        );
        assert_eq!(
            last_line(&buffer).trim(),
            "↑/↓ escolher · Enter abrir · x encerrar · Ctrl+\\ alternar · Esc voltar · q sair"
        );
        // The help shows the shortcut in use.
        f.app.shortcut = Shortcut::from_db("ctrl+]").unwrap();
        assert!(last_line(&render(&f.app)).contains("Ctrl+] alternar"));
    }

    #[test]
    fn the_view_without_agents_says_so() {
        let mut f = fixture();
        f.app.screen = Screen::ActiveAgents;
        assert_has(&render(&f.app), "Nenhum agente ativo.");
    }

    #[test]
    fn the_lists_say_how_many_agents_run_only_with_agents() {
        let (mut f, _) = with_running(&["proj", "fora", "outro"]);
        f.app.focus = Focus::List;
        f.app.screen = Screen::Modules;
        assert_eq!(
            last_line(&render(&f.app)).trim(),
            "↑/↓ mover · Enter abrir · c configurações · v agentes ativos (3) · q sair"
        );
        f.app.agents[1].unseen = true;
        f.app.screen = Screen::Projects;
        assert_eq!(
            last_line(&render(&f.app)).trim(),
            "↑/↓ mover · a adicionar · r remover · Esc voltar · v agentes ativos (3, 1 com \
             saída nova) · q sair"
        );
        f.app.screen = Screen::Tickets;
        let lines: Vec<String> = text(&render(&f.app)).lines().map(str::to_string).collect();
        let n = lines.len();
        assert!(
            format!("{} {}", lines[n - 2].trim(), lines[n - 1].trim()).ends_with(
                "g abrir agente · Enter detalhes · Esc voltar · v agentes ativos (3, 1 com saída \
                 nova) · q sair"
            ),
            "{lines:?}"
        );
        // With a dialog, the help of the dialog.
        f.app.dialog = Some(Dialog::ConfirmQuit);
        assert!(!last_line(&render(&f.app)).contains("v agentes"));
    }

    #[test]
    fn the_settings_show_the_shortcut_and_whether_it_is_the_default() {
        let mut f = fixture();
        f.app.screen = Screen::Settings;
        let buffer = render(&f.app);
        assert_has(&buffer, " Configurações ");
        assert_has(
            &buffer,
            "Atalho para alternar entre a lista e o terminal: Ctrl+\\ (padrão)",
        );
        assert_eq!(
            last_line(&buffer).trim(),
            "t trocar · p voltar ao padrão · Esc voltar · q sair"
        );
        f.app.shortcut = Shortcut::from_db("alt+x").unwrap();
        f.app.shortcut_stored = true;
        let buffer = render(&f.app);
        assert_has(
            &buffer,
            "Atalho para alternar entre a lista e o terminal: Alt+X",
        );
        assert!(find(&buffer, "(padrão)").is_none());
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
            // The ticket screen (NS-36) by itself, so a LIST_SCREENS without it is caught.
            for (screen, show_hidden) in LIST_SCREENS.into_iter().chain([(Screen::Ticket, false)]) {
                for focus in [Focus::Terminal, Focus::List] {
                    for unseen in [false, true] {
                        for dialog in [None, Some(Dialog::ConfirmQuit)] {
                            f.app.screen = screen;
                            f.app.show_hidden = show_hidden;
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
        // Without agents, the help takes only the rows it needs, as before the NS-34.
        let f = fixture();
        assert_eq!(layout(&f.app, Rect::new(0, 0, 100, 30)).help.height, 1);
    }

    /// Every screen of a list is in `LIST_SCREENS` (NS-36), so the rows of the help with an
    /// agent running never change between them. Today the help of the ticket screen fits in
    /// the rows of the tickets list, so the size test above alone would not catch it missing.
    #[test]
    fn every_list_screen_is_in_list_screens() {
        for screen in [
            Screen::Modules,
            Screen::Projects,
            Screen::Tickets,
            Screen::Ticket,
            Screen::ActiveAgents,
            Screen::Settings,
        ] {
            // Exhaustive: a new screen does not compile here until it is listed above.
            match screen {
                Screen::Modules
                | Screen::Projects
                | Screen::Tickets
                | Screen::Ticket
                | Screen::ActiveAgents
                | Screen::Settings => {}
            }
            assert!(
                LIST_SCREENS.iter().any(|(listed, _)| *listed == screen),
                "{screen:?} is not in LIST_SCREENS"
            );
        }
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
            let expected = terminal_in_panel(panel, &name, jail);
            // Open it as with_running does.
            let db = Db::open(&root.join("nick.db")).unwrap();
            let dir = root.join(last);
            std::fs::create_dir_all(&dir).unwrap();
            let p = db
                .insert_project("auto_scrum", last, dir.to_str().unwrap())
                .unwrap();
            let t = db.insert_ticket(p.id, "NS-1", false, "t").unwrap();
            f.app.project = Some(p);
            f.app.open_agent(
                &dir,
                &t,
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

    use crate::app::TicketDetail;
    use crate::db::{Agent, StageChangeRow};
    use crate::docs::Artifacts;
    use crate::tickets::TicketStage;

    fn staged(id: i64, key: &str, title: &str, stage: &str, hidden: bool) -> Ticket {
        Ticket {
            stage: stage.into(),
            ..ticket(id, key, title, false, hidden)
        }
    }

    fn change(from: &str, to: &str, at: &str) -> StageChangeRow {
        StageChangeRow {
            from_stage: from.into(),
            to_stage: to.into(),
            changed_at: at.into(),
        }
    }

    fn agent(id: i64, stage: &str, name: &str, state: &str) -> Agent {
        Agent {
            id,
            ticket_id: 1,
            stage: stage.into(),
            session_id: format!("s-{id}"),
            name: name.into(),
            opened_at: "2026-11-19T00:00:00Z".into(),
            state: state.into(),
        }
    }

    /// The ticket screen of NS-12 (in Dev) of meu-proj, with `changes` and `agents`.
    fn on_ticket(f: &mut Fixture, changes: Vec<StageChangeRow>, agents: Vec<Agent>) {
        on_tickets(f);
        f.app.screen = Screen::Ticket;
        f.app.ticket_detail = Some(TicketDetail {
            ticket: staged(1, "NS-12", "Título do ticket", "development", false),
            changes,
            agents,
            artifacts: Artifacts {
                plan: true,
                review: false,
            },
        });
    }

    #[test]
    fn the_list_shows_the_stage_after_the_id_and_done_in_green() {
        let mut f = fixture();
        on_tickets(&mut f);
        f.app.show_hidden = true;
        f.app.ticket_rows = vec![
            staged(1, "NS-1", "Um", "tech_leader", false),
            staged(2, "NS-2", "Dois", "done", false),
            staged(3, "NS-3", "Três", "done", true),
            staged(4, "NS-4", "Quatro", "archived", false),
        ];
        let buffer = render(&f.app);
        assert_has(&buffer, "NS-1  [Tech Leader]  Um");
        assert_has(&buffer, "NS-2  [✓ Concluído]  Dois");
        assert_has(&buffer, "NS-3  [✓ Concluído]  Três  [oculto]");
        assert_has(&buffer, "NS-4  [archived]  Quatro");
        let (y, x) = find(&buffer, "[✓ Concluído]  Dois").unwrap();
        assert_eq!(buffer[(x + 1, y)].fg, Color::Green);
        let (y, x) = find(&buffer, "[Tech Leader]").unwrap();
        assert_ne!(buffer[(x + 1, y)].fg, Color::Green);
    }

    #[test]
    fn the_ticket_screen_shows_the_stage_the_artifacts_the_history_and_the_help() {
        let mut f = fixture();
        on_ticket(&mut f, Vec::new(), Vec::new());
        let buffer = render(&f.app);
        assert_has(&buffer, " auto_scrum > meu-proj > NS-12 ");
        assert_has(&buffer, "NS-12  Título do ticket");
        assert_has(&buffer, "Etapa atual: Dev");
        assert_has(&buffer, "Plano (existe):  .nick/docs/NS-12/plan.md");
        // The path from docs.rs: the name of the artifact is also the one of a template, which
        // a structural test of the NS-35 keeps out of every other source.
        assert_has(
            &buffer,
            &format!("Review (não existe):  {}", docs::review_path("NS-12")),
        );
        assert_has(&buffer, "Histórico de etapas");
        assert_has(&buffer, "Nenhuma mudança: o ticket começou em PO.");
        assert_has(&buffer, "Agentes do ticket");
        assert_has(&buffer, "Nenhum agente aberto para este ticket.");
        assert_eq!(
            last_line(&buffer).trim(),
            "↑/↓ mover · Enter abrir agente · m mudar etapa · i editar ID · g novo agente · \
             Esc voltar · q sair"
        );
        // A history, oldest first, with the date as AAAA-MM-DD HH:MM UTC (raw when odd).
        on_ticket(
            &mut f,
            vec![
                change("po", "tech_leader", "2026-11-19T21:46:05Z"),
                change("tech_leader", "development", "ontem"),
            ],
            Vec::new(),
        );
        let buffer = render(&f.app);
        let (first, _) = find(&buffer, "2026-11-19 21:46 UTC  PO → Tech Leader").unwrap();
        let (second, _) = find(&buffer, "ontem  Tech Leader → Dev").unwrap();
        assert!(first < second);
        assert!(find(&buffer, "Nenhuma mudança").is_none());
        // Done, hidden and provisional, with the marks of the list.
        let mut t = staged(1, "abcd1234", "Fim", "done", true);
        t.is_provisional = true;
        f.app.ticket_detail.as_mut().unwrap().ticket = t;
        let buffer = render(&f.app);
        assert_has(&buffer, "abcd1234  Fim  [provisório]  [oculto]");
        assert_has(&buffer, "Etapa atual: [✓ Concluído]");
        let (y, x) = find(&buffer, "[✓ Concluído]").unwrap();
        assert_eq!(buffer[(x + 1, y)].fg, Color::Green);
    }

    #[test]
    fn a_long_history_shows_the_newest_and_how_many_are_older() {
        let mut f = fixture();
        let changes: Vec<StageChangeRow> = (0..30)
            .map(|i| {
                let (from, to) = if i % 2 == 0 {
                    ("po", "review")
                } else {
                    ("review", "po")
                };
                change(from, to, &format!("2026-11-19T10:{i:02}:00Z"))
            })
            .collect();
        on_ticket(
            &mut f,
            changes,
            vec![agent(9, "po", "meu-proj · PO · NS-12", "ended")],
        );
        let buffer = render(&f.app);
        assert_has(&buffer, "2026-11-19 10:29 UTC  Review → PO");
        assert!(
            find(&buffer, "2026-11-19 10:00 UTC").is_none(),
            "the oldest go"
        );
        let screen = text(&buffer);
        let shown = screen.matches("2026-11-19 10:").count();
        assert_has(&buffer, &format!("(+{} mudanças mais antigas)", 30 - shown));
        let (cut, _) = find(&buffer, "mudanças mais antigas").unwrap();
        let (newest, _) = find(&buffer, "10:29").unwrap();
        assert!(cut < newest, "the count on top of the section");
        assert_has(&buffer, "meu-proj · PO · NS-12  encerrado");
    }

    #[test]
    fn the_agents_of_the_ticket_show_their_state_and_the_selection() {
        let (mut f, root) = with_running(&["p"]);
        f.app.focus = Focus::List;
        let db = Db::open(&root.join("nick.db")).unwrap();
        let mut agents = db.list_ticket_agents(1).unwrap();
        assert_eq!(agents.len(), 1);
        agents.push(agent(900, "po", "p · PO · NS-1", "active"));
        agents.push(agent(901, "tech_leader", "p · Tech Leader · NS-1", "ended"));
        f.app.screen = Screen::Ticket;
        f.app.ticket_detail = Some(TicketDetail {
            ticket: staged(1, "NS-1", "t", "po", false),
            changes: Vec::new(),
            agents,
            artifacts: Artifacts::default(),
        });
        f.app.ticket_agent_index = 1;
        let buffer = render_at(&f.app, 140, 40, None);
        assert_has(&buffer, "p · Dev · NS-1  ● vivo");
        assert_has(&buffer, "> p · PO · NS-1  ativo em outro nick");
        assert_has(&buffer, "p · Tech Leader · NS-1  encerrado");
        // Beside the agent, the screen is on the left (40 columns), and the help has the view.
        let areas = layout(&f.app, Rect::new(0, 0, 140, 40));
        assert_eq!(areas.list.width, LIST_WIDTH);
        // The state of each artifact comes before its path, so the 40 columns keep it (review of
        // the NS-36, P3.1): only the end of the path is cut.
        assert_has(&buffer, "│Plano (não existe):  .nick/docs/NS-1/");
        assert_has(&buffer, "│Review (não existe):  .nick/docs/NS-1/");
        let help = help_text(&f.app);
        assert!(
            help.ends_with("Esc voltar · v agentes ativos (1) · q sair"),
            "{help}"
        );
        // Narrow terminals do not panic.
        for (w, h) in [(40, 12), (20, 6), (10, 3), (3, 3)] {
            render_at(&f.app, w, h, None);
        }
        let mut f = fixture();
        on_ticket(&mut f, vec![change("po", "done", "x")], Vec::new());
        for (w, h) in [(30, 8), (12, 4), (2, 2)] {
            render_at(&f.app, w, h, None);
        }
    }

    #[test]
    fn the_dialogs_of_the_stage_and_of_the_id() {
        let mut f = fixture();
        on_ticket(&mut f, Vec::new(), Vec::new());
        let t = f.app.ticket_detail.as_ref().unwrap().ticket.clone();
        f.app.dialog = Some(Dialog::ChooseTicketStage {
            ticket: t.clone(),
            index: 3,
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Mudar etapa ");
        assert_has(&buffer, "Ticket NS-12 — Título do ticket");
        assert_has(&buffer, "  PO");
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
}
