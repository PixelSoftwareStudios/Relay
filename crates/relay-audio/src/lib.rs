//! Audio devices, conversion, codec, mixing and noise-suppression adapters.
//!
//! The internal domain is mono `f32` at 48 kHz in 10 ms (480 sample) frames.

pub mod capture;
pub mod convert;
pub mod error;
pub mod format;
pub mod playback;
pub mod ring;

pub use error::{AudioError, Result};
pub use format::{BlockTiming, Discontinuity, FRAME_SAMPLES, INTERNAL_RATE, MAX_CALLS};
