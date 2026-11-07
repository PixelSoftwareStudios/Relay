//! The engine runs in-process on the host build: loopback network, fake audio,
//! a fake Keystore. This is the FFI surface Kotlin drives, minus the JVM.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use relay_audio::device::{Direction, FakeBackend, FakeDeviceSpec};
use relay_engine::daemon::{AudioSetup, NetworkOptions};
use relay_ui::model::Link;

use super::*;
use crate::bridge::{HostError, PickKind};

/// The host is a process-wide singleton, so host tests run one at a time.
pub(super) static SERIAL: Mutex<()> = Mutex::new(());

#[derive(Default)]
pub(super) struct FakeBridge {
	pub(super) seals: AtomicU64,
	pub(super) calls: Mutex<Vec<CallSummary>>,
	pub(super) scans: Mutex<Vec<u64>>,
}

impl HostBridge for FakeBridge {
	fn seal(&self, context: String, plain: Vec<u8>) -> Result<Vec<u8>, HostError> {
		self.seals.fetch_add(1, Ordering::Relaxed);
		let mut out = context.into_bytes();
		out.push(b'|');
		out.extend(plain.iter().map(|b| b ^ 0x5a));
		Ok(out)
	}

	fn open(&self, context: String, sealed: Vec<u8>) -> Result<Vec<u8>, HostError> {
		let prefix = format!("{context}|");
		sealed
			.strip_prefix(prefix.as_bytes())
			.map(|body| body.iter().map(|b| b ^ 0x5a).collect())
			.ok_or(HostError::Failed {
				reason: "wrong context".into(),
			})
	}

	fn copy_text(&self, _text: String) {}
	fn open_url(&self, _url: String) {}
	fn pick(&self, _request_id: u64, _kind: PickKind) {}
	fn scan_qr(&self, request_id: u64) {
		self.scans.lock().unwrap().push(request_id);
	}
	fn audio_streams_wanted(&self, _wanted: bool) {}

	fn call_changed(&self, summary: CallSummary) {
		self.calls.lock().unwrap().push(summary);
	}
	fn request_microphone(&self) {}
	fn background_changed(&self, _available: bool, _alert: String) {}
	fn request_battery_exemption(&self) {}
	fn request_notifications(&self) {}
	fn open_app_settings(&self) {}
	fn quit_app(&self) {}
}

pub(super) fn tweak() -> Tweak {
	Box::new(|options: &mut RunOptions| {
		options.network = NetworkOptions::loopback();
		options.audio = AudioSetup::Backend(Box::new(|| {
			Box::new(FakeBackend::new(vec![
				FakeDeviceSpec::new("mic", Direction::Input),
				FakeDeviceSpec::new("speaker", Direction::Output),
			]))
		}));
	})
}

pub(super) async fn wait_for(hub: &Hub, what: &str, mut done: impl FnMut(&AppState) -> bool) {
	for _ in 0..200 {
		if hub.read(&mut done) {
			return;
		}
		tokio::time::sleep(Duration::from_millis(50)).await;
	}
	panic!("timed out waiting for {what}");
}

