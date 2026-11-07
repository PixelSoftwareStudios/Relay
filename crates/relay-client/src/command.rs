//! The request vocabulary both clients speak. Values are already validated by
//! the caller; call-bound commands carry the controller generation to use.

use std::fmt;

use relay_api::pb::{AdvertMode, InboundPolicy, TransmitMode, TrustState};
use subtle::ConstantTimeEq;
use zeroize::{Zeroize, ZeroizeOnDrop};

/// A secret string (channel password, invite token). `Debug` never prints it,
/// so commands can be logged or stored in view models without leaking. The text
/// is zeroed on drop and compared in constant time.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Secret(String);

impl Secret {
	/// Wrap secret text.
	pub fn new(text: impl Into<String>) -> Self {
		Self(text.into())
	}

	/// The secret text; call only where it goes on the wire.
	pub fn expose(&self) -> &str {
		&self.0
	}

	/// Whether nothing was entered.
	pub fn is_empty(&self) -> bool {
		self.0.is_empty()
	}
}

impl PartialEq for Secret {
	fn eq(&self, other: &Self) -> bool {
		self.0.as_bytes().ct_eq(other.0.as_bytes()).into()
	}
}

impl Eq for Secret {}

impl fmt::Debug for Secret {
	fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
		f.write_str("Secret(<redacted>)")
	}
}

/// How a channel is advertised and joined.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ChannelOptions {
	/// Advertise the channel name to members' devices.
	pub listed: bool,
	/// Require a password to join.
	pub password: bool,
	/// Require an invite token to join.
	pub invite_only: bool,
}

/// Who to call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallTarget {
	/// A discovered peer by 32-byte id.
	Peer(Vec<u8>),
	/// A manual `ip:port` on the local network.
	Address(String),
	/// A pasted `relay://ip:port#fingerprint` string; the engine validates it.
	ConnectionString(String),
}

/// Lifecycle verbs on an existing call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallAction {
	/// Accept an incoming call.
	Accept,
	/// Decline an incoming call.
	Decline,
	/// End a call.
	End,
}

