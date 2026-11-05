//! Desktop notifications for incoming calls.
//!
//! A notification is shown only when a call starts ringing, the "Notify on incoming calls"
//! switch is on and the window is hidden, minimized or not in front. It carries the caller's
//! name and nothing else: never message text, never an address. When the system refuses,
//! the reason is kept and shown in Settings; calls still ring through the daemon and show in
//! the window and the tray tooltip.

use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use relay_io::LockExt;
use relay_ui::emitter::{Sink, TraySummary};
use relay_ui::model::{AppState, Link, Meters, Phase};

/// Longest caller name put in a notification.
const NAME_MAX_CHARS: usize = 64;

/// Shows one notification. The real one talks to the OS; tests use a recording fake.
pub trait Notifier: Send + Sync {
	/// Announce a ringing call from `caller`. `Err` carries the system's reason.
	fn incoming_call(&self, caller: &str) -> Result<(), String>;
}

/// `notify-rust`: the macOS notification centre, Windows toasts, freedesktop notifications.
pub struct SystemNotifier;

impl Notifier for SystemNotifier {
	fn incoming_call(&self, caller: &str) -> Result<(), String> {
		notify_rust::Notification::new()
			.appname("Relay")
			.summary("Incoming Relay call")
			.body(caller)
			.show()
			.map(drop)
			.map_err(|e| e.to_string())
	}
}

/// Which ringing calls still need a notification. Pure: no I/O, no clock.
#[derive(Default)]
pub struct IncomingTracker {
	seen: HashSet<String>,
}

impl IncomingTracker {
	/// Caller names to announce for `state`. A call is announced at most once, and a call
	/// that rang while the window was in front, or with the switch off, never is later.
	pub fn observe(&mut self, state: &AppState, enabled: bool, in_front: bool) -> Vec<String> {
		let ringing: Vec<_> = state
			.calls
			.iter()
			.filter(|c| c.phase == Phase::IncomingRinging)
			.collect();
		self.seen
			.retain(|id| ringing.iter().any(|c| &c.call_id == id));
		let mut names = Vec::new();
		for call in ringing {
			if !self.seen.insert(call.call_id.clone()) {
				continue;
			}
			if enabled && !in_front && state.link == Link::Connected {
				let peer = state.peers.iter().find(|p| p.peer_id == call.peer_id);
				names.push(caller_name(peer.map_or("", |p| p.display_name.as_str())));
			}
		}
		names
	}
}

/// A name safe to show: no control characters, bounded, never empty.
pub fn caller_name(name: &str) -> String {
	let clean: String = name
		.chars()
		.filter(|c| !c.is_control())
		.take(NAME_MAX_CHARS)
		.collect();
	let clean = clean.trim();
	if clean.is_empty() {
		"Unknown caller".to_owned()
	} else {
		clean.to_owned()
	}
}

/// Delivers announcements and remembers whether the system refused the last one.
pub struct Dispatcher {
	notifier: Arc<dyn Notifier>,
	failure: Mutex<Option<String>>,
}

impl Dispatcher {
	pub fn new(notifier: Arc<dyn Notifier>) -> Self {
		Self {
			notifier,
			failure: Mutex::new(None),
		}
	}

	/// Show one notification (blocking, as the OS call may be) and record the outcome.
	pub fn deliver(&self, caller: &str) {
		let result = self.notifier.incoming_call(caller);
		*self.failure.lock_unpoisoned() = result.err();
	}

	/// The system's reason for refusing the last notification, if it did.
	pub fn failure(&self) -> Option<String> {
		self.failure.lock_unpoisoned().clone()
	}
}

/// Wraps the emitter's sink: forwards everything, and announces new incoming calls.
pub struct NotifySink {
	inner: Arc<dyn Sink>,
	dispatcher: Arc<Dispatcher>,
	enabled: Arc<dyn Fn() -> bool + Send + Sync>,
	in_front: Arc<dyn Fn() -> bool + Send + Sync>,
	tracker: Mutex<IncomingTracker>,
}

impl NotifySink {
	pub fn new(
		inner: Arc<dyn Sink>,
		dispatcher: Arc<Dispatcher>,
		enabled: Arc<dyn Fn() -> bool + Send + Sync>,
		in_front: Arc<dyn Fn() -> bool + Send + Sync>,
	) -> Self {
		Self {
			inner,
			dispatcher,
			enabled,
			in_front,
			tracker: Mutex::default(),
		}
	}
}

impl Sink for NotifySink {
	fn state(&self, state: &AppState) {
		self.inner.state(state);
		let names =
			self.tracker
				.lock_unpoisoned()
				.observe(state, (self.enabled)(), (self.in_front)());
		for name in names {
			let dispatcher = self.dispatcher.clone();
			// The OS call may block (D-Bus, the notification centre), and this runs on the
			// emitter's task, so it gets its own short-lived thread.
			std::thread::spawn(move || dispatcher.deliver(&name));
		}
	}

	fn meters(&self, meters: &Meters) {
		self.inner.meters(meters);
	}

	fn tray(&self, summary: &TraySummary) {
		self.inner.tray(summary);
	}
}
