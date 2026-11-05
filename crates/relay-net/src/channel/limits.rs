//! Channel limits. Mirrors the table in `docs/channels.md`.

use std::time::Duration;

pub use relay_core::channel::{CHANNELS_MAX, NAME_MAX_CHARS};

/// Advertised blinded tag slots when at most [`TAG_SLOTS_SMALL`] channels are advertised.
pub const TAG_SLOTS_SMALL: usize = 4;
/// Tag slots when more channels are advertised. Also the cap on advertised channels.
pub const TAG_SLOTS_LARGE: usize = 8;
/// Listed (named, public) channels advertised at once.
pub const LISTED_ADVERTISED_MAX: usize = 3;
/// Length of a blinded tag in bytes.
pub const TAG_BYTES: usize = 8;
/// Tag rotation period.
pub const TAG_EPOCH_SECS: u64 = 3600;

/// Unexpired certificates an admitter tracks (and so members it admits) per channel.
pub const MEMBERS_ISSUED_MAX: usize = 64;
/// Delegated admins per channel.
pub const ADMINS_MAX: usize = 4;
/// Active invites per channel.
pub const INVITES_ACTIVE_MAX: usize = 16;
/// Default and maximum invite lifetimes.
pub const INVITE_TTL_DEFAULT: Duration = Duration::from_secs(24 * 3600);
pub const INVITE_TTL_MAX: Duration = Duration::from_secs(7 * 24 * 3600);
/// Default and maximum uses of one invite.
pub const INVITE_USES_DEFAULT: u8 = 1;
pub const INVITE_USES_MAX: u8 = 8;
/// Default and maximum membership certificate lifetimes; renewal grace after expiry.
pub const CERT_TTL_DEFAULT: Duration = Duration::from_secs(14 * 24 * 3600);
pub const CERT_TTL_MAX: Duration = Duration::from_secs(30 * 24 * 3600);
pub const CERT_RENEW_GRACE: Duration = Duration::from_secs(7 * 24 * 3600);
/// Delegations (admin authority) may not outlive this.
pub const DELEGATION_TTL_MAX: Duration = Duration::from_secs(30 * 24 * 3600);
/// Revocations retained per channel, and carried by one proof message.
pub const REVOCATIONS_MAX: usize = 128;
pub const REVOCATIONS_PER_PROOF_MAX: usize = 16;

/// Concurrent in-flight join exchanges across the admitter.
pub const JOINS_CONCURRENT_MAX: usize = 4;
/// One channel message, encoded.
pub const MESSAGE_MAX_BYTES: usize = 8192;

/// Password length bounds, in bytes.
pub const PASSWORD_MIN_BYTES: usize = 8;
pub const PASSWORD_MAX_BYTES: usize = 128;

/// Failures per (peer, channel) before a lockout starts.
pub const PEER_CHANNEL_FAILURES: u32 = 5;
/// First lockout length; doubles with every further strike up to [`LOCKOUT_MAX`].
pub const LOCKOUT_BASE: Duration = Duration::from_secs(60);
pub const LOCKOUT_MAX: Duration = Duration::from_secs(3600);
/// Failures per channel (any peer) per [`FAILURE_WINDOW`], then [`CHANNEL_COOLDOWN`].
pub const CHANNEL_FAILURES: usize = 20;
/// Failures per peer (any channel) per [`FAILURE_WINDOW`], then [`CHANNEL_COOLDOWN`].
pub const PEER_FAILURES: usize = 30;
pub const FAILURE_WINDOW: Duration = Duration::from_secs(3600);
pub const CHANNEL_COOLDOWN: Duration = Duration::from_secs(900);
/// Tracked (peer, channel) limiter entries.
pub const LIMITER_ENTRIES_MAX: usize = 1024;
