//! The types of the auto_scrum stages and their forms (NS-35), with no screen: the 8 types of
//! `choose_type` (auto_scrum/auto_scrum.sh), each with its menu label, its `Stage` (the database
//! keeps the stage, not the type) and the questions of its `ask_questions_<type>`, in the same
//! order, with the same labels and rules. The labels are duplicated here until the cleanup of the
//! script: tests compare the menu with the real `choose_type` and look for every label in the
//! script. The form only collects the answers: the prompt is built by bash (`bin/nick-agent
//! --render`), and nothing here reads a template or the project config.

use crate::agents::Stage;
use crate::docs;
use crate::tickets::{INVALID_ID_MESSAGE, jira_id_valid};

/// A type of conversation of the auto_scrum menu (`$TYPE` of the script).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageType {
    Po,
    PoDiscussion,
    PoJira,
    PoSentry,
    TechLeader,
    TechLeaderJira,
    Development,
    Review,
}

/// How a field is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    /// One line typed in the form, trimmed as `read -r` does (`trim_read`).
    Line,
    /// A long text written in `$EDITOR` (`read_via_editor`), taken as the editor left it, without
    /// the final line breaks (`$(cat ...)`), never trimmed.
    Editor,
}

/// The rule of a field.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rule {
    /// `read_required` (or `read_via_editor`): empty is refused.
    Required,
    /// `read_optional`: empty is not sent.
    Optional,
    /// `read_jira_id`: required, and `jira_id_valid` must accept it.
    JiraId,
    /// The plan of Dev and Review: optional; its label shows the default with the current ID.
    Plan,
}

/// A question of a form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Field {
    /// The text of the question in the script, without the `: ` (for `Rule::Plan`, the start of
    /// it: see `field_label`).
    pub label: &'static str,
    /// The argument of `bin/nick-agent --render` that carries the answer.
    pub flag: &'static str,
    pub rule: Rule,
    pub input: Input,
}

/// The start of the label of the plan, before the default path (`ask_questions_development`).
pub const PLAN_LABEL_START: &str = "Arquivo do plano (opcional, Enter para usar ";

const TITLE_PO: Field = Field {
    label: "Título do pedido",
    flag: "--title",
    rule: Rule::Required,
    input: Input::Line,
};
const DESCRIPTION_PO: Field = Field {
    label: "Descrição do pedido",
    flag: "--description",
    rule: Rule::Required,
    input: Input::Editor,
};
const SUBJECT: Field = Field {
    label: "Assunto para discutir",
    flag: "--title",
    rule: Rule::Required,
    input: Input::Line,
};
const JIRA_LINK: Field = Field {
    label: "Link do ticket no Jira",
    flag: "--jira-link",
    rule: Rule::Required,
    input: Input::Line,
};
const EXTRA_DESCRIPTION: Field = Field {
    label: "Descrição extra (opcional, Enter para pular)",
    flag: "--description",
    rule: Rule::Optional,
    input: Input::Line,
};
const SENTRY_LINK: Field = Field {
    label: "Link do issue no Sentry",
    flag: "--sentry-link",
    rule: Rule::Required,
    input: Input::Line,
};
const JIRA_ID: Field = Field {
    label: "ID do Jira (ex: SC-123)",
    flag: "--jira-id",
    rule: Rule::JiraId,
    input: Input::Line,
};
const TITLE_TASK: Field = Field {
    label: "Título da tarefa",
    flag: "--title",
    rule: Rule::Required,
    input: Input::Line,
};
const DESCRIPTION_CONTEXT: Field = Field {
    label: "Descrição / contexto",
    flag: "--description",
    rule: Rule::Required,
    input: Input::Editor,
};
const EXTRA_CONTEXT: Field = Field {
    label: "Contexto extra (opcional, Enter para pular)",
    flag: "--description",
    rule: Rule::Optional,
    input: Input::Line,
};
const PLAN: Field = Field {
    label: PLAN_LABEL_START,
    flag: "--plan-path",
    rule: Rule::Plan,
    input: Input::Line,
};

