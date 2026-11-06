//! Push-to-talk: the key that opens the microphone anywhere (where the
//! device has one) and how long the microphone stays open after letting go.

use egui::Ui;

use crate::capabilities::Capabilities;
use crate::format::hotkey_note;

use super::super::kit::{self, Cx};
use super::super::widgets::release_delay_slider;
use super::hotkey;
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let caps = Capabilities::of(&*cx.actions.shell);
	kit::help(ui, hotkey_note(caps.global_hotkey, caps.talk_button));
	kit::gap(ui);
	if caps.global_hotkey {
		kit::card(ui, |ui| hotkey::show(ui, cx.actions, cx.local));
		kit::gap(ui);
	}
	let Some(settings) = &cx.state.settings else {
		kit::help(ui, tr!("audio-none"));
		return;
	};
	kit::card(ui, |ui| {
		if let Some(ms) = release_delay_slider(
			ui,
			&mut cx.local.release_delay_edit,
			settings.ptt_release_delay_ms,
		) {
			cx.actions
				.run(|c| async move { c.set_ptt_release_delay(ms).await });
		}
		kit::help(ui, tr!("ptt-delay-help"));
	});
}
