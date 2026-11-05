//! In-memory session messaging with the semantics `docs/api.md` describes: transcripts with
//! ordinals that survive a clear, per-direction sequences, offers, pins, permissions, search,
//! export and an event log with revisions. Shared by the socket engine and `FakeControl`.

use relay_api::pb::session_event::Event;
use relay_api::pb::transcript_entry::Body;
use relay_api::pb::*;

pub const SESSION_A: [u8; 16] = [0x51; 16];
pub const SESSION_OLD: [u8; 16] = [0x52; 16];
pub const CALL_A: [u8; 16] = [0xc1; 16];
pub const CALL_OLD: [u8; 16] = [0xc2; 16];
pub const BASE_MS: u64 = 1_700_000_000_000;

pub struct ChatSession {
	pub summary: SessionSummary,
	pub entries: Vec<TranscriptEntry>,
	out_seq: u64,
	in_seq: u64,
}

pub struct ChatState {
	pub sessions: Vec<ChatSession>,
	pub revision: u64,
	pub events: Vec<(u64, SessionEvent)>,
	/// Events at or before this revision cannot be replayed (the engine answers a resync).
	pub replay_floor: u64,
	/// What the engine was asked to do, never message text.
	pub log: Vec<String>,
	/// The next mutation fails with this stable problem code.
	pub fail_with: Option<String>,
	/// Hand out `SessionResync` once after the next opened watch's first item.
	pub resync_next_watch: bool,
}

fn problem(code: &str) -> Operation {
	Operation {
		state: OperationState::Failed as i32,
		problem: Some(Problem {
			code: code.into(),
			message: format!("refused: {code}"),
			retryable: false,
			..Problem::default()
		}),
		..Operation::default()
	}
}

impl Default for ChatState {
	fn default() -> Self {
		Self::new()
	}
}

impl ChatState {
	/// One active session with Alice and one retained, ended one with Bob.
	pub fn new() -> Self {
		let mut state = Self {
			sessions: Vec::new(),
			revision: 10,
			events: Vec::new(),
			replay_floor: 0,
			log: Vec::new(),
			fail_with: None,
			resync_next_watch: false,
		};
		state.add_session(SESSION_A, CALL_A, "Alice", true);
		state
	}

	pub fn add_session(&mut self, id: [u8; 16], call: [u8; 16], peer: &str, active: bool) {
		self.sessions.push(ChatSession {
			summary: SessionSummary {
				session_id: id.to_vec(),
				call_id: call.to_vec(),
				peer_id: vec![id[0]; 32],
				peer_name: peer.into(),
				state: if active {
					SessionState::Active
				} else {
					SessionState::Ended
				} as i32,
				permissions: Some(SessionPermissions {
					allow_messages: true,
					allow_files: true,
				}),
				local_is_host: true,
				started_at_unix_ms: BASE_MS,
				next_ordinal: 1,
				limit_bytes: 64 * 1024 * 1024,
				..SessionSummary::default()
			},
			entries: Vec::new(),
			out_seq: 0,
			in_seq: 0,
		});
	}

	pub fn push_event(&mut self, event: Event) {
		self.revision += 1;
		self.events
			.push((self.revision, SessionEvent { event: Some(event) }));
	}

	fn index(&self, id: &[u8]) -> Option<usize> {
		self.sessions
			.iter()
			.position(|s| s.summary.session_id == id)
	}

	pub fn list(&self) -> ListSessionsResponse {
		ListSessionsResponse {
			sessions: self.sessions.iter().map(|s| s.summary.clone()).collect(),
			revision: self.revision,
			settings: Some(MessagingSettings {
				enabled: true,
				max_offer_size_mib: 2048,
				transfer_rate_limit_mbps: 16,
				transfer_port: 47_801,
			}),
		}
	}

	pub fn transcript(&self, id: &[u8], from: u64, limit: u32) -> Option<Transcript> {
		let s = &self.sessions[self.index(id)?];
		let limit = if limit == 0 { 2 } else { limit as usize };
		let mut rest = s.entries.iter().filter(|e| e.ordinal >= from);
		let entries: Vec<_> = rest.by_ref().take(limit).cloned().collect();
		let next = entries.last().map_or(from, |e| e.ordinal + 1);
		Some(Transcript {
			session: Some(s.summary.clone()),
			entries,
			next_ordinal: next,
			more: rest.next().is_some(),
			revision: self.revision,
		})
	}

