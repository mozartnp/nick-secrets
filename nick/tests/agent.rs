//! Integration tests of the agents (NS-33): the real `nick` binary in a PTY (tests/common), with
//! the real bin/nick-agent and the fake claude and ai-jail of bin/tests/fixtures/bin first in the
//! PATH, the fake EDITOR of tests/fixtures/bin, a temporary HOME and XDG_STATE_HOME (never the
//! operator's database), the logs of the prompts in the world (NICK_PROMPT_LOGS_DIR, never
//! auto_scrum/logs/) and a clean environment. Nothing real is launched. Since the NS-35 every
//! agent opens through the menu of the types, the form and the preview of the prompt
//! (`open_agent`). Rule of the NS-32: after an ESC, wait for a text that only exists in the next
//! state.

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use common::{DOWN, ENTER, ESC, LEFT, Nick, PANEL_COL, RIGHT, TAB, World};
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;

const ALT_OFF: &str = "\x1b[?1049l";
const CURSOR_ON: &str = "\x1b[?25h";
const MOUSE_ON: &str = "\x1b[?1000h";
const MOUSE_OFF: &str = "\x1b[?1000l";
const PASTE_ON: &str = "\x1b[?2004h";
const PASTE_OFF: &str = "\x1b[?2004l";
const OFF_JAIL: &str = "FORA DA JAIL: o agente roda direto no seu shell.";

/// The logs of the fakes, under the root of the world (out of the project).
struct Logs {
    claude: PathBuf,
    env: PathBuf,
    jail: PathBuf,
    pids: PathBuf,
}

fn logs(world: &World) -> Logs {
    Logs {
        claude: world.root.join("claude.argv"),
        env: world.root.join("claude.env"),
        jail: world.root.join("jail.argv"),
        pids: world.root.join("pids"),
    }
}

/// The directory of the logs of the prompts of a world (NICK_PROMPT_LOGS_DIR).
fn prompt_logs(world: &World) -> PathBuf {
    world.root.join("prompt_logs")
}

/// The fake EDITOR (tests/fixtures/bin/editor).
fn fake_editor() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bin/editor")
}

/// Runs the nick with the fakes first in the PATH, their logs in the world, the fake EDITOR, the
/// logs of the prompts in the world, the claude mode and `extra`.
fn spawn(world: &World, mode: &str, extra: &[(&str, &str)]) -> Nick {
    let l = logs(world);
    let path = common::fake_path();
    let (claude, env, jail, pids) = (
        l.claude.display().to_string(),
        l.env.display().to_string(),
        l.jail.display().to_string(),
        l.pids.display().to_string(),
    );
    let prompts = prompt_logs(world).display().to_string();
    let editor = fake_editor().display().to_string();
    let mut all = vec![
        ("PATH", path.as_str()),
        ("FAKE_CLAUDE_MODE", mode),
        ("FAKE_CLAUDE_LOG", claude.as_str()),
        ("FAKE_CLAUDE_ENV", env.as_str()),
        ("FAKE_JAIL_LOG", jail.as_str()),
        ("FAKE_CLAUDE_PIDS", pids.as_str()),
        ("NICK_PROMPT_LOGS_DIR", prompts.as_str()),
        ("EDITOR", editor.as_str()),
    ];
    all.extend_from_slice(extra);
    world.spawn(&world.root, &all)
}

/// A world with the project `proj` (the config `lines`) registered with the ticket NS-1.
fn world_with(lines: &[&str]) -> (World, PathBuf) {
    let world = World::new();
    let proj = world.project("proj", lines);
    world.register(&proj);
    world.ticket(&proj, "NS-1", "Primeiro");
    (world, proj)
}

/// On the tree (NS-38; before, the Tickets screen): the project `name` open and its first
/// ticket selected.
fn open_tickets(nick: &mut Nick, name: &str) {
    nick.open_project(name);
    nick.send(DOWN);
    nick.wait_for_flat("g abrir agente");
}

/// The type each stage of the NS-33/NS-34 tests (`downs` in PO, Tech Leader, Dev, Review)
/// opens with since the NS-35: the PO by its discussion (one line), the others by their own type.
const TYPE_OF_STAGE: [&str; 4] = [
    "PO - Conversar para definir ticket",
    "Tech Leader - Criar plano",
    "Desenvolvimento",
    "Review",
];

/// `g` and the check: true when the menu of the types opened, false on the alert.
fn open_menu(nick: &mut Nick) -> bool {
    nick.send("g");
    nick.wait_until("the menu or the alert", || {
        let screen = nick.screen();
        screen.contains("Qual tipo de conversa") || screen.contains("Agente não aberto")
    });
    nick.screen().contains("Qual tipo de conversa")
}

/// Every label of the menu of the types, the longest first (one is the start of another).
const TYPE_LABELS: [&str; 8] = [
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
fn selected_type(nick: &Nick) -> Option<&'static str> {
    let screen = nick.screen();
    TYPE_LABELS
        .into_iter()
        .find(|label| screen.contains(&format!("> {label}")))
}

/// On the menu, ↓ until `label` is selected (each ↓ waits for the selection to move), and
/// Enter: its form.
fn choose_type(nick: &mut Nick, label: &str) {
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

/// `g`, the type of the stage `downs` and its form answered (the ID comes from the ticket; the
/// long text from the fake EDITOR), then `s` on the preview: opens the agent of that stage on
/// the selected ticket (NS-35). Stops on the alert when the check fails.
fn open_agent(nick: &mut Nick, downs: usize) {
    if !open_menu(nick) {
        return;
    }
    choose_type(nick, TYPE_OF_STAGE[downs]);
    match downs {
        0 => nick.send("Assunto de teste"),
        1 => {
            nick.send(TAB);
            nick.send("Título de teste");
            nick.send(TAB);
            nick.send(ENTER);
            nick.wait_for("Descrição do editor falso (2 linhas; Enter edita)");
            // The focus went back to the ID.
        }
        _ => {}
    }
    nick.send(ENTER);
    nick.wait_for("Confirma o envio para o claude? (s/N)");
    nick.send("s");
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
}

/// The logs of the prompts of the world.
fn prompt_files(world: &World) -> Vec<PathBuf> {
    fs::read_dir(prompt_logs(world))
        .map(|dir| dir.map(|entry| entry.unwrap().path()).collect())
        .unwrap_or_default()
}

/// The only log of a prompt of the world.
fn only_log(world: &World) -> PathBuf {
    let files = prompt_files(world);
    assert_eq!(files.len(), 1, "{files:?}");
    files[0].clone()
}

/// The output after the last time the TUI left the alternate screen: only the cursor shows.
fn assert_restored_at_the_end(raw: &str) {
    let off = raw.rfind(ALT_OFF).expect("the alternate screen was left");
    let tail = &raw[off + ALT_OFF.len()..];
    assert!(tail.starts_with(CURSOR_ON), "the cursor is shown: {tail:?}");
    for (on, off) in [(MOUSE_ON, MOUSE_OFF), (PASTE_ON, PASTE_OFF)] {
        if let Some(last_on) = raw.rfind(on) {
            assert!(
                raw.rfind(off).is_some_and(|o| o > last_on),
                "{on:?} was turned off: {raw:?}"
            );
        }
    }
}

fn alive(pid: i32) -> bool {
    kill(Pid::from_raw(pid), None).is_ok()
}

fn pids(world: &World) -> Vec<i32> {
    read(&logs(world).pids)
        .split_whitespace()
        .map(|p| p.parse().unwrap())
        .collect()
}

fn wait_dead(nick: &Nick, pid: i32) {
    nick.wait_until(&format!("the end of pid {pid}"), || !alive(pid));
}

// ---- opening an agent ----

#[test]
fn an_agent_out_of_the_jail_runs_claude_in_the_project_and_ends() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "3")]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 2);
    // The header, in the panel beside the list: wrapped there (NS-34), so read as one line.
    nick.wait_for_panel(&format!("proj · Dev · NS-1 · {OFF_JAIL}"));
    nick.wait_for("READY");
    // Active while it runs, with the lock held.
    let agents = world.agents();
    assert_eq!(agents.len(), 1);
    let (session_id, stage, name, state) = agents[0].clone();
    assert_eq!(
        (stage.as_str(), name.as_str(), state.as_str()),
        ("development", "proj · Dev · NS-1", "active")
    );
    let lock = world.agents_dir().join(format!("{session_id}.lock"));
    assert!(lock.is_file());
    let raw = nick.raw();
    assert!(raw.contains(MOUSE_ON) && raw.contains(PASTE_ON), "{raw:?}");
    // á and Ctrl+C reach claude as bytes (Ctrl+C does not quit the nick).
    nick.send("á");
    nick.send("\x03");
    nick.wait_for("GOT c3 a1 03");
    nick.wait_for("O agente proj · Dev · NS-1 terminou.");
    // Decision 10 of NS-38: the selection stays on the node of the agent, now ended, with the
    // focus on the tree.
    nick.wait_for_flat("↑/↓ mover · r retomar · o arquivar · ← subir");
    let (_, row) = nick.selected_row().expect("a selection");
    assert!(row.contains("Dev  parado"), "the same node selected: {row}");
    // The argv: the session id of the database and the name, then the prompt (NS-35); the Dev
    // stage in auto mode (NS-35); the cwd is the project.
    let argv = read(&logs(&world).claude);
    assert!(
        argv.contains(&format!(
            "--session-id\n{session_id}\n--name\nproj · Dev · NS-1\n"
        )),
        "{argv}"
    );
    let log = only_log(&world);
    assert!(
        argv.ends_with(&format!("{}\n", read(&log).trim_end_matches('\n'))),
        "the prompt is the last argument: {argv}"
    );
    assert!(argv.contains("--permission-mode\nauto\n"), "{argv}");
    // NS-41: out of the jail too, the launcher sends one --append-system-prompt (the instruction
    // about the permission rules; its text is the bash library's, so only the flag is checked).
    assert_eq!(
        argv.lines()
            .filter(|l| *l == "--append-system-prompt")
            .count(),
        1,
        "{argv}"
    );
    let env = read(&logs(&world).env);
    assert!(
        env.lines().any(|l| l == format!("PWD={}", proj.display())),
        "{env}"
    );
    assert!(!logs(&world).jail.exists(), "no ai-jail out of the jail");
    assert_eq!(world.agents()[0].3, "stopped");
    assert!(!lock.exists(), "the lock is gone");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_restored_at_the_end(&nick.raw());
}

