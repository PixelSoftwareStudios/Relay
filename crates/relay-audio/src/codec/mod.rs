//! Opus wrappers with independent per-call state.

mod decoder;
mod encoder;

pub use decoder::{CallDecoder, DecodeKind};
pub use encoder::{CallEncoder, EncoderConfig};

/// Upper bound on an encoded packet in the v1 profile.
pub const MAX_PACKET_BYTES: usize = 1000;
