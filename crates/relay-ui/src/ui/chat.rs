//! Where the chat of a call appears. It is part of the call, never a page: on
//! the regular layout a panel below the call card (or the history row, read
//! only), on the compact layout a full-screen chat with a clear way back.
//! The panel itself is [`MessagesUi`](super::messages::MessagesUi).

use egui::{RichText, Ui};

use crate::theme;
use crate::tr;

use super::kit::Cx;
use super::local::{ChatTarget, Local};

/// Height of the chat panel below a card (points).
const HEIGHT: f32 = 420.0;

/// The chat opened from a card or a history row, if it is the one of `call_id`.
pub fn is_open(local: &Local, call_id: &str) -> bool {
	local.chat.as_ref().is_some_and(|c| c.call_id == call_id)
}

/// Open the chat of a call, or close it when it already shows. `over` marks
/// a call from the history: its chat is read only.
pub fn toggle(local: &mut Local, call_id: &str, name: &str, over: bool) {
	if is_open(local, call_id) {
		local.chat = None;
	} else {
		local.chat = Some(ChatTarget {
			call_id: call_id.to_owned(),
			name: name.to_owned(),
			over,
		});
	}
}

/// A card or row with its chat panel below it while that chat is open. In the
/// compact layout the chat is a screen of its own, so only the row is drawn.
pub fn with_chat(ui: &mut Ui, cx: &mut Cx, call_id: &str, add_row: impl FnOnce(&mut Ui, &mut Cx)) {
	add_row(ui, cx);
	if crate::layout::is_touch(ui) || !is_open(cx.local, call_id) {
		return;
	}
	ui.add_space(theme::SPACE_M);
	egui::Frame::group(ui.style())
		.fill(ui.visuals().faint_bg_color)
		.corner_radius(theme::CARD_RADIUS)
		.inner_margin(theme::SPACE_M)
		.show(ui, |ui| {
			ui.set_width(ui.available_width());
			ui.set_height(HEIGHT);
			panel(ui, cx, true);
		});
	ui.add_space(theme::SPACE_M);
}

/// The full-screen chat of the compact layout. Returns false when nothing is open.
pub fn fullscreen(ui: &mut Ui, cx: &mut Cx) -> bool {
	if cx.local.chat.is_none() {
		return false;
	}
	panel(ui, cx, false);
	true
}

/// Header with the way out, then the chat.
fn panel(ui: &mut Ui, cx: &mut Cx, boxed: bool) {
	let Some(target) = cx.local.chat.clone() else {
		return;
	};
	let mut close = false;
	ui.horizontal(|ui| {
		if boxed {
			ui.label(
				RichText::new(&target.name)
					.size(theme::TEXT_HEADING)
					.strong(),
			);
			ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
				close = ui.button(tr!("action-close")).clicked();
			});
		} else {
			let back = if target.over {
				tr!("action-back")
			} else {
				tr!("chat-back-call")
			};
			close = ui.button(format!("\u{2039} {back}")).clicked();
		}
	});
	if !boxed {
		ui.label(
			RichText::new(&target.name)
				.size(theme::TEXT_HEADING)
				.strong(),
		);
	}
	if close {
		cx.local.chat = None;
		return;
	}
	if target.over {
		ui.label(RichText::new(tr!("chat-read-only")).small().weak());
	}
	cx.messages
		.show(ui, cx.actions, &target.call_id, target.over);
}
