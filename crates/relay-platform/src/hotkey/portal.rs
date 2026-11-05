//! Wayland hotkeys through the XDG GlobalShortcuts portal (`ashpd`).
//!
//! The portal may be missing (older compositors, no backend). Probing connects to
//! the session bus and checks for the interface; absence is reported as
//! `Unsupported`, never faked.

use std::thread::JoinHandle;

use ashpd::desktop::global_shortcuts::{BindShortcutsOptions, GlobalShortcuts, NewShortcut};
use tokio::sync::{mpsc, oneshot};
use tokio_stream::StreamExt;

use super::{BackendStatus, HotkeyBackend, HotkeyBinding, HotkeyError, HotkeyEvent, HotkeyStream};

/// Backend over the GlobalShortcuts portal.
#[derive(Default)]
pub struct PortalBackend {
	worker: Option<Worker>,
}

struct Worker {
	stop: Option<oneshot::Sender<()>>,
	thread: Option<JoinHandle<()>>,
}

impl PortalBackend {
	/// An idle backend.
	pub fn new() -> Self {
		Self::default()
	}
}

fn runtime() -> std::io::Result<tokio::runtime::Runtime> {
	tokio::runtime::Builder::new_current_thread()
		.enable_all()
		.build()
}

impl HotkeyBackend for PortalBackend {
	fn name(&self) -> &'static str {
		"xdg-global-shortcuts"
	}

	fn probe(&self) -> BackendStatus {
		// Blocking probe on its own thread so it works inside or outside a runtime.
		let result = std::thread::spawn(|| {
			let rt = runtime().map_err(|e| e.to_string())?;
			rt.block_on(async { GlobalShortcuts::new().await.map(|p| p.version()) })
				.map_err(|e| e.to_string())
		})
		.join();
		match result {
			Ok(Ok(_)) => BackendStatus::Available,
			Ok(Err(e)) => {
				BackendStatus::Unsupported(format!("GlobalShortcuts portal missing: {e}"))
			}
			Err(_) => BackendStatus::Unsupported("portal probe panicked".into()),
		}
	}

	fn start(&mut self, binding: &HotkeyBinding) -> Result<HotkeyStream, HotkeyError> {
		if self.worker.is_some() {
			return Err(HotkeyError::AlreadyStarted);
		}
		let status = self.probe();
		if !status.is_available() {
			return Err(HotkeyError::Unavailable(status));
		}
		let (tx, rx) = mpsc::unbounded_channel();
		let (stop_tx, stop_rx) = oneshot::channel();
		let (ready_tx, ready_rx) = std::sync::mpsc::channel();
		let binding = binding.clone();
		let thread = std::thread::Builder::new()
			.name("relay-portal".into())
			.spawn(move || {
				let Ok(rt) = runtime() else {
					let _ = ready_tx.send(Err("cannot build runtime".to_string()));
					return;
				};
				rt.block_on(session(binding, tx, stop_rx, ready_tx));
			})
			.map_err(|e| HotkeyError::Registration(e.to_string()))?;
		match ready_rx.recv() {
			Ok(Ok(())) => {
				self.worker = Some(Worker {
					stop: Some(stop_tx),
					thread: Some(thread),
				});
				Ok(rx)
			}
			Ok(Err(reason)) => Err(HotkeyError::Registration(reason)),
			Err(_) => Err(HotkeyError::Registration("portal thread died".into())),
		}
	}

	fn stop(&mut self) {
		if let Some(mut worker) = self.worker.take() {
			drop(worker.stop.take());
			if let Some(thread) = worker.thread.take() {
				let _ = thread.join();
			}
		}
	}
}

impl Drop for PortalBackend {
	fn drop(&mut self) {
		self.stop();
	}
}

/// Create the session, bind the shortcut and forward activations until stopped.
async fn session(
	binding: HotkeyBinding,
	tx: mpsc::UnboundedSender<HotkeyEvent>,
	mut stop: oneshot::Receiver<()>,
	ready: std::sync::mpsc::Sender<Result<(), String>>,
) {
	let setup = async {
		let portal = GlobalShortcuts::new().await?;
		let session = portal.create_session(Default::default()).await?;
		let shortcut = NewShortcut::new(binding.id.as_str(), binding.description.as_str())
			.preferred_trigger(binding.accelerator.as_str());
		portal
			.bind_shortcuts(&session, &[shortcut], None, BindShortcutsOptions::default())
			.await?
			.response()?;
		let pressed = portal.receive_activated().await?;
		let released = portal.receive_deactivated().await?;
		Ok::<_, ashpd::Error>((portal, session, pressed, released))
	};
	let (_portal, session, pressed, released) = match setup.await {
		Ok(parts) => parts,
		Err(e) => {
			let _ = ready.send(Err(e.to_string()));
			return;
		}
	};
	let _ = ready.send(Ok(()));
	tokio::pin!(pressed, released);
	loop {
		let edge = tokio::select! {
			_ = &mut stop => break,
			Some(a) = pressed.next() => (a.shortcut_id() == binding.id).then_some(HotkeyEvent::Pressed),
			Some(d) = released.next() => (d.shortcut_id() == binding.id).then_some(HotkeyEvent::Released),
			else => break,
		};
		if let Some(edge) = edge
			&& tx.send(edge).is_err()
		{
			break;
		}
	}
	let _ = session.close().await;
}
