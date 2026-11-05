//! The PTT state machine. Fails closed: any doubt ends transmission.

use std::time::Duration;

use relay_api::pb::{ClientContext, InputAction, InputControlAck, InputControlMessage};
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, sleep_until, timeout};

use super::sink::LeaseSink;
use super::{BlockReason, PttConfig, PttError, PttInput, PttState, PttTarget};
use crate::hotkey::{HotkeyEvent, HotkeyStream};
use crate::session::SessionEvent;

/// Drives one `InputControl` stream from hotkey edges.
///
/// Sequence numbers start at 1 and rise by one per message sent, so they are strictly
/// monotonic for the life of the source. A new driver uses a new random `source_id`.
pub struct PttDriver<S: LeaseSink> {
	sink: S,
	cfg: PttConfig,
	client: ClientContext,
	source_id: Vec<u8>,
	seq: u64,
	target: Option<PttTarget>,
	held: bool,
	suppressed: bool,
	locked: bool,
	lease: Option<Lease>,
	state: watch::Sender<PttState>,
}

#[derive(Clone, Copy)]
struct Lease {
	renew_at: Instant,
	expires_at: Instant,
}

/// Outcome of one exchange with the sink.
enum Exchange {
	Ack(InputControlAck),
	TimedOut,
}

impl<S: LeaseSink> PttDriver<S> {
	/// Build a driver and a receiver for its state.
	pub fn new(
		sink: S,
		client: ClientContext,
		cfg: PttConfig,
	) -> (Self, watch::Receiver<PttState>) {
		let (state, rx) = watch::channel(PttState::Idle);
		let source_id = uuid::Uuid::new_v4().as_bytes().to_vec();
		let driver = Self {
			sink,
			cfg,
			client,
			source_id,
			seq: 0,
			target: None,
			held: false,
			suppressed: false,
			locked: false,
			lease: None,
			state,
		};
		(driver, rx)
	}

	/// Run until shutdown or failure. Always attempts a release before returning.
	pub async fn run(
		mut self,
		mut hotkeys: HotkeyStream,
		mut inputs: mpsc::UnboundedReceiver<PttInput>,
	) -> Result<(), PttError> {
		let result = self.event_loop(&mut hotkeys, &mut inputs).await;
		if self.lease.is_some() {
			self.release_best_effort(InputAction::ReleaseAll).await;
		}
		if let Err(e) = &result {
			self.set(PttState::Failed(e.to_string()));
		}
		result
	}

	async fn event_loop(
		&mut self,
		hotkeys: &mut HotkeyStream,
		inputs: &mut mpsc::UnboundedReceiver<PttInput>,
	) -> Result<(), PttError> {
		loop {
			let renew_at = self.lease.map(|l| l.renew_at);
			let expires_at = self.lease.map(|l| l.expires_at);
			tokio::select! {
				edge = hotkeys.recv() => match edge {
					Some(HotkeyEvent::Pressed) => self.on_press().await?,
					Some(HotkeyEvent::Released) => self.on_release().await,
					None => return Err(PttError::BackendClosed),
				},
				input = inputs.recv() => match input {
					Some(PttInput::Target(target)) => self.on_target(target).await,
					Some(PttInput::Session(event)) => self.on_session(event).await?,
					Some(PttInput::Shutdown) | None => return Ok(()),
				},
				() = sleep_opt(renew_at) => self.renew().await?,
				() = sleep_opt(expires_at) => self.expire().await,
			}
		}
	}

	fn set(&self, state: PttState) {
		self.state.send_replace(state);
	}

	fn message(&mut self, action: InputAction) -> InputControlMessage {
		self.seq += 1;
		let target = self.target.clone().unwrap_or(PttTarget {
			call_id: Vec::new(),
			controller_generation: 0,
			transmit_route_generation: 0,
		});
		InputControlMessage {
			client: Some(self.client.clone()),
			source_id: self.source_id.clone(),
			input_sequence: self.seq,
			call_id: target.call_id,
			controller_generation: target.controller_generation,
			transmit_route_generation: target.transmit_route_generation,
			action: action as i32,
			requested_lease_ms: self.cfg.lease_ms,
		}
	}

	/// Send one message and wait at most `limit` for its acknowledgement.
	async fn exchange(
		&mut self,
		action: InputAction,
		limit: Duration,
	) -> Result<Exchange, PttError> {
		let msg = self.message(action);
		let seq = msg.input_sequence;
		match timeout(limit, self.sink.exchange(msg)).await {
			Err(_) => Ok(Exchange::TimedOut),
			Ok(Err(e)) => Err(e.into()),
			Ok(Ok(ack)) if ack.input_sequence != seq || ack.source_id != self.source_id => {
				Err(PttError::Protocol(seq))
			}
			Ok(Ok(ack)) => Ok(Exchange::Ack(ack)),
		}
	}

