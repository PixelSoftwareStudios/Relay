//! The mirrored messaging state and how daemon events change it. Pure data:
//! no I/O, no toolkit. Every operation is idempotent so a resync may replay
//! events that a transcript read already contains.

use std::collections::HashMap;

use relay_api::pb::{self, session_event::Event};

use super::convert;
use super::model::{Body, Delivery, EntryView, SessionKey, SessionView, SettingsView};

/// Entries kept per session in this app. Older ones stay in the daemon (and
/// in an export); the UI says so.
pub const MAX_LOCAL_ENTRIES: usize = 5_000;

/// Where the messaging feed stands.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Feed {
	/// The user has not opted in: no connection with the `messages` capability exists.
	#[default]
	Off,
	Connecting,
	Live,
	/// The daemon refused the `messages` capability.
	Denied,
	/// The daemon is not reachable or the connection broke; reconnecting.
	Offline(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
	pub entries: Vec<EntryView>,
	/// Older entries exist in the daemon but are not held here.
	pub truncated: bool,
	/// Bumped on every change, so view caches know when to rebuild.
	pub generation: u64,
}

impl Transcript {
	fn touch(&mut self) {
		self.generation += 1;
	}

	fn push(&mut self, entry: EntryView) -> bool {
		if self
			.entries
			.last()
			.is_some_and(|l| l.ordinal >= entry.ordinal)
		{
			return false;
		}
		self.entries.push(entry);
		if self.entries.len() > MAX_LOCAL_ENTRIES {
			let drop = self.entries.len() - MAX_LOCAL_ENTRIES;
			self.entries.drain(..drop);
			self.truncated = true;
		}
		self.touch();
		true
	}
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessagesState {
	pub feed: Feed,
	pub settings: Option<SettingsView>,
	pub sessions: Vec<SessionView>,
	pub transcripts: HashMap<SessionKey, Transcript>,
	/// Text for the user from a failed action; cleared by dismissing it.
	pub notice: Option<String>,
}

impl MessagesState {
	pub fn session(&self, key: &str) -> Option<&SessionView> {
		self.sessions.iter().find(|s| s.key == key)
	}

	/// The live session of a call, if the daemon has one.
	pub fn session_for_call(&self, call_id: &str) -> Option<&SessionView> {
		self.sessions
			.iter()
			.find(|s| s.is_live() && s.call_id == call_id)
	}

	pub fn live_sessions(&self) -> impl Iterator<Item = &SessionView> {
		self.sessions.iter().filter(|s| s.is_live())
	}

	pub fn transcript(&self, key: &str) -> Option<&Transcript> {
		self.transcripts.get(key)
	}

	/// Forget everything that came from the daemon (opt-out or disconnect).
	pub fn forget_all(&mut self) {
		self.settings = None;
		self.sessions.clear();
		self.transcripts.clear();
	}

	/// Replace the session list; transcripts of vanished sessions are dropped.
	pub fn load_list(
		&mut self,
		sessions: &[pb::SessionSummary],
		settings: Option<&pb::MessagingSettings>,
	) {
		self.sessions = sessions.iter().map(convert::session).collect();
		self.settings = settings.map(convert::settings);
		let keep: Vec<&str> = self.sessions.iter().map(|s| s.key.as_str()).collect();
		self.transcripts.retain(|k, _| keep.contains(&k.as_str()));
	}

	/// Replace one transcript with freshly read entries.
	pub fn set_transcript(&mut self, key: &str, entries: &[pb::TranscriptEntry]) {
		let mut views: Vec<EntryView> = entries.iter().filter_map(convert::entry).collect();
		views.sort_by_key(|e| e.ordinal);
		views.dedup_by_key(|e| e.ordinal);
		let truncated = views.len() > MAX_LOCAL_ENTRIES;
		if truncated {
			let drop = views.len() - MAX_LOCAL_ENTRIES;
			views.drain(..drop);
		}
		let generation = self.transcripts.get(key).map_or(0, |t| t.generation) + 1;
		self.transcripts.insert(
			key.to_owned(),
			Transcript {
				entries: views,
				truncated,
				generation,
			},
		);
	}

	fn upsert_session(&mut self, summary: &pb::SessionSummary) {
		let view = convert::session(summary);
		match self.sessions.iter_mut().find(|s| s.key == view.key) {
			Some(slot) => *slot = view,
			None => self.sessions.push(view),
		}
	}

	fn remove_session(&mut self, key: &str) {
		self.sessions.retain(|s| s.key != key);
		self.transcripts.remove(key);
	}

	/// Apply one live event.
	pub fn apply(&mut self, event: &pb::SessionEvent) {
		let Some(event) = &event.event else { return };
		match event {
			Event::Session(s) => self.upsert_session(s),
			Event::Entry(n) => {
				let key = hex::encode(&n.session_id);
				if self.session(&key).is_none() {
					return;
				}
				if let Some(entry) = n.entry.as_ref().and_then(convert::entry) {
					self.transcripts.entry(key).or_default().push(entry);
				}
			}
			Event::Delivery(d) => {
				let delivery = convert::delivery(d.delivery);
				self.with_entry(&d.session_id, |e| {
					if !(e.outgoing && e.seq == d.seq) {
						return false;
					}
					e.delivery = delivery;
					e.failure = if delivery == Delivery::Failed {
						d.failure.clone()
					} else {
						String::new()
					};
					true
				});
			}
			Event::Offer(o) => {
				let Some(offer) = o.offer.as_ref().map(convert::offer) else {
					return;
				};
				self.with_entry(&o.session_id, |e| {
					if e.ordinal != o.ordinal || !matches!(e.body, Body::File(_)) {
						return false;
					}
					e.body = Body::File(offer.clone());
					true
				});
			}
			Event::Permissions(p) => {
				let key = hex::encode(&p.session_id);
				if let (Some(s), Some(perms)) = (
					self.sessions.iter_mut().find(|s| s.key == key),
					&p.permissions,
				) {
					s.perms.allow_messages = perms.allow_messages;
					s.perms.allow_files = perms.allow_files;
				}
			}
			Event::Pin(p) => {
				let Some(m) = &p.message else { return };
				self.with_entry(&p.session_id, |e| {
					if e.outgoing != m.outgoing || e.seq != m.seq {
						return false;
					}
					e.pinned = p.pinned;
					true
				});
			}
			Event::Cleared(c) => {
				let key = hex::encode(&c.session_id);
				if c.removed {
					self.remove_session(&key);
				} else if let Some(t) = self.transcripts.get_mut(&key) {
					t.entries.clear();
					t.truncated = false;
					t.touch();
				}
			}
			Event::Evicted(e) => self.remove_session(&hex::encode(&e.session_id)),
		}
	}

	/// Run `f` on entries of a session, newest first, until it reports a change.
	fn with_entry(&mut self, session_id: &[u8], mut f: impl FnMut(&mut EntryView) -> bool) {
		let Some(t) = self.transcripts.get_mut(&hex::encode(session_id)) else {
			return;
		};
		if t.entries.iter_mut().rev().any(&mut f) {
			t.touch();
		}
	}
}
