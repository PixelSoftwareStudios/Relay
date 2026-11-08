//! The Calls page: calls in progress, then the favorites for one-tap calling, then the
//! call history (collapsed until opened).

use egui::Ui;

use crate::viewmodel::{CallCard, PeerRow};

use super::kit::{self, Cx};
use super::{available, call_card, chat, favorites, history, microphone, outdated, startup};
use crate::tr;

pub fn show(ui: &mut Ui, cards: &[CallCard], rows: &[PeerRow], cx: &mut Cx) {
	kit::page_title(ui, tr!("calls-heading"));
	available::first_run(ui, cx);
	available::chip_compact(ui, cx);
	startup::card(ui, cx);
	outdated::card(ui, cx);
	microphone::show(ui, cx);
	for card in cards.iter().filter(|c| !c.incoming) {
		chat::with_chat(ui, cx, &card.call_id, |ui, cx| {
			call_card::show(ui, card, cx)
		});
		kit::gap(ui);
	}
	favorites::show(ui, rows, cx);
	history::show(ui, cx);
}
