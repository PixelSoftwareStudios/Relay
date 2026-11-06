//! The Calls page: calls in progress, then the favorites for one-tap calling, then the
//! call history (collapsed until opened).

use egui::Ui;

use crate::viewmodel::{CallCard, PeerRow};

use super::kit::{self, Cx};
use super::{call_card, chat, favorites, history, microphone, outdated};
use crate::tr;

pub fn show(ui: &mut Ui, cards: &[CallCard], rows: &[PeerRow], cx: &mut Cx) {
	kit::page_title(ui, tr!("calls-heading"));
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
