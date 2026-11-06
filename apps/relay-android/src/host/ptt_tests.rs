//! Hold-to-talk against the in-process engine: real daemon, real sockets, the
//! UI client attached the way the app attaches it, fake media and peers.
//! The gate the engine publishes is the truth: a key is only "down" when it is open.

use std::time::Instant;

use relay_core::{CallId, DeviceKeyId, PeerEvent, PeerMessage, SystemEvent};
use relay_engine::service::fakes::{FakePorts, MediaCall};
use relay_engine::service::{PeerInfo, PeerTrust};
use relay_ui::model::{Mode, Phase};

use super::tests::{FakeBridge, SERIAL, wait_for};
use super::*;

const PEER: u8 = 7;

struct Rig {
	host: Arc<Host>,
	fakes: FakePorts,
	handle: Handle,
	call_hex: String,
	call: CallId,
	_dir: tempfile::TempDir,
}

fn call_id_of(hex_id: &str) -> CallId {
	let bytes: [u8; 16] = hex::decode(hex_id).unwrap().try_into().unwrap();
	CallId::from_u128(u128::from_be_bytes(bytes))
}

impl Rig {
	/// A connected push-to-talk call, controlled by the UI client, key up.
	fn start() -> Self {
		let dir = tempfile::tempdir().unwrap();
		let fakes = FakePorts::default();
		let ports = fakes.ports();
		let config = EngineConfig {
			files_dir: dir.path().to_string_lossy().into_owned(),
			default_display_name: None,
		};
		let tweak: Tweak = Box::new(move |options: &mut RunOptions| {
			options.adapters = Some(Box::new(move |_| ports));
		});
		let host = Host::start(&config, Arc::new(FakeBridge::default()), Some(tweak)).unwrap();
		let handle = host.handle.clone();
		host.set_foreground(true);
		let mut rig = Self {
			host,
			fakes,
			handle,
			call_hex: String::new(),
			call: CallId::from_u128(0),
			_dir: dir,
		};
		let handle = rig.handle.clone();
		handle.block_on(rig.connect_call());
		rig
	}

