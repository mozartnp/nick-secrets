//! Translation of crossterm input (keys, paste, mouse) into the bytes an xterm-like terminal
//! sends to the program running in it. Pure: no PTY, no I/O, so every rule is unit tested.

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use ratatui::layout::Rect;
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

/// The reserved shortcut: it gives the focus back to the panel without touching the session.
/// Ctrl+\ is not bound by Claude Code (code.claude.com/docs/en/keybindings, checked for NS-29);
/// Ctrl+] is (it was avoided on purpose). In raw mode the terminal sends 0x1c, which crossterm
/// reports as Ctrl+4; terminals with the kitty protocol report Ctrl+\ itself.
pub const RESERVED_KEY_LABEL: &str = "Ctrl+\\";

/// The terminal modes of the session that change how a key is encoded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TermModes {
    /// DECCKM: the arrows (and Home/End) are sent as SS3 (ESC O x) instead of CSI (ESC [ x).
    pub application_cursor: bool,
}

/// Whether `event` is the reserved shortcut (see [`RESERVED_KEY_LABEL`]).
pub fn is_reserved(event: &KeyEvent) -> bool {
    event.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(event.code, KeyCode::Char('\\') | KeyCode::Char('4'))
}

/// The xterm modifier parameter (1 + Shift 1 + Alt 2 + Ctrl 4), or `None` without modifiers.
fn modifier_param(modifiers: KeyModifiers) -> Option<u8> {
    let mut param = 1;
    if modifiers.contains(KeyModifiers::SHIFT) {
        param += 1;
    }
    if modifiers.contains(KeyModifiers::ALT) {
        param += 2;
    }
    if modifiers.contains(KeyModifiers::CONTROL) {
        param += 4;
    }
    (param > 1).then_some(param)
}

/// The control byte for Ctrl+`c`, when there is one (letters, and the symbols crossterm reports
/// for 0x00 and 0x1b..0x1f).
fn ctrl_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' | 'A'..='Z' => Some((c.to_ascii_lowercase() as u8) & 0x1f),
        ' ' | '@' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' | '/' => Some(0x1f),
        _ => None,
    }
}

/// A cursor key (arrows, Home, End): CSI or SS3 by DECCKM, CSI 1;m with modifiers.
fn cursor_key(final_byte: char, modifiers: KeyModifiers, modes: TermModes) -> Vec<u8> {
    match modifier_param(modifiers) {
        Some(m) => format!("\x1b[1;{m}{final_byte}"),
        None if modes.application_cursor => format!("\x1bO{final_byte}"),
        None => format!("\x1b[{final_byte}"),
    }
    .into_bytes()
}

/// A key sent as CSI n ~ (CSI n;m ~ with modifiers).
fn tilde_key(number: u8, modifiers: KeyModifiers) -> Vec<u8> {
    match modifier_param(modifiers) {
        Some(m) => format!("\x1b[{number};{m}~"),
        None => format!("\x1b[{number}~"),
    }
    .into_bytes()
}

/// ESC followed by `bytes` when Alt is held, `bytes` otherwise.
fn with_alt(bytes: Vec<u8>, modifiers: KeyModifiers) -> Vec<u8> {
    if modifiers.contains(KeyModifiers::ALT) {
        let mut out = vec![0x1b];
        out.extend(bytes);
        out
    } else {
        bytes
    }
}

