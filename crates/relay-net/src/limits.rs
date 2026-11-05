//! Numeric limits shared across the crate. Values follow the project limits registry.

use std::time::Duration;

/// Maximum size of one UDP media datagram, header and tag included.
pub const UDP_RECORD_MAX_BYTES: usize = 1200;
/// Maximum size of one reliable control record, excluding its 4-byte TCP length prefix.
pub const CONTROL_RECORD_MAX_BYTES: usize = 16 * 1024;
/// Maximum size of one Noise handshake message.
pub const HANDSHAKE_MESSAGE_MAX_BYTES: usize = 4096;
/// Total time allowed for the TCP preface plus the three handshake messages.
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
/// Maximum number of simultaneously pending unauthenticated handshakes.
pub const PENDING_HANDSHAKES_MAX: usize = 8;
/// Pending unauthenticated handshakes allowed from one source (an IPv4 address or an IPv6 /64).
pub const PENDING_HANDSHAKES_PER_SOURCE: usize = 2;
/// Time a new connection has to send its 24-byte preface before the slot is released.
pub const PREFACE_TIMEOUT: Duration = Duration::from_secs(1);
/// Authenticated but untrusted links kept at once, and per source address.
pub const UNTRUSTED_LINKS_MAX: usize = 16;
pub const UNTRUSTED_LINKS_PER_SOURCE: usize = 2;
/// Maximum number of discovered entries retained.
pub const DISCOVERY_ENTRIES_MAX: usize = 256;
/// Size of the media replay window, in records.
pub const MEDIA_REPLAY_WINDOW: u64 = 1024;
/// Rekey before this many seconds of key lifetime.
pub const REKEY_MAX_AGE: Duration = Duration::from_secs(86_400);
/// Rekey before this many records per lane.
pub const REKEY_MAX_RECORDS: u64 = 1 << 32;
/// Maximum number of epochs admitted at once during a switchover.
pub const EPOCH_OVERLAP_MAX_COUNT: usize = 2;
/// Maximum duration of a switchover overlap.
pub const EPOCH_OVERLAP_MAX: Duration = Duration::from_secs(2);
/// Time allowed for initial UDP path validation.
pub const UDP_PATH_TIMEOUT: Duration = Duration::from_secs(3);
/// Maximum number of calls an `EpochSwitch` may name.
pub const ACTIVE_CALLS_MAX: usize = 8;
/// Maximum display name length in Unicode scalar values.
pub const DISPLAY_NAME_MAX_CHARS: usize = 64;
/// Maximum display name length in UTF-8 bytes.
pub const DISPLAY_NAME_MAX_BYTES: usize = 256;

/// Public record header length.
pub const RECORD_HEADER_BYTES: usize = 32;
/// AEAD tag length.
pub const AEAD_TAG_BYTES: usize = 16;
/// Link epoch length.
pub const EPOCH_BYTES: usize = 16;
/// Call identifier length.
pub const CALL_ID_BYTES: usize = 16;
/// Length of the TCP preface.
pub const PREFACE_BYTES: usize = 24;
