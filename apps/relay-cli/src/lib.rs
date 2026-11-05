//! The `relay` command line: scriptable commands and a terminal UI. The
//! engine runs as the separate `relayd`; everything talks to it through [`client::Control`],
//! so commands and screens are testable against a fake.

pub mod cli;
pub mod client;
pub mod commands;
mod host;
pub mod hotkey;
pub mod secret;
pub mod tui;

pub use host::main_exit;
