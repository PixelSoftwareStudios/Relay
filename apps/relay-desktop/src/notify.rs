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
use relay_ui::model::{AppState, Call, Link, Meters, Phase};
use relay_ui::tr;

/// Longest caller name put in a notification.
const NAME_MAX_CHARS: usize = 64;

/// What the user chose on a notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reply {
	Accept,
	Decline,
	/// The notification body was clicked: show the window.
	Open,
}

/// Receives a [`Reply`] for a call id. Built by the shell from its actions.
pub type Responder = Arc<dyn Fn(&str, Reply) + Send + Sync>;

/// Why a call is announced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
	/// It is ringing and waits for an answer.
	Ringing,
	/// The engine answered it by itself (auto-answer); nothing to accept.
	AutoAnswered,
}

/// One call to announce.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Announcement {
	pub call_id: String,
	pub caller: String,
	pub kind: Kind,
}

/// Shows one notification. The real one talks to the OS; tests use a recording fake.
pub trait Notifier: Send + Sync {
	/// Announce a ringing call from `caller`. `Err` carries the system's reason.
	fn incoming_call(&self, caller: &str) -> Result<(), String>;

	/// Like `incoming_call`, and where the platform supports actions also offers Accept and
	/// Decline and reports the choice through `respond`. The default has no actions.
	fn announce(&self, announcement: &Announcement, _respond: Responder) -> Result<(), String> {
		self.incoming_call(&announcement.caller)
	}
}

/// The title of an incoming-call notification, in the active language.
pub fn notification_title() -> &'static str {
	tr!("notification-title")
}

/// `notify-rust`: the macOS notification centre, Windows toasts, freedesktop notifications.
pub struct SystemNotifier;

impl Notifier for SystemNotifier {
	fn incoming_call(&self, caller: &str) -> Result<(), String> {
		base(notification_title(), caller)
			.show()
			.map(drop)
			.map_err(|e| e.to_string())
	}

	#[cfg(not(all(unix, not(target_os = "macos"))))]
	fn announce(&self, announcement: &Announcement, _respond: Responder) -> Result<(), String> {
		let (title, body) = text(announcement);
		base(title, &body)
			.show()
			.map(drop)
			.map_err(|e| e.to_string())
	}

	/// Accept, Decline and click-to-open need `wait_for_action`, which is used on Linux
	/// (freedesktop notifications). macOS and Windows show the notification only; the tray
	/// icon opens the window there.
	#[cfg(all(unix, not(target_os = "macos")))]
	fn announce(&self, announcement: &Announcement, respond: Responder) -> Result<(), String> {
		let (title, body) = text(announcement);
		let mut n = base(title, &body);
		if announcement.kind == Kind::Ringing {
			n.action("accept", tr!("notification-accept"))
				.action("decline", tr!("notification-decline"));
		}
		n.action("default", tr!("notification-open"));
		let handle = n.show().map_err(|e| e.to_string())?;
		let call_id = announcement.call_id.clone();
		std::thread::spawn(move || {
			handle.wait_for_action(|action| {
				if let Some(reply) = reply_for(action) {
					respond(&call_id, reply);
				}
			})
		});
		Ok(())
	}
}

/// Title and body of a notification. Only the caller's cleaned name goes in.
pub fn text(announcement: &Announcement) -> (&'static str, String) {
	match announcement.kind {
		Kind::Ringing => (notification_title(), announcement.caller.clone()),
		Kind::AutoAnswered => (
			"Relay",
			tr!(
				"notification-auto-answered",
				name = announcement.caller.as_str()
			),
		),
	}
}

/// The reply for a freedesktop action id; closing the notification is no reply.
pub fn reply_for(action: &str) -> Option<Reply> {
	match action {
		"accept" => Some(Reply::Accept),
		"decline" => Some(Reply::Decline),
		"default" => Some(Reply::Open),
		_ => None,
	}
}

