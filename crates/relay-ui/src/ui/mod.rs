//! egui front end: screen composition. Logic lives in `viewmodel` and
//! `format`; the platform shell calls [`RelayApp::begin_frame`] then
//! [`RelayApp::ui`] each frame and supplies services through `Shell`.
//!
//! Pages: Calls, Contacts, Connect and Channels, plus Settings (a separate page with its
//! own category list). The regular layout navigates with a sidebar, the
//! compact layout with bottom tabs. Messages are not a page: the chat of a
//! call opens from its call card or its history row (see `chat`).

mod banners;
mod call_card;
mod channels;
mod chat;
mod connect;
mod contacts;
mod history;
mod home;
mod incoming;
mod kit;
mod local;
pub mod messages;
mod microphone;
mod nav;
mod nearby;
mod settings;
mod sidebar;
mod textures;
mod visibility;
mod widgets;

use std::time::Instant;

use crate::actions::Actions;
use crate::capabilities::{Capabilities, SettingsCategory};
use crate::imaging::{Decoded, decode_for_texture};
use crate::qr_scan::{ScanTrust, Scanned};
use crate::layout::{LayoutClass, Page};
use crate::shell::Insets;
use crate::theme;
use crate::viewmodel::{call_cards, peer_rows};

use kit::Cx;
use local::{Local, PickedImage};
use messages::MessagesUi;
use textures::{Textures, upload};

/// Header logo, embedded from the repo-root assets folder.
const HEADER_LOGO_PNG: &[u8] = include_bytes!("../../../../assets/icon/relay-mark-64.png");

pub struct RelayApp {
	actions: Actions,
	local: Local,
	textures: Textures,
	logo: egui::TextureHandle,
	/// The layout the installed style was built for; `None` before the first frame.
	class: Option<LayoutClass>,
	page: Page,
	/// The page Settings goes back to (regular layout).
	back_to: Page,
	/// The Settings category chosen; `None` is the list in the compact layout.
	category: Option<SettingsCategory>,
	messages: MessagesUi,
	/// Keyboard height and focused field the view was last scrolled for.
	scrolled_for: Option<(u32, egui::Id)>,
	/// The language the installed fonts were built for; `None` before the first frame.
	font_language: Option<crate::i18n::Language>,
}

impl RelayApp {
	pub fn new(actions: Actions, ctx: &egui::Context) -> Self {
		let logo = decode_for_texture(HEADER_LOGO_PNG).unwrap_or(Decoded {
			size: [1, 1],
			rgba: vec![0; 4],
		});
		let zone = actions.shell.time_zone();
		Self {
			textures: Textures::new(ctx),
			logo: upload(ctx, "relay-logo", &logo),
			actions,
			local: Local::default(),
			class: None,
			page: Page::default(),
			back_to: Page::default(),
			category: None,
			messages: MessagesUi::new(zone),
			scrolled_for: None,
			font_language: None,
		}
	}

	/// The layout the last `begin_frame` chose, `None` before the first frame.
	pub fn layout_class(&self) -> Option<LayoutClass> {
		self.class
	}

	/// Open the chat of a call (hex id) as the call card's Messages button
	/// does, on the Calls page. A call that already ended is read only.
	pub fn open_chat(&mut self, call_id: &str) {
		let state = self.actions.shared.state();
		let cards = call_cards(&state, &self.actions.shared.meters());
		let (name, over) = match cards.iter().find(|c| c.call_id == call_id) {
			Some(card) => (card.name.clone(), false),
			None => (
				state
					.history
					.iter()
					.find(|e| e.call_id == call_id)
					.map(|e| e.peer_name.clone())
					.unwrap_or_default(),
				true,
			),
		};
		self.local.chat = Some(local::ChatTarget {
			call_id: call_id.to_owned(),
			name,
			over,
		});
		self.set_page(Page::Calls);
	}

	/// The call whose chat is open (hex id).
	pub fn chat_call(&self) -> Option<&str> {
		self.local.chat.as_ref().map(|c| c.call_id.as_str())
	}

	/// Close the open chat.
	pub fn close_chat(&mut self) {
		self.local.chat = None;
	}

