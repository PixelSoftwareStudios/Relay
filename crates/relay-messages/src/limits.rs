//! Every numeric bound of the crate in one place.

/// Maximum UTF-8 bytes of one text message.
pub const MAX_TEXT_BYTES: usize = 4096;
/// Maximum bytes of a sanitized display name.
pub const MAX_DISPLAY_NAME_BYTES: usize = 64;
/// Maximum bytes of a sanitized file name.
pub const MAX_FILE_NAME_BYTES: usize = 120;
/// Maximum extension bytes kept when a file name is truncated.
pub const MAX_EXTENSION_BYTES: usize = 16;

/// Per-session history budget (payload bytes plus per-message overhead).
pub const SESSION_BUDGET_BYTES: u64 = 64 * 1024 * 1024;
/// Conservative allowance added to every message for metadata and derived data.
pub const MESSAGE_OVERHEAD_BYTES: u64 = 1024;
/// Budget for the transcripts of all ended sessions.
pub const GLOBAL_RETAINED_BYTES: u64 = 256 * 1024 * 1024;

/// Sustained messages per second accepted from one sender.
pub const MESSAGE_RATE_PER_SEC: u32 = 10;
/// Burst of messages accepted from one sender.
pub const MESSAGE_BURST: u32 = 20;
/// Sustained inbound payload bytes per second per peer.
pub const INBOUND_BYTES_PER_SEC: u64 = 4 * 1024 * 1024;
/// Inbound payload burst per peer.
pub const INBOUND_BYTES_BURST: u64 = 16 * 1024 * 1024;
/// File offers per second (sustained) and burst.
pub const OFFER_RATE_PER_SEC: u32 = 2;
/// Burst of file offers.
pub const OFFER_BURST: u32 = 4;
/// File offers per session, both directions together.
pub const MAX_OFFERS_PER_SESSION: usize = 64;

/// Search results returned at most.
pub const MAX_SEARCH_RESULTS: usize = 100;
/// Longest search needle in bytes.
pub const MAX_SEARCH_QUERY_BYTES: usize = 256;

/// Markdown: blocks per document.
pub const MD_MAX_BLOCKS: usize = 256;
/// Markdown: total nodes per document.
pub const MD_MAX_NODES: usize = 4096;
/// Markdown: inline nesting depth.
pub const MD_MAX_DEPTH: usize = 3;
/// Markdown: longest URL that is linkified.
pub const MD_MAX_URL_BYTES: usize = 2048;
/// Markdown: longest code-fence language label.
pub const MD_MAX_LANG_BYTES: usize = 32;

/// Compressed image bytes.
pub const MAX_IMAGE_BYTES: usize = 8 * 1024 * 1024;
/// Decoded pixels (16 x 2^20, 64 MiB as RGBA).
pub const MAX_IMAGE_PIXELS: u64 = 16 * 1024 * 1024;
/// Longest image side.
pub const MAX_IMAGE_SIDE: u32 = 16_384;
/// Frames in an animated image.
pub const MAX_ANIMATION_FRAMES: u32 = 100;
/// Total decoded RGBA bytes of all frames of an animation.
pub const MAX_ANIMATION_DECODED_BYTES: u64 = 128 * 1024 * 1024;

/// Text preview bytes.
pub const MAX_PREVIEW_BYTES: usize = 16 * 1024;
/// Text preview lines.
pub const MAX_PREVIEW_LINES: usize = 100;
/// Largest offered file.
pub const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024 * 1024;
/// Concurrent active transfers per session.
pub const MAX_CONCURRENT_TRANSFERS: usize = 2;
/// Largest chunk.
pub const MAX_CHUNK_BYTES: usize = 64 * 1024;
/// Receiver-granted window.
pub const TRANSFER_WINDOW_BYTES: u64 = 1024 * 1024;
/// Free-space margin: a percentage of the size, at least `MIN_FREE_MARGIN_BYTES`.
pub const FREE_MARGIN_PERCENT: u64 = 10;
/// Minimum free-space margin.
pub const MIN_FREE_MARGIN_BYTES: u64 = 16 * 1024 * 1024;
/// Default pacing cap for bulk bytes per transfer.
pub const BULK_RATE_BYTES_PER_SEC: u64 = 16 * 1024 * 1024;

/// First ack timeout in milliseconds; doubles on each retry.
pub const ACK_TIMEOUT_MS: u64 = 5_000;
/// Resends before a message fails.
pub const MAX_RETRIES: u32 = 3;
