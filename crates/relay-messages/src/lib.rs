//! Sans-IO session messaging: call-scoped messages, safe markdown, bounded RAM history,
//! inline images and file offers. See `docs/session-messages.md`.
//!
//! The crate performs no I/O, reads no clocks and generates no randomness: the engine injects
//! [`Clock`], [`IdSource`] and (for downloads) [`transfer::FileSink`].

pub mod delivery;
pub mod env;
pub mod error;
pub mod history;
pub mod image;
pub mod limits;
pub mod markdown;
pub mod message;
pub mod rate;
pub mod sanitize;
pub mod store;
pub mod traffic;
pub mod transfer;

pub use env::{Clock, IdSource};
pub use error::{RejectReason, StoreError, TransferError};
pub use history::HistoryManager;
pub use message::*;
pub use sanitize::SafeFileName;
pub use store::{SessionInfo, SessionStore};
pub use traffic::TrafficClass;
