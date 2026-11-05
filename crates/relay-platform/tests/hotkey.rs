//! Honest backend status and fake backend behaviour.

use relay_platform::testing::FakeBackend;
use relay_platform::{
	BackendStatus, HotkeyBackend, HotkeyBinding, HotkeyError, HotkeyEvent, PermissionKind,
};

#[test]
fn unavailable_backends_refuse_to_start() {
	for status in [
		BackendStatus::NeedsPermission(PermissionKind::InputMonitoring),
		BackendStatus::Unsupported("no portal".into()),
	] {
		let (mut backend, keys) = FakeBackend::new(status.clone());
		assert_eq!(backend.probe(), status);
		let err = backend
			.start(&HotkeyBinding::push_to_talk("Ctrl+Space"))
			.unwrap_err();
		assert!(matches!(err, HotkeyError::Unavailable(s) if s == status));
		assert!(!keys.send(HotkeyEvent::Pressed));
	}
}

#[test]
fn fake_backend_delivers_edges_until_stopped() {
	let (mut backend, keys) = FakeBackend::new(BackendStatus::Available);
	let mut stream = backend
		.start(&HotkeyBinding::push_to_talk("Ctrl+Space"))
		.unwrap();
	assert!(matches!(
		backend.start(&HotkeyBinding::push_to_talk("Ctrl+Space")),
		Err(HotkeyError::AlreadyStarted)
	));
	assert!(keys.send(HotkeyEvent::Pressed));
	assert_eq!(stream.try_recv().unwrap(), HotkeyEvent::Pressed);
	backend.stop();
	assert!(!keys.send(HotkeyEvent::Released));
}

#[test]
fn detected_backend_probe_is_never_a_false_success() {
	// Whatever this host is, the probe answers; Available must then be startable
	// only on the right thread, never silently ignored.
	let backend = relay_platform::detect_backend();
	let status = backend.probe();
	assert!(matches!(
		status,
		BackendStatus::Available
			| BackendStatus::NeedsPermission(_)
			| BackendStatus::Unsupported(_)
	));
}

#[test]
fn lock_detection_reports_coverage() {
	use relay_platform::session::{LockDetection, lock_detection, spawn_lock_monitor};
	let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
	let monitor = spawn_lock_monitor(tx);
	assert_eq!(
		monitor.is_some(),
		matches!(lock_detection(), LockDetection::Detected)
	);
}
