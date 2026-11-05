//! Ring, ringback and call-sound controls of the settings tab.

use egui::Ui;

use super::local::Local;
use crate::actions::Actions;
use crate::model::Settings;

/// Volume slider whose drag value lives in `edit` until released.
fn volume_slider(ui: &mut Ui, edit: &mut Option<f32>, current: f32) -> Option<f32> {
	let mut value = edit.unwrap_or(current);
	let response = ui.add(
		egui::Slider::new(&mut value, 0.0..=1.0)
			.text("Ringtone volume")
			.fixed_decimals(2),
	);
	if response.dragged() || (response.changed() && edit.is_some()) {
		*edit = Some(value);
	}
	if response.drag_stopped() || (response.changed() && !response.dragged()) {
		*edit = None;
		return Some(value);
	}
	None
}

/// Checkboxes for the ring preferences, the volume slider and "Test ring".
pub fn ring_controls(ui: &mut Ui, settings: &Settings, actions: &Actions, local: &mut Local) {
	let mut ringtone = settings.ringtone_enabled;
	if ui
		.checkbox(&mut ringtone, "Ring for incoming calls")
		.changed()
	{
		actions.run(|c| async move {
			c.set_ring_preferences(Some(ringtone), None, None, None)
				.await
		});
	}
	let mut ringback = settings.ringback_enabled;
	if ui
		.checkbox(&mut ringback, "Ringback while calling")
		.changed()
	{
		actions.run(|c| async move {
			c.set_ring_preferences(None, None, Some(ringback), None)
				.await
		});
	}
	let mut sounds = settings.call_sounds_enabled;
	if ui
		.checkbox(&mut sounds, "Connect and hang-up sounds")
		.changed()
	{
		actions
			.run(|c| async move { c.set_ring_preferences(None, None, None, Some(sounds)).await });
	}
	ui.horizontal(|ui| {
		if let Some(volume) =
			volume_slider(ui, &mut local.ring_volume_edit, settings.ringtone_volume)
		{
			actions.run(
				|c| async move { c.set_ring_preferences(None, Some(volume), None, None).await },
			);
		}
		if ui
			.button("Test ring")
			.on_hover_text("Plays the ring on this computer for two seconds.")
			.clicked()
		{
			actions.run(|c| async move { c.preview_tone().await });
		}
	});
}
