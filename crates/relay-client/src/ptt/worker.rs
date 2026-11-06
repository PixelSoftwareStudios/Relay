//! The task behind [`super::PttWorker`].

use relay_api::pb::{ClientContext, InputAction, InputControlAck, InputControlMessage};
use relay_api::{Endpoint, RelayClient};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;
use tokio_stream::wrappers::ReceiverStream;

use super::{LEASE_MS, MAX_HOLD, Msg, PttState, PttTarget, RENEW, Resolve};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const SEND_TIMEOUT: Duration = Duration::from_millis(500);

struct Session {
	input: mpsc::Sender<InputControlMessage>,
	context: ClientContext,
	source: Vec<u8>,
	sequence: u64,
	acks: JoinHandle<()>,
	/// Keeps the connection (and so the stream's channel) open.
	_client: RelayClient,
}

impl Drop for Session {
	fn drop(&mut self) {
		self.acks.abort();
	}
}

struct Held {
	call_hex: String,
	target: PttTarget,
	/// Sequence of the press: acks of earlier messages do not describe this press.
	first_sequence: u64,
	since: Instant,
}

pub(super) struct Worker {
	endpoint: Endpoint,
	resolve: Resolve,
	/// Weak: the worker ends when the last handle is dropped.
	msgs: mpsc::WeakUnboundedSender<Msg>,
	state: watch::Sender<PttState>,
	session: Option<Session>,
	held: Option<Held>,
}

impl Worker {
	pub(super) fn new(
		endpoint: Endpoint,
		resolve: Resolve,
		msgs: mpsc::UnboundedSender<Msg>,
		state: watch::Sender<PttState>,
	) -> Self {
		Self {
			endpoint,
			resolve,
			msgs: msgs.downgrade(),
			state,
			session: None,
			held: None,
		}
	}

	pub(super) async fn run(mut self, mut inbox: mpsc::UnboundedReceiver<Msg>) {
		let mut tick = tokio::time::interval(RENEW);
		tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
		loop {
			tokio::select! {
				msg = inbox.recv() => match msg {
					Some(msg) => {
						let pressed = matches!(msg, Msg::Press(_));
						self.handle(msg).await;
						if pressed {
							tick.reset();
						}
					}
					None => break,
				},
				_ = tick.tick(), if self.held.is_some() => self.renew().await,
			}
		}
		// Dropping the session closes the stream: an immediate release.
		self.held = None;
		self.session = None;
	}

	fn set(&self, state: PttState) {
		self.state.send_replace(state);
	}

	async fn handle(&mut self, msg: Msg) {
		match msg {
			Msg::Press(call) => self.press(call).await,
			Msg::Release => self.release().await,
			Msg::Cancel => self.close(),
			Msg::Ack {
				sequence,
				accepted,
				transmitting,
				code,
			} => self.acked(sequence, accepted, transmitting, code),
		}
	}

	fn acked(&mut self, sequence: u64, accepted: bool, transmitting: bool, code: String) {
		let current = self
			.held
			.as_ref()
			.is_some_and(|h| sequence >= h.first_sequence);
		if !current {
			return;
		}
		if accepted {
			self.set(if transmitting {
				PttState::Transmitting
			} else {
				PttState::Gated
			});
		} else {
			self.held = None;
			tracing::info!(%code, "push-to-talk refused by the engine; released");
			self.set(PttState::Rejected { code });
		}
	}

	/// Close the stream, which the engine treats as an immediate release.
	/// No explicit release first: a plain release starts the release delay
	/// and the closed stream would no longer name the lease.
	fn close(&mut self) {
		self.held = None;
		self.session = None;
		self.set(PttState::Idle);
	}

