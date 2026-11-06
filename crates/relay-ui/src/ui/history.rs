//! The History section of the Calls page: past calls, newest first, with Call back, Show
//! more and a confirmed Clear. It is always shown; with nothing to list it says so.

use std::time::Duration;

use egui::{self, RichText, Ui};

use crate::history_view::{self, Blocked, HistoryRow, PAGE};
use crate::theme;

use super::kit::{self, Cx};
use super::widgets::button_row;
use crate::tr;

/// How often relative times such as "5 min ago" are refreshed while they can change.
const REFRESH: Duration = Duration::from_secs(30);

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	kit::section_title(ui, tr!("history-heading"));
	let enabled = cx.state.settings.as_ref().is_none_or(|s| s.history_enabled);
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
		entry(ui, row, cx);
		kit::gap(ui);
	}
	if history_view::has_more(cx.state, shown) && ui.button(tr!("history-show-more")).clicked() {
		cx.local.history_shown = shown + PAGE;
	}
	clear(ui, cx);
}

fn entry(ui: &mut Ui, row: &HistoryRow, cx: &mut Cx) {
	kit::card(ui, |ui| {
		ui.horizontal_wrapped(|ui| {
			match &row.avatar {
				Some(avatar) => {
					let tex = cx.textures.texture(avatar, cx.actions);
					super::widgets::avatar(ui, &tex, theme::AVATAR_ROW);
				}
				None => initial(ui, &row.name),
			}
			ui.vertical(|ui| {
				let name = RichText::new(&row.name).strong();
				ui.label(if row.missed {
					name.color(theme::ERROR)
				} else {
					name
				});
				ui.horizontal_wrapped(|ui| {
					let marker = RichText::new(row.marker);
					ui.label(if row.missed {
						marker.color(theme::ERROR).strong()
					} else {
						marker.weak()
					});
					let mut detail = row.when.text();
					if let Some(length) = &row.duration {
						detail = format!("{detail} · {length}");
					}
					ui.label(RichText::new(detail).weak());
				});
			});
		});
		let call = ui.add_enabled(
			row.blocked.is_none(),
			egui::Button::new(tr!("history-call-back")),
		);
		if call.clicked() {
			let id = row.peer_id.clone();
			cx.actions.run(|c| async move { c.start_call(&id).await });
		}
		match row.blocked {
			Some(Blocked::Cannot) => kit::help(ui, tr!("peer-call-blocked")),
			Some(Blocked::Offline) => kit::help(ui, tr!("peer-unreachable")),
			None => {}
		}
	});
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
