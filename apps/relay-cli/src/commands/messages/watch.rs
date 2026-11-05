//! `relay messages watch`: follow events as lines, or as JSON lines.
//!
//! One revision sequence covers every session. When the stream cannot continue
//! (the engine answers `SessionResync`, a gap, an overflow, a new incarnation) the
//! watch is reopened fresh and each session's transcript is read again from the
//! last ordinal that was printed, so nothing is shown twice and nothing is lost.

use std::collections::HashMap;
use std::time::Duration;

use relay_api::pb::session_event::Event;
use relay_api::pb::{SessionEvent, SessionState, SessionSummary, TranscriptEntry};
use relay_api::{ApiError, SessionItem};
use serde_json::{Value, json};

use super::read::views;
use super::target::{pick_session, read_all};
use super::view::{EntryView, OfferLine, OfferView, SessionView, delivery_name, plain};
use crate::cli::CliError;
use crate::client::{Feed, Messaging, needs_resync};
use crate::commands::{Out, names::hex_id};

/// Reopen attempts in a row without a single event before the watch gives up.
const MAX_IDLE_RESYNCS: u32 = 5;
const REOPEN_DELAY: Duration = Duration::from_millis(100);

/// What has been printed of one session.
struct Cursor {
	next_ordinal: u64,
	blocks: usize,
}

struct Watcher<'a, 'w> {
	out: &'a mut Out<'w>,
	cursors: HashMap<Vec<u8>, Cursor>,
	peers: HashMap<Vec<u8>, String>,
	active: HashMap<Vec<u8>, bool>,
	first_list: bool,
}

/// `relay messages watch`
pub async fn run<M: Messaging>(
	client: &mut M,
	call: Option<&str>,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let only = match call {
		Some(query) => {
			let list = client.list_sessions().await?;
			Some(
				pick_session(&list.sessions, Some(query))?
					.session_id
					.clone(),
			)
		}
		None => None,
	};
	let mut watcher = Watcher {
		out,
		cursors: HashMap::new(),
		peers: HashMap::new(),
		active: HashMap::new(),
		first_list: true,
	};
	tokio::select! {
		result = follow(client, only.as_deref(), &mut watcher) => result,
		_ = tokio::signal::ctrl_c() => Ok(()),
	}
}

async fn follow<M: Messaging>(
	client: &mut M,
	only: Option<&[u8]>,
	w: &mut Watcher<'_, '_>,
) -> Result<(), CliError> {
	let mut idle = 0;
	loop {
		let mut watch = client.open_session_watch(only, None).await?;
		let reason = loop {
			match watch.next().await {
				Ok(Some(SessionItem::List(list))) => w.list(client, &list.sessions).await?,
				Ok(Some(SessionItem::Event { event, .. })) => {
					idle = 0;
					w.event(*event)?;
				}
				Ok(Some(SessionItem::Resync(r))) => break r.reason,
				Ok(None) => return Err(CliError::Api(ApiError::StreamClosed)),
				Err(err) if needs_resync(&err) => break err.to_string(),
				Err(err) => return Err(err.into()),
			}
		};
		idle += 1;
		if idle > MAX_IDLE_RESYNCS {
			return Err(CliError::Api(ApiError::SubscriptionOverflow));
		}
		w.say(
			format!("-- resync: {}", plain(&reason, false)),
			json!({"event": "resync", "reason": reason}),
		)?;
		tokio::time::sleep(REOPEN_DELAY).await;
	}
}

impl Watcher<'_, '_> {
	fn say(&mut self, text: String, json: Value) -> Result<(), CliError> {
		self.out.emit_with(&json, || text)
	}

	fn tagged(&mut self, session: &[u8], line: &str, json: Value) -> Result<(), CliError> {
		let peer = self.peers.get(session).map_or("?", String::as_str);
		let text = format!("[{peer}] {line}");
		self.say(text, json)
	}

	/// Print the entries that are new for the session and move its cursor.
	fn show(&mut self, session: &[u8], entries: &[TranscriptEntry]) -> Result<(), CliError> {
		let (next, mut blocks) = self
			.cursors
			.get(session)
			.map_or((0, 0), |c| (c.next_ordinal, c.blocks));
		let mut last = None;
		for entry in entries.iter().filter(|e| e.ordinal >= next) {
			let view = EntryView::new(entry, blocks, self.out.zone());
			blocks += view.block_count();
			last = Some(entry.ordinal);
			let json = json!({
				"event": "entry",
				"session": hex_id(session),
				"peer": self.peers.get(session),
				"entry": view,
			});
			self.tagged(session, &view.to_string(), json)?;
		}
		if let Some(last) = last {
			self.cursors.insert(
				session.to_vec(),
				Cursor {
					next_ordinal: last + 1,
					blocks,
				},
			);
		}
		Ok(())
	}

