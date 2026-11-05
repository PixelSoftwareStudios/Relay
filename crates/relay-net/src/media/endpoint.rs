use std::net::SocketAddr;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Instant;

use thiserror::Error;
use tokio::net::UdpSocket;
use tokio::sync::watch;
use tokio::time::timeout;

use super::datagram::{BodyError, CallId};
use super::path::{PathValidator, ResponseLimiter};
use super::receiver::{MediaReceiver, Received};
use super::{seal_audio, seal_probe};
use crate::address::{AddressError, AddressPolicy};
use crate::limits::{UDP_PATH_TIMEOUT, UDP_RECORD_MAX_BYTES};
use crate::record::RecordKind;
use crate::session::{SealError, SecureSession};

const PATH_RESPONSES_PER_SECOND: u32 = 20;

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
			policy,
		})
	}

	pub fn local_addr(&self) -> Result<SocketAddr, MediaError> {
		Ok(self.socket.local_addr()?)
	}

	fn lock(&self) -> MutexGuard<'_, State> {
		self.state
			.lock()
			.unwrap_or_else(|poisoned| poisoned.into_inner())
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
		let datagram = seal_probe(session, RecordKind::PathChallenge, &challenge)?;
		self.socket.send_to(&datagram, candidate).await?;
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
		timeout(UDP_PATH_TIMEOUT, wait)
			.await
			.map_err(|_| MediaError::PathTimeout)?;
		Ok(())
	}

	/// Seals and sends one audio payload to the validated path.
	pub async fn send_audio(
		&self,
		call_id: &CallId,
		generation: u32,
		payload: &[u8],
	) -> Result<(), MediaError> {
		let (session, dest) = {
			let state = self.lock();
			(
				Arc::clone(&state.send_session),
				state.path.validated().ok_or(MediaError::PathNotValidated)?,
			)
		};
		let datagram = seal_audio(&session, call_id, generation, payload)?;
		self.socket.send_to(&datagram, dest).await?;
		Ok(())
	}

	/// Receives until an audio packet is accepted. Probes are handled internally; datagrams
	/// that fail any check are dropped silently with no reply.
	pub async fn recv(&self) -> Result<AudioPacket, MediaError> {
		let mut buf = [0u8; UDP_RECORD_MAX_BYTES + 1];
		loop {
			let (len, from) = self.socket.recv_from(&mut buf).await?;
			let outcome = self.lock().receiver.receive(&buf[..len], Instant::now());
			let received = match outcome {
				Ok(received) => received,
				Err(reason) => {
					tracing::debug!(%from, %reason, "dropped media datagram");
					continue;
				}
			};
			match received {
				Received::Audio {
					call_id,
					generation,
					payload,
					..
				} => {
					return Ok(AudioPacket {
						call_id,
						generation,
						payload,
					});
				}
				Received::PathChallenge { challenge, epoch } => {
					self.answer_challenge(&challenge, &epoch, from).await
				}
				Received::PathResponse { challenge, epoch } => {
					let accepted =
						self.lock()
							.path
							.on_response(&challenge, from, &epoch, Instant::now());
					if accepted.is_some() {
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
