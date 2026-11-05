use super::Lane;

/// Record kind registry. Unknown kinds are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum RecordKind {
	Audio = 0x01,
	PathChallenge = 0x02,
	PathResponse = 0x03,
	Envelope = 0x80,
}

impl RecordKind {
	pub const fn from_byte(byte: u8) -> Option<Self> {
		match byte {
			0x01 => Some(Self::Audio),
			0x02 => Some(Self::PathChallenge),
			0x03 => Some(Self::PathResponse),
			0x80 => Some(Self::Envelope),
			_ => None,
		}
	}

	/// The only lane on which this kind may appear.
	pub const fn lane(self) -> Lane {
		match self {
			Self::Audio | Self::PathChallenge | Self::PathResponse => Lane::Media,
			Self::Envelope => Lane::Control,
		}
	}
}