	async fn list<M: Messaging>(
		&mut self,
		client: &mut M,
		sessions: &[SessionSummary],
	) -> Result<(), CliError> {
		if std::mem::take(&mut self.first_list) {
			let views: Vec<SessionView> = sessions.iter().map(SessionView::new).collect();
			let line = if views.is_empty() {
				"watching: no sessions yet".to_owned()
			} else {
				let names: Vec<String> = views
					.iter()
					.map(|v| format!("{} ({})", v.peer, v.state))
					.collect();
				format!("watching: {}", names.join(", "))
			};
			self.say(line, json!({"event": "sessions", "sessions": views}))?;
		}
		for s in sessions {
			let id = &s.session_id;
			self.peers.insert(id.clone(), plain(&s.peer_name, false));
			self.active
				.insert(id.clone(), s.state == SessionState::Active as i32);
			let entries = read_all(client, id, 0).await?;
			if self.cursors.contains_key(id) {
				self.show(id, &entries)?;
			} else {
				// First sight: history is `log`'s job; remember where it ends.
				self.cursors.insert(
					id.clone(),
					Cursor {
						next_ordinal: entries.last().map_or(s.next_ordinal, |e| e.ordinal + 1),
						blocks: views(&entries, self.out.zone())
							.iter()
							.map(EntryView::block_count)
							.sum(),
					},
				);
			}
		}
		Ok(())
	}

	fn event(&mut self, event: SessionEvent) -> Result<(), CliError> {
		let Some(event) = event.event else {
			return Ok(());
		};
		match event {
			Event::Entry(n) => {
				let entry = n.entry.unwrap_or_default();
				self.show(&n.session_id, &[entry])
			}
			Event::Delivery(d) => {
				let state = delivery_name(d.delivery);
				let reason = if d.failure.is_empty() {
					String::new()
				} else {
					format!(" ({})", plain(&d.failure, false))
				};
				let json = json!({"event": "delivery", "session": hex_id(&d.session_id),
					"seq": d.seq, "delivery": state, "failure": d.failure});
				let line = format!("sent seq {} is {state}{reason}", d.seq);
				self.tagged(&d.session_id, &line, json)
			}
			Event::Offer(o) => {
				let offer = OfferView::new(&o.offer.unwrap_or_default());
				let line = OfferLine(&offer).to_string();
				let json = json!({"event": "offer", "session": hex_id(&o.session_id),
					"message": o.ordinal, "offer": offer});
				self.tagged(&o.session_id, &line, json)
			}
			Event::Permissions(p) => {
				let perms = p.permissions.unwrap_or_default();
				let on = |v| if v { "on" } else { "off" };
				let line = format!(
					"permissions: messages {}, files {}{}",
					on(perms.allow_messages),
					on(perms.allow_files),
					if p.by_peer { " (set by the peer)" } else { "" }
				);
				let json = json!({"event": "permissions", "session": hex_id(&p.session_id),
					"allow_messages": perms.allow_messages, "allow_files": perms.allow_files,
					"by_peer": p.by_peer});
				self.tagged(&p.session_id, &line, json)
			}
			Event::Pin(p) => {
				let message = p.message.unwrap_or_default().ordinal;
				let line = format!(
					"#{message} {}",
					if p.pinned { "pinned" } else { "unpinned" }
				);
				let json = json!({"event": "pin", "session": hex_id(&p.session_id),
					"message": message, "pinned": p.pinned});
				self.tagged(&p.session_id, &line, json)
			}
			Event::Session(s) => {
				self.peers
					.insert(s.session_id.clone(), plain(&s.peer_name, false));
				let active = s.state == SessionState::Active as i32;
				let known = self.active.insert(s.session_id.clone(), active);
				if known.is_none() {
					self.cursors.entry(s.session_id.clone()).or_insert(Cursor {
						next_ordinal: s.next_ordinal,
						blocks: 0,
					});
				}
				if known != Some(active) {
					let view = SessionView::new(&s);
					let line = format!("session {}", view.state);
					let json = json!({"event": "session", "session": view});
					self.tagged(&s.session_id, &line, json)?;
				}
				Ok(())
			}
			Event::Cleared(c) => {
				if c.removed {
					self.cursors.remove(&c.session_id);
				} else {
					self.cursors.insert(
						c.session_id.clone(),
						Cursor {
							next_ordinal: c.next_ordinal,
							blocks: 0,
						},
					);
				}
				let line = if c.removed {
					"transcript removed"
				} else {
					"transcript cleared"
				};
				let json = json!({"event": "cleared", "session": hex_id(&c.session_id),
					"removed": c.removed});
				self.tagged(&c.session_id, line, json)
			}
			Event::Evicted(e) => {
				self.cursors.remove(&e.session_id);
				let json = json!({"event": "evicted", "session": hex_id(&e.session_id),
					"reason": e.reason});
				self.tagged(&e.session_id, "retained transcript dropped", json)
			}
		}
	}
}
