//! Integration tests of the TUI: the real `nick` binary in a PTY (tests/common), with a
//! temporary HOME and XDG_STATE_HOME (never the operator's database) and NICK_INIT_BIN
//! pointing at the fake init of tests/fixtures/bin.

mod common;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

use common::{DOWN, ENTER, ESC, LEFT, World};

const ALT_ON: &str = "\x1b[?1049h";
const ALT_OFF: &str = "\x1b[?1049l";
const CURSOR_ON: &str = "\x1b[?25h";

/// Opens the Projects screen of auto_scrum.
fn open_projects(nick: &mut common::Nick) {
    nick.wait_for("Módulos");
    nick.send(ENTER);
    nick.wait_for("auto_scrum > Projetos");
}

/// Types `path` in the add field and confirms.
fn add(nick: &mut common::Nick, path: &Path) {
    nick.send("a");
    nick.wait_for("Caminho da pasta do projeto:");
    nick.send(path.to_str().unwrap());
    nick.wait_for(path.file_name().unwrap().to_str().unwrap());
    nick.send(ENTER);
}

/// The output after the last time the TUI left the alternate screen: only cursor shows.
fn assert_restored_at_the_end(raw: &str) {
    let off = raw.rfind(ALT_OFF).expect("the alternate screen was left");
    assert!(
        raw.rfind(ALT_ON).is_none_or(|on| on < off),
        "nothing re-entered it: {raw:?}"
    );
    let tail = &raw[off + ALT_OFF.len()..];
    assert!(tail.starts_with(CURSOR_ON), "the cursor is shown: {tail:?}");
}

// ---- any cwd, and where the database lives ----

#[test]
fn it_shows_the_same_list_from_any_cwd() {
    let world = World::new();
    let alfa = world.dir("alfa", true);
    let beta = world.dir("beta", true);
    world.register(&alfa);
    world.register(&beta);
    let elsewhere = world.dir("elsewhere", false);
    let mut screens = Vec::new();
    for cwd in [Path::new("/"), elsewhere.as_path(), alfa.as_path()] {
        let mut nick = world.spawn(cwd, &[]);
        open_projects(&mut nick);
        nick.wait_for("beta");
        screens.push(nick.screen());
        nick.send("q");
        assert_eq!(nick.wait_exit(), Some(0), "cwd {}", cwd.display());
    }
    assert!(screens[0].contains(&format!("alfa  {}", alfa.display())));
    assert!(screens[0].contains(&format!("beta  {}", beta.display())));
    assert_eq!(screens[0], screens[1]);
    assert_eq!(screens[0], screens[2]);
}

#[test]
fn the_database_is_born_in_xdg_state_home() {
    let world = World::new();
    let mut nick = world.spawn(&world.root, &[]);
    nick.wait_for("Módulos");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert!(world.db_path().is_file());
    assert!(!world.home().join(".local").exists());
}

