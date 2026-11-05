//! In-memory session messaging for the fake engine: capability checks, transcripts with
//! ordinals, a revisioned event feed with replay/resync, and a few mutations.

use std::collections::HashMap;
use std::sync::Mutex;

use relay_api::CLIENT_ID_HEADER;
use relay_api::pb::session_envelope::Body;
use relay_api::pb::*;
use tokio::sync::broadcast;
use tonic::{Request, Status};

pub const PAGE: usize = 3;
const HISTORY: usize = 8;

#[derive(Default)]
struct Session {
	entries: Vec<TranscriptEntry>,
	next_ordinal: u64,
	next_seq: u64,
	permissions: SessionPermissions,
}

#[derive(Default)]
struct Inner {
	revision: u64,
	history: Vec<(u64, SessionEvent)>,
	sessions: HashMap<Vec<u8>, Session>,
}

pub struct Messages {
	caps: Mutex<HashMap<Vec<u8>, Vec<String>>>,
	inner: Mutex<Inner>,
	tx: broadcast::Sender<(u64, SessionEvent)>,
}

impl Default for Messages {
	fn default() -> Self {
		Self {
			caps: Mutex::default(),
			inner: Mutex::default(),
			tx: broadcast::channel(4).0,
		}
	}
}

fn summary(id: &[u8], s: &Session) -> SessionSummary {
	SessionSummary {
		session_id: id.to_vec(),
		state: SessionState::Active as i32,
		permissions: Some(s.permissions),
		message_count: s.entries.len() as u64,
		next_ordinal: s.next_ordinal,
		..SessionSummary::default()
	}
}

impl Messages {
	pub fn grant(&self, client: Vec<u8>, caps: Vec<String>) {
		self.caps.lock().unwrap().insert(client, caps);
	}

	/// The `messages` capability, checked from the transport header.
	pub fn require<T>(&self, request: &Request<T>) -> Result<(), Status> {
		let id = request
			.metadata()
			.get(CLIENT_ID_HEADER)
			.and_then(|v| v.to_str().ok())
			.and_then(hex_decode)
			.ok_or_else(|| Status::unauthenticated("no client"))?;
		let caps = self.caps.lock().unwrap();
		if caps
			.get(&id)
			.is_some_and(|c| c.iter().any(|c| c == "messages"))
		{
			Ok(())
		} else {
			Err(Status::permission_denied("messages"))
		}
	}

	pub fn add_session(&self, id: &[u8]) {
		let mut inner = self.inner.lock().unwrap();
		let session = Session {
			next_ordinal: 1,
			next_seq: 1,
			permissions: SessionPermissions {
				allow_messages: true,
				allow_files: true,
			},
			..Session::default()
		};
		let event = SessionEvent {
			event: Some(session_event::Event::Session(summary(id, &session))),
		};
		inner.sessions.insert(id.to_vec(), session);
		self.publish(&mut inner, event);
	}

	fn publish(&self, inner: &mut Inner, event: SessionEvent) -> u64 {
		inner.revision += 1;
		inner.history.push((inner.revision, event.clone()));
		if inner.history.len() > HISTORY {
			inner.history.remove(0);
		}
		let _ = self.tx.send((inner.revision, event));
		inner.revision
	}

	/// An incoming message from the "peer".
	pub fn deliver(&self, id: &[u8], text: &str) {
		self.push_entry(id, text, false);
	}

	fn push_entry(&self, id: &[u8], text: &str, outgoing: bool) -> Option<(u64, u64)> {
		let mut inner = self.inner.lock().unwrap();
		let s = inner.sessions.get_mut(id)?;
		let (ordinal, seq) = (s.next_ordinal, s.next_seq);
		s.next_ordinal += 1;
		if outgoing {
			s.next_seq += 1;
		}
		let entry = TranscriptEntry {
			ordinal,
			outgoing,
			seq,
			delivery: DeliveryState::Delivered as i32,
			body: Some(transcript_entry::Body::Text(text.to_owned())),
			..TranscriptEntry::default()
		};
		s.entries.push(entry.clone());
		let event = SessionEvent {
			event: Some(session_event::Event::Entry(NewEntry {
				session_id: id.to_vec(),
				entry: Some(entry),
			})),
		};
		self.publish(&mut inner, event);
		Some((ordinal, seq))
	}