	/// Start the "send clipboard to a call" flow of the chat: it
	/// reads the clipboard, then asks which call and for confirmation.
	pub fn begin_clipboard_send(&mut self) {
		self.messages.begin_clipboard();
	}

	/// The page shown.
	pub fn page(&self) -> Page {
		self.page
	}

	/// Go to a page. Opening Settings remembers where Back returns to.
	pub fn set_page(&mut self, page: Page) {
		if page == Page::Settings && self.page != Page::Settings {
			self.back_to = self.page;
		}
		if page != Page::Calls {
			self.local.chat = None;
		}
		self.page = page;
	}

	/// Open Settings, on `category` when given (else the last one).
	pub fn open_settings(&mut self, category: Option<SettingsCategory>) {
		if category.is_some() {
			self.category = category;
		}
		self.set_page(Page::Settings);
	}

	/// Leave Settings: Back in the regular layout.
	pub fn close_settings(&mut self) {
		if self.page == Page::Settings {
			self.page = if self.back_to == Page::Settings {
				Page::Calls
			} else {
				self.back_to
			};
		}
	}

	/// The Settings category chosen (`None` is the compact list).
	pub fn settings_category(&self) -> Option<SettingsCategory> {
		self.category
	}

	/// Per-frame work before drawing: window-visible hint, style for the
	/// current width, and moving finished background results into UI state.
	pub fn begin_frame(&mut self, ctx: &egui::Context) {
		self.actions
			.hub
			.set_visible(self.actions.shell.window_visible());
		self.sync_language(ctx);
		let class = LayoutClass::for_width(ctx.content_rect().width());
		if self.class != Some(class) {
			theme::install(ctx, class);
			self.class = Some(class);
			ctx.request_repaint();
		}
		self.drain_inbox(ctx);
	}

	/// Follow the shell's language: switch the catalogs and reinstall the
	/// fonts (their order depends on the language) the frame it changes, and on
	/// the first frame.
	fn sync_language(&mut self, ctx: &egui::Context) {
		if let Some(options) = self.actions.shell.host_options() {
			let wanted = options.language();
			if wanted != crate::i18n::active() {
				crate::i18n::set_active(wanted);
			}
		}
		let active = crate::i18n::active();
		if self.font_language != Some(active) {
			crate::fonts::install(ctx, active);
			self.font_language = Some(active);
			ctx.request_repaint();
		}
	}

	/// Move finished background results into UI-owned state and textures.
	fn drain_inbox(&mut self, ctx: &egui::Context) {
		let inbox = &self.actions.shared.inbox;
		self.textures.drain(ctx, inbox);
		if let Some(result) = inbox.take_connection() {
			self.local.connection = Some(result);
		}
		if let Some(result) = inbox.take_scanned() {
			self.local.scanning = false;
			match crate::qr_scan::outcome(result) {
				Some(Scanned::Note(note)) => self.local.scan_note = Some(note),
				Some(Scanned::Connect(text)) => {
					self.local.scan_trust = Some(ScanTrust::Working);
					self.actions.connect_and_trust(text);
				}
				None => {}
			}
		}
		if let Some(result) = inbox.take_scan_trusted() {
			self.local.scan_trust = Some(result);
		}
		if let Some(p) = inbox.take_picked() {
			let texture = upload(ctx, "picked-avatar", &p.preview);
			self.local.picked = Some(PickedImage {
				bytes: p.bytes,
				media_type: p.media_type,
				texture,
			});
		}
		if let Some(result) = inbox.take_hotkey_loaded() {
			self.local.hotkey.loaded(result);
		}
		if let Some(result) = inbox.take_hotkey_saved() {
			self.local.hotkey.saved(result);
		}
		if let Some(result) = inbox.take_history_saved() {
			self.local.history_saving = false;
			self.local.history_report = Some(result.map_err(crate::setting_support::friendly));
		}
		if let Some(invite) = inbox.take_invite() {
			self.local.invite = Some(invite);
		}
	}

