#![cfg(unix)]
//! Push-to-talk on the desktop against a real in-process engine with fake
//! media and peers: the UI client attached the way the app attaches it, the
//! global key delivered by a fake backend. The gate the engine publishes is
//! the truth. Real key presses (the OS delivering the hotkey) are not covered.

#[path = "../../../crates/relay-engine/tests/service_support/mod.rs"]
mod service_support;

use std::sync::Arc;
use std::time::Duration;

use relay_core::{AudioGeneration, CallId, DeviceKeyId, PeerEvent, PeerMessage, SystemEvent};
use relay_desktop::host::DesktopShell;
use relay_desktop::hotkey::{HotkeyHost, HotkeyShared};
use relay_desktop::login::{LoginBackend, LoginItem};
use relay_desktop::notify::{Dispatcher, Notifier};
use relay_desktop::options::DesktopOptions;
use relay_desktop::ptt::PttControl;
use relay_engine::service::fakes::MediaCall;
use relay_engine::service::{PeerInfo, PeerTrust};
use relay_platform::hotkey::{BackendStatus, HotkeyEvent};
use relay_platform::testing::{FakeBackend, FakeKeyboard};
use relay_ui::actions::Shared;
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::hotkey_settings::HotkeyState;
use relay_ui::hub::Hub;
use relay_ui::model::{AppState, Link, Mode, Phase};
use relay_ui::relay_link::RelayLink;
use relay_ui::shell::Shell;
use relay_ui::supervisor;
use service_support::{PEER, Rig};

const KEY: &str = "Ctrl+Alt+Space";

struct Silent;

impl Notifier for Silent {
	fn incoming_call(&self, _: &str) -> Result<(), String> {
		Ok(())
	}
}

struct NoLogin;

impl LoginBackend for NoLogin {
	fn is_enabled(&self) -> Result<bool, String> {
		Ok(false)
	}
	fn enable(&self) -> Result<(), String> {
		Ok(())
	}
	fn disable(&self) -> Result<(), String> {
		Ok(())
	}
}

struct Desk {
	rig: Rig,
	hub: Arc<Hub>,
	slot: Arc<SessionSlot>,
	controller: Arc<Controller>,
	shell: Arc<DesktopShell>,
	keys: Arc<HotkeyShared>,
	host: Option<HotkeyHost>,
	keyboard: FakeKeyboard,
	call_hex: String,
	call: CallId,
	supervisor: tokio::task::JoinHandle<()>,
}

fn call_id_of(hex_id: &str) -> CallId {
	let bytes: [u8; 16] = hex::decode(hex_id).unwrap().try_into().unwrap();
	CallId::from_u128(u128::from_be_bytes(bytes))
}

async fn poll<T>(what: &str, mut f: impl FnMut() -> Option<T>) -> T {
	for _ in 0..200 {
		if let Some(found) = f() {
			return found;
		}
		tokio::time::sleep(Duration::from_millis(25)).await;
	}
	panic!("timed out waiting for {what}");
}

impl Desk {
	/// A connected call, controlled by the UI client, in the default mode,
	/// with the hotkey registered.
	async fn start() -> Self {
		let rig = Rig::start().await;
		let endpoint = relay_api::Endpoint::Unix(rig.socket.clone());
		let hub = Arc::new(Hub::default());
		let slot = Arc::new(SessionSlot::default());
		let shared = Arc::new(Shared::default());
		let controller = Arc::new(Controller::new(hub.clone(), slot.clone()));
		let ptt = {
			let endpoint = endpoint.clone();
			Arc::new(PttControl::new(
				hub.clone(),
				tokio::runtime::Handle::current(),
				Box::new(move || Ok(endpoint.clone())),
			))
		};
		let keys = Arc::new(HotkeyShared::default());
		let options = DesktopOptions::new(
			None,
			Arc::new(Dispatcher::new(Arc::new(Silent))),
			LoginItem::new(Ok(Arc::new(NoLogin) as Arc<dyn LoginBackend>)),
			Box::new(Vec::new),
		);
		let shell = Arc::new(
			DesktopShell::new(egui::Context::default(), Arc::new(options))
				.with_push_to_talk(ptt.clone(), keys.clone()),
		);
		let engine = Arc::new(RelayLink::new(move || Ok(endpoint.clone())));
		let dyn_shell: Arc<dyn Shell> = shell.clone();
		let supervisor = tokio::spawn(supervisor::supervise(
			engine,
			hub.clone(),
			slot.clone(),
			dyn_shell,
			shared.autostart.clone(),
		));
		let (backend, keyboard) = FakeBackend::new(BackendStatus::Available);
		let host = HotkeyHost::new(
			Box::new(backend),
			keys.clone(),
			ptt,
			tokio::runtime::Handle::current(),
		);
		let mut desk = Self {
			rig,
			hub,
			slot,
			controller,
			shell,
			keys,
			host: Some(host),
			keyboard,
			call_hex: String::new(),
			call: CallId::from_u128(0),
			supervisor,
		};
		desk.connect_call().await;
		desk.controller.set_ptt_release_delay(0).await.unwrap();
		desk.shell.hotkey_accelerator(KEY);
		desk.host().sync();
		desk
	}

