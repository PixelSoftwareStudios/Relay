//! Glue between the tokio side and the egui side: the latest published
//! state, user-visible action errors, and repaint requests.

use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use eframe::egui;
use tokio::runtime::Handle;

use crate::controller::{ControlError, Controller};
use crate::emitter::{Sink, TraySummary};
use crate::hub::Hub;
use crate::inbox::Inbox;
use crate::model::{AppState, Meters};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
	m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Everything the UI thread reads. Writers also request a repaint, so an
/// idle app is never redrawn.
pub struct Shared {
	state: Mutex<Arc<AppState>>,
	meters: Mutex<Arc<Meters>>,
	tray: Mutex<Option<TraySummary>>,
	error: Mutex<Option<String>>,
	pub quitting: AtomicBool,
	pub inbox: Inbox,
}

impl Default for Shared {
	fn default() -> Self {
		Self {
			state: Mutex::new(Arc::new(AppState::default())),
			meters: Mutex::new(Arc::new(Meters::default())),
			tray: Mutex::new(None),
			error: Mutex::new(None),
			quitting: AtomicBool::new(false),
			inbox: Inbox::default(),
		}
	}
}

impl Shared {
	pub fn state(&self) -> Arc<AppState> {
		lock(&self.state).clone()
	}

	pub fn meters(&self) -> Arc<Meters> {
		lock(&self.meters).clone()
	}

	/// Tray text changed since the last call, if any.
	pub fn take_tray(&self) -> Option<TraySummary> {
		lock(&self.tray).take()
	}

	pub fn error(&self) -> Option<String> {
		lock(&self.error).clone()
	}

	pub fn clear_error(&self) {
		*lock(&self.error) = None;
	}

	pub fn set_error(&self, message: String) {
		*lock(&self.error) = Some(message);
	}
}

/// Emitter sink that stores the newest values and wakes the UI.
pub struct EguiSink {
	pub shared: Arc<Shared>,
	pub ctx: egui::Context,
}

impl Sink for EguiSink {
	fn state(&self, state: &AppState) {
		*lock(&self.shared.state) = Arc::new(state.clone());
		self.ctx.request_repaint();
	}

	fn meters(&self, meters: &Meters) {
		*lock(&self.shared.meters) = Arc::new(meters.clone());
		self.ctx.request_repaint();
	}

	fn tray(&self, summary: &TraySummary) {
		*lock(&self.shared.tray) = Some(summary.clone());
		self.ctx.request_repaint();
	}
}

/// Runs controller calls off the UI thread and reports failures.
#[derive(Clone)]
pub struct Actions {
	pub controller: Arc<Controller>,
	pub hub: Arc<Hub>,
	pub shared: Arc<Shared>,
	pub runtime: Handle,
	pub ctx: egui::Context,
}

impl Actions {
	pub fn run<F>(&self, action: impl FnOnce(Arc<Controller>) -> F)
	where
		F: Future<Output = Result<(), ControlError>> + Send + 'static,
	{
		let (shared, ctx) = (self.shared.clone(), self.ctx.clone());
		let future = action(self.controller.clone());
		self.runtime.spawn(async move {
			shared.clear_error();
			if let Err(e) = future.await {
				shared.set_error(e.to_string());
			}
			ctx.request_repaint();
		});
	}

	pub fn show_window(&self) {
		self.hub.set_visible(true);
		self.ctx
			.send_viewport_cmd(egui::ViewportCommand::Visible(true));
		self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
	}

	pub fn hide_window(&self) {
		self.hub.set_visible(false);
		self.ctx
			.send_viewport_cmd(egui::ViewportCommand::Visible(false));
	}

	/// Quit this app only; the daemon is a separate process.
	pub fn quit_app(&self) {
		self.shared.quitting.store(true, Ordering::Release);
		self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
	}
}
