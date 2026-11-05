//! Bounds of the per-transfer data link.

use std::time::Duration;

use crate::record::sealed_len;

/// Largest file chunk in one frame. Smaller than the 64 KiB state-machine chunk so a frame
/// stays well inside one Noise message and never stalls the pacer for long; larger chunks are
/// split transparently on send.
pub const DATA_CHUNK_MAX_BYTES: usize = 32 * 1024;
/// Frame tag (1) plus offset (8) ahead of chunk data.
pub const CHUNK_HEADER_BYTES: usize = 9;
/// Longest sealed data record, excluding its 4-byte length prefix.
pub const DATA_RECORD_MAX_BYTES: usize = sealed_len(CHUNK_HEADER_BYTES + DATA_CHUNK_MAX_BYTES);

/// Default validity of a transfer token.
pub const TOKEN_TTL_DEFAULT: Duration = Duration::from_secs(30);
/// Longest validity a caller may request.
pub const TOKEN_TTL_MAX: Duration = Duration::from_secs(120);
/// Tokens issued and not yet claimed, per hub.
pub const TOKENS_OUTSTANDING_MAX: usize = 16;
/// Wrong-token attempts by the right peer before the token is revoked.
pub const TOKEN_ATTEMPTS_MAX: u8 = 3;

/// Established data links at once, per hub (both directions).
pub const DATA_LINKS_MAX: usize = 8;
/// Established data links from one source address.
pub const DATA_LINKS_PER_SOURCE: usize = 4;
/// No frame (or no write progress) for this long ends the link.
pub const DATA_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
/// Time to establish a TCP connection when dialling.
pub const DATA_CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// DSCP CS1 ("lower effort") shifted into the IP TOS byte.
pub const DSCP_CS1_TOS: u32 = 8 << 2;
