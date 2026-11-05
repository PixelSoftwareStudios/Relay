//! The API's own revisioned event stream.
//!
//! The domain core numbers lifecycle events; the API additionally carries
//! peer, device and settings changes. This feed gives all of them one gapless
//! sequence, keeps a bounded replay window and fans out to subscribers.

use std::collections::VecDeque;
use std::sync::Arc;

use relay_api::pb::StateEvent;
use tokio::sync::broadcast;

/// One delivery to subscribers.
#[derive(Debug, Clone)]
pub enum FeedItem {
	/// An event at an API revision.
	Event {
		/// Revision after applying the event.
		revision: u64,
		/// The event.
		event: Box<StateEvent>,
	},
	/// History was lost; subscribers must open a fresh watch.
	Reset {
		/// The revision to resume from after a snapshot.
		revision: u64,
	},
}

/// Gapless event sequence with replay and fan-out.
#[derive(Debug)]
pub struct Feed {
	revision: u64,
	ring: VecDeque<(u64, StateEvent)>,
	capacity: usize,
	max_lag: usize,
	tx: broadcast::Sender<Arc<FeedItem>>,
}

impl Feed {
	/// A feed replaying up to `capacity` events and fanning out through
	/// queues of `subscriber_queue` items.
	pub fn new(capacity: usize, subscriber_queue: usize) -> Self {
		let (tx, _) = broadcast::channel(subscriber_queue);
		Self {
			revision: 0,
			ring: VecDeque::with_capacity(capacity.min(1024)),
			capacity,
			max_lag: subscriber_queue,
			tx,
		}
	}

	/// The newest revision.
	pub fn revision(&self) -> u64 {
		self.revision
	}

	/// Appends an event and returns its revision.
	pub fn push(&mut self, event: StateEvent) -> u64 {
		self.revision += 1;
		if self.ring.len() == self.capacity {
			self.ring.pop_front();
		}
		self.ring.push_back((self.revision, event.clone()));
		// No receiver is fine: nobody is watching.
		let _ = self.tx.send(Arc::new(FeedItem::Event {
			revision: self.revision,
			event: Box::new(event),
		}));
		self.revision
	}

	/// Drops history and tells every subscriber to start over.
	pub fn reset(&mut self) {
		self.revision += 1;
		self.ring.clear();
		let _ = self.tx.send(Arc::new(FeedItem::Reset {
			revision: self.revision,
		}));
	}

	/// Events after `after` with no gap, or `None` when the window no longer
	/// covers them, the caller is ahead of us, or the backlog exceeds the
	/// subscriber queue.
	pub fn replay_after(&self, after: u64) -> Option<Vec<(u64, StateEvent)>> {
		if after > self.revision {
			return None;
		}
		let pending: Vec<_> = self
			.ring
			.iter()
			.filter(|(revision, _)| *revision > after)
			.cloned()
			.collect();
		let covered = pending.first().is_none_or(|(first, _)| *first == after + 1);
		(covered && pending.len() <= self.max_lag).then_some(pending)
	}

	/// A receiver for everything pushed from now on.
	pub fn subscribe(&self) -> broadcast::Receiver<Arc<FeedItem>> {
		self.tx.subscribe()
	}
}
