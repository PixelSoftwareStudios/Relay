#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime};

use relay_audio::device::{
	AudioBackend, CaptureStream, DeviceHandle, Direction, Enumeration, FakeBackend,
	FakeCaptureDriver, FakeDeviceSpec, FakePlaybackDriver, PlaybackStream, Signal, StreamRequest,
};
use relay_core::testing::SeqIds;
use relay_core::*;
use relay_engine::media::{MediaConfig, MediaEvent, MediaSystem, Selection};
use relay_engine::peers::{PeerHandle, PeerOutput, SharedTrust, peer_id, spawn};
use relay_net::AddressPolicy;
use relay_net::identity::{Identity, TrustRecord, TrustState};
use tokio::sync::mpsc;
use tokio::time::sleep;

#[path = "../support_peers/mod.rs"]
pub mod peers_support;
use peers_support::fast_config;

/// A backend several threads can reach: the device thread opens streams,
/// the simulation thread plays the hardware.
#[derive(Clone)]
pub struct SharedBackend(pub Arc<Mutex<FakeBackend>>);

impl AudioBackend for SharedBackend {
	fn enumerate(&mut self) -> relay_audio::Result<Enumeration> {
		self.0.lock().unwrap().enumerate()
	}
	fn open_capture(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> relay_audio::Result<CaptureStream> {
		self.0.lock().unwrap().open_capture(device, request)
	}
	fn open_playback(
		&mut self,
		device: DeviceHandle,
		request: &StreamRequest,
	) -> relay_audio::Result<PlaybackStream> {
		self.0.lock().unwrap().open_playback(device, request)
	}
}

/// Plays the audio hardware: feeds the microphone and drains the speaker in
/// real time, 10 ms at a time.
pub struct Sim {
	backend: SharedBackend,
	pub capture: Arc<Mutex<Option<FakeCaptureDriver>>>,
	pub playback: Arc<Mutex<Option<FakePlaybackDriver>>>,
	signal: Arc<Mutex<Signal>>,
	stop: Arc<AtomicBool>,
	join: Option<JoinHandle<()>>,
}

impl Sim {
	pub fn start(backend: SharedBackend, signal: Signal) -> Self {
		let capture = Arc::new(Mutex::new(None));
		let playback = Arc::new(Mutex::new(None));
		let signal = Arc::new(Mutex::new(signal));
		let stop = Arc::new(AtomicBool::new(false));
		let join = {
			let (backend, capture, playback, signal, stop) = (
				backend.clone(),
				capture.clone(),
				playback.clone(),
				signal.clone(),
				stop.clone(),
			);
			thread::spawn(move || {
				let mut next = Instant::now();
				while !stop.load(Ordering::Acquire) {
					{
						let mut hw = backend.0.lock().unwrap();
						if let Some(mut driver) = hw.take_capture_driver() {
							driver.set_signal(*signal.lock().unwrap());
							*capture.lock().unwrap() = Some(driver);
						}
						if let Some(mut driver) = hw.take_playback_driver() {
							driver.enable_recording(48_000 * 20);
							*playback.lock().unwrap() = Some(driver);
						}
					}
					if let Some(driver) = capture.lock().unwrap().as_mut() {
						driver.run(480);
					}
					if let Some(driver) = playback.lock().unwrap().as_mut() {
						driver.render(480);
					}
					next += Duration::from_millis(10);
					thread::sleep(next.saturating_duration_since(Instant::now()));
				}
			})
		};
		Self {
			backend,
			capture,
			playback,
			signal,
			stop,
			join: Some(join),
		}
	}

	pub fn unplug_microphone(&self) {
		if let Some(driver) = self.capture.lock().unwrap().as_mut() {
			driver.unplug();
		}
	}

	/// Everything the speaker played so far.
	pub fn heard(&self) -> Vec<f32> {
		self.playback
			.lock()
			.unwrap()
			.as_ref()
			.map(|d| d.recorded().to_vec())
			.unwrap_or_default()
	}
}

impl Drop for Sim {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Release);
		if let Some(join) = self.join.take() {
			let _ = join.join();
		}
		let _ = &self.backend;
		let _ = &self.signal;
	}
}

