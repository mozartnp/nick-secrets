//! Drawing the TUI with ratatui, with no business rule: everything comes from the `App` state
//! (and, on the agent screen, from the vt100 screen of the agent's terminal, drawn with
//! tui-term). The texts are in PT-BR; every text that comes from the operator or the system
//! goes through `paths::printable` (control characters become `?`).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::agents::Stage;
use crate::app::{App, Button, Dialog, Screen, TicketField};
use crate::modules::ModuleKind;
use crate::paths::printable;
use crate::projects;

/// Draws the whole screen without an agent's terminal (see `draw_with_terminal`).
pub fn draw(frame: &mut Frame, app: &App) {
    draw_with_terminal(frame, app, None);
}

/// Draws the whole screen: on the agent screen, the header and `terminal` (the agent's vt100
/// screen); otherwise the list, then the status line and the help line of the current screen,
/// and the open dialog over the list. The status and the help take the rows they need, word
/// wrapped (up to a third of the screen each), so a long one is not cut.
pub fn draw_with_terminal(frame: &mut Frame, app: &App, terminal: Option<&vt100::Screen>) {
    if app.screen == Screen::Agent {
        if let Some(agent) = app.agent() {
            draw_agent(frame, &agent.name, agent.jail, terminal);
        }
        return;
    }
    let area = frame.area();
    let status_lines = app.status.as_deref().map_or_else(
        || vec![String::new()],
        |message| wrap_words(&printable(message), area.width.max(1)),
    );
    let help_lines = wrap_words(help_text(app), area.width.max(1));
    let rows = |lines: &[String]| {
        u16::try_from(lines.len())
            .unwrap_or(u16::MAX)
            .min((area.height / 3).max(1))
    };
    let [body, status, help] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(rows(&status_lines)),
        Constraint::Length(rows(&help_lines)),
    ])
    .areas(area);

    match app.screen {
        Screen::Modules => draw_modules(frame, app, body),
        Screen::Projects => draw_projects(frame, app, body),
        Screen::Tickets => draw_tickets(frame, app, body),
        Screen::Agent => {}
    }
    if app.status.is_some() {
        let lines: Vec<Line> = status_lines.into_iter().map(Line::from).collect();
        frame.render_widget(Paragraph::new(lines), status);
    }
    let help_lines: Vec<Line> = help_lines.into_iter().map(Line::from).collect();
    frame.render_widget(
        Paragraph::new(help_lines).style(Style::new().add_modifier(Modifier::DIM)),
        help,
    );
    if let Some(dialog) = &app.dialog {
        draw_dialog(frame, dialog, body);
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

/// The keys of the current screen, in the help line.
fn help_text(app: &App) -> &'static str {
    match (&app.dialog, app.screen) {
        (Some(Dialog::AddPath { .. }), _) => "Enter confirmar · Esc cancelar",
        (Some(Dialog::NeedsConfig { .. }), _) => "←/→/Tab escolher · Enter acionar · Esc cancelar",
        (Some(Dialog::ConfirmRemove { .. }), _) => "s remover · qualquer outra tecla cancela",
        (Some(Dialog::NewTicket { .. }), _) => {
            "Tab trocar de campo · Enter confirmar · Esc cancelar"
        }
        (Some(Dialog::EditTitle { .. }), _) => "Enter confirmar · Esc cancelar",
        (Some(Dialog::ConfirmHide { .. }), _) => "s ocultar · qualquer outra tecla cancela",
        (Some(Dialog::ChooseStage { .. }), _) => "↑/↓ escolher · Enter abrir · Esc cancelar",
        (Some(Dialog::AgentError { .. }), _) => "Enter/Esc fechar",
        (None, Screen::Tickets) if app.show_hidden => {
            "↑/↓ mover · a novo · e editar título · o ocultar/restaurar · f esconder ocultos · \
             g abrir agente · Esc voltar · q sair"
        }
        (None, Screen::Tickets) => {
            "↑/↓ mover · a novo · e editar título · o ocultar/restaurar · f mostrar ocultos · \
             g abrir agente · Esc voltar · q sair"
        }
        // The agent screen has no help line: the whole screen is the terminal.
        (None, Screen::Agent) => "",
        (None, Screen::Modules) => "↑/↓ mover · Enter abrir · q sair",
        (None, Screen::Projects) => "↑/↓ mover · a adicionar · r remover · Esc voltar · q sair",
    }
}

/// The area of the agent's terminal inside the screen `area`: everything below the header
/// row. The PTY gets this size (at least 2x2: agent_session.rs).
pub fn agent_area(area: Rect) -> Rect {
    Rect {
        y: area.y + area.height.min(1),
        height: area.height.saturating_sub(1),
        ..area
    }
}