	async fn wait(&self, what: &str, mut done: impl FnMut(&AppState) -> bool) {
		poll(what, || self.hub.read(&mut done).then_some(())).await;
	}

	async fn connect_call(&mut self) {
		self.wait("the client to connect", |s| s.link == Link::Connected)
			.await;
		let control = self.rig.daemon.control().clone();
		let info = PeerInfo {
			id: DeviceKeyId::from_bytes([PEER; 32]),
			display_name: "Ada".to_owned(),
			addresses: vec!["127.0.0.1:7000".to_owned()],
			trust: PeerTrust::TofuUnverified,
			reachable: true,
			avatar: None,
			key_change: None,
		};
		control.peers_changed(vec![info]).await.unwrap();
		let peer_hex = hex::encode([PEER; 32]);
		self.wait("the peer", |s| {
			s.peers.iter().any(|p| p.peer_id == peer_hex)
		})
		.await;
		self.controller.start_call(&peer_hex).await.unwrap();
		self.wait("the call", |s| {
			s.calls.iter().any(|c| !c.phase.is_terminal())
		})
		.await;
		self.call_hex = self
			.hub
			.read(|s| {
				s.calls
					.iter()
					.find(|c| !c.phase.is_terminal())
					.map(|c| c.call_id.clone())
			})
			.unwrap();
		self.call = call_id_of(&self.call_hex);
		control
			.peer(PeerEvent::LinkUp(DeviceKeyId::from_bytes([PEER; 32])))
			.await
			.unwrap();
		let fakes = &self.rig.fakes;
		let session = poll("the invite", || {
			fakes.peers.sent().into_iter().find_map(|(_, m)| match m {
				PeerMessage::Invite { session, .. } => Some(session),
				_ => None,
			})
		})
		.await;
		let call = self.call;
		control
			.peer(PeerEvent::Accepted { call, session })
			.await
			.unwrap();
		control
			.system(SystemEvent::MediaPrepared { call, session })
			.await
			.unwrap();
		control
			.peer(PeerEvent::PathValidated { call, session })
			.await
			.unwrap();
		self.wait("the call to connect", |s| {
			s.calls
				.iter()
				.any(|c| c.phase == Phase::Connected && c.controlled_by_me)
		})
		.await;
	}

	/// Choose a mode the way the call card does and complete the capture open.
	async fn choose(&self, mode: Mode) {
		let opens = self.opens();
		self.controller
			.set_transmit(&self.call_hex, mode)
			.await
			.unwrap();
		if mode != Mode::Muted {
			let generation = poll("the capture to open", || {
				self.opens().into_iter().nth(opens.len())
			})
			.await;
			self.rig
				.daemon
				.control()
				.system(SystemEvent::CaptureReady { generation })
				.await
				.unwrap();
			self.wait("consent", |s| {
				s.calls
					.iter()
					.any(|c| c.transmit.mode == mode && c.transmit.consent_granted)
			})
			.await;
		}
	}

	fn opens(&self) -> Vec<AudioGeneration> {
		self.rig
			.fakes
			.media
			.calls()
			.into_iter()
			.filter_map(|c| match c {
				MediaCall::OpenCapture(generation) => Some(generation),
				_ => None,
			})
			.collect()
	}

	fn mode(&self) -> Mode {
		self.hub.read(|s| {
			s.calls
				.iter()
				.find(|c| c.call_id == self.call_hex)
				.map(|c| c.transmit.mode)
				.unwrap()
		})
	}

	fn gate_open(&self) -> bool {
		self.rig.daemon.gates().load().is_open(self.call)
	}

	async fn expect_gate(&self, open: bool, what: &str) {
		poll(what, || (self.gate_open() == open).then_some(())).await;
	}

	async fn gate_stays(&self, open: bool, what: &str) {
		for _ in 0..16 {
			assert_eq!(self.gate_open(), open, "{what}");
			tokio::time::sleep(Duration::from_millis(50)).await;
		}
	}

	fn host(&mut self) -> &mut HotkeyHost {
		self.host.as_mut().unwrap()
	}

	fn press(&self) {
		assert!(self.keyboard.send(HotkeyEvent::Pressed));
	}

	fn release(&self) {
		assert!(self.keyboard.send(HotkeyEvent::Released));
	}
}

