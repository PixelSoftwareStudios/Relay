//! Deterministic jitter buffer and playout planner.
//!
//! The buffer is a deadline scheduler over the RTP timeline. The caller
//! inserts authenticated packets with their arrival time and calls
//! [`JitterBuffer::poll`] once per render frame; each poll returns a
//! [`Playout`] decision expressed as data (play / conceal / skip).

mod buffer;
mod config;
mod estimator;
mod margin;
mod playout;
mod stats;

pub use buffer::{InsertOutcome, JitterBuffer, PacketMeta};
pub use config::JitterConfig;
pub use estimator::InterarrivalJitter;
pub use margin::MarginController;
pub use playout::{ConcealCause, Concealment, Played, Playout, SkipReason};
pub use stats::JitterStats;
