//! The `relay` command line: scriptable commands, a terminal UI and the
//! engine host. Everything talks to the engine through [`client::Control`],
//! so commands and screens are testable against a fake.

pub mod cli;
pub mod client;
pub mod commands;
mod host;
pub mod hotkey;
pub mod secret;
pub mod tui;

pub use host::main_exit;
