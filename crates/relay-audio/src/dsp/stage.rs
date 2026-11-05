//! What a processing stage provides, so the chain can refuse duplicates.

/// A kind of processing a stage performs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
	HighPass,
	EchoCancel,
	NoiseSuppress,
	AutoGain,
	Limit,
}

impl StageKind {
	/// Chain order is fixed: high-pass, AEC, NS, AGC, limiter.
	pub const ORDER: [StageKind; 5] = [
		Self::HighPass,
		Self::EchoCancel,
		Self::NoiseSuppress,
		Self::AutoGain,
		Self::Limit,
	];

	const fn bit(self) -> u8 {
		1 << self as u8
	}
}

/// The set of [`StageKind`]s a component implements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Provides(u8);

impl Provides {
	pub const NONE: Self = Self(0);

	pub const fn of(kind: StageKind) -> Self {
		Self(kind.bit())
	}

	#[must_use]
	pub const fn with(self, kind: StageKind) -> Self {
		Self(self.0 | kind.bit())
	}

	pub const fn contains(self, kind: StageKind) -> bool {
		self.0 & kind.bit() != 0
	}

	pub const fn is_empty(self) -> bool {
		self.0 == 0
	}

	/// The first kind present in both sets, in chain order.
	pub fn overlap(self, other: Self) -> Option<StageKind> {
		StageKind::ORDER
			.into_iter()
			.find(|k| self.contains(*k) && other.contains(*k))
	}

	pub fn union(self, other: Self) -> Self {
		Self(self.0 | other.0)
	}
}
