//! Audio: which microphone and speakers to use, their levels, and mute or
//! deafen for every call at once.

use egui::{self, Ui};

use crate::format::{device_option_label, selected_device_text};
use crate::model::{AppState, Device, Direction};

use super::super::kit::{self, Cx};
use super::super::widgets::{button_row, combo, gain_slider};
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let Some(settings) = &cx.state.settings else {
		kit::help(ui, tr!("audio-none"));
		return;
	};
	let actions = cx.actions;
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("audio-devices"));
		let output = Endpoint {
			pinned: &settings.output_device_id,
			is_default: settings.output_is_default,
			name: &settings.output_device_name,
			missing: settings.output_pinned_missing,
		};
		let input = Endpoint {
			pinned: &settings.input_device_id,
			is_default: settings.input_is_default,
			name: &settings.input_device_name,
			missing: settings.input_pinned_missing,
		};
		if let Some(id) = pick(
			ui,
			cx.state,
			tr!("audio-microphone"),
			Direction::Input,
			&input,
		) {
			let out = other_pin(&output);
			actions.run(|c| async move { c.select_devices(&id, &out).await });
		}
		if let Some(id) = pick(
			ui,
			cx.state,
			tr!("audio-speakers"),
			Direction::Output,
			&output,
		) {
			let inp = other_pin(&input);
			actions.run(|c| async move { c.select_devices(&inp, &id).await });
		}
		kit::help(ui, tr!("audio-devices-help"));
	});
	kit::gap(ui);
	super::mic_test::card(ui, cx);
	kit::gap(ui);
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("audio-levels"));
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
	super::processing::card(ui, cx, settings);
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

/// What the engine reports for one direction.
struct Endpoint<'a> {
	/// Pinned device id; empty when following the system default.
	pinned: &'a str,
	is_default: bool,
	/// The device in use.
	name: &'a str,
	/// The pinned device is not connected.
	missing: bool,
}

/// The pin to keep for the direction that did not change. A pinned device
/// that is not connected is dropped (back to the system default).
fn other_pin(endpoint: &Endpoint) -> String {
	if endpoint.missing {
		String::new()
	} else {
		endpoint.pinned.to_owned()
	}
}

/// A device drop-down. The OS default device is listed once, with the
/// "(System Default)" suffix, and choosing it means "follow the default"
/// (empty id); any other device is pinned. Returns the newly chosen id.
fn pick(
	ui: &mut Ui,
	state: &AppState,
	label: &str,
	dir: Direction,
	current: &Endpoint,
) -> Option<String> {
	let mut chosen = None;
	let shown = selected_device_text(
		&state.devices,
		dir,
		current.is_default || current.missing,
		current.name,
		current.pinned,
	);
	combo(ui, label, &shown).show_ui(ui, |ui| {
		let listed = |d: &&Device| d.direction == dir;
		if !state
			.devices
			.iter()
			.filter(listed)
			.any(|d| d.system_default)
		{
			let text = device_option_label("", true);
			if ui
				.add(egui::Button::selectable(current.is_default, text))
				.clicked()
			{
				chosen = Some(String::new());
			}
		}
		for d in state.devices.iter().filter(listed) {
			let text = device_option_label(&d.name, d.system_default);
			let on = if d.system_default {
				current.is_default
			} else {
				!current.is_default && d.device_id == current.pinned
			};
			if ui
				.add_enabled(d.available, egui::Button::selectable(on, text))
				.clicked()
			{
				chosen = Some(if d.system_default {
					String::new()
				} else {
					d.device_id.clone()
				});
			}
		}
	});
	if current.missing {
		kit::help(ui, tr!("audio-pinned-missing"));
	}
	chosen
}
