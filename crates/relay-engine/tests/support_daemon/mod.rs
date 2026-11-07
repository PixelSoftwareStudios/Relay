//! Whole engines on loopback with fake audio hardware.
#![allow(dead_code)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use relay_audio::device::{Direction, FakeBackend, FakeDeviceSpec, Signal};
use relay_config::Paths;
use relay_config::identity::MemoryStore;
use relay_engine::daemon::{AudioSetup, NetworkOptions, RunOptions, Transport};

#[path = "../support_e2e/mod.rs"]
pub mod e2e;
use e2e::SharedBackend;
pub use e2e::Sim;

/// Fake devices: a microphone playing `tone_hz` and a recording speaker.
pub fn fake_hardware(tone_hz: f32) -> (AudioSetup, Sim) {
	let mut mic = FakeDeviceSpec::new("mic", Direction::Input);
	mic.is_default = true;
	let mut speaker = FakeDeviceSpec::new("spk", Direction::Output);
	speaker.is_default = true;
	let backend = SharedBackend(Arc::new(Mutex::new(FakeBackend::new(vec![mic, speaker]))));
	let sim = Sim::start(
		backend.clone(),
		Signal::Sine {
			freq_hz: tone_hz,
			amplitude: 0.5,
		},
	);
	(
		AudioSetup::Backend(Box::new(move || Box::new(backend))),
		sim,
	)
}

/// Options for one engine: its own home under `home`, loopback networking,
/// fake audio, an in-memory identity.
pub fn engine_options(home: &Path, tone_hz: f32) -> (RunOptions, Sim) {
	let (audio, sim) = fake_hardware(tone_hz);
	let mut options = RunOptions::new(Paths::under(home.join("cfg")));
	options.transport = Transport::Unix(Some(home.join("e.sock")));
	options.identity_store = Some(Box::new(MemoryStore::default()));
	options.network = NetworkOptions::loopback();
	options.audio = audio;
	// Most tests are about client-held presence; the engine-held kind opts in.
	// A second call for a home another engine still holds keeps what is stored.
	if relay_config::Storage::open(&options.paths).is_ok() {
		set_background(&options.paths, false);
	}
	(options, sim)
}

/// Stores `background.available` before the engine starts (the real default is on).
pub fn set_background(paths: &Paths, available: bool) {
	let storage = relay_config::Storage::open(paths).expect("storage");
	relay_config::prefs::PreferenceStore::open(&storage.database)
		.expect("prefs")
		.set(
			"background.available",
			if available { "true" } else { "false" },
		)
		.expect("set");
}

/// A flat-colour PNG of the given size, for avatar uploads.
pub fn png(width: u32, height: u32) -> Vec<u8> {
	let mut out = std::io::Cursor::new(Vec::new());
	image::RgbaImage::from_pixel(width, height, image::Rgba([200, 30, 30, 255]))
		.write_to(&mut out, image::ImageFormat::Png)
		.expect("encode");
	out.into_inner()
}