	async fn press(&mut self, call_hex: String) {
		if self.held.as_ref().is_some_and(|h| h.call_hex == call_hex) {
			return; // auto-repeat of a key already down
		}
		self.release().await;
		let Some(target) = (self.resolve)(&call_hex) else {
			tracing::info!("push-to-talk ignored: not a controlled push-to-talk call");
			self.set(PttState::NoTarget);
			return;
		};
		if self.session.is_none() {
			match self.open(&target.client_id).await {
				Ok(session) => self.session = Some(session),
				Err(error) => {
					tracing::warn!(%error, "push-to-talk stream did not open");
					self.set(PttState::Failed(error));
					return;
				}
			}
		}
		let first_sequence = self.session.as_ref().map_or(0, |s| s.sequence + 1);
		let held = Held {
			call_hex,
			target,
			first_sequence,
			since: Instant::now(),
		};
		if self.send(&held, InputAction::Press, LEASE_MS).await {
			self.set(PttState::Gated);
			self.held = Some(held);
		}
	}

	async fn renew(&mut self) {
		let Some(held) = self.held.take() else {
			return;
		};
		if held.since.elapsed() >= MAX_HOLD {
			tracing::warn!("push-to-talk held too long; released as a stuck key");
			self.close();
			return;
		}
		let still = (self.resolve)(&held.call_hex).is_some_and(|t| t == held.target);
		if !still {
			// The call ended, left push-to-talk, changed controller or the
			// client detached: nothing is left to hold, so no release delay.
			tracing::info!("push-to-talk target gone; released");
			self.close();
			return;
		}
		if self.send(&held, InputAction::Renew, LEASE_MS).await {
			self.held = Some(held);
		}
	}

	/// Sends the release for what is held. Always attempted, never retried.
	async fn release(&mut self) {
		let Some(held) = self.held.take() else {
			return;
		};
		self.send(&held, InputAction::Release, 0).await;
		self.set(PttState::Idle);
	}

	/// One message on the stream; a stream that cannot take it is dropped, and
	/// with it every lease it held.
	async fn send(&mut self, held: &Held, action: InputAction, lease_ms: u32) -> bool {
		let Some(session) = self.session.as_mut() else {
			return false;
		};
		session.sequence += 1;
		let message = InputControlMessage {
			client: Some(session.context.clone()),
			source_id: session.source.clone(),
			input_sequence: session.sequence,
			call_id: held.target.call_id.clone(),
			controller_generation: held.target.controller_generation,
			transmit_route_generation: held.target.transmit_route_generation,
			action: action as i32,
			requested_lease_ms: lease_ms,
		};
		let sent = tokio::time::timeout(SEND_TIMEOUT, session.input.send(message)).await;
		if matches!(sent, Ok(Ok(()))) {
			return true;
		}
		tracing::warn!("push-to-talk stream closed; it will reopen on the next press");
		self.session = None;
		self.held = None;
		self.set(PttState::Failed(
			"the stream to the engine closed".to_owned(),
		));
		false
	}

	async fn open(&self, client_id: &[u8]) -> Result<Session, String> {
		let connect = async {
			let mut client = RelayClient::connect(&self.endpoint)
				.await
				.map_err(|e| e.to_string())?;
			let context = ClientContext {
				client_id: client_id.to_vec(),
				engine_incarnation: client.info().engine_incarnation.clone(),
			};
			client.adopt_context(context.clone());
			let (input, outbound) = mpsc::channel(8);
			let mut acks = client
				.raw()
				.input_control(ReceiverStream::new(outbound))
				.await
				.map_err(|e| e.to_string())?
				.into_inner();
			let msgs = self.msgs.clone();
			let acks = tokio::spawn(async move {
				while let Ok(Some(ack)) = acks.message().await {
					let Some(msgs) = msgs.upgrade() else {
						break;
					};
					let _ = msgs.send(answer(&ack));
				}
			});
			Ok::<_, String>(Session {
				input,
				context,
				source: relay_io::entropy::random_bytes::<16>().to_vec(),
				sequence: 0,
				acks,
				_client: client,
			})
		};
		tokio::time::timeout(CONNECT_TIMEOUT, connect)
			.await
			.map_err(|_| "timed out".to_owned())?
	}
}

fn answer(ack: &InputControlAck) -> Msg {
	Msg::Ack {
		sequence: ack.input_sequence,
		accepted: ack.accepted,
		transmitting: ack.effective_transmitting,
		code: ack
			.problem
			.as_ref()
			.map(|p| p.code.clone())
			.unwrap_or_default(),
	}
}
