//! Discoverability: whether other devices on the network can find this one.
//! A chip for the sidebar and, on the Connect page, a card that explains the
//! state in one line and changes it through the Privacy setting.

use egui::{Color32, RichText, Ui};

use crate::format::link_label;
use crate::icons::Icon;
use crate::model::{Advert, Inbound, Link};
use crate::theme;
use crate::viewmodel::{Discoverability, discoverability};

use super::kit::{self, Cx};
use super::widgets::button_row;
use crate::tr;

/// The chip's colour, short text and the full sentence (hover text).
fn describe(d: Discoverability, ui: &Ui) -> (Color32, Icon, &'static str, String) {
	match d {
		Discoverability::Discoverable => (
			theme::LIVE,
			Icon::Eye,
			tr!("visibility-discoverable"),
			tr!("visibility-discoverable-help").to_owned(),
		),
		Discoverability::Private => (
			ui.visuals().weak_text_color(),
			Icon::EyeOff,
			tr!("visibility-private"),
			tr!("visibility-private-help").to_owned(),
		),
		Discoverability::NotAdvertising(link) => (
			theme::WARN,
			Icon::WifiOff,
			tr!("visibility-not-advertising"),
			not_advertising(link),
		),
	}
}

fn not_advertising(link: Link) -> String {
	tr!(
		"visibility-not-advertising-reason",
		reason = link_label(link)
	)
	.to_owned()
}

/// A small coloured dot and the state, cut with an ellipsis when narrow.
pub fn chip(ui: &mut Ui, d: Discoverability) {
	let (color, icon, text, hover) = describe(d, ui);
	ui.horizontal(|ui| {
		ui.label(icon.text(12.0).color(color));
		ui.add(egui::Label::new(RichText::new(text).small().color(color)).truncate())
			.on_hover_text(hover);
	});
}

/// The Connect card: state, one sentence, and the switch.
pub fn card(ui: &mut Ui, cx: &mut Cx) {
	let Some(d) = discoverability(cx.state) else {
		return;
	};
	kit::card(ui, |ui| {
		let (color, icon, text, help) = describe(d, ui);
		ui.horizontal(|ui| {
			ui.label(icon.text(18.0).color(color));
			ui.label(RichText::new(text).strong());
		});
		kit::help(ui, &help);
		if let Some(settings) = &cx.state.settings {
			let policy = match settings.inbound {
				Inbound::Anyone => None,
				Inbound::SharedChannelOnly => Some(tr!("privacy-inbound-shared")),
				Inbound::Nobody => Some(tr!("privacy-inbound-nobody")),
			};
			if let Some(policy) = policy {
				kit::help(ui, &format!("{}: {}", tr!("privacy-who-rings"), policy));
			}
			let (label, target) = match settings.advert {
				Advert::Visible => (tr!("visibility-go-private"), Advert::Hidden),
				Advert::Hidden => (tr!("visibility-go-discoverable"), Advert::Visible),
			};
			let ready = cx.state.link == Link::Connected;
			button_row(ui, |ui| {
				if ui.add_enabled(ready, egui::Button::new(label)).clicked() {
					cx.actions
						.run(|c| async move { c.set_privacy(None, Some(target)).await });
				}
			});
		}
	});
}