impl Drop for Desk {
	fn drop(&mut self) {
		self.supervisor.abort();
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_hotkey_opens_the_gate_while_held_and_closes_it_on_release() {
	let desk = Desk::start().await;
	assert_eq!(
		desk.shell.hotkey_state(),
		HotkeyState::Registered(KEY.into())
	);
	desk.choose(Mode::PushToTalk).await;
	desk.gate_stays(false, "consent alone never transmits")
		.await;

	desk.press();
	desk.expect_gate(true, "key down").await;
	desk.gate_stays(true, "the lease is renewed past 400 ms")
		.await;
	desk.release();
	desk.expect_gate(false, "key up").await;
	desk.gate_stays(false, "stays closed").await;

	desk.press();
	desk.expect_gate(true, "second press").await;
	desk.release();
	desk.expect_gate(false, "second release").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_on_screen_button_presses_the_same_way() {
	let desk = Desk::start().await;
	desk.choose(Mode::PushToTalk).await;
	assert!(desk.shell.has_talk_button() && desk.shell.has_global_hotkey());
	desk.shell.push_to_talk(&desk.call_hex, true);
	desk.expect_gate(true, "button down").await;
	desk.shell.push_to_talk(&desk.call_hex, false);
	desk.expect_gate(false, "button up").await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_key_in_a_muted_call_does_nothing_and_changes_nothing() {
	let desk = Desk::start().await;
	assert_eq!(desk.mode(), Mode::Muted);
	desk.press();
	desk.gate_stays(false, "muted stays muted").await;
	desk.release();
	assert_eq!(desk.mode(), Mode::Muted, "the key never changes the mode");
	assert_eq!(
		desk.shell.hotkey_state(),
		HotkeyState::Registered(KEY.into())
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_key_in_an_open_mic_call_leaves_the_mode_and_the_gate_alone() {
	let desk = Desk::start().await;
	desk.choose(Mode::Unmuted).await;
	desk.expect_gate(true, "open mic transmits").await;
	desk.press();
	desk.gate_stays(true, "still open").await;
	desk.release();
	desk.gate_stays(true, "release does not close an open mic")
		.await;
	assert_eq!(desk.mode(), Mode::Unmuted);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_mode_change_ends_a_held_press() {
	let desk = Desk::start().await;
	desk.choose(Mode::PushToTalk).await;
	desk.controller.set_ptt_release_delay(2000).await.unwrap();
	desk.press();
	desk.expect_gate(true, "key down").await;
	desk.controller
		.set_transmit(&desk.call_hex, Mode::Muted)
		.await
		.unwrap();
	desk.expect_gate(false, "muted while held").await;
	// The worker notices within a renewal period and lets go of the key.
	tokio::time::sleep(Duration::from_millis(500)).await;
	// Back in push-to-talk the key still being down sends nothing by itself.
	desk.choose(Mode::PushToTalk).await;
	desk.gate_stays(false, "no press without a new key down")
		.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_end_of_the_call_and_a_detached_client_release_at_once() {
	let desk = Desk::start().await;
	desk.choose(Mode::PushToTalk).await;
	desk.controller.set_ptt_release_delay(2000).await.unwrap();

	desk.press();
	desk.expect_gate(true, "key down").await;
	let started = std::time::Instant::now();
	desk.hub.update(|s| s.set_link_down(Link::Offline, None));
	desk.expect_gate(false, "client detached").await;
	assert!(
		started.elapsed() < Duration::from_millis(1500),
		"released without the release delay"
	);
	desk.slot.clear();
	desk.release();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ending_the_call_releases_a_held_key() {
	let desk = Desk::start().await;
	desk.choose(Mode::PushToTalk).await;
	desk.controller.set_ptt_release_delay(2000).await.unwrap();
	desk.press();
	desk.expect_gate(true, "key down").await;
	desk.controller.end_call(&desk.call_hex).await.unwrap();
	desk.expect_gate(false, "call ended").await;
	desk.wait("the call to end", |s| {
		s.calls.iter().all(|c| c.phase.is_terminal())
	})
	.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_key_follows_the_setting() {
	let mut desk = Desk::start().await;
	desk.choose(Mode::PushToTalk).await;

	// A new key is registered again (the fake backend allows one start at a time).
	desk.shell.hotkey_accelerator("Ctrl+Shift+F9");
	desk.host().sync();
	assert_eq!(
		desk.shell.hotkey_state(),
		HotkeyState::Registered("Ctrl+Shift+F9".into())
	);
	desk.press();
	desk.expect_gate(true, "the rebound key works").await;

	// Rebinding while held releases at once.
	desk.shell.hotkey_accelerator(KEY);
	desk.host().sync();
	desk.expect_gate(false, "rebinding released the key").await;

	// An empty key turns it off too.
	desk.shell.hotkey_accelerator("");
	desk.host().sync();
	assert_eq!(desk.shell.hotkey_state(), HotkeyState::Off);
	assert!(desk.keys.state() == HotkeyState::Off);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn dropping_the_host_releases_a_held_key() {
	let mut desk = Desk::start().await;
	desk.choose(Mode::PushToTalk).await;
	desk.controller.set_ptt_release_delay(2000).await.unwrap();
	desk.press();
	desk.expect_gate(true, "key down").await;
	// The app quitting drops the host.
	desk.host = None;
	desk.expect_gate(false, "the quit released the key").await;
}
