//! The panel's state machine: which side has the focus, which session is selected and the
//! quit confirmation. Pure: it never touches a PTY; it turns input into an [`Action`] that
//! `main.rs` carries out, so every rule is unit tested with fake sessions.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

use crate::keys::{self, TermModes};

/// Lines scrolled by one mouse wheel step when the wheel scrolls the panel's scrollback.
pub const WHEEL_LINES: i32 = 3;

/// Where the keys go.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Focus {
    /// The session list: arrows pick a session, Enter enters it, q quits.
    Panel,
    /// The selected session's terminal: every key goes to its PTY but the reserved shortcut.
    Terminal,
}

/// What `main.rs` has to do after an input.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Action {
    /// Nothing outside the state (which may have changed: redraw anyway).
    None,
    /// Write these bytes to the selected session's PTY.
    Send(Vec<u8>),
    /// Scroll the selected session's scrollback one page up (into the history) or down.
    ScrollPageUp,
    ScrollPageDown,
    /// Scroll the selected session's scrollback by lines (positive: up, into the history).
    ScrollLines(i32),
    /// Terminate every session and leave.
    Quit,
}

/// What the state machine needs to know about a session.
pub trait SessionState {
    fn is_running(&self) -> bool;
}

/// The panel's state.
#[derive(Debug)]
pub struct App {
    pub focus: Focus,
    pub selected: usize,
    session_count: usize,
    /// While the quit confirmation is open: how many sessions were running when it opened.
    pub quit_prompt: Option<usize>,
}

impl App {
    pub fn new(session_count: usize) -> Self {
        App {
            focus: Focus::Panel,
            selected: 0,
            session_count,
            quit_prompt: None,
        }
    }

    /// Handles a key press (releases are ignored).
    pub fn handle_key<S: SessionState>(
        &mut self,
        event: &KeyEvent,
        sessions: &[S],
        modes: TermModes,
    ) -> Action {
        if event.kind == KeyEventKind::Release {
            return Action::None;
        }
        if self.quit_prompt.is_some() {
            // The nick's (s/N): only s/S confirms, any other key cancels.
            self.quit_prompt = None;
            let plain = !event
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
            return match event.code {
                KeyCode::Char('s' | 'S') if plain => Action::Quit,
                _ => Action::None,
            };
        }
        match self.focus {
            Focus::Terminal if keys::is_reserved(event) => {
                self.focus = Focus::Panel;
                Action::None
            }
            Focus::Terminal => keys::encode_key(event, modes).map_or(Action::None, Action::Send),
            Focus::Panel => self.panel_key(event, sessions),
        }
    }

    fn panel_key<S: SessionState>(&mut self, event: &KeyEvent, sessions: &[S]) -> Action {
        let shift = event.modifiers.contains(KeyModifiers::SHIFT);
        match event.code {
            KeyCode::Down | KeyCode::Char('j') if self.session_count > 0 => {
                self.selected = (self.selected + 1) % self.session_count;
                Action::None
            }
            KeyCode::Up | KeyCode::Char('k') if self.session_count > 0 => {
                self.selected = (self.selected + self.session_count - 1) % self.session_count;
                Action::None
            }
            KeyCode::Enter => {
                self.focus = Focus::Terminal;
                Action::None
            }
            KeyCode::PageUp if shift => Action::ScrollPageUp,
            KeyCode::PageDown if shift => Action::ScrollPageDown,
            KeyCode::Char('q') => {
                let running = sessions.iter().filter(|s| s.is_running()).count();
                if running == 0 {
                    Action::Quit
                } else {
                    self.quit_prompt = Some(running);
                    Action::None
                }
            }
            _ => Action::None,
        }
    }

    /// Handles a mouse event over the terminal's inner `area`: with the focus on the terminal
    /// and the program asking for the mouse, it goes to the program in its encoding; otherwise
    /// the wheel scrolls the panel's scrollback and the rest is ignored.
    pub fn handle_mouse(
        &mut self,
        event: &MouseEvent,
        mode: MouseProtocolMode,
        encoding: MouseProtocolEncoding,
        area: Rect,
    ) -> Action {
        if self.focus == Focus::Terminal && mode != MouseProtocolMode::None {
            return keys::encode_mouse(event, mode, encoding, area)
                .map_or(Action::None, Action::Send);
        }
        match event.kind {
            MouseEventKind::ScrollUp => Action::ScrollLines(WHEEL_LINES),
            MouseEventKind::ScrollDown => Action::ScrollLines(-WHEEL_LINES),
            _ => Action::None,
        }
    }

