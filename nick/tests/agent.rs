//! Integration tests of the agents (NS-33): the real `nick` binary in a PTY (tests/common), with
//! the real bin/nick-agent and the fake claude and ai-jail of bin/tests/fixtures/bin first in the
//! PATH, a temporary HOME and XDG_STATE_HOME (never the operator's database) and a clean
//! environment. Nothing real is launched. Rule of the NS-32: after an ESC, wait for a text that
//! only exists in the next state.

mod common;

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};

use common::{DOWN, ENTER, ESC, Nick, World};
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

/// Runs the nick with the fakes first in the PATH, their logs in the world, the claude mode and
/// `extra`.
fn spawn(world: &World, mode: &str, extra: &[(&str, &str)]) -> Nick {
    let l = logs(world);
    let path = common::fake_path();
    let (claude, env, jail, pids) = (
        l.claude.display().to_string(),
        l.env.display().to_string(),
        l.jail.display().to_string(),
        l.pids.display().to_string(),
    );
    let mut all = vec![
        ("PATH", path.as_str()),
        ("FAKE_CLAUDE_MODE", mode),
        ("FAKE_CLAUDE_LOG", claude.as_str()),
        ("FAKE_CLAUDE_ENV", env.as_str()),
        ("FAKE_JAIL_LOG", jail.as_str()),
        ("FAKE_CLAUDE_PIDS", pids.as_str()),
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

/// `g`, `downs` x ↓ and Enter: opens the agent of that stage on the selected ticket.
fn open_agent(nick: &mut Nick, downs: usize) {
    nick.send("g");
    nick.wait_for("Abrir agente");
    for _ in 0..downs {
        nick.send(DOWN);
    }
    nick.send(ENTER);
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_default()
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
    // The argv: the session id of the database and the name; the cwd is the project.
    let argv = read(&logs(&world).claude);
    assert!(
        argv.ends_with(&format!(
            "--session-id\n{session_id}\n--name\nproj · Dev · NS-1\n"
        )),
        "{argv}"
    );
    assert!(!argv.contains("--permission-mode"), "{argv}");
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