impl StageType {
    /// Every type, in the order of the menu with Jira and Sentry on.
    pub const ALL: [StageType; 8] = [
        StageType::Po,
        StageType::PoDiscussion,
        StageType::PoJira,
        StageType::PoSentry,
        StageType::TechLeader,
        StageType::TechLeaderJira,
        StageType::Development,
        StageType::Review,
    ];

    /// The `$TYPE` of the script (the `--render`/`--type` argument of the launcher).
    pub fn key(self) -> &'static str {
        match self {
            StageType::Po => "po",
            StageType::PoDiscussion => "po_discussion",
            StageType::PoJira => "po_jira",
            StageType::PoSentry => "po_sentry",
            StageType::TechLeader => "tech_leader",
            StageType::TechLeaderJira => "tech_leader_jira",
            StageType::Development => "development",
            StageType::Review => "review",
        }
    }

    /// The text of the menu of `choose_type`.
    pub fn label(self) -> &'static str {
        match self {
            StageType::Po => "PO - Criar ticket",
            StageType::PoDiscussion => "PO - Conversar para definir ticket",
            StageType::PoJira => "PO - Criar ticket a partir do Jira",
            StageType::PoSentry => "PO - Criar ticket a partir do Sentry",
            StageType::TechLeader => "Tech Leader - Criar plano",
            StageType::TechLeaderJira => "Tech Leader - Criar plano a partir do Jira",
            StageType::Development => "Desenvolvimento",
            StageType::Review => "Review",
        }
    }

    /// The stage of the database (`agents.stage`) and of the agent name.
    pub fn stage(self) -> Stage {
        match self {
            StageType::Po | StageType::PoDiscussion | StageType::PoJira | StageType::PoSentry => {
                Stage::Po
            }
            StageType::TechLeader | StageType::TechLeaderJira => Stage::TechLeader,
            StageType::Development => Stage::Development,
            StageType::Review => Stage::Review,
        }
    }

    /// The menu of `choose_type` with the flags of the project (the `jira=`/`sentry=` of the
    /// check): po_jira and tech_leader_jira only with Jira, po_sentry only with Sentry.
    pub fn menu(jira: bool, sentry: bool) -> Vec<StageType> {
        StageType::ALL
            .into_iter()
            .filter(|kind| match kind {
                StageType::PoJira | StageType::TechLeaderJira => jira,
                StageType::PoSentry => sentry,
                _ => true,
            })
            .collect()
    }

    /// The questions of `ask_questions_<type>`, in order.
    pub fn fields(self) -> &'static [Field] {
        match self {
            StageType::Po => &[TITLE_PO, DESCRIPTION_PO],
            StageType::PoDiscussion => &[SUBJECT],
            StageType::PoJira => &[JIRA_LINK, EXTRA_DESCRIPTION],
            StageType::PoSentry => &[SENTRY_LINK, EXTRA_DESCRIPTION],
            StageType::TechLeader => &[JIRA_ID, TITLE_TASK, DESCRIPTION_CONTEXT],
            StageType::TechLeaderJira => &[JIRA_LINK, EXTRA_CONTEXT],
            StageType::Development | StageType::Review => &[JIRA_ID, PLAN],
        }
    }

    /// The answers a new form starts with: the Jira ID filled with the ID of the ticket
    /// (editable), every other one empty.
    pub fn initial_values(self, ticket_key: &str) -> Vec<String> {
        self.fields()
            .iter()
            .map(|field| match field.rule {
                Rule::JiraId => ticket_key.to_string(),
                _ => String::new(),
            })
            .collect()
    }
}

/// `text` trimmed as `read -r` with the default IFS does: spaces, tabs and line breaks at both
/// ends, and nothing else (a no-break space stays, unlike `str::trim`).
pub fn trim_read(text: &str) -> &str {
    text.trim_matches([' ', '\t', '\n'])
}

