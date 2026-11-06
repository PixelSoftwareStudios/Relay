//! The desktop microphone gate with a fake OS: the prompt at launch, the
//! states the card reads, and a grant made in System Settings while running.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use relay_desktop::microphone::{MicrophoneGate, Permission};
use relay_platform::microphone::Authorization;
use relay_ui::microphone::MicrophoneAccess;

type Pending = Box<dyn FnOnce(bool) + Send>;

struct FakeOs {
	status: Mutex<Authorization>,
	prompts: AtomicUsize,
	pending: Mutex<Vec<Pending>>,
}

impl FakeOs {
	fn new(status: Authorization) -> Arc<Self> {
		Arc::new(Self {
			status: Mutex::new(status),
			prompts: AtomicUsize::new(0),
			pending: Mutex::new(Vec::new()),
		})
	}

	/// The user answers the prompt.
	fn answer(&self, allow: bool) {
		*self.status.lock().unwrap() = if allow {
			Authorization::Authorized
		} else {
			Authorization::Denied
		};
		let done = self.pending.lock().unwrap().pop().expect("a prompt is up");
		done(allow);
	}

	/// The user flips the switch in System Settings.
	fn set(&self, status: Authorization) {
		*self.status.lock().unwrap() = status;
	}
}

impl Permission for FakeOs {
	fn status(&self) -> Authorization {
		*self.status.lock().unwrap()
	}

	fn request(&self, done: Pending) {
		self.prompts.fetch_add(1, Ordering::SeqCst);
		self.pending.lock().unwrap().push(done);
	}
}

fn gate(os: &Arc<FakeOs>) -> (MicrophoneGate, Arc<AtomicUsize>) {
	let repaints = Arc::new(AtomicUsize::new(0));
	let counter = repaints.clone();
	let gate = MicrophoneGate::new(
		os.clone(),
		Box::new(move || {
			counter.fetch_add(1, Ordering::SeqCst);
		}),
	);
	(gate, repaints)
}

#[test]
fn a_first_launch_prompts_at_once_without_a_click() {
	let os = FakeOs::new(Authorization::NotDetermined);
	let (gate, _) = gate(&os);
	assert_eq!(gate.access(), MicrophoneAccess::NotAsked);
	gate.start();
	assert_eq!(os.prompts.load(Ordering::SeqCst), 1);
	// While the system prompt is up the card stays out of the way.
	assert_eq!(gate.access(), MicrophoneAccess::Ready);
	os.answer(true);
	assert_eq!(gate.access(), MicrophoneAccess::Ready);
	assert_eq!(os.prompts.load(Ordering::SeqCst), 1);
}

#[test]
fn a_refusal_shows_the_turned_off_state() {
	let os = FakeOs::new(Authorization::NotDetermined);
	let (gate, repaints) = gate(&os);
	gate.start();
	os.answer(false);
	assert_eq!(gate.access(), MicrophoneAccess::Blocked);
	assert!(
		repaints.load(Ordering::SeqCst) > 0,
		"the card must be drawn"
	);
	// macOS never prompts twice: asking again does nothing.
	gate.ask();
	assert_eq!(os.prompts.load(Ordering::SeqCst), 1);
}

#[test]
fn a_known_answer_never_prompts_at_launch() {
	for (status, access) in [
		(Authorization::Denied, MicrophoneAccess::Blocked),
		(Authorization::Restricted, MicrophoneAccess::Blocked),
		(Authorization::Authorized, MicrophoneAccess::Ready),
		(Authorization::NotRequired, MicrophoneAccess::Ready),
	] {
		let os = FakeOs::new(status);
		let (gate, _) = gate(&os);
		gate.start();
		assert_eq!(gate.access(), access, "{status:?}");
		assert_eq!(os.prompts.load(Ordering::SeqCst), 0, "{status:?}");
	}
}

#[test]
fn the_button_retries_when_the_launch_prompt_did_not_show() {
	let os = FakeOs::new(Authorization::NotDetermined);
	let (gate, _) = gate(&os);
	gate.start();
	// The prompt never answered (for instance it was dismissed by the system);
	// a second ask while it is still up does not stack another one.
	gate.ask();
	assert_eq!(os.prompts.load(Ordering::SeqCst), 1);
	os.pending.lock().unwrap().pop().unwrap()(false);
	assert_eq!(gate.access(), MicrophoneAccess::NotAsked);
	gate.ask();
	assert_eq!(os.prompts.load(Ordering::SeqCst), 2);
}

#[test]
fn a_grant_in_system_settings_is_noticed_on_the_next_refresh() {
	let os = FakeOs::new(Authorization::Denied);
	let (gate, repaints) = gate(&os);
	gate.start();
	assert_eq!(gate.access(), MicrophoneAccess::Blocked);
	let before = repaints.load(Ordering::SeqCst);
	os.set(Authorization::Authorized);
	// Nothing changes until the window is looked at again.
	assert_eq!(gate.access(), MicrophoneAccess::Blocked);
	gate.refresh();
	assert_eq!(gate.access(), MicrophoneAccess::Ready);
	assert!(repaints.load(Ordering::SeqCst) > before);
	// And a later revoke shows the card again.
	os.set(Authorization::Denied);
	gate.refresh();
	assert_eq!(gate.access(), MicrophoneAccess::Blocked);
}
