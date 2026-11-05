//! File offers and the sans-IO transfer state machines.
//!
//! Receiver contract (carried out through [`FileSink`]): write to a temporary name in the
//! destination directory, fsync, verify the BLAKE3 hash, then atomically rename without
//! overwriting. The file is visible only after verification.

mod manager;
mod offer;
mod receiver;
mod sender;
mod sink;
mod types;

pub use manager::Transfers;
pub use offer::{offer_from_meta, offer_to_meta, required_free_space, validate_preview};
pub use receiver::Receiver;
pub use sender::Sender;
pub use sink::{FileSink, SinkError};
pub use types::{CancelBy, Effect, FailureReason, TransferMsg, TransferState, Verdict};
