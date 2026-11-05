//! Sample-format, channel and rate adapters.

mod channels;
mod resample;
mod sample;

pub use channels::{ChannelSelect, fan_out};
pub use resample::{MAX_DRIFT_PPM, StreamResampler};
pub use sample::Sample;
