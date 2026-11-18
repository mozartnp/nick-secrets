//! Drawing the TUI with ratatui, with no business rule: everything comes from the `App` state.
//! The texts are in PT-BR; every text that comes from the operator or the system goes through
//! `paths::printable` (control characters become `?`).

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{App, Button, Dialog, Screen};
use crate::modules::ModuleKind;
use crate::paths::printable;
use crate::projects::{self, Availability};

/// Draws the whole screen: the list, then the status line and the help line of the current
/// screen, and the open dialog over the list.
pub fn draw(frame: &mut Frame, app: &App) {
    let [body, status, help] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    match app.screen {
        Screen::Modules => draw_modules(frame, app, body),
        Screen::Projects => draw_projects(frame, app, body),
    }
    if let Some(message) = &app.status {
        frame.render_widget(Paragraph::new(printable(message)), status);
    }
    frame.render_widget(
        Paragraph::new(help_text(app)).style(Style::new().add_modifier(Modifier::DIM)),
        help,
    );
    if let Some(dialog) = &app.dialog {
        draw_dialog(frame, dialog, body);
    }
}

/// The keys of the current screen, in the help line.
fn help_text(app: &App) -> &'static str {
    match (&app.dialog, app.screen) {
        (Some(Dialog::AddPath { .. }), _) => "Enter confirmar · Esc cancelar",
        (Some(Dialog::NeedsConfig { .. }), _) => "←/→/Tab escolher · Enter acionar · Esc cancelar",
        (Some(Dialog::ConfirmRemove { .. }), _) => "s remover · qualquer outra tecla cancela",
        (None, Screen::Modules) => "↑/↓ mover · Enter abrir · q sair",
        (None, Screen::Projects) => "↑/↓ mover · a adicionar · r remover · Esc voltar · q sair",
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

/// The mark of an unavailable project.
fn unavailable_mark(availability: Availability) -> Option<&'static str> {
    match availability {
        Availability::Available => None,
        Availability::DirMissing => Some("[indisponível: pasta não encontrada]"),
        Availability::ConfigMissing => Some("[indisponível: sem .nick/nick.conf]"),
    }
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
            if let Some(mark) = unavailable_mark(row.availability) {
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

fn draw_dialog(frame: &mut Frame, dialog: &Dialog, area: Rect) {
    let (title, lines, cursor) = match dialog {
        Dialog::AddPath { input } => {
            // One line, scrolled to the end (no wrap), so the cursor always shows.
            let inner_width = dialog_width(area).saturating_sub(2);
            let typed = input_tail(&printable(input), inner_width);
            let cursor = typed.chars().count();
            (
                " Adicionar projeto ",
                vec![
                    Line::from("Caminho da pasta do projeto:"),
                    Line::from(typed),
                ],
                Some(cursor),
            )
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
        Dialog::ConfirmRemove { project } => (
            " Remover projeto ",
            vec![Line::from(format!(
                "Remover o projeto {} ({}) do nick? Os arquivos do projeto não são tocados. \
                 (s/N)",
                printable(&project.name),
                printable(&project.path.to_string_lossy())
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
    if let Some(column) = cursor {
        // The cursor after the typed text (2nd line), as long as it fits in the box.
        let column = u16::try_from(column).unwrap_or(u16::MAX);
        if column < inner.width && inner.height > 1 {
            frame.set_cursor_position(Position::new(inner.x + column, inner.y + 1));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{Button, Dialog, Row, Screen};
    use crate::db::Db;
    use crate::projects::{Availability, Project};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui::buffer::Buffer;
    use ratatui::style::Modifier;
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
}