/// NS-44: the Ctrl+Z (0x1a in raw mode) never reaches claude, which would suspend itself
/// with no shell to resume it; the status says so, and the next key still goes to claude.
#[test]
fn ctrl_z_never_reaches_claude_and_the_status_says_so() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "1")]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    nick.send("\x1a");
    wait_flat(&nick, "O Ctrl+Z não tem efeito dentro do nick.");
    nick.keep_screen("Ctrl+Z descartado no terminal do agente");
    assert!(!nick.screen().contains("GOT"), "{}", nick.screen());
    // Had the 0x1a reached it, claude would show GOT 1a.
    nick.send("x");
    nick.wait_for("GOT 78");
    nick.keep_screen("depois do x: o claude falso recebeu só 78");
    wait_flat(&nick, "O agente proj · PO · NS-1 terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn an_agent_in_the_jail_goes_through_ai_jail() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"true\""]);
    let mut nick = spawn(&world, "sleep", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 1);
    nick.wait_for("proj · Tech Leader · NS-1 · na jail");
    nick.wait_for("READY");
    assert!(!nick.screen().contains("FORA DA JAIL"));
    let jail = read(&logs(&world).jail);
    let lines: Vec<&str> = jail.lines().collect();
    let dash = lines
        .iter()
        .position(|l| *l == "--")
        .expect("a -- in the argv");
    assert!(lines[..dash].contains(&"--no-inherit-env"), "{jail}");
    assert_eq!(&lines[dash + 1..dash + 3], ["claude", "--settings"]);
    assert!(jail.contains("--append-system-prompt"), "{jail}");
    // Ends the fake from outside: the end comes back to the list with the code.
    for pid in pids(&world) {
        let _ = kill(Pid::from_raw(pid), Signal::SIGKILL);
    }
    nick.wait_for("O agente proj · Tech Leader · NS-1 terminou com o código");
    assert_eq!(world.agents()[0].3, "stopped");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn each_project_runs_with_its_own_cwd_and_config() {
    let world = World::new();
    let a = world.project("alfa", &["JAIL_ENABLED=\"true\""]);
    let b = world.project("beta", &["JAIL_ENABLED=\"false\""]);
    for dir in [&a, &b] {
        world.register(dir);
        world.ticket(dir, "NS-1", "t");
    }
    let mut nick = spawn(&world, "", &[]);
    open_tickets(&mut nick, "beta");
    open_agent(&mut nick, 3);
    nick.wait_for("O agente beta · Review · NS-1 terminou.");
    let env = read(&logs(&world).env);
    assert!(
        env.lines().any(|l| l == format!("PWD={}", b.display())),
        "{env}"
    );
    assert!(
        !logs(&world).jail.exists(),
        "the config of beta: no ai-jail"
    );
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn a_failing_check_shows_the_jail_error_and_opens_nothing() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"true\""]);
    // A PATH with the fake claude and the tools of the launcher, but no ai-jail at all.
    let min_bin = world.root.join("min_bin");
    fs::create_dir_all(&min_bin).unwrap();
    for tool in [
        "bash", "dirname", "basename", "uname", "mktemp", "rm", "cat", "env", "mkdir",
    ] {
        let found = std::process::Command::new("bash")
            .args(["-c", &format!("command -v {tool}")])
            .output()
            .unwrap();
        let path = String::from_utf8(found.stdout).unwrap();
        symlink(path.trim(), min_bin.join(tool)).unwrap();
    }
    symlink(common::fake_bin().join("claude"), min_bin.join("claude")).unwrap();
    let min_path = min_bin.display().to_string();
    let before = common::snapshot(&proj);
    let mut nick = spawn(&world, "", &[("PATH", &min_path)]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("Agente não aberto");
    assert_eq!(
        common::snapshot(&proj),
        before,
        "nothing written in the project"
    );
    let flat = nick.flat_screen();
    assert!(flat.contains("ai-jail não encontrado no PATH."), "{flat}");
    assert!(flat.contains("Nada foi aberto."), "{flat}");
    assert!(
        flat.contains(&format!("{}/.nick/nick.conf", proj.display())),
        "{flat}"
    );
    assert_eq!(world.agents(), []);
    assert!(!logs(&world).claude.exists(), "claude never ran");
    nick.send(ENTER);
    nick.wait_for("g abrir agente");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn a_config_removed_after_the_list_opened_is_refused() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "", &[]);
    open_tickets(&mut nick, "proj");
    fs::remove_file(proj.join(".nick/nick.conf")).unwrap();
    nick.send("g");
    nick.wait_for("Projeto indisponível: [indisponível: sem .nick/nick.conf].");
    assert!(
        nick.flat_screen()
            .contains("O agente só abre com a pasta e o .nick/nick.conf no lugar."),
        "{}",
        nick.flat_screen()
    );
    assert!(!nick.screen().contains("Abrir agente"));
    assert_eq!(world.agents(), []);
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

// ---- the stages of the auto_scrum (NS-35): menu, form, editor and preview ----

/// The labels of the menu on the screen, in order (the types the check offered).
fn menu_labels(nick: &Nick) -> Vec<&'static str> {
    let screen = nick.screen();
    let mut found: Vec<(usize, &'static str)> = Vec::new();
    for label in TYPE_LABELS {
        for (row, line) in screen.lines().enumerate() {
            // The label alone on its row (one label is the start of another).
            let rest = line
                .find(label)
                .map(|at| line[at + label.len()..].trim_start_matches(' '));
            if rest.is_some_and(|rest| rest.is_empty() || rest.starts_with('│')) {
                found.push((row, label));
            }
        }
    }
    found.sort();
    found.into_iter().map(|(_, label)| label).collect()
}

#[test]
fn the_menu_follows_jira_and_sentry_of_the_project() {
    for (lines, expected) in [
        (
            vec!["JAIL_ENABLED=\"false\""],
            vec![
                "PO - Criar ticket",
                "PO - Conversar para definir ticket",
                "Tech Leader - Criar plano",
                "Desenvolvimento",
                "Review",
            ],
        ),
        (
            vec![
                "JAIL_ENABLED=\"false\"",
                "JIRA_ENABLED=\"true\"",
                "SENTRY_ENABLED=\"true\"",
            ],
            vec![
                "PO - Criar ticket",
                "PO - Conversar para definir ticket",
                "PO - Criar ticket a partir do Jira",
                "PO - Criar ticket a partir do Sentry",
                "Tech Leader - Criar plano",
                "Tech Leader - Criar plano a partir do Jira",
                "Desenvolvimento",
                "Review",
            ],
        ),
    ] {
        let (world, _) = world_with(&lines);
        let mut nick = spawn(&world, "", &[]);
        open_tickets(&mut nick, "proj");
        assert!(open_menu(&mut nick));
        nick.wait_for("> PO - Criar ticket");
        assert_eq!(menu_labels(&nick), expected, "{lines:?}");
        nick.send(ESC);
        nick.wait_for("g abrir agente");
        nick.send("q");
        assert_eq!(nick.wait_exit(), Some(0));
    }
}

#[test]
fn a_po_discussion_goes_through_the_form_and_the_preview_with_the_prompt_last() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let before = common::snapshot(&proj);
    let mut nick = spawn(&world, "", &[]);
    open_tickets(&mut nick, "proj");
    assert!(open_menu(&mut nick));
    choose_type(&mut nick, "PO - Conversar para definir ticket");
    nick.wait_for("Assunto para discutir:");
    // Spaces around it: trimmed as the read of the script does.
    nick.send("  Assunto do teste PTY  ");
    nick.send(ENTER);
    nick.wait_for("Confirma o envio para o claude? (s/N)");
    assert_eq!(
        common::snapshot(&proj),
        before,
        "the render writes nothing in the project"
    );
    let log = only_log(&world);
    let prompt = read(&log);
    assert!(prompt.contains("Assunto do teste PTY"), "{prompt}");
    assert!(
        !prompt.contains("  Assunto do teste PTY"),
        "trimmed: {prompt}"
    );
    let flat = nick.flat_screen();
    assert!(flat.contains(OFF_JAIL), "{flat}");
    assert!(
        flat.contains("Jail (ai-jail): desligada — o agente roda direto no seu shell."),
        "{flat}"
    );
    assert!(
        flat.contains(&format!("Salvo em {}", log.display())),
        "{flat}"
    );
    // The scroll: one row down, then back to the top.
    nick.wait_for("linhas 1–");
    nick.send(DOWN);
    nick.wait_for("linhas 2–");
    nick.send("\x1b[H");
    nick.wait_for("linhas 1–");
    nick.send("s");
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    let argv = read(&logs(&world).claude);
    assert!(
        argv.ends_with(&format!("{}\n", prompt.trim_end_matches('\n'))),
        "the prompt is the last argument: {argv}"
    );
    assert!(!argv.contains("--permission-mode"), "{argv}");
    assert_eq!(world.agents()[0].1, "po");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn the_development_stage_opens_in_auto_mode() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 2);
    nick.wait_for("O agente proj · Dev · NS-1 terminou.");
    let argv = read(&logs(&world).claude);
    assert!(argv.contains("--permission-mode\nauto\n"), "{argv}");
    let prompt = read(&only_log(&world));
    assert!(prompt.contains(".nick/docs/NS-1/plan.md"), "{prompt}");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn cancelling_on_the_preview_launches_nothing_and_keeps_the_log() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "", &[]);
    open_tickets(&mut nick, "proj");
    assert!(open_menu(&mut nick));
    choose_type(&mut nick, "Review");
    nick.send(ENTER);
    nick.wait_for("Confirma o envio para o claude? (s/N)");
    nick.send("n");
    let log = only_log(&world);
    wait_flat(
        &nick,
        &format!(
            "Cancelado. O prompt gerado continua salvo em: {}",
            log.display()
        ),
    );
    assert!(nick.screen().contains("g abrir agente"), "back on the list");
    assert_eq!(world.agents(), []);
    assert!(!logs(&world).claude.exists(), "claude never ran");
    assert!(log.is_file(), "the log stays");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn the_long_text_comes_from_the_editor_with_the_tui_back_whole() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let editor_log = world.root.join("editor.log");
    let editor_log_s = editor_log.display().to_string();
    let mut nick = spawn(&world, "", &[("FAKE_EDITOR_LOG", &editor_log_s)]);
    open_tickets(&mut nick, "proj");
    assert!(open_menu(&mut nick));
    choose_type(&mut nick, "PO - Criar ticket");
    nick.send("Título do pedido PTY");
    nick.send(TAB);
    nick.wait_for("(vazio — Enter abre o editor)");
    nick.send(ENTER);
    nick.wait_for("Descrição do editor falso (2 linhas; Enter edita)");
    assert!(nick.on_alternate_screen(), "the TUI is back");
    assert!(
        nick.screen().contains("Tipo: PO - Criar ticket"),
        "the form is back"
    );
    assert!(
        nick.raw().contains("Descrição do pedido — abrindo "),
        "the line of read_via_editor before the editor"
    );
    let used = read(&editor_log);
    assert_eq!(used.lines().count(), 1, "{used}");
    assert!(
        !Path::new(used.trim()).exists(),
        "the temporary file is gone: {used}"
    );
    // The focus went back to the title: Enter builds the prompt with the text of the editor.
    nick.send(ENTER);
    nick.wait_for("Confirma o envio para o claude? (s/N)");
    let prompt = read(&only_log(&world));
    assert!(
        prompt.contains("Descrição do editor falso\ncom duas linhas.\n"),
        "{prompt}"
    );
    nick.send(ESC);
    nick.wait_for("g abrir agente");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn an_empty_editor_leaves_the_field_empty_with_the_message() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "", &[("FAKE_EDITOR_MODE", "empty")]);
    open_tickets(&mut nick, "proj");
    assert!(open_menu(&mut nick));
    choose_type(&mut nick, "Tech Leader - Criar plano");
    nick.send(TAB);
    nick.send("Título");
    nick.send(TAB);
    nick.send(ENTER);
    nick.wait_for("Esse campo é obrigatório, tente de novo.");
    assert!(nick.on_alternate_screen(), "the TUI is back");
    assert!(nick.screen().contains("(vazio — Enter abre o editor)"));
    assert!(prompt_files(&world).is_empty(), "no render");
    nick.send(ESC);
    nick.wait_for("g abrir agente");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn ctrl_c_in_the_editor_only_interrupts_the_editor() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "", &[("FAKE_EDITOR_MODE", "wait")]);
    open_tickets(&mut nick, "proj");
    assert!(open_menu(&mut nick));
    choose_type(&mut nick, "PO - Criar ticket");
    nick.send(TAB);
    nick.send(ENTER);
    nick.wait_for("EDITOR-PRONTO");
    nick.send("\x03");
    wait_flat(&nick, "ele foi interrompido.");
    assert!(nick.on_alternate_screen(), "the TUI is back");
    assert!(
        nick.screen().contains("Tipo: PO - Criar ticket"),
        "the form stays"
    );
    nick.send(ESC);
    nick.wait_for("g abrir agente");
    // Back in raw mode, Ctrl+C is a key again and quits.
    nick.send("\x03");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_restored_at_the_end(&nick.raw());
}

// ---- the terminal of the agent ----

