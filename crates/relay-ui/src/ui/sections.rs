//! Section drawing. Reads view models, emits [`Actions`]; holds no logic.

use std::sync::atomic::Ordering;

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::format::{
	HOTKEY_NOTE, close_note, gain_percent, link_label, own_live_calls, quit_prompt,
};
use crate::model::{AppState, Direction, Link, Meters, Mode};
use crate::viewmodel::{CallCard, MODES, PeerRow, mode_enabled};

use super::local::Local;
use super::textures::Textures;
use super::widgets::{avatar, banner, gain_slider, meter, release_delay_slider, tone_color};

pub fn status(ui: &mut Ui, state: &AppState, error: Option<&str>, actions: &Actions) {
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
			text.push_str(
				if actions.shared.autostart.enabled() && !actions.shared.autostart.paused() {
					" Starting it automatically."
				} else {
					" Start the daemon; this window reconnects by itself."
				},
			);
		}
		banner(ui, &text, state.link == Link::Offline);
		if state.link == Link::Offline && actions.shell.can_manage_daemon() {
			start_daemon_button(ui, actions);
		}
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

fn start_daemon_button(ui: &mut Ui, actions: &Actions) {
	let busy = actions.shared.daemon_busy.load(Ordering::Acquire);
	let label = if busy {
		"Starting the daemon..."
	} else {
		"Start daemon"
	};
	if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
		actions.start_daemon();
	}
}

pub fn calls(
	ui: &mut Ui,
	cards: &[CallCard],
	actions: &Actions,
	local: &mut Local,
	textures: &mut Textures,
) {
	ui.heading("Calls");
	if cards.is_empty() {
		ui.label(RichText::new("No active calls.").weak());
	}
	for card in cards {
		egui::Frame::group(ui.style()).show(ui, |ui| {
			ui.set_width(ui.available_width());
			call_card(ui, card, actions, local, textures);
		});
	}
}

