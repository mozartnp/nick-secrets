//! The translation of the input of the outer terminal (keys, paste, mouse) into the bytes an
//! xterm sends to the program running in it, for the terminal of an agent (NS-33). Pure: no
//! PTY and no I/O, so every rule is unit tested. Written from the findings of the panel
//! prototype (NS-29, prototype/panel/src/keys.rs), whose cases are the source of the tests.
//! `encode_key` knows no reserved shortcut: every key it gets goes to the program, Ctrl+\
//! included (0x1c). The reserved shortcut that toggles the focus between the list and the
//! terminal (NS-34) is a `Shortcut`, tested by the app before a key reaches `encode_key`, with
//! the refusal of the keys the operator can't choose (`shortcut_refusal`).
//! The Ctrl+Z is dropped by the app before `encode_key` (`is_suspend_key`, NS-44), and
//! `encode_key` still knows no shortcut rule.
//! The Shift+Enter is sent as the Alt+Enter (`ESC CR`, the new line of the `claude`, NS-49): it
//! only arrives with the kitty keyboard protocol on (terminal.rs), since the legacy `ESC O M`
//! is dropped by crossterm. The Ctrl+Enter is still a bare `CR`. With that protocol on, a
//! small letter with Shift (the Konsole's Alt+Shift+a, `ESC[97;4u`) is sent capital, as the
//! legacy mode sent it (`capital`).

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent};
use ratatui::layout::Rect;
use vt100::{MouseProtocolEncoding, MouseProtocolMode};

/// The terminal modes of the program that change how a key is encoded.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TermModes {
    /// DECCKM: the arrows (and Home/End) are sent as SS3 (ESC O x) instead of CSI (ESC [ x).
    pub application_cursor: bool,
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

/// The control byte of Ctrl+`c`, when there is one: the letters, and the symbols crossterm
/// reports for 0x00 and 0x1b..0x1f in raw mode (Ctrl+\ arrives as Ctrl+4, 0x1c).
fn ctrl_byte(c: char) -> Option<u8> {
    match c {
        'a'..='z' | 'A'..='Z' => Some((c.to_ascii_lowercase() as u8) & 0x1f),
        ' ' | '@' | '2' => Some(0x00),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' | '/' => Some(0x1f),
        _ => None,
    }
}

/// The symbol a control byte of a non-letter is kept as in a `Shortcut` (the inverse of
/// `ctrl_byte` for 0x00 and 0x1b..0x1f).
fn ctrl_symbol(byte: u8) -> Option<char> {
    match byte {
        0x00 => Some('@'),
        0x1b => Some('['),
        0x1c => Some('\\'),
        0x1d => Some(']'),
        0x1e => Some('^'),
        0x1f => Some('_'),
        _ => None,
    }
}

/// The modifiers a `Shortcut` keeps.
const SHORTCUT_MODIFIERS: KeyModifiers = KeyModifiers::CONTROL
    .union(KeyModifiers::ALT)
    .union(KeyModifiers::SHIFT);

/// The modifiers of a `Shortcut`, in their order, in the database and on the screen.
const MODIFIER_NAMES: [(KeyModifiers, &str, &str); 3] = [
    (KeyModifiers::CONTROL, "ctrl+", "Ctrl+"),
    (KeyModifiers::ALT, "alt+", "Alt+"),
    (KeyModifiers::SHIFT, "shift+", "Shift+"),
];

/// The names of the keys that are not a character, in the database and on the screen.
const NAMED_KEYS: &[(KeyCode, &str, &str)] = &[
    (KeyCode::Enter, "enter", "Enter"),
    (KeyCode::Esc, "esc", "Esc"),
    (KeyCode::Tab, "tab", "Tab"),
    (KeyCode::Backspace, "backspace", "Backspace"),
    (KeyCode::Up, "up", "↑"),
    (KeyCode::Down, "down", "↓"),
    (KeyCode::Left, "left", "←"),
    (KeyCode::Right, "right", "→"),
    (KeyCode::Home, "home", "Home"),
    (KeyCode::End, "end", "End"),
    (KeyCode::PageUp, "pageup", "PageUp"),
    (KeyCode::PageDown, "pagedown", "PageDown"),
    (KeyCode::Insert, "insert", "Insert"),
    (KeyCode::Delete, "delete", "Delete"),
];

/// The reserved shortcut that toggles the focus between the list and the terminal (NS-34), in
/// its canonical form, so a key matches itself however crossterm reports it: a Ctrl+symbol
/// with a control byte is the symbol of the byte (Ctrl+4 and Ctrl+\ are Ctrl+\, 0x1c) without
/// Shift; a letter with Ctrl or Alt is lowercase, with Shift as an explicit modifier; any other
/// key keeps its code and modifiers (only Ctrl, Alt and Shift; Shift+Tab is Tab with Shift).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    code: KeyCode,
    modifiers: KeyModifiers,
}

impl Shortcut {
    /// The default, Ctrl+\ (0x1c, which claude does not use). It only exists in the code: going
    /// back to the default deletes the row of the database.
    pub const DEFAULT: Shortcut = Shortcut {
        code: KeyCode::Char('\\'),
        modifiers: KeyModifiers::CONTROL,
    };

