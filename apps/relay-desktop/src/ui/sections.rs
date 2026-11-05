//! Section drawing. Reads view models, emits [`Actions`]; holds no logic.

use std::collections::HashMap;

use eframe::egui::{self, RichText, Ui};

use crate::format::{
	HOTKEY_NOTE, MAX_AVATAR_BYTES, avatar_problem, gain_percent, link_label, media_type_for,
};
use crate::model::{AppState, Direction, Link, Meters, Mode};
use crate::shell::Actions;
use crate::viewmodel::{CallCard, MODES, PeerRow, local_avatar, mode_enabled};

use super::widgets::{avatar, banner, gain_slider, meter, tone_color};

/// Per-widget UI-only state.
#[derive(Default)]
pub struct Local {
	pub name_input: String,
	pub avatar_path: String,
	pub avatar_problem: Option<String>,
	pub consent: HashMap<String, bool>,
	pub gain_edits: HashMap<String, Option<f32>>,
}

pub fn status(ui: &mut Ui, state: &AppState, error: Option<&str>) {
	if state.mock {
		banner(
			ui,
			"Mock data: this view is not connected to a Relay daemon.",
			false,
		);
	} else if state.link != Link::Connected {
		let mut text = format!("{}.", link_label(state.link));
		if let Some(e) = &state.link_error {
			text.push_str(&format!(" {e}"));
		}
		if state.link == Link::Offline {
			text.push_str(" Start the daemon; this window reconnects by itself.");
		}
		banner(ui, &text, state.link == Link::Offline);
	}
	for w in &state.warnings {
		banner(ui, &w.message, false);
	}
	if let Some(e) = error {
		banner(ui, e, true);
	}
	if let Some(n) = &state.notice {
		banner(ui, n, false);
	}
}

pub fn calls(ui: &mut Ui, cards: &[CallCard], actions: &Actions, local: &mut Local) {
	ui.heading("Calls");
	if cards.is_empty() {
		ui.label(RichText::new("No active calls.").weak());
	}
	for card in cards {
		egui::Frame::group(ui.style()).show(ui, |ui| {
			ui.set_width(ui.available_width());
			call_card(ui, card, actions, local);
		});
	}
}

fn call_card(ui: &mut Ui, card: &CallCard, actions: &Actions, local: &mut Local) {
	let id = card.call_id.clone();
	ui.horizontal(|ui| {
		avatar(ui, card.avatar, 36.0);
		ui.vertical(|ui| {
			ui.label(RichText::new(&card.name).strong());
			let sub = RichText::new(&card.subtitle);
			ui.label(if card.speaking {
				sub.color(egui::Color32::from_rgb(63, 185, 80))
			} else {
				sub.weak()
			});
		});
		if card.receive_dbfs.is_some() {
			meter(
				ui,
				card.receive_dbfs,
				&format!("Incoming level from {}", card.name),
			);
		}
	});
	if let Some(p) = &card.call.problem {
		banner(ui, &format!("{} {}", p.message, p.recovery_action), true);
	}
	if card.incoming {
		ui.horizontal(|ui| {
			if ui.button("Accept").clicked() {
				actions.run(|c| async move { c.accept_call(&id).await });
			}
			if ui.button("Decline").clicked() {
				let id = card.call_id.clone();
				actions.run(|c| async move { c.decline_call(&id).await });
			}
			ui.label(RichText::new("Accepting does not turn on your microphone.").weak());
		});
		return;
	}
	controls(ui, card, actions, local);
}

