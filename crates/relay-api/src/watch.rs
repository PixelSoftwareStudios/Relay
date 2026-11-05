//! Client-side handling of `WatchState`: gap detection and bounded delivery.

use tokio::sync::mpsc;
use tonic::Streaming;

use crate::ApiError;
use crate::pb::state_envelope::Body;
use crate::pb::{ResyncRequired, Snapshot, StateEnvelope, StateEvent};

/// Parameters of a `WatchState` subscription.
#[derive(Debug, Clone)]
pub struct WatchOptions {
	/// Resume after this revision; `None` starts with an atomic snapshot.
	pub after_revision: Option<u64>,
	/// Restrict call events to these call ids (empty means all calls).
	pub call_filter: Vec<Vec<u8>>,
	/// Whether peer events are delivered.
	pub include_peers: bool,
}

impl Default for WatchOptions {
	fn default() -> Self {
		Self {
			after_revision: None,
			call_filter: Vec::new(),
			include_peers: true,
		}
	}
}

impl WatchOptions {
	/// Unfiltered streams must be contiguous; filtered ones may skip revisions.
	pub fn is_strict(&self) -> bool {
		self.call_filter.is_empty() && self.include_peers
	}
}

/// One validated item from a watch stream.
#[derive(Debug, Clone)]
pub enum WatchItem {
	/// Atomic snapshot; replaces all cached state.
	Snapshot(Snapshot),
	/// Incremental event at `revision`.
	Event {
		/// Revision after applying the event.
		revision: u64,
		/// The event payload.
		event: StateEvent,
	},
	/// Engine cannot replay; open a fresh watch (no `after_revision`).
	Resync(ResyncRequired),
}

/// Pure revision/incarnation checker, independent of any transport.
#[derive(Debug, Clone)]
pub struct StateTracker {
	strict: bool,
	incarnation: Option<Vec<u8>>,
	last: Option<u64>,
}

impl StateTracker {
	/// Track a fresh stream; `strict` rejects revision gaps.
	pub fn new(strict: bool) -> Self {
		Self {
			strict,
			incarnation: None,
			last: None,
		}
	}

	/// Track a stream that continues after `revision` of `incarnation`.
	pub fn resumed(strict: bool, incarnation: Vec<u8>, revision: u64) -> Self {
		Self {
			strict,
			incarnation: Some(incarnation),
			last: Some(revision),
		}
	}

	/// Last accepted revision.
	pub fn last_revision(&self) -> Option<u64> {
		self.last
	}

	/// Validate one envelope against the stream so far.
	pub fn observe(&mut self, envelope: StateEnvelope) -> Result<WatchItem, ApiError> {
		match &self.incarnation {
			Some(known) if *known != envelope.engine_incarnation => {
				return Err(ApiError::IncarnationChanged);
			}
			Some(_) => {}
			None => self.incarnation = Some(envelope.engine_incarnation.clone()),
		}
		let revision = envelope.revision;
		match envelope.body.ok_or(ApiError::StreamClosed)? {
			Body::Snapshot(snapshot) => {
				self.last = Some(revision);
				Ok(WatchItem::Snapshot(snapshot))
			}
			Body::Resync(resync) => {
				self.last = None;
				self.incarnation = None;
				Ok(WatchItem::Resync(resync))
			}
			Body::Event(event) => {
				if let Some(last) = self.last {
					if revision <= last {
						return Err(ApiError::StaleRevision {
							last,
							received: revision,
						});
					}
					if self.strict && revision != last + 1 {
						return Err(ApiError::RevisionGap {
							expected: last + 1,
							received: revision,
						});
					}
				}
				self.last = Some(revision);
				Ok(WatchItem::Event { revision, event })
			}
		}
	}
}

/// A live `WatchState` stream with gap detection.
pub struct StateWatch {
	stream: Streaming<StateEnvelope>,
	tracker: StateTracker,
}

impl StateWatch {
	pub(crate) fn new(stream: Streaming<StateEnvelope>, tracker: StateTracker) -> Self {
		Self { stream, tracker }
	}

	/// Next validated item; `Ok(None)` when the engine closed the stream.
	pub async fn next(&mut self) -> Result<Option<WatchItem>, ApiError> {
		match self.stream.message().await? {
			Some(envelope) => self.tracker.observe(envelope).map(Some),
			None => Ok(None),
		}
	}

	/// Move reading to a background task feeding a queue of `capacity` items.
	/// If the consumer falls behind, the queue ends with
	/// [`ApiError::SubscriptionOverflow`] instead of buffering without bound.
	pub fn into_bounded(mut self, capacity: usize) -> BoundedWatch {
		let (tx, rx) = mpsc::channel(capacity.max(1) + 1);
		tokio::spawn(async move {
			loop {
				let item = match self.next().await {
					Ok(Some(item)) => Ok(item),
					Ok(None) => break,
					Err(e) => Err(e),
				};
				let failed = item.is_err();
				if tx.capacity() <= 1 && !failed {
					// Keep the last slot for the overflow marker.
					let _ = tx.try_send(Err(ApiError::SubscriptionOverflow));
					break;
				}
				if tx.send(item).await.is_err() || failed {
					break;
				}
			}
		});
		BoundedWatch { rx }
	}
}

/// Receiver side of a bounded subscription.
pub struct BoundedWatch {
	rx: mpsc::Receiver<Result<WatchItem, ApiError>>,
}

impl BoundedWatch {
	/// Next item; `None` once the stream ended cleanly.
	pub async fn recv(&mut self) -> Option<Result<WatchItem, ApiError>> {
		self.rx.recv().await
	}
}