/// The notification without actions. It never plays a sound of its own: the daemon rings
/// (or stays silent) according to `calls.alert`.
fn base(title: &str, body: &str) -> notify_rust::Notification {
	let mut n = notify_rust::Notification::new();
	n.appname("Relay").summary(title).body(body);
	// About as long as the call rings; a stale Accept button is worse than none.
	n.timeout(notify_rust::Timeout::Milliseconds(30_000));
	#[cfg(all(unix, not(target_os = "macos")))]
	n.hint(notify_rust::Hint::SuppressSound(true));
	n
}

/// Which ringing calls still need a notification. Pure: no I/O, no clock.
#[derive(Default)]
pub struct IncomingTracker {
	seen: HashSet<String>,
	answered: HashSet<String>,
}

impl IncomingTracker {
	/// Caller names to announce for `state`. A call is announced at most once, and a call
	/// that rang while the window was in front, or with the switch off, never is later.
	pub fn observe(&mut self, state: &AppState, enabled: bool, in_front: bool) -> Vec<String> {
		self.announcements(state, enabled, in_front)
			.into_iter()
			.map(|a| a.caller)
			.collect()
	}

	/// Like `observe`, with the call id so a reply can be routed back.
	pub fn announcements(
		&mut self,
		state: &AppState,
		enabled: bool,
		in_front: bool,
	) -> Vec<Announcement> {
		let ringing: Vec<_> = state
			.calls
			.iter()
			.filter(|c| c.phase == Phase::IncomingRinging)
			.collect();
		self.seen
			.retain(|id| ringing.iter().any(|c| &c.call_id == id));
		let connected = state.link == Link::Connected;
		let mut found = Vec::new();
		for call in ringing {
			if !self.seen.insert(call.call_id.clone()) {
				continue;
			}
			if enabled && !in_front && connected {
				found.push(self.announcement(state, call, Kind::Ringing));
			}
		}
		let auto: Vec<_> = state
			.calls
			.iter()
			.filter(|c| c.auto_answered && !c.phase.is_terminal())
			.collect();
		self.answered
			.retain(|id| auto.iter().any(|c| &c.call_id == id));
		for call in auto {
			if self.answered.insert(call.call_id.clone()) && enabled && !in_front && connected {
				found.push(self.announcement(state, call, Kind::AutoAnswered));
			}
		}
		found
	}
}

impl IncomingTracker {
	fn announcement(&self, state: &AppState, call: &Call, kind: Kind) -> Announcement {
		let peer = state.peers.iter().find(|p| p.peer_id == call.peer_id);
		Announcement {
			call_id: call.call_id.clone(),
			caller: caller_name(peer.map_or("", |p| p.display_name.as_str())),
			kind,
		}
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
		tr!("notification-unknown-caller").to_owned()
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

	/// Like `deliver`, with Accept and Decline where the platform has them.
	pub fn deliver_call(&self, announcement: &Announcement, respond: Responder) {
		let result = self.notifier.announce(announcement, respond);
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
	respond: Responder,
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
			respond: Arc::new(|_, _| {}),
		}
	}

	/// Where Accept, Decline and click-to-open replies go.
	pub fn with_responder(mut self, respond: Responder) -> Self {
		self.respond = respond;
		self
	}
}

impl Sink for NotifySink {
	fn state(&self, state: &AppState) {
		self.inner.state(state);
		let found = self.tracker.lock_unpoisoned().announcements(
			state,
			(self.enabled)(),
			(self.in_front)(),
		);
		for announcement in found {
			let (dispatcher, respond) = (self.dispatcher.clone(), self.respond.clone());
			// The OS call may block (D-Bus, the notification centre), and this runs on the
			// emitter's task, so it gets its own short-lived thread.
			std::thread::spawn(move || dispatcher.deliver_call(&announcement, respond));
		}
	}

	fn meters(&self, meters: &Meters) {
		self.inner.meters(meters);
	}

	fn tray(&self, summary: &TraySummary) {
		self.inner.tray(summary);
	}
}
