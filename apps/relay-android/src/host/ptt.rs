//! Hold-to-talk through the engine's push-to-talk lease.
//!
//! The on-screen button and the volume keys both end up here. A press is an
//! `InputControl` message with a short lease (400 ms) that this worker renews
//! every 150 ms while the key is down, exactly like the CLI and the desktop
//! `PttDriver`; the engine opens the transmit gate only while a lease is
//! alive and closes it when the lease lapses, the stream ends or the
//! controller changes. Nothing here opens the microphone by itself: consent
//! and the push-to-talk route are set by the UI as before.
//!
//! The engine accepts a press only from the client that holds the call's
//! controller lease, and `relay-ui` keeps its own connection private. The
//! worker therefore opens a second connection to the same in-process engine
//! and adopts the UI client's registered context (`RelayClient::adopt_context`;
//! the client id is an identifier, not a credential, and both ends live in
//! this process). The lease generation is the one the UI already holds.
//!
//! Every way a key can vanish ends in a release: key up, [`Ptt::cancel`]
//! (pause, focus or surface loss) and [`Ptt::reset`] (client detach, call end,
//! engine stop), which close the stream: the engine treats that as an
//! immediate release of everything the source held. A call that stopped being
//! push-to-talk or controlled, or a rejected renewal, also ends the press, and
//! a lapsed lease closes the gate if this process dies.

use std::sync::Arc;
use std::time::Duration;

use relay_api::pb::{ClientContext, InputAction, InputControlAck, InputControlMessage};
use relay_api::{Endpoint, RelayClient};
use relay_ui::hub::Hub;
use relay_ui::model::Mode;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio_stream::wrappers::ReceiverStream;

/// Lease asked for on every press and renewal. The engine caps it.
const LEASE_MS: u32 = 400;
/// Renewal period: well inside the lease so one late tick does not drop it.
const RENEW: Duration = Duration::from_millis(150);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const SEND_TIMEOUT: Duration = Duration::from_millis(500);

enum Key {
	Down(String),
	Up(String),
	/// Close the stream. The engine releases everything the source held at
	/// once, without the release delay a plain key-up gets.
	Reset,
	/// The engine refused message `sequence`.
	Refused(u64),
}

/// Sender side: cheap to call from any thread, never blocks.
pub struct Ptt {
	keys: mpsc::UnboundedSender<Key>,
}

impl Ptt {
	pub fn spawn(handle: &Handle, hub: Arc<Hub>, endpoint: Endpoint) -> Self {
		let (keys, inbox) = mpsc::unbounded_channel();
		let worker = Worker {
			hub,
			endpoint,
			keys: keys.clone(),
			session: None,
			held: None,
		};
		handle.spawn(worker.run(inbox));
		Self { keys }
	}

	/// The key for `call_id` (hex) went down.
	pub fn down(&self, call_id: String) {
		let _ = self.keys.send(Key::Down(call_id));
	}

	/// The key for `call_id` (hex) went up.
	pub fn up(&self, call_id: String) {
		let _ = self.keys.send(Key::Up(call_id));
	}

	/// The button vanished (pause, focus or surface loss): release at once.
	pub fn cancel(&self) {
		self.reset();
	}

	/// The client went away (detach, call end, stop): close the stream, which
	/// releases at once. The next press opens a new one.
	pub fn reset(&self) {
		let _ = self.keys.send(Key::Reset);
	}
}

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
	call_id: Vec<u8>,
	generation: u64,
}

struct Target {
	call_id: Vec<u8>,
	generation: u64,
	client_id: Vec<u8>,
}

struct Worker {
	hub: Arc<Hub>,
	endpoint: Endpoint,
	keys: mpsc::UnboundedSender<Key>,
	session: Option<Session>,
	held: Option<Held>,
}

impl Worker {
	async fn run(mut self, mut inbox: mpsc::UnboundedReceiver<Key>) {
		let mut tick = tokio::time::interval(RENEW);
		tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
		loop {
			tokio::select! {
				key = inbox.recv() => match key {
					Some(key) => self.handle(key).await,
					None => break,
				},
				_ = tick.tick(), if self.held.is_some() => self.renew().await,
			}
		}
		self.release().await;
	}

	async fn handle(&mut self, key: Key) {
		match key {
			Key::Down(call) => self.press(call).await,
			Key::Up(call) => {
				if self.held.as_ref().is_some_and(|h| h.call_hex == call) {
					self.release().await;
				}
			}
			Key::Reset => {
				// No explicit release first: a plain release starts the release
				// delay and the closed stream would no longer name the lease.
				self.held = None;
				self.session = None;
			}
			Key::Refused(sequence) => {
				let current = self
					.session
					.as_ref()
					.is_some_and(|s| s.sequence == sequence);
				if current && self.held.take().is_some() {
					tracing::info!("push-to-talk refused by the engine; released");
				}
			}
		}
	}

	/// What the UI client currently controls for `call_hex`, if it is a
	/// push-to-talk call that can take a press.
	fn target(&self, call_hex: &str) -> Option<Target> {
		self.hub.read(|s| {
			let call = s.calls.iter().find(|c| c.call_id == call_hex)?;
			let usable = !call.phase.is_terminal()
				&& call.controlled_by_me
				&& call.transmit.mode == Mode::PushToTalk;
			if !usable {
				return None;
			}
			Some(Target {
				call_id: hex::decode(&call.call_id).ok()?,
				generation: call.controller_generation,
				client_id: s.client_id.clone(),
			})
		})
	}

	async fn press(&mut self, call_hex: String) {
		if self.held.as_ref().is_some_and(|h| h.call_hex == call_hex) {
			return; // auto-repeat of a key already down
		}
		self.release().await;
		let Some(target) = self.target(&call_hex) else {
			tracing::info!("push-to-talk ignored: the call is not a controlled push-to-talk call");
			return;
		};
		if self.session.is_none() {
			match self.open(&target.client_id).await {
				Ok(session) => self.session = Some(session),
				Err(error) => {
					tracing::warn!(%error, "push-to-talk stream did not open");
					return;
				}
			}
		}
		let held = Held {
			call_hex,
			call_id: target.call_id,
			generation: target.generation,
		};
		if self.send(&held, InputAction::Press, LEASE_MS).await {
			self.held = Some(held);
		}
	}

	async fn renew(&mut self) {
		let Some(held) = self.held.take() else {
			return;
		};
		if self.target(&held.call_hex).is_none() {
			self.held = Some(held);
			self.release().await;
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
			call_id: held.call_id.clone(),
			controller_generation: held.generation,
			transmit_route_generation: 0,
			action: action as i32,
			requested_lease_ms: lease_ms,
		};
		let sent = tokio::time::timeout(SEND_TIMEOUT, session.input.send(message)).await;
		if matches!(sent, Ok(Ok(()))) {
			return true;
		}
		tracing::warn!("push-to-talk stream closed; it will reopen on the next press");
		self.session = None;
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
			let keys = self.keys.clone();
			let acks = tokio::spawn(async move {
				while let Ok(Some(ack)) = acks.message().await {
					refused(&keys, &ack);
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

fn refused(keys: &mpsc::UnboundedSender<Key>, ack: &InputControlAck) {
	if !ack.accepted {
		let _ = keys.send(Key::Refused(ack.input_sequence));
	}
}
