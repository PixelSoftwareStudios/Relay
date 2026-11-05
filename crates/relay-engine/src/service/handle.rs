//! The cloneable front door of the control actor.

use std::sync::Arc;

use relay_api::pb::{Operation, Problem, Snapshot, StateEvent};
use relay_core::{
	ClientId, Input, MonoTime, OpResult, OperationId, PeerEvent, Request, Revision, SystemEvent,
};
use tokio::sync::{broadcast, mpsc, oneshot};

use super::error::ServiceError;
use super::feed::FeedItem;
use super::gates::GateReader;
use super::ledger::Fingerprint;
use super::ports::{DeviceInfo, PeerInfo, TrustRequest};

/// Work that is executed by an adapter rather than the domain core.
#[derive(Debug, Clone, PartialEq)]
pub enum ServiceOp {
	/// Select input and output devices.
	SelectDevices {
		/// Input device id.
		input: String,
		/// Output device id.
		output: String,
	},
	/// Select a noise suppression backend.
	NoiseSuppression(String),
	/// Start a microphone test.
	StartMicTest {
		/// Play the microphone back locally.
		local_playback: bool,
	},
	/// Stop the microphone test.
	StopMicTest,
	/// Change the advertised display name.
	Profile(String),
	/// Replace the avatar.
	Avatar(Vec<u8>),
	/// Decide a peer's trust.
	Trust(TrustRequest),
	/// Set the microphone gain.
	MicrophoneGain(f32),
}

/// A domain request's result.
#[derive(Debug, Clone)]
pub struct Applied {
	/// What the domain answered.
	pub result: OpResult,
	/// Domain revision after the request.
	pub revision: Revision,
	/// The answer came from the idempotency cache.
	pub duplicate: bool,
	/// API revision when the answer was produced.
	pub api_revision: u64,
	/// Actor clock when the answer was produced.
	pub at: MonoTime,
}

/// How a watcher is brought up to date.
#[derive(Debug)]
pub enum Subscribed {
	/// Start from this snapshot.
	Fresh {
		/// The snapshot.
		snapshot: Box<Snapshot>,
		/// Live items after the snapshot.
		live: broadcast::Receiver<Arc<FeedItem>>,
	},
	/// Continue after the requested revision.
	Resume {
		/// Missed events, oldest first.
		replay: Vec<(u64, StateEvent)>,
		/// Live items after the replay.
		live: broadcast::Receiver<Arc<FeedItem>>,
	},
	/// The request cannot be replayed; open a fresh watch.
	Resync {
		/// The engine's current revision.
		current: u64,
	},
}

type Reply<T> = oneshot::Sender<T>;

/// Messages the actor understands.
#[derive(Debug)]
pub(crate) enum Msg {
	Submit {
		request: Request,
		reply: Reply<Result<Applied, ServiceError>>,
	},
	Input(Input),
	Service {
		client: ClientId,
		operation: OperationId,
		print: Fingerprint,
		op: ServiceOp,
		reply: Reply<Result<Operation, ServiceError>>,
	},
	Snapshot(Reply<Snapshot>),
	Operation {
		client: ClientId,
		operation: OperationId,
		reply: Reply<Option<Operation>>,
	},
	Subscribe {
		after: Option<u64>,
		reply: Reply<Subscribed>,
	},
	Peers(Vec<PeerInfo>),
	Devices(Vec<DeviceInfo>),
	Attention {
		peer: Option<PeerInfo>,
		warning: Problem,
	},
	Shutdown,
}

/// Cloneable handle to the control actor.
#[derive(Debug, Clone)]
pub struct ControlHandle {
	tx: mpsc::Sender<Msg>,
	gates: GateReader,
}

impl ControlHandle {
	pub(crate) fn new(tx: mpsc::Sender<Msg>, gates: GateReader) -> Self {
		Self { tx, gates }
	}

	/// Lock-free access to the published transmit gates.
	pub fn gates(&self) -> &GateReader {
		&self.gates
	}

	/// Reports a device, media or OS event.
	pub async fn system(&self, event: SystemEvent) -> Result<(), ServiceError> {
		self.send_input(Input::System(event)).await
	}