    /// The canonical form of a key, or `None` when it can't be a shortcut (another modifier,
    /// such as Super, or a key that is neither a character, a function key nor a named key).
    pub fn from_key(event: &KeyEvent) -> Option<Shortcut> {
        if !(event.modifiers - SHORTCUT_MODIFIERS).is_empty() {
            return None;
        }
        let mut modifiers = event.modifiers;
        let code = match event.code {
            KeyCode::Char(c) if c.is_ascii_alphabetic() => {
                if c.is_ascii_uppercase() {
                    modifiers |= KeyModifiers::SHIFT;
                }
                KeyCode::Char(c.to_ascii_lowercase())
            }
            KeyCode::Char(c) => {
                // The Shift of a symbol is in the symbol itself ('^' is Shift+6).
                modifiers -= KeyModifiers::SHIFT;
                let symbol = modifiers
                    .contains(KeyModifiers::CONTROL)
                    .then(|| ctrl_byte(c).and_then(ctrl_symbol))
                    .flatten();
                KeyCode::Char(symbol.unwrap_or(c))
            }
            KeyCode::BackTab => {
                modifiers |= KeyModifiers::SHIFT;
                KeyCode::Tab
            }
            KeyCode::F(n @ 1..=12) => KeyCode::F(n),
            code if NAMED_KEYS.iter().any(|(named, _, _)| *named == code) => code,
            _ => return None,
        };
        Some(Shortcut { code, modifiers })
    }

    /// Whether `event` is this shortcut (whatever its kind: press, repeat or release).
    pub fn matches(&self, event: &KeyEvent) -> bool {
        Shortcut::from_key(event) == Some(*self)
    }

    /// The text kept in the database: lowercase, the modifiers in the order `ctrl+alt+shift+`,
    /// then the key (`ctrl+\`, `alt+x`, `ctrl+shift+f5`, `f7`).
    pub fn to_db(&self) -> String {
        let mut text = String::new();
        for (modifier, name, _) in MODIFIER_NAMES {
            if self.modifiers.contains(modifier) {
                text.push_str(name);
            }
        }
        match self.code {
            KeyCode::Char(' ') => text.push_str("space"),
            KeyCode::Char(c) => text.push(c),
            KeyCode::F(n) => text.push_str(&format!("f{n}")),
            code => {
                let name = NAMED_KEYS.iter().find(|(named, _, _)| *named == code);
                text.push_str(name.map_or("", |(_, name, _)| name));
            }
        }
        text
    }

    /// The shortcut of a text of the database, only in the canonical form `to_db` writes:
    /// anything else (unknown, repeated or out of order modifiers, uppercase, `ctrl+4` for
    /// `ctrl+\`) is `None`.
    pub fn from_db(text: &str) -> Option<Shortcut> {
        let mut rest = text;
        let mut modifiers = KeyModifiers::NONE;
        for (modifier, name, _) in MODIFIER_NAMES {
            if let Some(after) = rest.strip_prefix(name)
                && !after.is_empty()
            {
                modifiers |= modifier;
                rest = after;
            }
        }
        let mut chars = rest.chars();
        let code = match (chars.next(), chars.next()) {
            (Some(c), None) => KeyCode::Char(c),
            _ if rest == "space" => KeyCode::Char(' '),
            _ => match rest.strip_prefix('f').and_then(|n| n.parse::<u8>().ok()) {
                Some(n) if rest == format!("f{n}") => KeyCode::F(n),
                _ => NAMED_KEYS
                    .iter()
                    .find(|(_, name, _)| *name == rest)
                    .map(|(code, _, _)| *code)?,
            },
        };
        let shortcut = Shortcut::from_key(&KeyEvent::new(code, modifiers))?;
        (shortcut.to_db() == text).then_some(shortcut)
    }

    /// What the screen shows: `Ctrl+\`, `Alt+X`, `Ctrl+Shift+F5`, `F7`.
    pub fn label(&self) -> String {
        let mut text = String::new();
        for (modifier, _, label) in MODIFIER_NAMES {
            if self.modifiers.contains(modifier) {
                text.push_str(label);
            }
        }
        match self.code {
            KeyCode::Char(' ') => text.push_str("Espaço"),
            KeyCode::Char(c) => text.push(c.to_ascii_uppercase()),
            KeyCode::F(n) => text.push_str(&format!("F{n}")),
            code => {
                let name = NAMED_KEYS.iter().find(|(named, _, _)| *named == code);
                text.push_str(name.map_or("", |(_, _, label)| label));
            }
        }
        text
    }
}

/// The keys claude needs, refused as the shortcut with any modifier: typing and moving.
const CLAUDE_KEYS: &[KeyCode] = &[
    KeyCode::Enter,
    KeyCode::Esc,
    KeyCode::Tab,
    KeyCode::Backspace,
    KeyCode::Up,
    KeyCode::Down,
    KeyCode::Left,
    KeyCode::Right,
];

/// The documented shortcuts of Claude Code, in the form of `Shortcut::to_db` (a letter is
/// compared without its Shift). Source: code.claude.com/docs/en/interactive-mode, checked on
/// 2026-11-19. Extend it when Claude Code gets new shortcuts. The ones with Enter, Backspace
/// and the arrows (Ctrl+Enter, Ctrl+Backspace, Ctrl+↑/↓, Alt+↑/↓, Alt+Enter, Shift+Enter,
/// Shift+←/→) are refused with every key of `CLAUDE_KEYS`.
const CLAUDE_SHORTCUTS: &[&str] = &[
    "ctrl+a", "ctrl+b", "ctrl+c", "ctrl+d", "ctrl+e", "ctrl+g", "ctrl+j", "ctrl+k", "ctrl+l",
    "ctrl+n", "ctrl+o", "ctrl+p", "ctrl+r", "ctrl+s", "ctrl+t", "ctrl+u", "ctrl+v", "ctrl+w",
    "ctrl+x", "ctrl+y", "ctrl+z", "ctrl+_", "ctrl+[", "alt+b", "alt+d", "alt+f", "alt+m", "alt+o",
    "alt+p", "alt+t", "alt+v", "alt+y",
];