	async fn connect_call(&mut self) {
		let hub = self.host.hub.clone();
		wait_for(&hub, "the client to connect", |s| s.link == Link::Connected).await;
		let control = self.control().await;
		let peer = DeviceKeyId::from_bytes([PEER; 32]);
		let info = PeerInfo {
			id: peer,
			display_name: "Ada".to_owned(),
			addresses: vec!["127.0.0.1:7000".to_owned()],
			trust: PeerTrust::TofuUnverified,
			reachable: true,
			avatar: None,
		};
		control.peers_changed(vec![info]).await.unwrap();
		let peer_hex = hex::encode([PEER; 32]);
		wait_for(&hub, "the peer", |s| {
			s.peers.iter().any(|p| p.peer_id == peer_hex)
		})
		.await;
		self.host
			.actions
			.controller
			.start_call(&peer_hex)
			.await
			.unwrap();
		wait_for(&hub, "the call", |s| {
			s.calls.iter().any(|c| !c.phase.is_terminal())
		})
		.await;
		self.call_hex = self.host.active_call_id().unwrap();
		self.call = call_id_of(&self.call_hex);

		control.peer(PeerEvent::LinkUp(peer)).await.unwrap();
		let session = poll(|| {
			self.fakes
				.peers
				.sent()
				.into_iter()
				.find_map(|(_, m)| match m {
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
		wait_for(&hub, "the call to connect", |s| {
			s.calls
				.iter()
				.any(|c| c.phase == Phase::Connected && c.controlled_by_me)
		})
		.await;
	}

	/// Push-to-talk consent and an open capture, as the UI's Mode switch does.
	async fn arm(&self) {
		let hub = self.host.hub.clone();
		self.host
			.actions
			.controller
			.set_transmit(&self.call_hex, Mode::PushToTalk)
			.await
			.unwrap();
		let generation = poll(|| {
			self.fakes.media.calls().into_iter().find_map(|c| match c {
				MediaCall::OpenCapture(generation) => Some(generation),
				_ => None,
			})
		})
		.await;
		self.control()
			.await
			.system(SystemEvent::CaptureReady { generation })
			.await
			.unwrap();
		wait_for(&hub, "push-to-talk consent", |s| {
			s.calls
				.iter()
				.any(|c| c.transmit.mode == Mode::PushToTalk && c.transmit.consent_granted)
		})
		.await;
	}

	async fn control(&self) -> relay_engine::service::ControlHandle {
		let daemon = self.host.daemon.lock().await;
		daemon.as_ref().unwrap().control().clone()
	}

	async fn gate_open(&self) -> bool {
		let daemon = self.host.daemon.lock().await;
		daemon.as_ref().unwrap().gates().load().is_open(self.call)
	}

	async fn expect_gate(&self, open: bool, what: &str) {
		for _ in 0..60 {
			if self.gate_open().await == open {
				return;
			}
			tokio::time::sleep(Duration::from_millis(25)).await;
		}
		panic!(
			"the gate did not become {}: {what}",
			if open { "open" } else { "closed" }
		);
	}

	async fn release_delay(&self, millis: u32) {
		self.host
			.actions
			.controller
			.set_ptt_release_delay(millis)
			.await
			.unwrap();
	}

	/// The gate must stay as it is for a while (a renewal keeps it open).
	async fn gate_stays(&self, open: bool, what: &str) {
		for _ in 0..16 {
			assert_eq!(self.gate_open().await, open, "{what}");
			tokio::time::sleep(Duration::from_millis(50)).await;
		}
	}

	fn finish(self) {
		self.host.stop();
	}
}

async fn poll<T>(mut f: impl FnMut() -> Option<T>) -> T {
	for _ in 0..200 {
		if let Some(found) = f() {
			return found;
		}
		tokio::time::sleep(Duration::from_millis(25)).await;
	}
	panic!("condition not reached in time");
}

#[test]
fn the_button_holds_the_lease_and_release_closes_the_gate() {
	let _serial = SERIAL
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	let rig = Rig::start();
	let (host, id) = (rig.host.clone(), rig.call_hex.clone());
	rig.handle.block_on(async {
		// Not a push-to-talk route yet: the press is ignored.
		host.talk(Some(id.clone()), true);
		rig.gate_stays(false, "no press without a push-to-talk route")
			.await;
		host.talk(Some(id.clone()), false);

		rig.arm().await;
		rig.release_delay(0).await;
		rig.gate_stays(false, "consent alone never transmits").await;

		host.talk(Some(id.clone()), true);
		rig.expect_gate(true, "button down").await;
		// Longer than the 400 ms lease: only renewals keep it open.
		rig.gate_stays(true, "lease renewed while held").await;
		host.talk(Some(id.clone()), false);
		rig.expect_gate(false, "button up").await;
		rig.gate_stays(false, "stays closed after release").await;

		// A second press on the same stream works.
		host.talk(Some(id.clone()), true);
		rig.expect_gate(true, "second press").await;
		host.talk(Some(id), false);
		rig.expect_gate(false, "second release").await;
	});
	rig.finish();
}

#[test]
fn a_cancelled_button_releases() {
	let _serial = SERIAL
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	let rig = Rig::start();
	let (host, id) = (rig.host.clone(), rig.call_hex.clone());
	rig.handle.block_on(async {
		rig.arm().await;
		rig.release_delay(0).await;
		host.talk(Some(id), true);
		rig.expect_gate(true, "button down").await;
		// Surface loss, pause or focus loss: no up event ever arrives.
		host.cancel_talk();
		rig.expect_gate(false, "cancelled").await;
	});
	rig.finish();
}

#[test]
fn going_to_the_background_releases_a_held_press() {
	let _serial = SERIAL
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	let rig = Rig::start();
	let (host, id) = (rig.host.clone(), rig.call_hex.clone());
	rig.handle.block_on(async {
		rig.arm().await;
		rig.release_delay(0).await;
		host.talk(Some(id), true);
		rig.expect_gate(true, "button down").await;
		// A call keeps the client attached; the key still must not stay down.
		host.set_foreground(false);
		rig.expect_gate(false, "backgrounded").await;
	});
	rig.finish();
}

#[test]
fn a_held_press_does_not_survive_the_client_detaching() {
	let _serial = SERIAL
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	let rig = Rig::start();
	let (host, id) = (rig.host.clone(), rig.call_hex.clone());
	rig.handle.block_on(async {
		rig.arm().await;
		// A long release delay: only an immediate release closes the gate in time.
		rig.release_delay(2000).await;
		host.talk(Some(id), true);
		rig.expect_gate(true, "button down").await;

		host.detach();
		let started = Instant::now();
		rig.expect_gate(false, "client detached").await;
		assert!(
			started.elapsed() < Duration::from_millis(1500),
			"detach released at once, not after the release delay"
		);
	});
	rig.finish();
}

#[test]
fn ending_the_call_releases_and_drops_the_stream() {
	let _serial = SERIAL
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	let rig = Rig::start();
	let (host, id) = (rig.host.clone(), rig.call_hex.clone());
	rig.handle.block_on(async {
		rig.arm().await;
		rig.release_delay(2000).await;
		host.talk(Some(id), true);
		rig.expect_gate(true, "button down").await;
		host.hang_up();
		rig.expect_gate(false, "call ended").await;
		let hub = host.hub.clone();
		wait_for(&hub, "the call to end", |s| {
			s.calls.iter().all(|c| c.phase.is_terminal())
		})
		.await;
	});
	rig.finish();
}

#[test]
fn closing_the_stream_releases_at_once_even_with_the_client_attached() {
	let _serial = SERIAL
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	let rig = Rig::start();
	let (host, id) = (rig.host.clone(), rig.call_hex.clone());
	rig.handle.block_on(async {
		rig.arm().await;
		rig.release_delay(2000).await;
		host.talk(Some(id.clone()), true);
		rig.expect_gate(true, "button down").await;

		host.ptt.cancel();
		let started = Instant::now();
		rig.expect_gate(false, "stream closed").await;
		assert!(
			started.elapsed() < Duration::from_millis(1500),
			"a closed stream releases without the release delay"
		);

		// The next press opens a fresh stream.
		host.talk(Some(id), true);
		rig.expect_gate(true, "press after reset").await;
	});
	rig.finish();
}

#[test]
fn a_normal_key_up_keeps_the_gate_for_the_release_delay() {
	let _serial = SERIAL
		.lock()
		.unwrap_or_else(std::sync::PoisonError::into_inner);
	let rig = Rig::start();
	let (host, id) = (rig.host.clone(), rig.call_hex.clone());
	rig.handle.block_on(async {
		rig.arm().await;
		rig.release_delay(2000).await;
		host.talk(Some(id.clone()), true);
		rig.expect_gate(true, "button down").await;
		host.talk(Some(id), false);
		rig.gate_stays(true, "the engine holds the tail for the release delay")
			.await;
	});
	rig.finish();
}