/// One whole engine: domain state, peers, media and fake hardware.
pub struct Node {
	pub id: PeerId,
	pub identity: Arc<Identity>,
	pub state: State,
	pub peers: PeerHandle,
	pub media: MediaSystem,
	pub sim: Sim,
	pub ids: SeqIds,
	pub client: ClientId,
	pub effects: Vec<Effect>,
	pub trust: SharedTrust,
	peer_out: mpsc::Receiver<PeerOutput>,
	media_rx: Receiver<MediaEvent>,
	started: Instant,
}

impl Node {
	pub async fn new(byte: u8, tone_hz: f32) -> Self {
		Self::with(byte, tone_hz, |_| {}).await
	}

	pub async fn with(byte: u8, tone_hz: f32, tweak: impl FnOnce(&mut MediaConfig)) -> Self {
		let identity = Arc::new(Identity::generate().unwrap());
		let id = peer_id(&identity.fingerprint());
		let mut config = fast_config(&format!("node{byte}"));
		config.listen = "127.0.0.1:0".parse().unwrap();
		let trust = SharedTrust::in_memory();
		let (peers, peer_out) = spawn(config, identity.clone(), trust.clone(), [byte; 16])
			.await
			.unwrap();

		let mut mic = FakeDeviceSpec::new("mic", Direction::Input);
		mic.is_default = true;
		let mut spk = FakeDeviceSpec::new("spk", Direction::Output);
		spk.is_default = true;
		let backend = SharedBackend(Arc::new(Mutex::new(FakeBackend::new(vec![mic, spk]))));
		let sim = Sim::start(
			backend.clone(),
			Signal::Sine {
				freq_hz: tone_hz,
				amplitude: 0.5,
			},
		);
		let (tx, media_rx) = channel();
		let tx = Mutex::new(tx);
		let events: relay_engine::media::EventSink = Arc::new(move |event| {
			let _ = tx.lock().unwrap().send(event);
		});
		let mut media_config = MediaConfig::new(Selection::default());
		media_config.bind_ip = [127, 0, 0, 1].into();
		media_config.policy = AddressPolicy::lan_and_loopback();
		tweak(&mut media_config);
		let media = MediaSystem::start(
			media_config,
			Box::new(move || Box::new(backend)),
			None,
			peers.clone(),
			events,
			tokio::runtime::Handle::current(),
		)
		.unwrap();

		let mut ids = SeqIds::namespaced(byte);
		let mut state = State::new(id, ids.incarnation(), Limits::default()).unwrap();
		let client = ids.client();
		let attach = Request {
			client,
			operation: ids.operation(),
			command: Command::Attach,
		};
		state.apply(Input::Client(attach), MonoTime::ZERO);
		Self {
			id,
			identity,
			state,
			peers,
			media,
			sim,
			ids,
			client,
			effects: Vec::new(),
			trust,
			peer_out,
			media_rx,
			started: Instant::now(),
		}
	}

	/// Trusts `other` as a user would after comparing fingerprints.
	pub fn trust(&self, other: &Node) {
		self.trust
			.lock()
			.put(TrustRecord {
				fingerprint: other.identity.fingerprint(),
				state: TrustState::UnverifiedFirstUse,
				display_name: "peer".into(),
				trusted_at: SystemTime::now(),
			})
			.unwrap();
		self.peers
			.add_address(other.id, other.peers.local_addr())
			.unwrap();
	}

	fn now(&self) -> MonoTime {
		MonoTime::from_millis(self.started.elapsed().as_millis() as u64)
	}

	/// Applies an input and carries out its effects, as the control actor does.
	pub fn feed(&mut self, input: Input) -> Outcome {
		let outcome = self.state.apply(input, self.now());
		for effect in &outcome.effects {
			self.effects.push(*effect);
			match *effect {
				Effect::GateUpdate { gate, revision } => self.media.gates().publish(gate, revision),
				Effect::ReceiveMix(mix) => self.media.set_receive_mix(mix),
				Effect::OpenCapture { generation } => self.media.open_capture(generation),
				Effect::CloseCapture { generation } => self.media.close_capture(generation),
				Effect::EnsureLink { peer } => {
					let _ = self.peers.ensure_link(peer);
				}
				Effect::Peer { peer, message } => {
					let _ = self.peers.send(peer, message);
				}
				Effect::PrepareMedia { call, session } => {
					if let Err(error) = self.media.prepare(call, session) {
						eprintln!("prepare refused: {error}");
					}
				}
				Effect::StopMedia { call, session } => self.media.stop(call, session),
			}
		}
		outcome
	}

