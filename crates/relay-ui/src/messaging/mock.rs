//! Mock messaging for `--mock`: seeded sessions (live, ended, one with a long
//! history) and an engine that answers like the daemon would, so every state
//! of the Messages panel can be seen without a daemon. Nothing leaves the
//! process; "Export" writes nothing here.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use relay_api::pb::{self, session_event::Event, transcript_entry};

use super::engine::{MsgCommand, MsgOps};
use super::hub::MsgHub;
use crate::engine::EngineError;
use crate::shell::Shell;

pub use super::mock_seed::{ADA_CALL, ALAN_CALL, seed};
use super::mock_seed::{NOW_MS, entry, offer};

struct MockOps {
	hub: Arc<MsgHub>,
	shell: Arc<dyn Shell>,
}

impl MockOps {
	fn apply(&self, event: Event) {
		self.hub
			.update(|s| s.apply(&pb::SessionEvent { event: Some(event) }));
		self.shell.request_repaint();
	}

	fn after(&self, millis: u64, event: Event) {
		let (hub, shell) = (self.hub.clone(), self.shell.clone());
		tokio::spawn(async move {
			tokio::time::sleep(Duration::from_millis(millis)).await;
			hub.update(|s| s.apply(&pb::SessionEvent { event: Some(event) }));
			shell.request_repaint();
		});
	}

	fn push(
		&self,
		session: &[u8],
		outgoing: bool,
		body: transcript_entry::Body,
		delivery: pb::DeliveryState,
	) -> (u64, u64) {
		let key = hex::encode(session);
		let (ordinal, mut summary) = self.hub.read(|s| {
			let v = s.session(&key).cloned();
			(v.as_ref().map_or(1, |v| v.next_ordinal), v)
		});
		let mut e = entry(ordinal, outgoing, body);
		e.delivery = delivery as i32;
		e.sent_at_unix_ms = NOW_MS + ordinal * 60_000;
		self.apply(Event::Entry(pb::NewEntry {
			session_id: session.to_vec(),
			entry: Some(e),
		}));
		if let Some(v) = summary.take() {
			let mut next = summary_of(&v);
			next.next_ordinal = ordinal + 1;
			next.message_count += 1;
			self.apply(Event::Session(next));
		}
		(ordinal, ordinal)
	}

	fn set_offer(&self, session: &[u8], ordinal: u64, f: impl FnOnce(&mut pb::FileOfferBody)) {
		let key = hex::encode(session);
		let found = self.hub.read(|s| {
			s.transcript(&key)
				.and_then(|t| t.entries.iter().find(|e| e.ordinal == ordinal).cloned())
		});
		if let Some(super::model::Body::File(o)) = found.map(|e| e.body) {
			let mut body = offer_of(&o);
			f(&mut body);
			self.apply(Event::Offer(pb::OfferChanged {
				session_id: session.to_vec(),
				ordinal,
				offer: Some(body),
			}));
		}
	}

	fn ordinal_of(&self, session: &[u8], offer_id: &[u8]) -> Option<u64> {
		let key = hex::encode(session);
		self.hub.read(|s| {
			s.transcript(&key)?
				.entries
				.iter()
				.find_map(|e| match &e.body {
					super::model::Body::File(o) if o.offer_id == offer_id => Some(e.ordinal),
					_ => None,
				})
		})
	}

	fn transfer(&self, session: Vec<u8>, ordinal: u64, size: u64, done_name: String) {
		let (hub, shell) = (self.hub.clone(), self.shell.clone());
		let me = MockOps { hub, shell };
		tokio::spawn(async move {
			for step in 1..=5u64 {
				tokio::time::sleep(Duration::from_millis(400)).await;
				me.set_offer(&session, ordinal, |o| {
					if o.state != pb::OfferState::InProgress as i32 {
						return;
					}
					o.transferred_bytes = size * step / 5;
					if step == 5 {
						o.state = pb::OfferState::Completed as i32;
						o.saved_as.clone_from(&done_name);
					}
				});
			}
		});
	}
}

