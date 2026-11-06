//! The media system: workers, device thread, per-call lifecycle and the
//! control-side entry points used by the service adapters.

use relay_io::LockExt;
use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use relay_audio::FRAME_SAMPLES;
use relay_audio::codec::EncoderConfig;
use relay_audio::device::{DeviceDescriptor, LOW_LATENCY_BUFFER_FRAMES, StreamRequest};
use relay_audio::dsp::{DspLimits, NoiseSuppressor, OffSuppressor};
use relay_core::{AudioGeneration, CallId, PeerId, ReceiveMix, SessionId, SystemEvent};
use relay_net::AddressPolicy;
use relay_protocol::jitter::JitterConfig;
use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use super::capture_worker::{self, CaptureCmd};
use super::devices::{BackendFactory, DeviceCmd, DeviceSetup, DeviceThread, Selection};
use super::diag::{DeviceBuffers, PathStatus};
use super::events::{EventSink, MediaEvent};
use super::gate::GateBoard;
use super::inbox::Inbox;
use super::live::{MicTest, filter_events};
use super::load::{AdmissionLimits, LoadMeter, LoadSnapshot, Refusal};
use super::meters::{Meters, monitor_pair};
use super::net::{EndpointSink, PeerSocket};

use super::render_worker::{self, RenderCmd};
use super::send::{CallSender, StreamIdentity};
use super::state::{CallMedia, CallReport, PeerMedia, Phase, State};
use super::supervise::{
	HANG_AFTER, Heartbeat, Supervision, Watched, WorkerFailure, WorkerId, watchdog,
};
use crate::peers::{MediaSignal, PeerHandle};

const COMMAND_DEPTH: usize = 64;

/// Media configuration.
#[derive(Clone)]
pub struct MediaConfig {
	/// Device selection.
	pub selection: Selection,
	/// Stream parameters requested from the backend.
	pub request: StreamRequest,
	/// Per-call encoder settings.
	pub encoder: EncoderConfig,
	/// Receive scheduling profile.
	pub jitter: JitterConfig,
	/// Admission limits.
	pub limits: AdmissionLimits,
	/// Playback FIFO fill target in milliseconds.
	pub playout_target_ms: u32,
	/// Address media sockets bind to.
	pub bind_ip: IpAddr,
	/// Address policy for media paths.
	pub policy: AddressPolicy,
	/// The control actor's published gates; when set, send paths read them
	/// directly and `GateBoard::publish` is not needed.
	pub gates: Option<crate::service::GateReader>,
	/// How long a microphone test runs before it stops by itself.
	pub mic_test_max: std::time::Duration,
	/// Ask the OS to promote the capture and render workers (QoS, MMCSS,
	/// real-time scheduling). Failure is logged and never fatal.
	pub promote_workers: bool,
}

impl MediaConfig {
	/// LAN defaults with an explicit selection.
	pub fn new(selection: Selection) -> Self {
		Self {
			selection,
			request: StreamRequest {
				buffer_frames: Some(LOW_LATENCY_BUFFER_FRAMES),
				..StreamRequest::default()
			},
			encoder: EncoderConfig::default(),
			jitter: JitterConfig::BALANCED,
			limits: AdmissionLimits::default(),
			playout_target_ms: 20,
			bind_ip: IpAddr::from([0, 0, 0, 0]),
			policy: AddressPolicy::lan_only(),
			gates: None,
			mic_test_max: super::live::MIC_TEST_MAX,
			promote_workers: true,
		}
	}
}

