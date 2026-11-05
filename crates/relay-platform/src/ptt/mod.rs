//! Push-to-talk: turn key edges into renewable `InputControl` leases.

mod driver;
mod sink;

use relay_api::pb;

pub use driver::PttDriver;
pub use sink::{GrpcLeaseSink, LeaseSink, SinkError};

use crate::session::SessionEvent;

/// The call, controller generation and route generation a lease is bound to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PttTarget {
	/// Call being controlled.
	pub call_id: Vec<u8>,
	/// Controller lease generation this client holds.
	pub controller_generation: u64,
	/// Current transmit route generation.
	pub transmit_route_generation: u64,
}

impl PttTarget {
	/// Derive a target from an engine call, only if `client_id` is its controller.
	pub fn from_call(call: &pb::Call, client_id: &[u8]) -> Option<Self> {
		let controller = call.controller.as_ref()?;
		if controller.controller_client_id != client_id {
			return None;
		}
		Some(Self {
			call_id: call.call_id.clone(),
			controller_generation: controller.generation,
			transmit_route_generation: call.transmit.as_ref()?.generation,
		})
	}
}

/// Side inputs to the driver besides hotkey edges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PttInput {
	/// New target, or `None` when this client controls no call.
	Target(Option<PttTarget>),
	/// OS or UI session change.
	Session(SessionEvent),
	/// Release and stop the driver.
	Shutdown,
}

/// Why the driver will not transmit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockReason {
	/// No call is controlled by this client.
	NoTarget,
	/// Screen locked or system suspending.
	SessionLocked,
	/// The key was held across a forced release; release it and press again.
	NeedsRepress,
}

/// Honest PTT state for UIs and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PttState {
	/// Key up, nothing held.
	Idle,
	/// Lease held and the engine reports it is transmitting.
	Transmitting,
	/// Lease held but the engine is not transmitting (muted, no consent, ...).
	Gated,
	/// A press was ignored.
	Blocked(BlockReason),
	/// The engine refused the lease.
	Rejected {
		/// Stable engine problem code.
		code: String,
	},
	/// The engine did not answer before the lease ran out.
	Expired,
	/// The channel failed; the driver stopped.
	Failed(String),
}

/// Timing knobs; core caps leases at 500 ms.
#[derive(Debug, Clone, Copy)]
pub struct PttConfig {
	/// Lease requested on press and renewal.
	pub lease_ms: u32,
	/// Grace for best-effort release messages.
	pub release_timeout_ms: u64,
}

impl Default for PttConfig {
	fn default() -> Self {
		Self {
			lease_ms: 300,
			release_timeout_ms: 100,
		}
	}
}

/// Why the driver ended. Whenever it ends, transmission has been released or has
/// expired on its own.
#[derive(Debug, thiserror::Error)]
pub enum PttError {
	/// Channel to the engine failed.
	#[error("engine channel failed: {0}")]
	Sink(#[from] SinkError),
	/// The engine answered a different message than we sent.
	#[error("unexpected acknowledgement for sequence {0}")]
	Protocol(u64),
	/// The hotkey stream closed.
	#[error("hotkey backend stopped")]
	BackendClosed,
	/// The engine connection went away.
	#[error("disconnected from engine")]
	Disconnected,
}