/// The label of `field` on the screen and in its messages: the label of the script, and for the
/// plan the default path with the ID typed now (`.nick/docs/<ID>/plan.md`; only shown: the path
/// sent to the prompt is computed by bash).
pub fn field_label(field: &Field, jira_id: &str) -> String {
    match field.rule {
        Rule::Plan => format!("{PLAN_LABEL_START}{})", docs::plan_path(trim_read(jira_id))),
        _ => field.label.to_string(),
    }
}

/// The message of a required field left empty.
pub fn required_message(label: &str) -> String {
    format!("Esse campo é obrigatório: {label}.")
}

/// A refused answer: the index of its field and the message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    pub field: usize,
    pub message: String,
}

/// The arguments of `bin/nick-agent --render` for the answers `values` of `kind` (one per field,
/// in order), with the rules of the script: a line trimmed as `read` does, an editor text as it
/// is; an empty required one (or Jira ID) refused, an empty optional one left out, a Jira ID that
/// `jira_id_valid` refuses refused with `INVALID_ID_MESSAGE`. The first refusal, in the order of
/// the fields, is returned.
pub fn render_args(
    kind: StageType,
    values: &[String],
) -> Result<Vec<(&'static str, String)>, Refusal> {
    let fields = kind.fields();
    let jira_id = fields
        .iter()
        .position(|field| field.rule == Rule::JiraId)
        .and_then(|i| values.get(i))
        .map_or("", String::as_str);
    let mut args = Vec::new();
    for (index, field) in fields.iter().enumerate() {
        let raw = values.get(index).map_or("", String::as_str);
        let value = match field.input {
            Input::Line => trim_read(raw),
            Input::Editor => raw,
        };
        let refuse = |message: String| Refusal {
            field: index,
            message,
        };
        if value.is_empty() {
            match field.rule {
                Rule::Optional | Rule::Plan => continue,
                Rule::Required | Rule::JiraId => {
                    return Err(refuse(required_message(&field_label(field, jira_id))));
                }
            }
        }
        if field.rule == Rule::JiraId && !jira_id_valid(value) {
            return Err(refuse(INVALID_ID_MESSAGE.to_string()));
        }
        args.push((field.flag, value.to_string()));
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    fn auto_scrum_sh() -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../auto_scrum/auto_scrum.sh")
    }

    #[test]
    fn the_menu_follows_the_flags_in_the_order_of_choose_type() {
        use StageType::*;
        assert_eq!(
            StageType::menu(false, false),
            [Po, PoDiscussion, TechLeader, Development, Review]
        );
        assert_eq!(
            StageType::menu(true, false),
            [
                Po,
                PoDiscussion,
                PoJira,
                TechLeader,
                TechLeaderJira,
                Development,
                Review
            ]
        );
        assert_eq!(
            StageType::menu(false, true),
            [Po, PoDiscussion, PoSentry, TechLeader, Development, Review]
        );
        assert_eq!(StageType::menu(true, true), StageType::ALL);
    }

    #[test]
    fn key_label_and_stage_of_each_type() {
        let expected = [
            ("po", "PO - Criar ticket", Stage::Po),
            (
                "po_discussion",
                "PO - Conversar para definir ticket",
                Stage::Po,
            ),
            ("po_jira", "PO - Criar ticket a partir do Jira", Stage::Po),
            (
                "po_sentry",
                "PO - Criar ticket a partir do Sentry",
                Stage::Po,
            ),
            (
                "tech_leader",
                "Tech Leader - Criar plano",
                Stage::TechLeader,
            ),
            (
                "tech_leader_jira",
                "Tech Leader - Criar plano a partir do Jira",
                Stage::TechLeader,
            ),
            ("development", "Desenvolvimento", Stage::Development),
            ("review", "Review", Stage::Review),
        ];
        for (kind, (key, label, stage)) in StageType::ALL.into_iter().zip(expected) {
            assert_eq!(
                (kind.key(), kind.label(), kind.stage()),
                (key, label, stage)
            );
        }
    }

    #[test]
    fn the_fields_of_each_type() {
        let summary = |kind: StageType| -> Vec<(&str, &str, Rule, Input)> {
            kind.fields()
                .iter()
                .map(|f| (f.label, f.flag, f.rule, f.input))
                .collect()
        };
        use Input::*;
        use Rule::*;
        assert_eq!(
            summary(StageType::Po),
            [
                ("Título do pedido", "--title", Required, Line),
                ("Descrição do pedido", "--description", Required, Editor)
            ]
        );
        assert_eq!(
            summary(StageType::PoDiscussion),
            [("Assunto para discutir", "--title", Required, Line)]
        );
        assert_eq!(
            summary(StageType::PoJira),
            [
                ("Link do ticket no Jira", "--jira-link", Required, Line),
                (
                    "Descrição extra (opcional, Enter para pular)",
                    "--description",
                    Optional,
                    Line
                )
            ]
        );
        assert_eq!(
            summary(StageType::PoSentry),
            [
                ("Link do issue no Sentry", "--sentry-link", Required, Line),
                (
                    "Descrição extra (opcional, Enter para pular)",
                    "--description",
                    Optional,
                    Line
                )
            ]
        );
        assert_eq!(
            summary(StageType::TechLeader),
            [
                ("ID do Jira (ex: SC-123)", "--jira-id", JiraId, Line),
                ("Título da tarefa", "--title", Required, Line),
                ("Descrição / contexto", "--description", Required, Editor)
            ]
        );
        assert_eq!(
            summary(StageType::TechLeaderJira),
            [
                ("Link do ticket no Jira", "--jira-link", Required, Line),
                (
                    "Contexto extra (opcional, Enter para pular)",
                    "--description",
                    Optional,
                    Line
                )
            ]
        );
        for kind in [StageType::Development, StageType::Review] {
            assert_eq!(
                summary(kind),
                [
                    ("ID do Jira (ex: SC-123)", "--jira-id", JiraId, Line),
                    (PLAN_LABEL_START, "--plan-path", Plan, Line)
                ]
            );
        }
    }

    #[test]
    fn the_id_starts_with_the_ticket_and_the_rest_empty() {
        assert_eq!(
            StageType::TechLeader.initial_values("NS-9"),
            ["NS-9", "", ""]
        );
        assert_eq!(StageType::Review.initial_values("NS-9"), ["NS-9", ""]);
        assert_eq!(StageType::Po.initial_values("NS-9"), ["", ""]);
    }

    #[test]
    fn trim_read_is_the_trim_of_read() {
        assert_eq!(trim_read("  a  "), "a");
        assert_eq!(trim_read("\ta\t"), "a");
        assert_eq!(trim_read("\n a b \n"), "a b");
        // read does not take a no-break space (nor a carriage return): str::trim would.
        assert_eq!(trim_read("\u{a0}a"), "\u{a0}a");
        assert_eq!(trim_read("a\r"), "a\r");
        assert_eq!(trim_read(" \t "), "");
    }

    #[test]
    fn the_plan_label_shows_the_default_with_the_current_id() {
        let plan = StageType::Development.fields()[1];
        assert_eq!(
            field_label(&plan, " SC-7 "),
            "Arquivo do plano (opcional, Enter para usar .nick/docs/SC-7/plan.md)"
        );
        let id = StageType::Development.fields()[0];
        assert_eq!(field_label(&id, "SC-7"), "ID do Jira (ex: SC-123)");
    }

    fn values(list: &[&str]) -> Vec<String> {
        list.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn render_args_trims_lines_keeps_editor_texts_and_omits_empty_optionals() {
        assert_eq!(
            render_args(StageType::Po, &values(&["  Título  ", "  texto\n longo  "])),
            Ok(vec![
                ("--title", "Título".to_string()),
                ("--description", "  texto\n longo  ".to_string())
            ])
        );
        assert_eq!(
            render_args(StageType::PoJira, &values(&["\thttps://j/1 ", "   "])),
            Ok(vec![("--jira-link", "https://j/1".to_string())])
        );
        assert_eq!(
            render_args(StageType::Development, &values(&[" SC-1 ", ""])),
            Ok(vec![("--jira-id", "SC-1".to_string())])
        );
        assert_eq!(
            render_args(StageType::Review, &values(&["SC-1", " outro.md "])),
            Ok(vec![
                ("--jira-id", "SC-1".to_string()),
                ("--plan-path", "outro.md".to_string())
            ])
        );
        assert_eq!(
            render_args(StageType::PoDiscussion, &values(&["\u{a0}x"])),
            Ok(vec![("--title", "\u{a0}x".to_string())])
        );
    }

    #[test]
    fn render_args_refuses_the_first_bad_field() {
        assert_eq!(
            render_args(StageType::Po, &values(&["  ", ""])),
            Err(Refusal {
                field: 0,
                message: "Esse campo é obrigatório: Título do pedido.".into()
            })
        );
        assert_eq!(
            render_args(StageType::Po, &values(&["a", ""])),
            Err(Refusal {
                field: 1,
                message: "Esse campo é obrigatório: Descrição do pedido.".into()
            })
        );
        assert_eq!(
            render_args(StageType::TechLeader, &values(&[" ", "a", "b"])),
            Err(Refusal {
                field: 0,
                message: "Esse campo é obrigatório: ID do Jira (ex: SC-123).".into()
            })
        );
        for bad in ["a/b", "..", "a b", ".", "a\u{a0}b"] {
            assert_eq!(
                render_args(StageType::Development, &values(&[bad, ""])),
                Err(Refusal {
                    field: 0,
                    message: INVALID_ID_MESSAGE.into()
                }),
                "{bad:?}"
            );
        }
        assert_eq!(
            render_args(StageType::TechLeaderJira, &values(&["", "x"])),
            Err(Refusal {
                field: 0,
                message: "Esse campo é obrigatório: Link do ticket no Jira.".into()
            })
        );
    }

    /// The numbered items of the menu of the real `choose_type` with the flags (its `select`,
    /// one per line with COLUMNS=1), without "Sair (q)".
    fn script_menu(jira: bool, sentry: bool) -> Vec<String> {
        let output = Command::new("bash")
            .arg("-c")
            .arg(format!(
                "source '{}'; JIRA_ENABLED={jira}; SENTRY_ENABLED={sentry}; choose_type",
                auto_scrum_sh().display()
            ))
            .env("COLUMNS", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                child.stdin.take().unwrap().write_all(b"q\n")?;
                child.wait_with_output()
            })
            .expect("run choose_type");
        String::from_utf8_lossy(&output.stderr)
            .lines()
            .filter_map(|line| {
                let (number, item) = line.split_once(") ")?;
                number.parse::<u32>().ok()?;
                (item != "Sair (q)").then(|| item.to_string())
            })
            .collect()
    }

    #[test]
    fn the_menu_is_the_menu_of_the_real_choose_type() {
        for (jira, sentry) in [(false, false), (true, false), (false, true), (true, true)] {
            let script = script_menu(jira, sentry);
            assert!(script.len() >= 5, "the script menu was read: {script:?}");
            let ours: Vec<&str> = StageType::menu(jira, sentry)
                .into_iter()
                .map(StageType::label)
                .collect();
            assert_eq!(ours, script, "jira={jira} sentry={sentry}");
        }
    }

    #[test]
    fn every_field_label_is_in_the_script() {
        let script = std::fs::read_to_string(auto_scrum_sh()).unwrap();
        for kind in StageType::ALL {
            for field in kind.fields() {
                let label = match field.rule {
                    // The default path of the plan is dynamic in both: only its start is fixed.
                    Rule::Plan => PLAN_LABEL_START,
                    _ => field.label,
                };
                assert!(
                    script.contains(&format!("\"{label}")),
                    "{}: {label:?} is in auto_scrum.sh",
                    kind.key()
                );
            }
        }
        // Positive control: a made-up label is not found.
        assert!(!script.contains("\"Título inventado"));
    }
}
