//! Microphone processing: noise suppression with its strength, and automatic gain.

use egui::Ui;

use super::super::kit::{self, Cx};
use super::super::widgets::combo;
use crate::audio_processing::{NoiseLevel, ProcessingChange};
use crate::model::Settings;
use crate::tr;

fn level_text(level: NoiseLevel) -> &'static str {
	match level {
		NoiseLevel::Low => tr!("audio-ns-level-low"),
		NoiseLevel::Moderate => tr!("audio-ns-level-moderate"),
		NoiseLevel::High => tr!("audio-ns-level-high"),
	}
}

pub fn card(ui: &mut Ui, cx: &mut Cx, settings: &Settings) {
	let actions = cx.actions;
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("audio-processing"));
		let mut on = settings.noise_suppression_enabled;
		if ui.checkbox(&mut on, tr!("audio-ns")).changed() {
			let change = ProcessingChange {
				noise_suppression: Some(on),
				..ProcessingChange::default()
			};
			actions.run(|c| async move { c.set_audio_processing(change).await });
		}
		ui.add_enabled_ui(settings.noise_suppression_enabled, |ui| {
			let current = settings.noise_suppression_level;
			combo(ui, tr!("audio-ns-level"), level_text(current)).show_ui(ui, |ui| {
				for level in NoiseLevel::ALL {
					if ui
						.add(egui::Button::selectable(
							level == current,
							level_text(level),
						))
						.clicked()
					{
						let change = ProcessingChange {
							level: Some(level),
							..ProcessingChange::default()
						};
						actions.run(|c| async move { c.set_audio_processing(change).await });
					}
				}
			});
		});
		let mut gain = settings.auto_gain_enabled;
		if ui.checkbox(&mut gain, tr!("audio-agc")).changed() {
			let change = ProcessingChange {
				auto_gain: Some(gain),
				..ProcessingChange::default()
			};
			actions.run(|c| async move { c.set_audio_processing(change).await });
		}
		kit::help(ui, tr!("audio-processing-help"));
	});
}
