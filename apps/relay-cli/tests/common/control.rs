//! A scripted `Control`/`Subscribe` that records what it was asked to do.

use std::collections::VecDeque;

use relay_api::pb::*;
use relay_api::{ApiError, WatchItem};
use relay_cli::client::*;

use super::fixtures;

/// Items a scripted stream yields, then `Ok(None)`.
pub struct Script<T>(pub VecDeque<Result<T, ApiError>>);

impl<T: Send> Feed for Script<T> {
	type Item = T;
	async fn next(&mut self) -> Result<Option<T>, ApiError> {
		self.0.pop_front().transpose()
	}
}

pub struct FakeControl {
	pub info: EngineInfo,
	pub snapshot: Snapshot,
	pub log: Vec<String>,
	pub result: Operation,
	/// One entry per `open_watch` call; an exhausted list makes it fail.
	pub watches: VecDeque<Vec<Result<WatchItem, ApiError>>>,
	pub telemetry: Vec<TelemetrySnapshot>,
}

impl FakeControl {
	pub fn new() -> Self {
		Self {
			info: EngineInfo {
				application_version: "9.9.9".into(),
				api_major: 1,
				..EngineInfo::default()
			},
			snapshot: fixtures::snapshot(),
			log: Vec::new(),
			result: Operation {
				operation_id: vec![9; 16],
				state: OperationState::Applied as i32,
				revision: 11,
				..Operation::default()
			},
			watches: VecDeque::new(),
			telemetry: Vec::new(),
		}
	}

	fn record(&mut self, line: impl Into<String>) -> Result<Operation, ApiError> {
		self.log.push(line.into());
		Ok(self.result.clone())
	}
}

impl Control for FakeControl {
	fn info(&self) -> &EngineInfo {
		&self.info
	}
	async fn snapshot(&mut self) -> Result<Snapshot, ApiError> {
		Ok(self.snapshot.clone())
	}
	async fn start_call(&mut self, target: CallTarget) -> Result<Operation, ApiError> {
		self.record(format!("start_call {target:?}"))
	}
	async fn call_action(
		&mut self,
		action: CallAction,
		call_id: &[u8],
	) -> Result<Operation, ApiError> {
		self.record(format!("{action:?} {}", hex::encode(call_id)))
	}
	async fn set_route(
		&mut self,
		call_id: &[u8],
		mode: TransmitMode,
		consent: bool,
	) -> Result<Operation, ApiError> {
		self.record(format!(
			"route {} {mode:?} consent={consent}",
			hex::encode(call_id)
		))
	}
	async fn set_call_controls(
		&mut self,
		call_id: &[u8],
		c: CallControls,
	) -> Result<Operation, ApiError> {
		self.record(format!("call_controls {} {c:?}", hex::encode(call_id)))
	}
	async fn set_global_controls(&mut self, c: GlobalControls) -> Result<Operation, ApiError> {
		self.record(format!("global_controls {c:?}"))
	}
	async fn select_devices(&mut self, input: &str, output: &str) -> Result<Operation, ApiError> {
		self.record(format!("select_devices {input} {output}"))
	}
	async fn set_noise_suppression(&mut self, backend: &str) -> Result<Operation, ApiError> {
		self.record(format!("ns {backend}"))
	}
	async fn set_profile(&mut self, name: &str) -> Result<Operation, ApiError> {
		self.record(format!("profile {name}"))
	}
	async fn start_mic_test(
		&mut self,
		local: bool,
		confirmed: bool,
	) -> Result<Operation, ApiError> {
		self.record(format!(
			"mic_test_start local={local} confirmed={confirmed}"
		))
	}
	async fn stop_mic_test(&mut self) -> Result<Operation, ApiError> {
		self.record("mic_test_stop")
	}
	async fn decide_trust(&mut self, d: TrustDecision) -> Result<Operation, ApiError> {
		self.record(format!(
			"trust {} {:?} oob={} key={}",
			hex::encode(&d.peer_id),
			d.requested,
			d.out_of_band_verified,
			hex::encode(&d.public_key_sha256)
		))
	}
	async fn ptt(&mut self, request: PttRequest) -> Result<(), ApiError> {
		self.log.push(format!(
			"ptt {:?} gen={}",
			request.action, request.route_generation
		));
		Ok(())
	}
}

impl Subscribe for FakeControl {
	type Watch = Script<WatchItem>;
	type Telemetry = Script<TelemetrySnapshot>;

	async fn open_watch(&mut self) -> Result<Self::Watch, ApiError> {
		let items = self.watches.pop_front().ok_or(ApiError::StreamClosed)?;
		Ok(Script(items.into()))
	}

	async fn open_telemetry(&mut self, _: u32) -> Result<Self::Telemetry, ApiError> {
		Ok(Script(self.telemetry.drain(..).map(Ok).collect()))
	}
}
