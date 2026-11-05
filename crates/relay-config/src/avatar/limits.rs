//! Avatar limits, matching the project's resource budget.

/// Largest accepted encoded upload, and the largest canonical avatar that is
/// stored, served or accepted from a peer (1 MiB).
pub const ENCODED_MAX_BYTES: usize = 1_048_576;
/// Largest accepted input width or height.
pub const INPUT_DIMENSION_MAX: u32 = 2048;
/// Width and height of the canonical stored avatar.
pub const OUTPUT_DIMENSION: u32 = 256;
/// Per-user size bound of the disposable avatar cache (128 MiB).
pub const CACHE_MAX_BYTES: u64 = 134_217_728;
/// Decoder allocation ceiling: a 2048x2048 RGBA image, so a header lying about
/// its size cannot trigger a larger allocation.
pub const DECODE_ALLOC_MAX: u64 = (INPUT_DIMENSION_MAX as u64) * (INPUT_DIMENSION_MAX as u64) * 4;
