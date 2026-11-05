//! Height bookkeeping for a long list where only the visible rows are laid
//! out. Rows have unknown heights until drawn, so each starts at an estimate
//! and is corrected by measurement; the maths is independent of egui.

use std::ops::Range;

/// What to draw for one scroll position.
#[derive(Debug, Clone, PartialEq)]
pub struct Visible {
	/// Rows to lay out (visible plus overscan).
	pub rows: Range<usize>,
	/// Distance from the top of the list to the first drawn row.
	pub offset: f32,
}

#[derive(Debug, Clone)]
pub struct VirtualList {
	heights: Vec<f32>,
	total: f32,
	estimate: f32,
	/// Anything that changes row heights wholesale (width, filter).
	key: u64,
}

impl VirtualList {
	pub fn new(estimate: f32) -> Self {
		Self {
			heights: Vec::new(),
			total: 0.0,
			estimate: estimate.max(1.0),
			key: 0,
		}
	}

	pub fn len(&self) -> usize {
		self.heights.len()
	}

	pub fn is_empty(&self) -> bool {
		self.heights.is_empty()
	}

	pub fn total(&self) -> f32 {
		self.total
	}

	/// Match the list to `len` rows laid out under `key`. A longer list with
	/// the same key keeps its measurements (messages are appended); anything
	/// else starts over from the estimate.
	pub fn sync(&mut self, len: usize, key: u64) {
		if key != self.key || len < self.heights.len() {
			self.heights.clear();
			self.total = 0.0;
			self.key = key;
		}
		while self.heights.len() < len {
			self.heights.push(self.estimate);
			self.total += self.estimate;
		}
	}

	/// Record a measured height; true when it changed enough to matter.
	pub fn set_height(&mut self, row: usize, height: f32) -> bool {
		let Some(slot) = self.heights.get_mut(row) else {
			return false;
		};
		if !height.is_finite() || (*slot - height).abs() < 0.5 {
			return false;
		}
		self.total += height - *slot;
		*slot = height;
		true
	}

	/// Top of a row.
	pub fn offset(&self, row: usize) -> f32 {
		self.heights.iter().take(row).sum()
	}

	/// Rows intersecting `[top, top + height)` plus `overscan` points on each
	/// side. Cost is linear in the rows above the viewport; the list is
	/// bounded (a few thousand rows), and only these rows are ever laid out.
	pub fn visible(&self, top: f32, height: f32, overscan: f32) -> Visible {
		let from = (top - overscan).max(0.0);
		let to = top + height + overscan;
		let mut y = 0.0;
		let mut first = None;
		let mut end = self.heights.len();
		for (i, h) in self.heights.iter().enumerate() {
			if first.is_none() && y + h > from {
				first = Some((i, y));
			}
			if y >= to {
				end = i;
				break;
			}
			y += h;
		}
		match first {
			Some((i, offset)) => Visible {
				rows: i..end.max(i),
				offset,
			},
			None => Visible {
				rows: self.heights.len()..self.heights.len(),
				offset: self.total,
			},
		}
	}
}
