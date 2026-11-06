//! Push-to-talk presses from the desktop: the hold-to-talk button and the
//! global hotkey both end here, in the one shared `PttWorker`
//! (`relay_client::ptt`). A press is only ever sent for a live push-to-talk
//! call that this app's own client controls, so a key during a muted or open
//! microphone call does nothing (it never changes the mode) and shows no
//! error.

use std::sync::{Arc, Mutex};

use relay_api::Endpoint;
use relay_client::ptt::{PttState, PttWorker};
use relay_io::LockExt;
use relay_ui::hub::Hub;
use relay_ui::ptt_target;
use tokio::runtime::Handle;

/// Where the engine is. Asked when the first press needs a connection.
pub type EndpointFn = Box<dyn Fn() -> Result<Endpoint, String> + Send + Sync>;

pub struct PttControl {
	hub: Arc<Hub>,
	handle: Handle,
	endpoint: EndpointFn,
	/// Created by the first press: a desktop that never talks opens nothing.
	worker: Mutex<Option<PttWorker>>,
}

impl PttControl {
	pub fn new(hub: Arc<Hub>, handle: Handle, endpoint: EndpointFn) -> Self {
		Self {
			hub,
			handle,
			endpoint,
			worker: Mutex::new(None),
		}
	}

	/// The on-screen button went down for `call` (hex id).
	pub fn press(&self, call: &str) {
		let mut slot = self.worker.lock_unpoisoned();
		if slot.is_none() {
			match (self.endpoint)() {
				Ok(endpoint) => {
					let resolve = ptt_target::resolver(self.hub.clone());
					*slot = Some(PttWorker::spawn(&self.handle, endpoint, resolve));
				}
				Err(error) => {
					tracing::warn!(%error, "push-to-talk has no engine to talk to");
					return;
				}
			}
		}
		if let Some(worker) = slot.as_ref() {
			worker.press(call);
		}
	}

	/// The global key went down: press for the one push-to-talk call this app
	/// controls. Without one nothing happens.
	pub fn press_current(&self) {
		match self.hub.read(ptt_target::current_call) {
			Some(call) => self.press(&call),
			None => tracing::debug!("push-to-talk key ignored: no push-to-talk call here"),
		}
	}

	/// The key or button went up (the engine keeps the gate for its release delay).
	pub fn release(&self) {
		if let Some(worker) = self.worker.lock_unpoisoned().as_ref() {
			worker.release();
		}
	}

	/// Close the stream: the engine releases at once.
	pub fn cancel(&self) {
		if let Some(worker) = self.worker.lock_unpoisoned().as_ref() {
			worker.cancel();
		}
	}

	/// What the worker last reported.
	pub fn state(&self) -> PttState {
		self.worker
			.lock_unpoisoned()
			.as_ref()
			.map_or(PttState::Idle, |w| w.states().borrow().clone())
	}
}
