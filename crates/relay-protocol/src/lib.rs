//! Pure, sans-IO media-wire logic for Relay.
//!
//! Everything here is deterministic and free of clocks, sockets and threads:
//! callers pass arrival times and drive playout ticks explicitly.
//!
//! * [`rtp`] - the narrow fixed RTP profile (parse / serialize).
//! * [`seq`] - sequence / timestamp extension and replay windows.
//! * [`jitter`] - the jitter buffer and playout planner.
//! * [`drift`] - clock-drift ratio estimation.
//! * [`assembly`] - frame re-chunking between DSP and codec frame sizes.

pub mod assembly;
pub mod drift;
pub mod error;
pub mod jitter;
pub mod rtp;
pub mod seq;

pub use error::{AssemblyError, RtpError};

/// Opus RTP clock rate in Hz, fixed regardless of encoder bandwidth.
pub const RTP_CLOCK_HZ: u32 = 48_000;
