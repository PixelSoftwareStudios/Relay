//! The eframe wrapper: owns the runtime and the tray, handles close, and
//! delegates drawing to the shared `RelayApp`.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use relay_ui::actions::Actions;
use relay_ui::ui::RelayApp;

use crate::background::{Background, CloseAction, close_action};
use crate::hotkey::HotkeyHost;
use crate::tray::Tray;

pub struct DesktopApp {
	app: RelayApp,
	actions: Actions,
	tray: Option<Tray>,
	background: Arc<Background>,
	/// The global push-to-talk key; dropped before the runtime so a held key is released.
	hotkeys: HotkeyHost,
	/// Keeps the tokio runtime alive for the life of the window.
	_runtime: tokio::runtime::Runtime,
}

impl DesktopApp {
	pub fn new(
		app: RelayApp,
		actions: Actions,
		tray: Option<Tray>,
		background: Arc<Background>,
		hotkeys: HotkeyHost,
		runtime: tokio::runtime::Runtime,
	) -> Self {
		Self {
			app,
			actions,
			tray,
			background,
			hotkeys,
			_runtime: runtime,
		}
	}

	/// Close hides to the tray when one exists and "stay available in the background" is on;
	/// otherwise it quits this app.
	fn handle_close(&self, ctx: &egui::Context) {
		let wants_close = ctx.input(|i| i.viewport().close_requested());
		let action = close_action(
			self.tray.is_some(),
			self.background.enabled(),
			self.actions.shared.quitting.load(Ordering::Acquire),
		);
		if wants_close && action == CloseAction::HideToTray {
			ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
			self.actions.hide_window();
		}
	}
}

impl eframe::App for DesktopApp {
	fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		self.handle_close(ctx);
		// On the main thread, where the OS hotkey is registered.
		self.hotkeys.sync();
		if let (Some(tray), Some(summary)) = (&mut self.tray, self.actions.shared.take_tray()) {
			tray.apply(&summary);
		}
		if let (Some(tray), Some(facts)) = (&mut self.tray, self.background.take_stale()) {
			tray.set_facts(facts);
		}
		self.app.begin_frame(ctx);
	}

	fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
		self.app.ui(ui);
	}
}