#[test]
fn a_copy_of_the_agent_reaches_the_outer_terminal() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "osc52", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("terminou.");
    assert!(
        nick.raw().contains("\x1b]52;c;b2zDoQ==\x07"),
        "{:?}",
        nick.raw()
    );
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn a_paste_reaches_claude_with_a_single_end_marker() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "paste", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    // The outer terminal pastes "x ESC" (its own brackets around it): the ESC inside the paste
    // must not reach claude, so the pasted text can't close the bracket early.
    nick.send("\x1b[200~x\x1b\x1b[201~");
    nick.wait_for("GOT 1b 5b 32 30 30 7e 78 1b 5b 32 30 31 7e");
    nick.wait_for("terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn a_flood_ends_and_the_list_comes_back() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "flood", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn a_resize_reaches_claude_below_the_header() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "winsize", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    // The panel beside the list (NS-34): 100 - 40 columns less the 2 of its border (NS-37),
    // and 30 rows less the 2 of the border, the header (two rows: the warning out of the jail
    // wraps at 58 columns), the status and the help (two rows: the ones the help of the
    // tickets needs, whatever the focus; review NS-34, P1): 30 - 2 - 2 - 1 - 2.
    nick.wait_for("SIZE 23 58");
    // At 120 columns the list takes 40 and the header fits in one row: 40 - 2 - 1 - 1 - 2.
    nick.resize(40, 120);
    nick.wait_for("SIZE 34 78");
    // Ctrl+C goes to the program (cooked in this mode: it ends it).
    nick.send("\x03");
    nick.wait_for("O agente proj · PO · NS-1 terminou");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

// ---- leaving: signals and panic ----

fn assert_signal_ends_the_agent(signal: Signal, code: u32) {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "sleep", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    let pids = pids(&world);
    assert_eq!(pids.len(), 2);
    assert!(pids.iter().all(|p| alive(*p)), "control: both alive");
    kill(Pid::from_raw(nick.pid()), signal).unwrap();
    assert_eq!(nick.wait_exit(), Some(code), "{signal}");
    assert_restored_at_the_end(&nick.raw());
    for pid in pids {
        wait_dead(&nick, pid);
    }
    let agents = world.agents();
    assert_eq!(agents[0].3, "stopped", "{signal}");
    assert!(
        !world
            .agents_dir()
            .join(format!("{}.lock", agents[0].0))
            .exists()
    );
}

#[test]
fn sigterm_ends_the_agent_group_and_exits_with_143() {
    assert_signal_ends_the_agent(Signal::SIGTERM, 143);
}

#[test]
fn sighup_ends_the_agent_group_and_exits_with_129() {
    assert_signal_ends_the_agent(Signal::SIGHUP, 129);
}

#[test]
fn sigterm_without_an_agent_restores_and_exits_with_143() {
    let world = World::new();
    let mut nick = world.spawn(&world.root, &[]);
    nick.wait_for(" Navegação ");
    kill(Pid::from_raw(nick.pid()), Signal::SIGTERM).unwrap();
    assert_eq!(nick.wait_exit(), Some(143));
    assert_restored_at_the_end(&nick.raw());
}

#[test]
fn a_panic_with_an_agent_open_ends_its_group_and_exits_with_101() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "sleep", &[("NICK_TEST_PANIC", "agent")]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    let pids = pids(&world);
    assert_eq!(nick.wait_exit(), Some(101));
    let raw = nick.raw();
    let off = raw.rfind(ALT_OFF).expect("restored");
    let message = raw
        .find("NICK_TEST_PANIC: panic de teste com um agente aberto")
        .unwrap_or_else(|| panic!("the panic message: {raw:?}"));
    assert!(off < message, "restored before the message");
    for pid in pids {
        wait_dead(&nick, pid);
    }
    // The database is not touched: the next opening ends the agent.
    assert_eq!(world.agents()[0].3, "active");
    let mut again = world.spawn(&world.root, &[]);
    again.wait_for(" Navegação ");
    assert_eq!(world.agents()[0].3, "stopped");
    again.send("q");
    assert_eq!(again.wait_exit(), Some(0));
}

// ---- the cleanup when the nick opens ----

#[test]
fn an_active_agent_without_its_lock_is_ended_when_the_nick_opens() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    world.active_agent(&proj, "NS-1", "morto");
    let mut nick = world.spawn(&world.root, &[]);
    nick.wait_for(" Navegação ");
    assert_eq!(world.agents()[0].3, "stopped");
    assert!(!world.agents_dir().join("morto.lock").exists());
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn an_agent_whose_lock_another_nick_holds_stays_active() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    world.active_agent(&proj, "NS-1", "vivo");
    nick::paths::create_state_dir(&world.agents_dir()).unwrap();
    let lock = fs::File::create(world.agents_dir().join("vivo.lock")).unwrap();
    lock.lock().unwrap();
    let mut nick = world.spawn(&world.root, &[]);
    nick.wait_for(" Navegação ");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_eq!(world.agents()[0].3, "active");
    drop(lock);
    // Control: with the lock free, the next opening ends it.
    let mut again = world.spawn(&world.root, &[]);
    again.wait_for(" Navegação ");
    assert_eq!(world.agents()[0].3, "stopped");
    again.send("q");
    assert_eq!(again.wait_exit(), Some(0));
}

// ---- several agents side by side (NS-34) ----

/// The reserved shortcut by default, Ctrl+\ (0x1c in the PTY).
const SHORTCUT: &str = "\x1c";

/// A world with the projects `alfa` (in the jail) and `beta` (out of it), each with the
/// ticket NS-1, and a directory for the logs of each fake claude by project.
fn world_two() -> World {
    let world = World::new();
    let alfa = world.project("alfa", &["JAIL_ENABLED=\"true\""]);
    let beta = world.project("beta", &["JAIL_ENABLED=\"false\""]);
    for dir in [&alfa, &beta] {
        world.register(dir);
        world.ticket(dir, "NS-1", "Primeiro");
    }
    fs::create_dir_all(world.root.join("logs")).unwrap();
    world
}

/// Runs the nick of `world_two` with the fakes in the mode `sleep` (changed by project in
/// `extra`, FAKE_CLAUDE_MODE_<project>), their logs by project and `extra`.
fn spawn_two(world: &World, extra: &[(&str, &str)]) -> Nick {
    let logs = world.root.join("logs").display().to_string();
    let mut all = vec![("FAKE_CLAUDE_LOG_DIR", logs.as_str())];
    all.extend_from_slice(extra);
    spawn(world, "sleep", &all)
}

/// Waits until `text` is on the screen read as one line (a wrapped help line reads whole).
fn wait_flat(nick: &Nick, text: &str) {
    nick.wait_until(&format!("{text:?} on the screen"), || {
        nick.flat_screen().contains(text)
    });
}

/// Opens `alfa · Tech Leader · NS-1` (in the jail), back to the list by the shortcut, then
/// `beta · Dev · NS-1` (out of it), shown with the focus on its terminal; both READY.
fn open_both(nick: &mut Nick) {
    open_tickets(nick, "alfa");
    open_agent(nick, 1);
    nick.wait_for_panel("alfa · Tech Leader · NS-1 · na jail");
    nick.wait_for_panel("READY");
    open_beta(nick, 1);
    nick.wait_for_panel("READY");
}

/// With an agent open on the tickets of alfa, focused: back to the list, the projects, and the
/// agent of beta (`running` agents in the end).
fn open_beta(nick: &mut Nick, running: usize) {
    nick.send(SHORTCUT);
    wait_flat(nick, &format!("v agentes ativos ({running})"));
    open_tickets(nick, "beta");
    open_agent(nick, 2);
    nick.wait_for_panel(&format!("beta · Dev · NS-1 · {OFF_JAIL}"));
}

fn lock_of(world: &World, session_id: &str) -> PathBuf {
    world.agents_dir().join(format!("{session_id}.lock"))
}

