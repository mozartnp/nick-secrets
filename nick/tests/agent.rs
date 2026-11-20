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

use common::{DOWN, ENTER, ESC, Nick, TAB, World};
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

/// From the Modules screen, opens the tickets of the project `name`.
fn open_tickets(nick: &mut Nick, name: &str) {
    nick.wait_for("Módulos");
    nick.send(ENTER);
    nick.wait_for(&format!("> {name}"));
    nick.send(ENTER);
    nick.wait_for("g abrir agente");
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
    assert!(nick.screen().contains("g abrir agente"), "back on the list");
    assert!(nick.screen().contains("> NS-1"), "the same ticket selected");
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
    let env = read(&logs(&world).env);
    assert!(
        env.lines().any(|l| l == format!("PWD={}", proj.display())),
        "{env}"
    );
    assert!(!logs(&world).jail.exists(), "no ai-jail out of the jail");
    assert_eq!(world.agents()[0].3, "ended");
    assert!(!lock.exists(), "the lock is gone");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_restored_at_the_end(&nick.raw());
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
    assert_eq!(world.agents()[0].3, "ended");
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
    nick.wait_for("Módulos");
    nick.send(ENTER);
    nick.wait_for("> alfa");
    nick.send(DOWN);
    nick.wait_for("> beta");
    nick.send(ENTER);
    nick.wait_for("g abrir agente");
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
    // The panel beside the list (NS-34): 100 - 40 columns, and 30 rows less the header (two
    // rows: the warning out of the jail wraps at 60 columns), the status and the help (two
    // rows: the ones the help of the tickets needs, whatever the focus; review NS-34, P1).
    nick.wait_for("SIZE 25 60");
    // At 120 columns the list takes 40 and the header fits in one row: 40 - 1 - 1 - 2.
    nick.resize(40, 120);
    nick.wait_for("SIZE 36 80");
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
    assert_eq!(agents[0].3, "ended", "{signal}");
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
    nick.wait_for("Módulos");
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
    again.wait_for("Módulos");
    assert_eq!(world.agents()[0].3, "ended");
    again.send("q");
    assert_eq!(again.wait_exit(), Some(0));
}

// ---- the cleanup when the nick opens ----

#[test]
fn an_active_agent_without_its_lock_is_ended_when_the_nick_opens() {
    let (world, proj) = world_with(&["JAIL_ENABLED=\"false\""]);
    world.active_agent(&proj, "NS-1", "morto");
    let mut nick = world.spawn(&world.root, &[]);
    nick.wait_for("Módulos");
    assert_eq!(world.agents()[0].3, "ended");
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
    nick.wait_for("Módulos");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_eq!(world.agents()[0].3, "active");
    drop(lock);
    // Control: with the lock free, the next opening ends it.
    let mut again = world.spawn(&world.root, &[]);
    again.wait_for("Módulos");
    assert_eq!(world.agents()[0].3, "ended");
    again.send("q");
    assert_eq!(again.wait_exit(), Some(0));
}

// ---- several agents side by side (NS-34) ----

/// The reserved shortcut by default, Ctrl+\ (0x1c in the PTY).
const SHORTCUT: &str = "\x1c";
const UP: &str = "\x1b[A";

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
    nick.send(ESC);
    nick.wait_for("r remover");
    nick.send(DOWN);
    nick.wait_for("> beta");
    nick.send(ENTER);
    nick.wait_for("auto_scrum > beta > Tickets");
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
    // The view lists both, [FORA DA JAIL] only on beta.
    nick.send(SHORTCUT);
    wait_flat(&nick, "v agentes ativos (2)");
    nick.send("v");
    nick.wait_for("Agentes ativos");
    nick.wait_for("beta · Dev · NS-1  [FORA DA JAIL]");
    let screen = nick.screen();
    let alfa = screen
        .lines()
        .find(|l| l.contains("alfa · Tech Leader · NS-1"))
        .unwrap_or_else(|| panic!("alfa on the view:\n{screen}"));
    assert!(!alfa.contains("FORA DA JAIL"), "{alfa}");
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
    nick.send("v");
    nick.wait_for("Agentes ativos");
    assert!(!nick.screen().contains("saída nova"), "{}", nick.screen());
    // New text marks it, on the view (the list of 40 columns cuts the end of this line) and on
    // the help of the lists.
    fs::write(world.root.join("trigger.2"), "").unwrap();
    nick.wait_for("alfa · Tech Leader · NS-1  ● saída");
    nick.send(ESC);
    wait_flat(&nick, "v agentes ativos (2, 1 com saída nova)");
    nick.send("v");
    nick.wait_for("● saída");
    // Shown, it is seen: the mark goes.
    nick.send(UP);
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
    nick.send("v");
    nick.wait_for("Agentes ativos");
    nick.send(UP);
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
    nick.send("v");
    nick.wait_for("Agentes ativos");
    nick.send(UP);
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
    nick.wait_for("Módulos");
    nick.send("c");
    nick.wait_for("Atalho para alternar entre a lista e o terminal: Ctrl+\\ (padrão)");
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
    nick.wait_for("Atalho para alternar entre a lista e o terminal: Ctrl+]");
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
    again.wait_for("Módulos");
    again.send("c");
    again.wait_for("Atalho para alternar entre a lista e o terminal: Ctrl+]");
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
    nick.wait_for("Módulos");
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
    nick.send("v");
    nick.wait_for("Agentes ativos");
    nick.send(UP);
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
        ("ended", "active")
    );
    assert!(!lock_of(&world, &agents[0].0).exists());
    assert!(lock_of(&world, &agents[1].0).exists());
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
    assert!(agents.iter().all(|a| a.3 == "ended"), "{agents:?}");
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
    assert!(agents.iter().all(|a| a.3 == "ended"), "{agents:?}");
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
    nick.send(ESC);
    nick.wait_for("r remover");
    nick.send(DOWN);
    nick.wait_for("> beta");
    nick.send(ENTER);
    nick.wait_for("auto_scrum > beta > Tickets");
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
        assert_eq!(prompt_violations(&source), Vec::<String>::new(), "{name}");
    }
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
