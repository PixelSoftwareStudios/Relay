//! The system Back action (the Android button or gesture). The shell calls
//! [`RelayApp::handle_back`] on the UI thread between frames; `true` means
//! the UI used it, `false` means it is at the home screen and the shell may
//! send the app to the background. Esc runs the same `step_out`.
//!
//! State read: `local.chat`, the confirmation flags in `Local`
//! (`confirm_quit_*`, `revoke_confirm`, `leave_confirm`, `forget_confirm`,
//! `trust_confirm`, `confirm_remove_avatar`, `history_confirm_clear`),
//! `messages.dismiss_dialog`, `page`, `category`, `class`.

use std::mem;

use crate::layout::Page;

use super::{RelayApp, settings};

impl RelayApp {
	/// One step back, outermost first: an open dialog or confirmation, the
	/// chat, a Settings category, Settings itself, then any other page to
	/// Calls. `false` when nothing was left to close (Calls, nothing open).
	pub fn handle_back(&mut self) -> bool {
		if self.dismiss_confirmation() || self.messages.dismiss_dialog() {
			return true;
		}
		if self.step_out() {
			return true;
		}
		if self.page != Page::Calls {
			self.set_page(Page::Calls);
			return true;
		}
		false
	}

	/// Close the chat, else step out of Settings (a category to the list on
	/// the phone layout, Settings to the page it was opened from).
	pub(super) fn step_out(&mut self) -> bool {
		if self.local.chat.take().is_some() {
			return true;
		}
		if self.page != Page::Settings {
			return false;
		}
		let compact = self.class.is_some_and(|c| c.is_compact());
		if !(compact && settings::step_back(&mut self.category)) {
			self.close_settings();
		}
		true
	}

	/// Cancel the first inline confirmation that is showing.
	fn dismiss_confirmation(&mut self) -> bool {
		let l = &mut self.local;
		mem::take(&mut l.confirm_quit_app)
			|| mem::take(&mut l.confirm_quit_daemon)
			|| mem::take(&mut l.confirm_remove_avatar)
			|| mem::take(&mut l.history_confirm_clear)
			|| l.revoke_confirm.take().is_some()
			|| l.leave_confirm.take().is_some()
			|| l.forget_confirm.take().is_some()
			|| l.trust_confirm.take().is_some()
	}
}