#[test]
fn two_agents_of_two_projects_run_at_once_each_with_its_cwd_and_config() {
    let world = world_two();
    let mut nick = spawn_two(&world, &[]);
    open_both(&mut nick);
    let pids = pids(&world);
    assert_eq!(pids.len(), 4, "two fakes and their sleeps");
    assert!(pids.iter().all(|p| alive(*p)), "both alive at once");
    for (project, name) in [
        ("alfa", "alfa · Tech Leader · NS-1"),
        ("beta", "beta · Dev · NS-1"),
    ] {
        let env = read(&world.root.join(format!("logs/{project}.env")));
        let pwd = format!("PWD={}", world.root.join(project).display());
        assert!(env.lines().any(|l| l == pwd), "{project}: {env}");
        let argv = read(&world.root.join(format!("logs/{project}.argv")));
        assert!(argv.contains(&format!("--name\n{name}\n")), "{argv}");
    }
    // Only alfa goes through the ai-jail (its config), beta runs out of it.
    let jail = read(&logs(&world).jail);
    assert!(jail.contains("alfa · Tech Leader · NS-1"), "{jail}");
    assert!(!jail.contains("beta"), "{jail}");
    let agents = world.agents();
    assert_eq!(agents.len(), 2);
    assert!(agents.iter().all(|a| a.3 == "active"));
    assert!(agents.iter().all(|a| lock_of(&world, &a.0).exists()));
    // The filter of the active agents (NS-38; before, the view) lists both, [FORA DA JAIL]
    // only on beta.
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (2)");
    nick.send("v");
    nick.wait_for("(só ativos)");
    nick.wait_for("Dev  ● vivo  [FORA DA JAIL]");
    // Only the tree: on the rows of the left, inside its border (NS-37).
    let rows = nick.tree_rows();
    let alfa = rows
        .iter()
        .find(|row| row.contains("Tech Leader  ● vivo"))
        .unwrap_or_else(|| panic!("alfa on the tree:\n{}", nick.screen()));
    assert!(!alfa.contains("FORA DA JAIL"), "{alfa}");
    let _ = PANEL_COL;
    nick.send("q");
    nick.wait_for("Há 2 agentes ativos (alfa, beta). Encerrar todos e sair? (s/N)");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn the_shortcut_toggles_the_focus_and_never_reaches_claude() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "2")]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    nick.wait_for("Ctrl+\\ voltar à lista");
    // On the list, q is the list's: it asks to quit, and claude gets nothing.
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (1)");
    nick.send("q");
    nick.wait_for("Há 1 agente ativo (proj). Encerrar e sair? (s/N)");
    nick.send("n");
    nick.wait_until("the confirmation closed", || {
        !nick.screen().contains("Há 1 agente")
    });
    // On the terminal, q is claude's; the shortcut, both ways, never reaches it.
    nick.send(SHORTCUT);
    nick.wait_for("Ctrl+\\ voltar à lista");
    nick.send("q");
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (1)");
    nick.send(SHORTCUT);
    nick.wait_for("Ctrl+\\ voltar à lista");
    nick.send("z");
    nick.wait_for("GOT 71 7a");
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn a_hidden_agent_whose_text_changed_is_marked_until_it_is_shown() {
    let world = world_two();
    let trigger = world.root.join("trigger");
    let trigger_text = trigger.display().to_string();
    let mut nick = spawn_two(
        &world,
        &[
            ("FAKE_CLAUDE_MODE_alfa", "trigger"),
            ("FAKE_CLAUDE_TRIGGER", trigger_text.as_str()),
        ],
    );
    open_both(&mut nick);
    // alfa is hidden. A redraw of the same text (another color) does not mark it.
    fs::write(world.root.join("trigger.1"), "").unwrap();
    nick.wait_until("the redraw of alfa", || {
        world.root.join("trigger.1.done").exists()
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (2)");
    assert!(
        !nick.flat_screen().contains("saída nova"),
        "{}",
        nick.screen()
    );
    // The branch of alfa closed (the selection goes over its agent on the way: being shown,
    // it is seen, and it is hidden again once the selection leaves it).
    nick.select("alfa");
    nick.send(LEFT);
    nick.wait_for("▸ alfa");
    assert!(!nick.screen().contains("saída nova"), "{}", nick.screen());
    // New text marks it: on its closed project (NS-38: the mark goes up) and on the help.
    fs::write(world.root.join("trigger.2"), "").unwrap();
    nick.wait_for("▸ alfa  ● saída nova");
    wait_flat(&nick, "v agentes ativos (2, 1 com saída nova)");
    // With the filter of the active agents, on its node, whole: in the place of `● vivo` (review
    // of the NS-38, P1).
    nick.send("v");
    nick.wait_for("(só ativos)");
    nick.wait_for("Tech Leader  ● saída nova");
    wait_flat(&nick, "v mostrar tudo (2, 1 com saída nova)");
    // Shown, it is seen: the mark goes.
    nick.select("Tech Leader");
    nick.wait_for_panel("alfa · Tech Leader · NS-1 · na jail");
    nick.wait_for_panel("NOVO");
    nick.wait_until("the mark gone", || !nick.screen().contains("saída nova"));
    nick.send("q");
    nick.wait_for("Encerrar todos e sair?");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

/// Review NS-34, P1: toggling the focus does not change the size of the terminals, so a hidden
/// claude gets no SIGWINCH (which redraws it and marks it with no new output).
#[test]
fn toggling_the_focus_neither_resizes_nor_marks_a_hidden_agent() {
    let world = world_two();
    let mut nick = spawn_two(&world, &[("FAKE_CLAUDE_MODE_alfa", "winsize")]);
    open_tickets(&mut nick, "alfa");
    open_agent(&mut nick, 1);
    nick.wait_for_panel("SIZE");
    open_beta(&mut nick, 1);
    nick.wait_for_panel("READY");
    for _ in 0..3 {
        nick.send(SHORTCUT);
        wait_flat(&nick, "v agentes ativos (2)");
        nick.send(SHORTCUT);
        nick.wait_for("Ctrl+\\ voltar à lista");
    }
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (2)");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(
        !nick.flat_screen().contains("saída nova"),
        "{}",
        nick.screen()
    );
    // Shown by selecting its node on the tree (NS-38).
    nick.select("Tech Leader");
    nick.wait_for_panel("alfa · Tech Leader · NS-1 · na jail");
    let panel = nick.panel();
    assert_eq!(
        panel.matches("SIZE").count(),
        1,
        "a single size, never resized: {panel}"
    );
    nick.send("q");
    nick.wait_for("Encerrar todos e sair?");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

/// The symbol at `row`, `col` of the screen and whether it is cyan (`ui::FOCUS_COLOR`, the
/// border of the focused panel: SGR 36).
fn border_at(nick: &Nick, row: u16, col: u16) -> (String, bool) {
    let (symbol, color) = nick.cell(row, col);
    (symbol, color == vt100::Color::Idx(6))
}

/// Waits until the focused panel is the agent's (`on_terminal`) or the list's: its corner is
/// `┏` and cyan, the other one `┌` and without a color (NS-37).
fn wait_for_focus_border(nick: &Nick, on_terminal: bool) {
    let (focused, other) = if on_terminal {
        (common::PANEL_COL, 0)
    } else {
        (0, common::PANEL_COL)
    };
    nick.wait_until(&format!("the thick border on column {focused}"), || {
        border_at(nick, 0, focused) == ("┏".into(), true)
            && border_at(nick, 0, other) == ("┌".into(), false)
    });
}

/// NS-37: the shortcut moves the thick cyan border between the list and the panel at once.
#[test]
fn toggling_the_focus_moves_the_thick_border_at_once() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "sleep", &[]);
    open_tickets(&mut nick, "proj");
    // Without an agent too, the tree has the focus and the thick border (NS-38, decision 1;
    // before, the plain border of the NS-37).
    assert_eq!(border_at(&nick, 0, 0), ("┏".into(), true));
    assert_eq!(border_at(&nick, 0, PANEL_COL), ("┌".into(), false));
    open_agent(&mut nick, 0);
    nick.wait_for_panel("READY");
    wait_for_focus_border(&nick, true);
    // The panel reads only the inside of the border: the header wrapped at 58 columns whole,
    // and no piece of the border (its top, its bottom or a side).
    let panel = nick.panel();
    assert!(
        panel.contains("FORA DA JAIL: o agente roda direto no seu shell."),
        "{panel}"
    );
    assert!(
        !panel.contains(['┏', '┓', '┗', '┛', '━', '┃', '│']),
        "{panel}"
    );
    nick.keep_screen("NS-37: foco no terminal");
    nick.send(SHORTCUT);
    wait_for_focus_border(&nick, false);
    nick.keep_screen("NS-37: foco na lista");
    nick.send(SHORTCUT);
    wait_for_focus_border(&nick, true);
    nick.send(SHORTCUT);
    wait_for_focus_border(&nick, false);
    nick.send("q");
    nick.wait_for("Encerrar e sair?");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

/// NS-37 (review, N2): `Nick::panel` cuts only the border of the panel. A rule and a box row the
/// program draws across its whole width (as the real claude does) stay whole, ends included.
#[test]
fn the_panel_keeps_the_rules_the_program_draws_up_to_the_border() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "rules", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for_panel("READY");
    // The terminal of the agent: 100 - 40 columns less the 2 of the border.
    let width = usize::from(common::COLS - PANEL_COL - 2);
    let rule = "─".repeat(width);
    let boxed = format!("│{:<1$}│", "caixa", width - 2);
    let panel = nick.panel();
    assert!(panel.contains(&rule), "the rule whole:\n{panel}");
    assert!(panel.contains(&boxed), "the box row whole:\n{panel}");
    // And nothing of the border of the panel itself.
    assert!(!panel.contains(['┃', '┏', '┓', '┗', '┛', '━']), "{panel}");
    // Back to the list, so the q is the nick's and not the program's.
    nick.send(SHORTCUT);
    wait_for_focus_border(&nick, false);
    nick.send("q");
    nick.wait_for("Encerrar e sair?");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn only_the_copy_of_the_shown_agent_reaches_the_outer_terminal() {
    const COPY: &str = "\x1b]52;c;b2zDoQ==\x07";
    let world = world_two();
    let trigger = world.root.join("trigger");
    let trigger_text = trigger.display().to_string();
    let mut nick = spawn_two(
        &world,
        &[
            ("FAKE_CLAUDE_MODE_alfa", "osc52_on_trigger"),
            ("FAKE_CLAUDE_TRIGGER", trigger_text.as_str()),
        ],
    );
    open_both(&mut nick);
    // alfa, hidden, copies: dropped.
    fs::write(world.root.join("trigger.1"), "").unwrap();
    nick.wait_until("the copy of alfa", || {
        world.root.join("trigger.1.done").exists()
    });
    std::thread::sleep(std::time::Duration::from_millis(500));
    nick.send(SHORTCUT);
    // The text of alfa changed (COPIADO 1): it is marked.
    wait_flat(&nick, "v agentes ativos (2, 1 com saída nova)");
    nick.select("Tech Leader");
    nick.wait_for_panel("COPIADO 1");
    assert!(
        !nick.raw().contains(COPY),
        "the copy of a hidden agent was forwarded"
    );
    // Control: shown, its copy goes to the outer terminal.
    fs::write(world.root.join("trigger.2"), "").unwrap();
    nick.wait_for_panel("COPIADO 2");
    nick.wait_for_raw(COPY);
    nick.send("q");
    nick.wait_for("Encerrar todos e sair?");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn a_shortcut_saved_in_the_settings_works_right_away_and_at_the_next_opening() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "1")]);
    nick.wait_for(" Navegação ");
    nick.send("c");
    nick.wait_for_tree("Atalho para alternar entre a lista e o terminal: Ctrl+\\ (padrão)");
    nick.send("t");
    nick.wait_for("Aperte a tecla do novo atalho. Esc cancela.");
    nick.send("\x1d");
    nick.wait_for("Aperte Ctrl+] de novo para confirmar.");
    assert_eq!(
        world.setting("reserved_shortcut"),
        None,
        "once is not saved"
    );
    nick.send("\x1d");
    nick.wait_for("Atalho salvo: Ctrl+].");
    assert_eq!(
        world.setting("reserved_shortcut").as_deref(),
        Some("ctrl+]")
    );
    nick.wait_for_tree("Atalho para alternar entre a lista e o terminal: Ctrl+]");
    assert!(!nick.screen().contains("(padrão)"));
    // Right away: Ctrl+] toggles, and Ctrl+\ goes to claude.
    nick.send(ESC);
    nick.wait_for("c configurações");
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    nick.wait_for("Ctrl+] voltar à lista");
    nick.send("\x1d");
    wait_flat(&nick, "v agentes ativos (1)");
    nick.send("\x1d");
    nick.wait_for("Ctrl+] voltar à lista");
    nick.send(SHORTCUT);
    nick.wait_for("GOT 1c");
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    // At the next opening, from the database; p goes back to the default.
    let mut again = world.spawn(&world.root, &[]);
    again.wait_for(" Navegação ");
    again.send("c");
    again.wait_for_tree("Atalho para alternar entre a lista e o terminal: Ctrl+]");
    assert!(!again.screen().contains("(padrão)"));
    again.send("p");
    again.wait_for("Atalho de volta ao padrão: Ctrl+\\.");
    again.wait_for("Ctrl+\\ (padrão)");
    assert_eq!(world.setting("reserved_shortcut"), None);
    again.send("q");
    assert_eq!(again.wait_exit(), Some(0));
}

#[test]
fn the_capture_refuses_enter_and_ctrl_c_without_saving_nor_quitting() {
    let world = World::new();
    let mut nick = world.spawn(&world.root, &[]);
    nick.wait_for(" Navegação ");
    nick.send("c");
    nick.wait_for("(padrão)");
    nick.send("t");
    nick.wait_for("Aperte a tecla do novo atalho.");
    nick.send(ENTER);
    nick.wait_for("O claude precisa dessa tecla (Enter). Escolha outra.");
    nick.send("\x03");
    nick.wait_for("O claude precisa dessa tecla (Ctrl+C). Escolha outra.");
    assert!(
        nick.screen().contains("Aperte a tecla do novo atalho."),
        "still capturing"
    );
    assert_eq!(world.setting("reserved_shortcut"), None);
    nick.send(ESC);
    nick.wait_for("t trocar");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_eq!(world.setting("reserved_shortcut"), None);
}

#[test]
fn an_agent_ended_from_the_view_dies_alone() {
    let world = world_two();
    let mut nick = spawn_two(&world, &[]);
    open_both(&mut nick);
    let pids = pids(&world);
    let (alfa, beta) = (&pids[..2], &pids[2..]);
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (2)");
    // `x` on the node of the agent (NS-38; before, on the view).
    nick.select("Tech Leader");
    nick.wait_for_panel("alfa · Tech Leader · NS-1 · na jail");
    // n: nothing changes.
    nick.send("x");
    nick.wait_for("Encerrar o agente alfa · Tech Leader · NS-1? (s/N)");
    nick.send("n");
    nick.wait_until("the confirmation closed", || {
        !nick.screen().contains("Encerrar o agente")
    });
    assert!(pids.iter().all(|p| alive(*p)));
    assert!(world.agents().iter().all(|a| a.3 == "active"));
    // s: the group of alfa dies, its row ends, its lock goes; beta goes on.
    nick.send("x");
    nick.wait_for("Encerrar o agente alfa");
    nick.send("s");
    nick.wait_for("O agente alfa · Tech Leader · NS-1 foi encerrado.");
    for pid in alfa {
        wait_dead(&nick, *pid);
    }
    assert!(beta.iter().all(|p| alive(*p)), "beta goes on");
    let agents = world.agents();
    assert_eq!(
        (agents[0].3.as_str(), agents[1].3.as_str()),
        ("stopped", "active")
    );
    assert!(!lock_of(&world, &agents[0].0).exists());
    assert!(lock_of(&world, &agents[1].0).exists());
    // The ended one stays on its node (decision 10 of NS-38); beta is shown when selected.
    nick.wait_for("Tech Leader  parado");
    nick.select("Dev  ● vivo");
    nick.wait_for_panel("beta · Dev · NS-1");
    nick.send("q");
    nick.wait_for("Há 1 agente ativo (beta). Encerrar e sair? (s/N)");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
    for pid in beta {
        wait_dead(&nick, *pid);
    }
}

#[test]
fn quitting_with_two_agents_asks_and_ends_both() {
    let world = world_two();
    let mut nick = spawn_two(&world, &[]);
    open_both(&mut nick);
    let pids = pids(&world);
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (2)");
    nick.send("q");
    nick.wait_for("Há 2 agentes ativos (alfa, beta). Encerrar todos e sair? (s/N)");
    nick.send("n");
    nick.wait_until("the confirmation closed", || {
        !nick.screen().contains("Há 2 agentes")
    });
    assert!(pids.iter().all(|p| alive(*p)), "n changes nothing");
    assert!(world.agents().iter().all(|a| a.3 == "active"));
    nick.send("q");
    nick.wait_for("Encerrar todos e sair?");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_restored_at_the_end(&nick.raw());
    for pid in pids {
        wait_dead(&nick, pid);
    }
    let agents = world.agents();
    assert!(agents.iter().all(|a| a.3 == "stopped"), "{agents:?}");
    assert!(agents.iter().all(|a| !lock_of(&world, &a.0).exists()));
}

/// Review NS-34, N3: an error of the database while ending the agents on the way out is
/// printed after the terminal is restored; the locks go anyway.
#[test]
fn an_error_of_the_database_on_the_way_out_is_printed_after_the_restore() {
    let world = world_two();
    let mut nick = spawn_two(&world, &[]);
    open_both(&mut nick);
    nick::db::Db::open(&world.db_path())
        .unwrap()
        .connection()
        .execute_batch(
            "CREATE TRIGGER fail BEFORE UPDATE ON agents \
             BEGIN SELECT RAISE(ABORT, 'falha de teste'); END;",
        )
        .unwrap();
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (2)");
    nick.send("q");
    nick.wait_for("Encerrar todos e sair?");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
    let raw = nick.raw();
    let off = raw.rfind(ALT_OFF).expect("restored");
    let message = raw.rfind("falha de teste").expect("the error is printed");
    assert!(off < message, "after the restore: {raw:?}");
    let agents = world.agents();
    assert!(agents.iter().all(|a| !lock_of(&world, &a.0).exists()));
}

#[test]
fn sigterm_with_two_agents_ends_both_and_exits_with_143() {
    let world = world_two();
    let mut nick = spawn_two(&world, &[]);
    open_both(&mut nick);
    let pids = pids(&world);
    assert_eq!(pids.len(), 4);
    kill(Pid::from_raw(nick.pid()), Signal::SIGTERM).unwrap();
    assert_eq!(nick.wait_exit(), Some(143));
    assert_restored_at_the_end(&nick.raw());
    for pid in pids {
        wait_dead(&nick, pid);
    }
    let agents = world.agents();
    assert!(agents.iter().all(|a| a.3 == "stopped"), "{agents:?}");
    assert!(agents.iter().all(|a| !lock_of(&world, &a.0).exists()));
}

