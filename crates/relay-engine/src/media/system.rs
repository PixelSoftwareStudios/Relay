//! The media system: workers, device thread, per-call lifecycle and the
//! control-side entry points used by the service adapters.

use std::net::{IpAddr, SocketAddr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::Instant;

use relay_audio::FRAME_SAMPLES;
use relay_audio::codec::EncoderConfig;
use relay_audio::device::{DeviceDescriptor, StreamRequest};
use relay_audio::dsp::{DspLimits, NoiseSuppressor, OffSuppressor};
use relay_core::{AudioGeneration, CallId, PeerEvent, PeerId, ReceiveMix, SessionId, SystemEvent};
use relay_net::AddressPolicy;
use relay_protocol::jitter::JitterConfig;
use thiserror::Error;
use tokio::runtime::Handle;
use tokio::sync::oneshot;

use super::capture_worker::{self, CaptureCmd};
use super::devices::{BackendFactory, DeviceCmd, DeviceSetup, DeviceThread, Selection};
use super::events::{EventSink, MediaEvent};
use super::gate::GateBoard;
use super::inbox::Inbox;
use super::load::{AdmissionLimits, LoadMeter, LoadSnapshot, Refusal};
use super::net::{EndpointSink, PeerSocket};
use super::recv::{CallReceiver, RemoteStream};
use super::render_worker::{self, RenderCmd};
use super::send::{CallSender, StreamIdentity};
use super::state::{CallMedia, CallReport, PeerMedia, Phase, RemoteReady, State};
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
}

