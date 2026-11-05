//! Session messaging requests (`docs/api.md`, "Session messaging").
//!
//! A separate trait from [`super::Control`]: messaging needs the `messages`
//! capability, which `relay messages` and the TUI's messages connection ask for
//! and nothing else does. Message text only ever passes through these
//! signatures; nothing here logs it.

use relay_api::pb::{
	ExportSessionResponse, ListSessionsResponse, SearchSessionResponse, SessionOpResult, Transcript,
};
use relay_api::{ApiError, SessionItem, SessionWatch};

use super::Feed;
use super::real::RealClient;

/// A session watch that yields validated [`SessionItem`]s.
pub struct RealSessionWatch(SessionWatch);

impl Feed for RealSessionWatch {
	type Item = SessionItem;
	async fn next(&mut self) -> Result<Option<SessionItem>, ApiError> {
		self.0.next().await
	}
}

/// Request/response and streaming operations on call sessions. Every mutation
/// answers a final [`SessionOpResult`]; callers turn a failed one into an error.
#[allow(async_fn_in_trait, reason = "only used with concrete generics")]
pub trait Messaging {
	/// The session watch stream.
	type Watch: Feed<Item = SessionItem>;
	/// Sessions, live and retained, with the feed revision and settings.
	async fn list_sessions(&mut self) -> Result<ListSessionsResponse, ApiError>;
	/// One page of a transcript from `from_ordinal` (0 = the start).
	async fn get_transcript(
		&mut self,
		session: &[u8],
		from_ordinal: u64,
		limit: u32,
	) -> Result<Transcript, ApiError>;
	/// Open a watch: all sessions or one, fresh (`None`) or resuming.
	async fn open_session_watch(
		&mut self,
		session: Option<&[u8]>,
		after_revision: Option<u64>,
	) -> Result<Self::Watch, ApiError>;
	/// Send text.
	async fn send_text(&mut self, session: &[u8], text: &str) -> Result<SessionOpResult, ApiError>;
	/// Send an inline image (at most 15 KiB).
	async fn send_image(
		&mut self,
		session: &[u8],
		image: Vec<u8>,
	) -> Result<SessionOpResult, ApiError>;
	/// Retry a failed local message.
	async fn retry_message(
		&mut self,
		session: &[u8],
		seq: u64,
	) -> Result<SessionOpResult, ApiError>;
	/// Offer a file by absolute path; the daemon opens it.
	async fn offer_file(&mut self, session: &[u8], path: &str)
	-> Result<SessionOpResult, ApiError>;
	/// Accept (into `destination_dir`) or reject an offer.
	async fn respond_offer(
		&mut self,
		session: &[u8],
		offer: &[u8],
		accept: bool,
		destination_dir: &str,
		file_name: &str,
	) -> Result<SessionOpResult, ApiError>;
	/// Ask the sender for a text preview.
	async fn request_preview(
		&mut self,
		session: &[u8],
		offer: &[u8],
	) -> Result<SessionOpResult, ApiError>;
	/// Approve or refuse the peer's preview request.
	async fn approve_preview(
		&mut self,
		session: &[u8],
		offer: &[u8],
		approve: bool,
	) -> Result<SessionOpResult, ApiError>;
	/// Cancel a transfer, withdraw or decline an open offer.
	async fn cancel_offer(
		&mut self,
		session: &[u8],
		offer: &[u8],
	) -> Result<SessionOpResult, ApiError>;
	/// Change the session permissions (`None` keeps the value).
	async fn set_permissions(
		&mut self,
		session: &[u8],
		allow_messages: Option<bool>,
		allow_files: Option<bool>,
	) -> Result<SessionOpResult, ApiError>;
	/// Pin or unpin an entry.
	async fn pin(
		&mut self,
		session: &[u8],
		outgoing: bool,
		seq: u64,
		pinned: bool,
	) -> Result<SessionOpResult, ApiError>;
	/// Substring search.
	async fn search(
		&mut self,
		session: &[u8],
		query: &str,
	) -> Result<SearchSessionResponse, ApiError>;
	/// Clear a transcript (an ended session is removed).
	async fn clear(&mut self, session: &[u8]) -> Result<SessionOpResult, ApiError>;
	/// Export as JSON, optionally written by the daemon to `destination`.
	async fn export(
		&mut self,
		session: &[u8],
		embed_images: bool,
		destination: Option<&str>,
	) -> Result<ExportSessionResponse, ApiError>;
}

