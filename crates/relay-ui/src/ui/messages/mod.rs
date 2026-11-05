//! The Messages panel: a side panel in the regular layout and a bottom-nav
//! page in the compact one, drawn by the same code.
//!
//! Drawing holds the messaging state lock and so only records [`Intent`]s;
//! they are dispatched after the lock is released. Idle costs nothing: no
//! repaint is requested except to settle row heights after they were
//! measured.

mod compose;
mod dialogs;
mod markdown;
mod offer;
mod panels;
mod transcript;

use std::collections::HashMap;

use egui::{RichText, TextureHandle, Ui};

use crate::actions::Actions;
use crate::avatar::TextureCache;
use crate::imaging::Decoded;
use crate::messaging::actions::Intent;
use crate::messaging::gates::LinkGate;
use crate::messaging::model::SessionKey;
use crate::messaging::rules::{choose_session, compose_blocked};
use crate::messaging::state::{Feed, MessagesState};
use crate::theme;
use relay_io::localtime::LocalZone;

pub use dialogs::ClipFlow;
pub use panels::privacy_toggle;

/// Image textures kept at once; evicted ones are decoded again when scrolled to.
const IMAGE_TEXTURES: usize = 48;

pub struct MessagesUi {
	selected: Option<SessionKey>,
	/// A call whose session should be shown next frame.
	focus_call: Option<String>,
	drafts: HashMap<SessionKey, String>,
	search: String,
	pinned_only: bool,
	lists: HashMap<SessionKey, transcript::ListCache>,
	gate: LinkGate,
	clip: ClipFlow,
	confirm_clear: Option<SessionKey>,
	textures: TextureCache<TextureHandle>,
	/// A call was asked for but has no session.
	missing_call: Option<String>,
	/// Time zone for the clock in each row.
	zone: LocalZone,
}

impl MessagesUi {
	/// `zone` decides how message times are shown (the shell picks it).
	pub fn new(zone: LocalZone) -> Self {
		Self {
			zone,
			selected: None,
			focus_call: None,
			drafts: HashMap::new(),
			search: String::new(),
			pinned_only: false,
			lists: HashMap::new(),
			gate: LinkGate::default(),
			clip: ClipFlow::default(),
			confirm_clear: None,
			textures: TextureCache::new(IMAGE_TEXTURES),
			missing_call: None,
		}
	}

	/// Show the session of this call (hex id) the next time the panel draws.
	pub fn focus_call(&mut self, call_id: String) {
		self.focus_call = Some(call_id);
	}

	/// Wait for the clipboard text (the paste event), then confirm.
	pub fn begin_clipboard(&mut self) {
		self.clip = ClipFlow::Awaiting;
	}

	pub fn selected(&self) -> Option<&str> {
		self.selected.as_deref()
	}

	/// Draw into `ui`. Returns true when the user closed the panel.
	pub fn show(&mut self, ui: &mut Ui, actions: &Actions, closable: bool) -> bool {
		actions.shared.messages.open_default();
		self.drain(ui.ctx(), actions);
		if matches!(self.clip, ClipFlow::Awaiting)
			&& let Some(text) = dialogs::take_paste(ui)
		{
			self.clip = ClipFlow::Ready { text, picked: None };
		}
		let hub = actions.shared.messages.clone();
		let mut intents = Vec::new();
		let closed = hub.read(|state| self.draw(ui, state, closable, &mut intents));
		for intent in intents {
			actions.dispatch(intent);
		}
		closed
	}

	/// Move finished decodes into textures and unsent drafts back to the box.
	fn drain(&mut self, ctx: &egui::Context, actions: &Actions) {
		let hub = &actions.shared.messages;
		for (key, image) in hub.take_images() {
			let texture = image
				.ok()
				.map(|d: Decoded| super::textures::upload(ctx, "msg-image", &d));
			self.textures.fulfill(&key, texture);
		}
		for (session, text) in hub.take_returned() {
			let draft = self.drafts.entry(session).or_default();
			if draft.is_empty() {
				*draft = text;
			}
		}
	}

