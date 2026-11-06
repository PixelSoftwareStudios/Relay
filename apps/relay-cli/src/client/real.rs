//! [`Control`] and [`Subscribe`] over the real `RelayClient`.

use std::collections::HashMap;
use std::time::Duration;

use relay_api::pb::{
	EngineInfo, InputAction, InputControlMessage, Operation, Snapshot, TelemetrySnapshot,
	TransmitMode,
};
use relay_api::{ApiError, Endpoint, RelayClient, StateWatch, WatchItem, WatchOptions};
use relay_client::operation::settle;
use relay_client::wire::{acquire_generation, connect_registered, open_telemetry, send};
use relay_client::{CallAction, CallTarget, Command};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tonic::Streaming;

use super::{
	CallControls, Control, Feed, GlobalControls, PttAction, PttRequest, Subscribe, TrustDecision,
};
use crate::cli::GlobalArgs;

/// Lease length requested for push-to-talk presses.
const PTT_LEASE_MS: u32 = 400;
pub(super) const POLL_INTERVAL: Duration = Duration::from_millis(50);
const CLIENT_NAME: &str = "relay-cli";
const CAPABILITIES: &[&str] = &["observe", "call-control"];
/// Messaging commands ask for nothing else: least privilege.
const MESSAGES_CAPABILITIES: &[&str] = &["messages"];

