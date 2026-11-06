//! The microphone permission as the desktop shell tracks it.
//!
//! macOS asks for microphone access only when a stream first opens, which the
//! daemon does at a call, so without this the prompt would come late or look
//! unrelated. The gate reads the OS status without prompting, asks once right at
//! launch while the user was never asked, and re-reads the status whenever the
//! window comes back to the front (the user may have flipped the switch in
//! System Settings). The OS call sits behind [`Permission`] so tests use a fake.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use relay_platform::microphone::Authorization;
use relay_ui::microphone::MicrophoneAccess;

/// The OS side: read the status, show the prompt.
pub trait Permission: Send + Sync {
	fn status(&self) -> Authorization;
	/// Show the system prompt; `done` runs once with the answer, on any thread.
	fn request(&self, done: Box<dyn FnOnce(bool) + Send>);
}

/// The real thing (`relay-platform`).
pub struct System;

impl Permission for System {
	fn status(&self) -> Authorization {
		relay_platform::microphone::status()
	}

	fn request(&self, done: Box<dyn FnOnce(bool) + Send>) {
		relay_platform::microphone::request(done);
	}
}

fn access_of(status: Authorization) -> MicrophoneAccess {
	match status {
		Authorization::NotDetermined => MicrophoneAccess::NotAsked,
		// Refused: only System Settings changes it (macOS never prompts twice).
		Authorization::Denied | Authorization::Restricted => MicrophoneAccess::Blocked,
		Authorization::Authorized | Authorization::NotRequired => MicrophoneAccess::Ready,
	}
}

struct Inner {
	permission: Arc<dyn Permission>,
	access: AtomicU8,
	/// The system prompt is on screen.
	asking: AtomicBool,
	repaint: Box<dyn Fn() + Send + Sync>,
}

/// Cheap to clone; shared by the shell and the prompt's completion.
#[derive(Clone)]
pub struct MicrophoneGate(Arc<Inner>);

const ACCESSES: [MicrophoneAccess; 4] = [
	MicrophoneAccess::Ready,
	MicrophoneAccess::NotAsked,
	MicrophoneAccess::Denied,
	MicrophoneAccess::Blocked,
];

impl MicrophoneGate {
	pub fn new(permission: Arc<dyn Permission>, repaint: Box<dyn Fn() + Send + Sync>) -> Self {
		let gate = Self(Arc::new(Inner {
			permission,
			access: AtomicU8::new(0),
			asking: AtomicBool::new(false),
			repaint,
		}));
		gate.refresh();
		gate
	}

	/// What the UI shows: nothing while the system prompt is up.
	pub fn access(&self) -> MicrophoneAccess {
		if self.0.asking.load(Ordering::Acquire) {
			return MicrophoneAccess::Ready;
		}
		ACCESSES[usize::from(self.0.access.load(Ordering::Acquire))]
	}

	/// Read the status again (focus returned, the prompt closed).
	pub fn refresh(&self) {
		let now = access_of(self.0.permission.status());
		let index = ACCESSES.iter().position(|a| *a == now).unwrap_or(0);
		let before = self.0.access.swap(index as u8, Ordering::AcqRel);
		if usize::from(before) != index {
			(self.0.repaint)();
		}
	}

	/// Show the system prompt if the user was never asked. Does nothing when
	/// the answer is already known (macOS prompts once) or a prompt is up.
	pub fn ask(&self) {
		if self.0.permission.status() != Authorization::NotDetermined {
			self.refresh();
			return;
		}
		if self.0.asking.swap(true, Ordering::AcqRel) {
			return;
		}
		let gate = self.clone();
		self.0.permission.request(Box::new(move |_granted| {
			gate.0.asking.store(false, Ordering::Release);
			gate.refresh();
			(gate.0.repaint)();
		}));
	}

	/// At launch: read the status and, while the user was never asked, prompt
	/// at once with no click.
	pub fn start(&self) {
		self.refresh();
		self.ask();
	}
}
