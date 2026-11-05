//! Ports between messaging and the daemon. The real adapter is
//! [`link`](super::link); tests and mock mode plug in their own.

use std::sync::Arc;

use async_trait::async_trait;
use relay_api::SessionItem;
use relay_api::pb;

use crate::engine::EngineError;

/// Everything the Messages panel may ask the daemon to do. Ids are raw bytes.
#[derive(Clone, PartialEq)]
pub enum MsgCommand {
	SendText {
		session: Vec<u8>,
		text: String,
	},
	Retry {
		session: Vec<u8>,
		seq: u64,
	},
	OfferFile {
		session: Vec<u8>,
		/// Absolute path; the daemon opens it.
		path: String,
	},
	Respond {
		session: Vec<u8>,
		offer: Vec<u8>,
		accept: bool,
		destination_dir: String,
	},
	RequestPreview {
		session: Vec<u8>,
		offer: Vec<u8>,
	},
	ApprovePreview {
		session: Vec<u8>,
		offer: Vec<u8>,
		approve: bool,
	},
	Cancel {
		session: Vec<u8>,
		offer: Vec<u8>,
	},
	SetPermissions {
		session: Vec<u8>,
		allow_messages: Option<bool>,
		allow_files: Option<bool>,
	},
	Pin {
		session: Vec<u8>,
		outgoing: bool,
		seq: u64,
		pinned: bool,
	},
	Clear {
		session: Vec<u8>,
	},
	/// Export to a file the daemon creates (the user chose the path).
	Export {
		session: Vec<u8>,
		embed_images: bool,
		destination: String,
	},
	/// The daemon's live `messages.enabled` preference.
	SetDaemonEnabled(bool),
}

/// Content is never printed: only the kind of command.
impl std::fmt::Debug for MsgCommand {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		let name = match self {
			Self::SendText { .. } => "SendText",
			Self::Retry { .. } => "Retry",
			Self::OfferFile { .. } => "OfferFile",
			Self::Respond { .. } => "Respond",
			Self::RequestPreview { .. } => "RequestPreview",
			Self::ApprovePreview { .. } => "ApprovePreview",
			Self::Cancel { .. } => "Cancel",
			Self::SetPermissions { .. } => "SetPermissions",
			Self::Pin { .. } => "Pin",
			Self::Clear { .. } => "Clear",
			Self::Export { .. } => "Export",
			Self::SetDaemonEnabled(_) => "SetDaemonEnabled",
		};
		write!(f, "MsgCommand::{name}(<redacted>)")
	}
}

#[async_trait]
pub trait MsgOps: Send + Sync {
	/// Run a command; the daemon's final answer, success or a coded rejection.
	async fn run(&self, command: MsgCommand) -> Result<(), EngineError>;
}

/// Next validated item of a session watch.
#[async_trait]
pub trait MsgWatch: Send {
	async fn next(&mut self) -> Result<Option<SessionItem>, EngineError>;
}

/// Reads used for the initial load and every resync.
#[async_trait]
pub trait MsgFeed: Send {
	async fn list(&mut self) -> Result<pb::ListSessionsResponse, EngineError>;
	/// Every entry from `from_ordinal` on.
	async fn transcript(
		&mut self,
		session: &[u8],
		from_ordinal: u64,
	) -> Result<Vec<pb::TranscriptEntry>, EngineError>;
	/// A fresh watch of all sessions resuming after `revision`.
	async fn watch(&mut self, revision: u64) -> Result<Box<dyn MsgWatch>, EngineError>;
}

pub struct MsgConnection {
	pub feed: Box<dyn MsgFeed>,
	pub ops: Arc<dyn MsgOps>,
}

#[async_trait]
pub trait MsgEngine: Send + Sync + 'static {
	/// Connect and register with the `messages` capability.
	async fn connect(&self) -> Result<MsgConnection, EngineError>;
}

/// Whether the daemon refused the capability (not a connectivity problem).
pub fn is_denied(error: &EngineError) -> bool {
	matches!(error, EngineError::Rejected { code, .. } if code == "PermissionDenied")
}