impl Messaging for RealClient {
	type Watch = RealSessionWatch;

	async fn list_sessions(&mut self) -> Result<ListSessionsResponse, ApiError> {
		self.client_mut().list_sessions().await
	}
	async fn get_transcript(
		&mut self,
		session: &[u8],
		from_ordinal: u64,
		limit: u32,
	) -> Result<Transcript, ApiError> {
		self.client_mut()
			.get_transcript(session, from_ordinal, limit)
			.await
	}
	async fn open_session_watch(
		&mut self,
		session: Option<&[u8]>,
		after_revision: Option<u64>,
	) -> Result<RealSessionWatch, ApiError> {
		self.client_mut()
			.watch_session(session, after_revision)
			.await
			.map(RealSessionWatch)
	}
	async fn send_text(&mut self, session: &[u8], text: &str) -> Result<SessionOpResult, ApiError> {
		self.client_mut().send_session_message(session, text).await
	}
	async fn send_image(
		&mut self,
		session: &[u8],
		image: Vec<u8>,
	) -> Result<SessionOpResult, ApiError> {
		self.client_mut().send_session_image(session, image).await
	}
	async fn retry_message(
		&mut self,
		session: &[u8],
		seq: u64,
	) -> Result<SessionOpResult, ApiError> {
		self.client_mut().retry_session_message(session, seq).await
	}
	async fn offer_file(
		&mut self,
		session: &[u8],
		path: &str,
	) -> Result<SessionOpResult, ApiError> {
		self.client_mut().offer_file(session, path).await
	}
	async fn respond_offer(
		&mut self,
		session: &[u8],
		offer: &[u8],
		accept: bool,
		destination_dir: &str,
		file_name: &str,
	) -> Result<SessionOpResult, ApiError> {
		self.client_mut()
			.respond_offer(session, offer, accept, destination_dir, file_name)
			.await
	}
	async fn request_preview(
		&mut self,
		session: &[u8],
		offer: &[u8],
	) -> Result<SessionOpResult, ApiError> {
		self.client_mut().request_preview(session, offer).await
	}
	async fn approve_preview(
		&mut self,
		session: &[u8],
		offer: &[u8],
		approve: bool,
	) -> Result<SessionOpResult, ApiError> {
		self.client_mut()
			.approve_preview(session, offer, approve)
			.await
	}
	async fn cancel_offer(
		&mut self,
		session: &[u8],
		offer: &[u8],
	) -> Result<SessionOpResult, ApiError> {
		self.client_mut().cancel_transfer(session, offer).await
	}
	async fn set_permissions(
		&mut self,
		session: &[u8],
		allow_messages: Option<bool>,
		allow_files: Option<bool>,
	) -> Result<SessionOpResult, ApiError> {
		self.client_mut()
			.set_session_permissions(session, allow_messages, allow_files)
			.await
	}
	async fn pin(
		&mut self,
		session: &[u8],
		outgoing: bool,
		seq: u64,
		pinned: bool,
	) -> Result<SessionOpResult, ApiError> {
		self.client_mut()
			.pin_message(session, outgoing, seq, pinned)
			.await
	}
	async fn search(
		&mut self,
		session: &[u8],
		query: &str,
	) -> Result<SearchSessionResponse, ApiError> {
		self.client_mut().search_session(session, query).await
	}
	async fn clear(&mut self, session: &[u8]) -> Result<SessionOpResult, ApiError> {
		self.client_mut().clear_session(session).await
	}
	async fn export(
		&mut self,
		session: &[u8],
		embed_images: bool,
		destination: Option<&str>,
	) -> Result<ExportSessionResponse, ApiError> {
		self.client_mut()
			.export_session(session, embed_images, destination)
			.await
	}
}