#[test]
fn without_xdg_state_home_the_database_is_born_in_home() {
    let world = World::new();
    let env: Vec<(String, String)> = world
        .env()
        .into_iter()
        .filter(|(k, _)| k != "XDG_STATE_HOME")
        .collect();
    let mut nick = common::Nick::spawn(&world.root, &env);
    nick.wait_for("Módulos");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    let dir = world.home().join(".local/state/nick");
    assert!(dir.join("nick.db").is_file());
    assert_eq!(
        fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert!(!world.state().exists());
}

#[test]
fn without_home_it_does_not_open_and_exits_with_1() {
    let world = World::new();
    let env = [(
        "NICK_INIT_BIN".to_string(),
        common::fake_init().display().to_string(),
    )];
    let mut nick = common::Nick::spawn(&world.root, &env);
    assert_eq!(nick.wait_exit(), Some(1));
    let raw = nick.raw();
    assert!(
        raw.contains(
            "Erro: não sei onde guardar o banco do nick (HOME e XDG_STATE_HOME não definidos)."
        ),
        "{raw:?}"
    );
    assert!(!raw.contains(ALT_ON), "the TUI never opened");
}

// ---- leaving: q, error, panic ----

#[test]
fn q_exits_with_0_and_restores_the_terminal() {
    let world = World::new();
    let mut nick = world.spawn(&world.root, &[]);
    nick.wait_for("Módulos");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    let raw = nick.raw();
    assert!(raw.contains(ALT_ON));
    assert_restored_at_the_end(&raw);
}

#[test]
fn ctrl_c_exits_from_the_text_field() {
    let world = World::new();
    let mut nick = world.spawn(&world.root, &[]);
    open_projects(&mut nick);
    nick.send("a");
    nick.wait_for("Caminho da pasta do projeto:");
    nick.send("q");
    nick.send("\x03");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_restored_at_the_end(&nick.raw());
}

#[test]
fn an_unreadable_database_exits_with_1_and_a_message() {
    let world = World::new();
    fs::create_dir_all(world.db_path()).unwrap();
    let mut nick = world.spawn(&world.root, &[]);
    assert_eq!(nick.wait_exit(), Some(1));
    let raw = nick.raw();
    assert!(
        raw.contains(&format!(
            "Erro: não consegui abrir o banco do nick {}: ",
            world.db_path().display()
        )),
        "{raw:?}"
    );
    assert!(!raw.contains(ALT_ON), "the TUI never opened");
}

#[test]
fn an_error_inside_the_tui_restores_the_terminal_and_exits_with_1() {
    let world = World::new();
    let mut nick = world.spawn(&world.root, &[("NICK_TEST_PANIC", "error")]);
    assert_eq!(nick.wait_exit(), Some(1));
    let raw = nick.raw();
    let on = raw.find(ALT_ON).expect("the TUI opened");
    let off = raw.rfind(ALT_OFF).expect("the alternate screen was left");
    let message = raw
        .find("nick: NICK_TEST_PANIC: erro de teste")
        .unwrap_or_else(|| panic!("the error message: {raw:?}"));
    assert!(
        on < off && off < message,
        "restored before the message: {raw:?}"
    );
    assert!(raw[off..].contains(CURSOR_ON));
}

fn assert_clean_panic(trigger: &str) {
    let world = World::new();
    let mut nick = world.spawn(&world.root, &[("NICK_TEST_PANIC", trigger)]);
    let code = nick.wait_exit();
    let raw = nick.raw();
    assert_eq!(code, Some(101), "exits with 101 ({trigger}): {raw:?}");
    let off = raw.rfind(ALT_OFF).expect("the alternate screen was left");
    let message = raw
        .find("NICK_TEST_PANIC: panic de teste")
        .unwrap_or_else(|| panic!("the panic message: {raw:?}"));
    assert!(off < message, "restored before the panic message: {raw:?}");
    assert!(raw[off..message].contains(CURSOR_ON), "{raw:?}");
}

#[test]
fn a_panic_in_the_main_thread_restores_and_exits_with_101() {
    assert_clean_panic("main");
}

#[test]
fn a_panic_in_a_secondary_thread_restores_and_exits_with_101() {
    assert_clean_panic("thread");
}

// ---- adding through the TUI ----

#[test]
fn a_directory_with_config_is_registered() {
    let world = World::new();
    let proj = world.dir("meu", true);
    let mut nick = world.spawn(&world.root, &[]);
    open_projects(&mut nick);
    nick.wait_for("Nenhum projeto cadastrado. Pressione a para adicionar.");
    add(&mut nick, &proj);
    nick.wait_for(&format!("Projeto cadastrado: meu ({}).", proj.display()));
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_eq!(world.stored(), [proj]);
}

/// Opens the alert for a directory without config and presses "Criar configuração".
fn press_create(nick: &mut common::Nick, dir: &Path) {
    open_projects(nick);
    add(nick, dir);
    nick.wait_for("Esta pasta não tem a configuração do nick.");
    nick.send(LEFT);
    nick.wait_for("Cancelar");
    nick.send(ENTER);
}

#[test]
fn the_button_runs_the_init_in_the_project_and_registers_it() {
    let world = World::new();
    let bare = world.dir("bare", false);
    let mut nick = world.spawn(&world.root, &[("FAKE_INIT_MODE", "create")]);
    press_create(&mut nick, &bare);
    // The TUI is suspended: the init runs on the normal screen, with a cooked terminal (the
    // fake init reads a line; in raw mode the Enter would never end it).
    nick.wait_for("Criar a configuração? (s/N)");
    assert!(!nick.on_alternate_screen());
    assert!(nick.screen().contains(&format!(
        "Rodando {} em {}...",
        common::fake_init().display(),
        bare.display()
    )));
    nick.send("s\r");
    nick.wait_for("Pressione Enter para voltar ao nick.");
    assert!(nick.screen().contains("init falso: configuração criada"));
    assert_eq!(
        fs::read_to_string(world.marker()).unwrap().trim_end(),
        bare.display().to_string(),
        "the init ran with the cwd at the project"
    );
    assert!(!nick.on_alternate_screen(), "it waits for the Enter");
    nick.send(ENTER);
    nick.wait_for(&format!("Projeto cadastrado: bare ({}).", bare.display()));
    assert!(nick.on_alternate_screen(), "the TUI is back");
    assert_eq!(nick.raw().matches(ALT_ON).count(), 2);
    assert_eq!(world.stored(), [bare]);
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_restored_at_the_end(&nick.raw());
}

fn assert_init_failure(mode: &str, message: &str) {
    let world = World::new();
    let bare = world.dir("bare", false);
    let mut nick = world.spawn(&world.root, &[("FAKE_INIT_MODE", mode)]);
    press_create(&mut nick, &bare);
    nick.wait_for("Pressione Enter para voltar ao nick.");
    assert!(world.marker().is_file(), "{mode}: the init ran");
    nick.send(ENTER);
    nick.wait_for(message);
    assert!(nick.on_alternate_screen());
    assert_eq!(world.stored(), Vec::<std::path::PathBuf>::new(), "{mode}");
    assert!(!bare.join(".nick/nick.conf").exists());
    nick.send(ESC);
    nick.wait_for("Nenhum projeto cadastrado");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0), "{mode}");
}

