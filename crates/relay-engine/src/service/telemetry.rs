//! A bounded queue that sheds its oldest entry, for streams whose newest
//! sample matters more than a complete history.

use std::collections::VecDeque;

/// Fixed-capacity queue. Pushing into a full queue drops the oldest entry and
/// counts it, so a slow reader sees the present, never a growing backlog.
#[derive(Debug)]
pub struct DropOldest<T> {
	items: VecDeque<T>,
	capacity: usize,
	dropped: u64,
}

impl<T> DropOldest<T> {
	/// An empty queue holding at most `capacity` entries (at least one).
	pub fn new(capacity: usize) -> Self {
		let capacity = capacity.max(1);
		Self {
			items: VecDeque::with_capacity(capacity),
			capacity,
			dropped: 0,
		}
	}

	/// Appends an entry, dropping the oldest if the queue is full.
	pub fn push(&mut self, item: T) {
		if self.items.len() == self.capacity {
			self.items.pop_front();
			self.dropped += 1;
		}
		self.items.push_back(item);
	}

	/// Removes the oldest entry.
	pub fn pop(&mut self) -> Option<T> {
		self.items.pop_front()
	}

	/// Entries waiting.
	pub fn len(&self) -> usize {
		self.items.len()
	}

	/// Whether nothing is waiting.
	pub fn is_empty(&self) -> bool {
		self.items.is_empty()
	}

	/// Entries dropped so far.
	pub fn dropped(&self) -> u64 {
		self.dropped
	}
}
