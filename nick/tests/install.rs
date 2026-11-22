//! End-to-end test of the local installer (NS-52): `make install` from a copy of the repository
//! into a temporary prefix, the copy renamed (the checkout the binary was built from is gone), and
//! the nick installed opening an agent in the PTY (tests/common), with the fake claude and ai-jail
//! of bin/tests/fixtures/bin, a temporary HOME and XDG_STATE_HOME, from a cwd of its own. Then the
//! positive control (a file missing from the prefix is an error that points at the prefix, never a
//! fallback to a checkout) and `make uninstall`. It runs a real release build, in a target of its
//! own under CARGO_TARGET_TMPDIR (cached between runs, out of the way of the build of
//! `cargo test`).

mod common;

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use common::menu::{choose_type, open_menu};
use common::{DOWN, ENTER, Nick, World};

/// What `make install` needs of the repository, from its root.
const COPIED: [&str; 7] = [
    "Makefile",
    "install",
    "bin",
    "lib",
    "auto_scrum/prompt.sh",
    "auto_scrum/templates",
    "nick",
];

/// Copies `from` into `to` (a file or a directory, recursively), without any `target` dir.
fn copy(from: &Path, to: &Path) {
    let meta = fs::symlink_metadata(from).unwrap();
    if meta.is_dir() {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            if entry.file_name() == "target" {
                continue;
            }
            copy(&entry.path(), &to.join(entry.file_name()));
        }
    } else if meta.is_symlink() {
        std::os::unix::fs::symlink(fs::read_link(from).unwrap(), to).unwrap();
    } else {
        fs::create_dir_all(to.parent().unwrap()).unwrap();
        fs::copy(from, to).unwrap();
    }
}

