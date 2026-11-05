//! [`Control`] and [`Subscribe`] over the real `RelayClient`.

use std::collections::HashMap;
use std::time::Duration;

use relay_api::pb::{
	AcquireControllerRequest, CallMutation, ClientContext, DecidePeerTrustRequest, EngineInfo,
	InputAction, InputControlMessage, MutationContext, Operation, OperationState,
	SelectDevicesRequest, SetCallControlsRequest, SetGlobalControlsRequest,
	SetNoiseSuppressionRequest, SetProfileRequest, SetTransmitRouteRequest, Snapshot,
	StartCallRequest, StartMicTestRequest, TelemetryRequest, TelemetrySnapshot, TransmitMode,
	start_call_request,
};
use relay_api::{ApiError, Endpoint, RelayClient, StateWatch, WatchItem, WatchOptions};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Streaming;

use super::{
	CallAction, CallControls, CallTarget, Control, Feed, GlobalControls, PttAction, PttRequest,
	Subscribe, TrustDecision,
};
use crate::cli::GlobalArgs;

/// Lease length requested for push-to-talk presses.
const PTT_LEASE_MS: u32 = 400;
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// A registered connection to the engine.
pub struct RealClient {
	client: RelayClient,
	endpoint: Endpoint,
	takeover: bool,
	timeout: Duration,
	input: Option<InputStream>,
}

struct InputStream {
	tx: mpsc::Sender<InputControlMessage>,
	source_id: Vec<u8>,
	sequence: u64,
	generations: HashMap<Vec<u8>, u64>,
}

impl RealClient {
	/// Connect and register as `relay-cli`.
	pub async fn connect(endpoint: &Endpoint, global: &GlobalArgs) -> Result<Self, ApiError> {
		let mut client = RelayClient::connect(endpoint).await?;
		client
			.register(
				"relay-cli",
				env!("CARGO_PKG_VERSION"),
				&["observe", "call-control"],
			)
			.await?;
		Ok(Self {
			client,
			endpoint: endpoint.clone(),
			takeover: global.takeover,
			timeout: Duration::from_millis(global.timeout_ms),
			input: None,
		})
	}

	/// Open a second registered connection to the same engine.
	pub async fn duplicate(&self) -> Result<Self, ApiError> {
		let mut client = RelayClient::connect(&self.endpoint).await?;
		client
			.register(
				"relay-cli",
				env!("CARGO_PKG_VERSION"),
				&["observe", "call-control"],
			)
			.await?;
		Ok(Self {
			client,
			endpoint: self.endpoint.clone(),
			takeover: self.takeover,
			timeout: self.timeout,
			input: None,
		})
	}

	fn client_context(&self) -> Result<ClientContext, ApiError> {
		self.client.context().ok_or(ApiError::NotRegistered)
	}

	fn mutation(&self) -> Result<MutationContext, ApiError> {
		Ok(MutationContext {
			client: Some(self.client_context()?),
			operation_id: uuid::Uuid::new_v4().as_bytes().to_vec(),
			expected_revision: None,
		})
	}

	/// Wait for an accepted operation to leave `ACCEPTED`.
	async fn settle(&mut self, op: Operation) -> Result<Operation, ApiError> {
		if op.state != OperationState::Accepted as i32 {
			return Ok(op);
		}
		let id = op.operation_id.clone();
		self.client
			.wait_operation(&id, POLL_INTERVAL, self.timeout)
			.await
	}

	/// Take (or keep) the controller lease and return its generation.
	async fn controller_generation(&mut self, call_id: &[u8]) -> Result<u64, ApiError> {
		let request = AcquireControllerRequest {
			context: Some(self.mutation()?),
			call_id: call_id.to_vec(),
			request_explicit_takeover: self.takeover,
		};
		let lease = self
			.client
			.raw()
			.acquire_controller(request)
			.await?
			.into_inner();
		Ok(lease.generation)
	}

	async fn call_mutation(&mut self, call_id: &[u8]) -> Result<CallMutation, ApiError> {
		let generation = self.controller_generation(call_id).await?;
		Ok(CallMutation {
			context: Some(self.mutation()?),
			call_id: call_id.to_vec(),
			controller_generation: generation,
		})
	}

