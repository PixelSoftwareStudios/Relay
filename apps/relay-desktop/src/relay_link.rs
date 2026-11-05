//! Real engine adapter over `relay_api::RelayClient`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use relay_api::pb::{self, OperationState};
use relay_api::{ApiError, BoundedWatch, Endpoint, RelayClient, WatchItem};
use relay_client::operation::settle;
use relay_client::wire::{acquire_generation, connect_registered, open_telemetry, send};
use relay_client::{CallAction, CallTarget, Command as WireCommand};
use tokio::sync::Mutex;
use tonic::Code;

use crate::convert::mode_to_wire;
use crate::engine::{
	Command, Connection, Engine, EngineError, EventSource, Operations, Outcome, TelemetrySource,
};

const CLIENT_NAME: &str = "relay-desktop";
const CAPABILITIES: &[&str] = &["observe", "call-control"];
/// Local subscriber queue; overflow ends the watch and triggers a resync.
const WATCH_QUEUE: usize = 256;
const METERS_HZ: u32 = 10;
const STATS_HZ: u32 = 1;
const OP_POLL: Duration = Duration::from_millis(50);
const OP_TIMEOUT: Duration = Duration::from_secs(5);

type EndpointFn = dyn Fn() -> Result<Endpoint, EngineError> + Send + Sync;

pub struct RelayLink {
	endpoint: Box<EndpointFn>,
}

impl RelayLink {
	/// The endpoint is resolved on every connect so a restarted daemon is found.
	pub fn new(
		endpoint: impl Fn() -> Result<Endpoint, EngineError> + Send + Sync + 'static,
	) -> Self {
		Self {
			endpoint: Box::new(endpoint),
		}
	}
}

fn map_error(error: ApiError) -> EngineError {
	match error {
		ApiError::Io(_)
		| ApiError::Transport(_)
		| ApiError::BadProfile(_)
		| ApiError::InsecurePath { .. } => EngineError::Unavailable(error.to_string()),
		ApiError::Status(status) => match status.code() {
			Code::Unavailable => EngineError::Unavailable(status.message().to_owned()),
			Code::DeadlineExceeded => EngineError::Timeout,
			code => EngineError::Rejected {
				code: format!("{code:?}"),
				message: status.message().to_owned(),
			},
		},
		ApiError::OperationTimeout => EngineError::Timeout,
		other => EngineError::Broken(other.to_string()),
	}
}

fn status(s: tonic::Status) -> EngineError {
	map_error(s.into())
}

fn problem_error(problem: Option<pb::Problem>) -> EngineError {
	let p = problem.unwrap_or_default();
	let message = if p.message.is_empty() {
		"the engine rejected the request".to_owned()
	} else {
		p.message
	};
	let message = if p.recovery_action.is_empty() {
		message
	} else {
		format!("{message} ({})", p.recovery_action)
	};
	EngineError::Rejected {
		code: p.code,
		message,
	}
}

#[async_trait]
impl Engine for RelayLink {
	async fn connect(&self) -> Result<Connection, EngineError> {
		let endpoint = (self.endpoint)()?;
		let mut client = connect_registered(
			&endpoint,
			CLIENT_NAME,
			env!("CARGO_PKG_VERSION"),
			CAPABILITIES,
		)
		.await
		.map_err(map_error)?;
		let context = client
			.context()
			.ok_or_else(|| EngineError::Broken("no client context".into()))?;
		let (snapshot, watch) = client.snapshot_and_watch().await.map_err(map_error)?;
		let telemetry = open_stream(&mut client).await;
		Ok(Connection {
			info: client.info().clone(),
			client_id: context.client_id,
			snapshot,
			watch: Box::new(Watch(watch.into_bounded(WATCH_QUEUE))),
			telemetry,
			ops: Arc::new(Ops {
				client: Mutex::new(client),
			}),
		})
	}
}

/// Telemetry is optional: without it the UI shows "no signal data".
async fn open_stream(client: &mut RelayClient) -> Option<Box<dyn TelemetrySource>> {
	match open_telemetry(client, METERS_HZ, STATS_HZ).await {
		Ok(stream) => Some(Box::new(Telemetry(stream))),
		Err(error) => {
			tracing::info!(%error, "telemetry unavailable; meters stay empty");
			None
		}
	}
}

