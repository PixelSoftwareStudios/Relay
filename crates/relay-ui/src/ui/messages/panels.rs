//! Honest empty and error states, permissions and the Privacy toggle of the chat.

use egui::{RichText, Ui};

use crate::actions::Actions;
use crate::messaging::actions::Intent;
use crate::messaging::model::SessionView;
use crate::messaging::rules::usage_label;
use crate::messaging::state::{Feed, MessagesState};
use crate::theme;
use crate::tr;

/// What the user sees when the panel cannot show sessions. `true` when the
/// feed is usable (connected and the daemon allows messaging).
pub fn gate(ui: &mut Ui, state: &MessagesState, intents: &mut Vec<Intent>) -> bool {
	let note = |ui: &mut Ui, text: &str| {
		ui.label(RichText::new(text).weak());
	};
	match &state.feed {
		Feed::Off => {
			note(ui, tr!("msg-gate-off"));
			note(ui, tr!("msg-gate-memory"));
			if ui.button(tr!("msg-gate-enable")).clicked() {
				intents.push(Intent::Enable(true));
			}
			false
		}
		Feed::Connecting => {
			note(ui, tr!("msg-gate-connecting"));
			false
		}
		Feed::Denied => {
			ui.label(RichText::new(tr!("problem-permission-denied")).color(theme::ERROR));
			note(ui, tr!("msg-gate-policy"));
			false
		}
		Feed::Offline(why) => {
			ui.label(RichText::new(tr!("msg-gate-disconnected")).color(theme::WARN));
			note(ui, why);
			false
		}
		Feed::Live if state.settings.is_some_and(|s| !s.enabled) => {
			ui.label(RichText::new(tr!("blocked-daemon-off")).color(theme::WARN));
			note(ui, tr!("msg-gate-daemon-off-note"));
			if ui.button(tr!("msg-gate-daemon-enable")).clicked() {
				intents.push(Intent::EnableDaemon);
			}
			true
		}
		Feed::Live => true,
	}
}

/// What the options menu changes besides daemon intents.
pub struct Options<'a> {
	pub search_open: &'a mut bool,
	pub pinned_only: &'a mut bool,
	pub confirm_clear: &'a mut Option<String>,
}

/// The "Chat options" menu: search, the pinned filter, the permissions of the
/// call, export and clear, and how full the history is.
pub fn options(ui: &mut Ui, session: &SessionView, ctl: Options<'_>, intents: &mut Vec<Intent>) {
	ui.set_min_width(220.0);
	if ui.button(tr!("msg-search")).clicked() {
		*ctl.search_open = true;
		ui.close();
	}
	ui.checkbox(ctl.pinned_only, tr!("msg-pinned-filter"));
	ui.menu_button(tr!("msg-perms-heading"), |ui| {
		permissions(ui, session, intents)
	});
	ui.separator();
	if ui.button(tr!("msg-export")).clicked() {
		intents.push(super::compose::export_intent(session));
		ui.close();
	}
	if super::menu::danger(ui, tr!("msg-clear")).clicked() {
		*ctl.confirm_clear = Some(session.key.clone());
		ui.close();
	}
	ui.separator();
	ui.label(RichText::new(usage_label(session)).small().weak());
}

/// Permission toggles with the both-sides explanation.
fn permissions(ui: &mut Ui, session: &SessionView, intents: &mut Vec<Intent>) {
	ui.set_max_width(280.0);
	let live = session.is_live();
	let (mut msgs, mut files) = (session.perms.allow_messages, session.perms.allow_files);
	ui.add_enabled_ui(live, |ui| {
		ui.checkbox(&mut msgs, tr!("msg-perm-messages"));
		ui.checkbox(&mut files, tr!("msg-perm-files"));
	});
	if msgs != session.perms.allow_messages || files != session.perms.allow_files {
		intents.push(Intent::SetPerms {
			session: session.key.clone(),
			allow_messages: (msgs != session.perms.allow_messages).then_some(msgs),
			allow_files: (files != session.perms.allow_files).then_some(files),
		});
	}
	let who = if session.local_is_host {
		tr!("msg-perms-host")
	} else {
		tr!("msg-perms-guest")
	};
	ui.label(
		RichText::new(tr!("msg-perms-note", who = who))
			.small()
			.weak(),
	);
}

/// The Privacy panel's opt-in for the `messages` capability.
pub fn privacy_toggle(ui: &mut Ui, actions: &Actions) {
	let hub = &actions.shared.messages;
	let mut on = hub.enabled();
	let was = on;
	ui.checkbox(&mut on, tr!("msg-privacy-label"));
	ui.label(RichText::new(tr!("msg-privacy-note")).small().weak());
	if on != was {
		actions.dispatch(Intent::Enable(on));
	}
}
