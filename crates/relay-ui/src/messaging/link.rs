//! Real messaging adapter: its own `relay-api` client, registered with the
//! `messages` capability only, so call state and messaging stay separate and
//! a transcript is readable only by a client that asked for it.

use std::sync::Arc;

use async_trait::async_trait;
use relay_api::pb::{self, OperationState};
use relay_api::{ApiError, Endpoint, RelayClient, SessionItem, SessionWatch};
use relay_client::wire::connect_registered;
use tokio::sync::Mutex;

use super::engine::{MsgCommand, MsgConnection, MsgEngine, MsgFeed, MsgOps, MsgWatch};
use crate::engine::EngineError;
use crate::relay_link::map_api_error;

const CLIENT_NAME: &str = "relay-desktop-messages";
const CAPABILITIES: &[&str] = &["messages"];

type EndpointFn = dyn Fn() -> Result<Endpoint, EngineError> + Send + Sync;

pub struct MsgLink {
	endpoint: Box<EndpointFn>,
}

impl MsgLink {
	/// The endpoint is resolved on every connect so a restarted daemon is found.
	pub fn new(
		endpoint: impl Fn() -> Result<Endpoint, EngineError> + Send + Sync + 'static,
	) -> Self {
		Self {
			endpoint: Box::new(endpoint),
		}
	}
}

struct Shared {
	client: Mutex<RelayClient>,
}

#[async_trait]
impl MsgEngine for MsgLink {
	async fn connect(&self) -> Result<MsgConnection, EngineError> {
		let endpoint = (self.endpoint)()?;
		let client = connect_registered(
			&endpoint,
			CLIENT_NAME,
			env!("CARGO_PKG_VERSION"),
			CAPABILITIES,
		)
		.await
		.map_err(map_api_error)?;
		let shared = Arc::new(Shared {
			client: Mutex::new(client),
		});
		Ok(MsgConnection {
			feed: Box::new(Feed(shared.clone())),
			ops: Arc::new(Ops(shared)),
		})
	}
}

struct Feed(Arc<Shared>);

#[async_trait]
impl MsgFeed for Feed {
	async fn list(&mut self) -> Result<pb::ListSessionsResponse, EngineError> {
		let mut client = self.0.client.lock().await;
		client.list_sessions().await.map_err(map_api_error)
	}

	async fn transcript(
		&mut self,
		session: &[u8],
		from_ordinal: u64,
	) -> Result<Vec<pb::TranscriptEntry>, EngineError> {
		let mut client = self.0.client.lock().await;
		client
			.read_transcript(session, from_ordinal)
			.await
			.map_err(map_api_error)
	}

	async fn watch(&mut self, revision: u64) -> Result<Box<dyn MsgWatch>, EngineError> {
		let mut client = self.0.client.lock().await;
		let watch = client
			.watch_session(None, Some(revision))
			.await
			.map_err(map_api_error)?;
		Ok(Box::new(Watch(watch)))
	}
}

struct Watch(SessionWatch);

#[async_trait]
impl MsgWatch for Watch {
	async fn next(&mut self) -> Result<Option<SessionItem>, EngineError> {
		self.0.next().await.map_err(map_api_error)
	}
}

struct Ops(Arc<Shared>);

/// A messaging operation is final: `APPLIED` or `FAILED` with a problem code.
fn settled(result: Result<pb::SessionOpResult, ApiError>) -> Result<(), EngineError> {
	let result = result.map_err(map_api_error)?;
	check(result.operation)
}

fn check(operation: Option<pb::Operation>) -> Result<(), EngineError> {
	let operation = operation.unwrap_or_default();
	if operation.state == OperationState::Applied as i32 {
		return Ok(());
	}
	let problem = operation.problem.unwrap_or_default();
	Err(EngineError::Rejected {
		code: problem.code,
		message: problem.message,
	})
}

#[async_trait]
impl MsgOps for Ops {
	async fn run(&self, command: MsgCommand) -> Result<(), EngineError> {
		let mut c = self.0.client.lock().await;
		match command {
			MsgCommand::SendText { session, text } => {
				settled(c.send_session_message(&session, &text).await)
			}
			MsgCommand::Retry { session, seq } => {
				settled(c.retry_session_message(&session, seq).await)
			}
			MsgCommand::OfferFile { session, path } => settled(c.offer_file(&session, &path).await),
			MsgCommand::Respond {
				session,
				offer,
				accept,
				destination_dir,
			} => settled(
				c.respond_offer(&session, &offer, accept, &destination_dir, "")
					.await,
			),
			MsgCommand::RequestPreview { session, offer } => {
				settled(c.request_preview(&session, &offer).await)
			}
			MsgCommand::ApprovePreview {
				session,
				offer,
				approve,
			} => settled(c.approve_preview(&session, &offer, approve).await),
			MsgCommand::Cancel { session, offer } => {
				settled(c.cancel_transfer(&session, &offer).await)
			}
			MsgCommand::SetPermissions {
				session,
				allow_messages,
				allow_files,
			} => settled(
				c.set_session_permissions(&session, allow_messages, allow_files)
					.await,
			),
			MsgCommand::Pin {
				session,
				outgoing,
				seq,
				pinned,
			} => settled(c.pin_message(&session, outgoing, seq, pinned).await),
			MsgCommand::Clear { session } => settled(c.clear_session(&session).await),
			MsgCommand::Export {
				session,
				embed_images,
				destination,
			} => {
				let reply = c
					.export_session(&session, embed_images, Some(&destination))
					.await
					.map_err(map_api_error)?;
				check(reply.operation)
			}
			MsgCommand::SetDaemonEnabled(on) => {
				settled(c.set_messaging_settings(Some(on), None, None).await)
			}
		}
	}
}