/// The Ctrl+letters whose byte reaches claude as another key: Ctrl+H is Backspace, Ctrl+I Tab,
/// Ctrl+M Enter and Ctrl+[ Esc.
const BYTE_SYNONYMS: &[&str] = &["ctrl+h", "ctrl+i", "ctrl+m", "ctrl+["];

/// The refusal of a key without Ctrl nor Alt (and of a key that can't be a shortcut at all).
const MODIFIER_REFUSAL: &str = "Use uma tecla com Ctrl ou Alt (ou uma tecla de função, F1–F12).";

/// The shortcut of a key pressed in the capture, or why it is refused (`shortcut_refusal`).
pub fn shortcut_of_key(event: &KeyEvent) -> Result<Shortcut, String> {
    let shortcut = Shortcut::from_key(event).ok_or_else(|| MODIFIER_REFUSAL.to_string())?;
    match shortcut_refusal(&shortcut) {
        Some(refusal) => Err(refusal),
        None => Ok(shortcut),
    }
}

/// Why `shortcut` can't be the reserved shortcut, in PT-BR, or `None` when it can. Independent
/// of the current shortcut: choosing it again is fine.
pub fn shortcut_refusal(shortcut: &Shortcut) -> Option<String> {
    let claude = || {
        Some(format!(
            "O claude precisa dessa tecla ({}). Escolha outra.",
            shortcut.label()
        ))
    };
    let mods = shortcut.modifiers;
    if CLAUDE_KEYS.contains(&shortcut.code) {
        return claude();
    }
    if matches!(shortcut.code, KeyCode::PageUp | KeyCode::PageDown)
        && mods.contains(KeyModifiers::SHIFT)
    {
        return Some("O nick usa essa tecla para rolar o histórico. Escolha outra.".into());
    }
    if !matches!(shortcut.code, KeyCode::F(_))
        && !mods.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return Some(MODIFIER_REFUSAL.into());
    }
    let unshifted = match shortcut.code {
        KeyCode::Char(c) if c.is_ascii_alphabetic() => Shortcut {
            modifiers: mods - KeyModifiers::SHIFT,
            ..*shortcut
        },
        _ => *shortcut,
    }
    .to_db();
    if CLAUDE_SHORTCUTS.contains(&unshifted.as_str()) || BYTE_SYNONYMS.contains(&unshifted.as_str())
    {
        return claude();
    }
    None
}

/// The key as the shortcuts of the panel see it (NS-43): an ASCII letter without Ctrl nor Alt
/// is lowercase and without Shift, so Caps Lock and Shift don't change a shortcut (`J` and
/// Shift+`j` are `j`). Any other key is returned as it is: digits, symbols, non-ASCII letters
/// (`Ç`), named keys and letters with Ctrl or Alt. The kind and the state are kept. Not for
/// the text fields nor for the terminal of an agent, which get the key as it was typed.
pub fn shortcut_key(key: KeyEvent) -> KeyEvent {
    match key.code {
        KeyCode::Char(c)
            if c.is_ascii_alphabetic()
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            KeyEvent {
                code: KeyCode::Char(c.to_ascii_lowercase()),
                modifiers: key.modifiers - KeyModifiers::SHIFT,
                ..key
            }
        }
        _ => key,
    }
}

