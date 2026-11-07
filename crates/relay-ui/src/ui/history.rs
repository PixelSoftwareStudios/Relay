//! The History section of the Calls page: past calls, newest first, with Call back, Show
//! more and a confirmed Clear. It is always shown; with nothing to list it says so.

use std::time::Duration;

use egui::{self, RichText, Ui};

use crate::history_view::{self, Blocked, HistoryRow, PAGE};
use crate::icons::{Icon, icon_inline};
use crate::messaging::rules::{Kept, transcript_kept};
use crate::theme;

use super::chat;
use super::collapsing::{ChevronSide, collapsing_header};
use super::kit::{self, Cx};
use super::widgets::button_row;
use crate::tr;

/// How often "Just now" is refreshed while it can change.
const REFRESH: Duration = Duration::from_secs(30);

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	ui.add_space(theme::SPACE_M);
	collapsing_header(
		ui,
		"history-section",
		tr!("history-heading"),
		false,
		ChevronSide::Left,
		|ui| {
			ui.add_space(theme::SPACE_S);
			body(ui, cx);
		},
	);
}

fn body(ui: &mut Ui, cx: &mut Cx) {
	let enabled = cx
		.state
		.settings
		.as_ref()
		.is_none_or(super::outdated::history_on);
	if !enabled {
		kit::help(ui, tr!("history-off-note"));
	}
	if cx.state.history.is_empty() {
		kit::empty_state(ui, tr!("history-empty"), tr!("history-empty-help"));
		return;
	}
	let shown = if cx.local.history_shown == 0 {
		PAGE
	} else {
		cx.local.history_shown
	};
	let zone = cx.actions.shell.time_zone();
	let now = cx.actions.shell.now_unix_ms();
	let rows = history_view::rows(cx.state, shown, now, &zone);
	if rows.iter().any(|r| r.when.is_recent()) {
		ui.ctx().request_repaint_after(REFRESH);
	}
	for row in &rows {
		chat::with_chat(ui, cx, &row.call_id, |ui, cx| entry(ui, row, cx));
		kit::gap(ui);
	}
	if history_view::has_more(cx.state, shown) && ui.button(tr!("history-show-more")).clicked() {
		cx.local.history_shown = shown + PAGE;
	}
	clear(ui, cx);
}

fn entry(ui: &mut Ui, row: &HistoryRow, cx: &mut Cx) {
	kit::card(ui, |ui| {
		let contact = is_contact(cx, &row.peer_id);
		ui.horizontal_top(|ui| {
			match &row.avatar {
				Some(avatar) => {
					let tex = cx.textures.texture(avatar, cx.actions);
					super::widgets::avatar(ui, &tex, theme::AVATAR_ROW);
				}
				None => initial(ui, &row.name),
			}
			ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
				if let Some(length) = &row.duration {
					duration_badge(ui, length);
				}
				ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
					let red = row.missed.then_some(theme::ERROR);
					if kit::name_link(ui, &row.name, red, contact).clicked() {
						cx.local.focus_contact = Some(row.peer_id.clone());
						cx.local.goto = Some(crate::layout::Page::Contacts);
					}
					ui.horizontal_wrapped(|ui| {
						let tint = if row.unanswered {
							theme::ERROR
						} else {
							ui.visuals().weak_text_color()
						};
						icon_inline(ui, row.icon, tint);
						ui.label(RichText::new(row.marker).color(tint));
						ui.label(RichText::new(row.when.text()).weak().small())
							.on_hover_text(&row.when_hover);
					});
				});
			});
		});
		let kept = cx
			.actions
			.shared
			.messages
			.read(|state| transcript_kept(state, &row.call_id));
		button_row(ui, |ui| {
			let call = ui.add_enabled(
				row.blocked.is_none(),
				egui::Button::new(tr!("history-call-back")),
			);
			if call.clicked() {
				let id = row.peer_id.clone();
				cx.local.goto = Some(crate::layout::Page::Calls);
				cx.actions.run(|c| async move { c.start_call(&id).await });
			}
			messages_button(ui, row, kept, cx);
		});
		if kept == Kept::No {
			kit::help(ui, tr!("history-messages-gone"));
		}
		match row.blocked {
			Some(Blocked::Cannot) => kit::help(ui, tr!("peer-call-blocked")),
			Some(Blocked::Offline) => kit::help(ui, tr!("peer-unreachable")),
			None => {}
		}
	});
}

