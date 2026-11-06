//! Hands the system Back action from Kotlin's main thread to the UI thread.
//!
//! The UI (`RelayApp`) lives on the render thread and is not `Send`-shared, so
//! the Back callback cannot call it directly: it leaves a request here, wakes
//! the render loop and waits briefly for the answer. The loop answers between
//! frames. No answer in time (the surface is gone, nothing is drawing) counts
//! as "not used", so Back still leaves the app instead of hanging.

use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;

use relay_io::LockExt;

/// How long Back waits for a frame before giving up.
pub const WAIT: Duration = Duration::from_millis(400);

#[derive(Default)]
pub struct BackGate {
	waiting: Mutex<Option<Sender<bool>>>,
}

impl BackGate {
	/// Ask the UI to go back and wait for its answer. `wake` makes the render
	/// loop run a frame.
	pub fn ask(&self, wake: impl FnOnce(), wait: Duration) -> bool {
		let (tx, rx) = channel();
		*self.waiting.lock_unpoisoned() = Some(tx);
		wake();
		let used = rx.recv_timeout(wait).unwrap_or(false);
		self.waiting.lock_unpoisoned().take();
		used
	}

	/// Render thread, once per frame: answer a waiting request with `back`.
	pub fn serve(&self, back: impl FnOnce() -> bool) {
		let waiting = self.waiting.lock_unpoisoned().take();
		if let Some(tx) = waiting {
			let _ = tx.send(back());
		}
	}
}

#[cfg(test)]
mod tests {
	use std::sync::Arc;
	use std::thread;

	use super::*;

	#[test]
	fn the_render_thread_answers_a_waiting_request() {
		let gate = Arc::new(BackGate::default());
		for answer in [true, false] {
			let render = gate.clone();
			let wake = move || {
				thread::spawn(move || render.serve(|| answer));
			};
			assert_eq!(gate.ask(wake, Duration::from_secs(2)), answer);
		}
	}

	#[test]
	fn no_frame_in_time_is_not_used() {
		let gate = BackGate::default();
		assert!(!gate.ask(|| {}, Duration::from_millis(20)));
		// A late frame finds nobody waiting and does not run the UI.
		gate.serve(|| panic!("nobody asked"));
	}
}
