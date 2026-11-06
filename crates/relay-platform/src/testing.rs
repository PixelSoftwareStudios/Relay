//! Fakes for tests and demos: a scripted hotkey backend.

use relay_io::LockExt;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use crate::hotkey::{
	BackendStatus, HotkeyBackend, HotkeyBinding, HotkeyError, HotkeyEvent, HotkeyStream,
};

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