	fn add_entry(&mut self, i: usize, outgoing: bool, body: Body) -> TranscriptEntry {
		let s = &mut self.sessions[i];
		let seq = if outgoing {
			s.out_seq += 1;
			s.out_seq
		} else {
			s.in_seq += 1;
			s.in_seq
		};
		let ordinal = s.summary.next_ordinal;
		s.summary.next_ordinal += 1;
		s.summary.message_count += 1;
		let entry = TranscriptEntry {
			ordinal,
			outgoing,
			seq,
			sent_at_unix_ms: BASE_MS + ordinal * 1000,
			received_at_unix_ms: BASE_MS + ordinal * 1000,
			delivery: if outgoing {
				DeliveryState::Sending
			} else {
				DeliveryState::Delivered
			} as i32,
			body: Some(body),
			..TranscriptEntry::default()
		};
		s.entries.push(entry.clone());
		let session_id = s.summary.session_id.clone();
		self.push_event(Event::Entry(NewEntry {
			session_id,
			entry: Some(entry.clone()),
		}));
		entry
	}

	/// The peer sends text.
	pub fn incoming_text(&mut self, id: &[u8], text: &str) -> TranscriptEntry {
		let i = self.index(id).expect("session");
		self.add_entry(i, false, Body::Text(text.into()))
	}

	/// The peer offers a file; returns the offer id.
	pub fn incoming_offer(&mut self, id: &[u8], name: &str, size: u64) -> Vec<u8> {
		let i = self.index(id).expect("session");
		let offer_id = vec![0xe0 + self.sessions[i].entries.len() as u8; 16];
		self.add_entry(
			i,
			false,
			Body::File(FileOfferBody {
				offer_id: offer_id.clone(),
				name: name.into(),
				size,
				state: OfferState::Offered as i32,
				..FileOfferBody::default()
			}),
		);
		offer_id
	}

	/// Changes an offer and emits the event, as the transfer machinery would.
	pub fn update_offer(&mut self, id: &[u8], offer_id: &[u8], f: impl FnOnce(&mut FileOfferBody)) {
		let i = self.index(id).expect("session");
		let entry = self.sessions[i]
			.entries
			.iter_mut()
			.find(|e| matches!(&e.body, Some(Body::File(o)) if o.offer_id == offer_id))
			.expect("offer");
		let Some(Body::File(o)) = &mut entry.body else {
			unreachable!()
		};
		f(o);
		let (ordinal, offer) = (entry.ordinal, o.clone());
		self.push_event(Event::Offer(OfferChanged {
			session_id: id.to_vec(),
			ordinal,
			offer: Some(offer),
		}));
	}

	fn result(
		&mut self,
		id: &[u8],
		what: String,
		f: impl FnOnce(&mut Self, usize) -> Extra,
	) -> SessionOpResult {
		self.log.push(what);
		let operation = if let Some(code) = self.fail_with.take() {
			problem(&code)
		} else if self.index(id).is_none() {
			problem("SESSION_NOT_FOUND")
		} else {
			let i = self.index(id).unwrap();
			let extra = f(self, i);
			return SessionOpResult {
				operation: Some(Operation {
					state: OperationState::Applied as i32,
					revision: self.revision,
					..Operation::default()
				}),
				session_id: id.to_vec(),
				message: extra.0,
				offer_id: extra.1,
			};
		};
		SessionOpResult {
			operation: Some(operation),
			session_id: id.to_vec(),
			..SessionOpResult::default()
		}
	}

	pub fn send_text(&mut self, id: &[u8], text: &str) -> SessionOpResult {
		let what = format!("send_text bytes={}", text.len());
		self.result(id, what, |s, i| {
			let e = s.add_entry(i, true, Body::Text(text.into()));
			(
				Some(MessageRef {
					ordinal: e.ordinal,
					outgoing: true,
					seq: e.seq,
				}),
				Vec::new(),
			)
		})
	}

	pub fn send_image(&mut self, id: &[u8], image: Vec<u8>) -> SessionOpResult {
		let what = format!("send_image bytes={}", image.len());
		self.result(id, what, |s, i| {
			let body = Body::Image(ImageBody {
				media_type: "image/png".into(),
				width: 1,
				height: 1,
				frames: 1,
				data: image,
			});
			let e = s.add_entry(i, true, body);
			(
				Some(MessageRef {
					ordinal: e.ordinal,
					outgoing: true,
					seq: e.seq,
				}),
				Vec::new(),
			)
		})
	}

