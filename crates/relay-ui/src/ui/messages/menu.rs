//! The small menus of the chat. A kebab button (the More icon)
//! opens an egui popup menu; the popup closes on a choice, on a click
//! outside and on Esc.

use egui::{Id, Popup, PopupCloseBehavior, Rect, Response, Sense, Ui};
use relay_messages::markdown::{Block, Document, Inline};

use crate::icons::{Icon, icon_label};
use crate::messaging::actions::Intent;
use crate::messaging::model::{Delivery, EntryView, SessionView};
use crate::tr;

/// Side of a more-menu button: the icon button size (44 on touch, 32 with a pointer).
pub fn side(ui: &Ui) -> f32 {
	crate::icons::hit_size(ui)
}

/// A kebab button in `rect`. The icon is drawn when `shown` (touch screen, or
/// the pointer is over the row) or while its menu is open. `hover` names it.
pub fn dots_button(ui: &mut Ui, id: Id, rect: Rect, shown: bool, hover: &str) -> Response {
	let response = ui.interact(rect, id, Sense::click());
	response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, hover));
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
	ui.painter().text(
		rect.center(),
		egui::Align2::CENTER_CENTER,
		Icon::More.codepoint(),
		Icon::More.font((rect.height() * 0.6).round()),
		color,
	);
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
			&& icon_label(ui, Icon::Copy, tr!("msg-copy-text")).clicked()
		{
			intents.push(Intent::Copy(text.to_owned()));
		}
		if let Some(url) = link
			&& icon_label(ui, Icon::Copy, tr!("msg-copy-link")).clicked()
		{
			intents.push(Intent::Copy(url.to_owned()));
		}
		let pin = if entry.pinned {
			tr!("msg-unpin")
		} else {
			tr!("msg-pin")
		};
		if icon_label(ui, Icon::Pin, pin).clicked() {
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
			&& icon_label(ui, Icon::Refresh, tr!("msg-retry")).clicked()
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
	crate::icons::icon_label_red(ui, Icon::Trash, text)
}