#[test]
fn a_panic_with_two_agents_ends_both_groups_and_exits_with_101() {
    let world = world_two();
    let mut nick = spawn_two(&world, &[("NICK_TEST_PANIC", "second_agent")]);
    open_tickets(&mut nick, "alfa");
    open_agent(&mut nick, 1);
    nick.wait_for_panel("READY");
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (1)");
    open_tickets(&mut nick, "beta");
    open_agent(&mut nick, 2);
    nick.wait_until("both fakes started", || pids(&world).len() == 4);
    let pids = pids(&world);
    assert_eq!(nick.wait_exit(), Some(101));
    assert!(
        nick.raw()
            .contains("NICK_TEST_PANIC: panic de teste com um agente aberto"),
        "{:?}",
        nick.raw()
    );
    for pid in pids {
        wait_dead(&nick, pid);
    }
}

#[test]
fn a_hidden_agent_that_floods_does_not_block_the_shown_one() {
    let world = world_two();
    let trigger = world.root.join("trigger");
    let trigger_text = trigger.display().to_string();
    let mut nick = spawn_two(
        &world,
        &[
            ("FAKE_CLAUDE_MODE_alfa", "flood_on_trigger"),
            ("FAKE_CLAUDE_MODE_beta", "echo"),
            ("FAKE_CLAUDE_READ_BYTES", "2"),
            ("FAKE_CLAUDE_TRIGGER", trigger_text.as_str()),
        ],
    );
    open_both(&mut nick);
    // alfa, hidden, writes ~1 MB that nobody looks at, while beta answers.
    fs::write(world.root.join("trigger.1"), "").unwrap();
    nick.send("a");
    nick.wait_for("O agente alfa · Tech Leader · NS-1 terminou.");
    nick.send("b");
    nick.wait_for_panel("GOT 61 62");
    assert!(!nick.panel().contains("xxx"), "{}", nick.panel());
    nick.wait_for("O agente beta · Dev · NS-1 terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

// ---- structure: the only launch is bash with the launcher ----

/// Every .rs file under src/, with its text.
fn sources() -> Vec<(String, String)> {
    fn walk(dir: &Path, out: &mut Vec<(String, String)>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push((
                    path.display().to_string(),
                    fs::read_to_string(&path).unwrap(),
                ));
            }
        }
    }
    let mut out = Vec::new();
    walk(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut out);
    out
}

/// The violations of the launch rule in `source`: a command named claude or ai-jail, a
/// --permission-mode, a PTY command other than bash. Returns (PTY commands, violations).
fn launch_violations(source: &str) -> (usize, Vec<String>) {
    let mut violations = Vec::new();
    let compact: String = source.chars().filter(|c| !c.is_whitespace()).collect();
    for builder in ["Command::new(", "CommandBuilder::new("] {
        for (i, _) in compact.match_indices(builder) {
            let arg = &compact[i + builder.len()..];
            if arg.starts_with("\"claude\"") || arg.starts_with("\"ai-jail\"") {
                violations.push(format!("{builder} launches {}", &arg[..9]));
            }
            if builder == "Command::new(" && !arg.starts_with("\"bash\"") {
                violations.push(format!("Command::new of {}", &arg[..arg.len().min(12)]));
            }
        }
    }
    let pty = compact.matches("CommandBuilder::new(").count();
    for (i, _) in compact.match_indices("CommandBuilder::new(") {
        if !compact[i..].starts_with("CommandBuilder::new(\"bash\");command.arg(launcher);") {
            violations.push("a PTY command other than bash <launcher>".into());
        }
    }
    if source.contains("--permission-mode") {
        violations.push("--permission-mode".into());
    }
    (pty, violations)
}

#[test]
fn the_only_process_in_a_pty_is_bash_with_the_launcher() {
    let mut pty = 0;
    for (name, source) in sources() {
        let (count, violations) = launch_violations(&source);
        pty += count;
        assert_eq!(violations, Vec::<String>::new(), "{name}");
        // The prototype is a reference only (comments may cite it); no code depends on it.
        let code: Vec<&str> = source
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect();
        assert!(
            !code.iter().any(|l| l.contains("prototype/")),
            "{name} depends on the prototype"
        );
    }
    assert_eq!(pty, 1, "a single CommandBuilder::new(\"bash\") in src/");
    // Positive control: the filter sees each kind of violation.
    for bad in [
        "let c = CommandBuilder::new(\"claude\");",
        "std::process::Command::new(\"ai-jail\")",
        "Command::new(\"sh\")",
        "let mut c = CommandBuilder::new(\"bash\"); c.arg(other);",
        "args([\"--permission-mode\", \"auto\"])",
    ] {
        assert!(!launch_violations(bad).1.is_empty(), "{bad}");
    }
    let good = "let mut command = CommandBuilder::new(\"bash\");\n        command.arg(launcher);";
    assert_eq!(launch_violations(good), (1, Vec::new()));
}

/// The violations of the prompt rule (NS-35) in `source`, comments left out: a piece of the
/// prompt assembly (templates/, envsubst, exchange_block, build_*, *_BLOCK), the name of a
/// template, or a read of the config of the project. The prompt is built by bash only.
fn prompt_violations(source: &str) -> Vec<String> {
    let code: String = source
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let mut violations = Vec::new();
    for needle in [
        "templates/",
        "envsubst",
        "exchange_block",
        "build_",
        "_BLOCK",
    ] {
        if code.contains(needle) {
            violations.push(needle.to_string());
        }
    }
    for template in [
        "po",
        "po_discussion",
        "po_jira",
        "po_sentry",
        "tech_leader",
        "tech_leader_jira",
        "development",
        "review",
    ] {
        let name = format!("{template}.md");
        // The name alone (`"po.md"`, `/po.md`), not the end of another name.
        let found = code.match_indices(&name).any(|(at, _)| {
            !code[..at]
                .chars()
                .next_back()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
        });
        if found {
            violations.push(name);
        }
    }
    let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
    for read in ["read_to_string(", "fs::read("] {
        for (at, _) in compact.match_indices(read) {
            let call: String = compact[at..].chars().take(80).collect();
            if call.contains("nick.conf") || call.contains("config_file(") {
                violations.push(format!("{read} of the config"));
            }
        }
    }
    violations
}

#[test]
fn no_rust_source_builds_the_prompt_nor_reads_the_config() {
    for (name, source) in sources() {
        let mut violations = prompt_violations(&source);
        // NS-36 (decision of the user): src/docs.rs names the artifact of the ticket,
        // `.nick/docs/<ID>/review.md`, not the template of the Review; only that name is
        // exempt there, every other rule still applies to it.
        if Path::new(&name).ends_with("src/docs.rs") {
            violations.retain(|v| v != "review.md");
        }
        assert_eq!(violations, Vec::<String>::new(), "{name}");
    }
    // The exemption is only of docs.rs: the same name elsewhere is still caught.
    assert_eq!(
        prompt_violations("pub const REVIEW_FILE_NAME: &str = \"review.md\";"),
        ["review.md"]
    );
    // Positive control: the filter sees each kind of violation, and not a comment.
    for bad in [
        "let t = root.join(\"auto_scrum/templates/\");",
        "Command::new(\"bash\").arg(\"-c\").arg(\"envsubst < x\")",
        "let s = exchange_block();",
        "build_po_blocks();",
        "let b = PO_VALIDATION_BLOCK;",
        "let t = \"po.md\";",
        "dir.join(\"tech_leader_jira.md\")",
        "fs::read_to_string(project.join(\".nick/nick.conf\"))",
        "std::fs::read_to_string( projects::config_file(&dir) )",
    ] {
        assert!(!prompt_violations(bad).is_empty(), "{bad}");
    }
    assert_eq!(
        prompt_violations("// envsubst and po.md in a comment"),
        Vec::<String>::new()
    );
    assert_eq!(
        prompt_violations("let log = \"logs/x_1.md\"; let p = \"plan.md\";"),
        Vec::<String>::new()
    );
}

// ---- structure: the stage of a ticket and the writes in a project (NS-36) ----

