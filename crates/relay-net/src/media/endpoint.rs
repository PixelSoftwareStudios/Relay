use relay_io::LockExt;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::time::timeout;

use super::datagram::{BodyError, CallId};
use super::path::{PathValidator, ResponseLimiter};
use super::receiver::{MediaReceiver, ReceivedRef};
use super::{seal_audio_into, seal_probe};
use crate::address::{AddressError, AddressPolicy};
use crate::limits::{UDP_PATH_TIMEOUT, UDP_RECORD_MAX_BYTES};
use crate::record::RecordKind;
use crate::session::{SealError, SecureSession};

const PATH_RESPONSES_PER_SECOND: u32 = 20;
/// How often an unanswered path challenge is sent again.
const PATH_CHALLENGE_RESEND: std::time::Duration = std::time::Duration::from_millis(250);

#[derive(Debug, Error)]
pub enum MediaError {
	#[error("i/o error: {0}")]
	Io(#[from] std::io::Error),
	#[error("payload: {0}")]
	Body(BodyError),
	#[error("seal: {0}")]
	Seal(#[from] SealError),
	#[error("no validated UDP path; media is not sent until a path is confirmed")]
	PathNotValidated,
	#[error("path validation timed out")]
	PathTimeout,
	#[error("endpoint refused by address policy: {0}")]
	Address(#[from] AddressError),
}

/// An accepted audio payload with its authenticated binding.
#[derive(Debug, PartialEq, Eq)]
pub struct AudioPacket {
	pub call_id: CallId,
	pub generation: u32,
	pub payload: Vec<u8>,
}

/// Authenticated binding of an audio payload copied into the caller's buffer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioMeta {
	pub call_id: CallId,
	pub generation: u32,
	pub len: usize,
}

enum Inbound {
	Drop,
	Audio(AudioMeta),
	Challenge([u8; 16], crate::session::LinkEpoch),
	Validated(bool),
}

struct State {
	send_session: Arc<SecureSession>,
	receiver: MediaReceiver,
	path: PathValidator,
	limiter: ResponseLimiter,
}

/// One UDP socket bound to a peer's session. Cheap to share by reference between a task that
/// calls [`Self::recv`] and tasks that send or validate paths.
pub struct MediaEndpoint {
	socket: UdpSocket,
	state: Mutex<State>,
	validated: watch::Sender<u64>,
	resets: AtomicU64,
	policy: AddressPolicy,
}

impl MediaEndpoint {
	pub async fn bind(
		local: SocketAddr,
		session: Arc<SecureSession>,
		policy: AddressPolicy,
	) -> Result<Self, MediaError> {
		let socket = UdpSocket::bind(local).await?;
		let now = Instant::now();
		let state = State {
			send_session: Arc::clone(&session),
			receiver: MediaReceiver::new(session),
			path: PathValidator::new(),
			limiter: ResponseLimiter::new(PATH_RESPONSES_PER_SECOND, now),
		};
		Ok(Self {
			socket,
			state: Mutex::new(state),
			validated: watch::channel(0).0,
			resets: AtomicU64::new(0),
			policy,
		})
	}

	pub fn local_addr(&self) -> Result<SocketAddr, MediaError> {
		Ok(self.socket.local_addr()?)
	}

	/// Receive errors that were ICMP-unreachable echoes, not socket failures.
	pub fn benign_resets(&self) -> u64 {
		self.resets.load(Ordering::Relaxed)
	}

	fn lock(&self) -> MutexGuard<'_, State> {
		self.state.lock_unpoisoned()
	}

	/// Allows inbound audio for an accepted call stream.
	pub fn bind_stream(&self, call_id: CallId, generation: u32) -> bool {
		self.lock().receiver.bind_stream(call_id, generation)
	}

	pub fn unbind_call(&self, call_id: &CallId) {
		self.lock().receiver.unbind_call(call_id);
	}

	/// The destination media is currently sent to, once validated.
	pub fn validated_path(&self) -> Option<SocketAddr> {
		self.lock().path.validated()
	}

	/// Forgets the validated path (interface or address change); media stops until revalidated.
	pub fn invalidate_path(&self) {
		self.lock().path.invalidate();
	}

	/// Validates `candidate` with an authenticated challenge under the current epoch.
	/// Another task must be running [`Self::recv`] to process the response.
	pub async fn validate_path(&self, candidate: SocketAddr) -> Result<(), MediaError> {
		let session = Arc::clone(&self.lock().send_session);
		self.validate_with(candidate, &session).await
	}

	/// Rekey switchover: admits `next` for receiving, validates the path under it, and only
	/// then moves sending to it. The old epoch is accepted for at most the overlap window.
	pub async fn switch_epoch(
		&self,
		next: Arc<SecureSession>,
		candidate: SocketAddr,
	) -> Result<(), MediaError> {
		self.lock()
			.receiver
			.install_epoch(Arc::clone(&next), Instant::now());
		self.validate_with(candidate, &next).await?;
		self.lock().send_session = next;
		Ok(())
	}

	async fn validate_with(
		&self,
		candidate: SocketAddr,
		session: &SecureSession,
	) -> Result<(), MediaError> {
		self.policy.check_socket(candidate)?;
		let mut watcher = self.validated.subscribe();
		let challenge = self
			.lock()
			.path
			.begin(candidate, *session.epoch(), Instant::now());
		// UDP is lossy and the first datagram to a neighbour can be lost while the link layer
		// resolves its address (Wi-Fi, ARP / neighbour discovery) or while the far side is
		// still binding the stream: the same challenge is sent again (freshly sealed, so it is
		// no replay) until the echo arrives or the timeout runs out.
		let resend = async {
			let mut tick = tokio::time::interval(PATH_CHALLENGE_RESEND);
			loop {
				tick.tick().await;
				let datagram = seal_probe(session, RecordKind::PathChallenge, &challenge)?;
				self.socket.send_to(&datagram, candidate).await?;
			}
		};
		let wait = async {
			loop {
				if self.lock().path.validated() == Some(candidate) {
					return;
				}
				if watcher.changed().await.is_err() {
					return;
				}
			}
		};
		let waited: Result<Result<(), MediaError>, _> = timeout(UDP_PATH_TIMEOUT, async {
			tokio::select! {
				() = wait => Ok(()),
				failed = resend => failed,
			}
		})
		.await;
		waited.map_err(|_| MediaError::PathTimeout)?
	}

	fn audio_route(&self) -> Result<(Arc<SecureSession>, SocketAddr), MediaError> {
		let state = self.lock();
		Ok((
			Arc::clone(&state.send_session),
			state.path.validated().ok_or(MediaError::PathNotValidated)?,
		))
	}

	/// Seals and sends one audio payload to the validated path.
	pub async fn send_audio(
		&self,
		call_id: &CallId,
		generation: u32,
		payload: &[u8],
	) -> Result<(), MediaError> {
		let (session, dest) = self.audio_route()?;
		let mut datagram = [0u8; UDP_RECORD_MAX_BYTES];
		let n = seal_audio_into(&session, call_id, generation, payload, &mut datagram)?;
		self.socket.send_to(&datagram[..n], dest).await?;
		Ok(())
	}

	/// Seals and sends without waiting: callable from any thread, no allocation. A full
	/// socket buffer returns `Io(WouldBlock)`; real-time callers drop the packet.
	pub fn try_send_audio(
		&self,
		call_id: &CallId,
		generation: u32,
		payload: &[u8],
	) -> Result<(), MediaError> {
		let (session, dest) = self.audio_route()?;
		let mut datagram = [0u8; UDP_RECORD_MAX_BYTES];
		let n = seal_audio_into(&session, call_id, generation, payload, &mut datagram)?;
		self.socket.try_send_to(&datagram[..n], dest)?;
		Ok(())
	}

	/// Receives until an audio packet is accepted. Probes are handled internally; datagrams
	/// that fail any check are dropped silently with no reply.
	pub async fn recv(&self) -> Result<AudioPacket, MediaError> {
		let mut payload = [0u8; UDP_RECORD_MAX_BYTES];
		let meta = self.recv_into(&mut payload).await?;
		Ok(AudioPacket {
			call_id: meta.call_id,
			generation: meta.generation,
			payload: payload[..meta.len].to_vec(),
		})
	}

	/// [`Self::recv`] that copies the payload into `out` (at least
	/// [`super::MAX_PAYLOAD_BYTES`] long) instead of allocating.
	pub async fn recv_into(&self, out: &mut [u8]) -> Result<AudioMeta, MediaError> {
		let mut buf = [0u8; UDP_RECORD_MAX_BYTES + 1];
		let mut scratch = [0u8; UDP_RECORD_MAX_BYTES + 1];
		loop {
			let (len, from) = match self.socket.recv_from(&mut buf).await {
				Ok(received) => received,
				// A previous send provoked an ICMP unreachable (Windows reports it on the
				// next receive): benign, not a dead socket.
				Err(error) if is_benign_reset(&error) => {
					self.resets.fetch_add(1, Ordering::Relaxed);
					continue;
				}
				Err(error) => return Err(error.into()),
			};
			let action = {
				let mut state = self.lock();
				match state
					.receiver
					.receive_ref(&buf[..len], Instant::now(), &mut scratch)
				{
					Err(reason) => {
						tracing::debug!(%from, %reason, "dropped media datagram");
						Inbound::Drop
					}
					Ok(ReceivedRef::Audio {
						call_id,
						generation,
						payload,
						..
					}) => match out.get_mut(..payload.len()) {
						Some(dest) => {
							dest.copy_from_slice(payload);
							Inbound::Audio(AudioMeta {
								call_id,
								generation,
								len: payload.len(),
							})
						}
						None => Inbound::Drop,
					},
					Ok(ReceivedRef::PathChallenge { challenge, epoch }) => {
						Inbound::Challenge(challenge, epoch)
					}
					Ok(ReceivedRef::PathResponse { challenge, epoch }) => {
						let accepted =
							state
								.path
								.on_response(&challenge, from, &epoch, Instant::now());
						Inbound::Validated(accepted.is_some())
					}
				}
			};
			match action {
				Inbound::Drop => {}
				Inbound::Audio(meta) => return Ok(meta),
				Inbound::Challenge(challenge, epoch) => {
					self.answer_challenge(&challenge, &epoch, from).await;
				}
				Inbound::Validated(accepted) => {
					if accepted {
						self.validated.send_modify(|n| *n += 1);
					}
				}
			}
		}
	}

	async fn answer_challenge(
		&self,
		challenge: &[u8; 16],
		epoch: &crate::session::LinkEpoch,
		from: SocketAddr,
	) {
		if self.policy.check_socket(from).is_err() {
			return;
		}
		let reply = {
			let mut state = self.lock();
			if !state.limiter.allow(Instant::now()) {
				return;
			}
			let Some(session) = state.receiver.session_for(epoch) else {
				return;
			};
			seal_probe(&session, RecordKind::PathResponse, challenge)
		};
		if let Ok(datagram) = reply {
			// Same size class as the challenge: no amplification toward unverified sources.
			let _ = self.socket.send_to(&datagram, from).await;
		}
	}
}

fn is_benign_reset(error: &std::io::Error) -> bool {
	matches!(
		error.kind(),
		std::io::ErrorKind::ConnectionReset | std::io::ErrorKind::ConnectionRefused
	)
}
