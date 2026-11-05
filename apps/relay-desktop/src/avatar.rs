//! The placeholder shown wherever a profile has no avatar. It is a local
//! asset only and is never sent over the network.

/// PNG bytes of the default avatar (256x256).
pub const DEFAULT_PNG: &[u8] = include_bytes!("../../../assets/icon/default-avatar.png");

/// What to draw for a profile's picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Avatar {
	/// No avatar hash is set: draw the default image.
	Placeholder,
	/// An avatar hash is set. Its image is not fetched by this client yet, so
	/// the default image stands in until it is.
	Set,
}

impl Avatar {
	/// Resolve from "does the profile advertise an avatar hash".
	pub fn resolve(has_avatar_hash: bool) -> Self {
		if has_avatar_hash {
			Self::Set
		} else {
			Self::Placeholder
		}
	}
}
