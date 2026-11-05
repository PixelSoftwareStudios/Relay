//! Header, honest empty and error states, session picker, permissions and
//! the Privacy toggle of the Messages panel.

use egui::{RichText, Ui};

use crate::actions::Actions;
use crate::messaging::actions::Intent;
use crate::messaging::model::SessionView;
use crate::messaging::rules::usage_label;
use crate::messaging::state::{Feed, MessagesState};
use crate::theme;

/// What the user sees when the panel cannot show sessions. `true` when the
/// feed is usable (connected and the daemon allows messaging).
pub fn gate(ui: &mut Ui, state: &MessagesState, intents: &mut Vec<Intent>) -> bool {
	let note = |ui: &mut Ui, text: &str| {
		ui.label(RichText::new(text).weak());
	};
	match &state.feed {
		Feed::Off => {
			note(
				ui,
				"Messages are off in this app. Turning them on lets this app read and send the text, images and file offers of your calls through the daemon.",
			);
			note(
				ui,
				"Nothing is stored on disk: transcripts live in the daemon's memory and end with it.",
			);
			if ui.button("Turn on messages").clicked() {
				intents.push(Intent::Enable(true));
			}
			false
		}
		Feed::Connecting => {
			note(ui, "Connecting to the daemon…");
			false
		}
		Feed::Denied => {
			ui.label(
				RichText::new("The daemon did not grant this app the messages capability.")
					.color(theme::ERROR),
			);
			note(
				ui,
				"Transcripts are private to the people in the call, so only a client that asks for the capability can read them. Check the daemon's client policy; this app asks again every 30 seconds.",
			);
			false
		}
		Feed::Offline(why) => {
			ui.label(
				RichText::new("Not connected to the daemon, so messages are unavailable.")
					.color(theme::WARN),
			);
			note(ui, why);
			false
		}
		Feed::Live if state.settings.is_some_and(|s| !s.enabled) => {
			ui.label(
				RichText::new("Messaging is turned off in the daemon (messages.enabled = false).")
					.color(theme::WARN),
			);
			note(
				ui,
				"No messages are sent or received. Existing transcripts stay readable.",
			);
			if ui.button("Turn on in the daemon").clicked() {
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
			if s.is_live() { "live" } else { "ended" }
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
	egui::CollapsingHeader::new("Permissions for this call")
		.id_salt(("relay-msg-perms", &session.key))
		.show(ui, |ui| {
			let live = session.is_live();
			let (mut msgs, mut files) = (session.perms.allow_messages, session.perms.allow_files);
			ui.add_enabled_ui(live, |ui| {
				ui.checkbox(&mut msgs, "Allow messages");
				ui.checkbox(&mut files, "Allow images and files");
			});
			if msgs != session.perms.allow_messages || files != session.perms.allow_files {
				intents.push(Intent::SetPerms {
					session: session.key.clone(),
					allow_messages: (msgs != session.perms.allow_messages).then_some(msgs),
					allow_files: (files != session.perms.allow_files).then_some(files),
				});
			}
			let who = if session.local_is_host {
				"You started this call, so your choice is pushed to the other side."
			} else {
				"The other side started this call. Your change stays on this device, and their next change replaces it."
			};
			ui.label(RichText::new(format!(
				"Both daemons enforce these: a side that has an item turned off refuses it, whatever the other side's app shows. Turning one off stops new items; it does not delete existing ones. {who}"
			)).small().weak());
		});
}

/// The Privacy panel's opt-in for the `messages` capability.
pub fn privacy_toggle(ui: &mut Ui, actions: &Actions) {
	let hub = &actions.shared.messages;
	let mut on = hub.enabled();
	let was = on;
	ui.checkbox(&mut on, "Let this app read and send call messages");
	ui.label(
		RichText::new("Grants this app the daemon's messages capability on a separate connection: it can read every call's transcript, send text, images and file offers, and save files you accept. It is switched on the first time you open Messages. Turning it off disconnects and forgets what this app holds; the daemon keeps its own copy until the call history is cleared or the daemon exits.")
			.small()
			.weak(),
	);
	if on != was {
		actions.dispatch(Intent::Enable(on));
	}
}
