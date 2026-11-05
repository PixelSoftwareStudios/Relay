/// Static configuration of one receive scheduler.
///
/// Margins are expressed in whole frames: the playout origin can only move
/// in codec-frame steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JitterConfig {
	/// Samples per packet at 48 kHz (480 for 10 ms).
	pub frame_samples: u32,
	/// Packet slots; bounds memory, not delay.
	pub slots: usize,
	/// Margin applied at the start of a segment.
	pub start_margin_frames: u32,
	pub min_margin_frames: u32,
	pub max_margin_frames: u32,
	/// Extra frames beyond the margin ceiling still admitted as reordering.
	pub reorder_slack_frames: u32,
	/// Continuous concealment before the fade-out begins.
	pub max_plc_frames: u32,
	/// Frames of fade-out before the stream is declared interrupted.
	pub fade_frames: u32,
	/// A timestamp jump larger than this starts a fresh segment.
	pub max_gap_samples: u32,
	/// Minimum frames between two margin increases.
	pub grow_cooldown_frames: u32,
	/// Miss-free frames required before the margin shrinks by one frame.
	pub shrink_after_frames: u32,
}

impl JitterConfig {
	/// Balanced LAN profile: 15 ms start, 5-40 ms range, 10 ms frames.
	pub const BALANCED: Self = Self {
		frame_samples: 480,
		slots: 128,
		start_margin_frames: 2,
		min_margin_frames: 1,
		max_margin_frames: 4,
		reorder_slack_frames: 4,
		max_plc_frames: 3,
		fade_frames: 1,
		max_gap_samples: 48_000,
		grow_cooldown_frames: 20,
		shrink_after_frames: 500,
	};

	/// Aggressive profile: 5-10 ms start, small ceiling.
	pub const AGGRESSIVE: Self = Self {
		start_margin_frames: 1,
		max_margin_frames: 2,
		..Self::BALANCED
	};

	/// Stable profile: 25 ms start, 60 ms ceiling.
	pub const STABLE: Self = Self {
		start_margin_frames: 3,
		max_margin_frames: 6,
		..Self::BALANCED
	};

	/// Frames of lookahead admitted past the playhead.
	pub fn horizon_frames(&self) -> u64 {
		let wanted = u64::from(self.max_margin_frames + self.reorder_slack_frames);
		wanted.min(self.slots as u64 - 1)
	}

	/// Worst-case buffering delay in samples the planner can ever add.
	pub fn max_delay_samples(&self) -> u32 {
		self.max_margin_frames * self.frame_samples
	}
}