/// Why media could not be prepared.
#[derive(Debug, Error)]
pub enum PrepareError {
	#[error("refused: {0}")]
	Refused(#[from] Refusal),
	#[error("media for this call already exists")]
	Duplicate,
	#[error("no peer is known for this call")]
	UnknownCall,
	#[error("media is shutting down")]
	Stopped,
}

pub(super) struct Inner {
	pub(super) config: MediaConfig,
	pub(super) events: EventSink,
	pub(super) board: Arc<GateBoard>,
	pub(super) capture: Inbox<CaptureCmd>,
	pub(super) render: Inbox<RenderCmd>,
	pub(super) tones: super::tones::Tones,
	pub(super) devices: DeviceThread,
	pub(super) meters: Arc<Meters>,
	pub(super) mic_test: Arc<MicTest>,
	/// Buffer sizes of the open streams, for diagnostics.
	pub(super) buffers: Arc<DeviceBuffers>,
	/// A domain-owned capture generation is open or requested.
	pub(super) domain_capture: AtomicBool,
	capture_load: Arc<LoadMeter>,
	render_load: Arc<LoadMeter>,
	pub(super) stop: Arc<AtomicBool>,
	workers: Mutex<Vec<JoinHandle<()>>>,
	pub(super) peers: PeerHandle,
	pub(super) runtime: Handle,
	origin: Instant,
	state: Mutex<State>,
}

/// The media system. Cheap to clone; all methods are non-blocking.
#[derive(Clone)]
pub struct MediaSystem {
	pub(super) inner: Arc<Inner>,
}

impl Drop for Inner {
	fn drop(&mut self) {
		self.stop.store(true, Ordering::Release);
		let workers = std::mem::take(&mut *self.workers.lock_unpoisoned());
		for worker in workers {
			worker.thread().unpark();
			let name = worker.thread().name().unwrap_or("media worker").to_owned();
			if worker.join().is_err() {
				tracing::error!(%name, "media worker ended with a panic outside supervision");
			}
		}
	}
}

impl MediaSystem {
	/// Starts the worker threads and the device thread.
	pub fn start(
		config: MediaConfig,
		backend: BackendFactory,
		suppressor: Option<Box<dyn NoiseSuppressor>>,
		peers: PeerHandle,
		events: EventSink,
		runtime: Handle,
	) -> std::io::Result<Self> {
		let mut suppressor = suppressor.unwrap_or_else(|| Box::new(OffSuppressor::default()));
		let limits = DspLimits {
			max_block_samples: FRAME_SAMPLES,
			max_scratch_bytes: suppressor.descriptor().max_scratch_bytes,
		};
		suppressor
			.prepare(&limits)
			.map_err(|e| std::io::Error::other(e.to_string()))?;
		let stop = Arc::new(AtomicBool::new(false));
		let (capture_load, render_load) = (
			Arc::new(LoadMeter::default()),
			Arc::new(LoadMeter::default()),
		);
		let meters = Arc::new(Meters::default());
		let mic_test = Arc::new(MicTest::default());
		let events = filter_events(events, Arc::clone(&mic_test), Arc::clone(&meters));
		let (monitor_tx, monitor_rx) = monitor_pair();
		let (capture, capture_rx) = Inbox::new(COMMAND_DEPTH);
		let (render, render_rx) = Inbox::new(COMMAND_DEPTH);
		let report: Arc<dyn Fn(WorkerFailure) + Send + Sync> = {
			let events = Arc::clone(&events);
			Arc::new(move |failure| events(MediaEvent::WorkerFailed(failure)))
		};
		let supervision = |id| Supervision {
			id,
			heartbeat: Arc::new(Heartbeat::default()),
			failed: Arc::new(AtomicBool::new(false)),
			stop: Arc::clone(&stop),
			report: Arc::clone(&report),
			promote: config.promote_workers,
		};
		let (capture_sup, render_sup) = (
			supervision(WorkerId::Capture),
			supervision(WorkerId::Render),
		);
		let watched = [&capture_sup, &render_sup]
			.map(|s| Watched {
				id: s.id,
				heartbeat: Arc::clone(&s.heartbeat),
				failed: Arc::clone(&s.failed),
			})
			.into();
		let capture_thread = capture_worker::spawn(
			capture_rx,
			suppressor,
			Arc::clone(&events),
			Arc::clone(&capture_load),
			Arc::clone(&meters),
			monitor_tx,
			capture_sup,
		)?;
		capture.set_waker(capture_thread.thread().clone());
		let render_thread = render_worker::spawn(
			render_rx,
			Arc::clone(&events),
			Arc::clone(&render_load),
			Arc::clone(&meters),
			monitor_rx,
			render_sup,
		)?;
		render.set_waker(render_thread.thread().clone());
		let workers = vec![capture_thread, render_thread];
		runtime.spawn(watchdog(
			watched,
			Arc::clone(&stop),
			report,
			Duration::from_secs(1),
			HANG_AFTER,
		));
		let buffers = Arc::new(DeviceBuffers::default());
		let devices = DeviceThread::spawn(
			backend,
			DeviceSetup {
				request: config.request,
				selection: config.selection.clone(),
				playout_target_ms: config.playout_target_ms,
				buffers: Arc::clone(&buffers),
			},
			capture.clone(),
			render.clone(),
			Arc::clone(&events),
		)?;
		let (tones, tone_player) = super::tones::Tones::new();
		let board = config
			.gates
			.clone()
			.map_or_else(GateBoard::new, GateBoard::following);
		let system = Self {
			inner: Arc::new(Inner {
				config,
				events,
				board,
				capture,
				render,
				tones,
				devices,
				meters,
				mic_test,
				buffers,
				domain_capture: AtomicBool::new(false),
				capture_load,
				render_load,
				stop,
				workers: Mutex::new(workers),
				peers,
				runtime,
				origin: Instant::now(),
				state: Mutex::new(State::default()),
			}),
		};
		system.install_tone_player(tone_player);
		system
			.inner
			.runtime
			.spawn(super::rekey::follow(Arc::downgrade(&system.inner)));
		Ok(system)
	}