	pub fn offer_file(&mut self, id: &[u8], path: &str) -> SessionOpResult {
		self.result(id, format!("offer_file {path}"), |s, i| {
			let offer_id = vec![0xf0 + s.sessions[i].entries.len() as u8; 16];
			let name = path.rsplit('/').next().unwrap_or(path).to_owned();
			let body = Body::File(FileOfferBody {
				offer_id: offer_id.clone(),
				name,
				size: 1000,
				state: OfferState::Offered as i32,
				outgoing: true,
				..FileOfferBody::default()
			});
			let e = s.add_entry(i, true, body);
			(
				Some(MessageRef {
					ordinal: e.ordinal,
					outgoing: true,
					seq: e.seq,
				}),
				offer_id,
			)
		})
	}

	pub fn respond(
		&mut self,
		id: &[u8],
		offer: &[u8],
		accept: bool,
		dir: &str,
		name: &str,
	) -> SessionOpResult {
		self.result(
			id,
			format!("respond accept={accept} dest={dir} name={name}"),
			|s, _| {
				s.update_offer(id, offer, |o| {
					o.state = if accept {
						OfferState::InProgress
					} else {
						OfferState::Rejected
					} as i32;
				});
				(None, offer.to_vec())
			},
		)
	}

	pub fn preview(&mut self, id: &[u8], offer: &[u8], what: &str) -> SessionOpResult {
		self.result(id, format!("preview {what}"), |s, _| {
			s.update_offer(id, offer, |o| o.preview_requested = true);
			(None, offer.to_vec())
		})
	}

	pub fn cancel(&mut self, id: &[u8], offer: &[u8]) -> SessionOpResult {
		self.result(id, "cancel".into(), |s, _| {
			s.update_offer(id, offer, |o| o.state = OfferState::Cancelled as i32);
			(None, offer.to_vec())
		})
	}

	pub fn permissions(
		&mut self,
		id: &[u8],
		messages: Option<bool>,
		files: Option<bool>,
	) -> SessionOpResult {
		self.result(id, format!("permissions {messages:?} {files:?}"), |s, i| {
			let p = s.sessions[i]
				.summary
				.permissions
				.get_or_insert_with(Default::default);
			p.allow_messages = messages.unwrap_or(p.allow_messages);
			p.allow_files = files.unwrap_or(p.allow_files);
			let permissions = Some(*p);
			s.push_event(Event::Permissions(PermissionsChanged {
				session_id: id.to_vec(),
				permissions,
				by_peer: false,
			}));
			(None, Vec::new())
		})
	}

	pub fn pin(&mut self, id: &[u8], outgoing: bool, seq: u64, pinned: bool) -> SessionOpResult {
		self.result(
			id,
			format!("pin outgoing={outgoing} seq={seq} {pinned}"),
			|s, i| {
				for e in &mut s.sessions[i].entries {
					if e.outgoing == outgoing && e.seq == seq {
						e.pinned = pinned;
					}
				}
				s.push_event(Event::Pin(PinChanged {
					session_id: id.to_vec(),
					message: Some(MessageRef {
						ordinal: 0,
						outgoing,
						seq,
					}),
					pinned,
				}));
				(None, Vec::new())
			},
		)
	}

	pub fn clear(&mut self, id: &[u8]) -> SessionOpResult {
		self.result(id, "clear".into(), |s, i| {
			let active = s.sessions[i].summary.state == SessionState::Active as i32;
			let next = s.sessions[i].summary.next_ordinal;
			if active {
				s.sessions[i].entries.clear();
				s.sessions[i].summary.message_count = 0;
			} else {
				s.sessions.remove(i);
			}
			s.push_event(Event::Cleared(SessionCleared {
				session_id: id.to_vec(),
				removed: !active,
				next_ordinal: next,
			}));
			(None, Vec::new())
		})
	}

	pub fn search(&mut self, id: &[u8], query: &str) -> Option<SearchSessionResponse> {
		self.log.push("search".into());
		let s = &self.sessions[self.index(id)?];
		let q = query.to_lowercase();
		let hits = s
			.entries
			.iter()
			.filter(|e| match &e.body {
				Some(Body::Text(t)) => t.to_lowercase().contains(&q),
				Some(Body::File(o)) => o.name.to_lowercase().contains(&q),
				_ => false,
			})
			.map(|e| MessageRef {
				ordinal: e.ordinal,
				outgoing: e.outgoing,
				seq: e.seq,
			})
			.collect();
		Some(SearchSessionResponse {
			hits,
			truncated: false,
		})
	}

