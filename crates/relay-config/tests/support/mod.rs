#![allow(dead_code)]

use std::path::Path;

use relay_config::prefs::Preferences;
use relay_config::{Paths, Storage};

pub fn paths(root: &Path) -> Paths {
	Paths::under(root)
}

pub fn storage(root: &Path) -> Storage {
	Storage::open(&paths(root)).unwrap()
}

pub fn chosen_mic() -> Preferences {
	let mut p = Preferences::default();
	p.audio.input_selection = relay_config::prefs::DeviceSelection::Explicit;
	p.audio.preferred_input_id = "usb-mic".into();
	p.calls.default_transmit_mode = relay_config::prefs::TransmitModePref::Unmuted;
	p
}