/// The bytes for a key press, or `None` when the key sends nothing (a release, an unknown key,
/// or the reserved shortcut, which `app.rs` handles and never reaches the PTY).
pub fn encode_key(event: &KeyEvent, modes: TermModes) -> Option<Vec<u8>> {
    if event.kind == KeyEventKind::Release || is_reserved(event) {
        return None;
    }
    let mods = event.modifiers;
    let bytes = match event.code {
        KeyCode::Char(c) => {
            let ctrl = if mods.contains(KeyModifiers::CONTROL) {
                ctrl_byte(c)
            } else {
                None
            };
            let raw = match ctrl {
                Some(b) => vec![b],
                None => c.to_string().into_bytes(),
            };
            with_alt(raw, mods)
        }
        KeyCode::Enter => with_alt(b"\r".to_vec(), mods),
        KeyCode::Tab => with_alt(b"\t".to_vec(), mods),
        KeyCode::BackTab => b"\x1b[Z".to_vec(),
        KeyCode::Backspace if mods.contains(KeyModifiers::CONTROL) => vec![0x08],
        KeyCode::Backspace => with_alt(vec![0x7f], mods),
        KeyCode::Esc => with_alt(vec![0x1b], mods),
        KeyCode::Up => cursor_key('A', mods, modes),
        KeyCode::Down => cursor_key('B', mods, modes),
        KeyCode::Right => cursor_key('C', mods, modes),
        KeyCode::Left => cursor_key('D', mods, modes),
        KeyCode::Home => cursor_key('H', mods, modes),
        KeyCode::End => cursor_key('F', mods, modes),
        KeyCode::Insert => tilde_key(2, mods),
        KeyCode::Delete => tilde_key(3, mods),
        KeyCode::PageUp => tilde_key(5, mods),
        KeyCode::PageDown => tilde_key(6, mods),
        KeyCode::F(n @ 1..=4) => {
            let final_byte = (b'P' + n - 1) as char;
            match modifier_param(mods) {
                Some(m) => format!("\x1b[1;{m}{final_byte}").into_bytes(),
                None => format!("\x1bO{final_byte}").into_bytes(),
            }
        }
        KeyCode::F(n @ 5..=12) => {
            let number = [15, 17, 18, 19, 20, 21, 23, 24][usize::from(n - 5)];
            tilde_key(number, mods)
        }
        _ => return None,
    };
    Some(bytes)
}

const PASTE_START: &str = "\x1b[200~";
const PASTE_END: &str = "\x1b[201~";

/// The bytes for a paste: line breaks become CR, as xterm does; wrapped in ESC[200~ … ESC[201~
/// when the program asked for bracketed paste, raw otherwise. Inside the brackets, every
/// control character but tab and CR is dropped (C0, DEL and C1, as xterm does): with no ESC
/// left, the pasted text can't form an ESC[201~ that ends the paste early and runs the rest
/// as typed input. Removing only the marker would not do: "ESC ESC[201~" forms it again.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if bracketed {
        let inner: String = text
            .chars()
            .filter(|&c| c == '\t' || c == '\r' || !c.is_control())
            .collect();
        format!("{PASTE_START}{inner}{PASTE_END}").into_bytes()
    } else {
        text.into_bytes()
    }
}