struct Watch(BoundedWatch);

#[async_trait]
impl EventSource for Watch {
	async fn next(&mut self) -> Result<Option<WatchItem>, EngineError> {
		match self.0.recv().await {
			Some(Ok(item)) => Ok(Some(item)),
			Some(Err(e)) => Err(map_error(e)),
			None => Ok(None),
		}
	}
}

struct Telemetry(tonic::Streaming<pb::TelemetrySnapshot>);

#[async_trait]
impl TelemetrySource for Telemetry {
	async fn next(&mut self) -> Result<Option<pb::TelemetrySnapshot>, EngineError> {
		self.0.message().await.map_err(status)
	}
}

struct Ops {
	client: Mutex<RelayClient>,
}

impl Ops {
	/// Poll until the engine settles the operation. The lock is held per call
	/// only, so slow operations never block other commands.
	async fn settle(&self, operation: pb::Operation) -> Result<Outcome, EngineError> {
		let operation = settle(&mut &self.client, operation, OP_POLL, OP_TIMEOUT)
			.await
			.map_err(map_error)?;
		match OperationState::try_from(operation.state) {
			Ok(OperationState::Applied) => Ok(Outcome::default()),
			_ => Err(problem_error(operation.problem)),
		}
	}
}

/// The shared wire command for a UI command that mutates the engine.
fn wire_command(command: Command) -> Option<WireCommand> {
	Some(match command {
		Command::AcquireController { .. } => return None,
		Command::StartCall { peer_id } => WireCommand::StartCall(CallTarget::Peer(peer_id)),
		Command::AcceptCall {
			call_id,
			generation,
		} => call_action(CallAction::Accept, call_id, generation),
		Command::DeclineCall {
			call_id,
			generation,
		} => call_action(CallAction::Decline, call_id, generation),
		Command::EndCall {
			call_id,
			generation,
		} => call_action(CallAction::End, call_id, generation),
		Command::SetTransmit {
			call_id,
			generation,
			mode,
			consent,
		} => WireCommand::SetTransmit {
			call_id,
			generation,
			mode: mode_to_wire(mode),
			consent,
		},
		Command::SetCallControls {
			call_id,
			generation,
			muted,
			deafened,
			receive_gain,
		} => WireCommand::SetCallControls {
			call_id,
			generation,
			muted,
			deafened,
			receive_gain,
		},
		Command::SetGlobalControls {
			muted,
			deafened,
			microphone_gain,
			playback_gain,
		} => WireCommand::SetGlobalControls {
			muted,
			deafened,
			microphone_gain,
			playback_gain,
		},
		Command::SelectDevices {
			input_id,
			output_id,
		} => WireCommand::SelectDevices {
			input_id,
			output_id,
		},
		Command::SetProfile { display_name } => WireCommand::SetProfile { display_name },
		Command::UploadAvatar { bytes, media_type } => {
			WireCommand::UploadAvatar { bytes, media_type }
		}
	})
}

fn call_action(action: CallAction, call_id: Vec<u8>, generation: u64) -> WireCommand {
	WireCommand::CallAction {
		action,
		call_id,
		generation,
	}
}

#[async_trait]
impl Operations for Ops {
	async fn run(&self, command: Command) -> Result<Outcome, EngineError> {
		let Some(wire) = wire_command(command.clone()) else {
			let Command::AcquireController { call_id } = command else {
				unreachable!("only AcquireController has no wire command");
			};
			let generation = acquire_generation(&mut *self.client.lock().await, &call_id, false)
				.await
				.map_err(map_error)?;
			return Ok(Outcome {
				generation: Some(generation),
			});
		};
		let operation = send(&mut *self.client.lock().await, wire)
			.await
			.map_err(map_error)?;
		self.settle(operation).await
	}
}

/// Endpoint choice shared with the CLI: `RELAY_TCP_PROFILE`, `RELAY_SOCKET`,
/// else the per-user default. Call from inside the tokio runtime.
pub fn resolve_endpoint() -> Result<Endpoint, EngineError> {
	relay_client::endpoint::resolve().map_err(|e| EngineError::Unavailable(e.to_string()))
}
