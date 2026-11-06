//! The audio section: devices, levels and gains.

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::model::{AppState, Direction, Meters};

use super::local::Local;
use super::widgets::{combo, gain_slider, meter, release_delay_slider};
use crate::tr;

pub fn audio(ui: &mut Ui, state: &AppState, meters: &Meters, actions: &Actions, local: &mut Local) {
	ui.heading(tr!("audio-heading"));
	let Some(settings) = &state.settings else {
		ui.label(RichText::new(tr!("audio-none")).weak());
		return;
	};
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		let pick = |ui: &mut Ui, label: &str, dir: Direction, current: &str| -> Option<String> {
			let mut chosen = None;
			let shown = state
				.devices
				.iter()
				.find(|d| d.device_id == current)
				.map_or(current, |d| d.name.as_str());
			combo(ui, label, shown)
				.show_ui(ui, |ui| {
					for d in state.devices.iter().filter(|d| d.direction == dir) {
						let text = if d.system_default {
							format!("{} {}", d.name, tr!("device-default"))
						} else {
							d.name.clone()
						};
						if ui
							.add_enabled(
								d.available,
								egui::Button::selectable(d.device_id == current, text),
							)
							.clicked()
						{
							chosen = Some(d.device_id.clone());
						}
					}
				});
			chosen
		};
		if let Some(id) = pick(
			ui,
			tr!("audio-microphone"),
			Direction::Input,
			&settings.input_device_id,
		) {
			let out = settings.output_device_id.clone();
			actions.run(|c| async move { c.select_devices(&id, &out).await });
		}
		if let Some(id) = pick(
			ui,
			tr!("audio-speakers"),
			Direction::Output,
			&settings.output_device_id,
		) {
			let inp = settings.input_device_id.clone();
			actions.run(|c| async move { c.select_devices(&inp, &id).await });
		}
		ui.horizontal_wrapped(|ui| {
			ui.label(tr!("audio-mic-level"));
			meter(ui, meters.microphone_dbfs, tr!("audio-mic-level"));
		});
		let mic = local.gain_edits.entry("mic".into()).or_default();
		if let Some(g) = gain_slider(ui, mic, settings.microphone_gain, tr!("audio-mic-gain")) {
			actions.run(|c| async move { c.set_global_controls(None, None, Some(g), None).await });
		}
		let play = local.gain_edits.entry("play".into()).or_default();
		if let Some(g) = gain_slider(ui, play, settings.playback_gain, tr!("audio-playback-gain")) {
			actions.run(|c| async move { c.set_global_controls(None, None, None, Some(g)).await });
		}
		if let Some(ms) = release_delay_slider(
			ui,
			&mut local.release_delay_edit,
			settings.ptt_release_delay_ms,
		) {
			actions.run(|c| async move { c.set_ptt_release_delay(ms).await });
		}
		super::ring::ring_controls(ui, settings, actions, local);
		ui.horizontal_wrapped(|ui| {
			let (m, d) = (settings.global_muted, settings.global_deafened);
			if ui
				.selectable_label(
					m,
					if m {
						tr!("audio-unmute-all")
					} else {
						tr!("audio-mute-all")
					},
				)
				.clicked()
			{
				actions.run(
					|c| async move { c.set_global_controls(Some(!m), None, None, None).await },
				);
			}
			if ui
				.selectable_label(
					d,
					if d {
						tr!("audio-undeafen-all")
					} else {
						tr!("audio-deafen-all")
					},
				)
				.clicked()
			{
				actions.run(
					|c| async move { c.set_global_controls(None, Some(!d), None, None).await },
				);
			}
		});
	});
}
