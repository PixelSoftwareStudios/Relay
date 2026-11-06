//! One transcript row as a chat bubble: own messages right-aligned in the
//! accent colour, the other side's left-aligned in the card colour. Rows of
//! the same sender are grouped: the name comes once above the group, the time
//! once below it. A three-dot button (always on a touch screen, on hover
//! otherwise) opens the message menu.

use egui::{Align, Color32, CornerRadius, Layout, Margin, Rect, RichText, Ui, UiBuilder, vec2};

use super::transcript::RowCx;
use super::{image, markdown, menu, offer};
use crate::layout::{self, MIN_TOUCH_TARGET};
use crate::messaging::grouping::Place;
use crate::messaging::model::{Body, Delivery, EntryView};
use crate::messaging::rules::{delivery_failure, row_times};
use crate::theme;
use crate::timefmt;
use crate::tr;

/// Share of the row a bubble may take.
const MAX_SHARE: f32 = 0.8;
const BIG_RADIUS: u8 = 16;
const TAIL_RADIUS: u8 = 4;
/// Diameter of the sender's initial.
const AVATAR: f32 = 28.0;
const GAP: f32 = 6.0;

struct Colors {
	fill: Color32,
	text: Color32,
}

fn colors(ui: &Ui, outgoing: bool) -> Colors {
	let v = ui.visuals();
	if outgoing {
		Colors {
			fill: v.selection.bg_fill,
			text: v.selection.stroke.color,
		}
	} else {
		Colors {
			fill: v.widgets.inactive.weak_bg_fill,
			text: v.strong_text_color(),
		}
	}
}

fn radius(outgoing: bool, place: Place) -> CornerRadius {
	let tail = if place.last { TAIL_RADIUS } else { BIG_RADIUS };
	CornerRadius {
		nw: BIG_RADIUS,
		ne: BIG_RADIUS,
		sw: if outgoing { BIG_RADIUS } else { tail },
		se: if outgoing { tail } else { BIG_RADIUS },
	}
}

/// Draw one row: date separator, name, bubble with its menu, delivery
/// state and the group's time.
pub fn row(ui: &mut Ui, rcx: &mut RowCx<'_>, entry: &EntryView, place: Place) {
	if place.new_day {
		separator(ui, rcx, entry);
	}
	ui.add_space(if place.first { theme::SPACE_M } else { 2.0 });
	ui.spacing_mut().item_spacing.y = 2.0;
	let full = ui.available_rect_before_wrap();
	let indent = if entry.outgoing { 0.0 } else { AVATAR + GAP };
	let button = menu::side(ui);
	let max = (full.width() * MAX_SHARE)
		.min(full.width() - indent - button - GAP)
		.max(96.0);
	let mut inner = full;
	inner.min.x += indent;
	let align = if entry.outgoing {
		Align::Max
	} else {
		Align::Min
	};
	if !entry.outgoing && place.first {
		initial(ui, rcx, full);
	}
	let bubble = ui
		.scope_builder(
			UiBuilder::new()
				.max_rect(inner)
				.layout(Layout::top_down(align)),
			|ui| {
				if place.first && !entry.outgoing {
					ui.label(RichText::new(&rcx.session.peer_name).small().strong());
				}
				let rect = bubble(ui, rcx, entry, place, max);
				status(ui, rcx, entry);
				if place.last {
					stamp(ui, rcx, entry);
				}
				rect
			},
		)
		.inner;
	three_dots(ui, rcx, entry, bubble, full);
}

fn separator(ui: &mut Ui, rcx: &RowCx<'_>, entry: &EntryView) {
	ui.add_space(theme::SPACE_M);
	let text = timefmt::day_label(rcx.zone, rcx.now_ms, entry.received_ms);
	ui.vertical_centered(|ui| {
		egui::Frame::new()
			.fill(ui.visuals().widgets.inactive.weak_bg_fill)
			.corner_radius(BIG_RADIUS)
			.inner_margin(Margin::symmetric(10, 3))
			.show(ui, |ui| {
				ui.label(RichText::new(text).small().strong());
			});
	});
	ui.add_space(theme::SPACE_S);
}

/// The sender's first letter in a circle, level with the first bubble.
fn initial(ui: &Ui, rcx: &RowCx<'_>, full: Rect) {
	let name = rcx.session.peer_name.as_str();
	let letter = name
		.chars()
		.find(|c| c.is_alphanumeric())
		.map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect());
	let center = full.min + vec2(AVATAR / 2.0, AVATAR / 2.0 + 14.0);
	let v = ui.visuals();
	ui.painter()
		.circle_filled(center, AVATAR / 2.0, v.widgets.inactive.bg_fill);
	ui.painter().text(
		center,
		egui::Align2::CENTER_CENTER,
		letter,
		egui::FontId::proportional(theme::TEXT_SMALL + 2.0),
		v.text_color(),
	);
}