fn controls(ui: &mut Ui, card: &CallCard, actions: &Actions, local: &mut Local) {
	let call = &card.call;
	let cid = card.call_id.clone();
	ui.horizontal(|ui| {
		let end = if card.phase == crate::model::Phase::OutgoingRinging {
			"Cancel"
		} else {
			"End call"
		};
		if ui.button(end).clicked() {
			let id = cid.clone();
			actions.run(|c| async move { c.end_call(&id).await });
		}
		let (mute, deaf) = (call.user_muted, call.user_deafened);
		if ui
			.selectable_label(mute, if mute { "Unmute (call)" } else { "Mute (call)" })
			.clicked()
		{
			let id = cid.clone();
			actions.run(|c| async move { c.set_call_controls(&id, Some(!mute), None, None).await });
		}
		if ui
			.selectable_label(
				deaf,
				if deaf {
					"Undeafen (call)"
				} else {
					"Deafen (call)"
				},
			)
			.clicked()
		{
			let id = cid.clone();
			actions.run(|c| async move { c.set_call_controls(&id, None, Some(!deaf), None).await });
		}
	});
	let edit = local.gain_edits.entry(format!("gain-{cid}")).or_default();
	let label = format!(
		"Their volume {}",
		gain_percent(edit.unwrap_or(call.receive_gain))
	);
	if let Some(g) = gain_slider(ui, edit, call.receive_gain, &label) {
		let id = cid.clone();
		actions.run(|c| async move { c.set_call_controls(&id, None, None, Some(g)).await });
	}

	ui.separator();
	ui.label(RichText::new("Your microphone in this call").strong());
	let consent = local
		.consent
		.entry(cid.clone())
		.or_insert(call.transmit.consent_granted);
	ui.add_enabled(
		call.transmit.mode == Mode::ReceiveOnly,
		egui::Checkbox::new(
			consent,
			format!("I allow my microphone to be sent to {}", card.name),
		),
	);
	let consent = *consent;
	ui.horizontal(|ui| {
		for (label, mode) in MODES {
			let selected = call.transmit.mode == mode;
			let enabled = mode_enabled(mode, consent);
			if ui
				.add_enabled(enabled, egui::Button::selectable(selected, label))
				.clicked() && !selected
			{
				let id = cid.clone();
				actions.run(|c| async move { c.set_transmit(&id, mode, consent).await });
			}
		}
	});
	let t = &card.transmit;
	ui.label(RichText::new(format!("{}. {}", t.label, t.detail)).color(tone_color(t.tone, ui)));
}

pub fn peers(ui: &mut Ui, rows: &[PeerRow], actions: &Actions) {
	ui.heading("Nearby peers");
	if rows.is_empty() {
		ui.label(RichText::new("No peers found on the local network yet.").weak());
	}
	for row in rows {
		egui::Frame::group(ui.style()).show(ui, |ui| {
			ui.set_width(ui.available_width());
			ui.horizontal(|ui| {
				avatar(ui, row.avatar, 36.0);
				ui.vertical(|ui| {
					ui.label(&row.name);
					ui.label(RichText::new(&row.subtitle).weak());
				});
				ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
					if ui
						.add_enabled(row.can_call, egui::Button::new("Call"))
						.clicked()
					{
						let id = row.peer_id.clone();
						actions.run(|c| async move { c.start_call(&id).await });
					}
				});
			});
		});
	}
}