fn summary_of(v: &super::model::SessionView) -> pb::SessionSummary {
	pb::SessionSummary {
		session_id: v.id.clone(),
		call_id: hex::decode(&v.call_id).unwrap_or_default(),
		peer_name: v.peer_name.clone(),
		state: if v.is_live() {
			pb::SessionState::Active
		} else {
			pb::SessionState::Ended
		} as i32,
		permissions: Some(pb::SessionPermissions {
			allow_messages: v.perms.allow_messages,
			allow_files: v.perms.allow_files,
		}),
		local_is_host: v.local_is_host,
		started_at_unix_ms: v.started_ms,
		ended_at_unix_ms: v.ended_ms,
		message_count: v.message_count,
		next_ordinal: v.next_ordinal,
		used_bytes: v.used_bytes,
		limit_bytes: v.limit_bytes,
		peer_id: Vec::new(),
	}
}

fn offer_of(o: &super::model::OfferView) -> pb::FileOfferBody {
	use super::model::OfferStatus as S;
	pb::FileOfferBody {
		offer_id: o.offer_id.clone(),
		name: o.name.clone(),
		size: o.size,
		media_type: o.media_type.clone(),
		blake3: o.blake3.clone(),
		state: match o.status {
			S::Offered => pb::OfferState::Offered,
			S::InProgress => pb::OfferState::InProgress,
			S::Completed => pb::OfferState::Completed,
			S::Rejected => pb::OfferState::Rejected,
			S::Failed => pb::OfferState::Failed,
			S::Cancelled => pb::OfferState::Cancelled,
			S::Expired => pb::OfferState::Expired,
		} as i32,
		transferred_bytes: o.transferred,
		preview: o.preview.clone(),
		preview_requested: o.preview_requested,
		preview_approval_pending: o.preview_approval_pending,
		executable_warning: o.executable_warning,
		saved_as: o.saved_as.clone(),
		failure: o.failure.clone(),
		outgoing: o.outgoing,
	}
}

