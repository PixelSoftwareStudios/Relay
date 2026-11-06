//! Hold-to-talk through the engine's push-to-talk lease: the one worker every
//! client uses (the CLI key, the desktop hotkey and hold button, Android).
//!
//! A press is an `InputControl` message with a short lease ([`LEASE_MS`]) that
//! the worker renews every [`RENEW`] while the key is down; the engine opens
//! the transmit gate only while a lease is alive and closes it when the lease
//! lapses, the stream ends or the controller changes. Nothing here opens the
//! microphone by itself: the mode (muted, unmuted, push-to-talk) is set
//! elsewhere and a press does not change it.
//!
//! The engine accepts a press only from the client that holds the call's
//! controller lease. The worker therefore opens its own connection to the same
//! engine and adopts the controlling client's registered context
//! ([`relay_api::RelayClient::adopt_context`]; the client id is an identifier,
//! not a credential). The caller says which call and which generation that
//! client controls through a [`Resolve`] function, asked on every press and
//! renewal, so a call that ended, left push-to-talk or changed controller ends
//! the press at once.
//!
//! Every way a key can vanish ends in a release: [`PttWorker::release`] sends a
//! release (the engine's release delay applies), [`PttWorker::cancel`] and
//! dropping the worker close the stream, which the engine treats as an
//! immediate release of everything the source held. A held key older than
//! [`MAX_HOLD`] is released too (a key-up that never arrived), and if this
//! process dies the lease lapses within [`LEASE_MS`].

mod worker;

use std::sync::Arc;
use std::time::Duration;

use relay_api::Endpoint;
use tokio::runtime::Handle;
use tokio::sync::{mpsc, watch};

/// Lease asked for on every press and renewal. The engine caps it.
pub const LEASE_MS: u32 = 400;
/// Renewal period: well inside the lease so one late tick does not drop it.
pub const RENEW: Duration = Duration::from_millis(150);
/// A press older than this is released without waiting for a key-up.
pub const MAX_HOLD: Duration = Duration::from_secs(60);

/// What a press is sent for: a call this client controls.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PttTarget {
	/// Call being controlled.
	pub call_id: Vec<u8>,
	/// Controller lease generation the client holds.
	pub controller_generation: u64,
	/// Transmit route generation, or 0 when the caller does not know it.
	pub transmit_route_generation: u64,
	/// The registered client that controls the call.
	pub client_id: Vec<u8>,
}

/// Finds the target for a call (hex id): `Some` only for a live push-to-talk
/// call that the controlling client holds. Called often; keep it cheap.
pub type Resolve = Arc<dyn Fn(&str) -> Option<PttTarget> + Send + Sync>;

/// Honest state of the worker for UIs and logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PttState {
	/// Key up, nothing held.
	Idle,
	/// Lease held and the engine reports it is transmitting.
	Transmitting,
	/// Lease held but the engine is not transmitting (not in push-to-talk, no consent, ...).
	Gated,
	/// A press was ignored: no live push-to-talk call is controlled.
	NoTarget,
	/// The engine refused the lease.
	Rejected {
		/// Stable engine problem code.
		code: String,
	},
	/// The stream to the engine failed.
	Failed(String),
}

pub(crate) enum Msg {
	Press(String),
	Release,
	Cancel,
	/// The engine's answer to message `sequence`.
	Ack {
		sequence: u64,
		accepted: bool,
		transmitting: bool,
		code: String,
	},
}

/// Sender side: cheap to call from any thread, never blocks.
pub struct PttWorker {
	msgs: mpsc::UnboundedSender<Msg>,
	state: watch::Receiver<PttState>,
}

impl PttWorker {
	/// Start the worker on `handle`. It connects on the first press.
	pub fn spawn(handle: &Handle, endpoint: Endpoint, resolve: Resolve) -> Self {
		let (msgs, inbox) = mpsc::unbounded_channel();
		let (state_tx, state) = watch::channel(PttState::Idle);
		let worker = worker::Worker::new(endpoint, resolve, msgs.clone(), state_tx);
		handle.spawn(worker.run(inbox));
		Self { msgs, state }
	}

	/// The key for `call` (hex id) went down.
	pub fn press(&self, call: impl Into<String>) {
		let _ = self.msgs.send(Msg::Press(call.into()));
	}

	/// The key went up. The engine keeps the gate for its release delay.
	pub fn release(&self) {
		let _ = self.msgs.send(Msg::Release);
	}

	/// The key vanished (pause, focus loss, client detach, call end, mode
	/// change, quit): close the stream, which releases at once. The next press
	/// opens a new one.
	pub fn cancel(&self) {
		let _ = self.msgs.send(Msg::Cancel);
	}

	/// The worker's state, updated as the engine answers.
	pub fn states(&self) -> watch::Receiver<PttState> {
		self.state.clone()
	}
}