	/// Keyboard: Esc closes the open chat, or leaves Settings (or a category in
	/// the compact layout), Ctrl/Cmd+1 to 4 switch pages and Ctrl/Cmd+, opens Settings. Esc is left
	/// to an open drop-down.
	fn handle_keys(&mut self, ctx: &egui::Context, class: LayoutClass) {
		use egui::{Key, Modifiers};
		let (escape, jump) = ctx.input_mut(|i| {
			let escape = i.consume_key(Modifiers::NONE, Key::Escape);
			let keys = [Key::Num1, Key::Num2, Key::Num3, Key::Num4];
			let jump = keys
				.iter()
				.position(|k| i.consume_key(Modifiers::COMMAND, *k))
				.map(|n| Page::SIDEBAR[n]);
			let settings = i.consume_key(Modifiers::COMMAND, Key::Comma);
			(escape, jump.or(settings.then_some(Page::Settings)))
		});
		if escape && self.local.chat.is_some() && !ctx.any_popup_open() {
			self.local.chat = None;
		} else if escape && self.page == Page::Settings && !ctx.any_popup_open() {
			if class.is_compact() {
				settings::step_back(&mut self.category);
			} else {
				self.close_settings();
			}
		}
		if let Some(page) = jump {
			if page == Page::Settings && self.page == Page::Settings {
				return;
			}
			self.set_page(page);
		}
	}

	/// Draw the whole screen into `ui` (the shell's root ui).
	pub fn ui(&mut self, ui: &mut egui::Ui) {
		let class = self.class.unwrap_or(LayoutClass::Regular);
		let insets = self.actions.shell.safe_area_insets();
		let keyboard = self.actions.shell.keyboard_height().max(0.0);
		self.handle_keys(ui.ctx(), class);
		inset_panels(ui, insets, keyboard);
		let state = self.actions.shared.state();
		let meters = self.actions.shared.meters();
		let error = self.actions.shared.error();
		let cards = call_cards(&state, &meters);
		let rows = peer_rows(&state);
		track_calls(&mut self.local, &cards);
		let ringing = cards.iter().any(|c| c.incoming);
		let mut cx = Cx {
			state: &state,
			meters: &meters,
			actions: &self.actions,
			local: &mut self.local,
			textures: &mut self.textures,
			messages: &mut self.messages,
		};
		let chat_screen = class.is_compact() && cx.local.chat.is_some();
		if class.is_compact() {
			if keyboard <= 0.0 && !chat_screen {
				egui::Panel::bottom("relay-nav")
					.show(ui, |ui| nav::bottom_bar(ui, &mut self.page, ringing));
			}
		} else {
			egui::Panel::left("relay-sidebar")
				.resizable(false)
				.exact_size(theme::SIDEBAR_WIDTH)
				.show(ui, |ui| {
					let logo = &self.logo;
					sidebar::show(ui, &mut self.page, logo, &mut cx, ringing);
				});
		}
		// Settings is a calm page: no strips above it. A ringing call still
		// shows on the Calls entry of the sidebar or the tabs.
		let notices = banners::any(&state, error.as_deref());
		if (notices || ringing) && self.page != Page::Settings {
			egui::Panel::top("relay-top")
				.frame(top_frame(ui, class))
				.show(ui, |ui| {
					ui.set_max_width(ui.available_width().min(theme::CONTENT_MAX_WIDTH));
					banners::show(ui, &state, error.as_deref(), cx.actions);
					let incoming: Vec<_> = cards.iter().filter(|c| c.incoming).collect();
					incoming::show(ui, &incoming, &mut cx);
				});
		}
		let frame = if class.is_compact() {
			egui::Frame::central_panel(ui.style())
		} else {
			egui::Frame::central_panel(ui.style()).inner_margin(theme::SPACE_L)
		};
		let mut back = false;
		egui::CentralPanel::default().frame(frame).show(ui, |ui| {
			match (self.page, class.is_compact()) {
				(Page::Calls, true) if chat::fullscreen(ui, &mut cx) => {}
				(Page::Settings, false) => {
					back = settings::show_regular(ui, &mut self.category, &mut cx);
				}
				_ => scrolling(ui, |ui| match self.page {
					Page::Calls => home::show(ui, &cards, &rows, &mut cx),
					Page::Contacts => contacts::show(ui, &rows, &mut cx),
					Page::Connect => connect::show(ui, &rows, &mut cx),
					Page::Channels => channels::show(ui, &mut cx),
					_ => settings::show_compact(ui, &mut self.category, &mut cx),
				}),
			}
			keep_focus_visible(&mut self.scrolled_for, ui, keyboard);
		});
		if back {
			self.close_settings();
		}
		self.finish_frame(class);
	}