/// The agent screen: one header row (the name, and whether it runs in the jail; out of it, the
/// text of `jail_print_status` off, in red and bold) and the terminal of the agent below it.
pub fn draw_agent(frame: &mut Frame, name: &str, jail: bool, terminal: Option<&vt100::Screen>) {
    let area = frame.area();
    let header = Rect {
        height: area.height.min(1),
        ..area
    };
    let mut spans = vec![Span::raw(format!("{} · ", printable(name)))];
    if jail {
        spans.push(Span::raw("na jail"));
    } else {
        spans.push(Span::styled(
            "FORA DA JAIL: o agente roda direto no seu shell",
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
        spans.push(Span::raw("."));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), header);
    if let Some(screen) = terminal {
        // The program's cursor, hidden while the history is scrolled (it is not on screen).
        let mut cursor = Cursor::default();
        if screen.scrollback() > 0 || screen.hide_cursor() {
            cursor.hide();
        }
        frame.render_widget(PseudoTerminal::new(screen).cursor(cursor), agent_area(area));
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
            let mut spans = vec![
                Span::raw(printable(&ticket.key)),
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

fn draw_dialog(frame: &mut Frame, dialog: &Dialog, area: Rect) {
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
        Dialog::ChooseStage { ticket, index, .. } => {
            let mut lines = vec![
                Line::from(format!(
                    "Ticket {} — {}",
                    printable(&ticket.key),
                    printable(&ticket.title)
                )),
                Line::from(""),
            ];
            for (i, stage) in Stage::ALL.iter().enumerate() {
                lines.push(if i == *index {
                    Line::from(vec![
                        Span::raw("> "),
                        Span::styled(stage.label(), Style::new().add_modifier(Modifier::REVERSED)),
                    ])
                } else {
                    Line::from(format!("  {}", stage.label()))
                });
            }
            (" Abrir agente ", lines, None)
        }
        Dialog::AgentError { message } => (
            " Agente não aberto ",
            message.lines().map(|l| Line::from(printable(l))).collect(),
            None,
        ),
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Button, Dialog, Row, Screen, TicketField};
    use crate::db::Db;
    use crate::projects::{Availability, Project};
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
            "↑/↓ mover · Enter abrir · q sair"
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
        assert_has(&buffer, "NS-1  Corrigir o login");
        assert_has(&buffer, "3f9a0c7e  Sem ID ainda  [provisório]");
        let (y, _) = find(&buffer, "NS-1").unwrap();
        assert!(!text(&buffer).lines().nth(y as usize).unwrap().contains('['));
        assert!(find(&buffer, "(com ocultos)").is_none());
        // With `g abrir agente` (NS-33) the help passes 100 columns and wraps, as with the filter.
        let lines: Vec<String> = text(&buffer).lines().map(str::to_string).collect();
        let n = lines.len();
        assert_eq!(
            format!("{} {}", lines[n - 2].trim(), lines[n - 1].trim()),
            "↑/↓ mover · a novo · e editar título · o ocultar/restaurar · f mostrar ocultos · \
             g abrir agente · Esc voltar · q sair"
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
        assert_has(&buffer, "NS-2  Escondido  [oculto]");
        assert_has(
            &buffer,
            "abcd1234  Provisório escondido  [provisório]  [oculto]",
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
             g abrir agente · Esc voltar · q sair"
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
        assert_has(&buffer, "NS-1  a?[2Jb");
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
            .draw(|frame| draw_agent(frame, name, jail, screen))
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
        assert_eq!(
            agent_area(Rect::new(0, 0, 100, 30)),
            Rect::new(0, 1, 100, 29)
        );
        assert_eq!(agent_area(Rect::new(0, 0, 10, 1)).height, 0);
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
    fn the_agent_screen_of_the_app_draws_the_running_agent_header() {
        // Without an agent (never on screen in practice), only the header row is blank.
        let mut f = fixture();
        f.app.screen = Screen::Agent;
        let buffer = render(&f.app);
        assert_eq!(text(&buffer).trim(), "");
    }

    #[test]
    fn the_stage_dialog_lists_the_four_stages_with_the_selection() {
        let mut f = fixture();
        on_tickets(&mut f);
        let t = ticket(1, "NS-1", "Corrigir o login", false, false);
        f.app.ticket_rows = vec![t.clone()];
        f.app.dialog = Some(Dialog::ChooseStage {
            project: PathBuf::from("/x/meu-proj"),
            ticket: t,
            index: 2,
        });
        let buffer = render(&f.app);
        assert_has(&buffer, " Abrir agente ");
        assert_has(&buffer, "Ticket NS-1 — Corrigir o login");
        let (po, _) = find(&buffer, "  PO").unwrap();
        let (tl, _) = find(&buffer, "  Tech Leader").unwrap();
        let (dev, x) = find(&buffer, "> Dev").unwrap();
        let (review, _) = find(&buffer, "  Review").unwrap();
        assert!(po < tl && tl < dev && dev < review);
        assert!(buffer[(x + 2, dev)].modifier.contains(Modifier::REVERSED));
        assert_eq!(
            last_line(&buffer).trim(),
            "↑/↓ escolher · Enter abrir · Esc cancelar"
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
}
