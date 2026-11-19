//! The nick shell: the `nick` program, a TUI that lists the nick modules, the projects of
//! each module and the tickets of each project, kept in an SQLite database in the user's state
//! directory (ABOUT.md, "Rust no projeto: a casca `nick` (NS-31)" and "Rust no projeto: tickets
//! na casca `nick` (NS-32)"). The modules are public so the integration tests in
//! tests/ can use them; the binary (main.rs) only wires them together.
pub mod app;
pub mod db;
pub mod init;
pub mod modules;
pub mod paths;
pub mod projects;
pub mod terminal;
pub mod tickets;
pub mod ui;