	pub(super) fn state(&self) -> MutexGuard<'_, State> {
		self.inner.state.lock_unpoisoned()
	}

	pub(super) fn emit(&self, event: MediaEvent) {
		(self.inner.events)(event);
	}

	/// The board the domain's `GateUpdate` effects must be published to.
	pub fn gates(&self) -> Arc<GateBoard> {
		Arc::clone(&self.inner.board)
	}

	/// The latest device enumeration.
	pub fn device_listing(&self) -> Arc<Vec<DeviceDescriptor>> {
		self.inner.devices.listing.load_full()
	}

	/// Asks the device thread to open the microphone under `generation`.
	pub fn open_capture(&self, generation: AudioGeneration) {
		self.inner.domain_capture.store(true, Ordering::Release);
		if self
			.inner
			.devices
			.commands
			.send(DeviceCmd::OpenCapture(generation))
			.is_err()
		{
			// The device thread is gone: tell the domain instead of leaving
			// the open to time out.
			self.emit(MediaEvent::System(SystemEvent::CaptureFailed {
				generation,
			}));
		}
	}

	/// Asks the device thread to close the microphone.
	pub fn close_capture(&self, generation: AudioGeneration) {
		self.inner.domain_capture.store(false, Ordering::Release);
		let _ = self
			.inner
			.devices
			.commands
			.send(DeviceCmd::CloseCapture(generation));
	}

	/// Applies an explicit device selection.
	pub fn select_devices(&self, selection: Selection) {
		let _ = self
			.inner
			.devices
			.commands
			.send(DeviceCmd::Select(Box::new(selection)));
	}

	/// Updates a call's gain and audibility.
	pub fn set_receive_mix(&self, mix: ReceiveMix) {
		if self.inner.render.push(RenderCmd::Mix(mix)).is_err() {
			tracing::warn!("render queue full; receive mix dropped");
		}
	}

	/// Current load as admission sees it.
	pub fn load(&self) -> LoadSnapshot {
		LoadSnapshot {
			calls: self.state().calls.len(),
			capture_ns: self.inner.capture_load.ns_per_frame(),
			render_ns: self.inner.render_load.ns_per_frame(),
		}
	}

	/// Counters of one call, for tests and diagnostics.
	pub fn report(&self, call: CallId) -> Option<CallReport> {
		use std::sync::atomic::Ordering::Relaxed;
		let state = self.state();
		let media = state.calls.get(&call)?;
		let (send, recv) = (media.send_stats.as_ref(), media.recv_stats.as_ref());
		Some(CallReport {
			phase: media.phase,
			sent: send.map_or(0, |s| s.sent.load(Relaxed)),
			gate_closed: send.map_or(0, |s| s.gate_closed.load(Relaxed)),
			stale: send.map_or(0, |s| s.stale.load(Relaxed)),
			received_voice: recv.map_or(0, |s| s.voice.load(Relaxed)),
			received_concealed: recv.map_or(0, |s| s.concealed.load(Relaxed)),
			misbound: recv.map_or(0, |s| s.misbound.load(Relaxed)),
		})
	}

	/// Admits and starts preparing media for a call.
	///
	/// Admission looks only at measured load and the call count; a refusal
	/// leaves every running call untouched.
	pub fn prepare(&self, call: CallId, session: SessionId) -> Result<(), PrepareError> {
		let peer = self
			.inner
			.peers
			.peer_of(call)
			.ok_or(PrepareError::UnknownCall)?;
		if self.inner.stop.load(Ordering::Acquire) {
			return Err(PrepareError::Stopped);
		}
		{
			let mut state = self.state();
			if state.calls.contains_key(&call) {
				return Err(PrepareError::Duplicate);
			}
			let load = LoadSnapshot {
				calls: state.calls.len(),
				capture_ns: self.inner.capture_load.ns_per_frame(),
				render_ns: self.inner.render_load.ns_per_frame(),
			};
			self.inner.config.limits.admit(load)?;
			state.calls.insert(
				call,
				CallMedia {
					peer,
					session,
					phase: Phase::Preparing,
					ssrc: rand::random(),
					generation: 1,
					remote: None,
					sender: None,
					send_stats: None,
					recv_stats: None,
					path: PathStatus::waiting("preparing media"),
				},
			);
		}
		let system = self.clone();
		self.inner.runtime.spawn(async move {
			if let Err(error) = system.prepare_task(call, peer, session).await {
				tracing::warn!(%error, %call, "media preparation failed");
				system.retire(call);
			}
		});
		Ok(())
	}

