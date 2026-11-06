//! Audio: which microphone and speakers to use, their levels, and mute or
//! deafen for every call at once.

use egui::{self, Ui};

use crate::model::{AppState, Direction};

use super::super::kit::{self, Cx};
use super::super::widgets::{button_row, combo, gain_slider, meter};
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let Some(settings) = &cx.state.settings else {
		kit::help(ui, tr!("audio-none"));
		return;
	};
	let actions = cx.actions;
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("audio-devices"));
		if let Some(id) = pick(
			ui,
			cx.state,
			tr!("audio-microphone"),
			Direction::Input,
			&settings.input_device_id,
		) {
			let out = settings.output_device_id.clone();
			actions.run(|c| async move { c.select_devices(&id, &out).await });
		}
		if let Some(id) = pick(
			ui,
			cx.state,
			tr!("audio-speakers"),
			Direction::Output,
			&settings.output_device_id,
		) {
			let inp = settings.input_device_id.clone();
			actions.run(|c| async move { c.select_devices(&inp, &id).await });
		}
		kit::help(ui, tr!("audio-devices-help"));
	});
	kit::gap(ui);
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("audio-levels"));
		ui.horizontal_wrapped(|ui| {
			ui.label(tr!("audio-mic-level"));
			meter(ui, cx.meters.microphone_dbfs, tr!("audio-mic-level"));
		});
		let mic = cx.local.gain_edits.entry("mic".into()).or_default();
		if let Some(g) = gain_slider(ui, mic, settings.microphone_gain, tr!("audio-mic-gain")) {
			actions.run(|c| async move { c.set_global_controls(None, None, Some(g), None).await });
		}
		let play = cx.local.gain_edits.entry("play".into()).or_default();
		if let Some(g) = gain_slider(ui, play, settings.playback_gain, tr!("audio-playback-gain")) {
			actions.run(|c| async move { c.set_global_controls(None, None, None, Some(g)).await });
		}
	});
	kit::gap(ui);
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("audio-all-calls"));
		button_row(ui, |ui| {
			let (m, d) = (settings.global_muted, settings.global_deafened);
			let mute = if m {
				tr!("audio-unmute-all")
			} else {
				tr!("audio-mute-all")
			};
			if ui.selectable_label(m, mute).clicked() {
				actions.run(
					|c| async move { c.set_global_controls(Some(!m), None, None, None).await },
				);
			}
			let deafen = if d {
				tr!("audio-undeafen-all")
			} else {
				tr!("audio-deafen-all")
			};
			if ui.selectable_label(d, deafen).clicked() {
				actions.run(
					|c| async move { c.set_global_controls(None, Some(!d), None, None).await },
				);
			}
		});
		kit::help(ui, tr!("audio-all-help"));
	});
}

/// A device drop-down; returns the newly chosen device id.
fn pick(
	ui: &mut Ui,
	state: &AppState,
	label: &str,
	dir: Direction,
	current: &str,
) -> Option<String> {
	let mut chosen = None;
	let shown = state
		.devices
		.iter()
		.find(|d| d.device_id == current)
		.map_or(current, |d| d.name.as_str());
	combo(ui, label, shown).show_ui(ui, |ui| {
		for d in state.devices.iter().filter(|d| d.direction == dir) {
			let text = if d.system_default {
				format!("{} {}", d.name, tr!("device-default"))
			} else {
				d.name.clone()
			};
			let button = egui::Button::selectable(d.device_id == current, text);
			if ui.add_enabled(d.available, button).clicked() {
				chosen = Some(d.device_id.clone());
			}
		}
	});
	chosen
}
