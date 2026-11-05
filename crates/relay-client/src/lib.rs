//! What the Relay clients (CLI/TUI and desktop) share over `relay-api`.
//!
//! - [`endpoint`]: which engine to talk to, from the environment.
//! - [`command`] and [`wire`]: one request vocabulary and the single place it
//!   becomes gRPC calls (mutation contexts, controller leases, operations).
//! - [`connection`]: a lightweight `relay://` string check.
//! - [`operation`]: waiting for an accepted operation to settle.
//! - [`meter`]: level scaling for meters.
//!
//! UI toolkits, argument parsing and each client's own error taxonomy stay in
//! the apps. Nothing here carries audio.

pub mod command;
pub mod connection;
pub mod endpoint;
pub mod meter;
pub mod operation;
pub mod wire;

pub use command::{CallAction, CallTarget, ChannelOptions, Command, Secret};