	fn draw(
		&mut self,
		ui: &mut Ui,
		state: &MessagesState,
		closable: bool,
		intents: &mut Vec<Intent>,
	) -> bool {
		let mut closed = false;
		ui.horizontal(|ui| {
			ui.heading("Messages");
			if closable && ui.button("Close").clicked() {
				closed = true;
			}
		});
		if let Some(notice) = &state.notice {
			ui.horizontal_wrapped(|ui| {
				ui.label(RichText::new(notice).color(theme::ERROR));
				if ui.small_button("Dismiss").clicked() {
					intents.push(Intent::DismissNotice);
				}
			});
		}
		if panels::gate(ui, state, intents) {
			self.sessions(ui, state, intents);
		}
		dialogs::link(ui, &mut self.gate, intents);
		dialogs::clipboard(ui, &mut self.clip, state, intents);
		dialogs::clear(ui, &mut self.confirm_clear, state, intents);
		closed
	}

	fn sessions(&mut self, ui: &mut Ui, state: &MessagesState, intents: &mut Vec<Intent>) {
		let focus = self.focus_call.take();
		let chosen = choose_session(&state.sessions, self.selected.as_deref(), focus.as_deref());
		if let Some(call) = focus {
			self.missing_call = state.session_for_call(&call).is_none().then_some(call);
		}
		self.selected = chosen;
		let Some(key) = self.selected.clone() else {
			ui.label(RichText::new("No call sessions yet. A session starts with a call and its transcript stays readable after the call ends.").weak());
			return;
		};
		if let Some(picked) = panels::picker(ui, state, &key) {
			self.selected = Some(picked);
			self.missing_call = None;
			return;
		}
		if self.missing_call.is_some() {
			ui.label(RichText::new("That call has no messaging session. The other side may not support messages, or messaging is off for it.").color(theme::WARN));
		}
		let Some(session) = state.session(&key) else {
			return;
		};
		panels::permissions(ui, session, intents);
		ui.horizontal(|ui| {
			ui.add(
				egui::TextEdit::singleline(&mut self.search)
					.hint_text("Search this call")
					.desired_width(ui.available_width().min(220.0)),
			);
			ui.checkbox(&mut self.pinned_only, "Pinned");
			if ui.small_button("Export…").clicked() {
				intents.push(compose::export_intent(session));
			}
			if ui.small_button("Clear…").clicked() {
				self.confirm_clear = Some(key.clone());
			}
		});

		let block = compose_blocked(
			&state.feed,
			state.settings.is_none_or(|s| s.enabled),
			session,
		);
		let draft = self.drafts.entry(key.clone()).or_default();
		egui::Panel::bottom("relay-msg-compose-panel")
			.show_separator_line(false)
			.show(ui, |ui| match &block {
				Some(why) => compose::blocked(ui, why, session, intents, &mut self.confirm_clear),
				None => compose::editor(ui, draft, session, &mut self.clip, intents),
			});
		let Some(t) = state.transcript(&key) else {
			ui.label(RichText::new("Loading the transcript…").weak());
			return;
		};
		let needle = self.search.trim().to_lowercase();
		let cache = self.lists.entry(key).or_default();
		let mut rcx = transcript::RowCx {
			session,
			settings: state.settings,
			textures: &mut self.textures,
			gate: &mut self.gate,
			zone: &self.zone,
			intents,
		};
		egui::CentralPanel::default()
			.frame(egui::Frame::NONE)
			.show(ui, |ui| {
				transcript::show(ui, cache, t, &needle, self.pinned_only, &mut rcx)
			});
	}
}

/// Whether the feed shows any transcript at all (used by the layout tests).
pub fn is_live(state: &MessagesState) -> bool {
	state.feed == Feed::Live
}
