//! Entry point of `nick`: the command line, the state directory and the database (before the
//! TUI: an error there is printed on the normal terminal), the panic hook, and the loop of the
//! TUI. Only this file reads the environment (HOME, XDG_STATE_HOME, NICK_INIT_BIN and the
//! test hook NICK_TEST_PANIC).

use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::ExitCode;

use crossterm::event::{self, Event};
use nick::app::App;
use nick::db::Db;
use nick::init::{self, TerminalInitRunner};
use nick::paths::{self, DB_FILE_NAME};
use nick::terminal::{self, TerminalGuard};
use nick::ui;

fn main() -> ExitCode {
    let args: Vec<OsString> = std::env::args_os().skip(1).collect();
    if let Some(code) = handle_args(&args) {
        return code;
    }

    let home = std::env::var_os("HOME");
    let state_dir = match paths::state_dir(
        std::env::var_os("XDG_STATE_HOME").as_deref(),
        home.as_deref(),
    ) {
        Ok(dir) => dir,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };
    if let Err(err) = paths::create_state_dir(&state_dir) {
        eprintln!(
            "Erro: não consegui criar a pasta do banco do nick {}: {err}.",
            state_dir.display()
        );
        return ExitCode::FAILURE;
    }
    let db = match Db::open(&state_dir.join(DB_FILE_NAME)) {
        Ok(db) => db,
        Err(err) => {
            eprintln!("{err}");
            return ExitCode::FAILURE;
        }
    };
    let app = App::new(
        db,
        home.filter(|home| !home.is_empty()).map(PathBuf::from),
        std::env::current_dir().ok(),
        init::init_path(std::env::var_os("NICK_INIT_BIN").as_deref()),
    );

    terminal::install_panic_hook();
    match run(app) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("nick: {err}");
            ExitCode::FAILURE
        }
    }
}

/// `--version` alone prints the version; any other argument is refused with the usage, in
/// the pattern of bin/nick-init. Returns the exit code when the program must end here.
fn handle_args(args: &[OsString]) -> Option<ExitCode> {
    match args {
        [] => None,
        [only] if only == "--version" => {
            println!("nick {}", env!("CARGO_PKG_VERSION"));
            Some(ExitCode::SUCCESS)
        }
        _ => {
            let unknown = args
                .iter()
                .find(|arg| *arg != "--version")
                .unwrap_or_else(|| &args[1]);
            eprintln!(
                "Erro: argumento desconhecido: '{}'",
                unknown.to_string_lossy()
            );
            eprintln!("Uso: nick");
            Some(ExitCode::FAILURE)
        }
    }
}

/// The loop of the TUI: synchronous, reading the keys on the main thread (no reader thread:
/// it would compete for the stdin with bin/nick-init while the TUI is suspended).
fn run(mut app: App) -> io::Result<()> {
    let mut guard = TerminalGuard::enter()?;
    test_hook()?;
    loop {
        guard.terminal().draw(|frame| ui::draw(frame, &app))?;
        let Event::Key(key) = event::read()? else {
            // A resize (or anything else) only redraws.
            continue;
        };
        let mut runner = TerminalInitRunner::new(&mut guard);
        let quit = app.on_key(key, &mut runner);
        if let Some(err) = runner.error {
            return Err(err);
        }
        if quit {
            return Ok(());
        }
    }
}

/// Test hook (ABOUT.md, tests/tui.rs), only for the tests: NICK_TEST_PANIC=main panics in the
/// main thread, =thread panics in a secondary thread while the main one is blocked reading
/// the keys, and =error makes `run` return an error, all after entering the TUI.
fn test_hook() -> io::Result<()> {
    match std::env::var("NICK_TEST_PANIC").as_deref() {
        Ok("main") => panic!("NICK_TEST_PANIC: panic de teste na thread principal"),
        Ok("thread") => {
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_millis(300));
                panic!("NICK_TEST_PANIC: panic de teste numa thread secundária");
            });
            Ok(())
        }
        Ok("error") => Err(io::Error::other("NICK_TEST_PANIC: erro de teste")),
        _ => Ok(()),
    }
}