/// The bubble itself; returns its rectangle.
fn bubble(ui: &mut Ui, rcx: &mut RowCx<'_>, entry: &EntryView, place: Place, max: f32) -> Rect {
	let c = colors(ui, entry.outgoing);
	let touch = layout::is_touch(ui);
	let margin = if touch {
		Margin::symmetric(14, 10)
	} else {
		Margin::symmetric(12, 8)
	};
	egui::Frame::new()
		.fill(c.fill)
		.corner_radius(radius(entry.outgoing, place))
		.inner_margin(margin)
		.show(ui, |ui| {
			ui.set_max_width(max - f32::from(margin.left + margin.right));
			if touch {
				ui.set_min_height(MIN_TOUCH_TARGET - f32::from(margin.top + margin.bottom));
			}
			let v = ui.visuals_mut();
			v.override_text_color = Some(c.text);
			v.hyperlink_color = c.text;
			match &entry.body {
				Body::Text(t) => {
					// Text lines are not controls: no touch-target height.
					ui.spacing_mut().interact_size.y = 0.0;
					markdown::document(ui, &t.doc, rcx.gate, rcx.intents);
				}
				Body::Image(img) => image::show(ui, rcx, entry, img),
				Body::File(o) => offer::card(ui, o, rcx.session, rcx.settings, rcx.intents),
			}
		})
		.response
		.rect
}

/// Own messages that are not delivered yet, or failed, say so under the bubble.
fn status(ui: &mut Ui, rcx: &mut RowCx<'_>, entry: &EntryView) {
	if !entry.outgoing {
		return;
	}
	match entry.delivery {
		Delivery::Sending => {
			ui.label(RichText::new(tr!("msg-sending")).small().weak());
		}
		Delivery::Delivered => {}
		Delivery::Failed => {
			ui.label(
				RichText::new(delivery_failure(&entry.failure))
					.small()
					.color(theme::ERROR),
			);
			let retry = rcx.session.is_live()
				&& ui
					.add(egui::Button::new(RichText::new(tr!("msg-retry")).strong()))
					.clicked();
			if retry {
				rcx.intents.push(crate::messaging::actions::Intent::Retry {
					session: rcx.session.key.clone(),
					seq: entry.seq,
				});
			}
		}
	}
}

/// The time under the last message of a group, with the pin mark and, for
/// the sender's own message, that it arrived.
fn stamp(ui: &mut Ui, rcx: &RowCx<'_>, entry: &EntryView) {
	let times = row_times(rcx.zone, rcx.now_ms, entry.received_ms, entry.sent_ms);
	let mut line = times.label;
	if entry.outgoing && entry.delivery == Delivery::Delivered {
		line = format!("{line} · {}", tr!("msg-delivered"));
	}
	ui.horizontal(|ui| {
		ui.spacing_mut().item_spacing.x = theme::SPACE_S;
		ui.spacing_mut().interact_size.y = 0.0;
		ui.label(RichText::new(line).small().weak())
			.on_hover_text(times.hover);
		if entry.pinned {
			ui.label(RichText::new(tr!("msg-pinned")).small().color(theme::WARN));
		}
	});
}

/// The three-dot button beside the bubble and its menu.
fn three_dots(ui: &mut Ui, rcx: &mut RowCx<'_>, entry: &EntryView, bubble: Rect, full: Rect) {
	let side = menu::side(ui);
	let x = if entry.outgoing {
		bubble.min.x - side - 2.0
	} else {
		bubble.max.x + 2.0
	};
	let top = if layout::is_touch(ui) { 0.0 } else { 2.0 };
	let rect = Rect::from_min_size(egui::pos2(x, bubble.min.y + top), vec2(side, side));
	let id = ui.make_persistent_id(("relay-msg-dots", &rcx.session.key, entry.ordinal));
	let row = Rect::from_x_y_ranges(full.x_range(), bubble.y_range());
	let shown = layout::is_touch(ui) || ui.rect_contains_pointer(row);
	let button = menu::dots_button(ui, id, rect, shown, tr!("msg-more"));
	let link = match &entry.body {
		Body::Text(t) => menu::first_link(&t.doc),
		_ => None,
	};
	menu::message(&button, entry, rcx.session, link, rcx.intents);
}