impl MediaConfig {
	/// LAN defaults with an explicit selection.
	pub fn new(selection: Selection) -> Self {
		Self {
			selection,
			request: StreamRequest::default(),
			encoder: EncoderConfig::default(),
			jitter: JitterConfig::BALANCED,
			limits: AdmissionLimits::default(),
			playout_target_ms: 20,
			bind_ip: IpAddr::from([0, 0, 0, 0]),
			policy: AddressPolicy::lan_only(),
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
	pub(super) devices: DeviceThread,
	capture_load: Arc<LoadMeter>,
	render_load: Arc<LoadMeter>,
	stop: Arc<AtomicBool>,
	workers: Mutex<Vec<JoinHandle<()>>>,
	peers: PeerHandle,
	runtime: Handle,
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
		let workers = std::mem::take(&mut *self.workers.lock().unwrap_or_else(|p| p.into_inner()));
		for worker in workers {
			worker.thread().unpark();
			let _ = worker.join();
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
		let (capture, capture_rx) = Inbox::new(COMMAND_DEPTH);
		let (render, render_rx) = Inbox::new(COMMAND_DEPTH);
		let workers = vec![
			capture_worker::spawn(
				capture_rx,
				suppressor,
				Arc::clone(&events),
				Arc::clone(&capture_load),
				Arc::clone(&stop),
			)?,
			render_worker::spawn(
				render_rx,
				Arc::clone(&events),
				Arc::clone(&render_load),
				Arc::clone(&stop),
			)?,
		];
		let devices = DeviceThread::spawn(
			backend,
			DeviceSetup {
				request: config.request,
				selection: config.selection.clone(),
				playout_target_ms: config.playout_target_ms,
			},
			capture.clone(),
			render.clone(),
			Arc::clone(&events),
		)?;
		Ok(Self {
			inner: Arc::new(Inner {
				config,
				events,
				board: GateBoard::new(),
				capture,
				render,
				devices,
				capture_load,
				render_load,
				stop,
				workers: Mutex::new(workers),
				peers,
				runtime,
				origin: Instant::now(),
				state: Mutex::new(State::default()),
			}),
		})
	}

	fn state(&self) -> MutexGuard<'_, State> {
		self.inner.state.lock().unwrap_or_else(|p| p.into_inner())
	}

	fn emit(&self, event: MediaEvent) {
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
		let _ = self
			.inner
			.devices
			.commands
			.send(DeviceCmd::OpenCapture(generation));
	}

	/// Asks the device thread to close the microphone.
	pub fn close_capture(&self, generation: AudioGeneration) {
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
			Box::new(EndpointSink::new(endpoint, self.inner.runtime.clone())),
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
		let (socket, queue) = PeerSocket::open(
			bind,
			Arc::clone(&link.session),
			self.inner.config.policy,
			source,
			self.inner.origin,
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

	async fn open_playback(&self) -> Result<(), String> {
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

	/// Feeds the remote's media negotiation message into the system.
	pub fn handle_signal(&self, peer: PeerId, signal: MediaSignal) {
		match signal {
			MediaSignal::Offer { epoch, candidates } => {
				let known = self.state().peers.contains_key(&peer);
				if known {
					self.accept_offer(peer, epoch, candidates);
				} else {
					self.state().early_offers.insert(peer, (epoch, candidates));
				}
			}
			MediaSignal::Ready {
				call,
				ssrc,
				generation,
				epoch,
			} => {
				let ready = RemoteReady { ssrc, generation };
				{
					let mut state = self.state();
					match state.calls.get_mut(&call) {
						Some(media) if media.peer == peer => media.remote = Some(ready),
						Some(_) => return,
						None => {
							if state.early_ready.len() < 16 {
								state.early_ready.insert(call, (epoch, ready));
							}
							return;
						}
					}
				}
				self.activate_later(call);
			}
		}
	}

	fn accept_offer(&self, peer: PeerId, epoch: [u8; 16], candidates: Vec<SocketAddr>) {
		let calls: Vec<CallId> = {
			let mut state = self.state();
			let Some(media) = state.peers.get_mut(&peer) else {
				return;
			};
			if media.epoch != epoch {
				return;
			}
			media.remote_candidates = Some(candidates);
			state
				.calls
				.iter()
				.filter(|(_, c)| c.peer == peer)
				.map(|(id, _)| *id)
				.collect()
		};
		calls.into_iter().for_each(|call| self.activate_later(call));
	}

	fn activate_later(&self, call: CallId) {
		let system = self.clone();
		self.inner
			.runtime
			.spawn(async move { system.try_activate(call).await });
	}

	/// Installs both streams and validates the path once everything the call
	/// needs has arrived. Runs at most once per call.
	async fn try_activate(&self, call: CallId) {
		let plan = {
			let mut state = self.state();
			if let Some((epoch, ready)) = state.early_ready.remove(&call) {
				let own_epoch = state
					.calls
					.get(&call)
					.and_then(|c| state.peers.get(&c.peer))
					.map(|p| p.epoch);
				if let (Some(media), true) = (state.calls.get_mut(&call), own_epoch == Some(epoch))
				{
					media.remote.get_or_insert(ready);
				}
			}
			let Some(media) = state.calls.get(&call) else {
				return;
			};
			let (Phase::Prepared, Some(remote)) = (media.phase, media.remote) else {
				return;
			};
			let Some(peer) = state.peers.get(&media.peer) else {
				return;
			};
			let Some(candidates) = peer.remote_candidates.clone() else {
				return;
			};
			let endpoint = Arc::clone(&peer.socket.endpoint);
			let source = peer.socket.source;
			let session = media.session;
			let media = state.calls.get_mut(&call).expect("checked above");
			media.phase = Phase::Activating;
			(
				remote,
				candidates,
				endpoint,
				source,
				session,
				media.sender.take(),
			)
		};
		let (remote, candidates, endpoint, source, session, sender) = plan;
		let Some(sender) = sender else {
			return;
		};
		if !endpoint.bind_stream(call.to_bytes(), remote.generation) {
			tracing::warn!(%call, "stream binding refused");
			return;
		}
		let receiver = match CallReceiver::new(
			RemoteStream {
				call,
				ssrc: remote.ssrc,
				generation: remote.generation,
				source,
			},
			self.inner.config.jitter,
		) {
			Ok(receiver) => receiver,
			Err(error) => {
				tracing::warn!(%error, %call, "receiver creation failed");
				return;
			}
		};
		if let Some(media) = self.state().calls.get_mut(&call) {
			media.recv_stats = Some(receiver.stats());
		}
		let installed = self
			.inner
			.render
			.push(RenderCmd::AddCall(Box::new(receiver)))
			.is_ok() && self
			.inner
			.capture
			.push(CaptureCmd::AddSender(sender))
			.is_ok();
		if !installed {
			tracing::warn!(%call, "worker queue full; call not activated");
			return;
		}
		for candidate in candidates {
			if endpoint.validate_path(candidate).await.is_ok() {
				if let Some(media) = self.state().calls.get_mut(&call) {
					media.phase = Phase::Active;
				}
				self.emit(MediaEvent::Peer(PeerEvent::PathValidated { call, session }));
				return;
			}
		}
		tracing::warn!(%call, "no media path could be validated");
	}

	/// Retires a call's media and reports `MediaStopped`. Idempotent.
	pub fn stop(&self, call: CallId, session: SessionId) {
		self.retire(call);
		let system = self.clone();
		self.inner.runtime.spawn(async move {
			system.emit(MediaEvent::System(SystemEvent::MediaStopped {
				call,
				session,
			}));
		});
	}

	fn retire(&self, call: CallId) {
		let (removed_peer, none_left) = {
			let mut state = self.state();
			state.early_ready.remove(&call);
			let Some(media) = state.calls.remove(&call) else {
				return;
			};
			let peer_idle = !state.calls.values().any(|c| c.peer == media.peer);
			let peer = if peer_idle {
				state.peers.remove(&media.peer)
			} else {
				None
			};
			if let Some(peer) = &peer {
				peer.socket.endpoint.unbind_call(&call.to_bytes());
			} else if let Some(peer) = state.peers.get(&media.peer) {
				peer.socket.endpoint.unbind_call(&call.to_bytes());
			}
			let none_left = state.calls.is_empty();
			if none_left {
				state.playback_open = false;
			}
			(peer, none_left)
		};
		self.inner.board.remove(call);
		let _ = self.inner.capture.push(CaptureCmd::RemoveSender(call));
		let _ = self.inner.render.push(RenderCmd::RemoveCall(call));
		if let Some(peer) = removed_peer {
			let _ = self
				.inner
				.render
				.push(RenderCmd::RemoveSource(peer.socket.source));
		}
		if none_left {
			let _ = self.inner.devices.commands.send(DeviceCmd::ClosePlayback);
		}
	}
}
