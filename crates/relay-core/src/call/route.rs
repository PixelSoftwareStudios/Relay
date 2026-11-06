//! The per-call transmit mode, which is also the call's microphone consent.
//!
//! Choosing [`RouteMode::Unmuted`] or [`RouteMode::PushToTalk`] is the explicit consent to send
//! the microphone to that call; [`RouteMode::Muted`] withdraws it. Mode and consent are one
//! value, so they can never disagree, and a change takes a new route generation: audio
//! captured before the change is never sent (RT-02).

/// How the microphone may be routed to a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum RouteMode {
	/// Muted: the microphone is never sent. No consent is held. The default.
	#[default]
	Muted,
	/// Unmuted (open mic): continuous sending while the gate is open.
	Unmuted,
	/// Sending only while a live push-to-talk lease exists.
	PushToTalk,
}

/// Per-call transmit mode and consent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct TransmitRoute {
	/// The consent mode.
	pub mode: RouteMode,
	/// Keep transmitting after the controlling client goes away.
	/// Only meaningful for [`RouteMode::Unmuted`].
	pub background: bool,
}

impl TransmitRoute {
	/// Muted: no transmit consent.
	pub const MUTED: Self = Self {
		mode: RouteMode::Muted,
		background: false,
	};

	/// Open microphone, tied to the controller.
	pub const fn unmuted() -> Self {
		Self {
			mode: RouteMode::Unmuted,
			background: false,
		}
	}

	/// Open microphone that survives loss of the controller.
	pub const fn unmuted_background() -> Self {
		Self {
			mode: RouteMode::Unmuted,
			background: true,
		}
	}

	/// Push-to-talk.
	pub const fn push_to_talk() -> Self {
		Self {
			mode: RouteMode::PushToTalk,
			background: false,
		}
	}

	/// Whether the description is internally consistent.
	pub const fn is_valid(self) -> bool {
		!self.background || matches!(self.mode, RouteMode::Unmuted)
	}

	/// Whether the route grants any transmit consent.
	pub const fn is_authorized(self) -> bool {
		!matches!(self.mode, RouteMode::Muted)
	}
}

/// Receive gain in per-mille (1000 = unity), bounded to 0..=2000.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Gain(u16);

impl Gain {
	/// Highest permitted gain (+6 dB).
	pub const MAX_PERMILLE: u16 = 2000;
	/// Unity gain.
	pub const UNITY: Self = Self(1000);
	/// Silence. Not a privacy mute: the transmit gate decides what is sent.
	pub const SILENT: Self = Self(0);

	/// Builds a gain, clamping to the permitted range.
	pub const fn from_permille(value: u16) -> Self {
		if value > Self::MAX_PERMILLE {
			Self(Self::MAX_PERMILLE)
		} else {
			Self(value)
		}
	}

	/// Value in per-mille.
	pub const fn permille(self) -> u16 {
		self.0
	}
}

impl Default for Gain {
	fn default() -> Self {
		Self::UNITY
	}
}

/// Per-call user controls. Underlying mute is preserved across deafen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct CallControls {
	/// Stops this call's outgoing route only.
	pub muted: bool,
	/// Suppresses this call's receive audio and outgoing route.
	pub deafened: bool,
	/// Receive gain applied before the master gain.
	pub gain: Gain,
}