fn call_card(
	ui: &mut Ui,
	card: &CallCard,
	actions: &Actions,
	local: &mut Local,
	textures: &mut Textures,
) {
	let id = card.call_id.clone();
	ui.horizontal(|ui| {
		let tex = textures.texture(&card.avatar, actions);
		avatar(ui, &tex, 36.0);
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
		ui.label(RichText::new(trust_line(&card.trust, &card.fingerprint)).small());
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

/// The line under an incoming call: trust and the short key digest, which only helps tell
/// devices apart (verification compares the full digest).
fn trust_line(trust: &str, fingerprint: &str) -> String {
	if fingerprint.is_empty() {
		format!("{trust}.")
	} else {
		format!("{trust}. Device key {fingerprint} (to tell devices apart, not to verify).")
	}
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
		if ui
			.button("Messages")
			.on_hover_text("Text, images and files for this call")
			.clicked()
		{
			local.open_messages = Some(cid.clone());
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

pub fn peers(
	ui: &mut Ui,
	rows: &[PeerRow],
	actions: &Actions,
	local: &mut Local,
	textures: &mut Textures,
) {
	ui.heading("Nearby peers");
	if rows.is_empty() {
		ui.label(RichText::new("No peers found on the local network yet.").weak());
	}
	for row in rows {
		egui::Frame::group(ui.style()).show(ui, |ui| {
			ui.set_width(ui.available_width());
			ui.horizontal(|ui| {
				let tex = textures.texture(&row.avatar, actions);
				avatar(ui, &tex, 36.0);
				ui.vertical(|ui| {
					ui.label(&row.name);
					ui.label(RichText::new(&row.subtitle).weak());
					if !row.fingerprint.is_empty() {
						ui.label(
							RichText::new(format!("Key {}", row.fingerprint))
								.small()
								.weak(),
						);
					}
					if !row.badges.is_empty() {
						ui.label(
							RichText::new(format!("Shared channels: {}", row.badges.join(", ")))
								.small(),
						);
					}
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
			for (channel_id, channel_name) in &row.revocable {
				revoke_row(ui, actions, local, row, channel_id, channel_name);
			}
		});
	}
}

/// Two-step revoke: the first click asks, the second one acts.
fn revoke_row(
	ui: &mut Ui,
	actions: &Actions,
	local: &mut Local,
	row: &PeerRow,
	channel_id: &str,
	channel_name: &str,
) {
	let key = (channel_id.to_owned(), row.peer_id.clone());
	ui.horizontal(|ui| {
		if local.revoke_confirm.as_ref() == Some(&key) {
			ui.label(format!("Remove {} from {channel_name}?", row.name));
			if ui.button("Yes, revoke").clicked() {
				let (c, p) = key.clone();
				actions.run(|ctl| async move { ctl.revoke_member(&c, &p).await });
				local.revoke_confirm = None;
			}
			if ui.button("Keep").clicked() {
				local.revoke_confirm = None;
			}
		} else if ui
			.small_button(format!("Revoke from {channel_name}"))
			.clicked()
		{
			local.revoke_confirm = Some(key);
		}
	});
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
		if let Some(ms) = release_delay_slider(
			ui,
			&mut local.release_delay_edit,
			settings.ptt_release_delay_ms,
		) {
			actions.run(|c| async move { c.set_ptt_release_delay(ms).await });
		}
		super::ring::ring_controls(ui, settings, actions, local);
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

pub fn app_panel(
	ui: &mut Ui,
	state: &AppState,
	actions: &Actions,
	local: &mut Local,
	has_tray: bool,
) {
	ui.heading("This app");
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		let live = own_live_calls(&state.calls).len();
		ui.label(close_note(has_tray, live));
		ui.horizontal(|ui| {
			if has_tray && ui.button("Hide window").clicked() {
				actions.hide_window();
			}
			if live == 0 {
				local.confirm_quit_app = false;
			}
			if ui.button("Quit this app (daemon keeps running)").clicked() {
				if live == 0 {
					actions.quit_app();
				} else {
					local.confirm_quit_app = true;
				}
			}
		});
		if local.confirm_quit_app {
			ui.label(RichText::new(quit_prompt(live)).strong());
			ui.horizontal(|ui| {
				if ui.button("Hang up and quit").clicked() {
					actions.quit_app();
				}
				if ui.button("Cancel").clicked() {
					local.confirm_quit_app = false;
				}
			});
		}
		autostart_checkbox(ui, state, actions);
		super::host_switches::draw(ui, actions);
		ui.label(RichText::new(HOTKEY_NOTE).weak());
	});
	if actions.shell.can_manage_daemon() {
		daemon_panel(ui, state, actions, local);
	}
}

/// The stored "Start daemon automatically" preference.
fn autostart_checkbox(ui: &mut Ui, state: &AppState, actions: &Actions) {
	let Some(settings) = &state.settings else {
		return;
	};
	let mut on = settings.autostart_daemon;
	if ui
		.checkbox(&mut on, "Start the daemon automatically")
		.on_hover_text("When the daemon is not running, Relay apps start it. It advertises on the local network only while an app is open.")
		.changed()
	{
		actions.shared.autostart.set_setting(on);
		actions.run(|c| async move { c.set_autostart(on).await });
	}
}

/// The daemon is its own process; stopping it is a separate, confirmed act.
fn daemon_panel(ui: &mut Ui, state: &AppState, actions: &Actions, local: &mut Local) {
	ui.heading("Relay daemon");
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		let busy = actions.shared.daemon_busy.load(Ordering::Acquire);
		if state.link != Link::Connected {
			ui.label("The daemon is not connected.");
			start_daemon_button(ui, actions);
			if actions.shared.autostart.paused() {
				ui.label(RichText::new("Automatic start is paused until you press Start daemon.").weak());
			}
			return;
		}
		ui.label("The daemon keeps running when you close this app. Quitting it ends all calls and releases the microphone.");
		if !local.confirm_quit_daemon {
			if ui.add_enabled(!busy, egui::Button::new("Quit daemon...")).clicked() {
				local.confirm_quit_daemon = true;
			}
			return;
		}
		ui.label(RichText::new("Quit the daemon now? Active calls will end.").strong());
		ui.horizontal(|ui| {
			if ui.add_enabled(!busy, egui::Button::new("Quit daemon")).clicked() {
				local.confirm_quit_daemon = false;
				actions.quit_daemon();
			}
			if ui.button("Cancel").clicked() {
				local.confirm_quit_daemon = false;
			}
		});
	});
}