/// Whether the key is the Ctrl+Z that would reach the program as a bare 0x1a (SUSP), which
/// the terminal of an agent drops (NS-44): the `claude` suspends itself on it and waits for a
/// `fg`, and inside the nick there's no shell with job control to resume it, so its panel
/// would be stuck until the agent ended. `z` or `Z` (Caps Lock) with Ctrl; Shift and Super
/// don't matter, as `encode_key` ignores them on a letter with Ctrl. With Alt it isn't (ESC
/// 0x1a still goes to the program). The kind doesn't matter: a Repeat would be 0x1a too.
pub fn is_suspend_key(event: &KeyEvent) -> bool {
    matches!(event.code, KeyCode::Char('z' | 'Z'))
        && event.modifiers.contains(KeyModifiers::CONTROL)
        && !event.modifiers.contains(KeyModifiers::ALT)
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

/// The capital of a small letter typed with Shift (review NS-49, P1): with the kitty keyboard
/// mode on, the Konsole sends Alt+Shift+a as `ESC[97;4u`, the small letter, where the legacy
/// mode sent `ESC A`. A letter whose capital is not one char (`ß`), and any other char, stays
/// as it is: the legacy mode never sends a small letter with Shift, and the symbols already
/// come shifted (`ESC[33;4u` is the `!`).
fn capital(c: char) -> char {
    let mut upper = c.to_uppercase();
    match (upper.next(), upper.next()) {
        (Some(u), None) => u,
        _ => c,
    }
}

/// ESC before `bytes` when Alt is held.
fn with_alt(mut bytes: Vec<u8>, modifiers: KeyModifiers) -> Vec<u8> {
    if modifiers.contains(KeyModifiers::ALT) {
        bytes.insert(0, 0x1b);
    }
    bytes
}

/// The bytes of a key press (a repeat counts as a press), or `None` when the key sends nothing
/// (a release or a key xterm has no sequence for).
pub fn encode_key(event: &KeyEvent, modes: TermModes) -> Option<Vec<u8>> {
    if event.kind == KeyEventKind::Release {
        return None;
    }
    let mods = event.modifiers;
    let bytes = match event.code {
        KeyCode::Char(c) => {
            let control = mods
                .contains(KeyModifiers::CONTROL)
                .then(|| ctrl_byte(c))
                .flatten();
            let c = if mods.contains(KeyModifiers::SHIFT) && !mods.contains(KeyModifiers::CONTROL) {
                capital(c)
            } else {
                c
            };
            let raw = control.map_or_else(|| c.to_string().into_bytes(), |b| vec![b]);
            with_alt(raw, mods)
        }
        // Shift+Enter (only with the kitty keyboard protocol on, NS-49) is sent as Alt+Enter,
        // the new line of the `claude`; with both, a single ESC.
        KeyCode::Enter if mods.intersects(KeyModifiers::SHIFT | KeyModifiers::ALT) => {
            b"\x1b\r".to_vec()
        }
        KeyCode::Enter => b"\r".to_vec(),
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
            let final_byte = char::from(b'P' + n - 1);
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

/// The bytes of a paste: the line breaks become CR, as xterm does, wrapped in ESC[200~ …
/// ESC[201~ when the program asked for bracketed paste, raw otherwise. Inside the brackets,
/// every control character but tab and CR is dropped (C0, DEL and C1, as xterm does): with no
/// ESC left, the pasted text can't form an ESC[201~ that ends the paste early and runs the rest
/// as typed input. Removing only the marker would not do: "ESC ESC[201~" forms it again.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    let text = text.replace("\r\n", "\r").replace('\n', "\r");
    if !bracketed {
        return text.into_bytes();
    }
    let inner: String = text
        .chars()
        .filter(|&c| c == '\t' || c == '\r' || !c.is_control())
        .collect();
    format!("{PASTE_START}{inner}{PASTE_END}").into_bytes()
}

/// The bytes of a mouse event inside `area` (the terminal of the agent on the screen), in the
/// protocol the program asked for, with coordinates relative to `area`, 1-based. `None` when
/// the program did not ask for the mouse, the event is outside `area`, or it is not one of the
/// supported events (wheel, left button press/release, left drag in the motion modes).
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
        MouseEventKind::ScrollUp => (64u32, false),
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
    let mut button = button;
    for (modifier, add) in [
        (KeyModifiers::SHIFT, 4),
        (KeyModifiers::ALT, 8),
        (KeyModifiers::CONTROL, 16),
    ] {
        if event.modifiers.contains(modifier) {
            button += add;
        }
    }
    match encoding {
        MouseProtocolEncoding::Sgr => {
            let end = if release { 'm' } else { 'M' };
            Some(format!("\x1b[<{button};{x};{y}{end}").into_bytes())
        }
        MouseProtocolEncoding::Default | MouseProtocolEncoding::Utf8 => {
            // The legacy encodings have no release per button: a release is button 3.
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
    fn ctrl_backslash_and_ctrl_c_go_to_the_program() {
        // No reserved shortcut in this slice (F3b): Ctrl+\ reaches the PTY as 0x1c, as
        // crossterm reports it in raw mode (Ctrl+4) and as the kitty protocol reports it.
        assert_eq!(
            with(KeyCode::Char('4'), KeyModifiers::CONTROL),
            bytes("\x1c")
        );
        assert_eq!(
            with(KeyCode::Char('\\'), KeyModifiers::CONTROL),
            bytes("\x1c")
        );
        assert_eq!(
            with(KeyCode::Char('c'), KeyModifiers::CONTROL),
            bytes("\x03")
        );
        // A plain backslash and a plain 4 are text.
        assert_eq!(plain(KeyCode::Char('\\')), bytes("\\"));
        assert_eq!(plain(KeyCode::Char('4')), bytes("4"));
    }

    // ---- the reserved shortcut (NS-34) ----

    fn sc(code: KeyCode, modifiers: KeyModifiers) -> Shortcut {
        Shortcut::from_key(&key(code, modifiers)).expect("a shortcut")
    }

    fn ctrl(c: char) -> Shortcut {
        sc(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    #[test]
    fn ctrl_symbols_are_kept_in_the_canonical_form_of_their_byte() {
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        assert_eq!(ctrl('4'), ctrl('\\'), "raw mode and kitty");
        assert_eq!(ctrl('4'), Shortcut::DEFAULT);
        assert_eq!(ctrl('5'), ctrl(']'));
        assert_eq!(sc(KeyCode::Char('6'), ctrl_shift), ctrl('6'));
        assert_eq!(ctrl('6'), ctrl('^'));
        assert_eq!(sc(KeyCode::Char('^'), ctrl_shift), ctrl('^'));
        assert_eq!(ctrl('7'), ctrl('/'));
        assert_eq!(ctrl('7'), ctrl('_'));
        assert_eq!(ctrl('3'), ctrl('['));
        assert_eq!(ctrl('2'), ctrl('@'));
        assert_eq!(ctrl(' '), ctrl('@'));
        // The Shift stays on a letter (the refusal takes both, see below).
        assert_ne!(sc(KeyCode::Char('A'), ctrl_shift), ctrl('a'));
        assert_eq!(
            sc(KeyCode::Char('A'), ctrl_shift),
            sc(KeyCode::Char('a'), ctrl_shift)
        );
        assert_eq!(
            ctrl('A'),
            sc(KeyCode::Char('a'), ctrl_shift),
            "uppercase is Shift"
        );
        assert_eq!(
            sc(KeyCode::BackTab, KeyModifiers::SHIFT),
            sc(KeyCode::Tab, KeyModifiers::SHIFT)
        );
    }

    #[test]
    fn the_shortcut_goes_to_the_database_and_back() {
        let cases = [
            (ctrl('\\'), "ctrl+\\"),
            (ctrl(']'), "ctrl+]"),
            (ctrl('^'), "ctrl+^"),
            (ctrl('_'), "ctrl+_"),
            (ctrl('@'), "ctrl+@"),
            (ctrl('q'), "ctrl+q"),
            (sc(KeyCode::Char('x'), KeyModifiers::ALT), "alt+x"),
            (sc(KeyCode::Char(' '), KeyModifiers::ALT), "alt+space"),
            (
                sc(KeyCode::Char('X'), KeyModifiers::ALT | KeyModifiers::SHIFT),
                "alt+shift+x",
            ),
            (
                sc(KeyCode::F(5), KeyModifiers::CONTROL | KeyModifiers::SHIFT),
                "ctrl+shift+f5",
            ),
            (
                sc(
                    KeyCode::Char('k'),
                    KeyModifiers::CONTROL | KeyModifiers::ALT,
                ),
                "ctrl+alt+k",
            ),
            (sc(KeyCode::F(7), KeyModifiers::NONE), "f7"),
            (sc(KeyCode::Home, KeyModifiers::ALT), "alt+home"),
            (
                sc(KeyCode::PageDown, KeyModifiers::CONTROL),
                "ctrl+pagedown",
            ),
            (sc(KeyCode::Delete, KeyModifiers::CONTROL), "ctrl+delete"),
            (sc(KeyCode::Char('+'), KeyModifiers::ALT), "alt++"),
        ];
        for (shortcut, text) in cases {
            assert_eq!(shortcut.to_db(), text);
            assert_eq!(Shortcut::from_db(text), Some(shortcut), "{text}");
        }
    }

    #[test]
    fn a_database_value_that_is_not_canonical_is_refused() {
        for text in [
            "",
            "ctrl+",
            "ctrl+zz",
            "banana",
            "ctrl+ctrl+x",
            "alt+ctrl+x",
            "shift+ctrl+x",
            "CTRL+\\",
            "ctrl+A",
            "ctrl+4",
            "ctrl+f13",
            "f0",
            "super+x",
            " ctrl+x",
            "ctrl+x ",
        ] {
            assert_eq!(Shortcut::from_db(text), None, "{text:?}");
        }
    }

    #[test]
    fn the_label_is_what_the_screen_shows() {
        assert_eq!(ctrl('\\').label(), "Ctrl+\\");
        assert_eq!(ctrl(']').label(), "Ctrl+]");
        assert_eq!(sc(KeyCode::Char('x'), KeyModifiers::ALT).label(), "Alt+X");
        assert_eq!(
            sc(KeyCode::F(5), KeyModifiers::CONTROL | KeyModifiers::SHIFT).label(),
            "Ctrl+Shift+F5"
        );
        assert_eq!(sc(KeyCode::F(7), KeyModifiers::NONE).label(), "F7");
        assert_eq!(
            sc(KeyCode::Char(' '), KeyModifiers::ALT).label(),
            "Alt+Espaço"
        );
        assert_eq!(sc(KeyCode::Up, KeyModifiers::CONTROL).label(), "Ctrl+↑");
        assert_eq!(sc(KeyCode::Enter, KeyModifiers::NONE).label(), "Enter");
        assert_eq!(
            sc(KeyCode::BackTab, KeyModifiers::SHIFT).label(),
            "Shift+Tab"
        );
        assert_eq!(
            sc(KeyCode::PageUp, KeyModifiers::SHIFT).label(),
            "Shift+PageUp"
        );
    }

    #[test]
    fn the_default_matches_ctrl_4_and_ctrl_backslash_of_crossterm() {
        assert_eq!(Shortcut::DEFAULT.to_db(), "ctrl+\\");
        assert!(Shortcut::DEFAULT.matches(&key(KeyCode::Char('4'), KeyModifiers::CONTROL)));
        assert!(Shortcut::DEFAULT.matches(&key(KeyCode::Char('\\'), KeyModifiers::CONTROL)));
        assert!(!Shortcut::DEFAULT.matches(&key(KeyCode::Char('\\'), KeyModifiers::NONE)));
        assert!(!Shortcut::DEFAULT.matches(&key(KeyCode::Char('4'), KeyModifiers::NONE)));
        assert!(!Shortcut::DEFAULT.matches(&key(KeyCode::Char(']'), KeyModifiers::CONTROL)));
        // A key with a modifier the nick doesn't keep is never a shortcut.
        assert_eq!(
            Shortcut::from_key(&key(
                KeyCode::Char('\\'),
                KeyModifiers::CONTROL | KeyModifiers::SUPER
            )),
            None
        );
        assert_eq!(
            Shortcut::from_key(&key(KeyCode::CapsLock, KeyModifiers::NONE)),
            None
        );
    }

    const NEEDS: fn(&str) -> String =
        |label| format!("O claude precisa dessa tecla ({label}). Escolha outra.");
    const MODIFIER: &str = "Use uma tecla com Ctrl ou Alt (ou uma tecla de função, F1–F12).";
    const SCROLL: &str = "O nick usa essa tecla para rolar o histórico. Escolha outra.";

    fn refusal(code: KeyCode, modifiers: KeyModifiers) -> Option<String> {
        shortcut_refusal(&sc(code, modifiers))
    }

    #[test]
    fn keys_without_ctrl_or_alt_are_refused_but_the_function_keys() {
        for (code, modifiers) in [
            (KeyCode::Char('a'), KeyModifiers::NONE),
            (KeyCode::Char('A'), KeyModifiers::SHIFT),
            (KeyCode::Char('1'), KeyModifiers::NONE),
            (KeyCode::Char('!'), KeyModifiers::SHIFT),
            (KeyCode::Char(' '), KeyModifiers::NONE),
            (KeyCode::Home, KeyModifiers::NONE),
            (KeyCode::PageUp, KeyModifiers::NONE),
            (KeyCode::Delete, KeyModifiers::SHIFT),
        ] {
            assert_eq!(
                refusal(code, modifiers),
                Some(MODIFIER.to_string()),
                "{code:?} {modifiers:?}"
            );
        }
        for n in 1..=12 {
            assert_eq!(refusal(KeyCode::F(n), KeyModifiers::NONE), None, "F{n}");
            assert_eq!(
                refusal(KeyCode::F(n), KeyModifiers::SHIFT),
                None,
                "Shift+F{n}"
            );
            assert_eq!(
                refusal(KeyCode::F(n), KeyModifiers::CONTROL),
                None,
                "Ctrl+F{n}"
            );
        }
    }

    #[test]
    fn the_keys_claude_needs_are_refused_with_any_modifier() {
        let mods = [
            KeyModifiers::NONE,
            KeyModifiers::SHIFT,
            KeyModifiers::CONTROL,
            KeyModifiers::ALT,
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ];
        for code in [
            KeyCode::Enter,
            KeyCode::Esc,
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Backspace,
            KeyCode::Up,
            KeyCode::Down,
            KeyCode::Left,
            KeyCode::Right,
        ] {
            for modifiers in mods {
                let shortcut = sc(code, modifiers);
                assert_eq!(
                    shortcut_refusal(&shortcut),
                    Some(NEEDS(&shortcut.label())),
                    "{code:?} {modifiers:?}"
                );
            }
        }
    }

    #[test]
    fn the_documented_shortcuts_of_claude_are_refused() {
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        let alt_shift = KeyModifiers::ALT | KeyModifiers::SHIFT;
        for c in "abcdegjklnoprstuvwxyz".chars() {
            let label = format!("Ctrl+{}", c.to_ascii_uppercase());
            assert_eq!(
                refusal(KeyCode::Char(c), KeyModifiers::CONTROL),
                Some(NEEDS(&label)),
                "Ctrl+{c}"
            );
            assert!(
                refusal(KeyCode::Char(c.to_ascii_uppercase()), ctrl_shift).is_some(),
                "Ctrl+Shift+{c}"
            );
        }
        for c in "bdfmoptvy".chars() {
            let label = format!("Alt+{}", c.to_ascii_uppercase());
            assert_eq!(
                refusal(KeyCode::Char(c), KeyModifiers::ALT),
                Some(NEEDS(&label)),
                "Alt+{c}"
            );
            assert!(
                refusal(KeyCode::Char(c.to_ascii_uppercase()), alt_shift).is_some(),
                "Alt+Shift+{c}"
            );
        }
        for c in ['_', '7', '/', '[', '3'] {
            assert!(
                refusal(KeyCode::Char(c), KeyModifiers::CONTROL).is_some(),
                "Ctrl+{c}"
            );
        }
        assert_eq!(
            refusal(KeyCode::Char('d'), KeyModifiers::CONTROL),
            Some(NEEDS("Ctrl+D"))
        );
        assert_eq!(
            refusal(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some(NEEDS("Ctrl+C"))
        );
    }

    #[test]
    fn the_byte_synonyms_are_refused() {
        // They reach claude as another key: Backspace, Tab, Enter and Esc.
        for (c, label) in [
            ('h', "Ctrl+H"),
            ('i', "Ctrl+I"),
            ('m', "Ctrl+M"),
            ('[', "Ctrl+["),
        ] {
            assert_eq!(
                refusal(KeyCode::Char(c), KeyModifiers::CONTROL),
                Some(NEEDS(label)),
                "{label}"
            );
        }
    }

    #[test]
    fn shift_page_up_and_down_are_the_history_of_the_nick() {
        for code in [KeyCode::PageUp, KeyCode::PageDown] {
            assert_eq!(refusal(code, KeyModifiers::SHIFT), Some(SCROLL.to_string()));
            assert_eq!(
                refusal(code, KeyModifiers::CONTROL | KeyModifiers::SHIFT),
                Some(SCROLL.to_string())
            );
            assert_eq!(refusal(code, KeyModifiers::CONTROL), None, "Ctrl alone");
        }
    }

    #[test]
    fn the_free_keys_are_accepted() {
        let ctrl_shift = KeyModifiers::CONTROL | KeyModifiers::SHIFT;
        for (code, modifiers) in [
            (KeyCode::Char('\\'), KeyModifiers::CONTROL),
            (KeyCode::Char('4'), KeyModifiers::CONTROL),
            (KeyCode::Char(']'), KeyModifiers::CONTROL),
            (KeyCode::Char('^'), KeyModifiers::CONTROL),
            (KeyCode::Char('6'), ctrl_shift),
            (KeyCode::Char('@'), KeyModifiers::CONTROL),
            (KeyCode::Char(' '), KeyModifiers::CONTROL),
            (KeyCode::Char('f'), KeyModifiers::CONTROL),
            (KeyCode::Char('q'), KeyModifiers::CONTROL),
            (KeyCode::F(1), KeyModifiers::NONE),
            (KeyCode::F(5), KeyModifiers::CONTROL),
            (KeyCode::Char('x'), KeyModifiers::ALT),
            (KeyCode::Char('a'), KeyModifiers::ALT),
            (KeyCode::Home, KeyModifiers::CONTROL),
        ] {
            assert_eq!(refusal(code, modifiers), None, "{code:?} {modifiers:?}");
        }
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

    /// The fields of a key as they are: `KeyEvent`'s `==` ignores the case of a letter with
    /// Shift (`J` and Shift+`j` are equal), which would hide a wrong case.
    fn raw(event: KeyEvent) -> (KeyCode, KeyModifiers, KeyEventKind, KeyEventState) {
        (event.code, event.modifiers, event.kind, event.state)
    }

    #[test]
    fn shortcut_key_lowercases_plain_letters_only() {
        let none = KeyModifiers::NONE;
        let shift = KeyModifiers::SHIFT;
        let small_j = key(KeyCode::Char('j'), none);
        for event in [
            key(KeyCode::Char('J'), none),
            key(KeyCode::Char('J'), shift),
            key(KeyCode::Char('j'), shift),
        ] {
            assert_eq!(raw(shortcut_key(event)), raw(small_j), "{event:?}");
        }
        for (small, capital) in ('a'..='z').zip('A'..='Z') {
            let expected = key(KeyCode::Char(small), none);
            for event in [
                key(KeyCode::Char(small), none),
                key(KeyCode::Char(capital), none),
            ] {
                assert_eq!(raw(shortcut_key(event)), raw(expected), "{event:?}");
            }
        }
        for event in [
            key(KeyCode::Char('j'), none),
            key(KeyCode::Char('J'), KeyModifiers::CONTROL),
            key(KeyCode::Char('J'), KeyModifiers::ALT),
            key(KeyCode::Char('j'), KeyModifiers::CONTROL | shift),
            key(KeyCode::Char('1'), none),
            key(KeyCode::Char('!'), shift),
            key(KeyCode::Char('ç'), none),
            key(KeyCode::Char('Ç'), shift),
            key(KeyCode::Char(' '), none),
            key(KeyCode::Enter, none),
            key(KeyCode::Esc, none),
            key(KeyCode::Up, none),
            key(KeyCode::Tab, none),
            key(KeyCode::BackTab, shift),
        ] {
            assert_eq!(raw(shortcut_key(event)), raw(event), "{event:?}");
        }
        for kind in [
            KeyEventKind::Press,
            KeyEventKind::Repeat,
            KeyEventKind::Release,
        ] {
            let event = KeyEvent {
                code: KeyCode::Char('J'),
                modifiers: shift,
                kind,
                state: KeyEventState::CAPS_LOCK,
            };
            let expected = KeyEvent {
                code: KeyCode::Char('j'),
                modifiers: none,
                ..event
            };
            assert_eq!(raw(shortcut_key(event)), raw(expected), "{event:?}");
        }
    }

    #[test]
    fn is_suspend_key_is_ctrl_z_with_or_without_shift_and_caps_lock() {
        let ctrl = KeyModifiers::CONTROL;
        let shift = KeyModifiers::SHIFT;
        let alt = KeyModifiers::ALT;
        let none = KeyModifiers::NONE;
        let mut cases = vec![
            (key(KeyCode::Char('z'), ctrl), true),
            (key(KeyCode::Char('Z'), ctrl), true),
            (key(KeyCode::Char('Z'), ctrl | shift), true),
            (key(KeyCode::Char('z'), ctrl | shift), true),
            (key(KeyCode::Char('z'), ctrl | KeyModifiers::SUPER), true),
            (
                KeyEvent::new_with_kind(KeyCode::Char('z'), ctrl, KeyEventKind::Repeat),
                true,
            ),
            (key(KeyCode::Char('z'), none), false),
            (key(KeyCode::Char('Z'), none), false),
            (key(KeyCode::Char('z'), shift), false),
            (key(KeyCode::Char('Z'), shift), false),
            (key(KeyCode::Char('z'), ctrl | alt), false),
            (key(KeyCode::Char('Z'), ctrl | alt | shift), false),
            (key(KeyCode::Char('z'), alt), false),
            (key(KeyCode::Char('\\'), ctrl), false),
            (key(KeyCode::Char('4'), ctrl), false),
            (key(KeyCode::Char('c'), ctrl), false),
            (key(KeyCode::F(1), none), false),
        ];
        for (small, capital) in ('a'..='y').zip('A'..='Y') {
            cases.push((key(KeyCode::Char(small), ctrl), false));
            cases.push((key(KeyCode::Char(capital), ctrl), false));
        }
        for (event, expected) in cases {
            assert_eq!(is_suspend_key(&event), expected, "{event:?}");
        }
    }

    #[test]
    fn shift_enter_is_sent_as_alt_enter() {
        let shift = KeyModifiers::SHIFT;
        let alt = KeyModifiers::ALT;
        assert_eq!(with(KeyCode::Enter, shift), bytes("\x1b\r"));
        assert_eq!(with(KeyCode::Enter, shift | alt), bytes("\x1b\r"));
        assert_eq!(with(KeyCode::Enter, shift), with(KeyCode::Enter, alt));
        // Unchanged.
        assert_eq!(plain(KeyCode::Enter), bytes("\r"));
        assert_eq!(with(KeyCode::Enter, alt), bytes("\x1b\r"));
        assert_eq!(with(KeyCode::Enter, KeyModifiers::CONTROL), bytes("\r"));
        let repeat = KeyEvent::new_with_kind(KeyCode::Enter, shift, KeyEventKind::Repeat);
        assert_eq!(encode_key(&repeat, TermModes::default()), bytes("\x1b\r"));
        let release = KeyEvent::new_with_kind(KeyCode::Enter, shift, KeyEventKind::Release);
        assert_eq!(encode_key(&release, TermModes::default()), None);
    }

    /// The keys as crossterm 0.29 parses the sequences of the kitty keyboard protocol (flag 1)
    /// that the Konsole sends (`parse_csi_u_encoded_key_code`), NS-49: they do what the legacy
    /// ones did. The default shortcut with `Char('\\')`+Ctrl is
    /// `the_default_matches_ctrl_4_and_ctrl_backslash_of_crossterm`.
    #[test]
    fn kitty_keys_encode_like_the_legacy_ones() {
        let ctrl = KeyModifiers::CONTROL;
        // ESC[92;5u and the legacy 0x1c (Ctrl+4 for crossterm).
        assert_eq!(with(KeyCode::Char('\\'), ctrl), Some(vec![0x1c]));
        assert_eq!(with(KeyCode::Char('4'), ctrl), Some(vec![0x1c]));
        // ESC[99;5u.
        assert_eq!(with(KeyCode::Char('c'), ctrl), Some(vec![0x03]));
        // ESC[122;6u.
        assert!(is_suspend_key(&key(
            KeyCode::Char('z'),
            ctrl | KeyModifiers::SHIFT
        )));
        // ESC[97;65u: `a` with the Caps Lock bit (64) of the modifier.
        let caps = KeyEvent::new_with_kind_and_state(
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
            KeyEventState::CAPS_LOCK,
        );
        let expected = (
            KeyCode::Char('a'),
            KeyModifiers::NONE,
            KeyEventKind::Press,
            KeyEventState::CAPS_LOCK,
        );
        assert_eq!(raw(shortcut_key(caps)), expected);
        let caps_capital = KeyEvent {
            code: KeyCode::Char('A'),
            ..caps
        };
        assert_eq!(raw(shortcut_key(caps_capital)), expected);
    }

    /// Review NS-49, P1: with the kitty keyboard mode on, the Konsole sends Alt+Shift+a as
    /// `ESC[97;4u`, the small letter with Shift (crossterm: `Char('a')`+ALT|SHIFT); the legacy
    /// mode sent `ESC A` (`Char('A')`+ALT|SHIFT). A small letter with Shift and without Ctrl goes
    /// as the capital, as before. Alt+Shift+1 comes as `ESC[33;4u`, the `!` itself.
    #[test]
    fn a_small_letter_with_shift_is_sent_capital() {
        let shift = KeyModifiers::SHIFT;
        let alt = KeyModifiers::ALT;
        let ctrl = KeyModifiers::CONTROL;
        // The kitty forms.
        assert_eq!(with(KeyCode::Char('a'), alt | shift), bytes("\x1bA"));
        assert_eq!(with(KeyCode::Char('a'), shift), bytes("A"));
        assert_eq!(with(KeyCode::Char('ç'), alt | shift), bytes("\x1bÇ"));
        assert_eq!(with(KeyCode::Char('!'), alt | shift), bytes("\x1b!"));
        // The same bytes of the legacy forms, unchanged.
        assert_eq!(with(KeyCode::Char('A'), alt | shift), bytes("\x1bA"));
        assert_eq!(with(KeyCode::Char('A'), shift), bytes("A"));
        assert_eq!(with(KeyCode::Char('!'), alt), bytes("\x1b!"));
        // Without Shift, or with Ctrl, as before.
        assert_eq!(with(KeyCode::Char('a'), alt), bytes("\x1ba"));
        assert_eq!(with(KeyCode::Char('a'), ctrl | shift), Some(vec![0x01]));
        assert_eq!(
            with(KeyCode::Char('a'), ctrl | alt | shift),
            bytes("\x1b\x01")
        );
        assert_eq!(with(KeyCode::Char('ç'), ctrl | shift), bytes("ç"));
        // A letter whose capital is not one char stays as it is.
        assert_eq!(with(KeyCode::Char('ß'), shift), bytes("ß"));
    }
}
