//! `WatchSession`: the session list or a replay first, then live events. A watcher that
//! falls behind is told to resync and ends; it never makes the engine buffer.

use relay_api::pb::session_envelope::Body;
use relay_api::pb::{SessionEnvelope, SessionResync, WatchSessionRequest};
use tokio::sync::{broadcast, mpsc};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status};

use super::EngineService;
use super::messages::{reader, stopped};
use crate::messaging::{FeedItem, Subscribed};
use crate::service::error::ServiceError;

type Resp<T> = Result<Response<T>, Status>;

pub(super) async fn watch_session(
	service: &EngineService,
	request: Request<WatchSessionRequest>,
) -> Resp<super::Stream<SessionEnvelope>> {
	let (hub, caller) = reader(service, &request, |b| b.client.as_ref())?;
	let body = request.into_inner();
	let filter = match body.session_id.len() {
		0 => None,
		16 => <[u8; 16]>::try_from(body.session_id.as_slice()).ok(),
		_ => return Err(ServiceError::InvalidArgument("session id".to_owned()).into()),
	};
	let shared = &service.shared;
	let slot = shared.limiter.acquire()?;
	let ticket = shared.book.open_stream(caller);
	let after = body.after_revision;
	let subscribed = hub
		.ask(move |h| h.subscribe(after))
		.await
		.map_err(stopped)?;
	let (tx, rx) = mpsc::channel(shared.api.subscriber_queue);
	let stream = Watcher {
		tx,
		incarnation: service.incarnation().to_vec(),
		filter,
	};
	tokio::spawn(async move {
		stream.run(subscribed).await;
		drop((slot, ticket));
	});
	Ok(Response::new(ReceiverStream::new(rx)))
}

struct Watcher {
	tx: mpsc::Sender<Result<SessionEnvelope, Status>>,
	incarnation: Vec<u8>,
	filter: Option<[u8; 16]>,
}

impl Watcher {
	async fn send(&self, revision: u64, body: Body) -> bool {
		let envelope = SessionEnvelope {
			engine_incarnation: self.incarnation.clone(),
			revision,
			body: Some(body),
		};
		self.tx.send(Ok(envelope)).await.is_ok()
	}

	async fn event(&self, item: &FeedItem) -> bool {
		if self.filter.is_some_and(|wanted| wanted != item.session) {
			return true;
		}
		self.send(item.revision, Body::Event((*item.event).clone()))
			.await
	}

	async fn run(self, subscribed: Subscribed) {
		match subscribed {
			Subscribed::Resync { current } => {
				let _ = self.resync(current, "replay window").await;
			}
			Subscribed::Fresh {
				mut list,
				revision,
				live,
			} => {
				if let Some(wanted) = self.filter {
					list.sessions.retain(|s| s.session_id == wanted);
				}
				if self.send(revision, Body::List(list)).await {
					self.follow(live, revision).await;
				}
			}
			Subscribed::Resume { replay, live } => {
				let mut last = replay.first().map_or(0, |item| item.revision - 1);
				for item in &replay {
					last = item.revision;
					if !self.event(item).await {
						return;
					}
				}
				self.follow(live, last).await;
			}
		}
	}

	async fn resync(&self, current: u64, reason: &str) -> bool {
		let body = Body::Resync(SessionResync {
			current_revision: current,
			reason: reason.to_owned(),
		});
		self.send(current, body).await
	}

	async fn follow(&self, mut live: broadcast::Receiver<FeedItem>, mut last: u64) {
		loop {
			let received = tokio::select! {
				received = live.recv() => received,
				() = self.tx.closed() => return,
			};
			match received {
				Ok(item) => {
					if item.revision <= last {
						continue;
					}
					last = item.revision;
					if !self.event(&item).await {
						return;
					}
				}
				Err(broadcast::error::RecvError::Lagged(_)) => {
					let _ = self.resync(last, "subscriber lagged").await;
					return;
				}
				Err(broadcast::error::RecvError::Closed) => return,
			}
		}
	}
}
