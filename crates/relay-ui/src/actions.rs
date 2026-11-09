//! Glue between the runtime side and the UI side: the latest published
//! state, user-visible action errors, and repaint requests through the
//! [`Shell`]. The shell owns the tokio runtime; this crate only holds a `Handle`.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use relay_io::LockExt;

use tokio::runtime::Handle;

use crate::controller::{ControlError, Controller};
use crate::emitter::{Sink, TraySummary};
use crate::hub::Hub;
use crate::inbox::Inbox;
use crate::model::{AppState, Meters};
use crate::shell::Shell;

/// Everything the UI thread reads. Writers also request a repaint, so an
/// idle app is never redrawn.
pub struct Shared {
	state: Mutex<Arc<AppState>>,
	meters: Mutex<Arc<Meters>>,
	tray: Mutex<Option<TraySummary>>,
	error: Mutex<Option<String>>,
	pub quitting: AtomicBool,
	/// A start or stop of the daemon is in flight.
	pub daemon_busy: AtomicBool,
	/// When the app starts the daemon by itself.
	pub autostart: Arc<crate::autostart::AutoStart>,
	pub inbox: Inbox,
	/// Live statistics of the call whose connection details are open.
	pub stats: crate::stats_watch::StatsFeed,
	/// Session messaging state (separate from `Hub`; transcripts can be large).
	pub messages: Arc<crate::messaging::hub::MsgHub>,
	/// Self-update state (desktop only); independent of the daemon.
	pub updates: crate::updates::UpdateHub,
}

impl Default for Shared {
	fn default() -> Self {
		Self {
			state: Mutex::new(Arc::new(AppState::default())),
			meters: Mutex::new(Arc::new(Meters::default())),
			tray: Mutex::new(None),
			error: Mutex::new(None),
			quitting: AtomicBool::new(false),
			daemon_busy: AtomicBool::new(false),
			autostart: Arc::new(crate::autostart::AutoStart::default()),
			inbox: Inbox::default(),
			stats: crate::stats_watch::StatsFeed::default(),
			messages: Arc::default(),
			updates: crate::updates::UpdateHub::default(),
		}
	}
}

impl Shared {
	pub fn state(&self) -> Arc<AppState> {
		self.state.lock_unpoisoned().clone()
	}

	pub fn meters(&self) -> Arc<Meters> {
		self.meters.lock_unpoisoned().clone()
	}

	/// Tray text changed since the last call, if any.
	pub fn take_tray(&self) -> Option<TraySummary> {
		self.tray.lock_unpoisoned().take()
	}

	pub fn error(&self) -> Option<String> {
		self.error.lock_unpoisoned().clone()
	}

	pub fn clear_error(&self) {
		*self.error.lock_unpoisoned() = None;
	}

	pub fn set_error(&self, message: String) {
		*self.error.lock_unpoisoned() = Some(message);
	}
}

/// Emitter sink that stores the newest values and wakes the UI.
pub struct ShellSink {
	pub shared: Arc<Shared>,
	pub shell: Arc<dyn Shell>,
}

impl Sink for ShellSink {
	fn state(&self, state: &AppState) {
		*self.shared.state.lock_unpoisoned() = Arc::new(state.clone());
		self.shell.request_repaint();
	}

	fn meters(&self, meters: &Meters) {
		*self.shared.meters.lock_unpoisoned() = Arc::new(meters.clone());
		self.shell.request_repaint();
	}

	fn tray(&self, summary: &TraySummary) {
		*self.shared.tray.lock_unpoisoned() = Some(summary.clone());
		self.shell.request_repaint();
	}
}

/// Runs controller calls off the UI thread and reports failures.
#[derive(Clone)]
pub struct Actions {
	pub controller: Arc<Controller>,
	pub hub: Arc<Hub>,
	pub shared: Arc<Shared>,
	pub runtime: Handle,
	pub shell: Arc<dyn Shell>,
}

impl Actions {
	pub fn run<F>(&self, action: impl FnOnce(Arc<Controller>) -> F)
	where
		F: Future<Output = Result<(), ControlError>> + Send + 'static,
	{
		let (shared, shell) = (self.shared.clone(), self.shell.clone());
		let future = action(self.controller.clone());
		self.runtime.spawn(async move {
			shared.clear_error();
			if let Err(e) = future.await {
				shared.set_error(e.to_string());
			}
			shell.request_repaint();
		});
	}

	pub fn show_window(&self) {
		self.hub.set_visible(true);
		self.shell.show_window();
	}

	pub fn hide_window(&self) {
		self.hub.set_visible(false);
		self.shell.hide_window();
	}

	/// Quit this app only; the daemon is a separate process.
	///
	/// The calls this app owns are hung up first (bounded, best effort): the
	/// daemon would end them after its grace period anyway, but a clean
	/// hang-up tells the peer at once.
	pub fn quit_app(&self) {
		self.shared.quitting.store(true, Ordering::Release);
		let calls = self
			.hub
			.read(|state| crate::format::own_live_calls(&state.calls));
		if calls.is_empty() {
			self.shell.quit();
			return;
		}
		let (controller, shell) = (self.controller.clone(), self.shell.clone());
		self.runtime.spawn(async move {
			let hang_up = async {
				for id in &calls {
					let _ = controller.end_call(id).await;
				}
			};
			let _ = tokio::time::timeout(std::time::Duration::from_secs(2), hang_up).await;
			shell.quit();
		});
	}

	pub fn copy_text(&self, text: &str) {
		self.shell.copy_text(text);
	}
}