	async fn input_stream(&mut self) -> Result<&mut InputStream, ApiError> {
		if self.input.is_none() {
			let (tx, rx) = mpsc::channel(16);
			let mut acks = self
				.client
				.raw()
				.input_control(ReceiverStream::new(rx))
				.await?
				.into_inner();
			// Acks are advisory here; effective state arrives via the watch.
			tokio::spawn(async move { while let Ok(Some(_)) = acks.message().await {} });
			self.input = Some(InputStream {
				tx,
				source_id: uuid::Uuid::new_v4().as_bytes().to_vec(),
				sequence: 0,
				generations: HashMap::new(),
			});
		}
		self.input.as_mut().ok_or(ApiError::StreamClosed)
	}
}

impl Control for RealClient {
	fn info(&self) -> &EngineInfo {
		self.client.info()
	}

	async fn snapshot(&mut self) -> Result<Snapshot, ApiError> {
		self.client.snapshot().await
	}

	async fn start_call(&mut self, target: CallTarget) -> Result<Operation, ApiError> {
		let target = match target {
			CallTarget::Peer(id) => start_call_request::Target::PeerId(id),
			CallTarget::Address(addr) => start_call_request::Target::ManualLocalAddress(addr),
		};
		let request = StartCallRequest {
			context: Some(self.mutation()?),
			target: Some(target),
			allow_parallel_same_peer: false,
		};
		let op = self.client.raw().start_call(request).await?.into_inner();
		self.settle(op).await
	}

	async fn call_action(
		&mut self,
		action: CallAction,
		call_id: &[u8],
	) -> Result<Operation, ApiError> {
		let request = self.call_mutation(call_id).await?;
		let raw = self.client.raw();
		let op = match action {
			CallAction::Accept => raw.accept_call(request).await?,
			CallAction::Decline => raw.decline_call(request).await?,
			CallAction::End => raw.end_call(request).await?,
		}
		.into_inner();
		self.settle(op).await
	}

	async fn set_route(
		&mut self,
		call_id: &[u8],
		mode: TransmitMode,
		consent: bool,
	) -> Result<Operation, ApiError> {
		let request = SetTransmitRouteRequest {
			context: Some(self.mutation()?),
			call_id: call_id.to_vec(),
			controller_generation: self.controller_generation(call_id).await?,
			mode: mode as i32,
			explicit_microphone_consent: consent,
			authorize_background_open_mic: false,
		};
		let op = self
			.client
			.raw()
			.set_transmit_route(request)
			.await?
			.into_inner();
		self.settle(op).await
	}

	async fn set_call_controls(
		&mut self,
		call_id: &[u8],
		controls: CallControls,
	) -> Result<Operation, ApiError> {
		let request = SetCallControlsRequest {
			context: Some(self.mutation()?),
			call_id: call_id.to_vec(),
			controller_generation: self.controller_generation(call_id).await?,
			muted: controls.muted,
			deafened: controls.deafened,
			receive_gain: controls.receive_gain,
		};
		let op = self
			.client
			.raw()
			.set_call_controls(request)
			.await?
			.into_inner();
		self.settle(op).await
	}

	async fn set_global_controls(
		&mut self,
		controls: GlobalControls,
	) -> Result<Operation, ApiError> {
		let request = SetGlobalControlsRequest {
			context: Some(self.mutation()?),
			muted: controls.muted,
			deafened: controls.deafened,
			microphone_gain: controls.microphone_gain,
			playback_gain: controls.playback_gain,
		};
		let op = self
			.client
			.raw()
			.set_global_controls(request)
			.await?
			.into_inner();
		self.settle(op).await
	}

	async fn select_devices(&mut self, input: &str, output: &str) -> Result<Operation, ApiError> {
		let request = SelectDevicesRequest {
			context: Some(self.mutation()?),
			input_device_id: input.to_owned(),
			output_device_id: output.to_owned(),
		};
		let op = self
			.client
			.raw()
			.select_devices(request)
			.await?
			.into_inner();
		self.settle(op).await
	}

