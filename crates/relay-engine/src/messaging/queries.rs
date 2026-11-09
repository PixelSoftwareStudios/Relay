//! Reads: session list, transcript pages, search and subscriptions.

use relay_api::pb;
use relay_messages::SessionId;
use tokio::sync::broadcast;

use super::feed::FeedItem;
use super::hub::Hub;
use super::problem::Refusal;
use super::view;

/// Entries per transcript page unless the client asks for fewer.
const DEFAULT_PAGE: usize = 200;
/// Hard cap on entries per page.
const MAX_PAGE: usize = 500;
/// Approximate bytes per page (the message limit is 1 MiB).
const PAGE_BYTES: usize = 512 * 1024;

/// How a watcher starts.
pub enum Subscribed {
	/// The current sessions, then live events.
	Fresh {
		/// Sessions and settings.
		list: pb::SessionList,
		/// Revision of the list.
		revision: u64,
		/// Live events after it.
		live: broadcast::Receiver<FeedItem>,
	},
	/// Missed events, then live events.
	Resume {
		/// Replay, oldest first.
		replay: Vec<FeedItem>,
		/// Live events after the replay.
		live: broadcast::Receiver<FeedItem>,
	},
	/// The window no longer covers the request.
	Resync {
		/// Current revision.
		current: u64,
	},
}

impl Hub {
	/// The effective settings.
	pub fn settings_pb(&self) -> pb::MessagingSettings {
		let cfg = self.cfg();
		pb::MessagingSettings {
			enabled: cfg.enabled,
			max_offer_size_mib: cfg.max_offer_size_mib,
			transfer_rate_limit_mbps: cfg.transfer_rate_limit_mbps,
			transfer_port: self
				.env
				.transfers
				.as_ref()
				.map_or(0, |hub| u32::from(hub.local_addr().port())),
		}
	}

	fn summaries(&self) -> Vec<pb::SessionSummary> {
		let mut all: Vec<_> = self
			.sessions
			.keys()
			.filter_map(|sid| self.summary(*sid))
			.collect();
		all.sort_by(|a, b| {
			a.started_at_unix_ms
				.cmp(&b.started_at_unix_ms)
				.then_with(|| a.session_id.cmp(&b.session_id))
		});
		all
	}

	/// Every session, live and retained.
	pub fn list(&self) -> pb::ListSessionsResponse {
		pb::ListSessionsResponse {
			sessions: self.summaries(),
			revision: self.feed.revision(),
			settings: Some(self.settings_pb()),
		}
	}

	/// One page of a transcript from `from_ordinal`.
	pub fn transcript(
		&self,
		session: &[u8],
		from: u64,
		limit: u32,
	) -> Result<pb::Transcript, Refusal> {
		let sid = parse(session)?;
		let (meta, store) = self
			.sessions
			.get(&sid)
			.zip(self.history.get(sid))
			.ok_or_else(|| Refusal::new("SESSION_NOT_FOUND", "unknown session"))?;
		let limit = if limit == 0 {
			DEFAULT_PAGE
		} else {
			(limit as usize).min(MAX_PAGE)
		};
		let start = usize::try_from(from.saturating_sub(meta.base + 1)).unwrap_or(usize::MAX);
		let messages = store.messages();
		let mut entries = Vec::new();
		let mut bytes = 0usize;
		let mut index = start.min(messages.len());
		while let Some(message) = messages.get(index) {
			let cost = usize::try_from(message.body.payload_bytes()).unwrap_or(usize::MAX) + 256;
			if !entries.is_empty() && (entries.len() >= limit || bytes + cost > PAGE_BYTES) {
				break;
			}
			bytes += cost;
			entries.push(view::entry(view::ordinal(meta, index), message, meta));
			index += 1;
		}
		let next = view::ordinal(meta, index);
		Ok(pb::Transcript {
			session: Some(view::summary(meta, store)),
			entries,
			next_ordinal: next,
			more: index < messages.len(),
			revision: self.feed.revision(),
		})
	}

	/// Case-insensitive substring search.
	pub fn search(
		&self,
		session: &[u8],
		query: &str,
	) -> Result<pb::SearchSessionResponse, Refusal> {
		let sid = parse(session)?;
		let (meta, store) = self
			.sessions
			.get(&sid)
			.zip(self.history.get(sid))
			.ok_or_else(|| Refusal::new("SESSION_NOT_FOUND", "unknown session"))?;
		let found = store.search(query)?;
		let hits = found
			.hits
			.into_iter()
			.filter_map(|id| {
				let index = view::index_of(store, id)?;
				Some(pb::MessageRef {
					ordinal: view::ordinal(meta, index),
					outgoing: id.origin == relay_messages::Origin::Local,
					seq: id.seq,
				})
			})
			.collect();
		Ok(pb::SearchSessionResponse {
			hits,
			truncated: found.truncated,
		})
	}

	/// Opens a watch positioned atomically against the event sequence.
	pub fn subscribe(&self, after: Option<u64>) -> Subscribed {
		let Some(after) = after else {
			return Subscribed::Fresh {
				list: pb::SessionList {
					sessions: self.summaries(),
					settings: Some(self.settings_pb()),
				},
				revision: self.feed.revision(),
				live: self.feed.subscribe(),
			};
		};
		match self.feed.replay_after(after) {
			Some(replay) => Subscribed::Resume {
				replay,
				live: self.feed.subscribe(),
			},
			None => Subscribed::Resync {
				current: self.feed.revision(),
			},
		}
	}
}

/// A session id from request bytes.
pub fn parse(raw: &[u8]) -> Result<SessionId, Refusal> {
	<[u8; 16]>::try_from(raw)
		.map(SessionId)
		.map_err(|_| Refusal::new("SESSION_NOT_FOUND", "unknown session"))
}
