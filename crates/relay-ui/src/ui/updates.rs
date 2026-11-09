//! The update controls: the button above Settings in the sidebar, and the Updates group of
//! Settings, Startup. Both draw the same [`UpdateState`] and call the same actions; neither
//! exists where the shell cannot update itself.

use egui::{RichText, Ui};

use crate::theme;
use crate::tr;
use crate::updates::{UpdateOffer, UpdateState, status_text};

use super::kit::{self, Cx};
use super::widgets::button_row;

/// What the main button does and says, for an offer in `state`.
struct Main {
	label: String,
	/// The press does something (an install, or opening the release page).
	enabled: bool,
}

/// The button for the current state, or `None` when there is nothing to offer (no release, or a
/// check still running).
fn main_button(state: &UpdateState, in_call: bool) -> Option<Main> {
	if let Some(label) = state.progress_label() {
		return Some(Main {
			label,
			enabled: false,
		});
	}
	let offer: &UpdateOffer = match state {
		UpdateState::Available(offer) => offer,
		UpdateState::Failed {
			offer: Some(offer), ..
		} => offer,
		_ => return None,
	};
	if matches!(state, UpdateState::Failed { .. }) {
		return Some(Main {
			label: tr!("action-try-again").to_owned(),
			enabled: !(offer.can_install && in_call),
		});
	}
	Some(if offer.can_install {
		Main {
			label: tr!("update-button", version = offer.version.as_str()),
			enabled: !in_call,
		}
	} else {
		Main {
			label: tr!("update-view-release").to_owned(),
			enabled: true,
		}
	})
}

fn in_call(cx: &Cx) -> bool {
	cx.actions.install_blocked_by_call()
}

/// Whether the sidebar block has anything to draw.
pub fn sidebar_shown(cx: &Cx) -> bool {
	cx.actions.shell.can_self_update() && cx.actions.shared.updates.state().offer().is_some()
}

/// Drawn into the sidebar's bottom-up block right after the Settings entry, so the button sits
/// directly above it, with the call reason or the failure text above the button.
pub fn sidebar(ui: &mut Ui, cx: &mut Cx) {
	if !sidebar_shown(cx) {
		return;
	}
	let state = cx.actions.shared.updates.state();
	let in_call = in_call(cx);
	let Some(main) = main_button(&state, in_call) else {
		return;
	};
	let installs = state.offer().is_some_and(|o| o.can_install);
	// Bottom-up: the first line added is the lowest on screen.
	let button = egui::Button::new(main.label.as_str())
		.min_size(egui::vec2(ui.available_width(), 32.0))
		.wrap();
	if ui.add_enabled(main.enabled, button).clicked() {
		cx.actions.run_update();
	}
	if in_call && installs && !state.busy() {
		kit::help(ui, tr!("update-finish-call"));
	}
	if let UpdateState::Failed { failure, .. } = &state {
		kit::help(ui, failure.text());
	}
	ui.add_space(theme::SPACE_S);
}

/// The Updates group of Settings, Startup.
pub fn settings_group(ui: &mut Ui, cx: &mut Cx) {
	let shell = cx.actions.shell.clone();
	let state = cx.actions.shared.updates.state();
	let in_call = in_call(cx);
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("update-heading"));
		let mut on = shell.update_check_on_launch();
		if ui.checkbox(&mut on, tr!("update-launch-label")).changed() {
			shell.set_update_check_on_launch(on);
		}
		kit::help(ui, tr!("update-privacy"));
		ui.add_space(theme::SPACE_S);
		button_row(ui, |ui| {
			let check = egui::Button::new(tr!("update-check"));
			if ui.add_enabled(!state.busy(), check).clicked() {
				cx.actions.check_for_update(true);
			}
			if let Some(main) = main_button(&state, in_call)
				&& ui
					.add_enabled(main.enabled, egui::Button::new(main.label))
					.clicked()
			{
				cx.actions.run_update();
			}
		});
		if let Some(text) = status_text(&state) {
			ui.label(RichText::new(text));
		}
		let offers_install = state.offer().is_some_and(|o| o.can_install);
		if in_call && offers_install && !state.busy() {
			kit::help(ui, tr!("update-finish-call"));
		}
		if let Some(offer) = state.offer().filter(|o| !o.can_install) {
			kit::help(
				ui,
				&tr!("update-manual-note", version = offer.version.as_str()),
			);
		}
	});
}