/// How long the call lasted: its own right-aligned pill with a clock, in a monospace face,
/// so it cannot be read as part of the time the call happened.
fn duration_badge(ui: &mut Ui, length: &str) {
	let fill = ui.visuals().widgets.inactive.bg_fill;
	egui::Frame::new()
		.fill(fill)
		.corner_radius(theme::CARD_RADIUS)
		.inner_margin(egui::Margin::symmetric(8, 4))
		.show(ui, |ui| {
			ui.horizontal(|ui| {
				ui.spacing_mut().item_spacing.x = theme::SPACE_S;
				icon_inline(ui, Icon::Clock, ui.visuals().text_color());
				ui.label(RichText::new(length).monospace().strong());
			});
		});
}

/// Whether the peer is a trusted device, so Contacts has a card for it.
fn is_contact(cx: &Cx, peer_id: &str) -> bool {
	cx.state
		.peers
		.iter()
		.any(|p| p.peer_id == peer_id && p.trust != crate::model::Trust::Unknown)
}

/// Opens or closes the read-only transcript of the call. It is a normal button like Call back
/// (highlighted only while the transcript is open) and off only when the daemon reports it no
/// longer keeps that transcript; while that is not known yet (messaging is not connected)
/// pressing it connects and finds out.
fn messages_button(ui: &mut Ui, row: &HistoryRow, kept: Kept, cx: &mut Cx) {
	let open = chat::is_open(cx.local, &row.call_id);
	let button = egui::Button::new(tr!("history-view-messages")).selected(open);
	let response = ui.add_enabled(kept != Kept::No, button);
	if kept == Kept::No {
		response.on_disabled_hover_text(tr!("history-messages-gone"));
	} else {
		let response = match kept {
			Kept::Yes(count) if count > 0 => {
				response.on_hover_text(tr!("history-messages-count", count = count))
			}
			_ => response,
		};
		if response.clicked() {
			chat::toggle(cx.local, &row.call_id, &row.name, true);
		}
	}
}

/// A round badge with the first letter of the name, for a device that is no longer known.
fn initial(ui: &mut Ui, name: &str) {
	let size = theme::AVATAR_ROW;
	let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
	let letter = name
		.chars()
		.find(|c| c.is_alphanumeric())
		.map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect());
	let visuals = ui.visuals();
	ui.painter()
		.circle_filled(rect.center(), size / 2.0, visuals.widgets.inactive.bg_fill);
	ui.painter().text(
		rect.center(),
		egui::Align2::CENTER_CENTER,
		letter,
		egui::FontId::proportional(theme::TEXT_HEADING),
		visuals.text_color(),
	);
}

/// Two-step "Clear history": the first click asks, the second one deletes.
fn clear(ui: &mut Ui, cx: &mut Cx) {
	button_row(ui, |ui| {
		if cx.local.history_confirm_clear {
			ui.label(tr!("history-clear-ask"));
			if kit::danger_button(ui, tr!("history-clear-yes")).clicked() {
				cx.actions
					.run(|c| async move { c.clear_call_history().await });
				cx.local.history_confirm_clear = false;
				cx.local.history_shown = 0;
			}
			if ui.button(tr!("history-clear-keep")).clicked() {
				cx.local.history_confirm_clear = false;
			}
		} else if ui.button(tr!("history-clear")).clicked() {
			cx.local.history_confirm_clear = true;
		}
	});
}