	/// After the pages are drawn: follow what they asked for, let go of a
	/// held talk button that vanished, and drop state of panels not shown.
	fn finish_frame(&mut self, class: LayoutClass) {
		if let Some(page) = self.local.goto.take() {
			self.set_page(page);
		}
		if !self.local.talk_drawn
			&& let Some(call) = self.local.talking.take()
		{
			self.actions.shell.push_to_talk(&call, false);
		}
		self.local.talk_drawn = false;
		let caps = Capabilities::of(&*self.actions.shell);
		let shown = (self.page == Page::Settings).then(|| match class.is_compact() {
			true => self.category,
			false => Some(settings::current(self.category, caps)),
		});
		settings::release_unseen(&mut self.local, shown.flatten());
		if self.page != Page::Connect && self.local.connection_requested {
			connect::forget(&mut self.local);
		}
	}
}

/// A page that scrolls vertically and fills the window.
fn scrolling(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
	egui::ScrollArea::vertical()
		.auto_shrink([false, false])
		.show(ui, |ui| {
			ui.set_max_width(ui.available_width().min(theme::CONTENT_MAX_WIDTH));
			add(ui);
		});
}

/// Frame of the strip above the page (notices and the incoming-call prompt).
fn top_frame(ui: &egui::Ui, class: LayoutClass) -> egui::Frame {
	let margin = if class.is_compact() {
		theme::SPACE_M
	} else {
		theme::SPACE_L
	};
	egui::Frame::side_top_panel(ui.style()).inner_margin(margin)
}

/// Remember when each call was first seen connected, for its duration, and
/// forget calls that are gone.
fn track_calls(local: &mut Local, cards: &[crate::viewmodel::CallCard]) {
	local
		.call_since
		.retain(|id, _| cards.iter().any(|c| &c.call_id == id));
	for card in cards {
		if card.phase == crate::model::Phase::Connected {
			local
				.call_since
				.entry(card.call_id.clone())
				.or_insert_with(Instant::now);
		}
	}
}

/// While the soft keyboard is up, scroll the focused field into view once
/// per keyboard height or focus change (never fighting a later manual scroll).
fn keep_focus_visible(
	scrolled_for: &mut Option<(u32, egui::Id)>,
	ui: &mut egui::Ui,
	keyboard: f32,
) {
	let focused = if keyboard > 0.0 {
		ui.memory(|m| m.focused())
	} else {
		None
	};
	let key = focused.map(|id| (keyboard.to_bits(), id));
	if key == *scrolled_for {
		return;
	}
	*scrolled_for = key;
	if let Some(id) = focused
		&& let Some(response) = ui.ctx().read_response(id)
	{
		ui.scroll_to_rect(response.rect.expand(theme::SPACE_M), None);
	}
}

/// Empty panels that keep the content out of the system bars and the soft
/// keyboard. `keyboard` is the height from the screen bottom, so it covers the
/// bottom inset when shown.
fn inset_panels(ui: &mut egui::Ui, insets: Insets, keyboard: f32) {
	let spacer = |panel: egui::Panel, size: f32, ui: &mut egui::Ui| {
		if size > 0.0 {
			panel
				.resizable(false)
				.exact_size(size)
				.frame(egui::Frame::NONE)
				.show_separator_line(false)
				.show(ui, |_| {});
		}
	};
	spacer(egui::Panel::top("relay-inset-top"), insets.top, ui);
	spacer(egui::Panel::left("relay-inset-left"), insets.left, ui);
	spacer(egui::Panel::right("relay-inset-right"), insets.right, ui);
	let bottom = keyboard.max(insets.bottom);
	spacer(egui::Panel::bottom("relay-inset-bottom"), bottom, ui);
}
