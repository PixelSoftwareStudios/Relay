//! Session messaging helpers on [`RelayClient`] (`docs/session-messages.md`).
//!
//! Every call needs the `messages` capability, named at registration; without it the engine
//! answers `PERMISSION_DENIED`. The engine returns exact text, so copy actions are client-side.

use tonic::Streaming;

use crate::ApiError;
use crate::RelayClient;
use crate::pb::session_envelope::Body;
use crate::pb::transcript_entry;
use crate::pb::{
	ApprovePreviewRequest, ClearSessionRequest, ExportSessionRequest, ExportSessionResponse,
	GetTranscriptRequest, ListSessionsResponse, OfferFileRequest, OfferRequest, PinMessageRequest,
	RespondOfferRequest, RetrySessionMessageRequest, SearchSessionRequest, SearchSessionResponse,
	SendSessionImageRequest, SendSessionMessageRequest, SessionEnvelope, SessionEvent, SessionList,
	SessionOpResult, SessionResync, SetMessagingSettingsRequest, SetSessionPermissionsRequest,
	Transcript, TranscriptEntry, WatchSessionRequest,
};

/// Largest export JSON the engine returns inline (the message limit minus headroom).
pub const EXPORT_INLINE_MAX_BYTES: usize = 960 * 1024;

/// One validated item of a session watch.
#[derive(Debug, Clone)]
pub enum SessionItem {
	/// Sessions and settings at the start of a watch without `after_revision`.
	List(SessionList),
	/// An event at `revision`.
	Event {
		/// Feed revision after the event.
		revision: u64,
		/// The event.
		event: Box<SessionEvent>,
	},
	/// The engine could not replay or the stream lagged; call `list_sessions`,
	/// re-read transcripts with `get_transcript` and open a fresh watch.
	Resync(SessionResync),
}

/// Revision and incarnation checks for a session watch.
#[derive(Debug, Clone)]
pub struct SessionTracker {
	strict: bool,
	incarnation: Option<Vec<u8>>,
	last: Option<u64>,
}

impl SessionTracker {
	/// Fresh stream; `strict` (no session filter) rejects revision gaps.
	pub fn new(strict: bool) -> Self {
		Self {
			strict,
			incarnation: None,
			last: None,
		}
	}

	/// A stream continuing after `revision` of `incarnation`.
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

	/// Validates one envelope against the stream so far.
	pub fn observe(&mut self, envelope: SessionEnvelope) -> Result<SessionItem, ApiError> {
		match &self.incarnation {
			Some(known) if *known != envelope.engine_incarnation => {
				return Err(ApiError::IncarnationChanged);
			}
			Some(_) => {}
			None => self.incarnation = Some(envelope.engine_incarnation.clone()),
		}
		let revision = envelope.revision;
		match envelope.body.ok_or(ApiError::StreamClosed)? {
			Body::List(list) => {
				self.last = Some(revision);
				Ok(SessionItem::List(list))
			}
			Body::Resync(resync) => {
				self.last = None;
				self.incarnation = None;
				Ok(SessionItem::Resync(resync))
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
				Ok(SessionItem::Event {
					revision,
					event: Box::new(event),
				})
			}
		}
	}
}

/// A live `WatchSession` stream with gap detection.
pub struct SessionWatch {
	stream: Streaming<SessionEnvelope>,
	tracker: SessionTracker,
}

impl SessionWatch {
	/// Next validated item; `Ok(None)` when the engine closed the stream.
	pub async fn next(&mut self) -> Result<Option<SessionItem>, ApiError> {
		match self.stream.message().await? {
			Some(envelope) => self.tracker.observe(envelope).map(Some),
			None => Ok(None),
		}
	}

	/// Last revision accepted so far.
	pub fn last_revision(&self) -> Option<u64> {
		self.tracker.last_revision()
	}
}

/// The exact text of an entry, for a copy action.
pub fn entry_text(entry: &TranscriptEntry) -> Option<&str> {
	match entry.body.as_ref()? {
		transcript_entry::Body::Text(text) => Some(text),
		_ => None,
	}
}

impl RelayClient {
	/// Sessions (live and retained) and the messaging settings, with the feed revision to follow.
	pub async fn list_sessions(&mut self) -> Result<ListSessionsResponse, ApiError> {
		let ctx = self.require_context()?;
		Ok(self.inner.list_sessions(ctx).await?.into_inner())
	}

