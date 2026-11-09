//! The chat of one call: the transcript, composer and tools that `ui::chat`
//! places beside or below the call card (regular layout), or full screen
//! (compact layout), and read-only for a call in the history.
//!
//! Drawing holds the messaging state lock and so only records [`Intent`]s;
//! they are dispatched after the lock is released. Idle costs nothing: no
//! repaint is requested except to settle row heights after they were
//! measured.

mod back;
mod bubble;
mod compose;
mod dialogs;
mod image;
mod markdown;
mod menu;
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
use crate::messaging::rules::{compose_blocked, session_of_call, unread_count};
use crate::messaging::state::MessagesState;
use crate::theme;
use crate::tr;
use relay_io::localtime::LocalZone;

pub use dialogs::ClipFlow;
pub use panels::privacy_toggle;

/// Image textures kept at once; evicted ones are decoded again when scrolled to.
const IMAGE_TEXTURES: usize = 48;

pub struct MessagesUi {
	/// The highest message ordinal shown so far per session, for the unread count.
	seen: HashMap<SessionKey, u64>,
	drafts: HashMap<SessionKey, String>,
	search: String,
	pinned_only: bool,
	search_open: bool,
	search_started: bool,
	lists: HashMap<SessionKey, transcript::ListCache>,
	gate: LinkGate,
	clip: ClipFlow,
	confirm_clear: Option<SessionKey>,
	textures: TextureCache<TextureHandle>,
	/// Time zone for the clock in each row.
	zone: LocalZone,
	/// The shell's clock at the start of this frame, for "Today" and "Yesterday".
	now_ms: u64,
}

impl MessagesUi {
	/// `zone` decides how message times are shown (the shell picks it).
	pub fn new(zone: LocalZone) -> Self {
		Self {
			zone,
			now_ms: 0,
			seen: HashMap::new(),
			drafts: HashMap::new(),
			search: String::new(),
			pinned_only: false,
			search_open: false,
			search_started: false,
			lists: HashMap::new(),
			gate: LinkGate::default(),
			clip: ClipFlow::default(),
			confirm_clear: None,
			textures: TextureCache::new(IMAGE_TEXTURES),
		}
	}

	/// Wait for the clipboard text (the paste event), then confirm.
	pub fn begin_clipboard(&mut self) {
		self.clip = ClipFlow::Awaiting;
	}

	/// Messages received from the call (hex id) that its chat has not shown yet.
	pub fn unread(&self, actions: &Actions, call_id: &str) -> usize {
		actions.shared.messages.read(|state| {
			let Some(session) = session_of_call(&state.sessions, call_id) else {
				return 0;
			};
			let entries = state
				.transcript(&session.key)
				.map_or(&[][..], |t| &t.entries);
			unread_count(entries, self.seen.get(&session.key).copied())
		})
	}

	/// Draw the chat of `call_id` (hex) into `ui`. `over` says the call has
	/// ended and is shown from the history, so a missing session means the
	/// daemon no longer keeps it.
	pub fn show(&mut self, ui: &mut Ui, actions: &Actions, call_id: &str, over: bool) {
		actions.shared.messages.open_default();
		self.now_ms = actions.shell.now_unix_ms();
		self.drain(ui.ctx(), actions);
		if matches!(self.clip, ClipFlow::Awaiting)
			&& let Some(text) = dialogs::take_paste(ui)
		{
			self.clip = ClipFlow::Ready { text, picked: None };
		}
		let hub = actions.shared.messages.clone();
		let mut intents = Vec::new();
		hub.read(|state| self.draw(ui, state, call_id, over, &mut intents));
		for intent in intents {
			actions.dispatch(intent);
		}
	}