    /// Handles a paste: it goes to the terminal only while it has the focus.
    pub fn handle_paste(&self, text: &str, bracketed: bool) -> Action {
        match self.focus {
            Focus::Terminal => Action::Send(keys::encode_paste(text, bracketed)),
            Focus::Panel => Action::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventState, MouseButton};

    struct Fake(bool);

    impl SessionState for Fake {
        fn is_running(&self) -> bool {
            self.0
        }
    }

    const BOTH_RUNNING: [Fake; 2] = [Fake(true), Fake(true)];
    const NONE_RUNNING: [Fake; 2] = [Fake(false), Fake(false)];

    fn press(app: &mut App, code: KeyCode, sessions: &[Fake]) -> Action {
        press_with(app, code, KeyModifiers::NONE, sessions)
    }

    fn press_with(
        app: &mut App,
        code: KeyCode,
        modifiers: KeyModifiers,
        sessions: &[Fake],
    ) -> Action {
        app.handle_key(
            &KeyEvent::new(code, modifiers),
            sessions,
            TermModes::default(),
        )
    }

    fn reserved(app: &mut App, sessions: &[Fake]) -> Action {
        press_with(app, KeyCode::Char('4'), KeyModifiers::CONTROL, sessions)
    }

    #[test]
    fn starts_on_the_panel_with_the_first_session() {
        let app = App::new(2);
        assert_eq!(app.focus, Focus::Panel);
        assert_eq!(app.selected, 0);
        assert_eq!(app.quit_prompt, None);
    }

    #[test]
    fn arrows_and_jk_cycle_the_selection() {
        let mut app = App::new(3);
        let s = [Fake(true), Fake(true), Fake(true)];
        assert_eq!(press(&mut app, KeyCode::Down, &s), Action::None);
        assert_eq!(app.selected, 1);
        press(&mut app, KeyCode::Char('j'), &s);
        assert_eq!(app.selected, 2);
        press(&mut app, KeyCode::Down, &s);
        assert_eq!(app.selected, 0, "down from the last wraps to the first");
        press(&mut app, KeyCode::Up, &s);
        assert_eq!(app.selected, 2, "up from the first wraps to the last");
        press(&mut app, KeyCode::Char('k'), &s);
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn changing_the_selection_never_ends_a_session() {
        let mut app = App::new(2);
        for code in [
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::Char('j'),
            KeyCode::Char('k'),
        ] {
            let action = press(&mut app, code, &BOTH_RUNNING);
            assert_eq!(action, Action::None, "{code:?}");
            assert_eq!(app.quit_prompt, None);
        }
    }

    #[test]
    fn enter_focuses_the_terminal_and_the_reserved_key_comes_back() {
        let mut app = App::new(2);
        assert_eq!(press(&mut app, KeyCode::Enter, &BOTH_RUNNING), Action::None);
        assert_eq!(app.focus, Focus::Terminal);
        assert_eq!(reserved(&mut app, &BOTH_RUNNING), Action::None);
        assert_eq!(app.focus, Focus::Panel);
        assert_eq!(app.quit_prompt, None, "the reserved key never asks to quit");
        // Also as the kitty protocol reports it.
        press(&mut app, KeyCode::Enter, &BOTH_RUNNING);
        press_with(
            &mut app,
            KeyCode::Char('\\'),
            KeyModifiers::CONTROL,
            &BOTH_RUNNING,
        );
        assert_eq!(app.focus, Focus::Panel);
    }

    #[test]
    fn in_the_terminal_q_ctrl_c_and_esc_are_sent() {
        let mut app = App::new(2);
        press(&mut app, KeyCode::Enter, &BOTH_RUNNING);
        assert_eq!(
            press(&mut app, KeyCode::Char('q'), &BOTH_RUNNING),
            Action::Send(b"q".to_vec())
        );
        assert_eq!(
            press_with(
                &mut app,
                KeyCode::Char('c'),
                KeyModifiers::CONTROL,
                &BOTH_RUNNING
            ),
            Action::Send(vec![0x03])
        );
        assert_eq!(
            press(&mut app, KeyCode::Esc, &BOTH_RUNNING),
            Action::Send(vec![0x1b])
        );
        assert_eq!(
            press(&mut app, KeyCode::Enter, &BOTH_RUNNING),
            Action::Send(b"\r".to_vec())
        );
        assert_eq!(
            press(&mut app, KeyCode::Down, &BOTH_RUNNING),
            Action::Send(b"\x1b[B".to_vec())
        );
        assert_eq!(app.focus, Focus::Terminal);
        assert_eq!(
            app.selected, 0,
            "arrows in the terminal don't change the session"
        );
        assert_eq!(app.quit_prompt, None);
    }

    #[test]
    fn terminal_keys_follow_application_cursor() {
        let mut app = App::new(1);
        let s = [Fake(true)];
        press(&mut app, KeyCode::Enter, &s);
        let action = app.handle_key(
            &KeyEvent::new(KeyCode::Up, KeyModifiers::NONE),
            &s,
            TermModes {
                application_cursor: true,
            },
        );
        assert_eq!(action, Action::Send(b"\x1bOA".to_vec()));
    }

    #[test]
    fn q_with_running_sessions_asks_and_s_quits() {
        for confirm in ['s', 'S'] {
            let mut app = App::new(2);
            let s = [Fake(true), Fake(false)];
            assert_eq!(press(&mut app, KeyCode::Char('q'), &s), Action::None);
            assert_eq!(app.quit_prompt, Some(1));
            assert_eq!(press(&mut app, KeyCode::Char(confirm), &s), Action::Quit);
        }
    }

    #[test]
    fn any_other_key_cancels_the_quit() {
        for cancel in [
            KeyCode::Char('n'),
            KeyCode::Char('N'),
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Char('q'),
            KeyCode::Down,
        ] {
            let mut app = App::new(2);
            press(&mut app, KeyCode::Char('q'), &BOTH_RUNNING);
            assert_eq!(app.quit_prompt, Some(2));
            assert_eq!(
                press(&mut app, cancel, &BOTH_RUNNING),
                Action::None,
                "{cancel:?}"
            );
            assert_eq!(app.quit_prompt, None, "{cancel:?} closes the prompt");
            assert_eq!(app.selected, 0, "{cancel:?} only closes the prompt");
            assert_eq!(app.focus, Focus::Panel);
        }
    }

    #[test]
    fn q_without_running_sessions_quits_at_once() {
        let mut app = App::new(2);
        assert_eq!(
            press(&mut app, KeyCode::Char('q'), &NONE_RUNNING),
            Action::Quit
        );
        assert_eq!(app.quit_prompt, None);
    }

    #[test]
    fn shift_page_keys_scroll_on_the_panel() {
        let mut app = App::new(2);
        assert_eq!(
            press_with(
                &mut app,
                KeyCode::PageUp,
                KeyModifiers::SHIFT,
                &BOTH_RUNNING
            ),
            Action::ScrollPageUp
        );
        assert_eq!(
            press_with(
                &mut app,
                KeyCode::PageDown,
                KeyModifiers::SHIFT,
                &BOTH_RUNNING
            ),
            Action::ScrollPageDown
        );
    }

    #[test]
    fn releases_are_ignored() {
        let mut app = App::new(2);
        let release = KeyEvent {
            code: KeyCode::Enter,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(
            app.handle_key(&release, &BOTH_RUNNING, TermModes::default()),
            Action::None
        );
        assert_eq!(app.focus, Focus::Panel);
    }

    const AREA: Rect = Rect {
        x: 31,
        y: 1,
        width: 80,
        height: 20,
    };

    fn mouse(kind: MouseEventKind) -> MouseEvent {
        MouseEvent {
            kind,
            column: 40,
            row: 5,
            modifiers: KeyModifiers::NONE,
        }
    }

    #[test]
    fn wheel_goes_to_the_program_only_when_it_asked_for_the_mouse() {
        let mut app = App::new(1);
        press(&mut app, KeyCode::Enter, &[Fake(true)]);
        let up = mouse(MouseEventKind::ScrollUp);
        assert_eq!(
            app.handle_mouse(
                &up,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Sgr,
                AREA
            ),
            Action::Send(b"\x1b[<64;10;5M".to_vec())
        );
        assert_eq!(
            app.handle_mouse(
                &up,
                MouseProtocolMode::None,
                MouseProtocolEncoding::Sgr,
                AREA
            ),
            Action::ScrollLines(WHEEL_LINES)
        );
        let down = mouse(MouseEventKind::ScrollDown);
        assert_eq!(
            app.handle_mouse(
                &down,
                MouseProtocolMode::None,
                MouseProtocolEncoding::Sgr,
                AREA
            ),
            Action::ScrollLines(-WHEEL_LINES)
        );
    }

    #[test]
    fn on_the_panel_the_wheel_scrolls_and_clicks_are_not_sent() {
        let mut app = App::new(1);
        let mode = MouseProtocolMode::PressRelease;
        let enc = MouseProtocolEncoding::Sgr;
        assert_eq!(
            app.handle_mouse(&mouse(MouseEventKind::ScrollUp), mode, enc, AREA),
            Action::ScrollLines(WHEEL_LINES)
        );
        assert_eq!(
            app.handle_mouse(
                &mouse(MouseEventKind::Down(MouseButton::Left)),
                mode,
                enc,
                AREA
            ),
            Action::None
        );
    }

    #[test]
    fn paste_goes_to_the_terminal_only_with_the_focus() {
        let mut app = App::new(1);
        assert_eq!(app.handle_paste("x", true), Action::None);
        press(&mut app, KeyCode::Enter, &[Fake(true)]);
        assert_eq!(
            app.handle_paste("x", true),
            Action::Send(b"\x1b[200~x\x1b[201~".to_vec())
        );
        assert_eq!(app.handle_paste("x", false), Action::Send(b"x".to_vec()));
    }
}
