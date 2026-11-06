//! Following a call to the Calls page. Placing a call and accepting one
//! already go there (the pages set `Local::goto`). A call this client placed
//! that becomes connected, because the other side answered, brings the
//! user to Calls once, unless that would pull them out of something: a dialog,
//! a field being typed in, an open menu, or the chat of a call. State read:
//! `Shared::state`, `Local::awaiting_answer`, `Local::chat`, the confirmations.

use crate::layout::{LayoutClass, Page};
use crate::model::Phase;

use super::RelayApp;

impl RelayApp {
	/// Navigate for calls placed here whose other side answered. Runs on frames
	/// only, so an idle app stays idle; a hidden window waits until it is shown.
	pub(super) fn follow_calls(&mut self, ctx: &egui::Context) {
		let state = self.actions.shared.state();
		let waiting = &mut self.local.awaiting_answer;
		waiting.retain(|id| state.calls.iter().any(|c| &c.call_id == id));
		for call in &state.calls {
			if call.controlled_by_me
				&& matches!(call.phase, Phase::Preparing | Phase::OutgoingRinging)
			{
				waiting.insert(call.call_id.clone());
			}
		}
		if !self.actions.shell.window_visible() {
			return;
		}
		let answered: Vec<&str> = state
			.calls
			.iter()
			.filter(|c| c.phase == Phase::Connected && waiting.contains(&c.call_id))
			.map(|c| c.call_id.as_str())
			.collect();
		if answered.is_empty() {
			return;
		}
		// Looking at Calls already (a chat there stays open): nothing to do.
		if self.page != Page::Calls {
			if self.engaged(ctx) {
				return;
			}
			self.set_page(Page::Calls);
		}
		for id in answered {
			self.local.awaiting_answer.remove(id);
		}
	}

	/// The user is in the middle of something that navigation must not break.
	fn engaged(&self, ctx: &egui::Context) -> bool {
		self.confirmation_open()
			|| self.messages.dialog_open()
			|| ctx.egui_wants_keyboard_input()
			|| ctx.any_popup_open()
			|| self.actions.shell.keyboard_height() > 0.0
	}

	/// A page change on a phone closes menus and drops the focus of a field, which
	/// hides the soft keyboard.
	pub(super) fn settle_navigation(&mut self, ctx: &egui::Context, class: LayoutClass) {
		if self.drawn_page == self.page {
			return;
		}
		self.drawn_page = self.page;
		if class.is_compact() {
			egui::Popup::close_all(ctx);
			ctx.memory_mut(|m| {
				if let Some(id) = m.focused() {
					m.surrender_focus(id);
				}
			});
		}
	}
}
