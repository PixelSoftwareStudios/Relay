//! What the Relay clients (CLI/TUI and desktop) share over `relay-api`.
//!
//! - [`endpoint`]: which engine to talk to, from the environment.
//! - [`command`] and [`wire`]: one request vocabulary and the single place it
//!   becomes gRPC calls (mutation contexts, controller leases, operations).
//! - [`connection`]: a lightweight `relay://` string check.
//! - [`daemon`]: finding, starting and stopping the separate `relayd` process.
//! - [`refresh`]: replacing a stale or idle daemon with this program's own build.
//! - [`operation`]: waiting for an accepted operation to settle.
//! - [`meter`]: level scaling for meters.
//!
//! UI toolkits, argument parsing and each client's own error taxonomy stay in
//! the apps. Nothing here carries audio.

pub mod command;
pub mod connection;
pub mod daemon;
pub mod endpoint;
pub mod meter;
pub mod operation;
pub mod refresh;
pub mod wire;

pub use command::{CallAction, CallTarget, ChannelOptions, Command, Secret};
