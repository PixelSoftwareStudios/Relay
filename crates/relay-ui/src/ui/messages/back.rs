//! Back closes the chat's open dialog.

use super::{ClipFlow, MessagesUi};

impl MessagesUi {
	/// Cancel the link, clipboard or clear-history dialog that is showing.
	pub(crate) fn dismiss_dialog(&mut self) -> bool {
		if self.gate.pending().is_some() {
			self.gate.cancel();
			return true;
		}
		if !matches!(self.clip, ClipFlow::Idle) {
			self.clip = ClipFlow::Idle;
			return true;
		}
		self.confirm_clear.take().is_some()
	}
}
