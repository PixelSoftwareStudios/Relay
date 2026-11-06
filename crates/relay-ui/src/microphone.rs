//! What the shell knows about microphone permission, and what the Calls page
//! says about it. The state is the shell's (the OS owns the permission); the UI
//! only reads it per frame and asks the shell to prompt or open the settings.

/// Whether this app may record, as the shell last read it from the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MicrophoneAccess {
	/// Allowed, or a platform that asks for nothing. Nothing is shown.
	#[default]
	Ready,
	/// The OS has not been asked yet (or the first answer is pending).
	NotAsked,
	/// Refused, but the OS will show its prompt again.
	Denied,
	/// Refused for good: only the system settings can change it.
	Blocked,
}

/// The card on the Calls page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrophoneCard {
	/// "Relay needs your microphone" with an Allow button.
	Ask,
	/// "Microphone access is turned off" with an Open settings button.
	TurnedOff,
}

/// What the user did with the card.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MicrophoneChoice {
	Allow,
	OpenSettings,
	Dismiss,
}

/// The card to show, if any. A dismissed card stays hidden until the access
/// changes (the page clears the dismissal then, see [`Dismissal`]).
pub fn card(access: MicrophoneAccess, dismissed: bool) -> Option<MicrophoneCard> {
	if dismissed {
		return None;
	}
	match access {
		MicrophoneAccess::Ready => None,
		MicrophoneAccess::NotAsked | MicrophoneAccess::Denied => Some(MicrophoneCard::Ask),
		MicrophoneAccess::Blocked => Some(MicrophoneCard::TurnedOff),
	}
}

/// Remembers a dismissal for one access state: when the OS answer changes, the
/// card may come back (a revoked permission must not stay hidden).
#[derive(Debug, Default)]
pub struct Dismissal {
	seen: Option<MicrophoneAccess>,
	dismissed: bool,
}

impl Dismissal {
	/// Whether the card is dismissed for the current `access`.
	pub fn is_dismissed(&mut self, access: MicrophoneAccess) -> bool {
		if self.seen != Some(access) {
			self.seen = Some(access);
			self.dismissed = false;
		}
		self.dismissed
	}

	pub fn dismiss(&mut self) {
		self.dismissed = true;
	}
}
