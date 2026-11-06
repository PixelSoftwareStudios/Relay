//! The small menus of the chat. A three-dot button (drawn, three circles)
//! opens an egui popup menu; the popup closes on a choice, on a click
//! outside and on Esc.

use egui::{Color32, Id, Popup, PopupCloseBehavior, Rect, Response, RichText, Sense, Ui, vec2};
use relay_messages::markdown::{Block, Document, Inline};

use crate::layout::{self, MIN_TOUCH_TARGET};
use crate::messaging::actions::Intent;
use crate::messaging::model::{Delivery, EntryView, SessionView};
use crate::tr;

/// Side of the three-dot button on a pointer device (points).
const DESKTOP_SIDE: f32 = 26.0;

/// Side of a three-dot button: a full touch target on a touch screen.
pub fn side(ui: &Ui) -> f32 {
	if layout::is_touch(ui) {
		MIN_TOUCH_TARGET
	} else {
		DESKTOP_SIDE
	}
}

/// Three circles in a column, centred in `rect`.
pub fn paint_dots(ui: &Ui, rect: Rect, color: Color32) {
	let step = (rect.height().min(rect.width()) / 6.0).clamp(3.0, 5.0);
	for n in [-1.0, 0.0, 1.0] {
		let at = rect.center() + vec2(0.0, n * step);
		ui.painter().circle_filled(at, 1.7, color);
	}
}

/// A three-dot button in `rect`. The dots are drawn when `shown` (touch
/// screen, or the pointer is over the row) or while its menu is open.
/// `hover` names it.
pub fn dots_button(ui: &mut Ui, id: Id, rect: Rect, shown: bool, hover: &str) -> Response {
	let response = ui.interact(rect, id, Sense::click());
	let open = Popup::is_id_open(ui.ctx(), Popup::default_response_id(&response));
	if !shown && !open {
		return response;
	}
	let visuals = ui.visuals();
	let color = if response.hovered() || open {
		visuals.strong_text_color()
	} else {
		visuals.weak_text_color()
	};
	if response.hovered() || open {
		let fill = visuals.widgets.hovered.weak_bg_fill;
		ui.painter().rect_filled(rect, rect.height() / 2.0, fill);
	}
	paint_dots(ui, rect, color);
	response.on_hover_text(hover)
}

/// The first link of a document, for "Copy link".
pub fn first_link(doc: &Document) -> Option<&str> {
	fn in_inlines(inlines: &[Inline]) -> Option<&str> {
		inlines.iter().find_map(|i| match i {
			Inline::Link(l) => Some(l.url.as_str()),
			Inline::Bold(c) | Inline::Italic(c) => in_inlines(c),
			_ => None,
		})
	}
	doc.blocks.iter().find_map(|b| match b {
		Block::Paragraph(i) => in_inlines(i),
		Block::List { items, .. } => items.iter().find_map(|i| in_inlines(i)),
		Block::Code(_) => None,
	})
}

/// The menu of one message, attached to its three-dot button.
pub fn message(
	button: &Response,
	entry: &EntryView,
	session: &SessionView,
	link: Option<&str>,
	intents: &mut Vec<Intent>,
) {
	Popup::menu(button).show(|ui| {
		if let Some(text) = entry.text()
			&& ui.button(tr!("msg-copy-text")).clicked()
		{
			intents.push(Intent::Copy(text.to_owned()));
		}
		if let Some(url) = link
			&& ui.button(tr!("msg-copy-link")).clicked()
		{
			intents.push(Intent::Copy(url.to_owned()));
		}
		let pin = if entry.pinned {
			tr!("msg-unpin")
		} else {
			tr!("msg-pin")
		};
		if ui.button(pin).clicked() {
			intents.push(Intent::Pin {
				session: session.key.clone(),
				outgoing: entry.outgoing,
				seq: entry.seq,
				pinned: !entry.pinned,
			});
		}
		if entry.outgoing
			&& entry.delivery == Delivery::Failed
			&& session.is_live()
			&& ui.button(tr!("msg-retry")).clicked()
		{
			intents.push(Intent::Retry {
				session: session.key.clone(),
				seq: entry.seq,
			});
		}
	});
}

/// A menu that stays open while its checkboxes change; it closes on a click
/// outside, on Esc and when a plain item is chosen.
pub fn options(button: &Response, add: impl FnOnce(&mut Ui)) -> Option<egui::InnerResponse<()>> {
	Popup::menu(button)
		.close_behavior(PopupCloseBehavior::CloseOnClickOutside)
		.show(add)
}

/// A destructive menu item.
pub fn danger(ui: &mut Ui, text: &str) -> Response {
	ui.button(RichText::new(text).color(crate::theme::ERROR))
}
