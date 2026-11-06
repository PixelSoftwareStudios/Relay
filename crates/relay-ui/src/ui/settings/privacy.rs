//! Privacy and trust: whether nearby devices can find this one, session
//! messages on or off, and the devices that are trusted.

use egui::Ui;

use crate::model::Advert;
use crate::viewmodel::peer_rows;

use super::super::kit::{self, Cx};
use super::super::people;
use super::super::widgets::combo;
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	if let Some(settings) = &cx.state.settings {
		let actions = cx.actions;
		kit::card(ui, |ui| {
			let label = |v: Advert| match v {
				Advert::Visible => tr!("privacy-advert-visible"),
				Advert::Hidden => tr!("privacy-advert-hidden"),
			};
			let mut advert = settings.advert;
			combo(ui, tr!("privacy-visibility"), label(advert)).show_ui(ui, |ui| {
				for v in [Advert::Visible, Advert::Hidden] {
					ui.selectable_value(&mut advert, v, label(v));
				}
			});
			if advert != settings.advert {
				actions.run(|c| async move { c.set_privacy(None, Some(advert)).await });
			}
			kit::help(ui, tr!("privacy-visibility-help"));
		});
		kit::gap(ui);
	} else {
		kit::help(ui, tr!("privacy-none"));
	}
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("privacy-messages"));
		super::super::messages::privacy_toggle(ui, cx.actions);
	});
	kit::gap(ui);
	trusted(ui, cx);
}

/// Devices a trust decision is stored for, each with a confirmed Forget.
fn trusted(ui: &mut Ui, cx: &mut Cx) {
	kit::section_title(ui, tr!("privacy-trusted"));
	let rows: Vec<_> = peer_rows(cx.state)
		.into_iter()
		.filter(|r| r.known)
		.collect();
	if rows.is_empty() {
		kit::help(ui, tr!("privacy-trusted-none"));
		return;
	}
	kit::help(ui, tr!("privacy-trusted-help"));
	for row in &rows {
		kit::card(ui, |ui| people::trusted_row(ui, row, cx));
		kit::gap(ui);
	}
}
