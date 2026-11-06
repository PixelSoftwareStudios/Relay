//! Status banners above every page: the daemon link, the last failed action and
//! the notices strip (`notice_strip`), each with a way to dismiss it.

use std::sync::atomic::Ordering;

use egui::{self, Ui};

use crate::actions::Actions;
use crate::format::link_label;
use crate::model::{AppState, Link};

use super::notice_strip::{self, Outcome};
use super::widgets::banner;
use crate::icons::{Icon, icon_label_enabled};
use crate::notice_words::Severity;
use crate::notices::Notice;
use crate::tr;

/// Whether any strip would be drawn.
pub fn any(state: &AppState, error: Option<&str>, notices: &[Notice]) -> bool {
	state.mock || state.link != Link::Connected || error.is_some() || !notices.is_empty()
}

pub fn show(
	ui: &mut Ui,
	state: &AppState,
	error: Option<&str>,
	notices: &[Notice],
	expanded: bool,
	actions: &Actions,
) -> Outcome {
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
	if let Some(e) = error {
		let failed = Notice {
			id: 0,
			ids: Vec::new(),
			severity: Severity::Error,
			title: e.to_owned(),
			next_step: Some(tr!("error-next").to_owned()),
			details: None,
			count: 1,
		};
		if notice_strip::card(ui, &failed).0 {
			actions.shared.clear_error();
		}
	}
	notice_strip::show(ui, notices, expanded)
}

pub(super) fn start_daemon_button(ui: &mut Ui, actions: &Actions) {
	let busy = actions.shared.daemon_busy.load(Ordering::Acquire);
	let label = if busy {
		tr!("daemon-starting-button")
	} else {
		tr!("daemon-start-button")
	};
	if icon_label_enabled(ui, !busy, Icon::Refresh, label).clicked() {
		actions.start_daemon();
	}
}
