//! Fakes for tests and demos: a scripted hotkey backend and a fake lease sink.

use relay_io::LockExt;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use relay_api::pb::{InputAction, InputControlAck, InputControlMessage, Problem};
use tokio::sync::mpsc;

use crate::hotkey::{
	BackendStatus, HotkeyBackend, HotkeyBinding, HotkeyError, HotkeyEvent, HotkeyStream,
};
use crate::ptt::{LeaseSink, SinkError};

/// Backend whose status is chosen by the test and whose edges are injected.
pub struct FakeBackend {
	status: BackendStatus,
	tx: Arc<Mutex<Option<mpsc::UnboundedSender<HotkeyEvent>>>>,
}

/// Injects edges into a started [`FakeBackend`].
#[derive(Clone)]
pub struct FakeKeyboard {
	tx: Arc<Mutex<Option<mpsc::UnboundedSender<HotkeyEvent>>>>,
}

impl FakeKeyboard {
	/// Send an edge; false if the backend is not started.
	pub fn send(&self, event: HotkeyEvent) -> bool {
		let tx = self.tx.lock_unpoisoned();
		tx.as_ref().is_some_and(|tx| tx.send(event).is_ok())
	}
}

impl FakeBackend {
	/// A backend reporting `status`, plus the keyboard that drives it.
	pub fn new(status: BackendStatus) -> (Self, FakeKeyboard) {
		let tx = Arc::new(Mutex::new(None));
		(
			Self {
				status,
				tx: tx.clone(),
			},
			FakeKeyboard { tx },
		)
	}
}

impl HotkeyBackend for FakeBackend {
	fn name(&self) -> &'static str {
		"fake"
	}

	fn probe(&self) -> BackendStatus {
		self.status.clone()
	}

	fn start(&mut self, _binding: &HotkeyBinding) -> Result<HotkeyStream, HotkeyError> {
		if !self.status.is_available() {
			return Err(HotkeyError::Unavailable(self.status.clone()));
		}
		let mut slot = self.tx.lock_unpoisoned();
		if slot.is_some() {
			return Err(HotkeyError::AlreadyStarted);
		}
		let (tx, rx) = mpsc::unbounded_channel();
		*slot = Some(tx);
		Ok(rx)
	}

	fn stop(&mut self) {
		*self.tx.lock_unpoisoned() = None;
	}
}

/// How the fake sink answers the next message.
#[derive(Debug, Clone)]
pub enum Reply {
	/// Accept with this lease; `transmitting` is echoed in the ack.
	Accept { lease_ms: u32, transmitting: bool },
	/// Reject with a problem code.
	Reject(&'static str),
	/// Never answer (a stalled engine).
	Stall,
	/// Fail the stream.
	Error,
}

#[derive(Default)]
struct Script {
	replies: VecDeque<Reply>,
	default: Option<Reply>,
	seen: Vec<InputControlMessage>,
}

/// Fake `LeaseSink` recording every message.
#[derive(Clone, Default)]
pub struct FakeLeaseSink(Arc<Mutex<Script>>);

impl FakeLeaseSink {
	/// Sink that accepts 300 ms leases until told otherwise.
	pub fn accepting() -> Self {
		let sink = Self::default();
		sink.set_default(Reply::Accept {
			lease_ms: 300,
			transmitting: true,
		});
		sink
	}

	fn script(&self) -> std::sync::MutexGuard<'_, Script> {
		self.0.lock_unpoisoned()
	}

	/// Reply used when the queue is empty.
	pub fn set_default(&self, reply: Reply) {
		self.script().default = Some(reply);
	}

	/// Queue a one-off reply.
	pub fn push(&self, reply: Reply) {
		self.script().replies.push_back(reply);
	}

	/// All messages received so far.
	pub fn seen(&self) -> Vec<InputControlMessage> {
		self.script().seen.clone()
	}

	/// Actions received so far.
	pub fn actions(&self) -> Vec<InputAction> {
		self.seen()
			.iter()
			.map(|m| InputAction::try_from(m.action).unwrap_or(InputAction::Unspecified))
			.collect()
	}
}

impl LeaseSink for FakeLeaseSink {
	async fn exchange(&mut self, msg: InputControlMessage) -> Result<InputControlAck, SinkError> {
		let reply = {
			let mut script = self.script();
			script.seen.push(msg.clone());
			script
				.replies
				.pop_front()
				.or_else(|| script.default.clone())
		};
		let mut ack = InputControlAck {
			source_id: msg.source_id,
			input_sequence: msg.input_sequence,
			..Default::default()
		};
		match reply.unwrap_or(Reply::Stall) {
			Reply::Accept {
				lease_ms,
				transmitting,
			} => {
				ack.accepted = true;
				ack.effective_lease_ms = lease_ms;
				ack.effective_transmitting = transmitting;
			}
			Reply::Reject(code) => {
				ack.problem = Some(Problem {
					code: code.into(),
					..Default::default()
				});
			}
			Reply::Stall => std::future::pending().await,
			Reply::Error => return Err(SinkError::Closed),
		}
		Ok(ack)
	}
}