/// The production code of a source: without its `#[cfg(test)] mod tests` (the unit tests write
/// files and call everything on purpose) and without comment lines.
fn production(source: &str) -> String {
    let code = source
        .split("#[cfg(test)]\nmod tests")
        .next()
        .unwrap_or_default();
    code.lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The violations of the stage rule (plan D2/D9 of NS-36) in the production code of the file
/// `name` (a path ending in `src/...`): `change_ticket_stage` only in db/mod.rs (its
/// definition) and in tickets.rs (`change_stage`); `change_stage(` only in tickets.rs and, in
/// app.rs, a single call inside the confirmation of the dialog; no write of the stage nor of
/// its history outside src/db/.
fn stage_violations(name: &str, code: &str) -> Vec<String> {
    let mut violations = Vec::new();
    let file = name.rsplit("src/").next().unwrap_or(name);
    if code.contains("change_ticket_stage") && !matches!(file, "db/mod.rs" | "tickets.rs") {
        violations.push("change_ticket_stage outside db/ and tickets.rs".into());
    }
    let calls: Vec<usize> = code
        .match_indices("change_stage(")
        .map(|(at, _)| at)
        .filter(|at| !code[..*at].ends_with("ticket_"))
        .collect();
    match file {
        "tickets.rs" | "db/mod.rs" => {}
        "app.rs" => {
            if calls.len() != 1 {
                violations.push(format!("{} calls of change_stage in app.rs", calls.len()));
            }
            for at in &calls {
                let before = &code[at.saturating_sub(600)..*at];
                let arm = before.rfind("Some(Dialog::ConfirmStageChange");
                let other = before.rfind("Some(Dialog::").filter(|o| Some(*o) != arm);
                if arm.is_none() || other > arm {
                    violations.push("change_stage outside the confirmation".into());
                }
            }
        }
        _ if !calls.is_empty() => violations.push(format!("change_stage in {file}")),
        _ => {}
    }
    if !file.starts_with("db/") {
        for needle in ["UPDATE tickets SET stage", "ticket_stage_changes"] {
            if code.contains(needle) {
                violations.push(format!("{needle} outside db/"));
            }
        }
    }
    violations
}

#[test]
fn the_stage_is_only_written_by_the_confirmation_of_the_operator() {
    let mut app_calls = 0;
    for (name, source) in sources() {
        let code = production(&source);
        assert_eq!(
            stage_violations(&name, &code),
            Vec::<String>::new(),
            "{name}"
        );
        if name.ends_with("src/app.rs") {
            app_calls = code.matches("tickets::change_stage(").count();
        }
    }
    assert_eq!(app_calls, 1, "the confirmation calls it");
    // Positive control: the filter sees each kind of violation, and not a comment nor a test.
    let confirmation = "Some(Dialog::ConfirmStageChange { ticket, to }) => {\n    if is_yes(key) \
                        { tickets::change_stage(&self.db, &ticket, to); }\n}";
    assert_eq!(
        stage_violations("src/app.rs", confirmation),
        Vec::<String>::new()
    );
    for (name, bad) in [
        (
            "src/app.rs",
            "pub fn agent_ended(&mut self) { tickets::change_stage(&db, &t, d); }",
        ),
        (
            "src/app.rs",
            "Some(Dialog::ConfirmHide { ticket }) => { tickets::change_stage(&db, &t, d); }",
        ),
        (
            "src/agents.rs",
            "tickets::change_stage(db, &t, TicketStage::Done)",
        ),
        (
            "src/app.rs",
            "self.db.change_ticket_stage(id, \"po\", \"done\")",
        ),
        (
            "src/agents.rs",
            "conn.execute(\"UPDATE tickets SET stage = 'done'\", [])",
        ),
        (
            "src/app.rs",
            "\"INSERT INTO ticket_stage_changes VALUES (1)\"",
        ),
    ] {
        assert!(
            !stage_violations(name, &production(bad)).is_empty(),
            "{bad}"
        );
    }
    let two = format!("{confirmation}\n{confirmation}");
    assert!(
        !stage_violations("src/app.rs", &two).is_empty(),
        "two calls"
    );
    assert_eq!(
        production(
            "// tickets::change_stage(db, &t, d)\n#[cfg(test)]\nmod tests { change_stage(); }"
        ),
        ""
    );
}

/// The calls that write in a project. The only one allowed is the rename of docs.rs.
const PROJECT_WRITES: [&str; 8] = [
    "renameat2",
    "fs::rename",
    "remove_dir",
    "remove_file",
    "create_dir",
    "fs::write",
    "File::create",
    "OpenOptions",
];

/// The uses that already existed, none of them in a project: the lock of an agent, in the
/// state dir of the nick (agents.rs, NS-33), is opened with `OpenOptions` and removed with
/// `remove_file` when it is released. (The temporary file of the editor, NS-35, is a
/// `tempfile::NamedTempFile` in the temporary dir, not one of these calls.)
const PROJECT_WRITE_EXCEPTIONS: [(&str, &str); 2] =
    [("agents.rs", "OpenOptions"), ("agents.rs", "remove_file")];

fn project_write_violations(name: &str, code: &str) -> Vec<String> {
    let file = name.rsplit("src/").next().unwrap_or(name);
    if file == "docs.rs" {
        return Vec::new();
    }
    PROJECT_WRITES
        .into_iter()
        .filter(|needle| code.contains(needle))
        .filter(|needle| !PROJECT_WRITE_EXCEPTIONS.contains(&(file, needle)))
        .map(|needle| format!("{needle} in {file}"))
        .collect()
}

#[test]
fn no_source_but_docs_rs_writes_in_a_project() {
    let mut docs_renames = false;
    for (name, source) in sources() {
        let code = production(&source);
        assert_eq!(
            project_write_violations(&name, &code),
            Vec::<String>::new(),
            "{name}"
        );
        if name.ends_with("src/docs.rs") {
            docs_renames = code.contains("renameat2(");
        }
    }
    assert!(docs_renames, "docs.rs renames with renameat2");
    // Positive control: each call is seen outside docs.rs, the exceptions only in agents.rs.
    for needle in PROJECT_WRITES {
        let bad = format!("let _ = std::{needle}(dir);");
        assert!(
            !project_write_violations("src/tickets.rs", &bad).is_empty(),
            "{needle}"
        );
    }
    assert!(!project_write_violations("src/app.rs", "std::fs::remove_file(p)").is_empty());
    assert_eq!(
        project_write_violations("src/agents.rs", "std::fs::remove_file(p)"),
        Vec::<String>::new()
    );
    assert_eq!(
        project_write_violations("src/docs.rs", "std::fs::rename(a, b)"),
        Vec::<String>::new()
    );
}

// ---- the ticket screen with agents (NS-36) ----

/// The stored stage of the ticket `key`.
fn stage_of(world: &World, key: &str) -> String {
    let db = nick::db::Db::open(&world.db_path()).unwrap();
    db.connection()
        .query_row("SELECT stage FROM tickets WHERE key = ?1", [key], |row| {
            row.get(0)
        })
        .unwrap()
}

/// From the node of a live agent: `←` up to its ticket, its panel on the right (NS-38; before,
/// Enter opened the ticket screen).
fn open_ticket_screen(nick: &mut Nick) {
    nick.send(LEFT);
    wait_flat(nick, "p épico");
}

#[test]
fn a_live_agent_opens_from_the_ticket_screen_whatever_the_stage() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "1")]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (1)");
    open_ticket_screen(&mut nick);
    nick.wait_for("PO  ● vivo");
    nick.keep_screen("painel do ticket com o agente vivo na árvore");
    // The ticket goes to Dev.
    nick.send("m");
    nick.wait_for("Para qual etapa?");
    for label in ["> Tech Leader", "> Dev"] {
        nick.send(DOWN);
        nick.wait_for(label);
    }
    nick.send(ENTER);
    wait_flat(&nick, "Mudar a etapa do ticket NS-1 de PO para Dev? (s/N)");
    nick.send("s");
    wait_flat(&nick, "Etapa do ticket NS-1: PO → Dev.");
    assert_eq!(stage_of(&world, "NS-1"), "development");
    // Its PO agent opens from its node, with the focus on its terminal.
    nick.select("PO  ● vivo");
    nick.send(ENTER);
    nick.wait_for("Ctrl+\\ voltar à lista");
    nick.send("x");
    nick.wait_for("GOT 78");
    wait_flat(&nick, "O agente proj · PO · NS-1 terminou.");
    nick.wait_for("PO  parado");
    assert_eq!(
        stage_of(&world, "NS-1"),
        "development",
        "the end changes no stage"
    );
    assert_eq!(world.agents()[0].3, "stopped");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn editing_the_id_with_a_live_agent_is_refused() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    fs::create_dir_all(proj.join(".nick/docs/NS-1")).unwrap();
    fs::write(proj.join(".nick/docs/NS-1/plan.md"), "# plano\n").unwrap();
    let mut nick = spawn(&world, "sleep", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (1)");
    open_ticket_screen(&mut nick);
    let before = common::snapshot(&proj);
    nick.send("i");
    nick.wait_for("Novo ID do ticket (ex: SC-123):");
    nick.send("9");
    nick.send(ENTER);
    wait_flat(
        &nick,
        "O ticket NS-1 tem agente ativo. Encerre os agentes dele antes de editar",
    );
    nick.keep_screen("recusa de editar ID com agente ativo");
    assert_eq!(common::snapshot(&proj), before);
    assert_eq!(
        world.tickets(Some(&proj)),
        [("NS-1".into(), "Primeiro".into(), false)]
    );
    nick.send(ESC);
    wait_flat(&nick, "p épico");
    nick.send("q");
    nick.wait_for("Encerrar e sair? (s/N)");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn the_dev_form_without_a_plan_warns_and_opens_anyway() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "", &[]);
    open_tickets(&mut nick, "proj");
    assert!(open_menu(&mut nick));
    choose_type(&mut nick, "Desenvolvimento");
    wait_flat(
        &nick,
        "Aviso: não existe .nick/docs/NS-1/plan.md. Informe outro caminho no campo do plano, \
         ou siga sem ele.",
    );
    nick.keep_screen("formulário do Dev com o aviso de plano ausente");
    nick.send(ENTER);
    nick.wait_for("Confirma o envio para o claude? (s/N)");
    nick.send("s");
    nick.wait_for("O agente proj · Dev · NS-1 terminou.");
    assert_eq!(world.agents().len(), 1);
    assert_eq!(
        stage_of(&world, "NS-1"),
        "po",
        "opening a Dev changes no stage"
    );
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

// ---- the tree (NS-38) ----

/// Criterion 3 of NS-38: from the module down to the terminal of a live agent only with the
/// arrows and Enter, through no other screen.
#[test]
fn from_the_module_to_the_terminal_of_a_live_agent_only_with_the_arrows_and_enter() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "sleep", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for_panel("READY");
    // Back on the tree, every branch closed: up to the module (← on a leaf goes up; on an open
    // node it closes it).
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (1)");
    for row in [
        "▾ NS-1",
        "▸ NS-1",
        "▾ proj",
        "▸ proj",
        "▾ auto_scrum",
        "▸ auto_scrum",
    ] {
        nick.send(LEFT);
        nick.wait_until(&format!("{row} selected"), || {
            nick.selected_row()
                .is_some_and(|(_, sel)| sel.contains(row))
        });
    }
    nick.keep_screen("árvore fechada no módulo");
    assert_eq!(nick.tree_rows()[1], "", "only the module on the tree");
    // → opens, ↓ goes down, → opens, ↓, →, ↓ and Enter: the terminal of the agent.
    let steps = [
        (RIGHT, "▾ auto_scrum"),
        (DOWN, "▸ proj"),
        (RIGHT, "▾ proj"),
        (DOWN, "▸ NS-1"),
        (RIGHT, "▾ NS-1"),
        (DOWN, "PO  ● vivo"),
    ];
    for (key, row) in steps {
        nick.send(key);
        nick.wait_until(&format!("{row:?} selected"), || {
            nick.selected_row()
                .is_some_and(|(_, sel)| sel.contains(row))
        });
    }
    nick.keep_screen("agente vivo selecionado na árvore");
    nick.wait_for_panel("proj · PO · NS-1 · FORA DA JAIL");
    nick.send(ENTER);
    nick.wait_for("Ctrl+\\ voltar à lista");
    wait_for_focus_border(&nick, true);
    nick.keep_screen("terminal do agente com o foco");
    nick.send(SHORTCUT);
    nick.send("q");
    nick.wait_for("Encerrar e sair? (s/N)");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

/// Decision 2 of NS-38: the shortcut from a node that is not a live agent goes to the last
/// agent shown (its node selected, the thick border on its panel), and again back to the tree
/// on the same agent.
#[test]
fn the_shortcut_from_a_ticket_goes_to_the_last_agent_and_back() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "sleep", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for_panel("READY");
    nick.send(SHORTCUT);
    wait_for_focus_border(&nick, false);
    // The ticket selected: no agent shown, its panel on the right.
    nick.send(LEFT);
    wait_flat(&nick, "p épico");
    nick.wait_for(" Ticket NS-1 ");
    nick.keep_screen("ticket selecionado com um agente vivo escondido");
    nick.send(SHORTCUT);
    nick.wait_for("Ctrl+\\ voltar à lista");
    nick.wait_for_panel("READY");
    wait_for_focus_border(&nick, true);
    let (_, row) = nick.selected_row().expect("a selection");
    assert!(row.contains("PO  ● vivo"), "the node of the agent: {row}");
    nick.send(SHORTCUT);
    wait_for_focus_border(&nick, false);
    wait_flat(&nick, "Enter ir ao terminal");
    let (_, row) = nick.selected_row().expect("a selection");
    assert!(row.contains("PO  ● vivo"), "the same agent: {row}");
    nick.send("q");
    nick.wait_for("Encerrar e sair? (s/N)");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

