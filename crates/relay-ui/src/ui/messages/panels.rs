//! Header, honest empty and error states, session picker, permissions and
//! the Privacy toggle of the Messages panel.

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

/// Session combo; returns the newly chosen key.
pub fn picker(ui: &mut Ui, state: &MessagesState, selected: &str) -> Option<String> {
	let label = |s: &SessionView| {
		format!(
			"{} · {}",
			s.peer_name,
			if s.is_live() {
				tr!("msg-session-live")
			} else {
				tr!("msg-session-ended")
			}
		)
	};
	let current = state.session(selected).map(label).unwrap_or_default();
	let mut chosen = None;
	egui::ComboBox::from_id_salt("relay-msg-session")
		.selected_text(current)
		.width(ui.available_width().min(260.0))
		.show_ui(ui, |ui| {
			for s in &state.sessions {
				if ui.selectable_label(s.key == selected, label(s)).clicked() {
					chosen = Some(s.key.clone());
				}
			}
		});
	chosen
}

/// Permission toggles with the both-sides explanation, and history usage.
pub fn permissions(ui: &mut Ui, session: &SessionView, intents: &mut Vec<Intent>) {
	ui.label(RichText::new(usage_label(session)).small().weak());
	egui::CollapsingHeader::new(tr!("msg-perms-heading"))
		.id_salt(("relay-msg-perms", &session.key))
		.show(ui, |ui| {
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
		});
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