	pub fn command(&mut self, command: Command) -> OpResult {
		let request = Request {
			client: self.client,
			operation: self.ids.operation(),
			command,
		};
		self.feed(Input::Client(request)).result
	}

	/// Moves adapter completions into the domain.
	pub fn poll(&mut self) {
		while let Ok(event) = self.media_rx.try_recv() {
			match event {
				MediaEvent::System(event) => self.feed(Input::System(event)),
				MediaEvent::Peer(event) => self.feed(Input::Peer(event)),
				MediaEvent::OutputLost
				| MediaEvent::CaptureUnavailable(_)
				| MediaEvent::CaptureOpened
				| MediaEvent::WorkerFailed(_) => continue,
			};
		}
		while let Ok(output) = self.peer_out.try_recv() {
			match output {
				PeerOutput::Core(event) => {
					self.feed(Input::Peer(event));
				}
				PeerOutput::Media { peer, signal } => self.media.handle_signal(peer, signal),
				_ => {}
			}
		}
		self.feed(Input::Tick);
	}

	pub fn call_state(&self, call: CallId) -> Option<CallState> {
		self.state.call(call).map(|c| c.state)
	}

	pub fn gate_open(&self, call: CallId) -> bool {
		self.state
			.snapshot()
			.calls
			.get(&call)
			.is_some_and(|c| c.gate.open)
	}
}

/// Pumps both nodes until `done` holds or five seconds pass.
pub async fn until(a: &mut Node, b: &mut Node, what: &str, done: impl Fn(&Node, &Node) -> bool) {
	let deadline = Instant::now() + Duration::from_secs(8);
	while !done(a, b) {
		assert!(Instant::now() < deadline, "timed out waiting for: {what}");
		a.poll();
		b.poll();
		sleep(Duration::from_millis(5)).await;
	}
}

/// Pumps both nodes for a fixed time.
pub async fn run_for(a: &mut Node, b: &mut Node, time: Duration) {
	let end = Instant::now() + time;
	while Instant::now() < end {
		a.poll();
		b.poll();
		sleep(Duration::from_millis(5)).await;
	}
}

pub struct Pair {
	pub a: Node,
	pub b: Node,
	pub call: CallId,
	pub lease_a: ControllerLeaseId,
	pub lease_b: ControllerLeaseId,
}

pub async fn active_call_with(tweak_a: impl FnOnce(&mut MediaConfig)) -> Pair {
	let mut a = Node::with(1, 440.0, tweak_a).await;
	let mut b = Node::new(2, 1_700.0).await;
	a.trust(&b);
	b.trust(&a);
	let (call, session) = (a.ids.call(), a.ids.session());
	let peer = b.id;
	let lease_a = match a.command(Command::StartCall {
		call,
		session,
		peer,
		allow_parallel_same_peer: false,
	}) {
		Ok(Reply::Controller(lease)) => lease,
		other => panic!("{other:?}"),
	};
	until(&mut a, &mut b, "incoming call rings", |_, b| {
		b.call_state(call) == Some(CallState::IncomingRinging)
	})
	.await;
	let lease_b = match b.command(Command::AcquireController {
		call,
		takeover: false,
	}) {
		Ok(Reply::Controller(lease)) => lease,
		other => panic!("{other:?}"),
	};
	b.command(Command::AcceptCall {
		call,
		lease: lease_b,
	})
	.unwrap();
	until(&mut a, &mut b, "both sides active", |a, b| {
		a.call_state(call) == Some(CallState::Active)
			&& b.call_state(call) == Some(CallState::Active)
	})
	.await;
	Pair {
		a,
		b,
		call,
		lease_a,
		lease_b,
	}
}

pub async fn active_call() -> Pair {
	active_call_with(|_| {}).await
}