/// Moving the selection between the module, a project, a ticket and an agent, and the two
/// filters, resize no terminal: the claude gets no SIGWINCH (each one redraws it and marks a
/// hidden one). The fake prints a SIZE at the start and at each SIGWINCH.
#[test]
fn moving_on_the_tree_sends_no_sigwinch_to_claude() {
    let (world, _) = world_with(&["JAIL_ENABLED=\"false\""]);
    world.ticket(&world.root.join("proj"), "NS-2", "Segundo");
    let mut nick = spawn(&world, "winsize", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for_panel("SIZE 23 58");
    nick.send(SHORTCUT);
    wait_flat(&nick, "Enter ir ao terminal");
    // The ticket, the project, the module, down again to the other ticket, the filters.
    // ← goes up from a leaf and closes an open node; → opens.
    for (key, help) in [
        (LEFT, "p épico"),
        (LEFT, "p épico"),
        (LEFT, "a novo ticket"),
        (LEFT, "a novo ticket"),
        (LEFT, "a adicionar projeto"),
        (LEFT, "a adicionar projeto"),
        (RIGHT, "a adicionar projeto"),
        (DOWN, "a novo ticket"),
        (RIGHT, "a novo ticket"),
        (DOWN, "p épico"),
        (RIGHT, "p épico"),
        (DOWN, "Enter ir ao terminal"),
        (DOWN, "p épico"),
        ("f", "f esconder ocultos"),
        ("v", "v mostrar tudo"),
        ("v", "v agentes ativos"),
        ("f", "f mostrar ocultos"),
        (DOWN, "Enter ir ao terminal"),
    ] {
        nick.send(key);
        wait_flat(&nick, help);
    }
    std::thread::sleep(std::time::Duration::from_millis(300));
    nick.wait_for_panel("SIZE 23 58");
    let panel = nick.panel();
    assert_eq!(panel.matches("SIZE").count(), 1, "never resized: {panel}");
    nick.send("q");
    nick.wait_for("Encerrar e sair? (s/N)");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

/// The panel on the right of each kind of node, beside a live agent and an ended one (the
/// screens of the validation of NS-38 come from here, with `NICK_TEST_SCREENS`).
#[test]
fn each_kind_of_node_has_its_panel_and_its_help() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    world.ticket(&proj, "NS-2", "Segundo");
    world.hide_ticket(&proj, "NS-2");
    let mut nick = spawn(&world, "sleep", &[]);
    nick.wait_for(" Navegação ");
    nick.wait_for("Projetos: 1");
    nick.keep_screen("módulo fechado, sem agente (sempre lado a lado)");
    // An ended agent (the first) and a live one (the second), both of NS-1.
    open_tickets(&mut nick, "proj");
    nick.keep_screen("ticket selecionado, sem agente");
    open_agent(&mut nick, 0);
    nick.wait_for_panel("READY");
    nick.send(SHORTCUT);
    wait_flat(&nick, "x encerrar");
    nick.send("x");
    nick.wait_for("(s/N)");
    nick.send("s");
    wait_flat(&nick, "O agente proj · PO · NS-1 foi encerrado.");
    nick.select("NS-1");
    open_agent(&mut nick, 1);
    nick.wait_for_panel("READY");
    nick.keep_screen("agente vivo com o foco no terminal");
    nick.send(SHORTCUT);
    wait_flat(&nick, "Enter ir ao terminal");
    nick.keep_screen("agente vivo com o foco na árvore");
    for (label, title, help, screen) in [
        (
            "PO  parado",
            " Agente ",
            "↑/↓ mover · r retomar · o arquivar · ← subir",
            "agente parado",
        ),
        ("NS-1", " Ticket NS-1 ", "p épico", "ticket com agentes"),
        ("proj", " Projeto proj ", "a novo ticket", "projeto"),
        (
            "auto_scrum",
            " auto_scrum ",
            "a adicionar projeto",
            "módulo",
        ),
    ] {
        nick.select(label);
        nick.wait_for(title);
        wait_flat(&nick, help);
        nick.keep_screen(&format!(
            "{screen} selecionado, com um agente vivo escondido"
        ));
    }
    nick.wait_for("Agentes vivos neste nick: 1");
    // The filters: the hidden ticket shows; only the live branches.
    nick.send("f");
    nick.wait_for(" Navegação (com ocultos) ");
    nick.wait_for("NS-2  [PO]  Segundo  [oculto]");
    nick.keep_screen("filtro de ocultos");
    nick.send("v");
    nick.wait_for(" Navegação (com ocultos) (só ativos) ");
    nick.keep_screen("filtro de ocultos e de agentes ativos");
    nick.send("f");
    nick.wait_for(" Navegação (só ativos) ");
    assert!(!nick.screen().contains("NS-2"), "{}", nick.screen());
    assert!(
        !nick.screen().contains("PO  parado"),
        "only the live agents"
    );
    nick.keep_screen("filtro de agentes ativos");
    nick.send("q");
    nick.wait_for("Encerrar e sair? (s/N)");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

/// Removing a project ends its agents (decision of the user after the NS-38): the live one of
/// this nick dies with its group, its record and its lock go, the project and its files stay
/// out of it (no file of the project is touched).
#[test]
fn removing_a_project_ends_its_live_agent() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "sleep", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for_panel("READY");
    let group = pids(&world);
    assert!(group.iter().all(|p| alive(*p)));
    let session = world.agents()[0].0.clone();
    let before = common::snapshot(&proj);
    nick.send(SHORTCUT);
    wait_flat(&nick, "Enter ir ao terminal");
    nick.select("proj");
    wait_flat(&nick, "r remover");
    nick.send("r");
    wait_flat(
        &nick,
        "O agente vivo dele neste nick será encerrado. O ticket dele será apagado do banco. O \
         agente registrado dele será apagado do banco. (s/N)",
    );
    nick.keep_screen("remover projeto com um agente vivo");
    nick.send("s");
    wait_flat(
        &nick,
        "Projeto removido do nick: proj. Nenhum arquivo foi apagado. O agente proj · PO · NS-1 \
         foi encerrado.",
    );
    for pid in group {
        wait_dead(&nick, pid);
    }
    assert_eq!(world.stored(), Vec::<PathBuf>::new());
    assert_eq!(world.agents(), []);
    assert!(!lock_of(&world, &session).exists(), "the lock is gone");
    assert_eq!(
        common::snapshot(&proj),
        before,
        "no file of the project touched"
    );
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0), "no agent left: q quits at once");
}

// ---- resuming and archiving the agents (NS-39) ----

/// The file where claude keeps the conversation `session` of a session run at `proj`: under the
/// HOME of the world (no CLAUDE_CONFIG_DIR: the environment is clean), the physical path of the
/// project with every character but A-Z, a-z and 0-9 as "-".
fn conversation_file(world: &World, proj: &Path, session: &str) -> PathBuf {
    let dir: String = fs::canonicalize(proj)
        .unwrap()
        .display()
        .to_string()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    world
        .home()
        .join(".claude/projects")
        .join(dir)
        .join(format!("{session}.jsonl"))
}

/// Writes the conversation of `session` (as claude would).
fn keep_conversation(world: &World, proj: &Path, session: &str) {
    let file = conversation_file(world, proj, session);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, "{}\n").unwrap();
}

/// Opens the PO agent (`po_discussion`) on NS-1 of `proj`, whose fake claude (mode `echo`, one
/// byte) is ended by a key: its node is left selected, stopped. Returns its session id.
fn open_and_stop(nick: &mut Nick, world: &World) -> String {
    open_tickets(nick, "proj");
    open_agent(nick, 0);
    nick.wait_for("READY");
    nick.send("z");
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    nick.wait_for_flat("r retomar");
    let (_, row) = nick.selected_row().expect("a selection");
    assert!(row.contains("PO  parado"), "{row}");
    world.agents()[0].0.clone()
}

/// The argv of a resume of `session` named `name`: no prompt after the name.
fn assert_resumed(world: &World, session: &str, name: &str) {
    let argv = read(&logs(world).claude);
    assert!(
        argv.ends_with(&format!("--resume\n{session}\n--name\n{name}\n")),
        "{argv}"
    );
    assert!(!argv.contains("--session-id"), "{argv}");
}

#[test]
fn a_stopped_agent_is_resumed_in_its_conversation() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "1")]);
    let session = open_and_stop(&mut nick, &world);
    nick.keep_screen("nó parado selecionado, com o painel");
    assert_eq!(world.agents()[0].3, "stopped");
    keep_conversation(&world, &proj, &session);
    fs::remove_file(logs(&world).claude).unwrap();
    nick.send("r");
    nick.wait_for("READY");
    nick.wait_for_flat("O agente proj · PO · NS-1 foi retomado.");
    assert_resumed(&world, &session, "proj · PO · NS-1");
    assert_eq!(world.agents().len(), 1, "the same agent, no new row");
    assert_eq!(world.agents()[0].3, "active");
    assert!(world.agents_dir().join(format!("{session}.lock")).is_file());
    nick.keep_screen("agente retomado, com o foco no terminal");
    nick.send(SHORTCUT);
    nick.wait_for_flat("Enter ir ao terminal");
    let (_, row) = nick.selected_row().expect("a selection");
    assert!(row.contains("PO  ● vivo"), "{row}");
    // It ends as any agent.
    nick.send(ENTER);
    nick.send("z");
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    assert_eq!(world.agents()[0].3, "stopped");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn closing_and_opening_the_nick_resumes_nothing_until_r() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "sleep", &[]);
    open_tickets(&mut nick, "proj");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    let session = world.agents()[0].0.clone();
    nick.send(SHORTCUT);
    nick.send("q");
    nick.wait_for("Encerrar e sair? (s/N)");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_eq!(world.agents()[0].3, "stopped");
    fs::remove_file(logs(&world).claude).unwrap();
    // Opening again: the node is stopped, and no claude was launched.
    let mut again = spawn(&world, "sleep", &[]);
    open_tickets(&mut again, "proj");
    again.open_node("NS-1");
    again.select("PO  parado");
    assert!(!logs(&world).claude.exists(), "opening resumes nothing");
    assert_eq!(world.agents()[0].3, "stopped");
    keep_conversation(&world, &proj, &session);
    again.send("r");
    again.wait_for("READY");
    assert_resumed(&world, &session, "proj · PO · NS-1");
    again.send(SHORTCUT);
    again.send("q");
    again.wait_for("Encerrar e sair? (s/N)");
    again.send("s");
    assert_eq!(again.wait_exit(), Some(0));
}

#[test]
fn the_resume_uses_the_config_of_now() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "1")]);
    let session = open_and_stop(&mut nick, &world);
    assert!(!logs(&world).jail.exists(), "opened out of the jail");
    fs::write(proj.join(".nick/nick.conf"), "JAIL_ENABLED=\"true\"\n").unwrap();
    keep_conversation(&world, &proj, &session);
    nick.send("r");
    nick.wait_for_panel("proj · PO · NS-1 · na jail");
    nick.wait_for("READY");
    let jail = read(&logs(&world).jail);
    assert!(jail.contains(&format!("--resume\n{session}\n")), "{jail}");
    assert_resumed(&world, &session, "proj · PO · NS-1");
    nick.send("z");
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn a_lost_conversation_opens_nothing_and_the_agent_can_be_archived_and_back() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "1")]);
    let session = open_and_stop(&mut nick, &world);
    fs::remove_file(logs(&world).claude).unwrap();
    nick.send("r");
    nick.wait_for(" Conversa não encontrada ");
    nick.wait_for_flat("não existe mais no claude");
    nick.wait_for_flat("a arquivar · n abrir novo agente nesta etapa · Esc cancelar");
    nick.keep_screen("diálogo da conversa perdida");
    assert!(!logs(&world).claude.exists(), "no PTY opened");
    nick.send("a");
    nick.wait_for_flat("O agente proj · PO · NS-1 foi arquivado.");
    assert_eq!(world.agents()[0].3, "archived");
    assert!(
        !nick.tree_rows().iter().any(|row| row.contains("PO  ")),
        "out of the tree: {}",
        nick.screen()
    );
    // The tree still answers, and the archived one shows with `f`.
    nick.send("f");
    nick.wait_for("PO  [arquivado]");
    nick.select("PO  [arquivado]");
    nick.wait_for_flat("o desarquivar");
    nick.keep_screen("agente arquivado com o filtro f, com o painel");
    nick.send("r");
    nick.wait_for_flat("está arquivado. Desarquive (o) antes de retomar.");
    nick.send("o");
    nick.wait_for_flat("O agente proj · PO · NS-1 voltou para parado.");
    assert_eq!(world.agents()[0].3, "stopped");
    keep_conversation(&world, &proj, &session);
    nick.send("r");
    nick.wait_for("READY");
    assert_resumed(&world, &session, "proj · PO · NS-1");
    nick.send("z");
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn an_agent_active_in_another_nick_is_not_resumed_here() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    world.active_agent(&proj, "NS-1", "vivo");
    // Another nick holds its lock (the mold of
    // an_agent_whose_lock_another_nick_holds_stays_active).
    nick::paths::create_state_dir(&world.agents_dir()).unwrap();
    let lock = fs::File::create(world.agents_dir().join("vivo.lock")).unwrap();
    lock.lock().unwrap();
    keep_conversation(&world, &proj, "vivo");
    let mut nick = spawn(&world, "sleep", &[]);
    open_tickets(&mut nick, "proj");
    nick.open_node("NS-1");
    nick.select("PO  ativo em outro nick");
    nick.wait_for_flat("Ativo em outro nick: feche-o lá para retomar aqui.");
    nick.keep_screen("agente ativo em outro nick, com o painel");
    nick.send("r");
    nick.wait_for_flat("O agente x está ativo em outro nick e não pode ser retomado aqui.");
    assert!(!logs(&world).claude.exists(), "nothing launched");
    assert_eq!(world.agents()[0].3, "active");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    drop(lock);
}

#[test]
fn a_resume_after_the_id_changed_tells_the_new_folder() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "1")]);
    let session = open_and_stop(&mut nick, &world);
    nick.select("NS-1");
    nick.send("i");
    nick.wait_for(" Editar ID ");
    nick.send("\x7f\x7f\x7f\x7f");
    nick.send("NS-9");
    nick.send(ENTER);
    nick.wait_for("(s/N)");
    nick.send("s");
    nick.wait_for_flat("ID atualizado: NS-1 → NS-9.");
    nick.select("PO  parado");
    nick.wait_for_flat("ID na abertura: NS-1");
    nick.keep_screen("agente parado depois de editar o ID do ticket");
    keep_conversation(&world, &proj, &session);
    nick.send("r");
    nick.wait_for("READY");
    nick.wait_for_flat("Ele foi avisado de que a pasta agora é .nick/docs/NS-9/.");
    let argv = read(&logs(&world).claude);
    assert!(argv.contains("--append-system-prompt\n"), "{argv}");
    assert!(argv.contains(".nick/docs/NS-9/"), "{argv}");
    assert_resumed(&world, &session, "proj · PO · NS-1");
    nick.send("z");
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

/// The production code of the sources of src/ (without the `mod tests` and the comments), with
/// the name of each file.
fn production_sources() -> Vec<(String, String)> {
    sources()
        .into_iter()
        .map(|(name, source)| {
            let code = source.split("#[cfg(test)]").next().unwrap_or("");
            let code: Vec<&str> = code
                .lines()
                .filter(|line| !line.trim_start().starts_with("//"))
                .collect();
            (name, code.join("\n"))
        })
        .collect()
}

/// A fixed piece of the notice of the new ID (NS-39): the text is prompt, only in bash.
const ID_NOTICE_PIECE: &str = "depois que esta conversa começou";