#[test]
fn engine_starts_attaches_detaches_and_stops() {
	let _serial = SERIAL
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	let dir = tempfile::tempdir().unwrap();
	let bridge = Arc::new(FakeBridge::default());
	let config = EngineConfig {
		files_dir: dir.path().to_string_lossy().into_owned(),
		default_display_name: Some("Galaxy S25 Ultra".into()),
	};
	let host = Host::start(&config, bridge.clone(), Some(tweak())).expect("engine starts");
	assert!(crate::host::host().is_some());
	assert!(
		Host::start(&config, bridge.clone(), None).is_ok_and(|again| Arc::ptr_eq(&again, &host)),
		"a second start returns the running engine"
	);
	assert!(
		bridge.seals.load(Ordering::Relaxed) >= 1,
		"the identity was sealed"
	);
	assert!(dir.path().join("relay/data/identity.sealed").exists());
	assert!(
		!dir.path().join("relay/data/identity.insecure.key").exists(),
		"no plaintext identity file"
	);

	// Foreground attaches the UI client over the socket in filesDir.
	host.set_foreground(true);
	let hub = host.hub.clone();
	let waiter = host
		.runtime
		.lock()
		.unwrap()
		.as_ref()
		.unwrap()
		.handle()
		.clone();
	waiter.block_on(wait_for(&hub, "the client to connect", |s| {
		s.link == Link::Connected
	}));
	assert!(hub.read(|s| s.engine.is_some()));
	// A fresh install never chose a name: the device name Kotlin passed is the profile name.
	assert_eq!(
		hub.read(|s| s.settings.as_ref().map(|e| e.display_name.clone())),
		Some("Galaxy S25 Ultra".to_owned())
	);

	// Background without a call leaves the engine; foreground comes back.
	host.set_foreground(false);
	waiter.block_on(wait_for(&hub, "the client to detach", |s| {
		s.link == Link::Offline
	}));
	host.set_foreground(true);
	waiter.block_on(wait_for(&hub, "the client to reattach", |s| {
		s.link == Link::Connected
	}));

	host.stop();
	assert!(crate::host::host().is_none());
	assert!(
		!dir.path().join("run/engine.sock").exists(),
		"socket removed"
	);
}

#[test]
fn call_summary_reflects_the_active_call() {
	assert_eq!(summarize(&AppState::default()), CallSummary::idle());
	let summary = summarize(&relay_ui::mock::seed());
	assert!(summary.active && summary.incoming && !summary.muted);
	assert_eq!(summary.peer, "Ada Lovelace");
}

#[test]
fn the_shell_declares_what_a_phone_has() {
	let shell = AndroidShell::new(Arc::new(FakeBridge::default()));
	assert!(!shell.has_global_hotkey(), "no system-wide key on a phone");
	assert!(shell.has_talk_button(), "touch needs the on-screen button");
	assert!(
		shell.runs_in_background(),
		"the foreground service keeps a call alive"
	);
	assert!(!shell.hides_to_tray());
}

/// Asks the shell to scan, lets Kotlin answer with `outcome`, returns what the UI gets.
fn scan_with(
	outcome: crate::bridge::ScanOutcome,
) -> Result<Option<String>, relay_ui::qr_scan::ScanError> {
	let bridge = Arc::new(FakeBridge::default());
	let shell = Arc::new(AndroidShell::new(bridge.clone()));
	let runtime = tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
		.unwrap();
	runtime.block_on(async {
		let asking = shell.clone();
		let task = tokio::spawn(async move { asking.scan_qr().await });
		let id = loop {
			if let Some(id) = bridge.scans.lock().unwrap().first().copied() {
				break id;
			}
			tokio::time::sleep(Duration::from_millis(5)).await;
		};
		shell.complete_scan(id + 1, crate::bridge::ScanOutcome::Cancelled); // an unknown id is ignored
		shell.complete_scan(id, outcome);
		task.await.unwrap()
	})
}

#[test]
fn the_shell_has_a_scanner_only_when_kotlin_says_so() {
	let shell = AndroidShell::new(Arc::new(FakeBridge::default()));
	assert!(!shell.can_scan_qr());
	shell.set_scanner(true);
	assert!(shell.can_scan_qr());
}

#[test]
fn scanned_text_and_every_ending_reach_the_ui() {
	use crate::bridge::ScanOutcome as O;
	use relay_ui::qr_scan::ScanError as E;
	let text = scan_with(O::Text {
		text: "relay://10.0.0.9:7001#ab".into(),
	});
	assert_eq!(text, Ok(Some("relay://10.0.0.9:7001#ab".to_owned())));
	assert_eq!(scan_with(O::Cancelled), Ok(None));
	assert_eq!(scan_with(O::CameraDenied), Err(E::CameraDenied));
	assert_eq!(scan_with(O::CameraBlocked), Err(E::CameraBlocked));
	assert_eq!(scan_with(O::Unavailable), Err(E::Unavailable));
	assert_eq!(scan_with(O::Failed { reason: "x".into() }), Err(E::Failed));
}