/// `make <args>` at `dir`, with the build in its own target and no MAKEFLAGS of an outer make.
fn make(dir: &Path, args: &[&str]) -> Output {
    let target = Path::new(env!("CARGO_TARGET_TMPDIR")).join("install-target");
    Command::new("make")
        .args(args)
        .current_dir(dir)
        .env_remove("MAKEFLAGS")
        .env_remove("MFLAGS")
        .env_remove("MAKELEVEL")
        .env("CARGO_TARGET_DIR", target)
        .output()
        .unwrap()
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// From the menu, the PO discussion with its subject, and `s` on the preview.
fn open_po_discussion(nick: &mut Nick) {
    choose_type(nick, "PO - Conversar para definir ticket");
    nick.send("Assunto de teste");
    nick.send(ENTER);
    nick.wait_for("Confirma o envio para o claude? (s/N)");
    nick.send("s");
}

/// The files under `dir` (relative to it), from the snapshot of tests/common.
fn files(dir: &Path) -> Vec<PathBuf> {
    common::snapshot(dir)
        .into_iter()
        .filter(|(_, entry)| entry.kind == "file")
        .map(|(path, _)| path)
        .collect()
}

#[test]
fn the_nick_installed_opens_agents_without_the_checkout() {
    let world = World::new();
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let src = world.root.join("src");
    for rel in COPIED {
        copy(&repo.join(rel), &src.join(rel));
    }
    // A repository without a commit: the build says `desconhecido`.
    let init = Command::new("git")
        .args(["init", "-q"])
        .current_dir(&src)
        .output()
        .unwrap();
    assert!(init.status.success(), "{}", text(&init.stderr));

    // 1. make install into a temporary prefix.
    let prefix = world.root.join("prefix");
    let prefix_arg = format!("PREFIX={}", prefix.display());
    let out = make(&src, &["install", &prefix_arg]);
    assert!(
        out.status.success(),
        "make install: {}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    let installed = prefix.join("bin/nick");
    assert!(installed.is_file());
    assert!(prefix.join("share/nick/.nick-install").is_file());
    assert!(prefix.join("share/nick/bin/nick-agent").is_file());

    // 2. The checkout the binary came from is gone.
    let moved = world.root.join("src-movido");
    fs::rename(&src, &moved).unwrap();

    // 3. The installed binary answers, from anywhere.
    let version = Command::new(&installed)
        .arg("--version")
        .current_dir(&world.root)
        .env_clear()
        .output()
        .unwrap();
    assert_eq!(version.status.code(), Some(0));
    assert_eq!(
        text(&version.stdout),
        format!("nick {} (commit desconhecido)\n", env!("CARGO_PKG_VERSION"))
    );

    // 4. It opens an agent of a project: the prompt built by the files of the prefix, the log in
    // the state dir, the fake claude run.
    let proj = world.project("proj", &["JAIL_ENABLED=\"false\""]);
    world.register(&proj);
    world.ticket(&proj, "NS-1", "Primeiro");
    let id = world.project_id_at(&proj);
    let claude_log = world.root.join("claude.argv");
    let path = common::fake_path();
    let claude = claude_log.display().to_string();
    let elsewhere = world.dir("outro-lugar", false);
    let env = [
        ("PATH", path.as_str()),
        ("FAKE_CLAUDE_MODE", "exit"),
        ("FAKE_CLAUDE_LOG", claude.as_str()),
    ];
    let mut nick = world.spawn_bin(&installed, &elsewhere, &env);
    // The version of the installed binary on the screen "Configurações".
    nick.wait_for(" Navegação ");
    nick.send("c");
    nick.wait_for_flat(&format!(
        "Versão: {} (commit desconhecido)",
        env!("CARGO_PKG_VERSION")
    ));
    nick.keep_screen("NS-52: Configurações do nick instalado, com a versão");
    nick.send(common::ESC);
    nick.wait_for(" Navegação ");
    nick.open_project("proj");
    nick.send(DOWN);
    nick.wait_for_flat("g abrir agente");
    assert!(open_menu(&mut nick), "{}", nick.screen());
    open_po_discussion(&mut nick);
    nick.wait_for("O agente proj · PO · NS-1 terminou.");
    assert!(claude_log.is_file(), "the fake claude ran");
    let logs = world.state().join(format!("nick/logs/proj-{id}"));
    let prompts = files(&logs);
    assert_eq!(prompts.len(), 1, "{prompts:?}");
    let prompt = fs::read_to_string(logs.join(&prompts[0])).unwrap();
    assert!(prompt.contains("Assunto de teste"), "{prompt}");
    assert!(
        fs::read_to_string(&claude_log)
            .unwrap()
            .contains("Assunto de teste"),
        "the prompt reached claude"
    );
    assert!(
        !prefix.join("share/nick/auto_scrum/logs").exists(),
        "no log in the prefix"
    );
    nick.keep_screen("NS-52: nick instalado, agente aberto sem o checkout");

    // 5. Positive control: a file missing from the prefix is an error with the path in the
    // prefix (no fallback to any checkout).
    let missing = prefix.join("share/nick/auto_scrum/prompt.sh");
    fs::remove_file(&missing).unwrap();
    fs::remove_file(&claude_log).unwrap();
    // The selection stayed on the node of the agent: up to its ticket.
    nick.send(common::UP);
    nick.wait_for_flat("g abrir agente");
    nick.send("g");
    nick.wait_for("Agente não aberto");
    let flat = nick.flat_screen();
    // bash names it from bin/nick-agent: <prefix>/share/nick/bin/../auto_scrum/prompt.sh.
    let named = format!(
        "{}/share/nick/bin/../auto_scrum/prompt.sh",
        prefix.display()
    );
    assert!(flat.contains(&named), "{named} in {flat}");
    assert!(!missing.exists());
    assert!(flat.contains("Nada foi aberto."), "{flat}");
    assert!(!flat.contains("src-movido"), "{flat}");
    assert!(!claude_log.exists(), "claude never ran");
    nick.keep_screen("NS-52: arquivo ausente no prefixo");
    nick.send(common::ESC);
    nick.wait_for_flat("g abrir agente");
    nick.send("q");
    assert_eq!(nick.wait_exit(), Some(0));

    // 6. make uninstall from the moved copy: the prefix loses the nick, the state stays.
    let out = make(&moved, &["uninstall", &prefix_arg]);
    assert!(
        out.status.success(),
        "make uninstall: {}{}",
        text(&out.stdout),
        text(&out.stderr)
    );
    assert!(!installed.exists());
    assert!(!prefix.join("share/nick").exists());
    assert!(world.db_path().is_file(), "the database stays");
    assert_eq!(files(&logs), prompts, "the logs stay");
}