#[test]
fn the_notice_of_the_new_id_is_only_in_bash() {
    for (name, code) in production_sources() {
        assert!(!code.contains(ID_NOTICE_PIECE), "{name}");
    }
    // Positive control: the piece is the text of the notice, in auto_scrum/prompt.sh.
    let prompt =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../auto_scrum/prompt.sh"))
            .unwrap();
    assert!(prompt.contains(ID_NOTICE_PIECE));
}

/// `"ended"`/`'ended'` in `code`.
fn ended_literals(code: &str) -> usize {
    code.matches("\"ended\"").count() + code.matches("'ended'").count()
}

#[test]
fn no_rust_source_writes_the_state_ended() {
    for (name, code) in production_sources() {
        if name.ends_with("db/migrations.rs") {
            continue;
        }
        assert_eq!(ended_literals(&code), 0, "{name}");
    }
    // Positive controls: the migrations name it (the comment of the 3 and the UPDATE of the 6),
    // and the filter finds both quotes.
    let migrations = sources()
        .into_iter()
        .find(|(name, _)| name.ends_with("db/migrations.rs"))
        .expect("db/migrations.rs is read")
        .1;
    assert_eq!(ended_literals(&migrations), 2);
    assert_eq!(ended_literals("state = 'ended' or \"ended\""), 2);
}

// ---- the epics (NS-40) ----

/// From the node of the project `proj`, `n` and the dialog answered: the epic `key` created and
/// selected.
fn new_epic(nick: &mut Nick, title: &str, key: &str) {
    nick.select("proj");
    nick.send("n");
    nick.wait_for(" Novo épico ");
    nick.send(title);
    nick.send(TAB);
    nick.send(key);
    nick.send(ENTER);
    nick.wait_for_flat(&format!("Épico criado: {key} — {title}."));
}

#[test]
fn an_epic_from_its_creation_to_the_resume_of_its_agent() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    let mut nick = spawn(&world, "echo", &[("FAKE_CLAUDE_READ_BYTES", "1")]);
    nick.open_project("proj");
    new_epic(&mut nick, "Plano longo", "EPIC-1");
    let (_, row) = nick.selected_row().expect("the epic selected");
    assert!(row.contains("EPIC-1  [épico]  Plano longo"), "{row}");
    nick.wait_for_flat("Agentes do épico: só PO e Tech Leader.");
    nick.keep_screen("painel do épico");
    // `g`: only the types of PO and Tech Leader.
    assert!(open_menu(&mut nick));
    let screen = nick.screen();
    assert!(screen.contains("Épico EPIC-1 — Plano longo"), "{screen}");
    assert!(
        !screen.contains("Desenvolvimento") && !screen.contains("Review"),
        "{screen}"
    );
    choose_type(&mut nick, "PO - Conversar para definir ticket");
    nick.send("Assunto do épico");
    nick.send(ENTER);
    nick.wait_for("Confirma o envio para o claude? (s/N)");
    nick.send("s");
    nick.wait_for("READY");
    let argv = read(&logs(&world).claude);
    assert!(argv.contains("--name\nproj · PO · EPIC-1\n"), "{argv}");
    let session = world.agents()[0].0.clone();
    nick.send("z");
    nick.wait_for("O agente proj · PO · EPIC-1 terminou.");
    nick.wait_for_flat("r retomar");
    let rows = nick.tree_rows();
    let at = |label: &str| rows.iter().position(|r| r.contains(label)).unwrap();
    assert!(
        at("EPIC-1  [épico]") < at("PO  parado"),
        "the agent under the epic: {rows:?}"
    );
    assert!(
        at("PO  parado") < at("NS-1"),
        "the agents of the epic before the tickets: {rows:?}"
    );
    nick.keep_screen("árvore com o épico aberto: o agente do épico (parado) e o ticket sem épico");
    keep_conversation(&world, &proj, &session);
    fs::remove_file(logs(&world).claude).unwrap();
    nick.send("r");
    nick.wait_for("READY");
    nick.wait_for_flat("O agente proj · PO · EPIC-1 foi retomado.");
    assert_resumed(&world, &session, "proj · PO · EPIC-1");
    nick.send("z");
    nick.wait_for("O agente proj · PO · EPIC-1 terminou.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn moving_a_ticket_with_a_live_agent_touches_no_file_and_the_agent_goes_on() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    world.epic(&proj, "EPIC-1", "Plano longo", &[]);
    fs::create_dir_all(proj.join(".nick/docs/NS-1")).unwrap();
    fs::create_dir_all(proj.join(".nick/docs/EPIC-1")).unwrap();
    let mut nick = spawn(&world, "sleep", &[]);
    nick.open_project("proj");
    nick.select("NS-1");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    let pid = pids(&world)[0];
    nick.send(SHORTCUT);
    nick.wait_for_flat("Enter ir ao terminal");
    nick.select("NS-1");
    let before = common::snapshot(&proj);
    nick.send("p");
    nick.wait_for(" Mover ticket ");
    nick.wait_for_flat("Sem épico (atual)");
    nick.keep_screen("diálogo Mover ticket");
    nick.send(DOWN);
    nick.send(ENTER);
    nick.wait_for_flat("Ticket NS-1 movido para o épico EPIC-1.");
    assert_eq!(
        common::snapshot(&proj),
        before,
        "no file of the project touched"
    );
    assert!(alive(pid), "the agent goes on");
    assert_eq!(world.agents()[0].3, "active");
    // Reachable by the shortcut, now under the epic.
    nick.send(SHORTCUT);
    nick.wait_for_flat(&format!("{} voltar à lista", "Ctrl+\\"));
    nick.send(SHORTCUT);
    nick.wait_for_flat("Enter ir ao terminal");
    let rows = nick.tree_rows();
    let at = |label: &str| rows.iter().position(|r| r.contains(label)).unwrap();
    assert!(
        at("EPIC-1  [épico]") < at("NS-1") && at("NS-1") < at("PO  ● vivo"),
        "{rows:?}"
    );
    nick.keep_screen("agente na profundidade 4 (ticket dentro do épico)");
    nick.select("NS-1");
    nick.wait_for_flat("Épico: EPIC-1 — Plano longo");
    nick.keep_screen("painel do ticket com épico");
    nick.send("q");
    nick.wait_for("(s/N)");
    nick.send("s");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn hiding_an_epic_is_refused_with_a_live_agent_and_then_takes_and_brings_back_the_branch() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    world.ticket(&proj, "NS-2", "Segundo");
    world.hide_ticket(&proj, "NS-2");
    world.epic(&proj, "EPIC-1", "Plano longo", &["NS-1", "NS-2"]);
    let before = world.tickets(Some(&proj));
    let mut nick = spawn(&world, "sleep", &[]);
    nick.open_project("proj");
    nick.open_node("EPIC-1");
    nick.select("NS-1");
    open_agent(&mut nick, 0);
    nick.wait_for("READY");
    nick.send(SHORTCUT);
    nick.wait_for_flat("Enter ir ao terminal");
    nick.select("EPIC-1");
    nick.send("o");
    nick.wait_for_flat(
        "O épico EPIC-1 tem 1 agente ativo (no épico ou nos tickets dele). Encerre-os antes de \
         ocultar.",
    );
    assert!(!nick.screen().contains("(s/N)"), "no dialog");
    nick.keep_screen("recusa de ocultar o épico com agente ativo");
    nick.select("PO  ● vivo");
    nick.send("x");
    nick.wait_for("(s/N)");
    nick.send("s");
    nick.wait_for_flat("foi encerrado.");
    nick.select("EPIC-1");
    nick.send("o");
    nick.wait_for_flat("O ramo inteiro (2 tickets) sai da árvore e volta com f. (s/N)");
    nick.send("s");
    nick.wait_for_flat("Épico ocultado: EPIC-1.");
    nick.wait_until("the branch out of the tree", || {
        !nick
            .tree_rows()
            .iter()
            .any(|r| r.contains("[épico]") || r.contains("NS-1  [PO]"))
    });
    nick.send("f");
    // The `[oculto]` is past the 38 columns at depth 2 (the end of a row is cut, NS-34).
    nick.wait_for(" Navegação (com ocultos) ");
    nick.wait_for_tree("EPIC-1  [épico]  Plano longo");
    nick.keep_screen("épico oculto com f");
    nick.select("EPIC-1");
    nick.send("o");
    nick.wait_for_flat("Épico restaurado: EPIC-1.");
    assert_eq!(
        world.tickets(Some(&proj)),
        before,
        "every ticket as it was (NS-2 still hidden)"
    );
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}

#[test]
fn a_real_version_6_database_opens_migrated_with_the_tree_of_before() {
    let world = World::new();
    let proj = world.project("proj", &["JAIL_ENABLED=\"false\""]);
    nick::paths::create_state_dir(world.db_path().parent().unwrap()).unwrap();
    {
        let old = nick::db::Db::open_with(&world.db_path(), &nick::db::migrations::MIGRATIONS[..6])
            .unwrap();
        let canonical = fs::canonicalize(&proj).unwrap();
        let p = old
            .insert_project("auto_scrum", "proj", canonical.to_str().unwrap())
            .unwrap();
        let conn = old.connection();
        conn.execute(
            "INSERT INTO tickets (project_id, key, is_provisional, title, created_at) \
             VALUES (?1, 'NS-1', 0, 'Primeiro', '2026-11-20T10:00:00Z')",
            [p.id],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO agents (ticket_id, stage, session_id, name, opened_at, state, opened_key) \
             VALUES (1, 'po', 's-1', 'proj · PO · NS-1', '2026-11-20T10:00:00Z', 'stopped', 'NS-1')",
            [],
        )
        .unwrap();
        let version: u32 = conn
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .unwrap();
        assert_eq!(version, 6);
    }
    let mut nick = spawn(&world, "sleep", &[]);
    nick.open_project("proj");
    nick.open_node("NS-1");
    nick.wait_for_tree("PO  parado");
    let rows: Vec<String> = nick
        .tree_rows()
        .into_iter()
        .filter(|r| !r.trim().is_empty())
        .collect();
    assert!(
        !rows.iter().any(|r| r.contains("[épico]")),
        "no epic: {rows:?}"
    );
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    let db = nick::db::Db::open(&world.db_path()).unwrap();
    let version: u32 = db
        .connection()
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .unwrap();
    assert_eq!(version, 7);
    assert_eq!(world.agents()[0].3, "stopped");
}

/// A fixed piece of the notice of the new ID of an epic (NS-40): prompt, so only in bash.
const EPIC_NOTICE_PIECE: &str = "A pasta do épico agora é";

#[test]
fn the_notice_of_the_new_id_of_an_epic_is_only_in_bash() {
    for (name, code) in production_sources() {
        assert!(!code.contains(EPIC_NOTICE_PIECE), "{name}");
    }
    let prompt =
        fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../auto_scrum/prompt.sh"))
            .unwrap();
    assert!(prompt.contains(EPIC_NOTICE_PIECE), "positive control");
}

/// The writes of `tickets.epic_id` and the calls of `set_ticket_epic` in `code` (NS-40).
fn epic_moves(code: &str) -> (usize, usize) {
    (
        code.matches("UPDATE tickets SET epic_id").count(),
        code.matches("set_ticket_epic(").count(),
    )
}

#[test]
fn only_move_ticket_moves_a_ticket_between_epics() {
    let mut calls = Vec::new();
    for (name, code) in production_sources() {
        let (updates, sets) = epic_moves(&code);
        if !name.contains("/db/") {
            assert_eq!(updates, 0, "{name}: no UPDATE of epic_id out of src/db/");
        }
        if sets > 0 {
            calls.push((name, sets));
        }
    }
    calls.sort();
    let calls: Vec<(String, usize)> = calls
        .into_iter()
        .map(|(name, n)| (name.rsplit("src/").next().unwrap().to_string(), n))
        .collect();
    assert_eq!(
        calls,
        [("db/mod.rs".to_string(), 1), ("epics.rs".to_string(), 1)],
        "the definition and the one call, in epics::move_ticket"
    );
    let epics = production_sources()
        .into_iter()
        .find(|(name, _)| name.ends_with("epics.rs"))
        .unwrap()
        .1;
    let start = epics.find("pub fn move_ticket").expect("move_ticket");
    assert!(
        epics[start..].contains("set_ticket_epic("),
        "inside move_ticket"
    );
    // Positive control: the filter finds both.
    assert_eq!(
        epic_moves("db.set_ticket_epic(1, None); \"UPDATE tickets SET epic_id = 1\""),
        (1, 1)
    );
}
