//! The messaging event feed: one revision sequence over every session, a bounded replay
//! window and drop-oldest fan-out. A subscriber that falls behind gets a resync marker.

use std::collections::VecDeque;
use std::sync::Arc;

use relay_api::pb::SessionEvent;
use tokio::sync::broadcast;

/// One delivery to subscribers.
#[derive(Debug, Clone)]
pub struct FeedItem {
	/// Revision after the event.
	pub revision: u64,
	/// Session the event belongs to (for filters).
	pub session: [u8; 16],
	/// The event.
	pub event: Arc<SessionEvent>,
}

/// Replayable, revisioned fan-out.
pub struct SessionFeed {
	revision: u64,
	ring: VecDeque<FeedItem>,
	capacity: usize,
	max_lag: usize,
	tx: broadcast::Sender<FeedItem>,
}

impl SessionFeed {
	/// Replays up to `capacity` events; subscriber queues hold `queue` items.
	pub fn new(capacity: usize, queue: usize) -> Self {
		Self {
			revision: 0,
			ring: VecDeque::new(),
			capacity,
			max_lag: queue,
			tx: broadcast::channel(queue.max(1)).0,
		}
	}

	/// The newest revision.
	pub fn revision(&self) -> u64 {
		self.revision
	}

	/// Appends an event.
	pub fn push(&mut self, session: [u8; 16], event: SessionEvent) -> u64 {
		self.revision += 1;
		let item = FeedItem {
			revision: self.revision,
			session,
			event: Arc::new(event),
		};
		if self.ring.len() == self.capacity {
			self.ring.pop_front();
		}
		self.ring.push_back(item.clone());
		let _ = self.tx.send(item);
		self.revision
	}

	/// Events after `after` without a gap, or `None` when the window no longer covers them.
	pub fn replay_after(&self, after: u64) -> Option<Vec<FeedItem>> {
		if after > self.revision {
			return None;
		}
		let pending: Vec<_> = self
			.ring
			.iter()
			.filter(|item| item.revision > after)
			.cloned()
			.collect();
		let covered = pending
			.first()
			.is_none_or(|first| first.revision == after + 1);
		(covered && pending.len() <= self.max_lag).then_some(pending)
	}

	/// Everything pushed from now on.
	pub fn subscribe(&self) -> broadcast::Receiver<FeedItem> {
		self.tx.subscribe()
	}
}
