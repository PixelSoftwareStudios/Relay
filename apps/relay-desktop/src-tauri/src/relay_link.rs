//! Real engine adapter over `relay_api::RelayClient`.

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use relay_api::pb::{self, OperationState};
use relay_api::{ApiError, BoundedWatch, Endpoint, RelayClient, WatchItem};
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
		let mut client = RelayClient::connect(&endpoint).await.map_err(map_error)?;
		client
			.register(CLIENT_NAME, env!("CARGO_PKG_VERSION"), CAPABILITIES)
			.await
			.map_err(map_error)?;
		let context = client
			.context()
			.ok_or_else(|| EngineError::Broken("no client context".into()))?;
		let (snapshot, watch) = client.snapshot_and_watch().await.map_err(map_error)?;
		let telemetry = open_telemetry(&mut client, context.clone()).await;
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
async fn open_telemetry(
	client: &mut RelayClient,
	context: pb::ClientContext,
) -> Option<Box<dyn TelemetrySource>> {
	let request = pb::TelemetryRequest {
		client: Some(context),
		call_ids: Vec::new(),
		meters_hz: METERS_HZ,
		stats_hz: STATS_HZ,
	};
	match client.raw().watch_telemetry(request).await {
		Ok(stream) => Some(Box::new(Telemetry(stream.into_inner()))),
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
	/// Fresh mutation context; the id makes a retried request idempotent.
	async fn mutation(&self) -> Result<pb::MutationContext, EngineError> {
		let client = self
			.client
			.lock()
			.await
			.context()
			.ok_or_else(|| EngineError::Broken("no client context".into()))?;
		Ok(pb::MutationContext {
			client: Some(client),
			operation_id: uuid::Uuid::new_v4().as_bytes().to_vec(),
			expected_revision: None,
		})
	}

	/// Poll until the engine settles the operation. The lock is held per call
	/// only, so slow operations never block other commands.
	async fn settle(&self, mut operation: pb::Operation) -> Result<Outcome, EngineError> {
		let deadline = tokio::time::Instant::now() + OP_TIMEOUT;
		while operation.state == OperationState::Accepted as i32 {
			if tokio::time::Instant::now() >= deadline {
				return Err(EngineError::Timeout);
			}
			tokio::time::sleep(OP_POLL).await;
			let id = operation.operation_id.clone();
			operation = self
				.client
				.lock()
				.await
				.get_operation(&id)
				.await
				.map_err(map_error)?;
		}
		match OperationState::try_from(operation.state) {
			Ok(OperationState::Applied) => Ok(Outcome::default()),
			_ => Err(problem_error(operation.problem)),
		}
	}

	async fn send(
		&self,
		command: Command,
		context: pb::MutationContext,
	) -> Result<pb::Operation, EngineError> {
		let mut client = self.client.lock().await;
		let rpc = client.raw();
		let response = match command {
			Command::StartCall { peer_id } => {
				rpc.start_call(pb::StartCallRequest {
					context: Some(context),
					target: Some(pb::start_call_request::Target::PeerId(peer_id)),
					allow_parallel_same_peer: false,
				})
				.await
			}
			Command::AcceptCall {
				call_id,
				generation,
			} => {
				rpc.accept_call(call_mutation(context, call_id, generation))
					.await
			}
			Command::DeclineCall {
				call_id,
				generation,
			} => {
				rpc.decline_call(call_mutation(context, call_id, generation))
					.await
			}
			Command::EndCall {
				call_id,
				generation,
			} => {
				rpc.end_call(call_mutation(context, call_id, generation))
					.await
			}
			Command::SetTransmit {
				call_id,
				generation,
				mode,
				consent,
			} => {
				rpc.set_transmit_route(pb::SetTransmitRouteRequest {
					context: Some(context),
					call_id,
					controller_generation: generation,
					mode: mode_to_wire(mode) as i32,
					explicit_microphone_consent: consent,
					authorize_background_open_mic: false,
				})
				.await
			}
			Command::SetCallControls {
				call_id,
				generation,
				muted,
				deafened,
				receive_gain,
			} => {
				rpc.set_call_controls(pb::SetCallControlsRequest {
					context: Some(context),
					call_id,
					controller_generation: generation,
					muted,
					deafened,
					receive_gain,
				})
				.await
			}
			Command::SetGlobalControls {
				muted,
				deafened,
				microphone_gain,
				playback_gain,
			} => {
				rpc.set_global_controls(pb::SetGlobalControlsRequest {
					context: Some(context),
					muted,
					deafened,
					microphone_gain,
					playback_gain,
				})
				.await
			}
			Command::SelectDevices {
				input_id,
				output_id,
			} => {
				rpc.select_devices(pb::SelectDevicesRequest {
					context: Some(context),
					input_device_id: input_id,
					output_device_id: output_id,
				})
				.await
			}
			Command::SetProfile { display_name } => {
				rpc.set_profile(pb::SetProfileRequest {
					context: Some(context),
					display_name,
				})
				.await
			}
			Command::UploadAvatar { bytes, media_type } => {
				rpc.upload_avatar(pb::UploadAvatarRequest {
					context: Some(context),
					encoded_image: bytes,
					declared_media_type: media_type,
				})
				.await
			}
			Command::AcquireController { .. } => unreachable!("handled by run"),
		};
		Ok(response.map_err(status)?.into_inner())
	}
}

#[async_trait]
impl Operations for Ops {
	async fn run(&self, command: Command) -> Result<Outcome, EngineError> {
		let context = self.mutation().await?;
		if let Command::AcquireController { call_id } = command {
			let lease = self
				.client
				.lock()
				.await
				.raw()
				.acquire_controller(pb::AcquireControllerRequest {
					context: Some(context),
					call_id,
					request_explicit_takeover: false,
				})
				.await
				.map_err(status)?
				.into_inner();
			return Ok(Outcome {
				generation: Some(lease.generation),
			});
		}
		let operation = self.send(command, context).await?;
		self.settle(operation).await
	}
}

fn call_mutation(
	context: pb::MutationContext,
	call_id: Vec<u8>,
	generation: u64,
) -> pb::CallMutation {
	pb::CallMutation {
		context: Some(context),
		call_id,
		controller_generation: generation,
	}
}

/// Endpoint choice shared with the CLI: `RELAY_TCP_PROFILE`, `RELAY_SOCKET`,
/// else the per-user default. Call from inside the tokio runtime.
pub fn resolve_endpoint() -> Result<Endpoint, EngineError> {
	if let Some(profile_path) = std::env::var_os("RELAY_TCP_PROFILE") {
		return Ok(Endpoint::LoopbackTcp {
			profile_path: profile_path.into(),
		});
	}
	#[cfg(unix)]
	{
		if let Some(path) = std::env::var_os("RELAY_SOCKET") {
			return Ok(Endpoint::Unix(path.into()));
		}
		let uid = relay_api::current_uid().map_err(|e| EngineError::Unavailable(e.to_string()))?;
		Ok(Endpoint::Unix(relay_api::default_socket_path(
			std::env::var_os("XDG_RUNTIME_DIR"),
			&std::env::temp_dir(),
			uid,
		)))
	}
	#[cfg(not(unix))]
	{
		let dir = std::env::temp_dir().join("relay");
		Ok(Endpoint::LoopbackTcp {
			profile_path: relay_api::profile_path(&dir),
		})
	}
}
