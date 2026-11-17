//! Drawing of the panel: the session list on the left, the selected session's terminal on the
//! right (tui-term's PseudoTerminal over the session's vt100 screen), a one-line footer with
//! the shortcuts of the current focus and, while it is open, the quit confirmation.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use tui_term::widget::{Cursor, PseudoTerminal};

use crate::app::{App, Focus};
use crate::keys::RESERVED_KEY_LABEL;
use crate::session::Status;

/// Width of the session list.
pub const LIST_WIDTH: u16 = 30;

/// The panel's areas for a given screen size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Areas {
    pub list: Rect,
    /// The terminal with its border.
    pub terminal: Rect,
    /// Inside the border: the size of every session's PTY and parser.
    pub inner: Rect,
    pub footer: Rect,
}

/// What the list shows about a session.
#[derive(Clone, Copy, Debug)]
pub struct SessionView<'a> {
    pub label: &'a str,
    pub status: Status,
}

pub fn layout(area: Rect) -> Areas {
    let [body, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(area);
    let [list, terminal] =
        Layout::horizontal([Constraint::Length(LIST_WIDTH), Constraint::Min(1)]).areas(body);
    let inner = Block::default().borders(Borders::ALL).inner(terminal);
    Areas {
        list,
        terminal,
        inner,
        footer,
    }
}

pub fn status_text(status: Status) -> String {
    match status {
        Status::Running => "rodando".to_string(),
        Status::Exited(code) => format!("encerrada (código {code})"),
    }
}

pub fn footer_text(focus: Focus) -> String {
    match focus {
        Focus::Panel => format!(
            "↑/↓ j/k: sessão | Enter: entrar no terminal ({RESERVED_KEY_LABEL} volta) | Shift+PgUp/PgDn ou roda: rolar | q: sair"
        ),
        Focus::Terminal => format!(
            "Terminal: as teclas vão para o claude | {RESERVED_KEY_LABEL}: voltar ao painel (a sessão continua)"
        ),
    }
}

pub fn quit_prompt_text(running: usize) -> String {
    format!("Encerrar {running} sessão(ões) ativa(s)? (s/N)")
}

/// Greedy word wrap of `text` to `width` columns (a word longer than that stays whole: the
/// widget cuts it).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split(' ').filter(|w| !w.is_empty()) {
        match lines.last_mut() {
            Some(line) if line.chars().count() + 1 + word.chars().count() <= width => {
                line.push(' ');
                line.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

fn border_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    }
}

/// Draws the whole panel. `screen` is the selected session's screen.
pub fn draw(
    frame: &mut Frame,
    app: &App,
    sessions: &[SessionView],
    screen: Option<&vt100::Screen>,
) {
    let areas = layout(frame.area());

    let list_block = Block::default()
        .borders(Borders::ALL)
        .title(" Sessões ")
        .border_style(border_style(app.focus == Focus::Panel));
    let text_width = usize::from(list_block.inner(areas.list).width.saturating_sub(2)).max(1);
    let mut lines: Vec<Line> = Vec::new();
    for (i, session) in sessions.iter().enumerate() {
        let selected = i == app.selected;
        let label_style = if selected {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        for (n, part) in wrap(session.label, text_width).into_iter().enumerate() {
            let marker = if selected && n == 0 { "▶ " } else { "  " };
            lines.push(Line::from(vec![
                Span::styled(marker, Style::default().fg(Color::Yellow)),
                Span::styled(part, label_style),
            ]));
        }
        let status_color = match session.status {
            Status::Running => Color::Green,
            Status::Exited(_) => Color::Red,
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                status_text(session.status),
                Style::default().fg(status_color),
            ),
        ]));
        lines.push(Line::default());
    }
    frame.render_widget(Paragraph::new(lines).block(list_block), areas.list);

    let label = sessions.get(app.selected).map_or("", |s| s.label);
    let scrollback = screen.map_or(0, vt100::Screen::scrollback);
    let mut title = format!(" {label} ");
    if scrollback > 0 {
        title.push_str(&format!("[rolagem: {scrollback} linhas] "));
    }
    let term_block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(border_style(app.focus == Focus::Terminal));
    match screen {
        Some(screen) => {
            let mut cursor = Cursor::default();
            if app.focus != Focus::Terminal || scrollback > 0 {
                cursor.hide();
            }
            frame.render_widget(
                PseudoTerminal::new(screen).block(term_block).cursor(cursor),
                areas.terminal,
            );
        }
        None => frame.render_widget(term_block, areas.terminal),
    }

    frame.render_widget(
        Paragraph::new(footer_text(app.focus)).style(Style::default().fg(Color::Cyan)),
        areas.footer,
    );

    if let Some(running) = app.quit_prompt {
        let text = quit_prompt_text(running);
        let full = frame.area();
        let width = (text.chars().count() as u16 + 4).min(full.width);
        let height = 3.min(full.height);
        let popup = Rect::new(
            full.x + (full.width - width) / 2,
            full.y + (full.height - height) / 2,
            width,
            height,
        );
        frame.render_widget(Clear, popup);
        frame.render_widget(
            Paragraph::new(text).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red)),
            ),
            popup,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use crate::app::SessionState;
    use crate::keys::TermModes;

    const VIEWS: [SessionView; 2] = [
        SessionView {
            label: "Projeto Demo > NS-0001 > Tech Leader",
            status: Status::Running,
        },
        SessionView {
            label: "Projeto Demo > a1b2c3 > Dev",
            status: Status::Exited(3),
        },
    ];

    struct Running;

    impl SessionState for Running {
        fn is_running(&self) -> bool {
            true
        }
    }

    fn render(app: &App, screen: Option<&vt100::Screen>) -> String {
        let mut terminal = Terminal::new(TestBackend::new(110, 24)).unwrap();
        terminal.draw(|f| draw(f, app, &VIEWS, screen)).unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut out = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                out.push_str(buffer[(x, y)].symbol());
            }
            out.push('\n');
        }
        out
    }

    #[test]
    fn layout_splits_list_terminal_and_footer() {
        let areas = layout(Rect::new(0, 0, 110, 24));
        assert_eq!(areas.list, Rect::new(0, 0, LIST_WIDTH, 23));
        assert_eq!(areas.terminal, Rect::new(LIST_WIDTH, 0, 80, 23));
        assert_eq!(areas.inner, Rect::new(LIST_WIDTH + 1, 1, 78, 21));
        assert_eq!(areas.footer, Rect::new(0, 23, 110, 1));
    }

    #[test]
    fn status_texts() {
        assert_eq!(status_text(Status::Running), "rodando");
        assert_eq!(status_text(Status::Exited(0)), "encerrada (código 0)");
        assert_eq!(status_text(Status::Exited(130)), "encerrada (código 130)");
    }

    #[test]
    fn the_list_shows_every_label_and_state() {
        let out = render(&App::new(2), None);
        for part in [
            "Projeto Demo > NS-0001",
            "Tech Leader",
            "Projeto Demo > a1b2c3",
        ] {
            assert!(out.contains(part), "{part:?} missing:\n{out}");
        }
        assert!(out.contains("rodando"), "{out}");
        assert!(out.contains("encerrada (código 3)"), "{out}");
        assert!(out.contains("▶"), "selection marker:\n{out}");
    }

    #[test]
    fn the_footer_shows_the_reserved_shortcut_in_both_focuses() {
        let mut app = App::new(2);
        assert!(footer_text(Focus::Panel).contains(RESERVED_KEY_LABEL));
        assert!(footer_text(Focus::Terminal).contains(RESERVED_KEY_LABEL));
        let out = render(&app, None);
        assert!(out.contains(&footer_text(Focus::Panel)), "{out}");
        app.handle_key(
            &KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE),
            &[Running, Running],
            TermModes::default(),
        );
        let out = render(&app, None);
        assert!(out.contains(&footer_text(Focus::Terminal)), "{out}");
        assert!(out.contains("Ctrl+\\"), "{out}");
    }

    #[test]
    fn the_quit_prompt_shows_s_n() {
        let mut app = App::new(2);
        app.handle_key(
            &KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE),
            &[Running, Running],
            TermModes::default(),
        );
        assert_eq!(
            quit_prompt_text(2),
            "Encerrar 2 sessão(ões) ativa(s)? (s/N)"
        );
        let out = render(&app, None);
        assert!(
            out.contains("Encerrar 2 sessão(ões) ativa(s)? (s/N)"),
            "{out}"
        );
        assert!(!render(&App::new(2), None).contains("(s/N)"));
    }

    #[test]
    fn the_terminal_shows_the_selected_screen() {
        let mut parser = vt100::Parser::new(21, 78, 0);
        parser.process(b"ola do claude\r\nsegunda linha");
        let out = render(&App::new(2), Some(parser.screen()));
        let lines: Vec<&str> = out.lines().collect();
        assert!(lines[1].contains("ola do claude"), "{out}");
        assert!(lines[2].contains("segunda linha"), "{out}");
        assert!(
            lines[0].contains("Tech Leader"),
            "the border names the session:\n{out}"
        );
    }
}