	/// One page of a transcript from `from_ordinal` (0 = the start).
	pub async fn get_transcript(
		&mut self,
		session_id: &[u8],
		from_ordinal: u64,
		limit: u32,
	) -> Result<Transcript, ApiError> {
		let request = GetTranscriptRequest {
			client: Some(self.require_context()?),
			session_id: session_id.to_vec(),
			from_ordinal,
			limit,
		};
		Ok(self.inner.get_transcript(request).await?.into_inner())
	}

	/// Every entry from `from_ordinal` on, following pages.
	pub async fn read_transcript(
		&mut self,
		session_id: &[u8],
		mut from_ordinal: u64,
	) -> Result<Vec<TranscriptEntry>, ApiError> {
		let mut all = Vec::new();
		loop {
			let page = self.get_transcript(session_id, from_ordinal, 0).await?;
			all.extend(page.entries);
			if !page.more {
				return Ok(all);
			}
			from_ordinal = page.next_ordinal;
		}
	}

	/// Opens a session watch. `after_revision` resumes (a missed window yields `Resync`);
	/// `None` starts with a [`SessionItem::List`]. An empty `session_id` watches all sessions.
	pub async fn watch_session(
		&mut self,
		session_id: Option<&[u8]>,
		after_revision: Option<u64>,
	) -> Result<SessionWatch, ApiError> {
		let client = self.require_context()?;
		let strict = session_id.is_none_or(<[u8]>::is_empty);
		let tracker = match after_revision {
			Some(rev) => SessionTracker::resumed(strict, client.engine_incarnation.clone(), rev),
			None => SessionTracker::new(strict),
		};
		let stream = self
			.inner
			.watch_session(WatchSessionRequest {
				client: Some(client),
				after_revision,
				session_id: session_id.map(<[u8]>::to_vec).unwrap_or_default(),
			})
			.await?
			.into_inner();
		Ok(SessionWatch { stream, tracker })
	}

