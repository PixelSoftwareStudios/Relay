//! Push-to-talk: whether it may be used at all, the key that opens the microphone anywhere (where
//! the device has one) and how long the microphone stays open after letting go.

use egui::Ui;

use crate::capabilities::Capabilities;
use crate::format::hotkey_note;
use crate::setting_save::KEY_PTT_ENABLED;

use super::super::kit::{self, Cx};
use super::super::widgets::release_delay_slider;
use super::{hotkey, saved};
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let caps = Capabilities::of(&*cx.actions.shell);
	let Some(settings) = &cx.state.settings else {
		kit::help(ui, tr!("audio-none"));
		return;
	};
	let (enabled, delay) = (settings.ptt_enabled, settings.ptt_release_delay_ms);
	kit::card(ui, |ui| {
		let mut on = enabled;
		let changed = ui
			.add_enabled(
				!saved::busy(cx, KEY_PTT_ENABLED),
				egui::Checkbox::new(&mut on, tr!("ptt-enable")),
			)
			.changed();
		if changed {
			saved::save(cx, KEY_PTT_ENABLED, on.to_string());
		}
		kit::help(ui, tr!("ptt-enable-help"));
		saved::answer(ui, cx, KEY_PTT_ENABLED);
	});
	kit::gap(ui);
	if !enabled {
		kit::help(ui, tr!("ptt-off-note"));
		kit::gap(ui);
	}
	ui.add_enabled_ui(enabled, |ui| {
		kit::help(ui, hotkey_note(caps.global_hotkey, caps.talk_button));
		kit::gap(ui);
		if caps.global_hotkey {
			kit::card(ui, |ui| hotkey::show(ui, cx.actions, cx.local));
			kit::gap(ui);
		}
		kit::card(ui, |ui| {
			if let Some(ms) = release_delay_slider(ui, &mut cx.local.release_delay_edit, delay) {
				cx.actions
					.run(|c| async move { c.set_ptt_release_delay(ms).await });
			}
			kit::help(ui, tr!("ptt-delay-help"));
		});
	});
}
