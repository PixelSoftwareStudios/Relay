//! The Calls page: calls in progress, then the people nearby with one-click
//! Call, or what to do when nobody is there.

use egui::Ui;

use crate::layout::Page;
use crate::model::Link;
use crate::viewmodel::{CallCard, PeerRow};

use super::kit::{self, Cx};
use super::{call_card, chat, history, people};
use crate::tr;

pub fn show(ui: &mut Ui, cards: &[CallCard], rows: &[PeerRow], cx: &mut Cx) {
	kit::page_title(ui, tr!("calls-heading"));
	let active: Vec<&CallCard> = cards.iter().filter(|c| !c.incoming).collect();
	for card in &active {
		chat::with_chat(ui, cx, &card.call_id, |ui, cx| {
			call_card::show(ui, card, cx)
		});
		kit::gap(ui);
	}
	let nearby: Vec<&PeerRow> = rows.iter().filter(|r| r.reachable).collect();
	if active.is_empty() && nearby.is_empty() {
		if cx.state.link == Link::Connected {
			empty(ui, cx);
		}
	} else if !nearby.is_empty() {
		kit::section_title(ui, tr!("home-nearby"));
		for row in nearby {
			people::row(ui, row, cx, false);
			kit::gap(ui);
		}
	}
	history::show(ui, cx);
}

fn empty(ui: &mut Ui, cx: &mut Cx) {
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("home-empty-title"));
		kit::help(ui, tr!("home-empty-body"));
		if ui.button(tr!("home-add-by-address")).clicked() {
			cx.local.goto = Some(Page::People);
		}
	});
}
