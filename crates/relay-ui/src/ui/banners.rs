//! Status banners above every page: daemon link, warnings, the last failed
//! action (with a way to dismiss it) and notices.

use std::sync::atomic::Ordering;

use egui::{self, Ui};

use crate::actions::Actions;
use crate::format::link_label;
use crate::model::{AppState, Link};

use super::kit;
use super::widgets::banner;
use crate::tr;

/// The engine's code for a device that connected and waits for a trust decision.
/// Never shown: trust is decided in Connect, under "Found nearby", or by a scan.
const TRUST_REQUIRED: &str = "TRUST_REQUIRED";

/// The warnings worth showing.
fn shown(state: &AppState) -> impl Iterator<Item = &crate::model::Problem> {
	state.warnings.iter().filter(|w| w.code != TRUST_REQUIRED)
}

/// Whether any notice would be drawn.
pub fn any(state: &AppState, error: Option<&str>) -> bool {
	state.mock
		|| state.link != Link::Connected
		|| shown(state).next().is_some()
		|| error.is_some()
		|| state.notice.is_some()
}

pub fn show(ui: &mut Ui, state: &AppState, error: Option<&str>, actions: &Actions) {
	if state.mock {
		banner(ui, tr!("banner-mock"), false);
	} else if state.link != Link::Connected {
		let mut text = format!("{}.", link_label(state.link));
		if let Some(e) = &state.link_error {
			text.push_str(&format!(" {e}"));
		}
		if state.link == Link::Offline {
			text.push(' ');
			text.push_str(
				if actions.shared.autostart.enabled() && !actions.shared.autostart.paused() {
					tr!("link-offline-auto")
				} else {
					tr!("link-offline-manual")
				},
			);
		}
		banner(ui, &text, state.link == Link::Offline);
		if state.link == Link::Offline && actions.shell.can_manage_daemon() {
			start_daemon_button(ui, actions);
		}
	}
	for w in shown(state) {
		banner(ui, &w.message, false);
	}
	if let Some(e) = error {
		banner(ui, e, true);
		kit::help(ui, tr!("error-next"));
		if ui.button(tr!("action-dismiss")).clicked() {
			actions.shared.clear_error();
		}
	}
	if let Some(n) = &state.notice {
		banner(ui, n, false);
	}
}

pub(super) fn start_daemon_button(ui: &mut Ui, actions: &Actions) {
	let busy = actions.shared.daemon_busy.load(Ordering::Acquire);
	let label = if busy {
		tr!("daemon-starting-button")
	} else {
		tr!("daemon-start-button")
	};
	if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
		actions.start_daemon();
	}
}
