//! The composer pinned at the bottom: an attach button with its menu (offer a
//! file, send the clipboard), a rounded text box that grows to a few lines, and
//! a Send button. Enter sends on a pointer device and Shift+Enter adds a line;
//! on a touch screen Enter adds a line. The byte counter against the 4 KiB
//! limit appears near the limit. Instead of a box there is an explanation
//! when sending is not possible.

use egui::{Align, Layout, Margin, RichText, TextStyle, Ui, vec2};

use super::dialogs::ClipFlow;
use crate::layout::{self, MIN_TOUCH_TARGET};
use crate::messaging::actions::Intent;
use crate::messaging::model::SessionView;
use crate::messaging::rules::{Blocked, Counter, files_blocked};
use crate::theme;
use crate::tr;

/// Remove an Enter press without Shift from this frame's input and say
/// whether there was one. egui's own shortcut matching ignores extra Shift,
/// which would send on Shift+Enter, so the events are inspected directly.
fn take_plain_enter(ui: &Ui) -> bool {
	ui.input_mut(|i| {
		let at = i
			.events
			.iter()
			.position(crate::messaging::rules::is_plain_enter);
		at.map(|n| i.events.remove(n)).is_some()
	})
}

pub fn blocked(
	ui: &mut Ui,
	why: &Blocked,
	session: &SessionView,
	intents: &mut Vec<Intent>,
	confirm_clear: &mut Option<String>,
) {
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		ui.label(RichText::new(&why.reason).color(theme::WARN));
		if why.export_clear {
			ui.horizontal(|ui| {
				if ui.button(tr!("msg-export-json")).clicked() {
					intents.push(export_intent(session));
				}
				if ui.button(tr!("msg-clear")).clicked() {
					*confirm_clear = Some(session.key.clone());
				}
			});
		}
	});
}

pub fn export_intent(session: &SessionView) -> Intent {
	let name: String = session
		.peer_name
		.chars()
		.map(|c| if c.is_alphanumeric() { c } else { '_' })
		.take(32)
		.collect();
	Intent::Export {
		session: session.key.clone(),
		suggested_name: format!("relay-session-{name}.json"),
	}
}

/// Most height the text box grows to before it scrolls (points).
const MAX_BOX_HEIGHT: f32 = 120.0;
/// The byte counter shows from this share of the limit.
const COUNTER_FROM_PERCENT: usize = 80;

fn control_side(ui: &Ui) -> f32 {
	if layout::is_touch(ui) {
		MIN_TOUCH_TARGET
	} else {
		36.0
	}
}

/// The composer. Returns true when a message was handed over for sending.
pub fn editor(
	ui: &mut Ui,
	draft: &mut String,
	session: &SessionView,
	clip: &mut ClipFlow,
	intents: &mut Vec<Intent>,
) -> bool {
	let touch = layout::is_touch(ui);
	let id = ui.make_persistent_id(("relay-msg-compose", &session.key));
	let focused = ui.memory(|m| m.has_focus(id));
	let enter = !touch && focused && take_plain_enter(ui);
	let counter = Counter::of(draft);
	let side = control_side(ui);
	let gap = ui.spacing().item_spacing.x;
	let send_label = tr!("msg-send");
	let send_w = crate::icons::hit_size(ui);
	let mut clicked = false;
	ui.with_layout(Layout::left_to_right(Align::Min), |ui| {
		attach(ui, session, clip, intents);
		let box_w = (ui.available_width() - send_w - gap).max(side);
		let line = ui.text_style_height(&TextStyle::Body);
		let pad = ((side - line) / 2.0).max(0.0) as i8;
		egui::Frame::new()
			.fill(ui.visuals().extreme_bg_color)
			.stroke(ui.visuals().widgets.noninteractive.bg_stroke)
			.corner_radius(side / 2.0)
			.inner_margin(Margin::symmetric(12, 0))
			.show(ui, |ui| {
				ui.set_width(box_w - 24.0 - 2.0);
				egui::ScrollArea::vertical()
					.max_height(MAX_BOX_HEIGHT)
					.auto_shrink([false, true])
					.show(ui, |ui| {
						let hint = if touch {
							tr!("msg-compose-hint-touch")
						} else {
							tr!("msg-compose-hint")
						};
						// The margin is inside the text box, so the whole
						// rounded field takes the tap.
						ui.add(
							egui::TextEdit::multiline(draft)
								.id(id)
								.frame(egui::Frame::NONE)
								.margin(Margin::symmetric(0, pad))
								.desired_rows(1)
								.min_size(vec2(0.0, side))
								.desired_width(f32::INFINITY)
								.hint_text(hint),
						);
					});
			});
		clicked = ui
			.add_enabled(
				counter.sendable(draft),
				crate::icons::IconButton::new(crate::icons::Icon::Send, send_label).accent(),
			)
			.clicked();
	});
	let counter = Counter::of(draft);
	if counter.used * 100 >= counter.limit * COUNTER_FROM_PERCENT {
		ui.horizontal_wrapped(|ui| {
			let label = RichText::new(counter.label()).small();
			ui.label(if counter.over() {
				label.color(theme::ERROR)
			} else {
				label.weak()
			});
			if counter.over() {
				ui.label(
					RichText::new(tr!("msg-too-long"))
						.small()
						.color(theme::ERROR),
				);
			}
		});
	}
	let send = (enter || clicked) && counter.sendable(draft);
	if send {
		intents.push(Intent::Send {
			session: session.key.clone(),
			text: std::mem::take(draft),
		});
		ui.memory_mut(|m| m.request_focus(id));
	}
	send
}

/// The "+" button and its menu: offer a file, send the clipboard.
fn attach(ui: &mut Ui, session: &SessionView, clip: &mut ClipFlow, intents: &mut Vec<Intent>) {
	let button = crate::icons::icon_button(ui, crate::icons::Icon::Attach, tr!("msg-attach"));
	let blocked = files_blocked(session);
	egui::Popup::menu(&button).show(|ui| {
		let offer = crate::icons::icon_label_enabled(
			ui,
			blocked.is_none(),
			crate::icons::Icon::Attach,
			tr!("msg-offer-file"),
		);
		if let Some(why) = blocked {
			offer.clone().on_disabled_hover_text(why);
		}
		if offer.clicked() {
			intents.push(Intent::OfferFile {
				session: session.key.clone(),
			});
		}
		if crate::icons::icon_label(ui, crate::icons::Icon::Copy, tr!("msg-send-clipboard"))
			.on_hover_text(tr!("msg-send-clipboard-hover"))
			.clicked()
		{
			let id = ui.make_persistent_id(("relay-msg-compose", &session.key));
			ui.memory_mut(|m| m.surrender_focus(id));
			ui.ctx()
				.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
			*clip = ClipFlow::Awaiting;
		}
	});
}
