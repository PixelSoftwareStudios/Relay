//! The Audio settings list the OS default device once, with a "(System Default)"
//! suffix, and never show a blank selection; a pinned device replaces it. The default
//! language is English in this test binary.

use std::sync::Arc;
use std::time::Duration;

use relay_api::pb;
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::convert;
use relay_ui::format::{device_option_label, selected_device_text};
use relay_ui::hub::Hub;
use relay_ui::mock;
use relay_ui::model::{Device, Direction};

fn device(name: &str, direction: Direction, default: bool) -> Device {
	Device {
		device_id: format!("{direction:?}-{name}"),
		name: name.into(),
		direction,
		available: true,
		system_default: default,
	}
}

#[test]
fn the_default_device_is_listed_once_with_the_suffix() {
	assert_eq!(
		device_option_label("MacBook Pro Speakers", true),
		"MacBook Pro Speakers (System Default)"
	);
	assert_eq!(device_option_label("USB Headset", false), "USB Headset");
	assert_eq!(device_option_label("", true), "System Default");
}

#[test]
fn the_selected_text_is_never_blank() {
	let with_default = [
		device("MacBook Pro Microphone", Direction::Input, true),
		device("USB Mic", Direction::Input, false),
		device("MacBook Pro Speakers", Direction::Output, true),
		device("USB Headset", Direction::Output, false),
	];
	for (dir, want) in [
		(Direction::Input, "MacBook Pro Microphone (System Default)"),
		(Direction::Output, "MacBook Pro Speakers (System Default)"),
	] {
		// The engine's name, and the listed default when it sends none.
		assert_eq!(selected_device_text(&with_default, dir, true, "", ""), want);
		let named = want.trim_end_matches(" (System Default)");
		assert_eq!(selected_device_text(&[], dir, true, named, ""), want);
		// No default anywhere: still not blank.
		assert_eq!(
			selected_device_text(&[], dir, true, "", ""),
			"System Default"
		);
		// A pinned device shows its plain name; a vanished pin shows the default.
		let pinned = with_default
			.iter()
			.find(|d| d.direction == dir && !d.system_default);
		let pinned = pinned.unwrap();
		assert_eq!(
			selected_device_text(&with_default, dir, false, "", &pinned.device_id),
			pinned.name
		);
		assert_eq!(
			selected_device_text(&with_default, dir, true, "", "gone"),
			want
		);
	}
}

#[test]
fn engine_settings_carry_what_is_in_use() {
	let settings = convert::settings(&pb::Settings {
		input_is_default: true,
		input_device_name: "MacBook Pro Microphone".into(),
		output_device_id: "out:coreaudio:usb".into(),
		output_device_name: "USB Headset".into(),
		output_pinned_missing: true,
		..Default::default()
	});
	assert!(settings.input_is_default);
	assert!(settings.input_device_id.is_empty());
	assert_eq!(settings.input_device_name, "MacBook Pro Microphone");
	assert!(!settings.output_is_default);
	assert!(settings.output_pinned_missing);
}

#[tokio::test(start_paused = true)]
async fn the_mock_starts_on_the_defaults_and_pinning_replaces_them() {
	let hub = Arc::new(Hub::default());
	let slot = Arc::new(SessionSlot::default());
	tokio::spawn(mock::run(hub.clone(), slot.clone()));
	tokio::time::sleep(Duration::from_millis(10)).await;
	let ctl = Controller::new(hub.clone(), slot);
	let before = hub.state().settings.unwrap();
	assert!(before.input_is_default && before.output_is_default);
	assert_eq!(before.input_device_name, "MacBook Pro Microphone");

	ctl.select_devices("usb-headset-mic", "").await.unwrap();
	let after = hub.state().settings.unwrap();
	assert!(!after.input_is_default);
	assert_eq!(after.input_device_name, "USB Headset Mic");
	assert!(after.output_is_default);

	ctl.select_devices("", "").await.unwrap();
	assert!(hub.state().settings.unwrap().input_is_default);
}

#[test]
fn the_processing_fields_are_read_and_an_unknown_strength_is_the_default() {
	use relay_ui::audio_processing::NoiseLevel;
	let read = |level: &str| {
		convert::settings(&pb::Settings {
			noise_suppression_enabled: true,
			noise_suppression_level: level.into(),
			auto_gain_enabled: true,
			..pb::Settings::default()
		})
	};
	let high = read("high");
	assert!(high.noise_suppression_enabled && high.auto_gain_enabled);
	assert_eq!(high.noise_suppression_level, NoiseLevel::High);
	assert_eq!(read("low").noise_suppression_level, NoiseLevel::Low);
	assert_eq!(read("").noise_suppression_level, NoiseLevel::Moderate);
}
