use super::JitterConfig;

/// Chooses the target playout margin: grows quickly after deadline misses
/// (capped at the ceiling), shrinks slowly after a long stable stretch.
#[derive(Debug, Clone, Copy)]
pub struct MarginController {
	min: u32,
	max: u32,
	grow_cooldown: u32,
	shrink_after: u32,
	target: u32,
	since_miss: u32,
	since_grow: u32,
}

impl MarginController {
	pub fn new(cfg: &JitterConfig) -> Self {
		Self {
			min: cfg.min_margin_frames,
			max: cfg.max_margin_frames,
			grow_cooldown: cfg.grow_cooldown_frames,
			shrink_after: cfg.shrink_after_frames,
			target: cfg
				.start_margin_frames
				.clamp(cfg.min_margin_frames, cfg.max_margin_frames),
			since_miss: 0,
			since_grow: u32::MAX,
		}
	}

	pub const fn target(&self) -> u32 {
		self.target
	}

	/// A deadline was missed (late packet or concealment).
	pub fn on_miss(&mut self) {
		self.since_miss = 0;
		if self.since_grow >= self.grow_cooldown && self.target < self.max {
			self.target += 1;
			self.since_grow = 0;
		}
	}

	/// One render frame elapsed.
	pub fn on_tick(&mut self) {
		self.since_miss = self.since_miss.saturating_add(1);
		self.since_grow = self.since_grow.saturating_add(1);
		if self.since_miss >= self.shrink_after && self.target > self.min {
			self.target -= 1;
			self.since_miss = 0;
		}
	}
}