/// A registered connection to the engine.
pub struct RealClient {
	client: RelayClient,
	endpoint: Endpoint,
	takeover: bool,
	timeout: Duration,
	capabilities: &'static [&'static str],
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
		Self::open(
			endpoint,
			global.takeover,
			Duration::from_millis(global.timeout_ms),
			CAPABILITIES,
		)
		.await
	}

	/// Connect and register with the `messages` capability only.
	pub async fn connect_messages(
		endpoint: &Endpoint,
		global: &GlobalArgs,
	) -> Result<Self, ApiError> {
		Self::open(
			endpoint,
			false,
			Duration::from_millis(global.timeout_ms),
			MESSAGES_CAPABILITIES,
		)
		.await
	}

	/// Open a second registered connection to the same engine.
	pub async fn duplicate(&self) -> Result<Self, ApiError> {
		Self::open(
			&self.endpoint,
			self.takeover,
			self.timeout,
			self.capabilities,
		)
		.await
	}

	async fn open(
		endpoint: &Endpoint,
		takeover: bool,
		timeout: Duration,
		capabilities: &'static [&'static str],
	) -> Result<Self, ApiError> {
		let client = connect_registered(
			endpoint,
			CLIENT_NAME,
			env!("CARGO_PKG_VERSION"),
			capabilities,
		)
		.await?;
		Ok(Self {
			client,
			endpoint: endpoint.clone(),
			takeover,
			timeout,
			capabilities,
			input: None,
		})
	}

	/// The underlying API client, for RPCs without a shared command.
	pub fn client_mut(&mut self) -> &mut RelayClient {
		&mut self.client
	}

	/// How long operations may take to settle.
	pub(super) fn timeout(&self) -> Duration {
		self.timeout
	}

	/// Send a command and wait for it to leave `ACCEPTED`.
	pub(super) async fn run_command(&mut self, command: Command) -> Result<Operation, ApiError> {
		let op = send(&mut self.client, command).await?;
		settle(&mut self.client, op, POLL_INTERVAL, self.timeout).await
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
				source_id: relay_io::entropy::random_bytes::<16>().to_vec(),
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

	async fn controller_generation(&mut self, call_id: &[u8]) -> Result<u64, ApiError> {
		acquire_generation(&mut self.client, call_id, self.takeover).await
	}

	async fn snapshot(&mut self) -> Result<Snapshot, ApiError> {
		self.client.snapshot().await
	}

	async fn start_call(&mut self, target: CallTarget) -> Result<Operation, ApiError> {
		self.run_command(Command::StartCall(target)).await
	}

	async fn call_action(
		&mut self,
		action: CallAction,
		call_id: &[u8],
	) -> Result<Operation, ApiError> {
		// Ending takes ownership when another client holds the call: the
		// person asking for it to end means it.
		let takeover = self.takeover || matches!(action, CallAction::End);
		let generation = acquire_generation(&mut self.client, call_id, takeover).await?;
		self.run_command(Command::CallAction {
			action,
			call_id: call_id.to_vec(),
			generation,
		})
		.await
	}

	async fn set_transmit_mode(
		&mut self,
		call_id: &[u8],
		mode: TransmitMode,
	) -> Result<Operation, ApiError> {
		let generation = self.controller_generation(call_id).await?;
		self.run_command(Command::SetTransmit {
			call_id: call_id.to_vec(),
			generation,
			mode,
		})
		.await
	}

	async fn set_call_controls(
		&mut self,
		call_id: &[u8],
		controls: CallControls,
	) -> Result<Operation, ApiError> {
		let generation = self.controller_generation(call_id).await?;
		self.run_command(Command::SetCallControls {
			call_id: call_id.to_vec(),
			generation,
			muted: controls.muted,
			deafened: controls.deafened,
			receive_gain: controls.receive_gain,
		})
		.await
	}

	async fn set_global_controls(
		&mut self,
		controls: GlobalControls,
	) -> Result<Operation, ApiError> {
		let ring = if controls.has_ring() {
			Some(
				self.run_command(Command::SetRingPreferences {
					ringtone_enabled: controls.ringtone_enabled,
					ringtone_volume: controls.ringtone_volume,
					ringback_enabled: controls.ringback_enabled,
					call_sounds_enabled: controls.call_sounds_enabled,
				})
				.await?,
			)
		} else {
			None
		};
		if let Some(ring) = ring
			&& !controls.has_other()
		{
			return Ok(ring);
		}
		self.run_command(Command::SetGlobalControls {
			muted: controls.muted,
			deafened: controls.deafened,
			microphone_gain: controls.microphone_gain,
			playback_gain: controls.playback_gain,
			ptt_release_delay_ms: controls.ptt_release_delay_ms,
		})
		.await
	}

	async fn select_devices(&mut self, input: &str, output: &str) -> Result<Operation, ApiError> {
		self.run_command(Command::SelectDevices {
			input_id: input.to_owned(),
			output_id: output.to_owned(),
		})
		.await
	}

	async fn set_noise_suppression(&mut self, backend: &str) -> Result<Operation, ApiError> {
		self.run_command(Command::SetNoiseSuppression {
			backend: backend.to_owned(),
		})
		.await
	}

	async fn set_profile(&mut self, display_name: &str) -> Result<Operation, ApiError> {
		self.run_command(Command::SetProfile {
			display_name: display_name.to_owned(),
		})
		.await
	}

	async fn start_mic_test(
		&mut self,
		local_playback: bool,
		confirmed_feedback: bool,
	) -> Result<Operation, ApiError> {
		self.run_command(Command::StartMicTest {
			local_playback,
			confirmed_feedback,
		})
		.await
	}

	async fn stop_mic_test(&mut self) -> Result<Operation, ApiError> {
		self.run_command(Command::StopMicTest).await
	}

	async fn connect_and_trust(&mut self, connection_string: &str) -> Result<Operation, ApiError> {
		self.run_command(Command::ConnectAndTrust {
			connection_string: connection_string.to_owned(),
		})
		.await
	}

	async fn decide_trust(&mut self, decision: TrustDecision) -> Result<Operation, ApiError> {
		self.run_command(Command::DecideTrust {
			peer_id: decision.peer_id,
			public_key_sha256: decision.public_key_sha256,
			requested: decision.requested,
			out_of_band_verified: decision.out_of_band_verified,
		})
		.await
	}

	async fn ptt(&mut self, request: PttRequest) -> Result<(), ApiError> {
		let client = self.client.context().ok_or(ApiError::NotRegistered)?;
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
		open_telemetry(&mut self.client, meters_hz, 1)
			.await
			.map(RealTelemetry)
	}
}