#[test]
fn an_init_that_exits_with_0_without_config_registers_nothing() {
    assert_init_failure("zero", "sem criar a configuração");
}

#[test]
fn an_init_that_fails_registers_nothing() {
    assert_init_failure(
        "fail",
        "O nick-init falhou (código 1). Nada foi cadastrado.",
    );
}

#[test]
fn an_init_killed_by_a_signal_registers_nothing() {
    assert_init_failure(
        "signal",
        "O nick-init foi interrompido. Nada foi cadastrado.",
    );
}

/// Ctrl+C while the init runs (the natural way to cancel it) reaches the whole foreground
/// group, the nick included, since the TUI is suspended out of raw mode. The nick ignores it
/// while the init runs and while it waits for the Enter (review NS-31, P1): only the init dies,
/// and the alert says so.
#[test]
fn ctrl_c_during_the_init_only_interrupts_the_init() {
    let world = World::new();
    let bare = world.dir("bare", false);
    let mut nick = world.spawn(&world.root, &[("FAKE_INIT_MODE", "create")]);
    press_create(&mut nick, &bare);
    nick.wait_for("Criar a configuração? (s/N)");
    nick.send("\x03");
    nick.wait_for("Pressione Enter para voltar ao nick.");
    // Ctrl+C on the Enter prompt is ignored too: the nick keeps waiting.
    nick.send("\x03");
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert!(!nick.on_alternate_screen(), "still waiting for the Enter");
    nick.send(ENTER);
    nick.wait_for("O nick-init foi interrompido. Nada foi cadastrado.");
    assert!(nick.on_alternate_screen(), "the TUI is back");
    assert_eq!(world.stored(), Vec::<std::path::PathBuf>::new());
    assert!(!bare.join(".nick/nick.conf").exists());
    // Back in raw mode, Ctrl+C is a key again and quits (plan D9).
    nick.send("\x03");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_restored_at_the_end(&nick.raw());
}

