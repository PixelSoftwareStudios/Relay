//! Real engine adapter over `relay_api::RelayClient`.

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use relay_api::pb::{self, OperationState};
use relay_api::{ApiError, BoundedWatch, Endpoint, RelayClient, WatchItem};
use relay_client::operation::settle;
use relay_client::wire::{acquire_generation, connect_registered, open_telemetry, send};
use relay_client::{CallAction, CallTarget, Command as WireCommand, Secret};
use tokio::sync::Mutex;
use tonic::Code;

use crate::convert::{self, advert_to_wire, inbound_to_wire, mode_to_wire};
use crate::engine::{
	Command, Connection, Engine, EngineError, EventSource, Operations, Outcome, SettingOutcome,
	SettingResult, TelemetrySource,
};
use crate::model::{ConnectionView, HistoryEntry};
use crate::tr;

const CLIENT_NAME: &str = "relay-desktop";
const CAPABILITIES: &[&str] = &["observe", "call-control"];
/// Local subscriber queue; overflow ends the watch and triggers a resync.
const WATCH_QUEUE: usize = 256;
const METERS_HZ: u32 = 10;
const STATS_HZ: u32 = 1;
/// Entries asked of `ListCallHistory`; the engine caps the page at its own limit.
const HISTORY_FETCH: u32 = 200;
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
		ApiError::Starting(startup) => EngineError::Starting(convert::startup(&startup)),
		other => EngineError::Broken(other.to_string()),
	}
}

/// Shared with the messaging adapter.
pub(crate) fn map_api_error(error: ApiError) -> EngineError {
	map_error(error)
}

fn status(s: tonic::Status) -> EngineError {
	map_error(s.into())
}

fn problem_error(problem: Option<pb::Problem>) -> EngineError {
	let p = problem.unwrap_or_default();
	let message = if p.message.is_empty() {
		tr!("err-engine-rejected").to_owned()
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

struct CallStatsStream(tonic::Streaming<pb::CallStats>);

#[async_trait]
impl crate::engine::CallStatsSource for CallStatsStream {
	async fn next(&mut self) -> Result<Option<crate::model::CallStats>, EngineError> {
		Ok(self
			.0
			.message()
			.await
			.map_err(status)?
			.map(|s| convert::call_stats(&s)))
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
		Command::AcquireController { .. } | Command::SetAutostart { .. } => return None,
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
		} => WireCommand::SetTransmit {
			call_id,
			generation,
			mode: mode_to_wire(mode),
		},
		Command::ConnectAndTrust { text } => WireCommand::ConnectAndTrust {
			connection_string: text,
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
			ptt_release_delay_ms,
		} => WireCommand::SetGlobalControls {
			muted,
			deafened,
			microphone_gain,
			playback_gain,
			ptt_release_delay_ms,
		},
		Command::SetRingPreferences {
			ringtone_enabled,
			ringtone_volume,
			ringback_enabled,
			call_sounds_enabled,
		} => WireCommand::SetRingPreferences {
			ringtone_enabled,
			ringtone_volume,
			ringback_enabled,
			call_sounds_enabled,
		},
		Command::PreviewTone => WireCommand::PreviewTone,
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
		Command::ClearAvatar => WireCommand::ClearAvatar,
		Command::ForgetPeer { peer_id } => WireCommand::ForgetPeer {
			peer_id,
			query: String::new(),
		},
		Command::ReplacePinnedKey {
			peer_id,
			fingerprint,
		} => WireCommand::ReplacePinnedKey {
			peer_id,
			fingerprint,
			verified: true,
		},
		Command::SetFavorites { peer_ids } => WireCommand::SetFavorites { peer_ids },
		Command::TrustPeer { peer_id } => WireCommand::DecideTrust {
			public_key_sha256: peer_id.clone(),
			peer_id,
			requested: pb::TrustState::TofuUnverified,
			out_of_band_verified: false,
		},
		Command::BlockPeer { peer_id } => WireCommand::DecideTrust {
			public_key_sha256: peer_id.clone(),
			peer_id,
			requested: pb::TrustState::Blocked,
			out_of_band_verified: false,
		},
		Command::StartCallConnection { text } => {
			WireCommand::StartCall(CallTarget::ConnectionString(text))
		}
		Command::CreateChannel {
			name,
			options,
			password,
		} => WireCommand::CreateChannel {
			name,
			options,
			password,
		},
		Command::JoinChannel {
			channel_id,
			password,
			invite,
		} => WireCommand::JoinChannel {
			channel_id,
			password,
			invite,
		},
		Command::LeaveChannel { channel_id } => WireCommand::LeaveChannel { channel_id },
		Command::RevokeMember {
			channel_id,
			fingerprint,
		} => WireCommand::RevokeMember {
			channel_id,
			fingerprint,
		},
		Command::SetPrivacy { inbound, advert } => WireCommand::SetPrivacy {
			inbound: inbound.map(inbound_to_wire),
			advert: advert.map(advert_to_wire),
		},
		Command::ClearCallHistory => WireCommand::ClearCallHistory,
		Command::CreateInvite { .. } => return None,
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
			return self.run_unwired(command).await;
		};
		let operation = send(&mut *self.client.lock().await, wire)
			.await
			.map_err(map_error)?;
		self.settle(operation).await
	}

	async fn connection_info(&self) -> Result<ConnectionView, EngineError> {
		let info = self.client.lock().await.connection_info().await;
		info.map(|i| convert::connection(&i)).map_err(map_error)
	}

	async fn avatar(&self, sha256: Vec<u8>) -> Result<Vec<u8>, EngineError> {
		let image = self.client.lock().await.get_avatar(&sha256).await;
		image.map(|i| i.image).map_err(map_error)
	}

	async fn call_history(&self) -> Result<Vec<HistoryEntry>, EngineError> {
		let page = self
			.client
			.lock()
			.await
			.list_call_history(HISTORY_FETCH, None)
			.await
			.map_err(map_error)?;
		Ok(page.entries.iter().map(convert::history_entry).collect())
	}

	async fn watch_call_stats(
		&self,
		call_id: Vec<u8>,
	) -> Result<Box<dyn crate::engine::CallStatsSource>, EngineError> {
		let mut client = self.client.lock().await;
		let stream = relay_client::wire::open_call_stats(&mut client, &call_id)
			.await
			.map_err(map_error)?;
		Ok(Box::new(CallStatsStream(stream)))
	}

	async fn get_settings(&self) -> Result<BTreeMap<String, String>, EngineError> {
		let dump = self.client.lock().await.get_settings().await;
		dump.map_err(map_error)
	}

	async fn limit_usage(&self) -> Result<BTreeMap<String, u64>, EngineError> {
		let usage = self.client.lock().await.get_limit_usage().await;
		usage.map_err(map_error)
	}

	async fn set_settings(
		&self,
		values: Vec<(String, String)>,
	) -> Result<Vec<SettingResult>, EngineError> {
		let reply = self.client.lock().await.set_settings(values).await;
		let reply = reply.map_err(map_error)?;
		Ok(reply.results.into_iter().map(setting_result).collect())
	}
}

