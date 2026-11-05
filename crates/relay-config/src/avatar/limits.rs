//! Avatar limits, matching the project's resource budget.

/// Largest accepted encoded upload.
pub const ENCODED_MAX_BYTES: usize = 262_144;
/// Largest accepted input width or height.
pub const INPUT_DIMENSION_MAX: u32 = 2048;
/// Width and height of the canonical stored avatar.
pub const OUTPUT_DIMENSION: u32 = 128;
/// Largest size of the content-addressed cache directory.
pub const CACHE_MAX_BYTES: u64 = 33_554_432;
/// Decoder allocation ceiling: a 2048x2048 RGBA image, so a header lying about
/// its size cannot trigger a larger allocation.
pub const DECODE_ALLOC_MAX: u64 = (INPUT_DIMENSION_MAX as u64) * (INPUT_DIMENSION_MAX as u64) * 4;
