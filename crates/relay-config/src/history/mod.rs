//! Call history: a bounded, local-only list of past calls.
//!
//! One row per call that reached an end the user saw: who (the name shown at the time and
//! the peer's key fingerprint), which way, how it ended, when it started and how long it
//! was connected. Nothing else is kept: no audio, no message content, no addresses. The
//! list never leaves this machine; it is not part of a diagnostics bundle or sent to peers.

mod limits;
mod store;

pub use limits::{
	DEFAULT_LIST, Limits, MAX_AGE_MS, MAX_ENTRIES, MAX_LIST, MAX_NAME_CHARS, MAX_SHORT_CHARS,
};
pub use store::{CallHistory, CallOutcome, CallRecord, Direction, NewCallRecord};