/// The bytes for a mouse event inside `area` (the terminal's inner area), in the protocol the
/// program asked for. `None` when mouse reporting is off, the event is outside `area`, or it is
/// not one of the supported events (wheel, left button press/release, left drag in the motion
/// modes). Coordinates are relative to `area`, 1-based.
pub fn encode_mouse(
    event: &MouseEvent,
    mode: MouseProtocolMode,
    encoding: MouseProtocolEncoding,
    area: Rect,
) -> Option<Vec<u8>> {
    use crossterm::event::{MouseButton, MouseEventKind};
    let motion = matches!(
        mode,
        MouseProtocolMode::ButtonMotion | MouseProtocolMode::AnyMotion
    );
    let (button, release) = match event.kind {
        _ if mode == MouseProtocolMode::None => return None,
        MouseEventKind::ScrollUp => (64u16, false),
        MouseEventKind::ScrollDown => (65, false),
        MouseEventKind::Down(MouseButton::Left) => (0, false),
        MouseEventKind::Up(MouseButton::Left) if mode != MouseProtocolMode::Press => (0, true),
        MouseEventKind::Drag(MouseButton::Left) if motion => (32, false),
        _ => return None,
    };
    let inside = event.column >= area.x
        && event.column < area.x.saturating_add(area.width)
        && event.row >= area.y
        && event.row < area.y.saturating_add(area.height);
    if !inside {
        return None;
    }
    let x = u32::from(event.column - area.x) + 1;
    let y = u32::from(event.row - area.y) + 1;
    let mut button = u32::from(button);
    if event.modifiers.contains(KeyModifiers::SHIFT) {
        button += 4;
    }
    if event.modifiers.contains(KeyModifiers::ALT) {
        button += 8;
    }
    if event.modifiers.contains(KeyModifiers::CONTROL) {
        button += 16;
    }
    match encoding {
        MouseProtocolEncoding::Sgr => {
            let end = if release { 'm' } else { 'M' };
            Some(format!("\x1b[<{button};{x};{y}{end}").into_bytes())
        }
        MouseProtocolEncoding::Default | MouseProtocolEncoding::Utf8 => {
            // The legacy encodings have no release per button: release is button 3.
            let button = if release { (button & !3) | 3 } else { button };
            let mut out = b"\x1b[M".to_vec();
            for value in [button, x, y] {
                let value = value + 32;
                if encoding == MouseProtocolEncoding::Default {
                    out.push(u8::try_from(value).ok()?);
                } else {
                    let c = char::from_u32(value).filter(|_| value < 2048)?;
                    out.extend(c.to_string().into_bytes());
                }
            }
            Some(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEventState, MouseButton, MouseEventKind};

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn plain(code: KeyCode) -> Option<Vec<u8>> {
        encode_key(&key(code, KeyModifiers::NONE), TermModes::default())
    }

    fn with(code: KeyCode, modifiers: KeyModifiers) -> Option<Vec<u8>> {
        encode_key(&key(code, modifiers), TermModes::default())
    }

    fn app_cursor(code: KeyCode) -> Option<Vec<u8>> {
        encode_key(
            &key(code, KeyModifiers::NONE),
            TermModes {
                application_cursor: true,
            },
        )
    }

    fn bytes(s: &str) -> Option<Vec<u8>> {
        Some(s.as_bytes().to_vec())
    }

    #[test]
    fn printable_chars_are_utf8() {
        assert_eq!(plain(KeyCode::Char('a')), bytes("a"));
        assert_eq!(with(KeyCode::Char('A'), KeyModifiers::SHIFT), bytes("A"));
        assert_eq!(plain(KeyCode::Char('ç')), bytes("ç"));
        assert_eq!(plain(KeyCode::Char('ã')), bytes("ã"));
        assert_eq!(plain(KeyCode::Char('€')), bytes("€"));
        assert_eq!(plain(KeyCode::Char(' ')), bytes(" "));
    }

    #[test]
    fn editing_keys() {
        assert_eq!(plain(KeyCode::Enter), bytes("\r"));
        assert_eq!(plain(KeyCode::Tab), bytes("\t"));
        assert_eq!(with(KeyCode::BackTab, KeyModifiers::SHIFT), bytes("\x1b[Z"));
        assert_eq!(plain(KeyCode::BackTab), bytes("\x1b[Z"));
        assert_eq!(plain(KeyCode::Backspace), bytes("\x7f"));
        assert_eq!(plain(KeyCode::Esc), bytes("\x1b"));
    }

    #[test]
    fn ctrl_letters() {
        assert_eq!(
            with(KeyCode::Char('c'), KeyModifiers::CONTROL),
            bytes("\x03")
        );
        assert_eq!(
            with(KeyCode::Char('d'), KeyModifiers::CONTROL),
            bytes("\x04")
        );
        assert_eq!(
            with(KeyCode::Char('a'), KeyModifiers::CONTROL),
            bytes("\x01")
        );
        assert_eq!(
            with(KeyCode::Char('z'), KeyModifiers::CONTROL),
            bytes("\x1a")
        );
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(with(KeyCode::Char('C'), ctrl_shift), bytes("\x03"));
        for c in 'a'..='z' {
            assert_eq!(
                with(KeyCode::Char(c), KeyModifiers::CONTROL),
                Some(vec![(c as u8) & 0x1f]),
                "Ctrl+{c}"
            );
        }
    }

    #[test]
    fn ctrl_symbols_round_trip_crossterm() {
        // crossterm reports 0x00 as Ctrl+Space and 0x1d..0x1f as Ctrl+5..7.
        assert_eq!(
            with(KeyCode::Char(' '), KeyModifiers::CONTROL),
            Some(vec![0])
        );
        assert_eq!(
            with(KeyCode::Char('5'), KeyModifiers::CONTROL),
            bytes("\x1d")
        );
        assert_eq!(
            with(KeyCode::Char(']'), KeyModifiers::CONTROL),
            bytes("\x1d")
        );
        assert_eq!(
            with(KeyCode::Char('7'), KeyModifiers::CONTROL),
            bytes("\x1f")
        );
        assert_eq!(
            with(KeyCode::Char('_'), KeyModifiers::CONTROL),
            bytes("\x1f")
        );
    }

    #[test]
    fn alt_prefixes_esc() {
        assert_eq!(with(KeyCode::Char('x'), KeyModifiers::ALT), bytes("\x1bx"));
        assert_eq!(with(KeyCode::Char('ç'), KeyModifiers::ALT), bytes("\x1bç"));
        let ctrl_alt = KeyModifiers::CONTROL | KeyModifiers::ALT;
        assert_eq!(with(KeyCode::Char('c'), ctrl_alt), bytes("\x1b\x03"));
        assert_eq!(with(KeyCode::Enter, KeyModifiers::ALT), bytes("\x1b\r"));
        assert_eq!(
            with(KeyCode::Backspace, KeyModifiers::ALT),
            bytes("\x1b\x7f")
        );
    }

    #[test]
    fn arrows_normal_and_application_cursor() {
        assert_eq!(plain(KeyCode::Up), bytes("\x1b[A"));
        assert_eq!(plain(KeyCode::Down), bytes("\x1b[B"));
        assert_eq!(plain(KeyCode::Right), bytes("\x1b[C"));
        assert_eq!(plain(KeyCode::Left), bytes("\x1b[D"));
        assert_eq!(app_cursor(KeyCode::Up), bytes("\x1bOA"));
        assert_eq!(app_cursor(KeyCode::Down), bytes("\x1bOB"));
        assert_eq!(app_cursor(KeyCode::Right), bytes("\x1bOC"));
        assert_eq!(app_cursor(KeyCode::Left), bytes("\x1bOD"));
        assert_eq!(plain(KeyCode::Home), bytes("\x1b[H"));
        assert_eq!(plain(KeyCode::End), bytes("\x1b[F"));
        assert_eq!(app_cursor(KeyCode::Home), bytes("\x1bOH"));
        assert_eq!(app_cursor(KeyCode::End), bytes("\x1bOF"));
    }

    #[test]
    fn modified_cursor_keys_use_xterm_parameters() {
        assert_eq!(with(KeyCode::Up, KeyModifiers::SHIFT), bytes("\x1b[1;2A"));
        assert_eq!(with(KeyCode::Left, KeyModifiers::ALT), bytes("\x1b[1;3D"));
        assert_eq!(
            with(KeyCode::Right, KeyModifiers::CONTROL),
            bytes("\x1b[1;5C")
        );
        assert_eq!(
            with(KeyCode::End, KeyModifiers::CONTROL),
            bytes("\x1b[1;5F")
        );
        assert_eq!(
            with(KeyCode::PageUp, KeyModifiers::SHIFT),
            bytes("\x1b[5;2~")
        );
        assert_eq!(with(KeyCode::F(1), KeyModifiers::SHIFT), bytes("\x1b[1;2P"));
        assert_eq!(
            with(KeyCode::F(5), KeyModifiers::CONTROL),
            bytes("\x1b[15;5~")
        );
    }

    #[test]
    fn navigation_keys() {
        assert_eq!(plain(KeyCode::PageUp), bytes("\x1b[5~"));
        assert_eq!(plain(KeyCode::PageDown), bytes("\x1b[6~"));
        assert_eq!(plain(KeyCode::Delete), bytes("\x1b[3~"));
        assert_eq!(plain(KeyCode::Insert), bytes("\x1b[2~"));
    }

    #[test]
    fn function_keys() {
        let expected = [
            "\x1bOP", "\x1bOQ", "\x1bOR", "\x1bOS", "\x1b[15~", "\x1b[17~", "\x1b[18~", "\x1b[19~",
            "\x1b[20~", "\x1b[21~", "\x1b[23~", "\x1b[24~",
        ];
        for (i, seq) in expected.iter().enumerate() {
            assert_eq!(plain(KeyCode::F(i as u8 + 1)), bytes(seq), "F{}", i + 1);
        }
        assert_eq!(plain(KeyCode::F(13)), None);
    }

    #[test]
    fn reserved_shortcut_sends_nothing() {
        // As crossterm reports 0x1c in raw mode, and as the kitty protocol reports it.
        let as_ctrl_4 = key(KeyCode::Char('4'), KeyModifiers::CONTROL);
        let as_backslash = key(KeyCode::Char('\\'), KeyModifiers::CONTROL);
        assert!(is_reserved(&as_ctrl_4));
        assert!(is_reserved(&as_backslash));
        assert_eq!(encode_key(&as_ctrl_4, TermModes::default()), None);
        assert_eq!(encode_key(&as_backslash, TermModes::default()), None);
        // A plain backslash and a plain 4 are text.
        assert!(!is_reserved(&key(KeyCode::Char('\\'), KeyModifiers::NONE)));
        assert_eq!(plain(KeyCode::Char('\\')), bytes("\\"));
        assert_eq!(plain(KeyCode::Char('4')), bytes("4"));
        assert_eq!(RESERVED_KEY_LABEL, "Ctrl+\\");
    }

    #[test]
    fn releases_send_nothing() {
        let release = KeyEvent {
            code: KeyCode::Char('a'),
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Release,
            state: KeyEventState::NONE,
        };
        assert_eq!(encode_key(&release, TermModes::default()), None);
        let repeat = KeyEvent {
            kind: KeyEventKind::Repeat,
            ..release
        };
        assert_eq!(encode_key(&repeat, TermModes::default()), bytes("a"));
    }

    #[test]
    fn paste_bracketed_or_raw() {
        assert_eq!(encode_paste("olá", false), b"ol\xc3\xa1".to_vec());
        assert_eq!(
            encode_paste("olá", true),
            b"\x1b[200~ol\xc3\xa1\x1b[201~".to_vec()
        );
        assert_eq!(encode_paste("a\nb\r\nc", false), b"a\rb\rc".to_vec());
        assert_eq!(
            encode_paste("a\nb", true),
            b"\x1b[200~a\rb\x1b[201~".to_vec()
        );
    }

    /// The paste has exactly one end marker, at the very end.
    fn assert_single_end(out: &[u8]) {
        let end = b"\x1b[201~";
        let count = out.windows(end.len()).filter(|w| w == end).count();
        assert_eq!(count, 1, "{:?}", String::from_utf8_lossy(out));
        assert!(out.ends_with(end), "{:?}", String::from_utf8_lossy(out));
    }

    #[test]
    fn paste_cannot_close_the_bracket_early() {
        // A pasted end marker would let the rest run as typed input.
        assert_eq!(
            encode_paste("x\x1b[201~rm", true),
            b"\x1b[200~x[201~rm\x1b[201~".to_vec()
        );
        // Removing a single ESC must not join two of them back into a marker (review NS-29).
        for text in [
            "texto\x1b\x1b[201~rm -rf ~\n",
            "a\x1b\x1b\x1b[201~b",
            "a\x1b[201\x1b~b\x1b[201~",
            "a\x1bb\x1b[201~c\x1b",
        ] {
            let out = encode_paste(text, true);
            assert_single_end(&out);
            assert_eq!(
                out.iter().filter(|&&b| b == 0x1b).count(),
                2,
                "only the two markers carry an ESC: {:?}",
                String::from_utf8_lossy(&out)
            );
        }
    }

    #[test]
    fn bracketed_paste_drops_control_characters() {
        // As xterm does: C0 (but tab and line breaks), DEL and C1 never reach the program.
        assert_eq!(
            encode_paste("a\tb\x03c\x7fd\u{9b}e\x00f\nç", true),
            "\x1b[200~a\tbcdef\rç\x1b[201~".as_bytes().to_vec()
        );
        // Raw paste (the program did not ask for brackets) is passed as is, like a terminal.
        assert_eq!(encode_paste("a\x03b", false), b"a\x03b".to_vec());
    }

    fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
        MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }
    }

    const AREA: Rect = Rect {
        x: 31,
        y: 1,
        width: 80,
        height: 20,
    };

    fn sgr(event: MouseEvent, mode: MouseProtocolMode) -> Option<Vec<u8>> {
        encode_mouse(&event, mode, MouseProtocolEncoding::Sgr, AREA)
    }

    #[test]
    fn mouse_off_sends_nothing() {
        let wheel = mouse(MouseEventKind::ScrollUp, 40, 5);
        assert_eq!(sgr(wheel, MouseProtocolMode::None), None);
    }

    #[test]
    fn mouse_sgr_wheel_and_left_click_relative_to_area() {
        let mode = MouseProtocolMode::PressRelease;
        // Top-left cell of the area is (1, 1).
        assert_eq!(
            sgr(mouse(MouseEventKind::ScrollUp, 31, 1), mode),
            bytes("\x1b[<64;1;1M")
        );
        assert_eq!(
            sgr(mouse(MouseEventKind::ScrollDown, 40, 5), mode),
            bytes("\x1b[<65;10;5M")
        );
        assert_eq!(
            sgr(
                mouse(MouseEventKind::Down(MouseButton::Left), 110, 20),
                mode
            ),
            bytes("\x1b[<0;80;20M")
        );
        assert_eq!(
            sgr(mouse(MouseEventKind::Up(MouseButton::Left), 110, 20), mode),
            bytes("\x1b[<0;80;20m")
        );
    }

    #[test]
    fn mouse_outside_area_or_unsupported_sends_nothing() {
        let mode = MouseProtocolMode::AnyMotion;
        assert_eq!(sgr(mouse(MouseEventKind::ScrollUp, 30, 5), mode), None);
        assert_eq!(sgr(mouse(MouseEventKind::ScrollUp, 111, 5), mode), None);
        assert_eq!(sgr(mouse(MouseEventKind::ScrollUp, 40, 0), mode), None);
        assert_eq!(sgr(mouse(MouseEventKind::ScrollUp, 40, 21), mode), None);
        assert_eq!(
            sgr(mouse(MouseEventKind::Down(MouseButton::Right), 40, 5), mode),
            None
        );
        assert_eq!(sgr(mouse(MouseEventKind::Moved, 40, 5), mode), None);
    }

    #[test]
    fn mouse_modes_filter_events() {
        let up = mouse(MouseEventKind::Up(MouseButton::Left), 40, 5);
        let drag = mouse(MouseEventKind::Drag(MouseButton::Left), 40, 5);
        // X10 (Press) reports presses only.
        assert_eq!(sgr(up, MouseProtocolMode::Press), None);
        assert_eq!(sgr(drag, MouseProtocolMode::PressRelease), None);
        assert_eq!(
            sgr(drag, MouseProtocolMode::ButtonMotion),
            bytes("\x1b[<32;10;5M")
        );
    }

    #[test]
    fn mouse_modifiers_add_to_the_button() {
        let mut event = mouse(MouseEventKind::Down(MouseButton::Left), 40, 5);
        event.modifiers = KeyModifiers::SHIFT | KeyModifiers::CONTROL;
        assert_eq!(
            sgr(event, MouseProtocolMode::PressRelease),
            bytes("\x1b[<20;10;5M")
        );
    }

    #[test]
    fn mouse_default_encoding() {
        let event = mouse(MouseEventKind::Down(MouseButton::Left), 40, 5);
        assert_eq!(
            encode_mouse(
                &event,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Default,
                AREA
            ),
            Some(vec![0x1b, b'[', b'M', 32, 32 + 10, 32 + 5])
        );
        let release = mouse(MouseEventKind::Up(MouseButton::Left), 40, 5);
        assert_eq!(
            encode_mouse(
                &release,
                MouseProtocolMode::PressRelease,
                MouseProtocolEncoding::Default,
                AREA
            ),
            Some(vec![0x1b, b'[', b'M', 32 + 3, 32 + 10, 32 + 5])
        );
    }
}