fn setting_result(result: pb::SettingResult) -> SettingResult {
	let outcome = match pb::SettingOutcome::try_from(result.outcome) {
		Ok(pb::SettingOutcome::AppliedLive) => SettingOutcome::AppliedLive,
		Ok(pb::SettingOutcome::SavedNeedsRestart) => SettingOutcome::SavedNeedsRestart,
		// An unknown outcome is never shown as success.
		_ => SettingOutcome::Rejected,
	};
	SettingResult {
		key: result.key,
		outcome,
		reason: result.reason,
	}
}

impl Ops {
	/// Commands whose answer is not a plain operation.
	async fn run_unwired(&self, command: Command) -> Result<Outcome, EngineError> {
		let mut client = self.client.lock().await;
		match command {
			Command::AcquireController { call_id } => {
				let generation = acquire_generation(&mut client, &call_id, false)
					.await
					.map_err(map_error)?;
				Ok(Outcome {
					generation: Some(generation),
					..Outcome::default()
				})
			}
			Command::CreateInvite {
				channel_id,
				ttl_seconds,
				uses,
			} => {
				let reply = client
					.create_invite(&channel_id, ttl_seconds, uses)
					.await
					.map_err(map_error)?;
				Ok(Outcome {
					invite: Some(Secret::new(reply.invite)),
					..Outcome::default()
				})
			}
			Command::SetAutostart { enabled } => {
				let request = pb::SetGlobalControlsRequest {
					context: Some(client.new_mutation().map_err(map_error)?),
					muted: None,
					deafened: None,
					microphone_gain: None,
					playback_gain: None,
					ptt_release_delay_ms: None,
					autostart_daemon: Some(enabled),
					..pb::SetGlobalControlsRequest::default()
				};
				let operation = client
					.raw()
					.set_global_controls(request)
					.await
					.map_err(status)?
					.into_inner();
				drop(client);
				self.settle(operation).await
			}
			_ => unreachable!("every other command has a wire form"),
		}
	}
}

/// Endpoint choice shared with the CLI: `RELAY_TCP_PROFILE`, `RELAY_SOCKET`,
/// else the per-user default. Call from inside the tokio runtime.
pub fn resolve_endpoint() -> Result<Endpoint, EngineError> {
	relay_client::endpoint::resolve().map_err(|e| EngineError::Unavailable(e.to_string()))
}
