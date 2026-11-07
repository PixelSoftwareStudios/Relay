//! Ring, ringback and call-sound controls (the ringtone group is in `ringtone.rs`) of "Calls and notifications".

use egui::Ui;

use super::super::kit;
use super::super::local::Local;
use crate::actions::Actions;
use crate::model::Settings;
use crate::tr;

/// Checkboxes for the ring preferences, then the ringtone group.
pub fn ring_controls(ui: &mut Ui, settings: &Settings, actions: &Actions, local: &mut Local) {
	let mut ringtone = settings.ringtone_enabled;
	if ui.checkbox(&mut ringtone, tr!("ring-incoming")).changed() {
		actions.run(|c| async move {
			c.set_ring_preferences(Some(ringtone), None, None, None)
				.await
		});
	}
	let mut ringback = settings.ringback_enabled;
	if ui.checkbox(&mut ringback, tr!("ring-back")).changed() {
		actions.run(|c| async move {
			c.set_ring_preferences(None, None, Some(ringback), None)
				.await
		});
	}
	let mut sounds = settings.call_sounds_enabled;
	if ui.checkbox(&mut sounds, tr!("ring-sounds")).changed() {
		actions
			.run(|c| async move { c.set_ring_preferences(None, None, None, Some(sounds)).await });
	}
	super::ringtone::ringtone_group(ui, settings, actions, local);
	kit::help(ui, tr!("ring-help"));
}
