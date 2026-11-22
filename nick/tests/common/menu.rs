//! The menu of the types of an agent ("Abrir agente", NS-35) driven in the PTY: shared by
//! tests/agent.rs and tests/install.rs (review of the NS-52), so a label of the menu lives in one
//! place.

use super::{DOWN, ENTER, Nick};

/// `g` and the check: true when the menu of the types opened, false on the alert.
pub fn open_menu(nick: &mut Nick) -> bool {
    nick.send("g");
    nick.wait_until("the menu or the alert", || {
        let screen = nick.screen();
        screen.contains("Qual tipo de conversa") || screen.contains("Agente não aberto")
    });
    nick.screen().contains("Qual tipo de conversa")
}

/// Every label of the menu of the types, the longest first (one is the start of another).
pub const TYPE_LABELS: [&str; 8] = [
    "Tech Leader - Criar plano a partir do Jira",
    "PO - Criar ticket a partir do Sentry",
    "PO - Conversar para definir ticket",
    "PO - Criar ticket a partir do Jira",
    "Tech Leader - Criar plano",
    "PO - Criar ticket",
    "Desenvolvimento",
    "Review",
];

/// The type selected on the menu.
pub fn selected_type(nick: &Nick) -> Option<&'static str> {
    let screen = nick.screen();
    TYPE_LABELS
        .into_iter()
        .find(|label| screen.contains(&format!("> {label}")))
}

/// On the menu, ↓ until `label` is selected (each ↓ waits for the selection to move), and
/// Enter: its form.
pub fn choose_type(nick: &mut Nick, label: &str) {
    nick.wait_until("a type selected", || selected_type(nick).is_some());
    while selected_type(nick) != Some(label) {
        let before = selected_type(nick);
        nick.send(DOWN);
        nick.wait_until(&format!("the selection to leave {before:?}"), || {
            selected_type(nick) != before
        });
    }
    nick.send(ENTER);
    nick.wait_for(&format!("Tipo: {label}"));
}
