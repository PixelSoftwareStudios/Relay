//! Bounded command queues into worker threads.

use std::sync::{Arc, Mutex};

use rtrb::{Consumer, Producer, RingBuffer};

/// Producer side of a bounded SPSC queue that several control-side callers
/// may share. The lock is taken only on the control side; the worker pops
/// from the lock-free consumer.
pub struct Inbox<T>(Arc<Mutex<Producer<T>>>);

impl<T> Clone for Inbox<T> {
	fn clone(&self) -> Self {
		Self(Arc::clone(&self.0))
	}
}

impl<T> Inbox<T> {
	/// A queue holding at most `capacity` pending commands.
	pub fn new(capacity: usize) -> (Self, Consumer<T>) {
		let (producer, consumer) = RingBuffer::new(capacity);
		(Self(Arc::new(Mutex::new(producer))), consumer)
	}

	/// Enqueues a command, or hands it back if the queue is full.
	pub fn push(&self, command: T) -> Result<(), T> {
		let mut producer = self.0.lock().unwrap_or_else(|p| p.into_inner());
		producer.push(command).map_err(|rtrb::PushError::Full(c)| c)
	}
}
