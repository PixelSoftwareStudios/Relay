//! OS-level I/O helpers shared by every Relay crate.
//!
//! All filesystem, process, clock, entropy and per-user path code that is not a
//! network socket or an audio device lives here, so that the pure crates
//! (`relay-core`, `relay-protocol`, `relay-messages`) stay free of OS calls and
//! the security-relevant code (permissions, atomic replacement, locks, detached
//! launch) exists exactly once.
//!
//! This crate never opens sockets and does not depend on the networking or
//! engine crates. It supplies real implementations of the injected ports of the
//! pure crates ([`sink::DirSink`] for `FileSink`, [`clock::SystemClock`] and
//! [`entropy::OsEntropy`] for `Clock` and `IdSource`).
//!
//! Windows: owner-only protection relies on the per-user profile ACLs. Creating
//! files with an explicit owner-only DACL is an unmet hardening gate, see
//! `docs/portability.md`.

#![forbid(unsafe_code)]

pub mod clock;
pub mod entropy;
mod error;
pub mod fs;
pub mod lock;
pub mod paths;
pub mod proc;
pub mod sink;

pub use error::{IoError, Result};