pub fn audio(ui: &mut Ui, state: &AppState, meters: &Meters, actions: &Actions, local: &mut Local) {
	ui.heading("Audio");
	let Some(settings) = &state.settings else {
		ui.label(RichText::new("Audio settings appear once the daemon is connected.").weak());
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
			egui::ComboBox::from_label(label)
				.selected_text(shown)
				.show_ui(ui, |ui| {
					for d in state.devices.iter().filter(|d| d.direction == dir) {
						let text = format!(
							"{}{}",
							d.name,
							if d.system_default {
								" (system default)"
							} else {
								""
							}
						);
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
			"Microphone",
			Direction::Input,
			&settings.input_device_id,
		) {
			let out = settings.output_device_id.clone();
			actions.run(|c| async move { c.select_devices(&id, &out).await });
		}
		if let Some(id) = pick(
			ui,
			"Speakers",
			Direction::Output,
			&settings.output_device_id,
		) {
			let inp = settings.input_device_id.clone();
			actions.run(|c| async move { c.select_devices(&inp, &id).await });
		}
		ui.horizontal(|ui| {
			ui.label("Microphone level");
			meter(ui, meters.microphone_dbfs, "Microphone level");
		});
		let mic = local.gain_edits.entry("mic".into()).or_default();
		if let Some(g) = gain_slider(ui, mic, settings.microphone_gain, "Microphone gain") {
			actions.run(|c| async move { c.set_global_controls(None, None, Some(g), None).await });
		}
		let play = local.gain_edits.entry("play".into()).or_default();
		if let Some(g) = gain_slider(ui, play, settings.playback_gain, "Playback gain") {
			actions.run(|c| async move { c.set_global_controls(None, None, None, Some(g)).await });
		}
		ui.horizontal(|ui| {
			let (m, d) = (settings.global_muted, settings.global_deafened);
			if ui
				.selectable_label(m, if m { "Unmute all" } else { "Mute all" })
				.clicked()
			{
				actions.run(
					|c| async move { c.set_global_controls(Some(!m), None, None, None).await },
				);
			}
			if ui
				.selectable_label(d, if d { "Undeafen all" } else { "Deafen all" })
				.clicked()
			{
				actions.run(
					|c| async move { c.set_global_controls(None, Some(!d), None, None).await },
				);
			}
		});
	});
}

pub fn profile(ui: &mut Ui, state: &AppState, actions: &Actions, local: &mut Local) {
	ui.heading("Your profile");
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		ui.horizontal(|ui| {
			avatar(ui, local_avatar(state), 36.0);
			ui.add(
				egui::TextEdit::singleline(&mut local.name_input)
					.hint_text("Display name")
					.char_limit(64),
			);
			if ui
				.add_enabled(
					!local.name_input.trim().is_empty(),
					egui::Button::new("Save name"),
				)
				.clicked()
			{
				let name = local.name_input.clone();
				actions.run(|c| async move { c.set_profile(&name).await });
			}
		});
		ui.label("Profile image: path to a PNG or JPEG file up to 1 MiB");
		ui.horizontal(|ui| {
			ui.add(
				egui::TextEdit::singleline(&mut local.avatar_path).hint_text("/path/to/image.png"),
			);
			if ui
				.add_enabled(
					!local.avatar_path.trim().is_empty(),
					egui::Button::new("Upload"),
				)
				.clicked()
			{
				upload(actions, local);
			}
		});
		if let Some(p) = &local.avatar_problem {
			banner(ui, p, true);
		}
	});
}

fn upload(actions: &Actions, local: &mut Local) {
	let path = local.avatar_path.trim().to_owned();
	let media = media_type_for(&path);
	local.avatar_problem = None;
	// Check the size before reading so a huge file is never loaded.
	let size = std::fs::metadata(&path).map(|m| m.len() as usize);
	let result = match size {
		Err(e) => Err(format!("Cannot read the file: {e}")),
		Ok(n) => avatar_problem(n.min(MAX_AVATAR_BYTES + 1), media)
			.map(str::to_owned)
			.map_or_else(
				|| std::fs::read(&path).map_err(|e| format!("Cannot read the file: {e}")),
				Err,
			),
	};
	match result {
		Ok(bytes) => actions.run(|c| async move { c.upload_avatar(bytes, media).await }),
		Err(p) => local.avatar_problem = Some(p),
	}
}

pub fn app_panel(ui: &mut Ui, actions: &Actions, has_tray: bool) {
	ui.heading("This app");
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		let close = if has_tray { "Closing the window hides it to the tray." } else { "There is no tray icon on this platform, so closing the window quits this app." };
		ui.label(format!("{close} Quitting this app leaves the Relay daemon, and any call, running. This app cannot stop the daemon."));
		ui.horizontal(|ui| {
			if has_tray && ui.button("Hide window").clicked() {
				actions.hide_window();
			}
			if ui.button("Quit this app (daemon keeps running)").clicked() {
				actions.quit_app();
			}
		});
		ui.label(RichText::new(HOTKEY_NOTE).weak());
	});
}
