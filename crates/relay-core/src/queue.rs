//! A small bounded FIFO with explicit overflow.

use std::collections::VecDeque;

use crate::error::QueueFull;

/// Fixed-capacity queue; a full queue rejects instead of growing.
#[derive(Debug)]
pub struct BoundedQueue<T> {
	items: VecDeque<T>,
	capacity: usize,
}

impl<T> BoundedQueue<T> {
	/// Creates a queue holding at most `capacity` items.
	pub fn new(capacity: usize) -> Self {
		Self {
			items: VecDeque::with_capacity(capacity),
			capacity,
		}
	}

	/// Enqueues an item or reports the queue is full.
	pub fn push(&mut self, item: T) -> Result<(), QueueFull> {
		if self.items.len() >= self.capacity {
			return Err(QueueFull);
		}
		self.items.push_back(item);
		Ok(())
	}

	/// Dequeues the oldest item.
	pub fn pop(&mut self) -> Option<T> {
		self.items.pop_front()
	}

	/// Number of queued items.
	pub fn len(&self) -> usize {
		self.items.len()
	}

	/// Whether the queue is empty.
	pub fn is_empty(&self) -> bool {
		self.items.is_empty()
	}

	/// Maximum number of items.
	pub fn capacity(&self) -> usize {
		self.capacity
	}
}