	/// Reports a peer-link event.
	pub async fn peer(&self, event: PeerEvent) -> Result<(), ServiceError> {
		self.send_input(Input::Peer(event)).await
	}

	/// Tells the domain a client transport vanished.
	pub async fn disconnected(&self, client: ClientId) -> Result<(), ServiceError> {
		self.send_input(Input::Disconnected(client)).await
	}

	/// Awaits queue space, then submits any input.
	pub async fn send_input(&self, input: Input) -> Result<(), ServiceError> {
		self.tx
			.send(Msg::Input(input))
			.await
			.map_err(|_| ServiceError::Stopped)
	}

	/// Blocking variant of [`Self::send_input`] for adapter threads.
	pub fn send_input_blocking(&self, input: Input) -> Result<(), ServiceError> {
		self.tx
			.blocking_send(Msg::Input(input))
			.map_err(|_| ServiceError::Stopped)
	}

	/// Replaces the discovered peers.
	pub async fn peers_changed(&self, peers: Vec<PeerInfo>) -> Result<(), ServiceError> {
		self.post(Msg::Peers(peers)).await
	}

	/// Replaces the known devices.
	pub async fn devices_changed(&self, devices: Vec<DeviceInfo>) -> Result<(), ServiceError> {
		self.post(Msg::Devices(devices)).await
	}

	/// Raises a warning for clients, optionally flagging a peer that needs the
	/// user's attention (see `Directory::flag`).
	pub async fn attention(
		&self,
		peer: Option<PeerInfo>,
		warning: Problem,
	) -> Result<(), ServiceError> {
		self.post(Msg::Attention { peer, warning }).await
	}

	/// Stops the actor after it drains what it already accepted.
	pub async fn shutdown(&self) {
		let _ = self.tx.send(Msg::Shutdown).await;
	}

	async fn post(&self, msg: Msg) -> Result<(), ServiceError> {
		self.tx.send(msg).await.map_err(|_| ServiceError::Stopped)
	}

	/// Sends a message that expects an answer. Closing-only inputs may wait
	/// for space; everything else is refused with [`ServiceError::Busy`].
	async fn ask<T>(
		&self,
		msg: impl FnOnce(Reply<T>) -> Msg,
		may_wait: bool,
	) -> Result<T, ServiceError> {
		let (reply, answer) = oneshot::channel();
		let msg = msg(reply);
		if may_wait {
			self.post(msg).await?;
		} else {
			self.tx.try_send(msg).map_err(|error| match error {
				mpsc::error::TrySendError::Full(_) => ServiceError::Busy,
				mpsc::error::TrySendError::Closed(_) => ServiceError::Stopped,
			})?;
		}
		answer.await.map_err(|_| ServiceError::Stopped)
	}

	/// Submits a client request to the domain.
	pub async fn submit(&self, request: Request) -> Result<Applied, ServiceError> {
		let may_wait = Input::Client(request.clone()).is_safety_deny();
		self.ask(|reply| Msg::Submit { request, reply }, may_wait)
			.await?
	}

	/// Runs an adapter-executed operation with idempotency.
	pub async fn service_op(
		&self,
		client: ClientId,
		operation: OperationId,
		print: Fingerprint,
		op: ServiceOp,
	) -> Result<Operation, ServiceError> {
		self.ask(
			|reply| Msg::Service {
				client,
				operation,
				print,
				op,
				reply,
			},
			false,
		)
		.await?
	}

	/// A consistent snapshot.
	pub async fn snapshot(&self) -> Result<Snapshot, ServiceError> {
		self.ask(Msg::Snapshot, false).await
	}

	/// The recorded outcome of an operation, if still retained.
	pub async fn operation(
		&self,
		client: ClientId,
		operation: OperationId,
	) -> Result<Option<Operation>, ServiceError> {
		self.ask(
			|reply| Msg::Operation {
				client,
				operation,
				reply,
			},
			false,
		)
		.await
	}

	/// Opens a watch positioned atomically against the event sequence.
	pub async fn subscribe(&self, after: Option<u64>) -> Result<Subscribed, ServiceError> {
		self.ask(|reply| Msg::Subscribe { after, reply }, false)
			.await
	}
}