	pub fn send(&self, req: &SendSessionMessageRequest) -> SessionOpResult {
		let op = |state: OperationState, code: &str| Operation {
			operation_id: req
				.context
				.as_ref()
				.map(|c| c.operation_id.clone())
				.unwrap_or_default(),
			state: state as i32,
			problem: (!code.is_empty()).then(|| Problem {
				code: code.to_owned(),
				..Problem::default()
			}),
			..Operation::default()
		};
		match self.push_entry(&req.session_id, &req.text, true) {
			Some((ordinal, seq)) => SessionOpResult {
				operation: Some(op(OperationState::Applied, "")),
				session_id: req.session_id.clone(),
				message: Some(MessageRef {
					ordinal,
					outgoing: true,
					seq,
				}),
				offer_id: Vec::new(),
			},
			None => SessionOpResult {
				operation: Some(op(OperationState::Failed, "SESSION_NOT_FOUND")),
				..SessionOpResult::default()
			},
		}
	}

	pub fn list(&self) -> ListSessionsResponse {
		let inner = self.inner.lock().unwrap();
		ListSessionsResponse {
			sessions: inner.sessions.iter().map(|(k, v)| summary(k, v)).collect(),
			revision: inner.revision,
			settings: None,
		}
	}

	pub fn transcript(&self, req: &GetTranscriptRequest) -> Result<Transcript, Status> {
		let inner = self.inner.lock().unwrap();
		let s = inner
			.sessions
			.get(&req.session_id)
			.ok_or_else(|| Status::not_found("SESSION_NOT_FOUND"))?;
		let rest: Vec<_> = s
			.entries
			.iter()
			.filter(|e| e.ordinal >= req.from_ordinal)
			.cloned()
			.collect();
		let entries: Vec<_> = rest.iter().take(PAGE).cloned().collect();
		Ok(Transcript {
			session: Some(summary(&req.session_id, s)),
			next_ordinal: entries.last().map_or(req.from_ordinal, |e| e.ordinal + 1),
			more: rest.len() > PAGE,
			entries,
			revision: inner.revision,
		})
	}

	pub fn search(&self, req: &SearchSessionRequest) -> SearchSessionResponse {
		let inner = self.inner.lock().unwrap();
		let needle = req.query.to_lowercase();
		let hits = inner
			.sessions
			.get(&req.session_id)
			.map(|s| {
				s.entries
					.iter()
					.filter(|e| {
						matches!(&e.body, Some(transcript_entry::Body::Text(t)) if t.to_lowercase().contains(&needle))
					})
					.map(|e| MessageRef {
						ordinal: e.ordinal,
						outgoing: e.outgoing,
						seq: e.seq,
					})
					.collect()
			})
			.unwrap_or_default();
		SearchSessionResponse {
			hits,
			truncated: false,
		}
	}

	/// Initial items of a watch and a live receiver, taken under one lock.
	pub fn watch(
		&self,
		after: Option<u64>,
		incarnation: &[u8],
	) -> (
		Vec<SessionEnvelope>,
		broadcast::Receiver<(u64, SessionEvent)>,
	) {
		let inner = self.inner.lock().unwrap();
		let live = self.tx.subscribe();
		let envelope = |revision, body| SessionEnvelope {
			engine_incarnation: incarnation.to_vec(),
			revision,
			body: Some(body),
		};
		let initial = match after {
			None => vec![envelope(
				inner.revision,
				Body::List(SessionList {
					sessions: inner.sessions.iter().map(|(k, v)| summary(k, v)).collect(),
					settings: None,
				}),
			)],
			Some(r)
				if r <= inner.revision && inner.history.first().is_none_or(|h| h.0 <= r + 1) =>
			{
				inner
					.history
					.iter()
					.filter(|(rev, _)| *rev > r)
					.map(|(rev, e)| envelope(*rev, Body::Event(e.clone())))
					.collect()
			}
			Some(_) => vec![envelope(
				inner.revision,
				Body::Resync(SessionResync {
					current_revision: inner.revision,
					reason: "replay window exceeded".into(),
				}),
			)],
		};
		(initial, live)
	}
}

fn hex_decode(text: &str) -> Option<Vec<u8>> {
	(0..text.len())
		.step_by(2)
		.map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
		.collect()
}
