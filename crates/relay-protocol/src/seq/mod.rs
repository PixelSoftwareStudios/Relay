//! Wrap-aware counter extension and replay detection.

mod extend;
mod replay;

pub use extend::{SeqExtender, TimestampExtender};
pub use replay::{ReplayVerdict, ReplayWindow};