#[test]
fn the_init_only_runs_from_the_button() {
    let world = World::new();
    let bare = world.dir("bare", false);
    fs::write(bare.join("README.md"), "# bare\n").unwrap();
    fs::create_dir_all(bare.join("src")).unwrap();
    fs::write(bare.join("src/main.rs"), "fn main() {}\n").unwrap();
    let before = common::snapshot(&bare);
    let mut nick = world.spawn(&world.root, &[("FAKE_INIT_MODE", "create")]);
    open_projects(&mut nick);
    add(&mut nick, &bare);
    nick.wait_for("Esta pasta não tem a configuração do nick.");
    nick.send(ESC);
    nick.wait_for("Nenhum projeto cadastrado");
    // Again, now leaving through the focused "Cancelar".
    add(&mut nick, &bare);
    nick.wait_for("Esta pasta não tem a configuração do nick.");
    nick.send(ENTER);
    nick.wait_for("Nenhum projeto cadastrado");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert!(!world.marker().exists(), "the init never ran");
    assert_eq!(
        common::snapshot(&bare),
        before,
        "no file created in the project"
    );
    assert_eq!(world.stored(), Vec::<std::path::PathBuf>::new());
}

#[test]
fn a_missing_init_shows_the_path_and_the_tui_goes_on() {
    let world = World::new();
    let bare = world.dir("bare", false);
    let missing = world.root.join("nao/existe/nick-init");
    let missing_text = missing.display().to_string();
    let mut nick = world.spawn(&world.root, &[("NICK_INIT_BIN", missing_text.as_str())]);
    press_create(&mut nick, &bare);
    nick.wait_for("Não encontrei o bin/nick-init em");
    let flat: String = nick
        .screen()
        .lines()
        .map(|l| l.replace('│', "").trim().to_string())
        .collect();
    assert!(
        flat.contains(&format!(
            "Não encontrei o bin/nick-init em {missing_text}. Nada foi feito."
        )),
        "{}",
        nick.screen()
    );
    nick.send(ESC);
    nick.wait_for("Nenhum projeto cadastrado");
    nick.send(LEFT);
    nick.wait_for("Módulos");
    nick.send(DOWN);
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert!(!world.marker().exists());
}

// ---- removing and availability ----

#[test]
fn removing_keeps_every_file_of_the_project() {
    let world = World::new();
    let proj = world.dir("proj", true);
    fs::write(proj.join("dado.txt"), "importante\n").unwrap();
    world.register(&proj);
    let before = common::snapshot(&proj);
    let mut nick = world.spawn(&world.root, &[]);
    open_projects(&mut nick);
    nick.wait_for("proj");
    nick.send("r");
    nick.wait_for("(s/N)");
    nick.send("n");
    nick.wait_for("r remover");
    assert_eq!(world.stored(), std::slice::from_ref(&proj));
    nick.send("r");
    nick.wait_for("(s/N)");
    nick.send("s");
    nick.wait_for("Projeto removido do nick: proj. Nenhum arquivo foi apagado.");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
    assert_eq!(world.stored(), Vec::<std::path::PathBuf>::new());
    assert_eq!(common::snapshot(&proj), before);
}

#[test]
fn a_project_whose_directory_is_gone_is_marked_unavailable() {
    let world = World::new();
    let gone = world.dir("gone", true);
    let noconf = world.dir("noconf", true);
    world.register(&gone);
    world.register(&noconf);
    fs::remove_dir_all(&gone).unwrap();
    fs::remove_file(noconf.join(".nick/nick.conf")).unwrap();
    let mut nick = world.spawn(&world.root, &[]);
    open_projects(&mut nick);
    nick.wait_for("[indisponível: pasta não encontrada]");
    nick.wait_for("[indisponível: sem .nick/nick.conf]");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));
}
