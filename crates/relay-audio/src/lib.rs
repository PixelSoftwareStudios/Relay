//! Audio devices, conversion, codec, mixing and noise-suppression adapters.
//!
//! The internal domain is mono `f32` at 48 kHz in 10 ms (480 sample) frames.

pub mod capture;
pub mod codec;
pub mod convert;
pub mod device;
pub mod dsp;
pub mod error;
pub mod format;
pub mod mixer;
pub mod pipeline;
pub mod playback;
pub mod priority;
pub mod ring;
pub mod ringtone;
pub mod tones;

pub use error::{AudioError, Result};
pub use format::{BlockTiming, Discontinuity, FRAME_SAMPLES, INTERNAL_RATE, MAX_CALLS};
