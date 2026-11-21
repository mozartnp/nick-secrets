//! The nick shell: the `nick` program, a TUI that lists the nick modules, the projects of
//! each module and the tickets of each project, kept in an SQLite database in the user's state
//! directory, and opens the agent of a ticket in an embedded terminal through bin/nick-agent
//! (ABOUT.md, "Rust no projeto: a casca `nick` (NS-31)", "Rust no projeto: tickets na casca
//! `nick` (NS-32)" and "Rust no projeto: agentes na casca `nick` (NS-33)"). Since the NS-38 the
//! navigation is a tree (tree.rs, "Rust no projeto: navegação em árvore (NS-38)"). The modules are
//! public so the integration tests in tests/ can use them; the binary (main.rs) only wires them
//! together.
pub mod agent_session;
pub mod agents;
pub mod app;
pub mod db;
pub mod docs;
pub mod editor;
pub mod epics;
pub mod init;
pub mod keys;
pub mod launch;
pub mod mascot;
pub mod modules;
pub mod paths;
pub mod projects;
pub mod stages;
pub mod terminal;
pub mod tickets;
pub mod tree;
pub mod ui;