	/// The "Chat options" button for the header of the chat of `call_id`,
	/// with its menu: search, pinned filter, permissions, export and clear.
	pub fn options_button(&mut self, ui: &mut Ui, actions: &Actions, call_id: &str) {
		let hub = actions.shared.messages.clone();
		let mut intents = Vec::new();
		hub.read(|state| {
			let Some(session) = session_of_call(&state.sessions, call_id) else {
				return;
			};
			let side = menu::side(ui);
			let (rect, _) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::hover());
			let id = ui.make_persistent_id(("relay-msg-options", &session.key));
			let button = menu::dots_button(ui, id, rect, true, tr!("msg-chat-options"));
			let ctl = panels::Options {
				search_open: &mut self.search_open,
				pinned_only: &mut self.pinned_only,
				confirm_clear: &mut self.confirm_clear,
			};
			menu::options(&button, |ui| {
				panels::options(ui, session, ctl, &mut intents)
			});
		});
		for intent in intents {
			actions.dispatch(intent);
		}
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
		call_id: &str,
		over: bool,
		intents: &mut Vec<Intent>,
	) {
		self.seen.retain(|k, _| state.session(k).is_some());
		if let Some(notice) = &state.notice {
			ui.horizontal_wrapped(|ui| {
				ui.label(RichText::new(notice).color(theme::ERROR));
				if ui.button(tr!("action-dismiss")).clicked() {
					intents.push(Intent::DismissNotice);
				}
			});
		}
		if panels::gate(ui, state, intents) {
			self.chat(ui, state, call_id, over, intents);
		}
		dialogs::link(ui, &mut self.gate, intents);
		dialogs::clipboard(ui, &mut self.clip, state, intents);
		dialogs::clear(ui, &mut self.confirm_clear, state, intents);
	}

	fn chat(
		&mut self,
		ui: &mut Ui,
		state: &MessagesState,
		call_id: &str,
		over: bool,
		intents: &mut Vec<Intent>,
	) {
		let Some(session) = session_of_call(&state.sessions, call_id) else {
			let (text, color) = if over {
				(tr!("history-messages-gone"), None)
			} else {
				(tr!("msg-no-session"), Some(theme::WARN))
			};
			let text = RichText::new(text);
			ui.label(match color {
				Some(c) => text.color(c),
				None => text.weak(),
			});
			return;
		};
		let key = session.key.clone();
		if self.search_open {
			self.search_bar(ui);
		}

		let block = compose_blocked(
			&state.feed,
			state.settings.is_none_or(|s| s.enabled),
			session,
		);
		let draft = self.drafts.entry(key.clone()).or_default();
		let mut sent = false;
		let frame = egui::Frame::new()
			.fill(ui.visuals().panel_fill)
			.inner_margin(theme::SPACE_M);
		egui::Panel::bottom("relay-msg-compose-panel")
			.show_separator_line(true)
			.frame(frame)
			.show(ui, |ui| match &block {
				Some(why) => compose::blocked(ui, why, session, intents, &mut self.confirm_clear),
				None => sent = compose::editor(ui, draft, session, &mut self.clip, intents),
			});
		let Some(t) = state.transcript(&key) else {
			ui.label(RichText::new(tr!("msg-loading")).weak());
			return;
		};
		if let Some(last) = t.entries.last() {
			self.seen.insert(key.clone(), last.ordinal);
		}
		let needle = self.search.trim().to_lowercase();
		let cache = self.lists.entry(key).or_default();
		if sent {
			cache.follow_own();
		}
		let mut rcx = transcript::RowCx {
			session,
			settings: state.settings,
			textures: &mut self.textures,
			gate: &mut self.gate,
			zone: &self.zone,
			now_ms: self.now_ms,
			intents,
		};
		egui::CentralPanel::default()
			.frame(egui::Frame::NONE)
			.show(ui, |ui| {
				transcript::show(ui, cache, t, &needle, self.pinned_only, &mut rcx)
			});
	}
}

impl MessagesUi {
	/// The search box, shown from the options menu until closed.
	fn search_bar(&mut self, ui: &mut Ui) {
		ui.horizontal(|ui| {
			let side = menu::side(ui);
			let width = (ui.available_width() - side - ui.spacing().item_spacing.x).max(80.0);
			let edit = ui.add(
				egui::TextEdit::singleline(&mut self.search)
					.hint_text(tr!("msg-search-hint"))
					.desired_width(width),
			);
			if !self.search_started {
				edit.request_focus();
				self.search_started = true;
			}
			if crate::icons::icon_button(ui, crate::icons::Icon::Close, tr!("action-close"))
				.clicked()
			{
				self.search_open = false;
				self.search_started = false;
				self.search.clear();
			}
		});
	}
}