	async fn release_best_effort(&mut self, action: InputAction) {
		self.lease = None;
		let limit = Duration::from_millis(self.cfg.release_timeout_ms);
		let _ = self.exchange(action, limit).await;
	}

	async fn on_press(&mut self) -> Result<(), PttError> {
		if self.held {
			return Ok(()); // auto-repeat
		}
		self.held = true;
		if self.suppressed {
			self.set(PttState::Blocked(BlockReason::NeedsRepress));
			return Ok(());
		}
		if self.locked {
			self.set(PttState::Blocked(BlockReason::SessionLocked));
			return Ok(());
		}
		if self.target.is_none() {
			self.set(PttState::Blocked(BlockReason::NoTarget));
			return Ok(());
		}
		let limit = Duration::from_millis(u64::from(self.cfg.lease_ms));
		self.lease_exchange(InputAction::Press, limit).await
	}

	async fn on_release(&mut self) {
		self.held = false;
		self.suppressed = false;
		if self.lease.is_some() {
			self.release_best_effort(InputAction::Release).await;
		}
		self.set(PttState::Idle);
	}

	async fn renew(&mut self) -> Result<(), PttError> {
		let Some(lease) = self.lease else {
			return Ok(());
		};
		let limit = lease.expires_at.saturating_duration_since(Instant::now());
		self.lease_exchange(InputAction::Renew, limit).await
	}

	/// Press or renew, then apply the answer. Rejection and stalls end the lease.
	async fn lease_exchange(
		&mut self,
		action: InputAction,
		limit: Duration,
	) -> Result<(), PttError> {
		let sent = Instant::now();
		match self.exchange(action, limit).await {
			Err(e) => {
				self.lease = None;
				Err(e)
			}
			Ok(Exchange::TimedOut) => {
				self.stall().await;
				Ok(())
			}
			Ok(Exchange::Ack(ack)) if !ack.accepted || ack.effective_lease_ms == 0 => {
				self.lease = None;
				self.suppress_if_held();
				let code = ack.problem.map(|p| p.code).unwrap_or_default();
				self.set(PttState::Rejected { code });
				Ok(())
			}
			Ok(Exchange::Ack(ack)) => {
				let lease = Duration::from_millis(u64::from(ack.effective_lease_ms));
				self.lease = Some(Lease {
					renew_at: sent + lease / 3,
					expires_at: sent + lease,
				});
				self.set(if ack.effective_transmitting {
					PttState::Transmitting
				} else {
					PttState::Gated
				});
				Ok(())
			}
		}
	}

	/// No answer in time: assume the engine dropped us, and tell it to stop anyway.
	async fn stall(&mut self) {
		self.suppress_if_held();
		self.release_best_effort(InputAction::ReleaseAll).await;
		self.set(PttState::Expired);
	}

	async fn expire(&mut self) {
		if self.lease.is_some() {
			self.stall().await;
		}
	}

	fn suppress_if_held(&mut self) {
		if self.held {
			self.suppressed = true;
		}
	}

	/// Drop any lease because the context changed; the key must be pressed again.
	async fn force_release(&mut self, blocked: Option<BlockReason>) {
		if self.lease.is_some() {
			self.suppress_if_held();
			self.release_best_effort(InputAction::ReleaseAll).await;
		}
		self.set(blocked.map_or(PttState::Idle, PttState::Blocked));
	}

	async fn on_target(&mut self, target: Option<PttTarget>) {
		if self.target == target {
			return;
		}
		self.target = target;
		let blocked = self.target.is_none().then_some(BlockReason::NoTarget);
		self.force_release(blocked).await;
	}

	async fn on_session(&mut self, event: SessionEvent) -> Result<(), PttError> {
		match event {
			SessionEvent::Locked | SessionEvent::Suspending => {
				self.locked = true;
				self.suppress_if_held();
				self.force_release(Some(BlockReason::SessionLocked)).await;
			}
			SessionEvent::Unlocked | SessionEvent::Resumed => {
				self.locked = false;
				self.set(PttState::Idle);
			}
			SessionEvent::FocusLost => self.force_release(None).await,
			SessionEvent::FocusGained => {}
			SessionEvent::Disconnected => {
				self.force_release(None).await;
				return Err(PttError::Disconnected);
			}
		}
		Ok(())
	}
}

async fn sleep_opt(at: Option<Instant>) {
	match at {
		Some(at) => sleep_until(at).await,
		None => std::future::pending().await,
	}
}