	pub fn export(&mut self, id: &[u8], dest: &str) -> Option<ExportSessionResponse> {
		self.log.push(format!("export dest={dest}"));
		let s = &self.sessions[self.index(id)?];
		let messages: Vec<_> = s
			.entries
			.iter()
			.map(|e| match &e.body {
				Some(Body::Text(t)) => serde_json::json!({"ordinal": e.ordinal, "text": t}),
				_ => serde_json::json!({"ordinal": e.ordinal}),
			})
			.collect();
		let json = serde_json::to_vec(&serde_json::json!({"messages": messages})).unwrap();
		let ok = Operation {
			state: OperationState::Applied as i32,
			..Operation::default()
		};
		if dest.is_empty() {
			return Some(ExportSessionResponse {
				operation: Some(ok),
				json,
				written_path: String::new(),
			});
		}
		if std::path::Path::new(dest).exists() {
			return Some(ExportSessionResponse {
				operation: Some(problem("EXPORT_WRITE_FAILED")),
				..Default::default()
			});
		}
		std::fs::write(dest, &json).unwrap();
		Some(ExportSessionResponse {
			operation: Some(ok),
			json: Vec::new(),
			written_path: dest.into(),
		})
	}
}

type Extra = (Option<MessageRef>, Vec<u8>);

fn event_session(event: &SessionEvent) -> Vec<u8> {
	match event.event.as_ref() {
		Some(Event::Entry(e)) => e.session_id.clone(),
		Some(Event::Delivery(e)) => e.session_id.clone(),
		Some(Event::Offer(e)) => e.session_id.clone(),
		Some(Event::Permissions(e)) => e.session_id.clone(),
		Some(Event::Pin(e)) => e.session_id.clone(),
		Some(Event::Session(e)) => e.session_id.clone(),
		Some(Event::Cleared(e)) => e.session_id.clone(),
		Some(Event::Evicted(e)) => e.session_id.clone(),
		None => Vec::new(),
	}
}

/// The `WatchSession` stream: a list or a replay, then new events as they appear.
pub async fn serve_watch(
	chat: std::sync::Arc<std::sync::Mutex<ChatState>>,
	incarnation: Vec<u8>,
	after: Option<u64>,
	only: Vec<u8>,
	tx: tokio::sync::mpsc::Sender<Result<SessionEnvelope, tonic::Status>>,
) {
	use relay_api::pb::session_envelope::Body;
	let envelope = |revision, body| {
		Ok(SessionEnvelope {
			engine_incarnation: incarnation.clone(),
			revision,
			body: Some(body),
		})
	};
	let mut cursor = 0;
	let (resync, first) = {
		let mut c = chat.lock().unwrap();
		let resync = std::mem::take(&mut c.resync_next_watch);
		match after {
			None => {
				cursor = c.revision;
				let mut list = c.list();
				list.sessions
					.retain(|s| only.is_empty() || s.session_id == only);
				let body = Body::List(SessionList {
					sessions: list.sessions,
					settings: list.settings,
				});
				(resync, Some(envelope(cursor, body)))
			}
			Some(rev) if rev < c.replay_floor => {
				let body = Body::Resync(SessionResync {
					current_revision: c.revision,
					reason: "window".into(),
				});
				(true, Some(envelope(c.revision, body)))
			}
			Some(rev) => {
				cursor = rev;
				(resync, None)
			}
		}
	};
	if let Some(first) = first
		&& tx.send(first).await.is_err()
	{
		return;
	}
	if resync {
		let body = Body::Resync(SessionResync {
			current_revision: 0,
			reason: "lagged".into(),
		});
		let _ = tx.send(envelope(0, body)).await;
		return;
	}
	loop {
		let due: Vec<(u64, SessionEvent)> = {
			let c = chat.lock().unwrap();
			c.events
				.iter()
				.filter(|(r, e)| *r > cursor && (only.is_empty() || event_session(e) == only))
				.cloned()
				.collect()
		};
		for (rev, event) in due {
			cursor = rev;
			if tx.send(envelope(rev, Body::Event(event))).await.is_err() {
				return;
			}
		}
		if tx.is_closed() {
			return;
		}
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
	}
}