	/// Sends text into a session.
	pub async fn send_session_message(
		&mut self,
		session_id: &[u8],
		text: &str,
	) -> Result<SessionOpResult, ApiError> {
		let request = SendSessionMessageRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			text: text.to_owned(),
		};
		Ok(self.inner.send_session_message(request).await?.into_inner())
	}

	/// Sends an inline image (original compressed bytes, at most 15 KiB; larger ones are file offers).
	pub async fn send_session_image(
		&mut self,
		session_id: &[u8],
		image: Vec<u8>,
	) -> Result<SessionOpResult, ApiError> {
		let request = SendSessionImageRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			image,
		};
		Ok(self.inner.send_session_image(request).await?.into_inner())
	}

	/// Retries a failed local message (same sequence, fresh attempt budget).
	pub async fn retry_session_message(
		&mut self,
		session_id: &[u8],
		seq: u64,
	) -> Result<SessionOpResult, ApiError> {
		let request = RetrySessionMessageRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			seq,
		};
		Ok(self
			.inner
			.retry_session_message(request)
			.await?
			.into_inner())
	}

	/// Offers a file by absolute path. The daemon, not the client, opens it.
	pub async fn offer_file(
		&mut self,
		session_id: &[u8],
		path: &str,
	) -> Result<SessionOpResult, ApiError> {
		let request = OfferFileRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			path: path.to_owned(),
		};
		Ok(self.inner.offer_file(request).await?.into_inner())
	}

	/// Accepts (into `destination_dir`, optionally suggesting `file_name`) or rejects an offer.
	pub async fn respond_offer(
		&mut self,
		session_id: &[u8],
		offer_id: &[u8],
		accept: bool,
		destination_dir: &str,
		file_name: &str,
	) -> Result<SessionOpResult, ApiError> {
		let request = RespondOfferRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			offer_id: offer_id.to_vec(),
			accept,
			destination_dir: destination_dir.to_owned(),
			file_name: file_name.to_owned(),
		};
		Ok(self.inner.respond_offer(request).await?.into_inner())
	}

	fn offer_request(&self, session_id: &[u8], offer_id: &[u8]) -> Result<OfferRequest, ApiError> {
		Ok(OfferRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			offer_id: offer_id.to_vec(),
		})
	}

	/// Asks the sender for a text preview of an offered file.
	pub async fn request_preview(
		&mut self,
		session_id: &[u8],
		offer_id: &[u8],
	) -> Result<SessionOpResult, ApiError> {
		let request = self.offer_request(session_id, offer_id)?;
		Ok(self.inner.request_preview(request).await?.into_inner())
	}

	/// Sender: approves or refuses the peer's preview request.
	pub async fn approve_preview(
		&mut self,
		session_id: &[u8],
		offer_id: &[u8],
		approve: bool,
	) -> Result<SessionOpResult, ApiError> {
		let request = ApprovePreviewRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			offer_id: offer_id.to_vec(),
			approve,
		};
		Ok(self.inner.approve_preview(request).await?.into_inner())
	}

	/// Cancels a running transfer or withdraws an open offer.
	pub async fn cancel_transfer(
		&mut self,
		session_id: &[u8],
		offer_id: &[u8],
	) -> Result<SessionOpResult, ApiError> {
		let request = self.offer_request(session_id, offer_id)?;
		Ok(self.inner.cancel_transfer(request).await?.into_inner())
	}

	/// Changes what the session accepts and sends (`None` keeps the value).
	pub async fn set_session_permissions(
		&mut self,
		session_id: &[u8],
		allow_messages: Option<bool>,
		allow_files: Option<bool>,
	) -> Result<SessionOpResult, ApiError> {
		let request = SetSessionPermissionsRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			allow_messages,
			allow_files,
		};
		Ok(self
			.inner
			.set_session_permissions(request)
			.await?
			.into_inner())
	}

	/// Pins or unpins an entry locally.
	pub async fn pin_message(
		&mut self,
		session_id: &[u8],
		outgoing: bool,
		seq: u64,
		pinned: bool,
	) -> Result<SessionOpResult, ApiError> {
		let request = PinMessageRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			outgoing,
			seq,
			pinned,
		};
		Ok(self.inner.pin_message(request).await?.into_inner())
	}

	/// Case-insensitive substring search (at most 100 hits).
	pub async fn search_session(
		&mut self,
		session_id: &[u8],
		query: &str,
	) -> Result<SearchSessionResponse, ApiError> {
		let request = SearchSessionRequest {
			client: Some(self.require_context()?),
			session_id: session_id.to_vec(),
			query: query.to_owned(),
		};
		Ok(self.inner.search_session(request).await?.into_inner())
	}

	/// Clears a transcript locally (an ended session is removed entirely).
	pub async fn clear_session(&mut self, session_id: &[u8]) -> Result<SessionOpResult, ApiError> {
		let request = ClearSessionRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
		};
		Ok(self.inner.clear_session(request).await?.into_inner())
	}

	/// Exports the transcript as JSON. Without `destination_path` the bytes come back and the
	/// daemon writes nothing; with it (absolute, must not exist) the daemon writes the file.
	pub async fn export_session(
		&mut self,
		session_id: &[u8],
		embed_images: bool,
		destination_path: Option<&str>,
	) -> Result<ExportSessionResponse, ApiError> {
		let request = ExportSessionRequest {
			context: Some(self.new_mutation()?),
			session_id: session_id.to_vec(),
			embed_images,
			destination_path: destination_path.unwrap_or_default().to_owned(),
		};
		let response = self.inner.export_session(request).await?.into_inner();
		if response.json.len() > EXPORT_INLINE_MAX_BYTES {
			return Err(ApiError::InvalidResponse("export exceeds the inline limit"));
		}
		Ok(response)
	}

	/// Changes the live messaging preferences (`None` keeps the value).
	pub async fn set_messaging_settings(
		&mut self,
		enabled: Option<bool>,
		max_offer_size_mib: Option<u32>,
		transfer_rate_limit_mbps: Option<u32>,
	) -> Result<SessionOpResult, ApiError> {
		let request = SetMessagingSettingsRequest {
			context: Some(self.new_mutation()?),
			enabled,
			max_offer_size_mib,
			transfer_rate_limit_mbps,
		};
		Ok(self
			.inner
			.set_messaging_settings(request)
			.await?
			.into_inner())
	}
}
