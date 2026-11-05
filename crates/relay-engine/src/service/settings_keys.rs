//! Which preference keys `SetSettings` may change, and how a change takes effect.

/// Keys a client may not change through the settings RPC: they widen what the device exposes
/// or records, or decide where key material lives. They stay settable with the daemon stopped
/// (`relay config set`), where the user is at the keyboard of the machine itself.
const SENSITIVE_KEYS: [&str; 3] = [
	"diagnostics.record_raw_audio",
	"network.allow_wan",
	"network.allow_overlay_manual",
];

/// Keys the running daemon applies at once (the rest are saved and read at the next start).
const LIVE_KEYS: [&str; 14] = [
	"profile.display_name",
	"audio.microphone_gain",
	"audio.playback_gain",
	"audio.ringtone_enabled",
	"audio.ringtone_volume",
	"audio.ringback_enabled",
	"audio.call_sounds_enabled",
	"privacy.inbound_policy",
	"privacy.advert_mode",
	"ptt.release_delay_ms",
	"clients.autostart_daemon",
	"messages.enabled",
	"messages.max_offer_size_mib",
	"messages.transfer_rate_limit_mbps",
];

/// True for a key a client may not change over the API.
pub fn is_sensitive(key: &str) -> bool {
	key.starts_with("identity.") || SENSITIVE_KEYS.contains(&key)
}

/// True for a key a running daemon applies without a restart.
pub fn is_live(key: &str) -> bool {
	LIVE_KEYS.contains(&key)
}

/// True for the `messages.*` keys the messaging actor owns.
pub fn is_messaging(key: &str) -> bool {
	key.starts_with("messages.")
}

/// What became of one requested key.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum KeyOutcome {
	/// Saved and in effect now.
	AppliedLive,
	/// Saved; takes effect at the next daemon start.
	SavedNeedsRestart(String),
	/// Not saved, with the reason.
	Rejected(String),
}

/// One key of a settings request and its outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyResult {
	/// The preference key.
	pub key: String,
	/// What happened to it.
	pub outcome: KeyOutcome,
}
