//! Bounded command queues into worker threads.

use std::sync::{Arc, Mutex, OnceLock};
use std::thread::Thread;

use rtrb::{Consumer, Producer, RingBuffer};

struct Shared<T> {
	producer: Mutex<Producer<T>>,
	/// The consuming worker, set once it is running; pushes wake it.
	waker: OnceLock<Thread>,
}

/// Producer side of a bounded SPSC queue that several control-side callers
/// may share. The lock is taken only on the control side; the worker pops
/// from the lock-free consumer.
pub struct Inbox<T>(Arc<Shared<T>>);

impl<T> Clone for Inbox<T> {
	fn clone(&self) -> Self {
		Self(Arc::clone(&self.0))
	}
}

impl<T> Inbox<T> {
	/// A queue holding at most `capacity` pending commands.
	pub fn new(capacity: usize) -> (Self, Consumer<T>) {
		let (producer, consumer) = RingBuffer::new(capacity);
		let shared = Shared {
			producer: Mutex::new(producer),
			waker: OnceLock::new(),
		};
		(Self(Arc::new(shared)), consumer)
	}

	/// Registers the worker thread to unpark after every push, so an idle
	/// worker can sleep without a short poll. Only the first call counts.
	pub fn set_waker(&self, worker: Thread) {
		let _ = self.0.waker.set(worker);
	}

	/// Enqueues a command and wakes the worker, or hands the command back if
	/// the queue is full.
	pub fn push(&self, command: T) -> Result<(), T> {
		let mut producer = self.0.producer.lock().unwrap_or_else(|p| p.into_inner());
		producer
			.push(command)
			.map_err(|rtrb::PushError::Full(c)| c)?;
		drop(producer);
		if let Some(worker) = self.0.waker.get() {
			worker.unpark();
		}
		Ok(())
	}
}