	async fn set_noise_suppression(&mut self, backend: &str) -> Result<Operation, ApiError> {
		let request = SetNoiseSuppressionRequest {
			context: Some(self.mutation()?),
			qualified_backend_id: backend.to_owned(),
		};
		let op = self
			.client
			.raw()
			.set_noise_suppression(request)
			.await?
			.into_inner();
		self.settle(op).await
	}

	async fn set_profile(&mut self, display_name: &str) -> Result<Operation, ApiError> {
		let request = SetProfileRequest {
			context: Some(self.mutation()?),
			display_name: display_name.to_owned(),
		};
		let op = self.client.raw().set_profile(request).await?.into_inner();
		self.settle(op).await
	}

	async fn start_mic_test(
		&mut self,
		local_playback: bool,
		confirmed_feedback: bool,
	) -> Result<Operation, ApiError> {
		let request = StartMicTestRequest {
			context: Some(self.mutation()?),
			local_playback,
			confirmed_feedback_warning: confirmed_feedback,
		};
		let op = self
			.client
			.raw()
			.start_mic_test(request)
			.await?
			.into_inner();
		self.settle(op).await
	}

	async fn stop_mic_test(&mut self) -> Result<Operation, ApiError> {
		let context = self.mutation()?;
		let op = self.client.raw().stop_mic_test(context).await?.into_inner();
		self.settle(op).await
	}

	async fn decide_trust(&mut self, decision: TrustDecision) -> Result<Operation, ApiError> {
		let request = DecidePeerTrustRequest {
			context: Some(self.mutation()?),
			peer_id: decision.peer_id,
			expected_public_key_sha256: decision.public_key_sha256,
			requested_trust: decision.requested as i32,
			out_of_band_verified: decision.out_of_band_verified,
		};
		let op = self
			.client
			.raw()
			.decide_peer_trust(request)
			.await?
			.into_inner();
		self.settle(op).await
	}

	async fn ptt(&mut self, request: PttRequest) -> Result<(), ApiError> {
		let client = self.client_context()?;
		let known = self
			.input
			.as_ref()
			.and_then(|s| s.generations.get(&request.call_id).copied());
		let generation = match known {
			Some(generation) => generation,
			None => self.controller_generation(&request.call_id).await?,
		};
		let stream = self.input_stream().await?;
		stream
			.generations
			.insert(request.call_id.clone(), generation);
		stream.sequence += 1;
		let (action, lease) = match request.action {
			PttAction::Press => (InputAction::Press, PTT_LEASE_MS),
			PttAction::Renew => (InputAction::Renew, PTT_LEASE_MS),
			PttAction::Release => (InputAction::Release, 0),
		};
		let message = InputControlMessage {
			client: Some(client),
			source_id: stream.source_id.clone(),
			input_sequence: stream.sequence,
			call_id: request.call_id,
			controller_generation: generation,
			transmit_route_generation: request.route_generation,
			action: action as i32,
			requested_lease_ms: lease,
		};
		stream
			.tx
			.send(message)
			.await
			.map_err(|_| ApiError::StreamClosed)
	}
}

/// State watch adapter.
pub struct RealWatch(StateWatch);

impl Feed for RealWatch {
	type Item = WatchItem;
	async fn next(&mut self) -> Result<Option<WatchItem>, ApiError> {
		self.0.next().await
	}
}

/// Telemetry stream adapter.
pub struct RealTelemetry(Streaming<TelemetrySnapshot>);

impl Feed for RealTelemetry {
	type Item = TelemetrySnapshot;
	async fn next(&mut self) -> Result<Option<TelemetrySnapshot>, ApiError> {
		Ok(self.0.message().await?)
	}
}

impl Subscribe for RealClient {
	type Watch = RealWatch;
	type Telemetry = RealTelemetry;

	async fn open_watch(&mut self) -> Result<RealWatch, ApiError> {
		self.client
			.watch(WatchOptions::default())
			.await
			.map(RealWatch)
	}

	async fn open_telemetry(&mut self, meters_hz: u32) -> Result<RealTelemetry, ApiError> {
		let request = TelemetryRequest {
			client: Some(self.client_context()?),
			call_ids: Vec::new(),
			meters_hz,
			stats_hz: 1,
		};
		let stream = self
			.client
			.raw()
			.watch_telemetry(request)
			.await?
			.into_inner();
		Ok(RealTelemetry(stream))
	}
}