#[async_trait]
impl MsgOps for MockOps {
	async fn run(&self, command: MsgCommand) -> Result<(), EngineError> {
		match command {
			MsgCommand::SendText { session, text } => {
				let fail = text.contains("/fail");
				let (_, seq) = self.push(
					&session,
					true,
					transcript_entry::Body::Text(text),
					pb::DeliveryState::Sending,
				);
				let delivery = if fail {
					pb::DeliveryState::Failed
				} else {
					pb::DeliveryState::Delivered
				};
				self.after(
					500,
					Event::Delivery(pb::DeliveryChanged {
						session_id: session,
						seq,
						delivery: delivery as i32,
						failure: if fail {
							"timeout".into()
						} else {
							String::new()
						},
					}),
				);
			}
			MsgCommand::Retry { session, seq } => {
				self.apply(Event::Delivery(pb::DeliveryChanged {
					session_id: session.clone(),
					seq,
					delivery: pb::DeliveryState::Sending as i32,
					failure: String::new(),
				}));
				self.after(
					500,
					Event::Delivery(pb::DeliveryChanged {
						session_id: session,
						seq,
						delivery: pb::DeliveryState::Delivered as i32,
						failure: String::new(),
					}),
				);
			}
			MsgCommand::OfferFile { session, path } => {
				let name = path.rsplit(['/', '\\']).next().unwrap_or("file").to_owned();
				let body = offer(50, &name, 4_200_000, pb::OfferState::Offered, true);
				let (ordinal, _) = self.push(
					&session,
					true,
					transcript_entry::Body::File(body),
					pb::DeliveryState::Delivered,
				);
				self.set_offer(&session, ordinal, |_| {});
			}
			MsgCommand::Respond {
				session,
				offer,
				accept,
				destination_dir,
			} => {
				let Some(ordinal) = self.ordinal_of(&session, &offer) else {
					return Err(rejected("OFFER_NOT_FOUND"));
				};
				if !accept {
					self.set_offer(&session, ordinal, |o| {
						o.state = pb::OfferState::Rejected as i32
					});
				} else {
					if !destination_dir.starts_with('/') && !destination_dir.contains(':') {
						return Err(rejected("DESTINATION_INVALID"));
					}
					let mut size = 0;
					self.set_offer(&session, ordinal, |o| {
						o.state = pb::OfferState::InProgress as i32;
						size = o.size;
					});
					self.transfer(session, ordinal, size, "saved-file".into());
				}
			}
			MsgCommand::RequestPreview { session, offer } => {
				let Some(ordinal) = self.ordinal_of(&session, &offer) else {
					return Err(rejected("OFFER_NOT_FOUND"));
				};
				self.set_offer(&session, ordinal, |o| o.preview_requested = true);
			}
			MsgCommand::ApprovePreview {
				session,
				offer,
				approve,
			} => {
				let Some(ordinal) = self.ordinal_of(&session, &offer) else {
					return Err(rejected("OFFER_NOT_FOUND"));
				};
				self.set_offer(&session, ordinal, |o| {
					o.preview_approval_pending = false;
					if approve {
						o.preview = "first line\nsecond line".into();
					}
				});
			}
			MsgCommand::Cancel { session, offer } => {
				let Some(ordinal) = self.ordinal_of(&session, &offer) else {
					return Err(rejected("OFFER_NOT_FOUND"));
				};
				self.set_offer(&session, ordinal, |o| {
					o.state = pb::OfferState::Cancelled as i32
				});
			}
			MsgCommand::SetPermissions {
				session,
				allow_messages,
				allow_files,
			} => {
				let key = hex::encode(&session);
				let cur = self.hub.read(|s| s.session(&key).map(|v| v.perms));
				let Some(cur) = cur else {
					return Err(rejected("SESSION_NOT_FOUND"));
				};
				self.apply(Event::Permissions(pb::PermissionsChanged {
					session_id: session,
					permissions: Some(pb::SessionPermissions {
						allow_messages: allow_messages.unwrap_or(cur.allow_messages),
						allow_files: allow_files.unwrap_or(cur.allow_files),
					}),
					by_peer: false,
				}));
			}
			MsgCommand::Pin {
				session,
				outgoing,
				seq,
				pinned,
			} => {
				self.apply(Event::Pin(pb::PinChanged {
					session_id: session,
					message: Some(pb::MessageRef {
						ordinal: 0,
						outgoing,
						seq,
					}),
					pinned,
				}));
			}
			MsgCommand::Clear { session } => {
				let key = hex::encode(&session);
				let ended = self
					.hub
					.read(|s| s.session(&key).is_some_and(|v| !v.is_live()));
				self.apply(Event::Cleared(pb::SessionCleared {
					session_id: session,
					removed: ended,
					next_ordinal: 0,
				}));
			}
			MsgCommand::Export { .. } => {}
			MsgCommand::SetDaemonEnabled(on) => {
				self.hub.update(|s| {
					if let Some(x) = s.settings.as_mut() {
						x.enabled = on;
					}
				});
				self.shell.request_repaint();
			}
		}
		Ok(())
	}
}

fn rejected(code: &str) -> EngineError {
	EngineError::Rejected {
		code: code.into(),
		message: String::new(),
	}
}

/// Serve mock messaging while the user has opted in.
pub async fn run(hub: Arc<MsgHub>, shell: Arc<dyn Shell>) {
	loop {
		if !hub.enabled() {
			hub.changed().await;
			continue;
		}
		let epoch = hub.epoch();
		seed(&hub);
		hub.set_ops(Arc::new(MockOps {
			hub: hub.clone(),
			shell: shell.clone(),
		}));
		shell.request_repaint();
		while hub.epoch() == epoch {
			hub.changed().await;
		}
	}
}
