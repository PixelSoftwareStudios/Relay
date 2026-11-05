//! The thin layer between commands/screens and the engine.
//!
//! Commands are generic over [`Control`] (request/response) and, when they
//! stream, [`Subscribe`]. [`RealClient`] implements both over `RelayClient`;
//! tests supply fakes.

mod feed;
mod manage;
mod real;
mod requests;

use std::future::Future;

use relay_api::ApiError;
use relay_api::pb::{EngineInfo, Operation, Snapshot, TelemetrySnapshot};
use relay_api::{Endpoint, WatchItem};

pub use feed::{ConnectionState, Update, run_state_feed, run_telemetry_feed};
pub use manage::{ChannelRef, Credential, Manage};
pub use real::RealClient;
pub use relay_client::{CallAction, CallTarget};
pub use requests::{CallControls, GlobalControls, PttAction, PttRequest, TrustDecision};

use crate::cli::{CliError, GlobalArgs, endpoint};

/// A source of items that ends with `Ok(None)` when the engine closes it.
pub trait Feed: Send {
	/// Item type delivered.
	type Item: Send;
	/// Next item.
	fn next(&mut self) -> impl Future<Output = Result<Option<Self::Item>, ApiError>> + Send;
}

/// Opening of long-lived streams. Futures are `Send` so feeds can run in tasks.
pub trait Subscribe: Send {
	/// State watch; the first item is a snapshot.
	type Watch: Feed<Item = WatchItem>;
	/// Telemetry stream (meters, speaking).
	type Telemetry: Feed<Item = TelemetrySnapshot>;
	/// Open a fresh state watch.
	fn open_watch(&mut self) -> impl Future<Output = Result<Self::Watch, ApiError>> + Send;
	/// Open telemetry at roughly `meters_hz`.
	fn open_telemetry(
		&mut self,
		meters_hz: u32,
	) -> impl Future<Output = Result<Self::Telemetry, ApiError>> + Send;
}

/// Request/response operations against the engine. Every mutation returns the
/// settled [`Operation`]; callers decide what a failed one means.
#[allow(async_fn_in_trait, reason = "only used with concrete generics")]
pub trait Control {
	/// Engine information captured at connect time.
	fn info(&self) -> &EngineInfo;
	/// Take (or keep) the controller lease of a call and return its generation.
	async fn controller_generation(&mut self, call_id: &[u8]) -> Result<u64, ApiError>;
	/// Current snapshot.
	async fn snapshot(&mut self) -> Result<Snapshot, ApiError>;
	/// Start a call to a peer or manual address.
	async fn start_call(&mut self, target: CallTarget) -> Result<Operation, ApiError>;
	/// Accept, decline or end a call.
	async fn call_action(
		&mut self,
		action: CallAction,
		call_id: &[u8],
	) -> Result<Operation, ApiError>;
	/// Set the transmit route of a call.
	async fn set_route(
		&mut self,
		call_id: &[u8],
		mode: relay_api::pb::TransmitMode,
		consent: bool,
	) -> Result<Operation, ApiError>;
	/// Mute, deafen or set the receive gain of one call.
	async fn set_call_controls(
		&mut self,
		call_id: &[u8],
		controls: CallControls,
	) -> Result<Operation, ApiError>;
	/// Global mute, deafen and gains.
	async fn set_global_controls(
		&mut self,
		controls: GlobalControls,
	) -> Result<Operation, ApiError>;
	/// Select input and output devices (both ids must be set).
	async fn select_devices(&mut self, input: &str, output: &str) -> Result<Operation, ApiError>;
	/// Select a noise suppression backend.
	async fn set_noise_suppression(&mut self, backend: &str) -> Result<Operation, ApiError>;
	/// Change the display name.
	async fn set_profile(&mut self, display_name: &str) -> Result<Operation, ApiError>;
	/// Start the microphone test.
	async fn start_mic_test(
		&mut self,
		local_playback: bool,
		confirmed_feedback: bool,
	) -> Result<Operation, ApiError>;
	/// Stop the microphone test.
	async fn stop_mic_test(&mut self) -> Result<Operation, ApiError>;
	/// Record a trust decision about a peer.
	async fn decide_trust(&mut self, decision: TrustDecision) -> Result<Operation, ApiError>;
	/// Press, renew or release push-to-talk.
	async fn ptt(&mut self, request: PttRequest) -> Result<(), ApiError>;
}

/// Connect to the endpoint selected by the global flags and register.
pub async fn connect(global: &GlobalArgs) -> Result<RealClient, CliError> {
	let endpoint = endpoint::resolve(global)?;
	connect_to(&endpoint, global).await
}

/// Connect to an explicit endpoint and register.
pub async fn connect_to(endpoint: &Endpoint, global: &GlobalArgs) -> Result<RealClient, CliError> {
	RealClient::connect(endpoint, global)
		.await
		.map_err(|source| CliError::Unreachable {
			endpoint: endpoint::describe(endpoint),
			source,
		})
}