	async fn prepare_task(
		&self,
		call: CallId,
		peer: PeerId,
		session: SessionId,
	) -> Result<(), String> {
		let link = self
			.inner
			.peers
			.link(peer)
			.await
			.ok_or("no trusted link to the peer")?;
		let epoch = *link.session.epoch().as_bytes();
		let offer = self.ensure_peer_socket(peer, &link).await?;
		self.open_playback().await?;
		let (ssrc, generation, endpoint) = {
			let state = self.state();
			let media = state.calls.get(&call).ok_or("call retired")?;
			let socket = &state.peers.get(&peer).ok_or("peer socket missing")?.socket;
			(media.ssrc, media.generation, Arc::clone(&socket.endpoint))
		};
		let gate = self.inner.board.cell(call);
		let sender = CallSender::new(
			StreamIdentity {
				call,
				ssrc,
				generation,
				timestamp_base: rand::random(),
			},
			gate,
			self.inner.config.encoder,
			Box::new(EndpointSink::new(endpoint)),
		)
		.map_err(|e| e.to_string())?;
		{
			let mut state = self.state();
			let media = state.calls.get_mut(&call).ok_or("call retired")?;
			media.send_stats = Some(sender.stats());
			media.sender = Some(Box::new(sender));
			media.phase = Phase::Prepared;
		}
		if let Some(offer) = offer {
			let _ = self.inner.peers.send_media(peer, &offer);
		}
		let _ = self.inner.peers.send_media(
			peer,
			&MediaSignal::Ready {
				call,
				ssrc,
				generation,
				epoch,
			},
		);
		self.emit(MediaEvent::System(SystemEvent::MediaPrepared {
			call,
			session,
		}));
		self.try_activate(call).await;
		Ok(())
	}

	/// Creates the peer's socket on first use; returns the offer to send then.
	async fn ensure_peer_socket(
		&self,
		peer: PeerId,
		link: &crate::peers::LinkInfo,
	) -> Result<Option<MediaSignal>, String> {
		let source = {
			let mut state = self.state();
			if state.peers.contains_key(&peer) {
				return Ok(None);
			}
			state.next_source += 1;
			state.next_source
		};
		let bind = SocketAddr::new(self.inner.config.bind_ip, 0);
		let policy = match link.overlay_peer {
			Some(ip) => self.inner.config.policy.with_overlay_peer(ip),
			None => self.inner.config.policy,
		};
		let (socket, queue) = PeerSocket::open(
			bind,
			Arc::clone(&link.session),
			policy,
			source,
			self.inner.origin,
			Arc::clone(&self.inner.events),
		)
		.await
		.map_err(|e| e.to_string())?;
		let port = socket
			.endpoint
			.local_addr()
			.map_err(|e| e.to_string())?
			.port();
		let epoch = *link.session.epoch().as_bytes();
		self.inner
			.render
			.push(RenderCmd::AddSource(source, queue))
			.map_err(|_| "render queue full")?;
		let early = {
			let mut state = self.state();
			state.peers.insert(
				peer,
				PeerMedia {
					socket,
					epoch,
					remote_candidates: None,
					overlay: link.overlay_peer.is_some(),
				},
			);
			state.early_offers.remove(&peer)
		};
		if let Some((offer_epoch, candidates)) = early {
			self.accept_offer(peer, offer_epoch, candidates);
		}
		Ok(Some(MediaSignal::Offer {
			epoch,
			candidates: vec![SocketAddr::new(link.local_ip, port)],
		}))
	}

	pub(super) async fn open_playback(&self) -> Result<(), String> {
		if self.state().playback_open {
			return Ok(());
		}
		let (reply, answer) = oneshot::channel();
		self.inner
			.devices
			.commands
			.send(DeviceCmd::OpenPlayback(reply))
			.map_err(|_| "device thread stopped")?;
		answer.await.map_err(|_| "device thread stopped")??;
		self.state().playback_open = true;
		Ok(())
	}
}