/// One mutation of the engine.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
	/// Start a call.
	StartCall(CallTarget),
	/// Accept, decline or end a call.
	CallAction {
		/// Which verb.
		action: CallAction,
		/// Call id (16 bytes).
		call_id: Vec<u8>,
		/// Controller lease generation.
		generation: u64,
	},
	/// Set the call's transmit mode. `Unmuted` and `PushToTalk` are the consent to send the
	/// microphone; `Muted` withdraws it. One atomic request.
	SetTransmit {
		/// Call id.
		call_id: Vec<u8>,
		/// Controller lease generation.
		generation: u64,
		/// Requested mode.
		mode: TransmitMode,
	},
	/// Dial a scanned `relay://host:port#fingerprint` string and trust the device as verified
	/// (out of band) when its key matches the scanned fingerprint.
	ConnectAndTrust {
		/// The scanned connection string.
		connection_string: String,
	},
	/// Mute, deafen or set the receive gain of one call; `None` is unchanged.
	SetCallControls {
		/// Call id.
		call_id: Vec<u8>,
		/// Controller lease generation.
		generation: u64,
		/// Mute the microphone for this call.
		muted: Option<bool>,
		/// Deafen this call.
		deafened: Option<bool>,
		/// Receive gain.
		receive_gain: Option<f32>,
	},
	/// Global mute, deafen and gains; `None` is unchanged.
	SetGlobalControls {
		/// Global mute.
		muted: Option<bool>,
		/// Global deafen.
		deafened: Option<bool>,
		/// Microphone gain.
		microphone_gain: Option<f32>,
		/// Playback gain.
		playback_gain: Option<f32>,
		/// Push-to-talk release delay in milliseconds (0 to 2000).
		ptt_release_delay_ms: Option<u32>,
	},
	/// Ring, ringback and call-sound preferences; `None` is unchanged.
	SetRingPreferences {
		/// Ring for an incoming call while a client is attached.
		ringtone_enabled: Option<bool>,
		/// Ring volume, 0.0 to 1.0 (applies to a ring that plays).
		ringtone_volume: Option<f32>,
		/// Ringback while an outgoing call rings.
		ringback_enabled: Option<bool>,
		/// Connect and hang-up blips.
		call_sounds_enabled: Option<bool>,
	},
	/// Play the ring on this device's own output for about two seconds.
	PreviewTone,
	/// Use an audio file as this device's incoming-call ringtone. The engine decodes it within
	/// the `limits.ringtone_*` bounds and refuses what it cannot play.
	SetRingtone {
		/// Name shown in the settings (no path).
		file_name: String,
		/// The file as chosen.
		data: Vec<u8>,
	},
	/// Go back to the built-in ring.
	ResetRingtone,
	/// Select input and output devices.
	SelectDevices {
		/// Input device id.
		input_id: String,
		/// Output device id.
		output_id: String,
	},
	/// Select a noise suppression backend.
	SetNoiseSuppression {
		/// Qualified backend id.
		backend: String,
	},
	/// Change the display name.
	SetProfile {
		/// New name.
		display_name: String,
	},
	/// Upload an avatar as raw encoded bytes; the engine validates the image.
	UploadAvatar {
		/// Encoded image.
		bytes: Vec<u8>,
		/// Declared media type.
		media_type: String,
	},
	/// Remove the local avatar; trusted peers stop showing it.
	ClearAvatar,
	/// Forget a stored trust decision by peer id (32 bytes) or, when `peer_id` is empty, by
	/// fingerprint prefix or exact name in `query`.
	ForgetPeer {
		/// Peer id (32 bytes), or empty.
		peer_id: Vec<u8>,
		/// Hex fingerprint prefix (at least 8 digits) or exact display name.
		query: String,
	},
	/// Replace the pinned key of a contact whose key changed by the key it presents now.
	/// `peer_id` is that new key (32 bytes). The engine accepts it only when `fingerprint`
	/// (the 64 hex digits the user typed or scanned) is that key's full fingerprint and
	/// `verified` says they compared it on the other device.
	ReplacePinnedKey {
		/// The new (refused) key digest, 32 bytes.
		peer_id: Vec<u8>,
		/// Full fingerprint, separators and case ignored.
		fingerprint: String,
		/// The user compared it with the other device.
		verified: bool,
	},
	/// Replace the favorites (the quick-call grid) by this ordered list of trusted peers,
	/// each a 32-byte peer id. More than 6, a repeat or an untrusted peer fails the operation.
	SetFavorites {
		/// Peer ids, first = position 0.
		peer_ids: Vec<Vec<u8>>,
	},
	/// Change preferences by flat key while the daemon runs. Only the operation comes back;
	/// use `RelayClient::set_settings` for the per-key outcome.
	SetSettings {
		/// `section.key` and text value, parsed like `relay config set`.
		values: Vec<(String, String)>,
	},
	/// Delete every call history entry on this device.
	ClearCallHistory,
	/// Start the microphone test.
	StartMicTest {
		/// Play the microphone back locally.
		local_playback: bool,
		/// The feedback warning was confirmed.
		confirmed_feedback: bool,
	},
	/// Stop the microphone test.
	StopMicTest,
	/// Record a trust decision about a peer.
	DecideTrust {
		/// Peer id (32 bytes).
		peer_id: Vec<u8>,
		/// Public key SHA-256 the user compared; empty when forgetting.
		public_key_sha256: Vec<u8>,
		/// Requested state.
		requested: TrustState,
		/// Whether the fingerprint was compared out of band.
		out_of_band_verified: bool,
	},
	/// Create a channel owned by this device.
	CreateChannel {
		/// Channel name.
		name: String,
		/// Listing and join requirements.
		options: ChannelOptions,
		/// Password, when `options.password` is set.
		password: Secret,
	},
	/// Join a channel by id and password, or by invite token.
	JoinChannel {
		/// Channel id (16 bytes); empty when joining by invite alone.
		channel_id: Vec<u8>,
		/// Channel password, or empty.
		password: Secret,
		/// Invite token, or empty.
		invite: Secret,
	},
	/// Leave a channel and delete its local secrets.
	LeaveChannel {
		/// Channel id (16 bytes).
		channel_id: Vec<u8>,
	},
	/// Revoke a member by identity fingerprint.
	RevokeMember {
		/// Channel id (16 bytes).
		channel_id: Vec<u8>,
		/// Member fingerprint.
		fingerprint: Vec<u8>,
	},
	/// Change inbound policy and/or advertisement mode; `None` is unchanged.
	SetPrivacy {
		/// Who may ring this device.
		inbound: Option<InboundPolicy>,
		/// Whether this device is advertised.
		advert: Option<AdvertMode>,
	},
	/// Send text into a session (needs the `messages` capability, like every command below
	/// that names a `session_id`).
	SendSessionMessage {
		/// Session id (16 bytes).
		session_id: Vec<u8>,
		/// Text; the engine validates and sanitizes it.
		text: String,
	},
	/// Send an inline image (original bytes, at most 15 KiB).
	SendSessionImage {
		/// Session id.
		session_id: Vec<u8>,
		/// Compressed image bytes.
		image: Vec<u8>,
	},
	/// Retry a failed local message.
	RetrySessionMessage {
		/// Session id.
		session_id: Vec<u8>,
		/// The message's sequence number.
		seq: u64,
	},
	/// Offer a file. The daemon opens `path`: absolute, regular file, no symlinks.
	OfferFile {
		/// Session id.
		session_id: Vec<u8>,
		/// Absolute path on this machine.
		path: String,
	},
	/// Accept (into an absolute existing directory) or reject an offer.
	RespondOffer {
		/// Session id.
		session_id: Vec<u8>,
		/// Offer id (16 bytes).
		offer_id: Vec<u8>,
		/// Accept or reject.
		accept: bool,
		/// Destination directory when accepting.
		destination_dir: String,
		/// Suggested file name; empty keeps the offered one.
		file_name: String,
	},
	/// Ask the sender for a text preview of an offered file.
	RequestPreview {
		/// Session id.
		session_id: Vec<u8>,
		/// Offer id.
		offer_id: Vec<u8>,
	},
	/// Sender: approve or refuse a preview request.
	ApprovePreview {
		/// Session id.
		session_id: Vec<u8>,
		/// Offer id.
		offer_id: Vec<u8>,
		/// Approve the preview.
		approve: bool,
	},
	/// Cancel a transfer or withdraw an offer.
	CancelTransfer {
		/// Session id.
		session_id: Vec<u8>,
		/// Offer id.
		offer_id: Vec<u8>,
	},
	/// Change what a session accepts and sends; `None` is unchanged.
	SetSessionPermissions {
		/// Session id.
		session_id: Vec<u8>,
		/// Text messages.
		allow_messages: Option<bool>,
		/// Images and file offers.
		allow_files: Option<bool>,
	},
	/// Pin or unpin a message locally.
	PinMessage {
		/// Session id.
		session_id: Vec<u8>,
		/// Whether the message is ours.
		outgoing: bool,
		/// Sender's sequence.
		seq: u64,
		/// Pin state.
		pinned: bool,
	},
	/// Clear a transcript locally (an ended session is removed).
	ClearSession {
		/// Session id.
		session_id: Vec<u8>,
	},
	/// Live messaging preferences; `None` is unchanged.
	SetMessagingSettings {
		/// `messages.enabled`.
		enabled: Option<bool>,
		/// `messages.max_offer_size_mib`.
		max_offer_size_mib: Option<u32>,
		/// `messages.transfer_rate_limit_mbps`.
		transfer_rate_limit_mbps: Option<u32>,
	},
}

impl Command {
	/// True for the session messaging commands, which need the `messages` capability.
	pub fn is_messaging(&self) -> bool {
		matches!(
			self,
			Self::SendSessionMessage { .. }
				| Self::SendSessionImage { .. }
				| Self::RetrySessionMessage { .. }
				| Self::OfferFile { .. }
				| Self::RespondOffer { .. }
				| Self::RequestPreview { .. }
				| Self::ApprovePreview { .. }
				| Self::CancelTransfer { .. }
				| Self::SetSessionPermissions { .. }
				| Self::PinMessage { .. }
				| Self::ClearSession { .. }
				| Self::SetMessagingSettings { .. }
		)
	}
}
