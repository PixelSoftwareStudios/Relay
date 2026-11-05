//! `WatchState`: snapshot or replay first, then live events, never a gap.

use std::collections::HashSet;
use std::sync::Arc;

use relay_api::pb::state_envelope::Body;
use relay_api::pb::{
	ResyncRequired, Snapshot, StateEnvelope, StateEvent, WatchStateRequest, state_event,
};
use relay_core::ClientId;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Response, Status};

use super::EngineService;
use crate::service::convert::call_id;
use crate::service::error::ServiceError;
use crate::service::feed::FeedItem;
use crate::service::handle::Subscribed;

const MAX_FILTER_CALLS: usize = 64;

/// What a watcher asked to see.
struct Filter {
	calls: HashSet<Vec<u8>>,
	include_peers: bool,
}

impl Filter {
	fn passes(&self, event: &StateEvent) -> bool {
		match &event.event {
			Some(state_event::Event::Call(call)) => self.wants_call(&call.call_id),
			Some(state_event::Event::Peer(_)) => self.include_peers,
			Some(state_event::Event::Removed(removed)) => match removed.kind.as_str() {
				"call" => self.wants_call(&removed.id),
				"peer" => self.include_peers,
				_ => true,
			},
			_ => true,
		}
	}

	fn wants_call(&self, id: &[u8]) -> bool {
		self.calls.is_empty() || self.calls.contains(id)
	}

	fn narrow(&self, snapshot: &mut Snapshot) {
		snapshot.calls.retain(|call| self.wants_call(&call.call_id));
		if !self.include_peers {
			snapshot.peers.clear();
		}
	}
}

pub(super) async fn open(
	service: &EngineService,
	caller: ClientId,
	body: WatchStateRequest,
) -> Result<Response<ReceiverStream<Result<StateEnvelope, Status>>>, Status> {
	if body.call_filter.len() > MAX_FILTER_CALLS {
		return Err(ServiceError::InvalidArgument("too many call filters".to_owned()).into());
	}
	for id in &body.call_filter {
		call_id(id)?;
	}
	let filter = Filter {
		calls: body.call_filter.into_iter().collect(),
		include_peers: body.include_peers,
	};
	let shared = &service.shared;
	let slot = shared.limiter.acquire()?;
	let ticket = shared.book.open_stream(caller);
	let watcher = shared.presence.watch(caller);
	let subscribed = shared.handle.subscribe(body.after_revision).await?;
	let (tx, rx) = mpsc::channel(shared.api.subscriber_queue);
	let stream = Stream {
		tx,
		incarnation: service.incarnation().to_vec(),
		filter,
	};
	tokio::spawn(async move {
		stream.run(subscribed).await;
		drop((slot, ticket, watcher));
	});
	Ok(Response::new(ReceiverStream::new(rx)))
}

struct Stream {
	tx: mpsc::Sender<Result<StateEnvelope, Status>>,
	incarnation: Vec<u8>,
	filter: Filter,
}

impl Stream {
	fn envelope(&self, revision: u64, body: Body) -> Result<StateEnvelope, Status> {
		Ok(StateEnvelope {
			engine_incarnation: self.incarnation.clone(),
			revision,
			body: Some(body),
		})
	}

	async fn send(&self, revision: u64, body: Body) -> bool {
		self.tx.send(self.envelope(revision, body)).await.is_ok()
	}

	async fn resync(&self, current: u64, reason: &str) {
		let body = Body::Resync(ResyncRequired {
			current_revision: current,
			reason: reason.to_owned(),
		});
		let _ = self.send(current, body).await;
	}

	async fn run(self, subscribed: Subscribed) {
		match subscribed {
			Subscribed::Resync { current } => self.resync(current, "replay window").await,
			Subscribed::Fresh { snapshot, live } => {
				let mut snapshot = *snapshot;
				self.filter.narrow(&mut snapshot);
				let revision = snapshot.revision;
				if self.send(revision, Body::Snapshot(snapshot)).await {
					self.follow(live, revision).await;
				}
			}
			Subscribed::Resume { replay, live } => {
				let mut last = replay.first().map_or(0, |(rev, _)| rev - 1);
				for (revision, event) in replay {
					last = revision;
					if self.filter.passes(&event) && !self.send(revision, Body::Event(event)).await
					{
						return;
					}
				}
				self.follow(live, last).await;
			}
		}
	}

	async fn follow(&self, mut live: broadcast::Receiver<Arc<FeedItem>>, mut last: u64) {
		loop {
			// A client that went away ends the watch even when nothing happens.
			let received = tokio::select! {
				received = live.recv() => received,
				() = self.tx.closed() => return,
			};
			match received {
				Ok(item) => match &*item {
					FeedItem::Event { revision, event } => {
						if *revision <= last {
							continue;
						}
						last = *revision;
						if self.filter.passes(event)
							&& !self
								.send(*revision, Body::Event(StateEvent::clone(event)))
								.await
						{
							return;
						}
					}
					FeedItem::Reset { revision } => {
						self.resync(*revision, "history lost").await;
						return;
					}
				},
				Err(broadcast::error::RecvError::Lagged(_)) => {
					self.resync(last, "subscriber lagged").await;
					return;
				}
				Err(broadcast::error::RecvError::Closed) => return,
			}
		}
	}
}
